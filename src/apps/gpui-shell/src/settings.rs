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
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::setting::{
    NumberFieldOptions, SelectIndex, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};
use gpui_kit::component::sidebar::{Sidebar, SidebarFooter, SidebarItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Collapsible, Disableable, Sizable, StyledExt, WindowExt};
use gpui_kit::{
    div, prelude::*, px, Anchor, App, Axis, Entity, FontWeight, Global, IntoElement, SharedString,
    StyleRefinement, WeakEntity, Window, WindowAppearance,
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
/// G-SE17: the ordered user-level rules; the page rewrites the array in one
/// merged `policy` write so the preset and any newer keys survive.
pub(crate) const PERMISSION_POLICY_PATH: &str = "tool_permissions.policy";
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

/// G-SE13: the `memories` config document the memory page edits. Single keys
/// are merged back through [`persist_object_key`] so unknown keys survive.
pub(crate) const MEMORIES_PATH: &str = "memories";
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
/// G-SE24: the quick-actions array (`app.ai_experience.quick_actions`).
pub(crate) const QUICK_ACTIONS_PATH: &str = "app.ai_experience.quick_actions";
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
/// A2: the web `FontPreference` document (`{ uiSize: { level, customPx? } }`).
pub(crate) const FONT_PREFERENCE_PATH: &str = "font";
/// A2: the streaming-timeout pair (`null` = wait indefinitely).
pub(crate) const STREAM_TTFT_TIMEOUT_PATH: &str = "ai.stream_ttft_timeout_secs";
pub(crate) const STREAM_IDLE_TIMEOUT_PATH: &str = "ai.stream_idle_timeout_secs";
/// A2: the global proxy document (`{ enabled, url, username?, password? }`).
pub(crate) const PROXY_PATH: &str = "ai.proxy";
/// A2: the runtime-timeout pair and the subagent/swarm knobs.
pub(crate) const TOOL_EXECUTION_TIMEOUT_PATH: &str = "ai.tool_execution_timeout_secs";
pub(crate) const USER_QUESTION_TIMEOUT_PATH: &str = "ai.user_question_timeout_secs";
/// A2: one subagent-model document (`{ kind: inherit | fixed, model_id? }`).
pub(crate) const SUBAGENT_DEFAULT_MODEL_PATH: &str = "ai.agent_model_defaults.subagents.default";
/// A2: the subagent batch policy, kept as the same snake_case strings the
/// shared enum serializes to.
pub(crate) const SUBAGENT_BATCH_POLICY_PATH: &str = "ai.subagent_batch_execution_policy";
pub(crate) const SUBAGENT_MAX_CONCURRENCY_PATH: &str = "ai.subagent_max_concurrency";
pub(crate) const SWARM_MAX_CONCURRENCY_PATH: &str = "ai.swarm_max_concurrency";
/// A2: the deferred tool loading switch and the argument-JSON repair switch.
pub(crate) const DEFERRED_TOOL_LOADING_PATH: &str = "ai.enable_deferred_tool_loading";
pub(crate) const TOOL_JSON_REPAIR_PATH: &str = "ai.allow_tool_json_repair";
/// A2: the default review team (`ai.review_teams.default`); the page edits two
/// of its keys and round-trips the rest verbatim.
pub(crate) const REVIEW_TEAM_PATH: &str = "ai.review_teams.default";
/// G-SE15: the ChatInput preference document (`app.flow_chat`). The page edits
/// `default_mode_strategy` and `default_mode_id`, preserving every sibling key.
pub(crate) const FLOW_CHAT_PATH: &str = "app.flow_chat";
/// G-SE15: session-title generation (same keys the desktop page writes).
pub(crate) const SESSION_TITLE_ENABLED_PATH: &str =
    "app.ai_experience.enable_session_title_generation";
pub(crate) const SESSION_TITLE_MODEL_PATH: &str = "ai.task_models.session_title";
/// G-SE16: the managed-worktree defaults (`app.worktrees`).
pub(crate) const WORKTREES_PATH: &str = "app.worktrees";

/// The measure a settings row's control column carries before the row's own
/// label column would be squeezed; status sentences wrap inside it instead of
/// running past the row (web `ConfigPageRow` bounds its control column the
/// same way).
const CONTROL_COLUMN_MAX_WIDTH: f32 = 280.;

/// Inline export status for the diagnostics page as `(succeeded, message)`.
/// A global because the page is rebuilt every frame from `cx` alone.
pub(crate) struct DiagnosticsNotice(pub Option<(bool, String)>);

impl Global for DiagnosticsNotice {}

impl DiagnosticsNotice {
    pub(crate) fn current(cx: &App) -> Option<(bool, String)> {
        cx.try_global::<Self>().and_then(|notice| notice.0.clone())
    }

    pub(crate) fn set(message: Option<(bool, String)>, cx: &mut App) {
        cx.set_global(Self(message));
    }
}

/// The archived page's inline action receipt (G-SE30) as
/// `(succeeded, message)` — the shell's stand-in for the web's toasts, which
/// this surface does not have. Same global-holding pattern as
/// [`DiagnosticsNotice`], and the same reason: the page is rebuilt from `cx`.
pub(crate) struct ArchivedNotice(pub Option<(bool, String)>);

impl Global for ArchivedNotice {}

impl ArchivedNotice {
    pub(crate) fn current(cx: &App) -> Option<(bool, String)> {
        cx.try_global::<Self>().and_then(|notice| notice.0.clone())
    }

    pub(crate) fn set(message: Option<(bool, String)>, cx: &mut App) {
        cx.set_global(Self(message));
    }
}

/// The memory page's inline action receipt (G-SE13) as
/// `(succeeded, message)` — same shell stand-in for the web's toasts as
/// [`ArchivedNotice`].
pub(crate) struct MemoryNotice(pub Option<(bool, String)>);

impl Global for MemoryNotice {}

impl MemoryNotice {
    pub(crate) fn current(cx: &App) -> Option<(bool, String)> {
        cx.try_global::<Self>().and_then(|notice| notice.0.clone())
    }

    pub(crate) fn set(message: Option<(bool, String)>, cx: &mut App) {
        cx.set_global(Self(message));
    }
}

/// The memory page's own UI state (G-SE13): the advanced-block disclosure and
/// the in-flight guard the actions menu honors. A global because the page is
/// built inside the window's own render, where the window entity cannot be
/// read — the same reason [`MemoryNotice`] is one.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MemoryPageState {
    pub(crate) advanced_open: bool,
    pub(crate) busy: bool,
}

impl Global for MemoryPageState {}

impl MemoryPageState {
    /// The current state, or the defaults before anything touched it.
    pub(crate) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    /// Mutates the state, materializing the default when no bootstrap
    /// installed it yet.
    pub(crate) fn mutate<R>(cx: &mut App, change: impl FnOnce(&mut Self) -> R) -> R {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        change(cx.global_mut::<Self>())
    }
}

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
    /// G-T1: `terminal.terminal_panel_position` (`right` | `bottom`), the same
    /// shared key the web writes. Kept verbatim; the shell's dock resolves
    /// unknown values to its bottom dock.
    pub(crate) panel_position: String,
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
            panel_position: config.terminal_panel_position.clone(),
            profiles: config.profiles.clone(),
        }
    }
}

/// The shell's in-memory mirror of the settings it can change. Kept as a gpui
/// global so the settings page, the window bootstrap, and tests can read it
/// synchronously; the on-disk truth stays `GlobalConfig`.
#[derive(Debug, Clone)]
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
    /// A2 (G-SE5): `font.uiSize.level`, kept verbatim; an unknown id renders
    /// as the default size (see [`ui_font_base_px`]).
    pub(crate) ui_font_level: String,
    /// A2 (G-SE5): `font.uiSize.customPx`, only read on the custom level.
    pub(crate) ui_font_custom_px: Option<u32>,
    /// A2 (G-SE10): `ai.stream_ttft_timeout_secs`; `None` waits indefinitely.
    pub(crate) stream_ttft_timeout_secs: Option<u64>,
    /// A2 (G-SE10): `ai.stream_idle_timeout_secs`; `None` waits indefinitely.
    pub(crate) stream_idle_timeout_secs: Option<u64>,
    /// A2 (G-SE11): `ai.proxy` as the stored document — the page edits single
    /// keys and the write merges them back (see [`persist_object_key`]).
    pub(crate) proxy: serde_json::Value,
    /// A2 (G-SE18): `ai.tool_execution_timeout_secs`; `None` never times out.
    pub(crate) tool_execution_timeout_secs: Option<u64>,
    /// A2 (G-SE18): `ai.user_question_timeout_secs`; zero waits indefinitely.
    pub(crate) user_question_timeout_secs: Option<u32>,
    /// A2 (G-SE19): `ai.agent_model_defaults.subagents.default`.
    pub(crate) subagent_default_model:
        openbitfun_core::service::config::types::SubagentModelSelection,
    /// A2 (G-SE19): `ai.subagent_batch_execution_policy`, verbatim so an
    /// unlisted policy (the core enum's `serial`) shows what is stored.
    pub(crate) subagent_batch_policy: String,
    /// A2 (G-SE19): `ai.subagent_max_concurrency`.
    pub(crate) subagent_max_concurrency: u32,
    /// A2 (G-SE19): `ai.swarm_max_concurrency`.
    pub(crate) swarm_max_concurrency: u32,
    /// A2 (G-SE20): `ai.enable_deferred_tool_loading`.
    pub(crate) deferred_tool_loading: bool,
    /// A2 (G-SE21): `ai.allow_tool_json_repair`.
    pub(crate) allow_tool_json_repair: bool,
    /// A2 (G-SE22): `ai.review_teams.default` as the stored document; read for
    /// the two capacity keys, written back per key.
    pub(crate) review_team: serde_json::Value,
    /// G-SE13: the `memories` section as the typed shared document. Writes go
    /// key by key through [`persist_object_key`]; the mirror updates in place.
    pub(crate) memories: openbitfun_core::service::config::types::MemoriesConfig,
    /// G-SE15: `app.flow_chat` as the stored document. The page reads the
    /// strategy/fixed-mode keys with the web's compatibility rules and writes
    /// single keys, so sibling preferences survive.
    pub(crate) flow_chat: serde_json::Value,
    /// G-SE15: `app.ai_experience.enable_session_title_generation`.
    pub(crate) session_title_enabled: bool,
    /// G-SE15: `ai.task_models.session_title`: `inherit` or a model id.
    pub(crate) session_title_model: String,
    /// G-SE16: `app.worktrees` as the stored document, edited key by key.
    pub(crate) worktrees: serde_json::Value,
    /// G-SE17: `tool_permissions.policy.rules` — the ordered user-level rules.
    pub(crate) permission_rules: Vec<openbitfun_runtime_ports::PermissionRule>,
    /// G-SE12: `ai.models` — the configured model rows the editor works on.
    /// The catalog only carries id/name/provider/enabled; the row editor needs
    /// the full documents, and keeps this mirror current on every write.
    pub(crate) ai_models: Vec<openbitfun_core::service::config::types::AIModelConfig>,
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
            ui_font_level: config
                .font
                .as_ref()
                .map(|font| font.ui_size.level.clone())
                .unwrap_or_else(|| UI_FONT_LEVELS[2].0.to_string()),
            ui_font_custom_px: config.font.as_ref().and_then(|font| font.ui_size.custom_px),
            stream_ttft_timeout_secs: config.ai.stream_ttft_timeout_secs,
            stream_idle_timeout_secs: config.ai.stream_idle_timeout_secs,
            proxy: serde_json::to_value(&config.ai.proxy).unwrap_or(serde_json::Value::Null),
            tool_execution_timeout_secs: config.ai.tool_execution_timeout_secs,
            user_question_timeout_secs: config.ai.user_question_timeout_secs,
            subagent_default_model: config
                .ai
                .agent_model_defaults
                .subagents
                .default_selection
                .clone(),
            subagent_batch_policy: serde_json::to_value(config.ai.subagent_batch_execution_policy)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| SUBAGENT_BATCH_POLICIES[0].to_string()),
            subagent_max_concurrency: config.ai.subagent_max_concurrency as u32,
            swarm_max_concurrency: config.ai.swarm_max_concurrency as u32,
            deferred_tool_loading: config.ai.enable_deferred_tool_loading,
            allow_tool_json_repair: config.ai.allow_tool_json_repair,
            review_team: serde_json::to_value(
                config
                    .ai
                    .review_teams
                    .get("default")
                    .cloned()
                    .unwrap_or_default(),
            )
            .unwrap_or(serde_json::Value::Null),
            memories: config.memories.clone(),
            flow_chat: serde_json::to_value(&config.app.flow_chat)
                .unwrap_or(serde_json::Value::Null),
            session_title_enabled: config.app.ai_experience.enable_session_title_generation,
            session_title_model: task_model_selection_value(&config.ai.task_models.session_title),
            worktrees: serde_json::to_value(&config.app.worktrees)
                .unwrap_or(serde_json::Value::Null),
            permission_rules: config.tool_permissions.policy.rules.clone(),
            ai_models: config.ai.models.clone(),
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
            ui_font_level: UI_FONT_LEVELS[2].0.to_string(),
            ui_font_custom_px: None,
            stream_ttft_timeout_secs: None,
            stream_idle_timeout_secs: None,
            proxy: serde_json::Value::Null,
            tool_execution_timeout_secs: None,
            user_question_timeout_secs: None,
            subagent_default_model:
                openbitfun_core::service::config::types::SubagentModelSelection::Inherit,
            subagent_batch_policy: SUBAGENT_BATCH_POLICIES[0].to_string(),
            subagent_max_concurrency: DEFAULT_SUBAGENT_CONCURRENCY,
            swarm_max_concurrency: DEFAULT_SWARM_CONCURRENCY,
            deferred_tool_loading: true,
            allow_tool_json_repair: true,
            review_team: serde_json::Value::Null,
            memories: openbitfun_core::service::config::types::MemoriesConfig::default(),
            flow_chat: serde_json::Value::Null,
            session_title_enabled: true,
            session_title_model: "fast".to_string(),
            worktrees: default_worktree_settings_document(),
            permission_rules: Vec::new(),
            ai_models: Vec::new(),
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

/// A2 (G-SE5): the UI size levels the shared `font` document can carry, with
/// the base pixel size the web `PRESET_UI_BASE_PX` maps them to. `custom` is
/// the one level that takes its size from `uiSize.customPx` instead.
pub(crate) const UI_FONT_LEVELS: &[(&str, u32)] = &[
    ("compact", 12),
    ("small", 13),
    ("default", 14),
    ("medium", 15),
    ("large", 16),
];
pub(crate) const UI_FONT_CUSTOM_LEVEL: &str = "custom";
pub(crate) const MIN_UI_FONT_PX: u32 = 12;
pub(crate) const MAX_UI_FONT_PX: u32 = 20;
pub(crate) const DEFAULT_UI_FONT_PX: u32 = 14;
/// The window's em size as gpui starts it; the `default` level keeps it, so
/// the setting starts out as a no-op on a fresh install.
pub(crate) const SHELL_BASE_REM_PX: f32 = 16.;

/// A2 (G-SE5): the base pixel size of one stored preference. An unknown level
/// (a newer surface's, or a hand-edited document) falls back to the default
/// size instead of failing the page — the same tolerance the other appearance
/// values get.
pub(crate) fn ui_font_base_px(level: &str, custom_px: Option<u32>) -> u32 {
    if level == UI_FONT_CUSTOM_LEVEL {
        return custom_px
            .unwrap_or(DEFAULT_UI_FONT_PX)
            .clamp(MIN_UI_FONT_PX, MAX_UI_FONT_PX);
    }
    UI_FONT_LEVELS
        .iter()
        .find(|(id, _)| *id == level)
        .map(|(_, px)| *px)
        .unwrap_or(DEFAULT_UI_FONT_PX)
}

/// A2 (G-SE5): the window em size for one preference. The shell scales from
/// its own 16px base by the same ratio the level has to the web's 14px base,
/// so every level means the same relative size on both surfaces and `default`
/// renders exactly what the shell rendered before the setting existed.
pub(crate) fn ui_font_rem_px(level: &str, custom_px: Option<u32>) -> f32 {
    let base = ui_font_base_px(level, custom_px) as f32;
    SHELL_BASE_REM_PX * base / DEFAULT_UI_FONT_PX as f32
}

/// A2 (G-SE5): the `font` document written back, in the shape the web
/// `FontPreferenceService` persists (`customPx` only on the custom level).
pub(crate) fn ui_font_preference_value(level: &str, custom_px: Option<u32>) -> serde_json::Value {
    let mut ui_size = serde_json::Map::new();
    ui_size.insert(
        "level".to_string(),
        serde_json::Value::String(level.to_string()),
    );
    if level == UI_FONT_CUSTOM_LEVEL {
        ui_size.insert(
            "customPx".to_string(),
            serde_json::json!(ui_font_base_px(level, custom_px)),
        );
    }
    serde_json::json!({ "uiSize": serde_json::Value::Object(ui_size) })
}

/// A2 (G-SE5): applies the persisted UI size to one window. Called from the
/// render path, which is the only place every window (first frame, config
/// snapshot, live switch) passes through with a `&mut Window`.
pub(crate) fn apply_ui_font_size(window: &mut gpui_kit::Window, cx: &App) {
    let config = ShellConfig::current(cx);
    let rem = ui_font_rem_px(&config.ui_font_level, config.ui_font_custom_px);
    if window.rem_size() != px(rem) {
        window.set_rem_size(px(rem));
    }
}

/// A2 (G-SE19): the batch policies the shared enum serializes today. The web
/// runtime page exposes exactly these two; a stored `serial` (another surface
/// can write it, the core enum has it) stays verbatim in the page instead of
/// being rewritten.
pub(crate) const SUBAGENT_BATCH_POLICIES: &[&str] = &["safe_only", "force_parallel"];
pub(crate) const MIN_SUBAGENT_CONCURRENCY: u32 = 1;
/// The web page's `SUBAGENT_MAX_CONCURRENCY_LIMIT`; the core normalizes to 64,
/// so a larger stored value stays displayable but is never written back by the
/// shell.
pub(crate) const MAX_SUBAGENT_CONCURRENCY: u32 = 32;
pub(crate) const MAX_SWARM_CONCURRENCY: u32 = 64;
pub(crate) const DEFAULT_SUBAGENT_CONCURRENCY: u32 = 5;
pub(crate) const DEFAULT_SWARM_CONCURRENCY: u32 = 16;

/// Rounds a number-field value onto one concurrency domain (`min..=max`).
pub(crate) fn clamp_concurrency(value: f64, max: u32) -> Option<usize> {
    if !value.is_finite() {
        return None;
    }
    let rounded = value.round();
    if rounded < f64::from(MIN_SUBAGENT_CONCURRENCY) || rounded > f64::from(max) {
        return None;
    }
    Some(rounded as usize)
}

/// A2 (G-SE19): the dropdown value of one stored subagent-model selection —
/// `inherit` or the model id (`fast` / `primary` / a configured model).
pub(crate) fn subagent_model_choice(
    selection: &openbitfun_core::service::config::types::SubagentModelSelection,
) -> String {
    match selection {
        openbitfun_core::service::config::types::SubagentModelSelection::Inherit => {
            "inherit".to_string()
        }
        openbitfun_core::service::config::types::SubagentModelSelection::Fixed { model_id } => {
            model_id.clone()
        }
    }
}

/// A2 (G-SE19): the stored document for one dropdown choice, in the shape the
/// web page writes.
pub(crate) fn subagent_model_selection(
    choice: &str,
) -> openbitfun_core::service::config::types::SubagentModelSelection {
    use openbitfun_core::service::config::types::SubagentModelSelection;
    if choice == "inherit" {
        SubagentModelSelection::Inherit
    } else {
        SubagentModelSelection::fixed(choice)
    }
}

/// A2 (G-SE22): the review-capacity bounds the web section renders.
pub(crate) const MIN_REVIEW_PARALLELISM: u32 = 1;
pub(crate) const MAX_REVIEW_PARALLELISM: u32 = 2;
pub(crate) const MAX_REVIEW_QUEUE_WAIT_SECS: u32 = 3600;
/// The shared `ReviewTeamConfig` defaults, used for the display of a team
/// document that does not carry the key yet.
pub(crate) const DEFAULT_REVIEW_PARALLELISM: u64 = 2;
pub(crate) const DEFAULT_REVIEW_QUEUE_WAIT_SECS: u64 = 1200;

/// A2 (G-SE22): merges one key into a persisted object document. The shell
/// writes back the whole object, so it must not drop keys this build does not
/// know — a newer desktop or web build may have written them, and the shared
/// document has to survive a round trip through an older shell.
pub(crate) fn merged_object_value(
    current: &serde_json::Value,
    key: &str,
    value: serde_json::Value,
) -> serde_json::Value {
    let mut object = match current {
        serde_json::Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    object.insert(key.to_string(), value);
    serde_json::Value::Object(object)
}

/// A2 (G-SE22): one numeric key of an object document, or `fallback` when the
/// document, the key, or the value type is not there (the shared default).
pub(crate) fn object_u64(document: &serde_json::Value, key: &str, fallback: u64) -> u64 {
    document
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(fallback)
}

/// One boolean key of an object document (missing or mistyped = `fallback`).
pub(crate) fn object_bool(document: &serde_json::Value, key: &str, fallback: bool) -> bool {
    document
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(fallback)
}

/// One string key of an object document (missing or mistyped = `fallback`).
pub(crate) fn object_str<'a>(
    document: &'a serde_json::Value,
    key: &str,
    fallback: &'a str,
) -> &'a str {
    document
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
}

/// G-SE15: the default-Harness strategy, applying the web's compatibility
/// rule — an explicit `default_mode_strategy` wins; otherwise a stored
/// `default_mode_id` keeps its historical fixed meaning and an untouched
/// document follows the last selection.
pub(crate) fn flow_chat_strategy(document: &serde_json::Value) -> &'static str {
    match document
        .get("default_mode_strategy")
        .and_then(serde_json::Value::as_str)
    {
        Some("fixed") => "fixed",
        Some("follow_last") => "follow_last",
        _ => {
            let fixed = document
                .get("default_mode_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if fixed.is_empty() {
                "follow_last"
            } else {
                "fixed"
            }
        }
    }
}

/// G-SE15: the stored fixed default (falling back to the Standard Harness the
/// web uses when switching an unset preference to fixed).
pub(crate) fn flow_chat_fixed_mode(document: &serde_json::Value) -> String {
    document
        .get("default_mode_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("Standard")
        .to_string()
}

/// G-SE15: the most recent mode explicitly selected in the composer, when one
/// was recorded.
pub(crate) fn flow_chat_last_mode(document: &serde_json::Value) -> Option<String> {
    document
        .get("last_mode_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// G-SE15: one `TaskModelSelection` as the shell's select value (`inherit` or
/// the fixed model id).
pub(crate) fn task_model_selection_value(
    selection: &openbitfun_core::service::config::types::TaskModelSelection,
) -> String {
    selection
        .fixed_model_id()
        .map(str::to_string)
        .unwrap_or_else(|| "inherit".to_string())
}

/// G-SE15: the persisted document for a select value.
pub(crate) fn task_model_selection_document(value: &str) -> serde_json::Value {
    if value == "inherit" {
        serde_json::json!({ "kind": "inherit" })
    } else {
        serde_json::json!({ "kind": "fixed", "model_id": value })
    }
}

/// G-SE16: the shared worktree defaults (used before the first config read
/// and for a document that lost a key).
pub(crate) fn default_worktree_settings_document() -> serde_json::Value {
    serde_json::to_value(
        openbitfun_core::service::config::types::GlobalConfig::default()
            .app
            .worktrees,
    )
    .unwrap_or(serde_json::Value::Null)
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

/// Persists one key of an object document under `path`. The stored document
/// is read first and the key merged into it, so keys this build does not know
/// — written by a newer desktop or web build — survive the write instead of
/// being dropped by an older shell.
fn persist_object_key(path: &'static str, key: &str, value: serde_json::Value) {
    persist_object_keys(path, vec![(key.to_string(), value)]);
}

/// Persists several keys of one object document in a single read-modify-write,
/// so two related keys (a strategy plus its value) cannot land out of order.
fn persist_object_keys(path: &'static str, updates: Vec<(String, serde_json::Value)>) {
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
        let mut merged = service
            .get_config::<serde_json::Value>(Some(path))
            .await
            .unwrap_or(serde_json::Value::Null);
        for (key, value) in updates {
            merged = merged_object_value(&merged, &key, value);
        }
        match service.set_config(path, merged).await {
            Ok(()) => tracing::info!("Setting persisted: {}", path),
            Err(error) => tracing::error!("Failed to persist {}: {}", path, error),
        }
    });
}

/// A2 (G-SE5): stores one UI size preference (mirror, then document) and
/// repaints; the em size itself is applied per window in the render path.
pub(crate) fn set_ui_font(level: &str, custom_px: Option<u32>, cx: &mut App) {
    let stored_custom = (level == UI_FONT_CUSTOM_LEVEL).then(|| ui_font_base_px(level, custom_px));
    {
        let config = cx.global_mut::<ShellConfig>();
        config.ui_font_level = level.to_string();
        config.ui_font_custom_px = stored_custom;
    }
    persist(
        FONT_PREFERENCE_PATH,
        ui_font_preference_value(level, custom_px),
    );
    cx.refresh_windows();
}

/// A2 (G-SE11): stores one proxy key, keeping every other key of the stored
/// document.
pub(crate) fn set_proxy_key(key: &str, value: serde_json::Value, cx: &mut App) {
    let merged = merged_object_value(&ShellConfig::current(cx).proxy, key, value.clone());
    cx.global_mut::<ShellConfig>().proxy = merged;
    persist_object_key(PROXY_PATH, key, value);
    cx.refresh_windows();
}

/// A2 (G-SE22): stores one review-capacity key of the default team document.
pub(crate) fn set_review_capacity(key: &str, value: serde_json::Value, cx: &mut App) {
    let merged = merged_object_value(&ShellConfig::current(cx).review_team, key, value.clone());
    cx.global_mut::<ShellConfig>().review_team = merged;
    persist_object_key(REVIEW_TEAM_PATH, key, value);
    cx.refresh_windows();
}

/// Whether the stored proxy document has the proxy enabled (`false` for a
/// missing document, matching the shared default).
pub(crate) fn proxy_enabled(document: &serde_json::Value) -> bool {
    document
        .get("enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// One string key of the stored proxy document (`""` when absent).
pub(crate) fn proxy_text(document: &serde_json::Value, key: &str) -> String {
    document
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
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
/// The five registry groups of the settings tree, in the Web UI
/// `settingsRegistry` order. The tree mirrors the registry instead of
/// inventing its own taxonomy: every group and entry below stands for one
/// registry category/page, so the acceptance rule "the tree matches parity
/// §2.1 one-to-one" is a data property (see the `settings_tree` tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    Application,
    Ai,
    Workspace,
    Tools,
    Data,
}

pub(crate) const SETTINGS_CATEGORIES: &[SettingsCategory] = &[
    SettingsCategory::Application,
    SettingsCategory::Ai,
    SettingsCategory::Workspace,
    SettingsCategory::Tools,
    SettingsCategory::Data,
];

/// One tree entry of a group: either a page the shell hosts (rendered as a
/// titled group of settings) or a redirect to the shell surface that owns
/// the registry page. Entries without a registry id are shell-side
/// extensions that the web registry does not track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsEntry {
    General,
    Appearance,
    Terminal,
    Models,
    /// G-SE14: the cognitive-engine LLM page (`ai.trinity`), backed by the
    /// trinityd `llm.*` RPCs.
    TrinityEngine,
    /// G-SE13: the agent-memory page (`ai.memory`) over the shared
    /// `memories` config section.
    Memory,
    /// G-SE7: the read-only shortcut table (`application.shortcuts`). The
    /// shell's bindings are fixed per release; the web page's record/conflict
    /// machinery has no shell counterpart to persist into.
    Shortcuts,
    /// `workspace.session`: covered by the global search overlay.
    SessionSearch,
    /// `workspace.worktrees`: covered by the Git panel.
    WorktreesGit,
    Permissions,
    Automation,
    WebSearch,
    /// Shell-side extension (no registry page).
    Skills,
    /// Shell-side extension (no registry page).
    Agents,
    Mcp,
    Usage,
    Archived,
    Diagnostics,
}

impl SettingsEntry {
    /// The registry page id this entry stands for; `None` for shell-side
    /// extensions. Test-only: the production tree consumes the match arms in
    /// [`category_entries`] directly, the ids exist so the disposition test
    /// can prove the tree covers the registry one-to-one.
    #[cfg(test)]
    pub(crate) fn registry_id(self) -> Option<&'static str> {
        match self {
            Self::General => Some("application.general"),
            Self::Appearance => Some("application.appearance"),
            Self::Terminal => Some("application.terminal"),
            Self::Models => Some("ai.models"),
            Self::TrinityEngine => Some("ai.trinity"),
            Self::Memory => Some("ai.memory"),
            Self::Shortcuts => Some("application.shortcuts"),
            Self::SessionSearch => Some("workspace.session"),
            Self::WorktreesGit => Some("workspace.worktrees"),
            Self::Permissions => Some("tools.execution"),
            Self::Automation => Some("tools.automation"),
            Self::WebSearch => Some("tools.webSearch"),
            Self::Skills => None,
            Self::Agents => None,
            Self::Mcp => Some("tools.mcp"),
            Self::Usage => Some("data.usage"),
            Self::Archived => Some("data.archived"),
            Self::Diagnostics => Some("data.diagnostics"),
        }
    }
}

/// Registry pages with no shell presence of any kind. They stay out of the
/// tree entirely — no dead-end entry that only says "not here" (the same
/// no-fake-UI rule that keeps web-only switches out of the shell pages;
/// parity §2.1). pet/voice have no shell analog, browser/desktop-control and
/// ACP agents have no shell page, and the editor page stays postponed
/// (trinity graduated with G-SE14, memory with G-SE13, shortcuts with G-SE7).
/// Test-only: hidden-ness is enforced by omission from
/// [`category_entries`]; this list exists so the disposition test can prove
/// tree ∪ hidden covers the registry exactly.
#[cfg(test)]
pub(crate) const HIDDEN_REGISTRY_PAGES: &[&str] = &[
    "application.pet",
    "application.voice",
    "application.editor",
    "tools.desktop-control",
    "tools.acp",
];

/// Tree entries per group, in display order.
pub(crate) fn category_entries(category: SettingsCategory) -> &'static [SettingsEntry] {
    match category {
        SettingsCategory::Application => &[
            SettingsEntry::General,
            SettingsEntry::Appearance,
            SettingsEntry::Shortcuts,
            SettingsEntry::Terminal,
        ],
        SettingsCategory::Ai => &[
            SettingsEntry::Models,
            SettingsEntry::TrinityEngine,
            SettingsEntry::Memory,
        ],
        SettingsCategory::Workspace => &[SettingsEntry::SessionSearch, SettingsEntry::WorktreesGit],
        SettingsCategory::Tools => &[
            SettingsEntry::Permissions,
            SettingsEntry::Automation,
            SettingsEntry::WebSearch,
            SettingsEntry::Skills,
            SettingsEntry::Agents,
            SettingsEntry::Mcp,
        ],
        SettingsCategory::Data => &[
            SettingsEntry::Usage,
            SettingsEntry::Archived,
            SettingsEntry::Diagnostics,
        ],
    }
}

/// Every tree entry in nav order (category order, then each category's own
/// entry order). One flat, fixed sequence: the settings nav lists it grouped,
/// and the page indices the single-page renderer selects among come from it.
pub(crate) fn settings_entries() -> impl Iterator<Item = SettingsEntry> {
    SETTINGS_CATEGORIES
        .iter()
        .flat_map(|&category| category_entries(category).iter().copied())
}

/// The entry's index in [`settings_entries`] — also the gpui-kit page index
/// its content renders at (see [`render_page`]).
pub(crate) fn entry_index(entry: SettingsEntry) -> usize {
    settings_entries()
        .position(|candidate| candidate == entry)
        .unwrap_or(0)
}

/// Stable element-id slug for a tree entry (page keys, debug selectors).
pub(crate) fn entry_slug(entry: SettingsEntry) -> &'static str {
    match entry {
        SettingsEntry::General => "general",
        SettingsEntry::Appearance => "appearance",
        SettingsEntry::Terminal => "terminal",
        SettingsEntry::Models => "models",
        SettingsEntry::TrinityEngine => "trinity",
        SettingsEntry::Memory => "memory",
        SettingsEntry::Shortcuts => "shortcuts",
        SettingsEntry::SessionSearch => "session",
        SettingsEntry::WorktreesGit => "worktrees",
        SettingsEntry::Permissions => "execution",
        SettingsEntry::Automation => "automation",
        SettingsEntry::WebSearch => "websearch",
        SettingsEntry::Skills => "skills",
        SettingsEntry::Agents => "agents",
        SettingsEntry::Mcp => "mcp",
        SettingsEntry::Usage => "usage",
        SettingsEntry::Archived => "archived",
        SettingsEntry::Diagnostics => "diagnostics",
    }
}

