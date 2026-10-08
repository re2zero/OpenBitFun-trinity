mod i18n;
mod runtime;
mod theme;

use gpui::{
    div, list, prelude::*, px, size, App, Application, Bounds, Context, Entity, Global,
    ListAlignment, ListOffset, ListState, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants},
    input::{Input, InputState},
    scroll::ScrollableElement,
    text::TextView,
    ActiveTheme, Disableable, Root, Sizable, StyledExt,
};
use openbitfun_runtime_ports::AgentSessionSummary;
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{ChatEntry, RuntimeEvent, ToolCard, ToolStatus};
use crate::theme::{ThemePreset, DEFAULT_PRESET_ID};

/// Width of the fixed session pane.
const SESSION_PANE_WIDTH: f32 = 300.;
/// Composer height; explicit pixels because `rows()` does not size in 0.5.1.
const COMPOSER_HEIGHT: f32 = 88.;
/// Extra content rendered beyond the visible chat area so scrolling does not
/// pop unmeasured items into view.
const CHAT_OVERDRAW: f32 = 256.;

/// App-global shell state: theme presets from the CLI palette plus the active
/// selection. Lives outside `ShellWindow` so button handlers can reach it.
struct ShellState {
    presets: Vec<ThemePreset>,
    active_preset: usize,
}

impl Global for ShellState {}

/// Lifecycle of the embedded runtime, mirrored into the status line.
#[derive(Clone)]
enum RuntimePhase {
    Starting,
    Ready,
    Failed(String),
}

/// One session list entry, composed (including localized metadata) when the
/// snapshot arrives so the render path never touches the i18n service.
struct SessionRow {
    id: String,
    name: String,
    metadata: String,
}

/// The open session in the chat pane.
struct ChatView {
    session_id: String,
    name: String,
    agent_type: String,
    entries: Vec<ChatEntry>,
    /// A turn is executing (streaming or tool calls in flight).
    busy: bool,
    /// A submission is in flight; the optimistic user entry awaits
    /// [`RuntimeEvent::TurnAccepted`].
    submitting: bool,
}

impl ChatView {
    fn matches(&self, session_id: &str) -> bool {
        self.session_id == session_id
    }

    /// Streaming chunks accumulate into one assistant entry per model round;
    /// rounds arrive contiguously, so comparing against the tail is enough.
    /// Returns `true` when a new entry was pushed (as opposed to growing the
    /// tail entry in place), so the caller can sync the virtualized list.
    fn append_chunk(&mut self, round_id: &str, text: &str) -> bool {
        if let Some(ChatEntry::Assistant {
            round_id: tail_round,
            text: tail_text,
        }) = self.entries.last_mut()
        {
            if *tail_round == round_id {
                tail_text.push_str(text);
                return false;
            }
        }
        self.entries.push(ChatEntry::Assistant {
            round_id: round_id.to_string(),
            text: text.to_string(),
        });
        true
    }

    /// Upserts the tool card keyed by tool id; empty details never overwrite
    /// collected ones (lifecycle events carry them unevenly). Returns the
    /// index of an existing card that changed in place, or `None` when the
    /// card was appended.
    fn upsert_tool(&mut self, card: ToolCard) -> Option<usize> {
        let existing = self.entries.iter_mut().enumerate().find(
            |(_, entry)| matches!(entry, ChatEntry::Tool(existing) if existing.key == card.key),
        );
        if let Some((index, entry)) = existing {
            let ChatEntry::Tool(existing) = entry else {
                unreachable!("find matched a tool entry");
            };
            existing.status = card.status;
            if !card.detail.is_empty() {
                existing.detail = card.detail;
            }
            Some(index)
        } else {
            self.entries.push(ChatEntry::Tool(card));
            None
        }
    }
}

struct ShellWindow {
    draft: Entity<InputState>,
    phase: RuntimePhase,
    sessions: Vec<SessionRow>,
    list_error: Option<String>,
    chat: Option<ChatView>,
    chat_error: Option<String>,
    /// Virtualized transcript list state; item counts are kept in sync with
    /// `chat.entries` through the `list_sync_*` helpers.
    chat_list: ListState,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
}

