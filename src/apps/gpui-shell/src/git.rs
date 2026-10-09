//! Git changes panel: the workspace diff snapshot from the runtime git port,
//! rendered as a file list plus self-drawn unified-diff rows.
//!
//! The pinned component library (0.7.1) has no diff component, so the unified
//! patch produced by the port is parsed here and painted line by line (PLAN
//! M8 / D27). The panel is read-only: it shows what changed, it does not
//! stage, commit, or write.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt};
use gpui_kit::{
    div, list, prelude::*, px, App, Context, IntoElement, ListAlignment, ListState, Render, Window,
};
use openbitfun_runtime_ports::{
    WorkspaceDiffContent, WorkspaceDiffFile, WorkspaceDiffFileStatus, WorkspaceDiffSnapshot,
};

use crate::i18n::{self, ShellText};

/// Width of the changed-file column inside the panel.
const FILE_COLUMN_WIDTH: f32 = 300.;

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

/// The docked changes view: a changed-file list on the left, the diff of the
/// selected file on the right.
pub(crate) struct GitPanel {
    files: Vec<WorkspaceDiffFile>,
    selected: usize,
    loading: bool,
    error: Option<String>,
    truncated: bool,
    /// Parsed rows of the selected file's patch; rebuilt on selection change.
    diff: Rc<Vec<DiffLine>>,
    file_list: ListState,
    diff_list: ListState,
}

impl GitPanel {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            files: Vec::new(),
            selected: 0,
            loading: false,
            error: None,
            truncated: false,
            diff: Rc::new(Vec::new()),
            file_list: ListState::new(0, ListAlignment::Top, px(40.)),
            diff_list: ListState::new(0, ListAlignment::Top, px(40.)),
        };
        panel.refresh(cx);
        panel
    }

    /// Reloads the workspace diff snapshot through the runtime git port.
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
        let task = handle
            .tokio_handle()
            .spawn(async move { git.workspace_diff().await });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |panel, cx| {
                panel.loading = false;
                match outcome {
                    Ok(Ok(snapshot)) => panel.apply_snapshot(snapshot, cx),
                    Ok(Err(error)) => {
                        tracing::warn!("Workspace diff failed: {}", error);
                        panel.fail(&error.to_string(), cx);
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

/// Header: title, workspace name, totals, refresh action.
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
            Button::new("git-refresh")
                .icon(IconName::RefreshCw)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.git_refresh.clone())
                .disabled(panel.loading)
                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
        )
}

/// One changed-file row.
fn render_file_row(
    index: usize,
    file: &WorkspaceDiffFile,
    selected: bool,
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
/// diff reads without a gutter; colors come from the active theme.
fn render_diff_line(line: &DiffLine, cx: &App) -> gpui_kit::AnyElement {
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
        .truncate()
        .child(line.text.clone())
        .into_any_element()
}

impl Render for GitPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = i18n::current(cx);
        let selected_file = self.files.get(self.selected).cloned();
        let diff = self.diff.clone();
        let error = self.error.clone();
        let truncated = self.truncated;

        let file_list = {
            let this = cx.entity();
            list(self.file_list.clone(), move |index, _window, cx| {
                let panel = this.read(cx);
                let Some(file) = panel.files.get(index) else {
                    return div().into_any_element();
                };
                let text = i18n::current(cx);
                let row = render_file_row(index, file, index == panel.selected, &text, cx);
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
                .into_any_element()
        });

        let body = if let Some(error) = error {
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
                    list(
                        self.diff_list.clone(),
                        move |index, _window, cx| match diff.get(index) {
                            Some(line) => render_diff_line(line, cx),
                            None => div().into_any_element(),
                        },
                    )
                    .flex_1()
                    .min_h_0()
                    .into_any_element()
                }
            }
        };

        div()
            .size_full()
            .v_flex()
            .bg(cx.theme().background)
            .child(render_header(self, &text, cx))
            .child(
                div()
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
                            .child(body),
                    ),
            )
    }
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
}
