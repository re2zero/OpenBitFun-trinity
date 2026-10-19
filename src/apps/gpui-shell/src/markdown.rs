//! Rich-render hooks for assistant markdown: the parts of the desktop
//! `MarkdownRenderer` the transcript has to match.
//!
//! - Fenced code blocks get syntax highlighting (the same syntect palette the
//!   file viewer uses), a language label and a copy action with the web
//!   `CopyButton`'s two-state feedback.
//! - Links route by target: `http(s)` goes to the system opener, a file path
//!   opens in the shell's own viewer (with a `#L42` / `:12-14` line reveal),
//!   and schemes without a shell surface are dropped instead of falling
//!   through to the platform opener.
//!
//! The component's `TextView` exposes exactly the three extension points the
//! web renderer has (`code_block_highlighter` / `code_block_actions` /
//! `on_link_click`); the highlighter is installed once through
//! `TextViewDefaults` so its `Arc` identity survives frames and the code
//! block keeps its cached highlights.

use std::ops::Range;
use std::time::{Duration, Instant};

use gpui_kit::base::text::CodeBlock;
use gpui_kit::{
    div, prelude::*, App, ClipboardItem, FontStyle, FontWeight, Global, HighlightStyle,
    SharedString,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::FontStyle as SyntectFontStyle;
use syntect::util::LinesWithEndings;

use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::{ActiveTheme, Sizable as _};

use crate::files::{HighlightFlavor, SYNTAX_SET, THEME_SET};

/// How long the copy action keeps its check mark (web `CopyButton` resets to
/// the copy glyph after 2000ms).
const COPY_FEEDBACK: Duration = Duration::from_secs(2);

/// Language ids rendered with a human label, ported from the web
/// `formatCodeLanguageLabel` alias table so both surfaces print the same word.
const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    ("js", "JavaScript"),
    ("jsx", "JavaScript"),
    ("mjs", "JavaScript"),
    ("cjs", "JavaScript"),
    ("javascript", "JavaScript"),
    ("ts", "TypeScript"),
    ("tsx", "TSX"),
    ("typescript", "TypeScript"),
    ("py", "Python"),
    ("rs", "Rust"),
    ("rust", "Rust"),
    ("go", "Go"),
    ("rb", "Ruby"),
    ("sh", "Shell"),
    ("bash", "Bash"),
    ("zsh", "Zsh"),
    ("fish", "Fish"),
    ("md", "Markdown"),
    ("yml", "YAML"),
    ("yaml", "YAML"),
    ("json", "JSON"),
    ("html", "HTML"),
    ("css", "CSS"),
    ("scss", "SCSS"),
    ("sass", "Sass"),
    ("less", "Less"),
    ("cpp", "C++"),
    ("cxx", "C++"),
    ("hpp", "C++"),
    ("hxx", "C++"),
    ("cc", "C++"),
    ("c", "C"),
    ("cs", "C#"),
    ("fs", "F#"),
    ("swift", "Swift"),
    ("kt", "Kotlin"),
    ("java", "Java"),
    ("sql", "SQL"),
    ("graphql", "GraphQL"),
    ("dockerfile", "Dockerfile"),
    ("makefile", "Makefile"),
    ("toml", "TOML"),
    ("xml", "XML"),
];

/// The toolbar label for a fence language: an alias when one matches, the
/// capitalized token otherwise, and `Text` for an unlabelled fence (the web
/// `formatCodeLanguageLabel` wording).
pub(crate) fn language_label(lang: Option<&str>) -> String {
    let Some(lang) = lang.map(str::trim).filter(|token| !token.is_empty()) else {
        return "Text".to_string();
    };
    let key = lang.to_ascii_lowercase();
    if let Some((_, label)) = LANGUAGE_ALIASES.iter().find(|(alias, _)| *alias == key) {
        return (*label).to_string();
    }
    let raw = lang.replace(['_', '-'], " ");
    let mut chars = raw.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => "Text".to_string(),
    }
}

