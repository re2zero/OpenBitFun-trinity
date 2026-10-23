//! Git changes panel: the workspace git surface over the runtime git port —
//! the workspace diff snapshot rendered as a file list plus self-drawn
//! unified-diff rows, and the write path (stage/unstage, commit, push), a
//! read-only commit history, and the repository ownership-trust banner.
//!
//! The pinned component library (0.7.1) has no diff component, so the unified
//! patch produced by the port is parsed here and painted line by line (PLAN
//! M8 / D27). Write operations go through the same port the agent's git tools
//! use (PLAN M18 / D61); every `PermissionDenied` failure probes the trust
//! report, so an untrusted repository raises its banner instead of staying a
//! bare error.

use std::future::Future;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, ResizableState};
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, StyledExt};
use gpui_kit::{
    div, list, prelude::*, px, App, ClipboardItem, Context, Entity, IntoElement, ListAlignment,
    ListState, Pixels, Render, SharedString, TextRun, WeakEntity, Window,
};
use openbitfun_core::service::filesystem::FileSystemService;
use openbitfun_core::service::git::{GitBranch, GitGraph};
use openbitfun_core::service::snapshot::open_snapshot_manager_for_view;
use openbitfun_core::service::workspace::WorkspaceKind;
use openbitfun_core::service::worktree::{
    WorktreeCreateBranchRequest, WorktreeCreateRequest, WorktreeListRequest,
    WorktreePromoteRequest, WorktreeRemoveRequest, WorktreeService, WorktreeSessionBindingRequest,
};
use openbitfun_core_types::{WorktreeLifecycle, WorktreeSummary};
use openbitfun_runtime_ports::{
    GitCommitSummary, GitPort, GitRepositoryOverview, GitTrustSnapshot, GitTrustState, PortError,
    PortErrorKind, WorkspaceDiffContent, WorkspaceDiffFile, WorkspaceDiffFileStatus,
    WorkspaceDiffSnapshot,
};
use unicode_width::UnicodeWidthStr;

use crate::i18n::{self, ShellText};

/// Width of the changed-file column inside the panel.
const FILE_COLUMN_WIDTH: f32 = 300.;
/// Drag range of the main-tab file-column ↔ diff separator (G-G5).
const MIN_FILE_COLUMN_WIDTH: f32 = 180.;
const MAX_FILE_COLUMN_WIDTH: f32 = 560.;

/// Diff row typography: `text_xs` (0.75rem) with the theme's monospace family,
/// duplicated here so the unwrapped layout can shape a row at the same metrics.
const DIFF_FONT_SIZE: f32 = 12.;
/// Horizontal padding of one diff row (`px_2`, both sides).
const DIFF_ROW_PADDING: f32 = 16.;

/// One rendered diff row kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    /// File headers and index bookkeeping (`diff --git`, `index`, `---`/`+++`).
    Meta,
    /// Hunk header (`@@ -a,b +c,d @@`).
    Hunk,
    Add,
    Delete,
    Context,
}

/// One display row of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

/// Parses a unified diff patch (libgit2 `Patch::to_buf` output, served by the
/// runtime git port) into display rows.
pub(crate) fn parse_unified_patch(patch: &str) -> Vec<DiffLine> {
    patch
        .lines()
        .map(|line| {
            // Prefix checks run longest-first so `---`/`+++` are not read as
            // a deletion/addition.
            let kind = if line.starts_with("@@") {
                DiffLineKind::Hunk
            } else if line.starts_with("diff ")
                || line.starts_with("index ")
                || line.starts_with("---")
                || line.starts_with("+++")
                || line.starts_with("new file")
                || line.starts_with("deleted file")
                || line.starts_with("old mode")
                || line.starts_with("new mode")
                || line.starts_with("similarity index")
                || line.starts_with("rename ")
                || line.starts_with("Binary files")
                || line.starts_with('\\')
            {
                DiffLineKind::Meta
            } else if line.starts_with('+') {
                DiffLineKind::Add
            } else if line.starts_with('-') {
                DiffLineKind::Delete
            } else {
                DiffLineKind::Context
            };
            DiffLine {
                kind,
                text: line.to_string(),
            }
        })
        .collect()
}

/// The row whose rendered width decides the unwrapped layout. Selection uses
/// display width (CJK glyphs count as two columns) because the rows are drawn
/// in the monospace family, where that is what a column costs.
fn widest_line(lines: &[DiffLine]) -> Option<&DiffLine> {
    lines
        .iter()
        .max_by_key(|line| UnicodeWidthStr::width(line.text.as_str()))
}

/// Single-letter status marker (git porcelain vocabulary).
pub(crate) fn status_letter(status: WorkspaceDiffFileStatus) -> &'static str {
    match status {
        WorkspaceDiffFileStatus::Added => "A",
        WorkspaceDiffFileStatus::Modified => "M",
        WorkspaceDiffFileStatus::Deleted => "D",
        WorkspaceDiffFileStatus::Renamed => "R",
        WorkspaceDiffFileStatus::Conflicted => "U",
    }
}

/// Index status marker for one changed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageMarker {
    Staged,
    Unstaged,
    Untracked,
}

impl StageMarker {
    fn label(self, text: &ShellText) -> &str {
        match self {
            Self::Staged => &text.git_staged,
            Self::Unstaged => &text.git_unstaged,
            Self::Untracked => &text.git_untracked,
        }
    }
}

fn stage_markers(file: &WorkspaceDiffFile) -> Vec<StageMarker> {
    if file.untracked {
        return vec![StageMarker::Untracked];
    }
    let mut markers = Vec::new();
    if file.staged {
        markers.push(StageMarker::Staged);
    }
    if file.unstaged {
        markers.push(StageMarker::Unstaged);
    }
    markers
}

/// The one staging action a changed-file row offers. A file with worktree
/// content (unstaged or untracked) stages; a file whose changes are entirely
/// staged unstages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageAction {
    Stage,
    Unstage,
}

fn row_stage_action(file: &WorkspaceDiffFile) -> StageAction {
    if file.unstaged || file.untracked {
        StageAction::Stage
    } else {
        StageAction::Unstage
    }
}

/// Which body the panel shows below the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitView {
    Files,
    History,
    /// Managed-worktree lifecycle over the core `WorktreeService` (M19).
    Worktrees,
    /// Snapshot-system session list over the workspace snapshot manager (M19).
    Snapshots,
    /// Branch list with create/switch/delete over the core `GitService` (G-G4).
    Branches,
    /// Commit graph over the core `GitService` graph builder (G-G4).
    Graph,
}

/// How many commits the history view loads per request.
const HISTORY_LIMIT: usize = 50;

/// How many nodes the graph view loads per request (G-G4).
const GRAPH_LIMIT: usize = 100;

/// One snapshot-session row of the snapshots view: the core `SessionStats`
/// facts the snapshot manager serves per session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotRow {
    pub session_id: String,
    pub total_files: usize,
    pub total_turns: usize,
    pub total_changes: usize,
}

/// Parses the stats payload served by the snapshot manager
/// (`SessionStats` serialized with its plain field names) into a row.
/// Missing facts keep the row renderable with zeros instead of dropping it.
pub(crate) fn snapshot_row_from_stats(session_id: &str, stats: &serde_json::Value) -> SnapshotRow {
    let read = |key: &str| stats.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    SnapshotRow {
        session_id: session_id.to_string(),
        total_files: read("total_files"),
        total_turns: read("total_turns"),
        total_changes: read("total_changes"),
    }
}

/// Display model of one worktree row: the identity line plus the flags the
/// row renders as badges, resolved off the render path for testability.
pub(crate) struct WorktreeRowView {
    pub title: String,
    pub subtitle: String,
    pub head: String,
    pub lifecycle: WorktreeLifecycle,
    pub main: bool,
    pub dirty: bool,
    pub locked: bool,
    pub missing: bool,
    pub session_count: usize,
    /// Whether promote (managed-only) applies to this row.
    pub promotable: bool,
    /// Whether remove applies: the main checkout is never removable here.
    pub removable: bool,
}

impl WorktreeRowView {
    /// Short HEAD hash for the identity line (`None`/all-zero heads degrade
    /// to a dash).
    pub(crate) fn head_short(&self) -> String {
        let head = self.head.trim();
        if head.len() >= 7 && head.chars().any(|c| c != '0') {
            head[..7].to_string()
        } else {
            "-".to_string()
        }
    }
}

pub(crate) fn worktree_row_view(summary: &WorktreeSummary) -> WorktreeRowView {
    WorktreeRowView {
        title: summary
            .branch
            .clone()
            .unwrap_or_else(|| summary.path.clone()),
        subtitle: summary.path.clone(),
        head: summary.head.clone(),
        lifecycle: summary.lifecycle,
        main: summary.is_main,
        dirty: summary.dirty,
        locked: summary.locked,
        missing: summary.missing,
        session_count: summary.associated_session_count,
        promotable: summary.lifecycle == WorktreeLifecycle::Managed,
        removable: !summary.is_main,
    }
}

/// A per-file row action awaiting confirmation (G-G3): the first click arms
/// it, the second click runs it. At most one row action is armed at a time,
/// and any refresh or other row click disarms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileRowAction {
    /// Restore the tracked path (index + worktree) back to HEAD.
    Discard(usize),
    /// Delete an untracked file from the worktree.
    Delete(usize),
}

/// The docked changes view: a changed-file list on the left, the diff of the
/// selected file on the right, plus the write path (stage/unstage, commit,
/// push), a read-only history view, and the untrusted-repository banner.
pub(crate) struct GitPanel {
    files: Vec<WorkspaceDiffFile>,
    selected: usize,
    loading: bool,
    error: Option<String>,
    truncated: bool,
    /// Parsed rows of the selected file's patch; rebuilt on selection change.
    diff: Rc<Vec<DiffLine>>,
    /// Rows wrap at the viewport width (default). Unwrapped, every row keeps
    /// its own line and the view scrolls horizontally instead.
    wrap_lines: bool,
    file_list: ListState,
    diff_list: ListState,
    /// Files vs commit-history body.
    view: GitView,
    /// Branch/remotes facts fetched alongside every diff refresh.
    overview: Option<GitRepositoryOverview>,
    /// Recent commits of the current branch (history view).
    commits: Vec<GitCommitSummary>,
    /// Whether the history view already loaded its commits.
    history_loaded: bool,
    /// Message box for the next commit.
    commit_input: Entity<InputState>,
    /// A port operation is in flight; header and commit actions disable.
    busy: bool,
    /// Failure of the last write/history action (a failed refresh has its own
    /// body error). Cleared by the next successful operation or refresh.
    action_error: Option<String>,
    /// Untrusted-repository banner state; `Some` only while trust is required.
    trust: Option<GitTrustSnapshot>,
    /// Set after a successful commit; render clears the message box (it is
    /// the only place holding the window `set_value` needs).
    clear_commit_input: bool,
    /// AI commit-message generation is in flight (G-G2); the input row's
    /// generate button flips to its cancel shape while this is set.
    generating_commit: bool,
    /// Bumps on every generate/cancel: a completion for a superseded
    /// generation (cancelled or restarted) is dropped instead of painted.
    generation_epoch: u64,
    /// Generated message waiting for render to move it into the input
    /// (`set_value` needs the window, which only render holds).
    pending_commit_message: Option<String>,
    history_list: ListState,
    /// Branch rows of the branches view (G-G4), enhanced facts included.
    branches: Vec<GitBranch>,
    /// Whether the branches view already loaded its rows.
    branches_loaded: bool,
    branch_list: ListState,
    /// Name of the branch awaiting delete confirmation (two-step, G-G4).
    confirm_branch_delete: Option<String>,
    /// Hash of the graph node awaiting reset confirmation (two-step, G-G4).
    confirm_graph_reset: Option<String>,
    /// Loaded commit graph of the graph view (G-G4).
    graph: Option<GitGraph>,
    /// Whether the graph view already loaded its rows.
    graph_loaded: bool,
    graph_list: ListState,
    /// Branch name for the next branch creation (G-G4).
    new_branch_input: Entity<InputState>,
    /// Optional start point the next branch is cut from (G-G4).
    branch_base_input: Entity<InputState>,
    /// Set after a created branch; render clears both branch inputs.
    clear_branch_inputs: bool,
    /// Managed worktrees of the project owning the active workspace (M19).
    worktrees: Vec<WorktreeSummary>,
    /// Whether the worktrees view already loaded its rows.
    worktrees_loaded: bool,
    worktree_list: ListState,
    /// Branch name (optional) for the next worktree: non-empty names a branch
    /// for the freshly created worktree after it exists.
    worktree_branch_input: Entity<InputState>,
    /// Base ref (optional) the next worktree is cut from; empty follows the
    /// service default (HEAD).
    worktree_base_input: Entity<InputState>,
    /// Set after a created worktree; render clears the branch input.
    clear_worktree_input: bool,
    /// Snapshot-system session rows of the active workspace (M19).
    snapshots: Vec<SnapshotRow>,
    /// Whether the snapshots view already loaded its rows.
    snapshots_loaded: bool,
    snapshot_list: ListState,
    /// The chat's open session, captured at panel open; the session-isolation
    /// toggle addresses it. `None` (no open session) disables the toggle.
    active_session: Option<String>,
    /// The active workspace is remote: worktree management and the local
    /// snapshot runtime do not reach it, so both views say so explicitly.
    remote_workspace: bool,
    /// Docked right-panel form (R2c): the file list takes the whole panel and
    /// the diff column is dropped — a 360px panel cannot fit the fixed 300px
    /// file column plus a readable diff. The main tab keeps both columns.
    compact: bool,
    /// The active workspace is not a git repository at all (the port answered
    /// `NotFound`): the body offers the init card instead of a bare error
    /// (G-G1). Cleared by the next successful refresh.
    not_a_repository: bool,
    /// Row action waiting for its confirming second click (G-G3).
    confirm_action: Option<FileRowAction>,
    /// Shell handle for the "initialize repository" composer hand-off; `None`
    /// when the panel is mounted standalone in tests (G-G1).
    shell: Option<WeakEntity<crate::shell::ShellWindow>>,
    /// Drag state of the main-tab file-column ↔ diff separator (G-G5).
    file_split: Entity<ResizableState>,
}

