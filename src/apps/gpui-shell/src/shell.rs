use crate::{i18n, runtime, settings, theme};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable, ResizableState};
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::{
    bubble::{Bubble, BubbleContent, BubbleVariant},
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariant, ButtonVariants},
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    message_scroller::{MessageScroller, MessageScrollerState},
    scroll::ScrollableElement,
    shimmer::ShimmerText,
    sidebar::{Sidebar, SidebarFooter, SidebarItem},
    text::TextView,
    ActiveTheme, Collapsible, Disableable, Icon, Sizable, StyledExt, TitleBar, WindowExt,
};
use gpui_kit::InteractiveElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::{
    div, img, point, prelude::*, px, size, Anchor, App, Bounds, ClipboardEntry, ClipboardItem,
    Context, ElementId, Entity, ExternalPaths, FontWeight, Image, ImageFormat, KeyBinding, Pixels,
    SharedString, StyleRefinement, SystemNotification, TitlebarOptions, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowOptions,
};
use openbitfun_agent_runtime::sdk::{PendingUserQuestion, PermissionReply, PermissionRequest};
use openbitfun_runtime_ports::{AgentInputAttachment, AgentSessionLineageSnapshot};
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
/// Height of the two top chrome bars (nav top bar over the sidebar column,
/// scene top bar over the scene face). Web parity: the 45px
/// `--openbitfun-layout-toolbar-md-height` both `NavBar` and `SceneTopBar`
/// use after the desktop removed its standalone title bar (`92ce04f8d`).
const TOP_BAR_HEIGHT: f32 = 45.;
/// Width of the floating collapsed-nav strip (web `$_nav-collapsed-width`):
/// the small opener that survives at the window's top-left corner while the
/// sidebar column is folded away.
const NAV_COLLAPSED_STRIP_WIDTH: f32 = 76.;
/// Root sessions shown before the show-more fold (R1c); the web nav's
/// collapsed level-0 page. Expanding reveals the rest, virtualized.
const SESSIONS_COLLAPSED_VISIBLE: usize = 5;
/// Transcript reading column; long-window content stays centered on it.
/// Web parity: `--openbitfun-control-flow-chat-transcript-max-width`.
const READING_COLUMN_WIDTH: f32 = 900.;
/// User bubble width cap, so short messages do not stretch across the
/// reading column. Web parity: the user-message `max-width: min(72%, 48rem)`
/// — at the 900px column that is 648px, below the 768px rem cap, so a
/// definite pixel cap is exact (a relative() max-width does not constrain
/// content inside the virtualized row; measured 2026-10-12).
const USER_BUBBLE_MAX_WIDTH: f32 = 648.;
/// User bubble surface shape (web `UserMessageItem`: radius 10px, inline
/// padding equal to the radius, ~7px block padding).
const USER_BUBBLE_RADIUS: f32 = 10.;
const USER_BUBBLE_PADDING_X: f32 = 10.;
const USER_BUBBLE_PADDING_Y: f32 = 7.;
/// Gap before a user prompt (the turn boundary) and after it; the transcript
/// rows in between keep a tighter rhythm. Web parity: turn-gap + space-4 on
/// the user item, `flow-item-gap` under it, ~12px between flow items.
const TURN_GAP: f32 = 32.;
const FLOW_ITEM_GAP: f32 = 8.;
const TURN_RHYTHM_GAP: f32 = 12.;
/// Composer height; explicit pixels because an auto-grown textarea makes the
/// docked composer row jump while streaming text is typed.
const COMPOSER_HEIGHT: f32 = 88.;
/// Bounds of the docked terminal panel, dragged by the divider above it.
const MIN_TERMINAL_HEIGHT: f32 = 120.;
const MAX_TERMINAL_HEIGHT: f32 = 900.;
/// Bounds of the session's right resource panel (R2c), dragged by the divider
/// left of it; `DEFAULT` is the pre-drag width.
const MIN_RESOURCE_PANEL_WIDTH: f32 = 240.;
const MAX_RESOURCE_PANEL_WIDTH: f32 = 720.;
const DEFAULT_RESOURCE_PANEL_WIDTH: f32 = 360.;

/// Which surface the main content area shows. The shell's main area is a tab
/// bar of session tabs and scene tabs (R1a, mirroring the web-ui desktop
/// `SceneBar`); the tab model lives in [`crate::tabs`].
use crate::tabs::{ResourcePanelKind, SceneId, TabKind, TabsState};

gpui_kit::actions!(openbitfun_gpui_shell, [OpenGlobalSearch, ToggleSidebar]);

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

/// One open session in the chat pane; one per session tab (R1a multi-tab).
struct ChatView {
    session_id: String,
    name: String,
    agent_type: String,
    /// The session's current model selector; `None` means the runtime default.
    model_id: Option<String>,
    entries: Vec<ChatEntry>,
    /// Virtualized transcript state for this tab (the scroller moved here from
    /// the window when sessions became tabs: each tab follows its own scroll
    /// position, and background sessions skip live syncs while hidden).
    scroller: Entity<MessageScrollerState>,
    /// A turn is executing (streaming or tool calls in flight).
    busy: bool,
    /// A submission is in flight; the optimistic user entry awaits
    /// [`RuntimeEvent::TurnAccepted`].
    submitting: bool,
    /// The pane shows a hidden child session (subagent run or `/btw` side
    /// question, M20): the header grows its badge and the "back to parent"
    /// entry point. `None` = a normal, user-visible session.
    child_context: Option<ChildContext>,
    /// User prompts expanded past the three-line clamp (R2d), keyed by the
    /// content hash (`markdown_id_key`) so the flag survives transcript
    /// reloads that re-index entries.
    expanded_user_texts: BTreeSet<u64>,
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
    /// The coordinator dialog turn of a `/btw` child, the stop button's
    /// target; `None` for subagent runs, which cancel through the session
    /// loop.
    pub btw_turn_id: Option<String>,
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

/// Which review flavor a linked subagent belongs to (T2-6, the web review
/// badge): deep review runs report `DeepReview`, the plain review family
/// (worker/judge/fixer, `CodeReview`, and the legacy `Review*` ids) reports
/// its own type. Web derives this from a client-set session kind; the shell
/// classifies the agent type the runtime already reports on the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReviewActivityKind {
    Review,
    DeepReview,
}

fn review_activity_kind(agent_type: &str) -> Option<ReviewActivityKind> {
    if agent_type == "DeepReview" {
        Some(ReviewActivityKind::DeepReview)
    } else if agent_type.starts_with("Review") || agent_type == "CodeReview" {
        Some(ReviewActivityKind::Review)
    } else {
        None
    }
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
    fn new(
        session_id: String,
        name: String,
        agent_type: String,
        model_id: Option<String>,
        entries: Vec<ChatEntry>,
        cx: &mut Context<ShellWindow>,
    ) -> Self {
        let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));
        Self {
            session_id,
            name,
            agent_type,
            model_id,
            entries,
            scroller,
            busy: false,
            submitting: false,
            child_context: None,
            expanded_user_texts: BTreeSet::new(),
        }
    }

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

/// Sidebar navigation destinations (R1b): the desktop top-scene rows in
/// NavPanel order. The workbench scenes (files/git/insights) have no nav row
/// on the desktop either — they open through the global search palette's
/// quick actions until R2c re-homes them as workbench panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavAction {
    Chat,
    Trinity,
    Tasks,
}

impl NavAction {
    fn icon(self) -> IconName {
        match self {
            NavAction::Chat => IconName::Sparkles,
            NavAction::Trinity => IconName::BrainCircuit,
            NavAction::Tasks => IconName::Calendar,
        }
    }

    /// Whether the row is the active surface. Sessions resolve through the
    /// tab bar (any open session tab makes Chat the active nav row); a scene
    /// row is active while its scene tab is the active tab.
    fn is_active(self, tabs: &TabsState) -> bool {
        match self {
            NavAction::Chat => tabs
                .active_kind()
                .is_none_or(|kind| matches!(kind, TabKind::Session(_))),
            NavAction::Trinity => tabs.active_kind() == Some(&TabKind::Scene(SceneId::Trinity)),
            NavAction::Tasks => tabs.active_kind() == Some(&TabKind::Scene(SceneId::Tasks)),
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

/// All nav rows in display order (desktop NavPanel top-scene order); a pure
/// projection so tests can pin the active-state wiring without a window.
fn nav_actions() -> Vec<NavAction> {
    vec![NavAction::Chat, NavAction::Trinity, NavAction::Tasks]
}

/// Row label for a nav destination: the desktop nav nouns (智能助理/认知体/
/// 任务看板), not the scene titles the tabs show.
fn nav_label<'a>(action: NavAction, text: &'a ShellText) -> &'a str {
    match action {
        NavAction::Chat => &text.nav_assistant,
        NavAction::Trinity => &text.trinity_title,
        NavAction::Tasks => &text.nav_tasks,
    }
}

/// One `Sidebar` content entry: a session row (root or indented child, R1c),
/// a navigation row, the "show more" fold row, the empty-state line, or the
/// list error line. One type because `Sidebar` children share one element
/// type; collapsed-state is inert because entries are always rendered.
#[derive(Clone)]
enum SidebarEntry {
    Session {
        row: SessionRow,
        open: bool,
        /// Tree depth (R1c): 0 = workspace top-level session, 1 = subagent
        /// child indented under its root. Deeper lineage descendants stay
        /// hidden, matching the web nav's two-level projection.
        depth: u8,
        /// A turn is executing for this session (process-wide live set, so
        /// background sessions light up too).
        running: bool,
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
    More {
        /// Hidden root sessions the expansion would reveal.
        remaining: usize,
        expanded: bool,
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
                depth,
                running,
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
                // Background subagent activity (T2-6, the web
                // backgroundSubagentActivity + review badge pair): a count
                // pill while hidden subagent turns run, and the review label
                // when one of them is a review-family run.
                let (background_count, review_kind) = view
                    .upgrade()
                    .map(|shell| shell.read(cx).session_activity_badges(&id))
                    .unwrap_or((0, None));
                let text = i18n::current(cx);
                let activity_badges = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .flex_shrink_0()
                    .when_some(review_kind, |this, kind| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().accent)
                                .child(text.sidebar_review_running.clone()),
                        )
                    })
                    .when(background_count > 0, |this| {
                        this.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_0p5()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(Icon::new(IconName::Bot))
                                .child(background_count.to_string()),
                        )
                    });
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
                    .px_2()
                    .h(px(30.))
                    // Subagent children (R1c) indent one step under their
                    // root; the web nav nests one level the same way.
                    .when(depth > 0, |this| this.ml_4())
                    .rounded(cx.theme().radius)
                    // Web SessionsSection final layer: hover lights the
                    // neutral surface, the open row the selection surface —
                    // two different tokens, so a hovered row next to the open
                    // one never reads as two selections.
                    .when(open, |this| this.bg(cx.theme().selection))
                    .when(!open, |this| this.hover(|style| style.bg(cx.theme().muted)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(cx.theme().sidebar_foreground)
                            .truncate()
                            .when(archived, |this| this.opacity(0.6))
                            .child(name),
                    )
                    // Subagent children keep their kind as an inline badge
                    // (the web row's StatusPill); top-level workspace meta
                    // stays off the row — the desktop list is single-line.
                    .when(depth > 0 && !metadata.is_empty(), |this| {
                        this.child(
                            div()
                                .flex_shrink_0()
                                .px_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(metadata),
                        )
                    })
                    .child(activity_badges)
                    .when(running, |this| {
                        this.child(
                            div()
                                .size_1p5()
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(cx.theme().success),
                        )
                    })
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
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| this.apply_nav(action, cx));
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
                            .bg(cx.theme().selection)
                            .text_color(cx.theme().sidebar_accent_foreground)
                    })
                    .when(!active, |this| {
                        this.hover(|style| style.bg(cx.theme().muted))
                    })
                    .child(Icon::new(action.icon()))
                    .child(div().truncate().child(label))
                    .into_any_element()
            }
            SidebarEntry::More {
                remaining,
                expanded,
                view,
            } => {
                let text = i18n::current(cx);
                let label = if expanded {
                    text.sessions_show_less.clone()
                } else {
                    text.sessions_show_more.clone()
                };
                let chevron = if expanded {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                };
                div()
                    .id("sessions-show-more")
                    .on_click(move |_, _, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| {
                                this.sessions_expanded = !this.sessions_expanded;
                                cx.notify();
                            });
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|style| style.bg(cx.theme().sidebar_accent))
                    .child(Icon::new(chevron).small())
                    .child(label)
                    .when(!expanded && remaining > 0, |this| {
                        this.child(
                            div()
                                .px_1()
                                .text_xs()
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded(cx.theme().radius_tokens().sm)
                                .child(format!("+{remaining}")),
                        )
                    })
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

/// One `Sidebar` child. The sessions tree (R1c) flattens into top-level
/// children — the header, the workspace group row, and one child per tree row
/// — so the component's internal `list` virtualizes per row (the group-level
/// nesting it renders itself has no virtualization). One type because
/// `Sidebar` children share one element type. Nothing collapses at this level:
/// the component re-applies its own icon-rail state each frame.
#[derive(Clone)]
enum SidebarSection {
    Rows(Vec<SidebarEntry>),
    /// The sessions section header (R1b): the three desktop header verbs —
    /// new session, archive filter, list reload.
    SectionHeader {
        label: SharedString,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
        archived_on: bool,
        view: WeakEntity<ShellWindow>,
    },
    /// The workspace group row (R1c): chevron fold, workspace name, and the
    /// running dot when any visible session has a live turn.
    GroupHeader {
        name: SharedString,
        collapsed: bool,
        running: bool,
        view: WeakEntity<ShellWindow>,
    },
    Item(SidebarEntry),
}

impl Collapsible for SidebarSection {
    fn collapsed(self, _collapsed: bool) -> Self {
        // The sidebar re-applies its own icon-rail state to every group on
        // each frame (`Sidebar` render calls `.collapsed(layout
        // .icon_collapsed)`); every child is always-rendered.
        self
    }

    fn is_collapsed(&self) -> bool {
        false
    }
}

