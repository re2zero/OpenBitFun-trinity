//! Shell settings page: the shared-configuration slice the shell exposes, the
//! product-appearance to theme-preset mapping, and the persistence bridge back
//! into the process-global `ConfigService`.
//!
//! Values are read from and written to `GlobalConfig` (the same `app.json` the
//! CLI and the desktop app use), so a change made here is what the other
//! surfaces read. Reads never assume a field exists: a missing or unknown value
//! falls back to the shell default with an English warning instead of failing
//! the page.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme, StyledExt};
use gpui_kit::{
    div, prelude::*, Anchor, App, Global, IntoElement, SharedString, Window, WindowAppearance,
};
use openbitfun_core::service::config::get_global_config_service;
use openbitfun_core::service::config::types::GlobalConfig;
use openbitfun_core::service::i18n::LocaleId;
use openbitfun_runtime_ports::PermissionPolicyPreset;

use crate::i18n::ShellText;
use crate::theme;
use crate::theme_openbitfun;

/// Dot-paths of the shared-config entries this page owns. They are the same
/// paths the desktop app and the Web UI read and write.
pub(crate) const LANGUAGE_PATH: &str = "app.language";
pub(crate) const APPEARANCE_PATH: &str = "appearance.selection";
pub(crate) const PERMISSION_PRESET_PATH: &str = "tool_permissions.policy.preset";
pub(crate) const AUTO_APPROVE_ASK_PATH: &str = "tool_permissions.interaction.auto_approve_ask";
pub(crate) const TERMINAL_FONT_SIZE_PATH: &str = "terminal.font_size";
pub(crate) const TERMINAL_FONT_FAMILY_PATH: &str = "terminal.font_family";
pub(crate) const TERMINAL_SCROLLBACK_PATH: &str = "terminal.scrollback";
pub(crate) const TERMINAL_DEFAULT_SHELL_PATH: &str = "terminal.default_shell";
/// Default model new sessions start on (same path the desktop models page
/// writes).
pub(crate) const DEFAULT_MODEL_PATH: &str = "ai.default_models.primary";

/// Terminal preferences the built-in terminal consumes. Mirrors the shared
/// `terminal.*` section the desktop terminal reads, in the same value domain:
/// values outside it (or the empty string) mean "shell default" and are applied
/// at the pane, never rewritten in the config.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TerminalSettings {
    pub(crate) font_size: u32,
    pub(crate) font_family: String,
    pub(crate) scrollback: u32,
    pub(crate) default_shell: String,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        // Same defaults the shared document resolves to for a fresh install.
        let config = openbitfun_core::service::config::types::TerminalConfig::default();
        Self::from_global(&config)
    }
}

impl TerminalSettings {
    fn from_global(config: &openbitfun_core::service::config::types::TerminalConfig) -> Self {
        Self {
            font_size: config.font_size,
            font_family: config.font_family.clone(),
            scrollback: config.scrollback,
            default_shell: config.default_shell.clone(),
        }
    }
}

/// The shell's in-memory mirror of the settings it can change. Kept as a gpui
/// global so the settings page, the window bootstrap, and tests can read it
/// synchronously; the on-disk truth stays `GlobalConfig`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShellConfig {
    pub(crate) language: LocaleId,
    /// Raw `appearance.selection` value. Unknown ids are kept verbatim so the
    /// page shows what is actually persisted instead of a wrong selection.
    pub(crate) appearance: String,
    pub(crate) permission_preset: PermissionPolicyPreset,
    pub(crate) auto_approve_ask: bool,
    pub(crate) terminal: TerminalSettings,
    /// `ai.default_models.primary`: the model new sessions start on. `None`
    /// means the runtime picks the first enabled model.
    pub(crate) default_primary_model: Option<String>,
}

impl Global for ShellConfig {}

impl ShellConfig {
    /// Snapshot of the shared configuration read at bootstrap.
    pub(crate) fn from_global(config: &GlobalConfig) -> Self {
        Self {
            language: LocaleId::from_str(&config.app.language).unwrap_or(LocaleId::ZhCN),
            appearance: config.appearance.selection.clone(),
            permission_preset: config.tool_permissions.policy.preset,
            auto_approve_ask: config.tool_permissions.interaction.auto_approve_ask,
            terminal: TerminalSettings::from_global(&config.terminal),
            default_primary_model: config.ai.default_models.primary.clone(),
        }
    }