/// The entry's nav and page title (web `navigation.pages.<id>.label`, the
/// same string the group builders used to title their group).
pub(crate) fn entry_label<'a>(entry: SettingsEntry, text: &'a ShellText) -> &'a str {
    match entry {
        SettingsEntry::General => &text.settings_general,
        SettingsEntry::Appearance => &text.settings_appearance,
        SettingsEntry::Terminal => &text.settings_terminal,
        SettingsEntry::Models => &text.models_title,
        SettingsEntry::TrinityEngine => &text.settings_trinity_label,
        SettingsEntry::Memory => &text.settings_memory_label,
        SettingsEntry::Shortcuts => &text.settings_shortcuts_label,
        SettingsEntry::SessionSearch => &text.settings_jump_session_label,
        SettingsEntry::WorktreesGit => &text.settings_jump_worktrees_label,
        SettingsEntry::Permissions => &text.settings_permissions,
        SettingsEntry::Automation => &text.settings_automation,
        SettingsEntry::WebSearch => &text.settings_websearch,
        SettingsEntry::Skills => &text.manage_skills_title,
        SettingsEntry::Agents => &text.manage_agents_title,
        SettingsEntry::Mcp => &text.manage_mcp_title,
        SettingsEntry::Usage => &text.settings_usage,
        SettingsEntry::Archived => &text.settings_archived,
        SettingsEntry::Diagnostics => &text.settings_diagnostics,
    }
}

/// The entry's page-header subtitle (web `navigation.pages.<id>.description`).
pub(crate) fn entry_description<'a>(entry: SettingsEntry, text: &'a ShellText) -> &'a str {
    match entry {
        SettingsEntry::General => &text.settings_page_general_description,
        SettingsEntry::Appearance => &text.settings_page_appearance_description,
        SettingsEntry::Terminal => &text.settings_page_terminal_description,
        SettingsEntry::Models => &text.settings_page_models_description,
        SettingsEntry::TrinityEngine => &text.settings_page_trinity_description,
        SettingsEntry::Memory => &text.settings_page_memory_description,
        SettingsEntry::Shortcuts => &text.settings_page_shortcuts_description,
        SettingsEntry::SessionSearch => &text.settings_page_session_description,
        SettingsEntry::WorktreesGit => &text.settings_page_worktrees_description,
        SettingsEntry::Permissions => &text.settings_page_execution_description,
        SettingsEntry::Automation => &text.settings_page_automation_description,
        SettingsEntry::WebSearch => &text.settings_page_websearch_description,
        SettingsEntry::Skills => &text.settings_page_skills_description,
        SettingsEntry::Agents => &text.settings_page_agents_description,
        SettingsEntry::Mcp => &text.settings_page_mcp_description,
        SettingsEntry::Usage => &text.settings_page_usage_description,
        SettingsEntry::Archived => &text.settings_page_archived_description,
        SettingsEntry::Diagnostics => &text.settings_page_diagnostics_description,
    }
}

/// The entry's page content: the group the settings builders own for it.
///
/// The groups carry no title of their own any more — the nav item and the
/// page header already name the page, and a second copy of the page name
/// above the card is exactly the duplication the web page does not have
/// (`ConfigPageSection` titles name sections *inside* a page, and every shell
/// page has exactly one).
pub(crate) fn entry_group(
    entry: SettingsEntry,
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> SettingGroup {
    match entry {
        SettingsEntry::General => general_group(text, view, cx),
        SettingsEntry::Appearance => appearance_group(text, cx),
        SettingsEntry::Terminal => terminal_group(text, view),
        SettingsEntry::Models => models_group(text, cx),
        SettingsEntry::TrinityEngine => trinity_engine_group(text, view, cx),
        SettingsEntry::Memory => memory_group(text, view, cx),
        SettingsEntry::Shortcuts => shortcuts_group(text, cx),
        SettingsEntry::SessionSearch => session_search_group(text, view, cx),
        SettingsEntry::WorktreesGit => worktrees_git_group(text, view, cx),
        SettingsEntry::Permissions => permissions_group(text, cx),
        SettingsEntry::Automation => automation_group(text, cx),
        SettingsEntry::WebSearch => websearch_group(text, view, cx),
        SettingsEntry::Skills => crate::manage::skills_group(text, view),
        SettingsEntry::Agents => crate::manage::agents_group(text, view, cx),
        SettingsEntry::Mcp => crate::manage::mcp_group(text, view),
        SettingsEntry::Usage => usage_group(text, view),
        SettingsEntry::Archived => archived_group(text, view),
        SettingsEntry::Diagnostics => diagnostics_group(text, view),
    }
}

/// The settings page content for one tree entry.
///
/// The nav lives in the shell's own left column (web `SettingsNav` covers the
/// sidebar), so the component's sidebar is collapsed to zero width and the
/// page renders as the single visible page. Every entry still ships as a page
/// — the component's page index picks the visible one, and a fixed page order
/// is what keeps each page's internal scroll position its own.
pub(crate) fn render_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    entry: SettingsEntry,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let header = page_header_style(cx);
    let pages = settings_entries()
        .map(|candidate| {
            SettingPage::new(SharedString::from(entry_label(candidate, text).to_string()))
                .description(SharedString::from(
                    entry_description(candidate, text).to_string(),
                ))
                .resettable(false)
                .header_style(&header)
                .group(entry_group(candidate, text, view.clone(), cx))
        })
        .collect::<Vec<_>>();

    Settings::new(SharedString::from(format!(
        "shell-settings-{}",
        entry_slug(entry)
    )))
    // The settings nav is the shell's nav column, not the component's own
    // sidebar (web `SettingsNav` in `NavPanel`). Zero width and a zero-size
    // range leave the content pane the whole column.
    .sidebar_width(gpui_kit::px(0.))
    .sidebar_size_range(gpui_kit::px(0.)..gpui_kit::px(0.))
    .default_selected_index(SelectIndex {
        page_ix: entry_index(entry),
        group_ix: None,
    })
    // The component's own sidebar is off-screen here; drop its edge rule so
    // nothing of it paints inside the content column.
    .sidebar_style(&StyleRefinement::default().border_0())
    .with_group_variant(GroupBoxVariant::Fill)
    .pages(pages)
    .into_any_element()
}

/// The page header refinement: the web `ConfigPageHeader` geometry — space-5
/// above, no divider, the 24px bold page title — inside the centered column
/// the shell hands the page.
fn page_header_style(cx: &App) -> StyleRefinement {
    StyleRefinement::default()
        .border_0()
        .pt_5()
        .px_4()
        .pb_0()
        // The web stack: 36px between the header and the first section, minus
        // the 16px the first group's own top padding already contributes.
        .mb_5()
        .gap_2()
        .text_size(gpui_kit::px(24.))
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().foreground)
}

/// One row of the settings nav. Pure data: the projection in [`nav_groups`]
/// builds these from the locale copy and the active page, and the renderer
/// attaches the view handle and the click path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsNavRow {
    /// A registry page (web `NavigationPanelItem`, active = `aria-current`).
    Page {
        entry: SettingsEntry,
        label: String,
        active: bool,
    },
    /// A search result (web `SettingsNav`'s result row: the category path over
    /// the page description).
    Result {
        entry: SettingsEntry,
        path: String,
        description: String,
        active: bool,
    },
    /// The search found nothing (web `navigation.search.empty`).
    Empty(String),
}

/// One child of the nav's `Sidebar`: a category section (web
/// `NavigationPanelSection`) or the flat search-results block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingsNavGroup {
    /// The section heading; `None` for the search-results block.
    heading: Option<String>,
    rows: Vec<SettingsNavRow>,
}

/// The nav's content for one search query: the five category sections when
/// the field is empty (web `SettingsNav`'s tree), else the flat list of pages
/// whose title, description, category, or registry id matches the query.
///
/// The web index also carries each page's own copy phrases; the shell's pages
/// hold their rows behind the settings component, so the honest subset is the
/// page's own name, description, and category (a match the user can see in the
/// list either way).
pub(crate) fn nav_groups(
    text: &ShellText,
    active: SettingsEntry,
    query: &str,
) -> Vec<SettingsNavGroup> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return SETTINGS_CATEGORIES
            .iter()
            .map(|&category| SettingsNavGroup {
                heading: Some(category_label(category, text).to_string()),
                rows: category_entries(category)
                    .iter()
                    .map(|&entry| SettingsNavRow::Page {
                        entry,
                        label: entry_label(entry, text).to_string(),
                        active: entry == active,
                    })
                    .collect(),
            })
            .collect();
    }

    let rows = SETTINGS_CATEGORIES
        .iter()
        .flat_map(|&category| {
            category_entries(category).iter().map(move |&entry| {
                let label = entry_label(entry, text).to_string();
                let description = entry_description(entry, text).to_string();
                (
                    category,
                    entry,
                    format!(
                        "{} {} {} {}",
                        category_label(category, text),
                        label,
                        description,
                        entry_slug(entry)
                    )
                    .to_lowercase(),
                    label,
                    description,
                )
            })
        })
        .filter(|(_, _, haystack, _, _)| haystack.contains(&needle))
        .map(
            |(category, entry, _, label, description)| SettingsNavRow::Result {
                entry,
                path: format!("{} › {}", category_label(category, text), label),
                description,
                active: entry == active,
            },
        )
        .collect::<Vec<_>>();

    vec![SettingsNavGroup {
        heading: None,
        rows: if rows.is_empty() {
            vec![SettingsNavRow::Empty(
                text.settings_nav_search_empty.clone(),
            )]
        } else {
            rows
        },
    }]
}

/// The category's heading copy (web `navigation.categories.<id>`).
fn category_label(category: SettingsCategory, text: &ShellText) -> &str {
    match category {
        SettingsCategory::Application => &text.settings_category_application,
        SettingsCategory::Ai => &text.settings_category_ai,
        SettingsCategory::Workspace => &text.settings_category_workspace,
        SettingsCategory::Tools => &text.settings_category_tools,
        SettingsCategory::Data => &text.settings_category_data,
    }
}

/// The settings nav column: the shell's sidebar slot while the settings scene
/// is active (web `SettingsNav`, registered as the scene's `NavPanel`). The
/// component's own sidebar stays collapsed to zero width inside the page, so
/// this is the only settings tree on screen.
pub(crate) fn render_nav(
    text: &ShellText,
    active: SettingsEntry,
    search: &Entity<InputState>,
    width: f32,
    view: WeakEntity<crate::shell::ShellWindow>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let groups = nav_groups(text, active, &search.read(cx).value());
    Sidebar::new(NAV_ID)
        .w(px(width))
        .collapsible(false)
        .collapsed(false)
        // Web `SettingsNav` insets the field inside the panel's own padding;
        // the component's header slot only carries space-3 of its own.
        .header(
            div()
                .w_full()
                .px_3()
                .pt_3()
                .child(Input::new(search).prefix(IconName::Search)),
        )
        .children(
            groups
                .into_iter()
                .enumerate()
                .map(|(ix, group)| SettingsNavGroupElement {
                    group,
                    first: ix == 0,
                    view: view.clone(),
                    search: search.clone(),
                })
                .collect::<Vec<_>>(),
        )
        .footer(SidebarFooter::new())
        .into_any_element()
}

const NAV_ID: &str = "shell-settings-nav";

/// A nav group as one `Sidebar` child, so the sidebar's own inter-child gap
/// spaces the categories (the shell's session groups use the same shape).
#[derive(Clone)]
struct SettingsNavGroupElement {
    group: SettingsNavGroup,
    /// The first section sits right under the search field; the others carry
    /// the web's extra gap above their heading.
    first: bool,
    view: WeakEntity<crate::shell::ShellWindow>,
    search: Entity<InputState>,
}

impl Collapsible for SettingsNavGroupElement {
    fn collapsed(self, _collapsed: bool) -> Self {
        self
    }

    fn is_collapsed(&self) -> bool {
        false
    }
}

impl SidebarItem for SettingsNavGroupElement {
    fn render(
        self,
        id: impl Into<gpui_kit::ElementId>,
        _window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let _id = id.into();
        div()
            .flex()
            .flex_col()
            // Rows keep the desktop's indent past the column's own padding
            // (`SettingsNav` indents items past the icon slot MainNav rows
            // keep); the section gap above a heading matches the desktop's
            // `NavigationPanelSection` rhythm.
            .px_2()
            .when(!self.first, |this| this.pt_3())
            .gap_1()
            .when_some(self.group.heading, |this, heading| {
                this.child(
                    div()
                        .debug_selector(|| "settings-nav-category".to_string())
                        // Web `NavigationPanelSection` heading: label-xs over a
                        // 24px row, inset by the heading padding only.
                        .h(px(24.))
                        .px_2()
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .font_medium()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(heading)),
                )
            })
            .child(
                div().flex().flex_col().gap_0p5().children(
                    self.group.rows.into_iter().map(|row| {
                        settings_nav_row(row, self.view.clone(), self.search.clone(), cx)
                    }),
                ),
            )
    }
}

/// One nav row element. The row owns its click path: a page row selects the
/// page, a result row selects the page and clears the search field (web
/// `activate(destination, clear)`).
fn settings_nav_row(
    row: SettingsNavRow,
    view: WeakEntity<crate::shell::ShellWindow>,
    search: Entity<InputState>,
    cx: &App,
) -> gpui_kit::AnyElement {
    match row {
        SettingsNavRow::Page {
            entry,
            label,
            active,
        } => {
            let id = SharedString::from(format!("settings-nav-page-{}", entry_slug(entry)));
            let slug = entry_slug(entry);
            nav_item_row(id, active, cx)
                .debug_selector(move || format!("settings-nav-page-{slug}"))
                .on_click(move |_, _, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |shell, cx| shell.set_settings_page(entry, cx));
                    }
                })
                .child(SharedString::from(label))
                .into_any_element()
        }
        SettingsNavRow::Result {
            entry,
            path,
            description,
            active,
        } => {
            let id = SharedString::from(format!("settings-nav-result-{}", entry_slug(entry)));
            let slug = entry_slug(entry);
            let click_view = view.clone();
            let click_search = search.clone();
            nav_item_row(id, active, cx)
                .debug_selector(move || format!("settings-nav-result-{slug}"))
                .h(px(44.))
                .items_start()
                .py_1()
                .on_click(move |_, window, cx| {
                    if let Some(shell) = click_view.upgrade() {
                        shell.update(cx, |shell, cx| shell.set_settings_page(entry, cx));
                    }
                    click_search.update(cx, |state, cx| state.set_value("", window, cx));
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .min_w_0()
                        .child(div().truncate().child(SharedString::from(path)))
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(SharedString::from(description)),
                        ),
                )
                .into_any_element()
        }
        SettingsNavRow::Empty(message) => div()
            .debug_selector(|| "settings-nav-empty".to_string())
            .px_2()
            .py_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(message))
            .into_any_element(),
    }
}

/// The shared nav row frame: web item geometry (30px tall, 8px inline inset,
/// the item radius, the quiet-pill hover/selection pair the shell's nav rows
/// already use). The page rows indent their label past the icon slot the
/// desktop's other nav rows keep (web `SettingsNav` does the same).
fn nav_item_row(id: SharedString, active: bool, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_x_2()
        .min_w_0()
        .h(px(30.))
        .pl(px(36.))
        .pr_2()
        .rounded(cx.theme().radius)
        .text_sm()
        .when(active, |this| {
            this.font_medium()
                .bg(cx.theme().selection)
                .text_color(cx.theme().sidebar_accent_foreground)
        })
        .when(!active, |this| {
            this.hover(|style| style.bg(cx.theme().muted))
        })
}

/// The `workspace.session` entry (parity §2.1): the shell hosts no
/// session/workspace settings page — the global search overlay is the
/// surface that reaches sessions and workspace content. The card redirects
/// there instead of faking the web page's controls.
fn session_search_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &App,
) -> SettingGroup {
    let owned_text = text.clone();
    let open_field = SettingField::render(move |_options, _window, _cx| {
        Button::new("open-session-search")
            .label(owned_text.settings_jump_session_action.clone())
            .with_variant(ButtonVariant::Ghost)
            .on_click({
                let view = view.clone();
                move |_: &gpui_kit::ClickEvent, window, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |shell, cx| shell.open_search_overlay(window, cx));
                    }
                }
            })
            .into_any_element()
    });

    let mut items = vec![section_heading_item(
        "default-harness",
        text.settings_default_harness_section.clone(),
        text.settings_default_harness_section_note.clone(),
    )];
    items.extend(default_harness_rows(text, cx));
    items.push(section_heading_item(
        "session-title",
        text.settings_session_title_section.clone(),
        text.settings_session_title_section_note.clone(),
    ));
    items.extend(session_title_rows(text, cx));
    items.push(SettingItem::new(
        text.settings_jump_session_description.clone(),
        open_field,
    ));
    SettingGroup::new().items(items)
}

/// G-SE15: the default-Harness preference (web `DefaultHarnessConfig`): the
/// strategy plus, while fixed, the fixed Harness. Both writes are one merged
/// document write so an interleaved older shell cannot drop the sibling key.
fn default_harness_rows(text: &ShellText, cx: &App) -> Vec<SettingItem> {
    let flow_chat = ShellConfig::current(cx).flow_chat;
    let strategy = flow_chat_strategy(&flow_chat);
    let fixed_mode = flow_chat_fixed_mode(&flow_chat);
    let last_mode = flow_chat_last_mode(&flow_chat);

    let harness_label = |id: &str| {
        if crate::manage::HARNESS_IDS.contains(&id) {
            crate::i18n::shared_agent_name(id, cx)
        } else {
            id.to_string()
        }
    };
    let strategy_description = if strategy == "fixed" {
        text.settings_default_harness_fixed_description.clone()
    } else if let Some(last) = last_mode.as_deref() {
        crate::i18n::default_harness_follow_description(&harness_label(last), cx)
    } else {
        crate::i18n::default_harness_follow_empty_description(
            &crate::i18n::shared_agent_name("Standard", cx),
            cx,
        )
    };

    let strategy_field = SettingField::dropdown(
        vec![
            (
                SharedString::from("follow_last"),
                SharedString::from(text.settings_default_harness_follow_last.clone()),
            ),
            (
                SharedString::from("fixed"),
                SharedString::from(text.settings_default_harness_fixed.clone()),
            ),
        ],
        |cx: &App| SharedString::from(flow_chat_strategy(&ShellConfig::current(cx).flow_chat)),
        |value: SharedString, cx: &mut App| {
            let strategy = if value.as_ref() == "fixed" {
                "fixed"
            } else {
                "follow_last"
            };
            let mut document = ShellConfig::current(cx).flow_chat.clone();
            let mut updates = vec![(
                "default_mode_strategy".to_string(),
                serde_json::json!(strategy),
            )];
            document = merged_object_value(
                &document,
                "default_mode_strategy",
                serde_json::json!(strategy),
            );
            let missing_fixed = document
                .get("default_mode_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .map(str::is_empty)
                .unwrap_or(true);
            if strategy == "fixed" && missing_fixed {
                let fallback =
                    flow_chat_last_mode(&document).unwrap_or_else(|| "Standard".to_string());
                document =
                    merged_object_value(&document, "default_mode_id", serde_json::json!(fallback));
                updates.push(("default_mode_id".to_string(), serde_json::json!(fallback)));
            }
            cx.global_mut::<ShellConfig>().flow_chat = document;
            persist_object_keys(FLOW_CHAT_PATH, updates);
            cx.refresh_windows();
        },
    );

    let mut fixed_options: Vec<(SharedString, SharedString)> = crate::manage::HARNESS_IDS
        .iter()
        .map(|id| {
            (
                SharedString::from(*id),
                SharedString::from(crate::i18n::shared_agent_name(id, cx)),
            )
        })
        .collect();
    if !crate::manage::HARNESS_IDS.contains(&fixed_mode.as_str()) {
        fixed_options.insert(
            0,
            (
                SharedString::from(fixed_mode.clone()),
                SharedString::from(format!("{fixed_mode} ({})", text.value_compatibility)),
            ),
        );
    }
    let fixed_field = SettingField::dropdown(
        fixed_options,
        |cx: &App| SharedString::from(flow_chat_fixed_mode(&ShellConfig::current(cx).flow_chat)),
        |value: SharedString, cx: &mut App| {
            let mode = value.to_string();
            if mode.trim().is_empty() {
                return;
            }
            let mut document = ShellConfig::current(cx).flow_chat.clone();
            document = merged_object_value(
                &document,
                "default_mode_strategy",
                serde_json::json!("fixed"),
            );
            document = merged_object_value(&document, "default_mode_id", serde_json::json!(mode));
            cx.global_mut::<ShellConfig>().flow_chat = document;
            persist_object_keys(
                FLOW_CHAT_PATH,
                vec![
                    (
                        "default_mode_strategy".to_string(),
                        serde_json::json!("fixed"),
                    ),
                    (
                        "default_mode_id".to_string(),
                        serde_json::json!(mode.clone()),
                    ),
                ],
            );
            cx.refresh_windows();
        },
    );

    let mut rows = vec![SettingItem::new(
        text.settings_default_harness_strategy.clone(),
        strategy_field,
    )
    .description(strategy_description)];
    if strategy == "fixed" {
        rows.push(SettingItem::new(
            text.settings_default_harness_fixed_mode.clone(),
            fixed_field,
        ));
    }
    rows
}

/// G-SE15: the session-title switch and its model selector (web
/// `SessionTitleConfig`). The selector is disabled while generation is off.
fn session_title_rows(text: &ShellText, cx: &App) -> Vec<SettingItem> {
    let enabled_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).session_title_enabled,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().session_title_enabled = value;
            persist(SESSION_TITLE_ENABLED_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();
    let current = ShellConfig::current(cx).session_title_model;
    let model_options = session_title_model_options(&catalog.models, &current, text);
    let model_field = SettingField::dropdown(
        model_options,
        |cx: &App| SharedString::from(ShellConfig::current(cx).session_title_model),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if value.trim().is_empty() {
                return;
            }
            cx.global_mut::<ShellConfig>().session_title_model = value.clone();
            persist(
                SESSION_TITLE_MODEL_PATH,
                task_model_selection_document(&value),
            );
            cx.refresh_windows();
        },
    );

    let enabled = ShellConfig::current(cx).session_title_enabled;
    let model_description = if catalog.models.is_empty() {
        text.settings_session_title_models_empty.clone()
    } else {
        String::new()
    };
    let mut model_item =
        SettingItem::new(text.settings_session_title_model.clone(), model_field).disabled(!enabled);
    if !model_description.is_empty() {
        model_item = model_item.description(model_description);
    }

    vec![
        SettingItem::new(text.settings_session_title_enable.clone(), enabled_field),
        model_item,
    ]
}

/// G-SE15: the session-title model menu — inherit/fast/primary aliases, every
/// enabled configured model, and a stored id this build cannot resolve.
fn session_title_model_options(
    catalog: &[crate::runtime::ModelChoice],
    current: &str,
    text: &ShellText,
) -> Vec<(SharedString, SharedString)> {
    let mut options: Vec<(SharedString, SharedString)> = vec![
        (
            SharedString::from("inherit"),
            SharedString::from(text.settings_session_title_model_inherit.clone()),
        ),
        (
            SharedString::from("fast"),
            SharedString::from(text.settings_session_title_model_fast.clone()),
        ),
        (
            SharedString::from("primary"),
            SharedString::from(text.settings_session_title_model_primary.clone()),
        ),
    ];
    for model in catalog.iter().filter(|model| model.enabled) {
        options.push((model.id.clone().into(), model.label().into()));
    }
    if !options.iter().any(|(id, _)| id.as_ref() == current) {
        options.push((
            SharedString::from(current.to_string()),
            SharedString::from(format!(
                "{current} ({})",
                text.settings_session_title_model_unavailable
            )),
        ));
    }
    options
}

/// The `workspace.worktrees` entry (parity §2.1): the web isolation defaults
/// (G-SE16) plus the jump card that opens worktree management in the Git
/// panel, which remains this shell's worktree owner.
fn worktrees_git_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &App,
) -> SettingGroup {
    let owned_text = text.clone();
    let open_field = SettingField::render(move |_options, _window, _cx| {
        Button::new("open-worktrees-git")
            .label(owned_text.settings_jump_worktrees_action.clone())
            .with_variant(ButtonVariant::Ghost)
            .on_click({
                let view = view.clone();
                move |_: &gpui_kit::ClickEvent, window, cx| {
                    if let Some(shell) = view.upgrade() {
                        shell.update(cx, |shell, cx| {
                            shell.open_git_panel(window, cx);
                            shell.set_git_view(crate::git::GitView::Worktrees, cx);
                        });
                    }
                }
            })
            .into_any_element()
    });

    let document = ShellConfig::current(cx).worktrees.clone();
    let defaults = default_worktree_settings_document();
    let root_default = object_str(&defaults, "rootPath", "").to_string();
    let prefix_default = object_str(&defaults, "branchPrefix", "").to_string();
    let auto_delete_default = object_bool(&defaults, "autoDeleteEnabled", true);
    let limit_default = object_u64(&defaults, "autoDeleteLimit", 15);

    let root_field = worktree_text_field("rootPath", root_default);
    let prefix_field = worktree_text_field("branchPrefix", prefix_default);
    let copy_changes_field = worktree_switch_field("copyLocalChanges", false);
    let auto_delete_field = worktree_switch_field("autoDeleteEnabled", auto_delete_default);
    let limit_field = SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(WORKTREE_AUTO_DELETE_MIN),
            max: f64::from(WORKTREE_AUTO_DELETE_MAX),
            step: 1.,
        },
        move |cx: &App| {
            object_u64(
                &ShellConfig::current(cx).worktrees,
                "autoDeleteLimit",
                limit_default,
            ) as f64
        },
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let limit = (value.round() as u64).clamp(
                u64::from(WORKTREE_AUTO_DELETE_MIN),
                u64::from(WORKTREE_AUTO_DELETE_MAX),
            );
            set_worktree_key("autoDeleteLimit", serde_json::json!(limit), cx);
        },
    );

    let auto_delete_enabled = object_bool(&document, "autoDeleteEnabled", auto_delete_default);
    SettingGroup::new().items(vec![
        section_heading_item(
            "worktree-isolation",
            text.settings_worktree_section.clone(),
            text.settings_worktree_section_note.clone(),
        ),
        SettingItem::new(text.settings_worktree_root.clone(), root_field)
            .description(text.settings_worktree_root_description.clone()),
        SettingItem::new(text.settings_worktree_branch_prefix.clone(), prefix_field)
            .description(text.settings_worktree_branch_prefix_description.clone()),
        SettingItem::new(
            text.settings_worktree_copy_changes.clone(),
            copy_changes_field,
        )
        .description(text.settings_worktree_copy_changes_description.clone()),
        SettingItem::new(
            text.settings_worktree_auto_delete.clone(),
            auto_delete_field,
        )
        .description(text.settings_worktree_auto_delete_description.clone()),
        SettingItem::new(
            text.settings_worktree_auto_delete_limit.clone(),
            limit_field,
        )
        .description(text.settings_worktree_auto_delete_limit_description.clone())
        .disabled(!auto_delete_enabled),
        SettingItem::new(text.settings_jump_worktrees_description.clone(), open_field),
    ])
}

