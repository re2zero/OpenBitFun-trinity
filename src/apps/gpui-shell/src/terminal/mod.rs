//! Built-in terminal: VT emulation backend for the shell's terminal pane.
//!
//! The emulator wraps `alacritty_terminal`'s screen model (grid, scrollback,
//! alternate screen) fed by the PTY byte stream from `terminal-core`; the
//! rendering side converts grid rows into styled spans for the gpui paint
//! path (see `examples/terminal_spike.rs` for the minimal wired window).

pub mod colors;
pub mod emulator;
pub mod row_runs;
