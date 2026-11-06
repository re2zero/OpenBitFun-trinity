//! Main window shell of the OpenBitFun GPUI desktop. Everything rendered
//! inside [`shell::ShellWindow`] lives here — window chrome, session views,
//! the task board, the in-window pages (manage, settings cards, ACP, search,
//! insights), the settings page, the workspace pages (git changes, files
//! viewer, markdown hooks), sleep prevention, the embedded runtime bridge, and
//! the system tray — because they share the shell entity, its event pump, and
//! the runtime settings bridge.

// The insights generation future drags a long rmcp type chain through the
// Send check at the spawn boundary; the default 128 limit overflows.
#![recursion_limit = "256"]

pub mod acp;
pub mod files;
pub mod git;
pub mod insights;
pub mod manage;
pub mod markdown;
pub mod plugin_ui;
pub mod runtime;
pub mod search;
pub mod settings;
pub mod settings_cards;
pub mod shell;
pub mod sleep_prevention;
pub mod tabs;
pub mod tasks;
pub mod transcript;
pub mod tray;
