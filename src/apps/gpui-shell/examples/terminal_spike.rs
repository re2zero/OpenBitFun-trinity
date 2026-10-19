//! M7 spike: `terminal-core` PtyService + alacritty_terminal screen model +
//! a minimal self-drawn grid on gpui. Fixed 80x24, keyboard input, ANSI
//! colors, cursor. Resize/scrollback/selection/IME integration land with the
//! real terminal pane (M7 acceptance items); this file only proves the
//! pipeline end to end.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{
    canvas, div, hsla, point, px, size, App, Bounds, Context, Entity, FocusHandle, FontStyle,
    FontWeight, KeyDownEvent, Pixels, Point, Render, SharedString, TextAlign, TitlebarOptions,
    UnderlineStyle, Window, WindowBounds, WindowOptions,
};
use openbitfun_gpui_shell::terminal::colors::TerminalPalette;
use openbitfun_gpui_shell::terminal::emulator::{TerminalEmulator, DEFAULT_SCROLLBACK_LINES};
use openbitfun_gpui_shell::terminal::row_runs::row_spans;
use terminal_core::{PtyService, PtyServiceEvent, ShellConfig, ShellType, TerminalConfig};

const COLS: u16 = 80;
const ROWS: u16 = 24;
const FONT_SIZE: f32 = 14.;
const LINE_HEIGHT: f32 = 18.;
const MONO_FONT: &str = "DejaVu Sans Mono";

/// PTY pump to UI messages.
enum PtyMsg {
    Started(u32),
    Data(Vec<u8>),
    Exited,
    SpawnFailed(String),
}

struct TerminalSpike {
    emulator: TerminalEmulator,
    palette: TerminalPalette,
    service: Arc<PtyService>,
    pty_id: Option<u32>,
    tokio: tokio::runtime::Handle,
    focus: FocusHandle,
    exited: bool,
    error: Option<String>,
}

impl TerminalSpike {
    fn new(tokio: tokio::runtime::Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
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
                executable: "/bin/bash".to_string(),
                args: vec!["-i".to_string()],
                env,
                cwd: None,
                login: false,
            };
            match service_for_task
                .create_process(shell, ShellType::Bash, COLS, ROWS)
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
            emulator: TerminalEmulator::new(COLS, ROWS, DEFAULT_SCROLLBACK_LINES),
            palette,
            service,
            pty_id: None,
            tokio: tokio.clone(),
            focus,
            exited: false,
            error: None,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(pty_id) = self.pty_id else {
            return;
        };
        if self.exited {
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
        let service = self.service.clone();
        self.tokio.spawn(async move {
            if let Err(err) = service.write(pty_id, &bytes).await {
                tracing::warn!("Failed to write to pty: {}", err);
            }
        });
        cx.notify();
    }
}

