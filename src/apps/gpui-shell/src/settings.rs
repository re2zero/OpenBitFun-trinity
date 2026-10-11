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
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, WindowExt};
use gpui_kit::{
    div, prelude::*, px, Anchor, App, Global, IntoElement, SharedString, Window, WindowAppearance,
};
use openbitfun_core::service::config::get_global_config_service;
use openbitfun_core::service::config::types::GlobalConfig;
use openbitfun_core::service::i18n::LocaleId;
use openbitfun_runtime_ports::PermissionPolicyPreset;

use crate::i18n::{self, ShellText};
use crate::theme;
use crate::theme_openbitfun;
use crate::update::UpdateUiState;

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
/// Named launch profiles the terminal panel offers as extra tabs (M17); the
/// whole list is rewritten on each profile change.
pub(crate) const TERMINAL_PROFILES_PATH: &str = "terminal.profiles";
/// Default model new sessions start on (same path the desktop models page
/// writes).
pub(crate) const DEFAULT_MODEL_PATH: &str = "ai.default_models.primary";
/// What the window close button does (`app.close_button_behavior`). The shell
/// renders `quit`/`minimize_to_tray`; anything else (the desktop's `ask`, for
/// one) stays displayed verbatim and behaves like the desktop fallback:
/// minimize with the tray present.
pub(crate) const CLOSE_BEHAVIOR_PATH: &str = "app.close_button_behavior";
/// Whether the startup update probe runs (`app.auto_update`); the manual
/// check ignores it, same as the desktop store.
pub(crate) const AUTO_UPDATE_PATH: &str = "app.auto_update";
/// OS notification when a turn finishes in the background
/// (`app.notifications.dialog_completion_notify`), the desktop's
/// dialog-completion switch.
pub(crate) const NOTIFY_TURN_DONE_PATH: &str = "app.notifications.dialog_completion_notify";
/// Master switch for native agent hooks (`app.hooks.enabled`).
pub(crate) const HOOKS_ENABLED_PATH: &str = "app.hooks.enabled";
/// Whether project-scope hooks.json files may run
/// (`app.hooks.project_hooks_enabled`).
pub(crate) const HOOKS_PROJECT_PATH: &str = "app.hooks.project_hooks_enabled";
/// OpenBitFun co-author line in built-in commit workflows
/// (`app.ai_experience.enable_git_commit_coauthor`).
pub(crate) const GIT_COAUTHOR_PATH: &str = "app.ai_experience.enable_git_commit_coauthor";
/// Active web-search provider (`ai.web_search.provider`).
pub(crate) const WEB_SEARCH_PROVIDER_PATH: &str = "ai.web_search.provider";
/// Custom HTTP search service endpoint
/// (`ai.web_search.providers.openbitfun_search_http.endpoint`).
pub(crate) const WEB_SEARCH_HTTP_ENDPOINT_PATH: &str =
    "ai.web_search.providers.openbitfun_search_http.endpoint";
/// Custom HTTP search service auth mode
/// (`ai.web_search.providers.openbitfun_search_http.auth.mode`).
pub(crate) const WEB_SEARCH_HTTP_AUTH_MODE_PATH: &str =
    "ai.web_search.providers.openbitfun_search_http.auth.mode";
/// Custom HTTP search service credential header name
/// (`ai.web_search.providers.openbitfun_search_http.auth.headerName`).
pub(crate) const WEB_SEARCH_HTTP_HEADER_NAME_PATH: &str =
    "ai.web_search.providers.openbitfun_search_http.auth.headerName";
/// Runtime log level (`app.logging.level`), applied live through the tracing
/// reload handle.
pub(crate) const LOGGING_LEVEL_PATH: &str = "app.logging.level";

/// Serialized close-button values the shell offers.
pub(crate) const CLOSE_QUIT: &str = "quit";
pub(crate) const CLOSE_MINIMIZE: &str = "minimize_to_tray";

/// What a window close request does, resolved from the raw shared value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseBehavior {
    Quit,
    MinimizeToTray,
}

/// The desktop treats every non-`quit` value as "tray on close"; the shell
/// cannot hide a window, so those values minimize.
pub(crate) fn parse_close_behavior(raw: &str) -> CloseBehavior {
    match raw {
        CLOSE_QUIT => CloseBehavior::Quit,
        _ => CloseBehavior::MinimizeToTray,
    }
}

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
    /// Named launch profiles (M17); the shared document type carries them so
    /// the desktop terminal can adopt the same list later.
    pub(crate) profiles: Vec<openbitfun_core::service::config::types::TerminalProfile>,
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
            profiles: config.profiles.clone(),
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
    /// Raw `app.close_button_behavior` (`quit` | `minimize_to_tray` | `ask`).
    /// Kept verbatim so the page can show an unsupported value as-is.
    pub(crate) close_behavior_raw: String,
    /// `app.auto_update`: gates the startup update probe.
    pub(crate) auto_update: bool,
    /// `app.notifications.dialog_completion_notify`: desktop notification for
    /// a turn that settles while the window is in the background.
    pub(crate) dialog_completion_notify: bool,
    /// `app.hooks.enabled`: master switch for native agent hooks.
    pub(crate) hooks_enabled: bool,
    /// `app.hooks.project_hooks_enabled`: whether project-scope hooks.json
    /// files may run.
    pub(crate) project_hooks_enabled: bool,
    /// `app.ai_experience.enable_git_commit_coauthor`.
    pub(crate) git_commit_coauthor: bool,
    /// `ai.web_search.provider`, kept verbatim; unknown ids render as-is.
    pub(crate) web_search_provider: String,
    /// `app.ai_experience.quick_actions`: the chat header quick-actions menu
    /// reads this snapshot; editing lives on the web/desktop automation page.
    pub(crate) quick_actions: Vec<openbitfun_core::service::config::types::AiExperienceQuickAction>,
    /// `ai.web_search.providers.openbitfun_search_http.endpoint`.
    pub(crate) web_search_http_endpoint: String,
    /// `ai.web_search.providers.openbitfun_search_http.auth.mode`, kept
    /// verbatim; the empty string and unknown ids follow the shared contract
    /// (`""` behaves like `none`, unknown modes need a credential and fail at
    /// search time).
    pub(crate) web_search_http_auth_mode: String,
    /// `ai.web_search.providers.openbitfun_search_http.auth.headerName`.
    pub(crate) web_search_http_header_name: String,
    /// `app.logging.level`, one of the tracing level names.
    pub(crate) logging_level: String,
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
            close_behavior_raw: config.app.close_button_behavior.clone(),
            auto_update: config.app.auto_update,
            dialog_completion_notify: config.app.notifications.dialog_completion_notify,
            hooks_enabled: config.app.hooks.enabled,
            project_hooks_enabled: config.app.hooks.project_hooks_enabled,
            git_commit_coauthor: config.app.ai_experience.enable_git_commit_coauthor,
            web_search_provider: config.ai.web_search.provider.clone(),
            quick_actions: config.app.ai_experience.quick_actions.clone(),
            web_search_http_endpoint: config
                .ai
                .web_search
                .providers
                .openbitfun_search_http
                .endpoint
                .clone(),
            web_search_http_auth_mode: config
                .ai
                .web_search
                .providers
                .openbitfun_search_http
                .auth
                .mode
                .clone(),
            web_search_http_header_name: config
                .ai
                .web_search
                .providers
                .openbitfun_search_http
                .auth
                .header_name
                .clone(),
            logging_level: config.app.logging.level.clone(),
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
    /// defaults, matching the theme applied on the first frame. The close
    /// button starts as `quit` so the very first seconds keep the historic
    /// behavior (no tray is up yet); the snapshot applies the shared default.
    pub(crate) fn startup_default(locale: LocaleId) -> Self {
        Self {
            language: locale,
            appearance: DEFAULT_APPEARANCE_ID.to_string(),
            permission_preset: PermissionPolicyPreset::default(),
            auto_approve_ask: false,
            terminal: TerminalSettings::default(),
            default_primary_model: None,
            close_behavior_raw: CLOSE_QUIT.to_string(),
            auto_update: true,
            dialog_completion_notify: true,
            hooks_enabled: true,
            project_hooks_enabled: false,
            git_commit_coauthor: true,
            web_search_provider: "exa_mcp_free".to_string(),
            quick_actions: openbitfun_core::service::config::types::AIExperienceConfig::default()
                .quick_actions,
            web_search_http_endpoint: String::new(),
            web_search_http_auth_mode: "none".to_string(),
            web_search_http_header_name: String::new(),
            logging_level: "info".to_string(),
        }
    }
}

