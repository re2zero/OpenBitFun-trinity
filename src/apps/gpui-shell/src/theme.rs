//! CLI theme preset JSON to gpui-component theme mapping.
//!
//! Preset files stay owned by the CLI TUI (`src/apps/cli/themes/presets/`)
//! and are embedded here via `include_str!` so both surfaces read one source
//! of truth for the product palette. Only the semantic color keys the shell
//! renders today are mapped; diff/markdown/syntax tokens follow together
//! with the M4 renderers.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::highlighter::{HighlightThemeStyle, SyntaxColors};
use gpui_kit::component::theme::{Theme, ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::{App, SharedString};
use serde::Deserialize;

/// A parsed CLI theme preset ready to be applied to the global gpui theme.
#[derive(Debug, Clone)]
pub(crate) struct ThemePreset {
    pub id: &'static str,
    pub(crate) config: Rc<ThemeConfig>,
}

impl ThemePreset {
    pub(crate) fn apply(&self, cx: &mut App) {
        // 0.7.x applies a config by installing it as the mode's current theme
        // and reloading through `Theme::change`, which also refreshes the
        // Base-layer copy; `apply_config` no longer exists.
        let mode = self.config.mode;
        {
            let theme = Theme::global_mut(cx);
            if mode.is_dark() {
                theme.dark_theme = Rc::clone(&self.config);
            } else {
                theme.light_theme = Rc::clone(&self.config);
            }
        }
        Theme::change(mode, None, cx);
        cx.refresh_windows();
        tracing::info!("Theme preset applied: {}", self.id);
    }
}

/// Preset applied on the first frame: the design-system dark palette, with
/// the CLI presets below kept as the compatibility source.
pub(crate) const DEFAULT_PRESET_ID: &str = crate::theme_openbitfun::DARK_PRESET_ID;

/// App-global theme state: the parsed presets and the active selection. Lives
/// outside `ShellWindow` so button handlers, the settings page, and the
/// bootstrap can reach it with just `&mut App`.
pub(crate) struct ShellState {
    pub(crate) presets: Vec<ThemePreset>,
    pub(crate) active_preset: usize,
}

impl gpui_kit::Global for ShellState {}

/// Applies a preset by id. Returns `false` when no loaded preset carries that
/// id (the caller decides how to degrade).
pub(crate) fn apply_by_id(preset_id: &str, cx: &mut App) -> bool {
    let Some(index) = cx
        .global::<ShellState>()
        .presets
        .iter()
        .position(|preset| preset.id == preset_id)
    else {
        return false;
    };
    let preset = cx.global::<ShellState>().presets[index].clone();
    cx.global_mut::<ShellState>().active_preset = index;
    preset.apply(cx);
    true
}

static PRESET_FILES: &[(&str, &str)] = &[
    (
        "openbitfun-cyber",
        include_str!("../../cli/themes/presets/openbitfun-cyber.json"),
    ),
    (
        "openbitfun-dark",
        include_str!("../../cli/themes/presets/openbitfun-dark.json"),
    ),
    (
        "openbitfun-ink-night",
        include_str!("../../cli/themes/presets/openbitfun-ink-night.json"),
    ),
    (
        "openbitfun-light",
        include_str!("../../cli/themes/presets/openbitfun-light.json"),
    ),
    (
        "openbitfun-midnight",
        include_str!("../../cli/themes/presets/openbitfun-midnight.json"),
    ),
    (
        "openbitfun-tokyo-night",
        include_str!("../../cli/themes/presets/openbitfun-tokyo-night.json"),
    ),
];

/// Parses every palette shown in the shell: the design-system
/// `theme-openbitfun` tokens first (the product default), then the CLI
/// presets. Unparsable entries are skipped with a warning instead of failing
/// the shell startup.
pub(crate) fn load_presets() -> Vec<ThemePreset> {
    let mut presets = crate::theme_openbitfun::load();
    presets.extend(
        PRESET_FILES
            .iter()
            .filter_map(|(id, json)| match build_config(id, json) {
                Ok(config) => Some(ThemePreset {
                    id,
                    config: Rc::new(config),
                }),
                Err(err) => {
                    tracing::warn!("Skipping theme preset {}: {}", id, err);
                    None
                }
            }),
    );
    presets
}

#[derive(Debug, Deserialize)]
struct PresetFile {
    theme: HashMap<String, PresetColor>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PresetColor {
    Hex(String),
    Index(u8),
    Appearance {
        dark: Box<PresetColor>,
        light: Box<PresetColor>,
    },
}

fn resolve_color(value: &PresetColor, dark: bool) -> Option<String> {
    match value {
        PresetColor::Hex(hex) => Some(hex.clone()),
        // ANSI indexed colors have no meaning in a GPU shell.
        PresetColor::Index(index) => {
            tracing::debug!("Ignoring ANSI indexed color {} in theme preset", index);
            None
        }
        PresetColor::Appearance {
            dark: dark_value,
            light: light_value,
        } => resolve_color(if dark { dark_value } else { light_value }, dark),
    }
}

fn build_config(id: &'static str, json: &str) -> Result<ThemeConfig, String> {
    let file: PresetFile =
        serde_json::from_str(json).map_err(|err| format!("invalid preset JSON: {}", err))?;

    // Appearance follows the background luminance, matching how the CLI sorts
    // presets into light/dark groups. Shipped presets are flat hex maps; for a
    // hypothetical {dark, light} variant background the dark value decides.
    let background = resolved_color(&file, "background", true)
        .ok_or_else(|| "missing color \"background\"".to_string())?;
    let dark = relative_luminance(&background)
        .map(|lum| lum <= 0.5)
        .unwrap_or(true);

    let pick = |key: &str| -> Option<SharedString> {
        resolved_color(&file, key, dark).map(SharedString::from)
    };
    let required = |key: &str| -> Result<SharedString, String> {
        pick(key).ok_or_else(|| format!("missing color \"{}\"", key))
    };

    let primary = required("primary")?;
    let accent = required("accent")?;
    let danger = required("error")?;
    let warning = required("warning")?;
    let success = required("success")?;
    let info = required("info")?;
    let foreground = required("text")?;
    let background_panel = required("backgroundPanel")?;
    let border = required("border")?;
    let border_active = required("borderActive")?;

    // Struct literal syntax is not allowed outside the defining module
    // because ThemeConfigColors carries private base-color fields.
    let mut colors = ThemeConfigColors::default();
    colors.background = Some(SharedString::from(background));
    colors.foreground = Some(foreground.clone());
    colors.border = Some(border.clone());
    colors.primary = Some(primary.clone());
    colors.primary_foreground = Some(readable_foreground(&primary));
    colors.accent = Some(accent.clone());
    colors.accent_foreground = Some(readable_foreground(&accent));
    colors.danger = Some(danger.clone());
    colors.danger_foreground = Some(readable_foreground(&danger));
    colors.warning = Some(warning.clone());
    colors.warning_foreground = Some(readable_foreground(&warning));
    colors.success = Some(success.clone());
    colors.success_foreground = Some(readable_foreground(&success));
    colors.info = Some(info.clone());
    colors.info_foreground = Some(readable_foreground(&info));
    colors.muted_foreground = pick("textMuted");
    colors.muted = pick("backgroundElement");
    // R5 parity: the component falls back to a solid accent for a missing
    // sidebar selection surface, which is louder than the web nav's neutral
    // quiet-hover pill. Flat presets spend their own element surface on it,
    // with the preset text color on top.
    colors.sidebar_accent = pick("backgroundElement");
    colors.sidebar_accent_foreground = Some(foreground.clone());
    colors.popover = Some(background_panel.clone());
    colors.title_bar = Some(background_panel);
    colors.ring = Some(border_active);
    colors.input = Some(pick("borderSubtle").unwrap_or(border));
    colors.link = pick("markdownLink");

    Ok(ThemeConfig {
        name: id.into(),
        mode: if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        colors,
        // Syntax highlighting (M4 renderers): the markdown/diff presentation
        // keys stay on the CLI/owner surfaces — the shell's markdown view
        // derives its styles from the semantic colors above.
        highlight: build_highlight_style(&file, dark),
        ..Default::default()
    })
}

/// Maps the preset's `syntax*` color keys onto the highlighter style consumed
/// by the markdown/code renderer. `ThemeStyle` fields are private to the
/// component crate, so the style is built through its serde (Zed-compatible)
/// shape; unparsable presets degrade to the component defaults.
fn build_highlight_style(file: &PresetFile, dark: bool) -> Option<HighlightThemeStyle> {
    let token = |key: &str| -> serde_json::Value {
        match resolved_color(file, key, dark) {
            Some(hex) => serde_json::json!({ "color": hex }),
            None => serde_json::Value::Null,
        }
    };
    let syntax: SyntaxColors = serde_json::from_value(serde_json::json!({
        "comment": token("syntaxComment"),
        "keyword": token("syntaxKeyword"),
        "function": token("syntaxFunction"),
        "variable": token("syntaxVariable"),
        "string": token("syntaxString"),
        "number": token("syntaxNumber"),
        "type": token("syntaxType"),
        "operator": token("syntaxOperator"),
        "punctuation": token("syntaxPunctuation"),
    }))
    .ok()?;
    serde_json::from_value(serde_json::json!({
        "editor.background": resolved_color(file, "backgroundPanel", dark),
        "editor.foreground": resolved_color(file, "text", dark),
        "syntax": syntax,
    }))
    .ok()
}

fn resolved_color(file: &PresetFile, key: &str, dark: bool) -> Option<String> {
    file.theme
        .get(key)
        .and_then(|value| resolve_color(value, dark))
}

pub(crate) fn readable_foreground(hex: &str) -> SharedString {
    if relative_luminance(hex).unwrap_or(0.0) > 0.36 {
        "#000000".into()
    } else {
        "#ffffff".into()
    }
}

fn relative_luminance(hex: &str) -> Option<f64> {
    let (r, g, b) = parse_rgb(hex)?;
    fn channel(value: u8) -> f64 {
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    Some(0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b))
}

fn parse_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_str_opt(value: &Option<SharedString>) -> Option<&str> {
        value.as_ref().map(|s| s.as_ref())
    }

    #[test]
    fn all_builtin_presets_load() {
        let presets = load_presets();
        // Two theme-openbitfun design-system palettes plus the CLI presets.
        assert_eq!(presets.len(), PRESET_FILES.len() + 2);
        assert!(presets
            .first()
            .is_some_and(|preset| preset.id == crate::theme_openbitfun::DARK_PRESET_ID));
    }

    #[test]
    fn dark_preset_maps_core_colors() {
        let config = build_config(
            "test-dark",
            r##"{"theme": {
                "primary": "#60a5fa", "secondary": "#8b5cf6", "accent": "#8b5cf6",
                "error": "#ef4444", "warning": "#f59e0b", "success": "#34d399", "info": "#a1a1aa",
                "text": "#e7ecf3", "background": "#0e0e10", "backgroundPanel": "#1c1c1f",
                "backgroundElement": "#252528", "border": "#3b3b40", "borderActive": "#60a5fa"
            }}"##,
        )
        .expect("preset must parse");

        assert_eq!(config.mode, ThemeMode::Dark);
        assert_eq!(as_str_opt(&config.colors.background), Some("#0e0e10"));
        assert_eq!(as_str_opt(&config.colors.primary), Some("#60a5fa"));
        assert_eq!(as_str_opt(&config.colors.ring), Some("#60a5fa"));
        assert!(config.colors.muted_foreground.is_none());
    }

    #[test]
    fn light_background_resolves_to_light_mode() {
        let config = build_config(
            "test-light",
            r##"{"theme": {
                "primary": "#475569", "secondary": "#8b5cf6", "accent": "#8b5cf6",
                "error": "#ef4444", "warning": "#9a651f", "success": "#3f7d54", "info": "#a1a1aa",
                "text": "#1c1c1f", "background": "#f3f3f5", "backgroundPanel": "#ffffff",
                "border": "#d4d4d8", "borderActive": "#475569"
            }}"##,
        )
        .expect("preset must parse");

        assert_eq!(config.mode, ThemeMode::Light);
        assert_eq!(as_str_opt(&config.colors.title_bar), Some("#ffffff"));
    }

    #[test]
    fn missing_required_color_is_reported() {
        let err = build_config("test-broken", r##"{"theme": {"background": "#0e0e10"}}"##)
            .expect_err("incomplete preset must fail");
        assert!(err.contains("missing color"));
    }

    #[test]
    fn readable_foreground_follows_luminance() {
        assert_eq!(readable_foreground("#0e0e10"), "#ffffff");
        assert_eq!(readable_foreground("#f3f3f5"), "#000000");
    }

    #[test]
    fn appearance_variants_resolve_by_mode() {
        let value =
            serde_json::from_str::<PresetColor>(r##"{"dark": "#101010", "light": "#eeeeee"}"##)
                .expect("variant must parse");
        assert_eq!(resolve_color(&value, true).as_deref(), Some("#101010"));
        assert_eq!(resolve_color(&value, false).as_deref(), Some("#eeeeee"));
    }

    #[test]
    fn preset_syntax_keys_map_into_highlight_style() {
        let config = build_config(
            "test-syntax",
            r##"{"theme": {
                "primary": "#60a5fa", "secondary": "#8b5cf6", "accent": "#8b5cf6",
                "error": "#ef4444", "warning": "#f59e0b", "success": "#34d399", "info": "#a1a1aa",
                "text": "#e7ecf3", "background": "#0e0e10", "backgroundPanel": "#1c1c1f",
                "backgroundElement": "#252528", "border": "#3b3b40", "borderActive": "#60a5fa",
                "syntaxComment": "#6a737d", "syntaxKeyword": "#c792ea",
                "syntaxFunction": "#7dcfff", "syntaxVariable": "#7dcfff",
                "syntaxString": "#a5e844", "syntaxNumber": "#f78c6c",
                "syntaxType": "#ffc777", "syntaxOperator": "#c792ea",
                "syntaxPunctuation": "#e7ecf3"
            }}"##,
        )
        .expect("preset must parse");

        let highlight = config.highlight.expect("syntax keys must map");
        // ThemeStyle fields are private to the component crate; assert through
        // its serde shape (Hsla serializes with the alpha channel).
        let comment = serde_json::to_value(highlight.syntax.comment.expect("comment mapped"))
            .expect("comment serializes");
        assert_eq!(comment["color"], "#6a737dff");
        let keyword = serde_json::to_value(highlight.syntax.keyword.expect("keyword mapped"))
            .expect("keyword serializes");
        assert_eq!(keyword["color"], "#c792eaff");
        assert!(highlight.syntax.string.is_some());
    }

    #[test]
    fn preset_without_syntax_keys_has_no_highlight_style() {
        let config = build_config(
            "test-no-syntax",
            r##"{"theme": {
                "primary": "#60a5fa", "secondary": "#8b5cf6", "accent": "#8b5cf6",
                "error": "#ef4444", "warning": "#f59e0b", "success": "#34d399", "info": "#a1a1aa",
                "text": "#e7ecf3", "background": "#0e0e10", "backgroundPanel": "#1c1c1f",
                "backgroundElement": "#252528", "border": "#3b3b40", "borderActive": "#60a5fa"
            }}"##,
        )
        .expect("preset must parse");
        // Editor colors still map; only the syntax palette stays empty, which
        // keeps the component defaults for unstyled tokens.
        let highlight = config.highlight.expect("editor colors always map");
        assert!(highlight.syntax.keyword.is_none());
    }
}