/// G-SE16: the web root-path/branch-prefix bounds — an empty value is
/// required at save time on the web page, so the shell keeps the previous
/// value instead of persisting an unusable worktree root.
const WORKTREE_AUTO_DELETE_MIN: u32 = 1;
const WORKTREE_AUTO_DELETE_MAX: u32 = 100;

fn set_worktree_key(key: &str, value: serde_json::Value, cx: &mut App) {
    let merged = merged_object_value(&ShellConfig::current(cx).worktrees, key, value.clone());
    cx.global_mut::<ShellConfig>().worktrees = merged;
    persist_object_key(WORKTREES_PATH, key, value);
    cx.refresh_windows();
}

/// G-SE16: one string key of `app.worktrees`. Empty input is ignored (the web
/// page rejects it; keeping the stored value is the honest shell behavior).
fn worktree_text_field(key: &'static str, fallback: String) -> SettingField<SharedString> {
    SettingField::input(
        move |cx: &App| {
            SharedString::from(
                object_str(&ShellConfig::current(cx).worktrees, key, &fallback).to_string(),
            )
        },
        move |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if value.trim().is_empty() {
                tracing::warn!("Ignoring empty worktree setting: {key}");
                return;
            }
            set_worktree_key(key, serde_json::json!(value), cx);
        },
    )
}

/// G-SE16: one boolean key of `app.worktrees`.
fn worktree_switch_field(key: &'static str, fallback: bool) -> SettingField<bool> {
    SettingField::switch(
        move |cx: &App| object_bool(&ShellConfig::current(cx).worktrees, key, fallback),
        move |value: bool, cx: &mut App| set_worktree_key(key, serde_json::json!(value), cx),
    )
}

fn appearance_group(text: &ShellText, cx: &App) -> SettingGroup {
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

    // G-SE5: the web page's UI size levels. A stored id the shell does not
    // know stays verbatim in the menu, like every other verbatim value here.
    let current_level = ShellConfig::current(cx).ui_font_level.clone();
    let level_options = ui_font_level_options(&current_level, text);
    let level_field = SettingField::dropdown(
        level_options,
        |cx: &App| SharedString::from(ShellConfig::current(cx).ui_font_level.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if !ui_font_level_known(&value) {
                tracing::warn!("Ignoring unsupported UI font level: {value}");
                return;
            }
            set_ui_font(&value, ShellConfig::current(cx).ui_font_custom_px, cx);
        },
    );
    let custom_field = SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(MIN_UI_FONT_PX),
            max: f64::from(MAX_UI_FONT_PX),
            step: 1.,
        },
        |cx: &App| {
            f64::from(ui_font_base_px(
                &ShellConfig::current(cx).ui_font_level,
                ShellConfig::current(cx).ui_font_custom_px,
            ))
        },
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let px = (value.round() as u32).clamp(MIN_UI_FONT_PX, MAX_UI_FONT_PX);
            set_ui_font(UI_FONT_CUSTOM_LEVEL, Some(px), cx);
        },
    );

    let mut items = vec![
        SettingItem::new(text.language.clone(), language_field)
            .description(text.settings_language_description.clone()),
        SettingItem::new(text.theme.clone(), theme_field)
            .description(text.settings_theme_description.clone()),
        SettingItem::new(text.settings_ui_font.clone(), level_field)
            .description(text.settings_ui_font_description.clone()),
    ];
    // The custom size row is the web page's conditional second row: only the
    // custom level reads it.
    if ShellConfig::current(cx).ui_font_level == UI_FONT_CUSTOM_LEVEL {
        items.push(
            SettingItem::new(text.settings_ui_font_custom.clone(), custom_field)
                .description(text.settings_ui_font_custom_description.clone()),
        );
    }
    SettingGroup::new().items(items)
}

/// G-SE5: the UI size menu — the shared levels, then the custom entry, then a
/// stored id outside the list so the page shows what is persisted.
fn ui_font_level_options(current: &str, text: &ShellText) -> Vec<(SharedString, SharedString)> {
    let mut options: Vec<(SharedString, SharedString)> = UI_FONT_LEVELS
        .iter()
        .map(|(id, _)| {
            (
                SharedString::from(*id),
                SharedString::from(ui_font_level_label(id, text)),
            )
        })
        .collect();
    options.push((
        SharedString::from(UI_FONT_CUSTOM_LEVEL),
        SharedString::from(text.ui_font_level_custom.clone()),
    ));
    if !ui_font_level_known(current) {
        options.insert(
            0,
            (
                SharedString::from(current.to_string()),
                SharedString::from(format!("{current} ({})", text.value_unsupported)),
            ),
        );
    }
    options
}

fn ui_font_level_known(level: &str) -> bool {
    level == UI_FONT_CUSTOM_LEVEL || UI_FONT_LEVELS.iter().any(|(id, _)| *id == level)
}

fn ui_font_level_label(level: &str, text: &ShellText) -> String {
    match level {
        "compact" => text.ui_font_level_compact.clone(),
        "small" => text.ui_font_level_small.clone(),
        "default" => text.ui_font_level_default.clone(),
        "medium" => text.ui_font_level_medium.clone(),
        "large" => text.ui_font_level_large.clone(),
        _ => text.ui_font_level_custom.clone(),
    }
}

/// General group (M22): launch-at-login, the close-button behavior, and the
/// self-update flow (auto-update preference plus the manual check / install /
/// restart actions). The update state lives on the window and arrives through
/// the event pump; the tray is the other half of this page (menu events land
/// on the same actions).
fn general_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &App,
) -> SettingGroup {
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

    SettingGroup::new().items(vec![
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
    ])
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
    // The status line can carry a whole diagnostic sentence; bound it to the
    // control column's measure so it wraps instead of running past the row.
    let note = |value: String, danger: bool| {
        div()
            .max_w(px(CONTROL_COLUMN_MAX_WIDTH))
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

/// Models group (M11 minimal version): the configured models with their
/// provider and enabled state, plus the default model new sessions start on.
/// The catalog comes from the same core path the desktop models page uses;
/// the default-model choice persists to `ai.default_models.primary`.
fn models_group(text: &ShellText, cx: &App) -> SettingGroup {
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
        let receipts = ModelTestState::current(cx).receipts;
        let mut list = div().v_flex().gap_2().w_full();
        for model in &catalog.models {
            let (state, state_color) = if model.enabled {
                (text.models_enabled.clone(), cx.theme().muted_foreground)
            } else {
                (text.models_disabled.clone(), cx.theme().danger)
            };
            let testing = matches!(receipts.get(&model.id), Some(ModelTestReceipt::Testing));
            let test_label = if testing {
                text.models_testing.clone()
            } else {
                text.models_test.clone()
            };
            let test_id = model.id.clone();
            let edit_id = model.id.clone();
            let edit_label = crate::i18n::shared_term("tools.edit", cx);
            let row = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(div().min_w_0().truncate().text_sm().child(model.label()))
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(div().text_sm().text_color(state_color).child(state))
                        .child(
                            Button::new(SharedString::from(format!("model-test-{}", model.id)))
                                .label(test_label)
                                .small()
                                .with_variant(ButtonVariant::Ghost)
                                .loading(testing)
                                .on_click(move |_, _window, cx| {
                                    start_model_test(test_id.clone(), cx);
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("model-edit-{}", model.id)))
                                .label(edit_label)
                                .small()
                                .with_variant(ButtonVariant::Ghost)
                                .on_click(move |_, _window, cx| {
                                    ModelEditorState::mutate(cx, |state| {
                                        state.model_id = Some(edit_id.clone());
                                        state.drafts_ready = false;
                                        state.headers_text.clear();
                                        state.body_text.clear();
                                    });
                                    cx.refresh_windows();
                                }),
                        ),
                );
            let mut card = div().v_flex().gap_1().w_full().child(row);
            if let Some(ModelTestReceipt::Done(result)) = receipts.get(&model.id) {
                card = card.child(model_test_receipt(result, &text, cx));
            }
            list = list.child(card);
        }
        list.into_any_element()
    });

    let default_description = if no_models {
        text.models_empty.clone()
    } else {
        text.models_default_description.clone()
    };

    // G-SE10: the streaming-timeout pair. Zero means "no timeout" on both
    // rows, which persists the same `null` the web page's empty input writes.
    let ttft_field = stream_timeout_field(
        STREAM_TTFT_TIMEOUT_PATH,
        |config| config.stream_ttft_timeout_secs,
        |config, value| config.stream_ttft_timeout_secs = value,
    );
    let idle_field = stream_timeout_field(
        STREAM_IDLE_TIMEOUT_PATH,
        |config| config.stream_idle_timeout_secs,
        |config, value| config.stream_idle_timeout_secs = value,
    );

    // G-SE11: the proxy document, one row per stored key.
    let proxy = ShellConfig::current(cx).proxy;
    let proxy_on = proxy_enabled(&proxy);
    let proxy_enable_field = SettingField::switch(
        |cx: &App| proxy_enabled(&ShellConfig::current(cx).proxy),
        |value: bool, cx: &mut App| {
            set_proxy_key("enabled", serde_json::json!(value), cx);
        },
    );
    let proxy_url_field = proxy_text_field("url");
    let proxy_username_field = proxy_text_field("username");
    let proxy_password_field = proxy_text_field("password");

    let mut items = vec![
        SettingItem::new(text.models_default.clone(), default_field)
            .description(default_description),
        SettingItem::new(text.models_configured.clone(), list_field),
        section_heading_item(
            "stream",
            text.settings_stream_section.clone(),
            text.settings_stream_section_note.clone(),
        ),
        SettingItem::new(text.settings_stream_ttft.clone(), ttft_field)
            .description(text.settings_stream_ttft_description.clone()),
        SettingItem::new(text.settings_stream_idle.clone(), idle_field)
            .description(text.settings_stream_idle_description.clone()),
        section_heading_item(
            "proxy",
            text.settings_proxy_section.clone(),
            text.settings_proxy_section_note.clone(),
        ),
        SettingItem::new(text.settings_proxy_enable.clone(), proxy_enable_field)
            .description(text.settings_proxy_enable_description.clone()),
        SettingItem::new(text.settings_proxy_url.clone(), proxy_url_field)
            .description(text.settings_proxy_url_description.clone())
            .disabled(!proxy_on),
        SettingItem::new(text.settings_proxy_username.clone(), proxy_username_field)
            .disabled(!proxy_on),
        SettingItem::new(text.settings_proxy_password.clone(), proxy_password_field)
            .description(text.settings_proxy_password_description.clone())
            .disabled(!proxy_on),
    ];
    // G-SE12: the open row's editor sits directly under the model list.
    if let Some(editor_items) = model_editor_items(text, cx) {
        items.extend(editor_items);
    }
    SettingGroup::new().items(items)
}

/// G-SE12: the inline editor for the row opened from the model list. It
/// writes each change immediately (the shell's settings convention) through
/// `update_config`, so the shared row keeps unknown fields and sibling models.
/// The full reasoning-preset authoring panel stays on the web/desktop pages;
/// the shell selects the default among the presets the stored row already
/// carries and shows what they do.
fn model_editor_items(text: &ShellText, cx: &App) -> Option<Vec<SettingItem>> {
    let model_id = ModelEditorState::current(cx).model_id?;
    let model = ShellConfig::current(cx)
        .ai_models
        .iter()
        .find(|model| model.id == model_id)?
        .clone();

    let stored_category = serde_json::to_value(&model.category)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "general_chat".to_string());

    let mut items = vec![section_heading_item(
        "model-editor",
        format!("{} · {}", text.models_edit_title, model.name),
        String::new(),
    )];

    let close_text = text.models_edit_close.clone();
    items.push(SettingItem::render(move |_options, _window, _cx| {
        Button::new("model-editor-close")
            .label(close_text.clone())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(|_, _window, cx| {
                ModelEditorState::mutate(cx, |state| state.model_id = None);
                cx.refresh_windows();
            })
            .into_any_element()
    }));

    // Category: the web's four choices, plus a value written by a newer build
    // kept verbatim so it is never silently remapped.
    let mut category_options: Vec<(SharedString, SharedString)> = Vec::new();
    for value in model_category_values(&stored_category) {
        let (value, label) = if value == "__stored_unknown" {
            (stored_category.clone(), stored_category.clone())
        } else {
            let label = match value {
                "general_chat" => text.models_category_general_chat.clone(),
                "multimodal" => text.models_category_multimodal.clone(),
                "image_generation" => text.models_category_image_generation.clone(),
                "speech_recognition" => text.models_category_speech_recognition.clone(),
                other => other.to_string(),
            };
            (value.to_string(), label)
        };
        category_options.push((value.into(), label.into()));
    }
    let category_get_id = model_id.clone();
    let category_set_id = model_id.clone();
    let category_field = SettingField::dropdown(
        category_options,
        move |cx: &App| {
            let stored = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == category_get_id)
                .map(|model| {
                    serde_json::to_value(&model.category)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_string))
                        .unwrap_or_else(|| "general_chat".to_string())
                })
                .unwrap_or_else(|| "general_chat".to_string());
            SharedString::from(stored)
        },
        move |value: SharedString, cx: &mut App| {
            set_ai_model_category(&category_set_id, value.to_string(), cx);
        },
    );

    let context_get_id = model_id.clone();
    let context_set_id = model_id.clone();
    let context_field = SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(MODEL_EDITOR_MIN_CONTEXT_WINDOW),
            max: f64::MAX,
            step: 1_000.,
        },
        move |cx: &App| {
            ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == context_get_id)
                .and_then(|model| model.context_window)
                .unwrap_or(MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW) as f64
        },
        move |value: f64, cx: &mut App| {
            if value.is_finite() && value >= f64::from(MODEL_EDITOR_MIN_CONTEXT_WINDOW) {
                set_ai_model_context_window(&context_set_id, value.round() as u32, cx);
            }
        },
    );

    let max_tokens_get_id = model_id.clone();
    let max_tokens_set_id = model_id.clone();
    let max_tokens_field = SettingField::number_input(
        NumberFieldOptions {
            min: 0.,
            max: f64::MAX,
            step: 1_000.,
        },
        move |cx: &App| {
            ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == max_tokens_get_id)
                .and_then(|model| model.max_tokens)
                .unwrap_or(0) as f64
        },
        move |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let tokens = value.round().max(0.) as u32;
            if tokens == 0 {
                set_ai_model_max_tokens(&max_tokens_set_id, None, cx);
                return;
            }
            // The shared contract caps an explicit output limit at 40% of the
            // context window; an out-of-range value is ignored rather than
            // persisted invalid.
            let context = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == max_tokens_set_id)
                .and_then(|model| model.context_window)
                .unwrap_or(MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW);
            if openbitfun_core::service::config::types::is_valid_configured_max_output_tokens(
                context, tokens,
            ) {
                set_ai_model_max_tokens(&max_tokens_set_id, Some(tokens), cx);
            }
        },
    );

    let stored_default = model
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.default_preset.clone())
        .unwrap_or_default();
    let mut reasoning_options: Vec<(SharedString, SharedString)> = vec![(
        SharedString::from(""),
        text.models_reasoning_auto.clone().into(),
    )];
    if let Some(reasoning) = model.reasoning.as_ref() {
        for preset in &reasoning.presets {
            if preset.disabled || preset.actions.is_empty() {
                continue;
            }
            let label = preset
                .label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| preset.id.clone());
            reasoning_options.push((preset.id.clone().into(), label.into()));
        }
    }
    if !stored_default.is_empty()
        && !reasoning_options
            .iter()
            .any(|(value, _)| value.as_ref() == stored_default)
    {
        reasoning_options.push((stored_default.clone().into(), stored_default.clone().into()));
    }
    let reasoning_get_id = model_id.clone();
    let reasoning_set_id = model_id.clone();
    let reasoning_field = SettingField::dropdown(
        reasoning_options,
        move |cx: &App| {
            let stored = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == reasoning_get_id)
                .and_then(|model| model.reasoning.as_ref())
                .and_then(|reasoning| reasoning.default_preset.clone())
                .unwrap_or_default();
            SharedString::from(stored)
        },
        move |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            let preset = (!value.is_empty()).then_some(value);
            set_ai_model_reasoning_default(&reasoning_set_id, preset, cx);
        },
    );

    // What the row's presets actually do, read-only: selection above is the
    // editable part; authoring presets stays on the pages that own the panel.
    let presets = model
        .reasoning
        .as_ref()
        .map(|reasoning| reasoning.presets.clone())
        .unwrap_or_default();
    let presets_title = text.models_reasoning_presets.clone();
    let presets_empty = text.models_reasoning_empty.clone();
    let presets_disabled = text.models_disabled.clone();
    let presets_item = SettingItem::render(move |_options, _window, cx| {
        let mut column = div()
            .v_flex()
            .gap_1()
            .w_full()
            .debug_selector(|| "model-editor-presets".to_string())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(presets_title.clone()),
            );
        if presets.is_empty() {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(presets_empty.clone()),
            );
        }
        for preset in &presets {
            let name = preset
                .label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| preset.id.clone());
            let summary = if preset.actions.is_empty() {
                crate::i18n::reasoning_no_actions(cx)
            } else {
                preset
                    .actions
                    .iter()
                    .map(|action| crate::i18n::reasoning_action_summary(action, cx))
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            let mut line = format!("{name} · {summary}");
            if preset.disabled {
                line = format!("{line} · {presets_disabled}");
            }
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(line),
            );
        }
        column.into_any_element()
    })
    .layout(Axis::Vertical);

    items.push(SettingItem::new(
        text.models_category.clone(),
        category_field,
    ));
    items.push(
        SettingItem::new(text.models_context_window.clone(), context_field)
            .description(text.models_context_window_hint.clone()),
    );
    items.push(
        SettingItem::new(text.models_max_tokens.clone(), max_tokens_field)
            .description(text.models_max_tokens_hint.clone()),
    );
    items.push(SettingItem::new(
        text.models_reasoning_default.clone(),
        reasoning_field,
    ));
    items.push(presets_item);

    // G-SE12-3: request shaping. The provider format is fixed for a stored row
    // (changing it belongs to the provider picker on the web/desktop pages),
    // so it is shown read-only; the overrides below apply to it.
    items.push(section_heading_item(
        "model-advanced",
        text.models_advanced.clone(),
        text.models_advanced_note.clone(),
    ));
    let request_format = model.provider.clone();
    let request_format_title = text.models_request_format.clone();
    items.push(
        SettingItem::render(move |_options, _window, cx| {
            div()
                .v_flex()
                .gap_0p5()
                .w_full()
                .child(div().text_sm().child(request_format_title.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(request_format.clone()),
                )
                .into_any_element()
        })
        .layout(Axis::Vertical),
    );

    let inline_think_get_id = model_id.clone();
    let inline_think_set_id = model_id.clone();
    let inline_think_field = SettingField::switch(
        move |cx: &App| {
            ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == inline_think_get_id)
                .map(|model| model.inline_think_in_text)
                .unwrap_or(true)
        },
        move |value: bool, cx: &mut App| {
            set_ai_model_inline_think(&inline_think_set_id, value, cx);
        },
    );

    let skip_ssl_get_id = model_id.clone();
    let skip_ssl_set_id = model_id.clone();
    let skip_ssl_field = SettingField::switch(
        move |cx: &App| {
            ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == skip_ssl_get_id)
                .map(|model| model.skip_ssl_verify)
                .unwrap_or(false)
        },
        move |value: bool, cx: &mut App| {
            set_ai_model_skip_ssl(&skip_ssl_set_id, value, cx);
        },
    );

    let headers_get_id = model_id.clone();
    let headers_set_id = model_id.clone();
    let headers_field = SettingField::input(
        move |cx: &App| {
            let editor = ModelEditorState::current(cx);
            if editor.drafts_ready && editor.model_id.as_deref() == Some(headers_get_id.as_str()) {
                return SharedString::from(editor.headers_text);
            }
            let stored = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == headers_get_id)
                .and_then(|model| model.custom_headers.clone())
                .unwrap_or_default();
            SharedString::from(serde_json::to_string(&stored).unwrap_or_else(|_| "{}".to_string()))
        },
        move |value: SharedString, cx: &mut App| {
            set_ai_model_headers_text(&headers_set_id, value.to_string(), cx);
        },
    );

    let headers_mode_get_id = model_id.clone();
    let headers_mode_set_id = model_id.clone();
    let stored_headers_mode = model
        .custom_headers_mode
        .clone()
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "replace".to_string());
    let mut headers_mode_options = vec![
        (
            SharedString::from("replace"),
            text.models_headers_replace.clone().into(),
        ),
        (
            SharedString::from("merge"),
            text.models_headers_merge.clone().into(),
        ),
    ];
    if !headers_mode_options
        .iter()
        .any(|(value, _)| value.as_ref() == stored_headers_mode)
    {
        headers_mode_options.push((
            SharedString::from(stored_headers_mode.clone()),
            SharedString::from(stored_headers_mode.clone()),
        ));
    }
    let headers_mode_field = SettingField::dropdown(
        headers_mode_options,
        move |cx: &App| {
            let stored = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == headers_mode_get_id)
                .and_then(|model| model.custom_headers_mode.clone())
                .filter(|mode| !mode.is_empty())
                .unwrap_or_else(|| "replace".to_string());
            SharedString::from(stored)
        },
        move |value: SharedString, cx: &mut App| {
            set_ai_model_header_mode(&headers_mode_set_id, value.to_string(), cx);
        },
    );

    let body_get_id = model_id.clone();
    let body_set_id = model_id.clone();
    let body_field = SettingField::input(
        move |cx: &App| {
            let editor = ModelEditorState::current(cx);
            if editor.drafts_ready && editor.model_id.as_deref() == Some(body_get_id.as_str()) {
                return SharedString::from(editor.body_text);
            }
            SharedString::from(
                ShellConfig::current(cx)
                    .ai_models
                    .iter()
                    .find(|model| model.id == body_get_id)
                    .and_then(|model| model.custom_request_body.clone())
                    .unwrap_or_default(),
            )
        },
        move |value: SharedString, cx: &mut App| {
            set_ai_model_body_text(&body_set_id, value.to_string(), cx);
        },
    );

    let body_mode_get_id = model_id.clone();
    let body_mode_set_id = model_id.clone();
    let stored_body_mode = model
        .custom_request_body_mode
        .clone()
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "merge".to_string());
    let mut body_mode_options = vec![
        (
            SharedString::from("merge"),
            text.models_body_merge.clone().into(),
        ),
        (
            SharedString::from("trim"),
            text.models_body_trim.clone().into(),
        ),
    ];
    if !body_mode_options
        .iter()
        .any(|(value, _)| value.as_ref() == stored_body_mode)
    {
        body_mode_options.push((
            SharedString::from(stored_body_mode.clone()),
            SharedString::from(stored_body_mode.clone()),
        ));
    }
    let body_mode_field = SettingField::dropdown(
        body_mode_options,
        move |cx: &App| {
            let stored = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == body_mode_get_id)
                .and_then(|model| model.custom_request_body_mode.clone())
                .filter(|mode| !mode.is_empty())
                .unwrap_or_else(|| "merge".to_string());
            SharedString::from(stored)
        },
        move |value: SharedString, cx: &mut App| {
            set_ai_model_body_mode(&body_mode_set_id, value.to_string(), cx);
        },
    );

    let headers_description = if stored_headers_mode == "merge" {
        text.models_headers_merge_hint.clone()
    } else {
        text.models_headers_replace_hint.clone()
    };
    let body_description = if stored_body_mode == "trim" {
        text.models_body_trim_hint.clone()
    } else {
        text.models_body_hint.clone()
    };

    items.push(
        SettingItem::new(text.models_inline_think.clone(), inline_think_field)
            .description(text.models_inline_think_hint.clone()),
    );
    items.push(
        SettingItem::new(text.models_skip_ssl.clone(), skip_ssl_field)
            .description(text.models_skip_ssl_warning.clone()),
    );
    items.push(
        SettingItem::new(text.models_headers.clone(), headers_field)
            .description(format!(
                "{} {}",
                text.models_headers_hint, headers_description
            ))
            .layout(Axis::Vertical),
    );
    items.push(SettingItem::new(
        text.models_headers_mode.clone(),
        headers_mode_field,
    ));
    items.push(
        SettingItem::new(text.models_body.clone(), body_field)
            .description(body_description)
            .layout(Axis::Vertical),
    );
    items.push(SettingItem::new(
        text.models_body_mode.clone(),
        body_mode_field,
    ));

    // Invalid JSON stays in the draft (the stored row is untouched); say so
    // instead of silently showing a field that is not saving.
    let editor = ModelEditorState::current(cx);
    let drafts_here = editor.drafts_ready && editor.model_id.as_deref() == Some(model_id.as_str());
    let invalid = drafts_here
        && (!headers_json_valid(&editor.headers_text) || !body_json_valid(&editor.body_text));
    if invalid {
        let invalid_text = text.models_json_invalid.clone();
        items.push(
            SettingItem::render(move |_options, _window, cx| {
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(invalid_text.clone())
                    .into_any_element()
            })
            .layout(Axis::Vertical),
        );
    }

    Some(items)
}

/// One in-page section heading, standing in for the web `ConfigPageSection`
/// title a flat shell group has no equivalent for. The slug is the element id
/// the headless layout tests assert on.
fn section_heading_item(slug: &'static str, title: String, note: String) -> SettingItem {
    SettingItem::render(move |_options, _window, cx| {
        let mut heading = div()
            .debug_selector(move || format!("settings-section-{slug}"))
            .v_flex()
            .w_full()
            .gap_0p5()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child(title.clone()),
            );
        if !note.is_empty() {
            heading = heading.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(note.clone()),
            );
        }
        heading.into_any_element()
    })
}

/// One of the shell's own global key bindings (G-SE7). The shortcuts page
/// renders this table, and [`install_global_shortcuts`] binds the very same
/// entries, so the documented keys cannot drift from the installed ones.
pub(crate) struct GlobalShortcut {
    /// Keystroke string exactly as the binding records it.
    pub(crate) stroke: &'static str,
    /// The bound action; a unit action of the shell.
    pub(crate) action: fn() -> Box<dyn gpui_kit::Action>,
    /// Row label against the shell copy (reuses the string the action's own
    /// affordance shows).
    pub(crate) label: fn(&ShellText) -> String,
}

/// The shell's global shortcuts, in binding order. No key context: they match
/// wherever no focused view consumes the keystroke first (a terminal pane or a
/// text editor on the focus path wins, exactly as on the desktop).
pub(crate) const GLOBAL_SHORTCUTS: &[GlobalShortcut] = &[
    GlobalShortcut {
        stroke: "ctrl-k",
        action: || Box::new(crate::shell::OpenGlobalSearch),
        label: |text| text.settings_jump_session_action.clone(),
    },
    GlobalShortcut {
        stroke: "ctrl-b",
        action: || Box::new(crate::shell::ToggleSidebar),
        label: |text| text.sidebar_toggle.clone(),
    },
];

/// Installs the global bindings from [`GLOBAL_SHORTCUTS`] — the same table the
/// settings page documents. A stroke that does not parse panics here at
/// startup rather than silently shipping an unbound key.
pub(crate) fn install_global_shortcuts(cx: &mut App) {
    cx.bind_keys(GLOBAL_SHORTCUTS.iter().map(|shortcut| {
        gpui_kit::KeyBinding::load(
            shortcut.stroke,
            (shortcut.action)(),
            None,
            false,
            None,
            &gpui_kit::DummyKeyboardMapper,
        )
        .unwrap_or_else(|error| {
            panic!(
                "Shell shortcut {} does not parse: {}",
                shortcut.stroke, error
            )
        })
    }));
}

/// G-SE7 read-only shortcuts page (`application.shortcuts`): what the shell
/// honors, grouped the way the web page groups scopes — its own global
/// bindings first, then the keys the component layer and the shell's handlers
/// provide. The web page's recording, conflict detection and reset have no
/// shell counterpart (there is no keybinding document to write), so the page
/// states that instead of offering controls that could not persist.
fn shortcuts_group(text: &ShellText, _cx: &App) -> SettingGroup {
    let mut items = vec![
        SettingItem::render({
            let note = text.shortcuts_fixed_note.clone();
            move |_options, _window, cx| info_note(&note, cx)
        }),
        section_heading_item(
            "shortcuts-global",
            text.shortcuts_section_global.clone(),
            String::new(),
        ),
    ];

    for shortcut in GLOBAL_SHORTCUTS {
        items.push(shortcut_row(&[shortcut.stroke], (shortcut.label)(text)));
    }

    items.push(section_heading_item(
        "shortcuts-overlays",
        text.shortcuts_section_overlays.clone(),
        String::new(),
    ));
    items.push(shortcut_row(
        &["escape"],
        text.shortcuts_overlay_dismiss.clone(),
    ));
    items.push(shortcut_row(
        &["up", "down"],
        text.shortcuts_overlay_move.clone(),
    ));
    items.push(shortcut_row(
        &["enter"],
        text.shortcuts_overlay_confirm.clone(),
    ));

    items.push(section_heading_item(
        "shortcuts-terminal",
        text.shortcuts_section_terminal.clone(),
        String::new(),
    ));
    items.push(shortcut_row(
        &["ctrl-shift-c"],
        text.shortcuts_terminal_copy.clone(),
    ));
    items.push(shortcut_row(
        &["ctrl-shift-v"],
        text.shortcuts_terminal_paste.clone(),
    ));
    items.push(shortcut_row(&[], text.shortcuts_terminal_note.clone()));

    items.push(section_heading_item(
        "shortcuts-editing",
        text.shortcuts_section_editing.clone(),
        String::new(),
    ));
    items.push(shortcut_row(&[], text.shortcuts_editing_note.clone()));

    SettingGroup::new().items(items)
}