impl SidebarItem for SidebarSection {
    fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        match self {
            SidebarSection::Rows(rows) => {
                let id = id.into();
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows.into_iter().enumerate().map(|(ix, row)| {
                        let row_id = SharedString::from(format!("{}-{}", id, ix));
                        row.render(row_id, window, cx).into_any_element()
                    }))
                    .into_any_element()
            }
            SidebarSection::SectionHeader {
                label,
                tx,
                archived_on,
                view,
            } => {
                let id = id.into();
                // Session-header actions (R1b): the three desktop header
                // verbs the shell has real semantics for — new session,
                // archive filter, list reload.
                let new_tx = tx.clone();
                let reload_tx = tx.clone();
                let archive_view = view.clone();
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .pt_1()
                    .h_8()
                    .px_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().sidebar_foreground.opacity(0.7))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_0p5()
                            .child(
                                Button::new(SharedString::from(format!("{}-new", id)))
                                    .icon(IconName::Plus)
                                    .small()
                                    .with_variant(ButtonVariant::Ghost)
                                    .tooltip(i18n::current(cx).sessions_new.clone())
                                    .on_click(move |_, _, _| {
                                        if let Some(handle) = runtime::runtime_handle() {
                                            handle.create_session(new_tx.clone());
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("{}-archived", id)))
                                    .icon(IconName::Archive)
                                    .small()
                                    .with_variant(if archived_on {
                                        ButtonVariant::Secondary
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .tooltip(i18n::current(cx).sessions_show_archived.clone())
                                    .on_click(move |_, _, cx| {
                                        if let Some(view) = archive_view.upgrade() {
                                            view.update(cx, |this, cx| {
                                                this.show_archived = !this.show_archived;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("{}-reload", id)))
                                    .icon(IconName::RefreshCw)
                                    .small()
                                    .with_variant(ButtonVariant::Ghost)
                                    .tooltip(i18n::current(cx).sessions_reload.clone())
                                    .on_click(move |_, _, _| {
                                        if let Some(handle) = runtime::runtime_handle() {
                                            handle.reload_sessions(reload_tx.clone());
                                        }
                                    }),
                            ),
                    )
                    .into_any_element()
            }
            SidebarSection::GroupHeader {
                name,
                collapsed,
                running,
                view,
            } => div()
                .id("session-group")
                .on_click(move |_, _, cx| {
                    if let Some(view) = view.upgrade() {
                        view.update(cx, |this, cx| {
                            this.sessions_group_collapsed = !this.sessions_group_collapsed;
                            cx.notify();
                        });
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
                .text_sm()
                .hover(|style| style.bg(cx.theme().sidebar_accent))
                .child(
                    Icon::new(if collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .small(),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_medium()
                        .truncate()
                        .child(name),
                )
                .when(running, |this| {
                    this.child(
                        div()
                            .size_1p5()
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(cx.theme().success),
                    )
                })
                .into_any_element(),
            SidebarSection::Item(entry) => entry.render(id, window, cx).into_any_element(),
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
    /// One chat per session tab (R1a): the vec order follows the tab bar.
    /// Session tabs own their transcript even while hidden — background turns
    /// keep appending, and reopening a closed tab refetches the transcript.
    chats: Vec<ChatView>,
    chat_error: Option<String>,
    /// Main-area tab bar (R1a): session tabs and singleton scene tabs share
    /// it; the active tab decides what the main area renders.
    pub(crate) tabs: TabsState,
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
    /// The session tab's right resource panel (R2c): Files or Changes docked
    /// beside the conversation. A workspace-scoped layout preference, like the
    /// web's `workspaceResourceState` — it follows the workspace and survives
    /// session switches; opening it never activates a session.
    resource_panel: Option<ResourcePanelKind>,
    /// Split state of the session's right panel (the same shape the terminal
    /// dock keeps for its height).
    panel_split: Entity<ResizableState>,
    /// Width the right panel reopens at (the last dragged width, or the
    /// default before any drag).
    panel_width: Pixels,
    /// One-shot automation hooks for the settings page and the changes view,
    /// mirroring `OPENBITFUN_GPUI_TERMINAL_AUTOOPEN`.
    settings_auto_open: bool,
    git_auto_open: bool,
    /// Sidebar filter: archived sessions stay hidden until this is on.
    show_archived: bool,
    /// Per-root session lineage snapshots (R1c): the subagent children shown
    /// indented under each sidebar row. Replaced wholesale on each list
    /// reload; entries for since-deleted sessions match nothing.
    lineages: Vec<AgentSessionLineageSnapshot>,
    /// Session-tree view state (R1c): the workspace group fold, and the
    /// show-more expansion of long root lists (collapsed shows the web's
    /// level-0 page).
    sessions_group_collapsed: bool,
    sessions_expanded: bool,
    /// Global search palette (R1b): one boolean, because the query state and
    /// the input entity live on the window already (`search`/`search_input`).
    search_overlay_open: bool,
    /// Drawer state of the nav sidebar column (web WorkspaceBody): folded,
    /// the column leaves the layout entirely and a floating strip keeps an
    /// opener at the window's top-left corner (Ctrl B toggles, like the web).
    sidebar_open: bool,
    /// The resting focus target: gpui dispatches keystroke bindings along the
    /// focus chain, so with nothing focused Ctrl K would be dropped. The root
    /// takes focus whenever nothing else holds it (fresh window, after the
    /// palette closes), which keeps the shortcut live — the same behavior the
    /// desktop gets from its window-level shortcut scope.
    root_focus: gpui_kit::FocusHandle,
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
    /// T2-15 Trinity console page state (event-driven) + its form inputs.
    pub(crate) trinity: crate::trinity::TrinityPageState,
    /// R1d status bar: OS login name backing the user slot while the trinity
    /// daemon (which knows the user's chosen name) is not connected.
    os_user: String,
    /// R1d status bar: the one-shot lamp prefetch fired when the runtime
    /// first reports ready; the chip shares the console page's snapshot.
    trinity_status_fetched: bool,
    /// T2-16 unread turns per session: turns that finished while the window
    /// was inactive. Cleared per session on open, all via the tray action.
    unread_turns: std::collections::BTreeMap<String, (u32, bool)>,
    pub(crate) trinity_recall_input: Entity<InputState>,
    pub(crate) trinity_add_content: Entity<TextareaState>,
    pub(crate) trinity_add_project: Entity<InputState>,
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
    /// Task tool calls observed starting with a `run_in_background` input
    /// (T2-6, the web backgroundSubagentActivity signal), keyed by the tool
    /// card key. Monotonic: the flag arrives once on the `Started` event.
    background_task_tools: BTreeSet<SharedString>,
    /// Sessions with a turn in flight (T2-6). The event pump reports every
    /// session's lifecycle process-wide; the open chat consumes its own, and
    /// this set keeps the rest so background subagent activity can be derived
    /// without opening the session.
    live_sessions: BTreeSet<String>,
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
    /// T2-12 edit mode: `Some(agent_id)` while the shared create dialog is
    /// editing an existing custom agent (id/kind/level render fixed); the
    /// detail event prefills the inputs.
    pub(crate) agent_edit_target: Option<String>,
    pub(crate) agent_edit_loading: bool,
    /// T2-12 delete confirmation scratch (id + display name).
    pub(crate) agent_pending_delete: Option<crate::manage::AgentPendingDelete>,
    pub(crate) mcp_pending_delete: Option<String>,
    pub(crate) mcp_browse: Option<crate::manage::McpServerCatalogView>,
    pub(crate) mcp_browse_loading: Option<(String, String)>,
    pub(crate) mcp_interactions: crate::manage::McpInteractionQueue,
    pub(crate) mcp_interaction_result_input: Entity<TextareaState>,
    pub(crate) mcp_interaction_error: Option<String>,
    /// T2-11 per-mode skill selection dialog: one scratch state for the
    /// dialog builder (which runs every frame), driven by the
    /// `ModeSkillInfos`/`ManageFailed` runtime events.
    pub(crate) mode_skills: Option<crate::manage::ModeSkillsDialogState>,
    /// T2-7 raw JSON config editor: the multi-line field lives on the window
    /// (recreated never, reset on every open) plus the loaded snapshot's
    /// fingerprint and dialog status.
    pub(crate) mcp_json_state: Entity<TextareaState>,
    pub(crate) mcp_json_fingerprint: Option<String>,
    pub(crate) mcp_json_loading: bool,
    pub(crate) mcp_json_error: Option<String>,
    /// T2-7 pending or finished resource read in the browse dialog.
    pub(crate) mcp_resource_view: Option<crate::manage::McpResourceView>,
    /// M22 self-update state machine; the general settings page projects it.
    pub(crate) update_state: crate::update::UpdateUiState,
    /// M22 archived-sessions delete confirmation target (dialog input lives
    /// on the window like every other dialog scratch value).
    pub(crate) archived_delete_target: Option<String>,
    /// M22 global usage statistics snapshot for the settings usage page.
    pub(crate) usage_stats: Option<Box<openbitfun_services_core::token_usage::UsageStatistics>>,
    pub(crate) usage_range: String,
    pub(crate) usage_loading: bool,
    /// T2-8 activity heatmap snapshot: a fixed six-month custom-range fetch
    /// (day granularity); `Err` keeps the page on its empty line until the
    /// refresh button retries.
    pub(crate) usage_activity:
        Option<Result<Box<openbitfun_services_core::token_usage::UsageStatistics>, String>>,
    pub(crate) usage_activity_loading: bool,
    /// M22 web-search credential state for the selected provider
    /// (`provider`, `configured`); fetched when the settings view opens.
    pub(crate) websearch_status: Option<(String, bool)>,
    pub(crate) websearch_credential_input: Entity<InputState>,
    /// R7b plugin UI contributions: the projection (fixture provider behind
    /// the env gate) plus the per-scene shell runtime (control entities and
    /// visible diagnostics). Rendering dispatches through `crate::plugin_ui`.
    pub(crate) plugin_ui: openbitfun_core::plugin_ui::PluginUiProjection,
    pub(crate) plugin_ui_scenes: std::collections::BTreeMap<
        openbitfun_core::plugin_ui::SceneRef,
        crate::plugin_ui::PluginUiSceneRuntime,
    >,
    /// One-shot automation hook (`OPENBITFUN_PLUGIN_UI_AUTOOPEN`): opens the
    /// fixture scene tab once the runtime is ready.
    plugin_ui_auto_open: bool,
    /// R7c host test channel: the source keys whose providers installed from
    /// the bridge's Ready event; the channel-closed event retracts them.
    pub(crate) plugin_ui_host_keys: Vec<String>,
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
        let dock = cx.new(|_| ResizableState::default());
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let workspace_input = cx.new(|cx| InputState::new(window, cx));
        let export_input = cx.new(|cx| InputState::new(window, cx));
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let trinity_recall_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.trinity_memory_search_placeholder.clone())
        });
        let trinity_add_content = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(text.trinity_memory_add_placeholder.clone())
        });
        let trinity_add_project = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.trinity_memory_add_project.clone())
        });
        let profile_name_input = cx.new(|cx| InputState::new(window, cx));
        let profile_dir_input = cx.new(|cx| InputState::new(window, cx));
        let profile_command_input = cx.new(|cx| InputState::new(window, cx));
        let agent_id_input = cx.new(|cx| InputState::new(window, cx));
        let agent_name_input = cx.new(|cx| InputState::new(window, cx));
        let agent_description_input = cx.new(|cx| InputState::new(window, cx));
        let agent_prompt_input = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_interaction_result_input = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_json_state = cx.new(|cx| TextareaState::new(window, cx));
        let websearch_credential_input = cx.new(|cx| InputState::new(window, cx));
        // R7b plugin UI contributions: the projection owns the installed
        // scenes; the fixture provider installs behind its env gate.
        let (plugin_ui, _fixture_scene) = crate::plugin_ui::setup_projection();
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
            chats: Vec::new(),
            chat_error: None,
            tabs: TabsState::default(),
            event_tx,
            pending_permissions: Vec::new(),
            pending_questions: Vec::new(),
            modal_owner: None,
            terminal_open: false,
            terminals: Vec::new(),
            active_terminal: 0,
            terminal_auto_open: std::env::var_os("OPENBITFUN_GPUI_TERMINAL_AUTOOPEN").is_some(),
            git: None,
            files: None,
            search: Default::default(),
            search_input,
            trinity: Default::default(),
            os_user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default(),
            trinity_status_fetched: false,
            unread_turns: Default::default(),
            trinity_recall_input,
            trinity_add_content,
            trinity_add_project,
            dock,
            terminal_height: TerminalPane::panel_height(),
            resource_panel: None,
            panel_split: cx.new(|_| ResizableState::default()),
            panel_width: px(DEFAULT_RESOURCE_PANEL_WIDTH),
            settings_auto_open: std::env::var_os("OPENBITFUN_GPUI_SETTINGS_AUTOOPEN").is_some(),
            git_auto_open: std::env::var_os("OPENBITFUN_GPUI_GIT_AUTOOPEN").is_some(),
            show_archived: false,
            lineages: Vec::new(),
            sessions_group_collapsed: false,
            sessions_expanded: false,
            search_overlay_open: false,
            sidebar_open: true,
            root_focus: cx.focus_handle(),
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
            background_task_tools: BTreeSet::new(),
            live_sessions: BTreeSet::new(),
            pending_child: None,
            agent_id_input,
            agent_name_input,
            agent_description_input,
            agent_prompt_input,
            agent_create_active: false,
            agent_create_kind_mode: false,
            agent_create_project_level: false,
            agent_create_error: None,
            agent_edit_target: None,
            agent_edit_loading: false,
            agent_pending_delete: None,
            mcp_pending_delete: None,
            mcp_browse: None,
            mcp_browse_loading: None,
            mcp_interactions: Default::default(),
            mcp_interaction_result_input,
            mcp_interaction_error: None,
            mode_skills: None,
            mcp_json_state,
            mcp_json_fingerprint: None,
            mcp_json_loading: false,
            mcp_json_error: None,
            mcp_resource_view: None,
            update_state: Default::default(),
            archived_delete_target: None,
            usage_stats: None,
            usage_range: "thisMonth".to_string(),
            usage_loading: false,
            usage_activity: None,
            usage_activity_loading: false,
            websearch_status: None,
            websearch_credential_input,
            plugin_ui,
            plugin_ui_scenes: std::collections::BTreeMap::new(),
            plugin_ui_auto_open: Self::plugin_ui_auto_open_requested(),
            plugin_ui_host_keys: Vec::new(),
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
    /// the reader sits at the live edge). Hidden tabs skip the live sync —
    /// their scroller re-syncs from the entry count when the tab activates.
    fn list_sync_append(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if !self.tabs.is_active_session(session_id) {
            return;
        }
        let Some(chat) = self.chat_for(session_id) else {
            return;
        };
        chat.scroller.update(cx, |scroller, cx| {
            scroller.append(1, cx);
        });
    }

