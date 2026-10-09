//! Shell text backed by the core i18n-runtime (Fluent) service.
//!
//! The core service is async while the GPUI render path is sync, so every
//! locale switch resolves all shell keys in one `block` pass and stores the
//! result in a gpui global. Locale copy lives in
//! `src/crates/assembly/core/locales/*.ftl` under the `gpui-shell-*` keys.

use gpui_kit::{App, Global};
use openbitfun_core::service::i18n::{
    get_global_i18n_service, initialize_global_i18n_service, LocaleId, TranslationArgs,
};

/// Translated shell copy for the active locale.
#[derive(Debug, Clone)]
pub(crate) struct ShellText {
    pub locale: LocaleId,
    pub subtitle: String,
    pub theme: String,
    pub language: String,
    pub status_starting: String,
    pub status_ready: String,
    pub sessions_title: String,
    pub sessions_empty: String,
    pub sessions_reload: String,
    pub sessions_new: String,
    pub sessions_show_archived: String,
    pub session_rename: String,
    pub session_rename_title: String,
    pub session_archive: String,
    pub session_unarchive: String,
    pub session_delete: String,
    pub session_delete_title: String,
    pub session_delete_warning: String,
    pub session_delete_confirm: String,
    pub session_action_confirm: String,
    pub session_action_cancel: String,
    pub model_picker: String,
    pub model_picker_empty: String,
    pub model_default: String,
    pub models_title: String,
    pub models_default: String,
    pub models_default_description: String,
    pub models_configured: String,
    pub models_loading: String,
    pub models_empty: String,
    pub models_enabled: String,
    pub models_disabled: String,
    pub chat_empty: String,
    pub chat_input_placeholder: String,
    pub chat_jump_latest: String,
    pub chat_send: String,
    pub chat_stop: String,
    pub chat_busy: String,
    pub chat_submitting: String,
    pub tool_running: String,
    pub tool_waiting: String,
    pub tool_done: String,
    pub tool_failed: String,
    pub tool_cancelled: String,
    pub permission_title: String,
    pub permission_resources: String,
    pub permission_project: String,
    pub permission_risk: String,
    pub permission_allow_once: String,
    pub permission_allow_always: String,
    pub permission_deny: String,
    pub terminal_title: String,
    pub terminal_close: String,
    pub terminal_exited: String,
    pub settings_open: String,
    pub settings_appearance: String,
    pub settings_permissions: String,
    pub settings_language_description: String,
    pub settings_theme_description: String,
    pub settings_theme_system: String,
    pub settings_theme_unsupported: String,
    pub settings_policy: String,
    pub settings_policy_description: String,
    pub settings_policy_ask: String,
    pub settings_policy_full_access: String,
    pub settings_auto_approve: String,
    pub settings_auto_approve_description: String,
    pub settings_terminal: String,
    pub settings_terminal_font_size: String,
    pub settings_terminal_font_size_description: String,
    pub settings_terminal_font_family: String,
    pub settings_terminal_font_family_description: String,
    pub settings_terminal_scrollback: String,
    pub settings_terminal_scrollback_description: String,
    pub settings_terminal_shell: String,
    pub settings_terminal_shell_description: String,
    pub git_title: String,
    pub git_refresh: String,
    pub git_loading: String,
    pub git_empty: String,
    pub git_binary: String,
    pub git_too_large: String,
    pub git_truncated: String,
    pub git_staged: String,
    pub git_unstaged: String,
    pub git_untracked: String,
    pub git_select: String,
    pub git_diff_wrap: String,
}

impl Global for ShellText {}

pub(crate) fn init(cx: &mut App) {
    if let Err(err) = cx
        .foreground_executor()
        .block_on(initialize_global_i18n_service(None))
    {
        tracing::error!("Failed to initialize i18n service: {}", err);
        return;
    }
    load_locale(locale_from_env(), cx);
}

pub(crate) fn load_locale(locale: LocaleId, cx: &mut App) {
    let text = cx.foreground_executor().block_on(fetch_text(locale));
    cx.set_global(text);
    // The component library keeps its own copy (settings page navigation,
    // list search placeholders, dialog defaults); point it at the same locale.
    gpui_kit::component::set_locale(component_locale(locale));
    cx.refresh_windows();
    tracing::info!("Shell locale loaded: {}", locale.as_str());
}

/// Maps the shell locale onto the component library's locale ids.
fn component_locale(locale: LocaleId) -> &'static str {
    match locale {
        LocaleId::ZhCN => "zh-CN",
        LocaleId::ZhTW => "zh-TW",
        LocaleId::EnUS => "en",
    }
}

pub(crate) fn current(cx: &App) -> ShellText {
    cx.global::<ShellText>().clone()
}

