//! `theme-openbitfun` design-system semantic tokens to gpui_kit theme mapping.
//!
//! The authoring sources live in the design-system workspace
//! (`design-system/packages/theme-openbitfun/src/*.tokens.json`, style-dictionary
//! `$value` format) and are embedded here via `include_str!` so the shell reads
//! the same palette the web UI ships, with no generated snapshot in between.
//! Token expressions (`{ref.*}` lookups, `rgba()`, `color-mix(in srgb, ...)`)
//! are evaluated to `#rrggbbaa` because gpui's color parser accepts hex only.
//! CLI presets stay available as the compatibility source (`theme.rs`).

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::highlighter::{HighlightThemeStyle, SyntaxColors};
use gpui_kit::component::theme::{ThemeConfig, ThemeConfigColors, ThemeMode};
use gpui_kit::SharedString;

use crate::theme::{readable_foreground, ThemePreset};

const REFERENCE_TOKENS: &str =
    include_str!("../../../../design-system/packages/theme-openbitfun/src/reference.tokens.json");
const LIGHT_TOKENS: &str =
    include_str!("../../../../design-system/packages/theme-openbitfun/src/light.tokens.json");
const DARK_TOKENS: &str =
    include_str!("../../../../design-system/packages/theme-openbitfun/src/dark.tokens.json");

/// Shell theme ids for the design-system palettes; `theme.rs` chains these
/// ahead of the CLI compatibility presets.
pub(crate) const DARK_PRESET_ID: &str = "theme-openbitfun-dark";
pub(crate) const LIGHT_PRESET_ID: &str = "theme-openbitfun-light";

/// Builds both design-system presets; a failure degrades to skipping that
/// palette with a warning, mirroring the CLI preset loader.
pub(crate) fn load() -> Vec<ThemePreset> {
    [
        (DARK_PRESET_ID, DARK_TOKENS, ThemeMode::Dark),
        (LIGHT_PRESET_ID, LIGHT_TOKENS, ThemeMode::Light),
    ]
    .into_iter()
    .filter_map(|(id, json, mode)| match build_config(id, mode, json) {
        Ok(config) => Some(ThemePreset {
            id,
            config: Rc::new(config),
        }),
        Err(err) => {
            tracing::warn!("Skipping theme-openbitfun palette {}: {}", id, err);
            None
        }
    })
    .collect()
}

/// Flattened token table: dotted path → raw `$value` expression. Reference
/// documents nest under their root group (`ref.color.*`), theme documents
/// under `color.*`; every non-`$` root key becomes a path prefix.
fn token_table(reference_json: &str, theme_json: &str) -> Result<HashMap<String, String>, String> {
    fn flatten(json: &str, out: &mut HashMap<String, String>) -> Result<(), String> {
        let doc: serde_json::Value =
            serde_json::from_str(json).map_err(|err| format!("invalid token JSON: {}", err))?;
        walk(&doc, &mut vec![], out);
        Ok(())
    }

    fn walk(node: &serde_json::Value, path: &mut Vec<String>, out: &mut HashMap<String, String>) {
        match node {
            serde_json::Value::Object(map) => {
                // Leaf token: `{ "$type": ..., "$value": "#hex" }`
                if let Some(serde_json::Value::String(text)) = map.get("$value") {
                    out.insert(path.join("."), text.clone());
                    return;
                }
                for (key, value) in map {
                    if key.starts_with('$') {
                        continue;
                    }
                    path.push(key.clone());
                    walk(value, path, out);
                    path.pop();
                }
            }
            serde_json::Value::String(text) => {
                out.insert(path.join("."), text.clone());
            }
            _ => {}
        }
    }

    let mut table = HashMap::new();
    flatten(reference_json, &mut table)?;
    flatten(theme_json, &mut table)?;
    Ok(table)
}