    pub(crate) fn current(cx: &App) -> Self {
        cx.global::<ShellConfig>().clone()
    }

    /// The same snapshot, or `None` while the shared config has not been read
    /// yet (the terminal pane, for one, is created before that in tests).
    pub(crate) fn try_current(cx: &App) -> Option<Self> {
        cx.try_global::<ShellConfig>().cloned()
    }

    /// Mirror installed before the shared config has been read: the shell
    /// defaults, matching the theme applied on the first frame.
    pub(crate) fn startup_default(locale: LocaleId) -> Self {
        Self {
            language: locale,
            appearance: DEFAULT_APPEARANCE_ID.to_string(),
            permission_preset: PermissionPolicyPreset::default(),
            auto_approve_ask: false,
            terminal: TerminalSettings::default(),
            default_primary_model: None,
        }
    }
}

/// Appearance applied on the first frame, before the shared config is read.
pub(crate) const DEFAULT_APPEARANCE_ID: &str = "openbitfun-dark";

/// Value domains the shell grants the shared terminal settings. A value outside
/// its domain means "shell default" to the pane and is refused by the page, so
/// the persisted document never carries one this shell cannot render.
pub(crate) const MIN_FONT_SIZE: u32 = 8;
pub(crate) const MAX_FONT_SIZE: u32 = 32;
pub(crate) const MIN_SCROLLBACK: u32 = 100;
pub(crate) const MAX_SCROLLBACK: u32 = 1_000_000;

/// Font size a pane renders with, or `None` when the configured value is
/// outside the renderable range.
pub(crate) fn valid_font_size(value: u32) -> Option<u32> {
    (MIN_FONT_SIZE..=MAX_FONT_SIZE)
        .contains(&value)
        .then_some(value)
}

/// Scrollback depth a pane starts with, or `None` outside the domain.
pub(crate) fn valid_scrollback(value: u32) -> Option<u32> {
    (MIN_SCROLLBACK..=MAX_SCROLLBACK)
        .contains(&value)
        .then_some(value)
}

/// Rounds a number-field value onto the font-size domain.
fn clamp_font_size(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 0. {
        return None;
    }
    valid_font_size(value.round() as u32)
}

/// Rounds a number-field value onto the scrollback domain.
fn clamp_scrollback(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 0. {
        return None;
    }
    valid_scrollback(value.round() as u32)
}

/// One theme choice: the product appearance id (the value stored in
/// `appearance.selection`) and the shell preset that renders it.
struct AppearanceChoice {
    id: &'static str,
    /// Palette name; product palette names are not translated copy.
    label: &'static str,
    target: AppearanceTarget,
}

enum AppearanceTarget {
    /// The id maps to one shell preset regardless of the OS appearance.
    Fixed(&'static str),
    /// The id follows the window appearance.
    System {
        dark: &'static str,
        light: &'static str,
    },
}

/// Appearance ids the shell renders, all inside the Web UI / desktop value
/// domain. Ids outside this list (for example Web UI only palettes without a
/// shell preset) stay persisted but fall back to the default theme.
static APPEARANCE_CHOICES: &[AppearanceChoice] = &[
    AppearanceChoice {
        id: "system",
        label: "",
        target: AppearanceTarget::System {
            dark: theme_openbitfun::DARK_PRESET_ID,
            light: theme_openbitfun::LIGHT_PRESET_ID,
        },
    },
    AppearanceChoice {
        id: "openbitfun-dark",
        label: "Dark",
        target: AppearanceTarget::Fixed(theme_openbitfun::DARK_PRESET_ID),
    },
    AppearanceChoice {
        id: "openbitfun-light",
        label: "Light",
        target: AppearanceTarget::Fixed(theme_openbitfun::LIGHT_PRESET_ID),
    },
    AppearanceChoice {
        id: "openbitfun-midnight",
        label: "Midnight",
        target: AppearanceTarget::Fixed("openbitfun-midnight"),
    },
    AppearanceChoice {
        id: "openbitfun-tokyo-night",
        label: "Tokyo Night",
        target: AppearanceTarget::Fixed("openbitfun-tokyo-night"),
    },
    AppearanceChoice {
        id: "openbitfun-cyber",
        label: "Cyber",
        target: AppearanceTarget::Fixed("openbitfun-cyber"),
    },
];

/// Theme preset for one appearance value; `None` when the shell has no preset
/// for it (the caller degrades: keep the current theme and warn).
pub(crate) fn resolve_appearance(
    selection: &str,
    window: WindowAppearance,
) -> Option<&'static str> {
    let choice = APPEARANCE_CHOICES
        .iter()
        .find(|choice| choice.id == selection)?;
    Some(match choice.target {
        AppearanceTarget::Fixed(preset) => preset,
        AppearanceTarget::System { dark, light } => {
            if window_appearance_is_light(window) {
                light
            } else {
                dark
            }
        }
    })
}