impl GitPanel {
    /// Switches between the main tab (files + diff) and the docked panel
    /// (files only) mount forms; called by the shell on the mount move.
    pub(crate) fn set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.compact != compact {
            self.compact = compact;
            cx.notify();
        }
    }
}

impl GitPanel {
    pub(crate) fn new(
        active_session: Option<String>,
        shell: Option<WeakEntity<crate::shell::ShellWindow>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = i18n::current(cx);
        let commit_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(text.git_commit_placeholder.clone()));
        let worktree_branch_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.git_worktree_branch_placeholder.clone())
        });
        let worktree_base_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(text.git_worktree_base_ref_placeholder.clone())
        });
        let new_branch_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(text.git_branch_name_label.clone()));
        let branch_base_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(text.git_branch_base_label.clone()));
        let remote_workspace = crate::runtime::runtime_handle()
            .map(|handle| matches!(handle.workspace().workspace_kind, WorkspaceKind::Remote))
            .unwrap_or(false);
        let mut panel = Self {
            files: Vec::new(),
            selected: 0,
            loading: false,
            error: None,
            truncated: false,
            diff: Rc::new(Vec::new()),
            wrap_lines: true,
            file_list: ListState::new(0, ListAlignment::Top, px(40.)),
            diff_list: ListState::new(0, ListAlignment::Top, px(40.)),
            view: GitView::Files,
            overview: None,
            commits: Vec::new(),
            history_loaded: false,
            commit_input,
            busy: false,
            action_error: None,
            trust: None,
            clear_commit_input: false,
            generating_commit: false,
            generation_epoch: 0,
            pending_commit_message: None,
            history_list: ListState::new(0, ListAlignment::Top, px(40.)),
            branches: Vec::new(),
            branches_loaded: false,
            branch_list: ListState::new(0, ListAlignment::Top, px(48.)),
            confirm_branch_delete: None,
            confirm_graph_reset: None,
            graph: None,
            graph_loaded: false,
            graph_list: ListState::new(0, ListAlignment::Top, px(48.)),
            new_branch_input: new_branch_input.clone(),
            branch_base_input,
            clear_branch_inputs: false,
            worktrees: Vec::new(),
            worktrees_loaded: false,
            worktree_list: ListState::new(0, ListAlignment::Top, px(64.)),
            worktree_branch_input,
            worktree_base_input,
            clear_worktree_input: false,
            snapshots: Vec::new(),
            snapshots_loaded: false,
            snapshot_list: ListState::new(0, ListAlignment::Top, px(56.)),
            active_session,
            remote_workspace,
            compact: false,
            not_a_repository: false,
            confirm_action: None,
            shell,
            file_split: cx.new(|_| ResizableState::default()),
        };
        panel.refresh(cx);
        panel
    }

    /// Reloads the workspace diff snapshot and repository overview through
    /// the runtime git port.
    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            self.fail("runtime not ready", cx);
            return;
        };
        let Some(git) = handle.git() else {
            self.fail("git port unavailable", cx);
            return;
        };
        self.loading = true;
        self.error = None;
        self.confirm_action = None;
        cx.notify();
        // The port runs on the runtime thread (its provider may block on
        // libgit2); the gpui side only awaits the join handle.
        let task = handle.tokio_handle().spawn(async move {
            let diff = git.workspace_diff().await;
            let overview = git.repository_overview().await;
            (diff, overview)
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.loading = false;
                match outcome {
                    Ok((Ok(snapshot), overview)) => {
                        panel.action_error = None;
                        panel.not_a_repository = false;
                        panel.overview = overview.ok();
                        panel.apply_snapshot(snapshot, cx);
                    }
                    Ok((Err(error), _)) => {
                        // "Not a repository" is a state, not a failure: it
                        // gets the init card instead of the error line (G-G1).
                        if error.kind == PortErrorKind::NotFound {
                            panel.enter_not_a_repository(cx);
                        } else {
                            tracing::warn!("Workspace diff failed: {}", error);
                            panel.fail(&error.to_string(), cx);
                            panel.handle_port_failure(&error, cx);
                        }
                    }
                    Err(join_error) => {
                        tracing::error!("Workspace diff task failed: {}", join_error);
                        panel.fail("workspace diff task failed", cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn fail(&mut self, error: &str, cx: &mut Context<Self>) {
        self.loading = false;
        self.error = Some(i18n::git_failed(error, cx));
        cx.notify();
    }

    /// Classifies a port failure. Permission-denied probes the trust report
    /// (an untrusted repository refuses every operation, reads included) and
    /// the message becomes the action error; the banner replaces it once the
    /// user grants trust.
    fn handle_port_failure(&mut self, error: &PortError, cx: &mut Context<Self>) {
        if error.kind == PortErrorKind::PermissionDenied {
            self.probe_trust(cx);
        }
        self.action_error = Some(error.message.clone());
    }

    /// Runs one write-shaped port call: disables the actions, awaits the
    /// result on the runtime thread, then refreshes or surfaces the failure.
    fn run_write<F, Fut, Done>(&mut self, op: F, on_success: Done, cx: &mut Context<Self>)
    where
        F: FnOnce(Arc<dyn GitPort>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), PortError>> + Send + 'static,
        Done: FnOnce(&mut Self, &mut Context<Self>) + 'static,
    {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let Some(git) = handle.git() else {
            return;
        };
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(op(git));
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(())) => {
                        panel.action_error = None;
                        on_success(panel, cx);
                        panel.refresh(cx);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git write failed: {}", error);
                        panel.handle_port_failure(&error, cx);
                    }
                    Err(join_error) => {
                        tracing::error!("Git write task failed: {}", join_error);
                        panel.action_error = Some("git write task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn stage_all(&mut self, cx: &mut Context<Self>) {
        self.run_write(
            |git| async move { git.stage_files(Vec::new(), true, false).await },
            |_, _| {},
            cx,
        );
    }

    fn stage_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(path) = self.files.get(index).map(|file| file.path.clone()) else {
            return;
        };
        self.run_write(
            move |git| async move { git.stage_files(vec![path], false, false).await },
            |_, _| {},
            cx,
        );
    }

    fn unstage_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(path) = self.files.get(index).map(|file| file.path.clone()) else {
            return;
        };
        self.run_write(
            move |git| async move { git.unstage_files(vec![path]).await },
            |_, _| {},
            cx,
        );
    }

    fn commit_staged(&mut self, cx: &mut Context<Self>) {
        let message = self.commit_input.read(cx).value().trim().to_string();
        if message.is_empty() {
            return;
        }
        // The message is captured up front; the box clears on success in the
        // next paint (set_value needs the window, which only render holds).
        self.run_write(
            move |git| async move { git.commit_staged(message, false).await },
            |panel, _| panel.clear_commit_input = true,
            cx,
        );
    }

    fn push(&mut self, cx: &mut Context<Self>) {
        self.run_write(
            |git| async move { git.push(None, None, false).await },
            |_, _| {},
            cx,
        );
    }

    /// Pulls the current branch from its upstream (G-G6); the write path's
    /// refresh brings the new ahead/behind counts back.
    fn pull(&mut self, cx: &mut Context<Self>) {
        self.run_write(
            |git| async move { git.pull(None, None).await },
            |_, _| {},
            cx,
        );
    }

    /// The port answered `NotFound`: the workspace is not a repository at all,
    /// so the body switches to the init card instead of an error line (G-G1).
    fn enter_not_a_repository(&mut self, cx: &mut Context<Self>) {
        self.loading = false;
        self.error = None;
        self.action_error = None;
        self.trust = None;
        self.not_a_repository = true;
        self.overview = None;
        self.files.clear();
        self.selected = 0;
        self.diff = Rc::new(Vec::new());
        self.truncated = false;
        self.file_list.reset(0);
        self.diff_list.reset(0);
        cx.notify();
    }

    /// G-G1: hands the init prompt to the shell composer instead of running
    /// the command — the same contract as the desktop scene, where the agent
    /// performs the initialization with the user watching.
    fn request_git_init(&mut self, cx: &mut Context<Self>) {
        let Some(shell) = self.shell.clone() else {
            return;
        };
        let prompt = i18n::current(cx).git_init_prompt.clone();
        shell
            .update(cx, |shell, cx| shell.prefill_composer(prompt, cx))
            .ok();
    }

    /// First click arms the row action, the second runs it (G-G3). Arming a
    /// different row moves the confirmation; refresh disarms everything.
    fn row_action(&mut self, action: FileRowAction, cx: &mut Context<Self>) {
        if self.confirm_action == Some(action) {
            self.confirm_action = None;
            match action {
                FileRowAction::Discard(index) => self.discard_file(index, cx),
                FileRowAction::Delete(index) => self.delete_file(index, cx),
            }
        } else {
            self.confirm_action = Some(action);
            cx.notify();
        }
    }

    /// Restores one tracked path back to HEAD through the port (G-G3).
    fn discard_file(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(path) = self.files.get(index).map(|file| file.path.clone()) else {
            return;
        };
        self.run_write(
            move |git| async move { git.discard_files(vec![path]).await },
            |_, _| {},
            cx,
        );
    }

    /// Deletes an untracked file (G-G3). Untracked files are outside git's
    /// model, so this goes through the workspace filesystem service — which
    /// is exactly why the action stays disabled for remote workspaces, whose
    /// files this process cannot reach.
    fn delete_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.remote_workspace {
            return;
        }
        let Some(file) = self.files.get(index).cloned() else {
            return;
        };
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let target = handle
            .workspace()
            .root_path
            .join(&file.path)
            .to_string_lossy()
            .to_string();
        self.busy = true;
        cx.notify();
        let task = handle
            .tokio_handle()
            .spawn(async move { FileSystemService::default().delete_file(&target).await });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(())) => {
                        panel.action_error = None;
                        panel.refresh(cx);
                    }
                    Ok(Err(error)) => panel.action_error = Some(error.to_string()),
                    Err(join_error) => {
                        tracing::error!("Git file delete failed: {}", join_error);
                        panel.action_error = Some("file delete task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Probes repository trust without changing anything; the banner shows
    /// only while the state is `TrustRequired`.
    fn probe_trust(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let Some(git) = handle.git() else {
            return;
        };
        let task = handle
            .tokio_handle()
            .spawn(async move { git.trust_report().await });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.trust = match outcome {
                    Ok(Ok(snapshot)) => {
                        (snapshot.state == GitTrustState::TrustRequired).then_some(snapshot)
                    }
                    _ => None,
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Applies the explicitly confirmed trust decision; on success the panel
    /// reloads, on a stubborn state the banner updates in place.
    fn grant_trust(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let Some(git) = handle.git() else {
            return;
        };
        self.busy = true;
        cx.notify();
        let task = handle
            .tokio_handle()
            .spawn(async move { git.trust_repository().await });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(snapshot)) => {
                        panel.trust =
                            (snapshot.state == GitTrustState::TrustRequired).then_some(snapshot);
                        if panel.trust.is_none() {
                            panel.action_error = None;
                            panel.error = None;
                            panel.refresh(cx);
                        }
                    }
                    Ok(Err(error)) => panel.handle_port_failure(&error, cx),
                    Err(join_error) => {
                        tracing::error!("Trust task failed: {}", join_error);
                        panel.action_error = Some("trust task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn dismiss_action_error(&mut self, cx: &mut Context<Self>) {
        self.action_error = None;
        cx.notify();
    }

    fn dismiss_trust_banner(&mut self, cx: &mut Context<Self>) {
        self.trust = None;
        cx.notify();
    }

    pub(crate) fn set_view(&mut self, view: GitView, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        self.view = view;
        match view {
            GitView::History if !self.history_loaded => self.load_history(cx),
            GitView::Worktrees if !self.worktrees_loaded => self.load_worktrees(cx),
            GitView::Snapshots if !self.snapshots_loaded => self.load_snapshots(cx),
            GitView::Branches if !self.branches_loaded => self.load_branches(cx),
            GitView::Graph if !self.graph_loaded => self.load_graph(cx),
            _ => {}
        }
        cx.notify();
    }

    /// Reloads the view the panel currently shows (the header refresh button
    /// dispatches here): diff views refresh the workspace snapshot, the M19
    /// views reload their own lists.
    fn refresh_current_view(&mut self, cx: &mut Context<Self>) {
        match self.view {
            GitView::Files | GitView::History => self.refresh(cx),
            GitView::Worktrees => self.load_worktrees(cx),
            GitView::Snapshots => self.load_snapshots(cx),
            GitView::Branches => self.load_branches(cx),
            GitView::Graph => self.load_graph(cx),
        }
    }

    /// Active-workspace facts the M19 service calls need. Fails loudly for a
    /// remote workspace (no local runtime behind it) instead of inventing a
    /// local path.
    fn service_workspace() -> Result<(String, Option<String>), String> {
        let handle = crate::runtime::runtime_handle().ok_or("runtime not ready")?;
        let workspace = handle.workspace();
        if matches!(workspace.workspace_kind, WorkspaceKind::Remote) {
            return Err("remote workspace unsupported".to_string());
        }
        // A session executing inside a linked worktree still manages the
        // worktrees of the project checkout that owns the registry.
        let project_id = workspace.project_workspace_id().ok().map(str::to_string);
        Ok((
            workspace.root_path.to_string_lossy().to_string(),
            project_id,
        ))
    }

    // -----------------------------------------------------------------------
    // G-G2: AI commit-message generation
    // -----------------------------------------------------------------------

    /// Requests one quick AI commit message over the same core
    /// `GitFunctionAgent` the desktop command serves. Local-only: the agent
    /// reads the repository from the workspace path, which remote workspaces
    /// do not offer.
    fn generate_commit_message(&mut self, cx: &mut Context<Self>) {
        if self.files.is_empty() {
            self.action_error = Some(i18n::git_generate_no_changes(cx));
            cx.notify();
            return;
        }
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                cx.notify();
                return;
            }
        };
        self.generation_epoch += 1;
        let epoch = self.generation_epoch;
        self.generating_commit = true;
        self.action_error = None;
        cx.notify();
        let task = handle.tokio_handle().spawn(async move {
            let factory =
                openbitfun_core::infrastructure::ai::client_factory::get_global_ai_client_factory()
                    .await
                    .map_err(|error| error.to_string())?;
            let agent =
                openbitfun_core::function_agents::git_func_agent::GitFunctionAgent::new(factory);
            let message = agent
                .quick_commit_message(Path::new(&repo_path))
                .await
                .map_err(|error| error.to_string())?;
            Ok::<String, String>(message.full_message)
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await.unwrap_or_else(|error| Err(error.to_string()));
            this.update(cx, |panel, cx| {
                // A cancelled or restarted generation keeps its epoch; the
                // stale completion is dropped instead of painted.
                if panel.generation_epoch != epoch {
                    return;
                }
                panel.generating_commit = false;
                match outcome {
                    Ok(message) => panel.pending_commit_message = Some(message),
                    Err(error) => {
                        tracing::warn!("AI commit message failed: {error}");
                        panel.action_error = Some(i18n::git_failed(&error, cx));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Drops an in-flight generation: a later completion is recognized by its
    /// epoch and discarded (the web `cancelledCommitRef` semantics).
    fn cancel_commit_generation(&mut self, cx: &mut Context<Self>) {
        self.generation_epoch += 1;
        self.generating_commit = false;
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // G-G4: branches view + commit graph
    // -----------------------------------------------------------------------

    fn load_branches(&mut self, cx: &mut Context<Self>) {
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                self.branches_loaded = true;
                cx.notify();
                return;
            }
        };
        self.confirm_branch_delete = None;
        cx.notify();
        let task = crate::runtime::runtime_handle()
            .map(|handle| handle.tokio_handle())
            .map(|tokio| {
                tokio.spawn(async move {
                    openbitfun_core::service::git::GitService::get_enhanced_branches(
                        Path::new(&repo_path),
                        true,
                    )
                    .await
                })
            });
        let Some(task) = task else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.branches_loaded = true;
                match outcome {
                    Ok(Ok(branches)) => {
                        panel.action_error = None;
                        panel.branch_list.reset(branches.len());
                        panel.branches = branches;
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git branches failed: {error}");
                        panel.action_error = Some(error.to_string());
                    }
                    Err(join_error) => {
                        tracing::error!("Git branches task failed: {join_error}");
                        panel.action_error = Some("git branches task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_graph(&mut self, cx: &mut Context<Self>) {
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                self.graph_loaded = true;
                cx.notify();
                return;
            }
        };
        self.confirm_graph_reset = None;
        cx.notify();
        let task = crate::runtime::runtime_handle()
            .map(|handle| handle.tokio_handle())
            .map(|tokio| {
                tokio.spawn(async move {
                    openbitfun_core::service::git::GitService::get_git_graph_for_branch(
                        Path::new(&repo_path),
                        Some(GRAPH_LIMIT),
                        None,
                    )
                    .await
                })
            });
        let Some(task) = task else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.graph_loaded = true;
                match outcome {
                    Ok(Ok(graph)) => {
                        panel.action_error = None;
                        panel.graph_list.reset(graph.nodes.len());
                        panel.graph = Some(graph);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git graph failed: {error}");
                        panel.action_error = Some(error.to_string());
                    }
                    Err(join_error) => {
                        tracing::error!("Git graph task failed: {join_error}");
                        panel.action_error = Some("git graph task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Creates the branch named in the inputs (start point optional); the
    /// inputs clear on success in the next paint.
    fn create_branch(&mut self, cx: &mut Context<Self>) {
        let name = self.new_branch_input.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        let start_point = self.branch_base_input.read(cx).value().trim().to_string();
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                cx.notify();
                return;
            }
        };
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(async move {
            openbitfun_core::service::git::GitService::create_branch(
                Path::new(&repo_path),
                &name,
                Some(start_point.as_str()),
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(_)) => {
                        panel.action_error = None;
                        panel.clear_branch_inputs = true;
                        panel.load_branches(cx);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git branch create failed: {error}");
                        panel.action_error = Some(error.to_string());
                    }
                    Err(join_error) => {
                        tracing::error!("Git branch create task failed: {join_error}");
                        panel.action_error = Some("git branch create task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Two-step branch delete: the first click arms the row, the second runs.
    fn delete_branch(&mut self, name: String, cx: &mut Context<Self>) {
        if self.confirm_branch_delete.as_deref() != Some(name.as_str()) {
            self.confirm_branch_delete = Some(name);
            cx.notify();
            return;
        }
        self.confirm_branch_delete = None;
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                cx.notify();
                return;
            }
        };
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(async move {
            openbitfun_core::service::git::GitService::delete_branch(
                Path::new(&repo_path),
                &name,
                false,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(_)) => {
                        panel.action_error = None;
                        panel.load_branches(cx);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git branch delete failed: {error}");
                        panel.action_error = Some(error.to_string());
                    }
                    Err(join_error) => {
                        tracing::error!("Git branch delete task failed: {join_error}");
                        panel.action_error = Some("git branch delete task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Resets the current branch to the confirmed graph node (mixed: the
    /// working tree keeps its changes, the index aligns with the target).
    fn reset_to_commit(&mut self, hash: String, cx: &mut Context<Self>) {
        if self.confirm_graph_reset.as_deref() != Some(hash.as_str()) {
            self.confirm_graph_reset = Some(hash);
            cx.notify();
            return;
        }
        self.confirm_graph_reset = None;
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                cx.notify();
                return;
            }
        };
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(async move {
            openbitfun_core::service::git::GitService::reset_to_commit(
                Path::new(&repo_path),
                &hash,
                "mixed",
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(_)) => {
                        panel.action_error = None;
                        panel.load_graph(cx);
                        panel.refresh(cx);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git reset failed: {error}");
                        panel.action_error = Some(error.to_string());
                    }
                    Err(join_error) => {
                        tracing::error!("Git reset task failed: {join_error}");
                        panel.action_error = Some("git reset task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Checks out one local branch; the refresh brings the working tree and
    /// the ahead/behind counts back in line with the switch.
    fn switch_branch(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let repo_path = match Self::service_workspace() {
            Ok((path, _)) => path,
            Err(message) => {
                self.action_error = Some(message);
                cx.notify();
                return;
            }
        };
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(async move {
            openbitfun_core::service::git::GitService::checkout_branch(Path::new(&repo_path), &name)
                .await
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(_)) => {
                        panel.action_error = None;
                        panel.load_branches(cx);
                        panel.refresh(cx);
                    }
                    Ok(Err(error)) => {
                        tracing::warn!("Git branch switch failed: {error}");
                        panel.action_error = Some(error.to_string());
                    }
                    Err(join_error) => {
                        tracing::error!("Git branch switch task failed: {join_error}");
                        panel.action_error = Some("git branch switch task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_worktrees(&mut self, cx: &mut Context<Self>) {
        let (active_path, project_id) = match Self::service_workspace() {
            Ok(facts) => facts,
            Err(message) => {
                self.action_error = Some(message);
                self.worktrees_loaded = true;
                cx.notify();
                return;
            }
        };
        self.busy = true;
        cx.notify();
        let task = crate::runtime::runtime_handle()
            .expect("runtime handle checked by service_workspace")
            .tokio_handle()
            .spawn(async move {
                let project_path = WorktreeService::resolve_project_workspace_path(&active_path)
                    .await
                    .map_err(|error| error.to_string())?;
                WorktreeService::list(WorktreeListRequest {
                    project_workspace_id: project_id,
                    project_workspace_path: project_path,
                })
                .await
                .map_err(|error| error.to_string())
            });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                panel.worktrees_loaded = true;
                match outcome {
                    Ok(Ok(worktrees)) => {
                        panel.action_error = None;
                        panel.worktrees = worktrees;
                        panel.worktree_list.reset(panel.worktrees.len());
                    }
                    Ok(Err(message)) => {
                        tracing::warn!("Worktree list failed: {message}");
                        panel.action_error = Some(message);
                    }
                    Err(join_error) => {
                        tracing::error!("Worktree list task failed: {join_error}");
                        panel.action_error = Some("worktree list task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn create_worktree(&mut self, cx: &mut Context<Self>) {
        let branch = self
            .worktree_branch_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let base_ref = self.worktree_base_input.read(cx).value().trim().to_string();
        if self.busy || (branch.is_empty() && base_ref.is_empty()) {
            return;
        }
        let Ok((active_path, project_id)) = Self::service_workspace() else {
            return;
        };
        let request_id = format!("gpui-shell-worktree-{}", uuid::Uuid::new_v4());
        let branch_request_id = format!("gpui-shell-worktree-br-{}", uuid::Uuid::new_v4());
        // The inputs are captured up front; the branch box clears on success
        // in the next paint (set_value needs the window, which only render
        // holds). A named branch is created for the worktree right after the
        // (detached) worktree exists; the base ref selects the cut point.
        self.run_service(
            move || async move {
                let project_path = WorktreeService::resolve_project_workspace_path(&active_path)
                    .await
                    .map_err(|error| error.to_string())?;
                let created = WorktreeService::create(WorktreeCreateRequest {
                    request_id,
                    project_workspace_id: project_id,
                    project_workspace_path: project_path,
                    source_workspace_path: None,
                    base_ref: (!base_ref.is_empty()).then_some(base_ref),
                    copy_local_changes: false,
                    claimed_by: None,
                })
                .await
                .map_err(|error| error.to_string())?;
                if !branch.is_empty() {
                    WorktreeService::create_branch(WorktreeCreateBranchRequest {
                        request_id: branch_request_id,
                        project_workspace_id: None,
                        project_workspace_path: created.worktree.project_workspace_path,
                        worktree_id: created.worktree.worktree_id,
                        branch,
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
            |panel, _| panel.clear_worktree_input = true,
            cx,
        );
    }

    fn remove_worktree(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(worktree_id) = self.worktrees.get(index).map(|row| row.worktree_id.clone()) else {
            return;
        };
        let Ok((active_path, project_id)) = Self::service_workspace() else {
            return;
        };
        let request_id = format!("gpui-shell-worktree-rm-{}", uuid::Uuid::new_v4());
        // Non-force removal: a dirty or locked worktree fails loudly with the
        // git reason instead of destroying local work behind one click.
        self.run_service(
            move || async move {
                let project_path = WorktreeService::resolve_project_workspace_path(&active_path)
                    .await
                    .map_err(|error| error.to_string())?;
                WorktreeService::remove(WorktreeRemoveRequest {
                    request_id,
                    project_workspace_id: project_id,
                    project_workspace_path: project_path,
                    worktree_id,
                    force: false,
                })
                .await
                .map_err(|error| error.to_string())?;
                Ok(())
            },
            |panel, cx| panel.load_worktrees(cx),
            cx,
        );
    }

    fn promote_worktree(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(worktree_id) = self.worktrees.get(index).map(|row| row.worktree_id.clone()) else {
            return;
        };
        let Ok((active_path, project_id)) = Self::service_workspace() else {
            return;
        };
        let request_id = format!("gpui-shell-worktree-keep-{}", uuid::Uuid::new_v4());
        self.run_service(
            move || async move {
                let project_path = WorktreeService::resolve_project_workspace_path(&active_path)
                    .await
                    .map_err(|error| error.to_string())?;
                WorktreeService::promote(WorktreePromoteRequest {
                    request_id,
                    project_workspace_id: project_id,
                    project_workspace_path: project_path,
                    worktree_id,
                })
                .await
                .map_err(|error| error.to_string())?;
                Ok(())
            },
            |panel, cx| panel.load_worktrees(cx),
            cx,
        );
    }

    /// Moves the chat's open session into a managed worktree (isolation) or
    /// back to the project checkout. The runtime keeps executing the session;
    /// only its execution root changes.
    fn toggle_session_isolation(&mut self, enable: bool, cx: &mut Context<Self>) {
        let Some(session_id) = self.active_session.clone() else {
            return;
        };
        let Ok((active_path, project_id)) = Self::service_workspace() else {
            return;
        };
        let request_id = format!("gpui-shell-worktree-bind-{}", uuid::Uuid::new_v4());
        self.run_service(
            move || async move {
                let project_path = WorktreeService::resolve_project_workspace_path(&active_path)
                    .await
                    .map_err(|error| error.to_string())?;
                WorktreeService::bind_session(WorktreeSessionBindingRequest {
                    request_id,
                    session_id,
                    project_workspace_id: project_id,
                    project_workspace_path: Some(project_path),
                    enabled: enable,
                })
                .await
                .map_err(|error| error.to_string())?;
                Ok(())
            },
            |panel, cx| panel.load_worktrees(cx),
            cx,
        );
    }

    fn load_snapshots(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        if Self::service_workspace().is_err() {
            self.action_error = Some("remote workspace unsupported".to_string());
            self.snapshots_loaded = true;
            cx.notify();
            return;
        }
        let workspace_id = handle.workspace().id;
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(async move {
            let manager = open_snapshot_manager_for_view(&workspace_id)
                .await
                .map_err(|error| error.to_string())?;
            let sessions = manager
                .list_sessions()
                .await
                .map_err(|error| error.to_string())?;
            let mut rows = Vec::with_capacity(sessions.len());
            for session_id in sessions {
                let stats = manager
                    .get_session_stats(&session_id)
                    .await
                    .unwrap_or(serde_json::Value::Null);
                rows.push(snapshot_row_from_stats(&session_id, &stats));
            }
            Ok(rows)
        });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                panel.snapshots_loaded = true;
                match outcome {
                    Ok(Ok(rows)) => {
                        panel.action_error = None;
                        panel.snapshots = rows;
                        panel.snapshot_list.reset(panel.snapshots.len());
                    }
                    Ok(Err(message)) => {
                        tracing::warn!("Snapshot list failed: {message}");
                        panel.action_error = Some(message);
                    }
                    Err(join_error) => {
                        tracing::error!("Snapshot list task failed: {join_error}");
                        panel.action_error = Some("snapshot list task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Restores the workspace files a session changed back to their
    /// pre-session snapshots (the desktop rollback_session semantics: files
    /// only, the transcript keeps its history).
    fn restore_snapshot_files(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(session_id) = self.snapshots.get(index).map(|row| row.session_id.clone()) else {
            return;
        };
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let workspace_id = handle.workspace().id;
        self.run_service(
            move || async move {
                let manager = open_snapshot_manager_for_view(&workspace_id)
                    .await
                    .map_err(|error| error.to_string())?;
                manager
                    .rollback_session(&session_id)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(())
            },
            |panel, cx| panel.load_snapshots(cx),
            cx,
        );
    }

    /// Runs one core-service task for the M19 views: disables the actions,
    /// awaits the result on the runtime thread, then lets the success hook
    /// reload or surfaces the failure.
    fn run_service<F, Fut, Done>(&mut self, op: F, on_success: Done, cx: &mut Context<Self>)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), String>> + Send + 'static,
        Done: FnOnce(&mut Self, &mut Context<Self>) + 'static,
    {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        self.busy = true;
        cx.notify();
        let task = handle.tokio_handle().spawn(op());
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                match outcome {
                    Ok(Ok(())) => {
                        panel.action_error = None;
                        on_success(panel, cx);
                    }
                    Ok(Err(message)) => {
                        tracing::warn!("Service action failed: {message}");
                        panel.action_error = Some(message);
                    }
                    Err(join_error) => {
                        tracing::error!("Service task failed: {join_error}");
                        panel.action_error = Some("service task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_history(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let Some(git) = handle.git() else {
            return;
        };
        self.busy = true;
        cx.notify();
        let task = handle
            .tokio_handle()
            .spawn(async move { git.commit_history(HISTORY_LIMIT).await });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.busy = false;
                panel.history_loaded = true;
                match outcome {
                    Ok(Ok(commits)) => {
                        panel.action_error = None;
                        panel.commits = commits;
                        panel.history_list.reset(panel.commits.len());
                    }
                    Ok(Err(error)) => panel.handle_port_failure(&error, cx),
                    Err(join_error) => {
                        tracing::error!("History task failed: {}", join_error);
                        panel.action_error = Some("history task failed".to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_snapshot(&mut self, snapshot: WorkspaceDiffSnapshot, cx: &mut Context<Self>) {
        tracing::info!(
            "Workspace diff refreshed: {} changed files{}",
            snapshot.files.len(),
            if snapshot.truncated {
                " (truncated)"
            } else {
                ""
            }
        );
        self.files = snapshot.files;
        self.truncated = snapshot.truncated;
        if self.selected >= self.files.len() {
            self.selected = 0;
        }
        self.file_list.reset(self.files.len());
        self.rebuild_diff(cx);
    }

    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.files.len() || index == self.selected {
            return;
        }
        self.selected = index;
        self.rebuild_diff(cx);
        cx.notify();
    }

    /// Flips the diff between wrapped rows and one-line rows with horizontal
    /// scrolling. Row heights change wholesale, so the list remeasures from
    /// scratch.
    fn toggle_wrap(&mut self, cx: &mut Context<Self>) {
        self.wrap_lines = !self.wrap_lines;
        self.diff_list.reset(self.diff.len());
        cx.notify();
    }

    /// Parses the selected file's patch into display rows. Binary and
    /// oversized entries carry no patch by contract; they render a placeholder.
    fn rebuild_diff(&mut self, _cx: &mut Context<Self>) {
        let lines = match self.files.get(self.selected) {
            Some(file) => match &file.content {
                WorkspaceDiffContent::Text { patch } => parse_unified_patch(patch),
                WorkspaceDiffContent::Binary | WorkspaceDiffContent::TooLarge => Vec::new(),
            },
            None => Vec::new(),
        };
        self.diff = Rc::new(lines);
        self.diff_list.reset(self.diff.len());
    }

    fn totals(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(adds, dels), file| {
            (adds + file.additions, dels + file.deletions)
        })
    }
}

/// Header: title, workspace name, totals, branch chip, view toggle, and the
/// write actions (stage all, push, refresh).
fn render_header(panel: &GitPanel, text: &ShellText, cx: &mut Context<GitPanel>) -> gpui_kit::Div {
    let workspace = crate::runtime::runtime_handle()
        .map(|handle| handle.workspace().name.clone())
        .unwrap_or_default();
    let (additions, deletions) = panel.totals();
    let summary = match panel.view {
        GitView::Worktrees => format!("{}: {}", text.git_worktrees_view, panel.worktrees.len()),
        GitView::Snapshots => format!("{}: {}", text.git_snapshots_view, panel.snapshots.len()),
        GitView::Branches => format!("{}: {}", text.git_branches_view, panel.branches.len()),
        GitView::Graph => format!(
            "{}: {}",
            text.git_graph_view,
            panel
                .graph
                .as_ref()
                .map(|graph| graph.nodes.len())
                .unwrap_or(0)
        ),
        GitView::Files | GitView::History => match (panel.loading, panel.files.is_empty()) {
            (true, _) => text.git_loading.clone(),
            (false, true) => text.git_empty.clone(),
            (false, false) => format!(
                "{}: {} · +{} -{}",
                text.git_title,
                panel.files.len(),
                additions,
                deletions
            ),
        },
    };
    let active_variant = |view: GitView| {
        if panel.view == view {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        }
    };
    // Without an overview yet the push stays enabled and fails loudly; with
    // one, a remote-less repository disables it instead. Pull shares the
    // stance: both need a remote (G-G6).
    let push_enabled = !panel.busy
        && panel
            .overview
            .as_ref()
            .map(|overview| !overview.remotes.is_empty())
            .unwrap_or(true);
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_4()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().title_bar_border)
        .child(
            div()
                .v_flex()
                .gap_0p5()
                // Take the shrinkable share: without this the view-switch
                // buttons squeeze the titles into a one-glyph column in the
                // docked panel (R2c).
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .truncate()
                        .child(format!("{} · {}", text.git_title, workspace)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(summary),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .flex_shrink_0()
                .when_some(panel.overview.as_ref(), |this, overview| {
                    let branch = overview.current_branch.clone();
                    let ahead = overview.ahead;
                    let behind = overview.behind;
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().sidebar_accent)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .max_w(px(200.))
                            .child(div().truncate().child(branch))
                            .when(ahead > 0, |this| {
                                let label = i18n::git_ahead(ahead, cx);
                                this.child(
                                    div()
                                        .id("git-ahead-badge")
                                        .flex_shrink_0()
                                        .text_color(cx.theme().success)
                                        .child(format!("↑{ahead}"))
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(
                                                label.clone(),
                                            )
                                            .build(window, cx)
                                        }),
                                )
                            })
                            .when(behind > 0, |this| {
                                let label = i18n::git_behind(behind, cx);
                                this.child(
                                    div()
                                        .id("git-behind-badge")
                                        .flex_shrink_0()
                                        .text_color(cx.theme().warning)
                                        .child(format!("↓{behind}"))
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(
                                                label.clone(),
                                            )
                                            .build(window, cx)
                                        }),
                                )
                            }),
                    )
                })
                .child(
                    Button::new("git-view-files")
                        .label(text.git_files_view.clone())
                        .small()
                        .with_variant(active_variant(GitView::Files))
                        .tooltip(text.git_files_view.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_view(GitView::Files, cx);
                        })),
                )
                .child(
                    Button::new("git-view-history")
                        .label(text.git_history.clone())
                        .small()
                        .with_variant(active_variant(GitView::History))
                        .tooltip(text.git_history.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_view(GitView::History, cx);
                        })),
                )
                .child(
                    Button::new("git-view-worktrees")
                        .label(text.git_worktrees_view.clone())
                        .small()
                        .with_variant(active_variant(GitView::Worktrees))
                        .tooltip(text.git_worktrees_view.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_view(GitView::Worktrees, cx);
                        })),
                )
                .child(
                    Button::new("git-view-snapshots")
                        .label(text.git_snapshots_view.clone())
                        .small()
                        .with_variant(active_variant(GitView::Snapshots))
                        .tooltip(text.git_snapshots_view.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_view(GitView::Snapshots, cx);
                        })),
                )
                .child(
                    Button::new("git-view-branches")
                        .label(text.git_branches_view.clone())
                        .small()
                        .with_variant(active_variant(GitView::Branches))
                        .tooltip(text.git_branches_view.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_view(GitView::Branches, cx);
                        })),
                )
                .child(
                    Button::new("git-view-graph")
                        .label(text.git_graph_view.clone())
                        .small()
                        .with_variant(active_variant(GitView::Graph))
                        .tooltip(text.git_graph_view.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_view(GitView::Graph, cx);
                        })),
                )
                // Stage-all, pull and push address the workspace diff; they
                // hide outside the files view where they have no meaning.
                .when(panel.view == GitView::Files, |this| {
                    this.child(
                        Button::new("git-stage-all")
                            .icon(IconName::ListTodo)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.git_stage_all.clone())
                            .disabled(panel.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.stage_all(cx))),
                    )
                    .child(
                        Button::new("git-pull")
                            .icon(IconName::ArrowDown)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.git_pull.clone())
                            .disabled(!push_enabled)
                            .on_click(cx.listener(|this, _, _, cx| this.pull(cx))),
                    )
                    .child(
                        Button::new("git-push")
                            .icon(IconName::Upload)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .tooltip(text.git_push.clone())
                            .disabled(!push_enabled)
                            .on_click(cx.listener(|this, _, _, cx| this.push(cx))),
                    )
                })
                .child(
                    Button::new("git-refresh")
                        .icon(IconName::RefreshCw)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_refresh.clone())
                        .disabled(panel.loading || panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_current_view(cx))),
                ),
        )
}

/// The not-a-repository card (G-G1): shown instead of the changes body when
/// the port reports the workspace is no repository at all. The button does
/// not run `git init` itself — it hands the prompt to the chat composer,
/// matching the desktop scene, where the agent performs the initialization
/// with the user watching.
fn render_not_a_repository(text: &ShellText, cx: &mut Context<GitPanel>) -> gpui_kit::AnyElement {
    div()
        .debug_selector(|| "git-not-a-repository".to_string())
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .child(
            div()
                .v_flex()
                .items_center()
                .gap_1()
                .child(Icon::new(IconName::GitBranch).text_color(cx.theme().muted_foreground))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(text.git_not_repo_title.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.git_not_repo_detail.clone()),
                ),
        )
        .child(
            Button::new("git-init-repository")
                .icon(IconName::Plus)
                .label(text.git_init_button.clone())
                .small()
                .with_variant(ButtonVariant::Primary)
                .on_click(cx.listener(|this, _, _, cx| this.request_git_init(cx))),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text.git_init_hint.clone()),
        )
        .into_any_element()
}

/// The untrusted-repository banner: the port refused on ownership grounds,
/// so the surface offers the explicit trust decision instead of a bare error.
fn render_trust_banner(
    trust: &GitTrustSnapshot,
    busy: bool,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::Div {
    let detail = trust.detail.clone().unwrap_or_default();
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_1p5()
        .border_b_1()
        .border_color(cx.theme().title_bar_border)
        .bg(cx.theme().warning.opacity(0.12))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(text.git_untrusted_title.clone()),
                )
                .when(!detail.is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(detail),
                    )
                }),
        )
        .child(
            Button::new("git-trust-confirm")
                .label(text.git_untrusted_action.clone())
                .small()
                .with_variant(ButtonVariant::Primary)
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.grant_trust(cx))),
        )
        .child(
            Button::new("git-trust-dismiss")
                .icon(IconName::X)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_trust_banner(cx))),
        )
}

/// The last failed action's message, dismissible; a following success or the
/// trust banner supersedes it.
fn render_action_error(error: &str, cx: &mut Context<GitPanel>) -> gpui_kit::Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_4()
        .py_1p5()
        .border_b_1()
        .border_color(cx.theme().title_bar_border)
        .bg(cx.theme().danger.opacity(0.12))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().danger)
                .truncate()
                .child(error.to_string()),
        )
        .child(
            Button::new("git-action-error-dismiss")
                .icon(IconName::X)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_action_error(cx))),
        )
}

/// One changed-file row: status letter, path, line counts, the staging action
/// (stage when worktree content exists, unstage when the changes are fully
/// staged), and the destructive action the row's status allows — discard for
/// tracked paths, delete for untracked files (G-G3). The destructive action
/// is two-step: the first click arms it (button turns into a labelled danger
/// button), the second runs it.
fn render_file_row(
    index: usize,
    file: &WorkspaceDiffFile,
    selected: bool,
    busy: bool,
    armed: Option<FileRowAction>,
    remote_workspace: bool,
    panel: &Entity<GitPanel>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let status_color = match file.status {
        WorkspaceDiffFileStatus::Added => cx.theme().success,
        WorkspaceDiffFileStatus::Modified => cx.theme().warning,
        WorkspaceDiffFileStatus::Deleted | WorkspaceDiffFileStatus::Conflicted => cx.theme().danger,
        WorkspaceDiffFileStatus::Renamed => cx.theme().info,
    };
    let markers: Vec<&str> = stage_markers(file)
        .into_iter()
        .map(|marker| marker.label(text))
        .collect();
    let action = row_stage_action(file);
    let (action_icon, action_label) = match action {
        StageAction::Stage => (IconName::Plus, text.git_stage.clone()),
        StageAction::Unstage => (IconName::Minus, text.git_unstage.clone()),
    };
    let action_panel = panel.clone();
    // Untracked files have no git-side restore: their destructive action is a
    // filesystem delete, which cannot reach a remote workspace.
    let row_action = if file.untracked {
        FileRowAction::Delete(index)
    } else {
        FileRowAction::Discard(index)
    };
    let (row_icon, row_tooltip, row_confirm) = match row_action {
        FileRowAction::Discard(_) => (
            IconName::Undo2,
            text.git_discard.clone(),
            text.git_discard_confirm.clone(),
        ),
        FileRowAction::Delete(_) => (
            IconName::Trash,
            text.git_delete_file.clone(),
            text.git_delete_confirm.clone(),
        ),
    };
    let row_armed = armed == Some(row_action);
    let row_disabled = busy || (remote_workspace && matches!(row_action, FileRowAction::Delete(_)));
    let row_panel = panel.clone();
    div()
        .id(gpui_kit::SharedString::from(format!("git-file-{}", index)))
        .cursor_pointer()
        .v_flex()
        .gap_0p5()
        .min_w_0()
        .p_2()
        .rounded(cx.theme().radius)
        .when(selected, |this| this.bg(cx.theme().sidebar_accent))
        .when(!selected, |this| {
            this.hover(|style| style.bg(cx.theme().sidebar_accent))
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_letter(file.status)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(cx.theme().sidebar_foreground)
                        .truncate()
                        .child(file.path.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().success)
                        .child(format!("+{}", file.additions)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(format!("-{}", file.deletions)),
                )
                .child(
                    Button::new(gpui_kit::SharedString::from(format!(
                        "git-stage-action-{}",
                        index
                    )))
                    .icon(action_icon)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(action_label)
                    .disabled(busy)
                    .on_click(move |_, _, cx| {
                        action_panel.update(cx, |panel, cx| match action {
                            StageAction::Stage => panel.stage_file(index, cx),
                            StageAction::Unstage => panel.unstage_file(index, cx),
                        });
                    }),
                )
                .child({
                    let mut button = Button::new(gpui_kit::SharedString::from(format!(
                        "git-row-action-{}",
                        index
                    )))
                    .small()
                    .with_variant(if row_armed {
                        ButtonVariant::Danger
                    } else {
                        ButtonVariant::Ghost
                    })
                    // The confirm click must not bubble into the row's
                    // selection handler (the component library does not stop
                    // propagation on its own).
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        row_panel.update(cx, |panel, cx| panel.row_action(row_action, cx));
                    });
                    button = if row_armed {
                        button.label(row_confirm).tooltip(row_tooltip)
                    } else {
                        button.icon(row_icon).tooltip(row_tooltip)
                    };
                    if row_disabled {
                        button = button.disabled(true);
                    }
                    button
                }),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(markers.join(" · ")),
        )
        .into_any_element()
}

/// One diff row. Additions/deletions carry a faint tinted background so the
/// diff reads without a gutter; colors come from the active theme. Wrapped
/// rows flow onto as many visual lines as the text needs; unwrapped rows stay
/// on one line and the surrounding viewport scrolls sideways.
fn render_diff_line(line: &DiffLine, wrap: bool, cx: &App) -> gpui_kit::AnyElement {
    let color = match line.kind {
        DiffLineKind::Meta => cx.theme().muted_foreground,
        DiffLineKind::Hunk => cx.theme().info,
        DiffLineKind::Add => cx.theme().success,
        DiffLineKind::Delete => cx.theme().danger,
        DiffLineKind::Context => cx.theme().foreground,
    };
    div()
        .w_full()
        .px_2()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .text_color(color)
        .when(line.kind == DiffLineKind::Add, |this| {
            this.bg(cx.theme().success.opacity(0.08))
        })
        .when(line.kind == DiffLineKind::Delete, |this| {
            this.bg(cx.theme().danger.opacity(0.08))
        })
        .when(!wrap, |this| this.whitespace_nowrap())
        .child(line.text.clone())
        .into_any_element()
}

/// Natural pixel width of the widest diff row at the row typography. The
/// unwrapped layout lays the rows out at this width and lets the viewport
/// scroll horizontally, so nothing is cut off. Shaping a single line goes
/// through the text system's layout cache.
fn diff_content_width(lines: &[DiffLine], window: &mut Window, cx: &App) -> Pixels {
    let Some(widest) = widest_line(lines) else {
        return px(0.);
    };
    if widest.text.is_empty() {
        return px(DIFF_ROW_PADDING);
    }
    let shaped = window.text_system().shape_line(
        SharedString::from(widest.text.clone()),
        px(DIFF_FONT_SIZE),
        &[TextRun {
            len: widest.text.len(),
            font: gpui_kit::font(cx.theme().mono_font_family.clone()),
            color: cx.theme().foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );
    shaped.width() + px(DIFF_ROW_PADDING)
}

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A successful commit clears its message box here: render is the only
        // place holding the window that InputState::set_value requires. A
        // created worktree clears its branch name the same way.
        if self.clear_commit_input {
            self.clear_commit_input = false;
            cx.update_entity(&self.commit_input, |input, cx| {
                input.set_value("", window, cx);
            });
        }
        if self.clear_worktree_input {
            self.clear_worktree_input = false;
            cx.update_entity(&self.worktree_branch_input, |input, cx| {
                input.set_value("", window, cx);
            });
            cx.update_entity(&self.worktree_base_input, |input, cx| {
                input.set_value("", window, cx);
            });
        }
        if self.clear_branch_inputs {
            self.clear_branch_inputs = false;
            cx.update_entity(&self.new_branch_input, |input, cx| {
                input.set_value("", window, cx);
            });
            cx.update_entity(&self.branch_base_input, |input, cx| {
                input.set_value("", window, cx);
            });
        }
        if let Some(message) = self.pending_commit_message.take() {
            cx.update_entity(&self.commit_input, |input, cx| {
                input.set_value(message, window, cx);
            });
        }
        let text = i18n::current(cx);
        let view = self.view;
        let busy = self.busy;
        let trust = self.trust.clone();
        let action_error = self.action_error.clone();
        let selected_file = self.files.get(self.selected).cloned();
        let diff = self.diff.clone();
        let error = self.error.clone();
        let truncated = self.truncated;
        // Unwrapped rows need their natural width before the list is built; the
        // measurement only matters in that mode.
        let diff_width = if self.wrap_lines {
            Pixels::ZERO
        } else {
            diff_content_width(&diff, window, cx)
        };

        let file_list = {
            let this = cx.entity();
            list(self.file_list.clone(), move |index, _window, cx| {
                let panel = this.read(cx);
                let Some(file) = panel.files.get(index) else {
                    return div().into_any_element();
                };
                let text = i18n::current(cx);
                let row = render_file_row(
                    index,
                    file,
                    index == panel.selected,
                    panel.busy,
                    panel.confirm_action,
                    panel.remote_workspace,
                    &this,
                    &text,
                    cx,
                );
                let this = this.clone();
                div()
                    .id(gpui_kit::SharedString::from(format!(
                        "git-file-click-{}",
                        index
                    )))
                    .on_click(move |_, _, cx| {
                        this.update(cx, |panel, cx| panel.select(index, cx));
                    })
                    .child(row)
                    .into_any_element()
            })
        };

        // The diff header counts belong to the selected file; the panel header
        // carries the workspace totals.
        let (additions, deletions) = selected_file
            .as_ref()
            .map(|file| (file.additions, file.deletions))
            .unwrap_or((0, 0));
        let wrap_lines = self.wrap_lines;
        let diff_header = selected_file.as_ref().map(|file| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().title_bar_border)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(status_letter(file.status)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .truncate()
                        .child(file.path.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().success)
                        .child(format!("+{}", additions)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(format!("-{}", deletions)),
                )
                // Wrap toggle: on = rows flow at the viewport width, off = one
                // line per row with horizontal scrolling.
                .child(
                    Button::new("git-diff-wrap")
                        .icon(IconName::TextWrap)
                        .small()
                        .with_variant(if wrap_lines {
                            ButtonVariant::Secondary
                        } else {
                            ButtonVariant::Ghost
                        })
                        .tooltip(text.git_diff_wrap.clone())
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_wrap(cx))),
                )
                .into_any_element()
        });

        let files_body = if let Some(error) = error {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px_4()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error)
                .into_any_element()
        } else if self.loading && self.files.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.git_loading.clone())
                .into_any_element()
        } else {
            match selected_file.as_ref().map(|file| &file.content) {
                None => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.git_select.clone())
                    .into_any_element(),
                Some(WorkspaceDiffContent::Binary) => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.git_binary.clone())
                    .into_any_element(),
                Some(WorkspaceDiffContent::TooLarge) => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.git_too_large.clone())
                    .into_any_element(),
                Some(WorkspaceDiffContent::Text { .. }) => {
                    let diff = diff.clone();
                    let wrap_lines = self.wrap_lines;
                    let rows = list(self.diff_list.clone(), move |index, _window, cx| match diff
                        .get(index)
                    {
                        Some(line) => render_diff_line(line, wrap_lines, cx),
                        None => div().into_any_element(),
                    });
                    if wrap_lines {
                        rows.flex_1().min_h_0().into_any_element()
                    } else {
                        // Unwrapped: the rows keep their natural width and the
                        // viewport scrolls sideways instead of cutting them off.
                        div()
                            .id("git-diff-hscroll")
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_x_scroll()
                            .child(
                                div()
                                    .h_full()
                                    .w_full()
                                    .min_w(diff_width)
                                    .child(rows.size_full()),
                            )
                            .into_any_element()
                    }
                }
            }
        };

        let body = if self.not_a_repository {
            render_not_a_repository(&text, cx)
        } else {
            match view {
                GitView::Files if !self.loading && self.files.is_empty() => {
                    crate::shell::brand_empty(
                        IconName::GitBranch,
                        &text.git_empty_title,
                        &text.git_empty,
                    )
                }
                GitView::Files => {
                    let file_column = div()
                        .when(self.compact, |this| this.flex_1().min_w_0())
                        .when(!self.compact, |this| {
                            this.w(px(FILE_COLUMN_WIDTH)).flex_shrink_0()
                        })
                        .h_full()
                        .min_h_0()
                        .v_flex()
                        .when(!self.compact, |this| {
                            this.border_r_1().border_color(cx.theme().border)
                        })
                        .child(file_list.flex_1().min_h_0())
                        .child(render_commit_box(self, &text, cx))
                        .when(truncated, |this| {
                            this.child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .text_xs()
                                    .text_color(cx.theme().warning)
                                    .child(text.git_truncated.clone()),
                            )
                        });
                    // The diff column is a main-tab luxury: the docked panel
                    // shows the file list (selection still works) and the diff
                    // comes back when the panel pops out to a tab. In the main
                    // tab the two columns share one draggable separator (G-G5).
                    if self.compact {
                        file_column.into_any_element()
                    } else {
                        let diff_column = div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .v_flex()
                            .when_some(diff_header, |this, header| this.child(header))
                            .child(files_body);
                        h_resizable("git-files-split")
                            .with_state(&self.file_split)
                            .children([
                                resizable_panel()
                                    .size(px(FILE_COLUMN_WIDTH))
                                    .size_range(
                                        px(MIN_FILE_COLUMN_WIDTH)..px(MAX_FILE_COLUMN_WIDTH),
                                    )
                                    .child(file_column),
                                resizable_panel().child(diff_column),
                            ])
                            .into_any_element()
                    }
                }
                GitView::History => render_history(self, &text, cx),
                GitView::Worktrees => render_worktrees(self, &text, cx),
                GitView::Snapshots => render_snapshots(self, &text, cx),
                GitView::Branches => render_branches(self, &text, cx),
                GitView::Graph => render_graph(self, &text, cx),
            }
        };

        div()
            .size_full()
            .v_flex()
            .bg(cx.theme().background)
            .child(render_header(self, &text, cx))
            .when_some(trust, |this, trust| {
                this.child(render_trust_banner(&trust, busy, &text, cx))
            })
            .when_some(action_error, |this, error| {
                this.child(render_action_error(&error, cx))
            })
            .child(body)
    }
}

