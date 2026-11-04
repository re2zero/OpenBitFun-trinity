// Import bindings in this module resolve inside the `shell/*` child
// modules through `use super::*` glob chains. Rust does not credit an
// upstream binding as used when resolution happens only through a
// descendant glob, so per-binding import placement would need compiler
// feedback this workspace cannot iterate on quickly (multi-hour rounds).
// The subtree therefore opts out of unused-import noise here.
#![allow(unused_imports)]

use crate::{i18n, markdown, runtime, settings, theme};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable, ResizableState};
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::{
    bubble::{Bubble, BubbleContent, BubbleVariant},
    button::{Button, ButtonCustomVariant, ButtonRounded, ButtonVariant, ButtonVariants},
    empty::{Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
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
    Context, ElementId, Entity, ExternalPaths, FontWeight, Image, ImageFormat, Pixels,
    SharedString, StyleRefinement, SystemNotification, TitlebarOptions, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowOptions,
};
use openbitfun_agent_runtime::sdk::{PendingUserQuestion, PermissionReply, PermissionRequest};
use openbitfun_runtime_ports::{AgentInputAttachment, AgentSessionLineageSnapshot};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::assets::ShellAssets;
use crate::files::FilesPage;
use crate::git::GitPanel;
use crate::i18n::ShellText;
use crate::runtime::{ChatEntry, RuntimeEvent, ToolCard, ToolStatus, WorkspaceChoice};
use crate::subagent_identity;
use crate::terminal::pane::TerminalPane;
use crate::theme::{ShellState, DEFAULT_PRESET_ID};
use crate::transcript::{self, ToolKind};

/// Which surface the main content area shows. The shell's main area is a tab
/// bar of session tabs and scene tabs (R1a, mirroring the web-ui desktop
/// `SceneBar`); the tab model lives in [`crate::tabs`].
use crate::tabs::{ResourcePanelKind, SceneId, TabKind, TabsState};

gpui_kit::actions!(openbitfun_gpui, [OpenGlobalSearch, ToggleSidebar]);

mod actions;
mod chat_view;
mod chrome;
mod composer;
mod dialogs;
mod nav;
mod questions;
mod sessions;
mod terminal_dock;
mod tools_view;

#[cfg(test)]
mod tests;

use actions::*;
use chat_view::*;
use chrome::*;
use composer::*;
use dialogs::*;
use nav::*;
use questions::*;
use sessions::*;
use terminal_dock::*;
use tools_view::*;

pub(crate) use chat_view::ChildContext;
pub(crate) use chat_view::ChildKind;
pub(crate) use chat_view::ReviewActivityKind;
pub(crate) use chat_view::SubagentLink;
pub(crate) use composer::resolve_quick_action_text;
pub(crate) use composer::QUICK_ACTION_COMMIT_DEFAULTS;
pub(crate) use composer::QUICK_ACTION_CREATE_PR_DEFAULTS;
pub(crate) use nav::brand_empty;
pub(crate) use nav::brand_empty_card;
pub(crate) use sessions::ArchivedGroupView;
pub(crate) use sessions::ArchivedRowView;
pub(crate) use sessions::SessionRow;
pub(crate) use terminal_dock::PanelSide;
pub(crate) use terminal_dock::TerminalTier;

/// Width of the navigation sidebar, matching the web-ui desktop shell.
pub(crate) const SESSION_PANE_WIDTH: f32 = 300.;
/// The settings content column (web `$config-page-content-max-width`): the
/// page centers on this measure inside whatever the scene column gives it.
const SETTINGS_CONTENT_WIDTH: f32 = 760.;
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
/// Bounds of the docked terminal panel, dragged by the divider above it (or
/// beside it, in the right dock).
const MIN_TERMINAL_HEIGHT: f32 = 120.;
const MAX_TERMINAL_HEIGHT: f32 = 900.;
/// G-T1: the double-click size tiers, the web's
/// `BOTTOM_TERMINAL_PANEL_CONFIG` stops (compact 180, comfortable 300,
/// expanded 420) with its mode thresholds (below 220 compact, 420 and up
/// expanded).
const TERMINAL_TIER_COMPACT: f32 = 180.;
const TERMINAL_TIER_COMFORTABLE: f32 = 300.;
const TERMINAL_TIER_EXPANDED: f32 = 420.;
/// Width the right dock reopens at before any drag (the web's comfortable
/// default).
const DEFAULT_TERMINAL_WIDTH: f32 = 300.;
const TERMINAL_COMPACT_THRESHOLD: f32 = 220.;
const TERMINAL_EXPANDED_THRESHOLD: f32 = 420.;

/// Lifecycle of the embedded runtime, mirrored into the status line.
#[derive(Clone)]
enum RuntimePhase {
    Starting,
    Ready,
    Failed(String),
}

pub(crate) struct ShellWindow {
    draft: Entity<TextareaState>,
    /// Composer text staged by another surface for the next paint (G-G1: the
    /// git panel's init card). `set_value` needs the window, which only the
    /// render path holds, so the request parks here until render consumes it.
    pending_composer_draft: Option<String>,
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
    /// The same split state for the right dock (G-T1): one entity per axis,
    /// because each remembers its own panel sizes.
    dock_h: Entity<ResizableState>,
    /// Height the terminal dock reopens at (the last dragged height, or the
    /// pane default before any drag).
    terminal_height: Pixels,
    /// The same for the right dock (G-T1): the width it reopens at.
    terminal_width: Pixels,
    /// The session tab's right resource panel (R2c): Files or Changes docked
    /// beside the conversation. A workspace-scoped layout preference, like the
    /// web's `workspaceResourceState` — it follows the workspace and survives
    /// session switches; opening it never activates a session.
    resource_panel: Option<ResourcePanelKind>,
    /// Split state of the session's resource panel (the same shape the
    /// terminal dock keeps for its height).
    panel_split: Entity<ResizableState>,
    /// The same split state for the swapped arrangement (G-F8). One entity per
    /// side, because the group stores sizes by panel index: sharing one state
    /// would hand the panel's width to the chat column on the first swap.
    panel_split_left: Entity<ResizableState>,
    /// Width the resource panel reopens at (the last dragged width, or the
    /// default before any drag).
    panel_width: Pixels,
    /// Which side of the chat column the resource panel docks to (G-F8, web
    /// `SplitView` `secondarySide` + `swapPanes`). A window preference like
    /// the panel itself: it survives session and workspace switches.
    resource_panel_side: PanelSide,
    /// The resource panel fills the scene body, chat column hidden (G-F8, web
    /// `toggleMaximized` → `content-only`). Cleared whenever the panel closes;
    /// a maximized panel with nothing in it would be an empty scene.
    resource_panel_maximized: bool,
    /// One-shot automation hooks for the settings page and the changes view,
    /// mirroring `OPENBITFUN_GPUI_TERMINAL_AUTOOPEN`.
    settings_auto_open: bool,
    /// The settings page the settings nav has selected (web
    /// `useSettingsStore.activePageId`). A window preference, like the tab
    /// strip: switching tabs keeps it, and the nav column shows it while the
    /// settings scene is active.
    settings_page: settings::SettingsEntry,
    /// The settings nav's search field (web `SettingsNav`'s `SearchField`).
    /// Owned by the window because the nav is rebuilt every frame.
    settings_search_input: Entity<InputState>,
    git_auto_open: bool,
    /// One-shot automation hook (`OPENBITFUN_GPUI_AUTOOPEN_SESSION`): opens a
    /// named session once the runtime is ready, so a screenshot run can
    /// inspect a real transcript without synthesizing sidebar clicks.
    session_auto_open: Option<String>,
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
    /// Sessions list projection (grouping, ordering, show/status filters) —
    /// the web `workspaceSessionView` preferences the shell has data for.
    session_list: SessionListFilters,
    /// Sidebar session groups beyond the current workspace (web
    /// SessionsSection): one entry per recent workspace that reported rows.
    other_groups: Vec<OtherWorkspaceGroup>,
    /// The settings archived page's cross-workspace projection (G-SE30),
    /// ordered by each group's newest member (the web sorts every entry by
    /// `lastActiveAt` before grouping). Reloaded when the page is opened and
    /// after every archive-state change or delete while it is open.
    pub(crate) archived_groups: Vec<ArchivedGroupView>,
    /// Per-workspace read failures for that page (web `loadFailures`),
    /// pre-formatted; shown above the groups.
    pub(crate) archived_failures: Vec<String>,
    /// A projection read is in flight (web `loading`).
    pub(crate) archived_loading: bool,
    /// The projection answered at least once; later reads are background
    /// refreshes that keep a failed workspace's previous rows (web
    /// `hasLoadedRef`), and a fresh load collapses every group.
    pub(crate) archived_loaded: bool,
    /// Collapsed archived groups, keyed by root path. The web page collapses
    /// every group after a fresh load.
    pub(crate) archived_collapsed: Vec<String>,
    /// The archived page's in-flight row action: the success receipt to show
    /// when the projection it triggers lands (web toasts; the shell renders
    /// the same text inline). The `SessionActionFailed` handler clears it
    /// into a failure receipt instead.
    pub(crate) archived_pending: Option<String>,
    /// A reload request arrived while one was in flight; the landing
    /// projection re-reads once so a burst (delete-all) coalesces.
    archived_reload_dirty: bool,
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
    /// R1d status bar: OS login name backing the user slot.
    os_user: String,
    /// T2-16 unread turns per session: turns that finished while the window
    /// was inactive. Cleared per session on open, all via the tray action.
    unread_turns: std::collections::BTreeMap<String, (u32, bool)>,
    /// Insights page state (M14): latest reports + the open report.
    pub(crate) insights: crate::insights::InsightsPageState,
    /// Scheduled-jobs page state (M14).
    pub(crate) tasks: crate::tasks::TasksPageState,
    /// Workspaces the switcher offers: the active one first, then this
    /// process's visits, then the shared core catalog.
    workspaces: Vec<WorkspaceChoice>,
    /// Path input of the "switch workspace" dialog, reused across opens.
    workspace_input: Entity<InputState>,
    /// New-project dialog inputs (parent folder + project name), reused
    /// across opens like every dialog input.
    workspace_new_parent_input: Entity<InputState>,
    workspace_new_name_input: Entity<InputState>,
    /// Inline validation/create failure of the new-project dialog; cleared
    /// when the dialog opens.
    new_project_error: Option<String>,
    /// The action waiting for its owning workspace to become active (taken
    /// from another workspace's sidebar group): settled on
    /// [`RuntimeEvent::WorkspaceSwitched`], dropped on a failed switch.
    pending_group_action: Option<PendingGroupAction>,
    /// Last session-transcript load failure, shown in the sidebar footer
    /// status line (a failed open would otherwise only surface inside an
    /// active chat tab). Cleared by a successful open or a switch.
    session_open_error: Option<String>,
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
    /// G-M2: the paste/file import dialog (paste box, typed file path, the
    /// parsed preview rows with their target-id inputs, and the shared config
    /// snapshot the merge writes back into).
    pub(crate) mcp_import_text: Entity<TextareaState>,
    pub(crate) mcp_import_path: Entity<InputState>,
    /// G-M1/G-M3: the remote auth dialog's Bearer-token draft.
    pub(crate) mcp_auth_token_input: Entity<InputState>,
    pub(crate) mcp_import_active: bool,
    pub(crate) mcp_import_loading: bool,
    pub(crate) mcp_import_error: Option<String>,
    pub(crate) mcp_import_document: Option<serde_json::Value>,
    pub(crate) mcp_import_fingerprint: Option<String>,
    pub(crate) mcp_import_candidates: Vec<crate::manage::McpImportCandidate>,
    pub(crate) mcp_import_id_inputs: Vec<Entity<InputState>>,
    /// T2-7 pending or finished resource read in the browse dialog.
    pub(crate) mcp_resource_view: Option<crate::manage::McpResourceView>,
    /// M22 self-update state machine; the general settings page projects it.
    pub(crate) update_state: crate::update::UpdateUiState,
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
    /// G-P1: the task editor dialog's inputs and page flags. The inputs live
    /// on the window like the other dialog fields so a re-render keeps text;
    /// the flags below select the schedule/target shape and the edit target.
    pub(crate) task_name_input: Entity<InputState>,
    pub(crate) task_prompt_input: Entity<TextareaState>,
    pub(crate) task_at_input: Entity<InputState>,
    pub(crate) task_every_input: Entity<InputState>,
    pub(crate) task_anchor_input: Entity<InputState>,
    pub(crate) task_cron_input: Entity<InputState>,
    pub(crate) task_tz_input: Entity<InputState>,
    pub(crate) task_editor_target: Option<String>,
    pub(crate) task_editor_kind: crate::tasks::TaskScheduleKind,
    pub(crate) task_editor_unit: crate::tasks::TaskIntervalUnit,
    pub(crate) task_editor_target_kind: crate::tasks::TaskTargetKind,
    pub(crate) task_editor_session: Option<String>,
    pub(crate) task_editor_enabled: bool,
    pub(crate) task_editor_error: Option<String>,
    /// G-K2/G-K3: the add-skill path field.
    pub(crate) skill_path_input: Entity<InputState>,
    /// G-K4: the group editor's name field.
    pub(crate) skill_group_name_input: Entity<InputState>,
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
        let dock_h = cx.new(|_| ResizableState::default());
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let workspace_input = cx.new(|cx| InputState::new(window, cx));
        let workspace_new_parent_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.new_project_parent_placeholder.clone())
        });
        let workspace_new_name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.new_project_name_placeholder.clone())
        });
        // Editing any project-dialog field clears its inline error, the way
        // the web dialog does (`handleProjectNameChange`): a message about
        // the previous submission must not sit under a fixed value.
        for input in [
            &workspace_input,
            &workspace_new_parent_input,
            &workspace_new_name_input,
        ] {
            cx.subscribe(input, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && this.new_project_error.is_some() {
                    this.new_project_error = None;
                    cx.notify();
                }
            })
            .detach();
        }
        let export_input = cx.new(|cx| InputState::new(window, cx));
        let search_input = cx.new(|cx| InputState::new(window, cx));
        let profile_name_input = cx.new(|cx| InputState::new(window, cx));
        let profile_dir_input = cx.new(|cx| InputState::new(window, cx));
        let profile_command_input = cx.new(|cx| InputState::new(window, cx));
        let agent_id_input = cx.new(|cx| InputState::new(window, cx));
        let agent_name_input = cx.new(|cx| InputState::new(window, cx));
        let agent_description_input = cx.new(|cx| InputState::new(window, cx));
        let agent_prompt_input = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_interaction_result_input = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_json_state = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_import_text = cx.new(|cx| TextareaState::new(window, cx));
        let mcp_import_path = cx.new(|cx| InputState::new(window, cx));
        let mcp_auth_token_input = cx.new(|cx| InputState::new(window, cx));
        let websearch_credential_input = cx.new(|cx| InputState::new(window, cx));
        let task_name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.tasks_editor_name_placeholder.clone())
        });
        let task_prompt_input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(text.tasks_editor_prompt_placeholder.clone())
        });
        let task_at_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DDTHH:MM"));
        let task_every_input = cx.new(|cx| InputState::new(window, cx).placeholder("1"));
        let task_anchor_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DDTHH:MM"));
        let task_cron_input = cx.new(|cx| InputState::new(window, cx).placeholder("0 8 * * *"));
        let task_tz_input = cx.new(|cx| InputState::new(window, cx).placeholder("Asia/Shanghai"));
        let skill_path_input = cx.new(|cx| InputState::new(window, cx));
        let skill_group_name_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.skill_group_name_placeholder.clone())
        });
        let settings_search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.settings_nav_search_placeholder.clone())
        });
        // The settings nav list is a projection of this field's value, so a
        // keystroke has to repaint the window that renders it (the input owns
        // its own text; nothing else would notify the shell).
        cx.subscribe(
            &settings_search_input,
            |_: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        )
        .detach();
        // G-K5/G-A5: the skills and agents pages filter from globals, so
        // their search boxes are installed (and subscribed) here.
        crate::manage::install_page_filters(window, cx);
        // R7b plugin UI contributions: the projection owns the installed
        // scenes; the fixture provider installs behind its env gate.
        let (plugin_ui, _fixture_scene) = crate::plugin_ui::setup_projection();
        receive_runtime_events(event_rx, window, cx);
        spawn_tasks_tick(window, cx);

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
            pending_composer_draft: None,
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
            os_user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_default(),
            unread_turns: Default::default(),
            dock,
            dock_h,
            terminal_height: TerminalPane::panel_height(),
            terminal_width: px(DEFAULT_TERMINAL_WIDTH),
            resource_panel: None,
            panel_split: cx.new(|_| ResizableState::default()),
            panel_split_left: cx.new(|_| ResizableState::default()),
            panel_width: px(DEFAULT_RESOURCE_PANEL_WIDTH),
            resource_panel_side: PanelSide::Right,
            resource_panel_maximized: false,
            settings_auto_open: std::env::var_os("OPENBITFUN_GPUI_SETTINGS_AUTOOPEN").is_some(),
            settings_page: settings::SettingsEntry::General,
            settings_search_input,
            git_auto_open: std::env::var_os("OPENBITFUN_GPUI_GIT_AUTOOPEN").is_some(),
            session_auto_open: std::env::var("OPENBITFUN_GPUI_AUTOOPEN_SESSION")
                .ok()
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty()),
            show_archived: false,
            lineages: Vec::new(),
            sessions_group_collapsed: false,
            sessions_expanded: false,
            session_list: SessionListFilters::default(),
            other_groups: Vec::new(),
            archived_groups: Vec::new(),
            archived_failures: Vec::new(),
            archived_loading: false,
            archived_loaded: false,
            archived_collapsed: Vec::new(),
            archived_pending: None,
            archived_reload_dirty: false,
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
            workspace_new_parent_input,
            workspace_new_name_input,
            new_project_error: None,
            pending_group_action: None,
            session_open_error: None,
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
            mcp_import_text,
            mcp_import_path,
            mcp_auth_token_input,
            mcp_import_active: false,
            mcp_import_loading: false,
            mcp_import_error: None,
            mcp_import_document: None,
            mcp_import_fingerprint: None,
            mcp_import_candidates: Vec::new(),
            mcp_import_id_inputs: Vec::new(),
            mcp_resource_view: None,
            update_state: Default::default(),
            usage_stats: None,
            usage_range: "thisMonth".to_string(),
            usage_loading: false,
            usage_activity: None,
            usage_activity_loading: false,
            websearch_status: None,
            websearch_credential_input,
            task_name_input,
            task_prompt_input,
            task_at_input,
            task_every_input,
            task_anchor_input,
            task_cron_input,
            task_tz_input,
            skill_path_input,
            skill_group_name_input,
            task_editor_target: None,
            task_editor_kind: crate::tasks::TaskScheduleKind::At,
            task_editor_unit: crate::tasks::TaskIntervalUnit::Hour,
            task_editor_target_kind: crate::tasks::TaskTargetKind::Workspace,
            task_editor_session: None,
            task_editor_enabled: true,
            task_editor_error: None,
            plugin_ui,
            plugin_ui_scenes: std::collections::BTreeMap::new(),
            plugin_ui_auto_open: Self::plugin_ui_auto_open_requested(),
            plugin_ui_host_keys: Vec::new(),
        }
    }
}