/// Launch-at-login state read from the platform backend at bootstrap and
/// flipped optimistically by the toggle; a failed write publishes the real
/// state back through [`crate::runtime::RuntimeEvent::AutostartState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutostartState(pub(crate) bool);

impl Global for AutostartState {}

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

/// Inserts or replaces one terminal profile in the shared mirror and
/// persists the whole list. Callers validate the name; an empty id assigns
/// a fresh one.
pub(crate) fn upsert_terminal_profile(
    mut profile: openbitfun_core::service::config::types::TerminalProfile,
    cx: &mut App,
) {
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    let profiles = &mut cx.global_mut::<ShellConfig>().terminal.profiles;
    match profiles.iter().position(|p| p.id == profile.id) {
        Some(slot) => profiles[slot] = profile,
        None => profiles.push(profile),
    }
    persist_profiles(cx);
}

/// Removes one terminal profile from the shared mirror and persists.
pub(crate) fn delete_terminal_profile(id: &str, cx: &mut App) {
    let profiles = &mut cx.global_mut::<ShellConfig>().terminal.profiles;
    let before = profiles.len();
    profiles.retain(|p| p.id != id);
    if profiles.len() != before {
        persist_profiles(cx);
    } else {
        tracing::warn!("Ignoring delete for unknown terminal profile {id}");
    }
}

fn persist_profiles(cx: &App) {
    let profiles = ShellConfig::current(cx).terminal.profiles.clone();
    match serde_json::to_value(profiles) {
        Ok(value) => persist(TERMINAL_PROFILES_PATH, value),
        Err(error) => tracing::error!("Failed to serialize terminal profiles: {error}"),
    }
}

/// The settings page: general (M22 tray/autostart/updates), appearance
/// (language, theme), models, tool permissions, terminal preferences, and the
/// M21/M22 management and data pages. The shell weak handle lets page rows
/// open dialogs and read window state (D57).
pub(crate) fn render_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    Settings::new("shell-settings")
        .sidebar_width(gpui_kit::px(180.))
        .pages(vec![
            general_page(text, view.clone(), cx),
            appearance_page(text),
            models_page(text, cx),
            terminal_page(text, view.clone()),
            permissions_page(text, cx),
            automation_page(text),
            websearch_page(text, view.clone(), cx),
            crate::manage::skills_page(text),
            crate::manage::agents_page(text, view.clone()),
            crate::manage::mcp_page(text, view.clone()),
            usage_page(text, view.clone()),
            archived_page(text, view.clone()),
            diagnostics_page(text, view),
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

/// General page (M22): launch-at-login, the close-button behavior, and the
/// self-update flow (auto-update preference plus the manual check / install /
/// restart actions). The update state lives on the window and arrives through
/// the event pump; the tray is the other half of this page (menu events land
/// on the same actions).
fn general_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &App,
) -> SettingPage {
    let version_field = SettingField::render(|_options, _window, cx| {
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(format!("v{}", env!("CARGO_PKG_VERSION")))
            .into_any_element()
    });

    let login_field = SettingField::switch(
        |cx: &App| {
            cx.try_global::<AutostartState>()
                .map(|s| s.0)
                .unwrap_or(false)
        },
        |value: bool, cx: &mut App| set_autostart(value, cx),
    );

    // Close behavior: the two values the shell renders, plus the persisted
    // value verbatim when it is outside that domain (the desktop's `ask`).
    let current_close = ShellConfig::current(cx).close_behavior_raw.clone();
    let mut close_options = vec![
        (
            SharedString::from(CLOSE_QUIT),
            SharedString::from(text.settings_close_quit.clone()),
        ),
        (
            SharedString::from(CLOSE_MINIMIZE),
            SharedString::from(text.settings_close_minimize.clone()),
        ),
    ];
    if current_close != CLOSE_QUIT && current_close != CLOSE_MINIMIZE {
        close_options.insert(
            0,
            (
                SharedString::from(current_close.clone()),
                SharedString::from(format!(
                    "{current_close} ({})",
                    text.value_unsupported.clone()
                )),
            ),
        );
    }
    let close_field = SettingField::dropdown(
        close_options,
        |cx: &App| SharedString::from(ShellConfig::current(cx).close_behavior_raw.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if value != CLOSE_QUIT && value != CLOSE_MINIMIZE {
                tracing::warn!("Ignoring unsupported close behavior: {value}");
                return;
            }
            cx.global_mut::<ShellConfig>().close_behavior_raw = value.clone();
            crate::shell::sync_close_quits(cx);
            persist(CLOSE_BEHAVIOR_PATH, serde_json::Value::String(value));
            cx.refresh_windows();
        },
    );

    let auto_update_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).auto_update,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().auto_update = value;
            persist(AUTO_UPDATE_PATH, serde_json::Value::Bool(value));
            cx.refresh_windows();
        },
    );

    let notify_turn_done_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).dialog_completion_notify,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().dialog_completion_notify = value;
            persist(NOTIFY_TURN_DONE_PATH, serde_json::Value::Bool(value));
            cx.refresh_windows();
        },
    );

    let owned_text = text.clone();
    let update_field = SettingField::render(move |_options, _window, cx| {
        update_check_field(view.clone(), &owned_text, cx)
    });

    SettingPage::new(text.settings_general.clone())
        .icon(IconName::House)
        .resettable(false)
        .default_open(true)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.settings_version.clone(), version_field),
            SettingItem::new(text.settings_launch_login.clone(), login_field)
                .description(text.settings_launch_login_description.clone()),
            SettingItem::new(text.settings_close_behavior.clone(), close_field)
                .description(text.settings_close_behavior_description.clone()),
            SettingItem::new(
                text.settings_notify_turn_done.clone(),
                notify_turn_done_field,
            )
            .description(text.settings_notify_turn_done_description.clone()),
            SettingItem::new(text.settings_auto_update.clone(), auto_update_field)
                .description(text.settings_auto_update_description.clone()),
            SettingItem::new(text.settings_check_updates.clone(), update_field),
        ]))
}

/// Flips launch-at-login. The toggle is optimistic; a failed platform write
/// logs the reason and publishes the real state back through the pump.
fn set_autostart(enabled: bool, cx: &mut App) {
    cx.set_global(AutostartState(enabled));
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::error!("Cannot change launch-at-login before the runtime is ready");
        return;
    };
    handle.tokio_handle().spawn(async move {
        if let Err(reason) = crate::tray::autostart::set_enabled(enabled) {
            tracing::error!("Launch-at-login write failed: {reason}");
            let real = crate::tray::autostart::is_enabled().unwrap_or(!enabled);
            crate::runtime::publish(crate::runtime::RuntimeEvent::AutostartState(real));
        } else {
            tracing::info!("Launch-at-login set to {enabled}");
        }
    });
}

/// The check / install / restart control, projected from the window's update
/// state machine. Every button publishes the same event the tray menu uses,
/// so there is exactly one update flow.
fn update_check_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let state = view
        .upgrade()
        .map(|shell| shell.read(cx).update_state.clone())
        .unwrap_or_default();
    let note = |value: String, danger: bool| {
        div()
            .text_sm()
            .text_color(if danger {
                cx.theme().danger
            } else {
                cx.theme().muted_foreground
            })
            .child(value)
    };
    let mut column = div().v_flex().gap_2().w_full();
    match state {
        UpdateUiState::Idle | UpdateUiState::UpToDate => {
            if let UpdateUiState::UpToDate = state {
                column = column.child(note(
                    format!(
                        "{} (v{})",
                        text.settings_up_to_date,
                        env!("CARGO_PKG_VERSION")
                    ),
                    false,
                ));
            }
            column = column.child(
                Button::new("update-check")
                    .label(text.settings_check_updates.clone())
                    .small()
                    .with_variant(ButtonVariant::Secondary)
                    .on_click(|_, _window, _cx| {
                        crate::runtime::publish(crate::runtime::RuntimeEvent::UpdateCheckRequested);
                    }),
            );
        }
        UpdateUiState::Checking => {
            column = column.child(note(text.settings_update_checking.clone(), false));
        }
        UpdateUiState::Available { version } => {
            column = column
                .child(note(
                    format!("{} v{version}", text.settings_update_available),
                    false,
                ))
                .child(
                    Button::new("update-install")
                        .label(text.settings_update_install.clone())
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .on_click(|_, _window, _cx| {
                            crate::runtime::publish(
                                crate::runtime::RuntimeEvent::UpdateInstallRequested,
                            );
                        }),
                );
        }
        UpdateUiState::Installing => {
            column = column.child(note(text.settings_update_installing.clone(), false));
        }
        UpdateUiState::Installed { version } => {
            column = column
                .child(note(
                    format!("{} v{version}", text.settings_update_installed),
                    false,
                ))
                .child(
                    Button::new("update-restart")
                        .label(text.settings_update_restart.clone())
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .on_click(|_, _window, _cx| {
                            crate::runtime::publish(crate::runtime::RuntimeEvent::RestartRequested);
                        }),
                );
        }
        UpdateUiState::Unsupported { reason } => {
            column = column.child(note(
                format!("{}: {reason}", text.settings_update_unsupported),
                false,
            ));
        }
        UpdateUiState::Failed { message } => {
            column = column
                .child(note(
                    format!("{}: {message}", text.settings_update_failed),
                    true,
                ))
                .child(
                    Button::new("update-retry")
                        .label(text.settings_check_updates.clone())
                        .small()
                        .with_variant(ButtonVariant::Secondary)
                        .on_click(|_, _window, _cx| {
                            crate::runtime::publish(
                                crate::runtime::RuntimeEvent::UpdateCheckRequested,
                            );
                        }),
                );
        }
    }
    column.into_any_element()
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
fn terminal_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingPage {
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

    let owned_text = text.clone();
    let profiles_field = SettingField::render(move |_options, window, cx| {
        terminal_profiles_field(view.clone(), &owned_text, window, cx)
    });

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
            SettingItem::new(text.settings_terminal_profiles.clone(), profiles_field)
                .description(text.settings_terminal_profiles_description.clone()),
        ]))
}

