//! OpenBitFun GPUI desktop shell library: the window shell, embedded runtime
//! bridge, theming/i18n, and the built-in terminal. The binary target is a
//! thin bootstrap around [`shell::launch_main_window`].

// The insights generation future drags a long rmcp type chain through the
// Send check at the spawn boundary; the default 128 limit overflows.
#![recursion_limit = "256"]

pub mod assets;
pub mod cli;
pub mod diagnostics;
pub mod files;
pub mod git;
pub mod i18n;
pub mod insights;
pub mod instance;
pub mod manage;
pub mod runtime;
pub mod search;
pub mod settings;
pub mod shell;
pub mod tasks;
pub mod terminal;
pub mod theme;
pub mod theme_openbitfun;
pub mod tray;
pub mod update;