/// Syntax highlight ranges for one fenced code block, in the block's byte
/// space — the shape `TextView::code_block_highlighter` expects. The language
/// token comes from the fence; a token the bundled syntax set does not know
/// renders plain, like the web fallback before Prism has loaded.
pub(crate) fn code_block_highlighter(
    flavor: HighlightFlavor,
) -> impl Fn(&CodeBlock) -> Vec<(Range<usize>, HighlightStyle)> + Send + Sync + 'static {
    move |block| highlight_code(&block.code(), block.lang().as_deref(), flavor)
}

fn highlight_code(
    code: &str,
    lang: Option<&str>,
    flavor: HighlightFlavor,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(theme) = THEME_SET.themes.get(flavor.syntect_theme_name()) else {
        return Vec::new();
    };
    // Only a fence language the bundled syntax set resolves gets highlights;
    // an absent or unknown token renders plain, the same shape the web
    // fallback has before (or without) Prism highlighting.
    let Some(syntax) = lang
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .and_then(|token| {
            SYNTAX_SET
                .find_syntax_by_token(token)
                .or_else(|| SYNTAX_SET.find_syntax_by_extension(&token.to_ascii_lowercase()))
        })
    else {
        return Vec::new();
    };

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut ranges: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut offset = 0usize;
    for line in LinesWithEndings::from(code) {
        if let Ok(spans) = highlighter.highlight_line(line, &SYNTAX_SET) {
            let mut within = 0usize;
            for (style, text) in spans {
                if !text.is_empty() {
                    ranges.push((
                        (offset + within)..(offset + within + text.len()),
                        HighlightStyle {
                            color: Some(syntect_color(style.foreground)),
                            font_style: style
                                .font_style
                                .contains(SyntectFontStyle::ITALIC)
                                .then_some(FontStyle::Italic),
                            font_weight: style
                                .font_style
                                .contains(SyntectFontStyle::BOLD)
                                .then_some(FontWeight::BOLD),
                            ..Default::default()
                        },
                    ));
                }
                within += text.len();
            }
        }
        // Advance by the line's own length whether or not the highlighter
        // returned spans, so the next line's offsets stay exact.
        offset += line.len();
    }
    ranges
}

fn syntect_color(color: syntect::highlighting::Color) -> gpui_kit::Hsla {
    gpui_kit::Hsla::from(gpui_kit::Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: f32::from(color.a) / 255.,
    })
}

/// Where a markdown link points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    /// An `http(s)` URL: handed to the system opener.
    External(String),
    /// A workspace file, with the 1-based line a `#L42` / `:12-14` suffix
    /// asked to reveal.
    File { path: String, line: Option<usize> },
    /// In-page anchors, `mailto:` and other schemes the shell has no surface
    /// for. Dropped on purpose: the platform opener would either fail or hand
    /// the URL to an unrelated application.
    Unsupported,
}

/// Classifies a markdown link URL the way the web `MarkdownRenderer` routes
/// its `a` elements (`href` prefix dispatch) minus the in-app surfaces the
/// shell does not have (browser tab, canvas).
pub(crate) fn classify_link(url: &str) -> LinkTarget {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') {
        return LinkTarget::Unsupported;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return LinkTarget::External(url.to_string());
    }
    if lower.starts_with("mailto:") || lower.starts_with("tel:") || lower.starts_with("data:") {
        return LinkTarget::Unsupported;
    }
    // `file:///abs/path` (the scheme bare paths get autolinked with in the
    // desktop reader) resolves to `/abs/path`.
    let raw = url
        .strip_prefix("file://")
        .or_else(|| url.strip_prefix("FILE://"))
        .unwrap_or(url);
    if raw.contains("://") {
        return LinkTarget::Unsupported;
    }
    let (path, line) = split_line_suffix(raw);
    if path.trim().is_empty() {
        return LinkTarget::Unsupported;
    }
    LinkTarget::File { path, line }
}