impl ShellWindow {
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
            created_at_ms: session.created_at_ms,
            last_active_at_ms: session.last_active_at_ms,
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
        self.list_sync_reset_for(&session_id, cx);
    }

    /// [`Self::list_sync_reset`] for one named session rather than whichever
    /// tab happens to be active — a transcript reload refreshes a pane in
    /// place, including one behind the reader's active tab.
    fn list_sync_reset_for(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_for(session_id) else {
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

    /// G-M2: replaces the import paste box with one file's content and clears
    /// the previous preview (web `readImportFile`).
    fn mcp_import_update_text(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.mcp_import_error = None;
        self.mcp_import_candidates.clear();
        self.mcp_import_id_inputs.clear();
        self.mcp_import_text
            .update(cx, |state, cx| state.set_value(text, window, cx));
    }

    fn chat_mut_for(&mut self, session_id: &str) -> Option<&mut ChatView> {
        self.chats.iter_mut().find(|chat| chat.matches(session_id))
    }

    /// The active tab's chat, when it is a session tab.
    fn active_chat(&self) -> Option<&ChatView> {
        let id = self.tabs.active_session_id()?;
        self.chat_for(id)
    }
}

impl ShellWindow {
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
            RuntimeEvent::SessionLineages(lineages) => {
                self.lineages = lineages;
                self.apply_subagent_links();
            }
            RuntimeEvent::Failed(error) => {
                self.phase = RuntimePhase::Failed(i18n::failed_status(&error, cx));
            }
            RuntimeEvent::SessionOpened(opened) => {
                self.chat_error = None;
                self.session_open_error = None;
                // Opening a session is a chat action: the session tab the
                // transcript lands on activates below (tabs.open).
                // Opening the session is "reading it" (T2-16): its unread
                // turns drop out of the tray badge. A reload of an
                // already-open pane (the transcript refresh a settled turn
                // triggers) is not an open, so it keeps them — otherwise a
                // turn that finished while the window was inactive would
                // clear its own unread the moment its refresh lands.
                if !opened.reload {
                    self.clear_unread_session(&opened.session_id);
                }
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
                    // A reload carries no pane facts (the runtime sends an
                    // empty name and no model): it refreshes the transcript
                    // only, so the open pane keeps its tab title and model
                    // selector. Only a real open overwrites them.
                    if !opened.reload {
                        chat.name = opened.name;
                        chat.model_id = opened.model_id;
                    }
                    chat.turn_times = opened.turn_times;
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
                        opened.turn_times,
                        cx,
                    );
                    chat.busy = opened.busy;
                    if settled_child.is_some() {
                        chat.child_context = settled_child;
                    }
                    self.chats.push(chat);
                }
                // A transcript (re)load lands on the tab the user just
                // addressed when it is an open; a reload is not an open, so
                // it refreshes the pane in place instead. Pulling the
                // settling session's tab to the front would take the reader
                // off whatever they moved on to, and recording the hop would
                // also clobber their back/forward history with a navigation
                // they never made.
                if opened.reload {
                    self.list_sync_reset_for(&opened.session_id, cx);
                } else {
                    self.tabs.open(TabKind::Session(opened.session_id));
                    self.list_sync_reset(cx);
                }
                self.apply_subagent_links();
                tracing::info!("Session transcript ready in a session tab");
            }
            RuntimeEvent::SessionOpenFailed(error) => {
                // Loud, and outside the chat pane: the click that requested
                // this open may have had no active chat tab to show it in.
                self.session_open_error = Some(i18n::failed_status(&error, cx));
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
                    agent_id: None,
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
            RuntimeEvent::SendFailed {
                session_id,
                message: body,
                error,
            } => {
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.submitting = false;
                    // The user entry is appended on TurnAccepted; a failed
                    // submission keeps its prompt visible instead (the web
                    // failed preview), while /btw failures surface only the
                    // error notice.
                    if let Some(body) = body {
                        chat.entries
                            .push(ChatEntry::FailedUser { text: body, error });
                    } else {
                        chat.entries.push(ChatEntry::Notice {
                            text: i18n::failed_status(&error, cx),
                        });
                    }
                    self.list_sync_append(&session_id, cx);
                }
            }
            RuntimeEvent::TurnStarted {
                session_id,
                turn_id,
            } => {
                // Session-agnostic bookkeeping first (T2-6): the pump reports
                // every session's turns, and background subagent activity is
                // derived from exactly that.
                self.live_sessions.insert(session_id.clone());
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.submitting = false;
                    chat.busy = true;
                    // G-S1: the live turn's timing row opens here; the finish
                    // event closes it with the runtime's own duration.
                    chat.active_turn_id = Some(turn_id.clone());
                    let times = chat.turn_times.entry(turn_id).or_default();
                    times.started_at_ms = Some(runtime::now_ms());
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
            RuntimeEvent::TurnFinished {
                session_id,
                error,
                duration_ms,
                cancelled,
            } => {
                self.live_sessions.remove(&session_id);
                let failure = error.clone();
                if let Some(chat) = self.chat_mut_for(&session_id) {
                    chat.busy = false;
                    chat.submitting = false;
                    // G-S1: close the live turn's timing row with the
                    // completion clock and the runtime's duration.
                    if let Some(turn_id) = chat.active_turn_id.take() {
                        let times = chat.turn_times.entry(turn_id).or_default();
                        times.ended_at_ms = Some(runtime::now_ms());
                        times.duration_ms = duration_ms;
                        times.cancelled = cancelled;
                    }
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
            RuntimeEvent::AgentModelFailed(message) => {
                tracing::warn!("Subagent model update failed: {message}");
                let text = i18n::current(cx);
                crate::manage::ManageNotice::set(
                    Some(format!("{}: {message}", text.manage_agent_model_failed)),
                    cx,
                );
                cx.refresh_windows();
            }
            // G-M2: one typed import path's content landed; it replaces the
            // paste box and clears the previous preview.
            RuntimeEvent::McpImportFile(Ok(text)) => {
                self.mcp_import_update_text(text, window, cx);
                cx.refresh_windows();
            }
            RuntimeEvent::McpImportFile(Err(message)) => {
                tracing::warn!("MCP import file read failed: {message}");
                self.mcp_import_error =
                    Some(i18n::current(cx).manage_mcp_import_file_failed.clone());
                cx.refresh_windows();
            }
            // G-M1: an OAuth poll landed; only the dialog's own server
            // accepts it, so a stale loop for a previously opened server
            // cannot repaint the new one (same guard as G-A4).
            RuntimeEvent::McpOAuthSession(session) => {
                let mut accepted = session.is_none();
                if let Some(snapshot) = &session {
                    let mut state = crate::manage::McpAuthDialogState::current(cx);
                    if state.open_for(&snapshot.server_id) {
                        state.session = Some(snapshot.clone());
                        state.starting = false;
                        cx.set_global(state);
                        accepted = true;
                    }
                }
                if accepted {
                    cx.refresh_windows();
                }
            }
            // G-M3: a token save/clear (or failed OAuth start) settled. With
            // the dialog open the error lands inline; otherwise it falls back
            // to the page banner. Success closes the dialog with a notice.
            RuntimeEvent::McpAuthSaved(result) => {
                let dialog_open = crate::manage::McpAuthDialogState::current(cx)
                    .server_id
                    .is_some();
                match result {
                    Ok(()) => {
                        let saved = i18n::current(cx).manage_mcp_auth_saved.clone();
                        if dialog_open && window.has_active_dialog(cx) {
                            crate::manage::McpAuthDialogState::mutate(cx, |state| {
                                state.server_id = None;
                                state.saving = false;
                            });
                            window.close_dialog(cx);
                        } else {
                            crate::manage::McpAuthDialogState::mutate(cx, |state| {
                                state.server_id = None;
                                state.saving = false;
                            });
                        }
                        crate::manage::ManageNotice::set_success(saved, cx);
                    }
                    Err(message) => {
                        tracing::warn!("MCP remote auth operation failed: {message}");
                        let message = i18n::failed_status(&message, cx);
                        if dialog_open {
                            crate::manage::McpAuthDialogState::mutate(cx, |state| {
                                state.saving = false;
                                state.starting = false;
                                state.error = Some(message);
                            });
                        } else {
                            crate::manage::ManageNotice::set(Some(message), cx);
                        }
                    }
                }
                cx.refresh_windows();
            }
            // G-SE13 memory page: the two menu actions settle here. The
            // restored section is published back into the shared snapshot so
            // the page repaints from storage facts, not from what it wrote.
            RuntimeEvent::MemorySettingsReset(result) => {
                settings::MemoryPageState::mutate(cx, |state| state.busy = false);
                let text = i18n::current(cx);
                let notice = match result {
                    Ok(config) => {
                        cx.global_mut::<crate::settings::ShellConfig>().memories = *config;
                        (true, text.memory_settings_reset.clone())
                    }
                    Err(error) => {
                        tracing::warn!("Failed to reset memory settings: {}", error);
                        (false, text.memory_settings_reset_failed.clone())
                    }
                };
                crate::settings::MemoryNotice::set(Some(notice), cx);
            }
            RuntimeEvent::MemoryCleared(result) => {
                settings::MemoryPageState::mutate(cx, |state| state.busy = false);
                let text = i18n::current(cx);
                let notice = match result {
                    Ok(()) => (true, text.memory_reset.clone()),
                    Err(error) => {
                        tracing::warn!("Failed to clear memory data: {}", error);
                        (false, text.memory_reset_failed.clone())
                    }
                };
                crate::settings::MemoryNotice::set(Some(notice), cx);
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
            RuntimeEvent::SkillValidationFinished(result) => {
                crate::manage::SkillDialogsState::mutate(cx, |state| {
                    state.busy = false;
                    state.validation = Some(result);
                });
            }
            RuntimeEvent::SkillAdded(result) => match result {
                Ok(message) => {
                    tracing::info!("{message}");
                    crate::manage::SkillDialogsState::mutate(cx, |state| {
                        state.dialog = None;
                        state.busy = false;
                        state.validation = None;
                        state.mutation_error = None;
                    });
                    window.close_dialog(cx);
                }
                Err(error) => {
                    crate::manage::SkillDialogsState::mutate(cx, |state| {
                        state.busy = false;
                        state.mutation_error = Some(error);
                    });
                }
            },
            RuntimeEvent::SkillDeleted(result) => match result {
                Ok(message) => {
                    tracing::info!("{message}");
                    crate::manage::SkillDialogsState::mutate(cx, |state| {
                        state.dialog = None;
                        state.busy = false;
                        state.mutation_error = None;
                    });
                    window.close_dialog(cx);
                }
                Err(error) => {
                    crate::manage::SkillDialogsState::mutate(cx, |state| {
                        state.mutation_error = Some(error);
                    });
                }
            },
            RuntimeEvent::SkillGroups(result) => match result {
                Ok(groups) => {
                    crate::manage::SkillGroupsCatalog::set(
                        crate::manage::SkillGroupsCatalog {
                            loaded: true,
                            busy: false,
                            error: None,
                            groups,
                        },
                        cx,
                    );
                }
                Err(error) => {
                    tracing::warn!("Failed to load user skill groups: {error}");
                    crate::manage::SkillGroupsCatalog::mutate(cx, |state| {
                        state.loaded = true;
                        state.busy = false;
                        state.error = Some(error);
                    });
                }
            },
            RuntimeEvent::SkillGroupsSaved(result) => match result {
                Ok(groups) => {
                    let had_dialog = crate::manage::SkillGroupsDialogState::current(cx)
                        .dialog
                        .is_some();
                    crate::manage::SkillGroupsCatalog::set(
                        crate::manage::SkillGroupsCatalog {
                            loaded: true,
                            busy: false,
                            error: None,
                            groups,
                        },
                        cx,
                    );
                    crate::manage::SkillGroupsDialogState::mutate(cx, |state| {
                        state.dialog = None;
                        state.busy = false;
                        state.error = None;
                    });
                    if had_dialog {
                        window.close_dialog(cx);
                    }
                }
                Err(error) => {
                    tracing::warn!("Failed to save user skill groups: {error}");
                    crate::manage::SkillGroupsCatalog::mutate(cx, |state| {
                        state.busy = false;
                        state.error = Some(error.clone());
                    });
                    crate::manage::SkillGroupsDialogState::mutate(cx, |state| {
                        state.busy = false;
                        state.error = Some(error);
                    });
                }
            },
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
            RuntimeEvent::AgentCapabilities { profile_id, result } => {
                let current = crate::manage::AgentDetailState::current(cx);
                if current.agent_id.is_none() || current.profile_id != profile_id {
                    return;
                }
                match result {
                    Ok(payload) => {
                        crate::manage::AgentDetailState::mutate(cx, |state| {
                            state.pending_tools = payload
                                .tools
                                .iter()
                                .filter(|tool| tool.enabled)
                                .map(|tool| tool.name.clone())
                                .collect();
                            state.pending_subagents = payload
                                .subagents
                                .iter()
                                .filter(|subagent| subagent.enabled)
                                .map(|subagent| subagent.id.clone())
                                .collect();
                            state.tools = Some(payload.tools);
                            state.subagents = Some(payload.subagents);
                            state.busy = false;
                            state.tools_error = None;
                            state.subagents_error = None;
                        });
                    }
                    Err(error) => {
                        tracing::warn!("Agent capabilities load failed: {error}");
                        let message = i18n::failed_status(&error, cx);
                        crate::manage::AgentDetailState::mutate(cx, |state| {
                            state.tools_error = Some(message.clone());
                            state.subagents_error = Some(message);
                            state.busy = false;
                        });
                    }
                }
            }
            RuntimeEvent::AgentCapabilitiesSaved { profile_id, result } => {
                let current = crate::manage::AgentDetailState::current(cx);
                if current.agent_id.is_none() || current.profile_id != profile_id {
                    return;
                }
                match result {
                    Ok(payload) => {
                        crate::manage::AgentDetailState::mutate(cx, |state| {
                            state.pending_tools = payload
                                .tools
                                .iter()
                                .filter(|tool| tool.enabled)
                                .map(|tool| tool.name.clone())
                                .collect();
                            state.pending_subagents = payload
                                .subagents
                                .iter()
                                .filter(|subagent| subagent.enabled)
                                .map(|subagent| subagent.id.clone())
                                .collect();
                            state.tools = Some(payload.tools);
                            state.subagents = Some(payload.subagents);
                            state.busy = false;
                            state.tools_error = None;
                            state.subagents_error = None;
                        });
                    }
                    Err(error) => {
                        tracing::warn!("Agent capability write failed: {error}");
                        let message = i18n::failed_status(&error, cx);
                        crate::manage::AgentDetailState::mutate(cx, |state| {
                            state.busy = false;
                            state.tools_error = Some(message.clone());
                            state.subagents_error = Some(message);
                        });
                    }
                }
            }
            RuntimeEvent::McpCatalog(catalog) => {
                cx.set_global(catalog);
                cx.refresh_windows();
            }
            // E3-B5: the ACP agents page projection — the clients, the
            // requirement probes, and the config document the advanced-JSON
            // view edits.
            RuntimeEvent::AcpSnapshot(result) => {
                match result {
                    Ok(catalog) => {
                        cx.set_global(catalog);
                    }
                    Err(message) => {
                        crate::acp::AcpPageState::mutate(cx, |state| {
                            state.busy = false;
                            state.notice = Some(crate::acp::AcpNotice {
                                success: false,
                                message,
                            });
                        });
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::AcpJsonConfig(result) => {
                match result {
                    Ok(json) => {
                        crate::acp::AcpPageState::mutate(cx, |state| {
                            state.json_baseline = json.clone();
                            // A dirty draft stays the user's text; the
                            // baseline moves under it so 「还原」 returns to
                            // the document on disk.
                            if !state.json_dirty {
                                state.json_text = json;
                            }
                        });
                    }
                    Err(message) => {
                        crate::acp::AcpPageState::mutate(cx, |state| {
                            state.busy = false;
                            state.notice = Some(crate::acp::AcpNotice {
                                success: false,
                                message,
                            });
                        });
                    }
                }
                cx.refresh_windows();
            }
            RuntimeEvent::AcpAction(result) => {
                crate::acp::AcpPageState::mutate(cx, |state| {
                    state.busy = false;
                    match result {
                        Ok(key) => {
                            // The JSON save landed: the draft is the saved
                            // document, so the next snapshot may replace it.
                            if key == "json_saved" {
                                state.json_dirty = false;
                            }
                            state.notice = Some(crate::acp::AcpNotice {
                                success: true,
                                message: key,
                            });
                        }
                        Err(message) => {
                            state.notice = Some(crate::acp::AcpNotice {
                                success: false,
                                message,
                            });
                        }
                    }
                });
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
                self.mcp_import_loading = false;
                match result {
                    Ok(snapshot) => {
                        self.mcp_json_fingerprint = Some(snapshot.fingerprint.clone());
                        self.mcp_json_state.update(cx, |state, cx| {
                            state.set_value(snapshot.json_config.clone(), window, cx)
                        });
                        // G-M2: the same snapshot backs the import merge — the
                        // dialog parses it and writes back under this
                        // fingerprint.
                        if self.mcp_import_active {
                            self.mcp_import_document =
                                serde_json::from_str(&snapshot.json_config).ok();
                            self.mcp_import_fingerprint = Some(snapshot.fingerprint);
                        }
                    }
                    Err(message) => {
                        tracing::warn!("Failed to load MCP JSON config: {message}");
                        self.mcp_json_error = Some(message.clone());
                        if self.mcp_import_active {
                            self.mcp_import_error = Some(message);
                        }
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
                        self.mcp_import_active = false;
                        self.mcp_import_error = None;
                        self.mcp_import_candidates.clear();
                        self.mcp_import_id_inputs.clear();
                        window.close_dialog(cx);
                        crate::manage::ManageNotice::set_success(saved, cx);
                    }
                    Err(message) => {
                        tracing::warn!("Failed to save MCP JSON config: {message}");
                        self.mcp_json_error = Some(message.clone());
                        if self.mcp_import_active {
                            self.mcp_import_error = Some(message);
                        }
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
            RuntimeEvent::PreventSleepState(enabled) => {
                cx.set_global(settings::PreventSleepState(enabled));
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
                self.workspaces = choices.clone();
                // Sidebar groups beyond the current workspace (web
                // SessionsSection renders every workspace's sessions): one
                // list request per recent workspace, aggregated as they land.
                if let Some(handle) = runtime::runtime_handle() {
                    let current = handle.workspace().id;
                    handle.load_other_workspace_sessions(choices, &current, self.event_tx.clone());
                }
            }
            RuntimeEvent::OtherWorkspaceSessions {
                name,
                root_path,
                rows,
            } => {
                let composed: Vec<SessionRow> =
                    rows.iter().map(|row| self.compose_row(row, cx)).collect();
                if let Some(group) = self
                    .other_groups
                    .iter_mut()
                    .find(|group| group.root_path == root_path)
                {
                    group.name = name;
                    group.rows = composed;
                } else {
                    self.other_groups.push(OtherWorkspaceGroup {
                        name,
                        root_path,
                        rows: composed,
                        collapsed: false,
                        expanded: false,
                    });
                }
                cx.notify();
            }
            RuntimeEvent::ArchivedSessions(groups) => {
                let previous = std::mem::take(&mut self.archived_groups);
                let fresh = !self.archived_loaded;
                self.archived_loaded = true;
                self.archived_loading = false;
                let (composed, failures) = compose_archived_groups(groups, &previous, fresh, cx);
                self.archived_groups = composed;
                self.archived_failures = failures;
                // A landing projection settles the row action that asked for
                // it: the read exists only because the mutation succeeded.
                if let Some(success) = self.archived_pending.take() {
                    settings::ArchivedNotice::set(Some((true, success)), cx);
                }
                if fresh {
                    // The web collapses every workspace group after the first
                    // load lands.
                    self.archived_collapsed = self
                        .archived_groups
                        .iter()
                        .map(|group| group.root_path.clone())
                        .collect();
                } else {
                    // Groups that dropped out (workspace removed, last row
                    // deleted) leave no stale fold behind.
                    let roots: Vec<String> = self
                        .archived_groups
                        .iter()
                        .map(|group| group.root_path.clone())
                        .collect();
                    self.archived_collapsed.retain(|root| roots.contains(root));
                }
                cx.notify();
                if std::mem::take(&mut self.archived_reload_dirty) {
                    self.reload_archived_groups(cx);
                }
            }
            RuntimeEvent::ArchivedChanged => {
                if self.tabs.active_kind() == Some(&TabKind::Scene(SceneId::Settings))
                    && self.settings_page == settings::SettingsEntry::Archived
                {
                    self.reload_archived_groups(cx);
                }
            }
            RuntimeEvent::WorkspaceSwitched(choice) => {
                tracing::info!("Adopted workspace {} ({})", choice.name, choice.root_path);
                self.workspace_error = None;
                self.session_open_error = None;
                self.apply_workspace_reset(cx);
                // An action taken in another workspace's group runs now that
                // its owning workspace is active: a session row opens (the
                // restore reads the active workspace's store), a new session
                // is created there, or the Files panel is opened for it.
                if let Some(pending) = self.pending_group_action.take() {
                    match pending {
                        PendingGroupAction::OpenSession(session_id) => {
                            if let Some(handle) = runtime::runtime_handle() {
                                tracing::info!("Opening pending session {session_id} after switch");
                                handle.open_session(session_id, self.event_tx.clone());
                            }
                        }
                        PendingGroupAction::NewSession => {
                            if let Some(handle) = runtime::runtime_handle() {
                                tracing::info!("Creating a session in the switched workspace");
                                handle.create_session(self.event_tx.clone());
                            }
                        }
                        PendingGroupAction::OpenFiles => {
                            tracing::info!("Opening the files panel of the switched workspace");
                            self.toggle_files_view(window, cx);
                        }
                    }
                }
            }
            RuntimeEvent::WorkspaceSwitchFailed(message) => {
                tracing::warn!("Workspace switch failed: {message}");
                // The switch that was going to carry a pending action never
                // happened; running the action against the old workspace would
                // fail for a second, more confusing reason.
                self.pending_group_action = None;
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
                // An archived-page row action reports its failure where it was
                // fired from (the web toasts it; the shell's page shows the
                // receipt inline).
                if self.archived_pending.take().is_some() {
                    settings::ArchivedNotice::set(Some((false, line.clone())), cx);
                }
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
            RuntimeEvent::ModelTestFinished { model_id, result } => {
                crate::settings::ModelTestState::mutate(cx, |state| {
                    state
                        .receipts
                        .insert(model_id, crate::settings::ModelTestReceipt::Done(result));
                });
                cx.refresh_windows();
            }
            RuntimeEvent::HookImports(snapshot) => {
                settings::HookImportState::mutate(cx, |state| {
                    state.snapshot = Some(*snapshot);
                    state.loaded = true;
                    state.loading = false;
                    state.busy = false;
                    state.error = None;
                    state.plan = None;
                    state.plan_stale = false;
                });
                cx.refresh_windows();
            }
            RuntimeEvent::HookImportPlan(plan) => {
                settings::HookImportState::mutate(cx, |state| {
                    state.plan = Some(*plan);
                    state.plan_stale = false;
                    state.busy = false;
                    state.error = None;
                });
                cx.refresh_windows();
            }
            RuntimeEvent::HookImportPlanStale(plan) => {
                settings::HookImportState::mutate(cx, |state| {
                    state.plan = Some(*plan);
                    state.plan_stale = true;
                    state.busy = false;
                });
                cx.refresh_windows();
            }
            RuntimeEvent::HookImportFailed(error) => {
                settings::HookImportState::mutate(cx, |state| {
                    state.error = Some(error);
                    state.loading = false;
                    state.busy = false;
                });
                cx.refresh_windows();
            }
            RuntimeEvent::WorktreeProjects(projects) => {
                settings::WorktreeProjectsState::mutate(cx, |state| {
                    state.projects = projects;
                    state.loaded = true;
                    state.loading = false;
                    state.error = None;
                });
                cx.refresh_windows();
            }
            RuntimeEvent::WorktreeProjectsFailed(error) => {
                tracing::warn!("Worktree project list failed: {error}");
                settings::WorktreeProjectsState::mutate(cx, |state| {
                    state.loaded = true;
                    state.loading = false;
                    state.error = Some((String::new(), error));
                });
                cx.refresh_windows();
            }
            RuntimeEvent::WorktreeRemoved(worktree_id) => {
                settings::WorktreeProjectsState::mutate(cx, |state| {
                    state.removing = None;
                });
                tracing::info!("Worktree removed from the settings page: {worktree_id}");
                settings::fetch_worktree_projects(cx);
                cx.refresh_windows();
            }
            RuntimeEvent::WorktreeRemoveFailed { code, message } => {
                tracing::warn!("Worktree removal failed: code={code}, message={message}");
                settings::WorktreeProjectsState::mutate(cx, |state| {
                    state.removing = None;
                    state.error = Some((code, message));
                });
                cx.refresh_windows();
            }
        }
        cx.notify();
    }
}

impl ShellWindow {
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
}

impl ShellWindow {
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
            tag: "openbitfun-gpui-turn".into(),
            title: title.into(),
            body: session.into(),
            actions: Vec::new(),
        });
    }
}

/// `list` splice range invalidating the cached height of a transcript tail
/// that changed in place.
fn tail_changed_range(count: usize) -> std::ops::Range<usize> {
    (count - 1)..count
}

impl Render for ShellWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // G-SE5: every path that can change the UI size (first frame, config
        // snapshot, live switch) repaints, so the em size is applied here —
        // the one place each window passes through with a mutable window.
        settings::apply_ui_font_size(window, cx);
        if let Some(draft) = self.pending_composer_draft.take() {
            // Composer text staged by another surface (G-G1 git init card):
            // `set_value` needs the window, which only render holds.
            cx.update_entity(&self.draft, |state, cx| {
                state.set_value(draft, window, cx);
            });
        }
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
            if let Some(session_id) = self.session_auto_open.take() {
                if let Some(handle) = runtime::runtime_handle() {
                    handle.open_session(session_id, self.event_tx.clone());
                }
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
            Some(TabKind::Scene(SceneId::Settings)) => div()
                .id("settings-page")
                .debug_selector(|| "settings-page".to_string())
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .flex_col()
                // The desktop settings content is one bounded column centered
                // in the scene (the card path's own 760px reading column with
                // `margin-inline: auto`); a maximized window otherwise
                // stretches every row edge to edge.
                .child(
                    div()
                        .debug_selector(|| "settings-column".to_string())
                        .relative()
                        .w_full()
                        .max_w(px(SETTINGS_CONTENT_WIDTH))
                        .mx_auto()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(settings::render_page(
                            &text,
                            cx.entity().downgrade(),
                            self.settings_page,
                            cx,
                        )),
                )
                .into_any_element(),
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
                .debug_selector(|| "insights-page".to_string())
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(crate::insights::render_page(
                    &self.insights,
                    cx.entity().downgrade(),
                    &self.event_tx,
                    &text,
                    cx,
                ))
                .into_any_element(),
            Some(TabKind::Scene(SceneId::Tasks)) => div()
                .id("tasks-page")
                .debug_selector(|| "tasks-page".to_string())
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(crate::tasks::render_page(
                    &self.tasks,
                    cx.entity().downgrade(),
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
            // R7 seam: plugin-contributed scenes dispatch through the
            // contribution (one arm, not one per scene).
            Some(TabKind::Scene(SceneId::External(reference))) => {
                self.render_external_scene(reference, window, &text, cx)
            }
            // The desktop MiniApp/extension scenes have no shell runtime yet:
            // the tab renders the branded empty card instead of a fake
            // surface (loud degrade). The extension card carries the one
            // honest action — the external-agent settings page that the copy
            // names — while the MiniApp card stays actionless: no destination
            // exists yet, and an empty state never fakes a link (E4 batch ①).
            Some(TabKind::Scene(SceneId::MiniApps)) => div()
                .id("miniapps-page")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_y_scroll()
                .child(brand_empty_card(
                    IconName::LayoutDashboard,
                    &text.miniapps_title,
                    &text.miniapps_empty,
                    None,
                    cx,
                ))
                .into_any_element(),
            Some(TabKind::Scene(SceneId::Extensions)) => {
                let open_acp = {
                    let shell = cx.entity().downgrade();
                    Button::new("extensions-open-acp")
                        .debug_selector(|| "extensions-open-acp".to_string())
                        .label(text.extensions_open_acp.clone())
                        .small()
                        .with_variant(ButtonVariant::Secondary)
                        .on_click(move |_, _, cx| {
                            let Some(shell) = shell.upgrade() else {
                                return;
                            };
                            shell.update(cx, |shell, cx| {
                                shell
                                    .set_settings_page(settings::SettingsEntry::ExternalAgents, cx);
                                shell.open_settings_view(cx);
                            });
                        })
                };
                div()
                    .id("extensions-page")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .child(brand_empty_card(
                        IconName::Puzzle,
                        &text.extensions_title,
                        &text.extensions_empty,
                        Some(open_acp.into_any_element()),
                        cx,
                    ))
                    .into_any_element()
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
        // the user cluster and the tool buttons) and the scene face
        // (a 45px top bar with the tab strip, the session actions, and the
        // window controls, over the dock frame). No standalone title bar and
        // no full-width bottom status bar: the desktop dropped both when it
        // simplified its shell (`92ce04f8d`), and the drawer fold takes the
        // whole nav column out of the layout.
        // The settings scene owns the nav column (web `nav-registry`: the
        // settings scene registers `SettingsNav` as its NavPanel, which
        // replaces MainNav while the scene is active). The footer stays: the
        // desktop keeps its user cluster under the settings tree.
        let settings_active = self.tabs.active_kind() == Some(&TabKind::Scene(SceneId::Settings));
        let nav_body: gpui_kit::AnyElement = if settings_active {
            settings::render_nav(
                &text,
                self.settings_page,
                &self.settings_search_input,
                SESSION_PANE_WIDTH,
                cx.entity().downgrade(),
                cx,
            )
        } else {
            self.render_sidebar(&text, window, cx).into_any_element()
        };

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
                    .child(nav_body),
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
            // The scene body and the docked terminal are one split: the
            // divider above (or beside, in the right dock, G-T1) the terminal
            // drags its extent, and the scene keeps the rest. Closed, the
            // terminal panel is simply absent.
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
                        let dock = self.terminal_dock(cx);
                        if self.terminal_open {
                            let panel = match dock {
                                TerminalDock::Bottom => resizable_panel()
                                    .size(self.terminal_height)
                                    .size_range(px(MIN_TERMINAL_HEIGHT)..px(MAX_TERMINAL_HEIGHT))
                                    .child(self.render_terminal_panel(&text, dock, cx)),
                                TerminalDock::Right => resizable_panel()
                                    .size(self.terminal_width)
                                    .size_range(px(MIN_TERMINAL_HEIGHT)..px(MAX_TERMINAL_HEIGHT))
                                    .child(self.render_terminal_panel(&text, dock, cx)),
                            };
                            panels.push(panel.flex_none());
                        }
                        let state = self.terminal_dock_state(cx);
                        let group = match dock {
                            TerminalDock::Bottom => v_resizable("shell-dock"),
                            TerminalDock::Right => h_resizable("shell-dock"),
                        };
                        // The tooltip reports the tier the panel sits in right
                        // now, read from the live split state.
                        let remembered = match dock {
                            TerminalDock::Bottom => self.terminal_height,
                            TerminalDock::Right => self.terminal_width,
                        };
                        let extent = state.read(cx).sizes().get(1).copied().unwrap_or(remembered);
                        group
                            .with_state(&state)
                            // The divider carries the double-click size cycle
                            // (web `handleBottomDoubleClick`); the kit's own
                            // appearance is kept and wrapped.
                            .with_handle_appearance(terminal_dock_handle_appearance(
                                cx.entity().downgrade(),
                                dock,
                                dock.tier(extent),
                                &text,
                            ))
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

/// E4-b2: the task board's 30-second clock (web `CLOCK_TICK_MS`). While the
/// tasks scene is the active tab it ages the countdowns and re-pulls the
/// shared store, so a job changed by another surface — the agent's own
/// scheduling tool, a workspace editor — arrives without a manual reload; the
/// shell runtime has no change-push channel, so the poll is the board's
/// equivalent of the web panel's `SCHEDULED_JOBS_CHANGED` subscription.
fn spawn_tasks_tick(window: &Window, cx: &mut Context<ShellWindow>) {
    use std::time::Duration;
    cx.spawn_in(window, async move |this, cx| loop {
        cx.background_executor()
            .timer(Duration::from_secs(30))
            .await;
        let alive = cx
            .update(|_, cx| {
                this.update(cx, |shell, cx| {
                    let tasks_active =
                        shell.tabs.active_kind() == Some(&TabKind::Scene(SceneId::Tasks));
                    if tasks_active && shell.tasks.loaded {
                        shell.tasks.now_ms = chrono::Utc::now().timestamp_millis();
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.reload_cron_jobs(shell.event_tx.clone());
                        }
                        cx.notify();
                    }
                })
                .is_ok()
            })
            .unwrap_or(false);
        if !alive {
            break;
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
        .with_assets(crate::ely_support::compose_assets(ShellAssets))
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            crate::ely_support::init(cx);

            // Global search palette shortcut (R1b), matching the desktop's
            // Ctrl K, plus the sidebar drawer toggle (web Ctrl B). Both come
            // from the settings table that the shortcuts page (G-SE7)
            // documents, so the two cannot drift. No key context: the
            // bindings match wherever no focused view consumes the keystroke
            // first.
            settings::install_global_shortcuts(cx);

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