/// One read-only shortcut row: label on the left, the keycaps on the right —
/// the web page's `Kbd` column. A row with no strokes is a plain note line
/// (the terminal and editing rows state behavior whose keys the platform
/// owns). The row carries a debug selector named after its first stroke (or
/// its label for note rows), so a headless test can prove the table rendered.
fn shortcut_row(strokes: &[&str], label: String) -> SettingItem {
    let strokes: Vec<String> = strokes.iter().map(|stroke| stroke.to_string()).collect();
    let selector = match strokes.first() {
        Some(stroke) => format!("shortcut-row-{}", stroke),
        None => format!("shortcut-row-{}", label),
    };
    SettingItem::render(move |_options, _window, cx| {
        let mut caps = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .flex_shrink_0();
        for stroke in &strokes {
            if let Ok(keystroke) = gpui_kit::Keystroke::parse(stroke) {
                caps = caps.child(gpui_kit::component::kbd::Kbd::new(keystroke));
            }
        }
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .w_full()
            .debug_selector({
                let selector = selector.clone();
                move || selector.clone()
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(label.clone()),
                    ),
            )
            .child(caps)
            .into_any_element()
    })
}

/// G-SE10: one streaming-timeout number field. Zero writes `null`, the shared
/// "wait indefinitely" value; a positive value writes the seconds.
fn stream_timeout_field(
    path: &'static str,
    read: fn(&ShellConfig) -> Option<u64>,
    write: fn(&mut ShellConfig, Option<u64>),
) -> SettingField<f64> {
    SettingField::number_input(
        NumberFieldOptions {
            min: 0.,
            max: MAX_STREAM_TIMEOUT_SECS,
            step: 30.,
        },
        move |cx: &App| read(&ShellConfig::current(cx)).unwrap_or(0) as f64,
        move |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let secs = (value.round() as u64).min(MAX_STREAM_TIMEOUT_SECS as u64);
            let stored = (secs > 0).then_some(secs);
            write(cx.global_mut::<ShellConfig>(), stored);
            persist(
                path,
                match stored {
                    Some(secs) => serde_json::json!(secs),
                    None => serde_json::Value::Null,
                },
            );
            cx.refresh_windows();
        },
    )
}

/// G-SE11: one proxy string field, written back as a single key of the stored
/// document.
fn proxy_text_field(key: &'static str) -> SettingField<SharedString> {
    SettingField::input(
        move |cx: &App| SharedString::from(proxy_text(&ShellConfig::current(cx).proxy, key)),
        move |value: SharedString, cx: &mut App| {
            set_proxy_key(key, serde_json::json!(value.to_string()), cx);
        },
    )
}

/// Terminal preferences for the built-in terminal (M7 pane). The values are
/// the shared `terminal.*` entries, so the desktop terminal and this pane read
/// the same document; font size and family apply to a running pane, while the
/// scrollback and shell command are read when a pane starts.
fn terminal_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingGroup {
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

    SettingGroup::new().items(vec![
        SettingItem::new(text.settings_terminal_font_size.clone(), font_size_field)
            .description(text.settings_terminal_font_size_description.clone()),
        SettingItem::new(
            text.settings_terminal_font_family.clone(),
            font_family_field,
        )
        .description(text.settings_terminal_font_family_description.clone()),
        SettingItem::new(text.settings_terminal_scrollback.clone(), scrollback_field)
            .description(text.settings_terminal_scrollback_description.clone()),
        SettingItem::new(text.settings_terminal_shell.clone(), shell_field)
            .description(text.settings_terminal_shell_description.clone()),
        SettingItem::new(text.settings_terminal_profiles.clone(), profiles_field)
            .description(text.settings_terminal_profiles_description.clone()),
    ])
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
                .max_w(px(CONTROL_COLUMN_MAX_WIDTH))
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

/// A2 (G-SE10): the streaming-timeout rows clamp to a day; the shared service
/// has no upper bound, the field needs one.
pub(crate) const MAX_STREAM_TIMEOUT_SECS: f64 = 86_400.;
/// A2 (G-SE18): the runtime-timeout rows, the web `NumberInput` range.
pub(crate) const MAX_RUNTIME_TIMEOUT_SECS: f64 = 3_600.;

/// A2 (G-SE18): one runtime timeout field. `null_when_zero` distinguishes the
/// two shared contracts: the tool timeout stores `null` for "never", the
/// unattended-question timeout stores the zero the web page writes.
fn runtime_timeout_field(
    path: &'static str,
    read: fn(&ShellConfig) -> Option<u64>,
    write: fn(&mut ShellConfig, Option<u64>),
    null_when_zero: bool,
) -> SettingField<f64> {
    SettingField::number_input(
        NumberFieldOptions {
            min: 0.,
            max: MAX_RUNTIME_TIMEOUT_SECS,
            step: 5.,
        },
        move |cx: &App| read(&ShellConfig::current(cx)).unwrap_or(0) as f64,
        move |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let secs = (value.round() as u64).min(MAX_RUNTIME_TIMEOUT_SECS as u64);
            let stored = if null_when_zero && secs == 0 {
                None
            } else {
                Some(secs)
            };
            write(cx.global_mut::<ShellConfig>(), stored);
            let value = match stored {
                Some(secs) => serde_json::json!(secs),
                None => serde_json::Value::Null,
            };
            persist(path, value);
            cx.refresh_windows();
        },
    )
}

/// A2 (G-SE19): one concurrency field, clamped onto the domain the web page
/// grants its number inputs.
fn concurrency_field(
    max: u32,
    read: fn(&ShellConfig) -> u32,
    write: fn(&mut ShellConfig, u32),
    path: &'static str,
) -> SettingField<f64> {
    SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(MIN_SUBAGENT_CONCURRENCY),
            max: f64::from(max),
            step: 1.,
        },
        move |cx: &App| f64::from(read(&ShellConfig::current(cx))),
        move |value: f64, cx: &mut App| {
            let Some(limit) = clamp_concurrency(value, max) else {
                tracing::warn!("Ignoring out-of-range concurrency value: {}", value);
                return;
            };
            write(cx.global_mut::<ShellConfig>(), limit as u32);
            persist(path, serde_json::json!(limit));
            cx.refresh_windows();
        },
    )
}

/// A2 (G-SE19): the subagent-model menu — the two shared aliases first, then
/// every enabled configured model, then the stored id when it is unavailable
/// (the web combobox appends the same entry).
fn subagent_model_options(
    catalog: &[crate::runtime::ModelChoice],
    current: &str,
    text: &ShellText,
) -> Vec<(SharedString, SharedString)> {
    let mut options: Vec<(SharedString, SharedString)> = vec![
        (
            SharedString::from("inherit"),
            SharedString::from(text.settings_subagent_model_inherit.clone()),
        ),
        (
            SharedString::from("fast"),
            SharedString::from(text.settings_subagent_model_fast.clone()),
        ),
        (
            SharedString::from("primary"),
            SharedString::from(text.settings_subagent_model_primary.clone()),
        ),
    ];
    for model in catalog.iter().filter(|model| model.enabled) {
        options.push((model.id.clone().into(), model.label().into()));
    }
    if !options.iter().any(|(id, _)| id.as_ref() == current) {
        options.push((
            SharedString::from(current.to_string()),
            SharedString::from(format!("{current} ({})", text.value_unsupported)),
        ));
    }
    options
}

/// The batch-policy menu: the two shared policies, then a stored policy this
/// build does not render so the row shows what is actually persisted.
fn subagent_batch_options(current: &str, text: &ShellText) -> Vec<(SharedString, SharedString)> {
    let mut options: Vec<(SharedString, SharedString)> = SUBAGENT_BATCH_POLICIES
        .iter()
        .map(|policy| {
            let label = match *policy {
                "safe_only" => text.settings_subagent_batch_safe.clone(),
                _ => text.settings_subagent_batch_parallel.clone(),
            };
            (SharedString::from(*policy), SharedString::from(label))
        })
        .collect();
    if !SUBAGENT_BATCH_POLICIES.contains(&current) {
        options.insert(
            0,
            (
                SharedString::from(current.to_string()),
                SharedString::from(format!("{current} ({})", text.value_unsupported)),
            ),
        );
    }
    options
}

/// A2 (G-SE22): one review-capacity number field over the stored team
/// document; every other key of the document is kept by the write.
fn review_capacity_field(
    key: &'static str,
    min: u32,
    max: u32,
    step: f64,
    fallback: u64,
) -> SettingField<f64> {
    SettingField::number_input(
        NumberFieldOptions {
            min: f64::from(min),
            max: f64::from(max),
            step,
        },
        move |cx: &App| object_u64(&ShellConfig::current(cx).review_team, key, fallback) as f64,
        move |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as u64).clamp(u64::from(min), u64::from(max));
            set_review_capacity(key, serde_json::json!(clamped), cx);
        },
    )
}

/// G-SE13 memory page (`ai.memory`): the shared `memories` config section —
/// the switches, model selectors and advanced extraction/consolidation knobs
/// the desktop page edits, plus the memory-actions menu (open directory /
/// reset settings / wipe). Copy is the desktop `settings/memory` catalog
/// verbatim; the web's toasts become the inline [`MemoryNotice`].
fn memory_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> SettingGroup {
    let advanced_open = MemoryPageState::current(cx).advanced_open;
    let memories = ShellConfig::current(cx).memories.clone();
    let memory_enabled = memories.generate_memories && memories.use_memories;
    let work_disabled = !memory_enabled;

    // 基础：记忆总开关（generate_memories 与 use_memories 同行同写，与 web
    // `updateMemoryEnabled` 一致）、BTW 来源、外部上下文策略、记忆操作菜单。
    let enabled_field = SettingField::switch(
        move |cx: &App| {
            let memories = &ShellConfig::current(cx).memories;
            memories.generate_memories && memories.use_memories
        },
        |value: bool, cx: &mut App| {
            {
                let memories = &mut cx.global_mut::<ShellConfig>().memories;
                memories.generate_memories = value;
                memories.use_memories = value;
            }
            persist_object_key(MEMORIES_PATH, "generate_memories", serde_json::json!(value));
            persist_object_key(MEMORIES_PATH, "use_memories", serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    let btw_field = SettingField::switch(
        move |cx: &App| ShellConfig::current(cx).memories.generate_for_btw_sessions,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>()
                .memories
                .generate_for_btw_sessions = value;
            persist_object_key(
                MEMORIES_PATH,
                "generate_for_btw_sessions",
                serde_json::json!(value),
            );
            cx.refresh_windows();
        },
    );

    let policy_field = SettingField::dropdown(
        vec![
            (
                "clear_tool_results",
                text.memory_policy_clear_tool_results.clone(),
            ),
            ("allow", text.memory_policy_allow.clone()),
            ("skip_session", text.memory_policy_skip_session.clone()),
        ]
        .into_iter()
        .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
        .collect::<Vec<(SharedString, SharedString)>>(),
        |cx: &App| {
            SharedString::from(memory_policy_value(
                ShellConfig::current(cx).memories.external_context_policy,
            ))
        },
        |value: SharedString, cx: &mut App| {
            let policy = memory_policy_from_str(value.as_ref());
            cx.global_mut::<ShellConfig>()
                .memories
                .external_context_policy = policy;
            persist_object_key(
                MEMORIES_PATH,
                "external_context_policy",
                serde_json::json!(memory_policy_value(policy)),
            );
            cx.refresh_windows();
        },
    );

    let actions_view = view.clone();
    let actions_field = SettingField::render(move |_options, _window, cx| {
        let text = crate::i18n::current(cx);
        let busy = MemoryPageState::current(cx).busy;
        let menu_view = actions_view.clone();
        Button::new("memory-actions-menu")
            .label(text.memory_action_manage.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .disabled(busy)
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, cx| {
                memory_actions_menu(menu, menu_view.clone(), cx)
            })
            .into_any_element()
    });

    // 模型：抽取与整理各自跟随（空 = 跟随）或指定目录内启用模型。
    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();
    let extract_field = SettingField::dropdown(
        memory_model_options(
            &catalog,
            text.memory_model_follow_primary.clone(),
            memories.extract_model.as_deref(),
            cx,
        ),
        |cx: &App| {
            SharedString::from(
                ShellConfig::current(cx)
                    .memories
                    .extract_model
                    .clone()
                    .unwrap_or_default(),
            )
        },
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            let stored = (!value.is_empty()).then_some(value.clone());
            cx.global_mut::<ShellConfig>().memories.extract_model = stored.clone();
            persist_object_key(
                MEMORIES_PATH,
                "extract_model",
                stored.map_or(serde_json::Value::Null, serde_json::Value::String),
            );
            cx.refresh_windows();
        },
    );

    let consolidation_field = SettingField::dropdown(
        memory_model_options(
            &catalog,
            text.memory_model_follow_extraction.clone(),
            memories.consolidation_model.as_deref(),
            cx,
        ),
        |cx: &App| {
            SharedString::from(
                ShellConfig::current(cx)
                    .memories
                    .consolidation_model
                    .clone()
                    .unwrap_or_default(),
            )
        },
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            let stored = (!value.is_empty()).then_some(value.clone());
            cx.global_mut::<ShellConfig>().memories.consolidation_model = stored.clone();
            persist_object_key(
                MEMORIES_PATH,
                "consolidation_model",
                stored.map_or(serde_json::Value::Null, serde_json::Value::String),
            );
            cx.refresh_windows();
        },
    );

    // 高级设置：可折叠（web 的 section extra chevron），默认收起。
    let advanced_view = view.clone();
    let advanced_toggle = SettingField::render(move |_options, _window, cx| {
        let text = crate::i18n::current(cx);
        let open = MemoryPageState::current(cx).advanced_open;
        let click_view = advanced_view.clone();
        Button::new("memory-advanced-toggle")
            .label(if open {
                text.memory_action_collapse_advanced.clone()
            } else {
                text.memory_action_expand_advanced.clone()
            })
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(move |_, _window, cx| {
                if let Some(shell) = click_view.upgrade() {
                    shell.update(cx, |this, cx| this.toggle_memory_advanced(cx));
                }
            })
            .into_any_element()
    });

    let mut items = vec![
        SettingItem::render({
            let notice = text.memory_scope_notice.clone();
            move |_options, _window, cx| memory_scope_notice(&notice, cx)
        }),
        section_heading_item(
            "memory-basic",
            text.memory_section_basic.clone(),
            text.memory_section_basic_note.clone(),
        ),
        SettingItem::new(text.memory_enabled.clone(), enabled_field)
            .description(text.memory_enabled_description.clone()),
        SettingItem::new(text.memory_generate_btw.clone(), btw_field)
            .description(text.memory_generate_btw_description.clone())
            .disabled(work_disabled),
        SettingItem::new(text.memory_external_context.clone(), policy_field)
            .description(text.memory_external_context_description.clone())
            .disabled(work_disabled),
        SettingItem::new(text.memory_actions_label.clone(), actions_field)
            .description(text.memory_actions_description.clone()),
        section_heading_item(
            "memory-models",
            text.memory_section_models.clone(),
            text.memory_section_models_note.clone(),
        ),
        SettingItem::new(text.memory_extract_model.clone(), extract_field)
            .description(text.memory_extract_model_description.clone())
            .disabled(work_disabled),
        SettingItem::new(text.memory_consolidation_model.clone(), consolidation_field)
            .description(text.memory_consolidation_model_description.clone())
            .disabled(work_disabled),
        section_heading_item(
            "memory-advanced",
            text.memory_section_advanced.clone(),
            text.memory_section_advanced_note.clone(),
        ),
        SettingItem::new(text.memory_action_expand_advanced.clone(), advanced_toggle),
    ];

    if advanced_open {
        // A disclosure marker item, so a headless test can prove the block is
        // on the page (and gone again when collapsed) instead of only reading
        // the flag. Items cannot nest: `SettingItem` is rendered by the group
        // that owns it, not into a custom element.
        items.push(SettingItem::render(|_options, _window, cx| {
            div()
                .w_full()
                .h(px(1.))
                .bg(cx.theme().border)
                .debug_selector(|| "memory-advanced-block".to_string())
                .into_any_element()
        }));
        items.extend(memory_advanced_items(&memories, text));
    }

    // The page's inline receipt replaces the web toasts; it sits above the
    // first section so the outcome of a menu action stays visible.
    if let Some((succeeded, message)) = MemoryNotice::current(cx) {
        items.insert(
            0,
            SettingItem::render(move |_options, _window, cx| {
                div()
                    .w_full()
                    .text_sm()
                    .text_color(if succeeded {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().danger
                    })
                    .child(message.clone())
                    .into_any_element()
            }),
        );
    }

    SettingGroup::new().items(items)
}

/// The advanced numbers (web `sections.advanced`): extraction and
/// consolidation windows. Each key was validated to live in the `memories`
/// document; ranges mirror the web `NumberInput` bounds, and the two window
/// fields are cross-bounded (回溯 ≤ 保留, web `isValidMemoryWindowConfig`).
fn memory_advanced_items(
    memories: &openbitfun_core::service::config::types::MemoriesConfig,
    text: &ShellText,
) -> Vec<SettingItem> {
    let idle_field = SettingField::number_input(
        NumberFieldOptions {
            min: 1.,
            max: 48.,
            step: 1.,
        },
        move |cx: &App| ShellConfig::current(cx).memories.min_rollout_idle_hours as f64,
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as i64).clamp(1, 48);
            cx.global_mut::<ShellConfig>()
                .memories
                .min_rollout_idle_hours = clamped;
            persist_object_key(
                MEMORIES_PATH,
                "min_rollout_idle_hours",
                serde_json::json!(clamped),
            );
            cx.refresh_windows();
        },
    );

    let age_field = SettingField::number_input(
        NumberFieldOptions {
            min: 0.,
            max: memories.max_unused_days.max(0) as f64,
            step: 1.,
        },
        move |cx: &App| ShellConfig::current(cx).memories.max_rollout_age_days as f64,
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as i64).clamp(0, 3650);
            let retained = cx.global::<ShellConfig>().memories.max_unused_days;
            let next = clamped.min(retained);
            cx.global_mut::<ShellConfig>().memories.max_rollout_age_days = next;
            persist_object_key(
                MEMORIES_PATH,
                "max_rollout_age_days",
                serde_json::json!(next),
            );
            cx.refresh_windows();
        },
    );

    let per_startup_field = SettingField::number_input(
        NumberFieldOptions {
            min: 1.,
            max: 128.,
            step: 1.,
        },
        move |cx: &App| ShellConfig::current(cx).memories.max_rollouts_per_startup as f64,
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as usize).clamp(1, 128);
            cx.global_mut::<ShellConfig>()
                .memories
                .max_rollouts_per_startup = clamped;
            persist_object_key(
                MEMORIES_PATH,
                "max_rollouts_per_startup",
                serde_json::json!(clamped),
            );
            cx.refresh_windows();
        },
    );

    let scan_field = SettingField::number_input(
        NumberFieldOptions {
            min: 1.,
            max: 50_000.,
            step: 100.,
        },
        move |cx: &App| ShellConfig::current(cx).memories.max_rollouts_scan_limit as f64,
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as usize).clamp(1, 50_000);
            cx.global_mut::<ShellConfig>()
                .memories
                .max_rollouts_scan_limit = clamped;
            persist_object_key(
                MEMORIES_PATH,
                "max_rollouts_scan_limit",
                serde_json::json!(clamped),
            );
            cx.refresh_windows();
        },
    );

    let concurrency_field = SettingField::number_input(
        NumberFieldOptions {
            min: 1.,
            max: 16.,
            step: 1.,
        },
        move |cx: &App| ShellConfig::current(cx).memories.phase1_max_concurrency as f64,
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as usize).clamp(1, 16);
            cx.global_mut::<ShellConfig>()
                .memories
                .phase1_max_concurrency = clamped;
            persist_object_key(
                MEMORIES_PATH,
                "phase1_max_concurrency",
                serde_json::json!(clamped),
            );
            cx.refresh_windows();
        },
    );

    let raw_field = SettingField::number_input(
        NumberFieldOptions {
            min: 1.,
            max: 4096.,
            step: 1.,
        },
        move |cx: &App| {
            ShellConfig::current(cx)
                .memories
                .max_raw_memories_for_consolidation as f64
        },
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as usize).clamp(1, 4096);
            cx.global_mut::<ShellConfig>()
                .memories
                .max_raw_memories_for_consolidation = clamped;
            persist_object_key(
                MEMORIES_PATH,
                "max_raw_memories_for_consolidation",
                serde_json::json!(clamped),
            );
            cx.refresh_windows();
        },
    );

    let retention_field = SettingField::number_input(
        NumberFieldOptions {
            min: memories.max_rollout_age_days.max(0) as f64,
            max: 365.,
            step: 1.,
        },
        move |cx: &App| ShellConfig::current(cx).memories.max_unused_days as f64,
        |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let clamped = (value.round() as i64).clamp(0, 365);
            let age = cx.global::<ShellConfig>().memories.max_rollout_age_days;
            let next = clamped.max(age);
            cx.global_mut::<ShellConfig>().memories.max_unused_days = next;
            persist_object_key(MEMORIES_PATH, "max_unused_days", serde_json::json!(next));
            cx.refresh_windows();
        },
    );

    vec![
        SettingItem::new(text.memory_min_idle_hours.clone(), idle_field)
            .description(text.memory_min_idle_hours_description.clone()),
        SettingItem::new(text.memory_max_age_days.clone(), age_field)
            .description(text.memory_max_age_days_description.clone()),
        SettingItem::new(text.memory_per_startup.clone(), per_startup_field)
            .description(text.memory_per_startup_description.clone()),
        SettingItem::new(text.memory_scan_limit.clone(), scan_field)
            .description(text.memory_scan_limit_description.clone()),
        SettingItem::new(text.memory_concurrency.clone(), concurrency_field)
            .description(text.memory_concurrency_description.clone()),
        SettingItem::new(text.memory_raw_limit.clone(), raw_field)
            .description(text.memory_raw_limit_description.clone()),
        SettingItem::new(text.memory_retention_days.clone(), retention_field)
            .description(text.memory_retention_days_description.clone()),
    ]
}

/// A page-level info line, tinted with the info color: the memory page's
/// scope notice and the shortcuts page's fixed-bindings note share it.
fn info_note(message: &str, cx: &App) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .rounded(cx.theme().radius)
        .bg(cx.theme().info.opacity(0.08))
        .px_3()
        .py_2()
        .text_sm()
        .text_color(cx.theme().info)
        .child(message.to_string())
        .into_any_element()
}

/// The info line above the basic section (web `scopeNotice`).
fn memory_scope_notice(message: &str, cx: &App) -> gpui_kit::AnyElement {
    info_note(message, cx)
}

/// G-SE13: the memory-actions menu (web `actions.manage` menu): open the
/// memory directory, reset the settings document, wipe the memory data. The
/// two destructive entries gate on the confirm dialogs below.
fn memory_actions_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> gpui_kit::component::menu::PopupMenu {
    let text = i18n::current(cx);
    let open_view = view.clone();
    let reset_view = view.clone();
    let clear_view = view.clone();
    menu.item(
        PopupMenuItem::new(text.memory_action_open_directory.clone()).on_click(
            move |_, _window, cx| {
                if let Some(shell) = open_view.upgrade() {
                    shell.update(cx, |this, cx| this.open_memory_directory(cx));
                }
            },
        ),
    )
    .item(
        PopupMenuItem::new(text.memory_action_reset_settings.clone()).on_click(
            move |_, window, cx| {
                open_memory_confirm_dialog(
                    MemoryResetKind::Settings,
                    reset_view.clone(),
                    window,
                    cx,
                );
            },
        ),
    )
    .separator()
    .item(
        PopupMenuItem::new(text.memory_action_reset_memory.clone()).on_click(
            move |_, window, cx| {
                open_memory_confirm_dialog(MemoryResetKind::Memory, clear_view.clone(), window, cx);
            },
        ),
    )
}

/// G-SE13: which destructive memory action a confirm dialog guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemoryResetKind {
    Settings,
    Memory,
}