/// Initial locale from the session environment; the runtime bootstrap refines
/// it to the configured `app.language` once the config service is up.
fn locale_from_env() -> LocaleId {
    let lang = std::env::var("LANG")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if lang.starts_with("zh_tw") || lang.starts_with("zh-tw") || lang.starts_with("zh-hant") {
        LocaleId::ZhTW
    } else if lang.starts_with("zh") {
        LocaleId::ZhCN
    } else {
        LocaleId::EnUS
    }
}

/// Resolves the localized runtime-failure line for one concrete error.
pub(crate) fn failed_status(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-status-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the localized turn-count label for one session row.
pub(crate) fn turns_label(turn_count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-session-turns",
        TranslationArgs::new().with_number("count", turn_count as f64),
        cx,
    )
}

/// Resolves the localized workspace-diff failure line for one concrete error.
pub(crate) fn git_failed(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the localized subagent attribution line for one permission
/// request; composed once when the request arrives, never on the render path.
pub(crate) fn permission_subagent_label(subagent_type: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-permission-subagent",
        TranslationArgs::new().with_string("subagent", subagent_type),
        cx,
    )
}

fn resolve_with(key: &str, args: TranslationArgs, cx: &App) -> String {
    cx.foreground_executor().block_on(async move {
        match get_global_i18n_service().await {
            Some(service) => service.t_with(key, args).await,
            None => key.to_string(),
        }
    })
}

/// English copy used when the i18n service is unavailable; also the base for
/// test fixtures that only care about a few fields.
pub(crate) fn fallback_text(locale: LocaleId) -> ShellText {
    ShellText {
        locale,
        subtitle: "GPUI native frontend".to_string(),
        theme: "Theme".to_string(),
        language: "Language".to_string(),
        status_starting: "Starting OpenBitFun runtime…".to_string(),
        status_ready: "Runtime ready".to_string(),
        sessions_title: "Sessions".to_string(),
        sessions_empty: "No sessions in this workspace yet.".to_string(),
        sessions_reload: "Reload".to_string(),
        sessions_new: "New session".to_string(),
        sessions_show_archived: "Show archived sessions".to_string(),
        session_rename: "Rename".to_string(),
        session_rename_title: "Rename session".to_string(),
        session_archive: "Archive".to_string(),
        session_unarchive: "Unarchive".to_string(),
        session_delete: "Delete".to_string(),
        session_delete_title: "Delete session".to_string(),
        session_delete_warning:
            "This permanently deletes the session and its history. This cannot be undone."
                .to_string(),
        session_delete_confirm: "Delete".to_string(),
        session_action_confirm: "Confirm".to_string(),
        session_action_cancel: "Cancel".to_string(),
        model_picker: "Session model".to_string(),
        model_picker_empty: "No configured models".to_string(),
        model_default: "Default model".to_string(),
        models_title: "Models".to_string(),
        models_default: "Default model".to_string(),
        models_default_description:
            "Model new sessions start on; existing sessions keep their own selection.".to_string(),
        models_configured: "Configured models".to_string(),
        models_loading: "Loading model catalog…".to_string(),
        models_empty: "No models configured. Add models in the desktop app to pick one here."
            .to_string(),
        models_enabled: "enabled".to_string(),
        models_disabled: "disabled".to_string(),
        chat_empty: "Pick a session on the left, or create a new one.".to_string(),
        chat_input_placeholder: "Type a message to send to the agent…".to_string(),
        chat_jump_latest: "Jump to latest".to_string(),
        chat_send: "Send".to_string(),
        chat_stop: "Stop".to_string(),
        chat_busy: "Responding…".to_string(),
        chat_submitting: "Submitting…".to_string(),
        tool_running: "Running".to_string(),
        tool_waiting: "Waiting for approval".to_string(),
        tool_done: "Done".to_string(),
        tool_failed: "Failed".to_string(),
        tool_cancelled: "Cancelled".to_string(),
        permission_title: "Permission required".to_string(),
        permission_resources: "Resources".to_string(),
        permission_project: "Project".to_string(),
        permission_risk: "Risk".to_string(),
        permission_allow_once: "Allow once".to_string(),
        permission_allow_always: "Always allow".to_string(),
        permission_deny: "Deny".to_string(),
        terminal_title: "Terminal".to_string(),
        terminal_close: "Close terminal".to_string(),
        terminal_exited: "process exited".to_string(),
        settings_open: "Open settings".to_string(),
        settings_appearance: "Appearance".to_string(),
        settings_permissions: "Permissions".to_string(),
        settings_language_description: "Interface language for the shell and desktop app."
            .to_string(),
        settings_theme_description: "Color palette, shared with the desktop appearance setting."
            .to_string(),
        settings_theme_system: "Follow system".to_string(),
        settings_theme_unsupported: "unsupported".to_string(),
        settings_policy: "Permission policy".to_string(),
        settings_policy_description: "Ask prompts before tools run; full access allows every call."
            .to_string(),
        settings_policy_ask: "Ask".to_string(),
        settings_policy_full_access: "Full access".to_string(),
        settings_auto_approve: "Auto-approve requests".to_string(),
        settings_auto_approve_description:
            "Answer interactive approval requests automatically (Ask policy only).".to_string(),
        settings_terminal: "Terminal".to_string(),
        settings_terminal_font_size: "Font size".to_string(),
        settings_terminal_font_size_description: "Text size of the built-in terminal grid."
            .to_string(),
        settings_terminal_font_family: "Font family".to_string(),
        settings_terminal_font_family_description:
            "Font stack for the terminal; empty follows the shell default.".to_string(),
        settings_terminal_scrollback: "Scrollback lines".to_string(),
        settings_terminal_scrollback_description: "History kept above the live screen.".to_string(),
        settings_terminal_shell: "Shell command".to_string(),
        settings_terminal_shell_description: "Empty follows $SHELL. A new terminal picks this up."
            .to_string(),
        git_title: "Changes".to_string(),
        git_refresh: "Refresh".to_string(),
        git_loading: "Loading changes…".to_string(),
        git_empty: "No workspace changes.".to_string(),
        git_binary: "Binary file — diff not shown".to_string(),
        git_too_large: "Diff too large to display".to_string(),
        git_truncated: "Some files are not listed (snapshot truncated).".to_string(),
        git_staged: "staged".to_string(),
        git_unstaged: "unstaged".to_string(),
        git_untracked: "untracked".to_string(),
        git_select: "Select a file to see its diff.".to_string(),
        git_diff_wrap: "Toggle diff line wrapping".to_string(),
    }
}

