use crate::{i18n, runtime, settings, theme};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::resizable::{resizable_panel, v_resizable, ResizableState};
use gpui_kit::component::{
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariant, ButtonVariants},
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    message::MessageAlignment,
    message_scroller::{MessageScroller, MessageScrollerState},
    scroll::ScrollableElement,
    shimmer::ShimmerText,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarItem},
    text::TextView,
    ActiveTheme, Collapsible, Disableable, Icon, Sizable, StyledExt, TitleBar, WindowExt,
};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{
    div, img, point, prelude::*, px, size, Anchor, App, Bounds, ClipboardEntry, Context, ElementId,
    Entity, ExternalPaths, Image, ImageFormat, Pixels, SharedString, SystemNotification,
    TitlebarOptions, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions,
};
use openbitfun_agent_runtime::sdk::{PendingUserQuestion, PermissionReply, PermissionRequest};
use openbitfun_runtime_ports::AgentInputAttachment;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::assets::ShellAssets;
use crate::files::FilesPage;
use crate::git::GitPanel;
use crate::i18n::ShellText;
use crate::runtime::{ChatEntry, RuntimeEvent, ToolCard, ToolStatus, WorkspaceChoice};
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
/// Bounds of the docked terminal panel, dragged by the divider above it.
const MIN_TERMINAL_HEIGHT: f32 = 120.;
const MAX_TERMINAL_HEIGHT: f32 = 900.;

/// Which surface the main content area shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainView {
    Chat,
    /// The settings page (component-library `Settings` container).
    Settings,
    /// Workspace changes (git status + self-drawn diff).
    Git,
    /// Usage insights over recent sessions (M14).
    Insights,
    /// Scheduled jobs management (M14).
    Tasks,
    /// Workspace file tree + viewer (M15).
    Files,
    /// Global search: file content + sessions (M16).
    Search,
}

/// One terminal panel tab (M17): a pane owning its PTY plus the fallback
/// tab label (a profile's name, or the localized default-shell label; the
/// pane's OSC title wins once the shell sets one).
#[derive(Clone)]
struct TerminalTab {
    pane: Entity<TerminalPane>,
    label: SharedString,
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
pub(crate) struct SessionRow {
    pub(crate) id: String,
    pub(crate) name: String,
    metadata: String,
    /// Persisted archive state (from the session metadata store, not a local
    /// flag): archived rows are hidden until the "show archived" toggle is on.
    pub(crate) archived: bool,
}

/// One open session in the chat pane.
struct ChatView {
    session_id: String,
    name: String,
    agent_type: String,
    /// The session's current model selector; `None` means the runtime default.
    model_id: Option<String>,
    entries: Vec<ChatEntry>,
    /// A turn is executing (streaming or tool calls in flight).
    busy: bool,
    /// A submission is in flight; the optimistic user entry awaits
    /// [`RuntimeEvent::TurnAccepted`].
    submitting: bool,
    /// The pane shows a hidden child session (subagent run or `/btw` side
    /// question, M20): the header grows its badge and the "back to parent"
    /// entry point. `None` = a normal, user-visible session.
    child_context: Option<ChildContext>,
}

/// Why the chat pane is showing a hidden child session (M20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildKind {
    /// A subagent run opened from its parent tool card.
    Subagent,
    /// A `/btw` side-question session.
    Btw,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildContext {
    pub kind: ChildKind,
    pub parent_session_id: String,
    /// Subagent type of a linked run (badge detail); `None` for `/btw`.
    pub agent_type: Option<String>,
}

/// One hidden subagent session linked to its parent tool call (M20). Kept on
/// the window so the mapping survives transcript reloads; tool cards are
/// re-annotated from here whenever a transcript (re)loads.
#[derive(Debug, Clone)]
pub(crate) struct SubagentLink {
    pub parent_session_id: String,
    pub tool_call_id: String,
    pub subagent_session_id: String,
    pub agent_type: Option<String>,
}

/// A child session the user asked to open whose transcript is still loading
/// (`RuntimeEvent::SessionOpened` settles it into the chat's child context).
#[derive(Debug, Clone)]
struct PendingChild {
    child_session_id: String,
    context: ChildContext,
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

/// Which shell-owned modal currently owns the single dialog slot. The
/// component library's dialog stack closes strictly by LIFO and carries no
/// dialog id, so the shell keeps at most one modal open and hands the slot
/// over in priority order; two open modals would make a `close_dialog` meant
/// for one queue pop the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalOwner {
    Permissions,
    Questions,
}

/// One pending `ask_user` batch, pre-parsed for rendering. Selection state
/// lives here rather than in the dialog builder, so a queue rebuild keeps the
/// user's in-progress answers when another question joins or leaves.
#[derive(Debug, Clone)]
struct QuestionPromptItem {
    tool_id: String,
    session_id: String,
    /// Questions of this batch; empty when the payload could not be parsed
    /// (the dialog then offers Cancel only).
    questions: Vec<PromptQuestion>,
    /// Selected option indices per question; single-select holds at most one.
    selected: Vec<BTreeSet<usize>>,
}

/// One question of an `ask_user` batch as rendered by the dialog.
#[derive(Debug, Clone)]
struct PromptQuestion {
    header: String,
    question: String,
    /// "Question n of m", resolved in the active locale when the item was
    /// composed; the render path never calls the i18n service.
    progress_line: String,
    options: Vec<PromptOption>,
    multi_select: bool,
}

/// One selectable option of a question.
#[derive(Debug, Clone)]
struct PromptOption {
    label: String,
    description: String,
}

impl QuestionPromptItem {
    /// True once every question has a selection; a malformed batch is never
    /// answerable and only offers Cancel.
    fn is_answered(&self) -> bool {
        !self.questions.is_empty() && self.selected.iter().all(|set| !set.is_empty())
    }

    /// Answers in the wire shape the tool expects: question index as key,
    /// option labels as the value, an array for multi-select questions
    /// (`{"0": "A", "1": ["B", "C"]}` — same as the desktop card).
    fn answers(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (index, question) in self.questions.iter().enumerate() {
            let selected = question
                .options
                .iter()
                .enumerate()
                .filter(|(option_index, _)| {
                    self.selected
                        .get(index)
                        .is_some_and(|set| set.contains(option_index))
                })
                .map(|(_, option)| option.label.clone())
                .collect::<Vec<_>>();
            if selected.is_empty() {
                continue;
            }
            if question.multi_select {
                map.insert(index.to_string(), serde_json::Value::from(selected));
            } else if let Some(label) = selected.first() {
                map.insert(index.to_string(), serde_json::Value::from(label.clone()));
            }
        }
        serde_json::Value::Object(map)
    }
}

/// Parses one mailbox payload into render-ready questions. The payload is the
/// tool input plus timing metadata; an unparsable or option-less batch yields
/// `None` so the dialog stays visible with Cancel instead of hiding a live
/// ask. `question_label` localizes the "n of m" heading at composition time,
/// keeping the i18n service off the render path.
fn parse_question_payload(
    payload: &serde_json::Value,
    question_label: impl Fn(usize, usize) -> String,
) -> Option<Vec<PromptQuestion>> {
    let input: openbitfun_agent_runtime::user_questions::AskUserQuestionInput =
        serde_json::from_value(payload.clone()).ok()?;
    let total = input.questions.len();
    let questions = input
        .questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| PromptQuestion {
            header: question.header,
            question: question.question,
            progress_line: question_label(index + 1, total),
            options: question
                .options
                .into_iter()
                .map(|option| PromptOption {
                    label: option.label,
                    description: option.description,
                })
                .collect(),
            multi_select: question.multi_select,
        })
        .collect::<Vec<_>>();
    if questions.is_empty() || questions.iter().any(|question| question.options.is_empty()) {
        return None;
    }
    Some(questions)
}

/// Builds one render-ready item from a mailbox question.
fn compose_question_item(
    question: PendingUserQuestion,
    question_label: impl Fn(usize, usize) -> String,
) -> QuestionPromptItem {
    let questions = parse_question_payload(&question.questions, question_label);
    let selected = vec![BTreeSet::new(); questions.as_ref().map_or(0, Vec::len)];
    QuestionPromptItem {
        tool_id: question.tool_id,
        session_id: question.session_id,
        questions: questions.unwrap_or_default(),
        selected,
    }
}

/// Rebuilds the question queue from a mailbox snapshot. Existing items are
/// carried over whole so the user's in-progress selections survive an
/// unrelated question arriving or leaving; only new tool ids are parsed.
fn rebuild_question_queue(
    existing: &[QuestionPromptItem],
    incoming: Vec<PendingUserQuestion>,
    question_label: impl Fn(usize, usize) -> String,
) -> Vec<QuestionPromptItem> {
    incoming
        .into_iter()
        .map(|question| {
            match existing
                .iter()
                .find(|item| item.tool_id == question.tool_id)
            {
                Some(item) => item.clone(),
                None => compose_question_item(question, &question_label),
            }
        })
        .collect()
}

/// Which modal should own the dialog slot next; `None` when both queues are
/// empty. Approvals take the slot first: they gate a tool call that is
/// otherwise parked, while an attended question waits without a deadline.
fn next_modal_target(pending_permissions: usize, pending_questions: usize) -> Option<ModalOwner> {
    if pending_permissions > 0 {
        Some(ModalOwner::Permissions)
    } else if pending_questions > 0 {
        Some(ModalOwner::Questions)
    } else {
        None
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

/// Sidebar navigation destinations (M23): one row per main surface, mirroring
/// the web-ui NavPanel order (nav sections first, sessions below).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavAction {
    Chat,
    Git,
    Insights,
    Tasks,
    Files,
    Search,
}

impl NavAction {
    fn icon(self) -> IconName {
        match self {
            NavAction::Chat => IconName::Sparkles,
            NavAction::Git => IconName::GitBranch,
            NavAction::Insights => IconName::ChartPie,
            NavAction::Tasks => IconName::Calendar,
            NavAction::Files => IconName::FolderOpen,
            NavAction::Search => IconName::Search,
        }
    }

    /// Whether the row is the active surface.
    fn is_active(self, view: MainView) -> bool {
        match self {
            NavAction::Chat => view == MainView::Chat,
            NavAction::Git => view == MainView::Git,
            NavAction::Insights => view == MainView::Insights,
            NavAction::Tasks => view == MainView::Tasks,
            NavAction::Files => view == MainView::Files,
            NavAction::Search => view == MainView::Search,
        }
    }
}

/// Branded empty state (M23): the component `Empty` layout with an icon in a
/// muted frame over a title and description. Shared by the chat, files, and
/// search surfaces so the treatment stays identical; theme resolution happens
/// at render time inside the component.
pub(crate) fn brand_empty(icon: IconName, title: &str, description: &str) -> gpui_kit::AnyElement {
    Empty::new()
        .header(
            EmptyHeader::new()
                .media(
                    EmptyMedia::new()
                        .with_variant(EmptyMediaVariant::Icon)
                        .child(Icon::new(icon)),
                )
                .title(EmptyTitle::new().child(title.to_string()))
                .description(EmptyDescription::new().child(description.to_string())),
        )
        .into_any_element()
}

/// All nav rows in display order; a pure projection so tests can pin the
/// active-state wiring without a window.
fn nav_actions() -> Vec<NavAction> {
    vec![
        NavAction::Chat,
        NavAction::Files,
        NavAction::Git,
        NavAction::Search,
        NavAction::Insights,
        NavAction::Tasks,
    ]
}

/// Row label for a nav destination; the surface titles double as nav nouns.
fn nav_label<'a>(action: NavAction, text: &'a ShellText) -> &'a str {
    match action {
        NavAction::Chat => &text.chat_title,
        NavAction::Git => &text.git_title,
        NavAction::Insights => &text.insights_title,
        NavAction::Tasks => &text.tasks_title,
        NavAction::Files => &text.files_title,
        NavAction::Search => &text.search_title,
    }
}

/// One `Sidebar` content entry: a session row, a navigation row, the
/// empty-state line, or the list error line. One type because `Sidebar`
/// children share one element type; collapsed-state is inert because entries
/// are always rendered.
#[derive(Clone)]
enum SidebarEntry {
    Session {
        row: SessionRow,
        open: bool,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
        /// The shell view, so the row menu can drive dialogs and actions
        /// (row rendering runs outside the view's own context).
        view: WeakEntity<ShellWindow>,
    },
    Nav {
        action: NavAction,
        label: SharedString,
        active: bool,
        view: WeakEntity<ShellWindow>,
    },
    Empty(SharedString),
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
            SidebarEntry::Session {
                row,
                open,
                tx,
                view,
            } => {
                let SessionRow {
                    id,
                    name,
                    metadata,
                    archived,
                } = row;
                let open_id = id.clone();
                let menu = session_row_menu(
                    id.clone(),
                    name.clone(),
                    archived,
                    open,
                    view.clone(),
                    tx.clone(),
                );
                div()
                    .id(SharedString::from(format!("session-{id}")))
                    .on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.open_session(open_id.clone(), tx.clone());
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .when(open, |this| this.bg(cx.theme().sidebar_accent))
                    .when(!open, |this| {
                        this.hover(|style| style.bg(cx.theme().sidebar_accent))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .v_flex()
                            .gap_0p5()
                            .when(archived, |this| this.opacity(0.6))
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
                            ),
                    )
                    .child(menu)
                    .into_any_element()
            }
            SidebarEntry::Nav {
                action,
                label,
                active,
                view,
            } => {
                let id = SharedString::from(format!("nav-{:?}", action));
                div()
                    .id(id)
                    .on_click(move |_, window, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| this.apply_nav(action, window, cx));
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_x_2()
                    .min_w_0()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .when(active, |this| {
                        this.font_medium()
                            .bg(cx.theme().sidebar_accent)
                            .text_color(cx.theme().sidebar_accent_foreground)
                    })
                    .when(!active, |this| {
                        this.hover(|style| style.bg(cx.theme().sidebar_accent.opacity(0.8)))
                    })
                    .child(Icon::new(action.icon()))
                    .child(div().truncate().child(label))
                    .into_any_element()
            }
            SidebarEntry::Empty(text) => div()
                .flex()
                .flex_row()
                .items_start()
                .gap_x_2()
                .p_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(Icon::new(IconName::Inbox))
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

/// Per-row actions menu: rename, fork, compact (open session only), export,
/// archive toggle, delete. A Button (whose press stops propagation) keeps the
/// row's own click-to-open handler from firing when the menu trigger is used.
fn session_row_menu(
    session_id: String,
    name: String,
    archived: bool,
    open: bool,
    view: WeakEntity<ShellWindow>,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) -> gpui_kit::AnyElement {
    let rename_action = {
        let view = view.clone();
        let session_id = session_id.clone();
        let name = name.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            open_rename_dialog(view.clone(), session_id.clone(), name.clone(), window, cx);
        }
    };
    let fork_action = {
        let session_id = session_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.fork_session(session_id.clone(), tx.clone());
            }
        }
    };
    let compact_action = {
        let session_id = session_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.start_compaction(session_id.clone(), tx.clone());
            }
        }
    };
    let export_action = {
        let view = view.clone();
        let session_id = session_id.clone();
        let name = name.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            open_export_dialog(view.clone(), session_id.clone(), name.clone(), window, cx);
        }
    };
    let archive_action = {
        let session_id = session_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.set_session_archived(session_id.clone(), !archived, tx.clone());
            }
        }
    };
    let delete_action = {
        let view = view.clone();
        let session_id = session_id.clone();
        let name = name.clone();
        move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
            open_delete_dialog(view.clone(), session_id.clone(), name.clone(), window, cx);
        }
    };
    Button::new(SharedString::from(format!("session-menu-{session_id}")))
        .icon(IconName::Ellipsis)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
            let text = i18n::current(cx);
            let archive_label = if archived {
                text.session_unarchive.clone()
            } else {
                text.session_archive.clone()
            };
            let menu = menu
                .item(
                    PopupMenuItem::new(text.session_rename.clone()).on_click(rename_action.clone()),
                )
                .item(PopupMenuItem::new(text.session_fork.clone()).on_click(fork_action.clone()));
            let menu = if open {
                menu.item(
                    PopupMenuItem::new(text.session_compact.clone())
                        .on_click(compact_action.clone()),
                )
            } else {
                menu
            };
            menu.item(
                PopupMenuItem::new(text.session_export.clone()).on_click(export_action.clone()),
            )
            .item(PopupMenuItem::new(archive_label).on_click(archive_action.clone()))
            .item(PopupMenuItem::new(text.session_delete.clone()).on_click(delete_action.clone()))
        })
        .into_any_element()
}

/// Image formats the turn pipeline can decode reliably; anything else a
/// clipboard or drag offers is refused with a visible notice instead of
/// failing the turn later.
const SUPPORTED_IMAGE_MIMES: [&str; 5] = [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
];

/// One image waiting to ride along with the next composer send. The bytes are
/// always held shell-side: the turn request carries a `data:` URL, so the
/// image survives any runtime topology (peer/remote runtimes never see a
/// controller-local path).
struct PendingAttachment {
    id: String,
    name: String,
    mime: String,
    data_url: String,
    /// Decoded bytes for the chip thumbnail.
    preview: Arc<Image>,
}

impl PendingAttachment {
    fn to_input_attachment(&self) -> AgentInputAttachment {
        AgentInputAttachment::image_context(
            self.id.clone(),
            None,
            Some(self.data_url.clone()),
            self.mime.clone(),
            Some(serde_json::json!({ "name": self.name })),
        )
    }
}

/// `image/png` for `.png`, etc.; `None` for anything outside the supported
/// set (checked against [`SUPPORTED_IMAGE_MIMES`]).
fn image_format_for_extension(extension: &str) -> Option<ImageFormat> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(ImageFormat::Png),
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        "bmp" => Some(ImageFormat::Bmp),
        _ => None,
    }
}