/// G-SE13: the confirm dialog before a memory action that resets settings or
/// wipes data (web's two `ConfirmDialog`s; the wipe is the danger variant).
fn open_memory_confirm_dialog(
    kind: MemoryResetKind,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    let text = i18n::current(cx);
    let (title, message, confirm_label) = match kind {
        MemoryResetKind::Settings => (
            text.memory_reset_settings_confirm_title.clone(),
            text.memory_reset_settings_confirm_message.clone(),
            text.memory_reset_settings_confirm_action.clone(),
        ),
        MemoryResetKind::Memory => (
            text.memory_action_reset_memory.clone(),
            text.memory_reset_confirm_message.clone(),
            text.memory_reset_confirm_action.clone(),
        ),
    };
    let confirm_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        if let Some(shell) = confirm_view.upgrade() {
            shell.update(cx, |this, cx| match kind {
                MemoryResetKind::Settings => this.reset_memory_settings(cx),
                MemoryResetKind::Memory => this.clear_memory_data(cx),
            });
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        window.close_dialog(cx);
    };
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(title.clone()))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(message.clone()),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("memory-confirm-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("memory-confirm-accept")
                            .label(confirm_label.clone())
                            .with_variant(match kind {
                                MemoryResetKind::Settings => ButtonVariant::Primary,
                                MemoryResetKind::Memory => ButtonVariant::Danger,
                            })
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Serialized `external_context_policy` ids (the shared snake_case contract the
/// desktop page uses).
pub(crate) fn memory_policy_value(
    policy: openbitfun_core::service::config::types::MemoryExternalContextPolicy,
) -> &'static str {
    use openbitfun_core::service::config::types::MemoryExternalContextPolicy as Policy;
    match policy {
        Policy::ClearToolResults => "clear_tool_results",
        Policy::Allow => "allow",
        Policy::SkipSession => "skip_session",
    }
}

/// Parses a stored policy id; unknown ids fall back to the shared default
/// (clear_tool_results) instead of inventing a fourth state.
pub(crate) fn memory_policy_from_str(
    raw: &str,
) -> openbitfun_core::service::config::types::MemoryExternalContextPolicy {
    use openbitfun_core::service::config::types::MemoryExternalContextPolicy as Policy;
    match raw {
        "allow" => Policy::Allow,
        "skip_session" => Policy::SkipSession,
        _ => Policy::ClearToolResults,
    }
}

/// Model selector options for the memory page: the follow entry (empty
/// value), the two aliases, then the enabled catalog models. A stored id
/// missing from the catalog stays listed with the "unavailable" suffix (web
/// `buildModelOptions`) so the value is never silently dropped.
pub(crate) fn memory_model_options(
    catalog: &crate::runtime::ModelCatalog,
    follow_label: String,
    stored: Option<&str>,
    cx: &App,
) -> Vec<(SharedString, SharedString)> {
    let text = crate::i18n::current(cx);
    let mut options: Vec<(SharedString, SharedString)> = vec![
        (SharedString::from(""), SharedString::from(follow_label)),
        (
            SharedString::from("primary"),
            text.memory_model_primary.clone().into(),
        ),
        (
            SharedString::from("fast"),
            text.memory_model_fast.clone().into(),
        ),
    ];
    for model in catalog.models.iter().filter(|model| model.enabled) {
        options.push((model.id.clone().into(), model.label().into()));
    }
    if let Some(stored) = stored.filter(|id| !id.is_empty()) {
        if stored != "primary"
            && stored != "fast"
            && !options.iter().any(|(value, _)| value.as_ref() == stored)
        {
            options.push((
                SharedString::from(stored.to_string()),
                SharedString::from(crate::i18n::memory_model_unavailable(stored, cx)),
            ));
        }
    }
    options
}

/// G-SE14 engine page (`ai.trinity`): the trinityd LLM configuration the
/// desktop page edits — provider presets, endpoint/model/key fields, an
/// inline connection test and the save action. The daemon owns the toml and
/// hot-swaps on save; the page only forwards (web `TrinityEngineConfig` +
/// `TrinityLlmFields`). While the daemon cannot answer, the page states the
/// offline condition and offers the retry instead of showing fields whose
/// values nobody knows (honest degrade; web leaves them silently empty).
fn trinity_engine_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> SettingGroup {
    let engine = crate::trinity::TrinityEngineState::current(cx);
    let (loaded, loading, failure) = (engine.loaded, engine.loading, engine.failure.clone());

    if !loaded {
        let owned_text = text.clone();
        let retry_view = view.clone();
        let item = if let Some(reason) = failure {
            SettingItem::render(move |_options, _window, cx| {
                let mut column = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .w_full()
                    .debug_selector(|| "trinity-engine-offline".to_string())
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(owned_text.trinity_status_unavailable.clone()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(reason.clone()),
                    );
                let retry = retry_view.clone();
                column = column.child(
                    Button::new("trinity-engine-retry")
                        .label(owned_text.settings_archived_refresh.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, _window, cx| {
                            if let Some(shell) = retry.upgrade() {
                                shell.update(cx, |this, cx| this.reload_trinity_engine(cx));
                            }
                        }),
                );
                column.into_any_element()
            })
        } else {
            let _ = loading;
            let owned_text = text.clone();
            SettingItem::render(move |_options, _window, cx| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(owned_text.settings_archived_loading.clone())
                    .into_any_element()
            })
        };
        return SettingGroup::new().items(vec![item]);
    }

    // Provider presets (web `PROVIDER_PRESETS`): a pill row that fills the
    // endpoint and model; the active pill is the exact-endpoint-and-model
    // match ("custom" when none matches).
    let preset_view = view.clone();
    let provider_field = SettingField::render(move |_options, _window, cx| {
        let text = crate::i18n::current(cx);
        let engine = crate::trinity::TrinityEngineState::current(cx);
        let (api_url, model) = (engine.api_url, engine.model);
        let active = crate::trinity::detect_llm_preset(&api_url, &model);
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_2()
            .w_full()
            .debug_selector(|| "trinity-engine-presets".to_string());
        for (id, url, preset_model) in crate::trinity::LLM_ENGINE_PRESETS {
            let is_active = *id == active;
            let label = match *id {
                "deepseek" => text.trinity_engine_provider_deepseek.clone(),
                "zhipu" => text.trinity_engine_provider_zhipu.clone(),
                "openai" => text.trinity_engine_provider_openai.clone(),
                "ollama" => text.trinity_engine_provider_ollama.clone(),
                _ => text.trinity_engine_provider_custom.clone(),
            };
            let click_view = preset_view.clone();
            let (url, preset_model) = (*url, *preset_model);
            row = row.child(
                Button::new(SharedString::from(format!("trinity-engine-preset-{id}")))
                    .label(label)
                    .small()
                    .with_variant(if is_active {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    })
                    .on_click(move |_, _window, cx| {
                        if let Some(shell) = click_view.upgrade() {
                            shell.update(cx, |this, cx| {
                                this.apply_trinity_engine_preset(url, preset_model, cx);
                            });
                        }
                    }),
            );
        }
        row.into_any_element()
    });

    let set_view = view.clone();
    let api_url_field = SettingField::input(
        move |cx: &App| SharedString::from(crate::trinity::TrinityEngineState::current(cx).api_url),
        move |value: SharedString, cx: &mut App| {
            if let Some(shell) = set_view.upgrade() {
                shell.update(cx, |this, cx| {
                    this.set_trinity_engine_api_url(value.to_string(), cx);
                });
            }
        },
    );

    let set_view = view.clone();
    let model_field = SettingField::input(
        move |cx: &App| SharedString::from(crate::trinity::TrinityEngineState::current(cx).model),
        move |value: SharedString, cx: &mut App| {
            if let Some(shell) = set_view.upgrade() {
                shell.update(cx, |this, cx| {
                    this.set_trinity_engine_model(value.to_string(), cx);
                });
            }
        },
    );

    // The api key renders masked (web `type="password"`): the daemon hands
    // the stored key back on load, and a stray screenshot must not leak it.
    // `SettingField::input` has no mask, so this is the string field's own
    // shape plus `set_masked` at creation.
    let key_view = view.clone();
    let api_key_field = SettingField::render(move |options, window, cx| {
        let value = SharedString::from(crate::trinity::TrinityEngineState::current(cx).api_key);
        let state_entity =
            window.use_keyed_state(SharedString::from("trinity-engine-api-key-state"), cx, {
                let value = value.clone();
                let key_view = key_view.clone();
                move |window, cx| {
                    let input = cx.new(|cx| {
                        let mut state = InputState::new(window, cx).default_value(value);
                        state.set_masked(true, window, cx);
                        state
                    });
                    let subscription = cx.subscribe(&input, {
                        let view = key_view.clone();
                        move |_, input, event: &gpui_kit::component::input::InputEvent, cx| {
                            if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                                let value = input.read(cx).value().to_string();
                                if let Some(shell) = view.upgrade() {
                                    shell.update(cx, |this, cx| {
                                        this.set_trinity_engine_api_key(value, cx);
                                    });
                                }
                            }
                        }
                    });
                    (input, subscription)
                }
            });
        state_entity.update(cx, |(input, _subscription), cx| {
            if input.read(cx).value() != value {
                input.update(cx, |input, cx| {
                    input.set_value(value.clone(), window, cx);
                });
            }
        });
        let (input, _subscription) = state_entity.read(cx);
        Input::new(input)
            .with_size(options.size())
            .map(|this| {
                if matches!(options.layout(), Axis::Horizontal) {
                    this.w_64()
                } else {
                    this.w_full()
                }
            })
            .into_any_element()
    });

    // Test + save, with their verdicts inline (the web toasts the save; this
    // surface has no toast system — the receipt stays until the next action).
    let action_view = view.clone();
    let actions_item = SettingItem::render(move |_options, _window, cx| {
        let text = crate::i18n::current(cx);
        let engine = crate::trinity::TrinityEngineState::current(cx);
        let (testing, saving, draft_ready, test_result, notice) = (
            engine.testing,
            engine.saving,
            engine.draft_ready(),
            engine.test_result.clone(),
            engine.notice.clone(),
        );

        let mut row = div().flex().flex_row().items_center().gap_2().w_full();
        let click_view = action_view.clone();
        row = row.child(
            Button::new("trinity-engine-test")
                .debug_selector(|| "trinity-engine-test".to_string())
                .label(if testing {
                    text.trinity_engine_testing.clone()
                } else {
                    text.trinity_engine_test_connection.clone()
                })
                .small()
                .with_variant(ButtonVariant::Secondary)
                .disabled(testing || saving || !draft_ready)
                .on_click(move |_, _window, cx| {
                    if let Some(shell) = click_view.upgrade() {
                        shell.update(cx, |this, cx| this.test_trinity_engine(cx));
                    }
                }),
        );
        let click_view = action_view.clone();
        row = row.child(
            Button::new("trinity-engine-save")
                .debug_selector(|| "trinity-engine-save".to_string())
                .label(if saving {
                    text.trinity_engine_saving.clone()
                } else {
                    text.trinity_engine_save.clone()
                })
                .small()
                .with_variant(ButtonVariant::Primary)
                .disabled(saving || testing || !draft_ready)
                .on_click(move |_, _window, cx| {
                    if let Some(shell) = click_view.upgrade() {
                        shell.update(cx, |this, cx| this.save_trinity_engine(cx));
                    }
                }),
        );
        if let Some((ok, detail)) = test_result {
            row = row.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if ok {
                        cx.theme().foreground
                    } else {
                        cx.theme().danger
                    })
                    .child(detail),
            );
        }
        let mut column = div().flex().flex_col().gap_2().w_full().child(row);
        if let Some((ok, message)) = notice {
            column = column.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if ok {
                        cx.theme().muted_foreground
                    } else {
                        cx.theme().danger
                    })
                    .child(message),
            );
        }
        column.into_any_element()
    });

    SettingGroup::new().items(vec![
        SettingItem::new(text.trinity_engine_provider.clone(), provider_field)
            .layout(Axis::Vertical),
        SettingItem::new(text.trinity_engine_api_url.clone(), api_url_field),
        SettingItem::new(text.trinity_engine_model.clone(), model_field),
        SettingItem::new(text.trinity_engine_api_key.clone(), api_key_field),
        actions_item,
    ])
}

/// G-SE17: one editable row of the global permission rules. The id keys the
/// row's input states, so reordering or deleting never recycles a text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionRuleDraft {
    pub(crate) id: u64,
    pub(crate) action: String,
    pub(crate) resource: String,
    /// `allow` | `ask` | `deny` (the shared snake_case contract).
    pub(crate) effect: String,
}

/// G-SE17: the in-page editor state. `active = false` shows the stored rules
/// read-only; entering edit mode snapshots them into the draft and nothing is
/// persisted until Save.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PermissionRulesDraft {
    pub(crate) active: bool,
    pub(crate) rules: Vec<PermissionRuleDraft>,
    pub(crate) next_id: u64,
}

impl Global for PermissionRulesDraft {}

impl PermissionRulesDraft {
    pub(crate) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    pub(crate) fn mutate<R>(cx: &mut App, change: impl FnOnce(&mut Self) -> R) -> R {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        change(cx.global_mut::<Self>())
    }
}

/// The shared serialized effect id.
pub(crate) fn permission_effect_value(
    effect: openbitfun_runtime_ports::PermissionEffect,
) -> &'static str {
    use openbitfun_runtime_ports::PermissionEffect;
    match effect {
        PermissionEffect::Allow => "allow",
        PermissionEffect::Ask => "ask",
        PermissionEffect::Deny => "deny",
    }
}

/// Parses a stored/selected effect id; an unknown value falls back to `ask`
/// (the conservative interactive default) instead of dropping the rule.
pub(crate) fn permission_effect_from_value(
    value: &str,
) -> openbitfun_runtime_ports::PermissionEffect {
    use openbitfun_runtime_ports::PermissionEffect;
    match value {
        "allow" => PermissionEffect::Allow,
        "deny" => PermissionEffect::Deny,
        _ => PermissionEffect::Ask,
    }
}

fn permission_effect_label(value: &str, text: &ShellText) -> String {
    match value {
        "allow" => text.settings_permission_effect_allow.clone(),
        "deny" => text.settings_permission_effect_deny.clone(),
        _ => text.settings_permission_effect_ask.clone(),
    }
}

/// Snapshots the stored rules into the editor.
fn start_permission_rules_draft(cx: &mut App) {
    let stored = ShellConfig::current(cx).permission_rules;
    PermissionRulesDraft::mutate(cx, |draft| {
        draft.rules = stored
            .iter()
            .enumerate()
            .map(|(index, rule)| PermissionRuleDraft {
                id: index as u64,
                action: rule.action.clone(),
                resource: rule.resource.clone(),
                effect: permission_effect_value(rule.effect).to_string(),
            })
            .collect();
        draft.next_id = stored.len() as u64;
        draft.active = true;
    });
    cx.refresh_windows();
}

/// Writes the draft back as the ordered rule array. Incomplete rows are
/// refused (the Save control is disabled for them too).
fn save_permission_rules(cx: &mut App) {
    let draft = PermissionRulesDraft::current(cx);
    if draft
        .rules
        .iter()
        .any(|rule| rule.action.trim().is_empty() || rule.resource.trim().is_empty())
    {
        return;
    }
    let rules: Vec<openbitfun_runtime_ports::PermissionRule> = draft
        .rules
        .iter()
        .map(|rule| {
            openbitfun_runtime_ports::PermissionRule::new(
                rule.action.trim(),
                rule.resource.trim(),
                permission_effect_from_value(&rule.effect),
            )
        })
        .collect();
    cx.global_mut::<ShellConfig>().permission_rules = rules.clone();
    persist_object_key(
        PERMISSION_POLICY_PATH,
        "rules",
        serde_json::to_value(&rules).unwrap_or(serde_json::json!([])),
    );
    PermissionRulesDraft::mutate(cx, |draft| draft.active = false);
    cx.refresh_windows();
}

fn permission_rules_readonly_item(text: &ShellText, _cx: &App) -> SettingItem {
    let text = text.clone();
    SettingItem::render(move |_options, _window, cx| {
        let stored = ShellConfig::current(cx).permission_rules.clone();
        let mut column = div().v_flex().gap_1().w_full();
        if stored.is_empty() {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.settings_permission_rules_empty.clone()),
            );
        } else {
            for rule in &stored {
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
                                .child(format!("{} · {}", rule.action, rule.resource)),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(permission_effect_label(
                                    permission_effect_value(rule.effect),
                                    &text,
                                )),
                        ),
                );
            }
        }
        column = column.child(
            Button::new("permission-rules-manage")
                .label(text.settings_permission_rules_manage.clone())
                .small()
                .outline()
                .on_click(move |_, _window, cx| start_permission_rules_draft(cx)),
        );
        column.into_any_element()
    })
}

/// One editor row: action input, resource input and the effect menu.
fn permission_rule_item(rule_id: u64, index: usize, count: usize, text: &ShellText) -> SettingItem {
    let row_text = text.clone();
    SettingItem::render(move |_options, window, cx| {
        let draft = PermissionRulesDraft::current(cx);
        let Some(rule) = draft.rules.iter().find(|rule| rule.id == rule_id).cloned() else {
            return div().into_any_element();
        };
        let action = permission_rule_input(
            rule_id,
            "action",
            &rule.action,
            &row_text.settings_permission_rules_action,
            window,
            cx,
        );
        let resource = permission_rule_input(
            rule_id,
            "resource",
            &rule.resource,
            &row_text.settings_permission_rules_resource,
            window,
            cx,
        );

        let effect_value = rule.effect.clone();
        let effect_text = row_text.clone();
        let effect_button = Button::new(SharedString::from(format!(
            "permission-rule-{rule_id}-effect"
        )))
        .label(permission_effect_label(&rule.effect, &row_text))
        .tooltip(row_text.settings_permission_rules_effect.clone())
        .small()
        .outline()
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
            let mut menu = menu;
            for effect in ["allow", "ask", "deny"] {
                let selected = effect == effect_value;
                let label = permission_effect_label(effect, &effect_text);
                let value = effect.to_string();
                menu = menu.item(PopupMenuItem::new(label).checked(selected).on_click(
                    move |_, _window, cx| {
                        let value = value.clone();
                        PermissionRulesDraft::mutate(cx, |draft| {
                            if let Some(rule) =
                                draft.rules.iter_mut().find(|rule| rule.id == rule_id)
                            {
                                rule.effect = value;
                            }
                        });
                        cx.refresh_windows();
                    },
                ));
            }
            menu
        });

        let up = Button::new(SharedString::from(format!("permission-rule-{rule_id}-up")))
            .label("↑")
            .small()
            .with_variant(ButtonVariant::Ghost)
            .disabled(index == 0)
            .on_click(move |_, _window, cx| {
                PermissionRulesDraft::mutate(cx, |draft| {
                    if index > 0 {
                        draft.rules.swap(index - 1, index);
                    }
                });
                cx.refresh_windows();
            });
        let down = Button::new(SharedString::from(format!(
            "permission-rule-{rule_id}-down"
        )))
        .label("↓")
        .small()
        .with_variant(ButtonVariant::Ghost)
        .disabled(index + 1 >= count)
        .on_click(move |_, _window, cx| {
            PermissionRulesDraft::mutate(cx, |draft| {
                if index + 1 < draft.rules.len() {
                    draft.rules.swap(index, index + 1);
                }
            });
            cx.refresh_windows();
        });
        let remove = Button::new(SharedString::from(format!(
            "permission-rule-{rule_id}-remove"
        )))
        .label("×")
        .small()
        .with_variant(ButtonVariant::Ghost)
        .on_click(move |_, _window, cx| {
            PermissionRulesDraft::mutate(cx, |draft| {
                draft.rules.retain(|rule| rule.id != rule_id);
            });
            cx.refresh_windows();
        });

        div()
            .flex()
            .items_center()
            .gap_2()
            .w_full()
            .child(div().w(px(120.)).child(action))
            .child(div().flex_1().min_w_0().child(resource))
            .child(effect_button)
            .child(up)
            .child(down)
            .child(remove)
            .into_any_element()
    })
}

/// One text input of an editor row, keyed by the stable row id so the text
/// follows the row through reorders.
fn permission_rule_input(
    rule_id: u64,
    field: &'static str,
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let key = SharedString::from(format!("permission-rule-{rule_id}-{field}"));
    let initial = value.to_string();
    let placeholder = placeholder.to_string();
    let state = window.use_keyed_state(key, cx, move |window, cx| {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial.clone())
                .placeholder(placeholder.clone())
        });
        let subscription = cx.subscribe(
            &input,
            move |_, input, event: &gpui_kit::component::input::InputEvent, cx| {
                if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                    let value = input.read(cx).value().to_string();
                    PermissionRulesDraft::mutate(cx, |draft| {
                        if let Some(rule) = draft.rules.iter_mut().find(|rule| rule.id == rule_id) {
                            if field == "action" {
                                rule.action = value.clone();
                            } else {
                                rule.resource = value.clone();
                            }
                        }
                    });
                    cx.refresh_windows();
                }
            },
        );
        (input, subscription)
    });
    let value = SharedString::from(value.to_string());
    state.update(cx, |(input, _subscription), cx| {
        if input.read(cx).value() != value {
            input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
    });
    Input::new(&state.read(cx).0).small().into_any_element()
}

/// Add / Save / Discard under the editor rows.
fn permission_rules_actions_item(text: &ShellText) -> SettingItem {
    let text = text.clone();
    SettingItem::render(move |_options, _window, cx| {
        let draft = PermissionRulesDraft::current(cx);
        let valid = draft
            .rules
            .iter()
            .all(|rule| !rule.action.trim().is_empty() && !rule.resource.trim().is_empty());
        let mut row = div().flex().items_center().gap_2().w_full();
        if !valid {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(text.settings_permission_rules_invalid.clone()),
            );
        }
        row = row
            .child(
                Button::new("permission-rules-add")
                    .label(text.settings_permission_rules_add.clone())
                    .small()
                    .outline()
                    .on_click(move |_, _window, cx| {
                        PermissionRulesDraft::mutate(cx, |draft| {
                            let id = draft.next_id;
                            draft.next_id += 1;
                            draft.rules.push(PermissionRuleDraft {
                                id,
                                action: "*".to_string(),
                                resource: String::new(),
                                effect: "ask".to_string(),
                            });
                        });
                        cx.refresh_windows();
                    }),
            )
            .child(
                Button::new("permission-rules-save")
                    .label(text.settings_permission_rules_save.clone())
                    .small()
                    .with_variant(ButtonVariant::Primary)
                    .disabled(!valid)
                    .on_click(move |_, _window, cx| save_permission_rules(cx)),
            )
            .child(
                Button::new("permission-rules-discard")
                    .label(text.settings_permission_rules_discard.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, _window, cx| {
                        PermissionRulesDraft::mutate(cx, |draft| draft.active = false);
                        cx.refresh_windows();
                    }),
            );
        row.into_any_element()
    })
}

fn permissions_group(text: &ShellText, cx: &App) -> SettingGroup {
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

    // G-SE18: the two runtime timeouts.
    let tool_timeout_field = runtime_timeout_field(
        TOOL_EXECUTION_TIMEOUT_PATH,
        |config| config.tool_execution_timeout_secs,
        |config, value| config.tool_execution_timeout_secs = value,
        true,
    );
    let question_timeout_field = runtime_timeout_field(
        USER_QUESTION_TIMEOUT_PATH,
        |config| config.user_question_timeout_secs.map(u64::from),
        |config, value| config.user_question_timeout_secs = value.map(|secs| secs as u32),
        false,
    );

    // G-SE19: default subagent model, batch policy, and the two concurrency
    // limits (regular subagents and the swarm).
    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();
    let current_model = subagent_model_choice(&ShellConfig::current(cx).subagent_default_model);
    let subagent_model_field = SettingField::dropdown(
        subagent_model_options(&catalog.models, &current_model, text),
        |cx: &App| {
            SharedString::from(subagent_model_choice(
                &ShellConfig::current(cx).subagent_default_model,
            ))
        },
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if value.is_empty() {
                return;
            }
            let selection = subagent_model_selection(&value);
            cx.global_mut::<ShellConfig>().subagent_default_model = selection.clone();
            match serde_json::to_value(&selection) {
                Ok(document) => persist(SUBAGENT_DEFAULT_MODEL_PATH, document),
                Err(error) => tracing::error!("Failed to serialize the subagent model: {error}"),
            }
            cx.refresh_windows();
        },
    );

    let current_policy = ShellConfig::current(cx).subagent_batch_policy.clone();
    let batch_field = SettingField::dropdown(
        subagent_batch_options(&current_policy, text),
        |cx: &App| SharedString::from(ShellConfig::current(cx).subagent_batch_policy.clone()),
        |value: SharedString, cx: &mut App| {
            let value = value.to_string();
            if !SUBAGENT_BATCH_POLICIES.contains(&value.as_str()) {
                tracing::warn!("Ignoring unsupported subagent batch policy: {value}");
                return;
            }
            cx.global_mut::<ShellConfig>().subagent_batch_policy = value.clone();
            persist(SUBAGENT_BATCH_POLICY_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    let subagent_concurrency_field = concurrency_field(
        MAX_SUBAGENT_CONCURRENCY,
        |config| config.subagent_max_concurrency,
        |config, value| config.subagent_max_concurrency = value,
        SUBAGENT_MAX_CONCURRENCY_PATH,
    );
    let swarm_concurrency_field = concurrency_field(
        MAX_SWARM_CONCURRENCY,
        |config| config.swarm_max_concurrency,
        |config, value| config.swarm_max_concurrency = value,
        SWARM_MAX_CONCURRENCY_PATH,
    );

    // The row description follows the stored policy, matching the web page's
    // per-option helper text.
    let batch_description =
        if ShellConfig::current(cx).subagent_batch_policy == SUBAGENT_BATCH_POLICIES[1] {
            text.settings_subagent_batch_parallel_description.clone()
        } else {
            text.settings_subagent_batch_safe_description.clone()
        };

    // G-SE20/G-SE21: the two tool-runtime switches.
    let deferred_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).deferred_tool_loading,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().deferred_tool_loading = value;
            persist(DEFERRED_TOOL_LOADING_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );
    let json_repair_field = SettingField::switch(
        |cx: &App| ShellConfig::current(cx).allow_tool_json_repair,
        |value: bool, cx: &mut App| {
            cx.global_mut::<ShellConfig>().allow_tool_json_repair = value;
            persist(TOOL_JSON_REPAIR_PATH, serde_json::json!(value));
            cx.refresh_windows();
        },
    );

    // G-SE22: the two review-capacity keys.
    let review_parallelism_field = review_capacity_field(
        "max_parallel_reviewers",
        MIN_REVIEW_PARALLELISM,
        MAX_REVIEW_PARALLELISM,
        1.,
        DEFAULT_REVIEW_PARALLELISM,
    );
    let review_queue_field = review_capacity_field(
        "max_queue_wait_seconds",
        0,
        MAX_REVIEW_QUEUE_WAIT_SECS,
        60.,
        DEFAULT_REVIEW_QUEUE_WAIT_SECS,
    );

    let deferred_warning = (!ShellConfig::current(cx).deferred_tool_loading)
        .then(|| text.settings_deferred_tools_warning.clone());
    let mut deferred_item =
        SettingItem::new(text.settings_deferred_tools_enable.clone(), deferred_field);
    if let Some(warning) = deferred_warning {
        deferred_item = deferred_item.description(warning);
    }

    // G-SE17: the global rule list / editor sits between the two policy rows
    // and the execution section, matching the web page's placement.
    let mut items = vec![
        SettingItem::new(text.settings_policy.clone(), preset_field)
            .description(text.settings_policy_description.clone()),
        SettingItem::new(text.settings_auto_approve.clone(), auto_approve_field)
            .description(text.settings_auto_approve_description.clone())
            .disabled(auto_approve_disabled),
        section_heading_item(
            "permission-rules",
            text.settings_permission_rules_section.clone(),
            text.settings_permission_rules_description.clone(),
        ),
    ];
    let draft = PermissionRulesDraft::current(cx);
    if draft.active {
        let count = draft.rules.len();
        for (index, rule) in draft.rules.iter().enumerate() {
            items.push(permission_rule_item(rule.id, index, count, text));
        }
        items.push(permission_rules_actions_item(text));
    } else {
        items.push(permission_rules_readonly_item(text, cx));
    }
    items.extend(vec![
        section_heading_item(
            "execution",
            text.settings_execution_section.clone(),
            text.settings_execution_section_note.clone(),
        ),
        SettingItem::new(text.settings_tool_timeout.clone(), tool_timeout_field)
            .description(text.settings_tool_timeout_description.clone()),
        SettingItem::new(
            text.settings_question_timeout.clone(),
            question_timeout_field,
        )
        .description(text.settings_question_timeout_description.clone()),
        section_heading_item(
            "subagents",
            text.settings_subagents_section.clone(),
            text.settings_subagents_section_note.clone(),
        ),
        SettingItem::new(text.settings_subagent_model.clone(), subagent_model_field)
            .description(text.settings_subagent_model_description.clone()),
        SettingItem::new(text.settings_subagent_batch.clone(), batch_field)
            .description(batch_description),
        SettingItem::new(
            text.settings_subagent_concurrency.clone(),
            subagent_concurrency_field,
        )
        .description(text.settings_subagent_concurrency_description.clone()),
        SettingItem::new(
            text.settings_swarm_concurrency.clone(),
            swarm_concurrency_field,
        )
        .description(text.settings_swarm_concurrency_description.clone()),
        section_heading_item(
            "deferred-tools",
            text.settings_deferred_tools_section.clone(),
            text.settings_deferred_tools_section_note.clone(),
        ),
        deferred_item,
        section_heading_item(
            "json-repair",
            text.settings_json_repair_section.clone(),
            text.settings_json_repair_section_note.clone(),
        ),
        SettingItem::new(text.settings_json_repair.clone(), json_repair_field)
            .description(text.settings_json_repair_description.clone()),
        section_heading_item(
            "review-capacity",
            text.settings_review_capacity_section.clone(),
            text.settings_review_capacity_section_note.clone(),
        ),
        SettingItem::new(
            text.settings_review_parallelism.clone(),
            review_parallelism_field,
        )
        .description(text.settings_review_parallelism_description.clone()),
        SettingItem::new(text.settings_review_queue_wait.clone(), review_queue_field)
            .description(text.settings_review_queue_wait_description.clone()),
    ]);
    SettingGroup::new().items(items)
}

/// G-SE24: the quick-actions editor state. The draft is page-local: nothing
/// is written until Save, matching the web modal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct QuickActionsEditorState {
    /// Position in the stored array being edited.
    pub(crate) index: Option<usize>,
    pub(crate) label: String,
    pub(crate) prompt: String,
    /// Two-stage confirmation for a custom action's delete.
    pub(crate) confirm_delete: Option<usize>,
}

impl Global for QuickActionsEditorState {}

impl QuickActionsEditorState {
    fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    fn mutate<R>(cx: &mut App, change: impl FnOnce(&mut Self) -> R) -> R {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        change(cx.global_mut::<Self>())
    }
}

/// The canonical English text of a built-in action, when this is one.
fn quick_action_builtin_default(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "commit" => Some(crate::shell::QUICK_ACTION_COMMIT_DEFAULTS),
        "create_pr" => Some(crate::shell::QUICK_ACTION_CREATE_PR_DEFAULTS),
        _ => None,
    }
}

/// Writes the whole array (the shared key is a list, not an object) and
/// updates the composer menu's mirror.
fn persist_quick_actions(
    actions: Vec<openbitfun_core::service::config::types::AiExperienceQuickAction>,
    cx: &mut App,
) {
    cx.global_mut::<ShellConfig>().quick_actions = actions.clone();
    persist(
        QUICK_ACTIONS_PATH,
        serde_json::to_value(&actions).unwrap_or_else(|_| serde_json::json!([])),
    );
    cx.refresh_windows();
}

fn start_quick_action_edit(index: usize, cx: &mut App) {
    let Some(action) = ShellConfig::current(cx).quick_actions.get(index).cloned() else {
        return;
    };
    let text = crate::i18n::current(cx);
    let (label, prompt) = crate::shell::resolve_quick_action_text(&action, &text);
    QuickActionsEditorState::mutate(cx, |state| {
        state.index = Some(index);
        state.label = label;
        state.prompt = prompt;
    });
    cx.refresh_windows();
}

/// The stored-vs-localized normalization of one quick action (web
/// `normalizeQuickActionTextForStorage`): a built-in row that still carries
/// its canonical English default and was not edited away from the localized
/// text stores the default again; everything else keeps the typed text.
pub(crate) fn normalized_quick_action_text(
    id: &str,
    stored_label: &str,
    stored_prompt: &str,
    typed_label: &str,
    typed_prompt: &str,
    localized_label: &str,
    localized_prompt: &str,
) -> (String, String) {
    let Some((default_label, default_prompt)) = quick_action_builtin_default(id) else {
        return (typed_label.to_string(), typed_prompt.to_string());
    };
    let label = if stored_label == default_label && typed_label == localized_label {
        default_label.to_string()
    } else {
        typed_label.to_string()
    };
    let prompt = if stored_prompt == default_prompt && typed_prompt == localized_prompt {
        default_prompt.to_string()
    } else {
        typed_prompt.to_string()
    };
    (label, prompt)
}

fn save_quick_action_edit(cx: &mut App) {
    let state = QuickActionsEditorState::current(cx);
    let Some(index) = state.index else {
        return;
    };
    let typed_label = state.label.trim().to_string();
    let typed_prompt = state.prompt.trim().to_string();
    if typed_label.is_empty() || typed_prompt.is_empty() {
        return;
    }
    let mut actions = ShellConfig::current(cx).quick_actions;
    let Some(action) = actions.get_mut(index) else {
        return;
    };
    let text = crate::i18n::current(cx);
    let (localized_label, localized_prompt) =
        crate::shell::resolve_quick_action_text(action, &text);
    let (label, prompt) = normalized_quick_action_text(
        &action.id,
        &action.label,
        &action.prompt,
        &typed_label,
        &typed_prompt,
        &localized_label,
        &localized_prompt,
    );
    action.label = label;
    action.prompt = prompt;
    persist_quick_actions(actions, cx);
    QuickActionsEditorState::mutate(cx, |state| state.index = None);
    cx.refresh_windows();
}

fn add_quick_action(cx: &mut App) {
    let mut actions = ShellConfig::current(cx).quick_actions;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    actions.push(
        openbitfun_core::service::config::types::AiExperienceQuickAction {
            id: format!("custom_{now}"),
            label: String::new(),
            prompt: String::new(),
            enabled: true,
        },
    );
    let index = actions.len() - 1;
    persist_quick_actions(actions, cx);
    QuickActionsEditorState::mutate(cx, |state| {
        state.index = Some(index);
        state.label.clear();
        state.prompt.clear();
    });
    cx.refresh_windows();
}

