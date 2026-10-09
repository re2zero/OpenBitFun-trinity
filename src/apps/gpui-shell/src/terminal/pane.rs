use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use alacritty_terminal::index::{Column as GridColumn, Line as GridLine, Point as GridPoint, Side};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::TermMode;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::*;
use gpui_kit::{
    canvas, div, hsla, point, px, size, App, Bounds, ClipboardItem, Context, Entity, FocusHandle,
    FontStyle, FontWeight, InputHandler, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollWheelEvent, SharedString, TextAlign, UTF16Selection,
    UnderlineStyle, Window,
};
use terminal_core::{PtyService, PtyServiceEvent, ShellConfig, ShellType, TerminalConfig};

use crate::settings::{self, TerminalSettings};
use crate::terminal::colors::TerminalPalette;
use crate::terminal::emulator::{TerminalEmulator, DEFAULT_SCROLLBACK_LINES};
use crate::terminal::row_runs::row_spans;

const FONT_SIZE: f32 = 13.;
/// Line box as a multiple of the font size; the grid keeps every row this tall.
const LINE_HEIGHT_RATIO: f32 = 1.31;
const MIN_COLS: u16 = 20;
const MIN_ROWS: u16 = 6;
const MONO_FONT: &str = "DejaVu Sans Mono";
const DEFAULT_SHELL: &str = "/bin/bash";
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
    /// Grid typography, from the shared terminal settings (font size/family)
    /// with shell defaults for unset or unresolvable values. The configured
    /// family is kept next to the resolved one so the (allocating) font-list
    /// lookup and its warning run only when the setting actually changes.
    font_size: f32,
    font_family: SharedString,
    font_family_configured: String,
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
    /// Content-area bounds from the last paint, for pointer hit-testing.
    grid_bounds: Option<Bounds<Pixels>>,
    /// True while a left-button drag extends the selection.
    selecting: bool,
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

/// Grid line box for one font size; every row is exactly this tall.
fn line_height(font_size: f32) -> f32 {
    (font_size * LINE_HEIGHT_RATIO).round()
}

/// Font size the grid renders with; an unset or out-of-range value means the
/// default (the value domain itself is owned by the settings page).
fn resolve_font_size(configured: u32) -> f32 {
    settings::valid_font_size(configured).map_or(FONT_SIZE, |size| size as f32)
}

/// First family of a CSS-style stack that is actually installed here, so a
/// stack written for the Web UI still lands on a real face instead of silently
/// falling back to a proportional font the grid cannot align. The installed
/// spelling is what the text system is asked for. A configured stack that
/// resolves to nothing keeps the default and says so, rather than reporting a
/// font that is not what is drawn.
fn resolve_font_family(configured: &str, available: &[String]) -> SharedString {
    let resolved = configured
        .split(',')
        .map(|part| part.trim().trim_matches(['"', '\'']).trim())
        .filter(|name| !name.is_empty())
        .find_map(|name| {
            available
                .iter()
                .find(|font| font.eq_ignore_ascii_case(name))
                .cloned()
        });
    match resolved {
        Some(family) => SharedString::from(family),
        None => {
            if !configured.trim().is_empty() {
                tracing::warn!(
                    "Terminal font family '{}' is not installed; using {}",
                    configured,
                    MONO_FONT
                );
            }
            SharedString::from(MONO_FONT)
        }
    }
}

/// Scrollback depth the emulator keeps; unset or out-of-range means the
/// default.
fn resolve_scrollback(configured: u32) -> usize {
    settings::valid_scrollback(configured).map_or(DEFAULT_SCROLLBACK_LINES, |lines| lines as usize)
}

/// Shell command for the PTY: the configured command when set, else `$SHELL`.
fn resolve_shell(configured: &str) -> String {
    let configured = configured.trim();
    if configured.is_empty() {
        std::env::var("SHELL").unwrap_or_else(|_| DEFAULT_SHELL.to_string())
    } else {
        configured.to_string()
    }
}

