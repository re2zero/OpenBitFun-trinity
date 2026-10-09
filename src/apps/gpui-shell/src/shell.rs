use crate::{i18n, runtime, settings, theme};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariant, ButtonVariants},
    input::{Textarea, TextareaState},
    message::MessageAlignment,
    message_scroller::{MessageScroller, MessageScrollerState},
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarItem},
    text::TextView,
    ActiveTheme, Collapsible, Disableable, Sizable, StyledExt, TitleBar, WindowExt,
};
use gpui_kit::{
    div, point, prelude::*, px, size, App, Bounds, Context, ElementId, Entity, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use openbitfun_agent_runtime::sdk::{PermissionReply, PermissionRequest};
use openbitfun_runtime_ports::AgentSessionSummary;
use tokio::sync::mpsc;

use crate::assets::ShellAssets;
use crate::git::GitPanel;
use crate::i18n::ShellText;
use crate::runtime::{ChatEntry, RuntimeEvent, ToolCard, ToolStatus};
use crate::terminal::pane::TerminalPane;
use crate::theme::{ShellState, DEFAULT_PRESET_ID};

/// Width of the navigation sidebar, matching the web-ui desktop shell.
const SESSION_PANE_WIDTH: f32 = 300.;
/// Transcript reading column; long-window content stays centered on it.
const READING_COLUMN_WIDTH: f32 = 900.;
/// User bubble cap inside the reading column, so short messages do not
/// stretch across the whole column.
const USER_BUBBLE_MAX_WIDTH: f32 = 480.;
/// Composer height; explicit pixels because an auto-grown textarea makes the
/// docked composer row jump while streaming text is typed.
const COMPOSER_HEIGHT: f32 = 88.;

/// Which surface the main content area shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MainView {
    Chat,
    /// The settings page (component-library `Settings` container).
    Settings,
    /// Workspace changes (git status + self-drawn diff).
    Git,
}

/// Lifecycle of the embedded runtime, mirrored into the status line.
#[derive(Clone)]
enum RuntimePhase {
    Starting,
    Ready,
    Failed(String),
}

/// One session list entry, composed (including localized metadata) when the
/// snapshot arrives so the render path never touches the i18n service.
#[derive(Clone)]
struct SessionRow {
    id: String,
    name: String,
    metadata: String,
}

/// One open session in the chat pane.
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

/// One pending approval: the runtime request plus display lines composed once
/// at arrival, so the dialog builder never touches the i18n service on the
/// render path (same pattern as [`SessionRow`] metadata).
#[derive(Clone)]
struct PermissionPromptItem {
    request: PermissionRequest,
    resources_line: Option<String>,
    project_line: String,
    delegation_line: Option<String>,
    risk_line: Option<String>,
}

impl PermissionPromptItem {
    fn request_id(&self) -> &str {
        &self.request.request_id
    }
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

/// One `Sidebar` content entry: a session row, the empty-state line, or the
/// list error line. One type because `Sidebar` children share one element
/// type; collapsed-state is inert because entries are always rendered.
#[derive(Clone)]
enum SidebarEntry {
    Session {
        row: SessionRow,
        open: bool,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    },
    Muted(SharedString),
    Danger(SharedString),
}

impl Collapsible for SidebarEntry {
    fn collapsed(self, _collapsed: bool) -> Self {
        self
    }

    fn is_collapsed(&self) -> bool {
        false
    }
}

impl SidebarItem for SidebarEntry {
    fn render(
        self,
        id: impl Into<ElementId>,
        _window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let _id = id.into();
        match self {
            SidebarEntry::Session { row, open, tx } => {
                let SessionRow { id, name, metadata } = row;
                div()
                    .id(SharedString::from(format!("session-{}", id)))
                    .on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.open_session(id.clone(), tx.clone());
                        }
                    })
                    .cursor_pointer()
                    .v_flex()
                    .gap_0p5()
                    .min_w_0()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .when(open, |this| this.bg(cx.theme().sidebar_accent))
                    .when(!open, |this| {
                        this.hover(|style| style.bg(cx.theme().sidebar_accent))
                    })
                    .child(
                        div()
                            .text_color(cx.theme().sidebar_foreground)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(metadata),
                    )
                    .into_any_element()
            }
            SidebarEntry::Muted(text) => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text)
                .into_any_element(),
            SidebarEntry::Danger(text) => div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(text)
                .into_any_element(),
        }
    }
}

