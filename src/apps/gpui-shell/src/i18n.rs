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
    pub session_fork: String,
    pub session_compact: String,
    pub session_export: String,
    pub fork_at_turn: String,
    pub fork_before_turn: String,
    pub export_title: String,
    pub export_done_prefix: String,
    pub compaction_started: String,
    pub usage_title: String,
    pub usage_turns: String,
    pub usage_input_tokens: String,
    pub usage_output_tokens: String,
    pub usage_cached_tokens: String,
    pub usage_total_tokens: String,
    pub usage_wall_time: String,
    pub usage_active_time: String,
    pub usage_models: String,
    pub insights_title: String,
    pub insights_generate: String,
    pub insights_generating: String,
    pub insights_loading: String,
    pub insights_empty: String,
    pub insights_sessions: String,
    pub insights_analyzed: String,
    pub insights_messages: String,
    pub insights_days_covered: String,
    pub insights_whats_working: String,
    pub insights_whats_hindering: String,
    pub insights_quick_wins: String,
    pub insights_looking_ahead: String,
    pub tasks_title: String,
    pub tasks_empty: String,
    pub tasks_enable: String,
    pub tasks_disable: String,
    pub tasks_run_now: String,
    pub files_title: String,
    pub files_loading: String,
    pub files_empty: String,
    pub files_reload: String,
    pub files_too_large: String,
    pub files_binary: String,
    pub files_truncated: String,
    pub search_title: String,
    pub search_files_tab: String,
    pub search_sessions_tab: String,
    pub search_run: String,
    pub search_running: String,
    pub search_empty: String,
    pub search_unavailable: String,
    pub search_truncated: String,
    pub terminal_new_tab: String,
    pub terminal_new_tab_default: String,
    pub terminal_profiles_manage: String,
    pub settings_terminal_profiles: String,
    pub settings_terminal_profiles_description: String,
    pub terminal_profiles_empty: String,
    pub terminal_profile_add: String,
    pub terminal_profile_edit: String,
    pub terminal_profile_delete: String,
    pub terminal_profile_dialog_title: String,
    pub terminal_profile_name: String,
    pub terminal_profile_working_dir: String,
    pub terminal_profile_startup_command: String,
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
    pub question_title: String,
    pub question_multi: String,
    pub question_submit: String,
    pub question_cancel: String,
    pub question_unreadable: String,
    pub workspace_switch: String,
    pub workspace_open_path: String,
    pub workspace_open_path_description: String,
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
    pub git_stage_all: String,
    pub git_stage: String,
    pub git_unstage: String,
    pub git_commit: String,
    pub git_commit_placeholder: String,
    pub git_push: String,
    pub git_history: String,
    pub git_files_view: String,
    pub git_no_commits: String,
    pub git_untrusted_title: String,
    pub git_untrusted_action: String,
    pub git_worktrees_view: String,
    pub git_snapshots_view: String,
    pub git_remote_unsupported: String,
    pub git_worktree_empty: String,
    pub git_worktree_create: String,
    pub git_worktree_branch_placeholder: String,
    pub git_worktree_base_ref_placeholder: String,
    pub git_worktree_remove: String,
    pub git_worktree_promote: String,
    pub git_worktree_isolate: String,
    pub git_worktree_release: String,
    pub git_worktree_no_session: String,
    pub git_worktree_main: String,
    pub git_worktree_dirty: String,
    pub git_worktree_locked: String,
    pub git_worktree_missing: String,
    pub git_worktree_managed: String,
    pub git_worktree_permanent: String,
    pub git_worktree_external: String,
    pub git_snapshot_empty: String,
    pub git_snapshot_restore: String,
    pub rollback_at_turn: String,
    pub turn_menu: String,
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