pub(crate) struct ShellWindow {
    draft: Entity<TextareaState>,
    phase: RuntimePhase,
    pub(crate) sessions: Vec<SessionRow>,
    list_error: Option<String>,
    chat: Option<ChatView>,
    chat_error: Option<String>,
    /// Virtualized transcript state owned by the component library's
    /// `MessageScroller`; item counts are kept in sync with `chat.entries`
    /// through the `list_sync_*` helpers and tail-following keeps the newest
    /// content in view until the user scrolls up.
    chat_scroller: Entity<MessageScrollerState>,
    pub(crate) event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    /// Approvals waiting for a user decision, oldest first. Every ask in this
    /// process surfaces here regardless of which session is open — an
    /// unanswered ask parks its turn at the tool pipeline until cancellation,
    /// so hiding one would be exactly the deadlock this shell must not
    /// introduce.
    pending_permissions: Vec<PermissionPromptItem>,
    /// `ask_user` questions waiting for an answer, oldest first. Same
    /// no-session-filter rule as approvals: a hidden question is a parked
    /// turn.
    pending_questions: Vec<QuestionPromptItem>,
    /// Which shell-owned modal owns the single dialog slot; `None` while no
    /// modal is open.
    modal_owner: Option<ModalOwner>,
    /// Built-in terminal panel (M7): created lazily on first open, shut down
    /// and dropped when the panel closes.
    terminal_open: bool,
    /// Terminal panel tabs (M17): one PTY pane per tab plus its fallback
    /// label (profile name or the localized default; the pane's OSC title
    /// wins at render time). Closing a tab tears its PTY down.
    terminals: Vec<TerminalTab>,
    active_terminal: usize,
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
    /// Workspace files page (M15): created on first open and kept for the
    /// window's lifetime (tabs survive view switches), dropped on workspace
    /// switches.
    files: Option<Entity<FilesPage>>,
    /// Global search page state (M16): results cross the event pump, jumps
    /// reach the shell through the weak handle the render path carries.
    /// The search page module mutates `tab` directly (same-crate render).
    pub(crate) search: crate::search::SearchPageState,
    /// Search query input; lives on the window like the other dialogs'
    /// inputs so its text survives view switches.
    search_input: Entity<InputState>,
    /// Split state of the docked terminal panel: the panel keeps whatever
    /// height the divider was dragged to for as long as the window lives.
    dock: Entity<ResizableState>,
    /// Height the terminal dock reopens at (the last dragged height, or the
    /// pane default before any drag).
    terminal_height: Pixels,
    /// One-shot automation hooks for the settings page and the changes view,
    /// mirroring `OPENBITFUN_GPUI_TERMINAL_AUTOOPEN`.
    settings_auto_open: bool,
    git_auto_open: bool,
    /// Sidebar filter: archived sessions stay hidden until this is on.
    show_archived: bool,
    /// Rename dialog input, reused across opens (the dialog builder re-reads
    /// it every frame, so the entity is created once with the window).
    rename_input: Entity<InputState>,
    /// Session id the rename dialog is editing; `None` while the dialog is
    /// closed.
    rename_target: Option<String>,
    /// Export dialog input (destination file path), reused across opens.
    export_input: Entity<InputState>,
    /// Session id the export dialog is writing; `None` while closed.
    export_target: Option<String>,
    /// Terminal-profile editor inputs (M17), dialog-owned like the rename
    /// inputs; `pub(crate)` because the settings page fills and reads them.
    pub(crate) profile_name_input: Entity<InputState>,
    pub(crate) profile_dir_input: Entity<InputState>,
    pub(crate) profile_command_input: Entity<InputState>,
    /// Profile id being edited; `None` while the dialog edits a new profile
    /// or is closed.
    pub(crate) profile_edit_id: Option<String>,
    /// Latest session usage report, shown in the usage dialog.
    usage_report: Option<Box<runtime::SessionUsageSummary>>,
    /// Insights page state (M14): latest reports + the open report.
    insights: crate::insights::InsightsPageState,
    /// Scheduled-jobs page state (M14).
    tasks: crate::tasks::TasksPageState,
    /// Workspaces the switcher offers: the active one first, then this
    /// process's visits, then the shared core catalog.
    workspaces: Vec<WorkspaceChoice>,
    /// Path input of the "switch workspace" dialog, reused across opens.
    workspace_input: Entity<InputState>,
    /// Last workspace-switch failure, shown under the sidebar status; cleared
    /// by a successful switch.
    workspace_error: Option<String>,
    /// Images staged for the next send (paste or drag-and-drop). Cleared by
    /// the `TurnAccepted` echo, not optimistically, so a failed submission
    /// keeps the images for a retry.
    pending_attachments: Vec<PendingAttachment>,
    /// Hidden subagent sessions linked to parent tool calls (M20), newest
    /// last. Tool cards are annotated from this map on every transcript load.
    subagent_links: Vec<SubagentLink>,
    /// A child session whose transcript is still loading after the user asked
    /// to see it; `SessionOpened` settles it into the chat's child context.
    pending_child: Option<PendingChild>,
    /// M21 management state: dialog input states (dialog builders run every
    /// frame), agent-creation scratch, the pending MCP interaction queue, and
    /// the browse/delete dialog scratch.
    pub(crate) agent_id_input: Entity<InputState>,
    pub(crate) agent_name_input: Entity<InputState>,
    pub(crate) agent_description_input: Entity<InputState>,
    pub(crate) agent_prompt_input: Entity<TextareaState>,
    pub(crate) agent_create_active: bool,
    pub(crate) agent_create_kind_mode: bool,
    pub(crate) agent_create_project_level: bool,
    pub(crate) agent_create_error: Option<String>,
    pub(crate) mcp_pending_delete: Option<String>,
    pub(crate) mcp_browse: Option<crate::manage::McpServerCatalogView>,
    pub(crate) mcp_browse_loading: Option<(String, String)>,
    pub(crate) mcp_interactions: crate::manage::McpInteractionQueue,
    pub(crate) mcp_interaction_result_input: Entity<InputState>,
    pub(crate) mcp_interaction_error: Option<String>,
    /// M22 self-update state machine; the general settings page projects it.
    pub(crate) update_state: crate::update::UpdateUiState,
    /// M22 archived-sessions delete confirmation target (dialog input lives
    /// on the window like every other dialog scratch value).
    pub(crate) archived_delete_target: Option<String>,
    /// M22 global usage statistics snapshot for the settings usage page.
    pub(crate) usage_stats: Option<Box<openbitfun_services_core::token_usage::UsageStatistics>>,
    pub(crate) usage_range: String,
    pub(crate) usage_loading: bool,
    /// M22 web-search credential state for the selected provider
    /// (`provider`, `configured`); fetched when the settings view opens.
    pub(crate) websearch_status: Option<(String, bool)>,
    pub(crate) websearch_credential_input: Entity<InputState>,
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
        let dock = cx.new(|_| ResizableState::default());
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let workspace_input = cx.new(|cx| InputState::new(window, cx));
        let export_input = cx.new(|cx| InputState::new(window, cx));
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let profile_name_input = cx.new(|cx| InputState::new(window, cx));
        let profile_dir_input = cx.new(|cx| InputState::new(window, cx));
        let profile_command_input = cx.new(|cx| InputState::new(window, cx));
        let agent_id_input = cx.new(|cx| InputState::new(window, cx));
        let agent_name_input = cx.new(|cx| InputState::new(window, cx));
        let agent_description_input = cx.new(|cx| InputState::new(window, cx));
        let agent_prompt_input = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_interaction_result_input = cx.new(|cx| InputState::new(window, cx));
        let websearch_credential_input = cx.new(|cx| InputState::new(window, cx));
        receive_runtime_events(event_rx, window, cx);