/// Commit box docked under the file list: the message input plus the commit
/// button. Committing needs a non-empty message; the busy flag disables the
/// button while any port operation runs.
fn render_commit_box(
    panel: &GitPanel,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::Div {
    let message_empty = panel.commit_input.read(cx).value().trim().is_empty();
    let generating = panel.generating_commit;
    div()
        .w_full()
        .v_flex()
        .gap_1p5()
        .p_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_start()
                .gap_1p5()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&panel.commit_input)),
                )
                .child(if generating {
                    Button::new("git-generate-cancel")
                        .label(text.git_generate_cancel.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_generate_cancel.clone())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_commit_generation(cx);
                        }))
                } else {
                    Button::new("git-generate-message")
                        .label(text.git_generate_message.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_generate_message.clone())
                        .disabled(panel.busy || panel.not_a_repository)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.generate_commit_message(cx);
                        }))
                }),
        )
        .child(
            Button::new("git-commit")
                .label(text.git_commit.clone())
                .small()
                .with_variant(ButtonVariant::Primary)
                .disabled(panel.busy || generating || message_empty)
                .on_click(cx.listener(|this, _, _, cx| this.commit_staged(cx))),
        )
}

/// Centered muted body line shared by the M19 views' empty/unsupported states.
fn render_centered_note(note: &str, cx: &App) -> gpui_kit::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(note.to_string())
        .into_any_element()
}

