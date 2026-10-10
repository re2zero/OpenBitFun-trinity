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
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt};
use gpui_kit::{
    div, list, prelude::*, px, App, Context, Entity, IntoElement, ListAlignment, ListState, Pixels,
    Render, SharedString, TextRun, Window,
};
use openbitfun_runtime_ports::{
    GitCommitSummary, GitPort, GitRepositoryOverview, GitTrustSnapshot, GitTrustState, PortError,
    PortErrorKind, WorkspaceDiffContent, WorkspaceDiffFile, WorkspaceDiffFileStatus,
    WorkspaceDiffSnapshot,
};
use unicode_width::UnicodeWidthStr;

use crate::i18n::{self, ShellText};

/// Width of the changed-file column inside the panel.
const FILE_COLUMN_WIDTH: f32 = 300.;

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
enum GitView {
    Files,
    History,
}

/// How many commits the history view loads per request.
const HISTORY_LIMIT: usize = 50;

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
    history_list: ListState,
}

impl GitPanel {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = i18n::current(cx);
        let commit_input = cx
            .new(|cx| InputState::new(window, cx).placeholder(text.git_commit_placeholder.clone()));
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
            history_list: ListState::new(0, ListAlignment::Top, px(40.)),
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
                        panel.overview = overview.ok();
                        panel.apply_snapshot(snapshot, cx);
                    }
                    Ok((Err(error), _)) => {
                        tracing::warn!("Workspace diff failed: {}", error);
                        panel.fail(&error.to_string(), cx);
                        panel.handle_port_failure(&error, cx);
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

    fn toggle_view(&mut self, cx: &mut Context<Self>) {
        self.view = match self.view {
            GitView::Files => GitView::History,
            GitView::History => GitView::Files,
        };
        if self.view == GitView::History && !self.history_loaded {
            self.load_history(cx);
        }
        cx.notify();
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
    let summary = match (panel.loading, panel.files.is_empty()) {
        (true, _) => text.git_loading.clone(),
        (false, true) => text.git_empty.clone(),
        (false, false) => format!(
            "{}: {} · +{} -{}",
            text.git_title,
            panel.files.len(),
            additions,
            deletions
        ),
    };
    let (files_variant, history_variant) = match panel.view {
        GitView::Files => (ButtonVariant::Secondary, ButtonVariant::Ghost),
        GitView::History => (ButtonVariant::Ghost, ButtonVariant::Secondary),
    };
    // Without an overview yet the push stays enabled and fails loudly; with
    // one, a remote-less repository disables it instead.
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
                        .child(summary),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .when_some(panel.overview.as_ref(), |this, overview| {
                    let branch = overview.current_branch.clone();
                    this.child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().sidebar_accent)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .max_w(px(200.))
                            .child(branch),
                    )
                })
                .child(
                    Button::new("git-view-files")
                        .label(text.git_files_view.clone())
                        .small()
                        .with_variant(files_variant)
                        .tooltip(text.git_files_view.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.view != GitView::Files {
                                this.toggle_view(cx);
                            }
                        })),
                )
                .child(
                    Button::new("git-view-history")
                        .label(text.git_history.clone())
                        .small()
                        .with_variant(history_variant)
                        .tooltip(text.git_history.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.view != GitView::History {
                                this.toggle_view(cx);
                            }
                        })),
                )
                .child(
                    Button::new("git-stage-all")
                        .icon(IconName::ListTodo)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_stage_all.clone())
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.stage_all(cx))),
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
                .child(
                    Button::new("git-refresh")
                        .icon(IconName::RefreshCw)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.git_refresh.clone())
                        .disabled(panel.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                ),
        )
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

/// One changed-file row: status letter, path, line counts, and the single
/// staging action the row offers (stage when worktree content exists, unstage
/// when the changes are fully staged).
fn render_file_row(
    index: usize,
    file: &WorkspaceDiffFile,
    selected: bool,
    busy: bool,
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
                ),
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
        // place holding the window that InputState::set_value requires.
        if self.clear_commit_input {
            self.clear_commit_input = false;
            cx.update_entity(&self.commit_input, |input, cx| {
                input.set_value("", window, cx);
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

        let body = match view {
            GitView::Files => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .min_w_0()
                .child(
                    div()
                        .w(px(FILE_COLUMN_WIDTH))
                        .h_full()
                        .min_h_0()
                        .v_flex()
                        .border_r_1()
                        .border_color(cx.theme().border)
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
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .v_flex()
                        .when_some(diff_header, |this, header| this.child(header))
                        .child(files_body),
                )
                .into_any_element(),
            GitView::History => render_history(self, &text, cx),
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
    div()
        .w_full()
        .v_flex()
        .gap_1p5()
        .p_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .child(Input::new(&panel.commit_input))
        .child(
            Button::new("git-commit")
                .label(text.git_commit.clone())
                .small()
                .with_variant(ButtonVariant::Primary)
                .disabled(panel.busy || message_empty)
                .on_click(cx.listener(|this, _, _, cx| this.commit_staged(cx))),
        )
}

/// The read-only history body: recent commits of the current branch, newest
/// first. Empty until the first load; the loading state reuses the busy flag.
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
                let mut panel = GitPanel::new(window, cx);
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
        let panel = window.update(|window, cx| cx.new(|cx| GitPanel::new(window, cx)));
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
}