struct ShellWindow {
    draft: Entity<TextareaState>,
    phase: RuntimePhase,
    sessions: Vec<SessionRow>,
    list_error: Option<String>,
    chat: Option<ChatView>,
    chat_error: Option<String>,
    /// Virtualized transcript state owned by the component library's
    /// `MessageScroller`; item counts are kept in sync with `chat.entries`
    /// through the `list_sync_*` helpers and tail-following keeps the newest
    /// content in view until the user scrolls up.
    chat_scroller: Entity<MessageScrollerState>,
    event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    /// Approvals waiting for a user decision, oldest first. Every ask in this
    /// process surfaces here regardless of which session is open — an
    /// unanswered ask parks its turn at the tool pipeline until cancellation,
    /// so hiding one would be exactly the deadlock this shell must not
    /// introduce.
    pending_permissions: Vec<PermissionPromptItem>,
    /// The modal approval dialog tracks the queue head; `true` from
    /// [`ShellWindow::open_permission_dialog`] until the resolving echo drains
    /// the queue.
    permission_dialog_open: bool,
    /// Built-in terminal panel (M7): created lazily on first open, shut down
    /// and dropped when the panel closes.
    terminal: Option<Entity<TerminalPane>>,
    terminal_open: bool,
    /// One-shot automation hook (`OPENBITFUN_GPUI_TERMINAL_AUTOOPEN`): opens
    /// the panel once the runtime is ready, so a screenshot run can inspect
    /// the pane without synthesizing input events.
    terminal_auto_open: bool,
    /// Main content surface; sidebar interactions and the title-bar buttons
    /// switch it.
    main_view: MainView,
    /// Workspace changes panel (M8): created when the view opens, dropped when
    /// it closes so the snapshot is refetched on the next open.
    git: Option<Entity<GitPanel>>,
    /// One-shot automation hooks for the settings page and the changes view,
    /// mirroring `OPENBITFUN_GPUI_TERMINAL_AUTOOPEN`.
    settings_auto_open: bool,
    git_auto_open: bool,
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
            TextareaState::new(window, cx).placeholder(text.chat_input_placeholder.clone())
        });
        let chat_scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        receive_runtime_events(event_rx, window, cx);
        Self {
            draft,
            phase: RuntimePhase::Starting,
            sessions: Vec::new(),
            list_error: None,
            chat: None,
            chat_error: None,
            chat_scroller,
            event_tx,
            pending_permissions: Vec::new(),
            permission_dialog_open: false,
            terminal: None,
            terminal_open: false,
            terminal_auto_open: std::env::var_os("OPENBITFUN_GPUI_TERMINAL_AUTOOPEN").is_some(),
            main_view: MainView::Chat,
            git: None,
            settings_auto_open: std::env::var_os("OPENBITFUN_GPUI_SETTINGS_AUTOOPEN").is_some(),
            git_auto_open: std::env::var_os("OPENBITFUN_GPUI_GIT_AUTOOPEN").is_some(),
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

    /// Entry count grew by one: register the new item and keep the newest
    /// content in view (the scroller's tail-follow mode tracks appends while
    /// the reader sits at the live edge).
    fn list_sync_append(&mut self, cx: &mut Context<Self>) {
        self.chat_scroller.update(cx, |scroller, cx| {
            scroller.append(1, cx);
        });
    }

    /// The tail entry changed in place (streaming chunk or tool-card update);
    /// its cached height is stale until the list re-measures it.
    fn list_sync_tail_changed(&mut self, cx: &mut Context<Self>) {
        let count = self.entry_count();
        if count > 0 {
            self.chat_scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(tail_changed_range(count), cx);
            });
        }
    }

    /// An entry below the tail changed in place (tool lifecycle upserts land
    /// on cards created earlier in the turn).
    fn list_sync_item_changed(&mut self, index: usize, cx: &mut Context<Self>) {
        self.chat_scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(index..index + 1, cx);
        });
    }

    /// The transcript was replaced wholesale (session opened or reloaded);
    /// resetting also drops the follow anchor, which pins the view to the
    /// newest content.
    fn list_sync_reset(&mut self, cx: &mut Context<Self>) {
        self.chat_scroller.update(cx, |scroller, cx| {
            scroller.reset(self.entry_count(), cx);
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
            RuntimeEvent::Config(config) => {
                let config = *config;
                let locale = config.language;
                cx.set_global(config.clone());
                i18n::load_locale(locale, cx);
                let text = i18n::current(cx);
                self.apply_locale(&text, window, cx);
                // The persisted appearance drives the shell theme; ids without
                // a shell palette keep the current theme and warn.
                if settings::apply_appearance(&config.appearance, window.appearance(), cx).is_none()
                {
                    tracing::warn!(
                        "Persisted appearance {} has no shell palette; keeping the current theme",
                        config.appearance
                    );
                }
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
                // Opening a session is a chat action: leave the settings or
                // changes surface and show the transcript.
                self.main_view = MainView::Chat;
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
                self.list_sync_reset(cx);
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
                        self.list_sync_append(cx);
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
                        self.list_sync_append(cx);
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
                            self.list_sync_append(cx);
                        } else {
                            self.list_sync_tail_changed(cx);
                        }
                    }
                }
            }
            RuntimeEvent::ToolUpdate { session_id, card } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        match chat.upsert_tool(card) {
                            None => self.list_sync_append(cx),
                            Some(index) => self.list_sync_item_changed(index, cx),
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
                            self.list_sync_append(cx);
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
            RuntimeEvent::PermissionAsked { request } => {
                // FIFO arrival order: the runtime registers batch requests in
                // tool-call order, so the queue head is always the oldest
                // pending ask.
                let text = i18n::current(cx);
                self.pending_permissions
                    .push(compose_permission_item(request, &text, cx));
                if !self.permission_dialog_open {
                    self.open_permission_dialog(window, cx);
                }
            }
            RuntimeEvent::PermissionResolved { request_id } => {
                self.pending_permissions
                    .retain(|item| item.request_id() != request_id);
                if self.pending_permissions.is_empty() && self.permission_dialog_open {
                    window.close_dialog(cx);
                    self.permission_dialog_open = false;
                }
            }
            RuntimeEvent::PermissionSnapshot(requests) => {
                let text = i18n::current(cx);
                self.pending_permissions = requests
                    .into_iter()
                    .map(|request| compose_permission_item(request, &text, cx))
                    .collect();
                match (
                    self.pending_permissions.is_empty(),
                    self.permission_dialog_open,
                ) {
                    (true, true) => {
                        window.close_dialog(cx);
                        self.permission_dialog_open = false;
                    }
                    (false, false) => self.open_permission_dialog(window, cx),
                    // Head swapped under an open dialog: the builder re-reads
                    // the queue every frame, so the surface refreshes itself.
                    _ => {}
                }
            }
        }
        cx.notify();
    }

    /// Opens or closes the built-in terminal panel. First open creates the
    /// pane (and its PTY); close shuts the PTY down and drops the pane.
    fn toggle_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_open {
            self.terminal_open = false;
            if let Some(pane) = self.terminal.take() {
                pane.update(cx, |pane, _| pane.close());
            }
        } else {
            self.open_terminal_panel(window, cx);
        }
        cx.notify();
    }

    /// Opens the terminal panel, creating the pane (and its PTY) on first use.
    fn open_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_open = true;
        if self.terminal.is_none() {
            let tokio = runtime::runtime_handle()
                .map(|handle| handle.tokio_handle())
                .expect("runtime handle ready when terminal opens");
            self.terminal = Some(cx.new(|cx| TerminalPane::new(tokio, window, cx)));
        }
        cx.notify();
    }

    /// Shows the settings page in the main area. The page owns its own
    /// sidebar; the session sidebar stays where it is.
    fn open_settings_view(&mut self, cx: &mut Context<Self>) {
        self.main_view = MainView::Settings;
        cx.notify();
    }

    /// Opens the changes view (creating the panel, which fetches the workspace
    /// diff snapshot) or returns to the chat when it is already showing.
    fn toggle_git_view(&mut self, cx: &mut Context<Self>) {
        if self.main_view == MainView::Git {
            self.main_view = MainView::Chat;
            // Drop the panel with the view so the next open refetches.
            self.git = None;
        } else {
            self.main_view = MainView::Git;
            self.git = Some(cx.new(GitPanel::new));
        }
        cx.notify();
    }

    fn render_status_line(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::Div {
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

    /// Sidebar entries: one session row each, plus the empty-state and error
    /// lines. One type because `Sidebar` children share one element type.
    fn render_session_entries(&self, text: &ShellText) -> Vec<SidebarEntry> {
        let mut entries = Vec::new();
        if let Some(error) = &self.list_error {
            entries.push(SidebarEntry::Danger(error.clone().into()));
        }
        if self.sessions.is_empty() {
            entries.push(SidebarEntry::Muted(text.sessions_empty.clone().into()));
        }
        for row in &self.sessions {
            entries.push(SidebarEntry::Session {
                row: row.clone(),
                open: self.chat.as_ref().is_some_and(|chat| chat.matches(&row.id)),
                tx: self.event_tx.clone(),
            });
        }
        entries
    }

    /// The navigation sidebar: session commands in the header, the session
    /// list as the content, and the runtime status plus the settings menu
    /// (theme, language) in the footer.
    fn render_sidebar(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> Sidebar<SidebarGroup<SidebarEntry>> {
        let new_tx = self.event_tx.clone();
        let reload_tx = self.event_tx.clone();
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
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
                    .icon(IconName::RefreshCw)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(text.sessions_reload.clone())
                    .on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.reload_sessions(reload_tx.clone());
                        }
                    }),
            );

        Sidebar::new("shell-sidebar")
            .w(px(SESSION_PANE_WIDTH))
            .header(header)
            .child(
                SidebarGroup::new(text.sessions_title.clone())
                    .children(self.render_session_entries(text)),
            )
            .footer(
                SidebarFooter::new().child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(self.render_status_line(text, cx)),
                        )
                        .child(self.render_footer_actions(text, cx)),
                ),
            )
    }

    /// Footer actions: the surface toggles live here, not in the title bar. On
    /// Linux with client-side decorations the component title bar paints an
    /// absolute right-click window-menu overlay above its children, which
    /// swallows hover and clicks for every element inside it
    /// (gpui-component 0.7.1, reproduced on Xvfb 2026-10-09: sidebar buttons
    /// react, title-bar buttons never do). The sidebar footer is always
    /// visible and receives input; the active toggle keeps a raised variant.
    fn render_footer_actions(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::Div {
        let active_variant = |active: bool| {
            if active {
                ButtonVariant::Secondary
            } else {
                ButtonVariant::Ghost
            }
        };
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap_1()
            .child(
                Button::new("footer-changes")
                    .icon(IconName::GitBranch)
                    .small()
                    .with_variant(active_variant(self.main_view == MainView::Git))
                    .tooltip(text.git_title.clone())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_git_view(cx))),
            )
            .child(
                Button::new("footer-terminal")
                    .icon(IconName::SquareTerminal)
                    .small()
                    .with_variant(active_variant(self.terminal_open))
                    .tooltip(text.terminal_title.clone())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_terminal_panel(window, cx)),
                    ),
            )
            .child(self.render_settings_button(text, cx))
    }

    /// The settings entry: theme and language moved into the settings page
    /// (M8), so the footer keeps a single button.
    fn render_settings_button(&self, text: &ShellText, cx: &mut Context<Self>) -> Button {
        Button::new("sidebar-settings")
            .icon(IconName::Settings)
            .small()
            .with_variant(active_variant_settings(self.main_view))
            .tooltip(text.settings_open.clone())
            .on_click(cx.listener(|this, _, _, cx| this.open_settings_view(cx)))
    }
}

