use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{
    canvas, div, hsla, point, px, size, App, Bounds, Context, Entity, FocusHandle, FontStyle,
    FontWeight, InputHandler, KeyDownEvent, Pixels, Point, Render, ScrollWheelEvent, SharedString,
    TextAlign, UTF16Selection, UnderlineStyle, Window,
};
use terminal_core::{PtyService, PtyServiceEvent, ShellConfig, ShellType, TerminalConfig};

use crate::terminal::colors::TerminalPalette;
use crate::terminal::emulator::TerminalEmulator;
use crate::terminal::row_runs::row_spans;

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 17.;
const MIN_COLS: u16 = 20;
const MIN_ROWS: u16 = 6;
const MONO_FONT: &str = "DejaVu Sans Mono";
const PANEL_HEIGHT: f32 = 300.;

/// One interactive terminal session embedded in the shell: a `PtyService`
/// process, the alacritty_terminal screen model, and a self-drawn grid
/// (M7). Owns its PTY lifecycle; [`TerminalPane::close`] tears it down.
pub struct TerminalPane {
    emulator: TerminalEmulator,
    palette: TerminalPalette,
    service: Arc<PtyService>,
    pty_id: Option<u32>,
    tokio: tokio::runtime::Handle,
    focus: FocusHandle,
    exited: bool,
    error: Option<String>,
    title: Option<String>,
    /// Grid size currently applied to both the emulator and the PTY.
    cols: u16,
    rows: u16,
    /// Cell metrics from the last paint, reused for wheel-step conversion and
    /// IME cursor positioning before the first frame lands.
    cell_w: Pixels,
    cell_h: Pixels,
    /// Cursor cell origin in window coordinates from the last paint, for the
    /// IME candidate window anchor.
    cursor_px: Option<Point<Pixels>>,
    /// Set when the PTY starts and the pane should take keyboard focus on
    /// the next frame (focusing needs `Window`, which the pump lacks).
    needs_focus: bool,
}

enum PtyMsg {
    Started(u32),
    Data(Vec<u8>),
    Exited,
    SpawnFailed(String),
}