fn delete_quick_action(index: usize, cx: &mut App) {
    let mut actions = ShellConfig::current(cx).quick_actions;
    let Some(action) = actions.get(index) else {
        return;
    };
    if quick_action_builtin_default(&action.id).is_some() {
        return;
    }
    actions.remove(index);
    persist_quick_actions(actions, cx);
    QuickActionsEditorState::mutate(cx, |state| {
        state.confirm_delete = None;
        if state.index == Some(index) {
            state.index = None;
        }
    });
}

fn toggle_quick_action(index: usize, enabled: bool, cx: &mut App) {
    let mut actions = ShellConfig::current(cx).quick_actions;
    if let Some(action) = actions.get_mut(index) {
        action.enabled = enabled;
    }
    persist_quick_actions(actions, cx);
}

/// G-SE24: the automation page's quick-actions section — built-in rows
/// (editable, not deletable), custom rows (editable and deletable) and the
/// inline editor.
fn quick_actions_items(text: &ShellText, cx: &App) -> Vec<SettingItem> {
    let actions = ShellConfig::current(cx).quick_actions;
    let state = QuickActionsEditorState::current(cx);
    let mut items = vec![section_heading_item(
        "quick-actions-builtin",
        text.settings_quick_actions_builtin_section.clone(),
        text.settings_quick_actions_builtin_note.clone(),
    )];

    let mut custom_indices = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let builtin = quick_action_builtin_default(&action.id).is_some();
        if !builtin {
            custom_indices.push(index);
        }
        let (label, prompt) = crate::shell::resolve_quick_action_text(action, text);
        let toggle = SettingField::switch(
            move |cx: &App| {
                ShellConfig::current(cx)
                    .quick_actions
                    .get(index)
                    .map(|action| action.enabled)
                    .unwrap_or(false)
            },
            move |value: bool, cx: &mut App| toggle_quick_action(index, value, cx),
        );
        items.push(SettingItem::new(label, toggle).description(prompt));
    }

    items.push(section_heading_item(
        "quick-actions-custom",
        text.settings_quick_actions_custom_section.clone(),
        text.settings_quick_actions_custom_note.clone(),
    ));
    if custom_indices.is_empty() {
        items.push(SettingItem::render({
            let empty = text.settings_quick_actions_custom_empty.clone();
            move |_options, _window, cx| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(empty.clone())
                    .into_any_element()
            }
        }));
    }
    for &index in &custom_indices {
        let Some(action) = actions.get(index) else {
            continue;
        };
        let (label, _prompt) = crate::shell::resolve_quick_action_text(action, text);
        let row_text = text.clone();
        let confirming = state.confirm_delete == Some(index);
        items.push(SettingItem::render(move |_options, _window, cx| {
            let edit = Button::new(SharedString::from(format!("quick-action-{index}-edit")))
                .label(crate::i18n::shared_term("tools.edit", cx))
                .small()
                .outline()
                .on_click(move |_, _window, cx| start_quick_action_edit(index, cx));
            let delete_text = if confirming {
                row_text.settings_quick_actions_delete_confirm.clone()
            } else {
                row_text.settings_quick_actions_delete.clone()
            };
            let delete = Button::new(SharedString::from(format!("quick-action-{index}-delete")))
                .label(delete_text)
                .small()
                .with_variant(ButtonVariant::Danger)
                .on_click(move |_, _window, cx| {
                    let already =
                        QuickActionsEditorState::current(cx).confirm_delete == Some(index);
                    if already {
                        delete_quick_action(index, cx);
                    } else {
                        QuickActionsEditorState::mutate(cx, |state| {
                            state.confirm_delete = Some(index);
                        });
                        cx.refresh_windows();
                    }
                });
            div()
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(label.clone()),
                )
                .child(edit)
                .child(delete)
                .into_any_element()
        }));
    }
    items.push(SettingItem::render({
        let add_text = text.settings_quick_actions_add.clone();
        move |_options, _window, _cx| {
            Button::new("quick-action-add")
                .label(add_text.clone())
                .small()
                .outline()
                .on_click(move |_, _window, cx| add_quick_action(cx))
                .into_any_element()
        }
    }));

    if state.index.is_some() {
        let label_text = text.settings_quick_actions_label.clone();
        let prompt_text = text.settings_quick_actions_prompt.clone();
        let invalid_text = text.settings_quick_actions_invalid.clone();
        let label_field = SettingField::input(
            |cx: &App| SharedString::from(QuickActionsEditorState::current(cx).label),
            |value: SharedString, cx: &mut App| {
                QuickActionsEditorState::mutate(cx, |state| state.label = value.to_string());
                cx.refresh_windows();
            },
        );
        let prompt_field = SettingField::input(
            |cx: &App| SharedString::from(QuickActionsEditorState::current(cx).prompt),
            |value: SharedString, cx: &mut App| {
                QuickActionsEditorState::mutate(cx, |state| state.prompt = value.to_string());
                cx.refresh_windows();
            },
        );
        items.push(SettingItem::new(label_text, label_field).layout(Axis::Vertical));
        items.push(SettingItem::new(prompt_text, prompt_field).layout(Axis::Vertical));
        let save_text = text.settings_quick_actions_save.clone();
        let cancel_text = text.settings_quick_actions_cancel.clone();
        items.push(SettingItem::render(move |_options, _window, cx| {
            let state = QuickActionsEditorState::current(cx);
            let valid = !state.label.trim().is_empty() && !state.prompt.trim().is_empty();
            let mut row = div().flex().items_center().gap_2().w_full();
            if !valid {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(invalid_text.clone()),
                );
            }
            row = row
                .child(
                    Button::new("quick-action-save")
                        .label(save_text.clone())
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .disabled(!valid)
                        .on_click(move |_, _window, cx| save_quick_action_edit(cx)),
                )
                .child(
                    Button::new("quick-action-cancel")
                        .label(cancel_text.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, _window, cx| {
                            QuickActionsEditorState::mutate(cx, |state| state.index = None);
                            cx.refresh_windows();
                        }),
                );
            row.into_any_element()
        }));
    }

    items
}

/// Automation group (M22): the native-hooks master switches and the commit
/// co-author preference. The toggles are the same shared values the desktop
/// automation page writes; hook declarations themselves stay in hooks.json.
fn automation_group(text: &ShellText, cx: &App) -> SettingGroup {
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

    let mut items = vec![
        SettingItem::new(text.settings_hooks.clone(), hooks_field)
            .description(text.settings_hooks_description.clone()),
        SettingItem::new(text.settings_hooks_project.clone(), project_hooks_field)
            .description(text.settings_hooks_project_description.clone()),
        SettingItem::new(text.settings_git_coauthor.clone(), coauthor_field)
            .description(text.settings_git_coauthor_description.clone()),
        section_heading_item(
            "quick-actions",
            text.settings_quick_actions_section.clone(),
            text.settings_quick_actions_note.clone(),
        ),
    ];
    items.extend(quick_actions_items(text, cx));

    // G-SE25: where hooks are declared, the imported-source manager, and the
    // Codex contract reference (web `HooksConfig`).
    items.push(section_heading_item(
        "hook-locations",
        text.settings_hooks_locations_section.clone(),
        text.settings_hooks_locations_note.clone(),
    ));
    for (label, description) in [
        (
            text.settings_hooks_user_file.clone(),
            text.settings_hooks_user_file_desc.clone(),
        ),
        (
            text.settings_hooks_project_file.clone(),
            text.settings_hooks_project_file_desc.clone(),
        ),
    ] {
        items.push(
            SettingItem::render(move |_options, _window, cx| {
                div()
                    .v_flex()
                    .gap_0p5()
                    .w_full()
                    .child(div().text_sm().child(label.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(description.clone()),
                    )
                    .into_any_element()
            })
            .layout(Axis::Vertical),
        );
    }

    let imports_note = if ShellConfig::current(cx).hooks_enabled {
        text.settings_hooks_imports_note.clone()
    } else {
        text.settings_hooks_imports_master_disabled.clone()
    };
    items.push(section_heading_item(
        "hook-imports",
        text.settings_hooks_imports_section.clone(),
        imports_note,
    ));

    let refresh_label = text.settings_hooks_imports_refresh.clone();
    items.push(
        SettingItem::render(move |_options, _window, cx| {
            let state = HookImportState::current(cx);
            let (status, color) = if let Some(error) = state.error.clone() {
                (error, cx.theme().danger)
            } else if state.loading {
                (
                    crate::i18n::current(cx).settings_hooks_imports_loading,
                    cx.theme().muted_foreground,
                )
            } else {
                (String::new(), cx.theme().muted_foreground)
            };
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(color)
                        .child(status),
                )
                .child(
                    Button::new("hook-imports-refresh")
                        .label(refresh_label.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .loading(state.loading)
                        .on_click(|_, _window, cx| fetch_hook_imports(true, cx)),
                )
                .into_any_element()
        })
        .layout(Axis::Vertical),
    );

    let page = HookImportState::current(cx);
    let snapshot = page.snapshot.clone();
    let imports = snapshot
        .as_ref()
        .map(|snapshot| snapshot.imports.clone())
        .unwrap_or_default();
    let diagnostics = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic
                        .code
                        .starts_with("external_hook.import_store_corrupt.")
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let master_on = ShellConfig::current(cx).hooks_enabled;

    if page.loaded && imports.is_empty() && diagnostics.is_empty() {
        let empty_text = text.settings_hooks_imports_empty.clone();
        items.push(
            SettingItem::render(move |_options, _window, cx| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(empty_text.clone())
                    .into_any_element()
            })
            .layout(Axis::Vertical),
        );
    }

    for import in imports {
        let import_id = import.import_id.clone();
        let name = import.source.display_name.clone();
        let state_label =
            hook_import_state_label(&import.state, &import.source.location_hint, text);
        let enabled = import.enabled;
        let can_update = !matches!(
            import.state,
            openbitfun_product_domains::external_hook_import::ImportedHookSourceStateV1::SourceMissing
        );
        let busy = page.busy || page.loading;
        let armed = page.confirm.as_deref() == Some(&format!("remove:{import_id}"));
        let update_label = text.settings_hooks_imports_update.clone();
        let remove_label = text.settings_hooks_imports_remove.clone();
        let toggle_label = text.settings_hooks_import_toggle.clone();
        let cancel_label = text.settings_quick_actions_cancel.clone();
        let confirm_title = text.settings_hooks_import_remove_title.clone();
        let confirm_note = text.settings_hooks_import_remove_confirm_note.clone();
        let confirm_label = text.settings_hooks_import_remove_confirm.clone();
        let row_id = import_id.clone();
        let click_id = import_id.clone();
        let update_id = import_id.clone();
        let remove_id = import_id.clone();
        let cancel_id = import_id.clone();
        let confirm_id = import_id.clone();
        let update_source = import.source.key.clone();
        items.push(
            SettingItem::render(move |_options, _window, cx| {
                let mut column = div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .debug_selector({
                        let id = row_id.clone();
                        move || format!("hook-import-{id}")
                    })
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .w_full()
                            .child(div().min_w_0().truncate().text_sm().child(name.clone()))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Switch::new(SharedString::from(format!(
                                            "hook-import-toggle-{click_id}"
                                        )))
                                        .checked(enabled)
                                        .disabled(busy || !master_on)
                                        .accessibility_label(toggle_label.clone())
                                        .on_click({
                                            let id = click_id.clone();
                                            move |checked: &bool, _window, cx| {
                                                set_hook_import_enabled(id.clone(), *checked, cx);
                                            }
                                        }),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "hook-import-update-{update_id}"
                                        )))
                                        .label(update_label.clone())
                                        .small()
                                        .with_variant(ButtonVariant::Ghost)
                                        .disabled(busy || !can_update)
                                        .on_click({
                                            let source = update_source.clone();
                                            move |_, _window, cx| {
                                                review_hook_import(source.clone(), cx);
                                            }
                                        }),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!(
                                            "hook-import-remove-{remove_id}"
                                        )))
                                        .label(remove_label.clone())
                                        .small()
                                        .with_variant(ButtonVariant::Ghost)
                                        .disabled(busy)
                                        .on_click({
                                            let id = remove_id.clone();
                                            move |_, _window, cx| {
                                                HookImportState::mutate(cx, |state| {
                                                    state.confirm = Some(format!("remove:{id}"));
                                                });
                                                cx.refresh_windows();
                                            }
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(state_label.clone()),
                    );
                if armed {
                    column = column
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(format!("{confirm_title} {confirm_note}")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "hook-import-remove-confirm-{confirm_id}"
                                    )))
                                    .label(confirm_label.clone())
                                    .small()
                                    .with_variant(ButtonVariant::Primary)
                                    .on_click({
                                        let id = confirm_id.clone();
                                        move |_, _window, cx| {
                                            remove_hook_import(id.clone(), cx);
                                        }
                                    }),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "hook-import-remove-cancel-{cancel_id}"
                                    )))
                                    .label(cancel_label.clone())
                                    .small()
                                    .with_variant(ButtonVariant::Ghost)
                                    .on_click(
                                        |_, _window, cx| {
                                            HookImportState::mutate(cx, |state| {
                                                state.confirm = None;
                                            });
                                            cx.refresh_windows();
                                        },
                                    ),
                                ),
                        );
                }
                column.into_any_element()
            })
            .layout(Axis::Vertical),
        );
    }

    for diagnostic in diagnostics {
        let scope = if diagnostic.code.ends_with(".user_global") {
            openbitfun_product_domains::external_sources::ExternalSourceScope::UserGlobal
        } else {
            openbitfun_product_domains::external_sources::ExternalSourceScope::Project
        };
        let message = diagnostic.message.clone();
        let code = diagnostic.code.clone();
        let invalid_text = text.settings_hooks_import_store_invalid.clone();
        let reset_label = text.settings_hooks_import_reset.clone();
        let reset_title = text.settings_hooks_import_reset_title.clone();
        let reset_note = text.settings_hooks_import_reset_note.clone();
        let reset_confirm = text.settings_hooks_import_reset_confirm.clone();
        let cancel_label = text.settings_quick_actions_cancel.clone();
        let armed_key = format!("reset:{code}");
        let armed = page.confirm.as_deref() == Some(armed_key.as_str());
        let arm_code = code.clone();
        let run_code = code.clone();
        let cancel_code = code.clone();
        items.push(
            SettingItem::render(move |_options, _window, cx| {
                let mut column = div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .text_color(cx.theme().danger)
                                    .child(invalid_text.clone()),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "hook-import-reset-{arm_code}"
                                )))
                                .label(reset_label.clone())
                                .small()
                                .with_variant(ButtonVariant::Primary)
                                .on_click({
                                    let key = format!("reset:{arm_code}");
                                    move |_, _window, cx| {
                                        HookImportState::mutate(cx, |state| {
                                            state.confirm = Some(key.clone());
                                        });
                                        cx.refresh_windows();
                                    }
                                }),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(message.clone()),
                    );
                if armed {
                    column = column
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(format!("{reset_title} {reset_note}")),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "hook-import-reset-confirm-{run_code}"
                                    )))
                                    .label(reset_confirm.clone())
                                    .small()
                                    .with_variant(ButtonVariant::Primary)
                                    .on_click(
                                        move |_, _window, cx| {
                                            reset_hook_import_store(scope, cx);
                                        },
                                    ),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "hook-import-reset-cancel-{cancel_code}"
                                    )))
                                    .label(cancel_label.clone())
                                    .small()
                                    .with_variant(ButtonVariant::Ghost)
                                    .on_click(
                                        |_, _window, cx| {
                                            HookImportState::mutate(cx, |state| {
                                                state.confirm = None;
                                            });
                                            cx.refresh_windows();
                                        },
                                    ),
                                ),
                        );
                }
                column.into_any_element()
            })
            .layout(Axis::Vertical),
        );
    }

    if let Some(plan) = page.plan.clone() {
        let is_update = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot.imports.iter().any(|import| {
                    import.source.key.provider_id == plan.source.key.provider_id
                        && import.source.key.source_id == plan.source.key.source_id
                })
            })
            .unwrap_or(false);
        let title = format!(
            "{} · {}",
            text.settings_hooks_import_review_title, plan.source.display_name
        );
        let warning = text.settings_hooks_import_review_warning.clone();
        let stale = page
            .plan_stale
            .then(|| text.settings_hooks_import_stale.clone());
        let timeout_label = text.settings_hooks_import_timeout.clone();
        let skipped_label = text.settings_hooks_import_skipped.clone();
        let handlers = plan.handlers.clone();
        let skipped = plan.skipped.clone();
        let confirm_label = if is_update {
            text.settings_hooks_import_confirm_update.clone()
        } else {
            text.settings_hooks_import_confirm.clone()
        };
        let cancel_label = text.settings_quick_actions_cancel.clone();
        let busy = page.busy;
        items.push(
            SettingItem::render(move |_options, _window, cx| {
                let mut column = div()
                    .v_flex()
                    .gap_1()
                    .w_full()
                    .debug_selector(|| "hook-import-review".to_string())
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(warning.clone()),
                    );
                if let Some(stale) = stale.clone() {
                    column = column.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(stale),
                    );
                }
                for handler in &handlers {
                    let matcher = handler
                        .matcher
                        .as_deref()
                        .map(|matcher| format!(" · {matcher}"))
                        .unwrap_or_default();
                    let mut line = format!("{} · {}{}", handler.event, handler.command, matcher);
                    if let Some(timeout) = handler.timeout_seconds {
                        line.push_str(&format!(" · {timeout_label} {timeout}"));
                    }
                    column = column.child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(line),
                    );
                }
                for skipped in &skipped {
                    column = column.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{skipped_label} {} · {}",
                                skipped.count, skipped.reason_code
                            )),
                    );
                }
                column
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                Button::new("hook-import-review-confirm")
                                    .label(confirm_label.clone())
                                    .small()
                                    .with_variant(ButtonVariant::Primary)
                                    .loading(busy)
                                    .on_click(|_, _window, cx| apply_hook_import_plan(cx)),
                            )
                            .child(
                                Button::new("hook-import-review-cancel")
                                    .label(cancel_label.clone())
                                    .small()
                                    .with_variant(ButtonVariant::Ghost)
                                    .on_click(|_, _window, cx| dismiss_hook_import_plan(cx)),
                            ),
                    )
                    .into_any_element()
            })
            .layout(Axis::Vertical),
        );
    }

    items.push(section_heading_item(
        "hooks-compat",
        text.settings_hooks_compat_section.clone(),
        text.settings_hooks_compat_note.clone(),
    ));
    let reference_label = text.settings_hooks_compat_reference.clone();
    let open_label = text.settings_hooks_compat_open.clone();
    items.push(
        SettingItem::render(move |_options, _window, _cx| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .w_full()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .child(reference_label.clone()),
                )
                .child(
                    Button::new("hooks-compat-open")
                        .label(open_label.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(|_, _window, _cx| {
                            crate::markdown::open_external("https://learn.chatgpt.com/docs/hooks");
                        }),
                )
                .into_any_element()
        })
        .layout(Axis::Vertical),
    );

    SettingGroup::new().items(items)
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

/// Web search group (M22): the active provider plus credential management for
/// the key-based providers. The credential itself goes into the shared
/// encrypted vault through the same core service the server and CLI use.
/// T2-9: while the custom HTTP service is selected, its endpoint/auth rows
/// follow in the same group (web `sections.http`); the group is rebuilt per
/// frame, so the rows follow the provider and header-mode switches without
/// extra state.
fn websearch_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    cx: &mut App,
) -> SettingGroup {
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
    let mut items = vec![
        SettingItem::new(text.settings_websearch_provider.clone(), provider_field)
            .description(text.settings_websearch_provider_description.clone()),
        SettingItem::new(text.websearch_credential.clone(), credential_field)
            .description(text.websearch_credential_description.clone()),
    ];
    if snapshot.web_search_provider == "openbitfun_search_http" {
        items.extend(websearch_http_items(text, view, &snapshot));
    }
    SettingGroup::new().items(items)
}

/// The endpoint/auth rows for the custom HTTP search service: URL, auth
/// mode, and — for the header mode — the credential header name. Every field
/// persists immediately, so a credential saved afterwards always validates
/// against the mode that is already on disk (web saves its draft first for
/// the same reason).
fn websearch_http_items(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    snapshot: &ShellConfig,
) -> Vec<SettingItem> {
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
    items
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
fn usage_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingGroup {
    let owned_text = text.clone();
    let stats_view = view.clone();
    let range_field = SettingField::render(move |_options, window, cx| {
        usage_stats_field(stats_view.clone(), &owned_text, window, cx)
    });
    let activity_view = view;
    let activity_text = text.clone();
    let activity_field = SettingField::render(move |_options, _window, cx| {
        usage_activity_field(activity_view.clone(), &activity_text, cx)
    });

    SettingGroup::new().items(vec![
        SettingItem::new(text.settings_usage_range.clone(), range_field),
        SettingItem::new(text.usage_activity_title.clone(), activity_field)
            .description(text.usage_activity_description.clone()),
    ])
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
                    .child(
                        div()
                            .w(px(8.))
                            .h(px(8.))
                            .rounded(cx.theme().radius_tokens().sm)
                            .bg(cx.theme().info),
                    )
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
                            .rounded(cx.theme().radius_tokens().sm)
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
    window: &mut Window,
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

    // G-SE29: the distribution filter is page state, not a persisted
    // preference — the web resets it when the page mounts.
    let placeholder = text.settings_usage_filter.clone();
    let filter_state = window.use_keyed_state(
        SharedString::from("usage-distribution-filter"),
        cx,
        move |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.clone()));
            let subscription = cx.subscribe(
                &input,
                |_, input, event: &gpui_kit::component::input::InputEvent, cx| {
                    if matches!(event, gpui_kit::component::input::InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        if cx.try_global::<UsageFilter>().is_none() {
                            cx.set_global(UsageFilter::default());
                        }
                        cx.global_mut::<UsageFilter>().0 = value;
                        cx.refresh_windows();
                    }
                },
            );
            (input, subscription)
        },
    );
    let filter_entity = filter_state.read(cx).0.clone();
    let filter = cx
        .try_global::<UsageFilter>()
        .map(|filter| filter.0.trim().to_lowercase())
        .unwrap_or_default();
    column = column.child(div().w(px(260.)).child(Input::new(&filter_entity).small()));

    column = column.child(usage_distribution(
        &text.usage_models,
        &stats.by_model,
        &filter,
        text,
        cx,
    ));
    column = column.child(usage_distribution(
        &text.settings_usage_groups,
        &stats.by_group,
        &filter,
        text,
        cx,
    ));
    column = column.child(usage_distribution(
        &text.settings_usage_endpoints,
        &stats.by_endpoint,
        &filter,
        text,
        cx,
    ));
    column.into_any_element()
}

/// G-SE29: the distribution filter; the value lives only for the open
/// settings window (the web's page-local search state).
#[derive(Debug, Default, Clone)]
pub(crate) struct UsageFilter(pub(crate) String);

impl Global for UsageFilter {}

/// G-SE12: per-model connection-test receipts for the models page. The state
/// is in-memory only — a receipt describes the last probe this process ran,
/// never persisted configuration.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelTestState {
    pub(crate) receipts: std::collections::HashMap<String, ModelTestReceipt>,
}

impl Global for ModelTestState {}

impl ModelTestState {
    /// The current receipts, or the empty set before the first test.
    pub(crate) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    /// Mutates the receipts, materializing the default when no test ran yet.
    pub(crate) fn mutate<R>(cx: &mut App, change: impl FnOnce(&mut Self) -> R) -> R {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        change(cx.global_mut::<Self>())
    }
}

/// One model's latest probe: in flight, or finished with either a receipt or
/// the setup error that kept the test from running.
#[derive(Debug, Clone)]
pub(crate) enum ModelTestReceipt {
    Testing,
    Done(Result<openbitfun_core::util::types::ConnectionTestResult, String>),
}

/// G-SE12: starts a probe for one model unless its previous one is still in
/// flight. The result arrives through the runtime event pump.
fn start_model_test(model_id: String, cx: &mut App) {
    if matches!(
        ModelTestState::current(cx).receipts.get(&model_id),
        Some(ModelTestReceipt::Testing)
    ) {
        return;
    }
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    ModelTestState::mutate(cx, |state| {
        state
            .receipts
            .insert(model_id.clone(), ModelTestReceipt::Testing);
    });
    handle.test_model_connection(model_id);
    cx.refresh_windows();
}

/// Formats one finished probe as wrap-friendly lines: the verdict with its
/// wall-clock time, the localized hint for the runtime's message code, and
/// the raw details (web `runConfigConnectionTest`).
fn model_test_receipt(
    result: &Result<openbitfun_core::util::types::ConnectionTestResult, String>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let (success, mut lines) = match result {
        Ok(receipt) => {
            let mut lines = vec![crate::i18n::model_test_result_label(
                receipt.success,
                receipt.response_time_ms,
                cx,
            )];
            if let Some(code) = receipt.message_code.as_ref() {
                lines.push(crate::i18n::model_test_message_code(code, cx));
            }
            if let Some(details) = receipt
                .error_details
                .as_deref()
                .filter(|details| !details.trim().is_empty())
            {
                lines.push(if receipt.success {
                    details.to_string()
                } else {
                    format!("{}: {details}", text.models_test_details)
                });
            }
            (receipt.success, lines)
        }
        Err(error) => (
            false,
            vec![
                crate::i18n::model_test_error_label(cx),
                format!("{}: {error}", text.models_test_details),
            ],
        ),
    };
    let color = if success {
        cx.theme().success
    } else {
        cx.theme().danger
    };
    let mut column = div()
        .v_flex()
        .gap_0p5()
        .w_full()
        .pl_1()
        .child(div().text_xs().text_color(color).child(lines.remove(0)));
    for line in lines {
        column = column.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(line),
        );
    }
    column.into_any_element()
}

/// G-SE12: the model row currently open in the inline editor. Page-local like
/// the web modal's editing state; opening another row replaces it. The two
/// text drafts let a half-typed JSON field survive a frame without being
/// written to the shared config until it parses.
#[derive(Debug, Default, Clone)]
pub(crate) struct ModelEditorState {
    pub(crate) model_id: Option<String>,
    /// Raw headers/body text while the open row is being edited.
    pub(crate) headers_text: String,
    pub(crate) body_text: String,
    /// Whether the drafts belong to the open row; reset when it opens or
    /// another row replaces it.
    pub(crate) drafts_ready: bool,
}

impl Global for ModelEditorState {}

impl ModelEditorState {
    /// The open row, or `None` before any row was opened.
    pub(crate) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    /// Mutates the page state, materializing the default on first use.
    pub(crate) fn mutate<R>(cx: &mut App, change: impl FnOnce(&mut Self) -> R) -> R {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        change(cx.global_mut::<Self>())
    }
}

/// G-SE25: the automation page's Hook-import projection plus the page-local
/// review/busy state. A global for the same reason the other settings pages
/// keep theirs there: the page is built inside the window render.
#[derive(Debug, Clone, Default)]
pub(crate) struct HookImportState {
    pub(crate) loaded: bool,
    pub(crate) loading: bool,
    pub(crate) busy: bool,
    pub(crate) snapshot:
        Option<openbitfun_product_domains::external_hook_import::ExternalHookImportSnapshotV1>,
    /// The reviewed plan currently shown, when any.
    pub(crate) plan:
        Option<openbitfun_product_domains::external_hook_import::ExternalHookImportPlanV1>,
    /// The plan was rebuilt because the source changed; the page says nothing
    /// was written yet.
    pub(crate) plan_stale: bool,
    /// The last action's failure, shown above the list.
    pub(crate) error: Option<String>,
    /// An armed destructive action: `remove:<import_id>` or `reset:<scope>`.
    /// The second click on the same row runs it.
    pub(crate) confirm: Option<String>,
}

impl Global for HookImportState {}

impl HookImportState {
    /// The current page state, or the empty default before the first load.
    pub(crate) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().cloned().unwrap_or_default()
    }

    /// Mutates the page state, materializing the default on first use.
    pub(crate) fn mutate<R>(cx: &mut App, change: impl FnOnce(&mut Self) -> R) -> R {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }
        change(cx.global_mut::<Self>())
    }
}

/// G-SE25: reads the import projection (Refresh re-probes source updates).
pub(crate) fn fetch_hook_imports(refresh_updates: bool, cx: &mut App) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    let start = HookImportState::mutate(cx, |state| {
        if state.loading || state.busy {
            return false;
        }
        state.loading = true;
        state.error = None;
        true
    });
    if !start {
        return;
    }
    handle.reload_hook_imports(refresh_updates);
    cx.refresh_windows();
}

/// G-SE25: toggles one imported source. Mutations are revision-guarded; a
/// stale revision surfaces as the refreshed snapshot (core re-reads it).
pub(crate) fn set_hook_import_enabled(import_id: String, enabled: bool, cx: &mut App) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    let revision = HookImportState::current(cx)
        .snapshot
        .map(|snapshot| snapshot.revision);
    let Some(revision) = revision else {
        return;
    };
    HookImportState::mutate(cx, |state| {
        if state.busy {
            return;
        }
        state.busy = true;
        state.error = None;
        state.confirm = None;
    });
    handle.mutate_hook_import(
        revision,
        openbitfun_product_domains::external_hook_import::ExternalHookImportMutationV1::SetEnabled {
            import_id,
            enabled,
        },
    );
    cx.refresh_windows();
}