impl TerminalPane {
    /// Creates the pane from the shared terminal settings: the font drives the
    /// grid metrics, the scrollback depth sizes the history, and a configured
    /// shell command replaces `$SHELL`. Preferences that only affect a starting
    /// pane are read once, here.
    pub(crate) fn new(
        tokio: tokio::runtime::Handle,
        settings: &TerminalSettings,
        start_dir: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let palette = TerminalPalette::from_theme(
            cx.theme().foreground,
            cx.theme().background,
            cx.theme().accent,
        );
        let font_size = resolve_font_size(settings.font_size);
        let font_family =
            resolve_font_family(&settings.font_family, &cx.text_system().all_font_names());
        let font_family_configured = settings.font_family.clone();
        let scrollback = resolve_scrollback(settings.scrollback);
        let executable = resolve_shell(&settings.default_shell);
        let service = Arc::new(PtyService::new(TerminalConfig::default()));
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<PtyMsg>();

        // Spawn the interactive shell, then pump raw output to the UI thread.
        let service_for_task = service.clone();
        tokio.spawn(async move {
            let mut env = HashMap::new();
            env.insert("COLORTERM".to_string(), "truecolor".to_string());
            let shell = ShellConfig {
                executable,
                args: vec!["-i".to_string()],
                env,
                cwd: start_dir,
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
            emulator: TerminalEmulator::new(80, 24, scrollback),
            palette,
            service,
            pty_id: None,
            tokio: tokio.clone(),
            focus,
            exited: false,
            error: None,
            title: None,
            font_size,
            font_family,
            font_family_configured,
            cols: 80,
            rows: 24,
            cell_w: px(7.),
            cell_h: px(line_height(font_size)),
            cursor_px: None,
            grid_bounds: None,
            selecting: false,
            needs_focus: false,
        }
    }

    /// Applies the font preferences to a running pane. The next paint pass
    /// remeasures the cell from the new font, which reflows the grid and the
    /// PTY through the ordinary resize path. Values the grid cannot render
    /// (out of range, family not installed) resolve to the pane defaults.
    pub(crate) fn apply_font(&mut self, settings: &TerminalSettings, cx: &mut Context<Self>) {
        let font_size = resolve_font_size(settings.font_size);
        let family_changed = self.font_family_configured != settings.font_family;
        if self.font_size == font_size && !family_changed {
            return;
        }
        self.font_size = font_size;
        if family_changed {
            self.font_family_configured = settings.font_family.clone();
            self.font_family =
                resolve_font_family(&settings.font_family, &cx.text_system().all_font_names());
        }
        cx.notify();
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
                // The panel may have reflowed the grid before the pty reported
                // its id (the first paint can beat `create_process`), in which
                // case `resize_if_needed` had no pty to talk to. Push the
                // current size so the child's winsize matches the grid instead
                // of staying at the 80x24 spawn default.
                let (cols, rows) = (self.cols, self.rows);
                let service = self.service.clone();
                let tokio = self.tokio.clone();
                tokio.spawn(async move {
                    if let Err(err) = service.resize(id, cols, rows).await {
                        tracing::warn!("Failed to apply the initial pty size: {}", err);
                    }
                });
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

    /// Input follows the live edge: typing, IME commits, and pastes snap the
    /// viewport back to the bottom when the user is scrolled into history
    /// (terminal convention; also keeps the cursor visible for the next paint).
    /// Returns `true` when the viewport moved.
    fn follow_input_to_bottom(&mut self) -> bool {
        if self.emulator.display_offset() > 0 {
            self.emulator.scroll_to_edge(true);
            return true;
        }
        false
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
        // Copy/paste follow the terminal conventions (alacritty,
        // gnome-terminal): Ctrl+Shift+C copies, Ctrl+Shift+V pastes. Plain
        // Ctrl+C / Ctrl+V keep their control bytes (SIGINT, quoted insert).
        if keystroke.modifiers.control && keystroke.modifiers.shift {
            match keystroke.key.as_str() {
                "c" => {
                    self.copy_selection(cx);
                    return;
                }
                "v" => {
                    self.paste_clipboard(cx);
                    return;
                }
                _ => {}
            }
        }
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
        if self.follow_input_to_bottom() {
            cx.notify();
        }
        self.write_bytes(bytes);
        cx.notify();
    }

    /// The grid cell under a window-space position, in grid coordinates
    /// (`line` negative = scrollback) plus which half of the cell the pointer
    /// covers (the backend uses it as the drag's outer edge).
    fn cell_point(&self, position: Point<Pixels>) -> Option<(i32, usize, Side)> {
        let bounds = self.grid_bounds?;
        Some(grid_point_at(
            bounds,
            self.cell_w,
            self.cell_h,
            self.emulator.cols(),
            self.emulator.rows(),
            self.emulator.display_offset(),
            position,
        ))
    }

    /// Ctrl+Shift+C: the selection goes to the system clipboard.
    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn selected_text(&self) -> Option<String> {
        if !self.emulator.has_selection() {
            return None;
        }
        self.emulator
            .selection_text()
            .filter(|text| !text.is_empty())
    }

    /// Ctrl+Shift+V: clipboard text with terminal line endings, framed as a
    /// bracketed paste when the running application asked for that mode.
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.paste_text(&text, cx);
    }

    fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.pty_id.is_none() || self.exited || text.is_empty() {
            return;
        }
        if self.follow_input_to_bottom() {
            cx.notify();
        }
        let bracketed = self.emulator.mode().contains(TermMode::BRACKETED_PASTE);
        self.write_bytes(paste_payload(text, bracketed).into_bytes());
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
        cx: &mut App,
    ) {
        self.pane.update(cx, |pane, cx| {
            if pane.follow_input_to_bottom() {
                cx.notify();
            }
            pane.write_bytes(new_text.as_bytes().to_vec());
        });
    }

    fn paste(&mut self, item: ClipboardItem, _window: &mut Window, cx: &mut App) {
        // Platform-initiated pastes (menus, middle click) take the same path
        // as Ctrl+Shift+V: line endings normalized, bracketed when active.
        if let Some(text) = item.text() {
            self.pane.update(cx, |pane, cx| pane.paste_text(&text, cx));
        }
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
                MouseButton::Left,
                cx.listener(|pane, event: &MouseDownEvent, window, cx| {
                    window.focus(&pane.focus, cx);
                    // A press starts a fresh selection; a bare click leaves it
                    // empty (nothing to copy), like other terminals.
                    if let Some((line, col, side)) = pane.cell_point(event.position) {
                        pane.emulator.begin_selection(line, col, side);
                        pane.selecting = true;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|pane, event: &MouseMoveEvent, _window, cx| {
                if !pane.selecting || event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                if let Some((line, col, side)) = pane.cell_point(event.position) {
                    pane.emulator.update_selection(line, col, side);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pane, event: &MouseUpEvent, _window, cx| {
                    if !pane.selecting {
                        return;
                    }
                    pane.selecting = false;
                    if let Some((line, col, side)) = pane.cell_point(event.position) {
                        pane.emulator.update_selection(line, col, side);
                    }
                    // Releasing a drag copies, so drag-select-then-paste needs
                    // no second gesture; Ctrl+Shift+C copies again explicitly.
                    pane.copy_selection(cx);
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
            .child(
                div().size_full().child(
                    canvas(
                        move |bounds, window, cx| {
                            // Measure the monospace cell from the pane's current
                            // typography, then reflow the grid to the panel's
                            // real size (window resizes and font changes both
                            // land here on every frame).
                            let (font_family, font_size) = {
                                let pane = entity.read(cx);
                                (pane.font_family.clone(), pane.font_size)
                            };
                            let font = gpui_kit::font(font_family);
                            let shaped = window.text_system().shape_line(
                                SharedString::from("M"),
                                px(font_size),
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
                                cell_h: px(line_height(font_size)),
                            };
                            let cols = ((bounds.size.width / metrics.cell_w).floor() as u16)
                                .clamp(MIN_COLS, 500);
                            let rows = ((bounds.size.height / metrics.cell_h).floor() as u16)
                                .clamp(MIN_ROWS, 200);
                            if std::env::var_os("OPENBITFUN_TERMINAL_TRACE").is_some() {
                                tracing::debug!(
                                    "terminal canvas bounds {:?} cell {:?} -> {}x{}",
                                    bounds.size,
                                    (metrics.cell_w, metrics.cell_h),
                                    cols,
                                    rows
                                );
                            }
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
                    )
                    // A bare `canvas` requests an auto layout with no
                    // children, so it would measure 0 tall and the grid
                    // would clamp to `MIN_ROWS` forever (reproduced
                    // 2026-10-09: 1180x0 bounds, `stty size` stuck at 6
                    // rows, terminal content overflowing the panel).
                    .size_full(),
                ),
            )
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
    let (
        cols,
        rows,
        cursor,
        display_offset,
        palette,
        font_family,
        font_size,
        row_spans_by_row,
        selection_spans,
    ) = {
        let state = entity.read(cx);
        let emulator = &state.emulator;
        let cols = emulator.cols();
        let rows = emulator.rows();
        let spans = (0..rows)
            .map(|row| row_spans(emulator.visible_line(row), cols))
            .collect::<Vec<_>>();
        let selection = emulator
            .has_selection()
            .then(|| emulator.selection_range())
            .flatten();
        let selection_spans = (0..rows)
            .map(|row| {
                selection.and_then(|range| {
                    let line = GridLine(row as i32 - emulator.display_offset() as i32);
                    selection_span(&range, line, emulator.visible_line(row), cols)
                })
            })
            .collect::<Vec<_>>();
        (
            cols,
            rows,
            emulator.cursor_point(),
            emulator.display_offset(),
            state.palette,
            state.font_family.clone(),
            state.font_size,
            spans,
            selection_spans,
        )
    };
    let selection_color = cx.theme().selection;

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

    let font = gpui_kit::font(font_family);
    for (row, spans) in row_spans_by_row.iter().enumerate() {
        // Walk spans right-to-left so each span knows how many grid columns
        // it occupies (wide characters advance two).
        let mut next_col = cols;
        let spans_with_cols = spans
            .iter()
            .rev()
            .map(|span| {
                let span_cols = (next_col - span.col).max(1);
                next_col = span.col;
                (span, span_cols)
            })
            .collect::<Vec<_>>();

        // Cell backgrounds first. Quads instead of TextRun background
        // decorations: shape_line's force_width path displaces glyphs of
        // decorated runs (gpui-pre 0.7.1, reproduced 2026-10-09), and
        // cell-aligned quads are what the grid actually wants. Invisible spans
        // (default blanks) skip the background; every visible span gets its
        // background painted even when the text is whitespace, so a colored
        // band from `printf '\e[41m   '` renders.
        for (span, span_cols) in spans_with_cols.iter().rev() {
            let dim = span.flags.contains(Flags::DIM);
            let inverse = span.flags.contains(Flags::INVERSE);
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
                        size: size(metrics.cell_w * *span_cols as f32, metrics.cell_h),
                    },
                    bg,
                ));
            }
        }

        // Selection highlight between backgrounds and text: content stays
        // readable, colored bands keep their own color underneath.
        if let Some(selection) = selection_spans[row].clone() {
            window.paint_quad(gpui_kit::fill(
                Bounds {
                    origin: cell_origin(bounds, selection.start as f32, row as f32, metrics),
                    size: size(metrics.cell_w * selection.len() as f32, metrics.cell_h),
                },
                selection_color,
            ));
        }

        for (span, _) in spans_with_cols.iter().rev() {
            let dim = span.flags.contains(Flags::DIM);
            let inverse = span.flags.contains(Flags::INVERSE);
            let visible = !(span.is_invisible() || span.text.is_empty());
            if !visible {
                continue;
            }
            let mut run_font = font.clone();
            if span.flags.contains(Flags::BOLD) {
                run_font.weight = FontWeight::BOLD;
            }
            if span.flags.contains(Flags::ITALIC) {
                run_font.style = FontStyle::Italic;
            }
            let fg = if inverse {
                palette.resolve(span.bg, false)
            } else {
                palette.resolve(span.fg, dim)
            };
            let underline = span
                .flags
                .contains(Flags::UNDERLINE)
                .then(|| UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                });
            // No `force_width`: it displaces decorated-run glyphs (see the
            // background comment); natural mono advance fills the cells.
            let shaped = window.text_system().shape_line(
                SharedString::from(span.text.clone()),
                px(font_size),
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
        pane.grid_bounds = Some(bounds);
    });
}

/// Grid cell under a window-space position: columns and rows clamp to the
/// grid so drags past an edge keep extending, then the viewport row converts
/// to the backend's grid line (`0` = live screen top, negative = history).
/// The returned side is which half of the cell the pointer covers.
fn grid_point_at(
    bounds: Bounds<Pixels>,
    cell_w: Pixels,
    cell_h: Pixels,
    cols: usize,
    rows: usize,
    display_offset: usize,
    position: Point<Pixels>,
) -> (i32, usize, Side) {
    let col_f = (position.x - bounds.origin.x) / cell_w;
    let row_f = (position.y - bounds.origin.y) / cell_h;
    let raw_col = col_f.floor();
    let col = raw_col.clamp(0., (cols.max(1) - 1) as f32) as usize;
    let row = row_f.floor().clamp(0., (rows.max(1) - 1) as f32) as i32;
    // The side is which half of the hit cell the pointer covers; pointer
    // positions outside the grid widen the drag to that edge instead.
    let side = if raw_col < 0. || raw_col >= cols as f32 {
        if raw_col < 0. {
            Side::Left
        } else {
            Side::Right
        }
    } else if col_f.fract() < 0.5 {
        Side::Left
    } else {
        Side::Right
    };
    (row - display_offset as i32, col, side)
}

/// Columns of one grid line covered by the selection, extended over a wide
/// character's trailing spacer cell so the highlight stays rectangular.
fn selection_span(
    range: &SelectionRange,
    line: GridLine,
    row: &[Cell],
    cols: usize,
) -> Option<Range<usize>> {
    if cols == 0 {
        return None;
    }
    let first = (0..cols).find(|&col| range.contains(GridPoint::new(line, GridColumn(col))))?;
    let last = (0..cols)
        .rev()
        .find(|&col| range.contains(GridPoint::new(line, GridColumn(col))))?;
    let mut end = last + 1;
    if end < cols && row[last].flags.contains(Flags::WIDE_CHAR) {
        end += 1;
    }
    Some(first..end)
}

/// Clipboard text prepared for the PTY: terminals execute on CR, so every
/// line-ending flavour normalizes to CR; when the running application enabled
/// bracketed paste the payload is framed so it is inserted, not executed.
/// Applied by both Ctrl+Shift+V and platform-initiated pastes.
fn paste_payload(text: &str, bracketed: bool) -> String {
    let mut payload = String::with_capacity(text.len() + 16);
    if bracketed {
        payload.push_str("\x1b[200~");
    }
    payload.push_str(&text.replace("\r\n", "\r").replace('\n', "\r"));
    if bracketed {
        payload.push_str("\x1b[201~");
    }
    payload
}

fn cell_origin(bounds: Bounds<Pixels>, col: f32, row: f32, metrics: &GridMetrics) -> Point<Pixels> {
    point(
        bounds.origin.x + col * metrics.cell_w,
        bounds.origin.y + row * metrics.cell_h,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        grid_point_at, key_to_bytes, line_height, paste_payload, resolve_font_family,
        resolve_font_size, resolve_scrollback, resolve_shell, selection_span, Side, FONT_SIZE,
        MONO_FONT,
    };
    use crate::terminal::emulator::{TerminalEmulator, DEFAULT_SCROLLBACK_LINES};
    use alacritty_terminal::index::{Line as GridLine, Side as TermSide};
    use gpui_kit::{point, px, size, Bounds};

    fn bounds() -> Bounds<gpui_kit::Pixels> {
        Bounds {
            origin: point(px(10.), px(20.)),
            size: size(px(70.), px(68.)),
        }
    }

    #[test]
    fn grid_point_at_maps_pixels_to_cells_with_clamping() {
        let cell_w = px(7.);
        let cell_h = px(17.);
        let bounds = bounds();
        let at =
            |x: f32, y: f32| grid_point_at(bounds, cell_w, cell_h, 10, 4, 0, point(px(x), px(y)));
        // Inside the first cell, left half.
        assert_eq!(at(12., 24.), (0, 0, Side::Left));
        // Third column (x = 10 + 2*7 + 6), right half.
        assert_eq!(at(30., 20.), (0, 2, Side::Right));
        // Past the right and bottom edges clamps to the last cell.
        assert_eq!(at(500., 500.), (3, 9, Side::Right));
        // Above and left of the content clamps to the first cell.
        assert_eq!(at(-50., -50.), (0, 0, Side::Left));
    }

    #[test]
    fn grid_point_at_converts_viewport_rows_to_history_lines() {
        let point = grid_point_at(
            bounds(),
            px(7.),
            px(17.),
            10,
            4,
            3,
            gpui_kit::point(px(10.), px(20.)),
        );
        assert_eq!(point, (-3, 0, Side::Left));
    }

    /// Selects a range on a 10x2 grid and returns the per-row paint spans.
    fn spans_for(
        selection: Option<((i32, usize), (i32, usize))>,
    ) -> Vec<Option<std::ops::Range<usize>>> {
        let mut emulator = TerminalEmulator::new(10, 2, DEFAULT_SCROLLBACK_LINES);
        emulator.advance("ab汉字cd".as_bytes());
        if let Some((from, to)) = selection {
            // The drag's outer edges carry the sides: anchor Left, head Right
            // (reversed for a backwards drag).
            let (from_side, to_side) = if from <= to {
                (TermSide::Left, TermSide::Right)
            } else {
                (TermSide::Right, TermSide::Left)
            };
            emulator.begin_selection(from.0, from.1, from_side);
            emulator.update_selection(to.0, to.1, to_side);
        }
        let range = emulator.selection_range();
        (0..2)
            .map(|row| {
                range.and_then(|range| {
                    selection_span(
                        &range,
                        GridLine(row as i32),
                        emulator.visible_line(row),
                        emulator.cols(),
                    )
                })
            })
            .collect()
    }

    #[test]
    fn selection_span_covers_selected_columns_per_row() {
        // Row content "ab汉字cd": selecting columns 1..=2 covers 'b' plus the
        // leading cell of the first wide glyph, and the highlight extends over
        // that glyph's spacer column.
        let spans = spans_for(Some(((0, 1), (0, 2))));
        assert_eq!(spans[0], Some(1..4));
        assert_eq!(spans[1], None, "rows outside the selection paint nothing");
        // Selecting a plain column range stops after the last selected cell.
        let spans = spans_for(Some(((0, 6), (0, 7))));
        assert_eq!(spans[0], Some(6..8));
    }

    #[test]
    fn selection_span_extends_over_a_wide_char_spacer() {
        // Columns 2 and 3 hold the first wide glyph; a selection ending on its
        // leading cell still highlights both columns.
        let spans = spans_for(Some(((0, 2), (0, 2))));
        assert_eq!(spans[0], Some(2..4));
    }

    #[test]
    fn selection_span_is_empty_without_a_selection() {
        assert_eq!(spans_for(None), vec![None, None]);
    }

    #[test]
    fn paste_payload_normalizes_line_endings() {
        assert_eq!(paste_payload("a\nb", false), "a\rb");
        assert_eq!(paste_payload("a\r\nb", false), "a\rb");
        assert_eq!(paste_payload("a\rb", false), "a\rb");
        assert_eq!(paste_payload("plain", false), "plain");
    }

    #[test]
    fn paste_payload_frames_bracketed_paste() {
        assert_eq!(paste_payload("a\nb", true), "\x1b[200~a\rb\x1b[201~");
        assert_eq!(paste_payload("x", false), "x");
    }

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

    #[test]
    fn terminal_settings_resolve_onto_renderable_values() {
        // Unset (zero) and out-of-range values mean "shell default"; the pane
        // keeps its own constants for those.
        assert_eq!(resolve_font_size(0), FONT_SIZE);
        assert_eq!(resolve_font_size(4), FONT_SIZE);
        assert_eq!(resolve_font_size(200), FONT_SIZE);
        assert_eq!(resolve_font_size(16), 16.);

        assert_eq!(
            resolve_scrollback(0),
            crate::terminal::emulator::DEFAULT_SCROLLBACK_LINES
        );
        assert_eq!(
            resolve_scrollback(5),
            crate::terminal::emulator::DEFAULT_SCROLLBACK_LINES
        );
        assert_eq!(resolve_scrollback(5000), 5000);

        assert_eq!(line_height(13.), 17.);
        assert_eq!(line_height(20.), 26.);
    }

    #[test]
    fn font_stack_picks_the_first_installed_family() {
        let installed = vec![
            "Noto Sans CJK SC".to_string(),
            "JetBrains Mono".to_string(),
            "DejaVu Sans Mono".to_string(),
        ];
        // Web UI style stacks carry quotes and a generic tail.
        assert_eq!(
            resolve_font_family("\"JetBrains Mono\", \"Fira Code\", monospace", &installed)
                .as_ref(),
            "JetBrains Mono"
        );
        // Case-insensitive match, first hit wins.
        assert_eq!(
            resolve_font_family("fira code, jetbrains mono", &installed).as_ref(),
            "JetBrains Mono"
        );
        // Nothing installed or nothing configured: the shell default, never a
        // proportional fallback the grid cannot align.
        assert_eq!(
            resolve_font_family("monospace", &installed).as_ref(),
            MONO_FONT
        );
        assert_eq!(resolve_font_family("", &installed).as_ref(), MONO_FONT);
        assert_eq!(resolve_font_family("  ", &installed).as_ref(), MONO_FONT);
        // The desktop Web UI default stack, on a machine with none of it.
        assert_eq!(
            resolve_font_family("Consolas, \"Courier New\", monospace", &installed).as_ref(),
            MONO_FONT
        );
    }

    #[test]
    fn shell_command_prefers_the_configured_value() {
        assert_eq!(resolve_shell(" /usr/bin/zsh "), "/usr/bin/zsh");
        let fallback = resolve_shell("");
        assert!(!fallback.is_empty());
        assert_eq!(resolve_shell("   "), fallback);
    }
}