/// The M19 worktrees body: a session-isolation toggle for the chat's open
/// session, the create form, and one reconciled row per managed worktree.
fn render_worktrees(
    panel: &GitPanel,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::AnyElement {
    if panel.remote_workspace {
        return render_centered_note(&text.git_remote_unsupported, cx);
    }
    // The chat's open session sits inside a managed worktree when any row
    // lists it; the toggle offers the opposite transition.
    let isolated = panel.active_session.as_ref().is_some_and(|session_id| {
        panel
            .worktrees
            .iter()
            .any(|row| row.sessions.iter().any(|s| &s.session_id == session_id))
    });
    let isolation_button = match (&panel.active_session, isolated) {
        (None, _) => Button::new("git-worktree-isolate")
            .icon(IconName::GitBranch)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(text.git_worktree_no_session.clone())
            .disabled(true),
        (Some(_), false) => Button::new("git-worktree-isolate")
            .icon(IconName::GitBranch)
            .label(text.git_worktree_isolate.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .tooltip(text.git_worktree_isolate.clone())
            .disabled(panel.busy)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_session_isolation(true, cx))),
        (Some(_), true) => Button::new("git-worktree-isolate")
            .icon(IconName::GitBranch)
            .label(text.git_worktree_release.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .tooltip(text.git_worktree_release.clone())
            .disabled(panel.busy)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_session_isolation(false, cx))),
    };
    let create_form = div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .w(px(180.))
                .min_w_0()
                .child(Input::new(&panel.worktree_branch_input)),
        )
        .child(
            div()
                .w(px(180.))
                .min_w_0()
                .child(Input::new(&panel.worktree_base_input)),
        )
        .child(
            Button::new("git-worktree-create")
                .icon(IconName::Plus)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.git_worktree_create.clone())
                .disabled(panel.busy)
                .on_click(cx.listener(|this, _, _, cx| this.create_worktree(cx))),
        );
    let body: gpui_kit::AnyElement = if panel.worktrees.is_empty() {
        if panel.busy && !panel.worktrees_loaded {
            render_centered_note(&text.git_loading, cx)
        } else {
            render_centered_note(&text.git_worktree_empty, cx)
        }
    } else {
        let this = cx.entity();
        let rows = list(panel.worktree_list.clone(), move |index, _window, cx| {
            let panel = this.read(cx);
            let Some(summary) = panel.worktrees.get(index) else {
                return div().into_any_element();
            };
            let view = worktree_row_view(summary);
            let busy = panel.busy;
            let text = i18n::current(cx);
            let this = this.clone();
            div()
                .id(gpui_kit::SharedString::from(format!(
                    "git-worktree-{}",
                    index
                )))
                .child(render_worktree_row(index, &view, busy, &this, &text, cx))
                .into_any_element()
        });
        rows.flex_1().min_h_0().into_any_element()
    };
    div()
        .v_flex()
        .flex_1()
        .min_h_0()
        .child(create_form)
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(isolation_button),
        )
        .child(body)
        .into_any_element()
}