fn window_appearance_is_light(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Light | WindowAppearance::VibrantLight
    )
}

/// Applies an appearance value to the shell theme; returns the applied preset
/// id, or `None` when the value has no shell palette.
pub(crate) fn apply_appearance(
    selection: &str,
    window: WindowAppearance,
    cx: &mut App,
) -> Option<&'static str> {
    let preset_id = resolve_appearance(selection, window)?;
    if !theme::apply_by_id(preset_id, cx) {
        tracing::warn!(
            "Theme preset {} is not loaded for appearance {}",
            preset_id,
            selection
        );
        return None;
    }
    Some(preset_id)
}

/// Persists one setting through the process-global config service, off the UI
/// thread. Failures are logged; the in-memory selection stays as the user set
/// it rather than silently reverting.
pub(crate) fn persist(path: &'static str, value: serde_json::Value) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::error!("Cannot persist {} before the runtime is ready", path);
        return;
    };
    handle.tokio_handle().spawn(async move {
        let service = match get_global_config_service().await {
            Ok(service) => service,
            Err(error) => {
                tracing::error!(
                    "Config service unavailable while persisting {}: {}",
                    path,
                    error
                );
                return;
            }
        };
        match service.set_config(path, value).await {
            Ok(()) => tracing::info!("Setting persisted: {}", path),
            Err(error) => tracing::error!("Failed to persist {}: {}", path, error),
        }
    });
}

/// The settings page: appearance (language, theme), models, tool permissions,
/// and the terminal preferences the built-in terminal reads.
pub(crate) fn render_page(text: &ShellText, cx: &mut App) -> gpui_kit::AnyElement {
    Settings::new("shell-settings")
        .sidebar_width(gpui_kit::px(180.))
        .pages(vec![
            appearance_page(text),
            models_page(text, cx),
            terminal_page(text),
            permissions_page(text, cx),
        ])
        .into_any_element()
}

fn appearance_page(text: &ShellText) -> SettingPage {
    let language_options = vec![
        (SharedString::from("zh-CN"), SharedString::from("简体中文")),
        (SharedString::from("zh-TW"), SharedString::from("繁體中文")),
        (SharedString::from("en-US"), SharedString::from("English")),
    ];
    let language_field = SettingField::dropdown(
        language_options,
        |cx: &App| SharedString::from(ShellConfig::current(cx).language.as_str()),
        |value: SharedString, cx: &mut App| {
            let Some(locale) = LocaleId::from_str(&value) else {
                tracing::warn!("Ignoring unsupported language selection: {}", value);
                return;
            };
            cx.global_mut::<ShellConfig>().language = locale;
            crate::i18n::load_locale(locale, cx);
            persist(
                LANGUAGE_PATH,
                serde_json::Value::String(locale.as_str().to_string()),
            );
        },
    );

    let theme_field = {
        let text = text.clone();
        SettingField::render(move |_options, _window, cx| theme_button(&text, cx))
    };

    SettingPage::new(text.settings_appearance.clone())
        .icon(IconName::Palette)
        .resettable(false)
        .default_open(true)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.language.clone(), language_field)
                .description(text.settings_language_description.clone()),
            SettingItem::new(text.theme.clone(), theme_field)
                .description(text.settings_theme_description.clone()),
        ]))
}

