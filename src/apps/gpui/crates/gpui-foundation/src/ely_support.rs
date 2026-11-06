//! Ely component library bridge (the gpui-ely redesign era).
//!
//! Ely renders the redesigned card surfaces while gpui-kit keeps supplying the
//! interactive controls. The two libraries each own a theme global, so this
//! module composes their asset sources, bootstraps Ely next to
//! `gpui_kit::init`, and projects the active gpui-kit theme colors onto Ely's
//! palette at every preset change. Design decisions: `.relay/design/gpui-ely/README.md`.

use gpui_kit::component::theme::ThemeMode;
use gpui_kit::component::ActiveTheme;
use gpui_kit::App;

use crate::assets::ShellAssets;

/// Ely's bundled files (fonts) served ahead of the shell's own source.
pub(crate) type ShellAssetSource = ely_core::Before<ShellAssets>;

/// Composes the window asset source: Ely first, then the shell's icon set.
pub fn compose_assets(shell: ShellAssets) -> ShellAssetSource {
    ely_core::Assets::before(shell)
}

/// Bootstraps Ely (embedded fonts, theme global) after `gpui_kit::init`. A
/// failure keeps the shell on gpui-kit alone; Ely-backed surfaces must not
/// render against a missing palette, so they gate on the theme global being
/// present.
pub fn init(cx: &mut App) {
    if let Err(error) = ely_core::init(cx) {
        tracing::error!("Ely bootstrap failed, Ely surfaces stay disabled: {error:#}");
    }
}

/// Projects the active gpui-kit theme onto Ely's palette for `mode`. Unmapped
/// roles keep the Ely built-in values so a partial token set cannot blank a
/// surface. No-op when Ely's bootstrap did not complete.
pub fn sync_palette(mode: ThemeMode, cx: &mut App) {
    if !cx.has_global::<ely_core::theme::Theme>() {
        return;
    }
    let dark = mode.is_dark();
    // Theme derefs to the Copy `ThemeColor` set; copy it so the shared borrow
    // ends before `set_palette` takes the mutable one below.
    let theme: gpui_kit::component::theme::ThemeColor = **cx.theme();
    let mut palette = if dark {
        ely_core::theme::Palette::dark(false)
    } else {
        ely_core::theme::Palette::light(false)
    };
    palette.bg = theme.background;
    palette.surface = theme.group_box;
    palette.overlay = theme.popover;
    palette.hover = theme.list_hover;
    palette.active = theme.list_active;
    palette.border = theme.border;
    palette.border_strong = theme.title_bar_border;
    palette.fg = theme.foreground;
    palette.fg_muted = theme.muted_foreground;
    palette.fg_subtle = theme.muted_foreground;
    palette.accent = theme.primary;
    palette.accent_hover = theme.primary_hover;
    palette.on_accent = theme.primary_foreground;
    palette.focus = theme.ring;
    palette.link = theme.link;
    palette.selection = theme.selection;
    palette.success = theme.success;
    palette.warning = theme.warning;
    palette.danger = theme.danger;
    palette.info = theme.info;
    let ely_mode = if dark {
        ely_core::theme::Mode::Dark
    } else {
        ely_core::theme::Mode::Light
    };
    ely_core::theme::Theme::set_palette(ely_mode, Some(palette), cx);
}