impl TerminalPane {
    pub fn new(tokio: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let palette = TerminalPalette::from_theme(
            cx.theme().foreground,
            cx.theme().background,
            cx.theme().accent,
        );
        let service = Arc::new(PtyService::new(TerminalConfig::default()));
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<PtyMsg>();

        // Spawn the interactive shell, then pump raw output to the UI thread.
        let service_for_task = service.clone();
        tokio.spawn(async move {
            let mut env = HashMap::new();
            env.insert("COLORTERM".to_string(), "truecolor".to_string());
            let shell = ShellConfig {
                executable: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
                args: vec!["-i".to_string()],
                env,
                cwd: None,
                login: false,
            };
            // Initial 80x24; the first paint recomputes from the real bounds.
            match service_for_task
                .create_process(shell, ShellType::Bash, 80, 24)
                .await
            {
                Ok(id) => {
                    let _ = msg_tx.send(PtyMsg::Started(id));
                    loop {
                        match service_for_task.recv_event().await {
                            Some(PtyServiceEvent::ProcessData { data, .. }) => {
                                let _ = msg_tx.send(PtyMsg::Data(data));
                            }
                            Some(PtyServiceEvent::ProcessExit { .. }) => {
                                let _ = msg_tx.send(PtyMsg::Exited);
                                break;
                            }
                            Some(_) => {}
                            None => break,
                        }
                    }
                }
                Err(err) => {
                    let _ = msg_tx.send(PtyMsg::SpawnFailed(err.to_string()));
                }
            }
        });

        receive_pty_msgs(msg_rx, window, cx);

        Self {
            emulator: TerminalEmulator::new(80, 24),
            palette,
            service,
            pty_id: None,
            tokio: tokio.clone(),
            focus,
            exited: false,
            error: None,
            title: None,
            cols: 80,
            rows: 24,
            cell_w: px(7.),
            cell_h: px(LINE_HEIGHT),
            cursor_px: None,
            needs_focus: false,
        }
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub fn panel_height() -> Pixels {
        px(PANEL_HEIGHT)
    }

    /// Header label: OSC title from the shell, or a generic fallback.
    pub fn title_label(&self, fallback: SharedString) -> SharedString {
        self.title
            .clone()
            .unwrap_or_else(|| fallback.to_string())
            .into()
    }

    pub fn exited(&self) -> bool {
        self.exited
    }

    /// Shut the PTY down (panel close / shell teardown).
    pub fn close(&mut self) {
        if let Some(pty_id) = self.pty_id.take() {
            let service = self.service.clone();
            let tokio = self.tokio.clone();
            tokio.spawn(async move {
                if let Err(err) = service.shutdown(pty_id, false).await {
                    tracing::warn!("Failed to shut terminal pty down: {}", err);
                }
            });
        }
        self.exited = true;
    }

    /// Applies PTY output on the UI thread: bytes go into the emulator,
    /// PTY-bound replies go back through the service, consumed output is
    /// acknowledged so flow control does not park the process.
    fn apply_pty_msg(&mut self, msg: PtyMsg, cx: &mut Context<Self>) {
        match msg {
            PtyMsg::Started(id) => {
                self.pty_id = Some(id);
                self.needs_focus = true;
            }
            PtyMsg::Data(data) => {
                if std::env::var_os("OPENBITFUN_TERMINAL_TRACE").is_some() {
                    tracing::debug!(
                        "pty->emu {} bytes: {:?}",
                        data.len(),
                        String::from_utf8_lossy(&data)
                    );
                }
                self.emulator.advance(&data);
                let output = self.emulator.take_pending();
                if let Some(pty_id) = self.pty_id {
                    for text in output.pty_writes {
                        self.write_bytes(text.into_bytes());
                    }
                    // Acknowledge consumed bytes: past the 100k high-water
                    // mark the service pauses output until this lands.
                    let len = data.len();
                    let service = self.service.clone();
                    let tokio = self.tokio.clone();
                    tokio.spawn(async move {
                        if let Err(err) = service.acknowledge_data(pty_id, len).await {
                            tracing::warn!("Failed to acknowledge pty data: {}", err);
                        }
                    });
                }
                if let Some(title) = output
                    .events
                    .into_iter()
                    .rev()
                    .find_map(|event| match event {
                        crate::terminal::emulator::TerminalEvent::Title(title) => Some(title),
                        _ => None,
                    })
                {
                    self.title = Some(title);
                }
            }
            PtyMsg::Exited => {
                self.exited = true;
                tracing::info!("Terminal pane process exited");
            }
            PtyMsg::SpawnFailed(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn write_bytes(&self, bytes: Vec<u8>) {
        let Some(pty_id) = self.pty_id else {
            return;
        };
        let service = self.service.clone();
        let tokio = self.tokio.clone();
        tokio.spawn(async move {
            if let Err(err) = service.write(pty_id, &bytes).await {
                tracing::warn!("Failed to write to pty: {}", err);
            }
        });
    }

    /// Reflows the grid + PTY when the panel bounds imply a new cell size.
    /// Called from the paint phase with the freshly measured canvas bounds.
    fn resize_if_needed(&mut self, cols: u16, rows: u16, cx: &mut Context<Self>) {
        let cols = cols.max(MIN_COLS);
        let rows = rows.max(MIN_ROWS);
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.emulator.resize(cols, rows);
        if let Some(pty_id) = self.pty_id {
            let service = self.service.clone();
            let tokio = self.tokio.clone();
            tokio.spawn(async move {
                if let Err(err) = service.resize(pty_id, cols, rows).await {
                    tracing::warn!("Failed to resize pty: {}", err);
                }
            });
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pty_id.is_none() || self.exited {
            return;
        }
        let keystroke = &event.keystroke;
        let app_cursor = self
            .emulator
            .mode()
            .contains(alacritty_terminal::term::TermMode::APP_CURSOR);
        let Some(bytes) = key_to_bytes(
            &keystroke.key,
            keystroke.modifiers.shift,
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            app_cursor,
        ) else {
            return;
        };
        if bytes.is_empty() {
            return;
        }
        self.write_bytes(bytes);
        cx.notify();
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.exited {
            return;
        }
        let lines = match event.delta {
            gpui_kit::ScrollDelta::Pixels(p) => p.y / self.cell_h,
            gpui_kit::ScrollDelta::Lines(l) => l.y,
        };
        if lines != 0. {
            self.emulator.scroll_display(lines as i32);
            cx.notify();
        }
    }
}

/// Bridges the PTY pump into the pane on the UI thread.
fn receive_pty_msgs(
    mut msg_rx: tokio::sync::mpsc::UnboundedReceiver<PtyMsg>,
    window: &Window,
    cx: &mut Context<TerminalPane>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(msg) = msg_rx.recv().await {
            let alive = cx
                .update(|_window, cx| {
                    this.update(cx, |pane, cx| pane.apply_pty_msg(msg, cx))
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

/// Terminal key encoding: control keys, cursor keys (normal/application
/// mode), control combos, printable characters. Alt prefixes ESC.
fn key_to_bytes(
    key: &str,
    shift: bool,
    ctrl: bool,
    alt: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let _ = shift;
    let mut out = Vec::new();
    if alt {
        out.push(0x1b);
    }
    let cursor_seq = |letter: u8| -> Vec<u8> {
        if app_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    let seq: Vec<u8> = match key {
        "enter" => b"\r".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "tab" => {
            if shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        "escape" => b"\x1b".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "insert" => b"\x1b[2~".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        "home" => {
            if app_cursor {
                b"\x1bOH".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }
        }
        "end" => {
            if app_cursor {
                b"\x1bOF".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }
        }
        "up" => cursor_seq(b'A'),
        "down" => cursor_seq(b'B'),
        "right" => cursor_seq(b'C'),
        "left" => cursor_seq(b'D'),
        _ if ctrl && key.len() == 1 => {
            let letter = key.chars().next()?.to_ascii_uppercase();
            let byte = match letter {
                '@' => 0x00,
                'A'..='Z' => (letter as u8) - b'A' + 1,
                '[' => 0x1b,
                '\\' => 0x1c,
                ']' => 0x1d,
                '^' => 0x1e,
                '_' => 0x1f,
                '?' => 0x7f,
                _ => return None,
            };
            vec![byte]
        }
        // Printable characters (ASCII and space included) are owned by the
        // IME input handler (`replace_text_in_range`) while it is registered
        // for the focused pane; handling them here too would double every
        // keystroke (reproduced on the real machine, 2026-10-09).
        _ => return None,
    };
    out.extend(seq);
    Some(out)
}

struct GridMetrics {
    cell_w: Pixels,
    cell_h: Pixels,
}

/// IME bridge: committed composition text and platform pastes go to the PTY;
/// preedit marks are ignored (terminals commit on completion only).
struct TerminalInputHandler {
    pane: Entity<TerminalPane>,
}

impl InputHandler for TerminalInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        // The terminal document is the screen grid, not editable text: pin
        // the IME anchor to a virtual empty selection.
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, _cx: &mut App) -> Option<Range<usize>> {
        None
    }

    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        self.pane.update(_cx, |pane, _| {
            pane.write_bytes(new_text.as_bytes().to_vec());
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        _new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
        // Preedit updates stay local to the IME; the PTY sees the commit.
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut App) {}

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let pane = self.pane.read(cx);
        let origin = pane.cursor_px?;
        Some(Bounds {
            origin,
            size: size(pane.cell_w, pane.cell_h),
        })
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        None
    }
}

impl Render for TerminalPane {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        if self.needs_focus {
            self.needs_focus = false;
            window.focus(&self.focus, cx);
        }
        let entity = cx.entity();
        let focus = self.focus.clone();
        let exited = self.exited;
        let error = self.error.clone();
        let entity_for_paint = entity.clone();

        div()
            .id("terminal-pane")
            .size_full()
            .key_context("TerminalPane")
            .track_focus(&focus)
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|pane, _, window, cx| {
                    window.focus(&pane.focus, cx);
                }),
            )
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(div().size_full().child(canvas(
                move |bounds, window, cx| {
                    // Measure the monospace cell, then reflow the grid
                    // to the panel's real size (window resizes land
                    // here on every frame).
                    let font = gpui_kit::font(MONO_FONT);
                    let shaped = window.text_system().shape_line(
                        SharedString::from("M"),
                        px(FONT_SIZE),
                        &[gpui_kit::TextRun {
                            len: 1,
                            font,
                            color: gpui_kit::black(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        }],
                        None,
                    );
                    let metrics = GridMetrics {
                        cell_w: shaped.width,
                        cell_h: px(LINE_HEIGHT),
                    };
                    let cols =
                        ((bounds.size.width / metrics.cell_w).floor() as u16).clamp(MIN_COLS, 500);
                    let rows =
                        ((bounds.size.height / metrics.cell_h).floor() as u16).clamp(MIN_ROWS, 200);
                    entity.update(cx, |pane, cx| {
                        pane.cell_w = metrics.cell_w;
                        pane.cell_h = metrics.cell_h;
                        pane.resize_if_needed(cols, rows, cx);
                    });
                    metrics
                },
                move |bounds, metrics, window, cx| {
                    paint_grid(&entity_for_paint, bounds, &metrics, window, cx)
                },
            )))
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .absolute()
                        .top_2()
                        .left_2()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .when(exited, |this| {
                let exited_label = crate::i18n::current(cx).terminal_exited;
                this.child(
                    div()
                        .absolute()
                        .bottom_2()
                        .left_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(exited_label),
                )
            })
    }
}

fn paint_grid(
    entity: &Entity<TerminalPane>,
    bounds: Bounds<Pixels>,
    metrics: &GridMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    // Snapshot owned render data so the entity borrow ends before painting
    // (paint needs `&mut App` for quads and text).
    let (cols, rows, cursor, display_offset, palette, row_spans_by_row) = {
        let state = entity.read(cx);
        let emulator = &state.emulator;
        let cols = emulator.cols();
        let rows = emulator.rows();
        let spans = (0..rows)
            .map(|row| row_spans(emulator.visible_line(row), cols))
            .collect::<Vec<_>>();
        (
            cols,
            rows,
            emulator.cursor_point(),
            emulator.display_offset(),
            state.palette,
            spans,
        )
    };

    // Cursor block under the text (viewport-aware: history scroll can move
    // the cursor row out of view).
    let cursor_row = cursor.line.0 as i32 + display_offset as i32;
    let mut cursor_origin = None;
    if cursor_row >= 0 && (cursor_row as usize) < rows {
        let origin = cell_origin(bounds, cursor.column.0 as f32, cursor_row as f32, metrics);
        window.paint_quad(gpui_kit::fill(
            Bounds {
                origin,
                size: size(metrics.cell_w, metrics.cell_h),
            },
            hsla(palette.cursor.h, palette.cursor.s, palette.cursor.l, 0.45),
        ));
        cursor_origin = Some(origin);
    }

    let font = gpui_kit::font(MONO_FONT);
    for (row, spans) in row_spans_by_row.iter().enumerate() {
        // Walk spans right-to-left so each span knows how many grid columns
        // it occupies (wide characters advance two).
        let mut next_col = cols;
        for span in spans.iter().rev() {
            let span_cols = (next_col - span.col).max(1);
            next_col = span.col;
            let dim = span
                .flags
                .contains(alacritty_terminal::term::cell::Flags::DIM);
            let inverse = span
                .flags
                .contains(alacritty_terminal::term::cell::Flags::INVERSE);
            // Cell background as an exact quad. Invisible spans (default
            // blanks) skip both text and background; every visible span gets
            // its background painted even when the text is whitespace, so a
            // colored band from `printf '\e[41m   '` renders. Quads instead
            // of TextRun background decorations: shape_line's force_width
            // path displaces glyphs of decorated runs (gpui-pre 0.7.1,
            // reproduced 2026-10-09), and cell-aligned quads are what the
            // grid actually wants.
            let bg = if inverse {
                palette.resolve(span.fg, dim)
            } else {
                palette.resolve(span.bg, false)
            };
            let visible = !(span.is_invisible() || span.text.is_empty());
            if visible && bg != palette.background {
                window.paint_quad(gpui_kit::fill(
                    Bounds {
                        origin: cell_origin(bounds, span.col as f32, row as f32, metrics),
                        size: size(metrics.cell_w * span_cols as f32, metrics.cell_h),
                    },
                    bg,
                ));
            }
            if !visible {
                continue;
            }
            let mut run_font = font.clone();
            if span
                .flags
                .contains(alacritty_terminal::term::cell::Flags::BOLD)
            {
                run_font.weight = FontWeight::BOLD;
            }
            if span
                .flags
                .contains(alacritty_terminal::term::cell::Flags::ITALIC)
            {
                run_font.style = FontStyle::Italic;
            }
            let fg = if inverse {
                palette.resolve(span.bg, false)
            } else {
                palette.resolve(span.fg, dim)
            };
            let underline = span
                .flags
                .contains(alacritty_terminal::term::cell::Flags::UNDERLINE)
                .then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                });
            // No `force_width`: it displaces decorated-run glyphs (see the
            // background comment); natural mono advance fills the cells.
            let shaped = window.text_system().shape_line(
                SharedString::from(span.text.clone()),
                px(FONT_SIZE),
                &[gpui_kit::TextRun {
                    len: span.text.len(),
                    font: run_font,
                    color: fg,
                    background_color: None,
                    underline,
                    strikethrough: None,
                }],
                None,
            );
            let _ = shaped.paint(
                cell_origin(bounds, span.col as f32, row as f32, metrics),
                metrics.cell_h,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }

    // Register the IME handler for the focused frame and cache the cursor
    // anchor for `bounds_for_range`.
    let pane = entity.read(cx);
    window.handle_input(
        &pane.focus,
        TerminalInputHandler {
            pane: entity.clone(),
        },
        cx,
    );
    let cursor_px = cursor_origin;
    let cell_w = metrics.cell_w;
    let cell_h = metrics.cell_h;
    entity.update(cx, |pane, _| {
        pane.cursor_px = cursor_px;
        pane.cell_w = cell_w;
        pane.cell_h = cell_h;
    });
}

fn cell_origin(bounds: Bounds<Pixels>, col: f32, row: f32, metrics: &GridMetrics) -> Point<Pixels> {
    point(
        bounds.origin.x + col * metrics.cell_w,
        bounds.origin.y + row * metrics.cell_h,
    )
}

#[cfg(test)]
mod tests {
    use super::key_to_bytes;

    #[test]
    fn control_keys_map_to_terminal_sequences() {
        assert_eq!(
            key_to_bytes("enter", false, false, false, false).unwrap(),
            b"\r"
        );
        assert_eq!(
            key_to_bytes("backspace", false, false, false, false).unwrap(),
            b"\x7f"
        );
        assert_eq!(
            key_to_bytes("escape", false, false, false, false).unwrap(),
            b"\x1b"
        );
        assert_eq!(
            key_to_bytes("tab", false, false, false, false).unwrap(),
            b"\t"
        );
        assert_eq!(
            key_to_bytes("tab", true, false, false, false).unwrap(),
            b"\x1b[Z",
            "shift+tab is backtab"
        );
    }

    #[test]
    fn cursor_keys_follow_application_mode() {
        assert_eq!(
            key_to_bytes("up", false, false, false, false).unwrap(),
            b"\x1b[A"
        );
        assert_eq!(
            key_to_bytes("left", false, false, false, true).unwrap(),
            b"\x1bOD"
        );
        assert_eq!(
            key_to_bytes("home", false, false, false, true).unwrap(),
            b"\x1bOH"
        );
        assert_eq!(
            key_to_bytes("pagedown", false, false, false, false).unwrap(),
            b"\x1b[6~"
        );
    }

    #[test]
    fn control_combos_and_alt_prefix() {
        assert_eq!(
            key_to_bytes("c", false, true, false, false).unwrap(),
            vec![0x03]
        );
        assert_eq!(
            key_to_bytes("u", false, true, false, false).unwrap(),
            vec![0x15]
        );
        assert_eq!(
            key_to_bytes("[", false, true, false, false).unwrap(),
            vec![0x1b]
        );
        assert_eq!(
            key_to_bytes("c", false, true, true, false).unwrap(),
            vec![0x1b, 0x03],
            "alt prefixes ESC before the control byte"
        );
        // Plain alt+printable also belongs to the IME handler; the ESC meta
        // prefix for it is an accepted M7 gap (rare in shell usage).
        assert!(key_to_bytes("c", false, false, true, false).is_none());
    }

    #[test]
    fn printables_belong_to_the_ime_handler() {
        assert!(key_to_bytes("a", false, false, false, false).is_none());
        assert!(key_to_bytes(" ", false, false, false, false).is_none());
        assert!(key_to_bytes("$", true, false, false, false).is_none());
    }
}