    /// The tail entry changed in place (streaming chunk or tool-card update);
    /// its cached height is stale until the list re-measures it.
    fn list_sync_tail_changed(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if !self.tabs.is_active_session(session_id) {
            return;
        }
        let Some(chat) = self.chat_for(session_id) else {
            return;
        };
        let count = chat.entries.len();
        if count > 0 {
            chat.scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(tail_changed_range(count), cx);
            });
        }
    }

    /// An entry below the tail changed in place (tool lifecycle upserts land
    /// on cards created earlier in the turn).
    fn list_sync_item_changed(&mut self, session_id: &str, index: usize, cx: &mut Context<Self>) {
        if !self.tabs.is_active_session(session_id) {
            return;
        }
        let Some(chat) = self.chat_for(session_id) else {
            return;
        };
        chat.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(index..index + 1, cx);
        });
    }

    /// The transcript was replaced wholesale (session opened or reloaded);
    /// resetting also drops the follow anchor, which pins the view to the
    /// newest content.
    fn list_sync_reset(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.tabs.active_session_id().map(str::to_string) else {
            return;
        };
        let Some(chat) = self.chat_for(&session_id) else {
            return;
        };
        let count = chat.entries.len();
        chat.scroller.update(cx, |scroller, cx| {
            scroller.reset(count, cx);
        });
    }

    fn chat_for(&self, session_id: &str) -> Option<&ChatView> {
        self.chats.iter().find(|chat| chat.matches(session_id))
    }

    fn chat_mut_for(&mut self, session_id: &str) -> Option<&mut ChatView> {
        self.chats.iter_mut().find(|chat| chat.matches(session_id))
    }

    /// The active tab's chat, when it is a session tab.
    fn active_chat(&self) -> Option<&ChatView> {
        let id = self.tabs.active_session_id()?;
        self.chat_for(id)
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
                self.fetch_trinity_status_once();
            }
            RuntimeEvent::Sessions(rows) => {
                self.phase = RuntimePhase::Ready;
                self.list_error = None;
                self.sessions = rows.iter().map(|row| self.compose_row(row, cx)).collect();
                self.fetch_trinity_status_once();
            }
            RuntimeEvent::SessionsFailed(error) => {
                self.list_error = Some(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::SessionLineages(lineages) => {
                self.lineages = lineages;
            }
            RuntimeEvent::Failed(error) => {
                self.phase = RuntimePhase::Failed(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::SessionOpened(opened) => {
                self.chat_error = None;
                // Opening a session is a chat action: the session tab the
                // transcript lands on activates below (tabs.open).
                // Opening the session is "reading it" (T2-16): its unread
                // turns drop out of the tray badge.
                self.clear_unread_session(&opened.session_id);
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
                if let Some(chat) = self.chat_mut_for(&opened.session_id) {
                    // Transcript reload for an open tab keeps its child
                    // context (a reloaded child stays a child view).
                    chat.entries = opened.entries;
                    chat.busy = opened.busy;
                    chat.submitting = false;
                    chat.name = opened.name;
                    chat.model_id = opened.model_id;
                    if settled_child.is_some() {
                        chat.child_context = settled_child;
                    }
                } else {
                    let mut chat = ChatView::new(
                        opened.session_id.clone(),
                        opened.name,
                        opened.agent_type,
                        opened.model_id,
                        opened.entries,
                        cx,
                    );
                    chat.busy = opened.busy;
                    if settled_child.is_some() {
                        chat.child_context = settled_child;
                    }
                    self.chats.push(chat);
                }
                // A transcript (re)load always lands on a tab the user just
                // addressed; opening activates it and re-syncs the scroller.
                self.tabs.open(TabKind::Session(opened.session_id));
                self.apply_subagent_links();
                self.list_sync_reset(cx);
                tracing::info!("Session transcript ready in a session tab");
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
                turn_id,
            } => {
                // The child transcript loads asynchronously; the opened echo
                // settles the child context onto the chat pane.
                self.pending_child = Some(PendingChild {
                    child_session_id: child_session_id.clone(),
                    context: ChildContext {
                        kind: ChildKind::Btw,
                        parent_session_id,
                        agent_type: None,
                        btw_turn_id: Some(turn_id),
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
                if let Some(chat) = self.chat_mut_for(&session_id) {
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
                    self.list_sync_append(&session_id, cx);
                }
            }
            RuntimeEvent::SendFailed { session_id, error } => {
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.submitting = false;
                    // The user entry is appended on TurnAccepted, so a
                    // failed submission only surfaces the error notice.
                    chat.entries.push(ChatEntry::Notice {
                        text: i18n::failed_status(&error, cx),
                    });
                    self.list_sync_append(&session_id, cx);
                }
            }
            RuntimeEvent::TurnStarted { session_id, .. } => {
                // Session-agnostic bookkeeping first (T2-6): the pump reports
                // every session's turns, and background subagent activity is
                // derived from exactly that.
                self.live_sessions.insert(session_id.clone());
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.submitting = false;
                    chat.busy = true;
                }
            }
            RuntimeEvent::TextChunk {
                session_id,
                turn_id: _,
                round_id,
                text,
            } => {
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    if chat.append_chunk(&round_id, &text) {
                        self.list_sync_append(&session_id, cx);
                    } else {
                        self.list_sync_tail_changed(&session_id, cx);
                    }
                }
            }
            RuntimeEvent::ToolUpdate { session_id, card } => {
                // The run_in_background flag rides the Started event only:
                // record it before the chat-scoped upsert, which may never
                // see this session's transcript at all.
                if card.background {
                    self.background_task_tools.insert(card.key.clone());
                }
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    match chat.upsert_tool(card) {
                        None => self.list_sync_append(&session_id, cx),
                        Some(index) => self.list_sync_item_changed(&session_id, index, cx),
                    }
                }
            }
            RuntimeEvent::TurnFinished { session_id, error } => {
                self.live_sessions.remove(&session_id);
                let failure = error.clone();
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.busy = false;
                    chat.submitting = false;
                    if let Some(error) = error {
                        chat.entries.push(ChatEntry::Notice {
                            text: i18n::failed_status(&error, cx),
                        });
                        self.list_sync_append(&session_id, cx);
                    }
                }
                // Session names/metadata change as turns settle.
                if let Some(handle) = runtime::runtime_handle() {
                    handle.reload_sessions(self.event_tx.clone());
                }
                self.notify_turn_finished(&session_id, failure.as_deref(), window, cx);
                self.record_unread_turn(&session_id, failure.is_some(), window, cx);
            }
            RuntimeEvent::HistoryChanged { session_id } => {
                if self.chat_for(&session_id).is_some() {
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
            RuntimeEvent::TrinityStatusReady {
                psi,
                status,
                history,
            } => {
                self.trinity.status = Some((psi, status));
                self.trinity.history = history;
                self.trinity.status_error = None;
                self.trinity.loaded = true;
            }
            RuntimeEvent::TrinityStatusFailed(message) => {
                tracing::info!("Trinity console offline: {message}");
                self.trinity.status = None;
                self.trinity.status_error = Some(message);
                self.trinity.loaded = true;
            }
            RuntimeEvent::TrinityMemoriesReady {
                items,
                exhausted,
                append,
            } => {
                if append {
                    // A `before_ts` continuation merges into the loaded list,
                    // deduped by id (the daemon may re-serve boundary rows).
                    let seen: std::collections::HashSet<String> = self
                        .trinity
                        .memories
                        .iter()
                        .filter_map(|m| m.id.clone())
                        .collect();
                    for item in items {
                        let dup = item.id.as_ref().is_some_and(|id| seen.contains(id));
                        if !dup {
                            self.trinity.memories.push(item);
                        }
                    }
                    self.trinity.memories_exhausted = exhausted;
                } else {
                    self.trinity.memories = items;
                    self.trinity.memories_exhausted = exhausted;
                    self.trinity.memories_loaded = true;
                }
                self.trinity.memories_loading = false;
                self.trinity.forget_armed = None;
            }
            RuntimeEvent::TrinityMemoriesFailed(message) => {
                tracing::warn!("Trinity memory timeline failed: {message}");
                self.trinity.memories_loading = false;
                self.trinity.memories_loaded = true;
                self.trinity.action_error = Some(message);
            }
            RuntimeEvent::TrinityRecallReady(items) => {
                self.trinity.recall = Some(items);
                self.trinity.recall_busy = false;
            }
            RuntimeEvent::TrinityActionFailed(message) => {
                tracing::warn!("Trinity action failed: {message}");
                self.trinity.recall_busy = false;
                self.trinity.action_error = Some(message);
            }
            RuntimeEvent::PluginUiBridgeReady { providers } => {
                for install in providers {
                    let crate::runtime::PluginUiProviderInstall {
                        source_key,
                        provider,
                    } = install;
                    match self.plugin_ui.install(&source_key, provider) {
                        Ok(handles) => {
                            tracing::info!(
                                "Plugin UI host provider installed: key={source_key}, scenes={}",
                                handles.len()
                            );
                            self.plugin_ui_host_keys.push(source_key);
                        }
                        Err(gate) => tracing::error!(
                            "Plugin UI host provider rejected: key={source_key}, gate={gate:?}"
                        ),
                    }
                }
                cx.notify();
            }
            RuntimeEvent::PluginUiSceneUpdate { update } => {
                self.apply_plugin_ui_scene_update(update, cx);
            }
            RuntimeEvent::PluginUiBridgeFailed(error) => {
                tracing::error!("Plugin UI host channel failed: {error}");
            }
            RuntimeEvent::PluginUiChannelClosed => {
                for key in self.plugin_ui_host_keys.drain(..) {
                    if self.plugin_ui.uninstall(&key) {
                        tracing::info!("Plugin UI host provider retracted: key={key}");
                    }
                }
                cx.notify();
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
                    // The submission landed; the create/edit dialog closes on
                    // the catalog event that proves it.
                    self.agent_create_active = false;
                    self.agent_edit_target = None;
                    self.agent_edit_loading = false;
                    window.close_dialog(cx);
                }
                cx.refresh_windows();
            }
            RuntimeEvent::AgentDetail(result) => {
                match result {
                    Ok(row) => {
                        // Prefill the shared dialog inputs and fix the
                        // kind/level selectors so validation matches the
                        // existing definition.
                        self.agent_id_input.update(cx, |state, cx| {
                            state.set_value(row.agent_id.clone(), window, cx)
                        });
                        self.agent_name_input.update(cx, |state, cx| {
                            state.set_value(row.name.clone(), window, cx)
                        });
                        self.agent_description_input.update(cx, |state, cx| {
                            state.set_value(row.description.clone(), window, cx)
                        });
                        self.agent_prompt_input.update(cx, |state, cx| {
                            state.set_value(row.prompt.clone(), window, cx)
                        });
                        self.agent_create_kind_mode = row.mode_kind;
                        self.agent_create_project_level = row.project_level;
                        self.agent_edit_loading = false;
                        self.agent_create_error = None;
                    }
                    Err(message) => {
                        tracing::warn!("Custom agent detail load failed: {message}");
                        self.agent_edit_loading = false;
                        self.agent_create_error = Some(i18n::failed_status(&message, cx));
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::McpCatalog(catalog) => {
                cx.set_global(catalog);
                cx.refresh_windows();
            }
            RuntimeEvent::ModeSkillInfos { mode_id, rows } => {
                if let Some(state) = self.mode_skills.as_mut() {
                    if state.mode_id == mode_id {
                        // Successful fetch/save/reset: land in the view phase
                        // with fresh rows (the web edit mode exits on write).
                        state.rows = rows;
                        state.phase = crate::manage::ModeSkillsPhase::View;
                        state.pending = None;
                        state.submitting = false;
                        state.error = None;
                    }
                }
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
            RuntimeEvent::McpJsonConfig(result) => {
                self.mcp_json_loading = false;
                match result {
                    Ok(snapshot) => {
                        self.mcp_json_fingerprint = Some(snapshot.fingerprint);
                        self.mcp_json_state.update(cx, |state, cx| {
                            state.set_value(snapshot.json_config, window, cx)
                        });
                    }
                    Err(message) => {
                        tracing::warn!("Failed to load MCP JSON config: {message}");
                        self.mcp_json_error = Some(message);
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::McpJsonSaved(result) => {
                match result {
                    Ok(()) => {
                        let saved = i18n::current(cx).manage_mcp_json_saved;
                        self.mcp_json_fingerprint = None;
                        self.mcp_json_error = None;
                        window.close_dialog(cx);
                        crate::manage::ManageNotice::set(Some(saved), cx);
                    }
                    Err(message) => {
                        tracing::warn!("Failed to save MCP JSON config: {message}");
                        self.mcp_json_error = Some(message);
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::McpResourceRead(outcome) => {
                let outcome = *outcome;
                if let Some(view) = &mut self.mcp_resource_view {
                    if view.uri == outcome.uri {
                        view.loading = false;
                        view.result = Some(outcome.result);
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::ManageFailed(message) => {
                tracing::warn!("Management action failed: {message}");
                if let Some(state) = self.mode_skills.as_mut() {
                    state.submitting = false;
                    state.error = Some(i18n::failed_status(&message, cx));
                } else if self.agent_create_active {
                    self.agent_create_error = Some(i18n::failed_status(&message, cx));
                } else {
                    crate::manage::ManageNotice::set(Some(i18n::failed_status(&message, cx)), cx);
                }
                cx.refresh_windows();
            }
            RuntimeEvent::TrayShowWindow => {
                window.activate_window();
                cx.activate(true);
                // The user lands on the active session tab; that session is read.
                if let Some(session_id) = self.tabs.active_session_id().map(str::to_string) {
                    self.clear_unread_session(&session_id);
                }
            }
            RuntimeEvent::TrayMarkAllRead => {
                self.unread_turns.clear();
                self.sync_tray_unread();
                cx.refresh_windows();
            }
            RuntimeEvent::QuitRequested => {
                // Terminal PTYs die with the window; close them properly so
                // no child is orphaned by the exit.
                self.close_all_terminal_tabs(cx);
                crate::diagnostics::mark_clean_shutdown("quit_requested");
                cx.quit();
            }
            RuntimeEvent::RestartRequested => {
                self.close_all_terminal_tabs(cx);
                crate::diagnostics::mark_clean_shutdown("restart_requested");
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
            RuntimeEvent::UsageActivityReady(result) => {
                self.usage_activity_loading = false;
                match result {
                    Ok(stats) => self.usage_activity = Some(Ok(stats)),
                    Err(message) => {
                        tracing::warn!("Usage activity statistics failed: {message}");
                        self.usage_activity = Some(Err(message));
                    }
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
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.name = name;
                }
            }
            RuntimeEvent::SessionForked { .. } => {
                // The runtime opens the fork right away; the transcript swap
                // through `SessionOpened` is the visible feedback, and the
                // session list refresh rides the same batch.
            }
            RuntimeEvent::CompactionStarted { session_id } => {
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.busy = true;
                    chat.entries.push(ChatEntry::Info {
                        text: i18n::current(cx).compaction_started.clone(),
                    });
                    self.list_sync_append(&session_id, cx);
                }
            }
            RuntimeEvent::ExportFinished { session_id, path } => {
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    let text = i18n::current(cx);
                    chat.entries.push(ChatEntry::Info {
                        text: format!("{} {path}", text.export_done_prefix),
                    });
                    self.list_sync_append(&session_id, cx);
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
                // gone from storage, so its tab closes (and the chat state
                // drops with it). Reopening is impossible — the row is gone.
                if let Some(index) = self.chats.iter().position(|chat| chat.matches(&session_id)) {
                    self.chats.remove(index);
                    if let Some(removed) = self.tabs.index_of(&TabKind::Session(session_id.clone()))
                    {
                        let was_active = self.tabs.active_index() == Some(removed);
                        self.tabs.close(removed);
                        if was_active {
                            self.list_sync_reset(cx);
                        }
                    }
                }
                self.sessions.retain(|row| row.id != session_id);
            }
            RuntimeEvent::SessionActionFailed { message } => {
                let line = i18n::failed_status(&message, cx);
                tracing::warn!("Session action failed: {message}");
                if self.tabs.active_session_id().is_some() {
                    self.chat_error = Some(line);
                } else {
                    self.list_error = Some(line);
                }
            }
            RuntimeEvent::SessionModelChanged {
                session_id,
                model_id,
            } => {
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.model_id = Some(model_id);
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
        self.chats.clear();
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
        self.tabs = TabsState::default();
        // Staged images belong to the abandoned workspace's draft.
        self.pending_attachments.clear();
        cx.notify();
    }

    /// Desktop notification for a settled turn while the window is not the
    /// active one (the D36 ⑤ trigger surface: any finished turn — success or
    /// failure — in a session with an open chat). Platforms without a
    /// notification daemon no-op silently.
    /// Records one finished turn as unread when it landed while the window
    /// was inactive, and pushes the folded snapshot to the tray.
    fn record_unread_turn(
        &mut self,
        session_id: &str,
        failed: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if window.is_window_active() {
            return;
        }
        let entry = self
            .unread_turns
            .entry(session_id.to_string())
            .or_insert((0, false));
        entry.0 += 1;
        entry.1 |= failed;
        self.sync_tray_unread();
        cx.notify();
    }

    /// Clears one session's unread count (opening the session) and re-syncs
    /// the tray.
    fn clear_unread_session(&mut self, session_id: &str) {
        if self.unread_turns.remove(session_id).is_some() {
            self.sync_tray_unread();
        }
    }

    /// Folds the per-session map into the tray snapshot (total + any-failed).
    fn sync_tray_unread(&self) {
        let count = self
            .unread_turns
            .values()
            .map(|(count, _)| *count)
            .sum::<u32>();
        let any_failed = self.unread_turns.values().any(|(_, failed)| *failed);
        crate::tray::update_unread(crate::tray::TrayUnread { count, any_failed });
    }

    fn notify_turn_finished(
        &self,
        session_id: &str,
        error: Option<&str>,
        window: &Window,
        cx: &Context<Self>,
    ) {
        if window.is_window_active() || !self.chats.iter().any(|chat| chat.matches(session_id)) {
            return;
        }
        // The shared dialog-completion switch (`app.notifications`
        // `dialog_completion_notify`) gates this, same as the desktop's
        // turn-finished toast; before the shared config is read the
        // default-on path holds.
        if crate::settings::ShellConfig::try_current(cx)
            .is_some_and(|config| !config.dialog_completion_notify)
        {
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

    /// Re-annotates every open transcript's tool cards from the subagent link
    /// map (M20). Called on every transcript load and on each new link, so
    /// reloads never lose the "view run" entry points; session tabs all show
    /// links of their parent, so each open chat re-derives its own.
    fn apply_subagent_links(&mut self) {
        for chat in &mut self.chats {
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
    }

    /// The sidebar activity badges for one session row (T2-6, the web
    /// backgroundSubagentActivity and review badge pair): how many linked
    /// subagent turns are running behind the scenes, and whether a
    /// review-family subagent is among them. A subagent counts when its task
    /// tool started with `run_in_background`, or — for links whose `Started`
    /// event predates this process — when it has outlived the parent's turn.
    fn session_activity_badges(
        &self,
        parent_session_id: &str,
    ) -> (usize, Option<ReviewActivityKind>) {
        let parent_live = self.live_sessions.contains(parent_session_id);
        let mut count = 0usize;
        let mut review = None;
        for link in self
            .subagent_links
            .iter()
            .filter(|link| link.parent_session_id == parent_session_id)
        {
            if !self.live_sessions.contains(&link.subagent_session_id) {
                continue;
            }
            let tool_key = SharedString::from(format!("tool-{}", link.tool_call_id));
            if !self.background_task_tools.contains(&tool_key) && parent_live {
                continue;
            }
            count += 1;
            if review.is_none() {
                review = link.agent_type.as_deref().and_then(review_activity_kind);
            }
        }
        (count, review)
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
                btw_turn_id: None,
            },
        });
        if let Some(handle) = runtime::runtime_handle() {
            handle.open_session(subagent_session_id, self.event_tx.clone());
        }
        cx.notify();
    }

    /// Returns from a child view (subagent run / /btw) to its parent session.
    fn return_from_child(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat() else {
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
        self.tabs.open(TabKind::Scene(SceneId::Settings));
        if self.usage_stats.is_none() && !self.usage_loading {
            self.usage_loading = true;
            if let Some(handle) = runtime::runtime_handle() {
                handle.fetch_usage_stats(self.usage_range.clone());
            }
        }
        if self.usage_activity.is_none() && !self.usage_activity_loading {
            self.usage_activity_loading = true;
            if let Some(handle) = runtime::runtime_handle() {
                handle.fetch_usage_activity();
            }
        }
        let (provider, http_auth_mode) = settings::ShellConfig::try_current(cx)
            .map(|config| (config.web_search_provider, config.web_search_http_auth_mode))
            .unwrap_or_default();
        if matches!(provider.as_str(), "exa_search_api" | "tavily")
            || (provider == "openbitfun_search_http"
                && settings::websearch_http_credential_required(&http_auth_mode))
        {
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
    /// Opens the changes resource (R2c): docked into the active session tab's
    /// right panel, or as a main scene tab when no session tab is active —
    /// same opening policy as Files. Creating the panel fetches the workspace
    /// diff snapshot. Idempotent: an open tab or docked panel just stays.
    /// The chat's active session rides along: the worktree view's session
    /// isolation toggle addresses it (M19).
    fn toggle_git_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_resource(ResourcePanelKind::Git, window, cx);
    }

    /// The chat turn footer's snapshot entry point (R2d): opens the git
    /// resource the same way `toggle_resource` does but never closes it, then
    /// points the panel at the snapshots view for this workspace's sessions.
    fn open_git_snapshots(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_git_panel(window, cx);
        self.set_git_view(crate::git::GitView::Snapshots, cx);
    }

    /// Points the git panel at a view without touching its mount; a no-op
    /// while the panel is closed (the settings worktrees redirect pairs this
    /// with [`Self::open_git_panel`], R3).
    pub(crate) fn set_git_view(&mut self, view: crate::git::GitView, cx: &mut Context<Self>) {
        if let Some(git) = self.git.clone() {
            git.update(cx, |panel, cx| panel.set_view(view, cx));
        }
    }

    /// Opens the git resource with the resource opening policy minus the
    /// toggle: docked into the active session tab's right panel, or as a main
    /// scene tab when no session tab is active. Idempotent — an open tab or
    /// docked panel just stays, and the panel's current view is kept.
    pub(crate) fn open_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.tabs.active_kind(), Some(TabKind::Session(_))) {
            if self.resource_panel != Some(ResourcePanelKind::Git) {
                self.ensure_resource_page(ResourcePanelKind::Git, window, cx);
                self.dock_resource_panel(ResourcePanelKind::Git, cx);
            }
        } else {
            self.ensure_resource_page(ResourcePanelKind::Git, window, cx);
            self.set_resource_compact(ResourcePanelKind::Git, false, cx);
            self.tabs
                .open(TabKind::Scene(ResourcePanelKind::Git.scene()));
        }
        cx.notify();
    }

    /// Closes the top tab at `index`. A closing scene tab drops its page
    /// state where the old toggle did (the git panel refetches on reopen);
    /// a closing session tab drops that chat's transcript state.
    fn close_top_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(kind) = self.tabs.kinds().get(index).cloned() else {
            return;
        };
        self.tabs.close(index);
        match kind {
            TabKind::Scene(SceneId::Git) => {
                // Drop the panel with the tab so the next open refetches.
                self.git = None;
            }
            TabKind::Session(session_id) => {
                self.chats.retain(|chat| !chat.matches(&session_id));
            }
            TabKind::Scene(_) => {}
        }
        self.list_sync_reset(cx);
        cx.notify();
    }

    /// Sidebar nav row activation (M23 nav rows, R1a tab semantics): scene
    /// rows open (and thereby activate) their scene tab — clicking an active
    /// row keeps it active instead of toggling back, matching the desktop.
    /// Chat activates the most recent session tab; with none open it rests on
    /// the empty state.
    fn apply_nav(&mut self, action: NavAction, cx: &mut Context<Self>) {
        match action {
            NavAction::Chat => {
                if let Some(chat) = self.chats.last() {
                    let session_id = chat.session_id.clone();
                    self.tabs.open(TabKind::Session(session_id));
                } else {
                    self.tabs = TabsState::default();
                }
                cx.notify();
            }
            NavAction::Trinity => self.toggle_trinity_view(cx),
            NavAction::Tasks => self.toggle_tasks_view(cx),
        }
    }

    /// Insights page tab; the first open fetches the latest report list.
    fn toggle_insights_view(&mut self, cx: &mut Context<Self>) {
        if !self.insights.loaded {
            if let Some(handle) = runtime::runtime_handle() {
                handle.reload_insights(self.event_tx.clone());
            }
        }
        self.tabs.open(TabKind::Scene(SceneId::Insights));
        cx.notify();
    }

    /// R1d status bar: one lamp prefetch when the runtime first reports
    /// ready, so the chip answers without the console page being opened.
    /// Daemon absence settles as the offline lamp (loud degrade, no retry
    /// loop); the console page keeps its own fetch-on-first-open.
    fn fetch_trinity_status_once(&mut self) {
        if self.trinity_status_fetched {
            return;
        }
        self.trinity_status_fetched = true;
        if let Some(handle) = runtime::runtime_handle() {
            handle.trinity_refresh(self.event_tx.clone());
        }
    }

    /// Trinity console tab; the first open pulls status + timeline.
    fn toggle_trinity_view(&mut self, cx: &mut Context<Self>) {
        if !self.trinity.loaded {
            if let Some(handle) = runtime::runtime_handle() {
                handle.trinity_refresh(self.event_tx.clone());
                handle.trinity_load_memories(self.event_tx.clone(), None);
            }
        }
        self.tabs.open(TabKind::Scene(SceneId::Trinity));
        cx.notify();
    }

    /// Scheduled-jobs page tab; the first open fetches the job snapshot.
    fn toggle_tasks_view(&mut self, cx: &mut Context<Self>) {
        if !self.tasks.loaded {
            if let Some(handle) = runtime::runtime_handle() {
                handle.reload_cron_jobs(self.event_tx.clone());
            }
        }
        self.tabs.open(TabKind::Scene(SceneId::Tasks));
        cx.notify();
    }

    /// Files resource (R2c): with a session tab active the page docks into
    /// that tab's right panel; otherwise it opens as a main resource tab —
    /// the workbench opening policy's "no matching session tab" branch.
    /// Toggling the docked kind closed hides the panel again.
    fn toggle_files_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_resource(ResourcePanelKind::Files, window, cx);
    }

    /// The R2c resource entry point: docked right panel while a session tab
    /// is active, main scene tab otherwise — the workbench opening policy's
    /// "no matching session tab" branch. Docking keeps the page entity alive
    /// across the mount move and evicts the resource's main tab (the two
    /// forms are mutually exclusive); closing the panel remembers the dragged
    /// width, like the terminal dock remembers its height.
    fn toggle_resource(
        &mut self,
        kind: ResourcePanelKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.tabs.active_kind(), Some(TabKind::Session(_))) {
            if self.resource_panel == Some(kind) {
                self.close_resource_panel(cx);
            } else {
                self.ensure_resource_page(kind, window, cx);
                self.dock_resource_panel(kind, cx);
            }
        } else {
            self.ensure_resource_page(kind, window, cx);
            self.set_resource_compact(kind, false, cx);
            self.tabs.open(TabKind::Scene(kind.scene()));
            cx.notify();
        }
    }

    /// Creates the page entity behind a resource if it does not exist yet.
    /// Files lives for the window (its tabs survive view switches); the git
    /// panel is recreated so the next open refetches the diff snapshot.
    fn ensure_resource_page(
        &mut self,
        kind: ResourcePanelKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ResourcePanelKind::Files => {
                self.files
                    .get_or_insert_with(|| cx.new(crate::files::FilesPage::new));
            }
            ResourcePanelKind::Git => {
                if self.git.is_none() {
                    let active_session = self.active_chat().map(|chat| chat.session_id.clone());
                    self.git = Some(cx.new(|cx| GitPanel::new(active_session, window, cx)));
                }
            }
        }
    }

    /// Docks `kind` into the session tab's right panel and closes its main
    /// tab without dropping the page entity — one resource, one mount point.
    /// The panel is the page's compact form (single column; a 360px panel
    /// cannot fit the two-column main-tab layout).
    fn dock_resource_panel(&mut self, kind: ResourcePanelKind, cx: &mut Context<Self>) {
        self.set_resource_compact(kind, true, cx);
        self.resource_panel = Some(kind);
        self.tabs.remove(&TabKind::Scene(kind.scene()));
        cx.notify();
    }

    /// Flips a page between its docked (compact) and main tab (full) forms.
    fn set_resource_compact(
        &mut self,
        kind: ResourcePanelKind,
        compact: bool,
        cx: &mut Context<Self>,
    ) {
        match kind {
            ResourcePanelKind::Files => {
                if let Some(page) = &self.files {
                    page.update(cx, |page, cx| page.set_compact(compact, cx));
                }
            }
            ResourcePanelKind::Git => {
                if let Some(panel) = &self.git {
                    panel.update(cx, |panel, cx| panel.set_compact(compact, cx));
                }
            }
        }
    }

    /// Hides the right panel, remembering the dragged width so the next dock
    /// reopens at it (the terminal dock's height rule).
    fn close_resource_panel(&mut self, cx: &mut Context<Self>) {
        if let Some(size) = self.panel_split.read(cx).sizes().get(1) {
            self.panel_width =
                (*size).clamp(px(MIN_RESOURCE_PANEL_WIDTH), px(MAX_RESOURCE_PANEL_WIDTH));
        }
        self.resource_panel = None;
        cx.notify();
    }

    /// Pops the right panel out to its main scene tab (the web right-panel
    /// tab's pop-out): the panel hides, the tab opens and activates.
    fn pop_out_resource_panel(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.resource_panel else {
            return;
        };
        self.resource_panel = None;
        self.set_resource_compact(kind, false, cx);
        self.tabs.open(TabKind::Scene(kind.scene()));
        cx.notify();
    }

    /// Global search overlay (R1b): opens the palette, clears the previous
    /// query state, and focuses the query input so typing starts immediately.
    pub(crate) fn open_search_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::Focusable as _;
        self.search_overlay_open = true;
        self.search = crate::search::SearchPageState::default();
        self.search_input.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        let focus = self.search_input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Closes the palette and drops the query state, so a stale result set
    /// never lingers behind the next open. Focus blurs: the query input is
    /// about to unmount, and a dangling focus would swallow keystrokes
    /// (the render pass then parks focus back on the root).
    pub(crate) fn close_search_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_overlay_open {
            return;
        }
        self.search_overlay_open = false;
        self.search = crate::search::SearchPageState::default();
        window.blur(cx);
        cx.notify();
    }

    /// Runs the palette search (R1b): both paths at once, like the desktop's
    /// federated global search. Empty queries and in-flight runs are no-ops
    /// so the palette never floods or double-fires.
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
        handle.search_workspace_content(query.clone(), tx.clone());
        handle.search_sessions(query, tx.clone());
        cx.notify();
    }

    /// Dispatches a palette quick action and closes the overlay. Terminal
    /// needs the window (dock toggle drags a splitter); the rest are tab
    /// opens.
    pub(crate) fn run_search_quick_action(
        &mut self,
        action: crate::search::SearchQuickAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::search::SearchQuickAction as Q;
        self.close_search_overlay(window, cx);
        match action {
            Q::NewSession => {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.create_session(self.event_tx.clone());
                }
            }
            Q::Terminal => self.toggle_terminal_panel(window, cx),
            Q::Git => self.toggle_git_view(window, cx),
            Q::Files => self.toggle_files_view(window, cx),
            Q::Insights => self.toggle_insights_view(cx),
            Q::Settings => self.open_settings_view(cx),
        }
    }

    /// Jumps from a search hit to the files viewer with the file open — the
    /// same opening policy as the toggles (R2c): the active session tab's
    /// right panel, or the main Files tab when no session tab is active.
    pub(crate) fn open_file_in_files_view(&mut self, path: String, cx: &mut Context<Self>) {
        let page = self
            .files
            .get_or_insert_with(|| cx.new(crate::files::FilesPage::new));
        page.update(cx, |page, cx| page.open_path(path, cx));
        if matches!(self.tabs.active_kind(), Some(TabKind::Session(_))) {
            self.dock_resource_panel(ResourcePanelKind::Files, cx);
        } else {
            self.set_resource_compact(ResourcePanelKind::Files, false, cx);
            self.tabs.open(TabKind::Scene(SceneId::Files));
            cx.notify();
        }
    }

    /// The sessions tree flattened into sidebar children (R1c): the workspace
    /// group row, then one entry per row — top-level sessions with their
    /// subagent children indented beneath them, the running dot on live rows,
    /// and the show-more fold once the collapsed page overflows. Archived rows
    /// show only while the toggle is on; a failed list surfaces as the error
    /// line.
    fn render_session_entries(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> Vec<SidebarSection> {
        let view = cx.entity().downgrade();
        let mut sections = Vec::new();
        if let Some(error) = &self.list_error {
            sections.push(SidebarSection::Item(SidebarEntry::Danger(
                error.clone().into(),
            )));
            return sections;
        }

        let visible: Vec<&SessionRow> = self
            .sessions
            .iter()
            .filter(|row| self.show_archived || !row.archived)
            .collect();
        let group_running = visible.iter().any(|row| self.session_is_running(&row.id));
        sections.push(SidebarSection::GroupHeader {
            name: SharedString::from(
                runtime::runtime_handle()
                    .map(|handle| handle.workspace().name)
                    .unwrap_or_default(),
            ),
            collapsed: self.sessions_group_collapsed,
            running: group_running,
            view: view.clone(),
        });
        if self.sessions_group_collapsed {
            return sections;
        }
        if visible.is_empty() {
            sections.push(SidebarSection::Item(SidebarEntry::Empty(
                text.sessions_empty.clone().into(),
            )));
            return sections;
        }

        // Collapsed shows the web nav's level-0 page; expanding reveals the
        // rest (virtualized, so the count itself is cheap).
        let foldable = visible.len() > SESSIONS_COLLAPSED_VISIBLE;
        let (shown, more): (Vec<&SessionRow>, usize) = if self.sessions_expanded || !foldable {
            (visible, 0)
        } else {
            (
                visible[..SESSIONS_COLLAPSED_VISIBLE].to_vec(),
                visible.len() - SESSIONS_COLLAPSED_VISIBLE,
            )
        };

        for row in shown {
            let open = self.chats.iter().any(|chat| chat.matches(&row.id));
            sections.push(SidebarSection::Item(SidebarEntry::Session {
                row: row.clone(),
                open,
                depth: 0,
                running: self.session_is_running(&row.id),
                tx: self.event_tx.clone(),
                view: view.clone(),
            }));
            for child in self.lineage_children(&row.id) {
                let child_open = self.chats.iter().any(|chat| chat.matches(&child.id));
                let child_running = self.session_is_running(&child.id);
                sections.push(SidebarSection::Item(SidebarEntry::Session {
                    row: child,
                    open: child_open,
                    depth: 1,
                    running: child_running,
                    tx: self.event_tx.clone(),
                    view: view.clone(),
                }));
            }
        }
        // The fold row only exists when it can do something: reveal hidden
        // roots, or re-fold an expansion that actually hid something.
        if more > 0 || (self.sessions_expanded && foldable) {
            sections.push(SidebarSection::Item(SidebarEntry::More {
                remaining: more,
                expanded: self.sessions_expanded,
                view,
            }));
        }
        sections
    }

    /// A turn is executing for this session, whether its tab is open or not:
    /// the event pump reports every session's lifecycle process-wide.
    fn session_is_running(&self, session_id: &str) -> bool {
        self.live_sessions.contains(session_id)
    }

    /// Direct subagent children of a session (R1c), as sidebar rows: the
    /// lineage snapshot rooted at the session, filtered to depth one —
    /// deeper descendants stay hidden, matching the web nav's two-level
    /// projection. Children keep their pre-order arrival order.
    fn lineage_children(&self, root_session_id: &str) -> Vec<SessionRow> {
        let Some(snapshot) = self
            .lineages
            .iter()
            .find(|snapshot| snapshot.root_session_id == root_session_id)
        else {
            return Vec::new();
        };
        snapshot
            .sessions
            .iter()
            .filter(|entry| entry.parent_session_id.as_deref() == Some(root_session_id))
            .map(|entry| SessionRow {
                id: entry.session_id.clone(),
                name: entry.session_name.clone(),
                metadata: entry.subagent_type.clone().unwrap_or_default(),
                archived: false,
            })
            .collect()
    }

    /// The navigation sidebar (R1b, desktop NavPanel shape): the "search
    /// everything" trigger in the header, the three top-scene nav rows, the
    /// sessions section (titled header with new/archive/reload actions above
    /// the live list), and the runtime status line in the footer. R1d moved
    /// the footer tool buttons to the window-bottom global status bar.
    fn render_sidebar(&self, text: &ShellText, cx: &mut Context<Self>) -> Sidebar<SidebarSection> {
        let header = div().w_full().child(
            div()
                .id("search-everything")
                .debug_selector(|| "search-everything".to_string())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_search_overlay(window, cx);
                }))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_x_2()
                .h_8()
                .px_2()
                .rounded(cx.theme().radius)
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .hover(|style| style.bg(cx.theme().sidebar_accent))
                .child(Icon::new(IconName::Search).small())
                .child(div().flex_1().truncate().child(text.search_trigger.clone()))
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_tokens().sm)
                        .child("Ctrl K"),
                ),
        );

        // Nav rows first (web-ui NavPanel order: top scenes above the session
        // list), then the sessions section with the live list.
        let nav_rows: Vec<SidebarEntry> = nav_actions()
            .into_iter()
            .map(|action| SidebarEntry::Nav {
                action,
                label: SharedString::from(nav_label(action, text)),
                active: action.is_active(&self.tabs),
                view: cx.entity().downgrade(),
            })
            .collect();

        Sidebar::new("shell-sidebar")
            .w(px(SESSION_PANE_WIDTH))
            .header(header)
            .child(SidebarSection::Rows(nav_rows))
            .child(SidebarSection::SectionHeader {
                label: SharedString::from(text.sessions_title.clone()),
                tx: self.event_tx.clone(),
                archived_on: self.show_archived,
                view: cx.entity().downgrade(),
            })
            .children(self.render_session_entries(text, cx))
            // No component footer: the runtime status line and the tool
            // buttons moved to the nav column's own footer row (the web
            // PersistentFooterActions shape), which the collapse folds away
            // together with the column.
            .footer(SidebarFooter::new())
    }

    /// The sidebar footer (web `PersistentFooterActions`): the user slot and
    /// the trinity lamp chip on the left, the tool buttons on the right —
    /// terminal dock toggle, workspace switcher, settings. The runtime status
    /// line rides above the row only while it has something to say (starting,
    /// failed, or a workspace error); the all-clear "ready" line the old
    /// window-bottom status bar carried is ambient noise the desktop footer
    /// does not repeat.
    fn render_sidebar_footer(&self, text: &ShellText, cx: &mut Context<Self>) -> impl IntoElement {
        let phase = crate::trinity::chip_phase(
            self.trinity
                .status
                .as_ref()
                .map(|(_, status)| status.awakened),
        );
        let (chip_icon, chip_label, chip_color) = match phase {
            crate::trinity::ChipPhase::Awake => (
                IconName::Sparkles,
                text.trinity_phase_awake.clone(),
                cx.theme().foreground,
            ),
            crate::trinity::ChipPhase::Dormant => (
                IconName::BrainCircuit,
                text.trinity_phase_dormant.clone(),
                cx.theme().muted_foreground,
            ),
            crate::trinity::ChipPhase::Offline => (
                IconName::BrainCircuit,
                text.trinity_phase_offline.clone(),
                cx.theme().muted_foreground,
            ),
        };

        let mut left = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .max_w(px(160.))
                    .truncate()
                    .child(SharedString::from(status_bar_user(
                        self.trinity.status.as_ref().map(|(_, status)| &**status),
                        &self.os_user,
                    ))),
            )
            .child(
                div()
                    .id("status-trinity-chip")
                    .debug_selector(|| "status-trinity-chip".to_string())
                    .flex()
                    .items_center()
                    .gap_1()
                    .h_5()
                    .px_1p5()
                    .rounded(cx.theme().radius_tokens().sm)
                    .text_xs()
                    .text_color(chip_color)
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().muted))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_trinity_view(cx)))
                    .child(Icon::new(chip_icon).small())
                    .child(div().truncate().child(chip_label)),
            );
        if let crate::trinity::ChipPhase::Awake = phase {
            if let Some((psi, _)) = &self.trinity.status {
                let dot_color = if crate::trinity::emotion_dot_positive(&psi.valence) {
                    cx.theme().success
                } else if psi
                    .valence
                    .as_deref()
                    .is_some_and(|v| v.starts_with("negative"))
                {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                };
                left = left.child(
                    div()
                        .size_1p5()
                        .rounded_full()
                        .bg(dot_color)
                        .flex_shrink_0(),
                );
            }
        }

        let terminal_variant = if self.terminal_open {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        };
        let status_line = match &self.phase {
            RuntimePhase::Starting => {
                Some((text.status_starting.clone(), cx.theme().muted_foreground))
            }
            RuntimePhase::Failed(error) => Some((error.clone(), cx.theme().danger)),
            RuntimePhase::Ready => self
                .workspace_error
                .clone()
                .map(|error| (error, cx.theme().danger)),
        };
        div()
            .id("shell-sidebar-footer")
            .debug_selector(|| "shell-sidebar-footer".to_string())
            .w_full()
            .flex_none()
            .v_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_t_1()
            .border_color(cx.theme().border)
            .when_some(status_line, |this, (line, color)| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(color)
                        .min_w_0()
                        .truncate()
                        .child(line),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .min_w_0()
                    .child(left)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .flex_shrink_0()
                            .child(
                                Button::new("status-terminal")
                                    .icon(IconName::SquareTerminal)
                                    .small()
                                    .with_variant(terminal_variant)
                                    .tooltip(text.terminal_title.clone())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_terminal_panel(window, cx)
                                    })),
                            )
                            .child(self.render_workspace_button(text, cx))
                            .child(self.render_settings_button(text, cx)),
                    ),
            )
    }

    /// The settings entry: theme and language moved into the settings page
    /// (M8), so the footer keeps a single button.
    fn render_settings_button(&self, text: &ShellText, cx: &mut Context<Self>) -> Button {
        Button::new("sidebar-settings")
            .icon(IconName::Settings)
            .small()
            .with_variant(active_variant_settings(&self.tabs))
            .tooltip(text.settings_open.clone())
            .on_click(cx.listener(|this, _, _, cx| this.open_settings_view(cx)))
    }
}