/// The settings button keeps a raised variant while its page is showing.
fn active_variant_settings(view: MainView) -> ButtonVariant {
    if view == MainView::Settings {
        ButtonVariant::Secondary
    } else {
        ButtonVariant::Ghost
    }
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

/// One transcript entry as a scroller row. Row insets and the inter-row gap
/// belong to the `MessageScroller` row wrapper; the reading column is applied
/// by the renderer closure around this item.
fn render_chat_item(entry: &ChatEntry, text: &ShellText, cx: &mut App) -> gpui_kit::AnyElement {
    match entry {
        ChatEntry::User { text: body } => div()
            .w_full()
            // A column wrapper on purpose: inside a row-flex `justify_end`
            // parent the Bubble lays out but never paints (gpui-pre 0.7.1
            // stack, reproduced 2026-10-09). `MessageAlignment::End` already
            // carries the right alignment through the bubble's own
            // `ml_auto`/`self_end`.
            .v_flex()
            .child(
                Bubble::new()
                    .alignment(MessageAlignment::End)
                    .with_variant(BubbleVariant::Muted)
                    .max_w(px(USER_BUBBLE_MAX_WIDTH))
                    .child(body.clone()),
            )
            .into_any_element(),
        ChatEntry::Assistant { text: body, .. } => div()
            .w_full()
            .min_w_0()
            .text_sm()
            .child(
                TextView::markdown(
                    SharedString::from(format!("entry-{}-md", markdown_id_key(body))),
                    body.clone(),
                )
                .selectable(true),
            )
            .into_any_element(),
        ChatEntry::Tool(card) => render_tool_card(card, text, cx).into_any_element(),
        ChatEntry::Notice { text: body } => div()
            .w_full()
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

fn render_tool_card(card: &ToolCard, text: &ShellText, cx: &App) -> gpui_kit::Div {
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
    fn render_chat_area(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
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
        let jump_label = text.chat_jump_latest.clone();
        let text = text.clone();
        MessageScroller::new(
            "transcript",
            self.chat_scroller.clone(),
            move |index, _window, cx| {
                let entry = this
                    .read(cx)
                    .chat
                    .as_ref()
                    .and_then(|chat| chat.entries.get(index))
                    .cloned();
                match entry {
                    Some(entry) => div()
                        .w_full()
                        .max_w(px(READING_COLUMN_WIDTH))
                        .mx_auto()
                        .child(render_chat_item(&entry, &text, cx))
                        .into_any_element(),
                    // Scroller item counts are synced on the same mutation
                    // that grows `entries`, so an out-of-range index cannot
                    // persist.
                    None => div().into_any_element(),
                }
            },
        )
        .with_jump_button_label(jump_label)
        .with_bottom_fade(cx.theme().background)
        .flex_1()
        .min_h_0()
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

    fn render_composer(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(chat) = self.chat.as_ref() else {
            return div().into_any_element();
        };
        let busy = chat.busy;
        let submitting = chat.submitting;
        let session_id = chat.session_id.clone();

        let draft_value = self.draft.read(cx).value().to_string();
        let draft_for_send = self.draft.clone();
        let tx_for_send = self.event_tx.clone();
        let session_for_send = session_id.clone();

        let action_button = if busy {
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
                        handle.send_message(session_for_send.clone(), body, tx_for_send.clone());
                    }
                    draft_for_send.update(cx, |state, cx| {
                        state.set_value("", window, cx);
                    });
                })
        };

        div()
            .w_full()
            .max_w(px(READING_COLUMN_WIDTH))
            .mx_auto()
            .px_4()
            .pb_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_end()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_2xl())
                    .shadow_sm()
                    .p_2()
                    .child(
                        Textarea::new(&self.draft)
                            .appearance(false)
                            .flex_1()
                            .h(px(COMPOSER_HEIGHT)),
                    )
                    .child(action_button),
            )
            .into_any_element()
    }

    /// Opens the modal approval dialog bound to the head of the pending
    /// queue. The builder re-reads the queue every frame, so answering one
    /// request reveals the next without reopening; the dialog closes on the
    /// resolving [`RuntimeEvent::PermissionResolved`] echo instead of in the
    /// button handlers, keeping the surface a pure projection of the pending
    /// set. Every dismissal path without a decision (Escape, backdrop press,
    /// close icon) is disabled: an unanswered ask parks its turn, so the only
    /// ways out are the three replies.
    fn open_permission_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let Some(view) = view.upgrade() else {
                return dialog;
            };
            let Some(item) = view.read(cx).pending_permissions.first().cloned() else {
                return dialog;
            };
            let text = i18n::current(cx);
            dialog
                .title(
                    div()
                        .text_color(cx.theme().warning)
                        .child(text.permission_title.clone()),
                )
                .overlay_closable(false)
                .keyboard(false)
                .close_button(false)
                .child(permission_body(&item, cx))
                .footer(permission_footer(&item, &text, answer_permission))
        });
        self.permission_dialog_open = true;
    }
}