/// The terminal-profiles list (M17): one row per profile with edit/delete,
/// plus the add action. Rows render read-only; editing opens the profile
/// dialog through the shell (dialog state lives on the window).
fn terminal_profiles_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    _window: &mut gpui_kit::Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let profiles = ShellConfig::current(cx).terminal.profiles.clone();
    let mut column = div().v_flex().gap_2().w_full();

    if profiles.is_empty() {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.terminal_profiles_empty.clone()),
        );
    }
    for profile in &profiles {
        let edit_view = view.clone();
        let edit_id = profile.id.clone();
        let delete_id = profile.id.clone();
        let delete_label = profile.name.clone();
        let mut facts = profile.working_directory.clone();
        if !profile.startup_command.is_empty() {
            facts.push_str(" · ");
            facts.push_str(&profile.startup_command);
        }
        column = column.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(profile.name.clone()),
                        )
                        .when(!facts.is_empty(), |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(facts),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .flex_shrink_0()
                        .child(
                            Button::new(SharedString::from(format!(
                                "terminal-profile-edit-{}",
                                profile.id
                            )))
                            .label(text.terminal_profile_edit.clone())
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(move |_, window, cx| {
                                open_profile_dialog(
                                    edit_view.clone(),
                                    Some(edit_id.clone()),
                                    window,
                                    cx,
                                );
                            }),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "terminal-profile-delete-{}",
                                profile.id
                            )))
                            .label(text.terminal_profile_delete.clone())
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(move |_, _window, cx| {
                                tracing::info!("Deleting terminal profile {delete_label}");
                                delete_terminal_profile(&delete_id.clone(), cx);
                                cx.refresh_windows();
                            }),
                        ),
                ),
        );
    }

    column
        .child(
            Button::new("terminal-profile-add")
                .label(text.terminal_profile_add.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, window, cx| {
                    open_profile_dialog(view.clone(), None, window, cx);
                }),
        )
        .into_any_element()
}

/// Opens the terminal-profile editor: the three inputs live on the shell (a
/// dialog builder runs every frame), `profile_id` `None` means a new
/// profile. Dismissable; nothing blocks on it.
fn open_profile_dialog(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    profile_id: Option<String>,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (name_input, dir_input, command_input) = {
        let shell = shell.read(cx);
        (
            shell.profile_name_input.clone(),
            shell.profile_dir_input.clone(),
            shell.profile_command_input.clone(),
        )
    };
    let existing = profile_id.as_ref().and_then(|id| {
        ShellConfig::current(cx)
            .terminal
            .profiles
            .iter()
            .find(|p| &p.id == id)
            .cloned()
    });
    name_input.update(cx, |state, cx| {
        state.set_value(
            existing
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            window,
            cx,
        )
    });
    dir_input.update(cx, |state, cx| {
        state.set_value(
            existing
                .as_ref()
                .map(|p| p.working_directory.clone())
                .unwrap_or_default(),
            window,
            cx,
        )
    });
    command_input.update(cx, |state, cx| {
        state.set_value(
            existing
                .as_ref()
                .map(|p| p.startup_command.clone())
                .unwrap_or_default(),
            window,
            cx,
        )
    });
    shell.update(cx, |this, cx| {
        this.profile_edit_id = profile_id;
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let name = shell
            .read(cx)
            .profile_name_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if name.is_empty() {
            // Keep the dialog open: a nameless profile is unidentifiable in
            // the new-tab menu.
            tracing::warn!("Ignoring terminal profile with empty name");
            return;
        }
        let working_directory = shell
            .read(cx)
            .profile_dir_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let startup_command = shell
            .read(cx)
            .profile_command_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let id = shell.read(cx).profile_edit_id.clone().unwrap_or_default();
        let profile = openbitfun_core::service::config::types::TerminalProfile {
            id,
            name,
            working_directory,
            startup_command,
        };
        tracing::info!("Saving terminal profile {}", profile.name);
        upsert_terminal_profile(profile, cx);
        shell.update(cx, |this, cx| {
            this.profile_edit_id = None;
            cx.notify();
        });
        cx.refresh_windows();
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |this, cx| {
                this.profile_edit_id = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.terminal_profile_dialog_title.clone()))
            .child(
                div()
                    .w(px(420.))
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.terminal_profile_name.clone()),
                            )
                            .child(Input::new(&name_input)),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.terminal_profile_working_dir.clone()),
                            )
                            .child(Input::new(&dir_input)),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(text.terminal_profile_startup_command.clone()),
                            )
                            .child(Input::new(&command_input)),
                    ),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("terminal-profile-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("terminal-profile-confirm")
                            .label(text.session_action_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
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

/// Automation page (M22): the native-hooks master switches and the commit
/// co-author preference. The toggles are the same shared values the desktop
/// automation page writes; hook declarations themselves stay in hooks.json.
fn automation_page(text: &ShellText) -> SettingPage {
    let hooks_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).hooks_enabled,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().hooks_enabled = value;
            persist(HOOKS_ENABLED_PATH, serde_json::Value::Bool(value));
            cx.refresh_windows();
        },
    );
    let project_hooks_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).project_hooks_enabled,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().project_hooks_enabled = value;
            persist(HOOKS_PROJECT_PATH, serde_json::Value::Bool(value));
            cx.refresh_windows();
        },
    );
    let coauthor_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).git_commit_coauthor,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().git_commit_coauthor = value;
            persist(GIT_COAUTHOR_PATH, serde_json::Value::Bool(value));
            cx.refresh_windows();
        },
    );

    SettingPage::new(text.settings_automation.clone())
        .icon(IconName::Webhook)
        .resettable(false)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.settings_hooks.clone(), hooks_field)
                .description(text.settings_hooks_description.clone()),
            SettingItem::new(text.settings_hooks_project.clone(), project_hooks_field)
                .description(text.settings_hooks_project_description.clone()),
            SettingItem::new(text.settings_git_coauthor.clone(), coauthor_field)
                .description(text.settings_git_coauthor_description.clone()),
        ]))
}

/// Whether the custom HTTP search service auth mode needs a device
/// credential. `""` and `none` are the shared escape hatch (web
/// `credentialRequired`, core `credential_id_for_provider`); every other
/// mode — including unknown future ids — needs one and fails at search time.
pub(crate) fn websearch_http_credential_required(mode: &str) -> bool {
    !matches!(mode.trim(), "" | "none")
}

/// Dropdown-facing auth mode: the empty string behaves like `none` (the
/// shared config default), anything else stays verbatim so an unknown mode
/// shows what is actually persisted.
fn websearch_http_auth_mode_display(mode: &str) -> SharedString {
    let trimmed = mode.trim();
    SharedString::from(if trimmed.is_empty() { "none" } else { trimmed })
}