/// The settings button keeps a raised variant while its page is showing.
fn active_variant_settings(tabs: &TabsState) -> ButtonVariant {
    if tabs.active_kind() == Some(&TabKind::Scene(SceneId::Settings)) {
        ButtonVariant::Secondary
    } else {
        ButtonVariant::Ghost
    }
}

/// The status bar's user slot (R1d): the trinity daemon's name for the user
/// wins while connected; the OS login backs the slot when it is not.
fn status_bar_user<'a>(
    status: Option<&'a crate::trinity::StatusInfo>,
    os_user: &'a str,
) -> &'a str {
    status
        .and_then(|status| status.user_name.as_deref())
        .filter(|name| !name.is_empty())
        .unwrap_or(os_user)
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
/// belong to the [`MessageScroller`] row wrapper plus the spacing the chat
/// area's row closure applies (R2d); the reading column is applied by that
/// closure around this item.
fn render_chat_item(
    entry: &ChatEntry,
    text: &ShellText,
    fork: Option<&ChatForkContext>,
    row: usize,
    user_expanded: bool,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    match entry {
        ChatEntry::User {
            text: body,
            turn_id: _,
        } => {
            // Content-hash key: the expansion flag survives transcript
            // reloads that re-index entries; the row index keeps the element
            // id unique when the same prompt text appears twice.
            let key = markdown_id_key(body);
            let column = div()
                .debug_selector(|| "user-bubble".to_string())
                .id(ElementId::Name(
                    format!("user-bubble-{row}-{:016x}", key).into(),
                ))
                .w_full()
                // A column wrapper on purpose: inside a row-flex `justify_end`
                // parent the Bubble lays out but never paints (gpui-pre 0.7.1
                // stack, reproduced 2026-10-09). The web fraction ships as a
                // definite pixel cap (`min(72%, 48rem)` at the 900px column):
                // a relative() max-width does not constrain the content inside
                // the virtualized row (measured 2026-10-12), so the shell owns
                // the cap and the right alignment on its own wrapper.
                .v_flex()
                .cursor_pointer()
                // Web `UserMessageItem`: three lines collapsed, click expands
                // (the toggle is free — the bubble has no other interaction).
                .on_click({
                    let view = fork.map(|fork| fork.view.clone());
                    let session_id = fork.map(|fork| fork.session_id.clone());
                    move |_, _, cx| {
                        if let (Some(view), Some(session_id)) = (&view, &session_id) {
                            let _ = view.update(cx, |shell, cx| {
                                if let Some(chat) = shell.chat_mut_for(session_id) {
                                    if chat.expanded_user_texts.contains(&key) {
                                        chat.expanded_user_texts.remove(&key);
                                    } else {
                                        chat.expanded_user_texts.insert(key);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    }
                })
                .child(
                    div()
                        .debug_selector(|| "user-bubble-cap".to_string())
                        .self_end()
                        .max_w(px(USER_BUBBLE_MAX_WIDTH))
                        .min_w_0()
                        .child(
                            Bubble::new()
                                .with_variant(BubbleVariant::Muted)
                                // Web surface recipe (R2d): the component's
                                // default radius_2xl/padding read visibly
                                // airier than the web's 10px/10px/7px bubble.
                                .content(
                                    BubbleContent::new()
                                        .rounded(px(USER_BUBBLE_RADIUS))
                                        .px(px(USER_BUBBLE_PADDING_X))
                                        .py(px(USER_BUBBLE_PADDING_Y)),
                                )
                                .w_full()
                                .child(
                                    div()
                                        .when(!user_expanded, |this| this.line_clamp(3))
                                        .child(body.clone()),
                                ),
                        ),
                );
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

/// What the turn footer (R2d) shows under a finished turn's tail entry: the
/// fork boundary and the copy scopes derived from the turn's own rows. Zero
/// new runtime facts — everything comes from the transcript vec.
struct TurnFooterPlan {
    /// The turn's user prompt id (fork boundary); `None` leaves the fork
    /// entry out (transcript head without a persisted prompt id).
    turn_id: Option<String>,
    /// Whole-turn copy text (`User\n…` + every assistant round, web
    /// `formatTranscriptTurnText` shape); `None` when the turn has no
    /// assistant text and the copy entry hides.
    copy_full: Option<String>,
    /// Result-scope copy: the final assistant round.
    copy_result: Option<String>,
}

/// The finished-turn tail test for row `index`: the entry must close a turn
/// (the next row is a user prompt, or the transcript ends) and that turn must
/// be complete — a tail row while `busy`/`submitting` is still streaming.
/// Notice/info rows carry no actions of their own and never host the footer.
fn turn_footer_plan(
    entries: &[ChatEntry],
    index: usize,
    busy: bool,
    submitting: bool,
    text: &ShellText,
) -> Option<TurnFooterPlan> {
    match entries.get(index)? {
        ChatEntry::User { .. } | ChatEntry::Notice { .. } | ChatEntry::Info { .. } => {
            return None;
        }
        ChatEntry::Assistant { .. } | ChatEntry::Tool(_) => {}
    }
    match entries.get(index + 1) {
        Some(ChatEntry::User { .. }) => {}
        // Transcript tail: only a finished turn hosts a footer.
        None if busy || submitting => return None,
        None => {}
        Some(_) => return None,
    }
    // Backward scan to the turn's prompt: the first assistant met is the
    // final round (the copy result), the prompt supplies the fork boundary.
    let mut rounds: Vec<&str> = Vec::new();
    let mut turn: Option<(Option<String>, String)> = None;
    for entry in entries[..=index].iter().rev() {
        match entry {
            ChatEntry::User { text, turn_id } => {
                turn = Some((turn_id.clone(), text.clone()));
                break;
            }
            ChatEntry::Assistant { text, .. } => rounds.push(text),
            _ => {}
        }
    }
    let copy_result = rounds.first().map(|text| (*text).to_string());
    let copy_full = if rounds.is_empty() {
        None
    } else {
        let mut parts = Vec::new();
        if let Some((_, prompt)) = &turn {
            parts.push(format!("{}\n{}", text.turn_copy_user_label, prompt));
        }
        parts.extend(rounds.iter().rev().map(|text| (*text).to_string()));
        Some(parts.join("\n\n---\n\n"))
    };
    Some(TurnFooterPlan {
        turn_id: turn.map(|(turn_id, _)| turn_id).flatten(),
        copy_full,
        copy_result,
    })
}

/// The finished-turn footer (R2d, web `ModelRoundItem` metadata row): fork /
/// copy / snapshots, right-aligned under the turn's last entry. Every entry
/// hooks an existing capability — the per-turn fork menu, the clipboard, the
/// git snapshots view.
fn turn_footer(
    plan: TurnFooterPlan,
    row: usize,
    fork: &ChatForkContext,
    text: &ShellText,
) -> gpui_kit::AnyElement {
    let mut actions = div()
        .debug_selector(|| "turn-footer".to_string())
        .mt_1()
        .flex()
        .items_center()
        .justify_end()
        .gap_0p5();
    if let Some(turn_id) = plan.turn_id {
        actions = actions.child(turn_fork_menu(
            fork.session_id.clone(),
            turn_id,
            fork.tx.clone(),
            text,
        ));
    }
    if let (Some(result), Some(full)) = (plan.copy_result, plan.copy_full) {
        actions = actions.child(
            Button::new(ElementId::Name(format!("turn-copy-{row}").into()))
                .icon(IconName::Copy)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.turn_copy.clone())
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, cx| {
                    let text = i18n::current(cx);
                    let full = full.clone();
                    let result = result.clone();
                    menu.item(PopupMenuItem::new(text.turn_copy_full.clone()).on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(full.clone()));
                        },
                    ))
                    .item(
                        PopupMenuItem::new(text.turn_copy_result.clone()).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(result.clone()));
                            },
                        ),
                    )
                }),
        );
    }
    let view = fork.view.clone();
    actions = actions.child(
        Button::new(ElementId::Name(format!("turn-snapshot-{row}").into()))
            .icon(IconName::Archive)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.git_snapshots_view.clone())
            .on_click(move |_, window, cx| {
                if let Some(view) = view.upgrade() {
                    let _ = view.update(cx, |shell, cx| shell.open_git_snapshots(window, cx));
                }
            }),
    );
    actions.into_any_element()
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
    fn render_chat_area(
        &self,
        session_id: &str,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.chat_for(session_id) else {
            return brand_empty(IconName::Sparkles, &text.chat_empty_title, &text.chat_empty);
        };
        let scroller = chat.scroller.clone();
        let session_id = session_id.to_string();
        let this = cx.entity();
        let jump_label = text.chat_jump_latest.clone();
        let text = text.clone();
        // The scroller's uniform inter-row gap stands in for a CSS `gap`;
        // transcript rows own their rhythm instead (turn boundary 32px,
        // in-turn 8/12px below), so the wrapper's bottom padding zeroes out.
        let mut row_style = StyleRefinement::default();
        row_style.padding.bottom = Some(px(0.).into());
        MessageScroller::new("transcript", scroller, move |index, _window, cx| {
            let snapshot = this.read(cx);
            let chat = snapshot.chat_for(&session_id);
            let entry = chat.and_then(|chat| chat.entries.get(index)).cloned();
            let footer_plan = chat.and_then(|chat| {
                turn_footer_plan(&chat.entries, index, chat.busy, chat.submitting, &text)
            });
            let user_expanded = match (&entry, chat) {
                (Some(ChatEntry::User { text: body, .. }), Some(chat)) => {
                    chat.expanded_user_texts.contains(&markdown_id_key(body))
                }
                _ => false,
            };
            let fork = chat.map(|chat| ChatForkContext {
                session_id: chat.session_id.clone(),
                tx: snapshot.event_tx.clone(),
                view: this.downgrade(),
            });
            let is_user = matches!(entry, Some(ChatEntry::User { .. }));
            match entry {
                Some(entry) => {
                    let mut row = div()
                        .debug_selector(|| "chat-row".to_string())
                        .w_full()
                        .max_w(px(READING_COLUMN_WIDTH))
                        .mx_auto();
                    if is_user {
                        // The turn boundary opens above every prompt but the
                        // transcript's first row; the prompt hugs its answer.
                        if index > 0 {
                            row = row.mt(px(TURN_GAP));
                        }
                        row = row.mb(px(FLOW_ITEM_GAP));
                    } else {
                        row = row.mb(px(TURN_RHYTHM_GAP));
                    }
                    row = row.child(render_chat_item(
                        &entry,
                        &text,
                        fork.as_ref(),
                        index,
                        user_expanded,
                        cx,
                    ));
                    if let (Some(plan), Some(fork)) = (footer_plan, fork.as_ref()) {
                        row = row.child(turn_footer(plan, index, fork, &text));
                    }
                    row.into_any_element()
                }
                // Scroller item counts are synced on the same mutation
                // that grows `entries`, so an out-of-range index cannot
                // persist.
                None => div().into_any_element(),
            }
        })
        .with_row_style(row_style)
        .with_jump_button_label(jump_label)
        .with_bottom_fade(cx.theme().background)
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    /// Streaming indicator rendered as a fixed row between the transcript and
    /// the composer (list items are reserved for transcript entries).
    fn busy_label(&self, text: &ShellText) -> Option<String> {
        let chat = self.active_chat()?;
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
            .rounded(cx.theme().radius)
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                img(item.preview.clone())
                    .size(px(24.))
                    .rounded(cx.theme().radius_tokens().sm)
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
        let Some(chat) = self.active_chat() else {
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
            // A `/btw` side-question turn lives in the coordinator as a
            // dialog turn (the D68 direct-call pipeline), not in the agent
            // runtime's session loop: the stop must target the recorded
            // dialog turn, or it would be a silent no-op.
            let btw_cancel = chat
                .child_context
                .as_ref()
                .filter(|child| child.kind == ChildKind::Btw)
                .and_then(|child| child.btw_turn_id.clone())
                .map(|turn_id| (session_id.clone(), turn_id));
            // Web cancel shape: a round button holding a small square.
            Button::new("chat-stop")
                .icon(IconName::Square)
                .with_variant(ButtonVariant::Danger)
                .rounded(ButtonRounded::Size(px(999.)))
                .tooltip(text.chat_stop.clone())
                .on_click(move |_, _, _| {
                    if let Some(handle) = runtime::runtime_handle() {
                        match &btw_cancel {
                            Some((child_session_id, turn_id)) => {
                                handle.cancel_btw(child_session_id.clone(), turn_id.clone())
                            }
                            None => handle.cancel_turn(session_id.clone()),
                        }
                    }
                })
        } else {
            // Web send shape: a round primary button holding an up arrow.
            Button::new("chat-send")
                .icon(IconName::ArrowUp)
                .with_variant(ButtonVariant::Primary)
                .rounded(ButtonRounded::Size(px(999.)))
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
            // Context strip above the card (R2a, the web ChatComposer
            // contextBar): where the session runs and what the next turn may
            // do, as a bare muted track under the capsule.
            .child(self.render_composer_context_strip(text, cx))
            .child(
                div()
                    .debug_selector(|| "composer-card".to_string())
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
                        div().flex().flex_row().child(
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
                        ),
                    )
                    // Action row under the input (web expanded layout): the
                    // boost cluster on the left, the model picker and the
                    // round send button share the right cluster.
                    .child(
                        div()
                            .debug_selector(|| "composer-actions-row".to_string())
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(self.render_composer_boost_menu(text, cx))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .debug_selector(|| "composer-model-picker".to_string())
                                            .child(self.render_model_picker(text, cx)),
                                    )
                                    .child(action_button),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The composer context strip (R2a, the web `ChatInputWorkspaceStrip`):
    /// left rail names where the session runs — workspace and mode — and the
    /// right rail states the next turn's contract — local execution and the
    /// permission preset. Every value is a read-only fact projected from an
    /// existing snapshot (workspace handle, chat agent type, shell config);
    /// the strip adds no controls, so editing stays with its owner surface
    /// (workspace switching in the status bar, the preset on the settings
    /// permission page).
    fn render_composer_context_strip(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let workspace = runtime::runtime_handle()
            .map(|handle| handle.workspace().name)
            .unwrap_or_default();
        let mode = self
            .active_chat()
            .map(|chat| chat.agent_type.clone())
            .unwrap_or_default();
        let (permission_icon, permission_label, permission_full_access) =
            match settings::ShellConfig::try_current(cx).map(|config| config.permission_preset) {
                Some(preset) => Self::permission_badge(preset, text),
                None => (IconName::Shield, text.permission_ask.clone(), false),
            };

        let fact = |label: String| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(div().truncate().max_w(px(180.)).child(label))
        };
        let divider = || div().w_px().h_3().flex_shrink_0().bg(cx.theme().border);

        let has_workspace = !workspace.is_empty();
        let has_mode = !mode.is_empty();
        div()
            .debug_selector(|| "composer-context-strip".to_string())
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_2()
            .pb_1()
            .min_h(px(20.))
            // Left rail: where this session runs.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .when(has_workspace, |this| this.child(fact(workspace.clone())))
                    .when(has_workspace && has_mode, |this| this.child(divider()))
                    .when(has_mode, |this| this.child(fact(mode))),
            )
            // Right rail: what the next turn may do.
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Laptop).small())
                            .child(text.composer_local.clone()),
                    )
                    .child(
                        // A pill button, not a bare fact: the web trigger
                        // carries the mode description as its tooltip and its
                        // menu ends in "open default settings" — the click
                        // opens the settings scene that owns the preset. No
                        // mode switching here: the shell's preset surface
                        // stays on the settings permission page.
                        div()
                            .debug_selector(|| "composer-permission-badge".to_string())
                            .child(
                                Button::new("composer-permission-badge-btn")
                                    .xsmall()
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .color(cx.theme().sidebar_accent)
                                            .foreground(cx.theme().muted_foreground),
                                    )
                                    .rounded(ButtonRounded::Size(px(999.)))
                                    .font_medium()
                                    .icon(Icon::new(permission_icon).small().text_color(
                                        if permission_full_access {
                                            cx.theme().danger
                                        } else {
                                            cx.theme().success
                                        },
                                    ))
                                    .label(permission_label)
                                    .tooltip(if permission_full_access {
                                        text.permission_full_access_desc.clone()
                                    } else {
                                        text.permission_ask_desc.clone()
                                    })
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.open_settings_view(cx)),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Maps the settings permission preset onto the strip badge: icon ramp
    /// (web `PERMISSION_MODE_ICONS` — the shield gains a mark as
    /// confirmation is given up), label, and whether the icon takes the
    /// danger ink of the full-access step (`false` = the success ink of ask).
    fn permission_badge(
        preset: openbitfun_runtime_ports::PermissionPolicyPreset,
        text: &ShellText,
    ) -> (IconName, String, bool) {
        match preset {
            openbitfun_runtime_ports::PermissionPolicyPreset::Ask => {
                (IconName::Shield, text.permission_ask.clone(), false)
            }
            openbitfun_runtime_ports::PermissionPolicyPreset::FullAccess => (
                IconName::ShieldAlert,
                text.permission_full_access.clone(),
                true,
            ),
        }
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

/// Stored label/prompt of the built-in quick actions in the shared config
/// defaults (English; `config-contracts` writes the same values). The
/// localized copy replaces them only while the stored text still equals
/// these defaults — a customized action keeps its stored text verbatim.
const QUICK_ACTION_COMMIT_DEFAULTS: (&str, &str) = ("Commit", "Commit all current code changes");
const QUICK_ACTION_CREATE_PR_DEFAULTS: (&str, &str) =
    ("Create PR", "Create a Pull Request for the current branch");

/// One stored-vs-default text of a built-in quick action (web
/// `resolveQuickActionText` resolves label and prompt independently).
fn builtin_quick_action_text(stored: &str, default: &str, localized: &str) -> String {
    if stored == default {
        localized.to_string()
    } else {
        stored.to_string()
    }
}

/// Resolves a quick action's display label and the prompt a click sends
/// (web `resolveQuickActionText` port): built-in `commit`/`create_pr`
/// localize the shared English defaults, everything else stays verbatim.
fn resolve_quick_action_text(
    action: &openbitfun_core::service::config::types::AiExperienceQuickAction,
    text: &ShellText,
) -> (String, String) {
    match action.id.as_str() {
        "commit" => (
            builtin_quick_action_text(
                &action.label,
                QUICK_ACTION_COMMIT_DEFAULTS.0,
                &text.quick_action_commit_label,
            ),
            builtin_quick_action_text(
                &action.prompt,
                QUICK_ACTION_COMMIT_DEFAULTS.1,
                &text.quick_action_commit_prompt,
            ),
        ),
        "create_pr" => (
            builtin_quick_action_text(
                &action.label,
                QUICK_ACTION_CREATE_PR_DEFAULTS.0,
                &text.quick_action_create_pr_label,
            ),
            builtin_quick_action_text(
                &action.prompt,
                QUICK_ACTION_CREATE_PR_DEFAULTS.1,
                &text.quick_action_create_pr_prompt,
            ),
        ),
        _ => (action.label.clone(), action.prompt.clone()),
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
                self.toggle_git_view(window, cx);
            }
            if self.plugin_ui_auto_open && !self.plugin_ui.scenes().is_empty() {
                self.plugin_ui_auto_open = false;
                // A contributed scene installed (env-gated test contribution):
                // the R7b fixture installs at construction; the R7c host
                // channel lands later through PluginUiBridgeReady, so the
                // flag holds until the projection actually has a scene.
                if let Some(handle) = self.plugin_ui.scenes().first() {
                    let scene = handle.scene_ref;
                    self.open_plugin_scene(scene, cx);
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

        let main_area = match self.tabs.active_kind().cloned() {
            Some(TabKind::Scene(SceneId::Settings)) => {
                settings::render_page(&text, cx.entity().downgrade(), cx)
            }
            Some(TabKind::Scene(SceneId::Git)) => match self.git.clone() {
                Some(panel) => div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(panel)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            Some(TabKind::Scene(SceneId::Insights)) => div()
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
            Some(TabKind::Scene(SceneId::Tasks)) => div()
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
            Some(TabKind::Scene(SceneId::Files)) => match self.files.clone() {
                Some(page) => div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(page)
                    .into_any_element(),
                None => div().into_any_element(),
            },
            Some(TabKind::Scene(SceneId::Trinity)) => div()
                .id("trinity-page")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(crate::trinity::render_page(
                    &self.trinity,
                    cx.entity().downgrade(),
                    &self.trinity_recall_input,
                    &self.trinity_add_content,
                    &self.trinity_add_project,
                    &self.event_tx,
                    &text,
                    cx,
                ))
                .into_any_element(),
            // R7 seam: plugin-contributed scenes dispatch through the
            // contribution (one arm, not one per scene).
            Some(TabKind::Scene(SceneId::External(reference))) => {
                self.render_external_scene(reference, window, &text, cx)
            }
            Some(TabKind::Session(session_id)) => self.render_session_main(&session_id, &text, cx),
            // No tab at all: the tabless landing face (R2b, the web
            // WelcomeScene) — brand slogan plus one input that starts a
            // session. The brand-empty panel stays for the "tab open but
            // transcript not loaded" fallback inside render_chat_area.
            None => self.render_welcome_face(&text, cx),
        };

        // Nothing focused (fresh window, or after the palette closed): the
        // root parks window focus so keystroke bindings like Ctrl K keep a
        // chain to dispatch through.
        if window.focused(cx).is_none() {
            window.focus(&self.root_focus, cx);
        }

        // Body: two full-height columns, the desktop WorkspaceBody shape —
        // the nav column (its own 45px top bar, the sidebar, the footer with
        // the user/trinity cluster and the tool buttons) and the scene face
        // (a 45px top bar with the tab strip, the session actions, and the
        // window controls, over the dock frame). No standalone title bar and
        // no full-width bottom status bar: the desktop dropped both when it
        // simplified its shell (`92ce04f8d`), and the drawer fold takes the
        // whole nav column out of the layout.
        let nav_column = div()
            .id("shell-nav-column")
            .debug_selector(|| "shell-nav-column".to_string())
            .h_full()
            .flex_none()
            .v_flex()
            .child(self.render_nav_top_bar(&text, cx))
            .child(
                div()
                    .id("shell-body-sidebar")
                    .debug_selector(|| "shell-body-sidebar".to_string())
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(&text, cx)),
            )
            .child(self.render_sidebar_footer(&text, cx));

        let scene_column = div()
            .id("shell-scene-column")
            .debug_selector(|| "shell-scene-column".to_string())
            .h_full()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .v_flex()
            .child(self.render_scene_top_bar(window, &text, cx))
            // The scene body and the docked terminal are one vertical split:
            // the divider above the terminal drags its height, and the scene
            // keeps the rest. Closed, the terminal panel is simply absent.
            .child(
                div()
                    .id("shell-dock-frame")
                    .debug_selector(|| "shell-dock-frame".to_string())
                    .w_full()
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
            );

        div()
            .id("shell-root")
            .relative()
            .track_focus(&self.root_focus)
            .size_full()
            .flex()
            .flex_row()
            .text_color(cx.theme().foreground)
            // Ctrl K opens the global search palette from wherever the focus
            // sits (the terminal pane and text editors consume keys first —
            // same as the desktop, where the shortcut is also not global over
            // inputs that eat it). Ctrl B folds/unfolds the nav column.
            .on_action(cx.listener(|this, _: &OpenGlobalSearch, window, cx| {
                this.open_search_overlay(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .when(self.sidebar_open, |this| this.child(nav_column))
            .child(scene_column)
            .when(!self.sidebar_open, |this| {
                // The floating collapsed-nav strip (web WorkspaceBody): a
                // small opener pinned to the window's top-left corner while
                // the column is folded away.
                this.child(
                    div()
                        .id("shell-collapsed-nav")
                        .debug_selector(|| "shell-collapsed-nav".to_string())
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(px(NAV_COLLAPSED_STRIP_WIDTH))
                        .h(px(TOP_BAR_HEIGHT))
                        .flex()
                        .items_center()
                        .px(px(7.))
                        .child(
                            Button::new("sidebar-expand")
                                .icon(IconName::PanelLeft)
                                .small()
                                .with_variant(ButtonVariant::Ghost)
                                .tooltip(text.sidebar_toggle.clone())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sidebar_open = true;
                                    cx.notify();
                                })),
                        ),
                )
            })
            // The global search palette (R1b) paints last so it overlays the
            // sidebar, main area, and docked terminal alike.
            .when(self.search_overlay_open, |this| {
                this.child(crate::search::render_overlay(
                    cx.entity().downgrade(),
                    &self.search,
                    &self.search_input,
                    &self.event_tx,
                    &text,
                    cx,
                ))
            })
    }
}

impl ShellWindow {
    /// The scene top bar (web `SceneTopBar`, the desktop's right 45px chrome
    /// row): the tab strip leads — horizontally scrollable, so a full strip
    /// never covers the trailing actions — then the active session's chrome
    /// (usage, quick actions, the right resource panel toggle), then the
    /// window controls. The empty stretch between tabs and actions drags the
    /// window; double-click zooms, the contract the removed component
    /// `TitleBar` used to honor.
    fn render_scene_top_bar(
        &self,
        window: &mut Window,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut tabs = div()
            .id("shell-top-tabs")
            .debug_selector(|| "shell-top-tabs".to_string())
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .overflow_x_scroll();
        for (index, kind) in self.tabs.kinds().iter().enumerate() {
            let active = self.tabs.active_index() == Some(index);
            let (icon, label, tab_id) = match kind {
                TabKind::Session(session_id) => (
                    IconName::Sparkles,
                    self.chat_for(session_id)
                        .map(|chat| chat.name.clone())
                        .unwrap_or_else(|| session_id.clone()),
                    format!("top-tab-session-{index}"),
                ),
                TabKind::Scene(scene) => {
                    let def = crate::tabs::builtin_scenes()
                        .into_iter()
                        .find(|row| row.id == *scene);
                    let (icon, label) = match scene {
                        // Plugin scenes carry icon and title in the
                        // contribution; the builtin table has no row.
                        SceneId::External(reference) => (
                            self.plugin_scene_icon(*reference),
                            self.plugin_scene_label(*reference, text),
                        ),
                        _ => (
                            def.map(|row| row.icon).unwrap_or(IconName::Square),
                            crate::tabs::scene_title(*scene, text)
                                .unwrap_or_default()
                                .to_string(),
                        ),
                    };
                    (
                        icon,
                        label,
                        format!(
                            "top-tab-scene-{}-{index}",
                            format!("{scene:?}").to_lowercase()
                        ),
                    )
                }
            };
            let close_label = text.tab_close.clone();
            tabs = tabs.child(
                div()
                    .id(tab_id)
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_2()
                    .pr_1()
                    .h_7()
                    .rounded(cx.theme().radius)
                    .text_sm()
                    .flex_shrink_0()
                    .max_w(px(220.))
                    .when(active, |chip| {
                        chip.bg(cx.theme().secondary)
                            .text_color(cx.theme().secondary_foreground)
                    })
                    .when(!active, |chip| {
                        chip.text_color(cx.theme().muted_foreground)
                            .hover(|style| style.bg(cx.theme().muted))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tabs.activate(index);
                        this.list_sync_reset(cx);
                        cx.notify();
                    }))
                    .child(icon)
                    .child(
                        div()
                            .min_w_0()
                            .line_clamp(1)
                            .child(SharedString::from(label)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("top-tab-close-{index}")))
                            .icon(IconName::X)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(close_label)
                            .disabled(!active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_top_tab(index, cx);
                            })),
                    ),
            );
        }

        // Session chrome rides the top bar only while a session tab is the
        // active one (web SceneChromeHost: the active scene contributes).
        let session_active = matches!(self.tabs.active_kind(), Some(TabKind::Session(_)));
        let panel_icon = if self.resource_panel.is_some() {
            IconName::PanelRightClose
        } else {
            IconName::PanelRightOpen
        };
        let panel_label = if self.resource_panel.is_some() {
            text.panel_close.clone()
        } else {
            text.panel_open.clone()
        };

        // Window controls: the same gating the component TitleBar applies —
        // none on macOS (traffic lights), none under server-side decorations
        // on Linux, and only the buttons the window manager can honor. The
        // cluster renders at the end of the bar; the divider is painted only
        // when the cluster exists, so it is folded into the element itself.

        div()
            .id("shell-scene-top-bar")
            .debug_selector(|| "shell-scene-top-bar".to_string())
            .h(px(TOP_BAR_HEIGHT))
            .flex_none()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().background)
            // The folded-nav floating strip owns the corner; keep the tabs
            // clear of it.
            .when(!self.sidebar_open, |bar| {
                bar.pl(px(NAV_COLLAPSED_STRIP_WIDTH))
            })
            .on_double_click(|_, window, _| window.zoom_window())
            .child(tabs)
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .when(session_active, |bar| {
                bar.child(self.render_usage_button(cx))
                    .child(self.render_quick_actions_menu(text, cx))
                    .child(
                        Button::new("top-bar-resource-panel")
                            .icon(panel_icon)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(panel_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.resource_panel.is_some() {
                                    this.close_resource_panel(cx);
                                } else {
                                    this.dock_resource_panel(ResourcePanelKind::Files, cx);
                                }
                            })),
                    )
            })
            .children(self.render_window_controls(window))
    }

    /// The window control cluster (minimize / zoom / close), shaped after the
    /// component `TitleBar`'s private `WindowControls`: client-side
    /// decorations only, and only the buttons the platform supports. Close
    /// keeps the M22 semantics — quit goes through the event pump so the
    /// terminal PTYs shut down first, otherwise the window minimizes to the
    /// tray.
    fn render_window_controls(&self, window: &mut Window) -> Option<gpui_kit::AnyElement> {
        if cfg!(target_os = "macos") || cfg!(target_family = "wasm") {
            return None;
        }
        #[cfg(target_os = "linux")]
        if !matches!(
            window.window_decorations(),
            gpui_kit::Decorations::Client { .. }
        ) {
            return None;
        }
        let supported = window.window_controls();
        let maximized = window.is_maximized();
        let quit_tx = self.event_tx.clone();
        Some(
            div()
                .id("window-controls")
                .flex()
                .items_center()
                .flex_shrink_0()
                .when(supported.minimize, |this| {
                    this.child(
                        Button::new("window-minimize")
                            .icon(IconName::Minus)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                })
                .when(supported.maximize, |this| {
                    this.child(
                        Button::new("window-zoom")
                            .icon(if maximized {
                                IconName::Copy
                            } else {
                                IconName::Square
                            })
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, _| window.zoom_window()),
                    )
                })
                .child(
                    Button::new("window-close")
                        .icon(IconName::X)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, window, _| {
                            if CLOSE_QUITS.load(std::sync::atomic::Ordering::Relaxed) {
                                let _ = quit_tx.send(RuntimeEvent::QuitRequested);
                            } else {
                                window.minimize_window();
                            }
                        }),
                )
                .into_any_element(),
        )
    }

    /// The nav column's top bar (web `NavBar`): the sidebar drawer toggle and
    /// the brand at the left, the rest a native drag area. The window
    /// controls sit on the scene top bar, matching the desktop split.
    fn render_nav_top_bar(&self, text: &ShellText, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("shell-nav-top-bar")
            .debug_selector(|| "shell-nav-top-bar".to_string())
            .h(px(TOP_BAR_HEIGHT))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .on_double_click(|_, window, _| window.zoom_window())
            .child(
                Button::new("sidebar-toggle")
                    .icon(IconName::PanelLeftClose)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(text.sidebar_toggle.clone())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_open = false;
                        cx.notify();
                    })),
            )
            .child(div().text_sm().font_medium().child("OpenBitFun"))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
    }

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
    /// One per session tab; the session id comes from the active tab.
    fn render_chat_column(
        &self,
        session_id: &str,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let chat_area = self.render_chat_area(session_id, text, cx);
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

    /// The session tab's main area (R2c): the chat column, plus the right
    /// resource panel when one is docked — a horizontal split whose divider
    /// drags the panel width. The panel is a workspace-scoped preference, so
    /// it reappears on every session tab; with it hidden the chat column
    /// takes the full width. One `children` call: the group *replaces* its
    /// panel list (upstream 0.7.1), so the toggle between one and two panels
    /// is a full re-set — the same rule the terminal dock follows.
    fn render_session_main(
        &self,
        session_id: &str,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let chat = self.render_chat_column(session_id, text, cx);
        let Some(kind) = self.resource_panel else {
            return chat;
        };
        h_resizable("session-panel-split")
            .with_state(&self.panel_split)
            .children([
                resizable_panel().child(chat),
                resizable_panel()
                    .size(self.panel_width)
                    .size_range(px(MIN_RESOURCE_PANEL_WIDTH)..px(MAX_RESOURCE_PANEL_WIDTH))
                    .child(self.render_resource_panel(kind, text, cx)),
            ])
            .into_any_element()
    }

    /// The docked resource panel: a slim header (title, pop-out, close) over
    /// the page body. The pages render unchanged — the same entities the main
    /// scene tabs mount, moved to the docked mount point.
    fn render_resource_panel(
        &self,
        kind: ResourcePanelKind,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let title = match kind {
            ResourcePanelKind::Files => text.files_title.clone(),
            ResourcePanelKind::Git => text.git_title.clone(),
        };
        // The two pages are different entity types: each arm wraps its own
        // mount into the same panel-body shape.
        let body = match kind {
            ResourcePanelKind::Files => self.files.clone().map(|page| {
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(page)
                    .into_any_element()
            }),
            ResourcePanelKind::Git => self.git.clone().map(|panel| {
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(panel)
                    .into_any_element()
            }),
        };
        let head = div()
            .debug_selector(|| "resource-panel-head".to_string())
            .h_9()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().px_1().text_sm().font_medium().child(title))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        Button::new("resource-panel-pop-out")
                            .icon(IconName::ExternalLink)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.panel_pop_out.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pop_out_resource_panel(cx);
                            })),
                    )
                    .child(
                        Button::new("resource-panel-close")
                            .icon(IconName::X)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.panel_close.clone())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.close_resource_panel(cx);
                            })),
                    ),
            );
        div()
            .debug_selector(|| "resource-panel".to_string())
            .h_full()
            .min_w_0()
            .flex_1()
            .v_flex()
            .overflow_hidden()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(head)
            .child(match body {
                Some(body) => body,
                None => div().into_any_element(),
            })
            .into_any_element()
    }

    /// The tabless landing face (R2b, the web `WelcomeScene`): the brand line
    /// and slogan stand in for a session, and one input starts one —
    /// submitting creates the session and sends the text as its opening turn
    /// (`create_session_with_first_message`, so the tab opens before the
    /// reply stream starts). The composer card's visual language carries over
    /// so the hand-off to the session composer reads as one surface. The web
    /// typewriter cycles seven phrases; the shell pins the brand line
    /// ("把如果变成真。") — the web reduced-motion form — instead of ticking a
    /// timer through the whole shell every frame.
    fn render_welcome_face(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let draft_value = self.draft.read(cx).value().to_string();
        let draft = self.draft.clone();
        let tx = self.event_tx.clone();
        // The welcome card shares the session composer's round send button
        // (web WelcomeScene: one primary circle with an up arrow).
        let send = Button::new("welcome-send")
            .icon(IconName::ArrowUp)
            .with_variant(ButtonVariant::Primary)
            .rounded(ButtonRounded::Size(px(999.)))
            .disabled(draft_value.trim().is_empty())
            .on_click(move |_, window, cx| {
                let body = draft.read(cx).value().to_string();
                if body.trim().is_empty() {
                    return;
                }
                draft.update(cx, |state, cx| state.set_value("", window, cx));
                if let Some(handle) = runtime::runtime_handle() {
                    handle.create_session_with_first_message(body, tx.clone());
                }
            });
        div()
            .debug_selector(|| "welcome-face".to_string())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .debug_selector(|| "welcome-brand".to_string())
                    .v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(text.welcome_title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(28.))
                            .text_color(cx.theme().muted_foreground)
                            .child(text.welcome_slogan.clone()),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "welcome-input-card".to_string())
                    .w_full()
                    .max_w(px(READING_COLUMN_WIDTH))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
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
                    .child(send),
            )
            .into_any_element()
    }

    fn render_chat_header(&self, text: &ShellText, cx: &mut Context<Self>) -> gpui_kit::Div {
        match self.active_chat() {
            Some(chat) => {
                // The tab strip names the session and the top bar carries the
                // session chrome (usage, quick actions, right panel), so the
                // in-content header only exists while a hidden child view is
                // shown — the badge names what the pane shows and "back"
                // returns to the parent (M20). The busy label lives above the
                // composer (the shimmer line), not here.
                let Some(child) = chat.child_context.as_ref() else {
                    return div();
                };
                let review_label = child
                    .agent_type
                    .as_deref()
                    .and_then(review_activity_kind)
                    .map(|_| text.sidebar_review_running.clone());
                let label = match (child.kind, child.agent_type.as_deref()) {
                    (ChildKind::Subagent, Some(agent_type)) => {
                        let kind_label =
                            review_label.unwrap_or_else(|| text.subagent_badge.clone());
                        format!("{kind_label} · {agent_type}")
                    }
                    (ChildKind::Subagent, None) => text.subagent_badge.clone(),
                    (ChildKind::Btw, _) => text.btw_badge.clone(),
                };
                let back_label = text.child_back.clone();
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .gap_2()
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().selection)
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
            }
            None => div(),
        }
    }

    /// Header quick-actions menu (T2-10, the web SessionFilesBadge actions
    /// section): one entry per enabled `app.ai_experience.quick_actions`
    /// item. Clicking sends the action's prompt as a normal message (web
    /// sends through FlowChatManager.sendMessage with the same gating:
    /// nothing while the session is processing). Hidden when no action is
    /// enabled or no session is open.
    fn render_quick_actions_menu(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
            return div().into_any_element();
        };
        let enabled: Vec<(String, String, IconName)> = settings::ShellConfig::try_current(cx)
            .map(|config| config.quick_actions)
            .unwrap_or_default()
            .iter()
            .filter(|action| action.enabled)
            .map(|action| {
                let (label, prompt) = resolve_quick_action_text(action, text);
                let icon = match action.id.as_str() {
                    "commit" => IconName::GitCommitHorizontal,
                    "create_pr" => IconName::GitPullRequest,
                    _ => IconName::Zap,
                };
                (label, prompt, icon)
            })
            .collect();
        if enabled.is_empty() {
            return div().into_any_element();
        }
        let busy = chat.busy || chat.submitting;
        let session_id = chat.session_id.clone();
        let tx = self.event_tx.clone();
        Button::new("chat-quick-actions")
            .icon(IconName::Zap)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.quick_action_title.clone())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                let mut menu = menu;
                for (label, prompt, icon) in &enabled {
                    let prompt = prompt.clone();
                    let session_id = session_id.clone();
                    let tx = tx.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .icon(icon.clone())
                            .disabled(busy)
                            .on_click(move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.send_message(
                                        session_id.clone(),
                                        prompt.clone(),
                                        Vec::new(),
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

    /// The composer boost button (web "+" boost): a round ghost button whose
    /// menu carries the shell-capable subset of the web boost items — a new
    /// session and the global search palette. Image and file context keep
    /// their existing entrances (paste, drag, drop): the shell has no native
    /// file picker to back a menu entry, and a dead entry would be a fake.
    fn render_composer_boost_menu(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let tx = self.event_tx.clone();
        let view = cx.entity().downgrade();
        let new_session_label = text.sessions_new.clone();
        let search_label = text.search_trigger.clone();
        Button::new("composer-boost")
            .icon(IconName::Plus)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .rounded(ButtonRounded::Size(px(999.)))
            .tooltip(text.sessions_new.clone())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                menu.item(
                    PopupMenuItem::new(new_session_label.clone())
                        .icon(IconName::Plus)
                        .on_click({
                            let tx = tx.clone();
                            move |_, _, _| {
                                if let Some(handle) = runtime::runtime_handle() {
                                    handle.create_session(tx.clone());
                                }
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new(search_label.clone())
                        .icon(IconName::Search)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                if let Some(view) = view.upgrade() {
                                    view.update(cx, |shell, cx| {
                                        shell.open_search_overlay(window, cx)
                                    });
                                }
                            }
                        }),
                )
            })
            .into_any_element()
    }

    /// Header usage button: asks the runtime for the session usage report;
    /// the dialog opens when [`RuntimeEvent::SessionUsage`] arrives.
    fn render_usage_button(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
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

    /// Model picker for the open session (R2a: moved from the chat header
    /// into the composer's action row, the web end-actions cluster). The
    /// trigger shows the current model and the menu offers every enabled
    /// configured model. The selection is applied through the runtime
    /// (persisted per session), so the trigger only changes after
    /// [`RuntimeEvent::SessionModelChanged`] confirms it.
    fn render_model_picker(
        &self,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(chat) = self.active_chat() else {
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
/// Besides stdout the run logs into its session directory under the logs
/// root, which is what the diagnostics bundle export later aggregates.
pub fn init_tracing() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    crate::diagnostics::set_effective_level(&filter);
    let (filter_layer, handle) =
        tracing_subscriber::reload::Layer::new(tracing_subscriber::EnvFilter::new(filter));
    let _ = LOG_FILTER.set(handle);
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let session_log_dir = crate::diagnostics::create_session_log_dir();
    let startup_trace_id = format!(
        "{}-{}",
        chrono::Local::now().format("%Y%m%dT%H%M%S"),
        std::process::id()
    );
    crate::diagnostics::initialize_run_state(session_log_dir.clone(), &startup_trace_id);

    let registry = tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout));
    // An absent file just drops the layer; `Option<Layer>` is a layer too.
    let file_layer = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_log_dir.join(crate::diagnostics::SHELL_LOG_FILE))
        .ok()
        .map(|file| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
        });
    let _ = registry.with(file_layer).try_init();
    crate::diagnostics::install_panic_hook();
}

