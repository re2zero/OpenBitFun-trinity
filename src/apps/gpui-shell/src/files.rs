//! Shell files page: workspace file tree plus a tabbed viewer.
//!
//! The tree lazily loads one directory level at a time through the core
//! filesystem service's ignore-filtered listing (gitignore, `.ignore`, git
//! exclude), the same semantics the workspace search walker applies. The
//! viewer opens files read-only: code gets syntect highlighting precomputed
//! off the UI thread, markdown renders through the shared `TextView`, and
//! images decode through gpui's image pipeline. Everything is capped: text
//! beyond [`TEXT_VIEW_CAP`] is truncated at a char boundary with a visible
//! notice, images beyond [`IMAGE_VIEW_CAP`] are refused with a notice.
//!
//! The page owns its state (git.rs pattern): fetches run on the runtime's
//! tokio handle and land through `cx.spawn` entity updates, so the shell only
//! toggles the view and drops the page on workspace switches.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{
    div, img, list, prelude::*, px, App, Context, FontStyle, HighlightStyle, Hsla, Image,
    ImageFormat, IntoElement, ListAlignment, ListOffset, ListState, Render, Rgba, SharedString,
    StyledText, TextStyle, Window,
};
use openbitfun_core::service::filesystem::{FileSystemService, FileTreeNode};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SyntectFontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::i18n::{self, ShellText};

/// Upper bound for text viewed in the shell (1 MiB). Larger files are read
/// only up to this many bytes and flagged truncated; the service itself has
/// no read-side cap for the shell to lean on.
const TEXT_VIEW_CAP: u64 = 1024 * 1024;
/// Upper bound for image bytes handed to the decode pipeline (20 MiB).
const IMAGE_VIEW_CAP: u64 = 20 * 1024 * 1024;
/// Width of the tree column inside the page.
const TREE_COLUMN_WIDTH: f32 = 300.;
/// Virtualized row metrics.
const TREE_ROW_HEIGHT: f32 = 28.;
const CODE_ROW_OVERSCAN: f32 = 40.;

/// Syntax and theme sets load once and are shared across highlights (the CLI
/// TUI keeps the same process-wide sets).
pub(crate) static SYNTAX_SET: LazyLock<SyntaxSet> =
    LazyLock::new(SyntaxSet::load_defaults_newlines);
pub(crate) static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Which syntect theme the highlights were computed for. A tab highlighted
/// for the other flavor falls back to plain text until it is reopened; the
/// flavor is compared against the active shell theme at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HighlightFlavor {
    Dark,
    Light,
}

impl HighlightFlavor {
    pub(crate) fn for_theme(dark: bool) -> Self {
        if dark {
            Self::Dark
        } else {
            Self::Light
        }
    }

    pub(crate) fn syntect_theme_name(self) -> &'static str {
        match self {
            Self::Dark => "base16-ocean.dark",
            Self::Light => "base16-ocean.light",
        }
    }
}

/// One highlighted run of a code line: the text plus the syntect-derived
/// style the render path maps onto gpui highlights.
#[derive(Debug, Clone)]
pub(crate) struct CodeSpan {
    pub text: String,
    pub color: Hsla,
    pub italic: bool,
}

/// What the viewer shows for one open file.
#[derive(Debug, Clone)]
pub(crate) enum OpenFileKind {
    /// `Rc` shares the precomputed lines with the render closures.
    Code {
        lines: Rc<Vec<Vec<CodeSpan>>>,
        flavor: HighlightFlavor,
    },
    Markdown {
        content: String,
    },
    Image {
        preview: Arc<Image>,
    },
    /// The file is over the view cap and was not read at all.
    TooLarge,
    /// The bytes are not decodable text (NUL byte or invalid UTF-8).
    Binary,
}

/// One open viewer tab. `truncated` marks text cut at [`TEXT_VIEW_CAP`].
#[derive(Debug, Clone)]
pub(crate) struct OpenFile {
    pub path: String,
    pub name: String,
    pub kind: OpenFileKind,
    pub truncated: bool,
}

/// One visible tree row (flattened from the expanded directories).
#[derive(Debug, Clone)]
struct TreeRow {
    node: FileTreeNode,
    depth: u32,
}

/// Which viewer surface a path opens into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewerKind {
    Code,
    Markdown,
    Image,
}

fn viewer_kind(path: &str) -> ViewerKind {
    match extension_of(path).as_deref() {
        Some("md") | Some("markdown") | Some("mark") => ViewerKind::Markdown,
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp") => {
            ViewerKind::Image
        }
        _ => ViewerKind::Code,
    }
}

fn extension_of(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
}