/// Applies PTY output on the UI thread: bytes go into the emulator, PTY-bound
/// replies go back through the service, consumed output is acknowledged so
/// flow control does not park the process.
fn receive_pty_msgs(
    mut msg_rx: tokio::sync::mpsc::UnboundedReceiver<PtyMsg>,
    window: &Window,
    cx: &mut Context<TerminalSpike>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(msg) = msg_rx.recv().await {
            let alive = cx
                .update(|_window, cx| match msg {
                    PtyMsg::Started(id) => this
                        .update(cx, |this, cx| {
                            this.pty_id = Some(id);
                            cx.notify();
                        })
                        .is_ok(),
                    PtyMsg::Data(data) => this
                        .update(cx, |this, cx| {
                            // Raw PTY byte trace for spike debugging.
                            if std::env::var_os("M7_SPIKE_TRACE").is_some() {
                                eprintln!(
                                    "pty->emu {} bytes: {:?}",
                                    data.len(),
                                    String::from_utf8_lossy(&data)
                                );
                            }
                            this.emulator.advance(&data);
                            let output = this.emulator.take_pending();
                            if let Some(pty_id) = this.pty_id {
                                let tokio = this.tokio.clone();
                                for text in output.pty_writes {
                                    let service = this.service.clone();
                                    tokio.spawn(async move {
                                        if let Err(err) =
                                            service.write(pty_id, text.as_bytes()).await
                                        {
                                            tracing::warn!(
                                                "Failed to write emulator reply: {}",
                                                err
                                            );
                                        }
                                    });
                                }
                                let len = data.len();
                                let service = this.service.clone();
                                tokio.spawn(async move {
                                    if let Err(err) = service.acknowledge_data(pty_id, len).await {
                                        tracing::warn!("Failed to acknowledge pty data: {}", err);
                                    }
                                });
                            }
                            cx.notify();
                        })
                        .is_ok(),
                    PtyMsg::Exited => this
                        .update(cx, |this, cx| {
                            this.exited = true;
                            tracing::info!("Spike terminal process exited");
                            cx.notify();
                        })
                        .is_ok(),
                    PtyMsg::SpawnFailed(error) => this
                        .update(cx, |this, cx| {
                            this.error = Some(error);
                            cx.notify();
                        })
                        .is_ok(),
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
        "space" => b" ".to_vec(),
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
        // Printable characters arrive as single-char keys already shifted.
        _ if key.len() == 1 && !ctrl => key.as_bytes().to_vec(),
        _ => return None,
    };
    out.extend(seq);
    let _ = shift;
    Some(out)
}

struct GridMetrics {
    cell_w: Pixels,
    cell_h: Pixels,
}

fn measure_grid(_bounds: Bounds<Pixels>, window: &mut Window) -> GridMetrics {
    let font = gpui_kit::font(MONO_FONT);
    let line = window.text_system().shape_line(
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
    GridMetrics {
        cell_w: line.width,
        cell_h: px(LINE_HEIGHT),
    }
}

fn paint_grid(
    entity: &Entity<TerminalSpike>,
    bounds: Bounds<Pixels>,
    metrics: &GridMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    // Snapshot owned render data so the entity borrow ends before painting
    // (paint needs `&mut App`).
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
    if cursor_row >= 0 && (cursor_row as usize) < rows {
        let cursor_bounds = Bounds {
            origin: cell_origin(bounds, cursor.column.0 as f32, cursor_row as f32, metrics),
            size: size(metrics.cell_w, metrics.cell_h),
        };
        window.paint_quad(gpui_kit::fill(
            cursor_bounds,
            hsla(palette.cursor.h, palette.cursor.s, palette.cursor.l, 0.45),
        ));
    }

    let font = gpui_kit::font(MONO_FONT);
    let trace = std::env::var_os("M7_SPIKE_TRACE").is_some();
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
            if !(span.is_invisible() || span.text.is_empty()) && bg != palette.background {
                let bg_bounds = Bounds {
                    origin: cell_origin(bounds, span.col as f32, row as f32, metrics),
                    size: size(metrics.cell_w * span_cols as f32, metrics.cell_h),
                };
                window.paint_quad(gpui_kit::fill(bg_bounds, bg));
            }
            if span.is_invisible() || span.text.is_empty() {
                continue;
            }
            if trace && row <= 6 {
                eprintln!(
                    "row{} span col={} cols={} text={:?} flags={:?}",
                    row, span.col, span_cols, span.text, span.flags
                );
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
}

fn cell_origin(bounds: Bounds<Pixels>, col: f32, row: f32, metrics: &GridMetrics) -> Point<Pixels> {
    point(
        bounds.origin.x + col * metrics.cell_w,
        bounds.origin.y + row * metrics.cell_h,
    )
}

impl Render for TerminalSpike {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let entity = cx.entity();
        let focus = self.focus.clone();
        let exited = self.exited;
        let error = self.error.clone();
        let title = self.emulator.title().map(str::to_string);

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .id("terminal-pane")
                    .size_full()
                    .p_2()
                    .key_context("TerminalSpike")
                    .track_focus(&focus)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(div().size_full().child(canvas(
                        move |bounds, window, _cx| measure_grid(bounds, window),
                        move |bounds, metrics, window, cx| {
                            paint_grid(&entity, bounds, &metrics, window, cx)
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
                        this.child(
                            div()
                                .absolute()
                                .bottom_2()
                                .left_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("process exited - window can be closed"),
                        )
                    })
                    .when_some(title, |this, title| {
                        this.child(
                            div()
                                .absolute()
                                .bottom_2()
                                .right_2()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(title),
                        )
                    }),
            )
    }
}

fn main() {
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let tokio_handle = tokio_runtime.handle().clone();
    std::mem::forget(tokio_runtime);

    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let open_result = gpui_kit::open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("terminal spike")),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(80.), px(80.)),
                    size: size(px(900.), px(560.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| TerminalSpike::new(tokio_handle, window, cx)),
        );
        match open_result {
            Ok(_) => {
                tracing::info!("Spike window opened");
                cx.activate(true);
            }
            Err(err) => tracing::error!("Failed to open spike window: {}", err),
        }
    });
}