impl ShellWindow {
    fn new(
        event_tx: mpsc::UnboundedSender<RuntimeEvent>,
        event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = i18n::current(cx);
        let draft = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(text.chat_input_placeholder.clone())
                .multi_line(true)
        });
        receive_runtime_events(event_rx, window, cx);
        Self {
            draft,
            phase: RuntimePhase::Starting,
            sessions: Vec::new(),
            list_error: None,
            chat: None,
            chat_error: None,
            chat_list: ListState::new(0, ListAlignment::Bottom, px(CHAT_OVERDRAW)),
            event_tx,
        }
    }

    /// Applies one locale snapshot to the locale-sensitive input state.
    fn apply_locale(&mut self, text: &ShellText, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.update(cx, |state, cx| {
            state.set_placeholder(text.chat_input_placeholder.clone(), window, cx)
        });
    }

    fn compose_row(&self, session: &AgentSessionSummary, cx: &App) -> SessionRow {
        let mut metadata = vec![session.agent_type.clone()];
        if let Some(model) = &session.model_id {
            metadata.push(model.clone());
        }
        metadata.push(i18n::turns_label(session.turn_count, cx));
        metadata.push(runtime::format_last_active(session.last_active_at_ms));
        SessionRow {
            id: session.session_id.clone(),
            name: session.session_name.clone(),
            metadata: metadata
                .iter()
                .filter(|part| !part.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" · "),
        }
    }

    /// Entry count grew by one: register the new item as unmeasured and keep
    /// the newest content in view, matching the previous scroll-to-bottom
    /// behavior for appends.
    fn list_sync_append(&mut self) {
        let count = self.entry_count();
        self.chat_list.splice(appended_range(count), 1);
        self.scroll_list_to_bottom();
    }

    /// The tail entry changed in place (streaming chunk or tool-card update);
    /// its cached height is stale until the list re-measures it.
    fn list_sync_tail_changed(&mut self) {
        let count = self.entry_count();
        if count > 0 {
            self.chat_list.splice(tail_changed_range(count), 1);
        }
        self.scroll_list_to_bottom();
    }

    /// An entry below the tail changed in place (tool lifecycle upserts land
    /// on cards created earlier in the turn).
    fn list_sync_item_changed(&mut self, index: usize) {
        self.chat_list.splice(index..index + 1, 1);
    }

    /// The transcript was replaced wholesale (session opened or reloaded);
    /// resetting also drops the logical scroll top, which pins the
    /// bottom-aligned list to the newest content.
    fn list_sync_reset(&mut self) {
        self.chat_list.reset(self.entry_count());
    }

    fn scroll_list_to_bottom(&mut self) {
        // Out-of-range item indices are clamped to the end by `scroll_to`.
        self.chat_list.scroll_to(ListOffset {
            item_ix: self.entry_count(),
            offset_in_item: px(0.),
        });
    }

    fn entry_count(&self) -> usize {
        self.chat
            .as_ref()
            .map(|chat| chat.entries.len())
            .unwrap_or(0)
    }

    fn apply_runtime_event(
        &mut self,
        event: RuntimeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RuntimeEvent::Locale(locale) => {
                i18n::load_locale(locale, cx);
                let text = i18n::current(cx);
                self.apply_locale(&text, window, cx);
            }
            RuntimeEvent::Ready => {
                self.phase = RuntimePhase::Ready;
            }
            RuntimeEvent::Sessions(sessions) => {
                self.phase = RuntimePhase::Ready;
                self.list_error = None;
                self.sessions = sessions.iter().map(|s| self.compose_row(s, cx)).collect();
            }
            RuntimeEvent::SessionsFailed(error) => {
                self.list_error = Some(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::Failed(error) => {
                self.phase = RuntimePhase::Failed(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::SessionOpened(opened) => {
                self.chat_error = None;
                match self.chat.as_mut() {
                    // Transcript reload for the open session keeps the pane title.
                    Some(chat) if chat.matches(&opened.session_id) => {
                        chat.entries = opened.entries;
                        chat.busy = opened.busy;
                        chat.submitting = false;
                    }
                    _ => {
                        self.chat = Some(ChatView {
                            session_id: opened.session_id,
                            name: opened.name,
                            agent_type: opened.agent_type,
                            entries: opened.entries,
                            busy: opened.busy,
                            submitting: false,
                        });
                    }
                }
                self.list_sync_reset();
                tracing::info!("Session transcript ready in chat pane");
            }
            RuntimeEvent::SessionOpenFailed(error) => {
                self.chat_error = Some(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::TurnAccepted {
                session_id,
                turn_id: _,
                message,
            } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.submitting = false;
                        chat.busy = true;
                        chat.entries.push(ChatEntry::User { text: message });
                        self.list_sync_append();
                    }
                }
            }
            RuntimeEvent::SendFailed { session_id, error } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.submitting = false;
                        // The user entry is appended on TurnAccepted, so a
                        // failed submission only surfaces the error notice.
                        chat.entries.push(ChatEntry::Notice {
                            text: i18n::failed_status(&error, cx),
                        });
                        self.list_sync_append();
                    }
                }
            }
            RuntimeEvent::TurnStarted { session_id, .. } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.submitting = false;
                        chat.busy = true;
                    }
                }
            }
            RuntimeEvent::TextChunk {
                session_id,
                turn_id: _,
                round_id,
                text,
            } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        if chat.append_chunk(&round_id, &text) {
                            self.list_sync_append();
                        } else {
                            self.list_sync_tail_changed();
                        }
                    }
                }
            }
            RuntimeEvent::ToolUpdate { session_id, card } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        match chat.upsert_tool(card) {
                            None => self.list_sync_append(),
                            Some(index) => self.list_sync_item_changed(index),
                        }
                    }
                }
            }
            RuntimeEvent::TurnFinished { session_id, error } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.busy = false;
                        chat.submitting = false;
                        if let Some(error) = error {
                            chat.entries.push(ChatEntry::Notice {
                                text: i18n::failed_status(&error, cx),
                            });
                            self.list_sync_append();
                        }
                    }
                }
                // Session names/metadata change as turns settle.
                if let Some(handle) = runtime::runtime_handle() {
                    handle.reload_sessions(self.event_tx.clone());
                }
            }
            RuntimeEvent::HistoryChanged { session_id } => {
                if self
                    .chat
                    .as_ref()
                    .is_some_and(|chat| chat.matches(&session_id))
                {
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.reload_transcript(session_id, self.event_tx.clone());
                    }
                }
            }
        }
        cx.notify();
    }

    fn render_status_line(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::Div {
        match &self.phase {
            RuntimePhase::Starting => div()
                .text_color(cx.theme().muted_foreground)
                .child(text.status_starting.clone()),
            RuntimePhase::Ready => {
                let workspace = runtime::runtime_handle()
                    .map(|handle| handle.workspace().name.clone())
                    .unwrap_or_default();
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", text.status_ready, workspace))
            }
            RuntimePhase::Failed(error) => div().text_color(cx.theme().danger).child(error.clone()),
        }
    }

    fn render_sessions(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut header = div()
            .id("sessions-header")
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.sessions_title.clone()),
            );
        if matches!(self.phase, RuntimePhase::Ready) {
            let new_tx = self.event_tx.clone();
            let reload_tx = self.event_tx.clone();
            header = header.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_1()
                    .child(
                        Button::new("new-session")
                            .label(text.sessions_new.clone())
                            .small()
                            .with_variant(ButtonVariant::Primary)
                            .on_click(move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.create_session(new_tx.clone());
                                }
                            }),
                    )
                    .child(
                        Button::new("reload-sessions")
                            .label(text.sessions_reload.clone())
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.reload_sessions(reload_tx.clone());
                                }
                            }),
                    ),
            );
        }

        let mut list = div().v_flex().id("sessions-list").gap_2().p_1();
        if self.sessions.is_empty() {
            list = list.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.sessions_empty.clone()),
            );
        }
        for row in &self.sessions {
            let open = self.chat.as_ref().is_some_and(|chat| chat.matches(&row.id));
            list = list.child(
                div()
                    .id(SharedString::from(format!("session-{}", row.id)))
                    .on_click({
                        let session_id = row.id.clone();
                        let tx = self.event_tx.clone();
                        move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.open_session(session_id.clone(), tx.clone());
                            }
                        }
                    })
                    .cursor_pointer()
                    .v_flex()
                    .gap_0p5()
                    .min_w_0()
                    .p_1()
                    .rounded(cx.theme().radius)
                    .when(open, |this| this.bg(cx.theme().muted))
                    .when(!open, |this| this.hover(|style| style.bg(cx.theme().muted)))
                    .child(
                        div()
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(row.metadata.clone()),
                    ),
            );
        }

        let mut section = div()
            .v_flex()
            .id("sessions-section")
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(header);
        if let Some(error) = &self.list_error {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        section.child(list.overflow_y_scrollbar())
    }

    fn render_verification(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::Div {
        let language_button = Button::new("language")
            .label(text.locale.as_str())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(|_, _, cx| {
                let next = i18n::next_locale(i18n::current(cx).locale);
                i18n::load_locale(next, cx);
            });

        let active_preset = cx.global::<ShellState>().active_preset;
        let preset_labels: Vec<(&'static str, usize)> = cx
            .global::<ShellState>()
            .presets
            .iter()
            .enumerate()
            .map(|(index, preset)| (preset.label, index))
            .collect();

        let mut theme_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.theme.clone()),
            );
        for (label, index) in preset_labels {
            let variant = if index == active_preset {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Ghost
            };
            theme_row = theme_row.child(
                Button::new(SharedString::from(format!("preset-{label}")))
                    .label(label)
                    .small()
                    .with_variant(variant)
                    .on_click(move |_, _, cx| {
                        let preset = {
                            let state = cx.global_mut::<ShellState>();
                            state.active_preset = index;
                            state.presets[index].clone()
                        };
                        preset.apply(cx);
                    }),
            );
        }

        div()
            .v_flex()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}: {}", text.language, text.locale.as_str())),
                    )
                    .child(language_button),
            )
            .child(theme_row)
    }
}