/// Live handle onto the log filter; `L` is the inner filter type and `S` the
/// registry the layer is installed on. `reload` only swaps the filter.
type LogFilterHandle =
    tracing_subscriber::reload::Handle<tracing_subscriber::EnvFilter, tracing_subscriber::Registry>;
static LOG_FILTER: std::sync::OnceLock<LogFilterHandle> = std::sync::OnceLock::new();

/// Applies one `app.logging.level` value to the live filter. Before the
/// subscriber exists this is a no-op.
pub(crate) fn apply_log_level(level: &str) {
    crate::diagnostics::set_effective_level(level);
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

            // Global search palette shortcut (R1b), matching the desktop's
            // Ctrl K, plus the sidebar drawer toggle (web Ctrl B). No key
            // context: the bindings match wherever no focused view consumes
            // the keystroke first.
            cx.bind_keys([
                KeyBinding::new("ctrl-k", OpenGlobalSearch, None),
                KeyBinding::new("ctrl-b", ToggleSidebar, None),
            ]);

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
                    // the compositor blur shows through. Only Wayland has a
                    // per-window blur protocol (KDE's blur manager); there the
                    // window surface opts into it. On X11 `Blurred` switches
                    // the wgpu surface to the transparent pipeline with no
                    // blur behind it and visibly shifts the whole window's
                    // color rendition (measured: background #0e0e10 renders
                    // as rgb(18,19,41), reproduced on Xvfb, absent with
                    // Opaque), so X11 stays opaque and the 90% tint blends
                    // against the theme background — visually a slightly
                    // deeper sidebar. Override with OPENBITFUN_GPUI_FORCE_BLUR
                    // for X11 blur experiments.
                    window_background: if std::env::var("WAYLAND_DISPLAY").is_ok()
                        || std::env::var("WAYLAND_SOCKET").is_ok()
                        || std::env::var("OPENBITFUN_GPUI_FORCE_BLUR").is_ok()
                    {
                        WindowBackgroundAppearance::Blurred
                    } else {
                        WindowBackgroundAppearance::Opaque
                    },
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
    use crate::tabs;
    use openbitfun_core::service::i18n::LocaleId;
    use std::cell::Cell;
    use std::rc::Rc;

    fn chat_with(entries: Vec<ChatEntry>, cx: &mut gpui_kit::TestAppContext) -> ChatView {
        let scroller = cx.new(|cx| MessageScrollerState::new(entries.len(), cx));
        ChatView {
            session_id: "session".into(),
            name: "name".into(),
            agent_type: "Standard".into(),
            model_id: None,
            entries,
            scroller,
            busy: false,
            submitting: false,
            child_context: None,
            expanded_user_texts: BTreeSet::new(),
        }
    }

    #[gpui_kit::test]
    fn append_chunk_reports_tail_growth_vs_new_entry(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| gpui_kit::init(cx));
        let mut chat = chat_with(Vec::new(), cx);
        assert!(chat.append_chunk("round-1", "hello "));
        assert!(!chat.append_chunk("round-1", "world"));
        assert!(chat.append_chunk("round-2", "next round"));
        assert_eq!(chat.entries.len(), 2);
    }

    fn turn_entries() -> Vec<ChatEntry> {
        vec![
            ChatEntry::User {
                text: "fix the bug".into(),
                turn_id: Some("turn-1".into()),
            },
            ChatEntry::Assistant {
                round_id: "round-1".into(),
                text: "first answer".into(),
            },
            ChatEntry::Tool(ToolCard {
                key: "tool-1".into(),
                tool_name: "read_file".into(),
                status: ToolStatus::Done,
                detail: String::new(),
                subagent_session: None,
                background: false,
            }),
            ChatEntry::Assistant {
                round_id: "round-2".into(),
                text: "final answer".into(),
            },
        ]
    }

    #[test]
    fn turn_footer_plan_hosts_the_finished_turn_tail() {
        let text = crate::i18n::fallback_text(LocaleId::EnUS);
        let entries = turn_entries();
        // The tail assistant closes the finished turn; the copy scopes use
        // the web transcript shape (`User\n…` + rounds, `---` separated).
        let plan = turn_footer_plan(&entries, 3, false, false, &text).expect("tail hosts footer");
        assert_eq!(plan.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(plan.copy_result.as_deref(), Some("final answer"));
        assert_eq!(
            plan.copy_full.as_deref(),
            Some(
                format!(
                    "{}\nfix the bug\n\n---\n\nfirst answer\n\n---\n\nfinal answer",
                    text.turn_copy_user_label
                )
                .as_str()
            )
        );
        // A turn closed by the next prompt is finished history.
        let mut history = turn_entries();
        history.push(ChatEntry::User {
            text: "next please".into(),
            turn_id: Some("turn-2".into()),
        });
        assert!(turn_footer_plan(&history, 3, true, false, &text).is_some());
    }

    #[test]
    fn turn_footer_plan_rejects_streaming_and_mid_turn_rows() {
        let text = crate::i18n::fallback_text(LocaleId::EnUS);
        let entries = turn_entries();
        // The executing turn's tail stays footer-less until it finishes.
        assert!(turn_footer_plan(&entries, 3, true, false, &text).is_none());
        assert!(turn_footer_plan(&entries, 3, false, true, &text).is_none());
        // Tool cards inside the turn are not the tail; prompts and
        // system rows never host the footer.
        assert!(turn_footer_plan(&entries, 2, false, false, &text).is_none());
        assert!(turn_footer_plan(&entries, 0, false, false, &text).is_none());
        // A tool-only turn has no assistant text: the tail row still hosts
        // fork/snapshots while both copy scopes hide.
        let tool_only = vec![
            ChatEntry::User {
                text: "run it".into(),
                turn_id: Some("turn-1".into()),
            },
            ChatEntry::Tool(ToolCard {
                key: "tool-1".into(),
                tool_name: "run_command".into(),
                status: ToolStatus::Done,
                detail: String::new(),
                subagent_session: None,
                background: false,
            }),
        ];
        let plan = turn_footer_plan(&tool_only, 1, false, false, &text).expect("tool tail hosts");
        assert_eq!(plan.turn_id.as_deref(), Some("turn-1"));
        assert!(plan.copy_full.is_none() && plan.copy_result.is_none());
    }

    #[gpui_kit::test]
    fn upsert_tool_reports_changed_index_or_append(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| gpui_kit::init(cx));
        let card = |key: &'static str| ToolCard {
            key: SharedString::from(key),
            tool_name: "read_file".into(),
            status: ToolStatus::Running,
            detail: String::new(),
            subagent_session: None,
            background: false,
        };
        let mut chat = chat_with(Vec::new(), cx);
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

    #[test]
    fn quick_action_text_resolves_builtins_and_keeps_customizations() {
        let text = crate::i18n::fallback_text(LocaleId::EnUS);
        let action = |id: &str, label: &str, prompt: &str| {
            openbitfun_core::service::config::types::AiExperienceQuickAction {
                id: id.to_string(),
                label: label.to_string(),
                prompt: prompt.to_string(),
                enabled: true,
            }
        };

        // Built-in text still equal to the shared English defaults resolves
        // to the localized copy (with the English fallback both are equal).
        let (label, prompt) = resolve_quick_action_text(
            &action("commit", "Commit", "Commit all current code changes"),
            &text,
        );
        assert_eq!(label, text.quick_action_commit_label);
        assert_eq!(prompt, text.quick_action_commit_prompt);
        let (label, prompt) = resolve_quick_action_text(
            &action(
                "create_pr",
                "Create PR",
                "Create a Pull Request for the current branch",
            ),
            &text,
        );
        assert_eq!(label, text.quick_action_create_pr_label);
        assert_eq!(prompt, text.quick_action_create_pr_prompt);

        // Customized stored text stays verbatim (label and prompt resolve
        // independently, like the web helper).
        let (label, prompt) = resolve_quick_action_text(
            &action("commit", "Ship it", "Commit all current code changes"),
            &text,
        );
        assert_eq!(label, "Ship it");
        assert_eq!(prompt, text.quick_action_commit_prompt);

        // Unknown ids are custom actions: always verbatim.
        let (label, prompt) =
            resolve_quick_action_text(&action("deploy", "Deploy", "Ship to staging"), &text);
        assert_eq!(label, "Deploy");
        assert_eq!(prompt, "Ship to staging");
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
                background: false,
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
            let chat = shell.active_chat().unwrap();
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
            let chat = shell.read(cx).active_chat().unwrap();
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
            let chat = shell.read(cx).active_chat().unwrap();
            assert_eq!(chat.session_id, "parent");
            assert!(chat.child_context.is_none());
            match &chat.entries[0] {
                ChatEntry::Tool(card) => {
                    assert_eq!(card.subagent_session.as_deref(), Some("child-1"))
                }
                _ => panic!("expected a tool card"),
            }
        });

        // `/btw` rides the same child machinery with its own badge kind; the
        // dialog turn id rides along so the stop button can target it.
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::BtwStarted {
                        parent_session_id: "parent".into(),
                        child_session_id: "btw-1".into(),
                        turn_id: "turn-1".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(opened("btw-1", "BTW", Vec::new()), window, cx);
            });
        });
        window.update(|_, cx| {
            let chat = shell.read(cx).active_chat().unwrap();
            assert_eq!(chat.session_id, "btw-1");
            let child = chat.child_context.as_ref().expect("btw child context");
            assert_eq!(child.kind, ChildKind::Btw);
            assert_eq!(child.parent_session_id, "parent");
            assert_eq!(child.btw_turn_id.as_deref(), Some("turn-1"));
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
                        Some(entry) => render_chat_item(&entry, &text, None, index, false, cx),
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
                            group_key: None,
                            source_id: String::new(),
                            source_label: String::new(),
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

    #[gpui_kit::test]
    fn mcp_json_and_resource_events_land_in_window_state(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                // A failed save keeps the editor open with the inline error.
                shell.apply_runtime_event(
                    RuntimeEvent::McpJsonSaved(Err("JSON parsing failed".into())),
                    window,
                    cx,
                );
                // A resource reply lands on the matching pending read.
                shell.mcp_resource_view = Some(crate::manage::McpResourceView {
                    uri: "file:///a.txt".into(),
                    loading: true,
                    result: None,
                });
                shell.apply_runtime_event(
                    RuntimeEvent::McpResourceRead(Box::new(
                        crate::manage::McpResourceReadOutcome {
                            uri: "file:///a.txt".into(),
                            result: Ok(
                                openbitfun_core::service::mcp::protocol::ResourcesReadResult {
                                    contents: vec![openbitfun_core::service::mcp::protocol::MCPResourceContent {
                                        uri: "file:///a.txt".into(),
                                        content: Some("hello".into()),
                                        blob: None,
                                        mime_type: Some("text/plain".into()),
                                        annotations: None,
                                        meta: None,
                                    }],
                                },
                            ),
                        },
                    )),
                    window,
                    cx,
                );
            });
        });
        window.update(|_, cx| {
            shell.update(cx, |shell, _cx| {
                assert!(shell.mcp_json_error.is_some());
                let view = shell.mcp_resource_view.as_ref().expect("resource view");
                assert!(!view.loading);
                let result = view.result.as_ref().expect("resource result");
                assert_eq!(
                    crate::manage::render_resource_content(
                        result.as_ref().expect("read ok"),
                        &sample_shell_text()
                    ),
                    "[text/plain]\n\nhello"
                );
            });
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

    /// Renders only the archived list body for the bulk-delete test (T1-4);
    /// the field needs a view wrapper for the same layout reason as
    /// [`SettingsHarness`].
    struct ArchivedFieldHarness {
        text: ShellText,
        view: gpui_kit::WeakEntity<ShellWindow>,
    }

    impl Render for ArchivedFieldHarness {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            settings::archived_sessions_field(self.view.clone(), &self.text, window, cx)
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

    /// The status bar user slot (R1d): the trinity daemon's name for the user
    /// wins while connected, the OS login backs the slot otherwise.
    #[test]
    fn status_bar_user_prefers_the_trinity_name() {
        let os_user = "zero";
        assert_eq!(status_bar_user(None, os_user), "zero");
        let no_name = crate::trinity::StatusInfo::default();
        assert_eq!(status_bar_user(Some(&no_name), os_user), "zero");
        let named = crate::trinity::StatusInfo {
            user_name: Some("公子".to_string()),
            ..Default::default()
        };
        assert_eq!(status_bar_user(Some(&named), os_user), "公子");
        // An empty daemon name is no name.
        let empty = crate::trinity::StatusInfo {
            user_name: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(status_bar_user(Some(&empty), os_user), "zero");
    }

    /// The global status bar (R1d) lays out as a full-width strip pinned to
    /// the window bottom, under the body; the trinity chip lives inside it.
    #[gpui_kit::test]
    fn status_bar_lays_out_at_the_window_bottom(cx: &mut gpui_kit::TestAppContext) {
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
        // The desktop removed the full-width window-bottom status bar
        // (`92ce04f8d`): the user/trinity cluster and the tool buttons live in
        // the nav column's footer now — column-wide, hugging the window
        // bottom under the sidebar — while the scene face's top bar carries
        // the tab strip and fills the rest of the window width.
        let nav = window
            .debug_bounds("shell-nav-column")
            .expect("nav column lays out");
        let footer = window
            .debug_bounds("shell-sidebar-footer")
            .expect("sidebar footer lays out");
        assert_eq!(
            footer.size.width, nav.size.width,
            "the footer spans the nav column, not the window"
        );
        assert!(
            (footer.bottom() - nav.bottom()).abs() < px(1.),
            "the footer hugs the window bottom inside the nav column: footer {:?} nav {:?}",
            footer,
            nav
        );
        let sidebar = window
            .debug_bounds("shell-body-sidebar")
            .expect("sidebar lays out");
        assert!(
            sidebar.bottom() <= footer.origin.y + px(1.),
            "the footer sits below the sidebar, not beside it"
        );
        assert!(
            window.debug_bounds("status-trinity-chip").is_some(),
            "the trinity lamp chip renders inside the footer"
        );
        let top_bar = window
            .debug_bounds("shell-scene-top-bar")
            .expect("scene top bar lays out");
        assert!(
            (top_bar.size.width + nav.size.width - px(1200.)).abs() < px(1.),
            "the scene top bar fills the width the nav column leaves: bar {:?} nav {:?}",
            top_bar,
            nav
        );
        assert!(
            (top_bar.size.height - px(TOP_BAR_HEIGHT)).abs() < px(1.),
            "the scene top bar keeps the desktop 45px chrome height"
        );
    }

    /// The permission badge (R2a) projects the settings preset: ask keeps the
    /// plain shield on success ink, full access raises the alert shield on
    /// danger ink — the web icon ramp, one step per given-up confirmation.
    #[test]
    fn permission_badge_maps_the_settings_preset() {
        let text = crate::i18n::fallback_text(LocaleId::EnUS);
        use openbitfun_runtime_ports::PermissionPolicyPreset;
        let (icon, label, full_access) =
            ShellWindow::permission_badge(PermissionPolicyPreset::Ask, &text);
        assert_eq!(icon, IconName::Shield);
        assert_eq!(label, text.permission_ask);
        assert!(!full_access);
        let (icon, label, full_access) =
            ShellWindow::permission_badge(PermissionPolicyPreset::FullAccess, &text);
        assert_eq!(icon, IconName::ShieldAlert);
        assert_eq!(label, text.permission_full_access);
        assert!(full_access);
    }

    /// The composer (R2a) carries the context strip above the card and the
    /// model picker inside it: the strip reads as the capsule's footnote, and
    /// the picker's old header home renders no such selector anymore.
    #[gpui_kit::test]
    fn composer_strip_and_model_picker_lay_out_in_the_composer(cx: &mut gpui_kit::TestAppContext) {
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
                        model_id: Some("model-a".into()),
                        entries: Vec::new(),
                        busy: false,
                    }),
                    window,
                    cx,
                );
            });
        });
        window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
            let shell = shell.clone();
            move |_, _| shell.into_any_element()
        });
        window.run_until_parked();
        let strip = window
            .debug_bounds("composer-context-strip")
            .expect("context strip lays out");
        let card = window
            .debug_bounds("composer-card")
            .expect("composer card lays out");
        let badge = window
            .debug_bounds("composer-permission-badge")
            .expect("permission badge lays out");
        let picker = window
            .debug_bounds("composer-model-picker")
            .expect("model picker lays out inside the composer");
        assert!(
            strip.origin.y + strip.size.height <= card.origin.y,
            "the strip sits above the card: strip {:?} card {:?}",
            strip,
            card
        );
        assert!(
            badge.origin.y >= strip.origin.y
                && badge.origin.y + badge.size.height <= strip.origin.y + strip.size.height,
            "the permission badge lives in the strip's right rail: badge {:?} strip {:?}",
            badge,
            strip
        );
        assert!(
            picker.origin.y > strip.origin.y
                && picker.origin.y >= card.origin.y
                && picker.origin.y + picker.size.height <= card.origin.y + card.size.height,
            "the model picker moved into the card below the strip: picker {:?} strip {:?} card {:?}",
            picker,
            strip,
            card
        );
    }

    /// The tabless landing face (R2b): with no tab open the main area shows
    /// the brand slogan over the start-here input card, the web WelcomeScene
    /// order — not the empty panel (which stays for the unloaded-tab state).
    #[gpui_kit::test]
    fn welcome_face_lays_out_when_no_tab_is_active(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
        window.draw(point(px(0.), px(0.)), size(px(1200.), px(800.)), {
            let shell = shell.clone();
            move |_, _| shell.into_any_element()
        });
        window.run_until_parked();
        let face = window
            .debug_bounds("welcome-face")
            .expect("the welcome face lays out with no tabs open");
        let brand = window
            .debug_bounds("welcome-brand")
            .expect("the brand slogan lays out");
        let card = window
            .debug_bounds("welcome-input-card")
            .expect("the start-here input card lays out");
        assert!(
            brand.origin.y >= face.origin.y
                && brand.origin.y + brand.size.height <= face.origin.y + face.size.height,
            "the brand lives inside the face: brand {:?} face {:?}",
            brand,
            face
        );
        assert!(
            card.origin.y >= face.origin.y
                && card.origin.y + card.size.height <= face.origin.y + face.size.height,
            "the input card lives inside the face: card {:?} face {:?}",
            card,
            face
        );
        assert!(
            brand.origin.y + brand.size.height <= card.origin.y,
            "the brand stands above the input card: brand {:?} card {:?}",
            brand,
            card
        );
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
                shell.active_chat().map(|chat| chat.name.clone()),
                shell.active_chat().and_then(|chat| chat.model_id.clone()),
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
                shell.active_chat().map(|chat| chat.name.clone()),
                shell.active_chat().and_then(|chat| chat.model_id.clone()),
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
            (shell.sessions.len(), !shell.chats.is_empty())
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
            let chat = shell.active_chat().expect("chat opened");
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
            assert!(shell.chats.is_empty(), "page events never touch the chat");
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

    /// The sidebar tree shape: `(session id, depth, running)` per session row
    /// in render order, the fold row as `Err((remaining, expanded))`.
    fn session_tree_shape(
        window: &mut gpui_kit::VisualTestContext,
        shell: &Entity<ShellWindow>,
        text: &ShellText,
    ) -> Vec<Result<(String, u8, bool), (usize, bool)>> {
        let text = text.clone();
        window.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .render_session_entries(&text, cx)
                    .into_iter()
                    .filter_map(|section| match section {
                        SidebarSection::Item(SidebarEntry::Session {
                            row,
                            depth,
                            running,
                            ..
                        }) => Some(Ok((row.id, depth, running))),
                        SidebarSection::Item(SidebarEntry::More {
                            remaining,
                            expanded,
                            ..
                        }) => Some(Err((remaining, expanded))),
                        _ => None,
                    })
                    .collect()
            })
        })
    }

    fn lineage_entry(
        session_id: &str,
        session_name: &str,
        parent_session_id: Option<&str>,
        subagent_type: Option<&str>,
    ) -> openbitfun_runtime_ports::AgentSessionLineageEntry {
        openbitfun_runtime_ports::AgentSessionLineageEntry {
            session_id: session_id.into(),
            session_name: session_name.into(),
            agent_type: "Standard".into(),
            created_at_ms: 0,
            status: openbitfun_runtime_ports::AgentSessionLifecycleStatus::Active,
            active_turn_id: None,
            parent_session_id: parent_session_id.map(str::to_string),
            parent_tool_call_id: None,
            subagent_type: subagent_type.map(str::to_string),
            agent_id: None,
            workspace_id: None,
            workspace_path: None,
            remote_connection_id: None,
            remote_ssh_host: None,
            unread_completion: None,
            needs_user_attention: None,
        }
    }

    /// R1c: lineage snapshots project as subagent children indented under
    /// their root, one level deep — deeper descendants stay hidden like the
    /// web nav's two-level projection.
    #[gpui_kit::test]
    fn session_tree_projects_children_under_roots(cx: &mut gpui_kit::TestAppContext) {
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
                            summary: sample_summary("s-1", "Root one"),
                            archived: false,
                        },
                        crate::runtime::SessionListRow {
                            summary: sample_summary("s-2", "Root two"),
                            archived: false,
                        },
                    ]),
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::SessionLineages(vec![
                        openbitfun_runtime_ports::AgentSessionLineageSnapshot {
                            root_session_id: "s-1".into(),
                            sessions: vec![
                                lineage_entry("s-1", "Root one", None, None),
                                lineage_entry("c-1", "Child", Some("s-1"), Some("Explore")),
                                lineage_entry("g-1", "Grandchild", Some("c-1"), None),
                            ],
                        },
                        openbitfun_runtime_ports::AgentSessionLineageSnapshot {
                            root_session_id: "s-2".into(),
                            sessions: vec![lineage_entry("s-2", "Root two", None, None)],
                        },
                    ]),
                    window,
                    cx,
                );
            });
        });

        let shape = session_tree_shape(&mut window, &shell, &text);
        assert_eq!(
            shape,
            vec![
                Ok(("s-1".into(), 0, false)),
                Ok(("c-1".into(), 1, false)),
                Ok(("s-2".into(), 0, false)),
            ]
        );
    }

    /// R1c: long root lists fold to the collapsed page with a show-more row,
    /// expand back to every root, and the workspace group folds everything
    /// but its own row.
    #[gpui_kit::test]
    fn session_tree_folds_long_lists(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
        let text = sample_shell_text();

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let rows = (0..8)
                    .map(|ix| crate::runtime::SessionListRow {
                        summary: sample_summary(&format!("s-{ix}"), &format!("Session {ix}")),
                        archived: false,
                    })
                    .collect();
                shell.apply_runtime_event(RuntimeEvent::Sessions(rows), window, cx);
            });
        });

        let shape = session_tree_shape(&mut window, &shell, &text);
        assert_eq!(shape.iter().filter(|row| row.is_ok()).count(), 5);
        assert_eq!(shape.last(), Some(&Err((3, false))));

        window.update(|_, cx| {
            shell.update(cx, |shell, _cx| {
                shell.sessions_expanded = true;
            });
        });
        let shape = session_tree_shape(&mut window, &shell, &text);
        assert_eq!(shape.iter().filter(|row| row.is_ok()).count(), 8);
        assert_eq!(shape.last(), Some(&Err((0, true))));

        window.update(|_, cx| {
            shell.update(cx, |shell, _cx| {
                shell.sessions_group_collapsed = true;
            });
        });
        let shape = session_tree_shape(&mut window, &shell, &text);
        assert!(shape.is_empty());
    }

    /// R1c: the running dot follows the process-wide live set, so background
    /// sessions light up without being open.
    #[gpui_kit::test]
    fn session_tree_lights_running_rows(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let mut window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));
        let text = sample_shell_text();

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::Sessions(vec![crate::runtime::SessionListRow {
                        summary: sample_summary("s-1", "Background"),
                        archived: false,
                    }]),
                    window,
                    cx,
                );
            });
        });
        let shape = session_tree_shape(&mut window, &shell, &text);
        assert_eq!(shape, vec![Ok(("s-1".into(), 0, false))]);

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::TurnStarted {
                        session_id: "s-1".into(),
                        turn_id: "t-1".into(),
                    },
                    window,
                    cx,
                );
            });
        });
        let shape = session_tree_shape(&mut window, &shell, &text);
        assert_eq!(shape, vec![Ok(("s-1".into(), 0, true))]);

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::TurnFinished {
                        session_id: "s-1".into(),
                        error: None,
                    },
                    window,
                    cx,
                );
            });
        });
        let shape = session_tree_shape(&mut window, &shell, &text);
        assert_eq!(shape, vec![Ok(("s-1".into(), 0, false))]);
    }

    /// The archived settings page renders its bulk delete affordance next to
    /// the rows (T1-4); the destructive step stays behind its confirm dialog,
    /// so the render alone must not delete anything.
    #[gpui_kit::test]
    fn archived_settings_page_renders_the_bulk_delete(cx: &mut gpui_kit::TestAppContext) {
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

        let view = shell.downgrade();
        window.draw(
            point(px(0.), px(0.)),
            size(px(900.), px(600.)),
            move |_, cx| {
                // The component library asks the window for the current view
                // id during layout, so the field renders inside a view.
                cx.new(|_| ArchivedFieldHarness {
                    text: text.clone(),
                    view: view.clone(),
                })
                .into_any_element()
            },
        );
        window.run_until_parked();
    }

    /// Background subagent activity (T2-6): a linked child whose task tool
    /// started with `run_in_background` badges the parent row while its turn
    /// runs, and the badge clears when the child settles.
    #[gpui_kit::test]
    fn background_subagent_badges_track_the_child_lifecycle(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, _rx, window, cx)));
        let task_card = |background: bool| RuntimeEvent::ToolUpdate {
            session_id: "parent".into(),
            card: crate::runtime::ToolCard {
                key: "tool-call-1".into(),
                tool_name: "task".into(),
                status: crate::runtime::ToolStatus::Running,
                detail: String::new(),
                subagent_session: None,
                background,
            },
        };
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(task_card(true), window, cx);
                shell.apply_runtime_event(
                    RuntimeEvent::SubagentLinked {
                        parent_session_id: "parent".into(),
                        parent_tool_call_id: "call-1".into(),
                        subagent_session_id: "child-1".into(),
                        agent_type: None,
                    },
                    window,
                    cx,
                );
                // The child has not started: nothing is running yet.
                assert_eq!(shell.session_activity_badges("parent"), (0, None));
                shell.apply_runtime_event(
                    RuntimeEvent::TurnStarted {
                        session_id: "child-1".into(),
                        turn_id: "t1".into(),
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.session_activity_badges("parent"), (1, None));
                // The child outlives the parent's turn: still activity.
                shell.apply_runtime_event(
                    RuntimeEvent::TurnFinished {
                        session_id: "parent".into(),
                        error: None,
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.session_activity_badges("parent"), (1, None));
                shell.apply_runtime_event(
                    RuntimeEvent::TurnFinished {
                        session_id: "child-1".into(),
                        error: None,
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.session_activity_badges("parent"), (0, None));
                // A non-background task tool never badges while the parent
                // turn is live (the web store's run_in_background signal).
                shell.apply_runtime_event(task_card(false), window, cx);
                shell.apply_runtime_event(
                    RuntimeEvent::SubagentLinked {
                        parent_session_id: "parent".into(),
                        parent_tool_call_id: "call-2".into(),
                        subagent_session_id: "child-2".into(),
                        agent_type: None,
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::TurnStarted {
                        session_id: "parent".into(),
                        turn_id: "t2".into(),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::TurnStarted {
                        session_id: "child-2".into(),
                        turn_id: "t3".into(),
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.session_activity_badges("parent"), (0, None));
            });
        });
    }

    /// A review-family agent type surfaces the review badge on the parent
    /// row once its run goes background (parent turn settled).
    #[gpui_kit::test]
    fn review_subagents_classify_into_the_review_badge(cx: &mut gpui_kit::TestAppContext) {
        assert_eq!(
            review_activity_kind("ReviewWorker"),
            Some(ReviewActivityKind::Review)
        );
        assert_eq!(
            review_activity_kind("ReviewSecurity"),
            Some(ReviewActivityKind::Review)
        );
        assert_eq!(
            review_activity_kind("CodeReview"),
            Some(ReviewActivityKind::Review)
        );
        assert_eq!(
            review_activity_kind("DeepReview"),
            Some(ReviewActivityKind::DeepReview)
        );
        assert_eq!(review_activity_kind("Standard"), None);
        assert_eq!(review_activity_kind("CodebaseResearcher"), None);

        install_shell_globals(cx);
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, _rx, window, cx)));
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::SubagentLinked {
                        parent_session_id: "parent".into(),
                        parent_tool_call_id: "call-1".into(),
                        subagent_session_id: "reviewer".into(),
                        agent_type: Some("ReviewWorker".into()),
                    },
                    window,
                    cx,
                );
                shell.apply_runtime_event(
                    RuntimeEvent::TurnStarted {
                        session_id: "reviewer".into(),
                        turn_id: "t1".into(),
                    },
                    window,
                    cx,
                );
                // Foreground semantics: the parent turn is still live and the
                // task tool never ran in background, so no badge — matching
                // the web store.
                shell.apply_runtime_event(
                    RuntimeEvent::TurnStarted {
                        session_id: "parent".into(),
                        turn_id: "t0".into(),
                    },
                    window,
                    cx,
                );
                assert_eq!(shell.session_activity_badges("parent"), (0, None));
                // Once the parent turn settles, the still-running review run
                // has outlived it and shows the review badge.
                shell.apply_runtime_event(
                    RuntimeEvent::TurnFinished {
                        session_id: "parent".into(),
                        error: None,
                    },
                    window,
                    cx,
                );
                assert_eq!(
                    shell.session_activity_badges("parent"),
                    (1, Some(ReviewActivityKind::Review))
                );
            });
        });
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
                    .filter(|section| {
                        matches!(
                            section,
                            SidebarSection::Item(SidebarEntry::Session { depth: 0, .. })
                        )
                    })
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
                shell.toggle_files_view(window, cx);
            });
            inject_terminal_tab(&shell, "old-ws", window, cx);
        });
        window.update(|_, cx| {
            let shell = shell.read(cx);
            assert!(!shell.chats.is_empty());
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
                shell.chats.is_empty() && shell.tabs.active_kind().is_none(),
                "transcript tabs belong to the old workspace"
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
            assert_eq!(shell.tabs.active_kind(), None);
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

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.toggle_files_view(window, cx);
                assert_eq!(
                    shell.tabs.active_kind(),
                    Some(&TabKind::Scene(tabs::SceneId::Files))
                );
                assert!(shell.files.is_some());
            });
        });
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let page = shell.files.as_ref().unwrap().read(cx);
                assert!(page.error().is_some(), "no runtime here: degrade loudly");
            });
            shell.update(cx, |shell, cx| {
                // Tab semantics (R1a): the nav row activates idempotently;
                // the tab close drops the scene tab, the page entity stays.
                shell.toggle_files_view(window, cx);
                assert_eq!(
                    shell.tabs.active_kind(),
                    Some(&TabKind::Scene(tabs::SceneId::Files))
                );
                assert!(shell.files.is_some(), "tabs survive the scene close");

                shell.close_top_tab(
                    shell
                        .tabs
                        .index_of(&TabKind::Scene(tabs::SceneId::Files))
                        .unwrap(),
                    cx,
                );
                assert_eq!(shell.tabs.active_kind(), None);

                shell.toggle_files_view(window, cx);
                assert_eq!(
                    shell.tabs.active_kind(),
                    Some(&TabKind::Scene(tabs::SceneId::Files))
                );
            });
        });
    }

    /// R2c opening policy state machine: with a session tab active the
    /// resource docks into the session's right panel instead of opening a
    /// scene tab, panel and main tab stay mutually exclusive, and pop-out
    /// reverses the move without dropping the page entity.
    #[gpui_kit::test]
    fn resources_dock_into_the_session_panel_and_pop_out(cx: &mut gpui_kit::TestAppContext) {
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
                // Session tab active: Files docks, no scene tab appears.
                shell.toggle_files_view(window, cx);
                assert_eq!(shell.resource_panel, Some(ResourcePanelKind::Files));
                assert!(shell
                    .tabs
                    .index_of(&TabKind::Scene(tabs::SceneId::Files))
                    .is_none());
                // The other kind swaps the docked panel in place.
                shell.toggle_git_view(window, cx);
                assert_eq!(shell.resource_panel, Some(ResourcePanelKind::Git));
                // Toggling the docked kind closed hides the panel.
                shell.toggle_git_view(window, cx);
                assert_eq!(shell.resource_panel, None);
                // Dock again and pop out: the main tab opens and activates.
                shell.toggle_git_view(window, cx);
                shell.pop_out_resource_panel(cx);
                assert_eq!(shell.resource_panel, None);
                assert_eq!(
                    shell.tabs.active_kind(),
                    Some(&TabKind::Scene(tabs::SceneId::Git))
                );
                // The two forms are exclusive: docking evicts the main tab
                // while keeping the page entity alive across the move.
                shell.dock_resource_panel(ResourcePanelKind::Git, cx);
                assert!(shell
                    .tabs
                    .index_of(&TabKind::Scene(tabs::SceneId::Git))
                    .is_none());
                assert!(shell.git.is_some(), "the mount move keeps the entity");
            });
        });
    }

    /// The docked panel (R2c) lays out to the right of the sidebar over the
    /// chat column with its head chrome, and pop-out removes it from the
    /// session view.
    #[gpui_kit::test]
    fn user_bubble_wraps_and_caps_inside_the_reading_column(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        // A long CJK prompt: line-breaking must wrap it inside the bubble
        // (the R2d regression: it used to render as one clipped line).
        let long_prompt = "把如果变成真：".repeat(12);
        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.apply_runtime_event(
                    RuntimeEvent::SessionOpened(crate::runtime::OpenedSession {
                        session_id: "s-1".into(),
                        name: "First".into(),
                        agent_type: "Standard".into(),
                        model_id: None,
                        entries: vec![
                            ChatEntry::User {
                                text: long_prompt,
                                turn_id: Some("turn-1".into()),
                            },
                            ChatEntry::Assistant {
                                round_id: "round-1".into(),
                                text: "done".into(),
                            },
                        ],
                        busy: false,
                    }),
                    window,
                    cx,
                );
            });
        });
        let view = shell.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(1400.), px(900.)),
            move |_, _| view.into_any_element(),
        );
        window.run_until_parked();
        let cap = window
            .debug_bounds("user-bubble-cap")
            .expect("user bubble cap lays out");
        // 72% of the 900px reading column; the column is centered in the
        // chat area right of the 300px sidebar.
        assert!(
            cap.size.width <= px(900. * 0.72 + 1.),
            "bubble caps at the web fraction: {:?}",
            cap
        );
        // Wrapped CJK at text_sm/1.625 line height is ~21px per line; the
        // 168-char prompt cannot fit three 648px lines (clamp) — anything
        // below four lines means it rendered as one clipped line again.
        assert!(
            cap.size.height > px(60.),
            "bubble wraps to multiple lines: {:?}",
            cap
        );
    }

    #[gpui_kit::test]
    fn turn_footer_stays_inside_the_reading_column(cx: &mut gpui_kit::TestAppContext) {
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
                        entries: vec![
                            ChatEntry::User {
                                text: "rewrite the docs".into(),
                                turn_id: Some("turn-1".into()),
                            },
                            ChatEntry::Assistant {
                                round_id: "round-1".into(),
                                text: "docs rewritten".into(),
                            },
                        ],
                        busy: false,
                    }),
                    window,
                    cx,
                );
            });
        });
        let view = shell.clone();
        // A 1150px window: the reading column squeezes below its 900px max
        // (the Xvfb smoke geometry that first showed the overflow).
        window.draw(
            point(px(0.), px(0.)),
            size(px(1150.), px(838.)),
            move |_, _| view.into_any_element(),
        );
        window.run_until_parked();
        let footer = window
            .debug_bounds("turn-footer")
            .expect("finished turn hosts the footer");
        let row = window
            .debug_bounds("chat-row")
            .expect("reading-column row lays out");
        assert!(
            footer.origin.x >= row.origin.x
                && footer.origin.x + footer.size.width <= row.origin.x + row.size.width,
            "footer sits inside the reading column: footer {:?} row {:?}",
            footer,
            row
        );
    }

    #[gpui_kit::test]
    fn resource_panel_lays_out_right_of_the_chat_column(cx: &mut gpui_kit::TestAppContext) {
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
                shell.toggle_files_view(window, cx);
            });
        });
        let view = shell.clone();
        window.draw(
            point(px(0.), px(0.)),
            size(px(1400.), px(900.)),
            move |_, _| view.into_any_element(),
        );
        window.run_until_parked();
        let sidebar = window
            .debug_bounds("shell-body-sidebar")
            .expect("session view lays out");
        let panel = window
            .debug_bounds("resource-panel")
            .expect("docked panel lays out with the session");
        assert!(
            panel.origin.x > sidebar.origin.x + sidebar.size.width,
            "panel sits right of the sidebar: panel {:?} sidebar {:?}",
            panel,
            sidebar
        );
        let head = window
            .debug_bounds("resource-panel-head")
            .expect("panel head lays out");
        assert!(head.size.height > px(20.), "head height: {:?}", head.size);
        assert!(
            panel.size.width >= px(MIN_RESOURCE_PANEL_WIDTH),
            "panel width: {:?}",
            panel.size
        );

        window.update(|_, cx| {
            shell.update(cx, |shell, cx| shell.pop_out_resource_panel(cx));
        });
        window.draw(
            point(px(0.), px(0.)),
            size(px(1400.), px(900.)),
            move |_, _| shell.into_any_element(),
        );
        window.run_until_parked();
        assert!(window.debug_bounds("resource-panel").is_none());
    }

    /// A process-lifetime tokio runtime for pane construction in headless
    /// tests: `TerminalPane::new` spawns its PTY pump on the given handle, so
    /// the runtime must outlive the test (`pty_integration` drives the same
    /// service directly).
    fn test_terminal_tokio() -> tokio::runtime::Handle {
        static RT: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();
        RT.get_or_init(|| {
            // A current-thread runtime on purpose: panes only fire-and-forget
            // pumps onto this handle, and a never-driven runtime queues them
            // without a worker thread. The old multi-thread runtime ran real
            // PTY tasks concurrently with parallel tests, tripping the gpui
            // test scheduler's determinism check (~40% of full-suite runs).
            // Nothing here awaits on the handle, so queued tasks staying
            // parked is exactly the behavior the tests need.
            let runtime = tokio::runtime::Builder::new_current_thread()
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

    /// Search overlay state machine (R1b): open resets the query state, the
    /// pump projections land for both paths, and close drops them.
    #[gpui_kit::test]
    fn search_overlay_opens_projects_and_closes(cx: &mut gpui_kit::TestAppContext) {
        install_shell_globals(cx);
        let (tx, rx) = mpsc::unbounded_channel();
        let window = cx.add_empty_window();
        let shell = window.update(|window, cx| cx.new(|cx| ShellWindow::new(tx, rx, window, cx)));

        window.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                assert!(!shell.search_overlay_open);

                shell.open_search_overlay(window, cx);
                assert!(shell.search_overlay_open);

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

                // Closing drops the query state so a stale result set never
                // lingers behind the next open.
                shell.close_search_overlay(window, cx);
                assert!(!shell.search_overlay_open);
                assert!(shell.search.content.is_empty());
                assert!(shell.search.sessions.is_empty());
                assert_eq!(shell.search.query, "");
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

    /// R1b sidebar nav over the R1a tab model: the row set is the desktop
    /// top-scene trio in NavPanel order, and exactly one row lights up per
    /// active tab (Settings activates none — its own footer button keeps the
    /// raised variant). Workbench scenes (files/git/insights) light no row;
    /// they live in the search palette until R2c. A session tab lights the
    /// Chat row; no tabs light it too (the resting empty state).
    #[test]
    fn nav_rows_map_active_state_to_tabs() {
        let actions = nav_actions();
        assert_eq!(
            actions,
            vec![NavAction::Chat, NavAction::Trinity, NavAction::Tasks]
        );
        let mut tabs = TabsState::default();
        for kind in [
            TabKind::Session("s-1".to_string()),
            TabKind::Scene(tabs::SceneId::Files),
            TabKind::Scene(tabs::SceneId::Git),
            TabKind::Scene(tabs::SceneId::Insights),
            TabKind::Scene(tabs::SceneId::Tasks),
            TabKind::Scene(tabs::SceneId::Trinity),
        ] {
            let kind_label = format!("{kind:?}");
            tabs.open(kind.clone());
            let lit: Vec<&NavAction> = actions
                .iter()
                .filter(|candidate| candidate.is_active(&tabs))
                .collect();
            match kind {
                // Workbench scenes have no nav row until R2c re-homes them.
                TabKind::Scene(tabs::SceneId::Files)
                | TabKind::Scene(tabs::SceneId::Git)
                | TabKind::Scene(tabs::SceneId::Insights) => {
                    assert!(lit.is_empty(), "tab {kind_label} must light no nav row");
                }
                _ => {
                    assert_eq!(
                        lit.len(),
                        1,
                        "tab {kind_label} must light exactly one nav row"
                    );
                }
            }
        }
        // Settings keeps its footer-button variant, no nav row.
        tabs.open(TabKind::Scene(tabs::SceneId::Settings));
        let active = actions
            .iter()
            .filter(|candidate| candidate.is_active(&tabs))
            .count();
        assert_eq!(active, 0, "the settings tab lights no nav row");
        // The resting empty state is chat.
        let empty = TabsState::default();
        assert!(NavAction::Chat.is_active(&empty));
    }
}
