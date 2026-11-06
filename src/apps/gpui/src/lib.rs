//! OpenBitFun GPUI desktop shell. The former single crate was split into
//! per-concern crates under `crates/` so no single rustc process has to carry
//! the whole product's compile; this target is the thin library facade the
//! binary, integration tests, and examples consume. Every historical
//! `openbitfun_gpui::<module>` path keeps working through the re-exports
//! below.

pub use gpui_foundation::{assets, ely_support, i18n, subagent_identity, theme, theme_openbitfun};
pub use gpui_host::{cli, diagnostics, instance, storage, update};
pub use gpui_shell::{
    acp, files, git, insights, manage, markdown, plugin_ui, runtime, search, settings,
    settings_cards, shell, sleep_prevention, tabs, tasks, transcript, tray,
};
pub use gpui_terminal::terminal;