/// Web search page (M22): the active provider plus credential management for
/// the key-based providers. The credential itself goes into the shared
/// encrypted vault through the same core service the server and CLI use.
/// T2-9: while the custom HTTP service is selected, its endpoint/auth group
/// is inserted between the provider and credential groups (web
/// `sections.http`); the page is rebuilt per frame, so the group follows the
/// provider and header-mode switches without extra state.
fn websearch_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> SettingPage {
    let provider_field = SettingField::dropdown(
        vec![
            ("exa_mcp_free", text.websearch_provider_free.clone()),
            ("exa_search_api", text.websearch_provider_exa.clone()),
            ("tavily", text.websearch_provider_tavily.clone()),
            (
                "openbitfun_search_http",
                text.websearch_provider_http.clone(),
            ),
        ]
        .into_iter()
        .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
        .collect::<Vec<(SharedString, SharedString)>>(),
        |cx: &App| SharedString::from(ShellConfig::current(cx).web_search_provider.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            cx.global_mut::<ShellConfig>().web_search_provider = value.clone();
            persist(
                WEB_SEARCH_PROVIDER_PATH,
                serde_json::Value::String(value.clone()),
            );
            refresh_web_search_runtime();
            let http_requires_credential = value == "openbitfun_search_http"
                && websearch_http_credential_required(
                    &ShellConfig::current(cx).web_search_http_auth_mode,
                );
            if matches!(value.as_str(), "exa_search_api" | "tavily") || http_requires_credential {
                if let Some(handle) = crate::runtime::runtime_handle() {
                    handle.load_web_search_credential_status(value);
                }
            }
            cx.refresh_windows();
        },
    );

    let owned_text = text.clone();
    let credential_view = view.clone();
    let credential_field = SettingField::render(move |_options, _window, cx| {
        websearch_credential_field(credential_view.clone(), &owned_text, cx)
    });

    let snapshot = ShellConfig::current(cx);
    let mut page = SettingPage::new(text.settings_websearch.clone())
        .icon(IconName::Globe)
        .resettable(false)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.settings_websearch_provider.clone(), provider_field)
                .description(text.settings_websearch_provider_description.clone()),
            SettingItem::new(text.websearch_credential.clone(), credential_field)
                .description(text.websearch_credential_description.clone()),
        ]));
    if snapshot.web_search_provider == "openbitfun_search_http" {
        page = page.group(websearch_http_group(text, view, &snapshot));
    }
    page
}

/// The endpoint/auth group for the custom HTTP search service: URL, auth
/// mode, and — for the header mode — the credential header name. Every field
/// persists immediately, so a credential saved afterwards always validates
/// against the mode that is already on disk (web saves its draft first for
/// the same reason).
fn websearch_http_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    snapshot: &ShellConfig,
) -> SettingGroup {
    let endpoint_field = SettingField::input(
        |cx: &App| SharedString::from(ShellConfig::current(cx).web_search_http_endpoint.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            cx.global_mut::<ShellConfig>().web_search_http_endpoint = value.clone();
            persist(
                WEB_SEARCH_HTTP_ENDPOINT_PATH,
                serde_json::Value::String(value),
            );
            refresh_web_search_runtime();
            cx.refresh_windows();
        },
    );

    let status_view = view.clone();
    let auth_mode_field = SettingField::dropdown(
        vec![
            ("none", text.websearch_auth_none.clone()),
            ("bearer", text.websearch_auth_bearer.clone()),
            ("header", text.websearch_auth_header.clone()),
        ]
        .into_iter()
        .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
        .collect::<Vec<(SharedString, SharedString)>>(),
        |cx: &App| {
            websearch_http_auth_mode_display(&ShellConfig::current(cx).web_search_http_auth_mode)
        },
        move |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            let previously_required = websearch_http_credential_required(
                &ShellConfig::current(cx).web_search_http_auth_mode,
            );
            cx.global_mut::<ShellConfig>().web_search_http_auth_mode = value.clone();
            persist(
                WEB_SEARCH_HTTP_AUTH_MODE_PATH,
                serde_json::Value::String(value.clone()),
            );
            refresh_web_search_runtime();
            if websearch_http_credential_required(&value) {
                if !previously_required {
                    if let Some(handle) = crate::runtime::runtime_handle() {
                        handle.load_web_search_credential_status(
                            "openbitfun_search_http".to_string(),
                        );
                    }
                }
            } else {
                // No credential applies anymore; drop a stale status so the
                // credential row cannot claim a configured secret.
                if let Some(shell) = status_view.upgrade() {
                    shell.update(cx, |shell, _| shell.websearch_status = None);
                }
            }
            cx.refresh_windows();
        },
    );

    let mut items = vec![
        SettingItem::new(text.websearch_http_endpoint.clone(), endpoint_field)
            .description(text.websearch_http_endpoint_description.clone()),
        SettingItem::new(text.websearch_http_auth_mode.clone(), auth_mode_field),
    ];
    if snapshot.web_search_http_auth_mode.trim() == "header" {
        let header_name_field = SettingField::input(
            |cx: &App| {
                SharedString::from(ShellConfig::current(cx).web_search_http_header_name.clone())
            },
            |value: SharedString, cx: &mut App| {
                let value = value.to_string();
                cx.global_mut::<ShellConfig>().web_search_http_header_name = value.clone();
                persist(
                    WEB_SEARCH_HTTP_HEADER_NAME_PATH,
                    serde_json::Value::String(value),
                );
                refresh_web_search_runtime();
                cx.refresh_windows();
            },
        );
        items.push(
            SettingItem::new(text.websearch_http_header_name.clone(), header_name_field)
                .description(text.websearch_http_header_name_description.clone()),
        );
    }

    SettingGroup::new()
        .title(text.settings_websearch_http.clone())
        .description(text.settings_websearch_http_description.clone())
        .items(items)
}

/// Re-reads the shared config and pushes the web-search section into the
/// global provider runtime, so a provider change takes effect without a
/// restart (the credential save path does this itself).
fn refresh_web_search_runtime() {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    handle.tokio_handle().spawn(async move {
        let Ok(service) = get_global_config_service().await else {
            return;
        };
        if let Ok(config) = service.get_config::<GlobalConfig>(None).await {
            openbitfun_core::service::web_search::refresh_global_web_search_runtime(&config.ai)
                .await;
        }
    });
}

/// The credential row for the selected provider: a status line, an input and
/// a save button for the key-based providers. The input entity lives on the
/// window (dialog-builder convention: the render closure runs every frame).
fn websearch_credential_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let Some(shell) = view.upgrade() else {
        return div().into_any_element();
    };
    let provider = ShellConfig::current(cx).web_search_provider;
    // T2-9: the custom HTTP service shows the credential row only when its
    // auth mode needs one; without a credential it behaves like the free
    // provider.
    let needs_credential_ui = match provider.as_str() {
        "exa_search_api" | "tavily" => true,
        "openbitfun_search_http" => {
            websearch_http_credential_required(&ShellConfig::current(cx).web_search_http_auth_mode)
        }
        _ => false,
    };
    let input = shell.read(cx).websearch_credential_input.clone();
    let status = shell.read(cx).websearch_status.clone();

    let mut column = div().v_flex().gap_2().w_full();
    if needs_credential_ui {
        let configured = matches!(&status, Some((p, true)) if p == &provider);
        column = column.child(
            div()
                .text_sm()
                .text_color(if configured {
                    cx.theme().muted_foreground
                } else {
                    cx.theme().danger
                })
                .child(if configured {
                    text.websearch_configured.clone()
                } else {
                    text.websearch_not_configured.clone()
                }),
        );
        let save_view = view.clone();
        column = column.child(Input::new(&input)).child(
            Button::new("websearch-save-credential")
                .label(text.websearch_save.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _window, cx| {
                    let Some(shell) = save_view.upgrade() else {
                        return;
                    };
                    let secret = shell
                        .read(cx)
                        .websearch_credential_input
                        .read(cx)
                        .value()
                        .trim()
                        .to_string();
                    if secret.is_empty() {
                        tracing::warn!("Ignoring an empty WebSearch credential");
                        return;
                    }
                    let provider = ShellConfig::current(cx).web_search_provider.clone();
                    if let Some(handle) = crate::runtime::runtime_handle() {
                        handle.save_web_search_credential(provider, secret);
                    }
                }),
        );
    } else {
        // exa_mcp_free, credential-less HTTP auth modes and anything
        // unrecognized need no credential.
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.websearch_no_credential.clone()),
        );
    }
    column.into_any_element()
}