fn image_format(path: &str) -> Option<ImageFormat> {
    match extension_of(path).as_deref() {
        Some("png") => Some(ImageFormat::Png),
        Some("jpg") | Some("jpeg") => Some(ImageFormat::Jpeg),
        Some("webp") => Some(ImageFormat::Webp),
        Some("gif") => Some(ImageFormat::Gif),
        Some("bmp") => Some(ImageFormat::Bmp),
        _ => None,
    }
}

/// Cuts bytes at or before `cap` without splitting a UTF-8 code point.
fn truncate_at_char_boundary(bytes: &[u8], cap: usize) -> &[u8] {
    if bytes.len() <= cap {
        return bytes;
    }
    let mut end = cap.min(bytes.len());
    while end > 0 && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    &bytes[..end]
}

fn syntect_color(color: syntect::highlighting::Color) -> Hsla {
    Hsla::from(Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: f32::from(color.a) / 255.,
    })
}

/// Highlights `content` line by line. Multi-line constructs survive because
/// one [`HighlightLines`] walks the whole input sequentially; the result is
/// plain per-line spans the render path never recomputes.
fn highlight_code(content: &str, path: &str, flavor: HighlightFlavor) -> Vec<Vec<CodeSpan>> {
    let syntax = extension_of(path)
        .as_deref()
        .and_then(|ext| SYNTAX_SET.find_syntax_by_extension(ext))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());
    let Some(theme) = THEME_SET.themes.get(flavor.syntect_theme_name()) else {
        return plain_lines(content);
    };

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut rows = Vec::new();
    for line in LinesWithEndings::from(content) {
        let mut spans: Vec<CodeSpan> = Vec::new();
        match highlighter.highlight_line(line, &SYNTAX_SET) {
            Ok(ranges) => {
                for (style, text) in ranges {
                    let clean = text.trim_end_matches(['\r', '\n']);
                    if clean.is_empty() {
                        continue;
                    }
                    spans.push(CodeSpan {
                        text: clean.to_string(),
                        color: syntect_color(style.foreground),
                        italic: style.font_style.contains(SyntectFontStyle::ITALIC),
                    });
                }
            }
            Err(_) => spans.push(CodeSpan {
                text: line.trim_end_matches(['\r', '\n']).to_string(),
                color: theme
                    .settings
                    .foreground
                    .map(syntect_color)
                    .unwrap_or_else(|| {
                        syntect_color(syntect::highlighting::Color {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 255,
                        })
                    }),
                italic: false,
            }),
        }
        if spans.is_empty() {
            // Keep the row's line box: one zero-styled space.
            spans.push(CodeSpan {
                text: " ".to_string(),
                color: Hsla::default(),
                italic: false,
            });
        }
        rows.push(spans);
    }
    rows
}

fn plain_lines(content: &str) -> Vec<Vec<CodeSpan>> {
    content
        .lines()
        .map(|line| {
            vec![CodeSpan {
                text: if line.is_empty() {
                    " ".to_string()
                } else {
                    line.to_string()
                },
                color: Hsla::default(),
                italic: false,
            }]
        })
        .collect()
}

/// Outcome of the shell-side text read (cap + binary sniff happen here
/// because the service has no read-side size guard for the viewer).
enum TextRead {
    Text { content: String, truncated: bool },
    Binary,
}

async fn read_text_capped(service: &FileSystemService, path: &str) -> Result<TextRead, String> {
    let size = service
        .get_file_size(path)
        .await
        .map_err(|error| error.to_string())?;
    if size > TEXT_VIEW_CAP {
        let bytes = service
            .read_file_bytes(path)
            .await
            .map_err(|error| error.to_string())?;
        let cut = truncate_at_char_boundary(&bytes, TEXT_VIEW_CAP as usize);
        return Ok(TextRead::Text {
            content: String::from_utf8_lossy(cut).into_owned(),
            truncated: true,
        });
    }
    let bytes = service
        .read_file_bytes(path)
        .await
        .map_err(|error| error.to_string())?;
    if bytes.contains(&0) {
        return Ok(TextRead::Binary);
    }
    match String::from_utf8(bytes) {
        Ok(content) => Ok(TextRead::Text {
            content,
            truncated: false,
        }),
        Err(_) => Ok(TextRead::Binary),
    }
}

/// Sendable view payload produced on the runtime thread; the UI side wraps it
/// into an [`OpenFile`], `Rc`-sharing the code lines with render closures.
enum LoadedView {
    Code {
        lines: Vec<Vec<CodeSpan>>,
        flavor: HighlightFlavor,
        truncated: bool,
    },
    Markdown {
        content: String,
        truncated: bool,
    },
    Image {
        preview: Arc<Image>,
    },
    /// The file is over the view cap and was not read at all.
    TooLarge,
    /// The bytes are not decodable text (NUL byte or invalid UTF-8).
    Binary,
}