/// Evaluates one token expression to rgba components (0.0-1.0).
fn evaluate(value: &str, table: &HashMap<String, String>, depth: u8) -> Result<[f32; 4], String> {
    if depth > 8 {
        return Err("token reference chain too deep".to_string());
    }
    let value = value.trim();

    if let Some(reference) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
        let referenced = table
            .get(reference.trim())
            .ok_or_else(|| format!("unresolved token reference \"{}\"", reference.trim()))?;
        return evaluate(referenced, table, depth + 1);
    }

    if value == "transparent" {
        return Ok([0.0, 0.0, 0.0, 0.0]);
    }

    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex(hex);
    }

    if let Some(body) = value
        .strip_prefix("rgba(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let parts: Vec<&str> = body.split(',').collect();
        if parts.len() != 4 {
            return Err(format!("unsupported rgba() expression \"{}\"", value));
        }
        let number = |raw: &str| -> Result<f32, String> {
            raw.trim()
                .parse::<f32>()
                .map_err(|err| format!("unsupported rgba() component \"{}\": {}", raw.trim(), err))
        };
        return Ok([
            number(parts[0])? / 255.0,
            number(parts[1])? / 255.0,
            number(parts[2])? / 255.0,
            number(parts[3])?,
        ]);
    }

    if let Some(body) = value
        .strip_prefix("color-mix(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let body = body
            .trim()
            .strip_prefix("in srgb,")
            .ok_or_else(|| format!("unsupported color-mix space in \"{}\"", value))?;
        let mut parts = body.split(',');
        let weighted = parts.next().unwrap_or_default().trim();
        let second = parts.next().unwrap_or_default().trim();
        // The token sources always weight the first color (`{token} 40%`);
        // a second-color weight has no upstream producer.
        let (first, weight) = weighted
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("unsupported color-mix weights in \"{}\"", value))?;
        let weight: f32 = weight
            .trim()
            .trim_end_matches('%')
            .parse::<f32>()
            .map_err(|err| format!("unsupported color-mix weight in \"{}\": {}", value, err))?
            / 100.0;
        let a = evaluate(first, table, depth + 1)?;
        let b = evaluate(second, table, depth + 1)?;
        // srgb mixing is premultiplied per the CSS spec; `transparent` mixes
        // as zero-premultiplied so only the alpha scales.
        let mixed_r = a[0] * a[3] * weight + b[0] * b[3] * (1.0 - weight);
        let mixed_g = a[1] * a[3] * weight + b[1] * b[3] * (1.0 - weight);
        let mixed_b = a[2] * a[3] * weight + b[2] * b[3] * (1.0 - weight);
        let alpha = a[3] * weight + b[3] * (1.0 - weight);
        if alpha <= 0.0 {
            return Ok([0.0, 0.0, 0.0, 0.0]);
        }
        return Ok([mixed_r / alpha, mixed_g / alpha, mixed_b / alpha, alpha]);
    }

    Err(format!("unsupported token expression \"{}\"", value))
}

fn parse_hex(hex: &str) -> Result<[f32; 4], String> {
    fn channel(raw: &str) -> Result<f32, String> {
        u8::from_str_radix(raw, 16)
            .map(|value| f32::from(value) / 255.0)
            .map_err(|err| format!("invalid hex color \"{}\": {}", raw, err))
    }
    let components: Vec<f32> = match hex.len() {
        3 | 4 => hex
            .chars()
            .map(|c| {
                let mut raw = String::new();
                raw.push(c);
                raw.push(c);
                channel(&raw)
            })
            .collect::<Result<Vec<_>, _>>()?,
        6 | 8 => (0..hex.len())
            .step_by(2)
            .map(|at| channel(&hex[at..at + 2]))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(format!("unsupported hex color \"#{}\"", hex)),
    };
    let alpha = components.get(3).copied().unwrap_or(1.0);
    Ok([components[0], components[1], components[2], alpha])
}

fn to_hex8(components: [f32; 4]) -> String {
    let channel = |value: f32| format!("{:02x}", (value * 255.0).round().clamp(0.0, 255.0) as u8);
    format!(
        "#{}{}{}{}",
        channel(components[0]),
        channel(components[1]),
        channel(components[2]),
        channel(components[3])
    )
}