/// Models page (M11 minimal version): the configured models with their
/// provider and enabled state, plus the default model new sessions start on.
/// The catalog comes from the same core path the desktop models page uses;
/// the default-model choice persists to `ai.default_models.primary`.
fn models_page(text: &ShellText, cx: &App) -> SettingPage {
    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();

    let default_field = SettingField::dropdown(
        catalog
            .models
            .iter()
            .filter(|model| model.enabled)
            .map(|model| (model.id.clone().into(), model.label().into()))
            .collect::<Vec<(SharedString, SharedString)>>(),
        |cx: &App| {
            SharedString::from(
                ShellConfig::current(cx)
                    .default_primary_model
                    .unwrap_or_default(),
            )
        },
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if value.is_empty() {
                return;
            }
            cx.global_mut::<ShellConfig>().default_primary_model = Some(value.clone());
            persist(DEFAULT_MODEL_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    let no_models = catalog.models.is_empty();

    let list_field = SettingField::render(|_options, _window, cx| {
        let catalog = cx
            .try_global::<crate::runtime::ModelCatalog>()
            .cloned()
            .unwrap_or_default();
        let text = crate::i18n::current(cx);
        if !catalog.loaded {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.models_loading.clone())
                .into_any_element();
        }
        if catalog.models.is_empty() {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.models_empty.clone())
                .into_any_element();
        }
        let mut list = div().v_flex().gap_1().w_full();
        for model in &catalog.models {
            let (state, state_color) = if model.enabled {
                (text.models_enabled.clone(), cx.theme().muted_foreground)
            } else {
                (text.models_disabled.clone(), cx.theme().danger)
            };
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().min_w_0().truncate().text_sm().child(model.label()))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(state_color)
                            .child(state),
                    ),
            );
        }
        list.into_any_element()
    });

    let default_description = if no_models {
        text.models_empty.clone()
    } else {
        text.models_default_description.clone()
    };

    SettingPage::new(text.models_title.clone())
        .icon(IconName::Cpu)
        .resettable(false)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.models_default.clone(), default_field)
                .description(default_description),
            SettingItem::new(text.models_configured.clone(), list_field),
        ]))
}

/// Terminal preferences for the built-in terminal (M7 pane). The values are
/// the shared `terminal.*` entries, so the desktop terminal and this pane read
/// the same document; font size and family apply to a running pane, while the
/// scrollback and shell command are read when a pane starts.
fn terminal_page(text: &ShellText) -> SettingPage {
    let font_size_field = SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(MIN_FONT_SIZE),
            max: f64::from(MAX_FONT_SIZE),
            step: 1.,
        },
        |cx: &App| f64::from(ShellConfig::current(cx).terminal.font_size),
        |value: f64, cx: &mut App| {
            let Some(size) = clamp_font_size(value) else {
                tracing::warn!("Ignoring out-of-range terminal font size: {}", value);
                return;
            };
            cx.global_mut::<ShellConfig>().terminal.font_size = size;
            persist(TERMINAL_FONT_SIZE_PATH, serde_json::json!(size));
            cx.refresh_windows();
        },
    );

    let font_family_field = SettingField::input(
        |cx: &App| SharedString::from(ShellConfig::current(cx).terminal.font_family.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            cx.global_mut::<ShellConfig>().terminal.font_family = value.clone();
            persist(TERMINAL_FONT_FAMILY_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    let scrollback_field = SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(MIN_SCROLLBACK),
            max: f64::from(MAX_SCROLLBACK),
            step: 1000.,
        },
        |cx: &App| f64::from(ShellConfig::current(cx).terminal.scrollback),
        |value: f64, cx: &mut App| {
            let Some(lines) = clamp_scrollback(value) else {
                tracing::warn!("Ignoring out-of-range terminal scrollback: {}", value);
                return;
            };
            cx.global_mut::<ShellConfig>().terminal.scrollback = lines;
            persist(TERMINAL_SCROLLBACK_PATH, serde_json::json!(lines));
            cx.refresh_windows();
        },
    );

    let shell_field = SettingField::input(
        |cx: &App| SharedString::from(ShellConfig::current(cx).terminal.default_shell.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            cx.global_mut::<ShellConfig>().terminal.default_shell = value.clone();
            persist(TERMINAL_DEFAULT_SHELL_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    SettingPage::new(text.settings_terminal.clone())
        .icon(IconName::SquareTerminal)
        .resettable(false)
        .default_open(false)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.settings_terminal_font_size.clone(), font_size_field)
                .description(text.settings_terminal_font_size_description.clone()),
            SettingItem::new(text.settings_terminal_font_family.clone(), font_family_field)
                .description(text.settings_terminal_font_family_description.clone()),
            SettingItem::new(text.settings_terminal_scrollback.clone(), scrollback_field)
                .description(text.settings_terminal_scrollback_description.clone()),
            SettingItem::new(text.settings_terminal_shell.clone(), shell_field)
                .description(text.settings_terminal_shell_description.clone()),
        ]))
}

