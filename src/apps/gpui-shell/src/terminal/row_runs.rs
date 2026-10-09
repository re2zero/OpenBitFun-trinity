//! Grid row to styled spans: merges runs of identically-styled cells so the
//! renderer shapes one string per style change instead of one per cell.

use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::Color;

/// One contiguous run of same-styled cells. `col` is the first grid column
/// the span starts at; wide characters advance two columns and the trailing
/// spacer cell is not represented in `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowSpan {
    pub col: usize,
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
}

impl RowSpan {
    /// `true` when the span paints nothing: default-colored blanks.
    pub fn is_invisible(&self) -> bool {
        self.text.chars().all(char::is_whitespace)
            && self.flags.is_empty()
            && self.fg == Color::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground)
            && self.bg == Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background)
    }
}

/// Segments one grid row into styled spans covering `cols` columns.
pub fn row_spans(line: &[Cell], cols: usize) -> Vec<RowSpan> {
    let mut spans: Vec<RowSpan> = Vec::new();
    let mut current: Option<RowSpan> = None;
    for col in 0..cols {
        let cell = &line[col];
        // Spacer cells belong to the wide character's cell advance; the
        // leading-spacer marks the empty column after a wrapped wide char.
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let same_style = current.as_ref().is_some_and(|span| {
            span.fg == cell.fg && span.bg == cell.bg && span.flags == cell.flags
        });
        match &mut current {
            Some(span) if same_style => span.text.push(cell.c),
            _ => {
                if let Some(finished) = current.take() {
                    spans.push(finished);
                }
                current = Some(RowSpan {
                    col,
                    text: cell.c.to_string(),
                    fg: cell.fg,
                    bg: cell.bg,
                    flags: cell.flags,
                });
            }
        }
    }
    if let Some(finished) = current.take() {
        spans.push(finished);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::Line;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::Term;
    use alacritty_terminal::vte;

    /// Same test-only workaround as `emulator.rs`: upstream's tuple impl is
    /// behind its own `cfg(test)`.
    struct TestSize(usize, usize);

    impl Dimensions for TestSize {
        fn columns(&self) -> usize {
            self.1
        }

        fn screen_lines(&self) -> usize {
            self.0
        }

        fn total_lines(&self) -> usize {
            self.0
        }
    }

    fn line_after(input: &[u8], cols: usize) -> Vec<Cell> {
        let mut term: Term<VoidListener> =
            Term::new(Config::default(), &TestSize(4, cols), VoidListener);
        // `Term` speaks `ansi::Handler`; the `ansi::Processor` is the vte
        // entry point (same as the production emulator).
        let mut parser: vte::ansi::Processor = vte::ansi::Processor::new();
        parser.advance(&mut term, input);
        term.grid()[Line(0)][..].to_vec()
    }

    #[test]
    fn plain_text_is_one_span() {
        let line = line_after(b"hello", 10);
        let spans = row_spans(&line, 10);
        // Trailing default-colored blanks share the style of plain text, so
        // they merge into the same span (full row width, one shape call).
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].col, 0);
        assert_eq!(spans[0].text, "hello     ");
        assert!(!spans[0].is_invisible());
    }

    #[test]
    fn blank_row_is_single_invisible_span() {
        let line = line_after(b"", 10);
        let spans = row_spans(&line, 10);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].is_invisible());
        assert_eq!(spans[0].text.len(), 10);
    }

    #[test]
    fn style_change_splits_spans() {
        let line = line_after(b"ab\x1b[31mcd", 10);
        let spans = row_spans(&line, 10);
        let visible: Vec<&RowSpan> = spans.iter().filter(|span| !span.is_invisible()).collect();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].text, "ab");
        assert_eq!(visible[1].col, 2);
        assert_eq!(visible[1].text, "cd");
        assert_eq!(
            visible[1].fg,
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Red)
        );
    }

    #[test]
    fn wide_char_span_skips_spacer_column() {
        let line = line_after("汉字".as_bytes(), 10);
        let spans = row_spans(&line, 10);
        let visible: Vec<&RowSpan> = spans.iter().filter(|span| !span.is_invisible()).collect();
        assert_eq!(
            visible.len(),
            1,
            "identical style must merge across wide chars"
        );
        assert_eq!(visible[0].text, "汉字");
        // 4 columns consumed by two wide chars.
        assert!(visible[0].col == 0);
    }

    #[test]
    fn trailing_blank_columns_merge_with_plain_text() {
        let line = line_after(b"ab", 10);
        let spans = row_spans(&line, 10);
        assert_eq!(spans.len(), 1, "blanks after plain text share its style");
        assert_eq!(spans[0].col, 0);
        assert_eq!(spans[0].text.len(), 10);
    }

    #[test]
    fn colored_text_splits_from_trailing_blanks() {
        // Red text then blank columns: the blanks keep the default style, so
        // the row splits into a styled span and an invisible one.
        let line = line_after(b"\x1b[31mab", 10);
        let spans = row_spans(&line, 10);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "ab");
        assert_eq!(
            spans[0].fg,
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Red)
        );
        assert!(spans[1].is_invisible());
        assert_eq!(spans[1].col, 2);
    }

    #[test]
    fn background_color_run_covers_trailing_spaces() {
        let line = line_after(b"\x1b[41m  ", 10);
        let spans = row_spans(&line, 10);
        let visible: Vec<&RowSpan> = spans.iter().filter(|span| !span.is_invisible()).collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].text, "  ");
        assert_eq!(
            visible[0].bg,
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Red)
        );
    }
}