/// `list` splice range registering one new item at the end of a transcript of
/// `count` entries (the range is empty; the item is inserted past the end).
fn appended_range(count: usize) -> std::ops::Range<usize> {
    count.saturating_sub(1)..count.saturating_sub(1)
}

/// `list` splice range invalidating the cached height of a transcript tail
/// that changed in place.
fn tail_changed_range(count: usize) -> std::ops::Range<usize> {
    (count - 1)..count
}

fn tool_status_label<'a>(status: ToolStatus, text: &'a ShellText) -> &'a str {
    match status {
        ToolStatus::Running => &text.tool_running,
        ToolStatus::WaitingConfirm => &text.tool_waiting,
        ToolStatus::Done => &text.tool_done,
        ToolStatus::Failed => &text.tool_failed,
        ToolStatus::Cancelled => &text.tool_cancelled,
    }
}

/// One transcript entry as a list item. The gutter padding (`px_2`) and the
/// trailing gap (`pb_3`) are part of the item because gpui's `list` has no
/// inter-item gap support.
fn render_chat_item(
    entry: &ChatEntry,
    text: &ShellText,
    window: &mut Window,
    cx: &mut App,
) -> gpui::AnyElement {
    match entry {
        ChatEntry::User { text: body } => div()
            .w_full()
            .px_2()
            .pb_3()
            .flex()
            .justify_end()
            .child(
                div()
                    .max_w(px(480.))
                    .bg(cx.theme().muted)
                    .rounded(cx.theme().radius)
                    .px_2()
                    .py_1()
                    .child(body.clone()),
            )
            .into_any_element(),
        ChatEntry::Assistant { text: body, .. } => div()
            .w_full()
            .min_w_0()
            .px_2()
            .pb_3()
            .text_sm()
            .child(
                TextView::markdown(
                    SharedString::from(format!("entry-{}-md", markdown_id_key(body))),
                    body.clone(),
                    window,
                    cx,
                )
                .selectable(true),
            )
            .into_any_element(),
        ChatEntry::Tool(card) => div()
            .w_full()
            .px_2()
            .pb_3()
            .child(render_tool_card(card, text, cx))
            .into_any_element(),
        ChatEntry::Notice { text: body } => div()
            .w_full()
            .px_2()
            .pb_3()
            .text_sm()
            .text_color(cx.theme().danger)
            .child(body.clone())
            .into_any_element(),
    }
}

