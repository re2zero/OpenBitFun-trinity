use crate::{i18n, runtime, settings, theme};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::resizable::{resizable_panel, v_resizable, ResizableState};
use gpui_kit::component::{
    bubble::{Bubble, BubbleVariant},
    button::{Button, ButtonVariant, ButtonVariants},
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    message::MessageAlignment,
    message_scroller::{MessageScroller, MessageScrollerState},
    scroll::ScrollableElement,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarItem},
    text::TextView,
    ActiveTheme, Collapsible, Disableable, Icon, Sizable, StyledExt, TitleBar, WindowExt,
};
use gpui_kit::{
    div, img, point, prelude::*, px, size, Anchor, App, Bounds, ClipboardEntry, Context, ElementId,
    Entity, ExternalPaths, Image, ImageFormat, Pixels, SharedString, SystemNotification,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions,
};
use openbitfun_agent_runtime::sdk::{PendingUserQuestion, PermissionReply, PermissionRequest};
use openbitfun_runtime_ports::AgentInputAttachment;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::assets::ShellAssets;
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
    /// Persisted archive state (from the session metadata store, not a local
    /// flag): archived rows are hidden until the "show archived" toggle is on.
    archived: bool,
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

/// One `Sidebar` content entry: a session row, the empty-state line, or the
/// list error line. One type because `Sidebar` children share one element
/// type; collapsed-state is inert because entries are always rendered.
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
                let menu =
                    session_row_menu(id.clone(), name.clone(), archived, view.clone(), tx.clone());
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

/// Per-row actions menu: rename, archive toggle, delete. A Button (whose press
/// stops propagation) keeps the row's own click-to-open handler from firing
/// when the menu trigger is used.
fn session_row_menu(
    session_id: String,
    name: String,
    archived: bool,
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
            menu.item(
                PopupMenuItem::new(text.session_rename.clone()).on_click(rename_action.clone()),
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
    /// `ask_user` questions waiting for an answer, oldest first. Same
    /// no-session-filter rule as approvals: a hidden question is a parked
    /// turn.
    pending_questions: Vec<QuestionPromptItem>,
    /// Which shell-owned modal owns the single dialog slot; `None` while no
    /// modal is open.
    modal_owner: Option<ModalOwner>,
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
            pending_questions: Vec::new(),
            modal_owner: None,
            terminal: None,
            terminal_open: false,
            terminal_auto_open: std::env::var_os("OPENBITFUN_GPUI_TERMINAL_AUTOOPEN").is_some(),
            main_view: MainView::Chat,
            git: None,
            dock,
            terminal_height: TerminalPane::panel_height(),
            settings_auto_open: std::env::var_os("OPENBITFUN_GPUI_SETTINGS_AUTOOPEN").is_some(),
            git_auto_open: std::env::var_os("OPENBITFUN_GPUI_GIT_AUTOOPEN").is_some(),
            show_archived: false,
            rename_input,
            rename_target: None,
            workspaces: Vec::new(),
            workspace_input,
            workspace_error: None,
            pending_attachments: Vec::new(),
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
                            model_id: opened.model_id,
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
            RuntimeEvent::QuestionsChanged(questions) => {
                self.apply_questions_changed(questions, window, cx);
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
    /// pane (and its PTY); close shuts the PTY down and drops the pane.
    fn toggle_terminal_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_open {
            self.terminal_open = false;
            // Remember the dragged height so the next open docks at it instead
            // of the default; the second slot is the terminal panel.
            if let Some(size) = self.dock.read(cx).sizes().get(1) {
                self.terminal_height =
                    (*size).clamp(px(MIN_TERMINAL_HEIGHT), px(MAX_TERMINAL_HEIGHT));
            }
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
            // The pane starts from the shared terminal settings: the configured
            // shell command and scrollback are read here, font preferences are
            // also re-applied on every render below.
            let terminal = settings::ShellConfig::try_current(cx)
                .unwrap_or_else(|| settings::ShellConfig::startup_default(i18n::current(cx).locale))
                .terminal;
            // The PTY starts in the active workspace, so the terminal follows
            // a workspace switch instead of the process launch directory.
            let start_dir = runtime::runtime_handle()
                .map(|handle| handle.workspace().root_path.to_string_lossy().to_string());
            self.terminal =
                Some(cx.new(|cx| TerminalPane::new(tokio, &terminal, start_dir, window, cx)));
        }
        cx.notify();
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
        self.terminal = None;
        self.terminal_open = false;
        self.git = None;
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
            entries.push(SidebarEntry::Muted(text.sessions_empty.clone().into()));
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

        Sidebar::new("shell-sidebar")
            .w(px(SESSION_PANE_WIDTH))
            .header(header)
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
            .child(self.render_workspace_button(text, cx))
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
                    if body.trim().is_empty() && !has_attachments {
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
                    self.toggle_git_view(cx);
                }
            }
        }

        // Terminal font preferences apply to a live pane: the paint pass
        // remeasures the cell and reflows the grid (and the PTY) when they land.
        // The settings page handlers only get `&mut App`, so the window is what
        // pushes the shared config into the pane; the call is a no-op unless a
        // value actually changed.
        if let Some(pane) = self.terminal.clone() {
            let terminal = settings::ShellConfig::try_current(cx)
                .unwrap_or_else(|| settings::ShellConfig::startup_default(text.locale))
                .terminal;
            pane.update(cx, |pane, cx| pane.apply_font(&terminal, cx));
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
                                .child(div().flex_1().min_h_0().min_w_0().flex().child(main_area)),
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
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
            model_id: None,
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
        });
        let window = cx.add_empty_window();
        let text = sample_shell_text();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| cx.new(|_| SettingsHarness { text }).into_any_element(),
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
        let panel = cx.new(GitPanel::new);
        let window = cx.add_empty_window();
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
            });
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
            assert!(shell.terminal.is_none());
            assert!(!shell.terminal_open);
            assert!(shell.git.is_none());
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
}