/// Forwards one user reply to the embedded runtime. The queue and dialog
/// update on the broadcast echo, not here, so a failed answer leaves the
/// prompt standing (the request is still pending).
fn answer_permission(request_id: String, reply: PermissionReply) {
    if let Some(handle) = runtime::runtime_handle() {
        handle.respond_permission(request_id, reply);
    }
}

/// Composes the display lines for one arriving request. Everything the dialog
/// shows is resolved here against the active locale, mirroring the CLI prompt
/// content (`src/apps/cli/src/ui/permission.rs`); the render path stays free
/// of i18n service calls.
fn compose_permission_item(
    request: PermissionRequest,
    text: &ShellText,
    cx: &App,
) -> PermissionPromptItem {
    let resources_line = if request.resources.is_empty() {
        None
    } else {
        let resources = request
            .resources
            .iter()
            .map(|resource| runtime::truncate_chars(resource, 80))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("{}: {resources}", text.permission_resources))
    };
    let project = request
        .project_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .unwrap_or(&request.project_id);
    let project_line = format!("{}: {}", text.permission_project, project);
    let delegation_line = request.delegation.as_ref().map(|delegation| {
        format!(
            "{}: {} · {}",
            i18n::permission_subagent_label(&delegation.subagent_type, cx),
            delegation.subagent_type,
            request.session_id
        )
    });
    let risk_line = request
        .display_metadata
        .get("riskDescription")
        .or_else(|| request.display_metadata.get("risk"))
        .and_then(serde_json::Value::as_str)
        .filter(|risk| !risk.trim().is_empty())
        .map(|risk| {
            format!(
                "{}: {}",
                text.permission_risk,
                runtime::truncate_chars(risk, 100)
            )
        });
    PermissionPromptItem {
        resources_line,
        project_line,
        delegation_line,
        risk_line,
        request,
    }
}