/// One worktree row: identity line, fact badges, bound-session count, and the
/// lifecycle actions (keep permanently, remove).
fn render_worktree_row(
    index: usize,
    view: &WorktreeRowView,
    busy: bool,
    panel: &Entity<GitPanel>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let lifecycle_label = match view.lifecycle {
        WorktreeLifecycle::Managed => &text.git_worktree_managed,
        WorktreeLifecycle::Permanent => &text.git_worktree_permanent,
        WorktreeLifecycle::External => &text.git_worktree_external,
    };
    let mut badges = vec![lifecycle_label.as_str()];
    if view.main {
        badges.push(text.git_worktree_main.as_str());
    }
    if view.dirty {
        badges.push(text.git_worktree_dirty.as_str());
    }
    if view.locked {
        badges.push(text.git_worktree_locked.as_str());
    }
    if view.missing {
        badges.push(text.git_worktree_missing.as_str());
    }
    let promote_panel = panel.clone();
    let remove_panel = panel.clone();
    div()
        .v_flex()
        .gap_0p5()
        .min_w_0()
        .p_2()
        .rounded(cx.theme().radius)
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .text_color(cx.theme().sidebar_foreground)
                        .truncate()
                        .child(view.title.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().muted_foreground)
                        .child(view.head_short()),
                )
                .when(view.promotable, |this| {
                    this.child(
                        Button::new(gpui_kit::SharedString::from(format!(
                            "git-worktree-promote-{}",
                            index
                        )))
                        .icon(IconName::Pin)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_worktree_promote.clone())
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            promote_panel.update(cx, |panel, cx| panel.promote_worktree(index, cx));
                        }),
                    )
                })
                .when(view.removable, |this| {
                    this.child(
                        Button::new(gpui_kit::SharedString::from(format!(
                            "git-worktree-remove-{}",
                            index
                        )))
                        .icon(IconName::Trash)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_worktree_remove.clone())
                        .disabled(busy)
                        .on_click(move |_, _, cx| {
                            remove_panel.update(cx, |panel, cx| panel.remove_worktree(index, cx));
                        }),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(badges.join(" · ")),
                )
                .when(view.session_count > 0, |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(i18n::worktree_sessions_label(view.session_count, cx)),
                    )
                }),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(view.subtitle.clone()),
        )
        .into_any_element()
}