        // M22 close-button behavior: the callback mirrors the shared setting
        // through `CLOSE_QUITS` and always vetoes the direct close — quit
        // goes through the pump so the terminal PTYs shut down first, and
        // minimize keeps the window reachable from the taskbar.
        let close_tx = event_tx.clone();
        window.on_window_should_close(cx, move |window, _cx| {
            if CLOSE_QUITS.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = close_tx.send(RuntimeEvent::QuitRequested);
            } else {
                window.minimize_window();
            }
            false
        });

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
            pending_questions: Vec::new(),
            modal_owner: None,
            terminal_open: false,
            terminals: Vec::new(),
            active_terminal: 0,
            terminal_auto_open: std::env::var_os("OPENBITFUN_GPUI_TERMINAL_AUTOOPEN").is_some(),
            main_view: MainView::Chat,
            git: None,
            files: None,
            search: Default::default(),
            search_input,
            dock,
            terminal_height: TerminalPane::panel_height(),
            settings_auto_open: std::env::var_os("OPENBITFUN_GPUI_SETTINGS_AUTOOPEN").is_some(),
            git_auto_open: std::env::var_os("OPENBITFUN_GPUI_GIT_AUTOOPEN").is_some(),
            show_archived: false,
            rename_input,
            rename_target: None,
            export_input,
            export_target: None,
            profile_name_input,
            profile_dir_input,
            profile_command_input,
            profile_edit_id: None,
            usage_report: None,
            insights: Default::default(),
            tasks: Default::default(),
            workspaces: Vec::new(),
            workspace_input,
            workspace_error: None,
            pending_attachments: Vec::new(),
            subagent_links: Vec::new(),
            pending_child: None,
            agent_id_input,
            agent_name_input,
            agent_description_input,
            agent_prompt_input,
            agent_create_active: false,
            agent_create_kind_mode: false,
            agent_create_project_level: false,
            agent_create_error: None,
            mcp_pending_delete: None,
            mcp_browse: None,
            mcp_browse_loading: None,
            mcp_interactions: Default::default(),
            mcp_interaction_result_input,
            mcp_interaction_error: None,
            update_state: Default::default(),
            archived_delete_target: None,
            usage_stats: None,
            usage_range: "thisMonth".to_string(),
            usage_loading: false,
            websearch_status: None,
            websearch_credential_input,
        }
    }

    /// Stages clipboard image bytes for the next send; unsupported formats
    /// surface a visible notice instead of failing the turn later.
    fn attach_image_bytes(
        &mut self,
        format: ImageFormat,
        bytes: Vec<u8>,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mime = format.mime_type();
        if !SUPPORTED_IMAGE_MIMES.contains(&mime) {
            self.chat_error = Some(i18n::attachment_unsupported("clipboard image", cx));
            cx.notify();
            return;
        }
        let preview = Arc::new(Image::from_bytes(format, bytes.clone()));
        self.pending_attachments.push(PendingAttachment {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.unwrap_or_else(|| format!("clipboard.{}", format.extension())),
            mime: mime.to_string(),
            data_url: format!("data:{mime};base64,{}", BASE64.encode(bytes)),
            preview,
        });
        cx.notify();
    }

    /// Stages dropped / clipboard path entries. The bytes are read here so the
    /// turn payload is self-contained (data URL), which keeps the image
    /// readable on peer or remote runtimes that never see this filesystem.
    fn attach_image_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "image".to_string());
            let unsupported = |cx: &Context<Self>| i18n::attachment_unsupported(&name, cx);
            let Some(format) = path
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(image_format_for_extension)
            else {
                self.chat_error = Some(unsupported(cx));
                cx.notify();
                continue;
            };
            match std::fs::read(&path) {
                Ok(bytes) => self.attach_image_bytes(format, bytes, Some(name), cx),
                Err(error) => {
                    tracing::warn!("Failed to read dropped image {}: {}", path.display(), error);
                    self.chat_error = Some(unsupported(cx));
                    cx.notify();
                }
            }
        }
    }

    fn remove_attachment(&mut self, id: &str, cx: &mut Context<Self>) {
        self.pending_attachments.retain(|item| item.id != id);
        cx.notify();
    }

    /// The turn-request form of the staged images; the chips stay until the
    /// `TurnAccepted` echo so a failed submission can be retried as-is.
    fn pending_attachment_payload(&self) -> Vec<AgentInputAttachment> {
        self.pending_attachments
            .iter()
            .map(PendingAttachment::to_input_attachment)
            .collect()
    }

    /// Applies one locale snapshot to the locale-sensitive input state.
    fn apply_locale(&mut self, text: &ShellText, window: &mut Window, cx: &mut Context<Self>) {
        self.draft.update(cx, |state, cx| {
            state.set_placeholder(text.chat_input_placeholder.clone(), window, cx)
        });
    }

    fn compose_row(&self, row: &crate::runtime::SessionListRow, cx: &App) -> SessionRow {
        let session = &row.summary;
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
            archived: row.archived,
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
                sync_close_quits(cx);
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
            RuntimeEvent::Sessions(rows) => {
                self.phase = RuntimePhase::Ready;
                self.list_error = None;
                self.sessions = rows.iter().map(|row| self.compose_row(row, cx)).collect();
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
                // A requested child view (subagent run / /btw) settles here:
                // when the opened session is the awaited child its context
                // attaches to the chat; a different session drops the wait
                // (the user navigated elsewhere) and opens as a normal view.
                let mut settled_child: Option<ChildContext> = None;
                if let Some(pending) = self.pending_child.take() {
                    if pending.child_session_id == opened.session_id {
                        settled_child = Some(pending.context);
                    }
                }
                match self.chat.as_mut() {
                    // Transcript reload for the open session keeps the pane
                    // title and its child context (a reloaded child stays a
                    // child view).
                    Some(chat) if chat.matches(&opened.session_id) => {
                        chat.entries = opened.entries;
                        chat.busy = opened.busy;
                        chat.submitting = false;
                        if settled_child.is_some() {
                            chat.child_context = settled_child;
                        }
                    }
                    _ => {
                        self.chat = Some(ChatView {
                            session_id: opened.session_id,
                            name: opened.name,
                            agent_type: opened.agent_type,
                            model_id: opened.model_id,
                            entries: opened.entries,
                            busy: opened.busy,
                            submitting: false,
                            child_context: settled_child,
                        });
                    }
                }
                self.apply_subagent_links();
                self.list_sync_reset(cx);
                tracing::info!("Session transcript ready in chat pane");
            }
            RuntimeEvent::SessionOpenFailed(error) => {
                self.chat_error = Some(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::SubagentLinked {
                parent_session_id,
                parent_tool_call_id,
                subagent_session_id,
                agent_type,
            } => {
                // Newest link wins for a repeated tool id; the mapping lives
                // on the window so transcript reloads re-annotate the cards.
                self.subagent_links.retain(|link| {
                    link.parent_session_id != parent_session_id
                        || link.tool_call_id != parent_tool_call_id
                });
                self.subagent_links.push(SubagentLink {
                    parent_session_id,
                    tool_call_id: parent_tool_call_id,
                    subagent_session_id,
                    agent_type,
                });
                self.apply_subagent_links();
            }
            RuntimeEvent::BtwStarted {
                parent_session_id,
                child_session_id,
            } => {
                // The child transcript loads asynchronously; the opened echo
                // settles the child context onto the chat pane.
                self.pending_child = Some(PendingChild {
                    child_session_id: child_session_id.clone(),
                    context: ChildContext {
                        kind: ChildKind::Btw,
                        parent_session_id,
                        agent_type: None,
                    },
                });
                if let Some(handle) = runtime::runtime_handle() {
                    handle.open_session(child_session_id, self.event_tx.clone());
                }
            }
            RuntimeEvent::TurnAccepted {
                session_id,
                turn_id,
                message,
            } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.submitting = false;
                        chat.busy = true;
                        chat.entries.push(ChatEntry::User {
                            text: message,
                            turn_id: Some(turn_id),
                        });
                        // The turn carries the staged images now; a failed
                        // submission never reaches this echo, so the chips
                        // survive for a retry.
                        self.pending_attachments.clear();
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
                let failure = error.clone();
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
                self.notify_turn_finished(&session_id, failure.as_deref(), window, cx);
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
                self.open_next_modal(window, cx);
            }
            RuntimeEvent::PermissionResolved { request_id } => {
                self.pending_permissions
                    .retain(|item| item.request_id() != request_id);
                if self.pending_permissions.is_empty() {
                    self.release_modal(ModalOwner::Permissions, window, cx);
                }
            }
            RuntimeEvent::PermissionSnapshot(requests) => {
                let text = i18n::current(cx);
                self.pending_permissions = requests
                    .into_iter()
                    .map(|request| compose_permission_item(request, &text, cx))
                    .collect();
                if self.pending_permissions.is_empty() {
                    self.release_modal(ModalOwner::Permissions, window, cx);
                } else {
                    self.open_next_modal(window, cx);
                }
            }
            RuntimeEvent::InsightsLatest(metas) => {
                self.insights.latest = metas;
                self.insights.loaded = true;
            }
            RuntimeEvent::InsightsReport(report) => {
                self.insights.report = Some(report);
                self.insights.generating = false;
                self.insights.error = None;
            }
            RuntimeEvent::InsightsGenerating => {
                self.insights.generating = true;
            }
            RuntimeEvent::InsightsFailed(message) => {
                tracing::warn!("Insights action failed: {message}");
                self.insights.generating = false;
                self.insights.error = Some(message);
            }
            RuntimeEvent::CronJobs(jobs) => {
                self.tasks.jobs = jobs;
                self.tasks.loaded = true;
                self.tasks.error = None;
            }
            RuntimeEvent::CronFailed(message) => {
                tracing::warn!("Scheduled-jobs action failed: {message}");
                self.tasks.error = Some(message);
            }
            RuntimeEvent::SearchContent { hits, truncated } => {
                self.search.running = false;
                self.search.content =
                    crate::search::flatten_content_hits(&hits, crate::search::SEARCH_LIMIT);
                self.search.truncated = truncated;
            }
            RuntimeEvent::SearchSessions { hits, truncated } => {
                self.search.running = false;
                self.search.sessions = hits;
                self.search.truncated = truncated;
            }
            RuntimeEvent::SearchUnavailable(detail) => {
                self.search.running = false;
                self.search.unavailable = Some(detail);
            }
            RuntimeEvent::SearchFailed(message) => {
                tracing::warn!("Search action failed: {message}");
                self.search.running = false;
                self.search.error = Some(i18n::search_failed(&message, cx));
            }
            RuntimeEvent::QuestionsChanged(questions) => {
                self.apply_questions_changed(questions, window, cx);
            }
            RuntimeEvent::SkillCatalog(catalog) => {
                cx.set_global(catalog);
                cx.refresh_windows();
            }
            RuntimeEvent::AgentCatalog(catalog) => {
                cx.set_global(catalog);
                if self.agent_create_active {
                    // The submission landed; the create dialog closes on the
                    // catalog event that proves it.
                    self.agent_create_active = false;
                    window.close_dialog(cx);
                }
                cx.refresh_windows();
            }
            RuntimeEvent::McpCatalog(catalog) => {
                cx.set_global(catalog);
                cx.refresh_windows();
            }
            RuntimeEvent::McpServerCatalog(view) => {
                self.mcp_browse = Some(*view);
                self.mcp_browse_loading = None;
                cx.refresh_windows();
            }
            RuntimeEvent::McpInteractionAsked(request) => {
                let request = *request;
                let queue_was_empty = self.mcp_interactions.is_empty();
                self.mcp_interactions.push_back(request);
                if queue_was_empty {
                    crate::manage::open_interaction_dialog(cx.entity().downgrade(), window, cx);
                }
                cx.refresh_windows();
            }
            RuntimeEvent::ManageFailed(message) => {
                tracing::warn!("Management action failed: {message}");
                if self.agent_create_active {
                    self.agent_create_error = Some(i18n::failed_status(&message, cx));
                } else {
                    crate::manage::ManageNotice::set(Some(i18n::failed_status(&message, cx)), cx);
                }
                cx.refresh_windows();
            }
            RuntimeEvent::TrayShowWindow => {
                window.activate_window();
                cx.activate(true);
            }
            RuntimeEvent::QuitRequested => {
                // Terminal PTYs die with the window; close them properly so
                // no child is orphaned by the exit.
                self.close_all_terminal_tabs(cx);
                cx.quit();
            }
            RuntimeEvent::RestartRequested => {
                self.close_all_terminal_tabs(cx);
                match crate::update::spawn_detached_restart() {
                    Ok(()) => tracing::info!("Restart watcher spawned"),
                    Err(error) => tracing::error!("Restart failed: {error:#}"),
                }
                cx.quit();
            }
            RuntimeEvent::UpdateCheckRequested => {
                self.start_update_check(cx);
            }
            RuntimeEvent::UpdateCheckFinished { result } => {
                self.update_state = match result {
                    Ok(report) => match (&report.offered_version, &report.unsupported_reason) {
                        (Some(version), _) => crate::update::UpdateUiState::Available {
                            version: version.clone(),
                        },
                        (None, Some(reason)) => crate::update::UpdateUiState::Unsupported {
                            reason: reason.clone(),
                        },
                        (None, None) => crate::update::UpdateUiState::UpToDate,
                    },
                    Err(message) => crate::update::UpdateUiState::Failed { message },
                };
                cx.notify();
            }
            RuntimeEvent::UpdateInstallRequested => {
                self.start_update_install(cx);
            }
            RuntimeEvent::UpdateInstallFinished { result } => {
                self.update_state = match result {
                    Ok(version) => crate::update::UpdateUiState::Installed { version },
                    Err(message) => crate::update::UpdateUiState::Failed { message },
                };
                cx.notify();
            }
            RuntimeEvent::AutostartState(enabled) => {
                cx.set_global(settings::AutostartState(enabled));
                cx.refresh_windows();
            }
            RuntimeEvent::UsageReady(result) => {
                self.usage_loading = false;
                match result {
                    Ok(stats) => self.usage_stats = Some(stats),
                    Err(message) => tracing::warn!("Usage statistics failed: {message}"),
                }
                cx.refresh_windows();
            }
            RuntimeEvent::WebSearchStatus(result) => {
                match result {
                    Ok((provider, configured)) => {
                        self.websearch_status = Some((provider, configured));
                        // A save leaves no stale secret in the input.
                        self.websearch_credential_input.update(cx, |state, cx| {
                            state.set_value("", window, cx);
                        });
                    }
                    Err(message) => {
                        tracing::warn!("WebSearch credential status failed: {message}")
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::Workspaces(choices) => {
                self.workspaces = choices;
            }
            RuntimeEvent::WorkspaceSwitched(choice) => {
                tracing::info!("Adopted workspace {} ({})", choice.name, choice.root_path);
                self.workspace_error = None;
                self.apply_workspace_reset(cx);
            }
            RuntimeEvent::WorkspaceSwitchFailed(message) => {
                tracing::warn!("Workspace switch failed: {message}");
                self.workspace_error = Some(message);
            }
            RuntimeEvent::SessionRenamed { session_id, name } => {
                if let Some(row) = self.sessions.iter_mut().find(|row| row.id == session_id) {
                    row.name = name.clone();
                }
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.name = name;
                    }
                }
            }
            RuntimeEvent::SessionForked { .. } => {
                // The runtime opens the fork right away; the transcript swap
                // through `SessionOpened` is the visible feedback, and the
                // session list refresh rides the same batch.
            }
            RuntimeEvent::CompactionStarted { session_id } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.busy = true;
                        chat.entries.push(ChatEntry::Info {
                            text: i18n::current(cx).compaction_started.clone(),
                        });
                        self.list_sync_append(cx);
                    }
                }
            }
            RuntimeEvent::ExportFinished { session_id, path } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        let text = i18n::current(cx);
                        chat.entries.push(ChatEntry::Info {
                            text: format!("{} {path}", text.export_done_prefix),
                        });
                        self.list_sync_append(cx);
                    }
                }
            }
            RuntimeEvent::SessionUsage(summary) => {
                self.usage_report = Some(summary);
                let view = cx.entity().downgrade();
                window.open_dialog(cx, move |dialog, _window, cx| {
                    let text = i18n::current(cx);
                    let report = view
                        .upgrade()
                        .and_then(|shell| shell.read(cx).usage_report.clone());
                    let Some(report) = report else {
                        return dialog.child(div());
                    };
                    dialog
                        .title(div().child(text.usage_title.clone()))
                        .child(
                            div()
                                .v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(report.session_id.clone()),
                                )
                                .child(usage_report_rows(&report, &text, cx)),
                        )
                        .footer(
                            div().flex().flex_row().justify_end().child(
                                Button::new("usage-close")
                                    .label(text.session_action_cancel.clone())
                                    .with_variant(ButtonVariant::Ghost)
                                    .on_click(|_: &gpui_kit::ClickEvent, window, cx| {
                                        window.close_dialog(cx);
                                    }),
                            ),
                        )
                });
            }
            RuntimeEvent::SessionDeleted { session_id } => {
                // A deleted session cannot stay on screen: the transcript is
                // gone from storage, so every further action would fail.
                if self
                    .chat
                    .as_ref()
                    .is_some_and(|chat| chat.matches(&session_id))
                {
                    self.chat = None;
                    self.list_sync_reset(cx);
                }
                self.sessions.retain(|row| row.id != session_id);
            }
            RuntimeEvent::SessionActionFailed { message } => {
                let line = i18n::failed_status(&message, cx);
                tracing::warn!("Session action failed: {message}");
                if self.chat.is_some() {
                    self.chat_error = Some(line);
                } else {
                    self.list_error = Some(line);
                }
            }
            RuntimeEvent::SessionModelChanged {
                session_id,
                model_id,
            } => {
                if let Some(chat) = self.chat.as_mut() {
                    if chat.matches(&session_id) {
                        chat.model_id = Some(model_id);
                    }
                }
            }
            RuntimeEvent::ModelCatalog(models) => {
                cx.set_global(runtime::ModelCatalog {
                    loaded: true,
                    models,
                });
                cx.refresh_windows();
            }
        }
        cx.notify();
    }

    /// Opens or closes the built-in terminal panel. First open creates the
    /// default tab (and its PTY); close shuts every tab's PTY down and drops
    /// the panes.
    fn toggle_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_open {
            self.terminal_open = false;
            // Remember the dragged height so the next open docks at it instead
            // of the default; the second slot is the terminal panel.
            if let Some(size) = self.dock.read(cx).sizes().get(1) {
                self.terminal_height =
                    (*size).clamp(px(MIN_TERMINAL_HEIGHT), px(MAX_TERMINAL_HEIGHT));
            }
            self.close_all_terminal_tabs(cx);
        } else {
            self.open_terminal_panel(window, cx);
        }
        cx.notify();
    }

    /// Shuts every tab's PTY down and drops the panes (panel close, workspace
    /// switch).
    fn close_all_terminal_tabs(&mut self, cx: &mut Context<Self>) {
        for tab in self.terminals.drain(..) {
            tab.pane.update(cx, |pane, _| pane.close());
        }
        self.active_terminal = 0;
    }

    /// Opens the terminal panel, creating the default tab (and its PTY) on
    /// first use.
    fn open_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_open = true;
        if self.terminals.is_empty() {
            let tab = self.spawn_terminal_tab(None, window, cx);
            self.terminals.push(tab);
            self.active_terminal = 0;
        }
        cx.notify();
    }

    /// One pane launch: `None` means the default shell in the active
    /// workspace; a profile overrides the working directory and types its
    /// startup command once the PTY is up.
    fn spawn_terminal_tab(
        &mut self,
        profile: Option<openbitfun_core::service::config::types::TerminalProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TerminalTab {
        let tokio = runtime::runtime_handle()
            .map(|handle| handle.tokio_handle())
            .expect("runtime handle ready when terminal opens");
        // The pane starts from the shared terminal settings: the configured
        // shell command and scrollback are read here, font preferences are
        // also re-applied on every render below.
        let terminal = settings::ShellConfig::try_current(cx)
            .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale))
            .terminal;
        // The PTY starts in the profile's directory, else the active
        // workspace, so the terminal follows a workspace switch instead of
        // the process launch directory.
        let start_dir = profile
            .as_ref()
            .filter(|p| !p.working_directory.trim().is_empty())
            .map(|p| p.working_directory.clone())
            .or_else(|| {
                runtime::runtime_handle()
                    .map(|handle| handle.workspace().root_path.to_string_lossy().to_string())
            });
        let initial_command = profile
            .as_ref()
            .map(|p| p.startup_command.clone())
            .filter(|command| !command.trim().is_empty());
        let label = profile
            .as_ref()
            .map(|p| SharedString::from(p.name.clone()))
            .unwrap_or_else(|| i18n::current(cx).terminal_new_tab_default.clone().into());
        let pane = cx
            .new(|cx| TerminalPane::new(tokio, &terminal, start_dir, initial_command, window, cx));
        TerminalTab { pane, label }
    }

    /// Opens a new panel tab: the default shell when `profile_id` is `None`,
    /// else the configured profile with its directory and startup command.
    fn open_terminal_tab(
        &mut self,
        profile_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile = profile_id.and_then(|id| {
            settings::ShellConfig::current(cx)
                .terminal
                .profiles
                .into_iter()
                .find(|p| p.id == id)
        });
        let tab = self.spawn_terminal_tab(profile, window, cx);
        self.terminals.push(tab);
        self.active_terminal = self.terminals.len() - 1;
        self.terminal_open = true;
        cx.notify();
    }

    /// Closes one tab: its PTY is shut down and the active index stays on a
    /// sensible neighbor. Closing the last tab closes the whole panel (the
    /// dragged dock height is remembered, same as the panel toggle).
    fn close_terminal_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.terminals.len() {
            return;
        }
        let tab = self.terminals.remove(index);
        tab.pane.update(cx, |pane, _| pane.close());
        if self.terminals.is_empty() {
            if let Some(size) = self.dock.read(cx).sizes().get(1) {
                self.terminal_height =
                    (*size).clamp(px(MIN_TERMINAL_HEIGHT), px(MAX_TERMINAL_HEIGHT));
            }
            self.terminal_open = false;
            self.active_terminal = 0;
        } else {
            if self.active_terminal >= self.terminals.len() {
                self.active_terminal = self.terminals.len() - 1;
            } else if index < self.active_terminal {
                self.active_terminal -= 1;
            }
        }
        cx.notify();
    }

    /// Focuses one tab.
    fn select_terminal_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.terminals.len() {
            self.active_terminal = index;
            cx.notify();
        }
    }

    /// Resets every workspace-scoped surface after the active workspace
    /// changed: the open transcript, the terminal (its PTY runs with the old
    /// root), the changes snapshot, and the session list all belong to the
    /// previous workspace.
    fn apply_workspace_reset(&mut self, cx: &mut Context<Self>) {
        self.chat = None;
        self.chat_error = None;
        self.sessions.clear();
        self.list_error = None;
        self.show_archived = false;
        // The terminal PTYs run with the old root: shut them down instead of
        // dropping the panes (a bare drop leaks the child processes).
        self.close_all_terminal_tabs(cx);
        self.terminal_open = false;
        self.git = None;
        // The files page binds the abandoned workspace's root and tabs.
        self.files = None;
        // Search results belong to the abandoned workspace and its sessions.
        self.search = Default::default();
        self.main_view = MainView::Chat;
        // Staged images belong to the abandoned workspace's draft.
        self.pending_attachments.clear();
        cx.notify();
    }

    /// Desktop notification for a settled turn while the window is not the
    /// active one (the D36 ⑤ trigger surface: any finished turn — success or
    /// failure — in a session with an open chat). Platforms without a
    /// notification daemon no-op silently.
    fn notify_turn_finished(
        &self,
        session_id: &str,
        error: Option<&str>,
        window: &Window,
        cx: &Context<Self>,
    ) {
        if window.is_window_active() || self.chat.is_none() {
            return;
        }
        let session = self
            .sessions
            .iter()
            .find(|row| row.id == session_id)
            .map(|row| row.name.clone())
            .unwrap_or_else(|| session_id.to_string());
        let (title, key) = if error.is_some() {
            (i18n::notification_turn_failed(cx), "failed")
        } else {
            (i18n::notification_turn_done(cx), "done")
        };
        tracing::info!("Posting turn-finished notification ({key}) for session {session_id}");
        cx.show_system_notification(SystemNotification {
            tag: "openbitfun-gpui-shell-turn".into(),
            title: title.into(),
            body: session.into(),
            actions: Vec::new(),
        });
    }

    /// The workspace switcher: a footer button whose menu lists this
    /// process's workspaces plus the shared core catalog, and an "open by
    /// path" entry that registers a record without touching the desktop's
    /// current/recent selection. The builder re-reads the list every frame, so
    /// a switch or a failure is visible without reopening the menu.
    fn render_workspace_button(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity().downgrade();
        Button::new("footer-workspace")
            .icon(IconName::FolderOpen)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.workspace_switch.clone())
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _window, cx| {
                let (choices, error, active_id, tx) = match view.upgrade() {
                    Some(shell) => {
                        let shell = shell.read(cx);
                        (
                            shell.workspaces.clone(),
                            shell.workspace_error.clone(),
                            runtime::runtime_handle().map(|handle| handle.workspace().id),
                            shell.event_tx.clone(),
                        )
                    }
                    None => return menu,
                };
                let text = i18n::current(cx);
                let mut menu = menu;
                if let Some(error) = error {
                    menu = menu.item(PopupMenuItem::new(error).disabled(true));
                }
                for choice in choices {
                    let label = format!("{} · {}", choice.name, choice.root_path);
                    let item = if active_id.as_deref() == Some(choice.id.as_str()) {
                        PopupMenuItem::new(label).checked(true)
                    } else {
                        PopupMenuItem::new(label)
                    };
                    let root = choice.root_path;
                    let tx = tx.clone();
                    menu = menu.item(item.on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.switch_workspace(std::path::PathBuf::from(&root), tx.clone());
                        }
                    }));
                }
                menu.separator().item(
                    PopupMenuItem::new(text.workspace_open_path.clone()).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            open_workspace_path_dialog(view.clone(), window, cx);
                        }
                    }),
                )
            })
    }

    /// Re-annotates the open transcript's tool cards from the subagent link
    /// map (M20). Called on every transcript load and on each new link, so
    /// reloads never lose the "view run" entry points.
    fn apply_subagent_links(&mut self) {
        let Some(chat) = self.chat.as_mut() else {
            return;
        };
        for link in self
            .subagent_links
            .iter()
            .filter(|link| link.parent_session_id == chat.session_id)
        {
            let tool_key = format!("tool-{}", link.tool_call_id);
            for entry in &mut chat.entries {
                if let ChatEntry::Tool(card) = entry {
                    if card.key.as_ref() == tool_key {
                        card.subagent_session = Some(link.subagent_session_id.clone());
                    }
                }
            }
        }
    }

    /// Opens a hidden subagent session as the detail view (M20): the chat
    /// pane switches to the child transcript with a "back to parent" header
    /// entry point once the load settles.
    fn open_subagent_run(
        &mut self,
        subagent_session_id: String,
        parent_session_id: String,
        cx: &mut Context<Self>,
    ) {
        // The link records the subagent type; the badge shows it when known.
        let agent_type = self
            .subagent_links
            .iter()
            .find(|link| link.subagent_session_id == subagent_session_id)
            .and_then(|link| link.agent_type.clone());
        self.pending_child = Some(PendingChild {
            child_session_id: subagent_session_id.clone(),
            context: ChildContext {
                kind: ChildKind::Subagent,
                parent_session_id,
                agent_type,
            },
        });
        if let Some(handle) = runtime::runtime_handle() {
            handle.open_session(subagent_session_id, self.event_tx.clone());
        }
        cx.notify();
    }

    /// Returns from a child view (subagent run / /btw) to its parent session.
    fn return_from_child(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_ref() else {
            return;
        };
        let Some(child) = chat.child_context.as_ref() else {
            return;
        };
        let parent_session_id = child.parent_session_id.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.open_session(parent_session_id, self.event_tx.clone());
        }
        cx.notify();
    }

    /// Shows the settings page in the main area. The page owns its own
    /// sidebar; the session sidebar stays where it is. M22 page data (global
    /// usage stats, web-search credential status) loads here so a page render
    /// never triggers fetches.
    fn open_settings_view(&mut self, cx: &mut Context<Self>) {
        self.main_view = MainView::Settings;
        if self.usage_stats.is_none() && !self.usage_loading {
            self.usage_loading = true;
            if let Some(handle) = runtime::runtime_handle() {
                handle.fetch_usage_stats(self.usage_range.clone());
            }
        }
        let provider = settings::ShellConfig::try_current(cx)
            .map(|config| config.web_search_provider)
            .unwrap_or_default();
        if matches!(provider.as_str(), "exa_search_api" | "tavily") {
            if let Some(handle) = runtime::runtime_handle() {
                handle.load_web_search_credential_status(provider);
            }
        }
        cx.notify();
    }

    /// Runs one update check through the updater; the result lands back
    /// through the pump. Shared by the settings button and the tray menu.
    fn start_update_check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.update_state,
            crate::update::UpdateUiState::Checking | crate::update::UpdateUiState::Installing
        ) {
            return;
        }
        self.update_state = crate::update::UpdateUiState::Checking;
        cx.notify();
        let Some(handle) = runtime::runtime_handle() else {
            self.update_state = crate::update::UpdateUiState::Failed {
                message: "runtime is not ready".to_string(),
            };
            return;
        };
        handle.tokio_handle().spawn(async move {
            let result = crate::update::check_for_update(None)
                .await
                .map_err(|error| format!("{error:#}"));
            runtime::publish(RuntimeEvent::UpdateCheckFinished { result });
        });
    }

    /// Downloads and installs the offered release; the finished event carries
    /// the installed version and the page offers the restart.
    fn start_update_install(&mut self, cx: &mut Context<Self>) {
        if !matches!(
            self.update_state,
            crate::update::UpdateUiState::Available { .. }
        ) {
            return;
        }
        self.update_state = crate::update::UpdateUiState::Installing;
        cx.notify();
        let Some(handle) = runtime::runtime_handle() else {
            self.update_state = crate::update::UpdateUiState::Failed {
                message: "runtime is not ready".to_string(),
            };
            return;
        };
        handle.tokio_handle().spawn(async move {
            let result = crate::update::install_available_update(None)
                .await
                .map_err(|error| format!("{error:#}"));
            runtime::publish(RuntimeEvent::UpdateInstallFinished { result });
        });
    }

    /// Opens the changes view (creating the panel, which fetches the workspace
    /// diff snapshot) or returns to the chat when it is already showing.
    /// The chat's open session rides along: the worktree view's session
    /// isolation toggle addresses it (M19).
    fn toggle_git_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.main_view == MainView::Git {
            self.main_view = MainView::Chat;
            // Drop the panel with the view so the next open refetches.
            self.git = None;
        } else {
            self.main_view = MainView::Git;
            let active_session = self.chat.as_ref().map(|chat| chat.session_id.clone());
            self.git = Some(cx.new(|cx| GitPanel::new(active_session, window, cx)));
        }
        cx.notify();
    }

    /// Sidebar nav row activation (M23): every destination routes through the
    /// same toggle the footer actions used, so an active row toggles back to
    /// chat. Chat itself never toggles (it is the resting surface).
    fn apply_nav(&mut self, action: NavAction, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            NavAction::Chat => {
                self.main_view = MainView::Chat;
                cx.notify();
            }
            NavAction::Git => self.toggle_git_view(window, cx),
            NavAction::Insights => self.toggle_insights_view(cx),
            NavAction::Tasks => self.toggle_tasks_view(cx),
            NavAction::Files => self.toggle_files_view(cx),
            NavAction::Search => self.toggle_search_view(cx),
        }
    }

    /// Insights page toggle; the first open fetches the latest report list.
    fn toggle_insights_view(&mut self, cx: &mut Context<Self>) {
        if self.main_view == MainView::Insights {
            self.main_view = MainView::Chat;
        } else {
            self.main_view = MainView::Insights;
            if !self.insights.loaded {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.reload_insights(self.event_tx.clone());
                }
            }
        }
        cx.notify();
    }

    /// Scheduled-jobs page toggle; the first open fetches the job snapshot.
    fn toggle_tasks_view(&mut self, cx: &mut Context<Self>) {
        if self.main_view == MainView::Tasks {
            self.main_view = MainView::Chat;
        } else {
            self.main_view = MainView::Tasks;
            if !self.tasks.loaded {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.reload_cron_jobs(self.event_tx.clone());
                }
            }
        }
        cx.notify();
    }

    /// Files page toggle; the page is created on first open (which starts the
    /// root listing) and kept so open tabs survive switching back to chat.
    fn toggle_files_view(&mut self, cx: &mut Context<Self>) {
        if self.main_view == MainView::Files {
            self.main_view = MainView::Chat;
        } else {
            self.main_view = MainView::Files;
            self.files
                .get_or_insert_with(|| cx.new(crate::files::FilesPage::new));
        }
        cx.notify();
    }

    /// Global search page toggle (M16); state rides the window, so nothing
    /// is fetched on open.
    fn toggle_search_view(&mut self, cx: &mut Context<Self>) {
        if self.main_view == MainView::Search {
            self.main_view = MainView::Chat;
        } else {
            self.main_view = MainView::Search;
        }
        cx.notify();
    }

    /// Runs the search for the active tab (M16). Empty queries and in-flight
    /// runs are no-ops so the panel never floods or double-fires.
    pub(crate) fn run_search(
        &mut self,
        tx: &mpsc::UnboundedSender<RuntimeEvent>,
        cx: &mut Context<Self>,
    ) {
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() || self.search.running {
            return;
        }
        let Some(handle) = runtime::runtime_handle() else {
            self.search.error = Some("runtime not ready".to_string());
            cx.notify();
            return;
        };
        self.search.running = true;
        self.search.error = None;
        self.search.unavailable = None;
        self.search.query = query.clone();
        match self.search.tab {
            crate::search::SearchTab::Files => handle.search_workspace_content(query, tx.clone()),
            crate::search::SearchTab::Sessions => handle.search_sessions(query, tx.clone()),
        }
        cx.notify();
    }

    /// Jumps from a search hit to the files viewer with the file open.
    pub(crate) fn open_file_in_files_view(&mut self, path: String, cx: &mut Context<Self>) {
        self.main_view = MainView::Files;
        let page = self
            .files
            .get_or_insert_with(|| cx.new(crate::files::FilesPage::new));
        page.update(cx, |page, cx| page.open_path(path, cx));
        cx.notify();
    }

    fn render_status_line(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::Div {
        match &self.phase {
            RuntimePhase::Starting => div()
                .text_color(cx.theme().muted_foreground)
                .child(text.status_starting.clone()),
            RuntimePhase::Ready => {
                let workspace = runtime::runtime_handle()
                    .map(|handle| handle.workspace().name)
                    .unwrap_or_default();
                let line = div()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", text.status_ready, workspace));
                match &self.workspace_error {
                    Some(error) => line
                        .v_flex()
                        .gap_1()
                        .child(div().text_color(cx.theme().danger).child(error.clone())),
                    None => line,
                }
            }
            RuntimePhase::Failed(error) => div().text_color(cx.theme().danger).child(error.clone()),
        }
    }

    /// Sidebar entries: one session row each (archived rows only while the
    /// toggle is on), plus the empty-state and error lines. One type because
    /// `Sidebar` children share one element type.
    fn render_session_entries(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> Vec<SidebarEntry> {
        let mut entries = Vec::new();
        if let Some(error) = &self.list_error {
            entries.push(SidebarEntry::Danger(error.clone().into()));
        }
        let visible: Vec<&SessionRow> = self
            .sessions
            .iter()
            .filter(|row| self.show_archived || !row.archived)
            .collect();
        if visible.is_empty() {
            entries.push(SidebarEntry::Empty(text.sessions_empty.clone().into()));
        }
        let view = cx.entity().downgrade();
        for row in visible {
            entries.push(SidebarEntry::Session {
                row: row.clone(),
                open: self.chat.as_ref().is_some_and(|chat| chat.matches(&row.id)),
                tx: self.event_tx.clone(),
                view: view.clone(),
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
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("toggle-archived")
                            .icon(IconName::Archive)
                            .small()
                            .with_variant(if self.show_archived {
                                ButtonVariant::Secondary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .tooltip(text.sessions_show_archived.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_archived = !this.show_archived;
                                cx.notify();
                            })),
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
                    ),
            );

        // Nav rows first (web-ui NavPanel order: navigation sections above the
        // session list), then the sessions group with the live list.
        let nav_rows: Vec<SidebarEntry> = nav_actions()
            .into_iter()
            .map(|action| SidebarEntry::Nav {
                action,
                label: SharedString::from(nav_label(action, text)),
                active: action.is_active(self.main_view),
                view: cx.entity().downgrade(),
            })
            .collect();

        Sidebar::new("shell-sidebar")
            .w(px(SESSION_PANE_WIDTH))
            .header(header)
            .child(SidebarGroup::new(text.sidebar_nav_title.clone()).children(nav_rows))
            .child(
                SidebarGroup::new(text.sessions_title.clone())
                    .children(self.render_session_entries(text, cx)),
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

    /// Footer actions: workspace, terminal dock toggle, settings. The surface
    /// destinations moved into the sidebar nav group (M23); this footer stays
    /// because it is always visible and reliably receives input (the component
    /// title bar's right-click menu overlay swallows clicks above it —
    /// gpui-component 0.7.1, reproduced on Xvfb 2026-10-09).
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
            .child(self.render_workspace_button(text, cx))
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

/// Everything the per-turn fork menu and the subagent "view run" entry point
/// need; `None` in list harnesses that render entries without a live session
/// attached.
#[derive(Clone)]
struct ChatForkContext {
    session_id: String,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    /// Weak handle back to the shell window: the subagent run entry point
    /// settles the pending child context before opening the child session.
    view: gpui_kit::WeakEntity<ShellWindow>,
}

/// One transcript entry as a scroller row. Row insets and the inter-row gap
/// belong to the `MessageScroller` row wrapper; the reading column is applied
/// by the renderer closure around this item.
fn render_chat_item(
    entry: &ChatEntry,
    text: &ShellText,
    fork: Option<&ChatForkContext>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    match entry {
        ChatEntry::User {
            text: body,
            turn_id,
        } => {
            let mut column = div()
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
                );
            if let (Some(turn_id), Some(fork)) = (turn_id, fork) {
                column = column.child(turn_fork_menu(
                    fork.session_id.clone(),
                    turn_id.clone(),
                    fork.tx.clone(),
                    text,
                ));
            }
            column.into_any_element()
        }
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
        ChatEntry::Tool(card) => {
            // The "view run" entry point needs the shell handle; harnesses
            // that render without a fork render bare cards (M20).
            let run = fork.filter(|_| card.subagent_session.is_some());
            render_tool_card(card, run, text, cx).into_any_element()
        }
        ChatEntry::Notice { text: body } => div()
            .w_full()
            .text_sm()
            .text_color(cx.theme().danger)
            .child(body.clone())
            .into_any_element(),
        ChatEntry::Info { text: body } => div()
            .w_full()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(body.clone())
            .into_any_element(),
    }
}

/// Per-turn menu under a user prompt: fork including the selected turn, from
/// just before it, or roll the session back to the turn boundary (M19). Forks
/// open the copy when the runtime confirms; rollback reverts transcript and
/// workspace files in place and refreshes through the history-changed event.
fn turn_fork_menu(
    session_id: String,
    turn_id: String,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
) -> gpui_kit::AnyElement {
    let at_action = {
        let session_id = session_id.clone();
        let turn_id = turn_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.fork_session_turn(session_id.clone(), turn_id.clone(), false, tx.clone());
            }
        }
    };
    let before_action = {
        let session_id = session_id.clone();
        let turn_id = turn_id.clone();
        let tx = tx.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.fork_session_turn(session_id.clone(), turn_id.clone(), true, tx.clone());
            }
        }
    };
    let rollback_action = {
        let session_id = session_id.clone();
        let turn_id = turn_id.clone();
        move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
            if let Some(handle) = runtime::runtime_handle() {
                handle.rollback_session_to_turn(session_id.clone(), turn_id.clone(), tx.clone());
            }
        }
    };
    Button::new(SharedString::from(format!("turn-fork-{turn_id}")))
        .icon(IconName::GitBranch)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .tooltip(text.turn_menu.clone())
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
            let text = i18n::current(cx);
            menu.item(PopupMenuItem::new(text.fork_at_turn.clone()).on_click(at_action.clone()))
                .item(
                    PopupMenuItem::new(text.fork_before_turn.clone())
                        .on_click(before_action.clone()),
                )
                .separator()
                .item(
                    PopupMenuItem::new(text.rollback_at_turn.clone())
                        .on_click(rollback_action.clone()),
                )
        })
        .into_any_element()
}