fn build_config(
    id: &'static str,
    mode: ThemeMode,
    theme_json: &str,
) -> Result<ThemeConfig, String> {
    let table = token_table(REFERENCE_TOKENS, theme_json)?;
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut resolve = move |key: &str| -> Result<String, String> {
        if let Some(hit) = cache.get(key) {
            return Ok(hit.clone());
        }
        let raw = table
            .get(key)
            .ok_or_else(|| format!("missing color \"{}\"", key))?;
        let hex = to_hex8(evaluate(raw, &table, 0)?);
        cache.insert(key.to_string(), hex.clone());
        Ok(hex)
    };

    let mut status = |key: &str| -> Result<(SharedString, SharedString), String> {
        let hex = resolve(key)?;
        Ok((hex.clone().into(), readable_foreground(&hex)))
    };

    let (danger, danger_foreground) = status("color.status.danger.emphasis")?;
    let (warning, warning_foreground) = status("color.status.warning.emphasis")?;
    let (success, success_foreground) = status("color.status.success.emphasis")?;
    let (info, info_foreground) = status("color.status.info.emphasis")?;

    let foreground = resolve("color.content.primary")?;

    // Struct literal syntax is not allowed outside the defining module
    // because ThemeConfigColors carries private base-color fields.
    let mut colors = ThemeConfigColors::default();
    colors.background = Some(resolve("color.surface.canvas")?.into());
    colors.foreground = Some(foreground.clone().into());
    colors.border = Some(resolve("color.border.default")?.into());
    colors.primary = Some(resolve("color.action.primary.background")?.into());
    colors.primary_foreground = Some(resolve("color.action.primary.content")?.into());
    colors.primary_hover = Some(resolve("color.action.primary.hover")?.into());
    colors.primary_active = Some(resolve("color.action.primary.pressed")?.into());
    colors.secondary = Some(resolve("color.action.secondary.background")?.into());
    colors.secondary_foreground = Some(resolve("color.action.secondary.content")?.into());
    colors.secondary_hover = Some(resolve("color.action.secondary.hover")?.into());
    colors.secondary_active = Some(resolve("color.action.secondary.pressed")?.into());
    colors.accent = Some(resolve("color.accent.default")?.into());
    colors.accent_foreground = Some(resolve("color.content.onDark")?.into());
    colors.danger = Some(danger);
    colors.danger_foreground = Some(danger_foreground);
    colors.warning = Some(warning);
    colors.warning_foreground = Some(warning_foreground);
    colors.success = Some(success);
    colors.success_foreground = Some(success_foreground);
    colors.info = Some(info);
    colors.info_foreground = Some(info_foreground);
    colors.muted = Some(resolve("color.action.neutral.surface")?.into());
    colors.muted_foreground = Some(resolve("color.content.muted")?.into());
    colors.popover = Some(resolve("color.surface.panel")?.into());
    colors.popover_foreground = Some(foreground.into());
    colors.title_bar = Some(resolve("color.surface.chrome")?.into());
    colors.title_bar_border = Some(resolve("color.border.subtle")?.into());
    colors.status_bar = Some(resolve("color.surface.canvas")?.into());
    colors.status_bar_border = Some(resolve("color.border.subtle")?.into());
    colors.input = Some(resolve("color.field.border")?.into());
    colors.selection = Some(resolve("color.selection.surface")?.into());
    colors.ring = Some(resolve("color.focus.ring")?.into());
    colors.link = Some(resolve("color.link.default")?.into());
    colors.link_hover = Some(resolve("color.link.hover")?.into());
    colors.overlay = Some(resolve("color.overlay.scrim")?.into());
    colors.sidebar = Some(resolve("color.surface.panel")?.into());
    colors.sidebar_border = Some(resolve("color.border.subtle")?.into());
    colors.sidebar_foreground = Some(resolve("color.content.primary")?.into());
    colors.sidebar_accent = Some(resolve("color.accent.surface")?.into());
    colors.sidebar_accent_foreground = Some(resolve("color.accent.default")?.into());
    colors.scrollbar_thumb = Some(resolve("color.scrollbar.thumb")?.into());
    colors.scrollbar_thumb_hover = Some(resolve("color.scrollbar.thumbHover")?.into());
    colors.window_border = Some(resolve("color.border.subtle")?.into());

    Ok(ThemeConfig {
        name: id.into(),
        mode,
        colors,
        highlight: build_highlight_style(&mut resolve),
        ..Default::default()
    })
}