/// The M19 snapshots body: one row per session the snapshot system recorded,
/// with its file/turn/change facts and the file-restore action. Turn-level
/// rollback lives in the chat's per-turn menu.
fn render_snapshots(
    panel: &GitPanel,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::AnyElement {
    if panel.remote_workspace {
        return render_centered_note(&text.git_remote_unsupported, cx);
    }
    if panel.snapshots.is_empty() {
        if panel.busy && !panel.snapshots_loaded {
            return render_centered_note(&text.git_loading, cx);
        }
        return render_centered_note(&text.git_snapshot_empty, cx);
    }
    let this = cx.entity();
    let rows = list(panel.snapshot_list.clone(), move |index, _window, cx| {
        let panel = this.read(cx);
        let Some(row) = panel.snapshots.get(index) else {
            return div().into_any_element();
        };
        let session_id = row.session_id.clone();
        let stats =
            i18n::snapshot_stats_label(row.total_files, row.total_turns, row.total_changes, cx);
        let text = i18n::current(cx);
        let busy = panel.busy;
        let restore_panel = this.clone();
        div()
            .id(gpui_kit::SharedString::from(format!(
                "git-snapshot-{}",
                index
            )))
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .p_2()
            .rounded(cx.theme().radius)
            .hover(|style| style.bg(cx.theme().sidebar_accent))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().sidebar_foreground)
                            .truncate()
                            .child(session_id),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(stats),
                    ),
            )
            .child(
                Button::new(gpui_kit::SharedString::from(format!(
                    "git-snapshot-restore-{}",
                    index
                )))
                .icon(IconName::Undo2)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.git_snapshot_restore.clone())
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    restore_panel.update(cx, |panel, cx| panel.restore_snapshot_files(index, cx));
                }),
            )
            .into_any_element()
    });
    rows.flex_1().min_h_0().into_any_element()
}