/// Stable per-content markdown element id: transcript indices shift when
/// history reloads swap the entries vec, and a reused id pointing at new
/// content would briefly show the previous parse while the worker catches up.
fn markdown_id_key(body: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(body, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

fn render_tool_card(
    card: &ToolCard,
    run: Option<&ChatForkContext>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
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
    // A linked hidden subagent session (M20): the card grows the entry point
    // that opens the child transcript as the detail view.
    if let (Some(subagent_session_id), Some(fork)) = (&card.subagent_session, run) {
        let child = subagent_session_id.clone();
        let parent = fork.session_id.clone();
        let view = fork.view.clone();
        let label = text.subagent_open.clone();
        row = row.child(
            div().flex().justify_end().child(
                Button::new(SharedString::from(format!("{}-view-run", card.key)))
                    .label(label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |shell, cx| {
                                shell.open_subagent_run(child.clone(), parent.clone(), cx);
                            });
                        }
                    }),
            ),
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
            return brand_empty(IconName::Sparkles, &text.chat_empty_title, &text.chat_empty);
        }
        let this = cx.entity();
        let jump_label = text.chat_jump_latest.clone();
        let text = text.clone();
        MessageScroller::new(
            "transcript",
            self.chat_scroller.clone(),
            move |index, _window, cx| {
                let snapshot = this.read(cx);
                let entry = snapshot
                    .chat
                    .as_ref()
                    .and_then(|chat| chat.entries.get(index))
                    .cloned();
                let fork = snapshot.chat.as_ref().map(|chat| ChatForkContext {
                    session_id: chat.session_id.clone(),
                    tx: snapshot.event_tx.clone(),
                    view: this.downgrade(),
                });
                match entry {
                    Some(entry) => div()
                        .w_full()
                        .max_w(px(READING_COLUMN_WIDTH))
                        .mx_auto()
                        .child(render_chat_item(&entry, &text, fork.as_ref(), cx))
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

    /// One staged image chip: thumbnail, name, remove button.
    fn render_attachment_chip(
        &self,
        item: &PendingAttachment,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let view = cx.entity().downgrade();
        let id = item.id.clone();
        let remove = Button::new(ElementId::Name(
            format!("attachment-remove-{}", item.id).into(),
        ))
        .icon(IconName::X)
        .xsmall()
        .on_click(move |_, _, cx| {
            if let Some(view) = view.upgrade() {
                view.update(cx, |this, cx| this.remove_attachment(&id, cx));
            }
        });
        div()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .py_0p5()
            .rounded(cx.theme().radius_tokens().md)
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                img(item.preview.clone())
                    .size(px(24.))
                    .rounded_sm()
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .max_w(px(140.))
                    .text_xs()
                    .truncate()
                    .child(item.name.clone()),
            )
            .child(remove)
            .into_any_element()
    }

    /// Staged-image strip above the input row; `None` keeps the composer at
    /// its usual height.
    fn render_attachment_strip(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        if self.pending_attachments.is_empty() {
            return None;
        }
        let chips: Vec<gpui_kit::AnyElement> = self
            .pending_attachments
            .iter()
            .map(|item| self.render_attachment_chip(item, cx))
            .collect();
        Some(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .w_full()
                .children(chips)
                .into_any_element(),
        )
    }

    fn render_composer(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(chat) = self.chat.as_ref() else {
            return div().into_any_element();
        };
        let busy = chat.busy;
        let submitting = chat.submitting;
        let session_id = chat.session_id.clone();
        let has_attachments = !self.pending_attachments.is_empty();

        let draft_value = self.draft.read(cx).value().to_string();
        let draft_for_send = self.draft.clone();
        let tx_for_send = self.event_tx.clone();
        let session_for_send = session_id.clone();
        let view_for_send = cx.entity().downgrade();

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
                .disabled(submitting || (draft_value.trim().is_empty() && !has_attachments))
                .on_click(move |_, window, cx| {
                    let body = draft_for_send.read(cx).value().to_string();
                    let trimmed = body.trim();
                    if trimmed.is_empty() && !has_attachments {
                        return;
                    }
                    // `/btw <question>` routes to a side-question child
                    // session (M20); attachments stay behind — the child
                    // forks the parent context instead of carrying media.
                    if let Some(question) = btw_question(&body) {
                        if question.is_empty() {
                            if let Some(view) = view_for_send.upgrade() {
                                view.update(cx, |shell, cx| {
                                    shell.chat_error = Some(i18n::btw_empty(cx));
                                });
                            }
                            return;
                        }
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.start_btw(
                                session_for_send.clone(),
                                question.to_string(),
                                tx_for_send.clone(),
                            );
                        }
                        draft_for_send.update(cx, |state, cx| {
                            state.set_value("", window, cx);
                        });
                        return;
                    }
                    let attachments = view_for_send
                        .upgrade()
                        .map(|view| view.read(cx).pending_attachment_payload())
                        .unwrap_or_default();
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.send_message(
                            session_for_send.clone(),
                            body,
                            attachments,
                            tx_for_send.clone(),
                        );
                    }
                    draft_for_send.update(cx, |state, cx| {
                        state.set_value("", window, cx);
                    });
                })
        };

        let strip = self.render_attachment_strip(cx);
        let paste_view = cx.entity().downgrade();

        div()
            .w_full()
            .max_w(px(READING_COLUMN_WIDTH))
            .mx_auto()
            .px_4()
            .pb_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_2xl())
                    .shadow_sm()
                    .p_2()
                    .when_some(strip, |this, strip| this.child(strip))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .items_end()
                            .child(
                                Textarea::new(&self.draft)
                                    .appearance(false)
                                    .flex_1()
                                    .h(px(COMPOSER_HEIGHT))
                                    // Image and file pastes become staged
                                    // attachments; text falls through to the
                                    // input's own insertion.
                                    .on_paste(move |item, _window, cx| {
                                        let Some(view) = paste_view.upgrade() else {
                                            return false;
                                        };
                                        let mut taken = false;
                                        view.update(cx, |this, cx| {
                                            for entry in item.entries() {
                                                match entry {
                                                    ClipboardEntry::Image(image) => {
                                                        this.attach_image_bytes(
                                                            image.format,
                                                            image.bytes.clone(),
                                                            None,
                                                            cx,
                                                        );
                                                        taken = true;
                                                        break;
                                                    }
                                                    ClipboardEntry::ExternalPaths(paths) => {
                                                        let paths: Vec<PathBuf> = paths.0.to_vec();
                                                        this.attach_image_paths(paths, cx);
                                                        taken = true;
                                                        break;
                                                    }
                                                    ClipboardEntry::String(_) => {}
                                                }
                                            }
                                        });
                                        taken
                                    }),
                            )
                            .child(action_button),
                    ),
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
        self.modal_owner = Some(ModalOwner::Permissions);
    }

    /// Opens the modal question dialog bound to the head of the pending
    /// queue. Like the approval dialog the builder re-reads the queue every
    /// frame, so answering one batch reveals the next without reopening, and
    /// the dialog closes when the mailbox rebuild drops the last question
    /// rather than in the button handlers. Every dismissal path without a
    /// decision (Escape, backdrop press, close icon) is disabled: the
    /// question parks its turn and has no deadline left once the shell has
    /// shown it, so the only ways out are Submit and Cancel.
    fn open_question_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let Some(view) = view.upgrade() else {
                return dialog;
            };
            let Some(item) = view.read(cx).pending_questions.first().cloned() else {
                return dialog;
            };
            let text = i18n::current(cx);
            dialog
                .title(div().child(text.question_title.clone()))
                .overlay_closable(false)
                .keyboard(false)
                .close_button(false)
                .child(question_body(&view.downgrade(), &item, &text, cx))
                .footer(question_footer(
                    &item,
                    &text,
                    submit_question,
                    cancel_question,
                ))
        });
        self.modal_owner = Some(ModalOwner::Questions);
    }

    /// Opens the next waiting modal unless one is already open. Approvals
    /// take the slot first: they gate a tool call that is otherwise parked,
    /// while an attended question waits on the mailbox without a deadline.
    fn open_next_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal_owner.is_some() {
            return;
        }
        match next_modal_target(self.pending_permissions.len(), self.pending_questions.len()) {
            Some(ModalOwner::Permissions) => self.open_permission_dialog(window, cx),
            Some(ModalOwner::Questions) => self.open_question_dialog(window, cx),
            None => {}
        }
    }

    /// Releases the dialog slot once `owner`'s queue drained, then hands it
    /// to the next waiting queue. No-op while the other queue owns the slot.
    fn release_modal(&mut self, owner: ModalOwner, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal_owner != Some(owner) {
            return;
        }
        window.close_dialog(cx);
        self.modal_owner = None;
        self.open_next_modal(window, cx);
    }

    /// Rebuilds the question queue from a mailbox snapshot. Existing items are
    /// carried over so selections survive an unrelated question arriving or
    /// leaving; only new tool ids are parsed.
    fn apply_questions_changed(
        &mut self,
        questions: Vec<PendingUserQuestion>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_questions =
            rebuild_question_queue(&self.pending_questions, questions, |index, total| {
                i18n::question_progress(index, total, cx)
            });
        if self.pending_questions.is_empty() {
            self.release_modal(ModalOwner::Questions, window, cx);
        } else {
            self.open_next_modal(window, cx);
        }
    }

    /// Toggles one option of the dialog's head item: single-select replaces
    /// the selection, multi-select adds or removes it.
    fn toggle_question_option(
        &mut self,
        tool_id: &str,
        question_index: usize,
        option_index: usize,
    ) {
        let Some(item) = self
            .pending_questions
            .iter_mut()
            .find(|item| item.tool_id == tool_id)
        else {
            return;
        };
        let Some(question) = item.questions.get(question_index) else {
            return;
        };
        let multi_select = question.multi_select;
        let Some(selected) = item.selected.get_mut(question_index) else {
            return;
        };
        toggle_option_selection(selected, option_index, multi_select);
    }
}