/// Loads one file, doing every decode-heavy step (highlighting, truncation,
/// image bytes) on the runtime thread.
async fn load_view_file(path: &str, flavor: HighlightFlavor) -> Result<LoadedView, String> {
    let service = FileSystemService::default();
    match viewer_kind(path) {
        ViewerKind::Image => {
            let size = service
                .get_file_size(path)
                .await
                .map_err(|error| error.to_string())?;
            if size > IMAGE_VIEW_CAP {
                return Ok(LoadedView::TooLarge);
            }
            let bytes = service
                .read_file_bytes(path)
                .await
                .map_err(|error| error.to_string())?;
            let format = image_format(path).ok_or("unsupported image format")?;
            Ok(LoadedView::Image {
                preview: Arc::new(Image::from_bytes(format, bytes)),
            })
        }
        ViewerKind::Markdown => match read_text_capped(&service, path).await? {
            TextRead::Text { content, truncated } => {
                Ok(LoadedView::Markdown { content, truncated })
            }
            TextRead::Binary => Ok(LoadedView::Binary),
        },
        ViewerKind::Code => match read_text_capped(&service, path).await? {
            TextRead::Text { content, truncated } => Ok(LoadedView::Code {
                lines: highlight_code(&content, path, flavor),
                flavor,
                truncated,
            }),
            TextRead::Binary => Ok(LoadedView::Binary),
        },
    }
}

