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
    div, list, prelude::*, px, App, Context, IntoElement, ListAlignment, ListState, Pixels, Render,
    SharedString, TextRun, Window,
};
use openbitfun_runtime_ports::{
    WorkspaceDiffContent, WorkspaceDiffFile, WorkspaceDiffFileStatus, WorkspaceDiffSnapshot,
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
    /// Rows wrap at the viewport width (default). Unwrapped, every row keeps
    /// its own line and the view scrolls horizontally instead.
    wrap_lines: bool,
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
            wrap_lines: true,
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
        let text = i18n::current(cx);
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
        let panel = cx.new(GitPanel::new);
        panel.update(cx, |panel, cx| {
            // The panel is driven with a fixture snapshot instead of the git
            // port so the render path is exercised without a runtime.
            panel.error = None;
            panel.files = vec![sample_file(patch)];
            panel.file_list.reset(panel.files.len());
            panel.rebuild_diff(cx);
            assert!(panel.wrap_lines);
        });
        {
            let window = cx.add_empty_window();
            window.draw(
                gpui_kit::point(px(0.), px(0.)),
                gpui_kit::size(px(900.), px(600.)),
                {
                    let panel = panel.clone();
                    move |_, _| panel.into_any_element()
                },
            );
            window.run_until_parked();
        }

        panel.update(cx, |panel, _| panel.wrap_lines = false);
        let window = cx.add_empty_window();
        window.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(900.), px(600.)),
            move |_, _| panel.into_any_element(),
        );
        window.run_until_parked();
    }
}