/// Applies one option click: single-select replaces the selection,
/// multi-select adds or removes the option.
fn toggle_option_selection(
    selected: &mut BTreeSet<usize>,
    option_index: usize,
    multi_select: bool,
) {
    if multi_select {
        if !selected.remove(&option_index) {
            selected.insert(option_index);
        }
    } else {
        selected.clear();
        selected.insert(option_index);
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

/// Forwards the dialog's submitted answers to the embedded runtime. The queue
/// advances on the mailbox rebuild, not here.
/// Splits a `/btw <question>` composer input into its question. `None` when
/// the text is not the command ("/btwfoo" is just a message); `Some("")`
/// when the command is there but the question is missing.
fn btw_question(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix("/btw")?;
    if rest.is_empty() {
        Some("")
    } else if rest.starts_with(char::is_whitespace) {
        Some(rest.trim())
    } else {
        None
    }
}

fn submit_question(tool_id: String, answers: serde_json::Value) {
    if let Some(handle) = runtime::runtime_handle() {
        handle.respond_user_question(tool_id, answers);
    }
}

/// Declines one question on the user's behalf; the tool reports the
/// cancellation back to the model.
fn cancel_question(session_id: String, tool_id: String) {
    if let Some(handle) = runtime::runtime_handle() {
        handle.cancel_user_question(session_id, tool_id);
    }
}

/// Dialog body for one question batch: every question of the batch with its
/// options, scrollable so a long batch stays inside the dialog.
fn question_body(
    view: &WeakEntity<ShellWindow>,
    item: &QuestionPromptItem,
    text: &ShellText,
    cx: &App,
) -> impl IntoElement {
    let mut body = div()
        .v_flex()
        .gap_4()
        .w(px(520.))
        .max_h(px(420.))
        .overflow_y_scrollbar()
        .text_sm();
    if item.questions.is_empty() {
        return body.child(
            div()
                .text_color(cx.theme().warning)
                .child(text.question_unreadable.clone()),
        );
    }
    for (index, question) in item.questions.iter().enumerate() {
        let heading = if question.header.trim().is_empty() {
            question.progress_line.clone()
        } else {
            format!("{} · {}", question.header, question.progress_line)
        };
        let mut options = div().v_flex().gap_2();
        for (option_index, option) in question.options.iter().enumerate() {
            let selected = item
                .selected
                .get(index)
                .is_some_and(|set| set.contains(&option_index));
            options = options.child(question_option_row(
                view.clone(),
                item.tool_id.clone(),
                index,
                option_index,
                option,
                question.multi_select,
                selected,
                cx,
            ));
        }
        body = body.child(
            div()
                .v_flex()
                .gap_2()
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(heading),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().foreground)
                                .child(question.question.clone()),
                        )
                        .when(question.multi_select, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.question_multi.clone()),
                            )
                        }),
                )
                .child(options),
        );
    }
    body
}

/// One clickable option row. Selection lives on the shell item, so the click
/// updates that item and the dialog's next frame shows the new state.
#[allow(clippy::too_many_arguments)]
fn question_option_row(
    view: WeakEntity<ShellWindow>,
    tool_id: String,
    question_index: usize,
    option_index: usize,
    option: &PromptOption,
    multi_select: bool,
    selected: bool,
    cx: &App,
) -> impl IntoElement {
    let marker = match (multi_select, selected) {
        (true, true) => IconName::Check,
        (true, false) => IconName::Square,
        (false, true) => IconName::CircleDot,
        (false, false) => IconName::Circle,
    };
    div()
        .id(SharedString::from(format!(
            "question-option-{tool_id}-{question_index}-{option_index}"
        )))
        .cursor_pointer()
        .h_flex()
        .items_start()
        .gap_2()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if selected {
            cx.theme().primary
        } else {
            cx.theme().border
        })
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().muted))
        })
        .child(Icon::new(marker).size_4().text_color(if selected {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        }))
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .min_w_0()
                .child(
                    div()
                        .text_color(cx.theme().foreground)
                        .child(option.label.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(option.description.clone()),
                ),
        )
        .on_click(move |_, _, cx| {
            if let Some(shell) = view.upgrade() {
                shell.update(cx, |this, cx| {
                    this.toggle_question_option(&tool_id, question_index, option_index);
                    cx.notify();
                });
            }
        })
}

/// Dialog footer: Cancel (declines the question) and Submit, disabled until
/// every question has a selection (same affordance as the composer's send
/// button). The callbacks are injected so tests can observe the decisions
/// without a runtime.
fn question_footer(
    item: &QuestionPromptItem,
    text: &ShellText,
    on_submit: impl Fn(String, serde_json::Value) + Clone + 'static,
    on_cancel: impl Fn(String, String) + Clone + 'static,
) -> gpui_kit::Div {
    let submit_tool_id = item.tool_id.clone();
    let answers_item = item.clone();
    let cancel_session_id = item.session_id.clone();
    let cancel_tool_id = item.tool_id.clone();
    div()
        .flex()
        .justify_end()
        .gap_2()
        .child(
            Button::new("question-cancel")
                .label(text.question_cancel.clone())
                .with_variant(ButtonVariant::Ghost)
                .on_click({
                    let on_cancel = on_cancel.clone();
                    move |_, _, _| {
                        on_cancel(cancel_session_id.clone(), cancel_tool_id.clone());
                    }
                }),
        )
        .child(
            Button::new("question-submit")
                .label(text.question_submit.clone())
                .with_variant(ButtonVariant::Primary)
                .disabled(!item.is_answered())
                .on_click({
                    let on_submit = on_submit.clone();
                    move |_, _, _| {
                        on_submit(submit_tool_id.clone(), answers_item.answers());
                    }
                }),
        )
}