fn permissions_page(text: &ShellText, cx: &App) -> SettingPage {
    let preset_options = vec![
        (
            SharedString::from("ask"),
            SharedString::from(text.settings_policy_ask.clone()),
        ),
        (
            SharedString::from("full_access"),
            SharedString::from(text.settings_policy_full_access.clone()),
        ),
    ];
    let preset_field = SettingField::dropdown(
        preset_options,
        |cx: &App| SharedString::from(preset_value(ShellConfig::current(cx).permission_preset)),
        |value: SharedString, cx: &mut App| {
            let Some(preset) = preset_from_value(&value) else {
                tracing::warn!("Ignoring unsupported permission policy: {}", value);
                return;
            };
            cx.global_mut::<ShellConfig>().permission_preset = preset;
            persist(
                PERMISSION_PRESET_PATH,
                serde_json::Value::String(preset_value(preset).to_string()),
            );
            cx.refresh_windows();
        },
    );

    let auto_approve_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).auto_approve_ask,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().auto_approve_ask = value;
            persist(AUTO_APPROVE_ASK_PATH, serde_json::Value::Bool(value));
            cx.refresh_windows();
        },
    );

    // Auto-approval only changes behavior under the Ask policy; the preset
    // already resolves every ask when full access is on.
    let auto_approve_disabled =
        ShellConfig::current(cx).permission_preset == PermissionPolicyPreset::FullAccess;

    SettingPage::new(text.settings_permissions.clone())
        .icon(IconName::Shield)
        .resettable(false)
        .default_open(true)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.settings_policy.clone(), preset_field)
                .description(text.settings_policy_description.clone()),
            SettingItem::new(text.settings_auto_approve.clone(), auto_approve_field)
                .description(text.settings_auto_approve_description.clone())
                .disabled(auto_approve_disabled),
        ]))
}

/// Appearance picker: a dropdown button whose menu resolves `system` against
/// the live window appearance at click time. Uses the same
/// `Button + caret + dropdown_menu_with_anchor` shape as the typed setting
/// fields, so the whole button opens the menu.
fn theme_button(text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let current = ShellConfig::current(cx).appearance;
    let label = appearance_label(&current, text);
    let mut options: Vec<(String, String, bool)> = APPEARANCE_CHOICES
        .iter()
        .map(|choice| {
            (
                choice.id.to_string(),
                appearance_choice_label(choice, text),
                choice.id == current,
            )
        })
        .collect();
    if !APPEARANCE_CHOICES.iter().any(|choice| choice.id == current) {
        // Honest display: the persisted value has no shell palette (for
        // example a Web UI only appearance), so it is listed as-is instead of
        // showing another choice as active.
        options.insert(0, (current.clone(), label.clone(), true));
    }

    Button::new("shell-settings-theme")
        .label(label)
        .dropdown_caret(true)
        .outline()
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
            let mut menu = menu;
            for (id, option_label, checked) in &options {
                let id = id.clone();
                menu = menu.item(
                    PopupMenuItem::new(option_label.clone())
                        .checked(*checked)
                        .on_click(move |_, window, cx| {
                            select_appearance(id.clone(), window, cx);
                        }),
                );
            }
            menu
        })
        .into_any_element()
}

