mod i18n;
mod runtime;
mod theme;

use gpui::{
    div, point, prelude::*, px, size, App, Application, Bounds, Context, Entity, Global,
    ScrollHandle, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions,
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
    fn append_chunk(&mut self, round_id: &str, text: &str) {
        if let Some(ChatEntry::Assistant {
            round_id: tail_round,
            text: tail_text,
        }) = self.entries.last_mut()
        {
            if *tail_round == round_id {
                tail_text.push_str(text);
                return;
            }
        }
        self.entries.push(ChatEntry::Assistant {
            round_id: round_id.to_string(),
            text: text.to_string(),
        });
    }

    /// Upserts the tool card keyed by tool id; empty details never overwrite
    /// collected ones (lifecycle events carry them unevenly).
    fn upsert_tool(&mut self, card: ToolCard) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| matches!(entry, ChatEntry::Tool(existing) if existing.key == card.key))
        {
            let ChatEntry::Tool(existing) = entry else {
                unreachable!("find matched a tool entry");
            };
            existing.status = card.status;
            if !card.detail.is_empty() {
                existing.detail = card.detail;
            }
        } else {
            self.entries.push(ChatEntry::Tool(card));
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
    chat_scroll: ScrollHandle,
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
            chat_scroll: ScrollHandle::new(),
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

    /// Keeps the chat pane pinned to the newest content (best effort: the
    /// offset uses the previous frame's layout, which trails one frame behind
    /// fast streams but never scrolls the wrong way).
    fn scroll_chat_to_bottom(&self) {
        let bottom = self.chat_scroll.max_offset().height;
        self.chat_scroll.set_offset(point(px(0.), bottom));
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
                self.scroll_chat_to_bottom();
                tracing::info!("Session transcript ready in chat pane");
            }
            RuntimeEvent::SessionOpenFailed(error) => {
                self.chat_error = Some(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::TurnAccepted {
                session_id,
                turn_id: _,
            } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.submitting = false;
                        chat.busy = true;
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
                        chat.append_chunk(&round_id, &text);
                        self.scroll_chat_to_bottom();
                    }
                }
            }
            RuntimeEvent::ToolUpdate { session_id, card } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.upsert_tool(card);
                        self.scroll_chat_to_bottom();
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

    fn tool_status_label<'a>(&self, status: ToolStatus, text: &'a ShellText) -> &'a str {
        match status {
            ToolStatus::Running => &text.tool_running,
            ToolStatus::WaitingConfirm => &text.tool_waiting,
            ToolStatus::Done => &text.tool_done,
            ToolStatus::Failed => &text.tool_failed,
            ToolStatus::Cancelled => &text.tool_cancelled,
        }
    }

    fn render_entries(
        &self,
        text: &ShellText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let mut entries = div()
            .id("chat-entries")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap_3()
            .p_2()
            .overflow_y_scroll()
            .track_scroll(&self.chat_scroll);
        let Some(chat) = self.chat.as_ref() else {
            return entries.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.chat_empty.clone()),
            );
        };

        for (index, entry) in chat.entries.iter().enumerate() {
            match entry {
                ChatEntry::User { text: body } => {
                    entries = entries.child(
                        div().flex().justify_end().child(
                            div()
                                .max_w(px(480.))
                                .bg(cx.theme().muted)
                                .rounded(cx.theme().radius)
                                .px_2()
                                .py_1()
                                .child(body.clone()),
                        ),
                    )
                }
                ChatEntry::Assistant { text: body, .. } => {
                    entries = entries.child(
                        div()
                            .id(SharedString::from(format!("entry-{index}")))
                            .min_w_0()
                            .text_sm()
                            .child(
                                TextView::markdown(
                                    SharedString::from(format!("entry-{index}-md")),
                                    body.clone(),
                                    window,
                                    cx,
                                )
                                .selectable(true),
                            ),
                    )
                }
                ChatEntry::Tool(card) => {
                    entries = entries.child(self.render_tool_card(card, text, cx))
                }
                ChatEntry::Notice { text: body } => {
                    entries = entries.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(body.clone()),
                    )
                }
            }
        }
        if chat.busy || chat.submitting {
            let label = if chat.submitting {
                &text.chat_submitting
            } else {
                &text.chat_busy
            };
            entries = entries.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.clone()),
            );
        }
        entries
    }

    fn render_tool_card(
        &self,
        card: &ToolCard,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let status_color = match card.status {
            ToolStatus::Running | ToolStatus::WaitingConfirm => cx.theme().warning,
            ToolStatus::Done => cx.theme().success,
            ToolStatus::Failed | ToolStatus::Cancelled => cx.theme().danger,
        };
        let status_label = self.tool_status_label(card.status, text);
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
            .child(Input::new(&self.draft).w_full().h(px(COMPOSER_HEIGHT)))
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text: ShellText = i18n::current(cx);

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
                            .child(self.render_entries(&text, window, cx))
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
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
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