/// Stable element id key for paths.
fn row_key(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// State of the files page; lives as its own entity (git.rs pattern) and is
/// dropped by the shell on workspace switches.
pub(crate) struct FilesPage {
    root: String,
    root_name: String,
    /// Top-level entries; the empty directory key owns them.
    top: Vec<FileTreeNode>,
    /// Loaded children per expanded directory path.
    children: HashMap<String, Vec<FileTreeNode>>,
    expanded: HashSet<String>,
    /// Directories (and the root, keyed "") with a listing in flight.
    loading: HashSet<String>,
    loaded: bool,
    error: Option<String>,
    /// Flattened visible rows, rebuilt whenever expansion changes.
    rows: Vec<TreeRow>,
    tree_list: ListState,
    tabs: Vec<OpenFile>,
    active: Option<String>,
    /// The path an open request is in flight for, if any.
    opening: Option<String>,
    /// 1-based line to reveal once the requested file lands (transcript file
    /// links carry `#L42` / `:12-14` specs).
    pending_line: Option<usize>,
    code_list: ListState,
    /// Docked right-panel form (R2c): the tree takes the whole panel and the
    /// viewer column is dropped — a 360px panel cannot fit the fixed 300px
    /// tree plus a readable preview. The main tab keeps the two-column form.
    compact: bool,
}

impl FilesPage {
    /// Switches between the main tab (two columns) and the docked panel
    /// (tree only) mount forms; called by the shell on the mount move.
    pub(crate) fn set_compact(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.compact != compact {
            self.compact = compact;
            cx.notify();
        }
    }

    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            root: String::new(),
            root_name: String::new(),
            top: Vec::new(),
            children: HashMap::new(),
            expanded: HashSet::new(),
            loading: HashSet::new(),
            loaded: false,
            error: None,
            rows: Vec::new(),
            tree_list: ListState::new(0, ListAlignment::Top, px(TREE_ROW_HEIGHT)),
            tabs: Vec::new(),
            active: None,
            opening: None,
            pending_line: None,
            code_list: ListState::new(0, ListAlignment::Top, px(CODE_ROW_OVERSCAN)),
            compact: false,
        };
        page.load_root(cx);
        page
    }

    fn load_root(&mut self, cx: &mut Context<Self>) {
        match crate::runtime::runtime_handle() {
            Some(handle) => {
                let workspace = handle.workspace();
                self.root = workspace.root_path.to_string_lossy().to_string();
                self.root_name = workspace.name.clone();
            }
            None => {
                self.fail("runtime not ready", cx);
                return;
            }
        }
        self.load_dir(String::new(), cx);
    }

    /// Reloads the tree from the root (all expansion state is dropped).
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        self.top.clear();
        self.children.clear();
        self.expanded.clear();
        self.rows.clear();
        self.tree_list.reset(0);
        self.loaded = false;
        self.error = None;
        self.load_root(cx);
        cx.notify();
    }

    /// Expands a directory (loading it first if needed) or collapses it.
    /// Collapse drops the cached listing so a re-expand refetches.
    pub(crate) fn toggle_dir(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.expanded.contains(path) {
            if let Some(nodes) = self.children.get(path).cloned() {
                self.expanded.insert(path.to_string());
                self.apply_dir_nodes(path, nodes);
                cx.notify();
                return;
            }
            self.load_dir(path.to_string(), cx);
            return;
        }
        self.collapse_dir(path);
        cx.notify();
    }

    fn collapse_dir(&mut self, path: &str) {
        self.expanded.remove(path);
        self.children.remove(path);
        self.rebuild_rows();
    }

    fn load_dir(&mut self, path: String, cx: &mut Context<Self>) {
        if self.loading.contains(&path) {
            return;
        }
        if self.root.is_empty() {
            return;
        }
        self.loading.insert(path.clone());
        cx.notify();
        let target = if path.is_empty() {
            self.root.clone()
        } else {
            path.clone()
        };
        let task = crate::runtime::runtime_handle().map(|handle| {
            handle.tokio_handle().spawn(async move {
                let service = FileSystemService::default();
                service.get_directory_listing(&target).await
            })
        });
        let Some(task) = task else {
            self.loading.remove(&path);
            self.fail("runtime not ready", cx);
            return;
        };
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |page, cx| {
                page.loading.remove(&path);
                match outcome {
                    Ok(Ok(nodes)) => {
                        page.error = None;
                        page.expanded.insert(path.clone());
                        page.apply_dir_nodes(&path, nodes);
                    }
                    Ok(Err(error)) => page.fail(&error.to_string(), cx),
                    Err(join_error) => page.fail(&join_error.to_string(), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Inserts a loaded listing and rebuilds the visible rows.
    fn apply_dir_nodes(&mut self, dir: &str, nodes: Vec<FileTreeNode>) {
        if dir.is_empty() {
            self.top = nodes;
            self.loaded = true;
        } else {
            self.children.insert(dir.to_string(), nodes);
        }
        self.rebuild_rows();
        self.tree_list.reset(self.rows.len());
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        fn walk(
            nodes: &[FileTreeNode],
            depth: u32,
            children: &HashMap<String, Vec<FileTreeNode>>,
            expanded: &HashSet<String>,
            rows: &mut Vec<TreeRow>,
        ) {
            for node in nodes {
                rows.push(TreeRow {
                    node: node.clone(),
                    depth,
                });
                if node.is_directory && expanded.contains(&node.path) {
                    if let Some(kids) = children.get(&node.path) {
                        walk(kids, depth + 1, children, expanded, rows);
                    }
                }
            }
        }
        walk(&self.top, 0, &self.children, &self.expanded, &mut rows);
        self.rows = rows;
    }

    fn fail(&mut self, error: &str, cx: &mut Context<Self>) {
        tracing::warn!("Files page action failed: {error}");
        self.error = Some(i18n::files_failed(error, cx));
        cx.notify();
    }

    /// Opens a file in the viewer (upserting its tab when already open).
    pub(crate) fn open_path(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_path_at(path, None, cx);
    }

    /// Like [`Self::open_path`], revealing a 1-based line once the file lands
    /// (a transcript file link's `#L42` / `:12-14` suffix).
    pub(crate) fn open_path_at(
        &mut self,
        path: String,
        line: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.opening.as_deref() == Some(path.as_str()) {
            return;
        }
        self.pending_line = line;
        let Some(handle) = crate::runtime::runtime_handle() else {
            self.fail("runtime not ready", cx);
            return;
        };
        let dark = cx.theme().mode.is_dark();
        let flavor = HighlightFlavor::for_theme(dark);
        self.opening = Some(path.clone());
        cx.notify();
        let opened_path = path.clone();
        let task = handle
            .tokio_handle()
            .spawn(async move { load_view_file(&path, flavor).await });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |page, cx| {
                page.opening = None;
                match outcome {
                    Ok(Ok(view)) => {
                        page.error = None;
                        page.open_loaded(&opened_path, view);
                    }
                    Ok(Err(error)) => page.fail(&error, cx),
                    Err(join_error) => page.fail(&join_error.to_string(), cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Wraps a loaded payload into a tab and activates it. Code lines are
    /// `Rc`-shared from here on (UI side only).
    fn open_loaded(&mut self, path: &str, view: LoadedView) {
        let name = Path::new(path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        let (kind, truncated) = match view {
            LoadedView::Code {
                lines,
                flavor,
                truncated,
            } => (
                OpenFileKind::Code {
                    lines: Rc::new(lines),
                    flavor,
                },
                truncated,
            ),
            LoadedView::Markdown { content, truncated } => {
                (OpenFileKind::Markdown { content }, truncated)
            }
            LoadedView::Image { preview } => (OpenFileKind::Image { preview }, false),
            LoadedView::TooLarge => (OpenFileKind::TooLarge, false),
            LoadedView::Binary => (OpenFileKind::Binary, false),
        };
        self.upsert_tab(OpenFile {
            path: path.to_string(),
            name,
            kind,
            truncated,
        });
        // Reveal the requested line after `activate_tab` reset the list, so
        // the scroll lands on the fresh content instead of being cleared.
        if let Some(line) = self.pending_line.take() {
            if line > 1 {
                self.code_list.scroll_to(ListOffset {
                    item_ix: line - 1,
                    offset_in_item: px(0.),
                });
            }
        }
    }

    /// Replaces or inserts the tab for `file.path` and activates it.
    fn upsert_tab(&mut self, file: OpenFile) {
        let path = file.path.clone();
        if let Some(slot) = self.tabs.iter().position(|tab| tab.path == path) {
            self.tabs[slot] = file;
        } else {
            self.tabs.push(file);
        }
        self.activate_tab(&path);
    }

    fn activate_tab(&mut self, path: &str) {
        self.active = Some(path.to_string());
        self.code_list.reset(self.active_code_len());
    }

    pub(crate) fn select_tab(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.tabs.iter().any(|tab| tab.path == path) {
            self.activate_tab(path);
            cx.notify();
        }
    }

    pub(crate) fn close_tab(&mut self, path: &str, cx: &mut Context<Self>) {
        self.close_tab_at(path);
        cx.notify();
    }

    /// Pure tab-close logic (no notify), so state-machine tests drive it
    /// without a window context.
    fn close_tab_at(&mut self, path: &str) {
        let Some(slot) = self.tabs.iter().position(|tab| tab.path == path) else {
            return;
        };
        self.tabs.remove(slot);
        if self.active.as_deref() == Some(path) {
            let next = self
                .tabs
                .get(slot)
                .or_else(|| slot.checked_sub(1).and_then(|index| self.tabs.get(index)))
                .map(|tab| tab.path.clone());
            self.active = next;
            self.code_list.reset(self.active_code_len());
        }
    }

    fn active_tab(&self) -> Option<&OpenFile> {
        let active = self.active.as_deref()?;
        self.tabs.iter().find(|tab| tab.path == active)
    }

    /// Last failure line, if any (read side for shell-level tests).
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn active_code_len(&self) -> usize {
        match self.active_tab().map(|tab| &tab.kind) {
            Some(OpenFileKind::Code { lines, .. }) => lines.len(),
            _ => 0,
        }
    }
}

impl Render for FilesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = i18n::current(cx);
        let this = cx.entity();

        let mut column = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .text_color(cx.theme().foreground);

        let mut header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .text_lg()
                    .text_color(cx.theme().foreground)
                    .child(text.files_title.clone()),
            );
        header = header.child(
            Button::new("files-reload")
                .icon(IconName::RefreshCw)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.files_reload.clone())
                .on_click(move |_, _, cx| {
                    this.update(cx, |page, cx| page.reload(cx));
                }),
        );
        column = column.child(header);

        if !self.root.is_empty() {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(self.root.clone()),
            );
        }

        if let Some(error) = &self.error {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }

        let this = cx.entity();
        let tree = div()
            .when(self.compact, |this| this.flex_1().min_w_0())
            .when(!self.compact, |this| {
                this.w(px(TREE_COLUMN_WIDTH)).flex_shrink_0()
            })
            .h_full()
            .flex()
            .flex_col()
            .when(!self.compact, |this| {
                this.border_r_1().border_color(cx.theme().border).pr_1()
            })
            .child(self.render_tree(this.clone(), &text, cx));
        let viewer = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .when(!self.compact, |this| this.pl_2())
            .child(self.render_viewer(this, &text, cx));

        // The docked panel is one column wide, so tree and viewer cannot sit
        // side by side (R2c kept the tree): an opened file takes the panel
        // over — a transcript path link must land on the file itself — and
        // closing the last tab brings the tree back.
        let docked_viewer = self.compact && (!self.tabs.is_empty() || self.opening.is_some());
        let body = if docked_viewer {
            viewer
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .child(tree)
                .when(!self.compact, |this| this.child(viewer))
        };
        column.child(body)
    }
}

impl FilesPage {
    fn render_tree(
        &self,
        this: gpui_kit::Entity<Self>,
        text: &ShellText,
        cx: &App,
    ) -> gpui_kit::AnyElement {
        if let Some(error) = &self.error {
            if !self.loaded {
                return div()
                    .p_2()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone())
                    .into_any_element();
            }
        }
        if self.loading.contains("") && !self.loaded {
            return div()
                .p_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.files_loading.clone())
                .into_any_element();
        }
        if self.loaded && self.top.is_empty() {
            return crate::shell::brand_empty(
                IconName::FolderOpen,
                &text.files_empty_title,
                &text.files_empty,
            );
        }

        let rows = self.rows.clone();
        list(self.tree_list.clone(), move |index, _window, cx| {
            let Some(row) = rows.get(index) else {
                return div().into_any_element();
            };
            let node = row.node.clone();
            let depth = row.depth;
            let page = this.read(cx);
            let expanded = page.expanded.contains(&node.path);
            let indent = px(4. + depth as f32 * 16.);
            let (icon, icon_color) = if node.is_directory {
                (
                    if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    },
                    cx.theme().primary,
                )
            } else {
                (IconName::FileText, cx.theme().muted_foreground)
            };
            let path = node.path.clone();
            let is_directory = node.is_directory;
            let this = this.clone();
            div()
                .id(SharedString::from(format!("tree-row-{}", row_key(&path))))
                .flex()
                .items_center()
                .gap_1()
                .h(px(TREE_ROW_HEIGHT))
                .pl(indent)
                .pr_2()
                .rounded(cx.theme().radius)
                .cursor_pointer()
                .hover(|style| style.bg(cx.theme().sidebar_accent))
                .on_click(move |_, _, cx| {
                    this.update(cx, |page, cx| {
                        if is_directory {
                            page.toggle_dir(&path, cx);
                        } else {
                            page.open_path(path.clone(), cx);
                        }
                    });
                })
                .child(
                    div().flex_shrink_0().child(
                        Icon::new(if is_directory {
                            if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            }
                        } else {
                            IconName::ChevronRight
                        })
                        .size_3()
                        .text_color(cx.theme().muted_foreground),
                    ),
                )
                .child(Icon::new(icon).size_3().text_color(icon_color))
                .child(div().text_sm().truncate().child(node.name.clone()))
                .into_any_element()
        })
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    fn render_viewer(
        &self,
        this: gpui_kit::Entity<Self>,
        text: &ShellText,
        cx: &App,
    ) -> gpui_kit::AnyElement {
        if self.tabs.is_empty() {
            if self.opening.is_some() {
                return div()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.files_loading.clone())
                    .into_any_element();
            }
            return crate::shell::brand_empty(
                IconName::FileText,
                &text.files_empty_title,
                &text.files_empty,
            );
        }

        let active_path = self.active.clone();
        let mut column = div().flex_1().min_h_0().flex().flex_col().gap_1();

        // Tab bar: one chip per open file; the chip body selects, the close
        // mark closes. The two click targets are siblings, not nested, so a
        // close never also selects.
        let mut bar = div()
            .id("files-tab-bar")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .overflow_x_scroll();
        for tab in &self.tabs {
            let active = active_path.as_deref() == Some(tab.path.as_str());
            let select_path = tab.path.clone();
            let close_path = tab.path.clone();
            let select_entity = this.clone();
            let close_entity = this.clone();
            bar = bar.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .when(active, |style| style.bg(cx.theme().sidebar_accent))
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "tab-select-{}",
                                row_key(&select_path)
                            )))
                            .cursor_pointer()
                            .text_sm()
                            .text_color(if active {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .on_click(move |_, _, cx| {
                                select_entity
                                    .update(cx, |page, cx| page.select_tab(&select_path, cx));
                            })
                            .child(tab.name.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "tab-close-{}",
                                row_key(&close_path)
                            )))
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                close_entity.update(cx, |page, cx| page.close_tab(&close_path, cx));
                            })
                            .child(
                                Icon::new(IconName::Close)
                                    .size_3()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    ),
            );
        }
        column = column.child(bar);

        if self.opening.is_some() {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.files_loading.clone()),
            );
        }

        let Some(tab) =
            active_path.and_then(|path| self.tabs.iter().find(|tab| tab.path == path).cloned())
        else {
            return column.into_any_element();
        };

        if tab.truncated {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.files_truncated.clone()),
            );
        }

        column = column.child(match &tab.kind {
            OpenFileKind::Code { lines, flavor } => {
                let stale = *flavor != HighlightFlavor::for_theme(cx.theme().mode.is_dark());
                let lines = lines.clone();
                // `gpui-pre`'s list sizes itself from the caller's style: without
                // an explicit fill it collapses to a zero-height box and lays
                // out no rows (MessageScroller passes the same pair).
                let list_element = list(self.code_list.clone(), move |index, _window, cx| {
                    let Some(spans) = lines.get(index) else {
                        return div().into_any_element();
                    };
                    render_code_line(index, spans, stale, cx)
                })
                .size_full()
                .min_h_0();
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius)
                    .py_1()
                    .child(list_element)
                    .into_any_element()
            }
            OpenFileKind::Markdown { content } => div()
                .id("files-markdown")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_2()
                .child(
                    TextView::markdown(
                        SharedString::from(format!("files-md-{}", row_key(&tab.path))),
                        content.clone(),
                    )
                    .selectable(true),
                )
                .into_any_element(),
            OpenFileKind::Image { preview } => div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(img(preview.clone()).max_w_full().max_h_full())
                .into_any_element(),
            OpenFileKind::TooLarge => div()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.files_too_large.clone())
                .into_any_element(),
            OpenFileKind::Binary => div()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.files_binary.clone())
                .into_any_element(),
        });

        column.into_any_element()
    }
}