/// Applies and persists one appearance selection.
fn select_appearance(id: String, window: &mut Window, cx: &mut App) {
    if apply_appearance(&id, window.appearance(), cx).is_none() {
        tracing::warn!(
            "Appearance {} has no shell palette; keeping the current theme",
            id
        );
    }
    cx.global_mut::<ShellConfig>().appearance = id.clone();
    persist(APPEARANCE_PATH, serde_json::Value::String(id));
    cx.refresh_windows();
}

/// Display label for the current appearance value, including unsupported ids.
fn appearance_label(current: &str, text: &ShellText) -> String {
    match APPEARANCE_CHOICES
        .iter()
        .find(|choice| choice.id == current)
    {
        Some(choice) => appearance_choice_label(choice, text),
        None => format!("{} ({})", current, text.settings_theme_unsupported),
    }
}

fn appearance_choice_label(choice: &AppearanceChoice, text: &ShellText) -> String {
    if choice.id == "system" {
        text.settings_theme_system.clone()
    } else {
        choice.label.to_string()
    }
}

/// Serialized value of a permission policy preset; keep in sync with the
/// `snake_case` serde shape of [`PermissionPolicyPreset`] (asserted in tests).
pub(crate) fn preset_value(preset: PermissionPolicyPreset) -> &'static str {
    match preset {
        PermissionPolicyPreset::Ask => "ask",
        PermissionPolicyPreset::FullAccess => "full_access",
    }
}