/// G-SE25: removes the managed copy of one imported source (two-step).
pub(crate) fn remove_hook_import(import_id: String, cx: &mut App) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    let revision = HookImportState::current(cx)
        .snapshot
        .map(|snapshot| snapshot.revision);
    let Some(revision) = revision else {
        return;
    };
    HookImportState::mutate(cx, |state| {
        if state.busy {
            return;
        }
        state.busy = true;
        state.error = None;
        state.confirm = None;
    });
    handle.mutate_hook_import(
        revision,
        openbitfun_product_domains::external_hook_import::ExternalHookImportMutationV1::Remove {
            import_id,
        },
    );
    cx.refresh_windows();
}

/// G-SE25: resets one corrupt import store (two-step). Source application
/// files are never touched; only OpenBitFun's managed index is rebuilt.
pub(crate) fn reset_hook_import_store(
    scope: openbitfun_product_domains::external_sources::ExternalSourceScope,
    cx: &mut App,
) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    let revision = HookImportState::current(cx)
        .snapshot
        .map(|snapshot| snapshot.revision);
    let Some(revision) = revision else {
        return;
    };
    HookImportState::mutate(cx, |state| {
        if state.busy {
            return;
        }
        state.busy = true;
        state.error = None;
        state.confirm = None;
    });
    handle.mutate_hook_import(
        revision,
        openbitfun_product_domains::external_hook_import::ExternalHookImportMutationV1::ResetCorruptStore {
            scope,
        },
    );
    cx.refresh_windows();
}

/// G-SE25: prepares the review plan for an existing import or a catalog
/// source's first import.
pub(crate) fn review_hook_import(
    source: openbitfun_product_domains::external_sources::SourceKey,
    cx: &mut App,
) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    HookImportState::mutate(cx, |state| {
        if state.busy || state.loading {
            return;
        }
        state.busy = true;
        state.error = None;
        state.plan = None;
        state.plan_stale = false;
    });
    handle.plan_hook_import(source);
    cx.refresh_windows();
}

/// G-SE25: applies the reviewed plan (nothing is written before this).
pub(crate) fn apply_hook_import_plan(cx: &mut App) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        return;
    };
    let plan = HookImportState::current(cx).plan;
    let Some(plan) = plan else {
        return;
    };
    HookImportState::mutate(cx, |state| {
        if state.busy {
            return;
        }
        state.busy = true;
        state.error = None;
    });
    handle.apply_hook_import(plan.source.key, plan.plan_fingerprint);
    cx.refresh_windows();
}

/// G-SE25: closes the review block without writing.
pub(crate) fn dismiss_hook_import_plan(cx: &mut App) {
    HookImportState::mutate(cx, |state| {
        state.plan = None;
        state.plan_stale = false;
    });
    cx.refresh_windows();
}

/// G-SE25: one import state's row copy (web `imports.state.*`).
fn hook_import_state_label(
    state: &openbitfun_product_domains::external_hook_import::ImportedHookSourceStateV1,
    location: &str,
    text: &ShellText,
) -> String {
    use openbitfun_product_domains::external_hook_import::ImportedHookSourceStateV1 as State;
    match state {
        State::Current => format!("{} · {location}", text.settings_hook_import_state_current),
        State::UpdateAvailable => format!(
            "{} · {location}",
            text.settings_hook_import_state_update_available
        ),
        State::SourceMissing => text.settings_hook_import_state_source_missing.clone(),
        State::UpdateCheckFailed => text.settings_hook_import_state_update_check_failed.clone(),
        State::BundleMissing => text.settings_hook_import_state_bundle_missing.clone(),
    }
}

/// The context window the web editor shows for a model that never stored one
/// (`config.context_window || 300000`).
const MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW: u32 = 300_000;
/// The shared contract's lower bound (`MIN_MODEL_CONTEXT_WINDOW_TOKENS`).
const MODEL_EDITOR_MIN_CONTEXT_WINDOW: u32 =
    openbitfun_core::service::config::types::MIN_MODEL_CONTEXT_WINDOW_TOKENS;

/// Updates the in-memory mirror of one model row. The page reads this mirror,
/// so a change is visible on the next frame; the persisted copy follows
/// through the matching `persist_*` helper.
fn update_ai_model_mirror(
    model_id: &str,
    cx: &mut App,
    change: impl FnOnce(&mut openbitfun_core::service::config::types::AIModelConfig),
) -> bool {
    let config = cx.global_mut::<ShellConfig>();
    match config
        .ai_models
        .iter_mut()
        .find(|model| model.id == model_id)
    {
        Some(model) => {
            change(model);
            true
        }
        None => false,
    }
}

/// Writes one model row back into `ai.models` through `update_config`, which
/// serializes list mutations under the config lock. The row is edited as JSON
/// so fields this build does not know survive the write; `None` removes a key
/// (the web omits cleared optional values instead of writing null).
fn persist_ai_model_fields(model_id: &str, fields: Vec<(&'static str, Option<serde_json::Value>)>) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::error!("Cannot persist a model row before the runtime is ready");
        return;
    };
    let target_id = model_id.to_string();
    let log_id = target_id.clone();
    handle.tokio_handle().spawn(async move {
        let service = match get_global_config_service().await {
            Ok(service) => service,
            Err(error) => {
                tracing::error!(
                    "Config service unavailable while persisting model {log_id}: {error}"
                );
                return;
            }
        };
        let result = service
            .update_config::<serde_json::Value, ()>("ai.models", move |section| {
                let Some(models) = section.as_array_mut() else {
                    return Ok(());
                };
                let Some(model) = models.iter_mut().find(|model| {
                    model.get("id").and_then(serde_json::Value::as_str) == Some(target_id.as_str())
                }) else {
                    return Ok(());
                };
                let Some(object) = model.as_object_mut() else {
                    return Ok(());
                };
                for (key, value) in fields {
                    match value {
                        Some(value) => {
                            object.insert(key.to_string(), value);
                        }
                        None => {
                            object.remove(key);
                        }
                    }
                }
                Ok(())
            })
            .await;
        match result {
            Ok(()) => tracing::info!("Model row persisted: {log_id}"),
            Err(error) => tracing::error!("Failed to persist model row {log_id}: {error}"),
        }
    });
}

/// G-SE12: the row editor's category choice. The web offers four categories;
/// a value written by a newer build stays listed verbatim instead of being
/// silently replaced.
pub(crate) fn model_category_values(stored: &str) -> Vec<&'static str> {
    let mut values = vec![
        "general_chat",
        "multimodal",
        "image_generation",
        "speech_recognition",
    ];
    if !stored.is_empty() && !values.contains(&stored) {
        values.push("__stored_unknown");
    }
    values
}

/// G-SE12-3: whether a headers draft is empty or an object of string values.
fn headers_json_valid(text: &str) -> bool {
    text.trim().is_empty()
        || matches!(
            serde_json::from_str::<serde_json::Value>(text),
            Ok(serde_json::Value::Object(fields))
                if fields.values().all(serde_json::Value::is_string)
        )
}

/// G-SE12-3: whether a body draft is empty or a JSON object.
fn body_json_valid(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty()
        || matches!(
            serde_json::from_str::<serde_json::Value>(trimmed),
            Ok(serde_json::Value::Object(_))
        )
}

