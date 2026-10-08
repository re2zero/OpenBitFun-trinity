//! Shell text backed by the core i18n-runtime (Fluent) service.
//!
//! The core service is async while the GPUI render path is sync, so every
//! locale switch resolves all shell keys in one `block` pass and stores the
//! result in a gpui global. Locale copy lives in
//! `src/crates/assembly/core/locales/*.ftl` under the `gpui-shell-*` keys.

use gpui::{App, Global};
use openbitfun_core::service::i18n::{
    get_global_i18n_service, initialize_global_i18n_service, LocaleId, TranslationArgs,
};

/// Translated shell copy for the active locale.
#[derive(Debug, Clone)]
pub(crate) struct ShellText {
    pub locale: LocaleId,
    pub subtitle: String,
    pub query_placeholder: String,
    pub draft_placeholder: String,
    pub ping: String,
    pub theme: String,
    pub language: String,
    pub status_starting: String,
    pub status_ready: String,
    pub sessions_title: String,
    pub sessions_empty: String,
    pub sessions_reload: String,
}

impl Global for ShellText {}

pub(crate) fn init(cx: &mut App) {
    if let Err(err) = cx
        .background_executor()
        .block(initialize_global_i18n_service(None))
    {
        tracing::error!("Failed to initialize i18n service: {}", err);
        return;
    }
    load_locale(locale_from_env(), cx);
}

pub(crate) fn load_locale(locale: LocaleId, cx: &mut App) {
    let text = cx.background_executor().block(fetch_text(locale));
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

fn resolve_with(key: &str, args: TranslationArgs, cx: &App) -> String {
    cx.background_executor().block(async move {
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
            subtitle: "Theme and i18n verification".to_string(),
            query_placeholder: "IME single-line input (preedit here)".to_string(),
            draft_placeholder: "IME multi-line input (compose across lines)".to_string(),
            ping: "Ping".to_string(),
            theme: "Theme".to_string(),
            language: "Language".to_string(),
            status_starting: "Starting OpenBitFun runtime…".to_string(),
            status_ready: "Runtime ready".to_string(),
            sessions_title: "Sessions".to_string(),
            sessions_empty: "No sessions in this workspace yet.".to_string(),
            sessions_reload: "Reload".to_string(),
        };
    };

    service.set_locale(locale).await.unwrap_or_else(|err| {
        tracing::warn!("Failed to switch i18n locale: {}", err);
    });

    ShellText {
        locale,
        subtitle: service.t("gpui-shell-subtitle").await,
        query_placeholder: service.t("gpui-shell-input-query-placeholder").await,
        draft_placeholder: service.t("gpui-shell-input-draft-placeholder").await,
        ping: service.t("gpui-shell-ping").await,
        theme: service.t("gpui-shell-theme").await,
        language: service.t("gpui-shell-language").await,
        status_starting: service.t("gpui-shell-status-starting").await,
        status_ready: service.t("gpui-shell-status-ready").await,
        sessions_title: service.t("gpui-shell-sessions-title").await,
        sessions_empty: service.t("gpui-shell-sessions-empty").await,
        sessions_reload: service.t("gpui-shell-sessions-reload").await,
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