/// The read-only history body: recent commits of the current branch, newest
/// first. Empty until the first load; the loading state reuses the busy flag.
/// The G-G4 branches view: the enhanced branch list with create/switch/
/// delete/copy-hash over the core `GitService` (local workspaces only — the
/// service reads the repository from the workspace path).
fn render_branches(
    panel: &GitPanel,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::AnyElement {
    if panel.remote_workspace {
        return render_centered_note(&text.git_remote_unsupported, cx);
    }
    // flex_1 + min_h_0: the body gives this element the panel's remaining
    // height, and the row list's own flex_1 needs a bounded parent column.
    let mut column = div().w_full().v_flex().gap_2().p_2().flex_1().min_h_0();
    column = column.child(
        div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&panel.new_branch_input)),
            )
            .child(
                div()
                    .w(px(160.))
                    .flex_shrink_0()
                    .child(Input::new(&panel.branch_base_input)),
            )
            .child(
                Button::new("git-branch-create")
                    .label(text.git_branch_create.clone())
                    .small()
                    .with_variant(ButtonVariant::Secondary)
                    .disabled(
                        panel.busy || panel.new_branch_input.read(cx).value().trim().is_empty(),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.create_branch(cx))),
            ),
    );
    if panel.branches.is_empty() {
        let placeholder = if panel.busy {
            text.git_loading.clone()
        } else {
            text.git_branch_empty.clone()
        };
        column = column.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .py_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(placeholder),
        );
        return column.into_any_element();
    }
    let branches = panel.branches.clone();
    let confirm_delete = panel.confirm_branch_delete.clone();
    let busy = panel.busy;
    let branch_list = panel.branch_list.clone();
    let panel_entity = cx.entity();
    // Labels are cloned up front: referencing `text` inside the `move` item
    // closure would capture the `&ShellText` itself and demand 'static.
    let label_current = text.git_branch_current.clone();
    let label_remote = text.git_branch_remote.clone();
    let label_copy = text.git_branch_copy_hash.clone();
    let label_switch = text.git_branch_switch.clone();
    let label_delete = text.git_branch_delete.clone();
    let label_delete_confirm = text.git_branch_delete_confirm.clone();
    let rows = list(branch_list, move |index, _window, cx| {
        let Some(branch) = branches.get(index) else {
            return div().into_any_element();
        };
        let panel_entity = panel_entity.clone();
        let mut row = div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_sm()
            .when(branch.current, |this| {
                this.bg(cx.theme().accent.opacity(0.12))
            })
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .text_sm()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(branch.name.clone()),
                    )
                    .when(branch.current, |this| {
                        this.child(
                            div()
                                .px_1()
                                .py_0p5()
                                .rounded_sm()
                                .text_xs()
                                .flex_shrink_0()
                                .bg(cx.theme().accent.opacity(0.2))
                                .text_color(cx.theme().accent)
                                .child(label_current.clone()),
                        )
                    })
                    .when(branch.remote, |this| {
                        this.child(
                            div()
                                .px_1()
                                .py_0p5()
                                .rounded_sm()
                                .text_xs()
                                .flex_shrink_0()
                                .text_color(cx.theme().muted_foreground)
                                .child(label_remote.clone()),
                        )
                    }),
            );
        if let Some(upstream) = &branch.upstream {
            let mut fact = format!("↑{upstream}");
            if branch.ahead > 0 {
                fact.push_str(&format!(" +{}", branch.ahead));
            }
            if branch.behind > 0 {
                fact.push_str(&format!(" -{}", branch.behind));
            }
            row = row.child(
                div()
                    .max_w(px(180.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(fact),
            );
        }
        let mut actions = div().flex().items_center().gap_1().flex_shrink_0();
        if branch.last_commit.is_some() {
            let hash = branch.last_commit.clone();
            actions = actions.child(
                Button::new(SharedString::from(format!("git-branch-copy-{index}")))
                    .label(label_copy.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _, cx| {
                        if let Some(hash) = hash.clone() {
                            cx.write_to_clipboard(ClipboardItem::new_string(hash));
                        }
                    }),
            );
        }
        if !branch.current && !branch.remote {
            actions = actions.child(
                Button::new(SharedString::from(format!("git-branch-switch-{index}")))
                    .label(label_switch.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(busy)
                    .on_click({
                        let name = branch.name.clone();
                        let panel_entity = panel_entity.clone();
                        move |_, _, cx| {
                            panel_entity
                                .update(cx, |panel, cx| panel.switch_branch(name.clone(), cx));
                            cx.stop_propagation();
                        }
                    }),
            );
            let armed = confirm_delete.as_deref() == Some(branch.name.as_str());
            actions = actions.child(
                Button::new(SharedString::from(format!("git-branch-delete-{index}")))
                    .label(if armed {
                        label_delete_confirm.clone()
                    } else {
                        label_delete.clone()
                    })
                    .small()
                    .with_variant(if armed {
                        ButtonVariant::Danger
                    } else {
                        ButtonVariant::Ghost
                    })
                    .disabled(busy)
                    .on_click({
                        let name = branch.name.clone();
                        let panel_entity = panel_entity.clone();
                        move |_, _, cx| {
                            cx.stop_propagation();
                            panel_entity
                                .update(cx, |panel, cx| panel.delete_branch(name.clone(), cx));
                        }
                    }),
            );
        }
        row = row.child(div().flex_1()).child(actions);
        row.into_any_element()
    });
    column.child(rows.flex_1().min_h_0()).into_any_element()
}