/// G-SE12: one row's write actions, shared by the editor widgets.
fn set_ai_model_category(model_id: &str, category: String, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| {
        model.category =
            serde_json::from_value(serde_json::Value::String(category.clone())).unwrap_or_default();
    }) {
        persist_ai_model_fields(
            model_id,
            vec![("category", Some(serde_json::Value::String(category)))],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_context_window(model_id: &str, tokens: u32, cx: &mut App) {
    let tokens = tokens.max(MODEL_EDITOR_MIN_CONTEXT_WINDOW);
    let mut cleared_max_tokens = false;
    if update_ai_model_mirror(model_id, cx, |model| {
        model.context_window = Some(tokens);
        // The shared contract caps an explicit output limit at 40% of the
        // context window. Shrinking the window below a stored cap would make
        // the row invalid, so the cap falls back to the automatic tier (the
        // web blocks the save instead; the shell never persists an invalid
        // pair).
        if let Some(max_tokens) = model.max_tokens {
            if !openbitfun_core::service::config::types::is_valid_configured_max_output_tokens(
                tokens, max_tokens,
            ) {
                model.max_tokens = None;
                cleared_max_tokens = true;
            }
        }
    }) {
        let mut fields = vec![("context_window", Some(serde_json::json!(tokens)))];
        if cleared_max_tokens {
            fields.push(("max_tokens", None));
        }
        persist_ai_model_fields(model_id, fields);
        cx.refresh_windows();
    }
}

fn set_ai_model_max_tokens(model_id: &str, tokens: Option<u32>, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| model.max_tokens = tokens) {
        persist_ai_model_fields(
            model_id,
            vec![("max_tokens", tokens.map(|tokens| serde_json::json!(tokens)))],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_reasoning_default(model_id: &str, preset: Option<String>, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| {
        let reasoning = model
            .reasoning
            .get_or_insert_with(openbitfun_core::service::config::types::ReasoningConfig::default);
        reasoning.default_preset = preset.clone();
    }) {
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        let target_id = model_id.to_string();
        let log_id = target_id.clone();
        handle.tokio_handle().spawn(async move {
            let Ok(service) = get_global_config_service().await else {
                return;
            };
            let result = service
                .update_config::<serde_json::Value, ()>("ai.models", move |section| {
                    let Some(models) = section.as_array_mut() else {
                        return Ok(());
                    };
                    let Some(model) = models.iter_mut().find(|model| {
                        model.get("id").and_then(serde_json::Value::as_str)
                            == Some(target_id.as_str())
                    }) else {
                        return Ok(());
                    };
                    let Some(object) = model.as_object_mut() else {
                        return Ok(());
                    };
                    let reasoning = object
                        .entry("reasoning")
                        .or_insert_with(|| serde_json::json!({}));
                    if !reasoning.is_object() {
                        *reasoning = serde_json::json!({});
                    }
                    let Some(fields) = reasoning.as_object_mut() else {
                        return Ok(());
                    };
                    match preset {
                        Some(preset) => {
                            fields.insert(
                                "default_preset".to_string(),
                                serde_json::Value::String(preset),
                            );
                        }
                        None => {
                            fields.remove("default_preset");
                        }
                    }
                    Ok(())
                })
                .await;
            match result {
                Ok(()) => tracing::info!("Model reasoning default persisted: {log_id}"),
                Err(error) => {
                    tracing::error!("Failed to persist reasoning default {log_id}: {error}")
                }
            }
        });
        cx.refresh_windows();
    }
}

/// G-SE12-3: advanced request-shaping values. Each writes one key immediately;
/// the JSON text fields write only once the text parses, so a half-typed
/// brace never lands in the shared row.
fn set_ai_model_skip_ssl(model_id: &str, enabled: bool, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| model.skip_ssl_verify = enabled) {
        persist_ai_model_fields(
            model_id,
            vec![("skip_ssl_verify", Some(serde_json::json!(enabled)))],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_inline_think(model_id: &str, enabled: bool, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| model.inline_think_in_text = enabled) {
        persist_ai_model_fields(
            model_id,
            vec![("inline_think_in_text", Some(serde_json::json!(enabled)))],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_header_mode(model_id: &str, mode: String, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| {
        model.custom_headers_mode = Some(mode.clone());
    }) {
        persist_ai_model_fields(
            model_id,
            vec![("custom_headers_mode", Some(serde_json::json!(mode)))],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_body_mode(model_id: &str, mode: String, cx: &mut App) {
    if update_ai_model_mirror(model_id, cx, |model| {
        model.custom_request_body_mode = Some(mode.clone());
    }) {
        persist_ai_model_fields(
            model_id,
            vec![("custom_request_body_mode", Some(serde_json::json!(mode)))],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_headers_text(model_id: &str, text: String, cx: &mut App) {
    ModelEditorState::mutate(cx, |state| {
        state.drafts_ready = true;
        state.headers_text = text.clone();
    });
    let Some(serde_json::Value::Object(fields)) = (if text.trim().is_empty() {
        Some(serde_json::Value::Object(Default::default()))
    } else {
        serde_json::from_str(&text).ok()
    }) else {
        return;
    };
    if !fields.values().all(serde_json::Value::is_string) {
        return;
    }
    let headers: std::collections::HashMap<String, String> = fields
        .into_iter()
        .map(|(key, value)| (key, value.as_str().unwrap_or_default().to_string()))
        .collect();
    if update_ai_model_mirror(model_id, cx, |model| {
        model.custom_headers = if headers.is_empty() {
            None
        } else {
            Some(headers.clone())
        };
    }) {
        persist_ai_model_fields(
            model_id,
            vec![(
                "custom_headers",
                if headers.is_empty() {
                    None
                } else {
                    Some(serde_json::json!(headers))
                },
            )],
        );
        cx.refresh_windows();
    }
}

fn set_ai_model_body_text(model_id: &str, text: String, cx: &mut App) {
    ModelEditorState::mutate(cx, |state| {
        state.drafts_ready = true;
        state.body_text = text.clone();
    });
    let trimmed = text.trim();
    let stored = if trimmed.is_empty() {
        None
    } else {
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) if value.is_object() => Some(trimmed.to_string()),
            _ => return,
        }
    };
    if update_ai_model_mirror(model_id, cx, |model| {
        model.custom_request_body = stored.clone();
    }) {
        persist_ai_model_fields(
            model_id,
            vec![(
                "custom_request_body",
                stored.as_ref().map(|body| serde_json::json!(body)),
            )],
        );
        cx.refresh_windows();
    }
}

/// Whether one distribution row matches the filter: its display name or its
/// provider label, case-insensitively.
pub(crate) fn usage_entry_matches(
    entry: &openbitfun_services_core::token_usage::UsageStatisticsEntry,
    query: &str,
) -> bool {
    if query.is_empty() {
        return true;
    }
    entry.name.to_lowercase().contains(query)
        || entry
            .provider_name
            .as_deref()
            .map(|provider| provider.to_lowercase().contains(query))
            .unwrap_or(false)
}

/// One distribution block (model / group / endpoint) over the same totals the
/// summary uses. Every row carries requests, tokens and, when telemetry
/// exists, its cache hit rate; the filter applies across all three blocks, as
/// in the web panel.
fn usage_distribution(
    title: &str,
    entries: &[openbitfun_services_core::token_usage::UsageStatisticsEntry],
    filter: &str,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    if entries.is_empty() {
        return div().into_any_element();
    }
    let visible: Vec<_> = entries
        .iter()
        .filter(|entry| usage_entry_matches(entry, filter))
        .take(20)
        .collect();
    let mut block = div().v_flex().gap_1().w_full().child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(title.to_string()),
    );
    if visible.is_empty() {
        block = block.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.settings_usage_filter_empty.clone()),
        );
        return block.into_any_element();
    }
    for entry in visible {
        let mut facts = format!("{} · {}", entry.requests, entry.tokens);
        if let Some(rate) = entry.cache_hit_rate {
            facts.push_str(&format!(" · {:.0}%", rate * 100.));
        }
        let secondary = entry.provider_name.clone().unwrap_or_default();
        block = block.child(
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
                        .child(if secondary.is_empty() {
                            entry.name.clone()
                        } else {
                            format!("{} · {secondary}", entry.name)
                        }),
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
    block.into_any_element()
}

/// Archived sessions page (M22): the active workspace's archived sessions
/// with restore and delete. The rows reuse the sidebar's session snapshot
/// (archived rows are already carried for the show-archived toggle), and the
/// actions are the same SDK calls the sidebar uses.
fn archived_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingGroup {
    let owned_text = text.clone();
    let list_field = SettingField::render(move |_options, window, cx| {
        archived_sessions_field(view.clone(), &owned_text, window, cx)
    });

    SettingGroup::new().items(vec![SettingItem::new(
        text.settings_archived_list.clone(),
        list_field,
    )
    .description(text.settings_archived_description.clone())
    // The list is a page body, not a control: the default horizontal
    // item would hand it the narrow control slot beside the label, and
    // a row (name + date + two actions) does not fit there. Stacking
    // gives the rows the page's full width.
    .layout(Axis::Vertical)])
}

/// The archived list body, exposed for the headless render test; the page
/// itself stays private behind [`render_page`].
///
/// G-SE30: the body renders the shell's cross-workspace projection — one
/// collapsible header per workspace (label + count) over its rows (name,
/// last-active date, restore/delete) — plus the page's loading, per-workspace
/// failure, receipt, and empty states (web `ArchivedSessionsConfig`; the web's
/// toasts become the inline receipt because this surface has no toast system).
pub(crate) fn archived_sessions_field(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    _window: &mut gpui_kit::Window,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let Some(shell) = view.upgrade() else {
        return div().into_any_element();
    };
    let (groups, failures, loading, collapsed, busy) = {
        let shell = shell.read(cx);
        (
            shell.archived_groups.clone(),
            shell.archived_failures.clone(),
            shell.archived_loading,
            shell.archived_collapsed.clone(),
            shell.archived_pending.is_some(),
        )
    };
    let has_rows = groups.iter().any(|group| !group.rows.is_empty());

    // Header actions (web `headerExtra`): a refresh always, the bulk delete
    // while something can be deleted.
    let refresh_view = view.clone();
    let mut actions = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap_2()
        .child(
            Button::new("archived-refresh")
                .label(text.settings_archived_refresh.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .disabled(loading || busy)
                .on_click(move |_, _window, cx| {
                    if let Some(target) = refresh_view.upgrade() {
                        target.update(cx, |this, cx| this.reload_archived_groups(cx));
                    }
                }),
        );
    if has_rows {
        let delete_all_view = view.clone();
        actions = actions.child(
            Button::new("archived-delete-all")
                .label(text.settings_archived_delete_all.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .disabled(busy)
                .on_click(move |_, window, cx| {
                    open_archived_delete_all_dialog(delete_all_view.clone(), window, cx);
                }),
        );
    }
    let mut column = div().v_flex().gap_3().w_full().child(actions);

    // The last row action's receipt. The web toasts it; this surface has no
    // toast system, so it stays until the next action replaces it.
    if let Some((succeeded, message)) = ArchivedNotice::current(cx) {
        column = column.child(
            div()
                .min_w_0()
                .text_sm()
                .text_color(if succeeded {
                    cx.theme().foreground
                } else {
                    cx.theme().danger
                })
                .child(message),
        );
    }

    // The first read shows the web's loading line; later reads refresh in
    // place behind the disabled refresh button.
    if loading && groups.is_empty() && failures.is_empty() {
        return column
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.settings_archived_loading.clone()),
            )
            .into_any_element();
    }

    for failure in &failures {
        column = column.child(
            div()
                .min_w_0()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(failure.clone()),
        );
    }

    if !has_rows {
        if !loading {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.settings_archived_empty.clone()),
            );
        }
        return column.into_any_element();
    }

    for group in &groups {
        if group.rows.is_empty() {
            continue;
        }
        column = column.child(archived_group_section(
            group,
            view.clone(),
            text,
            collapsed.iter().any(|root| root == &group.root_path),
            busy,
            cx,
        ));
    }
    column.into_any_element()
}

/// One workspace's group on the archived page: the collapsible header (fold
/// chevron, folder glyph, label, count chip) and, expanded, its rows.
fn archived_group_section(
    group: &crate::shell::ArchivedGroupView,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    collapsed: bool,
    busy: bool,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let root = group.root_path.clone();
    let header_id = SharedString::from(format!("archived-group-{root}"));
    let toggle_view = view.clone();
    let toggle_root = root.clone();
    let mut section = div().v_flex().w_full().child(
        div()
            .id(header_id)
            .cursor_pointer()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .p_2()
            .rounded(cx.theme().radius)
            .text_sm()
            .hover(|style| style.bg(cx.theme().muted))
            .on_click(move |_, _window, cx| {
                if let Some(target) = toggle_view.upgrade() {
                    target.update(cx, |this, cx| this.toggle_archived_group(&toggle_root, cx));
                }
            })
            .child(
                gpui_kit::component::Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .small(),
            )
            .child(gpui_kit::component::Icon::new(IconName::Folder).small())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_medium()
                    .truncate()
                    .child(SharedString::from(group.label.clone())),
            )
            .child(archived_count_chip(group.rows.len(), cx)),
    );
    if !collapsed {
        let mut rows = div().v_flex().w_full();
        for row in &group.rows {
            rows = rows.child(archived_row(row, group, view.clone(), text, busy, cx));
        }
        section = section.child(rows);
    }
    section.into_any_element()
}

/// A group header's count chip (web
/// `archived-sessions-config__group-count`).
fn archived_count_chip(count: usize, cx: &App) -> gpui_kit::AnyElement {
    div()
        .flex_shrink_0()
        .min_w(px(18.))
        .h(px(18.))
        .px_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(cx.theme().muted)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from(count.to_string()))
        .into_any_element()
}

/// One archived row: the name over its last-active date, then the restore and
/// delete actions (web `ArchivedRow`; both stay disabled while another row
/// action is settling).
fn archived_row(
    row: &crate::shell::ArchivedRowView,
    group: &crate::shell::ArchivedGroupView,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    busy: bool,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let date = crate::runtime::format_last_active(row.last_active_at);
    let restore_view = view.clone();
    let restore_workspace = group.workspace_id.clone();
    let restore_session = row.session_id.clone();
    let restore_name = row.name.clone();
    let restore_label = group.label.clone();
    let delete_view = view;
    let delete_workspace = group.workspace_id.clone();
    let delete_session = row.session_id.clone();
    let delete_name = row.name.clone();
    let delete_label = group.label.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .w_full()
        .px_2()
        .py_1()
        .child(
            div()
                // `flex_1` (not just `min_w_0`): without it the name block keeps
                // its natural width, the row outgrows the settings column, and
                // the row's own actions — and every later header's count chip —
                // are laid out past the visible edge.
                .flex_1()
                .min_w_0()
                .v_flex()
                .child(
                    div()
                        .truncate()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(SharedString::from(row.name.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(date)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .flex_shrink_0()
                .child(
                    Button::new(SharedString::from(format!(
                        "archived-restore-{}",
                        row.session_id
                    )))
                    .debug_selector(|| "archived-restore".to_string())
                    .label(text.settings_archived_restore.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(busy)
                    .on_click(move |_, _window, cx| {
                        let Some(target) = restore_view.upgrade() else {
                            return;
                        };
                        let tx = target.read(cx).event_tx.clone();
                        let success =
                            i18n::archived_restore_succeeded(&restore_name, &restore_label, cx);
                        target.update(cx, |this, cx| this.begin_archived_action(success, cx));
                        if let Some(handle) = crate::runtime::runtime_handle() {
                            tracing::info!("Restoring archived session {restore_session}");
                            handle.set_session_archived_in(
                                restore_workspace.clone(),
                                restore_session.clone(),
                                false,
                                tx,
                            );
                        }
                    }),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "archived-delete-{}",
                        row.session_id
                    )))
                    .debug_selector(|| "archived-delete".to_string())
                    .label(text.session_delete.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        open_archived_delete_dialog(
                            delete_view.clone(),
                            delete_workspace.clone(),
                            delete_session.clone(),
                            delete_name.clone(),
                            delete_label.clone(),
                            window,
                            cx,
                        );
                    }),
                ),
        )
        .into_any_element()
}

/// Confirm dialog for deleting one archived session; the destructive action
/// never runs from the row directly (same shape as the MCP delete dialog).
/// The body names the session and its workspace because the page spans every
/// recent workspace and the delete is scoped to the owning one.
fn open_archived_delete_dialog(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    workspace_id: String,
    session_id: String,
    name: String,
    label: String,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm_view_name = name.clone();
    let confirm_view_label = label.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        let Some(target) = confirm_view.upgrade() else {
            return;
        };
        let tx = target.read(cx).event_tx.clone();
        let success = i18n::archived_delete_succeeded(&name, &label, cx);
        target.update(cx, |this, cx| this.begin_archived_action(success, cx));
        if let Some(handle) = crate::runtime::runtime_handle() {
            tracing::info!("Deleting archived session {session_id}");
            handle.delete_session_in(workspace_id.clone(), session_id.clone(), tx);
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        if let Some(target) = cancel_view.upgrade() {
            // A cancelled confirmation leaves no receipt behind.
            target.update(cx, |_this, cx| ArchivedNotice::set(None, cx));
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
                    .child(i18n::archived_delete_message(
                        &confirm_view_name,
                        &confirm_view_label,
                        cx,
                    )),
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

/// Confirm dialog for deleting every archived session the page shows. Both
/// counts come from the current projection (the web reports the entry count
/// and the workspace count); the ids are re-read at confirm time so the dialog
/// never acts on a stale list, and each deletion goes through the same
/// per-session, per-workspace call as the row action.
fn open_archived_delete_all_dialog(
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (count, workspace_count) = {
        let shell = shell.read(cx);
        (
            shell
                .archived_groups
                .iter()
                .map(|group| group.rows.len())
                .sum::<usize>(),
            shell
                .archived_groups
                .iter()
                .filter(|group| !group.rows.is_empty())
                .count(),
        )
    };
    if count == 0 {
        return;
    }

    let confirm_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, cx: &mut App| {
        if let Some(target) = confirm_view.upgrade() {
            let (tx, targets) = {
                let shell = target.read(cx);
                let targets: Vec<(String, String)> = shell
                    .archived_groups
                    .iter()
                    .flat_map(|group| {
                        group
                            .rows
                            .iter()
                            .map(|row| (group.workspace_id.clone(), row.session_id.clone()))
                    })
                    .collect();
                (shell.event_tx.clone(), targets)
            };
            let success = i18n::archived_delete_all_succeeded(count, cx);
            target.update(cx, |this, cx| this.begin_archived_action(success, cx));
            if let Some(handle) = crate::runtime::runtime_handle() {
                tracing::info!(
                    "Deleting all {} archived sessions across {} workspaces",
                    targets.len(),
                    workspace_count
                );
                for (workspace_id, session_id) in targets {
                    handle.delete_session_in(workspace_id, session_id, tx.clone());
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
                    .child(i18n::archived_delete_all_confirm_message(
                        count,
                        workspace_count,
                        cx,
                    )),
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
fn diagnostics_group(
    text: &ShellText,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
) -> SettingGroup {
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

    let owned_text = text.clone();
    let export_field = SettingField::render(move |_options, _window, cx| {
        let mut column = div().flex().flex_col().gap_2().w_full();
        if let Some((succeeded, message)) = DiagnosticsNotice::current(cx) {
            column = column.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if succeeded {
                        cx.theme().foreground
                    } else {
                        cx.theme().danger
                    })
                    .child(message),
            );
        }
        column = column.child(
            Button::new("diagnostics-export")
                .label(owned_text.settings_diagnostics_export_action.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(|_, window, cx| {
                    open_export_confirm(window, cx);
                }),
        );
        column.into_any_element()
    });

    let mut items = vec![
        SettingItem::new(text.settings_logging_level.clone(), level_field)
            .description(text.settings_logging_level_description.clone()),
        SettingItem::new(text.settings_logs_dir.clone(), logs_field)
            .description(text.settings_diagnostics_note.clone()),
        SettingItem::new(text.settings_diagnostics_export.clone(), export_field)
            .description(text.settings_diagnostics_export_description.clone()),
    ];
    // G-SE31: a run that ended without writing the clean-shutdown marker gets
    // its notice above the logging controls, matching the desktop page's alert.
    // The probe is a memory read of the startup snapshot, so this costs a
    // render-path branch, not a file stat.
    if let Some(previous) = crate::diagnostics::previous_unexpected_exit() {
        let notice = previous.clone();
        // A whole-element item, not a label/field pair: the notice is one
        // alert (icon, title, description) spanning the card, the shape the
        // desktop page gives its `Alert` above the logging section.
        items.insert(
            0,
            SettingItem::render(move |_options, _window, cx| previous_exit_notice(&notice, cx)),
        );
    }
    SettingGroup::new().items(items)
}

/// G-SE31: the title of the previous-run notice. The category decides which
/// copy applies — a written crash report reads differently from a run state
/// that simply never recorded a clean shutdown — and the desktop page makes
/// the same distinction (`logging.previousCrash` / `.previousUncleanShutdown`).
fn previous_exit_title(info: &crate::diagnostics::UnexpectedExitInfo, text: &ShellText) -> String {
    match info.category {
        crate::diagnostics::UnexpectedExitCategory::Crash => {
            text.settings_diagnostics_previous_crash.clone()
        }
        crate::diagnostics::UnexpectedExitCategory::UncleanShutdown => {
            text.settings_diagnostics_previous_unclean.clone()
        }
    }
}

/// The notice body: the session log directory line the desktop page points at.
/// The run-state `reason` stays out — the desktop alert shows the category
/// title plus this line only, and the reason is an English diagnostics string.
fn previous_exit_description(info: &crate::diagnostics::UnexpectedExitInfo, cx: &App) -> String {
    let path = info
        .session_log_dir
        .clone()
        .unwrap_or_else(|| "-".to_string());
    match info.category {
        crate::diagnostics::UnexpectedExitCategory::Crash => {
            i18n::diagnostics_previous_crash_logs(&path, cx)
        }
        crate::diagnostics::UnexpectedExitCategory::UncleanShutdown => {
            i18n::diagnostics_previous_unclean_logs(&path, cx)
        }
    }
}

/// The previous-run banner on the diagnostics page: warning-toned for a crash,
/// neutral-info for an unclean shutdown. Detected runs only — a clean exit
/// renders nothing, exactly like the desktop page.
fn previous_exit_notice(
    info: &crate::diagnostics::UnexpectedExitInfo,
    cx: &App,
) -> gpui_kit::AnyElement {
    let tone = match info.category {
        crate::diagnostics::UnexpectedExitCategory::Crash => cx.theme().warning,
        crate::diagnostics::UnexpectedExitCategory::UncleanShutdown => cx.theme().info,
    };
    let title = previous_exit_title(info, &i18n::current(cx));
    div()
        .debug_selector(|| "settings-diagnostics-previous-exit".to_string())
        .flex()
        .items_start()
        .gap_2()
        .w_full()
        .px_3()
        .py_2()
        .rounded(cx.theme().radius)
        .bg(tone.opacity(0.12))
        .child(
            gpui_kit::component::Icon::new(IconName::ShieldAlert)
                .size_4()
                .text_color(tone),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .v_flex()
                .gap_0p5()
                .child(
                    div()
                        .font_medium()
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(previous_exit_description(info, cx)),
                ),
        )
        .into_any_element()
}

/// Opens the export confirmation; the bundle is written on confirm only, so
/// the "reads recent logs" step never fires from the row directly.
fn open_export_confirm(window: &mut Window, cx: &mut App) {
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let text = i18n::current(cx);
        match crate::diagnostics::export_diagnostics_bundle() {
            Ok(info) => {
                tracing::info!("Diagnostics bundle exported: {}", info.bundle_path);
                if let Some(parent) = std::path::Path::new(&info.bundle_path).parent() {
                    open_path_in_file_manager(parent);
                }
                DiagnosticsNotice::set(
                    Some((true, text.settings_diagnostics_export_success.clone())),
                    cx,
                );
            }
            Err(error) => {
                tracing::error!("Failed to export diagnostics bundle: {error}");
                DiagnosticsNotice::set(
                    Some((
                        false,
                        format!("{} ({error})", text.settings_diagnostics_export_failed),
                    )),
                    cx,
                );
            }
        }
        window.close_dialog(cx);
        cx.refresh_windows();
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        dialog
            .title(div().child(text.settings_diagnostics_export_title.clone()))
            .child(
                div()
                    .w(px(420.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(text.settings_diagnostics_export_warning.clone()),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("diagnostics-export-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("diagnostics-export-confirm")
                            .label(text.settings_diagnostics_export_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the shared log directory in the platform file manager. Failures are
/// logged; there is nothing to show in the window for them.
fn open_logs_directory() {
    let Ok(manager) = openbitfun_core::infrastructure::try_get_path_manager_arc() else {
        return;
    };
    open_path_in_file_manager(&manager.logs_dir());
}

/// Opens a directory in the platform file manager. Failures are logged;
/// there is nothing to show in the window for them. Shared with the sidebar's
/// workspace-group "reveal" action (web `nav.workspaces.actions.reveal`).
pub(crate) fn open_path_in_file_manager(dir: &std::path::Path) {
    match openbitfun_services_core::process_manager::create_command("xdg-open")
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("Opened directory {}", dir.display()),
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

    /// The Web UI `settingsRegistry` page ids, verbatim from
    /// `src/web-ui/src/app/scenes/settings/settingsRegistry.ts`.
    const REGISTRY_PAGES: &[&str] = &[
        "application.general",
        "application.appearance",
        "application.pet",
        "application.voice",
        "application.shortcuts",
        "application.terminal",
        "application.editor",
        "ai.models",
        "ai.memory",
        "ai.trinity",
        "workspace.session",
        "workspace.worktrees",
        "tools.execution",
        "tools.desktop-control",
        "tools.automation",
        "tools.webSearch",
        "tools.mcp",
        "tools.acp",
        "data.usage",
        "data.archived",
        "data.diagnostics",
    ];

    /// Every entry carries the nav metadata the tree, the page header, and
    /// the element ids read: a label, a description, and a unique slug.
    #[test]
    fn settings_entries_carry_unique_nav_metadata() {
        let text = sample_text();
        let entries = settings_entries().collect::<Vec<_>>();
        assert_eq!(entries.len(), 18);
        let mut slugs = entries
            .iter()
            .map(|&entry| entry_slug(entry))
            .collect::<Vec<_>>();
        assert_eq!(slugs.len(), 18);
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), 18, "slugs key the page elements");
        for (ix, &entry) in entries.iter().enumerate() {
            assert!(!entry_label(entry, &text).is_empty(), "{entry:?}");
            assert!(!entry_description(entry, &text).is_empty(), "{entry:?}");
            assert_eq!(entry_index(entry), ix, "page index follows the nav order");
        }
    }

    /// The nav projects the category tree while the search field is empty and
    /// flattens to matching pages while it holds a query (web `SettingsNav`:
    /// `NavigationPanelSection` per category, else the result list).
    #[test]
    fn settings_nav_projects_the_tree_and_the_search() {
        let text = sample_text();

        let tree = nav_groups(&text, SettingsEntry::General, "");
        assert_eq!(tree.len(), SETTINGS_CATEGORIES.len());
        assert_eq!(
            tree[0].heading.as_deref(),
            Some(text.settings_category_application.as_str())
        );
        assert_eq!(
            tree.iter().map(|group| group.rows.len()).sum::<usize>(),
            18,
            "every tree entry is a row"
        );
        assert!(matches!(
            &tree[0].rows[0],
            SettingsNavRow::Page { entry: SettingsEntry::General, label, active: true }
                if label == &text.settings_general
        ));
        assert!(matches!(
            &tree[4].rows[2],
            SettingsNavRow::Page {
                entry: SettingsEntry::Diagnostics,
                active: false,
                ..
            }
        ));

        // A query flattens the tree: one result block, matched pages only.
        let searched = nav_groups(&text, SettingsEntry::Usage, "cache hit rate");
        assert_eq!(searched.len(), 1);
        assert!(searched[0].heading.is_none());
        assert_eq!(searched[0].rows.len(), 1);
        assert!(matches!(
            &searched[0].rows[0],
            SettingsNavRow::Result { entry: SettingsEntry::Usage, active: true, path, .. }
                if path.ends_with(&text.settings_usage)
        ));

        // No match is stated, never a silent empty list.
        let missing = nav_groups(&text, SettingsEntry::General, "no-such-setting");
        assert!(matches!(
            &missing[0].rows[..],
            [SettingsNavRow::Empty(message)] if message == &text.settings_nav_search_empty
        ));
    }

    /// The tree covers the registry one-to-one: every registry page is
    /// either a tree entry (rendered or redirected) or explicitly hidden —
    /// never dropped silently and never invented.
    #[test]
    fn settings_tree_covers_the_registry_one_to_one() {
        let mut tree_ids: Vec<&str> = SETTINGS_CATEGORIES
            .iter()
            .flat_map(|&category| category_entries(category).iter().copied())
            .filter_map(|entry| entry.registry_id())
            .collect();
        let mut hidden = HIDDEN_REGISTRY_PAGES.to_vec();
        tree_ids.extend(hidden.drain(..));
        tree_ids.sort_unstable();

        let mut registry = REGISTRY_PAGES.to_vec();
        registry.sort_unstable();
        assert_eq!(tree_ids, registry, "tree + hidden must equal the registry");

        let total: usize = SETTINGS_CATEGORIES
            .iter()
            .map(|&category| category_entries(category).len())
            .sum();
        assert_eq!(total, 18, "16 real pages + 2 redirect cards");
        assert_eq!(HIDDEN_REGISTRY_PAGES.len(), 5);
    }

    /// The two redirect entries are exactly the registry pages another shell
    /// surface owns (parity §2.1); everything else with a registry id is a
    /// real settings group.
    #[test]
    fn redirect_entries_cover_exactly_the_owned_surfaces() {
        let redirects = SETTINGS_CATEGORIES
            .iter()
            .flat_map(|&category| category_entries(category).iter().copied())
            .filter(|entry| {
                matches!(
                    entry,
                    SettingsEntry::SessionSearch | SettingsEntry::WorktreesGit
                )
            })
            .filter_map(|entry| entry.registry_id())
            .collect::<Vec<_>>();
        assert_eq!(redirects, vec!["workspace.session", "workspace.worktrees"]);
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
        // A2: the appearance font preference and the model/execution fields.
        config.font = Some(
            openbitfun_core::service::config::types::FontPreferenceSnapshot {
                ui_size: openbitfun_core::service::config::types::UiFontSizeSnapshot {
                    level: "custom".to_string(),
                    custom_px: Some(15),
                },
            },
        );
        config.ai.stream_ttft_timeout_secs = Some(90);
        config.ai.stream_idle_timeout_secs = None;
        config.ai.proxy.enabled = true;
        config.ai.proxy.url = "http://127.0.0.1:7890".to_string();
        config.ai.proxy.username = Some("proxy-user".to_string());
        config.ai.proxy.password = Some("proxy-secret".to_string());
        config.ai.tool_execution_timeout_secs = Some(600);
        config.ai.user_question_timeout_secs = Some(240);
        config.ai.agent_model_defaults.subagents.default_selection =
            openbitfun_core::service::config::types::SubagentModelSelection::fixed("model-b");
        config.ai.subagent_batch_execution_policy =
            openbitfun_core::service::config::types::SubagentBatchExecutionPolicy::SafeOnly;
        config.ai.subagent_max_concurrency = 7;
        config.ai.swarm_max_concurrency = 20;
        config.ai.enable_deferred_tool_loading = false;
        config.ai.allow_tool_json_repair = false;
        if let Some(team) = config.ai.review_teams.get_mut("default") {
            team.max_parallel_reviewers = 1;
            team.max_queue_wait_seconds = 300;
        }

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
        // A2.
        assert_eq!(snapshot.ui_font_level, "custom");
        assert_eq!(snapshot.ui_font_custom_px, Some(15));
        assert_eq!(snapshot.stream_ttft_timeout_secs, Some(90));
        assert_eq!(snapshot.stream_idle_timeout_secs, None);
        assert!(proxy_enabled(&snapshot.proxy));
        assert_eq!(proxy_text(&snapshot.proxy, "url"), "http://127.0.0.1:7890");
        assert_eq!(proxy_text(&snapshot.proxy, "username"), "proxy-user");
        assert_eq!(proxy_text(&snapshot.proxy, "password"), "proxy-secret");
        assert_eq!(snapshot.tool_execution_timeout_secs, Some(600));
        assert_eq!(snapshot.user_question_timeout_secs, Some(240));
        assert_eq!(
            subagent_model_choice(&snapshot.subagent_default_model),
            "model-b"
        );
        assert_eq!(snapshot.subagent_batch_policy, "safe_only");
        assert_eq!(snapshot.subagent_max_concurrency, 7);
        assert_eq!(snapshot.swarm_max_concurrency, 20);
        assert!(!snapshot.deferred_tool_loading);
        assert!(!snapshot.allow_tool_json_repair);
        assert_eq!(
            object_u64(
                &snapshot.review_team,
                "max_parallel_reviewers",
                DEFAULT_REVIEW_PARALLELISM
            ),
            1
        );
        assert_eq!(
            object_u64(
                &snapshot.review_team,
                "max_queue_wait_seconds",
                DEFAULT_REVIEW_QUEUE_WAIT_SECS
            ),
            300
        );
    }

    /// A2 (G-SE5): every level keeps the ratio it has to the default level on
    /// the web, and `default` is the shell's own base so the setting starts as
    /// a no-op.
    #[test]
    fn ui_font_levels_scale_from_the_shell_base() {
        assert_eq!(ui_font_rem_px("default", None), SHELL_BASE_REM_PX);
        let compact = ui_font_rem_px("compact", None);
        assert!((compact - SHELL_BASE_REM_PX * 12. / 14.).abs() < 1e-6);
        assert!(compact < ui_font_rem_px("small", None));
        assert!(ui_font_rem_px("small", None) < ui_font_rem_px("medium", None));
        assert!(ui_font_rem_px("medium", None) < ui_font_rem_px("large", None));
        // An unknown level renders as the default size rather than failing.
        assert_eq!(ui_font_rem_px("future-level", None), SHELL_BASE_REM_PX);
        // The custom level reads `customPx`, clamped onto the web's 12-20.
        assert_eq!(ui_font_base_px("custom", Some(17)), 17);
        assert_eq!(ui_font_base_px("custom", Some(4)), MIN_UI_FONT_PX);
        assert_eq!(ui_font_base_px("custom", Some(99)), MAX_UI_FONT_PX);
        assert_eq!(ui_font_base_px("custom", None), DEFAULT_UI_FONT_PX);
    }

    /// A2 (G-SE5): the persisted document matches the web `FontPreference`
    /// shape — `customPx` only on the custom level.
    #[test]
    fn ui_font_preference_document_matches_the_web_shape() {
        assert_eq!(
            ui_font_preference_value("large", None),
            serde_json::json!({ "uiSize": { "level": "large" } })
        );
        assert_eq!(
            ui_font_preference_value("custom", Some(21)),
            serde_json::json!({ "uiSize": { "level": "custom", "customPx": 20 } })
        );
    }

    /// A2 (G-SE5): a stored level the shell does not render stays selectable
    /// verbatim, the same tolerance the close-behavior and appearance rows
    /// give their own unsupported values.
    #[test]
    fn ui_font_level_options_keep_a_stored_unknown_level() {
        let text = sample_text();
        let known = ui_font_level_options("medium", &text);
        assert_eq!(known[0].0.as_ref(), "compact");
        assert_eq!(known[1].1.as_ref(), text.ui_font_level_small);
        assert_eq!(known.last().map(|(id, _)| id.as_ref()), Some("custom"));

        let unknown = ui_font_level_options("gigantic", &text);
        assert_eq!(unknown[0].0.as_ref(), "gigantic");
        assert!(unknown[0].1.contains(&text.value_unsupported));
        assert_eq!(unknown.len(), UI_FONT_LEVELS.len() + 2);
    }

    /// A2 (G-SE19): both concurrency rows accept exactly the web range.
    #[test]
    fn concurrency_limits_cover_the_web_domains() {
        assert_eq!(clamp_concurrency(1., MAX_SUBAGENT_CONCURRENCY), Some(1));
        assert_eq!(clamp_concurrency(5., MAX_SUBAGENT_CONCURRENCY), Some(5));
        assert_eq!(clamp_concurrency(32., MAX_SUBAGENT_CONCURRENCY), Some(32));
        assert_eq!(clamp_concurrency(0., MAX_SUBAGENT_CONCURRENCY), None);
        assert_eq!(clamp_concurrency(33., MAX_SUBAGENT_CONCURRENCY), None);
        assert_eq!(clamp_concurrency(f64::NAN, MAX_SUBAGENT_CONCURRENCY), None);
        assert_eq!(clamp_concurrency(64., MAX_SWARM_CONCURRENCY), Some(64));
        assert_eq!(clamp_concurrency(65., MAX_SWARM_CONCURRENCY), None);
        assert_eq!(clamp_concurrency(5.6, MAX_SWARM_CONCURRENCY), Some(6));
    }

    /// A2 (G-SE19): the model choice round-trips through the shared document
    /// shape (`inherit` | a model id).
    #[test]
    fn subagent_model_choices_round_trip_through_the_shared_shape() {
        use openbitfun_core::service::config::types::SubagentModelSelection;
        assert_eq!(
            subagent_model_choice(&subagent_model_selection("inherit")),
            "inherit"
        );
        assert_eq!(
            subagent_model_choice(&subagent_model_selection("fast")),
            "fast"
        );
        assert_eq!(
            subagent_model_choice(&subagent_model_selection("primary")),
            "primary"
        );
        assert_eq!(
            serde_json::to_value(subagent_model_selection("inherit")).unwrap(),
            serde_json::json!({ "kind": "inherit" })
        );
        assert_eq!(
            serde_json::to_value(subagent_model_selection("model-x")).unwrap(),
            serde_json::json!({ "kind": "fixed", "model_id": "model-x" })
        );
        assert_eq!(
            subagent_model_choice(&SubagentModelSelection::fixed("model-y")),
            "model-y"
        );
    }

    /// A2 (G-SE19): a stored batch policy outside the two the web renders (the
    /// core enum's `serial`) stays visible instead of being rewritten.
    #[test]
    fn subagent_batch_options_keep_an_unlisted_policy() {
        let text = sample_text();
        let listed = subagent_batch_options("force_parallel", &text);
        assert_eq!(listed.len(), SUBAGENT_BATCH_POLICIES.len());
        assert_eq!(listed[0].0.as_ref(), "safe_only");
        assert_eq!(listed[0].1.as_ref(), text.settings_subagent_batch_safe);

        let unlisted = subagent_batch_options("serial", &text);
        assert_eq!(unlisted.len(), SUBAGENT_BATCH_POLICIES.len() + 1);
        assert_eq!(unlisted[0].0.as_ref(), "serial");
        assert!(unlisted[0].1.contains(&text.value_unsupported));
    }

    /// A2 (G-SE11/G-SE22): the merge keeps keys this build does not know, so a
    /// document written by a newer surface survives an older shell's edit.
    #[test]
    fn object_merges_keep_keys_this_build_does_not_know() {
        let stored = serde_json::json!({ "url": "http://old", "future_key": { "a": 1 } });
        let merged = merged_object_value(&stored, "url", serde_json::json!("http://new"));
        assert_eq!(merged["url"], serde_json::json!("http://new"));
        assert_eq!(merged["future_key"], serde_json::json!({ "a": 1 }));

        // A missing document starts one; a non-object value is replaced.
        assert_eq!(
            merged_object_value(&serde_json::Value::Null, "enabled", serde_json::json!(true)),
            serde_json::json!({ "enabled": true })
        );
        assert_eq!(
            merged_object_value(
                &serde_json::json!("junk"),
                "enabled",
                serde_json::json!(false)
            ),
            serde_json::json!({ "enabled": false })
        );
    }

    /// A2 (G-SE22): the capacity readers fall back to the shared defaults for
    /// a missing document or a value of the wrong type.
    #[test]
    fn object_readers_fall_back_to_the_shared_defaults() {
        assert_eq!(
            object_u64(&serde_json::Value::Null, "max_parallel_reviewers", 2),
            2
        );
        assert_eq!(
            object_u64(&serde_json::json!({}), "max_queue_wait_seconds", 1200),
            1200
        );
        assert_eq!(
            object_u64(
                &serde_json::json!({ "max_queue_wait_seconds": "soon" }),
                "max_queue_wait_seconds",
                1200
            ),
            1200
        );
        assert_eq!(
            object_u64(
                &serde_json::json!({ "max_parallel_reviewers": 1 }),
                "max_parallel_reviewers",
                2
            ),
            1
        );
    }

    /// A2 (G-SE11): the proxy readers treat a missing document as "off with
    /// empty fields", the same defaults the shared type carries.
    #[test]
    fn proxy_readers_tolerate_a_missing_document() {
        assert!(!proxy_enabled(&serde_json::Value::Null));
        assert!(!proxy_enabled(&serde_json::json!({})));
        assert!(!proxy_enabled(&serde_json::json!({ "enabled": "yes" })));
        assert!(proxy_enabled(&serde_json::json!({ "enabled": true })));
        assert_eq!(proxy_text(&serde_json::Value::Null, "url"), "");
        assert_eq!(proxy_text(&serde_json::json!({ "url": 7 }), "url"), "");
        assert_eq!(proxy_text(&serde_json::json!({ "url": "x" }), "url"), "x");
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

    fn previous_exit_fixture() -> crate::diagnostics::UnexpectedExitInfo {
        crate::diagnostics::UnexpectedExitInfo {
            detected: true,
            started_at: Some("2026-10-15 12:00:00".to_string()),
            session_log_dir: Some("/logs/gpui-1".to_string()),
            crash_report_path: Some("/logs/gpui-1/crash.json".to_string()),
            category: crate::diagnostics::UnexpectedExitCategory::Crash,
            notify_on_startup: true,
            reason: "previous run wrote a crash report before shutdown".to_string(),
        }
    }

    /// G-SE31: the notice's title follows the run-state category (a crash
    /// report reads differently from an unclean shutdown).
    #[test]
    fn previous_exit_notice_title_follows_the_category() {
        let text = sample_text();
        let crash = previous_exit_fixture();
        assert_eq!(
            previous_exit_title(&crash, &text),
            text.settings_diagnostics_previous_crash
        );

        let unclean = crate::diagnostics::UnexpectedExitInfo {
            category: crate::diagnostics::UnexpectedExitCategory::UncleanShutdown,
            crash_report_path: None,
            notify_on_startup: false,
            ..crash
        };
        assert_eq!(
            previous_exit_title(&unclean, &text),
            text.settings_diagnostics_previous_unclean
        );
    }

    /// The banner lays out for a detected run (fixture run, no runtime).
    #[gpui_kit::test]
    fn previous_exit_notice_lays_out(cx: &mut gpui_kit::TestAppContext) {
        cx.update(install_theme_state);
        cx.update(|cx| cx.set_global(sample_text()));
        let window = cx.add_empty_window();
        let notice = window.update(|_, cx| previous_exit_notice(&previous_exit_fixture(), cx));
        window.draw(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(640.), px(200.)),
            move |_, _| notice.into_any_element(),
        );
        window.run_until_parked();
        assert!(
            window
                .debug_bounds("settings-diagnostics-previous-exit")
                .is_some(),
            "the previous-run banner lays out"
        );
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

    /// G-SE15: the strategy follows the web's compatibility rule — an explicit
    /// key wins, a legacy stored fixed id keeps its fixed meaning, and an
    /// untouched document follows the last selection.
    #[test]
    fn flow_chat_strategy_keeps_the_legacy_meaning() {
        assert_eq!(
            flow_chat_strategy(&serde_json::json!({ "default_mode_strategy": "follow_last" })),
            "follow_last"
        );
        assert_eq!(
            flow_chat_strategy(&serde_json::json!({ "default_mode_strategy": "fixed" })),
            "fixed"
        );
        assert_eq!(
            flow_chat_strategy(&serde_json::json!({ "default_mode_id": "Creative" })),
            "fixed"
        );
        assert_eq!(
            flow_chat_strategy(&serde_json::json!({ "default_mode_id": "   " })),
            "follow_last"
        );
        assert_eq!(flow_chat_strategy(&serde_json::json!({})), "follow_last");
        assert_eq!(flow_chat_strategy(&serde_json::Value::Null), "follow_last");
    }

    /// G-SE15: an unset fixed mode reads as Standard (the web's switch-to-fixed
    /// fallback), and the composer's last selection is optional.
    #[test]
    fn flow_chat_fixed_and_last_mode_values() {
        assert_eq!(flow_chat_fixed_mode(&serde_json::json!({})), "Standard");
        assert_eq!(
            flow_chat_fixed_mode(&serde_json::json!({ "default_mode_id": "Ultimate" })),
            "Ultimate"
        );
        assert_eq!(
            flow_chat_last_mode(&serde_json::json!({ "last_mode_id": "Minimal" })),
            Some("Minimal".to_string())
        );
        assert_eq!(
            flow_chat_last_mode(&serde_json::json!({ "last_mode_id": " " })),
            None
        );
        assert_eq!(flow_chat_last_mode(&serde_json::json!({})), None);
    }

    /// G-SE15: the session-title selection round-trips through the shared
    /// `TaskModelSelection` document shape.
    #[test]
    fn task_model_selection_round_trips_the_shared_shape() {
        use openbitfun_core::service::config::types::TaskModelSelection;
        assert_eq!(
            task_model_selection_document("inherit"),
            serde_json::json!({ "kind": "inherit" })
        );
        assert_eq!(
            task_model_selection_document("fast"),
            serde_json::json!({ "kind": "fixed", "model_id": "fast" })
        );
        let fixed: TaskModelSelection =
            serde_json::from_value(task_model_selection_document("model-b")).unwrap();
        assert_eq!(task_model_selection_value(&fixed), "model-b");
        let inherit: TaskModelSelection =
            serde_json::from_value(task_model_selection_document("inherit")).unwrap();
        assert_eq!(task_model_selection_value(&inherit), "inherit");
    }

    /// G-SE15: the session-title model menu carries the three aliases, the
    /// enabled catalog, and a stored id it cannot resolve.
    #[test]
    fn session_title_model_options_keep_an_unavailable_choice() {
        let text = sample_text();
        let catalog = vec![
            crate::runtime::ModelChoice {
                id: "model-a".to_string(),
                name: "Model A".to_string(),
                provider: "p".to_string(),
                enabled: true,
            },
            crate::runtime::ModelChoice {
                id: "model-off".to_string(),
                name: "Model Off".to_string(),
                provider: "p".to_string(),
                enabled: false,
            },
        ];
        let options = session_title_model_options(&catalog, "gone", &text);
        assert_eq!(options[0].0.as_ref(), "inherit");
        assert_eq!(options[1].0.as_ref(), "fast");
        assert_eq!(options[2].0.as_ref(), "primary");
        assert!(options.iter().any(|(id, _)| id.as_ref() == "model-a"));
        assert!(!options.iter().any(|(id, _)| id.as_ref() == "model-off"));
        let missing = &options.last().unwrap().1;
        assert!(missing.contains("gone"));
        assert!(missing.contains(&text.settings_session_title_model_unavailable));
    }

    /// G-SE16: the worktree fallbacks are the shared config defaults, so a
    /// first-run shell shows the same values the desktop page does.
    #[test]
    fn worktree_defaults_match_the_shared_config() {
        let defaults = default_worktree_settings_document();
        let shared = openbitfun_core::service::config::types::GlobalConfig::default()
            .app
            .worktrees;
        assert_eq!(object_str(&defaults, "rootPath", ""), shared.root_path);
        assert_eq!(
            object_str(&defaults, "branchPrefix", ""),
            shared.branch_prefix
        );
        assert_eq!(
            object_bool(&defaults, "copyLocalChanges", true),
            shared.copy_local_changes
        );
        assert_eq!(
            object_bool(&defaults, "autoDeleteEnabled", false),
            shared.auto_delete_enabled
        );
        assert_eq!(
            object_u64(&defaults, "autoDeleteLimit", 0),
            shared.auto_delete_limit as u64
        );
    }

    /// G-SE12: a category written by a newer build stays selectable instead of
    /// being remapped to a shell-known value.
    #[test]
    fn model_category_values_keep_an_unknown_stored_value() {
        let known = model_category_values("multimodal");
        assert_eq!(known.len(), 4);
        assert!(known.contains(&"speech_recognition"));

        let unknown = model_category_values("future_category");
        assert_eq!(unknown.len(), 5);
        assert_eq!(*unknown.last().unwrap(), "__stored_unknown");
    }

    /// G-SE12-3: only an object of string values may reach the shared header
    /// map, and an empty draft clears it; the body accepts an empty draft or
    /// any JSON object.
    #[test]
    fn model_json_drafts_accept_only_the_shared_shapes() {
        assert!(headers_json_valid(""));
        assert!(headers_json_valid("{\"X-Trace\":\"on\"}"));
        assert!(!headers_json_valid("{\"X-Trace\":1}"));
        assert!(!headers_json_valid("[]"));
        assert!(!headers_json_valid("{"));

        assert!(body_json_valid(""));
        assert!(body_json_valid("{\"temperature\":0.8}"));
        assert!(!body_json_valid("[1, 2]"));
        assert!(!body_json_valid("{"));
    }

    /// G-SE17: stored/selected effects round-trip the shared ids and an
    /// unknown id degrades to the interactive default instead of vanishing.
    #[test]
    fn permission_effects_round_trip_the_shared_ids() {
        use openbitfun_runtime_ports::PermissionEffect;
        for (value, effect) in [
            ("allow", PermissionEffect::Allow),
            ("ask", PermissionEffect::Ask),
            ("deny", PermissionEffect::Deny),
        ] {
            assert_eq!(permission_effect_from_value(value), effect);
            assert_eq!(permission_effect_value(effect), value);
        }
        assert_eq!(
            permission_effect_from_value("future-effect"),
            PermissionEffect::Ask
        );
    }

    /// G-SE29: the distribution filter matches the row name or its provider,
    /// case-insensitively, and an empty filter keeps everything.
    #[test]
    fn usage_entry_filter_matches_name_and_provider() {
        use openbitfun_services_core::token_usage::{UsageAttributionStatus, UsageStatisticsEntry};
        let entry = UsageStatisticsEntry {
            key: "model-a".to_string(),
            name: "Model A".to_string(),
            provider_name: Some("DeepSeek".to_string()),
            attribution_status: UsageAttributionStatus::Resolved,
            requests: 3,
            tokens: 120,
            cache_hit_rate: Some(0.5),
            cached_tokens: 60,
            reported_input_tokens: 120,
        };
        assert!(usage_entry_matches(&entry, ""));
        assert!(usage_entry_matches(&entry, "model a"));
        assert!(usage_entry_matches(&entry, "deepseek"));
        assert!(!usage_entry_matches(&entry, "openai"));
        let unattributed = UsageStatisticsEntry {
            provider_name: None,
            name: "Unknown".to_string(),
            ..entry
        };
        assert!(usage_entry_matches(&unattributed, "unknown"));
        assert!(!usage_entry_matches(&unattributed, "deepseek"));
    }

    /// G-SE24: the canonical English default is written back only while the
    /// stored text is still that default; customized text stays verbatim.
    #[test]
    fn quick_action_text_normalizes_only_unmodified_builtins() {
        let (label, prompt) = normalized_quick_action_text(
            "commit",
            "Commit",
            "Commit all current code changes",
            "提交",
            "提交当前所有代码变更",
            "提交",
            "提交当前所有代码变更",
        );
        assert_eq!(label, "Commit");
        assert_eq!(prompt, "Commit all current code changes");

        let (label, prompt) = normalized_quick_action_text(
            "commit",
            "My commit",
            "My prompt",
            "My commit",
            "My prompt",
            "My commit",
            "My prompt",
        );
        assert_eq!(label, "My commit");
        assert_eq!(prompt, "My prompt");

        let (label, prompt) = normalized_quick_action_text(
            "custom_1",
            "Anything",
            "Anything",
            "Typed",
            "Typed prompt",
            "Typed",
            "Typed prompt",
        );
        assert_eq!(label, "Typed");
        assert_eq!(prompt, "Typed prompt");
    }
}