/// Usage statistics page (M22): global token usage for a chosen time range —
/// totals plus per-model rows from the same core service the desktop
/// dashboard reads. The snapshot lives on the window; the settings-open path
/// and the refresh button fetch.
fn usage_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingPage {
    let owned_text = text.clone();
    let stats_view = view.clone();
    let range_field = SettingField::render(move |_options, _window, cx| {
        usage_stats_field(stats_view.clone(), &owned_text, cx)
    });
    let activity_view = view;
    let activity_text = text.clone();
    let activity_field = SettingField::render(move |_options, _window, cx| {
        usage_activity_field(activity_view.clone(), &activity_text, cx)
    });

    SettingPage::new(text.settings_usage.clone())
        .icon(IconName::ChartColumn)
        .resettable(false)
        .group(SettingGroup::new().items(vec![SettingItem::new(
            text.settings_usage_range.clone(),
            range_field,
        )]))
        .group(SettingGroup::new().items(vec![SettingItem::new(
            text.usage_activity_title.clone(),
            activity_field,
        )
        .description(text.usage_activity_description.clone())]))
}

/// One selectable time range of the usage page; the values are the shared
/// service's `time_range` strings.
const USAGE_RANGES: &[(&str, &str)] = &[
    ("today", "settings_usage_today"),
    ("thisWeek", "settings_usage_week"),
    ("thisMonth", "settings_usage_month"),
    ("all", "settings_usage_all"),
];

/// Maps a range value onto its localized label; unknown values render as-is.
fn usage_range_label(value: &str, text: &ShellText) -> String {
    match value {
        "today" => text.settings_usage_today.clone(),
        "thisWeek" => text.settings_usage_week.clone(),
        "thisMonth" => text.settings_usage_month.clone(),
        "all" => text.settings_usage_all.clone(),
        other => other.to_string(),
    }
}

/// Compact token count for chart axis labels: `960`, `12.3k`, `3.4M`, `1.2B`.
fn format_token_compact(value: f64) -> String {
    let (divisor, suffix) = if value >= 1_000_000_000. {
        (1_000_000_000., "B")
    } else if value >= 1_000_000. {
        (1_000_000., "M")
    } else if value >= 1_000. {
        (1_000., "k")
    } else {
        return format!("{}", value.round() as u64);
    };
    let scaled = value / divisor;
    if scaled >= 100. {
        format!("{scaled:.0}{suffix}")
    } else {
        format!("{scaled:.1}{suffix}")
    }
}

/// Smallest "nice" axis ceiling >= value (the web dashboard's `niceMax`):
/// the leading digit rounds up to 1 / 2 / 2.5 / 5 / 10 times a power of ten.
fn nice_max(value: f64) -> f64 {
    if value <= 0. {
        return 1.;
    }
    let magnitude = 10f64.powf(value.log10().floor());
    let normalized = value / magnitude;
    let step = if normalized <= 1. {
        1.
    } else if normalized <= 2. {
        2.
    } else if normalized <= 2.5 {
        2.5
    } else if normalized <= 5. {
        5.
    } else {
        10.
    };
    step * magnitude
}

/// One trend bar label: `MM-DD`, or `MM-DD HH:00` for hourly buckets.
fn trend_bucket_label(bucket: chrono::DateTime<chrono::Utc>, hourly: bool) -> String {
    if hourly {
        bucket.format("%m-%d %H:00").to_string()
    } else {
        bucket.format("%m-%d").to_string()
    }
}

/// UTC window for the activity heatmap: the day after the same day-of-month
/// six months back, through today's midnight (web `calendarWindow` port,
/// UTC-anchored — the shell requests no IANA zone, matching the service's
/// default bucketing).
pub(crate) fn activity_window_start(
    today: chrono::DateTime<chrono::Utc>,
) -> chrono::DateTime<chrono::Utc> {
    use chrono::{Datelike, TimeZone};
    let end_day = today.date_naive();
    let first_of_month = end_day.with_day(1).expect("first day exists");
    let start_month = first_of_month
        .checked_sub_months(chrono::Months::new(6))
        .unwrap_or(first_of_month);
    let next_month = if start_month.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(start_month.year() + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(start_month.year(), start_month.month() + 1, 1)
    }
    .expect("valid first day");
    let last_day = next_month.pred_opt().expect("date exists");
    let day = end_day.day().min(last_day.day()) + 1;
    // A day past the month's end rolls over, exactly like the web's
    // `setUTCDate` overflow (e.g. Oct 31 -> May 1).
    let start_day = start_month.with_day(day).unwrap_or(next_month);
    start_day
        .and_hms_opt(0, 0, 0)
        .map(|naive| chrono::Utc.from_utc_datetime(&naive))
        .unwrap_or(today)
}

/// One day cell of the activity heatmap.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActivityCell {
    pub(crate) tokens: u64,
    pub(crate) level: u8,
}

/// GitHub-style activity grid: Monday-first weeks, `week_count` columns of
/// seven day rows. Cells outside the window stay `None` (padding).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActivityGrid {
    /// Column-major: index = week * 7 + weekday (row 0 = Monday).
    pub(crate) cells: Vec<Option<ActivityCell>>,
    pub(crate) week_count: usize,
    pub(crate) total_tokens: u64,
}

/// Aggregates trend points into the activity grid (web `buildUsageActivity`
/// port): daily input+output totals (cache reads/writes already ride inside
/// the input figure), levels 0-4 scaled against the peak day.
pub(crate) fn build_activity_grid(
    points: &[openbitfun_services_core::token_usage::UsageTrendPoint],
    today: chrono::DateTime<chrono::Utc>,
) -> ActivityGrid {
    use chrono::{Datelike, Duration};
    let start = activity_window_start(today);
    let end_day = today.date_naive();
    let start_day = start.date_naive();
    let mut totals: std::collections::HashMap<chrono::NaiveDate, u64> =
        std::collections::HashMap::new();
    for point in points {
        let tokens = point.input_tokens + point.output_tokens;
        totals
            .entry(point.bucket.date_naive())
            .and_modify(|total| *total += tokens)
            .or_insert(tokens);
    }
    let offset = start.weekday().num_days_from_monday() as i64;
    let grid_start = start_day - Duration::days(offset);
    let day_count = (end_day - start_day).num_days() + 1;
    let week_count = ((day_count + offset) as usize).div_ceil(7);
    let mut cells = Vec::with_capacity(week_count * 7);
    let mut total_tokens = 0u64;
    for index in 0..week_count * 7 {
        let day = grid_start + Duration::days(index as i64);
        if day < start_day || day > end_day {
            cells.push(None);
            continue;
        }
        let tokens = totals.get(&day).copied().unwrap_or(0);
        total_tokens += tokens;
        cells.push(Some(ActivityCell { tokens, level: 0 }));
    }
    let peak = cells
        .iter()
        .flatten()
        .map(|cell| cell.tokens)
        .max()
        .unwrap_or(0);
    for cell in cells.iter_mut().flatten() {
        cell.level = if cell.tokens > 0 {
            let scaled = (cell.tokens as f64 / peak as f64 * 4.).ceil();
            (scaled as u8).min(4)
        } else {
            0
        };
    }
    ActivityGrid {
        cells,
        week_count,
        total_tokens,
    }
}

/// Heatmap cell color: the web heatmap ramps `status-info-emphasis` over the
/// border color at levels 1-4 (40/60/80/100%).
fn activity_level_color(level: u8, cx: &App) -> gpui_kit::Hsla {
    match level {
        0 => cx.theme().border,
        1 => cx.theme().info.opacity(0.4),
        2 => cx.theme().info.opacity(0.6),
        3 => cx.theme().info.opacity(0.8),
        _ => cx.theme().info,
    }
}