/// Opens the rename dialog for one session. The input entity lives on the
/// shell (a dialog builder runs every frame and must not create state), and the
/// dialog is dismissable: unlike a permission ask, a rename nobody answers
/// blocks nothing.
fn open_rename_dialog(
    view: WeakEntity<ShellWindow>,
    session_id: String,
    current_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let input = shell.read(cx).rename_input.clone();
    input.update(cx, |state, cx| state.set_value(current_name, window, cx));
    shell.update(cx, |this, cx| {
        this.rename_target = Some(session_id.clone());
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let name = shell
            .read(cx)
            .rename_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if name.is_empty() {
            // Keep the dialog open: an empty name would rename nothing the
            // user could recognize in the list.
            tracing::warn!("Ignoring empty session name");
            return;
        }
        let session_id = shell.read(cx).rename_target.clone().unwrap_or_default();
        if session_id.is_empty() {
            window.close_dialog(cx);
            return;
        }
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.rename_session(session_id, name, tx);
        }
        shell.update(cx, |this, cx| {
            this.rename_target = None;
            cx.notify();
        });
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |this, cx| {
                this.rename_target = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.session_rename_title.clone()))
            .child(div().w(px(360.)).child(Input::new(&input)))
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("rename-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("rename-confirm")
                            .label(text.session_action_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the "switch workspace by path" dialog: the typed path is registered
/// (or reused) as a workspace record and becomes this shell's active
/// workspace. The dialog is dismissable; unlike an ask, nobody answering it
/// blocks nothing.
fn open_workspace_path_dialog(view: WeakEntity<ShellWindow>, window: &mut Window, cx: &mut App) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let input = shell.read(cx).workspace_input.clone();
    input.update(cx, |state, cx| state.set_value("", window, cx));
    let confirm_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let path = shell
            .read(cx)
            .workspace_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if path.is_empty() {
            tracing::warn!("Ignoring empty workspace path");
            return;
        }
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.switch_workspace(std::path::PathBuf::from(path), tx);
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.workspace_switch.clone()))
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .w(px(420.))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(text.workspace_open_path_description.clone()),
                    )
                    .child(Input::new(&input)),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("workspace-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("workspace-confirm")
                            .label(text.workspace_switch.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the delete confirmation for one session. Deleting is destructive and
/// has no undo, so it never happens straight from the row menu.
fn open_export_dialog(
    view: WeakEntity<ShellWindow>,
    session_id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let input = shell.read(cx).export_input.clone();
    input.update(cx, |state, cx| {
        state.set_value(default_export_path(&name), window, cx)
    });
    shell.update(cx, |this, cx| {
        this.export_target = Some(session_id.clone());
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let path = shell
            .read(cx)
            .export_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if path.is_empty() {
            // Keep the dialog open: an empty path would write nowhere.
            tracing::warn!("Ignoring empty export path");
            return;
        }
        let session_id = shell.read(cx).export_target.clone().unwrap_or_default();
        if session_id.is_empty() {
            window.close_dialog(cx);
            return;
        }
        let tx = shell.read(cx).event_tx.clone();
        if let Some(handle) = runtime::runtime_handle() {
            handle.export_session(session_id, PathBuf::from(&path), tx);
        }
        shell.update(cx, |this, cx| {
            this.export_target = None;
            cx.notify();
        });
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |this, cx| {
                this.export_target = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.export_title.clone()))
            .child(div().w(px(420.)).child(Input::new(&input)))
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("export-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("export-confirm")
                            .label(text.session_action_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Export prefill: the user's Downloads directory (HOME / USERPROFILE),
/// falling back to the current directory when neither is set.
fn default_export_path(name: &str) -> String {
    let base = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(|home| PathBuf::from(home).join("Downloads"))
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join(format!("openbitfun-{}.md", export_slug(name)))
        .display()
        .to_string()
}

/// File-name slug for an export: ASCII lowercase words joined by dashes; a
/// name with no ASCII word characters collapses to `session`.
fn export_slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let compacted = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if compacted.is_empty() {
        "session".to_string()
    } else {
        compacted
    }
}

/// Key/value rows for the usage dialog: headline token counts, timing, then
/// one line per model the session called.
fn usage_report_rows(
    report: &runtime::SessionUsageSummary,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let mut entries: Vec<(String, String)> = vec![
        (text.usage_turns.clone(), report.turn_count.to_string()),
        (
            text.usage_input_tokens.clone(),
            optional_tokens(report.input_tokens),
        ),
        (
            text.usage_output_tokens.clone(),
            optional_tokens(report.output_tokens),
        ),
        (
            text.usage_cached_tokens.clone(),
            optional_tokens(report.cached_tokens),
        ),
        (
            text.usage_total_tokens.clone(),
            optional_tokens(report.total_tokens),
        ),
        (
            text.usage_wall_time.clone(),
            optional_duration(report.wall_time_ms),
        ),
        (
            text.usage_active_time.clone(),
            optional_duration(report.active_turn_ms),
        ),
    ];
    if !report.models.is_empty() {
        for model in &report.models {
            entries.push((
                format!("{} · {}", text.usage_models.clone(), model.model_id),
                format!("{} × {}", model.calls, optional_tokens(model.total_tokens)),
            ));
        }
    }
    let mut rows = div().v_flex().gap_1().w(px(420.));
    for (label, value) in entries {
        rows = rows.child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .gap_2()
                .text_sm()
                .child(div().text_color(cx.theme().muted_foreground).child(label))
                .child(div().child(value)),
        );
    }
    rows
}

/// Compact duration for the usage dialog: seconds below a minute, then whole
/// minutes and seconds.
fn optional_duration(ms: Option<u64>) -> String {
    let Some(ms) = ms else {
        return "—".to_string();
    };
    if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        let total_seconds = ms / 1000;
        format!("{} m {} s", total_seconds / 60, total_seconds % 60)
    }
}

fn optional_tokens(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "—".to_string())
}

fn open_delete_dialog(
    view: WeakEntity<ShellWindow>,
    session_id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let text = i18n::current(cx);
        let confirm_id = session_id.clone();
        let confirm_view = view.clone();
        alert
            .confirm()
            .title(text.session_delete_title.clone())
            .description(
                div()
                    .v_flex()
                    .gap_1()
                    .child(name.clone())
                    .child(text.session_delete_warning.clone()),
            )
            .ok_text(text.session_delete_confirm.clone())
            .cancel_text(text.session_action_cancel.clone())
            .on_ok(move |_, _, cx| {
                if let Some(shell) = confirm_view.upgrade() {
                    let tx = shell.read(cx).event_tx.clone();
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.delete_session(confirm_id.clone(), tx);
                    }
                }
                true
            })
    });
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
                    self.toggle_git_view(window, cx);
                }
            }
        }

        // Terminal font preferences apply to live panes: the paint pass
        // remeasures the cell and reflows the grid (and the PTY) when they land.
        // The settings page handlers only get `&mut App`, so the window is what
        // pushes the shared config into the panes; the call is a no-op unless a
        // value actually changed.
        if !self.terminals.is_empty() {
            let terminal = settings::ShellConfig::try_current(cx)
                .unwrap_or_else(|| settings::ShellConfig::startup_default(text.locale))
                .terminal;
            for tab in &self.terminals {
                tab.pane
                    .update(cx, |pane, cx| pane.apply_font(&terminal, cx));
            }
        }

        let main_area = match self.main_view {
            MainView::Settings => settings::render_page(&text, cx.entity().downgrade(), cx),
            MainView::Git => match self.git.clone() {
                Some(panel) => div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(panel)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            MainView::Insights => div()
                .id("insights-page")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(crate::insights::render_page(
                    &self.insights,
                    &self.event_tx,
                    &text,
                    cx,
                ))
                .into_any_element(),
            MainView::Tasks => div()
                .id("tasks-page")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(crate::tasks::render_page(
                    &self.tasks,
                    &self.event_tx,
                    &text,
                    cx,
                ))
                .into_any_element(),
            MainView::Files => match self.files.clone() {
                Some(page) => div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(page)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            MainView::Search => div()
                .id("search-page")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(crate::search::render_page(
                    cx.entity().downgrade(),
                    &self.search,
                    &self.search_input,
                    &self.event_tx,
                    &text,
                    cx,
                ))
                .into_any_element(),
            MainView::Chat => self.render_chat_column(&text, cx),
        };

        // No root background: the glass sidebar paints a 90% chrome tint over
        // the window surface (compositor blur shows through), so only the
        // main area and the docked terminal lay an opaque canvas below their
        // content. Every other surface paints its own background.
        div()
            .size_full()
            .v_flex()
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
            // The body and the docked terminal are one vertical split: the
            // divider above the terminal drags its height, and the body keeps
            // the rest. Closed, the terminal panel is simply absent, so the
            // body takes the whole split.
            .child(
                div()
                    .id("shell-dock-frame")
                    .debug_selector(|| "shell-dock-frame".to_string())
                    .size_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child({
                        // One `children` call with the whole panel list:
                        // `ResizablePanelGroup::children` *replaces* the panel
                        // list, so mixing it with `child` silently drops every
                        // panel added before it (upstream 0.7.1).
                        let mut panels = vec![resizable_panel().child(
                            div()
                                .size_full()
                                .flex()
                                .flex_row()
                                .min_w_0()
                                .min_h_0()
                                .child(
                                    div()
                                        .id("shell-body-sidebar")
                                        .debug_selector(|| "shell-body-sidebar".to_string())
                                        .h_full()
                                        .child(self.render_sidebar(&text, cx)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .min_w_0()
                                        .flex()
                                        .bg(cx.theme().background)
                                        .child(main_area),
                                ),
                        )];
                        if self.terminal_open {
                            panels.push(
                                resizable_panel()
                                    .size(self.terminal_height)
                                    .flex_none()
                                    .size_range(px(MIN_TERMINAL_HEIGHT)..px(MAX_TERMINAL_HEIGHT))
                                    .child(self.render_terminal_panel(&text, cx)),
                            );
                        }
                        v_resizable("shell-dock")
                            .with_state(&self.dock)
                            .children(panels)
                    }),
            )
    }
}

impl ShellWindow {
    /// The docked terminal panel: header (new-tab menu, one chip per tab,
    /// close) above the active pane. Each pane owns focus, keyboard, wheel
    /// scrollback, and reflows to whatever size the panel gives it.
    fn render_terminal_panel(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let active_title = self
            .terminals
            .get(self.active_terminal)
            .map(|tab| {
                let fallback = tab.label.clone();
                tab.pane.read(cx).title_label(fallback)
            })
            .unwrap_or_else(|| text.terminal_title.clone().into());

        // New-tab menu: the default shell plus every configured profile, and
        // the settings entry that manages the list (labels resolve here, in
        // the menu builder, like the insights range menu; item clicks carry
        // the window so pane creation goes through the ordinary path).
        let profiles = settings::ShellConfig::current(cx).terminal.profiles.clone();
        let menu_view = cx.entity().downgrade();
        let new_tab_button = Button::new("terminal-new-tab")
            .icon(IconName::Plus)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.terminal_new_tab.clone())
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, cx| {
                let default_label = crate::i18n::current(cx).terminal_new_tab_default.clone();
                let manage_label = crate::i18n::current(cx).terminal_profiles_manage.clone();
                let mut popup = menu.item(PopupMenuItem::new(default_label).on_click({
                    let menu_view = menu_view.clone();
                    move |_, window, cx| {
                        if let Some(shell) = menu_view.upgrade() {
                            shell.update(cx, |this, cx| this.open_terminal_tab(None, window, cx));
                        }
                    }
                }));
                for profile in &profiles {
                    let profile_id = profile.id.clone();
                    let menu_view = menu_view.clone();
                    popup = popup.item(PopupMenuItem::new(profile.name.clone()).on_click(
                        move |_, window, cx| {
                            if let Some(shell) = menu_view.upgrade() {
                                shell.update(cx, |this, cx| {
                                    this.open_terminal_tab(Some(profile_id.clone()), window, cx)
                                });
                            }
                        },
                    ));
                }
                popup = popup.item(PopupMenuItem::new(manage_label).on_click({
                    let menu_view = menu_view.clone();
                    move |_, _window, cx| {
                        if let Some(shell) = menu_view.upgrade() {
                            shell.update(cx, |this, cx| this.open_settings_view(cx));
                        }
                    }
                }));
                popup
            });

        let mut tab_bar = div().flex().items_center().gap_1().min_w_0();
        for (index, tab) in self.terminals.iter().enumerate() {
            let active = index == self.active_terminal;
            let fallback = tab.label.clone();
            let label = tab.pane.read(cx).title_label(fallback);
            let close_view = cx.entity();
            tab_bar = tab_bar.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded(cx.theme().radius)
                    .when(active, |style| style.bg(cx.theme().sidebar_accent))
                    .child(
                        div()
                            .id(SharedString::from(format!("terminal-tab-select-{index}")))
                            .cursor_pointer()
                            .max_w_32()
                            .truncate()
                            .text_xs()
                            .text_color(if active {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .on_click({
                                let this = cx.entity();
                                move |_, _, cx| {
                                    this.update(cx, |this, cx| this.select_terminal_tab(index, cx));
                                }
                            })
                            .child(label),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("terminal-tab-close-{index}")))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                close_view
                                    .update(cx, |this, cx| this.close_terminal_tab(index, cx));
                            })
                            .child(
                                Icon::new(IconName::Close)
                                    .size_3()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    ),
            );
        }

        div()
            .id("shell-terminal-dock")
            .debug_selector(|| "shell-terminal-dock".to_string())
            .v_flex()
            .size_full()
            .min_h_0()
            .border_t_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().background)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .child(new_tab_button)
                    .child(
                        div()
                            .id("terminal-tabbar-scroll")
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .child(div().flex().items_center().gap_1().child(tab_bar)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .max_w_48()
                            .child(active_title),
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
            .child(match self.terminals.get(self.active_terminal) {
                Some(tab) => div()
                    .flex_1()
                    .min_h_0()
                    .child(tab.pane.clone())
                    .into_any_element(),
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
            // External file drags land anywhere on the chat surface; the
            // highlight only marks the composer card as the drop target.
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths: Vec<PathBuf> = paths.0.to_vec();
                this.attach_image_paths(paths, cx);
            }))
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
                        // The streaming marker sweeps a highlight across the
                        // label; the component degrades to plain text under
                        // reduced motion.
                        .child(ShimmerText::new(label)),
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        // Hidden child view (M20): the badge names what the
                        // pane shows and "back" returns to the parent.
                        .when_some(chat.child_context.as_ref(), |this, child| {
                            let label = match (child.kind, child.agent_type.as_deref()) {
                                (ChildKind::Subagent, Some(agent_type)) => {
                                    format!("{} · {}", text.subagent_badge, agent_type)
                                }
                                (ChildKind::Subagent, None) => text.subagent_badge.clone(),
                                (ChildKind::Btw, _) => text.btw_badge.clone(),
                            };
                            let back_label = text.child_back.clone();
                            this.child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().sidebar_accent)
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(label),
                            )
                            .child(
                                Button::new("chat-child-back")
                                    .label(back_label)
                                    .small()
                                    .with_variant(ButtonVariant::Ghost)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.return_from_child(cx);
                                    })),
                            )
                        })
                        .child(self.render_usage_button(cx))
                        .child(self.render_model_picker(text, cx))
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
                ),
            None => div(),
        }
    }

    /// Header usage button: asks the runtime for the session usage report;
    /// the dialog opens when [`RuntimeEvent::SessionUsage`] arrives.
    fn render_usage_button(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(chat) = self.chat.as_ref() else {
            return div().into_any_element();
        };
        let session_id = chat.session_id.clone();
        let tx = self.event_tx.clone();
        let text = i18n::current(cx);
        Button::new("session-usage")
            .icon(IconName::ChartPie)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.usage_title.clone())
            .on_click(
                move |_: &gpui_kit::ClickEvent, _window: &mut Window, _cx: &mut App| {
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.session_usage(session_id.clone(), tx.clone());
                    }
                },
            )
            .into_any_element()
    }

    /// Model picker for the open session: the header shows the current model
    /// and the menu offers every enabled configured model. The selection is
    /// applied through the runtime (persisted per session), so the header only
    /// changes after [`RuntimeEvent::SessionModelChanged`] confirms it.
    fn render_model_picker(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.chat.as_ref() else {
            return div().into_any_element();
        };
        let session_id = chat.session_id.clone();
        let current = chat.model_id.clone();
        let catalog = cx
            .try_global::<runtime::ModelCatalog>()
            .cloned()
            .unwrap_or_default();
        let label = match current.as_deref() {
            Some(id) => catalog
                .models
                .iter()
                .find(|model| model.id == id)
                .map(|model| model.label())
                .unwrap_or_else(|| id.to_string()),
            None => text.model_default.clone(),
        };
        let options: Vec<(String, String, bool)> = catalog
            .models
            .iter()
            .filter(|model| model.enabled)
            .map(|model| {
                (
                    model.id.clone(),
                    model.label(),
                    current.as_deref() == Some(model.id.as_str()),
                )
            })
            .collect();
        let empty = options.is_empty();
        let tx = self.event_tx.clone();
        Button::new("chat-model-picker")
            .label(label)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .dropdown_caret(true)
            .disabled(empty)
            .tooltip(if empty {
                text.model_picker_empty.clone()
            } else {
                text.model_picker.clone()
            })
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                let mut menu = menu;
                for (id, option_label, checked) in &options {
                    let id = id.clone();
                    let tx = tx.clone();
                    let session_id = session_id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(option_label.clone())
                            .checked(*checked)
                            .on_click(move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.update_session_model(
                                        session_id.clone(),
                                        id.clone(),
                                        tx.clone(),
                                    );
                                }
                            }),
                    );
                }
                menu
            })
            .into_any_element()
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
/// binary entry before any logging happens. The filter is installed behind a
/// reload handle so `app.logging.level` applies live (M22 diagnostics page).
pub fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let (filter_layer, handle) =
        tracing_subscriber::reload::Layer::new(tracing_subscriber::EnvFilter::new(filter));
    let _ = LOG_FILTER.set(handle);
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .try_init();
}

/// Live handle onto the log filter; `L` is the inner filter type and `S` the
/// registry the layer is installed on. `reload` only swaps the filter.
type LogFilterHandle =
    tracing_subscriber::reload::Handle<tracing_subscriber::EnvFilter, tracing_subscriber::Registry>;
static LOG_FILTER: std::sync::OnceLock<LogFilterHandle> = std::sync::OnceLock::new();

/// Applies one `app.logging.level` value to the live filter. Before the
/// subscriber exists this is a no-op.
pub(crate) fn apply_log_level(level: &str) {
    if let Some(handle) = LOG_FILTER.get() {
        let _ = handle.reload(tracing_subscriber::EnvFilter::new(level.to_string()));
        tracing::info!("Log level applied: {level}");
    }
}