/// Stable per-content markdown element id: transcript indices shift when
/// history reloads swap the entries vec, and a reused id pointing at new
/// content would briefly show the previous parse while the worker catches up.
fn markdown_id_key(body: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(body, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

fn render_tool_card(card: &ToolCard, text: &ShellText, cx: &App) -> gpui::Div {
    let status_color = match card.status {
        ToolStatus::Running | ToolStatus::WaitingConfirm => cx.theme().warning,
        ToolStatus::Done => cx.theme().success,
        ToolStatus::Failed | ToolStatus::Cancelled => cx.theme().danger,
    };
    let status_label = tool_status_label(card.status, text);
    let mut row = div()
        .v_flex()
        .gap_0p5()
        .min_w_0()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .px_2()
        .py_1()
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(card.tool_name.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(status_color)
                        .child(status_label.to_string()),
                ),
        );
    if !card.detail.is_empty() {
        row = row.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(card.detail.clone()),
        );
    }
    row
}

impl ShellWindow {
    /// The virtualized transcript: gpui's native `list` element keeps measured
    /// per-item heights in a sum tree and only renders the visible range plus
    /// overdraw, so a 1k-message transcript neither re-parses off-screen
    /// markdown nor re-renders it per frame. Bottom alignment pins the view to
    /// the newest content until the user scrolls up.
    fn render_chat_area(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.chat.is_none() {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(text.chat_empty.clone())
                .into_any_element();
        }
        let this = cx.entity();
        let text = text.clone();
        list(self.chat_list.clone(), move |index, window, cx| {
            let entry = this
                .read(cx)
                .chat
                .as_ref()
                .and_then(|chat| chat.entries.get(index))
                .cloned();
            match entry {
                Some(entry) => render_chat_item(&entry, &text, window, cx),
                // List-state item counts are synced on the same mutation that
                // grows `entries`, so an out-of-range index cannot persist.
                None => div().into_any_element(),
            }
        })
        .flex_1()
        .min_h_0()
        .pt_2()
        .pb_2()
        .into_any_element()
    }