/// One virtualized code row: right-aligned line number plus the highlighted
/// text. Stale-flavor rows (theme switched after load) render unstyled.
fn render_code_line(
    index: usize,
    spans: &[CodeSpan],
    stale: bool,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut body = String::new();
    let mut highlights = Vec::new();
    for span in spans {
        let start = body.len();
        body.push_str(&span.text);
        if !stale {
            highlights.push((
                Range {
                    start,
                    end: body.len(),
                },
                HighlightStyle {
                    color: Some(span.color),
                    font_style: span.italic.then_some(FontStyle::Italic),
                    ..Default::default()
                },
            ));
        }
    }
    let text_style = TextStyle {
        font_family: cx.theme().mono_font_family.clone(),
        font_size: px(12.).into(),
        ..Default::default()
    };
    div()
        .debug_selector(|| "files-code-row".to_string())
        .flex()
        .items_start()
        .w_full()
        .px_2()
        .child(
            div()
                .w(px(52.))
                .flex_shrink_0()
                .text_right()
                .pr_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{}", index + 1)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().foreground)
                .child(StyledText::new(body).with_default_highlights(&text_style, highlights)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, path: &str, is_directory: bool) -> FileTreeNode {
        FileTreeNode::new(
            path.to_string(),
            name.to_string(),
            path.to_string(),
            is_directory,
        )
    }

    #[test]
    fn tree_expand_and_collapse_projects_rows() {
        let mut page = tree_page();
        page.apply_dir_nodes(
            "",
            vec![
                node("src", "/w/src", true),
                node("README.md", "/w/README.md", false),
            ],
        );
        assert!(page.loaded);
        assert_eq!(page.rows.len(), 2);

        page.expanded.insert("/w/src".to_string());
        // The listing arrives pre-sorted (directories first, service-side).
        page.apply_dir_nodes(
            "/w/src",
            vec![
                node("ui", "/w/src/ui", true),
                node("lib.rs", "/w/src/lib.rs", false),
            ],
        );
        assert_eq!(page.rows.len(), 4);
        // Depth-first: the expanded directory's children follow it directly.
        assert_eq!(page.rows[1].depth, 1);
        assert_eq!(page.rows[1].node.name, "ui");
        assert_eq!(page.rows[2].node.name, "lib.rs");
        assert_eq!(page.rows[3].node.name, "README.md");
        assert_eq!(page.rows[3].depth, 0);

        page.collapse_dir("/w/src");
        assert_eq!(page.rows.len(), 2);
        assert!(page.children.get("/w/src").is_none());
    }

    #[test]
    fn tabs_upsert_select_and_close() {
        let mut page = tree_page();
        let file = |path: &str| OpenFile {
            path: path.to_string(),
            name: "f".to_string(),
            kind: OpenFileKind::Binary,
            truncated: false,
        };
        page.upsert_tab(file("/w/a.rs"));
        page.upsert_tab(file("/w/b.rs"));
        assert_eq!(page.tabs.len(), 2);
        assert_eq!(page.active.as_deref(), Some("/w/b.rs"));

        // Reopening an open path replaces its tab instead of duplicating it.
        page.upsert_tab(file("/w/a.rs"));
        assert_eq!(page.tabs.len(), 2);
        assert_eq!(page.active.as_deref(), Some("/w/a.rs"));

        page.close_tab_at("/w/a.rs");
        assert_eq!(page.tabs.len(), 1);
        assert_eq!(page.active.as_deref(), Some("/w/b.rs"));

        page.close_tab_at("/w/b.rs");
        assert!(page.active.is_none());
    }

    #[test]
    fn text_cap_truncates_on_char_boundary() {
        let ascii = b"hello world".as_slice();
        assert_eq!(truncate_at_char_boundary(ascii, 5), b"hello");
        assert_eq!(truncate_at_char_boundary(ascii, 100), ascii);

        let multibyte = "中文中文".as_bytes();
        let cut = truncate_at_char_boundary(multibyte, 5);
        assert_eq!(cut, "中".as_bytes());
        assert!(std::str::from_utf8(cut).is_ok());
    }

    #[test]
    fn viewer_kind_routes_by_extension() {
        assert_eq!(viewer_kind("/w/a.md"), ViewerKind::Markdown);
        assert_eq!(viewer_kind("/w/a.markdown"), ViewerKind::Markdown);
        assert_eq!(viewer_kind("/w/a.png"), ViewerKind::Image);
        assert_eq!(viewer_kind("/w/a.JPEG"), ViewerKind::Image);
        assert_eq!(viewer_kind("/w/a.rs"), ViewerKind::Code);
        assert_eq!(viewer_kind("/w/Makefile"), ViewerKind::Code);
    }

    #[test]
    fn highlight_produces_styled_rows_and_survives_unknown_extensions() {
        let rows = highlight_code(
            "fn main() {}\nlet a = 1;\n",
            "/w/a.rs",
            HighlightFlavor::Dark,
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0].iter().any(|span| !span.text.is_empty()));
        assert!(rows[0].iter().any(|span| span.color != Hsla::default()));

        let plain = highlight_code("plain text\n", "/w/unknownxyz", HighlightFlavor::Dark);
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0][0].text, "plain text");

        // An empty line keeps one whitespace span so the row keeps height.
        let blank = highlight_code("a\n\nb\n", "/w/a.txt", HighlightFlavor::Light);
        assert_eq!(blank.len(), 3);
        assert_eq!(blank[1][0].text, " ");
    }

    #[test]
    fn image_format_maps_supported_extensions() {
        assert_eq!(image_format("/w/a.png"), Some(ImageFormat::Png));
        assert_eq!(image_format("/w/a.svg"), None);
        assert_eq!(image_format("/w/a.rs"), None);
    }

    /// Exercises the render path headlessly with a fixture tree and a code
    /// tab (same shape as the git panel's draw test): virtualized tree rows,
    /// the tab bar, and highlighted code rows must all lay out.
    #[gpui_kit::test]
    fn page_renders_tree_and_code_viewer(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(crate::i18n::fallback_text(
                openbitfun_core::service::i18n::LocaleId::EnUS,
            ));
        });
        let page = cx.new(|_| tree_page());
        page.update(cx, |page, _| {
            page.apply_dir_nodes(
                "",
                vec![
                    node("src", "/w/src", true),
                    node("README.md", "/w/README.md", false),
                ],
            );
            let lines = Rc::new(highlight_code(
                "fn main() {}\nlet a = 1;\n",
                "/w/src/lib.rs",
                HighlightFlavor::Dark,
            ));
            page.upsert_tab(OpenFile {
                path: "/w/src/lib.rs".to_string(),
                name: "lib.rs".to_string(),
                kind: OpenFileKind::Code {
                    lines,
                    flavor: HighlightFlavor::Dark,
                },
                truncated: false,
            });
            assert_eq!(page.active_code_len(), 2);
        });
        let window = cx.add_empty_window();
        window.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(1000.), px(700.)),
            {
                let page = page.clone();
                move |_, _| page.into_any_element()
            },
        );
        window.run_until_parked();
        // The code list must lay rows out: a list without an explicit size
        // collapses to a zero-height box and paints nothing (the shape that
        // shipped an empty viewer until this assertion landed).
        assert!(
            window.debug_bounds("files-code-row").is_some(),
            "the code viewer lays out its rows"
        );
    }

    /// A page wired as if the root listing arrived, without any runtime.
    fn tree_page() -> FilesPage {
        FilesPage {
            root: "/w".to_string(),
            root_name: "w".to_string(),
            top: Vec::new(),
            children: HashMap::new(),
            expanded: HashSet::new(),
            loading: HashSet::new(),
            loaded: false,
            error: None,
            rows: Vec::new(),
            tree_list: ListState::new(0, ListAlignment::Top, px(TREE_ROW_HEIGHT)),
            tabs: Vec::new(),
            active: None,
            opening: None,
            pending_line: None,
            code_list: ListState::new(0, ListAlignment::Top, px(CODE_ROW_OVERSCAN)),
            compact: false,
        }
    }
}
