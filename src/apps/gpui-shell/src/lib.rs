//! OpenBitFun GPUI desktop shell library: the window shell, embedded runtime
//! bridge, theming/i18n, and the built-in terminal. The binary target is a
//! thin bootstrap around [`shell::launch_main_window`].

pub mod assets;
pub mod cli;
pub mod git;
pub mod i18n;
pub mod runtime;
pub mod settings;
pub mod shell;
pub mod terminal;
pub mod theme;
pub mod theme_openbitfun;