/// The G-G4 graph view: the lane-allocated commit graph rendered as rows of
/// lane cells plus refs, subject and author, with copy-hash and a two-step
/// reset of the current branch to the node (local workspaces only).
fn render_graph(
    panel: &GitPanel,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::AnyElement {
    if panel.remote_workspace {
        return render_centered_note(&text.git_remote_unsupported, cx);
    }
    let Some(graph) = &panel.graph else {
        let placeholder = if panel.busy {
            text.git_loading.clone()
        } else {
            text.git_no_commits.clone()
        };
        return render_centered_note(&placeholder, cx);
    };
    if graph.nodes.is_empty() {
        return render_centered_note(&text.git_no_commits, cx);
    }
    let columns = (graph.max_lane + 1).max(1) as usize;
    let nodes = graph.nodes.clone();
    let confirm_reset = panel.confirm_graph_reset.clone();
    let busy = panel.busy;
    let graph_list = panel.graph_list.clone();
    let panel_entity = cx.entity();
    // Same up-front label clones as the branches view: the `move` item
    // closure must not capture the `&ShellText` itself.
    let label_copy = text.git_branch_copy_hash.clone();
    let label_reset = text.git_graph_reset.clone();
    let label_reset_confirm = text.git_graph_reset_confirm.clone();
    let rows = list(graph_list, move |index, _window, cx| {
        let Some(node) = nodes.get(index) else {
            return div().into_any_element();
        };
        let lane_cell = |lane: i32, cx: &App| {
            let cell = div()
                .w(px(14.))
                .h_full()
                .flex()
                .items_center()
                .justify_center();
            if lane == node.lane {
                cell.child(div().size_2().rounded_full().bg(cx.theme().info))
            } else if node.passing_lanes.contains(&lane)
                || node.forking_lanes.contains(&lane)
                || node.merging_lanes.contains(&lane)
            {
                cell.child(div().w(px(2.)).h(px(20.)).bg(cx.theme().border))
            } else {
                cell
            }
        };
        let mut row = div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1p5()
            .rounded_sm()
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .gap_0p5()
                            .children((0..columns as i32).map(|lane| lane_cell(lane, cx))),
                    ),
            );
        if !node.refs.is_empty() {
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .flex_shrink_0()
                    .children(node.refs.iter().map(|graph_ref| {
                        div()
                            .px_1()
                            .py_0p5()
                            .rounded_sm()
                            .text_xs()
                            .flex_shrink_0()
                            .when(graph_ref.is_current, |this| {
                                this.bg(cx.theme().accent.opacity(0.2))
                                    .text_color(cx.theme().accent)
                            })
                            .when(!graph_ref.is_current, |this| {
                                this.text_color(cx.theme().muted_foreground)
                            })
                            .child(graph_ref.name.clone())
                    })),
            );
        }
        row = row
            .child(
                div()
                    .w(px(72.))
                    .flex_shrink_0()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .text_color(cx.theme().info)
                    .child(node.hash.chars().take(7).collect::<String>()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .truncate()
                    .child(node.message.clone()),
            )
            .child(
                div()
                    .max_w(px(140.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(node.author_name.clone()),
            );
        let mut actions = div().flex().items_center().gap_1().flex_shrink_0();
        actions = actions.child(
            Button::new(SharedString::from(format!("git-graph-copy-{index}")))
                .label(label_copy.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click({
                    let hash = node.hash.clone();
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(hash.clone()))
                }),
        );
        let armed = confirm_reset.as_deref() == Some(node.hash.as_str());
        actions = actions.child(
            Button::new(SharedString::from(format!("git-graph-reset-{index}")))
                .label(if armed {
                    label_reset_confirm.clone()
                } else {
                    label_reset.clone()
                })
                .small()
                .with_variant(if armed {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Ghost
                })
                .disabled(busy)
                .on_click({
                    let hash = node.hash.clone();
                    let panel_entity = panel_entity.clone();
                    move |_, _, cx| {
                        cx.stop_propagation();
                        panel_entity
                            .update(cx, |panel, cx| panel.reset_to_commit(hash.clone(), cx));
                    }
                }),
        );
        row = row.child(actions);
        row.into_any_element()
    });
    rows.flex_1().min_h_0().into_any_element()
}

fn render_history(
    panel: &GitPanel,
    text: &ShellText,
    cx: &mut Context<GitPanel>,
) -> gpui_kit::AnyElement {
    let commits = panel.commits.clone();
    if commits.is_empty() {
        let placeholder = if panel.busy {
            text.git_loading.clone()
        } else {
            text.git_no_commits.clone()
        };
        return div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(placeholder)
            .into_any_element();
    }
    let history_list = panel.history_list.clone();
    let rows = list(history_list, move |index, _window, cx| {
        let Some(commit) = commits.get(index) else {
            return div().into_any_element();
        };
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_1p5()
            .child(
                div()
                    .w(px(72.))
                    .flex_shrink_0()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_xs()
                    .text_color(cx.theme().info)
                    .child(commit.short_hash.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .truncate()
                    .child(commit.subject.clone()),
            )
            .child(
                div()
                    .max_w(px(160.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(commit.author.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(commit.date.clone()),
            )
            .into_any_element()
    });
    rows.flex_1().min_h_0().into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PATCH: &str = "diff --git a/src/lib.rs b/src/lib.rs\nindex 1234567..89abcde 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,4 +1,5 @@\n fn main() {\n-    old();\n+    new();\n+    extra();\n }\n\\ No newline at end of file\n";

    #[test]
    fn patch_lines_split_into_kinds() {
        let lines = parse_unified_patch(SAMPLE_PATCH);
        let kinds: Vec<DiffLineKind> = lines.iter().map(|line| line.kind).collect();
        assert_eq!(
            kinds,
            vec![
                DiffLineKind::Meta,    // diff --git
                DiffLineKind::Meta,    // index
                DiffLineKind::Meta,    // ---
                DiffLineKind::Meta,    // +++
                DiffLineKind::Hunk,    // @@
                DiffLineKind::Context, // " fn main() {"
                DiffLineKind::Delete,  // -old
                DiffLineKind::Add,     // +new
                DiffLineKind::Add,     // +extra
                DiffLineKind::Context, // " }"
                DiffLineKind::Meta,    // \ No newline
            ]
        );
    }

    #[test]
    fn file_headers_are_not_read_as_changes() {
        let lines = parse_unified_patch("--- a/x\n+++ b/x\n+added\n-removed\n");
        assert_eq!(lines[0].kind, DiffLineKind::Meta);
        assert_eq!(lines[1].kind, DiffLineKind::Meta);
        assert_eq!(lines[2].kind, DiffLineKind::Add);
        assert_eq!(lines[3].kind, DiffLineKind::Delete);
    }

    #[test]
    fn empty_line_in_patch_is_context() {
        let lines = parse_unified_patch(" a\n\n b\n");
        assert!(lines.iter().all(|line| line.kind == DiffLineKind::Context));
        assert_eq!(lines[1].text, "");
    }

    #[test]
    fn status_letters_follow_git_vocabulary() {
        assert_eq!(status_letter(WorkspaceDiffFileStatus::Added), "A");
        assert_eq!(status_letter(WorkspaceDiffFileStatus::Modified), "M");
        assert_eq!(status_letter(WorkspaceDiffFileStatus::Deleted), "D");
        assert_eq!(status_letter(WorkspaceDiffFileStatus::Renamed), "R");
        assert_eq!(status_letter(WorkspaceDiffFileStatus::Conflicted), "U");
    }

    #[test]
    fn stage_markers_report_untracked_alone() {
        let file = WorkspaceDiffFile {
            path: "new.txt".into(),
            old_path: None,
            status: WorkspaceDiffFileStatus::Added,
            staged: false,
            unstaged: true,
            untracked: true,
            additions: 1,
            deletions: 0,
            content: WorkspaceDiffContent::Text {
                patch: "+hello\n".into(),
            },
        };
        assert_eq!(stage_markers(&file), vec![StageMarker::Untracked]);

        let staged = WorkspaceDiffFile {
            untracked: false,
            staged: true,
            unstaged: false,
            ..file
        };
        assert_eq!(stage_markers(&staged), vec![StageMarker::Staged]);
    }

    #[test]
    fn widest_line_measures_columns_not_characters() {
        let lines = vec![
            DiffLine {
                kind: DiffLineKind::Context,
                text: "x".repeat(12),
            },
            // Eight CJK glyphs occupy sixteen monospace columns, more than the
            // twelve Latin characters above.
            DiffLine {
                kind: DiffLineKind::Add,
                text: "中文中文中文中文".into(),
            },
            DiffLine {
                kind: DiffLineKind::Delete,
                text: "-short".into(),
            },
        ];
        assert_eq!(
            widest_line(&lines).map(|line| line.text.as_str()),
            Some("中文中文中文中文")
        );
        assert!(widest_line(&[]).is_none());
    }

    #[test]
    fn row_stage_action_follows_worktree_content() {
        let mut file = sample_file("+only worktree\n");
        // Worktree content (unstaged or untracked) stages…
        assert_eq!(row_stage_action(&file), StageAction::Stage);
        // …while a fully staged file unstages.
        file.unstaged = false;
        file.staged = true;
        assert_eq!(row_stage_action(&file), StageAction::Unstage);
    }

    fn sample_file(patch: &str) -> WorkspaceDiffFile {
        WorkspaceDiffFile {
            path: "src/wide.rs".into(),
            old_path: None,
            status: WorkspaceDiffFileStatus::Modified,
            staged: false,
            unstaged: true,
            untracked: false,
            additions: 1,
            deletions: 1,
            content: WorkspaceDiffContent::Text {
                patch: patch.to_string(),
            },
        }
    }

    /// Both diff layouts must draw: wrapped rows flow in the viewport, and the
    /// unwrapped mode measures the widest row (CJK included) before handing the
    /// list its content width.
    #[gpui_kit::test]
    fn diff_renders_wrapped_and_unwrapped(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::i18n::fallback_text(
                openbitfun_core::service::i18n::LocaleId::EnUS,
            ));
        });
        let patch = "diff --git a/src/wide.rs b/src/wide.rs\n\
                     index 1111111..2222222 100644\n\
                     --- a/src/wide.rs\n\
                     +++ b/src/wide.rs\n\
                     @@ -1,2 +1,2 @@\n\
                     -let old = \"a very long line that will not fit in the diff column at all\";\n\
                     +let new = \"中文中文中文中文中文中文中文中文中文中文中文中文\";\n";
        let window = cx.add_empty_window();
        let panel = window.update(|window, cx| {
            cx.new(|cx| {
                let mut panel = GitPanel::new(None, None, window, cx);
                // The panel is driven with a fixture snapshot instead of the
                // git port so the render path is exercised without a runtime.
                panel.error = None;
                panel.files = vec![sample_file(patch)];
                panel.file_list.reset(panel.files.len());
                panel
            })
        });
        {
            let panel = panel.clone();
            window.draw(
                gpui_kit::point(px(0.), px(0.)),
                gpui_kit::size(px(900.), px(600.)),
                move |_, _| panel.into_any_element(),
            );
            window.run_until_parked();
        }

        window.update(|_, cx| {
            panel.update(cx, |panel, _| panel.wrap_lines = false);
        });
        window.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(900.), px(600.)),
            move |_, _| panel.into_any_element(),
        );
        window.run_until_parked();
    }

    /// The M18 surfaces must draw without a runtime: the history list over
    /// fixture commits, the untrusted-repository banner, and the files view
    /// with its commit box and per-row staging action.
    #[gpui_kit::test]
    fn history_trust_banner_and_commit_box_render(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::i18n::fallback_text(
                openbitfun_core::service::i18n::LocaleId::EnUS,
            ));
        });
        let window = cx.add_empty_window();
        let panel = window.update(|window, cx| cx.new(|cx| GitPanel::new(None, None, window, cx)));
        window.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.commits = vec![GitCommitSummary {
                    hash: "0123456789abcdef0123456789abcdef01234567".into(),
                    short_hash: "0123456".into(),
                    subject: "feat(gpui-shell): terminal tabs".into(),
                    author: "tester".into(),
                    date: "2026-10-10 12:00:00".into(),
                }];
                panel.history_list.reset(panel.commits.len());
                panel.view = GitView::History;
                panel.trust = Some(GitTrustSnapshot {
                    state: GitTrustState::TrustRequired,
                    repository_path: Some("/srv/repo".into()),
                    detail: Some("detected ownership mismatch".into()),
                    manual_command: None,
                });
                cx.notify();
            });
        });
        {
            let panel = panel.clone();
            window.draw(
                gpui_kit::point(px(0.), px(0.)),
                gpui_kit::size(px(900.), px(600.)),
                move |_, _| panel.into_any_element(),
            );
            window.run_until_parked();
        }

        // Back to the files view: a fully staged file offers unstage and the
        // commit box docks under the list.
        window.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.view = GitView::Files;
                panel.files = vec![{
                    let mut file = sample_file("+worktree\n");
                    file.staged = true;
                    file.unstaged = false;
                    file
                }];
                panel.file_list.reset(panel.files.len());
                panel.rebuild_diff(cx);
                cx.notify();
            });
        });
        window.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(900.), px(600.)),
            move |_, _| panel.into_any_element(),
        );
        window.run_until_parked();
    }

    fn sample_worktree(
        branch: &str,
        lifecycle: WorktreeLifecycle,
        is_main: bool,
    ) -> WorktreeSummary {
        WorktreeSummary {
            workspace_id: None,
            worktree_id: format!("wt-{branch}"),
            project_workspace_path: "/repo".into(),
            path: format!("/repo/.worktrees/{branch}"),
            head: "0123456789abcdef0123456789abcdef01234567".into(),
            branch: Some(branch.into()),
            lifecycle,
            is_main,
            dirty: !is_main,
            locked: false,
            missing: false,
            has_unpublished_commits: false,
            associated_session_count: usize::from(!is_main),
            running_session_count: 0,
            sessions: Vec::new(),
        }
    }

    #[test]
    fn snapshot_row_parses_core_stats_json() {
        let row = snapshot_row_from_stats(
            "sess-1",
            &serde_json::json!({ "session_id": "sess-1", "total_files": 3, "total_turns": 5, "total_changes": 9 }),
        );
        assert_eq!(row.session_id, "sess-1");
        assert_eq!(row.total_files, 3);
        assert_eq!(row.total_turns, 5);
        assert_eq!(row.total_changes, 9);
        // Missing or malformed facts keep the row renderable with zeros.
        let degraded = snapshot_row_from_stats("sess-2", &serde_json::Value::Null);
        assert_eq!(
            degraded,
            SnapshotRow {
                session_id: "sess-2".into(),
                total_files: 0,
                total_turns: 0,
                total_changes: 0,
            }
        );
    }

    #[test]
    fn worktree_row_view_resolves_actions_and_head() {
        let main = worktree_row_view(&sample_worktree("main", WorktreeLifecycle::External, true));
        assert!(main.removable == false);
        assert!(main.promotable == false);
        assert!(main.dirty == false);
        assert_eq!(main.head_short(), "0123456");

        let managed = worktree_row_view(&sample_worktree(
            "feature",
            WorktreeLifecycle::Managed,
            false,
        ));
        assert!(managed.removable);
        assert!(managed.promotable);
        assert!(managed.dirty);

        // An unborn or placeholder HEAD degrades to a dash instead of a
        // misleading hash prefix.
        let mut unborn = sample_worktree("fresh", WorktreeLifecycle::Managed, false);
        unborn.head = "0".repeat(40);
        assert_eq!(worktree_row_view(&unborn).head_short(), "-");
    }

    /// The M19 bodies must draw without a runtime: fixture worktree rows with
    /// their badges and actions, the isolation row without an open session,
    /// and the snapshot list over fixture stats.
    #[gpui_kit::test]
    fn worktree_and_snapshot_views_render(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::i18n::fallback_text(
                openbitfun_core::service::i18n::LocaleId::EnUS,
            ));
        });
        let window = cx.add_empty_window();
        let panel = window.update(|window, cx| cx.new(|cx| GitPanel::new(None, None, window, cx)));
        window.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.worktrees = vec![
                    sample_worktree("main", WorktreeLifecycle::External, true),
                    sample_worktree("feature", WorktreeLifecycle::Managed, false),
                ];
                panel.worktrees_loaded = true;
                panel.worktree_list.reset(panel.worktrees.len());
                panel.view = GitView::Worktrees;
                cx.notify();
            });
        });
        {
            let panel = panel.clone();
            window.draw(
                gpui_kit::point(px(0.), px(0.)),
                gpui_kit::size(px(900.), px(600.)),
                move |_, _| panel.into_any_element(),
            );
            window.run_until_parked();
        }

        window.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.snapshots = vec![snapshot_row_from_stats(
                    "0123456789abcdef",
                    &serde_json::json!({ "total_files": 2, "total_turns": 4, "total_changes": 7 }),
                )];
                panel.snapshots_loaded = true;
                panel.snapshot_list.reset(panel.snapshots.len());
                panel.view = GitView::Snapshots;
                cx.notify();
            });
        });
        window.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(900.), px(600.)),
            move |_, _| panel.into_any_element(),
        );
        window.run_until_parked();
    }

    /// G-G1: the not-a-repository answer empties the changes view and renders
    /// the init card instead of an error, and G-G3's row action is two-step —
    /// the first click arms, a different row moves the arming, and the
    /// confirming click clears it and runs the action.
    #[gpui_kit::test]
    fn not_a_repository_card_and_row_actions(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::i18n::fallback_text(
                openbitfun_core::service::i18n::LocaleId::EnUS,
            ));
        });
        let window = cx.add_empty_window();
        let panel = window.update(|window, cx| cx.new(|cx| GitPanel::new(None, None, window, cx)));
        window.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.files = vec![sample_file("")];
                panel.selected = 0;
                panel.file_list.reset(1);
                panel.enter_not_a_repository(cx);
                assert!(panel.not_a_repository);
                assert!(panel.files.is_empty(), "the card replaces the changes");
                assert!(panel.overview.is_none());
                assert!(
                    panel.error.is_none(),
                    "a missing repository is a state, not an error"
                );
            });
        });
        {
            let panel = panel.clone();
            window.draw(
                gpui_kit::point(px(0.), px(0.)),
                gpui_kit::size(px(900.), px(600.)),
                move |_, _| panel.into_any_element(),
            );
            window.run_until_parked();
        }
        assert!(
            window.debug_bounds("git-not-a-repository").is_some(),
            "the not-a-repository card lays out"
        );

        window.update(|_, cx| {
            panel.update(cx, |panel, cx| {
                panel.not_a_repository = false;
                panel.files = vec![sample_file("")];
                panel.file_list.reset(1);
                panel.confirm_action = None;

                panel.row_action(FileRowAction::Discard(0), cx);
                assert_eq!(panel.confirm_action, Some(FileRowAction::Discard(0)));
                // Arming another action moves the confirmation.
                panel.row_action(FileRowAction::Delete(0), cx);
                assert_eq!(panel.confirm_action, Some(FileRowAction::Delete(0)));
                // The confirming click clears the arming; without a runtime
                // the run itself is a safe no-op.
                panel.row_action(FileRowAction::Delete(0), cx);
                assert_eq!(panel.confirm_action, None);
            });
        });
    }
}
