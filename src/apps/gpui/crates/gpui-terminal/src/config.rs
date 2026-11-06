//! Terminal-domain settings slice. Lives in the terminal crate (not the
//! settings page) because both the settings grid and the panes validate
//! against the same renderable domains; the values mirror the shared
//! document's `terminal` section.

use openbitfun_core::service::config::types::TerminalConfig;

pub const MIN_FONT_SIZE: u32 = 8;
pub const MAX_FONT_SIZE: u32 = 32;
pub const MIN_SCROLLBACK: u32 = 100;
pub const MAX_SCROLLBACK: u32 = 1_000_000;

/// Terminal section of the shell's in-memory settings mirror. Values outside
/// a renderable domain (or the empty string) mean "shell default" and are
/// applied at the pane, never rewritten in the config.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalSettings {
    pub font_size: u32,
    pub font_family: String,
    pub scrollback: u32,
    pub default_shell: String,
    /// G-T1: `terminal.terminal_panel_position` (`right` | `bottom`), the same
    /// shared key the web writes. Kept verbatim; the shell's dock resolves
    /// unknown values to its bottom dock.
    pub panel_position: String,
    /// Named launch profiles (M17); the shared document type carries them so
    /// the desktop terminal can adopt the same list later.
    pub profiles: Vec<openbitfun_core::service::config::types::TerminalProfile>,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        // Same defaults the shared document resolves to for a fresh install.
        let config = TerminalConfig::default();
        Self::from_global(&config)
    }
}

impl TerminalSettings {
    pub fn from_global(config: &TerminalConfig) -> Self {
        Self {
            font_size: config.font_size,
            font_family: config.font_family.clone(),
            scrollback: config.scrollback,
            default_shell: config.default_shell.clone(),
            panel_position: config.terminal_panel_position.clone(),
            profiles: config.profiles.clone(),
        }
    }
}

/// Font size a pane renders with, or `None` when the configured value is
/// outside the renderable range.
pub fn valid_font_size(value: u32) -> Option<u32> {
    (MIN_FONT_SIZE..=MAX_FONT_SIZE)
        .contains(&value)
        .then_some(value)
}

/// Scrollback depth a pane starts with, or `None` outside the domain.
pub fn valid_scrollback(value: u32) -> Option<u32> {
    (MIN_SCROLLBACK..=MAX_SCROLLBACK)
        .contains(&value)
        .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_domains_reject_what_the_grid_cannot_render() {
        assert_eq!(valid_font_size(14), Some(14));
        assert_eq!(valid_font_size(0), None);
        assert_eq!(valid_font_size(MIN_FONT_SIZE - 1), None);
        assert_eq!(valid_font_size(MAX_FONT_SIZE + 1), None);

        assert_eq!(valid_scrollback(1000), Some(1000));
        assert_eq!(valid_scrollback(0), None);
    }
}