/// Resolves the localized bound-session count label for one worktree row.
pub(crate) fn worktree_sessions_label(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-worktree-sessions",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// Resolves the localized facts line for one snapshot-session row.
pub(crate) fn snapshot_stats_label(files: usize, turns: usize, changes: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-snapshot-stats",
        TranslationArgs::new()
            .with_number("files", files as f64)
            .with_number("turns", turns as f64)
            .with_number("changes", changes as f64),
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

/// Resolves the localized files-page failure line for one concrete error.
pub(crate) fn files_failed(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-files-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the localized search failure line for one concrete error.
pub(crate) fn search_failed(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-search-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the unsupported-image-attachment notice for one rejected item.
pub(crate) fn attachment_unsupported(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-chat-attachment-unsupported",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// Resolves the desktop-notification headline for a finished turn.
pub(crate) fn notification_turn_done(cx: &App) -> String {
    resolve_with(
        "gpui-shell-notification-turn-done",
        TranslationArgs::new(),
        cx,
    )
}

/// Resolves the desktop-notification headline for a failed turn.
pub(crate) fn notification_turn_failed(cx: &App) -> String {
    resolve_with(
        "gpui-shell-notification-turn-failed",
        TranslationArgs::new(),
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

/// Resolves the localized "question n of m" heading for one question of an
/// `ask_user` batch.
pub(crate) fn question_progress(index: usize, total: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-question-progress",
        TranslationArgs::new()
            .with_number("index", index as f64)
            .with_number("total", total as f64),
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
        session_fork: "Fork".to_string(),
        session_compact: "Compact context".to_string(),
        session_export: "Export transcript".to_string(),
        fork_at_turn: "Fork from this turn".to_string(),
        fork_before_turn: "Fork before this turn".to_string(),
        export_title: "Export transcript".to_string(),
        export_done_prefix: "Exported to".to_string(),
        compaction_started: "Compacting session context…".to_string(),
        usage_title: "Session usage".to_string(),
        usage_turns: "Turns".to_string(),
        usage_input_tokens: "Input tokens".to_string(),
        usage_output_tokens: "Output tokens".to_string(),
        usage_cached_tokens: "Cached tokens".to_string(),
        usage_total_tokens: "Total tokens".to_string(),
        usage_wall_time: "Wall time".to_string(),
        usage_active_time: "Active turn time".to_string(),
        usage_models: "Model".to_string(),
        insights_title: "Insights".to_string(),
        insights_generate: "Generate report".to_string(),
        insights_generating: "Generating report…".to_string(),
        insights_loading: "Loading…".to_string(),
        insights_empty: "No insight reports yet. Generate one to see session trends.".to_string(),
        insights_sessions: "Sessions".to_string(),
        insights_analyzed: "Analyzed".to_string(),
        insights_messages: "Messages".to_string(),
        insights_days_covered: "Days covered".to_string(),
        insights_whats_working: "What's working".to_string(),
        insights_whats_hindering: "What's hindering".to_string(),
        insights_quick_wins: "Quick wins".to_string(),
        insights_looking_ahead: "Looking ahead".to_string(),
        tasks_title: "Scheduled jobs".to_string(),
        tasks_empty: "No scheduled jobs. Ask the agent to create one, or add it in the desktop app.".to_string(),
        tasks_enable: "Enable".to_string(),
        tasks_disable: "Disable".to_string(),
        tasks_run_now: "Run now".to_string(),
        files_title: "Files".to_string(),
        files_loading: "Loading workspace files…".to_string(),
        files_empty: "No files in this workspace.".to_string(),
        files_reload: "Reload file tree".to_string(),
        files_too_large: "File is too large to display".to_string(),
        files_binary: "Binary file, no preview".to_string(),
        files_truncated: "Showing the first 1 MB of this file.".to_string(),
        search_title: "Search".to_string(),
        search_files_tab: "Files".to_string(),
        search_sessions_tab: "Sessions".to_string(),
        search_run: "Search".to_string(),
        search_running: "Searching…".to_string(),
        search_empty: "No results.".to_string(),
        search_unavailable: "Workspace search is unavailable here.".to_string(),
        search_truncated: "More results available; refine the query.".to_string(),
        terminal_new_tab: "New terminal tab".to_string(),
        terminal_new_tab_default: "Default shell".to_string(),
        terminal_profiles_manage: "Manage profiles…".to_string(),
        settings_terminal_profiles: "Terminal profiles".to_string(),
        settings_terminal_profiles_description: {
            "Named shells the terminal panel offers as extra tabs: working directory and \
             optional startup command."
                .to_string()
        },
        terminal_profiles_empty: "No profiles yet. Add one to open it as a new tab.".to_string(),
        terminal_profile_add: "Add profile".to_string(),
        terminal_profile_edit: "Edit".to_string(),
        terminal_profile_delete: "Delete".to_string(),
        terminal_profile_dialog_title: "Terminal profile".to_string(),
        terminal_profile_name: "Name".to_string(),
        terminal_profile_working_dir: "Working directory".to_string(),
        terminal_profile_startup_command: "Startup command".to_string(),
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
        question_title: "Your answer is needed".to_string(),
        question_multi: "Select all that apply".to_string(),
        question_submit: "Submit".to_string(),
        question_cancel: "Cancel".to_string(),
        question_unreadable: "This question could not be displayed.".to_string(),
        workspace_switch: "Switch workspace".to_string(),
        workspace_open_path: "Open workspace by path…".to_string(),
        workspace_open_path_description:
            "Folder to use as this shell's workspace. It is registered in the shared workspace list without changing the desktop's selection."
                .to_string(),
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
        git_stage_all: "Stage all changes".to_string(),
        git_stage: "Stage file".to_string(),
        git_unstage: "Unstage file".to_string(),
        git_commit: "Commit".to_string(),
        git_commit_placeholder: "Commit message".to_string(),
        git_push: "Push to remote".to_string(),
        git_history: "History".to_string(),
        git_files_view: "Changes".to_string(),
        git_no_commits: "No commits yet.".to_string(),
        git_untrusted_title: "Repository ownership is not trusted".to_string(),
        git_untrusted_action: "Trust repository".to_string(),
        git_worktrees_view: "Worktrees".to_string(),
        git_snapshots_view: "Snapshots".to_string(),
        git_remote_unsupported:
            "Worktree management and snapshots are not supported for remote workspaces."
                .to_string(),
        git_worktree_empty: "No managed worktrees yet.".to_string(),
        git_worktree_create: "Create worktree".to_string(),
        git_worktree_branch_placeholder: "Branch name (optional)".to_string(),
        git_worktree_base_ref_placeholder: "Base ref (default HEAD)".to_string(),
        git_worktree_remove: "Remove".to_string(),
        git_worktree_promote: "Keep permanently".to_string(),
        git_worktree_isolate: "Move this session into a managed worktree".to_string(),
        git_worktree_release: "Move this session back to the project checkout".to_string(),
        git_worktree_no_session: "Open a session in the chat first.".to_string(),
        git_worktree_main: "main".to_string(),
        git_worktree_dirty: "dirty".to_string(),
        git_worktree_locked: "locked".to_string(),
        git_worktree_missing: "missing".to_string(),
        git_worktree_managed: "managed".to_string(),
        git_worktree_permanent: "permanent".to_string(),
        git_worktree_external: "external".to_string(),
        git_snapshot_empty: "The snapshot system has no recorded sessions yet.".to_string(),
        git_snapshot_restore: "Undo file changes".to_string(),
        rollback_at_turn: "Roll back to this turn".to_string(),
        turn_menu: "Turn actions".to_string(),
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
        session_fork: service.t("gpui-shell-session-fork").await,
        session_compact: service.t("gpui-shell-session-compact").await,
        session_export: service.t("gpui-shell-session-export").await,
        fork_at_turn: service.t("gpui-shell-fork-at-turn").await,
        fork_before_turn: service.t("gpui-shell-fork-before-turn").await,
        export_title: service.t("gpui-shell-export-title").await,
        export_done_prefix: service.t("gpui-shell-export-done-prefix").await,
        compaction_started: service.t("gpui-shell-compaction-started").await,
        usage_title: service.t("gpui-shell-usage-title").await,
        usage_turns: service.t("gpui-shell-usage-turns").await,
        usage_input_tokens: service.t("gpui-shell-usage-input-tokens").await,
        usage_output_tokens: service.t("gpui-shell-usage-output-tokens").await,
        usage_cached_tokens: service.t("gpui-shell-usage-cached-tokens").await,
        usage_total_tokens: service.t("gpui-shell-usage-total-tokens").await,
        usage_wall_time: service.t("gpui-shell-usage-wall-time").await,
        usage_active_time: service.t("gpui-shell-usage-active-time").await,
        usage_models: service.t("gpui-shell-usage-models").await,
        insights_title: service.t("gpui-shell-insights-title").await,
        insights_generate: service.t("gpui-shell-insights-generate").await,
        insights_generating: service.t("gpui-shell-insights-generating").await,
        insights_loading: service.t("gpui-shell-insights-loading").await,
        insights_empty: service.t("gpui-shell-insights-empty").await,
        insights_sessions: service.t("gpui-shell-insights-sessions").await,
        insights_analyzed: service.t("gpui-shell-insights-analyzed").await,
        insights_messages: service.t("gpui-shell-insights-messages").await,
        insights_days_covered: service.t("gpui-shell-insights-days-covered").await,
        insights_whats_working: service.t("gpui-shell-insights-whats-working").await,
        insights_whats_hindering: service.t("gpui-shell-insights-whats-hindering").await,
        insights_quick_wins: service.t("gpui-shell-insights-quick-wins").await,
        insights_looking_ahead: service.t("gpui-shell-insights-looking-ahead").await,
        tasks_title: service.t("gpui-shell-tasks-title").await,
        tasks_empty: service.t("gpui-shell-tasks-empty").await,
        tasks_enable: service.t("gpui-shell-tasks-enable").await,
        tasks_disable: service.t("gpui-shell-tasks-disable").await,
        tasks_run_now: service.t("gpui-shell-tasks-run-now").await,
        files_title: service.t("gpui-shell-files-title").await,
        files_loading: service.t("gpui-shell-files-loading").await,
        files_empty: service.t("gpui-shell-files-empty").await,
        files_reload: service.t("gpui-shell-files-reload").await,
        files_too_large: service.t("gpui-shell-files-too-large").await,
        files_binary: service.t("gpui-shell-files-binary").await,
        files_truncated: service.t("gpui-shell-files-truncated").await,
        search_title: service.t("gpui-shell-search-title").await,
        search_files_tab: service.t("gpui-shell-search-files-tab").await,
        search_sessions_tab: service.t("gpui-shell-search-sessions-tab").await,
        search_run: service.t("gpui-shell-search-run").await,
        search_running: service.t("gpui-shell-search-running").await,
        search_empty: service.t("gpui-shell-search-empty").await,
        search_unavailable: service.t("gpui-shell-search-unavailable").await,
        search_truncated: service.t("gpui-shell-search-truncated").await,
        terminal_new_tab: service.t("gpui-shell-terminal-new-tab").await,
        terminal_new_tab_default: service.t("gpui-shell-terminal-new-tab-default").await,
        terminal_profiles_manage: service.t("gpui-shell-terminal-profiles-manage").await,
        settings_terminal_profiles: service.t("gpui-shell-terminal-profiles").await,
        settings_terminal_profiles_description: service
            .t("gpui-shell-terminal-profiles-desc")
            .await,
        terminal_profiles_empty: service.t("gpui-shell-terminal-profiles-empty").await,
        terminal_profile_add: service.t("gpui-shell-terminal-profile-add").await,
        terminal_profile_edit: service.t("gpui-shell-terminal-profile-edit").await,
        terminal_profile_delete: service.t("gpui-shell-terminal-profile-delete").await,
        terminal_profile_dialog_title: service.t("gpui-shell-terminal-profile-title").await,
        terminal_profile_name: service.t("gpui-shell-terminal-profile-name").await,
        terminal_profile_working_dir: service.t("gpui-shell-terminal-profile-dir").await,
        terminal_profile_startup_command: service.t("gpui-shell-terminal-profile-command").await,
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
        question_title: service.t("gpui-shell-question-title").await,
        question_multi: service.t("gpui-shell-question-multi").await,
        question_submit: service.t("gpui-shell-question-submit").await,
        question_cancel: service.t("gpui-shell-question-cancel").await,
        question_unreadable: service.t("gpui-shell-question-unreadable").await,
        workspace_switch: service.t("gpui-shell-workspace-switch").await,
        workspace_open_path: service.t("gpui-shell-workspace-open-path").await,
        workspace_open_path_description: service
            .t("gpui-shell-workspace-open-path-description")
            .await,
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
        git_stage_all: service.t("gpui-shell-git-stage-all").await,
        git_stage: service.t("gpui-shell-git-stage").await,
        git_unstage: service.t("gpui-shell-git-unstage").await,
        git_commit: service.t("gpui-shell-git-commit").await,
        git_commit_placeholder: service.t("gpui-shell-git-commit-placeholder").await,
        git_push: service.t("gpui-shell-git-push").await,
        git_history: service.t("gpui-shell-git-history").await,
        git_files_view: service.t("gpui-shell-git-files-view").await,
        git_no_commits: service.t("gpui-shell-git-no-commits").await,
        git_untrusted_title: service.t("gpui-shell-git-untrusted-title").await,
        git_untrusted_action: service.t("gpui-shell-git-untrusted-action").await,
        git_worktrees_view: service.t("gpui-shell-git-worktrees-view").await,
        git_snapshots_view: service.t("gpui-shell-git-snapshots-view").await,
        git_remote_unsupported: service.t("gpui-shell-git-remote-unsupported").await,
        git_worktree_empty: service.t("gpui-shell-git-worktree-empty").await,
        git_worktree_create: service.t("gpui-shell-git-worktree-create").await,
        git_worktree_branch_placeholder: service
            .t("gpui-shell-git-worktree-branch-placeholder")
            .await,
        git_worktree_base_ref_placeholder: service
            .t("gpui-shell-git-worktree-base-ref-placeholder")
            .await,
        git_worktree_remove: service.t("gpui-shell-git-worktree-remove").await,
        git_worktree_promote: service.t("gpui-shell-git-worktree-promote").await,
        git_worktree_isolate: service.t("gpui-shell-git-worktree-isolate").await,
        git_worktree_release: service.t("gpui-shell-git-worktree-release").await,
        git_worktree_no_session: service.t("gpui-shell-git-worktree-no-session").await,
        git_worktree_main: service.t("gpui-shell-git-worktree-main").await,
        git_worktree_dirty: service.t("gpui-shell-git-worktree-dirty").await,
        git_worktree_locked: service.t("gpui-shell-git-worktree-locked").await,
        git_worktree_missing: service.t("gpui-shell-git-worktree-missing").await,
        git_worktree_managed: service.t("gpui-shell-git-worktree-managed").await,
        git_worktree_permanent: service.t("gpui-shell-git-worktree-permanent").await,
        git_worktree_external: service.t("gpui-shell-git-worktree-external").await,
        git_snapshot_empty: service.t("gpui-shell-git-snapshot-empty").await,
        git_snapshot_restore: service.t("gpui-shell-git-snapshot-restore").await,
        rollback_at_turn: service.t("gpui-shell-rollback-at-turn").await,
        turn_menu: service.t("gpui-shell-turn-menu").await,
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