/// Maps `color.codeDiff.syntax.*` onto the highlighter style consumed by the
/// markdown/code renderer, through the component's serde (Zed-compatible)
/// shape; tokens without a slot (`property`) and missing entries fall back to
/// the component defaults via `null`.
fn build_highlight_style(
    resolve: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Option<HighlightThemeStyle> {
    let mut token = |key: &str| -> serde_json::Value {
        match resolve(&format!("color.codeDiff.syntax.{}", key)) {
            Ok(hex) => serde_json::json!({ "color": hex }),
            Err(_) => serde_json::Value::Null,
        }
    };
    let syntax: SyntaxColors = serde_json::from_value(serde_json::json!({
        "comment": token("comment"),
        "keyword": token("keyword"),
        "function": token("function"),
        "variable": token("variable"),
        "string": token("string"),
        "number": token("number"),
        "type": token("declaration"),
        "operator": token("operator"),
        "punctuation": token("punctuation"),
    }))
    .ok()?;
    serde_json::from_value(serde_json::json!({
        "editor.background": resolve("color.surface.scene").ok(),
        "editor.foreground": resolve("color.content.primary").ok(),
        "syntax": syntax,
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark_table() -> HashMap<String, String> {
        token_table(REFERENCE_TOKENS, DARK_TOKENS).expect("dark tokens flatten")
    }

    #[test]
    fn rgba_and_color_mix_evaluate_to_hex8() {
        let table = dark_table();
        // rgba(255, 255, 255, 0.18)
        assert_eq!(
            to_hex8(evaluate(&table["color.border.default"], &table, 0).unwrap()),
            "#ffffff2e"
        );
        // color-mix(in srgb, #60a5fa 40%, transparent)
        assert_eq!(
            to_hex8(evaluate(&table["color.accent.border"], &table, 0).unwrap()),
            "#60a5fa66"
        );
    }

    #[test]
    fn references_resolve_transitively() {
        let table = dark_table();
        // color.surface.canvas → {ref.color.neutral.950} → #0e0e10
        assert_eq!(
            to_hex8(evaluate(&table["color.surface.canvas"], &table, 0).unwrap()),
            "#0e0e10ff"
        );
        assert_eq!(
            to_hex8(evaluate(&table["color.accent.default"], &table, 0).unwrap()),
            "#60a5faff"
        );
    }

    #[test]
    fn unresolved_reference_is_reported() {
        let err = evaluate("{color.does.not.exist}", &dark_table(), 0)
            .expect_err("missing reference must fail");
        assert!(err.contains("color.does.not.exist"));
    }

    #[test]
    fn dark_palette_maps_core_colors() {
        let config = build_config("test", ThemeMode::Dark, DARK_TOKENS).expect("dark config");
        assert_eq!(config.mode, ThemeMode::Dark);
        assert_eq!(config.colors.background.as_deref(), Some("#0e0e10ff"));
        assert_eq!(config.colors.accent.as_deref(), Some("#60a5faff"));
        // content.primary → {ref.color.neutral.200}
        assert_eq!(config.colors.foreground.as_deref(), Some("#e8e8e8ff"));
        // Status content stays readable on the dark canvas.
        assert_eq!(config.colors.danger_foreground.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn light_palette_switches_mode_and_anchors() {
        let config = build_config("test", ThemeMode::Light, LIGHT_TOKENS).expect("light config");
        assert_eq!(config.mode, ThemeMode::Light);
        // surface.panel → {ref.color.neutral.0} = white; light content.primary
        // is a translucent black.
        assert_eq!(config.colors.popover.as_deref(), Some("#ffffffff"));
        assert_eq!(config.colors.foreground.as_deref(), Some("#000000cc"));
    }

    #[test]
    fn syntax_tokens_map_into_highlight_style() {
        let config = build_config("test", ThemeMode::Dark, DARK_TOKENS).expect("dark config");
        let highlight = config.highlight.expect("syntax palette maps");
        let keyword = serde_json::to_value(highlight.syntax.keyword.expect("keyword mapped"))
            .expect("keyword serializes");
        // codeDiff.syntax.keyword → {ref.color.pink.450} = #ff678d
        assert_eq!(keyword["color"], "#ff678dff");
    }
}