/// The trend chart: stacked input/output bars over the last (up to) 90
/// buckets, mirroring the web dashboard's TrendChart — same visible-bucket
/// cap, nice-max axis, range line, and legend. Self-drawn with plain divs.
fn render_usage_trend(
    stats: &openbitfun_services_core::token_usage::UsageStatistics,
    text: &ShellText,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    use openbitfun_services_core::token_usage::UsageGranularity;
    const MAX_BARS: usize = 90;
    const PLOT_HEIGHT: f32 = 120.;
    let visible: Vec<_> = stats.trend.iter().rev().take(MAX_BARS).rev().collect();
    if visible.is_empty() {
        return div().into_any_element();
    }
    let max_tokens = nice_max(
        visible
            .iter()
            .map(|point| (point.input_tokens + point.output_tokens) as f64)
            .fold(0., f64::max),
    ) as f32;
    let visible_total: u64 = visible
        .iter()
        .map(|point| point.input_tokens + point.output_tokens)
        .sum();
    let partial = visible.len() < stats.trend.len() || visible_total < stats.total_tokens;
    let hourly = stats.granularity == UsageGranularity::Hour;
    let first = trend_bucket_label(visible[0].bucket, hourly);
    let last = trend_bucket_label(visible[visible.len() - 1].bucket, hourly);
    let mut range_line = i18n::usage_trend_range(&first, &last, cx);
    if partial {
        range_line.push_str(" · ");
        range_line.push_str(&text.usage_trend_partial);
    }

    let mut plot = div().relative().flex_1().h(px(PLOT_HEIGHT));
    for fraction in [1.0f32, 0.5, 0.0] {
        plot = plot.child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px((1. - fraction) * PLOT_HEIGHT))
                .h(px(1.))
                .bg(cx.theme().border),
        );
    }
    let mut bars = div().absolute().inset_0().flex().items_end();
    for point in &visible {
        let input_height = if point.input_tokens > 0 {
            (point.input_tokens as f32 / max_tokens * PLOT_HEIGHT).max(1.)
        } else {
            0.
        };
        let output_height = if point.output_tokens > 0 {
            (point.output_tokens as f32 / max_tokens * PLOT_HEIGHT).max(1.)
        } else {
            0.
        };
        bars = bars.child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_end()
                .justify_center()
                .child(
                    div()
                        .v_flex()
                        .justify_end()
                        .w(px(14.))
                        .max_w_full()
                        .when(output_height > 0., |bar| {
                            bar.child(div().w_full().h(px(output_height)).bg(cx.theme().chart_2))
                        })
                        .when(input_height > 0., |bar| {
                            bar.child(div().w_full().h(px(input_height)).bg(cx.theme().info))
                        }),
                ),
        );
    }
    plot = plot.child(bars);
    let axis_label = |value: f64| {
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(format_token_compact(value))
    };
    let gutter = div()
        .relative()
        .w(px(44.))
        .h(px(PLOT_HEIGHT))
        .flex_shrink_0()
        .child(
            axis_label(max_tokens as f64)
                .absolute()
                .top(px(0.))
                .left_0(),
        )
        .child(
            axis_label(max_tokens as f64 * 0.5)
                .absolute()
                .top(px(PLOT_HEIGHT * 0.5 - 7.))
                .left_0(),
        )
        .child(axis_label(0.).absolute().bottom(px(0.)).left_0());

    let mid = visible.len() / 2;
    let mut column = div().v_flex().gap_1().w_full();
    column = column.child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(text.usage_trend_title.clone()),
    );
    column = column.child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(range_line),
    );
    column = column.child(
        div()
            .flex()
            .items_stretch()
            .gap_2()
            .child(gutter)
            .child(plot),
    );
    column = column.child(
        div()
            .flex()
            .justify_between()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(div().child(first))
            .child(div().child(trend_bucket_label(visible[mid].bucket, hourly)))
            .child(div().child(last)),
    );
    column = column.child(
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().w(px(8.)).h(px(8.)).rounded_sm().bg(cx.theme().info))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text.usage_trend_input.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .w(px(8.))
                            .h(px(8.))
                            .rounded_sm()
                            .bg(cx.theme().chart_2),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text.usage_trend_output.clone()),
                    ),
            ),
    );
    column.into_any_element()
}

/// The activity heatmap field: renders the six-month snapshot from the window
/// state; the fetch is kicked off when the settings view opens (same path as
/// the range statistics), the refresh button re-fetches both.
pub(crate) fn usage_activity_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let Some(shell) = view.upgrade() else {
        return div().into_any_element();
    };
    let activity = shell.read(cx).usage_activity.clone();
    let Some(activity) = activity else {
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.settings_usage_loading.clone())
            .into_any_element();
    };
    match activity {
        Ok(stats) => render_usage_activity(&stats, text, cx),
        Err(message) => {
            tracing::warn!("Usage activity snapshot unavailable: {message}");
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.settings_usage_empty.clone())
                .into_any_element()
        }
    }
}

/// The activity heatmap: GitHub-style day grid (Monday-first weeks, 11px
/// round cells, levels 0-4), weekday labels, and the less-to-more legend with
/// the window total. Month labels and per-day tooltips stay web-only.
fn render_usage_activity(
    stats: &openbitfun_services_core::token_usage::UsageStatistics,
    text: &ShellText,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    const CELL: f32 = 11.;
    const GAP: f32 = 3.;
    let grid = build_activity_grid(&stats.trend, chrono::Utc::now());
    let muted = cx.theme().muted_foreground;
    let label = |value: String| div().text_xs().text_color(muted).child(value);

    let mut weekday_column = div().v_flex().gap(px(GAP)).flex_shrink_0();
    for row in 0..7usize {
        let name = match row {
            0 => Some(text.usage_activity_mon.clone()),
            2 => Some(text.usage_activity_wed.clone()),
            4 => Some(text.usage_activity_fri.clone()),
            6 => Some(text.usage_activity_sun.clone()),
            _ => None,
        };
        weekday_column = weekday_column.child(
            div()
                .w(px(20.))
                .h(px(CELL))
                .flex()
                .items_center()
                .when_some(name, |row, name| row.child(label(name))),
        );
    }
    let mut grid_element = div().flex().gap(px(GAP)).min_w_0();
    for week in 0..grid.week_count {
        let mut week_column = div().v_flex().gap(px(GAP));
        for row in 0..7usize {
            week_column = week_column.child(match &grid.cells[week * 7 + row] {
                Some(cell) => div()
                    .w(px(CELL))
                    .h(px(CELL))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(activity_level_color(cell.level, cx)),
                None => div().w(px(CELL)).h(px(CELL)).flex_shrink_0(),
            });
        }
        grid_element = grid_element.child(week_column);
    }

    let mut legend = div().flex().items_center().gap_1();
    legend = legend.child(label(text.usage_activity_less.clone()));
    for level in 0..=4u8 {
        legend = legend.child(
            div()
                .w(px(CELL))
                .h(px(CELL))
                .rounded_full()
                .bg(activity_level_color(level, cx)),
        );
    }
    legend = legend.child(label(text.usage_activity_more.clone()));

    let mut column = div().v_flex().gap_2().w_full();
    column = column.child(
        div()
            .flex()
            .items_start()
            .gap(px(2.))
            .child(weekday_column)
            .child(grid_element),
    );
    column = column.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(label(i18n::usage_activity_total(grid.total_tokens, cx)))
            .child(legend),
    );
    column.into_any_element()
}

fn usage_stats_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let Some(shell) = view.upgrade() else {
        return div().into_any_element();
    };
    let (range, loading, stats) = {
        let shell_ref = shell.read(cx);
        (
            shell_ref.usage_range.clone(),
            shell_ref.usage_loading,
            shell_ref.usage_stats.clone(),
        )
    };
    // The menu builder and item closures are `Fn` and outlive this render, so
    // everything they touch is cloned in (D70).

    let mut column = div().v_flex().gap_2().w_full();
    let mut controls = div().flex().items_center().gap_2();
    let menu_view = view.clone();
    let menu_text = text.clone();
    let menu_range = range.clone();
    controls = controls.child(
        Button::new("usage-range")
            .label(usage_range_label(&range, text))
            .dropdown_caret(true)
            .outline()
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
                let mut menu = menu;
                for (value, _key) in USAGE_RANGES {
                    let value = (*value).to_string();
                    let selected = value == menu_range;
                    let item_view = menu_view.clone();
                    let item_text = menu_text.clone();
                    let item_value = value.clone();
                    menu = menu.item(
                        PopupMenuItem::new(usage_range_label(&value, &item_text))
                            .checked(selected)
                            .on_click(move |_, _window, cx| {
                                if let Some(target) = item_view.upgrade() {
                                    target.update(cx, |this, cx| {
                                        this.usage_range = item_value.clone();
                                        this.usage_loading = true;
                                        cx.notify();
                                    });
                                }
                                if let Some(handle) = crate::runtime::runtime_handle() {
                                    handle.fetch_usage_stats(item_value.clone());
                                }
                            }),
                    );
                }
                menu
            }),
    );
    let refresh_view = view.clone();
    controls = controls.child(
        Button::new("usage-refresh")
            .label(text.settings_usage_refresh.clone())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(move |_, _window, cx| {
                let Some(target) = refresh_view.upgrade() else {
                    return;
                };
                target.update(cx, |this, _| {
                    this.usage_loading = true;
                    this.usage_activity_loading = true;
                });
                let range = target.read(cx).usage_range.clone();
                if let Some(handle) = crate::runtime::runtime_handle() {
                    handle.fetch_usage_stats(range);
                    handle.fetch_usage_activity();
                }
            }),
    );
    column = column.child(controls);

    if loading {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.settings_usage_loading.clone()),
        );
        return column.into_any_element();
    }
    let Some(stats) = stats else {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.settings_usage_empty.clone()),
        );
        return column.into_any_element();
    };

    let mut totals = div().flex().flex_wrap().gap_4().w_full();
    let total = |label: String, value: String| {
        div()
            .v_flex()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(value),
            )
    };
    totals = totals
        .child(total(
            text.settings_usage_requests.clone(),
            stats.total_requests.to_string(),
        ))
        .child(total(
            text.usage_total_tokens.clone(),
            stats.total_tokens.to_string(),
        ))
        .child(total(
            text.usage_input_tokens.clone(),
            stats.total_input_tokens.to_string(),
        ))
        .child(total(
            text.usage_output_tokens.clone(),
            stats.total_output_tokens.to_string(),
        ));
    if stats.total_cache_reported_input_tokens > 0 {
        let rate =
            stats.total_cached_tokens as f64 / stats.total_cache_reported_input_tokens as f64;
        totals = totals.child(total(
            text.settings_usage_cache_hit.clone(),
            format!("{:.0}%", rate * 100.),
        ));
    }
    column = column.child(totals);
    column = column.child(render_usage_trend(&stats, text, cx));

    if !stats.by_model.is_empty() {
        column = column.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text.usage_models.clone()),
        );
        let mut rows = div().v_flex().gap_1().w_full();
        for model in stats.by_model.iter().take(20) {
            let mut facts = format!("{} · {}", model.requests, model.tokens);
            if let Some(rate) = model.cache_hit_rate {
                facts.push_str(&format!(" · {:.0}%", rate * 100.));
            }
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(model.name.clone()),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(facts),
                    ),
            );
        }
        column = column.child(rows);
    }
    column.into_any_element()
}

