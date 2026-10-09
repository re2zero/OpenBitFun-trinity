use std::cell::RefCell;
use std::rc::Rc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte;

/// Scrollback lines kept behind the live screen.
const SCROLLBACK_LINES: usize = 10_000;

/// Grid dimensions for `Term::new`/`Term::resize` (the `(usize, usize)` impl
/// upstream is test-only).
#[derive(Clone, Copy)]
struct GridSize {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for GridSize {
    fn columns(&self) -> usize {
        self.columns
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn total_lines(&self) -> usize {
        self.screen_lines
    }
}

/// Emulator events the shell acts on. PTY-bound traffic (`PtyWrite` DA/DSR
/// replies, clipboard round-trips, color queries) never surfaces here — it
/// comes back through [`TerminalEmulator::take_pending`] separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// OSC 0/2 window title change.
    Title(String),
    /// Bell.
    Bell,
}

/// Everything drained from the emulator after one or more `advance` calls.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EmulatorOutput {
    /// Render/host events (title, bell).
    pub events: Vec<TerminalEvent>,
    /// Strings the emulator wants written back to the PTY (cursor position
    /// reports, device attribute replies). Dropping them freezes programs
    /// that query the terminal, so the caller must forward these bytes.
    pub pty_writes: Vec<String>,
}

/// Shared queue between the `Term` (writer) and the emulator (drainer).
#[derive(Default, Clone)]
struct EventQueue(Rc<RefCell<Vec<Event>>>);

impl EventListener for EventQueue {
    fn send_event(&self, event: Event) {
        self.0.borrow_mut().push(event);
    }
}

/// VT emulator over `alacritty_terminal`'s screen model. Not `Sync`: it lives
/// on the UI thread inside the pane entity and is advanced with raw PTY
/// bytes as they arrive from the event pump.
pub struct TerminalEmulator {
    term: Term<EventQueue>,
    /// `ansi::Processor` is the vte entry point dispatching into `Term`'s
    /// `Handler` impl (Term does not implement the raw `Perform` trait).
    parser: vte::ansi::Processor,
    queue: EventQueue,
    cols: usize,
    rows: usize,
    title: Option<String>,
}