/// Dialog body: the action as the headline, then the composed context lines.
fn permission_body(item: &PermissionPromptItem, cx: &App) -> gpui_kit::Div {
    let mut body = div()
        .v_flex()
        .gap_1p5()
        .text_sm()
        .child(
            div()
                .text_color(cx.theme().foreground)
                .truncate()
                .child(item.request.action.clone()),
        )
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(item.project_line.clone()),
        );
    if let Some(line) = &item.resources_line {
        body = body.child(
            div()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(line.clone()),
        );
    }
    if let Some(line) = &item.delegation_line {
        body = body.child(
            div()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(line.clone()),
        );
    }
    if let Some(line) = &item.risk_line {
        body = body.child(
            div()
                .text_color(cx.theme().warning)
                .truncate()
                .child(line.clone()),
        );
    }
    body
}

/// Dialog footer: Deny / Always allow / Allow once. The reply callback is
/// injected so tests can observe the decisions without a runtime.
fn permission_footer(
    item: &PermissionPromptItem,
    text: &ShellText,
    on_reply: impl Fn(String, PermissionReply) + Clone + 'static,
) -> gpui_kit::Div {
    let deny_id = item.request_id().to_string();
    let always_id = item.request_id().to_string();
    let once_id = item.request_id().to_string();
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            Button::new("permission-deny")
                .label(text.permission_deny.clone())
                .with_variant(ButtonVariant::Danger)
                .on_click({
                    let on_reply = on_reply.clone();
                    move |_, _, _| {
                        on_reply(deny_id.clone(), PermissionReply::Reject { feedback: None });
                    }
                }),
        )
        .child(
            Button::new("permission-always")
                .label(text.permission_allow_always.clone())
                .on_click({
                    let on_reply = on_reply.clone();
                    move |_, _, _| {
                        on_reply(always_id.clone(), PermissionReply::Always);
                    }
                }),
        )
        .child(
            Button::new("permission-once")
                .label(text.permission_allow_once.clone())
                .with_variant(ButtonVariant::Primary)
                .on_click(move |_, _, _| {
                    on_reply(once_id.clone(), PermissionReply::Once);
                }),
        )
}