    /// Streaming indicator rendered as a fixed row between the transcript and
    /// the composer (list items are reserved for transcript entries).
    fn busy_label(&self, text: &ShellText) -> Option<String> {
        let chat = self.chat.as_ref()?;
        if chat.submitting {
            Some(text.chat_submitting.clone())
        } else if chat.busy {
            Some(text.chat_busy.clone())
        } else {
            None
        }
    }

    fn render_composer(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::Div {
        let Some(chat) = self.chat.as_ref() else {
            return div();
        };
        let busy = chat.busy;
        let submitting = chat.submitting;
        let session_id = chat.session_id.clone();

        let draft_value = self.draft.read(cx).value().to_string();
        let draft_for_send = self.draft.clone();
        let tx_for_send = self.event_tx.clone();
        let session_for_send = session_id.clone();

        let composer = div()
            .flex()
            .flex_row()
            .gap_2()
            .items_end()
            .child(Input::new(&self.draft).flex_1().h(px(COMPOSER_HEIGHT)))
            .child(if busy {
                Button::new("chat-stop")
                    .label(text.chat_stop.clone())
                    .with_variant(ButtonVariant::Danger)
                    .on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.cancel_turn(session_id.clone());
                        }
                    })
            } else {
                Button::new("chat-send")
                    .label(text.chat_send.clone())
                    .with_variant(ButtonVariant::Primary)
                    .disabled(submitting || draft_value.trim().is_empty())
                    .on_click(move |_, window, cx| {
                        let body = draft_for_send.read(cx).value().to_string();
                        if body.trim().is_empty() {
                            return;
                        }
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.send_message(
                                session_for_send.clone(),
                                body,
                                tx_for_send.clone(),
                            );
                        }
                        draft_for_send.update(cx, |state, cx| {
                            state.set_value("", window, cx);
                        });
                    })
            });
        composer
    }
}

