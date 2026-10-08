//! CLI theme preset JSON to gpui-component theme mapping.
//!
//! Preset files stay owned by the CLI TUI (`src/apps/cli/themes/presets/`)
//! and are embedded here via `include_str!` so both surfaces read one source
//! of truth for the product palette. Only the semantic color keys the shell
//! renders today are mapped; diff/markdown/syntax tokens follow together
//! with the M4 renderers.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{App, SharedString};
use gpui_component::theme::{Theme, ThemeConfig, ThemeConfigColors, ThemeMode};
use serde::Deserialize;

/// A parsed CLI theme preset ready to be applied to the global gpui theme.
#[derive(Debug, Clone)]
pub(crate) struct ThemePreset {
    pub id: &'static str,
    pub label: &'static str,
    config: Rc<ThemeConfig>,
}

impl ThemePreset {
    pub(crate) fn apply(&self, cx: &mut App) {
        Theme::global_mut(cx).apply_config(&self.config);
        cx.refresh_windows();
        tracing::info!("Theme preset applied: {}", self.id);
    }
}

/// Preset applied on the first frame, matching the CLI default palette.
pub(crate) const DEFAULT_PRESET_ID: &str = "openbitfun-dark";

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

/// Parses every embedded preset; unparsable presets are skipped with a warning
/// instead of failing the shell startup.
pub(crate) fn load_presets() -> Vec<ThemePreset> {
    PRESET_FILES
        .iter()
        .filter_map(|(id, json)| match build_config(id, json) {
            Ok(config) => Some(ThemePreset {
                id,
                label: id.strip_prefix("openbitfun-").unwrap_or(id),
                config: Rc::new(config),
            }),
            Err(err) => {
                tracing::warn!("Skipping theme preset {}: {}", id, err);
                None
            }
        })
        .collect()
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
    colors.foreground = Some(foreground);
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
        ..Default::default()
    })
}

fn resolved_color(file: &PresetFile, key: &str, dark: bool) -> Option<String> {
    file.theme
        .get(key)
        .and_then(|value| resolve_color(value, dark))
}

fn readable_foreground(hex: &str) -> SharedString {
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
        assert_eq!(presets.len(), PRESET_FILES.len());
        for preset in &presets {
            assert!(preset.label.len() < preset.id.len());
        }
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
}