impl Render for ShellWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text: ShellText = i18n::current(cx);
        // Automation hooks: open one surface once the runtime is ready, so a
        // screenshot run can inspect it without synthesizing input events.
        if matches!(self.phase, RuntimePhase::Ready) {
            if self.terminal_auto_open {
                self.terminal_auto_open = false;
                self.open_terminal_panel(window, cx);
            }
            if self.settings_auto_open {
                self.settings_auto_open = false;
                self.open_settings_view(cx);
            }
            if self.git_auto_open {
                self.git_auto_open = false;
                if self.main_view != MainView::Git {
                    self.toggle_git_view(cx);
                }
            }
        }

        let main_area = match self.main_view {
            MainView::Settings => settings::render_page(&text, cx),
            MainView::Git => match self.git.clone() {
                Some(panel) => div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(panel)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            MainView::Chat => self.render_chat_column(&text, cx),
        };

        div()
            .size_full()
            .v_flex()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap_2()
                                .child(div().text_sm().child("OpenBitFun GPUI Shell"))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(text.subtitle.clone()),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .min_w_0()
                    .child(self.render_sidebar(&text, cx))
                    .child(div().flex_1().min_h_0().min_w_0().flex().child(main_area)),
            )
            .when(self.terminal_open, |this| {
                this.child(self.render_terminal_panel(&text, cx))
            })
    }
}