/// Archived sessions page (M22): the active workspace's archived sessions
/// with restore and delete. The rows reuse the sidebar's session snapshot
/// (archived rows are already carried for the show-archived toggle), and the
/// actions are the same SDK calls the sidebar uses.
fn archived_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingPage {
    let owned_text = text.clone();
    let list_field = SettingField::render(move |_options, window, cx| {
        archived_sessions_field(view.clone(), &owned_text, window, cx)
    });

    SettingPage::new(text.settings_archived.clone())
        .icon(IconName::Archive)
        .resettable(false)
        .group(SettingGroup::new().items(vec![SettingItem::new(
            text.settings_archived_list.clone(),
            list_field,
        )
        .description(text.settings_archived_description.clone())]))
}

/// The archived list body, exposed for the headless render test; the page
/// itself stays private behind [`render_page`].
pub(crate) fn archived_sessions_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    _window: &mut gpui_kit::Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let Some(shell) = view.upgrade() else {
        return div().into_any_element();
    };
    let rows: Vec<(String, String)> = shell
        .read(cx)
        .sessions
        .iter()
        .filter(|row| row.archived)
        .map(|row| (row.id.clone(), row.name.clone()))
        .collect();

    let mut column = div().v_flex().gap_2().w_full();
    if rows.is_empty() {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.settings_archived_empty.clone()),
        );
        return column.into_any_element();
    }
    // The bulk action sits above the list, same slot as the web page's header
    // button; the destructive step goes through its own confirm dialog.
    let delete_all_view = view.clone();
    column = column.child(
        div().flex().justify_end().child(
            Button::new("archived-delete-all")
                .label(text.settings_archived_delete_all.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, window, cx| {
                    open_archived_delete_all_dialog(delete_all_view.clone(), window, cx);
                }),
        ),
    );
    for (session_id, name) in rows {
        let restore_view = view.clone();
        let restore_id = session_id.clone();
        let delete_view = view.clone();
        let delete_id = session_id.clone();
        column = column.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(name),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .flex_shrink_0()
                        .child(
                            Button::new(SharedString::from(format!(
                                "archived-restore-{session_id}"
                            )))
                            .label(text.settings_archived_restore.clone())
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(move |_, _window, cx| {
                                let Some(target) = restore_view.upgrade() else {
                                    return;
                                };
                                let tx = target.read(cx).event_tx.clone();
                                if let Some(handle) = crate::runtime::runtime_handle() {
                                    tracing::info!("Restoring archived session {restore_id}");
                                    handle.set_session_archived(restore_id.clone(), false, tx);
                                }
                            }),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "archived-delete-{session_id}"
                            )))
                            .label(text.session_delete.clone())
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(move |_, window, cx| {
                                open_archived_delete_dialog(
                                    delete_view.clone(),
                                    delete_id.clone(),
                                    window,
                                    cx,
                                );
                            }),
                        ),
                ),
        );
    }
    column.into_any_element()
}