/// Splits a `#L42` / `#42` fragment or a trailing `:42` / `:12-14` line spec
/// off a path href (web `MarkdownRenderer`'s line-range parsing).
fn split_line_suffix(raw: &str) -> (String, Option<usize>) {
    if let Some((head, fragment)) = raw.split_once('#') {
        return match parse_line_spec(fragment) {
            Some(line) => (head.to_string(), Some(line)),
            // Not a line fragment: it belongs to the path (a rare filename
            // `#`, an in-file anchor the shell cannot resolve).
            None => (raw.to_string(), None),
        };
    }
    if let Some((head, tail)) = raw.rsplit_once(':') {
        if !head.is_empty() {
            if let Some(line) = parse_line_spec(tail) {
                return (head.to_string(), Some(line));
            }
        }
    }
    (raw.to_string(), None)
}

/// `42`, `L42`, `12-14`, `L12-L14` → the first line. Anything else is not a
/// line spec.
fn parse_line_spec(spec: &str) -> Option<usize> {
    let spec = spec.trim();
    let spec = spec
        .strip_prefix('L')
        .or_else(|| spec.strip_prefix('l'))
        .unwrap_or(spec);
    let digits: String = spec.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() != spec.split('-').next().unwrap_or("").len() {
        return None;
    }
    digits.parse::<usize>().ok().filter(|line| *line > 0)
}

/// What the transcript's last copy action copied: a fenced code block (keyed
/// by content hash) or a tool row (keyed by card key). One slot, because only
/// the freshest copy shows its check mark — the web keeps per-button state
/// with the same single-visible-window effect.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum CopiedThing {
    Block(u64),
    ToolRow(String),
}

/// The copy feedback window: what was copied and when. Rendering reads
/// [`code_block_copied`] / [`tool_row_copied`]; the expiring repaint is
/// scheduled by [`copy_with_feedback`] so the check mark does not overstay
/// when nothing else redraws the window.
#[derive(Default)]
struct CopiedFeedback {
    key: Option<CopiedThing>,
    at: Option<Instant>,
}

impl Global for CopiedFeedback {}