fn preset_from_value(value: &str) -> Option<PermissionPolicyPreset> {
    match value {
        "ask" => Some(PermissionPolicyPreset::Ask),
        "full_access" => Some(PermissionPolicyPreset::FullAccess),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbitfun_core::service::i18n::LocaleId;

    fn sample_text() -> ShellText {
        crate::i18n::fallback_text(LocaleId::ZhCN)
    }

    #[test]
    fn appearance_ids_resolve_to_shell_presets() {
        let dark = WindowAppearance::Dark;
        assert_eq!(
            resolve_appearance("openbitfun-dark", dark),
            Some(theme_openbitfun::DARK_PRESET_ID)
        );
        assert_eq!(
            resolve_appearance("openbitfun-light", dark),
            Some(theme_openbitfun::LIGHT_PRESET_ID)
        );
        assert_eq!(
            resolve_appearance("openbitfun-tokyo-night", dark),
            Some("openbitfun-tokyo-night")
        );
    }

    #[test]
    fn system_appearance_follows_the_window() {
        assert_eq!(
            resolve_appearance("system", WindowAppearance::Light),
            Some(theme_openbitfun::LIGHT_PRESET_ID)
        );
        assert_eq!(
            resolve_appearance("system", WindowAppearance::Dark),
            Some(theme_openbitfun::DARK_PRESET_ID)
        );
        assert_eq!(
            resolve_appearance("system", WindowAppearance::VibrantLight),
            Some(theme_openbitfun::LIGHT_PRESET_ID)
        );
    }

    #[test]
    fn unknown_appearance_is_not_claimed() {
        // Web UI only palettes have no shell preset: the shell must fall back
        // instead of pretending another choice is active.
        assert_eq!(
            resolve_appearance("openbitfun-slate", WindowAppearance::Dark),
            None
        );
        assert_eq!(resolve_appearance("", WindowAppearance::Dark), None);
        let text = sample_text();
        assert_eq!(
            appearance_label("openbitfun-slate", &text),
            "openbitfun-slate (unsupported)"
        );
        assert_eq!(appearance_label("system", &text), "Follow system");
        assert_eq!(appearance_label("openbitfun-cyber", &text), "Cyber");
    }

    #[test]
    fn preset_values_match_the_serde_shape() {
        for preset in [
            PermissionPolicyPreset::Ask,
            PermissionPolicyPreset::FullAccess,
        ] {
            let serialized = serde_json::to_value(preset).expect("permission preset serializes");
            assert_eq!(
                serialized,
                serde_json::Value::String(preset_value(preset).to_string())
            );
            assert_eq!(preset_from_value(preset_value(preset)), Some(preset));
        }
        assert_eq!(preset_from_value("everything"), None);
    }

    #[test]
    fn shell_config_snapshot_reads_the_shared_fields() {
        let mut config = GlobalConfig::default();
        config.app.language = "en-US".to_string();
        config.appearance.selection = "openbitfun-light".to_string();
        config.tool_permissions.policy.preset = PermissionPolicyPreset::Ask;
        config.tool_permissions.interaction.auto_approve_ask = true;
        config.terminal.font_size = 18;
        config.terminal.font_family = "\"JetBrains Mono\", monospace".to_string();
        config.terminal.scrollback = 50_000;
        config.terminal.default_shell = "/usr/bin/zsh".to_string();
        config.ai.default_models.primary = Some("model-a".to_string());

        let snapshot = ShellConfig::from_global(&config);
        assert_eq!(snapshot.language, LocaleId::EnUS);
        assert_eq!(snapshot.appearance, "openbitfun-light");
        assert_eq!(snapshot.permission_preset, PermissionPolicyPreset::Ask);
        assert!(snapshot.auto_approve_ask);
        assert_eq!(snapshot.terminal.font_size, 18);
        assert_eq!(
            snapshot.terminal.font_family,
            "\"JetBrains Mono\", monospace"
        );
        assert_eq!(snapshot.terminal.scrollback, 50_000);
        assert_eq!(snapshot.terminal.default_shell, "/usr/bin/zsh");
        assert_eq!(snapshot.default_primary_model.as_deref(), Some("model-a"));
    }

    #[test]
    fn terminal_value_domains_reject_what_the_grid_cannot_render() {
        assert_eq!(valid_font_size(14), Some(14));
        assert_eq!(valid_font_size(0), None);
        assert_eq!(valid_font_size(MIN_FONT_SIZE - 1), None);
        assert_eq!(valid_font_size(MAX_FONT_SIZE + 1), None);
        assert_eq!(clamp_font_size(14.4), Some(14));
        assert_eq!(clamp_font_size(f64::NAN), None);
        assert_eq!(clamp_font_size(-1.), None);

        assert_eq!(valid_scrollback(1000), Some(1000));
        assert_eq!(valid_scrollback(0), None);
        assert_eq!(clamp_scrollback(1000.6), Some(1001));
        assert_eq!(clamp_scrollback(f64::INFINITY), None);
    }

    #[test]
    fn shell_config_snapshot_tolerates_unknown_language() {
        let mut config = GlobalConfig::default();
        config.app.language = "fr-FR".to_string();
        // Unknown locale degrades to the product default instead of failing.
        assert_eq!(ShellConfig::from_global(&config).language, LocaleId::ZhCN);
    }

    fn install_theme_state(cx: &mut gpui_kit::App) {
        gpui_kit::init(cx);
        let presets = crate::theme::load_presets();
        let active = presets
            .iter()
            .position(|preset| preset.id == theme_openbitfun::DARK_PRESET_ID)
            .unwrap_or(0);
        if let Some(preset) = presets.get(active) {
            preset.apply(cx);
        }
        cx.set_global(crate::theme::ShellState {
            presets,
            active_preset: active,
        });
    }

    fn active_preset(cx: &gpui_kit::App) -> &'static str {
        let state = cx.global::<crate::theme::ShellState>();
        state.presets[state.active_preset].id
    }

    #[gpui_kit::test]
    fn applying_a_known_appearance_switches_the_preset(cx: &mut gpui_kit::TestAppContext) {
        cx.update(install_theme_state);
        cx.update(|cx| {
            assert_eq!(
                apply_appearance("openbitfun-tokyo-night", WindowAppearance::Dark, cx),
                Some("openbitfun-tokyo-night")
            );
            assert_eq!(active_preset(cx), "openbitfun-tokyo-night");

            // An id without a shell palette keeps the previous theme.
            assert_eq!(
                apply_appearance("openbitfun-slate", WindowAppearance::Dark, cx),
                None
            );
            assert_eq!(active_preset(cx), "openbitfun-tokyo-night");

            // `system` resolves against the window appearance.
            assert_eq!(
                apply_appearance("system", WindowAppearance::Light, cx),
                Some(theme_openbitfun::LIGHT_PRESET_ID)
            );
            assert_eq!(active_preset(cx), theme_openbitfun::LIGHT_PRESET_ID);
        });
    }
}
