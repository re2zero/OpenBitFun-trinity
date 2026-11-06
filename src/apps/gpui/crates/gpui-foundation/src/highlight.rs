//! Syntax highlighting primitives shared by the theme defaults, the markdown
//! render hooks, and the files viewer. They live in the foundation crate so
//! `theme` can install the code-block highlighter without reaching into the
//! workspace pages; the syntax and theme sets load once per process (the CLI
//! TUI keeps the same process-wide sets).

use std::ops::Range;
use std::sync::LazyLock;

use gpui_kit::base::text::CodeBlock;
use gpui_kit::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba};
use syntect::easy::HighlightLines;
use syntect::highlighting::FontStyle as SyntectFontStyle;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
pub static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Which syntect theme the highlights were computed for. A tab highlighted
/// for the other flavor falls back to plain text until it is reopened; the
/// flavor is compared against the active shell theme at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightFlavor {
    Dark,
    Light,
}

impl HighlightFlavor {
    pub fn for_theme(dark: bool) -> Self {
        if dark {
            Self::Dark
        } else {
            Self::Light
        }
    }

    pub fn syntect_theme_name(self) -> &'static str {
        match self {
            Self::Dark => "base16-ocean.dark",
            Self::Light => "base16-ocean.light",
        }
    }
}

/// Syntax highlight ranges for one fenced code block, in the block's byte
/// space — the shape `TextView::code_block_highlighter` expects. The language
/// token comes from the fence; a token the bundled syntax set does not know
/// renders plain, like the web fallback before Prism has loaded.
pub fn code_block_highlighter(
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

fn syntect_color(color: syntect::highlighting::Color) -> Hsla {
    Hsla::from(Rgba {
        r: f32::from(color.r) / 255.,
        g: f32::from(color.g) / 255.,
        b: f32::from(color.b) / 255.,
        a: f32::from(color.a) / 255.,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