impl TerminalEmulator {
    pub fn new(cols: u16, rows: u16) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let queue = EventQueue::default();
        let term = Term::new(
            config,
            &GridSize {
                columns: usize::from(cols),
                screen_lines: usize::from(rows),
            },
            queue.clone(),
        );
        Self {
            cols: usize::from(cols),
            rows: usize::from(rows),
            term,
            parser: vte::ansi::Processor::new(),
            queue,
            title: None,
        }
    }

    /// Feeds raw PTY output into the screen model.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Drains everything the emulator produced since the last call. The
    /// `Term` event variants that are neither host events nor PTY writes
    /// (`Wakeup`, `MouseCursorDirty`, clipboard and color round-trips) are
    /// dropped; clipboard and color query support is a later milestone.
    pub fn take_pending(&mut self) -> EmulatorOutput {
        let mut output = EmulatorOutput::default();
        let pending: Vec<Event> = self.queue.0.borrow_mut().drain(..).collect();
        for event in pending {
            match event {
                Event::Title(title) => {
                    self.title = Some(title.clone());
                    output.events.push(TerminalEvent::Title(title));
                }
                Event::Bell => output.events.push(TerminalEvent::Bell),
                Event::PtyWrite(text) => output.pty_writes.push(text),
                _ => {}
            }
        }
        output
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Reflows the grid to a new cell size.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = usize::from(cols).max(1);
        let rows = usize::from(rows).max(1);
        self.cols = cols;
        self.rows = rows;
        self.term.resize(GridSize {
            columns: cols,
            screen_lines: rows,
        });
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn mode(&self) -> &TermMode {
        self.term.mode()
    }

    /// Lines of history currently above the viewport.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Scrolls the viewport: positive `lines` moves toward history, negative
    /// toward the live edge.
    pub fn scroll_display(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    /// Jumps the viewport: `true` pins to the live edge, `false` to the
    /// oldest history.
    pub fn scroll_to_edge(&mut self, live: bool) {
        self.term
            .scroll_display(if live { Scroll::Bottom } else { Scroll::Top });
    }

    /// The cursor position in screen coordinates.
    pub fn cursor_point(&self) -> Point<Line, Column> {
        self.term.grid().cursor.point
    }

    /// One visible grid row, `0` = top of the viewport (history-aware).
    pub fn visible_line(&self, row: usize) -> &[Cell] {
        let line = Line(row as i32 - self.display_offset() as i32);
        &self.term.grid()[line][..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term::cell::Flags;
    use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

    fn emulator() -> TerminalEmulator {
        TerminalEmulator::new(80, 24)
    }

    fn cell(emulator: &TerminalEmulator, row: usize, col: usize) -> &Cell {
        &emulator.visible_line(row)[col]
    }

    #[test]
    fn plain_text_lands_in_grid() {
        let mut emulator = emulator();
        emulator.advance(b"hello");
        let line: String = (0..5).map(|col| cell(&emulator, 0, col).c).collect();
        assert_eq!(line, "hello");
        assert_eq!(emulator.cursor_point().line, Line(0));
        assert_eq!(emulator.cursor_point().column, Column(5));
    }

    #[test]
    fn newline_scrolls_into_history() {
        let mut emulator = emulator();
        // Newlines between lines only: the last line keeps the cursor on its
        // own row without scrolling it away.
        for i in 0..30 {
            emulator.advance(format!("line {i}").as_bytes());
            if i < 29 {
                emulator.advance(b"\r\n");
            }
        }
        assert_eq!(emulator.display_offset(), 0);
        let last: String = (0..7).map(|col| cell(&emulator, 23, col).c).collect();
        assert_eq!(last, "line 29");
        // 6 lines scrolled out of a 24-row screen.
        assert_eq!(emulator.term.grid().history_size(), 6);
    }

    #[test]
    fn sgr_named_colors_reach_cells() {
        let mut emulator = emulator();
        emulator.advance(b"\x1b[31mR\x1b[0m\x1b[91mB");
        assert_eq!(
            cell(&emulator, 0, 0).fg,
            Color::Named(NamedColor::Red),
            "SGR 31 must map to named red"
        );
        assert_eq!(
            cell(&emulator, 0, 1).fg,
            Color::Named(NamedColor::BrightRed),
            "SGR 91 must map to bright red"
        );
    }

    #[test]
    fn sgr_indexed_and_truecolor_reach_cells() {
        let mut emulator = emulator();
        emulator.advance(b"\x1b[38;5;196mi\x1b[49m\x1b[38;2;1;2;3mt");
        assert_eq!(cell(&emulator, 0, 0).fg, Color::Indexed(196));
        assert_eq!(
            cell(&emulator, 0, 1).fg,
            Color::Spec(Rgb { r: 1, g: 2, b: 3 })
        );
    }

    #[test]
    fn cursor_positioning_sequence_moves_cursor() {
        let mut emulator = emulator();
        emulator.advance(b"\x1b[2;3HX");
        assert_eq!(cell(&emulator, 1, 2).c, 'X');
    }

    #[test]
    fn alternate_screen_is_restored() {
        let mut emulator = emulator();
        emulator.advance(b"base");
        emulator.advance(b"\x1b[?1049h");
        assert!(emulator.mode().contains(TermMode::ALT_SCREEN));
        emulator.advance(b"\x1b[HALT");
        assert_eq!(cell(&emulator, 0, 0).c, 'A');
        emulator.advance(b"\x1b[?1049l");
        assert!(!emulator.mode().contains(TermMode::ALT_SCREEN));
        let restored: String = (0..4).map(|col| cell(&emulator, 0, col).c).collect();
        assert_eq!(restored, "base", "primary screen must survive alt screen");
    }

    #[test]
    fn wide_char_occupies_two_cells() {
        let mut emulator = emulator();
        emulator.advance("汉".as_bytes());
        let first = cell(&emulator, 0, 0);
        let second = cell(&emulator, 0, 1);
        assert_eq!(first.c, '汉');
        assert!(first.flags.contains(Flags::WIDE_CHAR));
        assert!(second.flags.contains(Flags::WIDE_CHAR_SPACER));
    }

    #[test]
    fn resize_reflows_without_losing_content() {
        let mut emulator = emulator();
        emulator.advance(b"content");
        emulator.resize(100, 30);
        assert_eq!(emulator.cols(), 100);
        assert_eq!(emulator.rows(), 30);
        let line: String = (0..7).map(|col| cell(&emulator, 0, col).c).collect();
        assert_eq!(line, "content");
    }

    #[test]
    fn display_offset_moves_viewport_into_history() {
        let mut emulator = emulator();
        for i in 0..30 {
            emulator.advance(format!("line {i}").as_bytes());
            if i < 29 {
                emulator.advance(b"\r\n");
            }
        }
        emulator.scroll_display(5);
        assert_eq!(emulator.display_offset(), 5);
        // Offset 5 shows five history lines above the screen: the topmost
        // visible row is the second line ever written.
        let top: String = (0..7).map(|col| cell(&emulator, 0, col).c).collect();
        assert_eq!(top.trim_end(), "line 1");
        emulator.scroll_to_edge(true);
        assert_eq!(emulator.display_offset(), 0);
        let live_top: String = (0..7).map(|col| cell(&emulator, 0, col).c).collect();
        assert_eq!(live_top.trim_end(), "line 6");
    }

    #[test]
    fn osc_title_and_bell_surface_as_events() {
        let mut emulator = emulator();
        emulator.advance(b"\x1b]2;spike title\x07");
        emulator.advance(b"\x07");
        assert_eq!(
            emulator.take_pending().events,
            vec![
                TerminalEvent::Title("spike title".to_string()),
                TerminalEvent::Bell,
            ]
        );
        assert_eq!(emulator.take_pending(), EmulatorOutput::default());
        assert_eq!(emulator.title(), Some("spike title"));
    }

    #[test]
    fn pty_writes_drain_without_eating_titles() {
        let mut emulator = emulator();
        emulator.advance(b"\x1b[6n");
        let output = emulator.take_pending();
        assert!(
            !output.pty_writes.is_empty(),
            "cursor position report must come back as a PTY write"
        );
        emulator.advance(b"\x1b]2;t2\x07");
        assert_eq!(
            emulator.take_pending().events,
            vec![TerminalEvent::Title("t2".to_string())]
        );
    }
}