/// Whether the window close button quits (`true`) or minimizes (`false`),
/// mirrored from the shared close-button setting. Read by the
/// `on_window_should_close` callback, which has no cx access.
static CLOSE_QUITS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Re-reads the close-button setting into [`CLOSE_QUITS`]. Called when the
/// config snapshot lands and when the general settings page changes it.
pub(crate) fn sync_close_quits(cx: &App) {
    let raw = settings::ShellConfig::try_current(cx)
        .map(|config| config.close_behavior_raw)
        .unwrap_or_else(|| settings::CLOSE_QUIT.to_string());
    let quits = settings::parse_close_behavior(&raw) == settings::CloseBehavior::Quit;
    CLOSE_QUITS.store(quits, std::sync::atomic::Ordering::Relaxed);
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
                    // Glass sidebar: the design-system palettes paint the
                    // sidebar column at 90% chrome over the window surface, so
                    // the compositor blur shows through. Opaque themes (CLI
                    // presets keep the component-default sidebar) simply cover
                    // the surface; without blur support the 90% tint still
                    // carries the look.
                    window_background: WindowBackgroundAppearance::Blurred,
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
            model_id: None,
            entries,
            busy: false,
            submitting: false,
            child_context: None,
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
            subagent_session: None,
        };
        let mut chat = chat_with(Vec::new());
        assert!(chat.upsert_tool(card("tool-1")).is_none());
        assert!(chat.upsert_tool(card("tool-2")).is_none());
        assert_eq!(chat.upsert_tool(card("tool-1")), Some(0));
        assert!(chat.upsert_tool(card("tool-3")).is_none());
        assert_eq!(chat.entries.len(), 3);
    }

    #[test]
    fn btw_question_parses_the_command_prefix() {
        assert_eq!(btw_question("/btw what is this?"), Some("what is this?"));
        assert_eq!(btw_question("/btw"), Some(""));
        assert_eq!(btw_question("  /btw   spaced  "), Some("spaced"));
        // "/btwfoo" is a plain message, not the command.
        assert_eq!(btw_question("/btwfoo"), None);
        assert_eq!(btw_question("plain question"), None);
    }

    /// The M20 child-view loop projects onto the shell: a linked subagent
    /// annotates the parent tool card, the card entry point opens the child
    /// with a settled child context, and returning to the parent re-annotates
    /// the reloaded transcript from the link map. `/btw` rides the same path.
    #[gpui_kit::test]
    fn subagent_and_btw_child_views_project_onto_the_shell(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        let opened = |session_id: &str, name: &str, entries: Vec<ChatEntry>| {
            RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                session_id: session_id.into(),
                name: name.into(),
                agent_type: "Standard".into(),
                model_id: None,
                entries,
                busy: false,
            })
        };
        let parent_entries = || {
            vec![ChatEntry::Tool(crate::runtime::ToolCard {
                key: "tool-call-1".into(),
                tool_name: "task".into(),
                status: ToolStatus::Running,
                detail: String::new(),
                subagent_session: None,
            })]
        };

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(opened("parent", "Parent", parent_entries()), window, cx);
                // The runtime links the hidden child to that tool call.
                shell.apply_runtime_event(
                    RuntimeEvent::SubagentLinked {
                        parent_session_id: "parent".into(),
                        parent_tool_call_id: "call-1".into(),
                        subagent_session_id: "child-1".into(),
                        agent_type: Some("Explore".into()),
                    },
                    window,
                    cx,
                );
            });
        });
        window.update(|_, cx| {
            let shell = shell.read(cx);
            assert_eq!(shell.subagent_links.len(), 1);
            let chat = shell.chat.as_ref().unwrap();
            match &chat.entries[0] {
                ChatEntry::Tool(card) => {
                    assert_eq!(card.subagent_session.as_deref(), Some("child-1"))
                }
                _ => panic!("expected a tool card"),
            }
        });

        // The card entry point opens the child; the opened echo settles the
        // subagent child context onto the pane.
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_subagent_run("child-1".into(), "parent".into(), cx);
            });
        });
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(opened("child-1", "child", Vec::new()), window, cx);
            });
        });
        window.update(|_, cx| {
            let chat = shell.read(cx).chat.as_ref().unwrap();
            assert_eq!(chat.session_id, "child-1");
            let child = chat.child_context.as_ref().expect("child context set");
            assert_eq!(child.kind, ChildKind::Subagent);
            assert_eq!(child.agent_type.as_deref(), Some("Explore"));
        });

        // Returning to the parent re-annotates the reloaded card.
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.return_from_child(cx));
        });
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(opened("parent", "Parent", parent_entries()), window, cx);
            });
        });
        window.update(|_, cx| {
            let chat = shell.read(cx).chat.as_ref().unwrap();
            assert_eq!(chat.session_id, "parent");
            assert!(chat.child_context.is_none());
            match &chat.entries[0] {
                ChatEntry::Tool(card) => {
                    assert_eq!(card.subagent_session.as_deref(), Some("child-1"))
                }
                _ => panic!("expected a tool card"),
            }
        });

        // `/btw` rides the same child machinery with its own badge kind.
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::BtwStarted {
                        parent_session_id: "parent".into(),
                        child_session_id: "btw-1".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(opened("btw-1", "BTW", Vec::new()), window, cx);
            });
        });
        window.update(|_, cx| {
            let chat = shell.read(cx).chat.as_ref().unwrap();
            assert_eq!(chat.session_id, "btw-1");
            let child = chat.child_context.as_ref().expect("btw child context");
            assert_eq!(child.kind, ChildKind::Btw);
            assert_eq!(child.parent_session_id, "parent");
        });
    }

    #[test]
    fn list_sync_ranges_match_entry_counts() {
        assert_eq!(tail_changed_range(1), 0..1);
        assert_eq!(tail_changed_range(40), 39..40);
    }

    fn sample_usage_stats() -> openbitfun_services_core::token_usage::UsageStatistics {
        openbitfun_services_core::token_usage::UsageStatistics {
            total_requests: 3,
            total_tokens: 300,
            total_input_tokens: 200,
            total_output_tokens: 100,
            total_cached_tokens: 50,
            total_cache_write_tokens: 0,
            total_cache_reported_input_tokens: 200,
            by_model: Vec::new(),
            by_group: Vec::new(),
            by_endpoint: Vec::new(),
            trend: Vec::new(),
            granularity: openbitfun_services_core::token_usage::UsageGranularity::Day,
        }
    }

    /// M22 pump projections: the update state machine, the launch-at-login
    /// global, and the settings-page data events land on the window in the
    /// shape the general/usage/web-search pages render.
    #[gpui_kit::test]
    fn m22_settings_events_project_onto_the_window(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                // An offered release becomes Available; the install finishing
                // moves to Installed; a failed check surfaces its message.
                shell.apply_runtime_event(
                    RuntimeEvent::UpdateCheckFinished {
                        result: Ok(crate::update::UpdateCheckReport {
                            offered_version: Some("9.9.9".into()),
                            unsupported_reason: None,
                        }),
                    },
                    window,
                    cx,
                );
                assert_eq!(
                    shell.update_state,
                    crate::update::UpdateUiState::Available {
                        version: "9.9.9".into()
                    }
                );
                shell.apply_runtime_event(
                    RuntimeEvent::UpdateInstallFinished {
                        result: Ok("9.9.9".into()),
                    },
                    window,
                    cx,
                );
                assert_eq!(
                    shell.update_state,
                    crate::update::UpdateUiState::Installed {
                        version: "9.9.9".into()
                    }
                );
                shell.apply_runtime_event(
                    RuntimeEvent::UpdateCheckFinished {
                        result: Err("offline".into()),
                    },
                    window,
                    cx,
                );
                assert_eq!(
                    shell.update_state,
                    crate::update::UpdateUiState::Failed {
                        message: "offline".into()
                    }
                );

                // Launch-at-login state lands in its global for the toggle.
                shell.apply_runtime_event(RuntimeEvent::AutostartState(true), window, cx);
                assert!(
                    cx.try_global::<settings::AutostartState>()
                        .expect("autostart global")
                        .0
                );

                // The usage snapshot replaces the loading flag.
                shell.usage_loading = true;
                shell.apply_runtime_event(
                    RuntimeEvent::UsageReady(Ok(Box::new(sample_usage_stats()))),
                    window,
                    cx,
                );
                assert!(!shell.usage_loading);
                assert!(shell.usage_stats.is_some());

                // The credential status arrives with its provider attached.
                shell.apply_runtime_event(
                    RuntimeEvent::WebSearchStatus(Ok(("tavily".into(), true))),
                    window,
                    cx,
                );
                assert_eq!(shell.websearch_status, Some(("tavily".to_string(), true)));
            });
        });
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
                        Some(entry) => render_chat_item(&entry, &text, None, cx),
                        None => div().into_any_element(),
                    }
                },
            )
            .w(px(800.))
            .h(px(600.))
        }
    }

    #[gpui_kit::test]
    fn management_catalogs_project_onto_the_shell(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::SkillCatalog(crate::manage::SkillCatalog {
                        loaded: true,
                        skills: vec![crate::manage::SkillRow {
                            key: "user::demo".into(),
                            name: "Demo".into(),
                            description: "demo skill".into(),
                            project_level: false,
                            builtin: false,
                            shadowed: false,
                            disabled: true,
                        }],
                    }),
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::McpCatalog(crate::manage::McpCatalog {
                        loaded: true,
                        servers: vec![crate::manage::McpServerRow {
                            id: "demo".into(),
                            name: "Demo".into(),
                            remote: false,
                            target: "demo-server --stdio".into(),
                            state: crate::manage::McpServerState::Failed,
                            state_message: Some("spawn failed".into()),
                            enabled: true,
                            auto_start: true,
                            builtin: false,
                        }],
                    }),
                    window,
                    cx,
                );
                // A failure outside a dialog lands in the banner state.
                shell.apply_runtime_event(RuntimeEvent::ManageFailed("boom".into()), window, cx);
            });
        });
        window.update(|_, cx| {
            let skills = cx
                .try_global::<crate::manage::SkillCatalog>()
                .expect("skill catalog global");
            assert_eq!(skills.skills.len(), 1);
            assert!(skills.skills[0].disabled);

            let servers = cx
                .try_global::<crate::manage::McpCatalog>()
                .expect("mcp catalog global");
            assert_eq!(servers.servers.len(), 1);
            assert_eq!(
                servers.servers[0].state,
                crate::manage::McpServerState::Failed
            );

            assert!(crate::manage::ManageNotice::current(cx).is_some());
        });
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
                turn_id: None,
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
                turn_id: None,
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
            cx.set_global(sample_shell_text());
        });
        let window = cx.add_empty_window();
        let text = sample_shell_text();
        let (ttx, trx) = mpsc::unbounded_channel();
        let shell_view = window.update(|window, cx| {
            cx.new(|cx| ShellWindow::new(ttx, trx, window, cx))
                .downgrade()
        });
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                // The page must render inside a view: the component library
                // asks the window for the current view id during layout.
                cx.new(|_| SettingsHarness {
                    text: text.clone(),
                    view: shell_view.clone(),
                })
                .into_any_element()
            },
        );
        window.run_until_parked();
    }

    /// The models page must render the configured catalog (including disabled
    /// entries) without a live runtime.
    #[gpui_kit::test]
    fn settings_models_page_renders_the_catalog(cx: &mut gpui_kit::TestAppContext) {
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
            cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
            cx.set_global(crate::runtime::ModelCatalog {
                loaded: true,
                models: vec![
                    crate::runtime::ModelChoice {
                        id: "model-a".into(),
                        name: "Model A".into(),
                        provider: "Provider X".into(),
                        enabled: true,
                    },
                    crate::runtime::ModelChoice {
                        id: "model-b".into(),
                        name: "Model B".into(),
                        provider: "Provider Y".into(),
                        enabled: false,
                    },
                ],
            });
            cx.set_global(sample_shell_text());
        });
        let window = cx.add_empty_window();
        let text = sample_shell_text();
        // A real shell backs the weak handle the profile rows capture; the
        // harness layout never upgrades it.
        let (ttx, trx) = mpsc::unbounded_channel();
        let shell_view = window.update(|window, cx| {
            cx.new(|cx| ShellWindow::new(ttx, trx, window, cx))
                .downgrade()
        });
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                cx.new(|_| SettingsHarness {
                    text: text.clone(),
                    view: shell_view.clone(),
                })
                .into_any_element()
            },
        );
        window.run_until_parked();
    }

    struct SettingsHarness {
        text: ShellText,
        /// The settings page opens profile dialogs through the shell weak
        /// handle (D57); the harness never upgrades it, a shell-shaped
        /// placeholder is enough for layout.
        view: gpui_kit::WeakEntity<ShellWindow>,
    }

    impl Render for SettingsHarness {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            settings::render_page(&self.text, self.view.clone(), cx)
        }
    }

    /// The dock split must hand the body a real box whether or not the terminal
    /// is docked: a `ResizablePanelGroup` carries `size_full()` internally and
    /// collapses inside a parent whose height is only flex-derived, which is why
    /// the split lives in its own sized frame (same recipe as gpui-base's own
    /// dock area).
    #[gpui_kit::test]
    fn dock_split_keeps_the_body_sized(cx: &mut gpui_kit::TestAppContext) {
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
            cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
            cx.set_global(sample_shell_text());
        });
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
            let shell = shell.clone();
            move |_, _| shell.into_any_element()
        });
        window.run_until_parked();
        let body = window
            .debug_bounds("shell-body-sidebar")
            .unwrap_or_else(|| {
                panic!(
                    "body lays out with the terminal closed: frame {:?}",
                    window.debug_bounds("shell-dock-frame")
                )
            });
        assert!(body.size.height > px(300.), "body height: {:?}", body.size);
        assert!(body.size.width > px(200.), "body width: {:?}", body.size);
        assert!(window.debug_bounds("shell-terminal-dock").is_none());

        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.terminal_open = true;
                cx.notify();
            })
        });
        window.draw(
            point(px(0.), px(0.)),
            size(px(1200.), px(800.)),
            move |_, _| shell.into_any_element(),
        );
        window.run_until_parked();
        let docked = window
            .debug_bounds("shell-terminal-dock")
            .expect("terminal dock lays out once opened");
        assert_eq!(docked.size.height, TerminalPane::panel_height());
        let body = window
            .debug_bounds("shell-body-sidebar")
            .expect("body keeps its box with the terminal docked");
        assert!(body.size.height > px(300.), "body height: {:?}", body.size);
    }

    /// The changes view must render its failure state (no runtime attached in
    /// tests) instead of panicking on the missing git port.
    #[gpui_kit::test]
    fn git_panel_renders_without_a_runtime(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(sample_shell_text());
        });
        let window = cx.add_empty_window();
        let panel = window.update(|window, cx| cx.new(|cx| GitPanel::new(None, window, cx)));
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, _| panel.into_any_element(),
        );
        window.run_until_parked();
    }

    /// The M11 session-management events must project onto both surfaces: a
    /// rename re-labels the row and the open header, a delete closes the pane,
    /// and a failure surfaces instead of disappearing.
    #[gpui_kit::test]
    fn session_management_events_project_onto_the_shell(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::Sessions(vec![
                        crate::runtime::SessionListRow {
                            summary: sample_summary("s-1", "First"),
                            archived: false,
                        },
                        crate::runtime::SessionListRow {
                            summary: sample_summary("s-2", "Second"),
                            archived: true,
                        },
                    ]),
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                        session_id: "s-1".into(),
                        name: "First".into(),
                        agent_type: "Standard".into(),
                        model_id: Some("model-a".into()),
                        entries: Vec::new(),
                        busy: false,
                    }),
                    window,
                    cx,
                );
            });
        });

        let (rows, open_name, model) = window.update(|_, cx| {
            let shell = shell.read(cx);
            (
                shell.sessions.clone(),
                shell.chat.as_ref().map(|chat| chat.name.clone()),
                shell.chat.as_ref().and_then(|chat| chat.model_id.clone()),
            )
        });
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.archived));
        assert_eq!(open_name.as_deref(), Some("First"));
        assert_eq!(model.as_deref(), Some("model-a"));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::SessionRenamed {
                        session_id: "s-1".into(),
                        name: "Renamed".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::SessionModelChanged {
                        session_id: "s-1".into(),
                        model_id: "model-b".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::SessionActionFailed {
                        message: "delete: boom".into(),
                    },
                    window,
                    cx,
                );
            });
        });

        let (row_name, chat_name, model, chat_error) = window.update(|_, cx| {
            let shell = shell.read(cx);
            (
                shell
                    .sessions
                    .iter()
                    .find(|row| row.id == "s-1")
                    .map(|row| row.name.clone()),
                shell.chat.as_ref().map(|chat| chat.name.clone()),
                shell.chat.as_ref().and_then(|chat| chat.model_id.clone()),
                shell.chat_error.clone(),
            )
        });
        assert_eq!(row_name.as_deref(), Some("Renamed"));
        assert_eq!(chat_name.as_deref(), Some("Renamed"));
        assert_eq!(model.as_deref(), Some("model-b"));
        assert!(chat_error.is_some(), "a failed action must surface");

        // Deleting the open session closes the pane and drops the row.
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::SessionDeleted {
                        session_id: "s-1".into(),
                    },
                    window,
                    cx,
                );
            });
        });
        let (rows, chat_open) = window.update(|_, cx| {
            let shell = shell.read(cx);
            (shell.sessions.len(), shell.chat.is_some())
        });
        assert_eq!(rows, 1);
        assert!(!chat_open);
    }

    /// The turn echo stamps the fork boundary onto the user entry, and the
    /// compaction/export/usage events feed their dialogs and status lines.
    #[gpui_kit::test]
    fn turn_echo_and_maintenance_events_update_chat_state(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                        session_id: "s-1".into(),
                        name: "First".into(),
                        agent_type: "Standard".into(),
                        model_id: None,
                        entries: Vec::new(),
                        busy: false,
                    }),
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::TurnAccepted {
                        session_id: "s-1".into(),
                        turn_id: "turn-9".into(),
                        message: "do the thing".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::CompactionStarted {
                        session_id: "s-1".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::ExportFinished {
                        session_id: "s-1".into(),
                        path: "/tmp/export.md".into(),
                    },
                    window,
                    cx,
                );
            });
        });

        window.update(|_, cx| {
            let shell = shell.read(cx);
            let chat = shell.chat.as_ref().expect("chat opened");
            assert!(chat.busy, "compaction marks the session busy");
            let mut entries = chat.entries.iter();
            match entries.next_back() {
                Some(ChatEntry::Info { text }) => assert!(text.contains("/tmp/export.md")),
                other => panic!("expected export info line, got {other:?}"),
            }
            match entries.next_back() {
                Some(ChatEntry::Info { text }) => assert!(!text.is_empty()),
                other => panic!("expected compaction info line, got {other:?}"),
            }
            match entries.next_back() {
                Some(ChatEntry::User { turn_id, .. }) => {
                    assert_eq!(turn_id.as_deref(), Some("turn-9"));
                }
                other => panic!("expected user entry with turn id, got {other:?}"),
            }
            assert!(shell.usage_report.is_none(), "usage dialog stays closed");
        });
    }

    #[test]
    fn usage_summary_formats_tokens_and_durations() {
        assert_eq!(optional_tokens(Some(120)), "120");
        assert_eq!(optional_tokens(None), "—");
        assert_eq!(optional_duration(Some(1_500)), "1.5 s");
        assert_eq!(optional_duration(Some(61_000)), "1 m 1 s");
        assert_eq!(optional_duration(None), "—");
    }

    /// Insights and scheduled-jobs events project onto their page state
    /// without touching the chat surface.
    #[gpui_kit::test]
    fn insights_and_tasks_events_project_onto_page_state(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(RuntimeEvent::InsightsGenerating, window, cx);
                assert!(shell.insights.generating);
                shell.apply_runtime_event(
                    RuntimeEvent::InsightsFailed("insights: boom".into()),
                    window,
                    cx,
                );
                assert!(!shell.insights.generating);
                assert_eq!(shell.insights.error.as_deref(), Some("insights: boom"));
                shell.apply_runtime_event(RuntimeEvent::CronJobs(vec![]), window, cx);
                shell.apply_runtime_event(
                    RuntimeEvent::CronFailed("cron: boom".into()),
                    window,
                    cx,
                );
            });
        });

        window.update(|_, cx| {
            let shell = shell.read(cx);
            assert!(shell.tasks.loaded);
            assert_eq!(shell.tasks.error.as_deref(), Some("cron: boom"));
            assert!(shell.chat.is_none(), "page events never touch the chat");
        });
    }

    /// Archived rows stay out of the sidebar until the toggle is on, and the
    /// toggle itself never invents rows the runtime did not report.
    #[gpui_kit::test]
    fn archived_rows_hide_behind_the_toggle(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
        let text = sample_shell_text();

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::Sessions(vec![
                        crate::runtime::SessionListRow {
                            summary: sample_summary("s-1", "Active"),
                            archived: false,
                        },
                        crate::runtime::SessionListRow {
                            summary: sample_summary("s-2", "Archived"),
                            archived: true,
                        },
                    ]),
                    window,
                    cx,
                );
            });
        });

        let count = visible_session_rows(&mut window, &shell, &text);
        assert_eq!(count, 1);
        window.update(|_, cx| {
            shell.update(cx, |shell, _cx| {
                shell.show_archived = true;
            });
        });
        let count = visible_session_rows(&mut window, &shell, &text);
        assert_eq!(count, 2);
    }

    /// Number of session rows the sidebar would render with the current
    /// archive toggle.
    fn visible_session_rows(
        window: &mut gpui_kit::VisualTestContext,
        shell: &Entity<ShellWindow>,
        text: &ShellText,
    ) -> usize {
        let text = text.clone();
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .render_session_entries(&text, cx)
                    .into_iter()
                    .filter(|entry| matches!(entry, SidebarEntry::Session { .. }))
                    .count()
            })
        })
    }

    fn install_shell_globals(cx: &mut gpui_kit::TestAppContext) {
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
            cx.set_global(settings::ShellConfig::startup_default(LocaleId::ZhCN));
            cx.set_global(sample_shell_text());
        });
    }

    fn sample_summary(id: &str, name: &str) -> openbitfun_runtime_ports::AgentSessionSummary {
        openbitfun_runtime_ports::AgentSessionSummary {
            session_id: id.into(),
            session_name: name.into(),
            agent_type: "Standard".into(),
            model_id: None,
            reasoning_preset: None,
            last_user_dialog_agent_type: None,
            last_submitted_agent_type: None,
            turn_count: 0,
            created_at_ms: 0,
            last_active_at_ms: 0,
        }
    }

    /// One `ask_user` mailbox payload shaped like the tool's input plus the
    /// timing metadata the runtime adds.
    fn question_payload(questions: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "questions": questions,
            "responseHostNowMs": 1_700_000_000_000u64,
            "responseDeadlineMs": serde_json::Value::Null,
        })
    }

    fn sample_question(tool_id: &str, payload: serde_json::Value) -> PendingUserQuestion {
        PendingUserQuestion::new(
            tool_id,
            "session-1",
            Some("turn-1".to_string()),
            None,
            payload,
        )
    }

    /// Deterministic stand-in for the localized "n of m" label.
    fn test_question_label(index: usize, total: usize) -> String {
        format!("Question {index} / {total}")
    }

    fn two_choice_payload(multi_select: bool) -> serde_json::Value {
        question_payload(serde_json::json!([{
            "question": "Which path should be used?",
            "header": "Path",
            "options": [
                { "label": "A", "description": "Use A" },
                { "label": "B", "description": "Use B" }
            ],
            "multiSelect": multi_select
        }]))
    }

    #[test]
    fn question_payload_parses_into_render_items() {
        let item = compose_question_item(
            sample_question("tool-1", two_choice_payload(false)),
            test_question_label,
        );
        assert_eq!(item.tool_id, "tool-1");
        assert_eq!(item.questions.len(), 1);
        assert_eq!(item.questions[0].options.len(), 2);
        assert_eq!(item.questions[0].options[1].label, "B");
        assert!(!item.questions[0].multi_select);
        assert_eq!(item.questions[0].progress_line, "Question 1 / 1");
        assert!(!item.is_answered());
        // Timing metadata alongside the tool input is ignored.
        assert_eq!(item.selected.len(), 1);
    }

    #[test]
    fn question_payload_without_options_is_unanswerable() {
        for questions in [
            serde_json::json!([]),
            serde_json::json!([{ "question": "q", "header": "h", "options": [] }]),
            serde_json::json!([{ "question": "q", "header": "h" }]),
        ] {
            let item = compose_question_item(
                sample_question("tool-1", question_payload(questions)),
                test_question_label,
            );
            assert!(item.questions.is_empty(), "batch must stay cancel-only");
            assert!(!item.is_answered());
            assert!(item.answers().as_object().is_some_and(|map| map.is_empty()));
        }
    }

    #[test]
    fn question_answers_follow_the_tool_wire_shape() {
        let mut item = compose_question_item(
            sample_question("tool-1", two_choice_payload(false)),
            test_question_label,
        );
        toggle_option_selection(&mut item.selected[0], 1, false);
        assert!(item.is_answered());
        assert_eq!(item.answers(), serde_json::json!({ "0": "B" }));

        // Multi-select answers are arrays of labels, and unanswered questions
        // are omitted.
        let multi = question_payload(serde_json::json!([
            {
                "question": "Which ones?",
                "header": "Pick",
                "options": [
                    { "label": "X", "description": "x" },
                    { "label": "Y", "description": "y" }
                ],
                "multiSelect": true
            },
            {
                "question": "And one?",
                "header": "One",
                "options": [
                    { "label": "P", "description": "p" },
                    { "label": "Q", "description": "q" }
                ]
            }
        ]));
        let mut item = compose_question_item(sample_question("tool-1", multi), test_question_label);
        toggle_option_selection(&mut item.selected[0], 0, true);
        toggle_option_selection(&mut item.selected[0], 1, true);
        assert!(!item.is_answered(), "the second question has no selection");
        assert_eq!(item.answers(), serde_json::json!({ "0": ["X", "Y"] }));
    }

    #[test]
    fn question_option_selection_toggles_or_replaces() {
        let mut selected = BTreeSet::new();
        toggle_option_selection(&mut selected, 0, false);
        toggle_option_selection(&mut selected, 1, false);
        assert_eq!(selected.iter().copied().collect::<Vec<_>>(), vec![1]);

        let mut selected = BTreeSet::new();
        toggle_option_selection(&mut selected, 0, true);
        toggle_option_selection(&mut selected, 1, true);
        assert_eq!(selected.iter().copied().collect::<Vec<_>>(), vec![0, 1]);
        toggle_option_selection(&mut selected, 0, true);
        assert_eq!(selected.iter().copied().collect::<Vec<_>>(), vec![1]);
    }

    /// The question queue is rebuilt from the mailbox snapshot on every
    /// change: in-progress selections survive an unrelated question arriving
    /// or leaving, resolved questions drop out, and the dialog slot follows
    /// the queue.
    #[test]
    fn question_queue_rebuild_keeps_selections_and_drops_resolved() {
        let mut head = compose_question_item(
            sample_question("tool-1", two_choice_payload(false)),
            test_question_label,
        );
        toggle_option_selection(&mut head.selected[0], 1, false);
        let existing = vec![head];

        let rebuilt = rebuild_question_queue(
            &existing,
            vec![
                sample_question("tool-1", two_choice_payload(false)),
                sample_question("tool-2", two_choice_payload(true)),
            ],
            test_question_label,
        );
        assert_eq!(rebuilt.len(), 2);
        assert!(rebuilt[0].is_answered());
        assert_eq!(
            rebuilt[0].selected[0].iter().copied().collect::<Vec<_>>(),
            vec![1]
        );
        assert!(!rebuilt[1].is_answered());

        let rebuilt = rebuild_question_queue(
            &rebuilt,
            vec![sample_question("tool-2", two_choice_payload(true))],
            test_question_label,
        );
        assert_eq!(rebuilt.len(), 1);
        assert_eq!(rebuilt[0].tool_id, "tool-2");

        assert!(rebuild_question_queue(&rebuilt, Vec::new(), test_question_label).is_empty());
    }

    /// A workspace switch resets every workspace-scoped surface: the open
    /// transcript, the session list (and its archive toggle), the terminal,
    /// the changes pane, and the error lines — while a failure keeps the
    /// previous workspace active and surfaces the reason.
    #[gpui_kit::test]
    fn workspace_switch_resets_scoped_surfaces(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::Sessions(vec![crate::runtime::SessionListRow {
                        summary: sample_summary("s-1", "First"),
                        archived: false,
                    }]),
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                        session_id: "s-1".into(),
                        name: "First".into(),
                        agent_type: "Standard".into(),
                        model_id: None,
                        entries: Vec::new(),
                        busy: false,
                    }),
                    window,
                    cx,
                );
                shell.show_archived = true;
                shell.list_error = Some("stale".into());
                shell.toggle_files_view(cx);
            });
            inject_terminal_tab(&shell, "old-ws", window, cx);
        });
        window.update(|_, cx| {
            let shell = shell.read(cx);
            assert!(shell.chat.is_some());
            assert_eq!(shell.sessions.len(), 1);
        });

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::WorkspaceSwitched(WorkspaceChoice {
                        id: "ws-2".into(),
                        name: "Second".into(),
                        root_path: "/tmp/ws-2".into(),
                    }),
                    window,
                    cx,
                );
            });
        });
        window.update(|_, cx| {
            let shell = shell.read(cx);
            assert!(
                shell.chat.is_none(),
                "transcript belongs to the old workspace"
            );
            assert!(
                shell.sessions.is_empty(),
                "session list reloads for the new root"
            );
            assert!(!shell.show_archived, "archive filter resets with the list");
            assert!(shell.list_error.is_none());
            assert!(
                shell.terminals.is_empty(),
                "terminal PTYs shut down with the old workspace"
            );
            assert!(!shell.terminal_open);
            assert!(shell.git.is_none());
            assert!(shell.files.is_none(), "files page binds the old workspace");
            assert_eq!(shell.main_view, MainView::Chat);
            assert!(shell.workspace_error.is_none());
        });

        // A rejected switch keeps the current workspace and shows the reason.
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::WorkspaceSwitchFailed("owned by a shared runtime".into()),
                    window,
                    cx,
                );
            });
        });
        window.update(|_, cx| {
            assert_eq!(
                shell.read(cx).workspace_error.as_deref(),
                Some("owned by a shared runtime")
            );
        });
    }

    /// The files page toggles from the footer and its entity (open tabs
    /// included) survives switching back to chat; without a runtime the page
    /// degrades loudly instead of rendering a dead tree.
    #[gpui_kit::test]
    fn files_view_toggles_and_survives_view_switches(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.toggle_files_view(cx);
                assert_eq!(shell.main_view, MainView::Files);
                assert!(shell.files.is_some());
            });
        });
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                let page = shell.files.as_ref().unwrap().read(cx);
                assert!(page.error().is_some(), "no runtime here: degrade loudly");
            });
            shell.update(cx, |shell, cx| {
                shell.toggle_files_view(cx);
                assert_eq!(shell.main_view, MainView::Chat);
                assert!(shell.files.is_some(), "tabs survive the view switch");

                shell.toggle_files_view(cx);
                assert_eq!(shell.main_view, MainView::Files);
            });
        });
    }

    /// A process-lifetime tokio runtime for pane construction in headless
    /// tests: `TerminalPane::new` spawns its PTY pump on the given handle, so
    /// the runtime must outlive the test (`pty_integration` drives the same
    /// service directly).
    fn test_terminal_tokio() -> tokio::runtime::Handle {
        static RT: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();
        RT.get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("test terminal runtime");
            let handle = runtime.handle().clone();
            std::mem::forget(runtime);
            handle
        })
        .clone()
    }

    /// Injects one real terminal tab (spawned bash) into the shell; tab
    /// lifecycle tests then run the ordinary close path end to end.
    fn inject_terminal_tab(
        shell: &Entity<ShellWindow>,
        label: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        let settings = settings::ShellConfig::try_current(cx)
            .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale))
            .terminal;
        let pane = cx
            .new(|cx| TerminalPane::new(test_terminal_tokio(), &settings, None, None, window, cx));
        shell.update(cx, |shell, _| {
            shell.terminals.push(TerminalTab {
                pane,
                label: SharedString::from(label.to_string()),
            });
        });
    }

    /// Terminal tab lifecycle: closing the active tab moves focus to a
    /// neighbor, and closing the last tab closes the whole panel (each close
    /// shuts the tab's real PTY down).
    #[gpui_kit::test]
    fn terminal_tabs_close_adjusts_active_and_last_closes_panel(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            inject_terminal_tab(&shell, "one", window, cx);
            inject_terminal_tab(&shell, "two", window, cx);
            shell.update(cx, |shell, cx| {
                shell.terminal_open = true;
                shell.active_terminal = 1;
                assert_eq!(shell.terminals.len(), 2);
            });
        });
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.close_terminal_tab(1, cx);
                assert_eq!(shell.terminals.len(), 1);
                assert_eq!(shell.active_terminal, 0, "focus moves to the neighbor");
                assert!(shell.terminal_open);
            });
        });
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.close_terminal_tab(0, cx);
                assert!(shell.terminals.is_empty());
                assert!(!shell.terminal_open, "the last close closes the panel");
            });
        });
        // The tabbed layout must render: the tab bar, the new-tab menu
        // button, and the active pane all lay out inside the dock.
        window.update(|window, cx| {
            inject_terminal_tab(&shell, "again", window, cx);
            shell.update(cx, |shell, cx| shell.terminal_open = true);
        });
        window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
            let shell = shell.clone();
            move |_, _| shell.into_any_element()
        });
        window.run_until_parked();
        let dock = window
            .debug_bounds("shell-terminal-dock")
            .expect("docked terminal renders with tabs");
        assert!(dock.size.height > px(50.), "dock height: {:?}", dock.size);
    }

    /// Terminal profiles flow through the shared mirror: an empty id is
    /// assigned on insert, the same id replaces, delete removes.
    #[gpui_kit::test]
    fn terminal_profiles_upsert_and_delete_flow_through_the_mirror(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        install_shell_globals(cx);
        cx.update(|cx| {
            let profile = |name: &str| openbitfun_core::service::config::types::TerminalProfile {
                id: String::new(),
                name: name.to_string(),
                working_directory: "/work".to_string(),
                startup_command: "just dev".to_string(),
            };
            settings::upsert_terminal_profile(profile("dev"), cx);
            let profiles = &settings::ShellConfig::current(cx).terminal.profiles;
            assert_eq!(profiles.len(), 1);
            assert!(!profiles[0].id.is_empty(), "insert assigns an id");
            let id = profiles[0].id.clone();

            settings::upsert_terminal_profile(
                openbitfun_core::service::config::types::TerminalProfile {
                    id: id.clone(),
                    name: "dev2".to_string(),
                    working_directory: String::new(),
                    startup_command: String::new(),
                },
                cx,
            );
            let profiles = &settings::ShellConfig::current(cx).terminal.profiles;
            assert_eq!(profiles.len(), 1, "same id replaces");
            assert_eq!(profiles[0].name, "dev2");

            settings::delete_terminal_profile(&id, cx);
            assert!(settings::ShellConfig::current(cx)
                .terminal
                .profiles
                .is_empty());
        });
    }

    /// Search page state machine: toggle, tab switch, and the pump
    /// projections for both tabs plus the unavailable gate.
    #[gpui_kit::test]
    fn search_view_toggles_and_projects_results(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.toggle_search_view(cx);
                assert_eq!(shell.main_view, MainView::Search);

                shell.apply_runtime_event(
                    RuntimeEvent::SearchContent {
                        hits: vec![sample_workspace_hit()],
                        truncated: true,
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.search.content.len(), 1);
                assert_eq!(shell.search.content[0].path, "/w/a.rs");
                assert!(shell.search.truncated);

                shell.search.tab = crate::search::SearchTab::Sessions;
                shell.apply_runtime_event(
                    RuntimeEvent::SearchSessions {
                        hits: vec![sample_session_hit()],
                        truncated: false,
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.search.sessions.len(), 1);
                assert_eq!(shell.search.sessions[0].session_id, "s-9");

                shell.apply_runtime_event(RuntimeEvent::SearchUnavailable(None), window, cx);
                assert!(matches!(shell.search.unavailable, Some(None)));

                shell.toggle_search_view(cx);
                assert_eq!(shell.main_view, MainView::Chat);
            });
        });
    }

    /// Search page state machine fixtures: one grouped content hit and one
    /// session hit, shaped like the wire DTOs.
    fn sample_workspace_hit() -> openbitfun_core::service::search::WorkspaceSearchHit {
        openbitfun_core::service::search::WorkspaceSearchHit {
            path: "/w/a.rs".to_string(),
            matches: vec![openbitfun_core::service::search::WorkspaceSearchMatch {
                location: openbitfun_core::service::search::WorkspaceSearchMatchLocation {
                    line: 3,
                    column: 0,
                },
                snippet: "let x = query;".to_string(),
                matched_text: "query".to_string(),
            }],
            lines: Vec::new(),
        }
    }

    fn sample_session_hit() -> openbitfun_runtime_ports::SessionSearchHit {
        openbitfun_runtime_ports::SessionSearchHit {
            kind: openbitfun_runtime_ports::SessionSearchHitKind::Session,
            matched_field: openbitfun_runtime_ports::SessionSearchMatchField::Title,
            session_id: "s-9".to_string(),
            session_title: "Indexed session".to_string(),
            turn_id: None,
            turn_index: None,
            snippet: "answer text".to_string(),
            archived: false,
            updated_at_ms: 0,
            score: 1.0,
        }
    }

    /// Approvals gate a parked tool call, so they take the single dialog slot
    /// ahead of questions; the slot is empty once both queues drain.
    #[test]
    fn approvals_take_the_modal_slot_before_questions() {
        assert_eq!(next_modal_target(0, 0), None);
        assert_eq!(next_modal_target(1, 0), Some(ModalOwner::Permissions));
        assert_eq!(next_modal_target(0, 1), Some(ModalOwner::Questions));
        assert_eq!(next_modal_target(2, 3), Some(ModalOwner::Permissions));
    }

    #[test]
    fn image_formats_resolve_from_safe_extensions_only() {
        assert_eq!(image_format_for_extension("png"), Some(ImageFormat::Png));
        assert_eq!(image_format_for_extension("JPEG"), Some(ImageFormat::Jpeg));
        assert_eq!(image_format_for_extension("webp"), Some(ImageFormat::Webp));
        assert_eq!(image_format_for_extension("svg"), None);
        assert_eq!(image_format_for_extension("exe"), None);
    }

    #[test]
    fn attachment_payload_carries_a_self_contained_data_url() {
        let item = PendingAttachment {
            id: "att-1".to_string(),
            name: "shot.png".to_string(),
            mime: "image/png".to_string(),
            data_url: "data:image/png;base64,aGVsbG8=".to_string(),
            preview: Arc::new(Image::empty()),
        };
        let attachment = item.to_input_attachment();
        assert_eq!(attachment.kind, "remote_image");
        assert_eq!(attachment.id, "att-1");
        assert_eq!(
            attachment.metadata.get("dataUrl"),
            Some(&serde_json::json!("data:image/png;base64,aGVsbG8="))
        );
        // No controller-local path ever crosses the wire: peer and remote
        // runtimes decode the embedded bytes instead.
        assert!(attachment.metadata.get("imagePath").is_none());
        assert_eq!(
            attachment.metadata.get("metadata"),
            Some(&serde_json::json!({ "name": "shot.png" }))
        );
    }

    /// M23 sidebar nav: the row set is fixed and exactly one row lights up
    /// per surface (Settings activates none — its own footer button keeps the
    /// raised variant).
    #[test]
    fn nav_rows_map_active_state_to_main_views() {
        let actions = nav_actions();
        assert_eq!(
            actions,
            vec![
                NavAction::Chat,
                NavAction::Files,
                NavAction::Git,
                NavAction::Search,
                NavAction::Insights,
                NavAction::Tasks,
            ]
        );
        for view in [
            MainView::Chat,
            MainView::Files,
            MainView::Git,
            MainView::Search,
            MainView::Insights,
            MainView::Tasks,
            MainView::Settings,
        ] {
            let active = actions
                .iter()
                .filter(|candidate| candidate.is_active(view))
                .count();
            assert_eq!(
                active,
                if view == MainView::Settings { 0 } else { 1 },
                "view {view:?} must light exactly one nav row"
            );
        }
    }
}