async fn fetch_text(locale: LocaleId) -> ShellText {
    let Some(service) = get_global_i18n_service().await else {
        tracing::warn!("i18n service unavailable, keeping English fallback copy");
        return fallback_text(locale);
    };

    service.set_locale(locale).await.unwrap_or_else(|err| {
        tracing::warn!("Failed to switch i18n locale: {}", err);
    });

    ShellText {
        locale,
        subtitle: service.t("gpui-shell-subtitle").await,
        theme: service.t("gpui-shell-theme").await,
        language: service.t("gpui-shell-language").await,
        status_starting: service.t("gpui-shell-status-starting").await,
        status_ready: service.t("gpui-shell-status-ready").await,
        sessions_title: service.t("gpui-shell-sessions-title").await,
        sessions_empty: service.t("gpui-shell-sessions-empty").await,
        sessions_reload: service.t("gpui-shell-sessions-reload").await,
        sessions_new: service.t("gpui-shell-sessions-new").await,
        sessions_show_archived: service.t("gpui-shell-sessions-show-archived").await,
        session_rename: service.t("gpui-shell-session-rename").await,
        session_rename_title: service.t("gpui-shell-session-rename-title").await,
        session_archive: service.t("gpui-shell-session-archive").await,
        session_unarchive: service.t("gpui-shell-session-unarchive").await,
        session_delete: service.t("gpui-shell-session-delete").await,
        session_delete_title: service.t("gpui-shell-session-delete-title").await,
        session_delete_warning: service.t("gpui-shell-session-delete-warning").await,
        session_delete_confirm: service.t("gpui-shell-session-delete-confirm").await,
        session_action_confirm: service.t("gpui-shell-session-action-confirm").await,
        session_action_cancel: service.t("gpui-shell-session-action-cancel").await,
        model_picker: service.t("gpui-shell-model-picker").await,
        model_picker_empty: service.t("gpui-shell-model-picker-empty").await,
        model_default: service.t("gpui-shell-model-default").await,
        models_title: service.t("gpui-shell-models-title").await,
        models_default: service.t("gpui-shell-models-default").await,
        models_default_description: service.t("gpui-shell-models-default-desc").await,
        models_configured: service.t("gpui-shell-models-configured").await,
        models_loading: service.t("gpui-shell-models-loading").await,
        models_empty: service.t("gpui-shell-models-empty").await,
        models_enabled: service.t("gpui-shell-models-enabled").await,
        models_disabled: service.t("gpui-shell-models-disabled").await,
        chat_empty: service.t("gpui-shell-chat-empty").await,
        chat_input_placeholder: service.t("gpui-shell-chat-input-placeholder").await,
        chat_jump_latest: service.t("gpui-shell-chat-jump-latest").await,
        chat_send: service.t("gpui-shell-chat-send").await,
        chat_stop: service.t("gpui-shell-chat-stop").await,
        chat_busy: service.t("gpui-shell-chat-busy").await,
        chat_submitting: service.t("gpui-shell-chat-submitting").await,
        tool_running: service.t("gpui-shell-tool-running").await,
        tool_waiting: service.t("gpui-shell-tool-waiting").await,
        tool_done: service.t("gpui-shell-tool-done").await,
        tool_failed: service.t("gpui-shell-tool-failed").await,
        tool_cancelled: service.t("gpui-shell-tool-cancelled").await,
        permission_title: service.t("gpui-shell-permission-title").await,
        permission_resources: service.t("gpui-shell-permission-resources").await,
        permission_project: service.t("gpui-shell-permission-project").await,
        permission_risk: service.t("gpui-shell-permission-risk").await,
        permission_allow_once: service.t("gpui-shell-permission-allow-once").await,
        permission_allow_always: service.t("gpui-shell-permission-allow-always").await,
        permission_deny: service.t("gpui-shell-permission-deny").await,
        terminal_title: service.t("gpui-shell-terminal-title").await,
        terminal_close: service.t("gpui-shell-terminal-close").await,
        terminal_exited: service.t("gpui-shell-terminal-exited").await,
        settings_open: service.t("gpui-shell-settings-open").await,
        settings_appearance: service.t("gpui-shell-settings-appearance").await,
        settings_permissions: service.t("gpui-shell-settings-permissions").await,
        settings_language_description: service.t("gpui-shell-settings-language-desc").await,
        settings_theme_description: service.t("gpui-shell-settings-theme-desc").await,
        settings_theme_system: service.t("gpui-shell-settings-theme-system").await,
        settings_theme_unsupported: service.t("gpui-shell-settings-theme-unsupported").await,
        settings_policy: service.t("gpui-shell-settings-policy").await,
        settings_policy_description: service.t("gpui-shell-settings-policy-desc").await,
        settings_policy_ask: service.t("gpui-shell-settings-policy-ask").await,
        settings_policy_full_access: service.t("gpui-shell-settings-policy-full-access").await,
        settings_auto_approve: service.t("gpui-shell-settings-auto-approve").await,
        settings_auto_approve_description: service.t("gpui-shell-settings-auto-approve-desc").await,
        settings_terminal: service.t("gpui-shell-settings-terminal").await,
        settings_terminal_font_size: service.t("gpui-shell-settings-terminal-font-size").await,
        settings_terminal_font_size_description: service
            .t("gpui-shell-settings-terminal-font-size-desc")
            .await,
        settings_terminal_font_family: service.t("gpui-shell-settings-terminal-font-family").await,
        settings_terminal_font_family_description: service
            .t("gpui-shell-settings-terminal-font-family-desc")
            .await,
        settings_terminal_scrollback: service.t("gpui-shell-settings-terminal-scrollback").await,
        settings_terminal_scrollback_description: service
            .t("gpui-shell-settings-terminal-scrollback-desc")
            .await,
        settings_terminal_shell: service.t("gpui-shell-settings-terminal-shell").await,
        settings_terminal_shell_description: service
            .t("gpui-shell-settings-terminal-shell-desc")
            .await,
        git_title: service.t("gpui-shell-git-title").await,
        git_refresh: service.t("gpui-shell-git-refresh").await,
        git_loading: service.t("gpui-shell-git-loading").await,
        git_empty: service.t("gpui-shell-git-empty").await,
        git_binary: service.t("gpui-shell-git-binary").await,
        git_too_large: service.t("gpui-shell-git-too-large").await,
        git_truncated: service.t("gpui-shell-git-truncated").await,
        git_staged: service.t("gpui-shell-git-staged").await,
        git_unstaged: service.t("gpui-shell-git-unstaged").await,
        git_untracked: service.t("gpui-shell-git-untracked").await,
        git_select: service.t("gpui-shell-git-select").await,
        git_diff_wrap: service.t("gpui-shell-git-diff-wrap").await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_locale_detection() {
        let detect = |value: &str| {
            let lang = value.to_ascii_lowercase();
            if lang.starts_with("zh_tw") || lang.starts_with("zh-tw") || lang.starts_with("zh-hant")
            {
                LocaleId::ZhTW
            } else if lang.starts_with("zh") {
                LocaleId::ZhCN
            } else {
                LocaleId::EnUS
            }
        };
        assert_eq!(detect("zh_CN.UTF-8"), LocaleId::ZhCN);
        assert_eq!(detect("zh_TW.UTF-8"), LocaleId::ZhTW);
        assert_eq!(detect("en_US.UTF-8"), LocaleId::EnUS);
        assert_eq!(detect("C"), LocaleId::EnUS);
    }
}