/// Stable content key for one code block: the same hashing the transcript uses
/// for markdown element ids.
fn code_hash(code: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

/// Copies text and arms the two-state feedback for whichever thing the copy
/// came from (web `useCopyTextAction`: write, then a check mark that resets;
/// the shell keeps the code block's 2s window for every copy action).
pub(crate) fn copy_with_feedback(thing: CopiedThing, text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    cx.set_global(CopiedFeedback {
        key: Some(thing.clone()),
        at: Some(Instant::now()),
    });
    cx.refresh_windows();
    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        executor.timer(COPY_FEEDBACK).await;
        cx.update(|cx| {
            // Only clear the thing this copy armed: a later copy of another
            // thing owns the feedback now.
            let stale = cx
                .try_global::<CopiedFeedback>()
                .is_some_and(|state| state.key.as_ref() == Some(&thing));
            if stale {
                cx.global_mut::<CopiedFeedback>().at = None;
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Copies a fenced code block and arms its feedback (web `CopyButton`:
/// `navigator.clipboard.writeText` then a check mark).
pub(crate) fn copy_code_block(code: &SharedString, cx: &mut App) {
    copy_with_feedback(CopiedThing::Block(code_hash(code)), code.to_string(), cx);
}

/// Copies one tool row's payload (web `ToolCardCopyAction`).
pub(crate) fn copy_tool_row(key: &SharedString, text: &str, cx: &mut App) {
    copy_with_feedback(CopiedThing::ToolRow(key.to_string()), text.to_string(), cx);
}

/// Whether the thing just copied still has its feedback window open.
fn copied_within(key: &CopiedThing, cx: &App) -> bool {
    cx.try_global::<CopiedFeedback>().is_some_and(|state| {
        state.key.as_ref() == Some(key) && state.at.is_some_and(|at| at.elapsed() < COPY_FEEDBACK)
    })
}

/// Whether the block just copied still has its feedback window open.
fn block_was_copied(hash: u64, cx: &App) -> bool {
    copied_within(&CopiedThing::Block(hash), cx)
}

/// Whether the tool row just copied still has its feedback window open.
pub(crate) fn tool_row_copied(key: &str, cx: &App) -> bool {
    copied_within(&CopiedThing::ToolRow(key.to_string()), cx)
}

/// The toolbar the component pins to the top-right of a fenced block (the web
/// `code-block-toolbar`): the language label beside the copy action. The
/// component's overlay slot anchors right, so the label rides with the copy
/// button instead of sitting on the block's left edge.
pub(crate) fn code_block_toolbar(
    block: &CodeBlock,
    copy_label: SharedString,
    copied_label: SharedString,
    cx: &App,
) -> gpui_kit::AnyElement {
    let code = block.code();
    let hash = code_hash(&code);
    let copied = block_was_copied(hash, cx);
    let label = language_label(block.lang().as_deref());
    div()
        .debug_selector(|| "code-block-toolbar".to_string())
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            gpui_kit::component::button::Button::new("copy")
                .icon(if copied {
                    gpui_kit::assets::IconName::Check
                } else {
                    gpui_kit::assets::IconName::Copy
                })
                .xsmall()
                .with_variant(gpui_kit::component::button::ButtonVariant::Ghost)
                .tooltip(if copied { copied_label } else { copy_label })
                .on_click(move |_, _, cx| {
                    copy_code_block(&code, cx);
                }),
        )
        .into_any_element()
}

/// Resolves a link's file path against the active workspace root — the
/// desktop reader's markdown `basePath`. Absolute paths pass through, `~/`
/// expands to the home directory, and a relative path joins the root.
pub(crate) fn resolve_path(root: &str, path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    if std::path::Path::new(path).is_absolute() || root.is_empty() {
        return path.to_string();
    }
    let relative = path.strip_prefix("./").unwrap_or(path);
    std::path::Path::new(root)
        .join(relative)
        .to_string_lossy()
        .to_string()
}

/// Opens a URL through the system opener. `App::open_url` resolves through
/// ashpd/zbus on Linux, which panics on a main thread without a tokio reactor
/// under the workspace feature union; `process_manager` + `xdg-open` is the
/// sanctioned child-process path and avoids that (same helper shape the
/// settings page uses to reveal the log directory).
pub(crate) fn open_external(url: &str) {
    match openbitfun_services_core::process_manager::create_command("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("Opened external URL {url}"),
        Err(error) => tracing::warn!("Failed to open {url}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G-S2: the copy feedback is per-thing. A tool row's copy arms that row's
    /// check mark and leaves every other row (and every code block) alone —
    /// which is what makes the row's icon flip for exactly two seconds.
    #[gpui_kit::test]
    fn copy_feedback_arms_only_the_thing_copied(cx: &mut gpui_kit::TestAppContext) {
        let row: SharedString = "tool-call-1".into();
        let other: SharedString = "tool-call-2".into();
        let block = CodeBlock::from_code("fn main() {}\n", Some("rs"));

        cx.update(|cx| {
            assert!(!tool_row_copied(&row, cx), "nothing copied yet");
            copy_tool_row(&row, "cargo test --lib", cx);
            assert!(tool_row_copied(&row, cx), "the copied row arms its check");
            assert!(
                !tool_row_copied(&other, cx),
                "a sibling row keeps its copy glyph"
            );
            assert!(
                !block_was_copied(code_hash(&block.code()), cx),
                "a code block keeps its copy glyph"
            );

            // A later copy takes the feedback over from the first row.
            copy_code_block(&block.code(), cx);
            assert!(
                !tool_row_copied(&row, cx),
                "the newer copy owns the feedback"
            );
            assert!(block_was_copied(code_hash(&block.code()), cx));
        });
    }

    #[test]
    fn language_labels_follow_the_web_alias_table() {
        assert_eq!(language_label(Some("rs")), "Rust");
        assert_eq!(language_label(Some("rust")), "Rust");
        assert_eq!(language_label(Some("tsx")), "TSX");
        assert_eq!(language_label(Some("sh")), "Shell");
        assert_eq!(language_label(Some("bash")), "Bash");
        // Unknown tokens capitalize instead of dropping the fence label.
        assert_eq!(language_label(Some("zeek")), "Zeek");
        assert_eq!(language_label(Some("my-lang")), "My lang");
        assert_eq!(language_label(None), "Text");
        assert_eq!(language_label(Some("  ")), "Text");
    }

    #[test]
    fn links_route_by_target() {
        assert_eq!(
            classify_link("https://example.com/a?b=1"),
            LinkTarget::External("https://example.com/a?b=1".to_string())
        );
        assert_eq!(
            classify_link("src/apps/gpui-shell/src/shell.rs"),
            LinkTarget::File {
                path: "src/apps/gpui-shell/src/shell.rs".to_string(),
                line: None
            }
        );
        assert_eq!(
            classify_link("/abs/path/main.rs#L42"),
            LinkTarget::File {
                path: "/abs/path/main.rs".to_string(),
                line: Some(42)
            }
        );
        assert_eq!(
            classify_link("file:///abs/path/lib.rs:12-14"),
            LinkTarget::File {
                path: "/abs/path/lib.rs".to_string(),
                line: Some(12)
            }
        );
        // Fragments that are not line specs stay part of the path; in-page
        // anchors and foreign schemes have no shell surface.
        assert_eq!(
            classify_link("docs/readme.md#install"),
            LinkTarget::File {
                path: "docs/readme.md#install".to_string(),
                line: None
            }
        );
        assert_eq!(classify_link("#section"), LinkTarget::Unsupported);
        assert_eq!(classify_link("mailto:a@b.c"), LinkTarget::Unsupported);
        assert_eq!(
            classify_link("openbitfun://runtime"),
            LinkTarget::Unsupported
        );
        assert_eq!(classify_link(""), LinkTarget::Unsupported);
    }

    #[test]
    fn paths_resolve_against_the_workspace_root() {
        assert_eq!(
            resolve_path("/home/zero/proj", "src/main.rs"),
            "/home/zero/proj/src/main.rs"
        );
        assert_eq!(
            resolve_path("/home/zero/proj", "./src/main.rs"),
            "/home/zero/proj/src/main.rs"
        );
        assert_eq!(resolve_path("/home/zero/proj", "/etc/hosts"), "/etc/hosts");
        assert_eq!(resolve_path("", "src/main.rs"), "src/main.rs");
    }

    #[test]
    fn highlight_ranges_land_inside_the_code() {
        let code = "fn main() {\n    let x = 1;\n}\n";
        let ranges = highlight_code(code, Some("rust"), HighlightFlavor::Dark);
        assert!(!ranges.is_empty(), "rust is in the bundled syntax set");
        for (range, style) in &ranges {
            assert!(range.start < range.end, "empty range {range:?}");
            assert!(range.end <= code.len(), "range past the code: {range:?}");
            assert!(style.color.is_some());
        }
        // Consecutive ranges tile the line without gaps or overlaps.
        let mut cursor = 0usize;
        for (range, _) in &ranges {
            assert!(range.start >= cursor, "overlapping ranges");
            cursor = range.end;
        }
        // An unknown language renders without highlights instead of failing.
        assert!(highlight_code("plain", Some("no-such-lang"), HighlightFlavor::Dark).is_empty());
    }

    #[test]
    fn the_default_highlighter_covers_the_fence_language() {
        let block = CodeBlock::from_code("fn main() {}\n", Some("rs"));
        let highlighter = code_block_highlighter(HighlightFlavor::Dark);
        assert!(!highlighter(&block).is_empty());
    }
}