impl ShellWindow {
    /// The docked terminal panel: slim header (OSC title + close) above the
    /// pane. The pane owns focus, keyboard, wheel scrollback, and reflows to
    /// whatever size the panel gives it.
    fn render_terminal_panel(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let title = self
            .terminal
            .as_ref()
            .map(|pane| {
                pane.read(cx)
                    .title_label(text.terminal_title.clone().into())
            })
            .unwrap_or_else(|| text.terminal_title.clone().into());
        div()
            .v_flex()
            .h(TerminalPane::panel_height())
            .min_h_0()
            .border_t_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        Button::new("terminal-close")
                            .icon(IconName::X)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.terminal_close.clone())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_terminal_panel(window, cx)
                            })),
                    ),
            )
            .child(match self.terminal.clone() {
                Some(pane) => div().flex_1().min_h_0().child(pane).into_any_element(),
                None => div().flex_1().min_h_0().into_any_element(),
            })
            .into_any_element()
    }

    /// The chat surface: session header, transcript, busy marker, composer.
    fn render_chat_column(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let chat_area = self.render_chat_area(text, cx);
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .v_flex()
            .child(self.render_chat_header(text, cx))
            .when_some(self.chat_error.clone(), |this, error| {
                this.child(
                    div()
                        .mx_4()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(chat_area)
            .when_some(self.busy_label(text), |this, label| {
                this.child(
                    div()
                        .w_full()
                        .max_w(px(READING_COLUMN_WIDTH))
                        .mx_auto()
                        .px_4()
                        .pb_1()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                )
            })
            .child(self.render_composer(text, cx))
            .into_any_element()
    }

    fn render_chat_header(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::Div {
        match self.chat.as_ref() {
            Some(chat) => div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().title_bar_border)
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

/// Installs the tracing subscriber honoring `RUST_LOG`. Called once from the
/// binary entry before any logging happens.
pub fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stdout)
        .try_init();
}

/// Bootstraps theme/i18n/runtime assembly and opens the main window. Called
/// from the binary entry after the process-lifetime tokio runtime exists.
pub fn launch_main_window(tokio_handle: tokio::runtime::Handle) {
    let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
    // The view keeps its own sender for the session reload action.
    let view_event_tx = event_tx.clone();

    gpui_kit::application()
        .with_assets(ShellAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);

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

            // Mirror the shell defaults until the bootstrap snapshot arrives;
            // the settings page renders against this global from the first
            // frame.
            let locale = i18n::current(cx).locale;
            cx.set_global(settings::ShellConfig::startup_default(locale));

            runtime::spawn_bootstrap(tokio_handle, event_tx);

            let open_result = gpui_kit::open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some(SharedString::from("OpenBitFun GPUI Shell")),
                        ..TitleBar::title_bar_options()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(80.), px(80.)),
                        size: size(px(1180.), px(760.)),
                    })),
                    window_min_size: Some(size(px(960.), px(640.))),
                    ..TitleBar::window_options()
                },
                cx,
                |window, cx| cx.new(|cx| ShellWindow::new(view_event_tx, event_rx, window, cx)),
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
        assert_eq!(tail_changed_range(1), 0..1);
        assert_eq!(tail_changed_range(40), 39..40);
    }

    /// A harness view mirroring `ShellWindow::render_chat_area` so the test
    /// exercises the same scroller construction and item renderer.
    struct TranscriptListHarness {
        scroller: Entity<MessageScrollerState>,
        entries: Rc<Vec<ChatEntry>>,
        render_count: Rc<Cell<usize>>,
        text: ShellText,
    }

    impl Render for TranscriptListHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let this = cx.entity();
            let render_count = self.render_count.clone();
            let text = self.text.clone();
            MessageScroller::new(
                "transcript",
                self.scroller.clone(),
                move |index, _window, cx| {
                    render_count.set(render_count.get() + 1);
                    match this.read(cx).entries.get(index).cloned() {
                        Some(entry) => render_chat_item(&entry, &text, cx),
                        None => div().into_any_element(),
                    }
                },
            )
            .w(px(800.))
            .h(px(600.))
        }
    }

    fn sample_shell_text() -> ShellText {
        crate::i18n::fallback_text(LocaleId::EnUS)
    }

    fn permission_item(request_id: &str) -> PermissionPromptItem {
        PermissionPromptItem {
            request: crate::runtime::test_permission_request(request_id),
            resources_line: Some("Resources: resource-a".into()),
            project_line: "Project: project".into(),
            delegation_line: None,
            risk_line: None,
        }
    }

    #[test]
    fn permission_queue_resolution_drains_by_request_id() {
        let mut queue = vec![permission_item("req-a"), permission_item("req-b")];
        queue.retain(|item| item.request_id() != "req-a");
        assert_eq!(queue.len(), 1);
        queue.retain(|item| item.request_id() != "req-b");
        assert!(queue.is_empty());
    }

    /// Composition happens at arrival; without the i18n service initialized
    /// the interpolating subagent label falls back to the raw key, which keeps
    /// the assertion deterministic.
    #[gpui_kit::test]
    fn permission_item_composes_context_lines(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            let text = sample_shell_text();
            let mut request = crate::runtime::test_permission_request("req-1");
            request.project_path = Some("/tmp/ws".into());
            let item = compose_permission_item(request, &text, cx);
            assert_eq!(item.project_line, "Project: /tmp/ws");
            assert_eq!(
                item.resources_line.as_deref(),
                Some("Resources: resource-a")
            );
            assert!(item.delegation_line.is_none());
            assert!(item.risk_line.is_none());

            let mut delegated = crate::runtime::test_permission_request("req-2");
            delegated.display_metadata = Default::default();
            let delegated_item = compose_permission_item(delegated, &text, cx);
            assert_eq!(delegated_item.project_line, "Project: project");
        });
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

    #[gpui_kit::test]
    fn transcript_list_renders_visible_range_only(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| gpui_kit::init(cx));

        const ROUNDS: usize = 500;
        let entries = Rc::new(stress_entries(ROUNDS));
        let render_count: Rc<Cell<usize>> = Default::default();
        let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));

        let window = cx.add_empty_window();
        let draw_frame = |window: &mut gpui_kit::VisualTestContext,
                          scroller: &Entity<MessageScrollerState>,
                          entries: &Rc<Vec<ChatEntry>>,
                          render_count: &Rc<Cell<usize>>|
         -> std::time::Duration {
            render_count.set(0);
            let scroller = scroller.clone();
            let entries = entries.clone();
            let render_count = render_count.clone();
            let text = sample_shell_text();
            let started = std::time::Instant::now();
            window.draw(
                point(px(0.), px(0.)),
                size(px(800.), px(600.)),
                move |_, cx| {
                    cx.new(|_| TranscriptListHarness {
                        scroller,
                        entries,
                        render_count,
                        text,
                    })
                    .into_any_element()
                },
            );
            started.elapsed()
        };

        // Transcript restore: the first frame must neither render nor measure
        // all 1k entries (markdown parse only happens for rendered items).
        draw_frame(window, &scroller, &entries, &render_count);
        let restored = render_count.get();
        assert!(
            restored < 100,
            "restore rendered {restored} of {} entries in one frame",
            entries.len()
        );

        // Sweep the whole list top to bottom by jumping row anchors; every
        // frame stays bounded while the scroller re-measures the landed range.
        let mut max_per_frame = 0usize;
        let mut total = std::time::Duration::ZERO;
        let steps = 60;
        let total_entries = entries.len();
        for step in 0..steps {
            let anchor = step * (total_entries.saturating_sub(1)) / steps;
            window.update(|_, cx| {
                scroller.update(cx, |scroller, cx| {
                    scroller.scroll_to_item(anchor, cx);
                });
            });
            window.run_until_parked();
            let elapsed = draw_frame(window, &scroller, &entries, &render_count);
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
            window.update(|_, cx| {
                scroller.update(cx, |scroller, cx| {
                    scroller.append(1, cx);
                });
            });
            grown.push(ChatEntry::Assistant {
                round_id: format!("live-{round}"),
                text: "streamed **markdown** body.\n\n- item\n- item".to_string(),
            });
            let count = grown.len();
            window.update(|_, cx| {
                scroller.update(cx, |scroller, cx| {
                    scroller.append(1, cx);
                    scroller.remeasure_items(tail_changed_range(count), cx);
                });
            });
        }
        window.update(|_, cx| {
            scroller.update(cx, |scroller, _| {
                assert_eq!(scroller.item_count(), grown.len());
            });
        });
        let entries = Rc::new(grown);
        draw_frame(window, &scroller, &entries, &render_count);
        assert!(
            render_count.get() < 100,
            "post-growth frame rendered {} entries",
            render_count.get()
        );
    }

    /// The settings page must build against the shell globals (theme, shell
    /// config mirror, locale copy) without a live runtime.
    #[gpui_kit::test]
    fn settings_page_renders_headlessly(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let presets = theme::load_presets();
            let active = presets
                .iter()
                .position(|preset| preset.id == DEFAULT_PRESET_ID)
                .unwrap_or(0);
            if let Some(preset) = presets.get(active) {
                preset.apply(cx);
            }
            cx.set_global(ShellState {
                presets,
                active_preset: active,
            });
            cx.set_global(settings::ShellConfig::startup_default(
                openbitfun_core::service::i18n::LocaleId::ZhCN,
            ));
        });
        let window = cx.add_empty_window();
        let text = sample_shell_text();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                // The page must render inside a view: the component library
                // asks the window for the current view id during layout.
                cx.new(|_| SettingsHarness { text }).into_any_element()
            },
        );
        window.run_until_parked();
    }

    struct SettingsHarness {
        text: ShellText,
    }

    impl Render for SettingsHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            settings::render_page(&self.text, cx)
        }
    }

    /// The changes view must render its failure state (no runtime attached in
    /// tests) instead of panicking on the missing git port.
    #[gpui_kit::test]
    fn git_panel_renders_without_a_runtime(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(sample_shell_text());
        });
        let panel = cx.new(GitPanel::new);
        let window = cx.add_empty_window();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, _| panel.into_any_element(),
        );
        window.run_until_parked();
    }
}