impl Render for ShellWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text: ShellText = i18n::current(cx);
        let chat_area = self.render_chat_area(&text, cx);

        div()
            .size_full()
            .v_flex()
            .gap_3()
            .p_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .v_flex()
                            .gap_0p5()
                            .child(div().text_size(px(20.)).child("OpenBitFun GPUI Shell"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.subtitle.clone()),
                            ),
                    )
                    .child(self.render_status_line(&text, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(
                        div()
                            .w(px(SESSION_PANE_WIDTH))
                            .min_h_0()
                            .v_flex()
                            .gap_2()
                            .child(self.render_sessions(&text, cx))
                            .child(self.render_verification(&text, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .v_flex()
                            .gap_2()
                            .child(self.render_chat_header(&text, cx))
                            .when_some(self.chat_error.clone(), |this, error| {
                                this.child(
                                    div().text_sm().text_color(cx.theme().danger).child(error),
                                )
                            })
                            .child(chat_area)
                            .when_some(self.busy_label(&text), |this, label| {
                                this.child(
                                    div()
                                        .px_2()
                                        .pb_1()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(label),
                                )
                            })
                            .child(self.render_composer(&text, cx)),
                    ),
            )
    }
}

impl ShellWindow {
    fn render_chat_header(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui::Div {
        match self.chat.as_ref() {
            Some(chat) => div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .v_flex()
                        .gap_0p5()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(chat.name.clone()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(chat.agent_type.clone()),
                        ),
                )
                .child({
                    let active = chat.busy || chat.submitting;
                    let label = if chat.submitting {
                        text.chat_submitting.clone()
                    } else {
                        text.chat_busy.clone()
                    };
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .when(active, |this| this.child(label))
                }),
            None => div(),
        }
    }
}

/// Bridges runtime events into the view: the tokio side of the shell pushes
/// [`RuntimeEvent`]s, this loop applies them on the UI thread.
fn receive_runtime_events(
    mut event_rx: mpsc::UnboundedReceiver<RuntimeEvent>,
    window: &Window,
    cx: &mut Context<ShellWindow>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(event) = event_rx.recv().await {
            let alive = cx
                .update(|window, cx| {
                    this.update(cx, |this, cx| this.apply_runtime_event(event, window, cx))
                        .is_ok()
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        }
    })
    .detach();
}

fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stdout)
        .try_init();
}