/// Confirm dialog for deleting one archived session; the destructive action
/// never runs from the row directly (same shape as the MCP delete dialog).
fn open_archived_delete_dialog(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    session_id: String,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    shell.update(cx, |this, cx| {
        this.archived_delete_target = Some(session_id.clone());
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        let Some(target) = confirm_view.upgrade() else {
            return;
        };
        let pending = target.read(cx).archived_delete_target.clone();
        target.update(cx, |this, cx| {
            this.archived_delete_target = None;
            cx.notify();
        });
        if let Some(session_id) = pending {
            let tx = confirm_view
                .upgrade()
                .map(|target| target.read(cx).event_tx.clone());
            if let (Some(handle), Some(tx)) = (crate::runtime::runtime_handle(), tx) {
                tracing::info!("Deleting archived session {session_id}");
                handle.delete_session(session_id, tx);
            }
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        if let Some(target) = cancel_view.upgrade() {
            target.update(cx, |this, cx| {
                this.archived_delete_target = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.settings_archived_delete_title.clone()))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(text.session_delete_warning.clone()),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("archived-delete-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("archived-delete-confirm")
                            .label(text.session_delete.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Confirm dialog for deleting every archived session of this workspace. The
/// ids are read at confirm time so the dialog never acts on a stale list;
/// each deletion goes through the same per-session call as the row action.
fn open_archived_delete_all_dialog(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let count = shell
        .read(cx)
        .sessions
        .iter()
        .filter(|row| row.archived)
        .count();
    if count == 0 {
        return;
    }

    let confirm_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        if let Some(target) = confirm_view.upgrade() {
            let ids: Vec<String> = target
                .read(cx)
                .sessions
                .iter()
                .filter(|row| row.archived)
                .map(|row| row.id.clone())
                .collect();
            let tx = target.read(cx).event_tx.clone();
            if let Some(handle) = crate::runtime::runtime_handle() {
                tracing::info!("Deleting all {} archived sessions", ids.len());
                for session_id in ids {
                    handle.delete_session(session_id, tx.clone());
                }
            }
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.settings_archived_delete_all_confirm_title.clone()))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(i18n::archived_delete_all_confirm_message(count, cx)),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("archived-delete-all-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("archived-delete-all-confirm")
                            .label(text.settings_archived_delete_all.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Diagnostics page (M22): the runtime log level (applied live through the
/// tracing reload handle) and the shared log directory. The shell logs to
/// stdout; the desktop's file bundle export is a desktop capability.
fn diagnostics_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingPage {
    let level_field = SettingField::dropdown(
        ["trace", "debug", "info", "warn", "error", "off"]
            .into_iter()
            .map(|level| (SharedString::from(level), SharedString::from(level)))
            .collect::<Vec<(SharedString, SharedString)>>(),
        |cx: &App| SharedString::from(ShellConfig::current(cx).logging_level.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            cx.global_mut::<ShellConfig>().logging_level = value.clone();
            crate::shell::apply_log_level(&value);
            persist(LOGGING_LEVEL_PATH, serde_json::Value::String(value));
            cx.refresh_windows();
        },
    );

    let owned_text = text.clone();
    let logs_field = SettingField::render(move |_options, _window, cx| {
        let Some(manager) = openbitfun_core::infrastructure::try_get_path_manager_arc()
            .ok()
            .map(|manager| manager.logs_dir())
        else {
            return div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("—")
                .into_any_element();
        };
        let display = manager.display().to_string();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(display),
            )
            .child(
                Button::new("diagnostics-open-logs")
                    .label(owned_text.settings_logs_open.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _window, _cx| {
                        open_logs_directory();
                    }),
            )
            .into_any_element()
    });
    // Keep the weak handle alive for future window-bound diagnostics state.
    let _ = view;

    SettingPage::new(text.settings_diagnostics.clone())
        .icon(IconName::Activity)
        .resettable(false)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.settings_logging_level.clone(), level_field)
                .description(text.settings_logging_level_description.clone()),
            SettingItem::new(text.settings_logs_dir.clone(), logs_field)
                .description(text.settings_diagnostics_note.clone()),
        ]))
}

/// Opens the shared log directory in the platform file manager. Failures are
/// logged; there is nothing to show in the window for them.
fn open_logs_directory() {
    let Ok(manager) = openbitfun_core::infrastructure::try_get_path_manager_arc() else {
        return;
    };
    let dir = manager.logs_dir();
    match openbitfun_services_core::process_manager::create_command("xdg-open")
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("Opened log directory {}", dir.display()),
        Err(error) => tracing::warn!("Failed to open {}: {error}", dir.display()),
    }
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
    use chrono::TimeZone as _;
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
    fn close_behavior_follows_the_desktop_domain() {
        // `quit` quits; everything else (the shared default `minimize_to_tray`,
        // the desktop-only `ask`, or any unknown value) minimizes.
        assert_eq!(parse_close_behavior(CLOSE_QUIT), CloseBehavior::Quit);
        for raw in [CLOSE_MINIMIZE, "ask", "", "nonsense"] {
            assert_eq!(parse_close_behavior(raw), CloseBehavior::MinimizeToTray);
        }
    }

    #[test]
    fn usage_range_label_covers_the_service_domain() {
        let text = sample_text();
        assert_eq!(usage_range_label("today", &text), text.settings_usage_today);
        assert_eq!(
            usage_range_label("thisWeek", &text),
            text.settings_usage_week
        );
        assert_eq!(
            usage_range_label("thisMonth", &text),
            text.settings_usage_month
        );
        assert_eq!(usage_range_label("all", &text), text.settings_usage_all);
        // Unknown values stay verbatim instead of pretending to be a choice.
        assert_eq!(usage_range_label("custom", &text), "custom");
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
        // M22 general/automation/websearch/diagnostics fields.
        config.app.close_button_behavior = "ask".to_string();
        config.app.auto_update = false;
        config.app.notifications.dialog_completion_notify = false;
        config.app.hooks.enabled = false;
        config.app.hooks.project_hooks_enabled = true;
        config.app.ai_experience.enable_git_commit_coauthor = false;
        config.app.ai_experience.quick_actions = vec![
            openbitfun_core::service::config::types::AiExperienceQuickAction {
                id: "commit".to_string(),
                label: "Commit".to_string(),
                prompt: "Commit all current code changes".to_string(),
                enabled: false,
            },
        ];
        config.ai.web_search.provider = "tavily".to_string();
        config
            .ai
            .web_search
            .providers
            .openbitfun_search_http
            .endpoint = "https://search.example.com/search".to_string();
        config
            .ai
            .web_search
            .providers
            .openbitfun_search_http
            .auth
            .mode = "header".to_string();
        config
            .ai
            .web_search
            .providers
            .openbitfun_search_http
            .auth
            .header_name = "X-Search-Key".to_string();
        config.app.logging.level = "debug".to_string();

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
        assert_eq!(snapshot.close_behavior_raw, "ask");
        assert!(!snapshot.auto_update);
        assert!(!snapshot.dialog_completion_notify);
        assert!(!snapshot.hooks_enabled);
        assert!(snapshot.project_hooks_enabled);
        assert!(!snapshot.git_commit_coauthor);
        assert_eq!(snapshot.web_search_provider, "tavily");
        assert_eq!(
            snapshot.web_search_http_endpoint,
            "https://search.example.com/search"
        );
        assert_eq!(snapshot.web_search_http_auth_mode, "header");
        assert_eq!(snapshot.web_search_http_header_name, "X-Search-Key");
        assert_eq!(snapshot.logging_level, "debug");
        assert_eq!(snapshot.quick_actions.len(), 1);
        assert_eq!(snapshot.quick_actions[0].id, "commit");
        assert!(!snapshot.quick_actions[0].enabled);
    }

    #[test]
    fn http_credential_requirement_follows_the_shared_mode_contract() {
        // `""` and `none` are the shared no-credential modes; every other
        // mode string — bearer, header, or an unknown future id — needs one.
        assert!(!websearch_http_credential_required(""));
        assert!(!websearch_http_credential_required("none"));
        assert!(websearch_http_credential_required("bearer"));
        assert!(websearch_http_credential_required("header"));
        assert!(websearch_http_credential_required("future_mode"));
        // Display normalizes only the empty string; unknown ids stay verbatim.
        assert_eq!(websearch_http_auth_mode_display(""), "none");
        assert_eq!(websearch_http_auth_mode_display(" none "), "none");
        assert_eq!(websearch_http_auth_mode_display("header"), "header");
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

    fn trend_point(
        bucket: chrono::DateTime<chrono::Utc>,
        input: u64,
        output: u64,
    ) -> openbitfun_services_core::token_usage::UsageTrendPoint {
        openbitfun_services_core::token_usage::UsageTrendPoint {
            bucket,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cache_hit_rate: None,
        }
    }

    #[test]
    fn nice_max_follows_the_web_axis_steps() {
        assert_eq!(nice_max(0.), 1.);
        assert_eq!(nice_max(4.), 5.);
        assert_eq!(nice_max(2.2), 2.5);
        assert_eq!(nice_max(260.), 500.);
        assert_eq!(nice_max(7_500.), 10_000.);
    }

    #[test]
    fn compact_token_labels_abbreviate_large_counts() {
        assert_eq!(format_token_compact(999.), "999");
        assert_eq!(format_token_compact(1_000.), "1.0k");
        assert_eq!(format_token_compact(12_340.), "12.3k");
        assert_eq!(format_token_compact(150_000.), "150k");
        assert_eq!(format_token_compact(3_400_000.), "3.4M");
        assert_eq!(format_token_compact(1_200_000_000.), "1.2B");
    }

    #[test]
    fn trend_labels_show_times_only_for_hourly_buckets() {
        let bucket = chrono::Utc
            .with_ymd_and_hms(2026, 10, 11, 9, 30, 0)
            .unwrap();
        assert_eq!(trend_bucket_label(bucket, false), "10-11");
        assert_eq!(trend_bucket_label(bucket, true), "10-11 09:00");
    }

    #[test]
    fn activity_window_starts_a_day_after_six_months_back() {
        // Apr 12 2026 is the day after "Oct 11 minus six months" (Apr 11).
        let today = chrono::Utc
            .with_ymd_and_hms(2026, 10, 11, 15, 0, 0)
            .unwrap();
        let start = activity_window_start(today);
        assert_eq!(start.to_rfc3339(), "2026-04-12T00:00:00+00:00");
        // A day past the shorter start month rolls over like setUTCDate:
        // Mar 31 -> Sep 11 last day is 30 -> day 31 rolls to Oct 1.
        let today = chrono::Utc.with_ymd_and_hms(2026, 3, 31, 8, 0, 0).unwrap();
        let start = activity_window_start(today);
        assert_eq!(start.to_rfc3339(), "2025-10-01T00:00:00+00:00");
    }

    #[test]
    fn activity_grid_buckets_days_monday_first_with_levels() {
        let today = chrono::Utc
            .with_ymd_and_hms(2026, 10, 11, 23, 0, 0)
            .unwrap();
        let day = |month: u32, day: u32| {
            chrono::Utc
                .with_ymd_and_hms(2026, month, day, 0, 0, 0)
                .unwrap()
        };
        // Sep 1 carries the peak, Sep 2 a quarter of it (level ceil(0.25*4)=1).
        let points = vec![
            trend_point(day(9, 1), 80, 20),
            trend_point(day(9, 2), 25, 0),
        ];
        let grid = build_activity_grid(&points, today);

        // Window Apr 12..Oct 11 2026 (183 days); start is a Sunday, so the
        // grid opens with six padding cells and Monday Apr 6.
        assert_eq!(grid.cells.first().cloned().flatten(), None);
        assert_eq!(
            grid.cells[6],
            Some(ActivityCell {
                tokens: 0,
                level: 0
            })
        );
        // Sep 1 sits at index 148 (week 21, row 1 = Tuesday).
        assert_eq!(
            grid.cells[148],
            Some(ActivityCell {
                tokens: 100,
                level: 4
            })
        );
        assert_eq!(
            grid.cells[149],
            Some(ActivityCell {
                tokens: 25,
                level: 1
            })
        );
        assert_eq!(grid.total_tokens, 125);
        assert_eq!(grid.cells.len(), grid.week_count * 7);
        assert_eq!(grid.week_count, 27);
    }
}
