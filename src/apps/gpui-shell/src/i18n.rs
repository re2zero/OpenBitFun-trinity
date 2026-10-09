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
    pub chat_empty: String,
    pub chat_input_placeholder: String,
    pub chat_jump_latest: String,
    pub settings: String,
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
    cx.refresh_windows();
    tracing::info!("Shell locale loaded: {}", locale.as_str());
}

pub(crate) fn current(cx: &App) -> ShellText {
    cx.global::<ShellText>().clone()
}

pub(crate) fn next_locale(locale: LocaleId) -> LocaleId {
    match locale {
        LocaleId::ZhCN => LocaleId::ZhTW,
        LocaleId::ZhTW => LocaleId::EnUS,
        LocaleId::EnUS => LocaleId::ZhCN,
    }
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

async fn fetch_text(locale: LocaleId) -> ShellText {
    let Some(service) = get_global_i18n_service().await else {
        tracing::warn!("i18n service unavailable, keeping English fallback copy");
        return ShellText {
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
            chat_empty: "Pick a session on the left, or create a new one.".to_string(),
            chat_input_placeholder: "Type a message to send to the agent…".to_string(),
            chat_jump_latest: "Jump to latest".to_string(),
            settings: "Settings".to_string(),
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
        };
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
        chat_empty: service.t("gpui-shell-chat-empty").await,
        chat_input_placeholder: service.t("gpui-shell-chat-input-placeholder").await,
        chat_jump_latest: service.t("gpui-shell-chat-jump-latest").await,
        settings: service.t("gpui-shell-settings").await,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_cycles_through_supported_locales() {
        assert_eq!(next_locale(LocaleId::ZhCN), LocaleId::ZhTW);
        assert_eq!(next_locale(LocaleId::ZhTW), LocaleId::EnUS);
        assert_eq!(next_locale(LocaleId::EnUS), LocaleId::ZhCN);
    }

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