fn main() {
    init_tracing();
    tracing::info!("Starting OpenBitFun GPUI shell");

    // Process-lifetime tokio runtime: core services are async (config, i18n,
    // runtime bootstrap, session store) while the GPUI main loop is not.
    // Agent execution exceeds the default worker stack in debug builds, same
    // as the CLI (`src/apps/cli/src/main.rs` uses 16MiB for the same reason);
    // runtime threads do not inherit the spawning thread's stack size.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let tokio_handle = tokio_runtime.handle().clone();
    std::mem::forget(tokio_runtime);

    let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
    // The view keeps its own sender for the session reload action.
    let view_event_tx = event_tx.clone();

    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);

        let presets = theme::load_presets();
        let active_preset = presets
            .iter()
            .position(|preset| preset.id == DEFAULT_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active_preset) {
            preset.apply(cx);
        }
        cx.set_global(ShellState {
            presets,
            active_preset,
        });

        i18n::init(cx);

        runtime::spawn_bootstrap(tokio_handle, event_tx);

        let open_result = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("OpenBitFun GPUI Shell")),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: gpui::point(px(80.), px(80.)),
                    size: size(px(1180.), px(760.)),
                })),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| ShellWindow::new(view_event_tx, event_rx, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );

        match open_result {
            Ok(_) => {
                tracing::info!("Main window opened");
                cx.activate(true);
            }
            Err(err) => {
                tracing::error!("Failed to open main window: {}", err);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ToolStatus;
    use openbitfun_core::service::i18n::LocaleId;
    use std::cell::Cell;
    use std::rc::Rc;

    fn chat_with(entries: Vec<ChatEntry>) -> ChatView {
        ChatView {
            session_id: "session".into(),
            name: "name".into(),
            agent_type: "Standard".into(),
            entries,
            busy: false,
            submitting: false,
        }
    }

    #[test]
    fn append_chunk_reports_tail_growth_vs_new_entry() {
        let mut chat = chat_with(Vec::new());
        assert!(chat.append_chunk("round-1", "hello "));
        assert!(!chat.append_chunk("round-1", "world"));
        assert!(chat.append_chunk("round-2", "next round"));
        assert_eq!(chat.entries.len(), 2);
    }

    #[test]
    fn upsert_tool_reports_changed_index_or_append() {
        let card = |key: &'static str| ToolCard {
            key: SharedString::from(key),
            tool_name: "read_file".into(),
            status: ToolStatus::Running,
            detail: String::new(),
        };
        let mut chat = chat_with(Vec::new());
        assert!(chat.upsert_tool(card("tool-1")).is_none());
        assert!(chat.upsert_tool(card("tool-2")).is_none());
        assert_eq!(chat.upsert_tool(card("tool-1")), Some(0));
        assert!(chat.upsert_tool(card("tool-3")).is_none());
        assert_eq!(chat.entries.len(), 3);
    }

    #[test]
    fn list_sync_ranges_match_entry_counts() {
        assert_eq!(appended_range(0), 0..0);
        assert_eq!(appended_range(3), 2..2);
        assert_eq!(tail_changed_range(1), 0..1);
        assert_eq!(tail_changed_range(40), 39..40);
    }

    /// A harness view mirroring `ShellWindow::render_chat_area` so the test
    /// exercises the same list construction and item renderer.
    struct TranscriptListHarness {
        state: ListState,
        entries: Rc<Vec<ChatEntry>>,
        render_count: Rc<Cell<usize>>,
        text: ShellText,
    }

    impl Render for TranscriptListHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let render_count = self.render_count.clone();
            let text = self.text.clone();
            list(self.state.clone(), move |index, window, cx| {
                render_count.set(render_count.get() + 1);
                match this.read(cx).entries.get(index).cloned() {
                    Some(entry) => render_chat_item(&entry, &text, window, cx),
                    None => div().into_any_element(),
                }
            })
            .w(px(800.))
            .h(px(600.))
        }
    }

    fn sample_shell_text() -> ShellText {
        ShellText {
            locale: LocaleId::EnUS,
            subtitle: String::new(),
            theme: String::new(),
            language: String::new(),
            status_starting: String::new(),
            status_ready: String::new(),
            sessions_title: String::new(),
            sessions_empty: String::new(),
            sessions_reload: String::new(),
            sessions_new: String::new(),
            chat_empty: String::new(),
            chat_input_placeholder: String::new(),
            chat_send: String::new(),
            chat_stop: String::new(),
            chat_busy: String::new(),
            chat_submitting: String::new(),
            tool_running: "Running".into(),
            tool_waiting: "Waiting".into(),
            tool_done: "Done".into(),
            tool_failed: "Failed".into(),
            tool_cancelled: "Cancelled".into(),
        }
    }

    /// Alternating user/assistant pairs with markdown of varying length, plus
    /// a fenced code block every tenth round.
    fn stress_entries(rounds: usize) -> Vec<ChatEntry> {
        let mut entries = Vec::with_capacity(rounds * 2);
        for index in 0..rounds {
            entries.push(ChatEntry::User {
                text: format!("Stress round {index}: please explain item {index}."),
            });
            let paragraph = format!(
                "Answer **{index}** with a [link](https://example.com/{index}) and `inline code`.\n\n\
                 - first point for round {index}\n- second point with a longer line of text that wraps\n\n\
                 | column a | column b |\n|---|---|\n| {index} | value |"
            );
            let body = if index % 10 == 0 {
                format!(
                    "{paragraph}\n\n```rust\nfn round_{index}() -> usize {{\n    {index} * 2\n}}\n```"
                )
            } else {
                paragraph
            };
            entries.push(ChatEntry::Assistant {
                round_id: format!("round-{index}"),
                text: body,
            });
        }
        entries
    }

    #[gpui::test]
    fn transcript_list_renders_visible_range_only(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| gpui_component::init(cx));

        const ROUNDS: usize = 500;
        let entries = Rc::new(stress_entries(ROUNDS));
        let render_count: Rc<Cell<usize>> = Default::default();
        let state = ListState::new(entries.len(), ListAlignment::Bottom, px(CHAT_OVERDRAW));
        let text = sample_shell_text();

        let window = cx.add_empty_window();
        let draw_frame = |window: &mut gpui::VisualTestContext,
                          state: &ListState,
                          entries: &Rc<Vec<ChatEntry>>,
                          render_count: &Rc<Cell<usize>>|
         -> std::time::Duration {
            render_count.set(0);
            let state = state.clone();
            let entries = entries.clone();
            let render_count = render_count.clone();
            let text = sample_shell_text();
            let started = std::time::Instant::now();
            window.draw(
                gpui::point(px(0.), px(0.)),
                size(px(800.), px(600.)),
                move |_, cx| {
                    cx.new(|_| TranscriptListHarness {
                        state,
                        entries,
                        render_count,
                        text,
                    })
                },
            );
            started.elapsed()
        };

        // Transcript restore: the first frame must neither render nor measure
        // all 1k entries (markdown parse only happens for rendered items).
        let elapsed = draw_frame(window, &state, &entries, &render_count);
        let restored = render_count.get();
        assert!(
            restored < 100,
            "restore rendered {restored} of {} entries in one frame",
            entries.len()
        );

        // Sweep the whole list top to bottom; every frame stays bounded and
        // the list keeps tracking the bottom-pinned offset.
        let mut max_per_frame = 0usize;
        let mut total = std::time::Duration::ZERO;
        let steps = 60;
        for _ in 0..steps {
            state.scroll_by(px(1000.));
            window.run_until_parked();
            let elapsed = draw_frame(window, &state, &entries, &render_count);
            max_per_frame = max_per_frame.max(render_count.get());
            total += elapsed;
        }
        assert!(
            max_per_frame < 100,
            "a scroll frame rendered {max_per_frame} entries"
        );
        let average_us = total.as_micros() / steps as u128;
        println!(
            "transcript sweep: {} entries, {steps} frames, max {max_per_frame} rendered/frame, \
             {average_us} us average frame layout",
            entries.len()
        );

        // Streaming growth on top of the restored transcript keeps item
        // accounting exact.
        let mut grown = entries.as_ref().clone();
        for round in 0..5 {
            grown.push(ChatEntry::User {
                text: format!("live round {round}"),
            });
            state.splice(appended_range(grown.len()), 1);
            grown.push(ChatEntry::Assistant {
                round_id: format!("live-{round}"),
                text: "streamed **markdown** body.\n\n- item\n- item".to_string(),
            });
            state.splice(appended_range(grown.len()), 1);
            state.splice(tail_changed_range(grown.len()), 1);
        }
        assert_eq!(state.item_count(), grown.len());
        let entries = Rc::new(grown);
        draw_frame(window, &state, &entries, &render_count);
        assert!(
            render_count.get() < 100,
            "post-growth frame rendered {} entries",
            render_count.get()
        );
    }
}
