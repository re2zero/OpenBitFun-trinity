//! Shell text backed by the core i18n-runtime (Fluent) service.
//!
//! The core service is async while the GPUI render path is sync, so every
//! locale switch resolves all shell keys in one `block` pass and stores the
//! result in a gpui global. The shell catalog is shell-owned copy under
//! `locales/*.ftl` (compiled in at init); it is registered into the core
//! service so resolution goes through the same Fluent path. Keys keep the
//! `gpui-shell-*` prefix they carried in the core catalog era.

use gpui_kit::{App, Global};
use openbitfun_core::service::i18n::{
    get_global_i18n_service, initialize_global_i18n_service, LocaleId, TranslationArgs,
};

/// Shell-registered callback run after every locale load (see `load_locale`).
type LocaleLoadedHook = fn(locale: LocaleId, cx: &App);

static LOCALE_LOADED_HOOK: std::sync::Mutex<Option<LocaleLoadedHook>> = std::sync::Mutex::new(None);

/// The shell registers `tray::apply_locale` here at bootstrap.
pub fn set_locale_loaded_hook(hook: LocaleLoadedHook) {
    *LOCALE_LOADED_HOOK.lock().unwrap() = Some(hook);
}

fn locale_loaded_hook() -> Option<LocaleLoadedHook> {
    *LOCALE_LOADED_HOOK.lock().unwrap()
}

/// Translated shell copy for the active locale.
#[derive(Debug, Clone)]
pub struct ShellText {
    pub locale: LocaleId,
    pub theme: String,
    pub language: String,
    pub status_starting: String,
    pub sessions_title: String,
    pub sessions_empty: String,
    /// Desktop nav nouns (R1b): the sidebar rows use the web NavPanel labels
    /// (智能助理/认知体/任务看板), not the scene titles their tabs show.
    pub nav_assistant: String,
    pub nav_tasks: String,
    pub sessions_reload: String,
    pub sessions_new: String,
    pub sessions_show_archived: String,
    /// Tooltip of the sessions header's workspace-menu trigger (web
    /// `nav.tooltips.addSessionGroup`, shell scope: projects only).
    pub sessions_add_group: String,
    /// Session-tree fold rows (R1c).
    pub sessions_show_more: String,
    /// Desktop group-tail collapse row.
    pub sessions_collapse: String,
    /// Desktop nav scenes the shell degrades honestly (no MiniApp runtime;
    /// the extension/compat family is not yet scoped for the shell).
    pub miniapps_title: String,
    pub miniapps_empty: String,
    pub extensions_title: String,
    pub extensions_empty: String,
    /// The extensions empty card's one honest action (E4 batch ①): jump to
    /// the external-agent settings page the copy names.
    pub extensions_open_acp: String,
    pub sessions_show_less: String,
    /// Sessions list projection (web `nav.sessions.viewToggle` /
    /// `nav.sessions.viewMenu`): the honest subset the shell has data for —
    /// ordering updated/created/name, show unread, status running — plus the
    /// two group-header actions (copy path, reveal in file manager).
    pub sessions_view_all_tooltip: String,
    pub sessions_view_grouped_tooltip: String,
    pub sessions_filter_tooltip: String,
    pub sessions_ordering: String,
    pub sessions_order_updated: String,
    pub sessions_order_created: String,
    pub sessions_order_name: String,
    pub sessions_show: String,
    pub sessions_show_all: String,
    pub sessions_show_unread: String,
    pub sessions_filters: String,
    pub sessions_status_running: String,
    pub sessions_reset_filters: String,
    pub sessions_no_matches: String,
    pub sessions_collapse_all: String,
    pub sessions_mark_all_read: String,
    pub sessions_copy_path: String,
    pub sessions_reveal: String,
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
    pub insights_empty_title: String,
    pub insights_empty: String,
    pub insights_sessions: String,
    pub insights_analyzed: String,
    pub insights_messages: String,
    pub insights_days_covered: String,
    pub insights_whats_working: String,
    pub insights_whats_hindering: String,
    pub insights_quick_wins: String,
    pub insights_looking_ahead: String,
    /// G-I1: the insights header controls.
    pub insights_model_label: String,
    pub insights_model_primary: String,
    pub insights_range_label: String,
    /// G-I6: the report-history section header and its retention hint.
    pub insights_history: String,
    pub insights_keep_latest: String,
    /// G-I5: the suggestion sections and their copy affordance.
    pub insights_md_additions: String,
    pub insights_features_to_try: String,
    pub insights_usage_patterns: String,
    pub insights_try_this_prompt: String,
    pub insights_copy: String,
    pub tasks_title: String,
    pub tasks_empty_title: String,
    pub tasks_empty: String,
    pub tasks_run_now: String,
    /// E4-b2 board chrome: page subtitle, overview metrics, calendar card.
    pub tasks_subtitle: String,
    pub tasks_overview: String,
    pub tasks_overview_unit: String,
    pub tasks_overview_total: String,
    pub tasks_overview_due_soon: String,
    pub tasks_wd_mon: String,
    pub tasks_wd_tue: String,
    pub tasks_wd_wed: String,
    pub tasks_wd_thu: String,
    pub tasks_wd_fri: String,
    pub tasks_wd_sat: String,
    pub tasks_wd_sun: String,
    pub tasks_cal_title: String,
    pub tasks_cal_prev: String,
    pub tasks_cal_next: String,
    pub tasks_cal_today: String,
    /// E4-b2: the day panel's fallback title and empty line (the titled form
    /// is the parameterized `tasks_day_detail_title` helper's first run).
    pub tasks_day_detail_title: String,
    pub tasks_day_clear: String,
    pub tasks_day_empty: String,
    /// G-P3: the web row badges (running / nearest upcoming run / overdue).
    pub tasks_badge_running: String,
    pub tasks_badge_next_run: String,
    pub tasks_badge_overdue: String,
    /// G-P3: the inactive partition's per-job reason chips.
    pub tasks_inactive_paused: String,
    pub tasks_inactive_done: String,
    pub tasks_inactive_invalid: String,
    pub tasks_list_empty: String,
    /// G-P1: the scheduled-job create/edit dialog.
    pub tasks_new: String,
    pub tasks_editor_create_title: String,
    pub tasks_editor_edit_title: String,
    pub tasks_editor_create_desc: String,
    pub tasks_editor_edit_desc: String,
    pub tasks_editor_btn_create: String,
    pub tasks_editor_btn_save: String,
    pub tasks_editor_cron_hint: String,
    pub tasks_editor_name_placeholder: String,
    pub tasks_editor_prompt_placeholder: String,
    pub tasks_editor_name: String,
    pub tasks_editor_prompt: String,
    pub tasks_editor_schedule: String,
    pub tasks_editor_kind_at: String,
    pub tasks_editor_kind_every: String,
    pub tasks_editor_kind_cron: String,
    pub tasks_editor_at: String,
    pub tasks_editor_at_hint: String,
    pub tasks_editor_every: String,
    pub tasks_editor_anchor: String,
    pub tasks_editor_anchor_hint: String,
    pub tasks_editor_unit_minute: String,
    pub tasks_editor_unit_hour: String,
    pub tasks_editor_unit_day: String,
    pub tasks_editor_cron_expr: String,
    pub tasks_editor_cron_tz: String,
    pub tasks_editor_target: String,
    pub tasks_editor_target_workspace: String,
    pub tasks_editor_target_session: String,
    pub tasks_editor_session_placeholder: String,
    pub tasks_editor_no_sessions: String,
    pub tasks_editor_enabled: String,
    pub tasks_editor_error_name: String,
    pub tasks_editor_error_prompt: String,
    pub tasks_editor_error_prompt_long: String,
    pub tasks_editor_error_at: String,
    pub tasks_editor_error_every: String,
    pub tasks_editor_error_anchor: String,
    pub tasks_editor_error_cron: String,
    pub tasks_editor_error_session: String,
    pub tasks_editor_error_workspace: String,
    /// R2c docked resource panel chrome.
    pub panel_pop_out: String,
    pub panel_close: String,
    pub panel_maximize: String,
    pub panel_restore: String,
    pub panel_swap: String,
    /// Top-bar resource panel toggle (opens the panel when none is docked).
    pub panel_open: String,
    /// Sidebar drawer toggle (nav top bar + collapsed strip).
    pub sidebar_toggle: String,
    pub files_title: String,
    pub files_loading: String,
    pub files_empty: String,
    pub files_empty_title: String,
    /// The viewer column's unselected state (E4 batch ① copy debt): distinct
    /// from `files_empty*`, which stays "the workspace truly has no files".
    pub files_viewer_empty: String,
    pub files_viewer_empty_title: String,
    pub files_reload: String,
    /// Local filter box above the tree (G-FL2); the full-workspace search
    /// stays in the Ctrl K overlay.
    pub files_filter_placeholder: String,
    pub files_no_matches: String,
    pub files_too_large: String,
    pub files_binary: String,
    pub files_truncated: String,
    pub search_trigger: String,
    pub search_files_tab: String,
    pub search_sessions_tab: String,
    pub search_run: String,
    pub search_running: String,
    pub search_empty: String,
    pub search_empty_title: String,
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
    /// Composer strip (R2a): local execution fact and the permission preset
    /// badge copied from the web strip labels (web truth: flow-chat
    /// `chatInput.permissionMode.*`, device `footerLocalSimple`).
    pub composer_local: String,
    pub permission_ask: String,
    pub permission_ask_desc: String,
    pub permission_auto: String,
    pub permission_auto_desc: String,
    pub permission_full_access: String,
    pub permission_full_access_desc: String,
    pub models_title: String,
    pub models_loading: String,
    pub models_empty: String,
    pub models_disabled: String,
    /// G-SE12: the per-model connection probe labels and its result receipt.
    pub models_test: String,
    pub models_testing: String,
    pub models_test_details: String,
    /// G-SE12: the model row editor (category, limits, reasoning presets).
    pub models_edit_title: String,
    pub models_edit_close: String,
    pub models_category: String,
    pub models_category_general_chat: String,
    pub models_category_multimodal: String,
    pub models_category_image_generation: String,
    pub models_category_speech_recognition: String,
    pub models_context_window: String,
    pub models_context_window_hint: String,
    pub models_max_tokens: String,
    pub models_max_tokens_hint: String,
    pub models_reasoning_default: String,
    pub models_reasoning_auto: String,
    pub models_reasoning_presets: String,
    pub models_reasoning_empty: String,
    /// G-SE12-3: request shaping in the model editor's advanced block.
    pub models_advanced: String,
    pub models_advanced_note: String,
    pub models_inline_think: String,
    pub models_inline_think_hint: String,
    pub models_skip_ssl: String,
    pub models_skip_ssl_warning: String,
    pub models_headers: String,
    pub models_headers_hint: String,
    pub models_headers_mode: String,
    pub models_headers_replace: String,
    pub models_headers_merge: String,
    pub models_headers_replace_hint: String,
    pub models_headers_merge_hint: String,
    pub models_body: String,
    pub models_body_hint: String,
    pub models_body_mode: String,
    pub models_body_merge: String,
    pub models_body_trim: String,
    pub models_body_trim_hint: String,
    pub models_json_invalid: String,
    /// E3-B3: the acquisition, selection-mode, all-models, more-settings, and
    /// new-model copy the web page uses (`settings/models.json`).
    pub models_acquisition_title: String,
    pub models_acquisition_api: String,
    pub models_acquisition_none: String,
    pub models_acquisition_subscriptions: String,
    pub models_subscriptions_unavailable: String,
    pub models_add: String,
    pub models_delete_provider: String,
    pub models_selection_modes_title: String,
    pub models_mode_smart: String,
    pub models_mode_smart_hint: String,
    pub models_mode_unavailable: String,
    pub models_pool_title: String,
    pub models_default_primary: String,
    pub models_default_primary_desc: String,
    pub models_default_fast: String,
    pub models_default_fast_desc: String,
    pub models_default_image: String,
    pub models_default_image_desc: String,
    pub models_default_speech: String,
    pub models_default_speech_desc: String,
    pub models_default_not_set: String,
    pub models_search: String,
    pub models_filter_all_capabilities: String,
    pub models_filter_all_providers: String,
    pub models_capability_text: String,
    pub models_capability_image: String,
    pub models_capability_speech: String,
    pub models_capability_tools: String,
    pub models_no_matches: String,
    pub models_clear_filter: String,
    pub models_delete: String,
    pub models_delete_confirm: String,
    pub models_delete_message: String,
    pub models_more: String,
    pub models_more_expand: String,
    pub models_more_collapse: String,
    pub models_editor_name: String,
    pub models_editor_provider: String,
    pub models_editor_model_name: String,
    pub models_editor_base_url: String,
    pub models_editor_api_key: String,
    pub models_editor_enabled: String,
    pub models_editor_delete: String,
    pub models_create_title: String,
    pub models_create_save: String,
    pub models_create_cancel: String,
    pub models_create_error_name: String,
    pub models_create_error_model: String,
    pub models_create_error_url: String,
    pub models_create_error_key: String,
    pub models_create_error_context: String,
    pub chat_empty: String,
    pub chat_empty_title: String,
    /// Tabless landing face (R2b): the brand line and the pinned slogan. Web
    /// truth: `welcomeScene.space.*` + `welcomeScene.phrases.create`, shown
    /// in the shell's reduced-motion form (one static phrase, no typewriter).
    pub welcome_title: String,
    pub welcome_slogan: String,
    pub chat_input_placeholder: String,
    pub chat_jump_latest: String,
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
    /// 打开项目 / 新建项目 (web MainNav workspace menu): the sessions header's
    /// FolderPlus trigger, the nav top bar button, and the composer pill all
    /// open this one menu.
    pub workspace_open_project: String,
    pub workspace_new_project: String,
    pub workspace_open_path_description: String,
    /// New-project dialog (web `NewProjectDialog`; the parent folder is typed
    /// instead of picked — see `open_workspace_path_dialog`).
    pub new_project_title: String,
    pub new_project_subtitle: String,
    pub new_project_parent: String,
    pub new_project_parent_placeholder: String,
    pub new_project_name: String,
    pub new_project_name_placeholder: String,
    pub new_project_full_path: String,
    pub new_project_create: String,
    pub new_project_error_parent: String,
    pub new_project_error_name: String,
    pub new_project_error_missing: String,
    pub new_project_error_create: String,
    pub terminal_title: String,
    pub terminal_close: String,
    pub terminal_reveal: String,
    pub notifications_title: String,
    pub notifications_empty: String,
    pub notifications_clear_all: String,
    pub terminal_mode_compact: String,
    pub terminal_mode_comfortable: String,
    pub terminal_mode_expanded: String,
    pub terminal_exited: String,
    pub settings_open: String,
    /// Settings tab title (R1a), resolved from the shared `features.settings`
    /// term (the D75 shared-term channel).
    pub settings_title: String,
    /// Top tab bar (R1a): close button tooltip.
    pub tab_close: String,
    pub settings_appearance: String,
    /// The 通用 page's section card heads (web `applicationGroups`): 界面
    /// (language) / 启动与更新 / 窗口与通知.
    pub settings_appearance_interface_title: String,
    pub settings_appearance_interface_description: String,
    pub settings_general_startup_title: String,
    pub settings_general_startup_description: String,
    pub settings_general_window_title: String,
    pub settings_general_window_description: String,
    /// The 外观 page's card heads (web `ConfigPageSection`: theme / font size).
    pub settings_appearance_font_title: String,
    /// The web-search page's card heads (web `sections.provider`/`http`).
    pub settings_websearch_provider_title: String,
    pub settings_websearch_http_title: String,
    pub settings_websearch_http_description: String,
    pub settings_permissions: String,
    pub settings_theme_system: String,
    pub settings_theme_unsupported: String,
    // G-SE17: the global permission rules editor.
    pub settings_permission_rules_section: String,
    pub settings_permission_rules_description: String,
    pub settings_permission_rules_manage: String,
    pub settings_permission_rules_add: String,
    pub settings_permission_rules_save: String,
    pub settings_permission_rules_discard: String,
    pub settings_permission_rules_empty: String,
    pub settings_permission_rules_action: String,
    pub settings_permission_rules_resource: String,
    pub settings_permission_rules_effect: String,
    pub settings_permission_rules_invalid: String,
    pub settings_permission_effect_allow: String,
    pub settings_permission_effect_ask: String,
    pub settings_permission_effect_deny: String,
    // G-SE24: quick-actions management.
    pub settings_quick_actions_section: String,
    pub settings_quick_actions_add: String,
    pub settings_quick_actions_delete: String,
    pub settings_quick_actions_delete_title: String,
    pub settings_quick_actions_delete_action: String,
    pub settings_quick_actions_label: String,
    pub settings_quick_actions_prompt: String,
    pub settings_quick_actions_save: String,
    pub settings_quick_actions_cancel: String,
    pub settings_quick_actions_invalid: String,
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
    pub git_empty_title: String,
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
    /// Per-file discard/delete row actions (G-G3) with their two-step
    /// confirm labels.
    pub git_discard: String,
    pub git_discard_confirm: String,
    pub git_delete_file: String,
    pub git_delete_confirm: String,
    pub git_commit: String,
    pub git_commit_placeholder: String,
    pub git_push: String,
    /// Branch-sync surface (G-G6): pull action plus the not-a-repository
    /// card's copy (G-G1).
    pub git_pull: String,
    pub git_not_repo_title: String,
    pub git_not_repo_detail: String,
    pub git_init_button: String,
    pub git_init_hint: String,
    pub git_init_prompt: String,
    pub git_history: String,
    pub git_files_view: String,
    pub git_no_commits: String,
    pub git_untrusted_title: String,
    pub git_untrusted_action: String,
    pub git_worktrees_view: String,
    pub git_snapshots_view: String,
    pub git_remote_unsupported: String,
    pub git_branches_view: String,
    pub git_graph_view: String,
    pub git_branch_create: String,
    pub git_branch_name_label: String,
    pub git_branch_base_label: String,
    pub git_branch_switch: String,
    pub git_branch_delete: String,
    pub git_branch_delete_confirm: String,
    pub git_branch_copy_hash: String,
    pub git_branch_current: String,
    pub git_branch_remote: String,
    pub git_branch_empty: String,
    pub git_graph_reset: String,
    pub git_graph_reset_confirm: String,
    pub git_generate_message: String,
    pub git_generate_cancel: String,
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
    pub turn_copy: String,
    pub turn_meta_completed: String,
    pub turn_meta_stopped: String,
    pub turn_meta_duration: String,
    pub turn_copy_full: String,
    pub turn_copy_result: String,
    /// Fenced-code toolbar (web `markdown.copyCode` / `markdown.copySuccess`).
    pub code_copy: String,
    pub code_copied: String,
    /// Tool-row affordances (web `tooltip.openDetails` / `tooltip.copy`).
    pub tool_open_details: String,
    pub tool_copy: String,
    pub tool_copied: String,
    pub turn_copy_user_label: String,
    pub subagent_badge: String,
    pub subagent_open: String,
    /// Subagent display names, in the web pool's order
    /// (`subagentIdentity.names.*`); the index comes from
    /// [`crate::subagent_identity::name_index`].
    pub subagent_names: Vec<String>,
    /// Transcript tool rows: the action label each tool family renders before
    /// its payload (M24 rich transcript).
    pub tool_action_command: String,
    pub tool_action_read: String,
    pub tool_action_search: String,
    pub tool_action_web: String,
    pub tool_action_write: String,
    pub tool_action_edit: String,
    pub tool_action_tool_spec: String,
    pub tool_action_tool_call: String,
    pub tool_action_agent_spawn: String,
    pub tool_action_agent_wait: String,
    pub tool_action_agent_list: String,
    pub tool_action_agent_send: String,
    pub tool_action_agent_delete: String,
    pub btw_badge: String,
    pub sidebar_review_running: String,
    pub child_back: String,
    pub manage_skills_title: String,
    pub manage_skills_loading: String,
    pub manage_skills_empty_title: String,
    pub manage_skills_empty: String,
    /// G-K5: the skills page's local filter row and its no-match state.
    pub manage_skills_search_placeholder: String,
    pub manage_skills_hide_shadowed: String,
    pub manage_skills_no_match: String,
    pub manage_skill_builtin: String,
    pub manage_skill_user: String,
    pub manage_skill_project: String,
    pub manage_skill_shadowed: String,
    /// G-K2/G-K3: the skill detail, add, and delete dialogs.
    pub manage_skill_add: String,
    pub manage_skill_detail: String,
    pub manage_skill_dialog_add_title: String,
    pub manage_skill_dialog_detail_title: String,
    pub manage_skill_dialog_delete_title: String,
    pub manage_skill_source: String,
    pub manage_skill_source_native: String,
    pub manage_skill_path: String,
    pub manage_skill_path_hint: String,
    pub manage_skill_validate: String,
    pub manage_skill_valid: String,
    pub manage_skill_invalid: String,
    pub manage_skill_runtime_unavailable: String,
    pub manage_skill_add_action: String,
    pub manage_skill_delete_action: String,
    pub manage_skill_close: String,
    pub manage_skill_not_found: String,
    pub manage_skill_no_description: String,
    /// G-K4: the user skill group manager and its editor/delete dialogs.
    pub skill_groups_title: String,
    pub skill_groups_subtitle: String,
    pub skill_groups_manage: String,
    pub skill_groups_mine: String,
    pub skill_groups_builtin: String,
    pub skill_groups_loading: String,
    pub skill_groups_empty: String,
    pub skill_groups_empty_group: String,
    pub skill_groups_unresolved: String,
    pub skill_group_new: String,
    pub skill_group_name_label: String,
    pub skill_group_name_placeholder: String,
    pub skill_group_dialog_edit_title: String,
    pub skill_group_dialog_delete_title: String,
    pub skill_group_save: String,
    pub skill_group_copy: String,
    pub skill_group_up: String,
    pub skill_group_down: String,
    pub skill_group_delete: String,
    /// G-A1/G-A2/G-A4: the agent detail dialog.
    pub agent_detail_open: String,
    pub agent_detail_missing: String,
    pub agent_detail_type: String,
    pub agent_detail_source: String,
    pub agent_detail_follow_up: String,
    pub agent_detail_supported: String,
    pub agent_detail_unsupported: String,
    pub agent_detail_capabilities: String,
    pub agent_detail_shared_profile: String,
    pub agent_detail_shared_default: String,
    pub agent_detail_tools: String,
    pub agent_detail_subagents: String,
    pub agent_detail_loading: String,
    pub agent_detail_no_subagents: String,
    pub agent_detail_save: String,
    pub agent_detail_reset: String,
    pub agent_detail_close: String,
    /// R4 facet sidebar: group titles and the market degrade card.
    pub skills_facet_library: String,
    pub skills_facet_all: String,
    pub skills_facet_groups: String,
    pub skills_facet_sources: String,
    pub skills_facet_explore: String,
    pub skills_market_title: String,
    pub skills_market_degraded: String,
    pub manage_agents_title: String,
    pub manage_agents_modes: String,
    pub manage_agents_subagents: String,
    pub manage_agents_create: String,
    pub manage_agents_loading: String,
    pub manage_agents_empty_title: String,
    pub manage_agents_empty: String,
    /// G-A5: the agents page's search box, source and type filters, and its
    /// no-match state.
    pub manage_agents_search_placeholder: String,
    pub manage_agents_source_label: String,
    pub manage_agents_source_any: String,
    pub manage_agents_kind_label: String,
    pub manage_agents_kind_any: String,
    pub manage_agents_kind_agent: String,
    pub manage_agents_kind_subagent: String,
    pub manage_agents_no_match: String,
    /// A3 tail: the harness profile zone (web `harnessZone`) and the subagent
    /// model override menu (web `agentCard.modelSelector`; inherit/fast/
    /// primary reuse the settings subagent-model keys above).
    pub manage_harness_title: String,
    pub manage_harness_subtitle: String,
    pub manage_harness_purpose_minimal: String,
    pub manage_harness_purpose_standard: String,
    pub manage_harness_purpose_ultimate: String,
    pub manage_harness_purpose_creative: String,
    pub manage_agent_model: String,
    pub manage_agent_model_default: String,
    pub manage_agent_model_failed: String,
    pub manage_agent_create_title: String,
    pub manage_agent_kind_mode: String,
    pub manage_agent_kind_subagent: String,
    pub manage_agent_level_user: String,
    pub manage_agent_level_project: String,
    pub manage_agent_id: String,
    pub manage_agent_name: String,
    pub manage_agent_description: String,
    pub manage_agent_prompt: String,
    pub manage_agent_readonly: String,
    pub manage_agent_review: String,
    pub manage_agent_enabled: String,
    pub manage_agent_disabled: String,
    pub manage_agent_source_builtin: String,
    pub manage_agent_source_user: String,
    pub manage_agent_source_project: String,
    pub manage_agent_source_external: String,
    pub manage_agent_error_empty_id: String,
    pub manage_agent_error_bad_id: String,
    pub manage_agent_error_empty_name: String,
    pub manage_agent_error_empty_description: String,
    pub manage_agent_error_empty_prompt: String,
    pub manage_agent_error_mode_project: String,
    pub manage_agent_edit_title: String,
    pub manage_agent_delete: String,
    pub manage_agent_delete_title: String,
    pub agent_card_edit: String,
    pub mode_skills_open: String,
    pub mode_skills_title: String,
    pub mode_skills_loading: String,
    pub mode_skills_empty: String,
    pub mode_skills_edit: String,
    pub mode_skills_save_hint: String,
    pub mode_skills_status_selected: String,
    pub mode_skills_status_enabled: String,
    pub mode_skills_reset: String,
    pub mode_skills_source_other: String,
    pub manage_mcp_title: String,
    pub manage_mcp_loading: String,
    pub manage_mcp_empty_title: String,
    pub manage_mcp_empty: String,
    pub manage_mcp_start: String,
    pub manage_mcp_stop: String,
    pub manage_mcp_restart: String,
    pub manage_mcp_enable: String,
    pub manage_mcp_disable: String,
    pub manage_mcp_delete: String,
    pub manage_mcp_delete_title: String,
    pub manage_mcp_delete_warning: String,
    pub manage_mcp_browse: String,
    pub manage_mcp_browse_title: String,
    pub manage_mcp_resources: String,
    pub manage_mcp_prompts: String,
    pub manage_mcp_refresh: String,
    pub manage_mcp_disabled: String,
    pub manage_mcp_location_builtin: String,
    pub manage_mcp_state_uninitialized: String,
    pub manage_mcp_state_starting: String,
    pub manage_mcp_state_connected: String,
    pub manage_mcp_state_healthy: String,
    pub manage_mcp_state_needs_auth: String,
    pub manage_mcp_state_reconnecting: String,
    pub manage_mcp_state_failed: String,
    pub manage_mcp_state_stopping: String,
    pub manage_mcp_state_stopped: String,
    pub manage_interaction_title: String,
    pub manage_interaction_params: String,
    pub manage_interaction_result_hint: String,
    pub manage_interaction_approve: String,
    pub manage_interaction_reject: String,
    pub manage_mcp_json_item: String,
    pub manage_mcp_json_edit: String,
    pub manage_mcp_json_loading: String,
    pub manage_mcp_json_save: String,
    pub manage_mcp_json_saved: String,
    /// G-M2: the paste/file import dialog (web `MCPServerConfigDialog` import
    /// mode; the shell reads a typed path instead of the native picker).
    pub manage_mcp_import: String,
    pub manage_mcp_import_paste: String,
    pub manage_mcp_import_path: String,
    pub manage_mcp_import_read: String,
    pub manage_mcp_import_file_failed: String,
    pub manage_mcp_import_preview: String,
    pub manage_mcp_import_hint: String,
    pub manage_mcp_import_selected: String,
    pub manage_mcp_import_conflict: String,
    pub manage_mcp_import_select_servers: String,
    pub manage_mcp_import_invalid_json: String,
    pub manage_mcp_import_invalid_config: String,
    pub manage_mcp_import_id_required: String,
    pub manage_mcp_import_invalid_id: String,
    pub manage_mcp_import_id_conflict: String,
    pub manage_mcp_import_shape: String,
    pub manage_mcp_resource_hint: String,
    pub manage_mcp_resource_loading: String,
    pub manage_mcp_resource_binary: String,
    pub manage_mcp_resource_empty: String,
    pub manage_mcp_resource_failed: String,
    /// G-M1/G-M3: the remote auth dialog and the row auth summary (web
    /// `McpToolsConfig` auth dialog + `getRemoteAuthSummary`).
    pub manage_mcp_auth: String,
    pub manage_mcp_auth_oauth_hint: String,
    pub manage_mcp_oauth_idle: String,
    pub manage_mcp_oauth_awaiting_browser: String,
    pub manage_mcp_oauth_awaiting_callback: String,
    pub manage_mcp_oauth_exchanging_token: String,
    pub manage_mcp_oauth_authorized: String,
    pub manage_mcp_oauth_failed: String,
    pub manage_mcp_oauth_cancelled: String,
    pub manage_mcp_oauth_start: String,
    pub manage_mcp_oauth_restart: String,
    pub manage_mcp_oauth_reconnect: String,
    pub manage_mcp_auth_token_hint: String,
    pub manage_mcp_auth_secret_hint: String,
    pub manage_mcp_auth_token_label: String,
    pub manage_mcp_auth_clear: String,
    pub manage_mcp_auth_cancel_flow: String,
    pub manage_mcp_auth_save: String,
    pub manage_mcp_auth_required: String,
    pub manage_mcp_auth_saved: String,
    pub manage_mcp_auth_configured: String,
    pub manage_mcp_auth_oauth_ready: String,
    pub manage_mcp_auth_missing: String,
    pub manage_mcp_auth_source_headers: String,
    pub manage_mcp_auth_source_env: String,
    pub manage_mcp_auth_source_oauth: String,
    pub manage_error_dismiss: String,
    // M22 general page (tray/autostart/updates) and the new settings pages.
    pub value_unsupported: String,
    pub settings_general: String,
    pub settings_version: String,
    pub settings_launch_login: String,
    pub settings_launch_login_description: String,
    pub settings_close_behavior: String,
    pub settings_close_behavior_description: String,
    pub settings_close_quit: String,
    pub settings_close_minimize: String,
    pub settings_auto_update: String,
    pub settings_auto_update_description: String,
    pub settings_notify_turn_done: String,
    pub settings_notify_turn_done_description: String,
    pub settings_prevent_sleep: String,
    pub settings_prevent_sleep_description: String,
    pub settings_check_updates: String,
    pub settings_update_checking: String,
    pub settings_up_to_date: String,
    pub settings_update_available: String,
    pub settings_update_install: String,
    pub settings_update_installing: String,
    pub settings_update_installed: String,
    pub settings_update_restart: String,
    pub settings_update_unsupported: String,
    pub settings_update_failed: String,
    pub settings_automation: String,
    pub settings_hooks: String,
    pub settings_hooks_description: String,
    pub settings_hooks_project: String,
    pub settings_hooks_project_description: String,
    pub settings_project_hooks_risk_title: String,
    pub settings_project_hooks_risk_confirm: String,
    pub settings_project_hooks_risk_current_workspace: String,
    /// G-SE25: Hook declaration locations, the imported-source manager and the
    /// Codex contract reference on the automation page.
    pub settings_hooks_user_file: String,
    pub settings_hooks_user_file_desc: String,
    pub settings_hooks_project_file: String,
    pub settings_hooks_project_file_desc: String,
    pub settings_hooks_imports_master_disabled: String,
    pub settings_hooks_imports_refresh: String,
    pub settings_hooks_imports_loading: String,
    pub settings_hooks_imports_empty: String,
    pub settings_hooks_imports_update: String,
    pub settings_hooks_imports_remove: String,
    pub settings_hook_import_state_current: String,
    pub settings_hook_import_state_update_available: String,
    pub settings_hook_import_state_source_missing: String,
    pub settings_hook_import_state_update_check_failed: String,
    pub settings_hook_import_state_bundle_missing: String,
    pub settings_hooks_import_review_title: String,
    pub settings_hooks_import_review_warning: String,
    pub settings_hooks_import_timeout: String,
    pub settings_hooks_import_skipped: String,
    pub settings_hooks_import_toggle: String,
    pub settings_hooks_import_confirm: String,
    pub settings_hooks_import_confirm_update: String,
    pub settings_hooks_import_stale: String,
    pub settings_hooks_import_remove_title: String,
    pub settings_hooks_import_remove_confirm_note: String,
    pub settings_hooks_import_remove_confirm: String,
    pub settings_hooks_import_store_invalid: String,
    pub settings_hooks_import_reset: String,
    pub settings_hooks_import_reset_title: String,
    pub settings_hooks_import_reset_note: String,
    pub settings_hooks_import_reset_confirm: String,
    pub settings_hooks_compat_reference: String,
    pub settings_hooks_compat_open: String,
    /// The automation page's card heads (web `quick-actions` + `hooks`
    /// section titles and descriptions).
    pub settings_automation_hooks_title: String,
    pub settings_automation_hooks_desc: String,
    pub settings_automation_qa_builtin_title: String,
    pub settings_automation_qa_builtin_desc: String,
    pub settings_automation_qa_custom_title: String,
    pub settings_automation_qa_custom_desc: String,
    pub settings_automation_hook_locations_title: String,
    pub settings_automation_hook_locations_desc: String,
    pub settings_automation_hook_imports_title: String,
    pub settings_automation_hook_imports_desc: String,
    pub settings_automation_hooks_compat_title: String,
    pub settings_automation_hooks_compat_desc: String,
    /// The MCP page's card head (web `McpToolsConfig` section).
    pub settings_mcp_servers_title: String,
    pub settings_mcp_servers_desc: String,
    pub settings_websearch: String,
    pub settings_websearch_provider: String,
    pub settings_websearch_provider_description: String,
    pub settings_category_application: String,
    pub settings_category_ai: String,
    pub settings_category_workspace: String,
    pub settings_category_tools: String,
    pub settings_category_data: String,
    /// Settings page header subtitles (web `navigation.pages.<id>.description`),
    /// shown under the page title; the nav search field copy rides along.
    pub settings_nav_search_placeholder: String,
    pub settings_nav_search_empty: String,
    pub settings_page_general_description: String,
    pub settings_page_appearance_description: String,
    pub settings_page_terminal_description: String,
    pub settings_page_models_description: String,
    pub settings_page_session_description: String,
    /// The Workspace & Git page's nav label (web
    /// `navigation.pages.workspaceGit.label`; the page title and the nav row
    /// share it).
    pub settings_worktrees_page_label: String,
    pub settings_page_worktrees_description: String,
    pub settings_page_execution_description: String,
    pub settings_page_automation_description: String,
    pub settings_page_websearch_description: String,
    pub settings_page_skills_description: String,
    pub settings_page_agents_description: String,
    pub settings_page_mcp_description: String,
    pub settings_acp_label: String,
    pub settings_page_acp_description: String,
    pub settings_page_usage_description: String,
    pub settings_page_archived_description: String,
    pub settings_page_diagnostics_description: String,
    pub acp_view_local: String,
    pub acp_view_json: String,
    pub acp_registry_title: String,
    pub acp_registry_description: String,
    pub acp_action_refresh: String,
    pub acp_search_placeholder: String,
    pub acp_filter_all: String,
    pub acp_filter_enabled: String,
    pub acp_filter_not_installed: String,
    pub acp_filter_invalid: String,
    pub acp_loading: String,
    pub acp_empty_title: String,
    pub acp_empty: String,
    pub acp_status_enabled: String,
    pub acp_status_ready: String,
    pub acp_status_partial: String,
    pub acp_status_checking: String,
    pub acp_status_cli_missing: String,
    pub acp_status_acp_missing: String,
    pub acp_status_path_invalid: String,
    pub acp_status_version_mismatch: String,
    pub acp_status_connection_failed: String,
    pub acp_status_permission_denied: String,
    pub acp_status_config_invalid: String,
    pub acp_permission_ask: String,
    pub acp_permission_allow_once: String,
    pub acp_action_install_cli: String,
    pub acp_action_configure: String,
    pub acp_action_view_instructions: String,
    pub acp_action_view_error: String,
    pub acp_action_add: String,
    pub acp_action_add_config: String,
    pub acp_cli_missing_detail: String,
    pub acp_json_title: String,
    pub acp_json_description: String,
    pub acp_json_warning: String,
    pub acp_action_revert: String,
    pub acp_action_save_json: String,
    pub acp_notice_save_success: String,
    pub acp_notice_install_success: String,
    pub acp_notice_configure_success: String,
    pub acp_notice_config_added: String,
    pub acp_preset_opencode: String,
    pub acp_preset_dsh: String,
    pub acp_preset_omp: String,
    pub acp_preset_claude_code: String,
    pub acp_preset_codex: String,
    pub acp_docs: String,
    pub acp_notice_open_link_failed: String,
    pub acp_legacy_reject_warning: String,
    pub acp_permission_save_apply: String,
    pub acp_action_cancel: String,
    pub acp_install_confirm_local_host: String,
    pub acp_install_confirm_confirm: String,
    pub acp_install_confirm_package_unknown: String,
    pub settings_jump_session_label: String,
    pub settings_jump_session_description: String,
    pub settings_jump_session_action: String,
    pub websearch_provider_free: String,
    pub websearch_provider_exa: String,
    pub websearch_provider_tavily: String,
    pub websearch_provider_http: String,
    pub websearch_http_endpoint: String,
    pub websearch_http_endpoint_description: String,
    pub websearch_http_auth_mode: String,
    pub websearch_auth_none: String,
    pub websearch_auth_bearer: String,
    pub websearch_auth_header: String,
    pub websearch_http_header_name: String,
    pub websearch_http_header_name_description: String,
    pub websearch_credential: String,
    pub websearch_credential_description: String,
    pub websearch_configured: String,
    pub websearch_not_configured: String,
    pub websearch_save: String,
    pub websearch_no_credential: String,
    pub quick_action_title: String,
    pub quick_action_commit_label: String,
    pub quick_action_commit_prompt: String,
    pub quick_action_create_pr_label: String,
    pub quick_action_create_pr_prompt: String,
    pub settings_usage: String,
    pub settings_usage_today: String,
    pub settings_usage_week: String,
    pub settings_usage_month: String,
    pub settings_usage_all: String,
    pub settings_usage_refresh: String,
    pub settings_usage_loading: String,
    pub settings_usage_empty: String,
    pub settings_usage_requests: String,
    pub settings_usage_cache_hit: String,
    /// G-SE29: distribution blocks and their page-local filter.
    pub settings_usage_groups: String,
    pub settings_usage_endpoints: String,
    pub settings_usage_filter: String,
    pub settings_usage_filter_empty: String,
    pub usage_trend_title: String,
    pub usage_trend_partial: String,
    pub usage_trend_input: String,
    pub usage_trend_output: String,
    pub usage_activity_title: String,
    pub usage_activity_description: String,
    pub settings_usage_overview_title: String,
    pub settings_usage_distributions_title: String,
    pub settings_usage_details_title: String,
    pub usage_activity_less: String,
    pub usage_activity_more: String,
    pub usage_activity_mon: String,
    pub usage_activity_wed: String,
    pub usage_activity_fri: String,
    pub usage_activity_sun: String,
    pub settings_archived: String,
    pub settings_archived_list: String,
    pub settings_archived_description: String,
    pub settings_archived_empty: String,
    pub settings_archived_loading: String,
    pub settings_archived_refresh: String,
    pub settings_archived_restore: String,
    pub settings_archived_delete_title: String,
    pub settings_archived_delete_all: String,
    pub settings_archived_delete_all_confirm_title: String,
    pub settings_diagnostics: String,
    pub settings_logging_level: String,
    pub settings_logging_level_description: String,
    pub settings_logs_dir: String,
    pub settings_logs_open: String,
    pub settings_diagnostics_note: String,
    pub settings_diagnostics_export: String,
    pub settings_diagnostics_export_description: String,
    pub settings_diagnostics_export_action: String,
    pub settings_diagnostics_export_title: String,
    pub settings_diagnostics_export_warning: String,
    pub settings_diagnostics_export_confirm: String,
    pub settings_diagnostics_export_success: String,
    pub settings_diagnostics_export_failed: String,
    /// G-SE31: the previous-run notice on the diagnostics page. The category
    /// picks the pair (a written crash report vs. a run state left unclean);
    /// both descriptions name the session log directory.
    pub settings_diagnostics_previous_crash: String,
    pub settings_diagnostics_previous_unclean: String,
    /// G-SE13 memory page (`ai.memory`): nav label, page description,
    /// section headings, copy, and receipts, copied verbatim from the
    /// desktop settings catalog (`settings/memory.json`).
    pub settings_memory_label: String,
    pub settings_page_memory_description: String,
    pub memory_scope_notice: String,
    pub memory_section_basic: String,
    pub memory_section_basic_note: String,
    pub memory_section_models: String,
    pub memory_section_models_note: String,
    pub memory_section_advanced: String,
    pub memory_section_advanced_note: String,
    pub memory_enabled: String,
    pub memory_enabled_description: String,
    pub memory_generate_btw: String,
    pub memory_generate_btw_description: String,
    pub memory_external_context: String,
    pub memory_external_context_description: String,
    pub memory_policy_clear_tool_results: String,
    pub memory_policy_allow: String,
    pub memory_policy_skip_session: String,
    pub memory_actions_label: String,
    pub memory_actions_description: String,
    pub memory_action_manage: String,
    pub memory_action_open_directory: String,
    pub memory_action_reset_settings: String,
    pub memory_action_reset_memory: String,
    pub memory_reset_settings_confirm_title: String,
    pub memory_reset_settings_confirm_message: String,
    pub memory_reset_settings_confirm_action: String,
    pub memory_reset_confirm_message: String,
    pub memory_reset_confirm_action: String,
    pub memory_extract_model: String,
    pub memory_extract_model_description: String,
    pub memory_consolidation_model: String,
    pub memory_consolidation_model_description: String,
    pub memory_model_primary: String,
    pub memory_model_fast: String,
    pub memory_model_follow_primary: String,
    pub memory_model_follow_extraction: String,
    pub memory_min_idle_hours: String,
    pub memory_min_idle_hours_description: String,
    pub memory_max_age_days: String,
    pub memory_max_age_days_description: String,
    pub memory_per_startup: String,
    pub memory_per_startup_description: String,
    pub memory_scan_limit: String,
    pub memory_scan_limit_description: String,
    pub memory_concurrency: String,
    pub memory_concurrency_description: String,
    pub memory_raw_limit: String,
    pub memory_raw_limit_description: String,
    pub memory_retention_days: String,
    pub memory_retention_days_description: String,
    pub memory_action_expand_advanced: String,
    pub memory_action_collapse_advanced: String,
    pub memory_settings_reset: String,
    pub memory_settings_reset_failed: String,
    pub memory_open_directory_failed: String,
    pub memory_reset: String,
    pub memory_reset_failed: String,
    /// G-SE7 shortcuts page (`application.shortcuts`): the read-only table
    /// of the bindings the shell honors. Section titles copy the desktop
    /// `keyboard.scopes` where the scope exists; the rest is shell-authored
    pub settings_shortcuts_label: String,
    pub settings_page_shortcuts_description: String,
    pub shortcuts_fixed_note: String,
    pub shortcuts_section_global: String,
    pub shortcuts_section_overlays: String,
    pub shortcuts_section_terminal: String,
    pub shortcuts_section_editing: String,
    pub shortcuts_overlay_dismiss: String,
    pub shortcuts_overlay_move: String,
    pub shortcuts_overlay_confirm: String,
    pub shortcuts_terminal_copy: String,
    pub shortcuts_terminal_paste: String,
    pub shortcuts_terminal_note: String,
    pub shortcuts_editing_note: String,
    /// R7b plugin scene diagnostics: retraction face, rejection banner, and
    /// the round-trip timeout banner.
    pub plugin_ui_removed: String,
    pub plugin_ui_rejected: String,
    pub plugin_ui_timeout: String,
    /// A2 (settings small fields, G-SE5/G-SE10/G-SE11/G-SE18..G-SE22): the
    /// appearance, model, and execution-page copy.
    pub settings_ui_font: String,
    pub settings_ui_font_description: String,
    pub ui_font_level_compact: String,
    pub ui_font_level_small: String,
    pub ui_font_level_default: String,
    pub ui_font_level_medium: String,
    pub ui_font_level_large: String,
    pub ui_font_level_custom: String,
    pub settings_ui_font_custom: String,
    pub settings_ui_font_custom_description: String,
    pub settings_stream_section: String,
    pub settings_stream_section_note: String,
    pub settings_stream_ttft: String,
    pub settings_stream_ttft_description: String,
    pub settings_stream_idle: String,
    pub settings_stream_idle_description: String,
    pub settings_proxy_section: String,
    pub settings_proxy_section_note: String,
    pub settings_proxy_enable: String,
    pub settings_proxy_enable_description: String,
    pub settings_proxy_url: String,
    pub settings_proxy_url_description: String,
    pub settings_proxy_username: String,
    pub settings_proxy_password: String,
    pub settings_proxy_password_description: String,
    pub settings_tool_timeout: String,
    pub settings_tool_timeout_description: String,
    pub settings_question_timeout: String,
    pub settings_question_timeout_description: String,
    pub settings_subagent_model: String,
    pub settings_subagent_model_description: String,
    pub settings_subagent_model_settings: String,
    pub settings_subagent_model_inherit: String,
    pub settings_subagent_model_fast: String,
    pub settings_subagent_model_primary: String,
    pub settings_subagent_batch: String,
    pub settings_subagent_batch_safe: String,
    pub settings_subagent_batch_safe_description: String,
    pub settings_subagent_batch_parallel: String,
    pub settings_subagent_batch_parallel_description: String,
    pub settings_subagent_concurrency: String,
    pub settings_subagent_concurrency_description: String,
    pub settings_swarm_concurrency: String,
    pub settings_swarm_concurrency_description: String,
    pub settings_deferred_tools_enable: String,
    pub settings_deferred_tools_warning: String,
    pub settings_permissions_policy_title: String,
    pub settings_permissions_policy_desc: String,
    pub settings_permissions_deferred_title: String,
    pub settings_permissions_deferred_desc: String,
    pub settings_permissions_tool_presentation_title: String,
    pub settings_permissions_tool_presentation_desc: String,
    pub settings_tool_presentation_mode: String,
    pub settings_tool_presentation_description: String,
    pub settings_tool_presentation_native: String,
    pub settings_tool_presentation_ptc: String,
    pub settings_tool_presentation_ptc_hint: String,
    pub settings_permissions_mode: String,
    pub settings_permissions_mode_description: String,
    pub settings_permissions_mode_ask: String,
    pub settings_permissions_mode_ask_description: String,
    pub settings_permissions_mode_auto: String,
    pub settings_permissions_mode_auto_description: String,
    pub settings_permissions_mode_full: String,
    pub settings_permissions_mode_full_description: String,
    pub settings_permissions_show_in_chat_input: String,
    pub settings_permissions_show_in_chat_input_description: String,
    pub settings_permissions_full_access_title: String,
    pub settings_permissions_full_access_message: String,
    pub settings_permissions_full_access_confirm: String,
    pub settings_permissions_execution_title: String,
    pub settings_permissions_execution_desc: String,
    pub settings_permissions_subagents_title: String,
    pub settings_permissions_subagents_desc: String,
    pub settings_permissions_json_repair_title: String,
    pub settings_permissions_json_repair_desc: String,
    pub settings_permissions_review_title: String,
    pub settings_permissions_review_desc: String,
    pub settings_json_repair: String,
    pub settings_json_repair_description: String,
    pub settings_review_parallelism: String,
    pub settings_review_parallelism_description: String,
    pub settings_review_queue_wait: String,
    pub settings_review_queue_wait_description: String,
    // G-SE15: default Harness preference and session-title generation.
    pub settings_default_harness_section: String,
    pub settings_default_harness_section_note: String,
    pub settings_default_harness_strategy: String,
    pub settings_default_harness_follow_last: String,
    pub settings_default_harness_fixed: String,
    pub settings_default_harness_fixed_description: String,
    pub settings_default_harness_fixed_mode: String,
    pub settings_session_title_section: String,
    pub settings_session_title_section_note: String,
    pub settings_session_title_enable: String,
    pub settings_session_title_model: String,
    pub settings_session_title_model_inherit: String,
    pub settings_session_title_model_fast: String,
    pub settings_session_title_model_primary: String,
    pub settings_session_title_models_empty: String,
    pub settings_session_title_model_unavailable: String,
    /// G-SE15: the suffix the fixed-Harness menu puts on a stored id this
    /// build's Harness list does not own (web `defaultHarness.compatibilityMode`).
    pub value_compatibility: String,
    /// G-SE16: managed-worktree isolation defaults.
    pub settings_worktree_section: String,
    pub settings_worktree_section_note: String,
    pub settings_worktree_root: String,
    pub settings_worktree_root_description: String,
    pub settings_worktree_branch_prefix: String,
    pub settings_worktree_branch_prefix_description: String,
    pub settings_worktree_copy_changes: String,
    pub settings_worktree_copy_changes_description: String,
    pub settings_worktree_auto_delete: String,
    pub settings_worktree_auto_delete_description: String,
    pub settings_worktree_auto_delete_limit: String,
    pub settings_worktree_auto_delete_limit_description: String,
    /// E3-B2: the 工作区与 Git page's remaining web sections — accelerated
    /// workspace search, commit attribution, the isolation reset action, and
    /// the host's project/worktree catalog.
    pub settings_workspace_search_title: String,
    pub settings_workspace_search_description: String,
    pub settings_workspace_search_enable: String,
    pub settings_git_attribution_title: String,
    pub settings_git_attribution_scope: String,
    pub settings_git_attribution_label: String,
    pub settings_worktree_reset: String,
    pub settings_worktree_management_title: String,
    pub settings_worktree_management_description: String,
    pub settings_worktree_management_refresh: String,
    pub settings_worktree_management_loading: String,
    pub settings_worktree_management_load_failed: String,
    pub settings_worktree_management_retry: String,
    pub settings_worktree_management_empty: String,
    pub settings_worktree_lifecycle_permanent: String,
    pub settings_worktree_lifecycle_external: String,
    pub settings_worktree_state_dirty: String,
    pub settings_worktree_state_unpublished: String,
    pub settings_worktree_state_locked: String,
    pub settings_worktree_state_missing: String,
    pub settings_worktree_delete_action: String,
    pub settings_worktree_delete_title: String,
    pub settings_worktree_delete_message: String,
    pub settings_worktree_delete_force_title: String,
    pub settings_worktree_delete_force_message: String,
    pub settings_worktree_delete_message_sessions: String,
    pub settings_worktree_delete_force_message_sessions: String,
    pub settings_worktree_delete_force_action: String,
    pub settings_worktree_delete_cancel: String,
    pub settings_worktree_protection_locked: String,
    pub settings_worktree_protection_missing: String,
    pub settings_worktree_error_locked: String,
    pub settings_worktree_error_dirty: String,
    pub settings_worktree_error_unpublished: String,
    pub settings_worktree_error_not_found: String,
    pub settings_worktree_error_remote_unsupported: String,
    pub settings_worktree_error_delete_failed: String,
}

impl Global for ShellText {}

pub fn init(cx: &mut App) {
    let service = cx
        .foreground_executor()
        .block_on(initialize_global_i18n_service(None));
    match service {
        Ok(service) => {
            // The shell catalog is compiled in beside the app, not shipped in
            // the core locale registry; register it for every supported
            // locale so later switches resolve without re-registering.
            for locale in [LocaleId::ZhCN, LocaleId::ZhTW, LocaleId::EnUS] {
                let source = match locale.as_str() {
                    "zh-CN" => include_str!("../../../locales/zh-CN.ftl"),
                    "zh-TW" => include_str!("../../../locales/zh-TW.ftl"),
                    _ => include_str!("../../../locales/en-US.ftl"),
                };
                if let Err(error) = cx
                    .foreground_executor()
                    .block_on(service.register_locale_resource(locale, source))
                {
                    tracing::error!(
                        "Failed to register shell ftl for {}: {}",
                        locale.as_str(),
                        error
                    );
                }
            }
        }
        Err(err) => {
            tracing::error!("Failed to initialize i18n service: {}", err);
            return;
        }
    }
    load_locale(locale_from_env(), cx);
}

pub fn load_locale(locale: LocaleId, cx: &mut App) {
    let text = fetch_text(cx, locale);
    cx.set_global(text);
    // The component library keeps its own copy (settings page navigation,
    // list search placeholders, dialog defaults); point it at the same locale.
    gpui_kit::component::set_locale(component_locale(locale));
    cx.refresh_windows();
    tracing::info!("Shell locale loaded: {}", locale.as_str());
    // The tray labels are resolved through the same service; the shell hooks
    // its refresh here so the menu follows the switch (M22) without a
    // foundation -> shell dependency.
    if let Some(hook) = locale_loaded_hook() {
        hook(locale, cx);
    }
}

/// Maps the shell locale onto the component library's locale ids.
fn component_locale(locale: LocaleId) -> &'static str {
    match locale {
        LocaleId::ZhCN => "zh-CN",
        LocaleId::ZhTW => "zh-TW",
        LocaleId::EnUS => "en",
    }
}

pub fn current(cx: &App) -> ShellText {
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
pub fn failed_status(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-status-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// G-G6: the ahead/behind badge tooltips of the branch chip.
pub fn git_ahead(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-ahead",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn git_behind(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-behind",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-SE31: the diagnostics page's previous-run description for a crashed run.
pub fn diagnostics_previous_crash_logs(path: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-diagnostics-previous-crash-description",
        TranslationArgs::new().with_string("path", path),
        cx,
    )
}

/// G-SE31: the same line for a run that never recorded a clean shutdown.
pub fn diagnostics_previous_unclean_logs(path: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-diagnostics-previous-unclean-description",
        TranslationArgs::new().with_string("path", path),
        cx,
    )
}

/// G-I6: the history card's session count (web `analyzed / total`).
pub fn insights_sessions_ratio(analyzed: usize, total: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-sessions-ratio",
        TranslationArgs::new()
            .with_number("analyzed", analyzed as f64)
            .with_number("total", total as f64),
        cx,
    )
}

/// G-I6: the history card's message count.
pub fn insights_messages_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-messages-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I6: the history card's tracked hours (already formatted, so the locale
/// keeps the number formatting the page chose).
pub fn insights_hours(hours: String, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-hours",
        TranslationArgs::new().with_string("hours", &hours),
        cx,
    )
}

/// G-I6: the history card's covered-day count.
pub fn insights_days_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-days-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I6: the history card's generation-token line.
pub fn insights_generation_tokens(count: u64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-generation-tokens",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I1: one chip of the range control (web `7 Days`).
pub fn insights_range_days(count: u32, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-range-days",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-P3: the inactive partition's subheader with its job count (web
/// `inactive.title`).
pub fn tasks_inactive_title(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-inactive-title",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// E4-b2: the job list's count title (web `list.countTitle`).
pub fn tasks_list_count_title(total: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-list-count-title",
        TranslationArgs::new().with_number("count", total as f64),
        cx,
    )
}

/// E4-b2: a calendar cell's "+N" overflow line (web `calendar.moreCount`).
pub fn tasks_cal_more(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-cal-more",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// E4-b2: the relative countdown lines (web `relative.*`).
pub fn tasks_relative_overdue(cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-relative-overdue",
        TranslationArgs::new(),
        cx,
    )
}

pub fn tasks_relative_now(cx: &App) -> String {
    resolve_with("gpui-shell-tasks-relative-now", TranslationArgs::new(), cx)
}

pub fn tasks_relative_in_minutes(value: i64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-relative-in-minutes",
        TranslationArgs::new().with_number("value", value as f64),
        cx,
    )
}

pub fn tasks_relative_in_hours(value: i64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-relative-in-hours",
        TranslationArgs::new().with_number("value", value as f64),
        cx,
    )
}

pub fn tasks_relative_in_one_day(cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-relative-in-one-day",
        TranslationArgs::new(),
        cx,
    )
}

pub fn tasks_relative_in_days(value: i64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-relative-in-days",
        TranslationArgs::new().with_number("value", value as f64),
        cx,
    )
}

/// E4-b2: the localized schedule summaries (web `schedule.*`).
pub fn tasks_schedule_once(at: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-schedule-once",
        TranslationArgs::new().with_string("at", at),
        cx,
    )
}

pub fn tasks_schedule_cron(expr: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-schedule-cron",
        TranslationArgs::new().with_string("expr", expr),
        cx,
    )
}

pub fn tasks_schedule_cron_tz(expr: &str, tz: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-schedule-cron-tz",
        TranslationArgs::new()
            .with_string("expr", expr)
            .with_string("tz", tz),
        cx,
    )
}

pub fn tasks_schedule_every_minutes(value: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-schedule-every-minutes",
        TranslationArgs::new().with_string("value", value),
        cx,
    )
}

pub fn tasks_schedule_every_hours(value: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-schedule-every-hours",
        TranslationArgs::new().with_string("value", value),
        cx,
    )
}

pub fn tasks_schedule_every_days(value: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-schedule-every-days",
        TranslationArgs::new().with_string("value", value),
        cx,
    )
}

/// E4-b2: the row's launch target (web `target.newSession`); the existing
/// session reading reuses the editor's field label.
pub fn tasks_target_new_session(agent_type: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-target-new-session",
        TranslationArgs::new().with_string("agent_type", agent_type),
        cx,
    )
}

/// E4-b2: the delete confirmation naming its job (web `deleteDialog.title`).
pub fn tasks_delete_confirm_title(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-delete-confirm-title",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// G-A6: the localized name of a builtin agent (web `agentName` over the
/// shared `shared:agents.*` terms).
pub fn shared_agent_name(id: &str, cx: &App) -> String {
    resolve_with(&format!("shared.agents.{id}"), TranslationArgs::new(), cx)
}

/// Resolves one shared term (`shared.<path>`, e.g. `tools.edit`) so a shell
/// affordance can reuse the contract copy instead of introducing a duplicate.
pub fn shared_term(path: &str, cx: &App) -> String {
    resolve_with(&format!("shared.{path}"), TranslationArgs::new(), cx)
}

pub fn skill_validation_failed(detail: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-skill-validation-failed",
        TranslationArgs::new().with_string("detail", detail.to_string()),
        cx,
    )
}

pub fn skill_delete_warning(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-skill-delete-warning",
        TranslationArgs::new().with_string("name", name.to_string()),
        cx,
    )
}

pub fn group_member_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-skill-groups-member-count",
        TranslationArgs::new().with_string("count", count.to_string()),
        cx,
    )
}

pub fn group_unavailable_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-skill-groups-unavailable-count",
        TranslationArgs::new().with_string("count", count.to_string()),
        cx,
    )
}

pub fn group_copy_name(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-skill-groups-copy-name",
        TranslationArgs::new().with_string("name", name.to_string()),
        cx,
    )
}

pub fn group_copy_name_numbered(name: &str, count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-skill-groups-copy-name-numbered",
        TranslationArgs::new()
            .with_string("name", name.to_string())
            .with_string("count", count.to_string()),
        cx,
    )
}

pub fn group_delete_warning(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-skill-groups-delete-warning",
        TranslationArgs::new().with_string("name", name.to_string()),
        cx,
    )
}

/// G-M1/G-M3: the remote auth dialog's parametrized lines (web
/// `modal.remoteAuthTitle` / `remoteOAuthCurrentStatus` /
/// `remoteOAuthRedirectUri` / `remoteAuthServerUrl`).
pub fn mcp_auth_dialog_title(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-mcp-auth-title",
        TranslationArgs::new().with_string("name", name.to_string()),
        cx,
    )
}

pub fn mcp_auth_status_line(status: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-mcp-auth-status-line",
        TranslationArgs::new().with_string("status", status.to_string()),
        cx,
    )
}

pub fn mcp_auth_redirect_line(uri: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-mcp-auth-redirect-line",
        TranslationArgs::new().with_string("uri", uri.to_string()),
        cx,
    )
}

pub fn mcp_auth_server_url_line(url: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-mcp-auth-server-url-line",
        TranslationArgs::new().with_string("url", url.to_string()),
        cx,
    )
}

pub fn group_operation_failed(detail: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-skill-groups-operation-failed",
        TranslationArgs::new().with_string("detail", detail.to_string()),
        cx,
    )
}

pub fn agent_external_provider(source: &str, provider: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-agent-external-provider",
        TranslationArgs::new()
            .with_string("source", source.to_string())
            .with_string("provider", provider.to_string()),
        cx,
    )
}

pub fn agent_shared_profile_description(modes: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-agent-shared-profile-description",
        TranslationArgs::new().with_string("modes", modes.to_string()),
        cx,
    )
}

pub fn agent_tools_selected(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-agent-tools-selected",
        TranslationArgs::new().with_string("count", count.to_string()),
        cx,
    )
}

pub fn agent_subagents_selected(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-agent-subagents-selected",
        TranslationArgs::new().with_string("count", count.to_string()),
        cx,
    )
}

/// G-SE12: the verdict line of a finished connection test, with the wall
/// clock the runtime measured. A real network probe always measures one, so
/// the shell always shows it (the web omits a zero).
pub fn model_test_result_label(success: bool, response_time_ms: u64, cx: &App) -> String {
    resolve_with(
        if success {
            "gpui-shell-models-test-success"
        } else {
            "gpui-shell-models-test-failed"
        },
        TranslationArgs::new().with_string("ms", &response_time_ms.to_string()),
        cx,
    )
}

/// G-SE12: the setup failure line — no probe ran, so there is no wall clock
/// to show.
pub fn model_test_error_label(cx: &App) -> String {
    resolve_with("gpui-shell-models-test-error", TranslationArgs::new(), cx)
}

/// E3-B3: the acquisition row's configured-services summary (web
/// `acquisition.configuredSummary`).
pub fn models_acquisition_summary(providers: usize, models: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-models-acquisition-summary",
        TranslationArgs::new()
            .with_string("providers", providers.to_string())
            .with_string("models", models.to_string()),
        cx,
    )
}

/// E3-B3: the provider-delete confirmation title (the web's danger dialog).
pub fn models_delete_provider_title(provider: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-models-delete-provider-title",
        TranslationArgs::new().with_string("provider", provider),
        cx,
    )
}

/// E3-B3: how many model rows the provider delete removes.
pub fn models_delete_provider_message(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-models-delete-provider-message",
        TranslationArgs::new().with_string("count", count.to_string()),
        cx,
    )
}

/// E3-B3: the model-delete confirmation title.
pub fn models_delete_model_title(model: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-models-delete-model-title",
        TranslationArgs::new().with_string("model", model),
        cx,
    )
}

/// G-SE12: the localized hint for the runtime's diagnosis code (web
/// `translateConnectionTestMessage`).
pub fn model_test_message_code(
    code: &openbitfun_core::util::types::ConnectionTestMessageCode,
    cx: &App,
) -> String {
    use openbitfun_core::util::types::ConnectionTestMessageCode as Code;
    let key = match code {
        Code::ToolCallsNotDetected => "gpui-shell-models-test-tool-calls",
        Code::ImageInputCheckFailed => "gpui-shell-models-test-image-input",
        Code::TlsOrCertificateIssue => "gpui-shell-models-test-tls",
        Code::ProxyIssue => "gpui-shell-models-test-proxy",
        Code::NetworkIssue => "gpui-shell-models-test-network",
    };
    resolve_with(key, TranslationArgs::new(), cx)
}

/// G-SE12: one reasoning preset action described the way the web preset
/// summary does.
pub fn reasoning_action_summary(
    action: &openbitfun_core::service::config::types::ReasoningPresetAction,
    cx: &App,
) -> String {
    use openbitfun_core::service::config::types::ReasoningPresetAction as Action;
    match action {
        Action::Effort { value } => resolve_with(
            "gpui-shell-models-reasoning-effort",
            TranslationArgs::new().with_string("value", value),
            cx,
        ),
        Action::Toggle { enabled: true } => {
            resolve_with("gpui-shell-models-reasoning-on", TranslationArgs::new(), cx)
        }
        Action::Toggle { enabled: false } => resolve_with(
            "gpui-shell-models-reasoning-off",
            TranslationArgs::new(),
            cx,
        ),
        Action::BudgetTokens { value } => resolve_with(
            "gpui-shell-models-reasoning-budget",
            TranslationArgs::new().with_number("value", f64::from(*value)),
            cx,
        ),
        Action::RequestPatch { .. } => resolve_with(
            "gpui-shell-models-reasoning-patch",
            TranslationArgs::new().with_number("count", 1_f64),
            cx,
        ),
    }
}

/// G-SE12: the summary line for a preset that carries no actions.
pub fn reasoning_no_actions(cx: &App) -> String {
    resolve_with(
        "gpui-shell-models-reasoning-no-actions",
        TranslationArgs::new(),
        cx,
    )
}

/// G-SE15: the default-Harness description when a last selection was
/// recorded (web `defaultHarness.followLastDescription`).
pub fn default_harness_follow_description(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-default-harness-follow-last-desc",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// G-SE15: the default-Harness description before any selection was recorded
/// (web `defaultHarness.followLastEmptyDescription`).
pub fn default_harness_follow_empty_description(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-default-harness-follow-last-empty-desc",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// G-M2: the per-candidate target-id field label (web `visual.importId`).
pub fn manage_mcp_import_id(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-mcp-import-id",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// T2-11: the selected-count line in the per-mode skills dialog.
pub fn mode_skills_selected_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-mode-skills-selected-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// T2-11: the covered-by status label naming the shadowing source.
pub fn mode_skills_covered(source: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-mode-skills-status-covered",
        TranslationArgs::new().with_string("source", source),
        cx,
    )
}

/// T2-12: the custom-agent delete confirmation naming the target.
pub fn manage_agent_delete_warning(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-agent-delete-warning",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// Resolves the localized notice for a `/btw` command without a question.
pub fn btw_empty(cx: &App) -> String {
    resolve_with("gpui-shell-btw-empty", TranslationArgs::new(), cx)
}

/// The composer model picker's localized label for a runtime tier alias;
/// unknown ids pass through as their raw id (the previous behavior).
pub fn model_alias_label(id: &str, cx: &App) -> String {
    match id {
        "primary" => resolve_with(
            "gpui-shell-memory-model-primary",
            TranslationArgs::new(),
            cx,
        ),
        "fast" => resolve_with("gpui-shell-memory-model-fast", TranslationArgs::new(), cx),
        other => other.to_string(),
    }
}

/// The transcript alert's generic send-failure label (web `error.sendFailed`);
/// the concrete error text follows it in the row.
pub fn chat_send_failed(cx: &App) -> String {
    resolve_with("gpui-shell-chat-send-failed", TranslationArgs::new(), cx)
}

/// The failed-submission alert's refill action (web `message.continueEditing`).
pub fn chat_continue_editing(cx: &App) -> String {
    resolve_with(
        "gpui-shell-chat-continue-editing",
        TranslationArgs::new(),
        cx,
    )
}

/// Resolves the localized turn-count label for one session row.
pub fn turns_label(turn_count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-session-turns",
        TranslationArgs::new().with_number("count", turn_count as f64),
        cx,
    )
}

/// Resolves the localized bound-session count label for one worktree row.
pub fn worktree_sessions_label(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-worktree-sessions",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// E3-B2: the commit-attribution row's description with the co-author handle
/// interpolated (web `commitAttribution.description`; the handle is derived in
/// `settings` from the shared GitHub attribution URL, exactly like the web
/// constant).
pub fn git_attribution_description(account: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-git-attribution-description",
        TranslationArgs::new().with_string("account", account),
        cx,
    )
}

/// E3-B2: the localized managed-worktree count of one project block on the
/// settings page (web `management.worktreeCount`).
pub fn worktree_count_label(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-worktree-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// E3-B2: the settings page's bound-session count (web
/// `management.sessions.summary`; the Git panel's row keeps its own shorter
/// label).
pub fn worktree_management_sessions_label(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-worktree-sessions",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// E3-B2: the web row's detached-checkout label — the short HEAD hash stands
/// in for a missing branch name (`labels.detached`).
pub fn worktree_detached_label(head: &str, cx: &App) -> String {
    let short = head.trim();
    let short = if short.len() >= 7 {
        &short[..7]
    } else if short.is_empty() || short.chars().all(|c| c == '0') {
        "-"
    } else {
        short
    };
    resolve_with(
        "gpui-shell-settings-worktree-detached",
        TranslationArgs::new().with_string("commit", short),
        cx,
    )
}

/// Resolves the localized facts line for one snapshot-session row.
pub fn snapshot_stats_label(files: usize, turns: usize, changes: usize, cx: &App) -> String {
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
pub fn git_failed(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the AI-commit-generation guard shown when the workspace has no
/// changes to describe (G-G2).
pub fn git_generate_no_changes(cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-generate-no-changes",
        TranslationArgs::new(),
        cx,
    )
}

/// Resolves the localized trend range line for the usage chart.
pub fn usage_trend_range(start: &str, end: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-usage-trend-range",
        TranslationArgs::new()
            .with_string("start", start)
            .with_string("end", end),
        cx,
    )
}

/// Resolves the localized activity-window total line.
pub fn usage_activity_total(tokens: u64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-usage-activity-total",
        TranslationArgs::new().with_number("tokens", tokens as f64),
        cx,
    )
}

/// Resolves the localized files-page failure line for one concrete error.
pub fn files_failed(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-files-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the localized search failure line for one concrete error.
pub fn search_failed(error: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-search-failed",
        TranslationArgs::new().with_string("error", error),
        cx,
    )
}

/// Resolves the unsupported-image-attachment notice for one rejected item.
pub fn attachment_unsupported(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-chat-attachment-unsupported",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// Resolves the desktop-notification headline for a finished turn.
pub fn notification_turn_done(cx: &App) -> String {
    resolve_with(
        "gpui-shell-notification-turn-done",
        TranslationArgs::new(),
        cx,
    )
}

/// Resolves the desktop-notification headline for a failed turn.
pub fn notification_turn_failed(cx: &App) -> String {
    resolve_with(
        "gpui-shell-notification-turn-failed",
        TranslationArgs::new(),
        cx,
    )
}

/// Resolves the localized subagent attribution line for one permission
/// request; composed once when the request arrives, never on the render path.
pub fn permission_subagent_label(subagent_type: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-permission-subagent",
        TranslationArgs::new().with_string("subagent", subagent_type),
        cx,
    )
}

/// Resolves the delete-all-archived confirmation body for the counts the
/// dialog was opened with (G-SE30: every workspace the page groups).
pub fn archived_delete_all_confirm_message(
    count: usize,
    workspace_count: usize,
    cx: &App,
) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-all-confirm-message",
        TranslationArgs::new()
            .with_number("count", count as f64)
            .with_number("workspaceCount", workspace_count as f64),
        cx,
    )
}

/// Resolves one workspace's archived-sessions load failure (web
/// `loadArchivedFailedForWorkspace`).
pub fn archived_load_failed(workspace: &str, path: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-load-failed",
        TranslationArgs::new()
            .with_string("workspace", workspace)
            .with_string("path", path),
        cx,
    )
}

/// G-X6: one notification-center row, naming the session and how many turns
/// finished in it while the user was elsewhere.
pub fn notifications_row(name: &str, count: u32, cx: &App) -> String {
    resolve_with(
        "gpui-shell-notifications-row",
        TranslationArgs::new()
            .with_string("name", name)
            .with_number("count", count as f64),
        cx,
    )
}

/// G-T1: the dock divider's tooltip (web `layout.resizer.title`): drag to
/// resize, double-click to switch mode, and the mode the panel is in now.
pub fn terminal_resizer_title(mode: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-terminal-resizer-title",
        TranslationArgs::new().with_string("mode", mode),
        cx,
    )
}

/// Resolves the archived row actions' inline receipts (web
/// `restoreSucceeded` / `deleteArchivedSucceeded` / `deleteAllArchivedSucceeded`)
/// with the row's name and its workspace label.
pub fn archived_restore_succeeded(name: &str, workspace: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-restore-succeeded",
        TranslationArgs::new()
            .with_string("name", name)
            .with_string("workspace", workspace),
        cx,
    )
}

pub fn archived_delete_succeeded(name: &str, workspace: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-succeeded",
        TranslationArgs::new()
            .with_string("name", name)
            .with_string("workspace", workspace),
        cx,
    )
}

pub fn archived_delete_all_succeeded(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-all-succeeded",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// Resolves the single-row delete confirmation body, which names the session
/// and the workspace it is deleted from (web `deleteArchivedConfirmMessage`).
pub fn archived_delete_message(name: &str, workspace: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-message",
        TranslationArgs::new()
            .with_string("name", name)
            .with_string("workspace", workspace),
        cx,
    )
}

/// Resolves the project-Hooks risk dialog body, which names the workspace the
/// gate would trust (web `projectHooksRisk.message`); an empty path falls
/// back to the generic "current workspace" phrase.
pub fn project_hooks_risk_message(workspace_path: &str, cx: &App) -> String {
    let trimmed = workspace_path.trim();
    let workspace = if trimmed.is_empty() {
        let text = current(cx);
        text.settings_project_hooks_risk_current_workspace
            .to_string()
    } else {
        trimmed.to_string()
    };
    resolve_with(
        "gpui-shell-settings-project-hooks-risk-message",
        TranslationArgs::new().with_string("workspace", &workspace),
        cx,
    )
}

/// Resolves the quick-action delete confirmation body, which names the action
/// (web `delete.confirmMessage`).
pub fn quick_action_delete_message(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-quick-actions-delete-message",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// Resolves the ACP install confirmation title, which names the agent (web
/// `installConfirm.title`).
pub fn acp_install_confirm_title(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-acp-install-confirm-title",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// Resolves the ACP install confirmation body, which names the target host
/// and the exact command (web `installConfirm.message`).
pub fn acp_install_confirm_message(host: &str, command: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-acp-install-confirm-message",
        TranslationArgs::new()
            .with_string("host", host)
            .with_string("command", command),
        cx,
    )
}

/// Tool-row chips that take their number at render time (the pattern lives in
/// the locale file, so word order stays locale-owned).
pub fn tool_meta_exit_code(code: i64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-exit-code",
        TranslationArgs::new().with_number("code", code as f64),
        cx,
    )
}

pub fn tool_meta_lines(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-lines",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn tool_meta_replacements(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-replacements",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn tool_meta_matches(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-matches",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn tool_meta_agents(total: usize, running: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-agents",
        TranslationArgs::new()
            .with_number("total", total as f64)
            .with_number("running", running as f64),
        cx,
    )
}

pub fn tool_meta_failed(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-failed",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// The exploration/execution group chip (web `workspaceGroup.summary`) and
/// the category counts its description lists.
pub fn transcript_group_summary(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-summary",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn transcript_group_reads(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-reads",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn transcript_group_searches(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-searches",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub fn transcript_group_commands(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-commands",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// Resolves the localized "question n of m" heading for one question of an
/// `ask_user` batch.
pub fn question_progress(index: usize, total: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-question-progress",
        TranslationArgs::new()
            .with_number("index", index as f64)
            .with_number("total", total as f64),
        cx,
    )
}

/// G-SE13: a stored memory model id that is not in the current catalog stays
/// selectable with this suffix (web `models.unavailable`) instead of being
/// silently dropped from the dropdown.
pub fn memory_model_unavailable(id: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-memory-model-unavailable",
        TranslationArgs::new().with_string("id", id),
        cx,
    )
}

/// Resolves one key through the global i18n service. `pub` because the
/// manage pages map their own error enums onto shell keys from the
/// `gpui-shell` crate.
pub fn resolve_with(key: &str, args: TranslationArgs, cx: &App) -> String {
    cx.foreground_executor().block_on(async move {
        match get_global_i18n_service().await {
            Some(service) => service.t_with(key, args).await,
            None => key.to_string(),
        }
    })
}

/// English copy used when the i18n service is unavailable; also the base for
/// test fixtures that only care about a few fields.
pub fn fallback_text(locale: LocaleId) -> ShellText {
    ShellText {
        locale,
        theme: "Theme".to_string(),
        language: "Language".to_string(),
        status_starting: "Starting OpenBitFun runtime…".to_string(),
        sessions_title: "Sessions".to_string(),
        sessions_empty: "No sessions in this workspace yet.".to_string(),
        nav_assistant: "AI Assistant".to_string(),
        nav_tasks: "Task Board".to_string(),
        sessions_reload: "Reload".to_string(),
        sessions_new: "New session".to_string(),
        sessions_show_archived: "Show archived sessions".to_string(),
        sessions_add_group: "Add project".to_string(),
        sessions_show_more: "Show more".to_string(),
        sessions_collapse: "Collapse".to_string(),
        miniapps_title: "MiniApps".to_string(),
        miniapps_empty: "The desktop hosts MiniApps here; the shell has no MiniApp runtime yet.".to_string(),
        extensions_title: "Extensions & Compatibility".to_string(),
        extensions_empty: "The desktop hosts ecosystem import, ACP, and external hooks here; that capability is not yet scoped for the shell.".to_string(),
        extensions_open_acp: "Open ACP settings".to_string(),
        sessions_show_less: "Show less".to_string(),
        sessions_view_all_tooltip: "Show every session in one list".to_string(),
        sessions_view_grouped_tooltip: "Organize sessions by project and assistant".to_string(),
        sessions_filter_tooltip: "Organize and filter sessions".to_string(),
        sessions_ordering: "Ordering".to_string(),
        sessions_order_updated: "Recently updated".to_string(),
        sessions_order_created: "Recently created".to_string(),
        sessions_order_name: "Name".to_string(),
        sessions_show: "Show".to_string(),
        sessions_show_all: "All sessions".to_string(),
        sessions_show_unread: "Unread sessions".to_string(),
        sessions_filters: "Filters".to_string(),
        sessions_status_running: "Running".to_string(),
        sessions_reset_filters: "Reset".to_string(),
        sessions_no_matches: "No sessions match the current filters".to_string(),
        sessions_collapse_all: "Collapse All".to_string(),
        sessions_mark_all_read: "Mark All as Read".to_string(),
        sessions_copy_path: "Copy path".to_string(),
        sessions_reveal: "Reveal in explorer".to_string(),
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
        insights_empty_title: "No insights yet".to_string(),
        insights_empty: "No insight reports yet. Generate one to see session trends.".to_string(),
        insights_sessions: "Sessions".to_string(),
        insights_analyzed: "Analyzed".to_string(),
        insights_messages: "Messages".to_string(),
        insights_days_covered: "Days covered".to_string(),
        insights_whats_working: "What's working".to_string(),
        insights_whats_hindering: "What's hindering".to_string(),
        insights_quick_wins: "Quick wins".to_string(),
        insights_looking_ahead: "Looking ahead".to_string(),
        insights_model_label: "Model".to_string(),
        insights_model_primary: "Primary model".to_string(),
        insights_range_label: "Range".to_string(),
        insights_history: "Report history".to_string(),
        insights_keep_latest: "Only the 5 most recent reports are kept".to_string(),
        insights_md_additions: "OPENBITFUN.md Additions".to_string(),
        insights_features_to_try: "Features to Try".to_string(),
        insights_usage_patterns: "Usage Patterns".to_string(),
        insights_try_this_prompt: "Try this prompt".to_string(),
        insights_copy: "Copy".to_string(),
        tasks_title: "Task Board".to_string(),
        tasks_empty_title: "No scheduled jobs".to_string(),
        tasks_empty: "No scheduled jobs. Ask the agent to create one, or add it in the desktop app.".to_string(),
        tasks_run_now: "Run now".to_string(),
        tasks_subtitle: "Plan, manage, and run your scheduled work".to_string(),
        tasks_overview: "Plan overview".to_string(),
        tasks_overview_unit: "items".to_string(),
        tasks_overview_total: "Scheduled tasks".to_string(),
        tasks_overview_due_soon: "Due within 24 hours".to_string(),
        tasks_wd_mon: "Mo".to_string(),
        tasks_wd_tue: "Tu".to_string(),
        tasks_wd_wed: "We".to_string(),
        tasks_wd_thu: "Th".to_string(),
        tasks_wd_fri: "Fr".to_string(),
        tasks_wd_sat: "Sa".to_string(),
        tasks_wd_sun: "Su".to_string(),
        tasks_cal_title: "Calendar".to_string(),
        tasks_cal_prev: "Previous month".to_string(),
        tasks_cal_next: "Next month".to_string(),
        tasks_cal_today: "Today".to_string(),
        tasks_day_detail_title: "Selected day".to_string(),
        tasks_day_clear: "Clear".to_string(),
        tasks_day_empty: "Nothing scheduled on this day.".to_string(),
        tasks_badge_running: "Running now".to_string(),
        tasks_badge_next_run: "Next run".to_string(),
        tasks_badge_overdue: "Overdue".to_string(),
        tasks_inactive_paused: "Paused".to_string(),
        tasks_inactive_done: "Finished".to_string(),
        tasks_inactive_invalid: "Invalid schedule".to_string(),
        tasks_list_empty: "No pending tasks yet".to_string(),
        tasks_new: "New task".to_string(),
        tasks_editor_create_title: "New scheduled task".to_string(),
        tasks_editor_edit_title: "Edit scheduled task".to_string(),
        tasks_editor_create_desc: "Create a task and OpenBitFun will run it automatically at the scheduled time.".to_string(),
        tasks_editor_edit_desc: "Update the task details and how OpenBitFun should run it.".to_string(),
        tasks_editor_btn_create: "Create Todo".to_string(),
        tasks_editor_btn_save: "Save changes".to_string(),
        tasks_editor_cron_hint: "Fields are minute hour day month weekday. Weekday runs 1-7 with Sunday as 1, so 0 8 * * 2 is every Monday at 08:00.".to_string(),
        tasks_editor_name_placeholder: "For example: Daily standup summary".to_string(),
        tasks_editor_prompt_placeholder: "What should the agent do when this Todo fires?".to_string(),
        tasks_editor_name: "Name".to_string(),
        tasks_editor_prompt: "Prompt".to_string(),
        tasks_editor_schedule: "Schedule".to_string(),
        tasks_editor_kind_at: "One time".to_string(),
        tasks_editor_kind_every: "Recurring".to_string(),
        tasks_editor_kind_cron: "Cron".to_string(),
        tasks_editor_at: "Local date and time".to_string(),
        tasks_editor_at_hint: "Local time, YYYY-MM-DDTHH:MM.".to_string(),
        tasks_editor_every: "Interval".to_string(),
        tasks_editor_anchor: "Anchor time (optional)".to_string(),
        tasks_editor_anchor_hint: "Leave blank to use the creation time.".to_string(),
        tasks_editor_unit_minute: "Minutes".to_string(),
        tasks_editor_unit_hour: "Hours".to_string(),
        tasks_editor_unit_day: "Days".to_string(),
        tasks_editor_cron_expr: "Cron expression".to_string(),
        tasks_editor_cron_tz: "Time zone (optional)".to_string(),
        tasks_editor_target: "Run target".to_string(),
        tasks_editor_target_workspace: "Current workspace".to_string(),
        tasks_editor_target_session: "Existing session".to_string(),
        tasks_editor_session_placeholder: "Choose a session".to_string(),
        tasks_editor_no_sessions: "No sessions in this workspace yet.".to_string(),
        tasks_editor_enabled: "Enabled".to_string(),
        tasks_editor_error_name: "Enter a task name.".to_string(),
        tasks_editor_error_prompt: "Enter the prompt to send.".to_string(),
        tasks_editor_error_prompt_long: "The prompt is limited to 1000 characters.".to_string(),
        tasks_editor_error_at: "Enter a valid local date and time.".to_string(),
        tasks_editor_error_every: "Enter a positive interval.".to_string(),
        tasks_editor_error_anchor: "Enter a valid local anchor time, or leave it blank.".to_string(),
        tasks_editor_error_cron: "Enter a cron expression.".to_string(),
        tasks_editor_error_session: "Choose the session to bind.".to_string(),
        tasks_editor_error_workspace: "The active workspace is unavailable.".to_string(),
        panel_pop_out: "Pop out to a tab".to_string(),
        panel_close: "Hide panel".to_string(),
        panel_maximize: "Maximize panel".to_string(),
        panel_restore: "Restore panel".to_string(),
        panel_swap: "Swap panes".to_string(),
        panel_open: "Show panel".to_string(),
        sidebar_toggle: "Toggle sidebar".to_string(),
        files_title: "Files".to_string(),
        files_loading: "Loading workspace files…".to_string(),
        files_empty: "No files in this workspace.".to_string(),
        files_empty_title: "No files".to_string(),
        files_viewer_empty: "Select a file on the left to view its contents.".to_string(),
        files_viewer_empty_title: "No file selected".to_string(),
        files_reload: "Reload file tree".to_string(),
        files_filter_placeholder: "Filter files…".to_string(),
        files_no_matches: "No matching files".to_string(),
        files_too_large: "File is too large to display".to_string(),
        files_binary: "Binary file, no preview".to_string(),
        files_truncated: "Showing the first 1 MB of this file.".to_string(),
        search_trigger: "Search everything".to_string(),
        search_files_tab: "Files".to_string(),
        search_sessions_tab: "Sessions".to_string(),
        search_run: "Search".to_string(),
        search_running: "Searching…".to_string(),
        search_empty: "No results.".to_string(),
        search_empty_title: "No results".to_string(),
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
        composer_local: "On this device".to_string(),
        permission_ask: "Ask".to_string(),
        permission_ask_desc:
            "External access, file changes, and command execution require confirmation."
                .to_string(),
        permission_auto: "Auto approve".to_string(),
        permission_auto_desc:
            "Automatically approve requests that require confirmation.".to_string(),
        permission_full_access: "Full access".to_string(),
        permission_full_access_desc: "Tools are allowed by default without confirmation."
            .to_string(),
        models_title: "Models".to_string(),
        models_loading: "Loading model catalog…".to_string(),
        models_empty:
            "Add API models or import a subscription to build your model pool.".to_string(),
        models_disabled: "disabled".to_string(),
        models_test: "Test Connection".to_string(),
        models_testing: "Testing…".to_string(),
        models_test_details: "Error details".to_string(),
        models_edit_title: "Model settings".to_string(),
        models_edit_close: "Close".to_string(),
        models_category: "Model Category".to_string(),
        models_category_general_chat: "Text Generation".to_string(),
        models_category_multimodal: "Image Understanding".to_string(),
        models_category_image_generation: "Image Generation".to_string(),
        models_category_speech_recognition: "Speech Recognition".to_string(),
        models_context_window: "Context Window Size".to_string(),
        models_context_window_hint:
            "Determines when context is compacted; may be lower than the model's supported limit."
                .to_string(),
        models_max_tokens: "Max Output Tokens".to_string(),
        models_max_tokens_hint:
            "Limits tokens in a single response; 0 keeps the automatic cap.".to_string(),
        models_reasoning_default: "Default preset".to_string(),
        models_reasoning_auto: "Auto (model default)".to_string(),
        models_reasoning_presets: "Reasoning presets".to_string(),
        models_reasoning_empty: "No custom presets.".to_string(),
        models_advanced: "Advanced Settings".to_string(),
        models_advanced_note:
            "Adapt requests and responses to service-specific requirements; keep the defaults if unsure."
                .to_string(),
        models_inline_think: "Parse <think> Tags In Text".to_string(),
        models_inline_think_hint:
            "Display content inside <think>...</think> tags as reasoning.".to_string(),
        models_skip_ssl: "Skip SSL Certificate Verification".to_string(),
        models_skip_ssl_warning:
            "Skipping SSL verification is a security risk, use only in test or internal network environments!"
                .to_string(),
        models_headers: "Custom Headers".to_string(),
        models_headers_hint: "Add or modify HTTP request headers.".to_string(),
        models_headers_mode: "Apply Mode".to_string(),
        models_headers_replace: "Replace".to_string(),
        models_headers_merge: "Merge".to_string(),
        models_headers_replace_hint:
            "Replace all default headers; provide required fields such as Content-Type and Authorization yourself."
                .to_string(),
        models_headers_merge_hint:
            "Keep default headers and override matching fields with custom values.".to_string(),
        models_body: "Custom Request Body".to_string(),
        models_body_hint:
            "Enter request body fields in JSON format to override default request parameters."
                .to_string(),
        models_body_mode: "Apply Mode".to_string(),
        models_body_merge: "Merge".to_string(),
        models_body_trim: "Trim".to_string(),
        models_body_trim_hint:
            "Keep only the essential runtime fields before applying the custom request body."
                .to_string(),
        models_json_invalid: "Invalid JSON; the shared configuration was not changed."
            .to_string(),
        models_acquisition_title: "Add or import models".to_string(),
        models_acquisition_api: "API models".to_string(),
        models_acquisition_none: "No API services configured yet".to_string(),
        models_acquisition_subscriptions: "Subscriptions".to_string(),
        models_subscriptions_unavailable:
            "Subscription sign-in and quotas are not available in this surface yet; subscription models imported elsewhere keep working in the list below."
                .to_string(),
        models_add: "Add model".to_string(),
        models_delete_provider: "Delete service".to_string(),
        models_selection_modes_title: "Model selection modes".to_string(),
        models_mode_smart: "Smart mode".to_string(),
        models_mode_smart_hint:
            "Powered by OpenBitFun adaptive routing and collaborative multi-model reasoning to improve cost efficiency and task performance."
                .to_string(),
        models_mode_unavailable: "Not available yet".to_string(),
        models_pool_title: "All models".to_string(),
        models_default_primary: "Primary model".to_string(),
        models_default_primary_desc:
            "Complex tasks and default conversations use this model.".to_string(),
        models_default_fast: "Fast model".to_string(),
        models_default_fast_desc:
            "Used for lightweight tasks; falls back to the primary model when unset.".to_string(),
        models_default_image: "Image understanding model".to_string(),
        models_default_image_desc: "Reads images attached to a conversation.".to_string(),
        models_default_speech: "Speech recognition model".to_string(),
        models_default_speech_desc: "Transcribes audio into text.".to_string(),
        models_default_not_set: "Not set".to_string(),
        models_search: "Search models, sources, or tags".to_string(),
        models_filter_all_capabilities: "All capabilities".to_string(),
        models_filter_all_providers: "All services".to_string(),
        models_capability_text: "Text chat".to_string(),
        models_capability_image: "Image understanding".to_string(),
        models_capability_speech: "Speech recognition".to_string(),
        models_capability_tools: "Function calling".to_string(),
        models_no_matches: "No models match these filters".to_string(),
        models_clear_filter: "Clear filters".to_string(),
        models_delete: "Delete".to_string(),
        models_delete_confirm: "Delete model".to_string(),
        models_delete_message:
            "Sessions using this model will need to pick another one.".to_string(),
        models_more: "More settings".to_string(),
        models_more_expand: "Expand".to_string(),
        models_more_collapse: "Collapse".to_string(),
        models_editor_name: "Config name".to_string(),
        models_editor_provider: "Request format".to_string(),
        models_editor_model_name: "Model name".to_string(),
        models_editor_base_url: "API address".to_string(),
        models_editor_api_key: "API key".to_string(),
        models_editor_enabled: "Enabled".to_string(),
        models_editor_delete: "Delete model".to_string(),
        models_create_title: "Add model".to_string(),
        models_create_save: "Save".to_string(),
        models_create_cancel: "Cancel".to_string(),
        models_create_error_name: "Enter a config name.".to_string(),
        models_create_error_model: "Enter a model name.".to_string(),
        models_create_error_url:
            "Enter an API address starting with http:// or https://.".to_string(),
        models_create_error_key: "Enter an API key.".to_string(),
        models_create_error_context:
            "The context window must be at least 32,000 tokens.".to_string(),
        chat_empty: "Pick a session on the left, or create a new one.".to_string(),
        chat_empty_title: "Start a conversation".to_string(),
        welcome_title: "OpenBitFun,".to_string(),
        welcome_slogan: "Make it real.".to_string(),
        chat_input_placeholder: "Type a message to send to the agent…".to_string(),
        chat_jump_latest: "Jump to latest".to_string(),
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
        workspace_open_project: "Open Project".to_string(),
        workspace_new_project: "New Project".to_string(),
        workspace_open_path_description:
            "Folder to use as this shell's workspace. It is registered in the shared workspace list without changing the desktop's selection."
                .to_string(),
        new_project_title: "New Project".to_string(),
        new_project_subtitle: "Create a new workspace directory".to_string(),
        new_project_parent: "Parent Folder".to_string(),
        new_project_parent_placeholder: "Absolute path of the parent folder".to_string(),
        new_project_name: "Project Name".to_string(),
        new_project_name_placeholder: "e.g. my-project".to_string(),
        new_project_full_path: "Full Path".to_string(),
        new_project_create: "Create".to_string(),
        new_project_error_parent: "Enter the parent folder path".to_string(),
        new_project_error_name: "Please enter a project name".to_string(),
        new_project_error_missing: "That folder does not exist".to_string(),
        new_project_error_create: "Failed to create project".to_string(),
        terminal_title: "Terminal".to_string(),
        terminal_close: "Close terminal".to_string(),
        terminal_reveal: "Show in file manager".to_string(),
        notifications_title: "Notification Center".to_string(),
        notifications_empty: "No notifications".to_string(),
        notifications_clear_all: "Clear All".to_string(),
        terminal_mode_compact: "Compact".to_string(),
        terminal_mode_comfortable: "Comfortable".to_string(),
        terminal_mode_expanded: "Expanded".to_string(),
        terminal_exited: "process exited".to_string(),
        settings_open: "Open settings".to_string(),
        settings_title: "Settings".to_string(),
        tab_close: "Close tab".to_string(),
        settings_appearance: "Appearance".to_string(),
        settings_appearance_interface_title: "Interface".to_string(),
        settings_appearance_interface_description:
            "Choose the language for menus, buttons, and messages; changes apply immediately"
                .to_string(),
        settings_general_startup_title: "Startup & updates".to_string(),
        settings_general_startup_description: "Choose automatic behavior at sign-in and while the app runs; installing updates still requires your confirmation".to_string(),
        settings_general_window_title: "Window & notifications".to_string(),
        settings_general_window_description: "Choose whether closing the window quits the app or keeps it running, and which events need your attention".to_string(),
        settings_appearance_font_title: "Font Size".to_string(),
        settings_websearch_provider_title: "Search provider".to_string(),
        settings_websearch_http_title: "Custom search service".to_string(),
        settings_websearch_http_description:
            "The service must implement OpenBitFun Search HTTP Protocol v1".to_string(),
        settings_permissions: "Permissions".to_string(),
        settings_theme_system: "Follow system".to_string(),
        settings_theme_unsupported: "unsupported".to_string(),
        settings_permission_rules_section: "Global rules".to_string(),
        settings_permission_rules_description: "Applies to all workspaces; project and agent rules can override these, while mandatory product restrictions always take precedence".to_string(),
        settings_permission_rules_manage: "Manage rules".to_string(),
        settings_permission_rules_add: "Add rule".to_string(),
        settings_permission_rules_save: "Save rules".to_string(),
        settings_permission_rules_discard: "Discard changes".to_string(),
        settings_permission_rules_empty: "No custom global rules configured. The selected permission mode remains active.".to_string(),
        settings_permission_rules_action: "Action".to_string(),
        settings_permission_rules_resource: "Resource".to_string(),
        settings_permission_rules_effect: "Effect".to_string(),
        settings_permission_rules_invalid: "Every rule needs an action and a resource.".to_string(),
        settings_permission_effect_allow: "Allow".to_string(),
        settings_permission_effect_ask: "Ask".to_string(),
        settings_permission_effect_deny: "Deny".to_string(),
        settings_quick_actions_section: "Quick actions".to_string(),
        settings_quick_actions_add: "Add action".to_string(),
        settings_quick_actions_delete: "Delete".to_string(),
        settings_quick_actions_delete_title: {
            "Delete custom action?".to_string()
        },
        settings_quick_actions_delete_action: "Delete action".to_string(),
        settings_quick_actions_label: "Action name".to_string(),
        settings_quick_actions_prompt: "Prompt".to_string(),
        settings_quick_actions_save: "Save".to_string(),
        settings_quick_actions_cancel: "Cancel".to_string(),
        settings_quick_actions_invalid: "A name and a prompt are required.".to_string(),
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
        git_empty_title: "No changes".to_string(),
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
        git_discard: "Discard changes".to_string(),
        git_discard_confirm: "Confirm discard".to_string(),
        git_delete_file: "Delete file".to_string(),
        git_delete_confirm: "Confirm delete".to_string(),
        git_commit: "Commit".to_string(),
        git_commit_placeholder: "Commit message".to_string(),
        git_push: "Push to remote".to_string(),
        git_pull: "Pull from remote".to_string(),
        git_not_repo_title: "Initialize Git".to_string(),
        git_not_repo_detail: "Current directory is not a Git repository".to_string(),
        git_init_button: "Initialize Repository".to_string(),
        git_init_hint: "The AI assistant runs the initialization for you".to_string(),
        git_init_prompt:
            "Please help me initialize a Git repository in the current workspace."
                .to_string(),
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
        git_branches_view: "Branches".to_string(),
        git_graph_view: "Graph".to_string(),
        git_branch_create: "Create branch".to_string(),
        git_branch_name_label: "Branch name".to_string(),
        git_branch_base_label: "Start point (optional)".to_string(),
        git_branch_switch: "Switch".to_string(),
        git_branch_delete: "Delete".to_string(),
        git_branch_delete_confirm: "Confirm delete".to_string(),
        git_branch_copy_hash: "Copy hash".to_string(),
        git_branch_current: "current".to_string(),
        git_branch_remote: "remote".to_string(),
        git_branch_empty: "No branches yet.".to_string(),
        git_graph_reset: "Reset here".to_string(),
        git_graph_reset_confirm: "Confirm reset".to_string(),
        git_generate_message: "AI message".to_string(),
        git_generate_cancel: "Stop".to_string(),
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
        turn_copy: "Copy dialog".to_string(),
        turn_meta_completed: "Completed".to_string(),
        turn_meta_stopped: "Stopped".to_string(),
        turn_meta_duration: "Duration".to_string(),
        turn_copy_full: "Copy full transcript".to_string(),
        turn_copy_result: "Copy result".to_string(),
        code_copy: "Copy code".to_string(),
        code_copied: "Copy success".to_string(),
        tool_open_details: "Open details".to_string(),
        tool_copy: "Copy".to_string(),
        tool_copied: "Copy success".to_string(),
        turn_copy_user_label: "\u{1f464} User:".to_string(),
        subagent_badge: "Subagent run".to_string(),
        subagent_open: "View run".to_string(),
        subagent_names: crate::subagent_identity::ENGLISH_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        tool_action_command: "Run command".to_string(),
        tool_action_read: "Read file".to_string(),
        tool_action_search: "Search files".to_string(),
        tool_action_web: "Open page".to_string(),
        tool_action_write: "Write file".to_string(),
        tool_action_edit: "Edit file".to_string(),
        tool_action_tool_spec: "Read tool spec".to_string(),
        tool_action_tool_call: "Call tool".to_string(),
        tool_action_agent_spawn: "Start subagent".to_string(),
        tool_action_agent_wait: "Wait for subagents".to_string(),
        tool_action_agent_list: "View subagents".to_string(),
        tool_action_agent_send: "Send instruction".to_string(),
        tool_action_agent_delete: "Delete subagent".to_string(),
        btw_badge: "Side question (/btw)".to_string(),
        sidebar_review_running: "Reviewing".to_string(),
        child_back: "Back".to_string(),
        manage_skills_title: "Skills".to_string(),
        manage_skills_loading: "Loading skills…".to_string(),
        manage_skills_empty_title: "No skills".to_string(),
        manage_skills_empty: "No skills found.".to_string(),
        manage_skills_search_placeholder: "Search skills…".to_string(),
        manage_skills_hide_shadowed: "Hide duplicates".to_string(),
        manage_skills_no_match: "No matching skills found.".to_string(),
        manage_skill_builtin: "Built-in".to_string(),
        manage_skill_user: "User".to_string(),
        manage_skill_project: "Project".to_string(),
        manage_skill_shadowed: "Shadowed".to_string(),
        manage_skill_add: "Add skill".to_string(),
        manage_skill_detail: "Details".to_string(),
        manage_skill_dialog_add_title: "Add skill".to_string(),
        manage_skill_dialog_detail_title: "Skill details".to_string(),
        manage_skill_dialog_delete_title: "Delete skill".to_string(),
        manage_skill_source: "Source".to_string(),
        manage_skill_source_native: "OpenBitFun".to_string(),
        manage_skill_path: "Folder path".to_string(),
        manage_skill_path_hint: "Point to a folder containing SKILL.md.".to_string(),
        manage_skill_validate: "Check".to_string(),
        manage_skill_valid: "Valid skill".to_string(),
        manage_skill_invalid: "Not a valid skill".to_string(),
        manage_skill_runtime_unavailable: "The runtime is not ready yet. Try again.".to_string(),
        manage_skill_add_action: "Add".to_string(),
        manage_skill_delete_action: "Delete".to_string(),
        manage_skill_close: "Close".to_string(),
        manage_skill_not_found: "This skill is no longer in the catalog.".to_string(),
        manage_skill_no_description: "No description.".to_string(),
        skill_groups_title: "Skill groups".to_string(),
        skill_groups_subtitle: "Create and organize reusable groups of skills.".to_string(),
        skill_groups_manage: "Manage groups".to_string(),
        skill_groups_mine: "My groups".to_string(),
        skill_groups_builtin: "Built-in groups".to_string(),
        skill_groups_loading: "Loading skill groups…".to_string(),
        skill_groups_empty: "No skill groups yet.".to_string(),
        skill_groups_empty_group: "No skills added".to_string(),
        skill_groups_unresolved: "Members are unavailable in the current catalog".to_string(),
        skill_group_new: "New group".to_string(),
        skill_group_name_label: "Group name".to_string(),
        skill_group_name_placeholder: "For example: Everyday development".to_string(),
        skill_group_dialog_edit_title: "Skill group".to_string(),
        skill_group_dialog_delete_title: "Delete skill group".to_string(),
        skill_group_save: "Save".to_string(),
        skill_group_copy: "Copy".to_string(),
        skill_group_up: "Move up".to_string(),
        skill_group_down: "Move down".to_string(),
        skill_group_delete: "Delete".to_string(),
        agent_detail_open: "Details".to_string(),
        agent_detail_missing: "This agent is no longer in the catalog.".to_string(),
        agent_detail_type: "Type".to_string(),
        agent_detail_source: "Source".to_string(),
        agent_detail_follow_up: "Follow-up".to_string(),
        agent_detail_supported: "Supported".to_string(),
        agent_detail_unsupported: "Not supported".to_string(),
        agent_detail_capabilities: "Capabilities".to_string(),
        agent_detail_shared_profile: "Shared profile".to_string(),
        agent_detail_shared_default: "Shared coding profile".to_string(),
        agent_detail_tools: "Tools".to_string(),
        agent_detail_subagents: "SubAgents".to_string(),
        agent_detail_loading: "Loading…".to_string(),
        agent_detail_no_subagents: "No manageable SubAgents available".to_string(),
        agent_detail_save: "Save".to_string(),
        agent_detail_reset: "Reset".to_string(),
        agent_detail_close: "Close".to_string(),
        skills_facet_library: "Library".to_string(),
        skills_facet_all: "All".to_string(),
        skills_facet_groups: "Skill groups".to_string(),
        skills_facet_sources: "Sources".to_string(),
        skills_facet_explore: "Discover".to_string(),
        skills_market_title: "Skill market".to_string(),
        skills_market_degraded: "The skill market is not available in this shell yet. Browse and install skills from the Web app.".to_string(),
        manage_agents_title: "Agents".to_string(),
        manage_agents_modes: "Modes".to_string(),
        manage_agents_subagents: "Subagents".to_string(),
        manage_agents_create: "New agent…".to_string(),
        manage_agents_loading: "Loading agents…".to_string(),
        manage_agents_empty_title: "No agents".to_string(),
        manage_agents_empty: "No agents found.".to_string(),
        manage_agents_search_placeholder: "Search agents by name or description…".to_string(),
        manage_agents_source_label: "Source".to_string(),
        manage_agents_source_any: "Any source".to_string(),
        manage_agents_kind_label: "Type".to_string(),
        manage_agents_kind_any: "Any type".to_string(),
        manage_agents_kind_agent: "Agent".to_string(),
        manage_agents_kind_subagent: "SubAgent".to_string(),
        manage_agents_no_match: "No matching agents.".to_string(),
        manage_harness_title: "Agent Harness".to_string(),
        manage_harness_subtitle: "Choose how a session executes work; each Harness owns its tool and collaboration policy".to_string(),
        manage_harness_purpose_minimal: "Lean flow, fast progress".to_string(),
        manage_harness_purpose_standard: "Balance speed and quality".to_string(),
        manage_harness_purpose_ultimate: "Deep planning and collaboration".to_string(),
        manage_harness_purpose_creative: "Customize and build on OpenBitFun".to_string(),
        manage_agent_model: "Model".to_string(),
        manage_agent_model_default: "Use default SubAgent model".to_string(),
        manage_agent_model_failed: "Failed to update the SubAgent model".to_string(),
        manage_agent_create_title: "Create custom agent".to_string(),
        manage_agent_kind_mode: "Mode".to_string(),
        manage_agent_kind_subagent: "Subagent".to_string(),
        manage_agent_level_user: "User level".to_string(),
        manage_agent_level_project: "Project level".to_string(),
        manage_agent_id: "Id".to_string(),
        manage_agent_name: "Name".to_string(),
        manage_agent_description: "Description".to_string(),
        manage_agent_prompt: "Prompt".to_string(),
        manage_agent_readonly: "Read-only".to_string(),
        manage_agent_review: "Review".to_string(),
        manage_agent_enabled: "Enabled".to_string(),
        manage_agent_disabled: "Disabled".to_string(),
        manage_agent_source_builtin: "Built-in".to_string(),
        manage_agent_source_user: "User".to_string(),
        manage_agent_source_project: "Project".to_string(),
        manage_agent_source_external: "External".to_string(),
        manage_agent_error_empty_id: "An id is required.".to_string(),
        manage_agent_error_bad_id: "Id must start with a letter and contain only letters, digits, -, _.".to_string(),
        manage_agent_error_empty_name: "A name is required.".to_string(),
        manage_agent_error_empty_description: "A description is required.".to_string(),
        manage_agent_error_empty_prompt: "A prompt is required.".to_string(),
        manage_agent_error_mode_project: "Custom modes are user-level only.".to_string(),
        manage_agent_edit_title: "Edit custom agent".to_string(),
        manage_agent_delete: "Delete".to_string(),
        manage_agent_delete_title: "Delete custom agent".to_string(),
        // "Edit" is a shared term; the dedicated key would trip the
        // shared-term duplicate ratchet (same alias the terminal profile
        // uses).
        agent_card_edit: "Edit".to_string(),
        mode_skills_open: "Skills".to_string(),
        mode_skills_title: "Skill selection".to_string(),
        mode_skills_loading: "Loading skills…".to_string(),
        mode_skills_empty: "No skills available for this agent.".to_string(),
        mode_skills_edit: "Edit".to_string(),
        mode_skills_save_hint: "Applies after saving".to_string(),
        mode_skills_status_selected: "Selected for runtime".to_string(),
        mode_skills_status_enabled: "Enabled, not selected".to_string(),
        mode_skills_reset: "Reset".to_string(),
        mode_skills_source_other: "Other source".to_string(),
        manage_mcp_title: "MCP servers".to_string(),
        manage_mcp_loading: "Loading MCP servers…".to_string(),
        manage_mcp_empty_title: "No MCP servers".to_string(),
        manage_mcp_empty: "No MCP servers configured. Add one through the desktop or CLI JSON config.".to_string(),
        manage_mcp_start: "Start".to_string(),
        manage_mcp_stop: "Stop".to_string(),
        manage_mcp_restart: "Restart".to_string(),
        manage_mcp_enable: "Enable".to_string(),
        manage_mcp_disable: "Disable".to_string(),
        manage_mcp_delete: "Delete".to_string(),
        manage_mcp_delete_title: "Delete MCP server".to_string(),
        manage_mcp_delete_warning: "This removes the server and its persisted configuration.".to_string(),
        manage_mcp_browse: "Browse".to_string(),
        manage_mcp_browse_title: "Server catalog".to_string(),
        manage_mcp_resources: "Resources".to_string(),
        manage_mcp_prompts: "Prompts".to_string(),
        manage_mcp_refresh: "Refresh".to_string(),
        manage_mcp_disabled: "Disabled".to_string(),
        manage_mcp_location_builtin: "Built-in".to_string(),
        manage_mcp_state_uninitialized: "Not started".to_string(),
        manage_mcp_state_starting: "Starting".to_string(),
        manage_mcp_state_connected: "Connected".to_string(),
        manage_mcp_state_healthy: "Healthy".to_string(),
        manage_mcp_state_needs_auth: "Needs auth".to_string(),
        manage_mcp_state_reconnecting: "Reconnecting".to_string(),
        manage_mcp_state_failed: "Failed".to_string(),
        manage_mcp_state_stopping: "Stopping".to_string(),
        manage_mcp_state_stopped: "Stopped".to_string(),
        manage_interaction_title: "MCP server request".to_string(),
        manage_interaction_params: "Request payload".to_string(),
        manage_interaction_result_hint: "Result JSON (optional)".to_string(),
        manage_interaction_approve: "Approve".to_string(),
        manage_interaction_reject: "Reject".to_string(),
        manage_mcp_json_item: "Configuration JSON".to_string(),
        manage_mcp_json_edit: "Edit JSON".to_string(),
        manage_mcp_json_loading: "Loading configuration…".to_string(),
        manage_mcp_json_save: "Save".to_string(),
        manage_mcp_json_saved: "MCP configuration saved.".to_string(),
        manage_mcp_import: "Import config".to_string(),
        manage_mcp_import_paste: "Paste an MCP configuration".to_string(),
        manage_mcp_import_path: "JSON file path".to_string(),
        manage_mcp_import_read: "Read file".to_string(),
        manage_mcp_import_file_failed: {
            "Could not read the file. Check the path, or paste the JSON content instead."
                .to_string()
        },
        manage_mcp_import_preview: "Preview import".to_string(),
        manage_mcp_import_hint: {
            "Use the mcpServers JSON format. Select servers below; imported servers stay \
             disabled. Existing entries are never overwritten."
                .to_string()
        },
        manage_mcp_import_selected: "Import selected servers".to_string(),
        manage_mcp_import_conflict: "ID exists. Change it to import a separate server.".to_string(),
        manage_mcp_import_select_servers: "Select at least one server to import.".to_string(),
        manage_mcp_import_invalid_json: {
            "Invalid JSON. Check the syntax and try again.".to_string()
        },
        manage_mcp_import_invalid_config: {
            "Expected an object containing an mcpServers object with server entries."
                .to_string()
        },
        manage_mcp_import_id_required: "Enter a service ID.".to_string(),
        manage_mcp_import_invalid_id: {
            "Use 1-128 letters, digits, dots, hyphens, or underscores for a new ID.".to_string()
        },
        manage_mcp_import_id_conflict: {
            "This ID already exists or is selected twice. Choose a different ID.".to_string()
        },
        manage_mcp_import_shape: {
            "This configuration uses fields the shell cannot import safely. Keep it in the \
             JSON editor."
                .to_string()
        },
        manage_mcp_resource_hint: "Click a resource to read its content.".to_string(),
        manage_mcp_resource_loading: "Reading resource…".to_string(),
        manage_mcp_resource_binary: "(Binary content not shown)".to_string(),
        manage_mcp_resource_empty: "The resource returned no content.".to_string(),
        manage_mcp_resource_failed: "Failed to read the resource content.".to_string(),
        manage_mcp_auth: "Remote Auth".to_string(),
        manage_mcp_auth_oauth_hint: {
            "Use OAuth to connect this remote MCP server. OpenBitFun will listen on a local \
             callback URL and reconnect the server after authorization."
                .to_string()
        },
        manage_mcp_oauth_idle: "Not started yet".to_string(),
        manage_mcp_oauth_awaiting_browser: "Waiting to open browser".to_string(),
        manage_mcp_oauth_awaiting_callback: "Waiting for provider callback".to_string(),
        manage_mcp_oauth_exchanging_token: "Exchanging authorization code".to_string(),
        manage_mcp_oauth_authorized: "Authorized".to_string(),
        manage_mcp_oauth_failed: "Failed".to_string(),
        manage_mcp_oauth_cancelled: "Cancelled".to_string(),
        manage_mcp_oauth_start: "Connect with OAuth".to_string(),
        manage_mcp_oauth_restart: "Restart OAuth".to_string(),
        manage_mcp_oauth_reconnect: "Reconnect with OAuth".to_string(),
        manage_mcp_auth_token_hint: {
            "Paste a Bearer token or a full Authorization header value. Saving will restart the \
             remote MCP server."
                .to_string()
        },
        manage_mcp_auth_secret_hint: {
            "Authorization is masked by default and saved only on the current execution host. Do \
             not paste it into chats, tickets, or online troubleshooting tools."
                .to_string()
        },
        manage_mcp_auth_token_label: "Access token (Bearer)".to_string(),
        manage_mcp_auth_clear: "Clear auth".to_string(),
        manage_mcp_auth_cancel_flow: "Cancel OAuth".to_string(),
        manage_mcp_auth_save: "Save and Reconnect".to_string(),
        manage_mcp_auth_required: {
            "Please provide a Bearer token or full Authorization header value.".to_string()
        },
        manage_mcp_auth_saved: "Remote auth updated.".to_string(),
        manage_mcp_auth_configured: "configured".to_string(),
        manage_mcp_auth_oauth_ready: "OAuth configured, authorization required".to_string(),
        manage_mcp_auth_missing: "not configured".to_string(),
        manage_mcp_auth_source_headers: "configured via headers".to_string(),
        manage_mcp_auth_source_env: "configured via env".to_string(),
        manage_mcp_auth_source_oauth: "configured via OAuth".to_string(),
        manage_error_dismiss: "Dismiss".to_string(),
        value_unsupported: "unsupported".to_string(),
        settings_general: "General".to_string(),
        settings_version: "Version".to_string(),
        settings_launch_login: "Launch at login".to_string(),
        settings_launch_login_description: {
            "Start OpenBitFun automatically when you sign in.".to_string()
        },
        settings_close_behavior: "Close button behavior".to_string(),
        settings_close_behavior_description: {
            "What the window close button does: quit OpenBitFun, or minimize it so the \
             tray icon keeps it reachable."
                .to_string()
        },
        settings_close_quit: "Quit".to_string(),
        settings_close_minimize: "Minimize".to_string(),
        settings_auto_update: "Check for updates at startup".to_string(),
        settings_auto_update_description: {
            "Checks the release feed once a day. The manual check below always runs."
                .to_string()
        },
        settings_notify_turn_done: "Task completion notification".to_string(),
        settings_notify_turn_done_description: {
            "Notify you when an AI task finishes while the window is in the background."
                .to_string()
        },
        settings_prevent_sleep: "Prevent sleep".to_string(),
        settings_prevent_sleep_description: {
            "Keep the computer awake while OpenBitFun is running, even without a task. \
             The display may turn off; the system controls manual sleep and lid-close behavior"
                .to_string()
        },
        settings_check_updates: "Check for updates".to_string(),
        settings_update_checking: "Checking for updates…".to_string(),
        settings_up_to_date: "Up to date".to_string(),
        settings_update_available: "New version available:".to_string(),
        settings_update_install: "Download and install".to_string(),
        settings_update_installing: "Downloading and installing…".to_string(),
        settings_update_installed: "Installed".to_string(),
        settings_update_restart: "Restart to apply".to_string(),
        settings_update_unsupported: "Self-update unavailable".to_string(),
        settings_update_failed: "Update check failed".to_string(),
        settings_automation: "Automation".to_string(),
        settings_hooks: "Agent hooks".to_string(),
        settings_hooks_description: {
            "Master switch for native lifecycle hooks (hooks.json).".to_string()
        },
        settings_hooks_project: "Project hooks".to_string(),
        settings_hooks_project_description: {
            "Allow hook files from the checked-out repository. Only enable this for \
             workspaces you trust."
                .to_string()
        },
        settings_project_hooks_risk_title: {
            "Allow workspace code to run Hooks?".to_string()
        },
        settings_project_hooks_risk_confirm: {
            "I reviewed it, enable Hooks".to_string()
        },
        settings_project_hooks_risk_current_workspace: {
            "the current workspace".to_string()
        },
        settings_hooks_user_file: "User hook file".to_string(),
        settings_hooks_user_file_desc: {
            "config/hooks.json in the OpenBitFun user config directory.".to_string()
        },
        settings_hooks_project_file: "Project hook file".to_string(),
        settings_hooks_project_file_desc: {
            ".openbitfun/config/hooks.json in the current workspace. Requires \"Enable project hook file\"."
                .to_string()
        },
        settings_hooks_imports_master_disabled: {
            "Imported hooks are preserved, but the master switch prevents them from running."
                .to_string()
        },
        settings_hooks_imports_refresh: "Refresh".to_string(),
        settings_hooks_imports_loading: "Loading imported hooks…".to_string(),
        settings_hooks_imports_empty: {
            "No imported hooks. Refresh to check Claude Code and Codex.".to_string()
        },
        settings_hooks_imports_update: "Review update".to_string(),
        settings_hooks_imports_remove: "Remove".to_string(),
        settings_hook_import_state_current: "Current snapshot".to_string(),
        settings_hook_import_state_update_available: "An update is available".to_string(),
        settings_hook_import_state_source_missing: {
            "The source is missing; the reviewed snapshot remains available".to_string()
        },
        settings_hook_import_state_update_check_failed: {
            "The update check failed; the reviewed snapshot remains available".to_string()
        },
        settings_hook_import_state_bundle_missing: {
            "Managed files are missing; review the source again to repair them".to_string()
        },
        settings_hooks_import_review_title: "Review hooks from".to_string(),
        settings_hooks_import_review_warning: {
            "These commands run with your account privileges. Confirm only after reviewing every command below."
                .to_string()
        },
        settings_hooks_import_timeout: "Effective timeout (seconds):".to_string(),
        settings_hooks_import_skipped: "Skipped".to_string(),
        settings_hooks_import_toggle: "Enable imported hooks from this source".to_string(),
        settings_hooks_import_confirm: "Import reviewed hooks".to_string(),
        settings_hooks_import_confirm_update: "Apply reviewed update".to_string(),
        settings_hooks_import_stale: {
            "The source changed. Nothing was written; review this refreshed plan.".to_string()
        },
        settings_hooks_import_remove_title: "Remove imported hooks?".to_string(),
        settings_hooks_import_remove_confirm_note: {
            "Only OpenBitFun's managed copy is removed. The Claude Code or Codex source is not changed."
                .to_string()
        },
        settings_hooks_import_remove_confirm: "Remove managed copy".to_string(),
        settings_hooks_import_store_invalid: {
            "Imported hook state needs repair".to_string()
        },
        settings_hooks_import_reset: "Reset managed state".to_string(),
        settings_hooks_import_reset_title: {
            "Reset invalid imported hook state?".to_string()
        },
        settings_hooks_import_reset_note: {
            "This removes the invalid OpenBitFun-managed index for this scope. Source application files are not changed."
                .to_string()
        },
        settings_hooks_import_reset_confirm: "Reset managed state".to_string(),
        settings_hooks_compat_reference: "Event and payload reference".to_string(),
        settings_hooks_compat_open: "Open docs".to_string(),
        settings_automation_hooks_title: "Agent Hooks".to_string(),
        settings_automation_hooks_desc: "Commands run as the user running OpenBitFun; enable hooks only from trusted sources".to_string(),
        settings_automation_qa_builtin_title: "Built-in actions".to_string(),
        settings_automation_qa_builtin_desc: "Edit the names and prompts of built-in actions, or turn off actions you do not need".to_string(),
        settings_automation_qa_custom_title: "Custom actions".to_string(),
        settings_automation_qa_custom_desc: "Save your own prompts for recurring tasks; triggering an action sends its prompt to the current session".to_string(),
        settings_automation_hook_locations_title: "Where hooks are declared".to_string(),
        settings_automation_hook_locations_desc: "Changes to hooks.json take effect without restarting OpenBitFun.".to_string(),
        settings_automation_hook_imports_title: "Imported hooks".to_string(),
        settings_automation_hook_imports_desc: "Review and manage hooks imported from Claude Code or Codex.".to_string(),
        settings_automation_hooks_compat_title: "Codex compatibility".to_string(),
        settings_automation_hooks_compat_desc: "OpenBitFun uses the Codex Hooks contract and can run compatible hook scripts directly.".to_string(),
        settings_mcp_servers_title: "MCP Server List".to_string(),
        settings_mcp_servers_desc: "A server must be enabled and connected before its tools are available to agents".to_string(),
        settings_websearch: "Web search".to_string(),
        settings_websearch_provider: "Provider".to_string(),
        settings_websearch_provider_description: {
            "The search provider the agent's web search tool uses.".to_string()
        },
        settings_category_application: "App & Interface".to_string(),
        settings_category_ai: "AI & Models".to_string(),
        settings_category_workspace: "Sessions & Workspace".to_string(),
        settings_category_tools: "Tools & Integrations".to_string(),
        settings_category_data: "Data & Maintenance".to_string(),
        settings_nav_search_placeholder: "Search settings…".to_string(),
        settings_nav_search_empty: "No matching settings".to_string(),
        settings_page_general_description: "Set startup, update, sleep, window, and notification behavior".to_string(),
        settings_page_appearance_description: "Choose the interface language, theme, and text size".to_string(),
        settings_page_terminal_description: "Choose the shell for new terminals and commands in local workspaces".to_string(),
        settings_page_models_description: "Connect model providers and choose models for each type of task".to_string(),
        settings_page_session_description: "Set the runtime for new sessions, title generation, and workspace search".to_string(),
        settings_worktrees_page_label: "Workspace & Git".to_string(),
        settings_page_worktrees_description: "Configure file search, commit attribution, and worktrees, and manage isolated working directories".to_string(),
        settings_page_execution_description: "Set tool approval rules, timeouts, and task concurrency".to_string(),
        settings_page_automation_description: "Set up reusable AI prompts and commands triggered by events".to_string(),
        settings_page_websearch_description: "Choose the service agents use to search the web".to_string(),
        settings_page_skills_description: "Review installed skills and control their global enabled state".to_string(),
        settings_page_agents_description: "Create and manage agents with a name, description, and system prompt".to_string(),
        settings_page_mcp_description: "Connect MCP servers to give agents access to external tools and data".to_string(),
        settings_acp_label: "ACP".to_string(),
        // Divergence D1: the shell has no SSH view, so the description does
        // not promise one (the web copy mentions SSH hosts too).
        settings_page_acp_description: "Connect external agents on this host through ACP".to_string(),
        acp_view_local: "Local".to_string(),
        acp_view_json: "Advanced JSON".to_string(),
        acp_registry_title: "Agents".to_string(),
        acp_registry_description: "Discover agents on the current execution host and enable them for selection in sessions".to_string(),
        acp_action_refresh: "Recheck".to_string(),
        acp_search_placeholder: "Search agents...".to_string(),
        acp_filter_all: "All".to_string(),
        acp_filter_enabled: "Enabled".to_string(),
        acp_filter_not_installed: "Not Installed".to_string(),
        acp_filter_invalid: "Invalid config".to_string(),
        acp_loading: "Loading ACP agents…".to_string(),
        acp_empty_title: "No matching ACP agents.".to_string(),
        acp_empty: "Adjust the search or the filter to see the built-in agents again.".to_string(),
        acp_status_enabled: "Enabled".to_string(),
        acp_status_ready: "Available".to_string(),
        acp_status_partial: "Partially available".to_string(),
        acp_status_checking: "Checking".to_string(),
        acp_status_cli_missing: "CLI missing".to_string(),
        acp_status_acp_missing: "ACP missing".to_string(),
        acp_status_path_invalid: "Path invalid".to_string(),
        acp_status_version_mismatch: "Version mismatch".to_string(),
        acp_status_connection_failed: "Connection failed".to_string(),
        acp_status_permission_denied: "Permission denied".to_string(),
        acp_status_config_invalid: "Invalid config".to_string(),
        acp_permission_ask: "Ask".to_string(),
        acp_permission_allow_once: "Auto approve".to_string(),
        acp_action_install_cli: "Install CLI".to_string(),
        acp_action_configure: "Configure ACP".to_string(),
        acp_action_view_instructions: "View instructions".to_string(),
        acp_action_view_error: "View error".to_string(),
        acp_action_add: "Add".to_string(),
        acp_action_add_config: "Add config".to_string(),
        acp_cli_missing_detail: "Oh My Pi is configured separately from OpenBitFun. Install the omp CLI and make sure it is on PATH, then recheck detection.".to_string(),
        acp_json_title: "Advanced JSON configuration".to_string(),
        acp_json_description: "Use when the list cannot express the settings you need; edits acpClients on the current execution host".to_string(),
        acp_json_warning: "ACP JSON and environment variables may contain API keys or access tokens. OpenBitFun saves them in the configuration on the current execution host. Do not paste them into online JSON validators, chats, or support tickets.".to_string(),
        acp_action_revert: "Revert".to_string(),
        acp_action_save_json: "Save JSON".to_string(),
        acp_notice_save_success: "ACP config saved".to_string(),
        acp_notice_install_success: "ACP CLI installed".to_string(),
        acp_notice_configure_success: "ACP adapter prepared".to_string(),
        acp_notice_config_added: "ACP agent config added".to_string(),
        acp_preset_opencode: "Native ACP support.".to_string(),
        acp_preset_dsh: "Uses OpenBitFun's bundled ACP bridge and the model settings from dsh.".to_string(),
        acp_preset_omp: "Native ACP support.".to_string(),
        acp_preset_claude_code: "Connects through the official ACP adapter.".to_string(),
        acp_preset_codex: "Connects through the official ACP adapter.".to_string(),
        acp_docs: "ACP docs".to_string(),
        acp_notice_open_link_failed: "Failed to open link".to_string(),
        acp_legacy_reject_warning: "Some agents still have Auto reject saved. The editor replaces it with Ask. Save and apply your current choices to make them take effect; until then, those agents will continue to reject requests automatically.".to_string(),
        acp_permission_save_apply: "Save and apply".to_string(),
        acp_action_cancel: "Cancel".to_string(),
        acp_install_confirm_local_host: "Current execution host".to_string(),
        acp_install_confirm_confirm: "Install on target host".to_string(),
        acp_install_confirm_package_unknown: "OpenBitFun could not verify the install package for this Agent, so installation was cancelled".to_string(),
        settings_page_usage_description: "See model request counts, token use, and cache hit rate".to_string(),
        settings_page_archived_description: "Review, restore, or permanently delete archived sessions".to_string(),
        settings_page_diagnostics_description: "Adjust logging and export troubleshooting information".to_string(),
        settings_jump_session_label: "Session & Workspace".to_string(),
        settings_jump_session_description: {
            "Covered by the global search overlay in this shell: search sessions and workspace content from the command bar."
                .to_string()
        },
        settings_jump_session_action: "Open Global Search".to_string(),
        websearch_provider_free: "Exa (free, MCP)".to_string(),
        websearch_provider_exa: "Exa Search API".to_string(),
        websearch_provider_tavily: "Tavily".to_string(),
        websearch_provider_http: "OpenBitFun Search HTTP".to_string(),
        websearch_http_endpoint: "Service URL".to_string(),
        websearch_http_endpoint_description: {
            "HTTPS is required. Plain HTTP is accepted only for localhost or a \
             loopback IP."
                .to_string()
        },
        websearch_http_auth_mode: "Authentication".to_string(),
        websearch_auth_none: "No authentication".to_string(),
        websearch_auth_bearer: "Bearer credential".to_string(),
        websearch_auth_header: "Restricted header credential".to_string(),
        websearch_http_header_name: "Credential header name".to_string(),
        websearch_http_header_name_description: {
            "Enter a custom header such as X-API-Key; choose Bearer credentials \
             for Authorization"
                .to_string()
        },
        websearch_credential: "API credential".to_string(),
        websearch_credential_description: {
            "Stored in the local encrypted vault; the key never lands in the settings \
             file."
                .to_string()
        },
        websearch_configured: "Credential configured".to_string(),
        websearch_not_configured: "No credential configured".to_string(),
        websearch_save: "Save credential".to_string(),
        websearch_no_credential: "This provider needs no credential.".to_string(),
        quick_action_title: "Quick actions".to_string(),
        quick_action_commit_label: "Commit".to_string(),
        quick_action_commit_prompt: "Commit all current code changes".to_string(),
        quick_action_create_pr_label: "Create PR".to_string(),
        quick_action_create_pr_prompt: "Create a Pull Request for the current branch".to_string(),
        settings_usage: "Usage statistics".to_string(),
        settings_usage_today: "Today".to_string(),
        settings_usage_week: "This week".to_string(),
        settings_usage_month: "This month".to_string(),
        settings_usage_all: "All time".to_string(),
        settings_usage_refresh: "Refresh".to_string(),
        settings_usage_loading: "Loading usage statistics…".to_string(),
        settings_usage_empty: "No usage recorded in this range.".to_string(),
        settings_usage_requests: "Requests".to_string(),
        settings_usage_cache_hit: "Cache hit".to_string(),
        settings_usage_groups: "Provider".to_string(),
        settings_usage_endpoints: "Endpoint".to_string(),
        settings_usage_filter: "Search these details".to_string(),
        settings_usage_filter_empty: "No matches".to_string(),
        usage_trend_title: "Detailed trend".to_string(),
        usage_trend_partial: {
            "Only the most recent trend is shown; totals still cover the full \
             range."
                .to_string()
        },
        usage_trend_input: "Input".to_string(),
        usage_trend_output: "Output".to_string(),
        usage_activity_title: "Daily Usage: Past Six Months".to_string(),
        usage_activity_description: "Fixed last six months (UTC).".to_string(),
        settings_usage_overview_title: "Selected Period".to_string(),
        settings_usage_distributions_title: "Usage Sources".to_string(),
        settings_usage_details_title: "More Details".to_string(),
        usage_activity_less: "Less".to_string(),
        usage_activity_more: "More".to_string(),
        usage_activity_mon: "Mon".to_string(),
        usage_activity_wed: "Wed".to_string(),
        usage_activity_fri: "Fri".to_string(),
        usage_activity_sun: "Sun".to_string(),
        settings_archived: "Archived sessions".to_string(),
        settings_archived_list: "Archived Sessions".to_string(),
        settings_archived_description: {
            "Grouped by workspace; restore a session to continue it, or delete it permanently with no undo"
                .to_string()
        },
        settings_archived_empty: "No archived sessions".to_string(),
        settings_archived_loading: "Loading".to_string(),
        settings_archived_refresh: "Refresh".to_string(),
        settings_archived_restore: "Restore".to_string(),
        settings_archived_delete_title: "Delete archived session".to_string(),
        settings_archived_delete_all: "Delete All Archived".to_string(),
        settings_archived_delete_all_confirm_title: "Delete All Archived Sessions".to_string(),
        settings_diagnostics: "Diagnostics".to_string(),
        settings_logging_level: "Log level".to_string(),
        settings_logging_level_description: {
            "Applies immediately and persists for future launches.".to_string()
        },
        settings_logs_dir: "Log directory".to_string(),
        settings_logs_open: "Open".to_string(),
        settings_diagnostics_note: {
            "Logs are written per session under the logs root; the bundle \
             keeps the five most recent sessions."
                .to_string()
        },
        settings_diagnostics_export: "Diagnostics Bundle".to_string(),
        settings_diagnostics_export_description: {
            "Export recent logs, crash reports, and runtime information as a \
             ZIP file; nothing is uploaded automatically."
                .to_string()
        },
        settings_diagnostics_export_action: "Export Diagnostics".to_string(),
        settings_diagnostics_export_title: "Export diagnostics bundle?".to_string(),
        settings_diagnostics_export_warning: {
            "The bundle may contain local paths, runtime details, and error \
             information. Save it only to a trusted location and review it \
             before sharing."
                .to_string()
        },
        settings_diagnostics_export_confirm: "Export".to_string(),
        settings_diagnostics_export_success: "Diagnostics bundle exported".to_string(),
        settings_diagnostics_export_failed: "Failed to export diagnostics bundle".to_string(),
        settings_diagnostics_previous_crash: "Previous session ended unexpectedly".to_string(),
        settings_diagnostics_previous_unclean: {
            "Previous session did not record a normal shutdown".to_string()
        },
        settings_memory_label: "Memory".to_string(),
        settings_page_memory_description: {
            "Control how agents create and use memories from sessions".to_string()
        },
        memory_scope_notice: {
            "Settings, candidate sessions, and memory files belong to the current execution host; other devices keep separate memories".to_string()
        },
        memory_section_basic: "Basics".to_string(),
        memory_section_basic_note: {
            "Turning memory off keeps existing data; use Memory actions to clear it separately".to_string()
        },
        memory_section_models: "Models".to_string(),
        memory_section_models_note: {
            "Unless set separately, extraction uses the primary model and consolidation uses the extraction model".to_string()
        },
        memory_section_advanced: "Advanced settings".to_string(),
        memory_section_advanced_note: {
            "Limit which sessions are processed and how much memory work runs; defaults usually suffice".to_string()
        },
        memory_enabled: "Memory".to_string(),
        memory_enabled_description: {
            "Generate memories from completed sessions for use in later sessions".to_string()
        },
        memory_generate_btw: "Generate memories from BTW sessions".to_string(),
        memory_generate_btw_description: {
            "Include side conversations (BTW) when generating memories".to_string()
        },
        memory_external_context: "External context handling".to_string(),
        memory_external_context_description: {
            "Choose how memory extraction handles external tool results, such as web searches and web pages".to_string()
        },
        memory_policy_clear_tool_results: "Clear external tool results".to_string(),
        memory_policy_allow: "Allow external tool results".to_string(),
        memory_policy_skip_session: "Skip the session".to_string(),
        memory_actions_label: "Memory actions".to_string(),
        memory_actions_description: {
            "Manage memory on the current execution host; clearing it permanently deletes memory data".to_string()
        },
        memory_action_manage: "Manage".to_string(),
        memory_action_open_directory: "Open Memory Folder".to_string(),
        memory_action_reset_settings: "Reset Memory Settings".to_string(),
        memory_action_reset_memory: "Permanently clear memory".to_string(),
        memory_reset_settings_confirm_title: "Reset memory settings?".to_string(),
        memory_reset_settings_confirm_message: {
            "This restores the memory switches, models, and advanced parameters on the current execution host to their defaults. Existing memory data is not deleted.".to_string()
        },
        memory_reset_settings_confirm_action: "Restore defaults".to_string(),
        memory_reset_confirm_message: {
            "This permanently deletes all memory data and memory workspace files on the current execution host. This cannot be undone.".to_string()
        },
        memory_reset_confirm_action: "Permanently clear".to_string(),
        memory_extract_model: "Extraction model".to_string(),
        memory_extract_model_description: {
            "Model used to distill candidate memories from session transcripts.".to_string()
        },
        memory_consolidation_model: "Consolidation model".to_string(),
        memory_consolidation_model_description: {
            "Model used to merge candidates and update long-term memory files.".to_string()
        },
        memory_model_primary: "Primary model".to_string(),
        memory_model_fast: "Fast model".to_string(),
        memory_model_follow_primary: "Follow primary model".to_string(),
        memory_model_follow_extraction: "Follow extraction model".to_string(),
        memory_min_idle_hours: "Minimum idle time".to_string(),
        memory_min_idle_hours_description: {
            "How long a completed session must sit idle before extraction.".to_string()
        },
        memory_max_age_days: "Session lookback days".to_string(),
        memory_max_age_days_description: {
            "Only extract from sessions within this many days; cannot exceed candidate retention".to_string()
        },
        memory_per_startup: "Extraction limit per run".to_string(),
        memory_per_startup_description: "Maximum sessions extracted in each memory run".to_string(),
        memory_scan_limit: "Scan limit per run".to_string(),
        memory_scan_limit_description: {
            "Maximum completed sessions checked when looking for candidates".to_string()
        },
        memory_concurrency: "Extraction concurrency".to_string(),
        memory_concurrency_description: {
            "Maximum sessions undergoing memory extraction at the same time".to_string()
        },
        memory_raw_limit: "Consolidation candidate limit".to_string(),
        memory_raw_limit_description: {
            "Maximum extracted results combined in each long-term memory consolidation".to_string()
        },
        memory_retention_days: "Candidate retention".to_string(),
        memory_retention_days_description: {
            "Unused extracted results expire from consolidation after this many days; cannot be shorter than session lookback".to_string()
        },
        memory_action_expand_advanced: "Expand advanced settings".to_string(),
        memory_action_collapse_advanced: "Collapse advanced settings".to_string(),
        memory_settings_reset: "Memory settings reset".to_string(),
        memory_settings_reset_failed: "Failed to reset memory settings".to_string(),
        memory_open_directory_failed: "Failed to open memory folder".to_string(),
        memory_reset: "Memory permanently cleared".to_string(),
        memory_reset_failed: "Failed to permanently clear memory".to_string(),
        settings_shortcuts_label: "Keyboard Shortcuts".to_string(),
        settings_page_shortcuts_description: "The keyboard shortcuts in effect".to_string(),
        shortcuts_fixed_note: {
            "The shell's bindings are fixed per release; there is no remapping yet".to_string()
        },
        shortcuts_section_global: "Global".to_string(),
        shortcuts_section_overlays: "Overlays and menus".to_string(),
        shortcuts_section_terminal: "Terminal".to_string(),
        shortcuts_section_editing: "Text editing".to_string(),
        shortcuts_overlay_dismiss: {
            "Close the current overlay, cancel a menu, or leave the search panel".to_string()
        },
        shortcuts_overlay_move: "Move the selection in menus and lists".to_string(),
        shortcuts_overlay_confirm: {
            "Confirm the selected menu item, or run the global search".to_string()
        },
        shortcuts_terminal_copy: "Copy the terminal's selection".to_string(),
        shortcuts_terminal_paste: "Paste into the terminal".to_string(),
        shortcuts_terminal_note: {
            "Every other key goes to the terminal as-is; control characters such as Ctrl+C are not intercepted".to_string()
        },
        shortcuts_editing_note: {
            "Text fields use the system's standard editing keys (copy, paste, select all, word motion), handled by the text engine".to_string()
        },
        plugin_ui_removed: "Contribution removed".to_string(),
        plugin_ui_rejected: "Plugin UI update rejected".to_string(),
        plugin_ui_timeout: "Plugin interaction timed out".to_string(),
        settings_ui_font: "Interface and chat".to_string(),
        settings_ui_font_description: "Applies to the interface and chat text; the default level is the size the shell used before this setting.".to_string(),
        ui_font_level_compact: "Compact".to_string(),
        ui_font_level_small: "Small".to_string(),
        ui_font_level_default: "Default".to_string(),
        ui_font_level_medium: "Medium".to_string(),
        ui_font_level_large: "Large".to_string(),
        ui_font_level_custom: "Custom".to_string(),
        settings_ui_font_custom: "Custom size (12-20 px)".to_string(),
        settings_ui_font_custom_description: "Interface text size in device pixels; the web interface stores and applies the same value.".to_string(),
        settings_stream_section: "Model response timeouts".to_string(),
        settings_stream_section_note: "Changes take effect on the next turn.".to_string(),
        settings_stream_ttft: "First response timeout (seconds)".to_string(),
        settings_stream_ttft_description: "Longest wait for the first streamed chunk; 0 waits indefinitely.".to_string(),
        settings_stream_idle: "Response idle timeout (seconds)".to_string(),
        settings_stream_idle_description: "Longest gap between streamed chunks; 0 waits indefinitely.".to_string(),
        settings_proxy_section: "Proxy".to_string(),
        settings_proxy_section_note: "When enabled, AI requests are sent through the proxy.".to_string(),
        settings_proxy_enable: "Enable proxy".to_string(),
        settings_proxy_enable_description: "Applies to new requests; a turn already streaming keeps its connection.".to_string(),
        settings_proxy_url: "Proxy URL".to_string(),
        settings_proxy_url_description: "HTTP / HTTPS / SOCKS5, e.g. http://127.0.0.1:7890 or socks5://127.0.0.1:1080".to_string(),
        settings_proxy_username: "Username (optional)".to_string(),
        settings_proxy_password: "Password (optional)".to_string(),
        settings_proxy_password_description: "Proxy authentication password, stored in the shared config file and shown here in clear text.".to_string(),
        settings_tool_timeout: "Tool timeout (seconds)".to_string(),
        settings_tool_timeout_description: "Longest a single tool operation may run; 0 waits indefinitely.".to_string(),
        settings_question_timeout: "Question timeout (seconds)".to_string(),
        settings_question_timeout_description: "Longest wait for you to start answering a question; 0 waits indefinitely.".to_string(),
        settings_subagent_model: "Default subagent model".to_string(),
        settings_subagent_model_description: "Used by subagents without an individual model override.".to_string(),
        settings_subagent_model_settings:
            "Go to the Agents page to set models for individual subagents".to_string(),
        settings_subagent_model_inherit: "Inherit parent session".to_string(),
        settings_subagent_model_fast: "Fast model".to_string(),
        settings_subagent_model_primary: "Primary model".to_string(),
        settings_subagent_batch: "Task concurrency".to_string(),
        settings_subagent_batch_safe: "Safe parallel".to_string(),
        settings_subagent_batch_safe_description: "Run tasks together only when they do not modify content.".to_string(),
        settings_subagent_batch_parallel: "Prefer parallel".to_string(),
        settings_subagent_batch_parallel_description: "Run multiple tasks together whenever possible.".to_string(),
        settings_subagent_concurrency: "Concurrent task limit".to_string(),
        settings_subagent_concurrency_description: "Maximum number of regular subagent tasks running at once.".to_string(),
        settings_swarm_concurrency: "Swarm worker concurrency".to_string(),
        settings_swarm_concurrency_description: "Concurrency limit for worker and reviewer agents in Swarm.".to_string(),
        settings_deferred_tools_enable: "Enable".to_string(),
        settings_deferred_tools_warning: "Turning this off loads every tool definition and may increase model usage.".to_string(),
        settings_permissions_policy_title: "Tool permissions".to_string(),
        settings_permissions_policy_desc: "Choose when tool actions need approval and which actions are allowed or denied".to_string(),
        settings_permissions_deferred_title: "Load tools as needed".to_string(),
        settings_permissions_deferred_desc: "Provide tool details to the model on demand to reduce context usage".to_string(),
        settings_permissions_tool_presentation_title: "Tool presentation".to_string(),
        settings_permissions_tool_presentation_desc: "Choose how the model accesses tools."
            .to_string(),
        settings_tool_presentation_mode: "Tool presentation".to_string(),
        settings_tool_presentation_description: "Choose how tools are exposed to the model. This setting applies only to sessions created after it changes.".to_string(),
        settings_tool_presentation_native: "Native tools".to_string(),
        settings_tool_presentation_ptc: "PTC (TypeScript)".to_string(),
        settings_tool_presentation_ptc_hint: "PTC executes TypeScript through RunCode."
            .to_string(),
        settings_permissions_mode: "Default permission mode".to_string(),
        settings_permissions_mode_description:
            "Used for new sessions and can be changed temporarily from the chat input."
                .to_string(),
        settings_permissions_mode_ask: "Ask for confirmation".to_string(),
        settings_permissions_mode_ask_description:
            "Confirm before files change, commands run, or external services are accessed."
                .to_string(),
        settings_permissions_mode_auto: "Auto approve".to_string(),
        settings_permissions_mode_auto_description:
            "Automatically allow actions that would normally need confirmation.".to_string(),
        settings_permissions_mode_full: "Full access".to_string(),
        settings_permissions_mode_full_description:
            "Do not confirm each tool action.".to_string(),
        settings_permissions_show_in_chat_input: "Show permission mode selector".to_string(),
        settings_permissions_show_in_chat_input_description:
            "Show the permission mode below the chat input so it can be changed for the current session.".to_string(),
        settings_permissions_full_access_title: "Enable full access?".to_string(),
        settings_permissions_full_access_message:
            "Full access allows tools by default without asking each time.".to_string(),
        settings_permissions_full_access_confirm: "Enable full access".to_string(),
        settings_permissions_execution_title: "Tool execution".to_string(),
        settings_permissions_execution_desc: "Tool execution and waiting for an answer have separate timers; set either to 0 for no timeout".to_string(),
        settings_permissions_subagents_title: "Subagents".to_string(),
        settings_permissions_subagents_desc: "Regular subagents and Swarm use separate concurrency limits".to_string(),
        settings_permissions_json_repair_title: "Tool Argument JSON Repair".to_string(),
        settings_permissions_json_repair_desc: "Applies to the next model round.".to_string(),
        settings_permissions_review_title: "Review capacity".to_string(),
        settings_permissions_review_desc: "Control how supplemental reviews use model capacity; changing concurrency keeps the review scope unchanged".to_string(),
        settings_json_repair: "Repair malformed tool arguments".to_string(),
        settings_json_repair_description: "Attempt to repair argument JSON only after the provider reports a completed tool call; repair runs before normal tool validation.".to_string(),
        settings_review_parallelism: "Review parallelism".to_string(),
        settings_review_parallelism_description: "Run supplemental review tasks one at a time or two at once.".to_string(),
        settings_review_queue_wait: "Capacity wait time".to_string(),
        settings_review_queue_wait_description: "How long optional reviews wait for provider capacity; excludes checks already running.".to_string(),
        settings_default_harness_section: "Default Agent Harness".to_string(),
        settings_default_harness_section_note: "Applies to new standard sessions; assistants, ACP, and sessions with an explicit mode are unaffected".to_string(),
        settings_default_harness_strategy: "Default behavior".to_string(),
        settings_default_harness_follow_last: "Follow last selection".to_string(),
        settings_default_harness_fixed: "Use a fixed default".to_string(),
        settings_default_harness_fixed_description: "New sessions always use the selected Harness; existing sessions are unchanged.".to_string(),
        settings_default_harness_fixed_mode: "Default Harness".to_string(),
        settings_session_title_section: "Auto Session Title".to_string(),
        settings_session_title_section_note: "AI automatically generates concise titles for new conversations".to_string(),
        settings_session_title_enable: "Enable".to_string(),
        settings_session_title_model: "Session Title Generation Model".to_string(),
        settings_session_title_model_inherit: "Current session model".to_string(),
        settings_session_title_model_fast: "Fast Model".to_string(),
        settings_session_title_model_primary: "Primary Model".to_string(),
        settings_session_title_models_empty: "No available models. Please add models in \"Models\" first.".to_string(),
        settings_session_title_model_unavailable: "disabled or missing".to_string(),
        value_compatibility: "compatibility setting".to_string(),
        settings_worktree_section: "Session isolation".to_string(),
        settings_worktree_section_note: "Sessions run in the project directory by default. The worktree toggle next to the branch moves a session into its own checkout.".to_string(),
        settings_worktree_root: "Worktree root".to_string(),
        settings_worktree_root_description: "Path on the current execution host; changes affect only newly created worktrees".to_string(),
        settings_worktree_branch_prefix: "Branch prefix".to_string(),
        settings_worktree_branch_prefix_description: "Suggested prefix when creating a branch for a detached worktree.".to_string(),
        settings_worktree_copy_changes: "Copy local changes by default".to_string(),
        settings_worktree_copy_changes_description: "Copy uncommitted project changes into new worktrees, preserving staged changes".to_string(),
        settings_worktree_auto_delete: "Automatically delete old worktrees".to_string(),
        settings_worktree_auto_delete_description: "After creating a worktree, remove the oldest managed worktrees that are safe to delete if the retention count is exceeded".to_string(),
        settings_worktree_auto_delete_limit: "Worktrees to keep per workspace".to_string(),
        settings_worktree_auto_delete_limit_description: "Worktrees with sessions, local changes, unpublished commits, or Git locks are kept, so the actual count may exceed this value".to_string(),
        settings_workspace_search_title: "Accelerated workspace search".to_string(),
        settings_workspace_search_description: "Speed up file search in large workspaces. Standard search remains available when this is off.".to_string(),
        settings_workspace_search_enable: "Enable accelerated workspace search".to_string(),
        settings_git_attribution_title: "Commit attribution".to_string(),
        settings_git_attribution_scope: "Shared across workspaces on this execution host. Applies the next time a built-in commit workflow is loaded.".to_string(),
        settings_git_attribution_label: "Add OpenBitFun as a co-author".to_string(),
        settings_worktree_reset: "Restore defaults".to_string(),
        settings_worktree_management_title: "Workspace worktrees".to_string(),
        settings_worktree_management_description: "Grouped by workspace; only worktrees inside the OpenBitFun worktree root on this execution host are listed".to_string(),
        settings_worktree_management_refresh: "Refresh worktrees".to_string(),
        settings_worktree_management_loading: "Scanning OpenBitFun worktrees…".to_string(),
        settings_worktree_management_load_failed: "Unable to load the OpenBitFun worktree list.".to_string(),
        settings_worktree_management_retry: "Retry".to_string(),
        settings_worktree_management_empty: "No OpenBitFun worktrees yet".to_string(),
        settings_worktree_lifecycle_permanent: "Kept permanently".to_string(),
        settings_worktree_lifecycle_external: "External".to_string(),
        settings_worktree_state_dirty: "Local changes".to_string(),
        settings_worktree_state_unpublished: "Unpublished commits".to_string(),
        settings_worktree_state_locked: "Git locked".to_string(),
        settings_worktree_state_missing: "Directory missing".to_string(),
        settings_worktree_delete_action: "Delete".to_string(),
        settings_worktree_delete_title: "Delete this worktree?".to_string(),
        settings_worktree_delete_message: "The checkout will be deleted. This cannot be undone.".to_string(),
        settings_worktree_delete_force_title: "Delete and discard local work?".to_string(),
        settings_worktree_delete_force_message: "This worktree has local changes or dangling commits. They will be lost permanently.".to_string(),
        settings_worktree_delete_message_sessions: "The checkout will be deleted. Associated sessions stay in history but lose this checkout.".to_string(),
        settings_worktree_delete_force_message_sessions: "Local changes or dangling commits will be lost permanently. Associated sessions stay in history but no longer have this checkout.".to_string(),
        settings_worktree_delete_force_action: "Delete and discard".to_string(),
        settings_worktree_delete_cancel: "Cancel".to_string(),
        settings_worktree_protection_locked: "Unlock this worktree in Git before deleting it.".to_string(),
        settings_worktree_protection_missing: "The worktree directory is missing; clean up the stale Git record manually.".to_string(),
        settings_worktree_error_locked: "This worktree is locked by Git and cannot be deleted.".to_string(),
        settings_worktree_error_dirty: "This worktree still has local changes.".to_string(),
        settings_worktree_error_unpublished: "This worktree still has unpublished commits.".to_string(),
        settings_worktree_error_not_found: "This worktree no longer exists. Refresh the list and try again.".to_string(),
        settings_worktree_error_remote_unsupported: "Remote SSH worktrees are not supported yet.".to_string(),
        settings_worktree_error_delete_failed: "Unable to delete the worktree.".to_string(),
    }
}

/// Resolves the subagent name pool in pool order: the web client hashes a run
/// identity to an index into this list, so the shell keeps the same order and
/// reads the localized copy out of the locale files.
async fn fetch_subagent_names(
    service: &openbitfun_core::service::i18n::I18nService,
) -> Vec<String> {
    let mut names = Vec::with_capacity(crate::subagent_identity::NAME_KEYS.len());
    for key in crate::subagent_identity::NAME_KEYS {
        names.push(service.t(&format!("gpui-shell-subagent-name-{key}")).await);
    }
    names
}

fn fetch_text(cx: &App, locale: LocaleId) -> ShellText {
    let Some(service) = cx.foreground_executor().block_on(get_global_i18n_service()) else {
        tracing::warn!("i18n service unavailable, keeping English fallback copy");
        return fallback_text(locale);
    };

    cx.foreground_executor()
        .block_on(service.set_locale(locale))
        .unwrap_or_else(|err| {
            tracing::warn!("Failed to switch i18n locale: {}", err);
        });

    // Each lookup blocks the caller exactly as the former `.await` chain did:
    // the only caller wraps this in `block_on` anyway. A sync body is not
    // cosmetic — 131 `.await` points over a ShellText-sized partial state
    // made rustc's async state machine explode typeck memory (the source of
    // the old monolithic shell's ~48GB compile peak).
    let t = |key: &str| cx.foreground_executor().block_on(service.t(key));

    ShellText {
        locale,
        theme: t("gpui-shell-theme"),
        language: t("gpui-shell-language"),
        status_starting: t("gpui-shell-status-starting"),
        sessions_title: t("gpui-shell-sessions-title"),
        sessions_empty: t("gpui-shell-sessions-empty"),
        nav_assistant: t("gpui-shell-nav-assistant"),
        nav_tasks: t("gpui-shell-nav-tasks"),
        sessions_reload: t("gpui-shell-sessions-reload"),
        sessions_new: t("gpui-shell-sessions-new"),
        sessions_add_group: t("gpui-shell-sessions-add-group"),
        sessions_show_archived: t("gpui-shell-sessions-show-archived"),
        sessions_show_more: t("gpui-shell-sessions-show-more"),
        sessions_collapse: t("gpui-shell-sessions-collapse"),
        miniapps_title: t("gpui-shell-miniapps-title"),
        miniapps_empty: t("gpui-shell-miniapps-empty"),
        extensions_title: t("gpui-shell-extensions-title"),
        extensions_empty: t("gpui-shell-extensions-empty"),
        extensions_open_acp: t("gpui-shell-extensions-open-acp"),
        sessions_show_less: t("gpui-shell-sessions-show-less"),
        sessions_view_all_tooltip: t("gpui-shell-sessions-view-all-tooltip"),
        sessions_view_grouped_tooltip: t("gpui-shell-sessions-view-grouped-tooltip"),
        sessions_filter_tooltip: t("gpui-shell-sessions-filter-tooltip"),
        sessions_ordering: t("gpui-shell-sessions-ordering"),
        sessions_order_updated: t("gpui-shell-sessions-order-updated"),
        sessions_order_created: t("gpui-shell-sessions-order-created"),
        sessions_order_name: t("gpui-shell-sessions-order-name"),
        sessions_show: t("gpui-shell-sessions-show"),
        sessions_show_all: t("gpui-shell-sessions-show-all"),
        sessions_show_unread: t("gpui-shell-sessions-show-unread"),
        sessions_filters: t("gpui-shell-sessions-filters"),
        // "Running" is a shared term; a dedicated key would trip the
        // shared-term duplicate ratchet (same alias the web
        // `$t(shared:statuses.running)` uses).
        sessions_status_running: t("shared.statuses.running"),
        sessions_reset_filters: t("gpui-shell-sessions-reset-filters"),
        sessions_no_matches: t("gpui-shell-sessions-no-matches"),
        sessions_collapse_all: t("gpui-shell-sessions-collapse-all"),
        sessions_mark_all_read: t("gpui-shell-sessions-mark-all-read"),
        sessions_copy_path: t("gpui-shell-sessions-copy-path"),
        sessions_reveal: t("gpui-shell-sessions-reveal"),
        session_rename: t("gpui-shell-session-rename"),
        session_rename_title: t("gpui-shell-session-rename-title"),
        session_archive: t("gpui-shell-session-archive"),
        session_unarchive: t("gpui-shell-session-unarchive"),
        session_delete: t("gpui-shell-session-delete"),
        session_delete_title: t("gpui-shell-session-delete-title"),
        session_delete_warning: t("gpui-shell-session-delete-warning"),
        session_delete_confirm: t("gpui-shell-session-delete-confirm"),
        session_action_confirm: t("gpui-shell-session-action-confirm"),
        session_action_cancel: t("gpui-shell-session-action-cancel"),
        session_fork: t("gpui-shell-session-fork"),
        session_compact: t("gpui-shell-session-compact"),
        session_export: t("gpui-shell-session-export"),
        fork_at_turn: t("gpui-shell-fork-at-turn"),
        fork_before_turn: t("gpui-shell-fork-before-turn"),
        export_title: t("gpui-shell-export-title"),
        export_done_prefix: t("gpui-shell-export-done-prefix"),
        compaction_started: t("gpui-shell-compaction-started"),
        usage_title: t("gpui-shell-usage-title"),
        usage_turns: t("gpui-shell-usage-turns"),
        usage_input_tokens: t("gpui-shell-usage-input-tokens"),
        usage_output_tokens: t("gpui-shell-usage-output-tokens"),
        usage_cached_tokens: t("gpui-shell-usage-cached-tokens"),
        usage_total_tokens: t("gpui-shell-usage-total-tokens"),
        usage_wall_time: t("gpui-shell-usage-wall-time"),
        usage_active_time: t("gpui-shell-usage-active-time"),
        usage_models: t("gpui-shell-usage-models"),
        insights_title: t("gpui-shell-insights-title"),
        insights_generate: t("gpui-shell-insights-generate"),
        insights_generating: t("gpui-shell-insights-generating"),
        insights_loading: t("gpui-shell-insights-loading"),
        insights_empty_title: t("gpui-shell-insights-empty-title"),
        insights_empty: t("gpui-shell-insights-empty"),
        insights_sessions: t("gpui-shell-insights-sessions"),
        insights_analyzed: t("gpui-shell-insights-analyzed"),
        insights_messages: t("gpui-shell-insights-messages"),
        insights_days_covered: t("gpui-shell-insights-days-covered"),
        insights_whats_working: t("gpui-shell-insights-whats-working"),
        insights_whats_hindering: t("gpui-shell-insights-whats-hindering"),
        insights_quick_wins: t("gpui-shell-insights-quick-wins"),
        insights_looking_ahead: t("gpui-shell-insights-looking-ahead"),
        insights_model_label: t("gpui-shell-insights-model-label"),
        insights_model_primary: t("gpui-shell-insights-model-primary"),
        insights_range_label: t("gpui-shell-insights-range-label"),
        insights_history: t("gpui-shell-insights-history"),
        insights_keep_latest: t("gpui-shell-insights-keep-latest"),
        insights_md_additions: t("gpui-shell-insights-md-additions"),
        insights_features_to_try: t("gpui-shell-insights-features-to-try"),
        insights_usage_patterns: t("gpui-shell-insights-usage-patterns"),
        insights_try_this_prompt: t("gpui-shell-insights-try-this-prompt"),
        insights_copy: t("gpui-shell-insights-copy"),
        tasks_title: t("gpui-shell-tasks-title"),
        tasks_empty_title: t("gpui-shell-tasks-empty-title"),
        tasks_empty: t("gpui-shell-tasks-empty"),
        tasks_run_now: t("gpui-shell-tasks-run-now"),
        tasks_subtitle: t("gpui-shell-tasks-subtitle"),
        tasks_overview: t("gpui-shell-tasks-overview"),
        tasks_overview_unit: t("gpui-shell-tasks-overview-unit"),
        tasks_overview_total: t("gpui-shell-tasks-overview-total"),
        tasks_overview_due_soon: t("gpui-shell-tasks-overview-due-soon"),
        tasks_wd_mon: t("gpui-shell-tasks-wd-mon"),
        tasks_wd_tue: t("gpui-shell-tasks-wd-tue"),
        tasks_wd_wed: t("gpui-shell-tasks-wd-wed"),
        tasks_wd_thu: t("gpui-shell-tasks-wd-thu"),
        tasks_wd_fri: t("gpui-shell-tasks-wd-fri"),
        tasks_wd_sat: t("gpui-shell-tasks-wd-sat"),
        tasks_wd_sun: t("gpui-shell-tasks-wd-sun"),
        tasks_cal_title: t("gpui-shell-tasks-cal-title"),
        tasks_cal_prev: t("gpui-shell-tasks-cal-prev"),
        tasks_cal_next: t("gpui-shell-tasks-cal-next"),
        tasks_cal_today: t("gpui-shell-tasks-cal-today"),
        tasks_day_detail_title: t("gpui-shell-tasks-day-detail-title"),
        tasks_day_clear: t("gpui-shell-tasks-day-clear"),
        tasks_day_empty: t("gpui-shell-tasks-day-empty"),
        tasks_badge_running: t("gpui-shell-tasks-badge-running"),
        tasks_badge_next_run: t("gpui-shell-tasks-badge-next-run"),
        tasks_badge_overdue: t("gpui-shell-tasks-badge-overdue"),
        tasks_inactive_paused: t("gpui-shell-tasks-inactive-paused"),
        tasks_inactive_done: t("gpui-shell-tasks-inactive-done"),
        tasks_inactive_invalid: t("gpui-shell-tasks-inactive-invalid"),
        tasks_list_empty: t("gpui-shell-tasks-list-empty"),
        tasks_new: t("gpui-shell-tasks-new"),
        tasks_editor_create_title: t("gpui-shell-tasks-editor-create-title"),
        tasks_editor_edit_title: t("gpui-shell-tasks-editor-edit-title"),
        tasks_editor_create_desc: t("gpui-shell-tasks-editor-create-desc"),
        tasks_editor_edit_desc: t("gpui-shell-tasks-editor-edit-desc"),
        tasks_editor_btn_create: t("gpui-shell-tasks-editor-btn-create"),
        tasks_editor_btn_save: t("gpui-shell-tasks-editor-btn-save"),
        tasks_editor_cron_hint: t("gpui-shell-tasks-editor-cron-hint"),
        tasks_editor_name_placeholder: t("gpui-shell-tasks-editor-name-placeholder"),
        tasks_editor_prompt_placeholder: t("gpui-shell-tasks-editor-prompt-placeholder"),
        tasks_editor_name: t("gpui-shell-tasks-editor-name"),
        tasks_editor_prompt: t("gpui-shell-tasks-editor-prompt"),
        tasks_editor_schedule: t("gpui-shell-tasks-editor-schedule"),
        tasks_editor_kind_at: t("gpui-shell-tasks-editor-kind-at"),
        tasks_editor_kind_every: t("gpui-shell-tasks-editor-kind-every"),
        tasks_editor_kind_cron: t("gpui-shell-tasks-editor-kind-cron"),
        tasks_editor_at: t("gpui-shell-tasks-editor-at"),
        tasks_editor_at_hint: t("gpui-shell-tasks-editor-at-hint"),
        tasks_editor_every: t("gpui-shell-tasks-editor-every"),
        tasks_editor_anchor: t("gpui-shell-tasks-editor-anchor"),
        tasks_editor_anchor_hint: t("gpui-shell-tasks-editor-anchor-hint"),
        tasks_editor_unit_minute: t("gpui-shell-tasks-editor-unit-minute"),
        tasks_editor_unit_hour: t("gpui-shell-tasks-editor-unit-hour"),
        tasks_editor_unit_day: t("gpui-shell-tasks-editor-unit-day"),
        tasks_editor_cron_expr: t("gpui-shell-tasks-editor-cron-expr"),
        tasks_editor_cron_tz: t("gpui-shell-tasks-editor-cron-tz"),
        tasks_editor_target: t("gpui-shell-tasks-editor-target"),
        tasks_editor_target_workspace: t("gpui-shell-tasks-editor-target-workspace"),
        tasks_editor_target_session: t("gpui-shell-tasks-editor-target-session"),
        tasks_editor_session_placeholder: t("gpui-shell-tasks-editor-session-placeholder"),
        tasks_editor_no_sessions: t("gpui-shell-tasks-editor-no-sessions"),
        tasks_editor_enabled: t("gpui-shell-tasks-editor-enabled"),
        tasks_editor_error_name: t("gpui-shell-tasks-editor-error-name"),
        tasks_editor_error_prompt: t("gpui-shell-tasks-editor-error-prompt"),
        tasks_editor_error_prompt_long: t("gpui-shell-tasks-editor-error-prompt-long"),
        tasks_editor_error_at: t("gpui-shell-tasks-editor-error-at"),
        tasks_editor_error_every: t("gpui-shell-tasks-editor-error-every"),
        tasks_editor_error_anchor: t("gpui-shell-tasks-editor-error-anchor"),
        tasks_editor_error_cron: t("gpui-shell-tasks-editor-error-cron"),
        tasks_editor_error_session: t("gpui-shell-tasks-editor-error-session"),
        tasks_editor_error_workspace: t("gpui-shell-tasks-editor-error-workspace"),
        panel_pop_out: t("gpui-shell-panel-pop-out"),
        panel_close: t("gpui-shell-panel-close"),
        panel_maximize: t("gpui-shell-panel-maximize"),
        panel_restore: t("gpui-shell-panel-restore"),
        panel_swap: t("gpui-shell-panel-swap"),
        panel_open: t("gpui-shell-panel-open"),
        sidebar_toggle: t("gpui-shell-sidebar-toggle"),
        files_title: t("gpui-shell-files-title"),
        files_loading: t("gpui-shell-files-loading"),
        files_empty: t("gpui-shell-files-empty"),
        files_empty_title: t("gpui-shell-files-empty-title"),
        files_viewer_empty: t("gpui-shell-files-viewer-empty"),
        files_viewer_empty_title: t("gpui-shell-files-viewer-empty-title"),
        files_reload: t("gpui-shell-files-reload"),
        files_filter_placeholder: t("gpui-shell-files-filter-placeholder"),
        files_no_matches: t("gpui-shell-files-no-matches"),
        files_too_large: t("gpui-shell-files-too-large"),
        files_binary: t("gpui-shell-files-binary"),
        files_truncated: t("gpui-shell-files-truncated"),
        search_trigger: t("gpui-shell-search-trigger"),
        search_files_tab: t("gpui-shell-search-files-tab"),
        search_sessions_tab: t("gpui-shell-search-sessions-tab"),
        search_run: t("shared.tools.search"),
        search_running: t("gpui-shell-search-running"),
        search_empty: t("gpui-shell-search-empty"),
        search_empty_title: t("gpui-shell-search-empty-title"),
        search_unavailable: t("gpui-shell-search-unavailable"),
        search_truncated: t("gpui-shell-search-truncated"),
        terminal_new_tab: t("gpui-shell-terminal-new-tab"),
        terminal_new_tab_default: t("gpui-shell-terminal-new-tab-default"),
        terminal_profiles_manage: t("gpui-shell-terminal-profiles-manage"),
        settings_terminal_profiles: t("gpui-shell-terminal-profiles"),
        settings_terminal_profiles_description: t("gpui-shell-terminal-profiles-desc"),
        terminal_profiles_empty: t("gpui-shell-terminal-profiles-empty"),
        terminal_profile_add: t("gpui-shell-terminal-profile-add"),
        terminal_profile_edit: t("shared.tools.edit"),
        terminal_profile_delete: t("gpui-shell-terminal-profile-delete"),
        terminal_profile_dialog_title: t("gpui-shell-terminal-profile-title"),
        terminal_profile_name: t("gpui-shell-terminal-profile-name"),
        terminal_profile_working_dir: t("gpui-shell-terminal-profile-dir"),
        terminal_profile_startup_command: t("gpui-shell-terminal-profile-command"),
        model_picker: t("gpui-shell-model-picker"),
        model_picker_empty: t("gpui-shell-model-picker-empty"),
        model_default: t("gpui-shell-model-default"),
        composer_local: t("gpui-shell-composer-local"),
        permission_ask: t("gpui-shell-permission-ask"),
        permission_ask_desc: t("gpui-shell-permission-ask-desc"),
        permission_auto: t("gpui-shell-permission-auto"),
        permission_auto_desc: t("gpui-shell-permission-auto-desc"),
        permission_full_access: t("gpui-shell-permission-full-access"),
        permission_full_access_desc: t("gpui-shell-permission-full-access-desc"),
        models_title: t("gpui-shell-models-title"),
        models_loading: t("gpui-shell-models-loading"),
        models_empty: t("gpui-shell-models-empty"),
        models_disabled: t("gpui-shell-models-disabled"),
        models_test: t("gpui-shell-models-test"),
        models_testing: t("gpui-shell-models-testing"),
        models_test_details: t("gpui-shell-models-test-details"),
        models_edit_title: t("gpui-shell-models-edit-title"),
        models_edit_close: t("action-close"),
        models_category: t("gpui-shell-models-category"),
        models_category_general_chat: t("gpui-shell-models-category-general-chat"),
        models_category_multimodal: t("gpui-shell-models-category-multimodal"),
        models_category_image_generation: t("gpui-shell-models-category-image-generation"),
        models_category_speech_recognition: t("gpui-shell-models-category-speech-recognition"),
        models_context_window: t("gpui-shell-models-context-window"),
        models_context_window_hint: t("gpui-shell-models-context-window-hint"),
        models_max_tokens: t("gpui-shell-models-max-tokens"),
        models_max_tokens_hint: t("gpui-shell-models-max-tokens-hint"),
        models_reasoning_default: t("gpui-shell-models-reasoning-default"),
        models_reasoning_auto: t("gpui-shell-models-reasoning-auto"),
        models_reasoning_presets: t("gpui-shell-models-reasoning-presets"),
        models_reasoning_empty: t("gpui-shell-models-reasoning-empty"),
        models_advanced: t("gpui-shell-models-advanced"),
        models_advanced_note: t("gpui-shell-models-advanced-note"),
        models_inline_think: t("gpui-shell-models-inline-think"),
        models_inline_think_hint: t("gpui-shell-models-inline-think-hint"),
        models_skip_ssl: t("gpui-shell-models-skip-ssl"),
        models_skip_ssl_warning: t("gpui-shell-models-skip-ssl-warning"),
        models_headers: t("gpui-shell-models-headers"),
        models_headers_hint: t("gpui-shell-models-headers-hint"),
        models_headers_mode: t("gpui-shell-models-headers-mode"),
        models_headers_replace: t("gpui-shell-models-headers-replace"),
        models_headers_merge: t("gpui-shell-models-headers-merge"),
        models_headers_replace_hint: t("gpui-shell-models-headers-replace-hint"),
        models_headers_merge_hint: t("gpui-shell-models-headers-merge-hint"),
        models_body: t("gpui-shell-models-body"),
        models_body_hint: t("gpui-shell-models-body-hint"),
        models_body_mode: t("gpui-shell-models-body-mode"),
        models_body_merge: t("gpui-shell-models-body-merge"),
        models_body_trim: t("gpui-shell-models-body-trim"),
        models_body_trim_hint: t("gpui-shell-models-body-trim-hint"),
        models_json_invalid: t("gpui-shell-models-json-invalid"),
        models_acquisition_title: t("gpui-shell-models-acquisition-title"),
        models_acquisition_api: t("gpui-shell-models-acquisition-api"),
        models_acquisition_none: t("gpui-shell-models-acquisition-none"),
        models_acquisition_subscriptions: t("gpui-shell-models-acquisition-subscriptions"),
        models_subscriptions_unavailable: t("gpui-shell-models-subscriptions-unavailable"),
        models_add: t("gpui-shell-models-add"),
        models_delete_provider: t("gpui-shell-models-delete-provider"),
        models_selection_modes_title: t("gpui-shell-models-selection-modes-title"),
        models_mode_smart: t("gpui-shell-models-mode-smart"),
        models_mode_smart_hint: t("gpui-shell-models-mode-smart-hint"),
        models_mode_unavailable: t("gpui-shell-models-mode-unavailable"),
        models_pool_title: t("gpui-shell-models-pool-title"),
        models_default_primary: t("gpui-shell-models-default-primary"),
        models_default_primary_desc: t("gpui-shell-models-default-primary-desc"),
        models_default_fast: t("gpui-shell-models-default-fast"),
        models_default_fast_desc: t("gpui-shell-models-default-fast-desc"),
        models_default_image: t("gpui-shell-models-default-image"),
        models_default_image_desc: t("gpui-shell-models-default-image-desc"),
        models_default_speech: t("gpui-shell-models-default-speech"),
        models_default_speech_desc: t("gpui-shell-models-default-speech-desc"),
        models_default_not_set: t("gpui-shell-models-default-not-set"),
        models_search: t("gpui-shell-models-search"),
        models_filter_all_capabilities: t("gpui-shell-models-filter-all-capabilities"),
        models_filter_all_providers: t("gpui-shell-models-filter-all-providers"),
        models_capability_text: t("gpui-shell-models-capability-text"),
        models_capability_image: t("gpui-shell-models-capability-image"),
        models_capability_speech: t("gpui-shell-models-capability-speech"),
        models_capability_tools: t("gpui-shell-models-capability-tools"),
        models_no_matches: t("gpui-shell-models-no-matches"),
        models_clear_filter: t("gpui-shell-models-clear-filter"),
        models_delete: t("gpui-shell-models-delete"),
        models_delete_confirm: t("gpui-shell-models-delete-confirm"),
        models_delete_message: t("gpui-shell-models-delete-message"),
        models_more: t("gpui-shell-models-more"),
        models_more_expand: t("gpui-shell-models-more-expand"),
        models_more_collapse: t("gpui-shell-models-more-collapse"),
        models_editor_name: t("gpui-shell-models-editor-name"),
        models_editor_provider: t("gpui-shell-models-editor-provider"),
        models_editor_model_name: t("gpui-shell-models-editor-model-name"),
        models_editor_base_url: t("gpui-shell-models-editor-base-url"),
        models_editor_api_key: t("gpui-shell-models-editor-api-key"),
        models_editor_enabled: t("gpui-shell-models-editor-enabled"),
        models_editor_delete: t("gpui-shell-models-editor-delete"),
        models_create_title: t("gpui-shell-models-create-title"),
        models_create_save: t("gpui-shell-models-create-save"),
        models_create_cancel: t("gpui-shell-models-create-cancel"),
        models_create_error_name: t("gpui-shell-models-create-error-name"),
        models_create_error_model: t("gpui-shell-models-create-error-model"),
        models_create_error_url: t("gpui-shell-models-create-error-url"),
        models_create_error_key: t("gpui-shell-models-create-error-key"),
        models_create_error_context: t("gpui-shell-models-create-error-context"),
        chat_empty: t("gpui-shell-chat-empty"),
        chat_empty_title: t("gpui-shell-chat-empty-title"),
        welcome_title: t("gpui-shell-welcome-title"),
        welcome_slogan: t("gpui-shell-welcome-slogan"),
        chat_input_placeholder: t("gpui-shell-chat-input-placeholder"),
        chat_jump_latest: t("gpui-shell-chat-jump-latest"),
        chat_stop: t("gpui-shell-chat-stop"),
        chat_busy: t("gpui-shell-chat-busy"),
        chat_submitting: t("gpui-shell-chat-submitting"),
        tool_running: t("gpui-shell-tool-running"),
        tool_waiting: t("gpui-shell-tool-waiting"),
        tool_done: t("gpui-shell-tool-done"),
        tool_failed: t("gpui-shell-tool-failed"),
        tool_cancelled: t("gpui-shell-tool-cancelled"),
        permission_title: t("gpui-shell-permission-title"),
        permission_resources: t("gpui-shell-permission-resources"),
        permission_project: t("gpui-shell-permission-project"),
        permission_risk: t("gpui-shell-permission-risk"),
        permission_allow_once: t("gpui-shell-permission-allow-once"),
        permission_allow_always: t("gpui-shell-permission-allow-always"),
        permission_deny: t("gpui-shell-permission-deny"),
        question_title: t("gpui-shell-question-title"),
        question_multi: t("gpui-shell-question-multi"),
        question_submit: t("gpui-shell-question-submit"),
        question_cancel: t("gpui-shell-question-cancel"),
        question_unreadable: t("gpui-shell-question-unreadable"),
        workspace_switch: t("gpui-shell-workspace-switch"),
        workspace_open_project: t("gpui-shell-workspace-open-project"),
        workspace_new_project: t("gpui-shell-workspace-new-project"),
        workspace_open_path_description: t("gpui-shell-workspace-open-path-description"),
        new_project_title: t("gpui-shell-new-project-title"),
        new_project_subtitle: t("gpui-shell-new-project-subtitle"),
        new_project_parent: t("gpui-shell-new-project-parent"),
        new_project_parent_placeholder: t("gpui-shell-new-project-parent-placeholder"),
        new_project_name: t("gpui-shell-new-project-name"),
        new_project_name_placeholder: t("gpui-shell-new-project-name-placeholder"),
        new_project_full_path: t("gpui-shell-new-project-full-path"),
        new_project_create: t("gpui-shell-new-project-create"),
        new_project_error_parent: t("gpui-shell-new-project-error-parent"),
        new_project_error_name: t("gpui-shell-new-project-error-name"),
        new_project_error_missing: t("gpui-shell-new-project-error-missing"),
        new_project_error_create: t("gpui-shell-new-project-error-create"),
        terminal_title: t("gpui-shell-terminal-title"),
        terminal_close: t("gpui-shell-terminal-close"),
        terminal_reveal: t("gpui-shell-terminal-reveal"),
        notifications_title: t("gpui-shell-notifications-title"),
        notifications_empty: t("gpui-shell-notifications-empty"),
        notifications_clear_all: t("gpui-shell-notifications-clear-all"),
        terminal_mode_compact: t("gpui-shell-terminal-mode-compact"),
        terminal_mode_comfortable: t("gpui-shell-terminal-mode-comfortable"),
        terminal_mode_expanded: t("gpui-shell-terminal-mode-expanded"),
        terminal_exited: t("gpui-shell-terminal-exited"),
        settings_open: t("gpui-shell-settings-open"),
        settings_title: t("shared.features.settings"),
        tab_close: t("gpui-shell-tab-close"),
        settings_appearance: t("gpui-shell-settings-appearance"),
        settings_appearance_interface_title: t("gpui-shell-settings-appearance-interface-title"),
        settings_appearance_interface_description: t(
            "gpui-shell-settings-appearance-interface-desc",
        ),
        settings_general_startup_title: t("gpui-shell-settings-general-startup-title"),
        settings_general_startup_description: t("gpui-shell-settings-general-startup-desc"),
        settings_general_window_title: t("gpui-shell-settings-general-window-title"),
        settings_general_window_description: t("gpui-shell-settings-general-window-desc"),
        settings_appearance_font_title: t("gpui-shell-settings-appearance-font-title"),
        settings_websearch_provider_title: t("gpui-shell-settings-websearch-provider-title"),
        settings_websearch_http_title: t("gpui-shell-settings-websearch-http-title"),
        settings_websearch_http_description: t("gpui-shell-settings-websearch-http-desc"),
        settings_permissions: t("gpui-shell-settings-permissions"),
        settings_theme_system: t("gpui-shell-settings-theme-system"),
        settings_theme_unsupported: t("gpui-shell-settings-theme-unsupported"),
        settings_permission_rules_section: t("gpui-shell-settings-permission-rules-section"),
        settings_permission_rules_description: t("gpui-shell-settings-permission-rules-desc"),
        settings_permission_rules_manage: t("gpui-shell-settings-permission-rules-manage"),
        settings_permission_rules_add: t("gpui-shell-settings-permission-rules-add"),
        settings_permission_rules_save: t("gpui-shell-settings-permission-rules-save"),
        settings_permission_rules_discard: t("gpui-shell-settings-permission-rules-discard"),
        settings_permission_rules_empty: t("gpui-shell-settings-permission-rules-empty"),
        settings_permission_rules_action: t("gpui-shell-settings-permission-rules-action"),
        settings_permission_rules_resource: t("gpui-shell-settings-permission-rules-resource"),
        settings_permission_rules_effect: t("gpui-shell-settings-permission-rules-effect"),
        settings_permission_rules_invalid: t("gpui-shell-settings-permission-rules-invalid"),
        settings_permission_effect_allow: t("gpui-shell-settings-permission-effect-allow"),
        settings_permission_effect_ask: t("gpui-shell-settings-permission-effect-ask"),
        settings_permission_effect_deny: t("gpui-shell-settings-permission-effect-deny"),
        settings_quick_actions_section: t("gpui-shell-settings-quick-actions-section"),
        settings_quick_actions_add: t("gpui-shell-settings-quick-actions-add"),
        settings_quick_actions_delete: t("gpui-shell-settings-quick-actions-delete"),
        settings_quick_actions_delete_title: t("gpui-shell-settings-quick-actions-delete-title"),
        settings_quick_actions_delete_action: t("gpui-shell-settings-quick-actions-delete-action"),
        settings_quick_actions_label: t("gpui-shell-settings-quick-actions-label"),
        settings_quick_actions_prompt: t("gpui-shell-settings-quick-actions-prompt"),
        settings_quick_actions_save: t("gpui-shell-settings-quick-actions-save"),
        settings_quick_actions_cancel: t("gpui-shell-settings-quick-actions-cancel"),
        settings_quick_actions_invalid: t("gpui-shell-settings-quick-actions-invalid"),
        settings_terminal: t("gpui-shell-settings-terminal"),
        settings_terminal_font_size: t("gpui-shell-settings-terminal-font-size"),
        settings_terminal_font_size_description: t("gpui-shell-settings-terminal-font-size-desc"),
        settings_terminal_font_family: t("gpui-shell-settings-terminal-font-family"),
        settings_terminal_font_family_description: t(
            "gpui-shell-settings-terminal-font-family-desc",
        ),
        settings_terminal_scrollback: t("gpui-shell-settings-terminal-scrollback"),
        settings_terminal_scrollback_description: t("gpui-shell-settings-terminal-scrollback-desc"),
        settings_terminal_shell: t("gpui-shell-settings-terminal-shell"),
        settings_terminal_shell_description: t("gpui-shell-settings-terminal-shell-desc"),
        git_title: t("gpui-shell-git-title"),
        git_refresh: t("gpui-shell-git-refresh"),
        git_loading: t("gpui-shell-git-loading"),
        git_empty_title: t("gpui-shell-git-empty-title"),
        git_empty: t("gpui-shell-git-empty"),
        git_binary: t("gpui-shell-git-binary"),
        git_too_large: t("gpui-shell-git-too-large"),
        git_truncated: t("gpui-shell-git-truncated"),
        git_staged: t("gpui-shell-git-staged"),
        git_unstaged: t("gpui-shell-git-unstaged"),
        git_untracked: t("gpui-shell-git-untracked"),
        git_select: t("gpui-shell-git-select"),
        git_diff_wrap: t("gpui-shell-git-diff-wrap"),
        git_stage_all: t("gpui-shell-git-stage-all"),
        git_stage: t("gpui-shell-git-stage"),
        git_unstage: t("gpui-shell-git-unstage"),
        git_discard: t("gpui-shell-git-discard"),
        git_discard_confirm: t("gpui-shell-git-discard-confirm"),
        git_delete_file: t("gpui-shell-git-delete-file"),
        git_delete_confirm: t("gpui-shell-git-delete-confirm"),
        git_commit: t("gpui-shell-git-commit"),
        git_commit_placeholder: t("gpui-shell-git-commit-placeholder"),
        git_push: t("gpui-shell-git-push"),
        git_pull: t("gpui-shell-git-pull"),
        git_not_repo_title: t("gpui-shell-git-not-repo-title"),
        git_not_repo_detail: t("gpui-shell-git-not-repo-detail"),
        git_init_button: t("gpui-shell-git-init-button"),
        git_init_hint: t("gpui-shell-git-init-hint"),
        git_init_prompt: t("gpui-shell-git-init-prompt"),
        git_history: t("gpui-shell-git-history"),
        git_files_view: t("gpui-shell-git-files-view"),
        git_no_commits: t("gpui-shell-git-no-commits"),
        git_untrusted_title: t("gpui-shell-git-untrusted-title"),
        git_untrusted_action: t("gpui-shell-git-untrusted-action"),
        git_worktrees_view: t("gpui-shell-git-worktrees-view"),
        git_snapshots_view: t("gpui-shell-git-snapshots-view"),
        git_remote_unsupported: t("gpui-shell-git-remote-unsupported"),
        git_branches_view: t("gpui-shell-git-branches-view"),
        git_graph_view: t("gpui-shell-git-graph-view"),
        git_branch_create: t("gpui-shell-git-branch-create"),
        git_branch_name_label: t("gpui-shell-git-branch-name-label"),
        git_branch_base_label: t("gpui-shell-git-branch-base-label"),
        git_branch_switch: t("gpui-shell-git-branch-switch"),
        git_branch_delete: t("gpui-shell-git-branch-delete"),
        git_branch_delete_confirm: t("gpui-shell-git-branch-delete-confirm"),
        git_branch_copy_hash: t("gpui-shell-git-branch-copy-hash"),
        git_branch_current: t("gpui-shell-git-branch-current"),
        git_branch_remote: t("gpui-shell-git-branch-remote"),
        git_branch_empty: t("gpui-shell-git-branch-empty"),
        git_graph_reset: t("gpui-shell-git-graph-reset"),
        git_graph_reset_confirm: t("gpui-shell-git-graph-reset-confirm"),
        git_generate_message: t("gpui-shell-git-generate-message"),
        git_generate_cancel: t("gpui-shell-git-generate-cancel"),
        git_worktree_empty: t("gpui-shell-git-worktree-empty"),
        git_worktree_create: t("gpui-shell-git-worktree-create"),
        git_worktree_branch_placeholder: t("gpui-shell-git-worktree-branch-placeholder"),
        git_worktree_base_ref_placeholder: t("gpui-shell-git-worktree-base-ref-placeholder"),
        git_worktree_remove: t("gpui-shell-git-worktree-remove"),
        git_worktree_promote: t("gpui-shell-git-worktree-promote"),
        git_worktree_isolate: t("gpui-shell-git-worktree-isolate"),
        git_worktree_release: t("gpui-shell-git-worktree-release"),
        git_worktree_no_session: t("gpui-shell-git-worktree-no-session"),
        git_worktree_main: t("gpui-shell-git-worktree-main"),
        git_worktree_dirty: t("gpui-shell-git-worktree-dirty"),
        git_worktree_locked: t("gpui-shell-git-worktree-locked"),
        git_worktree_missing: t("gpui-shell-git-worktree-missing"),
        git_worktree_managed: t("gpui-shell-git-worktree-managed"),
        git_worktree_permanent: t("gpui-shell-git-worktree-permanent"),
        git_worktree_external: t("gpui-shell-git-worktree-external"),
        git_snapshot_empty: t("gpui-shell-git-snapshot-empty"),
        git_snapshot_restore: t("gpui-shell-git-snapshot-restore"),
        rollback_at_turn: t("gpui-shell-rollback-at-turn"),
        turn_menu: t("gpui-shell-turn-menu"),
        turn_copy: t("gpui-shell-turn-copy"),
        turn_meta_completed: t("gpui-shell-turn-meta-completed"),
        turn_meta_stopped: t("gpui-shell-turn-meta-stopped"),
        turn_meta_duration: t("gpui-shell-turn-meta-duration"),
        turn_copy_full: t("gpui-shell-turn-copy-full"),
        turn_copy_result: t("gpui-shell-turn-copy-result"),
        code_copy: t("gpui-shell-code-copy"),
        code_copied: t("gpui-shell-code-copied"),
        tool_open_details: t("gpui-shell-tool-open-details"),
        tool_copy: t("gpui-shell-tool-copy"),
        tool_copied: t("gpui-shell-tool-copied"),
        turn_copy_user_label: t("gpui-shell-turn-copy-user-label"),
        subagent_badge: t("gpui-shell-subagent-badge"),
        subagent_open: t("gpui-shell-subagent-open"),
        subagent_names: cx
            .foreground_executor()
            .block_on(fetch_subagent_names(&service)),
        tool_action_command: t("gpui-shell-tool-action-command"),
        tool_action_read: t("gpui-shell-tool-action-read"),
        tool_action_search: t("gpui-shell-tool-action-search"),
        tool_action_web: t("gpui-shell-tool-action-web"),
        tool_action_write: t("gpui-shell-tool-action-write"),
        tool_action_edit: t("gpui-shell-tool-action-edit"),
        tool_action_tool_spec: t("gpui-shell-tool-action-tool-spec"),
        tool_action_tool_call: t("gpui-shell-tool-action-tool-call"),
        tool_action_agent_spawn: t("gpui-shell-tool-action-agent-spawn"),
        tool_action_agent_wait: t("gpui-shell-tool-action-agent-wait"),
        tool_action_agent_list: t("gpui-shell-tool-action-agent-list"),
        tool_action_agent_send: t("gpui-shell-tool-action-agent-send"),
        tool_action_agent_delete: t("gpui-shell-tool-action-agent-delete"),
        btw_badge: t("gpui-shell-btw-badge"),
        sidebar_review_running: t("gpui-shell-sidebar-review-running"),
        child_back: t("gpui-shell-child-back"),
        manage_skills_title: t("gpui-shell-manage-skills-title"),
        manage_skills_loading: t("gpui-shell-manage-skills-loading"),
        manage_skills_empty_title: t("gpui-shell-manage-skills-empty-title"),
        manage_skills_empty: t("gpui-shell-manage-skills-empty"),
        manage_skills_search_placeholder: t("gpui-shell-manage-skills-search-placeholder"),
        manage_skills_hide_shadowed: t("gpui-shell-manage-skills-hide-shadowed"),
        manage_skills_no_match: t("gpui-shell-manage-skills-no-match"),
        manage_skill_builtin: t("gpui-shell-manage-skill-builtin"),
        manage_skill_user: t("gpui-shell-manage-skill-user"),
        manage_skill_project: t("gpui-shell-manage-skill-project"),
        manage_skill_shadowed: t("gpui-shell-manage-skill-shadowed"),
        manage_skill_add: t("gpui-shell-manage-skill-add"),
        manage_skill_detail: t("gpui-shell-manage-skill-detail"),
        manage_skill_dialog_add_title: t("gpui-shell-manage-skill-dialog-add-title"),
        manage_skill_dialog_detail_title: t("gpui-shell-manage-skill-dialog-detail-title"),
        manage_skill_dialog_delete_title: t("gpui-shell-manage-skill-dialog-delete-title"),
        manage_skill_source: t("gpui-shell-manage-skill-source"),
        manage_skill_source_native: t("shared.product.name"),
        manage_skill_path: t("gpui-shell-manage-skill-path"),
        manage_skill_path_hint: t("gpui-shell-manage-skill-path-hint"),
        manage_skill_validate: t("gpui-shell-manage-skill-validate"),
        manage_skill_valid: t("gpui-shell-manage-skill-valid"),
        manage_skill_invalid: t("gpui-shell-manage-skill-invalid"),
        manage_skill_runtime_unavailable: t("gpui-shell-manage-skill-runtime-unavailable"),
        manage_skill_add_action: t("gpui-shell-manage-skill-add-action"),
        manage_skill_delete_action: t("gpui-shell-manage-skill-delete-action"),
        manage_skill_close: t("gpui-shell-manage-skill-close"),
        manage_skill_not_found: t("gpui-shell-manage-skill-not-found"),
        manage_skill_no_description: t("gpui-shell-manage-skill-no-description"),
        skill_groups_title: t("gpui-shell-skill-groups-title"),
        skill_groups_subtitle: t("gpui-shell-skill-groups-subtitle"),
        skill_groups_manage: t("gpui-shell-skill-groups-manage"),
        skill_groups_mine: t("gpui-shell-skill-groups-mine"),
        skill_groups_builtin: t("gpui-shell-skill-groups-builtin"),
        skill_groups_loading: t("gpui-shell-skill-groups-loading"),
        skill_groups_empty: t("gpui-shell-skill-groups-empty"),
        skill_groups_empty_group: t("gpui-shell-skill-groups-empty-group"),
        skill_groups_unresolved: t("gpui-shell-skill-groups-unresolved"),
        skill_group_new: t("gpui-shell-skill-group-new"),
        skill_group_name_label: t("gpui-shell-skill-group-name-label"),
        skill_group_name_placeholder: t("gpui-shell-skill-group-name-placeholder"),
        skill_group_dialog_edit_title: t("gpui-shell-skill-group-dialog-edit-title"),
        skill_group_dialog_delete_title: t("gpui-shell-skill-group-dialog-delete-title"),
        skill_group_save: t("gpui-shell-skill-group-save"),
        skill_group_copy: t("gpui-shell-skill-group-copy"),
        skill_group_up: t("gpui-shell-skill-group-up"),
        skill_group_down: t("gpui-shell-skill-group-down"),
        skill_group_delete: t("gpui-shell-skill-group-delete"),
        agent_detail_open: t("gpui-shell-agent-detail-open"),
        agent_detail_missing: t("gpui-shell-agent-detail-missing"),
        agent_detail_type: t("gpui-shell-agent-detail-type"),
        agent_detail_source: t("gpui-shell-agent-detail-source"),
        agent_detail_follow_up: t("gpui-shell-agent-detail-follow-up"),
        agent_detail_supported: t("gpui-shell-agent-detail-supported"),
        agent_detail_unsupported: t("gpui-shell-agent-detail-unsupported"),
        agent_detail_capabilities: t("gpui-shell-agent-detail-capabilities"),
        agent_detail_shared_profile: t("gpui-shell-agent-detail-shared-profile"),
        agent_detail_shared_default: t("gpui-shell-agent-detail-shared-default"),
        agent_detail_tools: t("gpui-shell-agent-detail-tools"),
        agent_detail_subagents: t("gpui-shell-agent-detail-subagents"),
        agent_detail_loading: t("gpui-shell-agent-detail-loading"),
        agent_detail_no_subagents: t("gpui-shell-agent-detail-no-subagents"),
        agent_detail_save: t("gpui-shell-agent-detail-save"),
        agent_detail_reset: t("gpui-shell-agent-detail-reset"),
        agent_detail_close: t("gpui-shell-agent-detail-close"),
        skills_facet_library: t("gpui-shell-skills-facet-library"),
        skills_facet_all: t("gpui-shell-skills-facet-all"),
        skills_facet_groups: t("gpui-shell-skills-facet-groups"),
        skills_facet_sources: t("gpui-shell-skills-facet-sources"),
        skills_facet_explore: t("gpui-shell-skills-facet-explore"),
        skills_market_title: t("gpui-shell-skills-market-title"),
        skills_market_degraded: t("gpui-shell-skills-market-degraded"),
        manage_agents_title: t("gpui-shell-manage-agents-title"),
        manage_agents_modes: t("gpui-shell-manage-agents-modes"),
        manage_agents_subagents: t("gpui-shell-manage-agents-subagents"),
        manage_agents_create: t("gpui-shell-manage-agents-create"),
        manage_agents_loading: t("gpui-shell-manage-agents-loading"),
        manage_agents_empty_title: t("gpui-shell-manage-agents-empty-title"),
        manage_agents_empty: t("gpui-shell-manage-agents-empty"),
        manage_agents_search_placeholder: t("gpui-shell-manage-agents-search-placeholder"),
        manage_agents_source_label: t("gpui-shell-manage-agents-source-label"),
        manage_agents_source_any: t("gpui-shell-manage-agents-source-any"),
        manage_agents_kind_label: t("gpui-shell-manage-agents-kind-label"),
        manage_agents_kind_any: t("gpui-shell-manage-agents-kind-any"),
        manage_agents_kind_agent: t("gpui-shell-manage-agents-kind-agent"),
        manage_agents_kind_subagent: t("gpui-shell-manage-agents-kind-subagent"),
        manage_agents_no_match: t("gpui-shell-manage-agents-no-match"),
        manage_agent_create_title: t("gpui-shell-manage-agent-create-title"),
        manage_agent_kind_mode: t("gpui-shell-manage-agent-kind-mode"),
        manage_agent_kind_subagent: t("gpui-shell-manage-agent-kind-subagent"),
        manage_agent_level_user: t("gpui-shell-manage-agent-level-user"),
        manage_agent_level_project: t("gpui-shell-manage-agent-level-project"),
        manage_agent_id: t("gpui-shell-manage-agent-id"),
        manage_agent_name: t("gpui-shell-manage-agent-name"),
        manage_agent_description: t("gpui-shell-manage-agent-description"),
        manage_agent_prompt: t("gpui-shell-manage-agent-prompt"),
        manage_agent_readonly: t("gpui-shell-manage-agent-readonly"),
        manage_agent_review: t("gpui-shell-manage-agent-review"),
        manage_agent_enabled: t("gpui-shell-manage-agent-enabled"),
        manage_agent_disabled: t("gpui-shell-manage-agent-disabled"),
        manage_agent_source_builtin: t("gpui-shell-manage-agent-source-builtin"),
        manage_agent_source_user: t("gpui-shell-manage-agent-source-user"),
        manage_agent_source_project: t("gpui-shell-manage-agent-source-project"),
        manage_agent_source_external: t("gpui-shell-manage-agent-source-external"),
        manage_agent_error_empty_id: t("gpui-shell-manage-agent-error-empty-id"),
        manage_agent_error_bad_id: t("gpui-shell-manage-agent-error-bad-id"),
        manage_agent_error_empty_name: t("gpui-shell-manage-agent-error-empty-name"),
        manage_agent_error_empty_description: t("gpui-shell-manage-agent-error-empty-description"),
        manage_agent_error_empty_prompt: t("gpui-shell-manage-agent-error-empty-prompt"),
        manage_agent_error_mode_project: t("gpui-shell-manage-agent-error-mode-project"),
        manage_agent_edit_title: t("gpui-shell-manage-agent-edit-title"),
        manage_agent_model: t("gpui-shell-manage-agent-model"),
        manage_agent_model_default: t("gpui-shell-manage-agent-model-default"),
        manage_agent_model_failed: t("gpui-shell-manage-agent-model-failed"),
        manage_harness_title: t("gpui-shell-manage-harness-title"),
        manage_harness_subtitle: t("gpui-shell-manage-harness-subtitle"),
        manage_harness_purpose_minimal: t("gpui-shell-manage-harness-purpose-minimal"),
        manage_harness_purpose_standard: t("gpui-shell-manage-harness-purpose-standard"),
        manage_harness_purpose_ultimate: t("gpui-shell-manage-harness-purpose-ultimate"),
        manage_harness_purpose_creative: t("gpui-shell-manage-harness-purpose-creative"),
        manage_agent_delete: t("gpui-shell-manage-agent-delete"),
        manage_agent_delete_title: t("gpui-shell-manage-agent-delete-title"),
        // "Edit" is a shared term; a dedicated key would trip the shared-term
        // duplicate ratchet (same alias the terminal profile uses).
        agent_card_edit: t("shared.tools.edit"),
        mode_skills_open: t("gpui-shell-mode-skills-open"),
        mode_skills_title: t("gpui-shell-mode-skills-title"),
        mode_skills_loading: t("gpui-shell-mode-skills-loading"),
        mode_skills_empty: t("gpui-shell-mode-skills-empty"),
        // "Edit" is a shared term; a dedicated key would trip the shared-term
        // duplicate ratchet (same alias the terminal profile uses).
        mode_skills_edit: t("shared.tools.edit"),
        mode_skills_save_hint: t("gpui-shell-mode-skills-save-hint"),
        mode_skills_status_selected: t("gpui-shell-mode-skills-status-selected"),
        mode_skills_status_enabled: t("gpui-shell-mode-skills-status-enabled"),
        mode_skills_reset: t("gpui-shell-mode-skills-reset"),
        mode_skills_source_other: t("gpui-shell-mode-skills-source-other"),
        manage_mcp_title: t("gpui-shell-manage-mcp-title"),
        manage_mcp_loading: t("gpui-shell-manage-mcp-loading"),
        manage_mcp_empty_title: t("gpui-shell-manage-mcp-empty-title"),
        manage_mcp_empty: t("gpui-shell-manage-mcp-empty"),
        manage_mcp_start: t("gpui-shell-manage-mcp-start"),
        manage_mcp_stop: t("gpui-shell-manage-mcp-stop"),
        manage_mcp_restart: t("gpui-shell-manage-mcp-restart"),
        manage_mcp_enable: t("gpui-shell-manage-mcp-enable"),
        manage_mcp_disable: t("gpui-shell-manage-mcp-disable"),
        manage_mcp_delete: t("gpui-shell-manage-mcp-delete"),
        manage_mcp_delete_title: t("gpui-shell-manage-mcp-delete-title"),
        manage_mcp_delete_warning: t("gpui-shell-manage-mcp-delete-warning"),
        manage_mcp_browse: t("gpui-shell-manage-mcp-browse"),
        manage_mcp_browse_title: t("gpui-shell-manage-mcp-browse-title"),
        manage_mcp_resources: t("gpui-shell-manage-mcp-resources"),
        manage_mcp_prompts: t("gpui-shell-manage-mcp-prompts"),
        manage_mcp_refresh: t("gpui-shell-manage-mcp-refresh"),
        manage_mcp_disabled: t("gpui-shell-manage-mcp-disabled"),
        manage_mcp_location_builtin: t("gpui-shell-manage-mcp-location-builtin"),
        manage_mcp_state_uninitialized: t("gpui-shell-manage-mcp-state-uninitialized"),
        manage_mcp_state_starting: t("gpui-shell-manage-mcp-state-starting"),
        manage_mcp_state_connected: t("gpui-shell-manage-mcp-state-connected"),
        manage_mcp_state_healthy: t("gpui-shell-manage-mcp-state-healthy"),
        manage_mcp_state_needs_auth: t("gpui-shell-manage-mcp-state-needs-auth"),
        manage_mcp_state_reconnecting: t("gpui-shell-manage-mcp-state-reconnecting"),
        manage_mcp_state_failed: t("shared.statuses.failed"),
        manage_mcp_state_stopping: t("gpui-shell-manage-mcp-state-stopping"),
        manage_mcp_state_stopped: t("gpui-shell-manage-mcp-state-stopped"),
        manage_interaction_title: t("gpui-shell-manage-interaction-title"),
        manage_interaction_params: t("gpui-shell-manage-interaction-params"),
        manage_interaction_result_hint: t("gpui-shell-manage-interaction-result-hint"),
        manage_interaction_approve: t("gpui-shell-manage-interaction-approve"),
        manage_interaction_reject: t("gpui-shell-manage-interaction-reject"),
        manage_mcp_json_item: t("gpui-shell-manage-mcp-json-item"),
        manage_mcp_json_edit: t("gpui-shell-manage-mcp-json-edit"),
        manage_mcp_json_loading: t("gpui-shell-manage-mcp-json-loading"),
        manage_mcp_json_save: t("gpui-shell-manage-mcp-json-save"),
        manage_mcp_json_saved: t("gpui-shell-manage-mcp-json-saved"),
        manage_mcp_import: t("gpui-shell-manage-mcp-import"),
        manage_mcp_import_paste: t("gpui-shell-manage-mcp-import-paste"),
        manage_mcp_import_path: t("gpui-shell-manage-mcp-import-path"),
        manage_mcp_import_read: t("gpui-shell-manage-mcp-import-read"),
        manage_mcp_import_file_failed: t("gpui-shell-manage-mcp-import-file-failed"),
        manage_mcp_import_preview: t("gpui-shell-manage-mcp-import-preview"),
        manage_mcp_import_hint: t("gpui-shell-manage-mcp-import-hint"),
        manage_mcp_import_selected: t("gpui-shell-manage-mcp-import-selected"),
        manage_mcp_import_conflict: t("gpui-shell-manage-mcp-import-conflict"),
        manage_mcp_import_select_servers: t("gpui-shell-manage-mcp-import-select-servers"),
        manage_mcp_import_invalid_json: t("gpui-shell-manage-mcp-import-invalid-json"),
        manage_mcp_import_invalid_config: t("gpui-shell-manage-mcp-import-invalid-config"),
        manage_mcp_import_id_required: t("gpui-shell-manage-mcp-import-id-required"),
        manage_mcp_import_invalid_id: t("gpui-shell-manage-mcp-import-invalid-id"),
        manage_mcp_import_id_conflict: t("gpui-shell-manage-mcp-import-id-conflict"),
        manage_mcp_import_shape: t("gpui-shell-manage-mcp-import-shape"),
        manage_mcp_resource_hint: t("gpui-shell-manage-mcp-resource-hint"),
        manage_mcp_resource_loading: t("gpui-shell-manage-mcp-resource-loading"),
        manage_mcp_resource_binary: t("gpui-shell-manage-mcp-resource-binary"),
        manage_mcp_resource_empty: t("gpui-shell-manage-mcp-resource-empty"),
        manage_mcp_resource_failed: t("gpui-shell-manage-mcp-resource-failed"),
        manage_mcp_auth: t("gpui-shell-manage-mcp-auth"),
        manage_mcp_auth_oauth_hint: t("gpui-shell-manage-mcp-auth-oauth-hint"),
        manage_mcp_oauth_idle: t("gpui-shell-manage-mcp-oauth-idle"),
        manage_mcp_oauth_awaiting_browser: t("gpui-shell-manage-mcp-oauth-awaiting-browser"),
        manage_mcp_oauth_awaiting_callback: t("gpui-shell-manage-mcp-oauth-awaiting-callback"),
        manage_mcp_oauth_exchanging_token: t("gpui-shell-manage-mcp-oauth-exchanging-token"),
        manage_mcp_oauth_authorized: t("gpui-shell-manage-mcp-oauth-authorized"),
        manage_mcp_oauth_failed: t("shared.statuses.failed"),
        manage_mcp_oauth_cancelled: t("shared.statuses.cancelled"),
        manage_mcp_oauth_start: t("gpui-shell-manage-mcp-oauth-start"),
        manage_mcp_oauth_restart: t("gpui-shell-manage-mcp-oauth-restart"),
        manage_mcp_oauth_reconnect: t("gpui-shell-manage-mcp-oauth-reconnect"),
        manage_mcp_auth_token_hint: t("gpui-shell-manage-mcp-auth-token-hint"),
        manage_mcp_auth_secret_hint: t("gpui-shell-manage-mcp-auth-secret-hint"),
        manage_mcp_auth_token_label: t("gpui-shell-manage-mcp-auth-token-label"),
        manage_mcp_auth_clear: t("gpui-shell-manage-mcp-auth-clear"),
        manage_mcp_auth_cancel_flow: t("gpui-shell-manage-mcp-auth-cancel-flow"),
        manage_mcp_auth_save: t("gpui-shell-manage-mcp-auth-save"),
        manage_mcp_auth_required: t("gpui-shell-manage-mcp-auth-required"),
        manage_mcp_auth_saved: t("gpui-shell-manage-mcp-auth-saved"),
        manage_mcp_auth_configured: t("gpui-shell-manage-mcp-auth-configured"),
        manage_mcp_auth_oauth_ready: t("gpui-shell-manage-mcp-auth-oauth-ready"),
        manage_mcp_auth_missing: t("gpui-shell-manage-mcp-auth-missing"),
        manage_mcp_auth_source_headers: t("gpui-shell-manage-mcp-auth-source-headers"),
        manage_mcp_auth_source_env: t("gpui-shell-manage-mcp-auth-source-env"),
        manage_mcp_auth_source_oauth: t("gpui-shell-manage-mcp-auth-source-oauth"),
        manage_error_dismiss: t("gpui-shell-manage-error-dismiss"),
        value_unsupported: t("gpui-shell-value-unsupported"),
        settings_general: t("gpui-shell-settings-general"),
        settings_version: t("gpui-shell-settings-version"),
        settings_launch_login: t("gpui-shell-settings-launch-login"),
        settings_launch_login_description: t("gpui-shell-settings-launch-login-description"),
        settings_close_behavior: t("gpui-shell-settings-close-behavior"),
        settings_close_behavior_description: t("gpui-shell-settings-close-behavior-description"),
        settings_close_quit: t("gpui-shell-settings-close-quit"),
        settings_close_minimize: t("gpui-shell-settings-close-minimize"),
        settings_auto_update: t("gpui-shell-settings-auto-update"),
        settings_auto_update_description: t("gpui-shell-settings-auto-update-description"),
        settings_notify_turn_done: t("gpui-shell-settings-notify-turn-done"),
        settings_notify_turn_done_description: t(
            "gpui-shell-settings-notify-turn-done-description",
        ),
        settings_prevent_sleep: t("gpui-shell-settings-prevent-sleep"),
        settings_prevent_sleep_description: t("gpui-shell-settings-prevent-sleep-description"),
        settings_check_updates: t("gpui-shell-settings-check-updates"),
        settings_update_checking: t("gpui-shell-settings-update-checking"),
        settings_up_to_date: t("gpui-shell-settings-up-to-date"),
        settings_update_available: t("gpui-shell-settings-update-available"),
        settings_update_install: t("gpui-shell-settings-update-install"),
        settings_update_installing: t("gpui-shell-settings-update-installing"),
        settings_update_installed: t("gpui-shell-settings-update-installed"),
        settings_update_restart: t("gpui-shell-settings-update-restart"),
        settings_update_unsupported: t("gpui-shell-settings-update-unsupported"),
        settings_update_failed: t("gpui-shell-settings-update-failed"),
        settings_automation: t("gpui-shell-settings-automation"),
        settings_hooks: t("gpui-shell-settings-hooks"),
        settings_hooks_description: t("gpui-shell-settings-hooks-description"),
        settings_hooks_project: t("gpui-shell-settings-hooks-project"),
        settings_project_hooks_risk_title: t("gpui-shell-settings-project-hooks-risk-title"),
        settings_project_hooks_risk_confirm: t("gpui-shell-settings-project-hooks-risk-confirm"),
        settings_project_hooks_risk_current_workspace: t(
            "gpui-shell-settings-project-hooks-risk-current-workspace",
        ),
        settings_hooks_project_description: t("gpui-shell-settings-hooks-project-description"),
        settings_hooks_user_file: t("gpui-shell-settings-hooks-user-file"),
        settings_hooks_user_file_desc: t("gpui-shell-settings-hooks-user-file-desc"),
        settings_hooks_project_file: t("gpui-shell-settings-hooks-project-file"),
        settings_hooks_project_file_desc: t("gpui-shell-settings-hooks-project-file-desc"),
        settings_hooks_imports_master_disabled: t(
            "gpui-shell-settings-hooks-imports-master-disabled",
        ),
        settings_hooks_imports_refresh: t("gpui-shell-settings-hooks-imports-refresh"),
        settings_hooks_imports_loading: t("gpui-shell-settings-hooks-imports-loading"),
        settings_hooks_imports_empty: t("gpui-shell-settings-hooks-imports-empty"),
        settings_hooks_imports_update: t("gpui-shell-settings-hooks-imports-update"),
        settings_hooks_imports_remove: t("gpui-shell-settings-hooks-imports-remove"),
        settings_hook_import_state_current: t("gpui-shell-settings-hook-import-state-current"),
        settings_hook_import_state_update_available: t(
            "gpui-shell-settings-hook-import-state-update-available",
        ),
        settings_hook_import_state_source_missing: t(
            "gpui-shell-settings-hook-import-state-source-missing",
        ),
        settings_hook_import_state_update_check_failed: t(
            "gpui-shell-settings-hook-import-state-update-check-failed",
        ),
        settings_hook_import_state_bundle_missing: t(
            "gpui-shell-settings-hook-import-state-bundle-missing",
        ),
        settings_hooks_import_review_title: t("gpui-shell-settings-hooks-import-review-title"),
        settings_hooks_import_review_warning: t("gpui-shell-settings-hooks-import-review-warning"),
        settings_hooks_import_timeout: t("gpui-shell-settings-hooks-import-timeout"),
        settings_hooks_import_skipped: t("gpui-shell-settings-hooks-import-skipped"),
        settings_hooks_import_toggle: t("gpui-shell-settings-hooks-import-toggle"),
        settings_hooks_import_confirm: t("gpui-shell-settings-hooks-import-confirm"),
        settings_hooks_import_confirm_update: t("gpui-shell-settings-hooks-import-confirm-update"),
        settings_hooks_import_stale: t("gpui-shell-settings-hooks-import-stale"),
        settings_hooks_import_remove_title: t("gpui-shell-settings-hooks-import-remove-title"),
        settings_hooks_import_remove_confirm_note: t(
            "gpui-shell-settings-hooks-import-remove-confirm-note",
        ),
        settings_hooks_import_remove_confirm: t("gpui-shell-settings-hooks-import-remove-confirm"),
        settings_hooks_import_store_invalid: t("gpui-shell-settings-hooks-import-store-invalid"),
        settings_hooks_import_reset: t("gpui-shell-settings-hooks-import-reset"),
        settings_hooks_import_reset_title: t("gpui-shell-settings-hooks-import-reset-title"),
        settings_hooks_import_reset_note: t("gpui-shell-settings-hooks-import-reset-note"),
        settings_hooks_import_reset_confirm: t("gpui-shell-settings-hooks-import-reset-confirm"),
        settings_hooks_compat_reference: t("gpui-shell-settings-hooks-compat-reference"),
        settings_hooks_compat_open: t("gpui-shell-settings-hooks-compat-open"),
        settings_automation_hooks_title: t("gpui-shell-settings-automation-hooks-title"),
        settings_automation_hooks_desc: t("gpui-shell-settings-automation-hooks-desc"),
        settings_automation_qa_builtin_title: t("gpui-shell-settings-automation-qa-builtin-title"),
        settings_automation_qa_builtin_desc: t("gpui-shell-settings-automation-qa-builtin-desc"),
        settings_automation_qa_custom_title: t("gpui-shell-settings-automation-qa-custom-title"),
        settings_automation_qa_custom_desc: t("gpui-shell-settings-automation-qa-custom-desc"),
        settings_automation_hook_locations_title: t(
            "gpui-shell-settings-automation-hook-locations-title",
        ),
        settings_automation_hook_locations_desc: t(
            "gpui-shell-settings-automation-hook-locations-desc",
        ),
        settings_automation_hook_imports_title: t(
            "gpui-shell-settings-automation-hook-imports-title",
        ),
        settings_automation_hook_imports_desc: t(
            "gpui-shell-settings-automation-hook-imports-desc",
        ),
        settings_automation_hooks_compat_title: t(
            "gpui-shell-settings-automation-hooks-compat-title",
        ),
        settings_automation_hooks_compat_desc: t(
            "gpui-shell-settings-automation-hooks-compat-desc",
        ),
        settings_mcp_servers_title: t("gpui-shell-settings-mcp-servers-title"),
        settings_mcp_servers_desc: t("gpui-shell-settings-mcp-servers-desc"),
        settings_websearch: t("gpui-shell-settings-websearch"),
        settings_websearch_provider: t("gpui-shell-settings-websearch-provider"),
        settings_websearch_provider_description: t(
            "gpui-shell-settings-websearch-provider-description",
        ),
        settings_category_application: t("gpui-shell-settings-category-application"),
        settings_category_ai: t("gpui-shell-settings-category-ai"),
        settings_category_workspace: t("gpui-shell-settings-category-workspace"),
        settings_category_tools: t("gpui-shell-settings-category-tools"),
        settings_category_data: t("gpui-shell-settings-category-data"),
        settings_nav_search_placeholder: t("gpui-shell-settings-nav-search-placeholder"),
        settings_nav_search_empty: t("gpui-shell-settings-nav-search-empty"),
        settings_page_general_description: t("gpui-shell-settings-page-general-description"),
        settings_page_appearance_description: t("gpui-shell-settings-page-appearance-description"),
        settings_page_terminal_description: t("gpui-shell-settings-page-terminal-description"),
        settings_page_models_description: t("gpui-shell-settings-page-models-description"),
        settings_page_session_description: t("gpui-shell-settings-page-session-description"),
        settings_worktrees_page_label: t("gpui-shell-settings-worktrees-page"),
        settings_page_worktrees_description: t("gpui-shell-settings-page-worktrees-description"),
        settings_page_execution_description: t("gpui-shell-settings-page-execution-description"),
        settings_page_automation_description: t("gpui-shell-settings-page-automation-description"),
        settings_page_websearch_description: t("gpui-shell-settings-page-websearch-description"),
        settings_page_skills_description: t("gpui-shell-settings-page-skills-description"),
        settings_page_agents_description: t("gpui-shell-settings-page-agents-description"),
        settings_page_mcp_description: t("gpui-shell-settings-page-mcp-description"),
        settings_acp_label: t("gpui-shell-settings-acp-label"),
        settings_page_acp_description: t("gpui-shell-settings-page-acp-description"),
        acp_view_local: t("gpui-shell-acp-view-local"),
        acp_view_json: t("gpui-shell-acp-view-json"),
        acp_registry_title: t("gpui-shell-acp-registry-title"),
        acp_registry_description: t("gpui-shell-acp-registry-description"),
        acp_action_refresh: t("gpui-shell-acp-action-refresh"),
        acp_search_placeholder: t("gpui-shell-acp-search-placeholder"),
        acp_filter_all: t("gpui-shell-acp-filter-all"),
        acp_filter_enabled: t("gpui-shell-acp-filter-enabled"),
        acp_filter_not_installed: t("gpui-shell-acp-filter-not-installed"),
        acp_filter_invalid: t("gpui-shell-acp-filter-invalid"),
        acp_loading: t("gpui-shell-acp-loading"),
        acp_empty_title: t("gpui-shell-acp-empty-title"),
        acp_empty: t("gpui-shell-acp-empty"),
        acp_status_enabled: t("gpui-shell-acp-status-enabled"),
        acp_status_ready: t("gpui-shell-acp-status-ready"),
        acp_status_partial: t("gpui-shell-acp-status-partial"),
        acp_status_checking: t("gpui-shell-acp-status-checking"),
        acp_status_cli_missing: t("gpui-shell-acp-status-cli-missing"),
        acp_status_acp_missing: t("gpui-shell-acp-status-acp-missing"),
        acp_status_path_invalid: t("gpui-shell-acp-status-path-invalid"),
        acp_status_version_mismatch: t("gpui-shell-acp-status-version-mismatch"),
        acp_status_connection_failed: t("gpui-shell-acp-status-connection-failed"),
        acp_status_permission_denied: t("gpui-shell-acp-status-permission-denied"),
        acp_status_config_invalid: t("gpui-shell-acp-status-config-invalid"),
        acp_permission_ask: t("gpui-shell-acp-permission-ask"),
        acp_permission_allow_once: t("gpui-shell-acp-permission-allow-once"),
        acp_action_install_cli: t("gpui-shell-acp-action-install-cli"),
        acp_action_configure: t("gpui-shell-acp-action-configure"),
        acp_action_view_instructions: t("gpui-shell-acp-action-view-instructions"),
        acp_action_view_error: t("gpui-shell-acp-action-view-error"),
        acp_action_add: t("gpui-shell-acp-action-add"),
        acp_action_add_config: t("gpui-shell-acp-action-add-config"),
        acp_cli_missing_detail: t("gpui-shell-acp-cli-missing-detail"),
        acp_json_title: t("gpui-shell-acp-json-title"),
        acp_json_description: t("gpui-shell-acp-json-description"),
        acp_json_warning: t("gpui-shell-acp-json-warning"),
        acp_action_revert: t("gpui-shell-acp-action-revert"),
        acp_action_save_json: t("gpui-shell-acp-action-save-json"),
        acp_notice_save_success: t("gpui-shell-acp-notice-save-success"),
        acp_notice_install_success: t("gpui-shell-acp-notice-install-success"),
        acp_notice_configure_success: t("gpui-shell-acp-notice-configure-success"),
        acp_notice_config_added: t("gpui-shell-acp-notice-config-added"),
        acp_preset_opencode: t("gpui-shell-acp-preset-opencode"),
        acp_preset_dsh: t("gpui-shell-acp-preset-dsh"),
        acp_preset_omp: t("gpui-shell-acp-preset-omp"),
        acp_preset_claude_code: t("gpui-shell-acp-preset-claude-code"),
        acp_preset_codex: t("gpui-shell-acp-preset-codex"),
        acp_docs: t("gpui-shell-acp-docs"),
        acp_notice_open_link_failed: t("gpui-shell-acp-notice-open-link-failed"),
        acp_legacy_reject_warning: t("gpui-shell-acp-legacy-reject-warning"),
        acp_permission_save_apply: t("gpui-shell-acp-permission-save-apply"),
        acp_action_cancel: t("gpui-shell-acp-action-cancel"),
        acp_install_confirm_local_host: t("gpui-shell-acp-install-confirm-local-host"),
        acp_install_confirm_confirm: t("gpui-shell-acp-install-confirm-confirm"),
        acp_install_confirm_package_unknown: t("gpui-shell-acp-install-confirm-package-unknown"),
        settings_page_usage_description: t("gpui-shell-settings-page-usage-description"),
        settings_page_archived_description: t("gpui-shell-settings-page-archived-description"),
        settings_page_diagnostics_description: t(
            "gpui-shell-settings-page-diagnostics-description",
        ),
        settings_jump_session_label: t("gpui-shell-settings-jump-session-label"),
        settings_jump_session_description: t("gpui-shell-settings-jump-session-description"),
        settings_jump_session_action: t("gpui-shell-settings-jump-session-action"),
        websearch_provider_free: t("gpui-shell-websearch-provider-free"),
        websearch_provider_exa: t("gpui-shell-websearch-provider-exa"),
        websearch_provider_tavily: t("gpui-shell-websearch-provider-tavily"),
        websearch_provider_http: t("gpui-shell-websearch-provider-http"),
        websearch_http_endpoint: t("gpui-shell-websearch-http-endpoint"),
        websearch_http_endpoint_description: t("gpui-shell-websearch-http-endpoint-description"),
        websearch_http_auth_mode: t("gpui-shell-websearch-http-auth-mode"),
        websearch_auth_none: t("gpui-shell-websearch-auth-none"),
        websearch_auth_bearer: t("gpui-shell-websearch-auth-bearer"),
        websearch_auth_header: t("gpui-shell-websearch-auth-header"),
        websearch_http_header_name: t("gpui-shell-websearch-http-header-name"),
        websearch_http_header_name_description: t(
            "gpui-shell-websearch-http-header-name-description",
        ),
        websearch_credential: t("gpui-shell-websearch-credential"),
        websearch_credential_description: t("gpui-shell-websearch-credential-description"),
        websearch_configured: t("gpui-shell-websearch-configured"),
        websearch_not_configured: t("gpui-shell-websearch-not-configured"),
        websearch_save: t("gpui-shell-websearch-save"),
        websearch_no_credential: t("gpui-shell-websearch-no-credential"),
        quick_action_title: t("gpui-shell-quick-action-title"),
        quick_action_commit_label: t("gpui-shell-quick-action-commit-label"),
        quick_action_commit_prompt: t("gpui-shell-quick-action-commit-prompt"),
        quick_action_create_pr_label: t("gpui-shell-quick-action-create-pr-label"),
        quick_action_create_pr_prompt: t("gpui-shell-quick-action-create-pr-prompt"),
        settings_usage: t("gpui-shell-settings-usage"),
        settings_usage_today: t("gpui-shell-settings-usage-today"),
        settings_usage_week: t("gpui-shell-settings-usage-week"),
        settings_usage_month: t("gpui-shell-settings-usage-month"),
        settings_usage_all: t("gpui-shell-settings-usage-all"),
        settings_usage_refresh: t("gpui-shell-settings-usage-refresh"),
        settings_usage_loading: t("gpui-shell-settings-usage-loading"),
        settings_usage_empty: t("gpui-shell-settings-usage-empty"),
        settings_usage_requests: t("gpui-shell-settings-usage-requests"),
        settings_usage_cache_hit: t("gpui-shell-settings-usage-cache-hit"),
        settings_usage_groups: t("gpui-shell-settings-usage-groups"),
        settings_usage_endpoints: t("gpui-shell-settings-usage-endpoints"),
        settings_usage_filter: t("gpui-shell-settings-usage-filter"),
        settings_usage_filter_empty: t("gpui-shell-settings-usage-filter-empty"),
        usage_trend_title: t("gpui-shell-usage-trend-title"),
        usage_trend_partial: t("gpui-shell-usage-trend-partial"),
        usage_trend_input: t("gpui-shell-usage-trend-input"),
        usage_trend_output: t("gpui-shell-usage-trend-output"),
        usage_activity_title: t("gpui-shell-usage-activity-title"),
        usage_activity_description: t("gpui-shell-usage-activity-description"),
        settings_usage_overview_title: t("gpui-shell-settings-usage-overview-title"),
        settings_usage_distributions_title: t("gpui-shell-settings-usage-distributions-title"),
        settings_usage_details_title: t("gpui-shell-settings-usage-details-title"),
        usage_activity_less: t("gpui-shell-usage-activity-less"),
        usage_activity_more: t("gpui-shell-usage-activity-more"),
        usage_activity_mon: t("gpui-shell-usage-activity-mon"),
        usage_activity_wed: t("gpui-shell-usage-activity-wed"),
        usage_activity_fri: t("gpui-shell-usage-activity-fri"),
        usage_activity_sun: t("gpui-shell-usage-activity-sun"),
        settings_archived: t("gpui-shell-settings-archived"),
        settings_archived_list: t("gpui-shell-settings-archived-list"),
        settings_archived_description: t("gpui-shell-settings-archived-description"),
        settings_archived_empty: t("gpui-shell-settings-archived-empty"),
        settings_archived_loading: t("gpui-shell-settings-archived-loading"),
        settings_archived_refresh: t("gpui-shell-settings-archived-refresh"),
        settings_archived_restore: t("gpui-shell-settings-archived-restore"),
        settings_archived_delete_title: t("gpui-shell-settings-archived-delete-title"),
        settings_archived_delete_all: t("gpui-shell-settings-archived-delete-all"),
        settings_archived_delete_all_confirm_title: t(
            "gpui-shell-settings-archived-delete-all-confirm-title",
        ),
        settings_diagnostics: t("gpui-shell-settings-diagnostics"),
        settings_logging_level: t("gpui-shell-settings-logging-level"),
        settings_logging_level_description: t("gpui-shell-settings-logging-level-description"),
        settings_logs_dir: t("gpui-shell-settings-logs-dir"),
        settings_logs_open: t("gpui-shell-settings-logs-open"),
        settings_diagnostics_note: t("gpui-shell-settings-diagnostics-note"),
        settings_diagnostics_export: t("gpui-shell-settings-diagnostics-export"),
        settings_diagnostics_export_description: t(
            "gpui-shell-settings-diagnostics-export-description",
        ),
        settings_diagnostics_export_action: t("gpui-shell-settings-diagnostics-export-action"),
        settings_diagnostics_export_title: t("gpui-shell-settings-diagnostics-export-title"),
        settings_diagnostics_export_warning: t("gpui-shell-settings-diagnostics-export-warning"),
        settings_diagnostics_export_confirm: t("gpui-shell-settings-diagnostics-export-confirm"),
        settings_diagnostics_export_success: t("gpui-shell-settings-diagnostics-export-success"),
        settings_diagnostics_export_failed: t("gpui-shell-settings-diagnostics-export-failed"),
        settings_diagnostics_previous_crash: t("gpui-shell-settings-diagnostics-previous-crash"),
        settings_diagnostics_previous_unclean: t(
            "gpui-shell-settings-diagnostics-previous-unclean",
        ),
        settings_memory_label: t("gpui-shell-settings-memory-label"),
        settings_page_memory_description: t("gpui-shell-settings-page-memory-description"),
        memory_scope_notice: t("gpui-shell-memory-scope-notice"),
        memory_section_basic: t("gpui-shell-memory-section-basic"),
        memory_section_basic_note: t("gpui-shell-memory-section-basic-note"),
        memory_section_models: t("gpui-shell-memory-section-models"),
        memory_section_models_note: t("gpui-shell-memory-section-models-note"),
        memory_section_advanced: t("gpui-shell-memory-section-advanced"),
        memory_section_advanced_note: t("gpui-shell-memory-section-advanced-note"),
        memory_enabled: t("gpui-shell-memory-enabled"),
        memory_enabled_description: t("gpui-shell-memory-enabled-description"),
        memory_generate_btw: t("gpui-shell-memory-generate-btw"),
        memory_generate_btw_description: t("gpui-shell-memory-generate-btw-description"),
        memory_external_context: t("gpui-shell-memory-external-context"),
        memory_external_context_description: t("gpui-shell-memory-external-context-description"),
        memory_policy_clear_tool_results: t("gpui-shell-memory-policy-clear-tool-results"),
        memory_policy_allow: t("gpui-shell-memory-policy-allow"),
        memory_policy_skip_session: t("gpui-shell-memory-policy-skip-session"),
        memory_actions_label: t("gpui-shell-memory-actions-label"),
        memory_actions_description: t("gpui-shell-memory-actions-description"),
        memory_action_manage: t("gpui-shell-memory-action-manage"),
        memory_action_open_directory: t("gpui-shell-memory-action-open-directory"),
        memory_action_reset_settings: t("gpui-shell-memory-action-reset-settings"),
        memory_action_reset_memory: t("gpui-shell-memory-action-reset-memory"),
        memory_reset_settings_confirm_title: t("gpui-shell-memory-reset-settings-confirm-title"),
        memory_reset_settings_confirm_message: t(
            "gpui-shell-memory-reset-settings-confirm-message",
        ),
        memory_reset_settings_confirm_action: t("gpui-shell-memory-reset-settings-confirm-action"),
        memory_reset_confirm_message: t("gpui-shell-memory-reset-confirm-message"),
        memory_reset_confirm_action: t("gpui-shell-memory-reset-confirm-action"),
        memory_extract_model: t("gpui-shell-memory-extract-model"),
        memory_extract_model_description: t("gpui-shell-memory-extract-model-description"),
        memory_consolidation_model: t("gpui-shell-memory-consolidation-model"),
        memory_consolidation_model_description: t(
            "gpui-shell-memory-consolidation-model-description",
        ),
        memory_model_primary: t("gpui-shell-memory-model-primary"),
        memory_model_fast: t("gpui-shell-memory-model-fast"),
        memory_model_follow_primary: t("gpui-shell-memory-model-follow-primary"),
        memory_model_follow_extraction: t("gpui-shell-memory-model-follow-extraction"),
        memory_min_idle_hours: t("gpui-shell-memory-min-idle-hours"),
        memory_min_idle_hours_description: t("gpui-shell-memory-min-idle-hours-description"),
        memory_max_age_days: t("gpui-shell-memory-max-age-days"),
        memory_max_age_days_description: t("gpui-shell-memory-max-age-days-description"),
        memory_per_startup: t("gpui-shell-memory-per-startup"),
        memory_per_startup_description: t("gpui-shell-memory-per-startup-description"),
        memory_scan_limit: t("gpui-shell-memory-scan-limit"),
        memory_scan_limit_description: t("gpui-shell-memory-scan-limit-description"),
        memory_concurrency: t("gpui-shell-memory-concurrency"),
        memory_concurrency_description: t("gpui-shell-memory-concurrency-description"),
        memory_raw_limit: t("gpui-shell-memory-raw-limit"),
        memory_raw_limit_description: t("gpui-shell-memory-raw-limit-description"),
        memory_retention_days: t("gpui-shell-memory-retention-days"),
        memory_retention_days_description: t("gpui-shell-memory-retention-days-description"),
        memory_action_expand_advanced: t("gpui-shell-memory-action-expand-advanced"),
        memory_action_collapse_advanced: t("gpui-shell-memory-action-collapse-advanced"),
        memory_settings_reset: t("gpui-shell-memory-settings-reset"),
        memory_settings_reset_failed: t("gpui-shell-memory-settings-reset-failed"),
        memory_open_directory_failed: t("gpui-shell-memory-open-directory-failed"),
        memory_reset: t("gpui-shell-memory-reset"),
        memory_reset_failed: t("gpui-shell-memory-reset-failed"),
        settings_shortcuts_label: t("gpui-shell-settings-shortcuts-label"),
        settings_page_shortcuts_description: t("gpui-shell-settings-page-shortcuts-description"),
        shortcuts_fixed_note: t("gpui-shell-shortcuts-fixed-note"),
        shortcuts_section_global: t("gpui-shell-shortcuts-section-global"),
        shortcuts_section_overlays: t("gpui-shell-shortcuts-section-overlays"),
        shortcuts_section_terminal: t("gpui-shell-shortcuts-section-terminal"),
        shortcuts_section_editing: t("gpui-shell-shortcuts-section-editing"),
        shortcuts_overlay_dismiss: t("gpui-shell-shortcuts-overlay-dismiss"),
        shortcuts_overlay_move: t("gpui-shell-shortcuts-overlay-move"),
        shortcuts_overlay_confirm: t("gpui-shell-shortcuts-overlay-confirm"),
        shortcuts_terminal_copy: t("gpui-shell-shortcuts-terminal-copy"),
        shortcuts_terminal_paste: t("gpui-shell-shortcuts-terminal-paste"),
        shortcuts_terminal_note: t("gpui-shell-shortcuts-terminal-note"),
        shortcuts_editing_note: t("gpui-shell-shortcuts-editing-note"),
        plugin_ui_removed: t("gpui-shell-plugin-ui-removed"),
        plugin_ui_rejected: t("gpui-shell-plugin-ui-rejected"),
        plugin_ui_timeout: t("gpui-shell-plugin-ui-timeout"),
        settings_ui_font: t("gpui-shell-settings-ui-font"),
        settings_ui_font_description: t("gpui-shell-settings-ui-font-desc"),
        ui_font_level_compact: t("gpui-shell-ui-font-level-compact"),
        ui_font_level_small: t("gpui-shell-ui-font-level-small"),
        ui_font_level_default: t("gpui-shell-ui-font-level-default"),
        ui_font_level_medium: t("gpui-shell-ui-font-level-medium"),
        ui_font_level_large: t("gpui-shell-ui-font-level-large"),
        ui_font_level_custom: t("gpui-shell-ui-font-level-custom"),
        settings_ui_font_custom: t("gpui-shell-settings-ui-font-custom"),
        settings_ui_font_custom_description: t("gpui-shell-settings-ui-font-custom-desc"),
        settings_stream_section: t("gpui-shell-settings-stream-section"),
        settings_stream_section_note: t("gpui-shell-settings-stream-section-note"),
        settings_stream_ttft: t("gpui-shell-settings-stream-ttft"),
        settings_stream_ttft_description: t("gpui-shell-settings-stream-ttft-desc"),
        settings_stream_idle: t("gpui-shell-settings-stream-idle"),
        settings_stream_idle_description: t("gpui-shell-settings-stream-idle-desc"),
        settings_proxy_section: t("gpui-shell-settings-proxy-section"),
        settings_proxy_section_note: t("gpui-shell-settings-proxy-section-note"),
        settings_proxy_enable: t("gpui-shell-settings-proxy-enable"),
        settings_proxy_enable_description: t("gpui-shell-settings-proxy-enable-desc"),
        settings_proxy_url: t("gpui-shell-settings-proxy-url"),
        settings_proxy_url_description: t("gpui-shell-settings-proxy-url-desc"),
        settings_proxy_username: t("gpui-shell-settings-proxy-username"),
        settings_proxy_password: t("gpui-shell-settings-proxy-password"),
        settings_proxy_password_description: t("gpui-shell-settings-proxy-password-desc"),
        settings_tool_timeout: t("gpui-shell-settings-tool-timeout"),
        settings_tool_timeout_description: t("gpui-shell-settings-tool-timeout-desc"),
        settings_question_timeout: t("gpui-shell-settings-question-timeout"),
        settings_question_timeout_description: t("gpui-shell-settings-question-timeout-desc"),
        settings_subagent_model: t("gpui-shell-settings-subagent-model"),
        settings_subagent_model_description: t("gpui-shell-settings-subagent-model-desc"),
        settings_subagent_model_settings: t("gpui-shell-settings-subagent-model-settings"),
        settings_subagent_model_inherit: t("gpui-shell-settings-subagent-model-inherit"),
        settings_subagent_model_fast: t("gpui-shell-settings-subagent-model-fast"),
        settings_subagent_model_primary: t("gpui-shell-settings-subagent-model-primary"),
        settings_subagent_batch: t("gpui-shell-settings-subagent-batch"),
        settings_subagent_batch_safe: t("gpui-shell-settings-subagent-batch-safe"),
        settings_subagent_batch_safe_description: t("gpui-shell-settings-subagent-batch-safe-desc"),
        settings_subagent_batch_parallel: t("gpui-shell-settings-subagent-batch-parallel"),
        settings_subagent_batch_parallel_description: t(
            "gpui-shell-settings-subagent-batch-parallel-desc",
        ),
        settings_subagent_concurrency: t("gpui-shell-settings-subagent-concurrency"),
        settings_subagent_concurrency_description: t(
            "gpui-shell-settings-subagent-concurrency-desc",
        ),
        settings_swarm_concurrency: t("gpui-shell-settings-swarm-concurrency"),
        settings_swarm_concurrency_description: t("gpui-shell-settings-swarm-concurrency-desc"),
        settings_deferred_tools_enable: t("gpui-shell-settings-deferred-tools-enable"),
        settings_deferred_tools_warning: t("gpui-shell-settings-deferred-tools-warning"),
        settings_permissions_policy_title: t("gpui-shell-settings-permissions-policy-title"),
        settings_permissions_policy_desc: t("gpui-shell-settings-permissions-policy-desc"),
        settings_permissions_deferred_title: t("gpui-shell-settings-permissions-deferred-title"),
        settings_permissions_deferred_desc: t("gpui-shell-settings-permissions-deferred-desc"),
        settings_permissions_tool_presentation_title: t(
            "gpui-shell-settings-permissions-tool-presentation-title",
        ),
        settings_permissions_tool_presentation_desc: t(
            "gpui-shell-settings-permissions-tool-presentation-desc",
        ),
        settings_tool_presentation_mode: t("gpui-shell-settings-tool-presentation-mode"),
        settings_tool_presentation_description: t("gpui-shell-settings-tool-presentation-desc"),
        settings_tool_presentation_native: t("gpui-shell-settings-tool-presentation-native"),
        settings_tool_presentation_ptc: t("gpui-shell-settings-tool-presentation-ptc"),
        settings_tool_presentation_ptc_hint: t("gpui-shell-settings-tool-presentation-ptc-hint"),
        settings_permissions_mode: t("gpui-shell-settings-permissions-mode"),
        settings_permissions_mode_description: t("gpui-shell-settings-permissions-mode-desc"),
        settings_permissions_mode_ask: t("gpui-shell-settings-permissions-mode-ask"),
        settings_permissions_mode_ask_description: t(
            "gpui-shell-settings-permissions-mode-ask-desc",
        ),
        settings_permissions_mode_auto: t("gpui-shell-settings-permissions-mode-auto"),
        settings_permissions_mode_auto_description: t(
            "gpui-shell-settings-permissions-mode-auto-desc",
        ),
        settings_permissions_mode_full: t("gpui-shell-settings-permissions-mode-full"),
        settings_permissions_mode_full_description: t(
            "gpui-shell-settings-permissions-mode-full-desc",
        ),
        settings_permissions_show_in_chat_input: t(
            "gpui-shell-settings-permissions-show-in-chat-input",
        ),
        settings_permissions_show_in_chat_input_description: t(
            "gpui-shell-settings-permissions-show-in-chat-input-desc",
        ),
        settings_permissions_full_access_title: t(
            "gpui-shell-settings-permissions-full-access-title",
        ),
        settings_permissions_full_access_message: t(
            "gpui-shell-settings-permissions-full-access-message",
        ),
        settings_permissions_full_access_confirm: t(
            "gpui-shell-settings-permissions-full-access-confirm",
        ),
        settings_permissions_execution_title: t("gpui-shell-settings-permissions-execution-title"),
        settings_permissions_execution_desc: t("gpui-shell-settings-permissions-execution-desc"),
        settings_permissions_subagents_title: t("gpui-shell-settings-permissions-subagents-title"),
        settings_permissions_subagents_desc: t("gpui-shell-settings-permissions-subagents-desc"),
        settings_permissions_json_repair_title: t(
            "gpui-shell-settings-permissions-json-repair-title",
        ),
        settings_permissions_json_repair_desc: t(
            "gpui-shell-settings-permissions-json-repair-desc",
        ),
        settings_permissions_review_title: t("gpui-shell-settings-permissions-review-title"),
        settings_permissions_review_desc: t("gpui-shell-settings-permissions-review-desc"),
        settings_json_repair: t("gpui-shell-settings-json-repair"),
        settings_json_repair_description: t("gpui-shell-settings-json-repair-desc"),
        settings_review_parallelism: t("gpui-shell-settings-review-parallelism"),
        settings_review_parallelism_description: t("gpui-shell-settings-review-parallelism-desc"),
        settings_review_queue_wait: t("gpui-shell-settings-review-queue-wait"),
        settings_review_queue_wait_description: t("gpui-shell-settings-review-queue-wait-desc"),
        settings_default_harness_section: t("gpui-shell-settings-default-harness-section"),
        settings_default_harness_section_note: t(
            "gpui-shell-settings-default-harness-section-note",
        ),
        settings_default_harness_strategy: t("gpui-shell-settings-default-harness-strategy"),
        settings_default_harness_follow_last: t("gpui-shell-settings-default-harness-follow-last"),
        settings_default_harness_fixed: t("gpui-shell-settings-default-harness-fixed"),
        settings_default_harness_fixed_description: t(
            "gpui-shell-settings-default-harness-fixed-desc",
        ),
        settings_default_harness_fixed_mode: t("gpui-shell-settings-default-harness-fixed-mode"),
        settings_session_title_section: t("gpui-shell-settings-session-title-section"),
        settings_session_title_section_note: t("gpui-shell-settings-session-title-section-note"),
        settings_session_title_enable: t("gpui-shell-settings-session-title-enable"),
        settings_session_title_model: t("gpui-shell-settings-session-title-model"),
        settings_session_title_model_inherit: t("gpui-shell-settings-session-title-model-inherit"),
        settings_session_title_model_fast: t("gpui-shell-settings-session-title-model-fast"),
        settings_session_title_model_primary: t("gpui-shell-settings-session-title-model-primary"),
        settings_session_title_models_empty: t("gpui-shell-settings-session-title-models-empty"),
        settings_session_title_model_unavailable: t(
            "gpui-shell-settings-session-title-model-unavailable",
        ),
        value_compatibility: t("gpui-shell-value-compatibility"),
        settings_worktree_section: t("gpui-shell-settings-worktree-section"),
        settings_worktree_section_note: t("gpui-shell-settings-worktree-section-note"),
        settings_worktree_root: t("gpui-shell-settings-worktree-root"),
        settings_worktree_root_description: t("gpui-shell-settings-worktree-root-desc"),
        settings_worktree_branch_prefix: t("gpui-shell-settings-worktree-branch-prefix"),
        settings_worktree_branch_prefix_description: t(
            "gpui-shell-settings-worktree-branch-prefix-desc",
        ),
        settings_worktree_copy_changes: t("gpui-shell-settings-worktree-copy-changes"),
        settings_worktree_copy_changes_description: t(
            "gpui-shell-settings-worktree-copy-changes-desc",
        ),
        settings_worktree_auto_delete: t("gpui-shell-settings-worktree-auto-delete"),
        settings_worktree_auto_delete_description: t(
            "gpui-shell-settings-worktree-auto-delete-desc",
        ),
        settings_worktree_auto_delete_limit: t("gpui-shell-settings-worktree-auto-delete-limit"),
        settings_worktree_auto_delete_limit_description: t(
            "gpui-shell-settings-worktree-auto-delete-limit-desc",
        ),
        settings_workspace_search_title: t("gpui-shell-settings-workspace-search-title"),
        settings_workspace_search_description: t(
            "gpui-shell-settings-workspace-search-description",
        ),
        settings_workspace_search_enable: t("gpui-shell-settings-workspace-search-enable"),
        settings_git_attribution_title: t("gpui-shell-settings-git-attribution-title"),
        settings_git_attribution_scope: t("gpui-shell-settings-git-attribution-scope"),
        settings_git_attribution_label: t("gpui-shell-settings-git-attribution-label"),
        settings_worktree_reset: t("gpui-shell-settings-worktree-reset"),
        settings_worktree_management_title: t("gpui-shell-settings-worktree-management-title"),
        settings_worktree_management_description: t(
            "gpui-shell-settings-worktree-management-description",
        ),
        settings_worktree_management_refresh: t("gpui-shell-settings-worktree-management-refresh"),
        settings_worktree_management_loading: t("gpui-shell-settings-worktree-management-loading"),
        settings_worktree_management_load_failed: t(
            "gpui-shell-settings-worktree-management-load-failed",
        ),
        settings_worktree_management_retry: t("gpui-shell-settings-worktree-management-retry"),
        settings_worktree_management_empty: t("gpui-shell-settings-worktree-management-empty"),
        settings_worktree_lifecycle_permanent: t(
            "gpui-shell-settings-worktree-lifecycle-permanent",
        ),
        settings_worktree_lifecycle_external: t("gpui-shell-settings-worktree-lifecycle-external"),
        settings_worktree_state_dirty: t("gpui-shell-settings-worktree-state-dirty"),
        settings_worktree_state_unpublished: t("gpui-shell-settings-worktree-state-unpublished"),
        settings_worktree_state_locked: t("gpui-shell-settings-worktree-state-locked"),
        settings_worktree_state_missing: t("gpui-shell-settings-worktree-state-missing"),
        settings_worktree_delete_action: t("gpui-shell-settings-worktree-delete-action"),
        settings_worktree_delete_title: t("gpui-shell-settings-worktree-delete-title"),
        settings_worktree_delete_message: t("gpui-shell-settings-worktree-delete-message"),
        settings_worktree_delete_force_title: t("gpui-shell-settings-worktree-delete-force-title"),
        settings_worktree_delete_force_message: t(
            "gpui-shell-settings-worktree-delete-force-message",
        ),
        settings_worktree_delete_message_sessions: t(
            "gpui-shell-settings-worktree-delete-message-sessions",
        ),
        settings_worktree_delete_force_message_sessions: t(
            "gpui-shell-settings-worktree-delete-force-message-sessions",
        ),
        settings_worktree_delete_force_action: t(
            "gpui-shell-settings-worktree-delete-force-action",
        ),
        settings_worktree_delete_cancel: t("gpui-shell-settings-worktree-delete-cancel"),
        settings_worktree_protection_locked: t("gpui-shell-settings-worktree-protection-locked"),
        settings_worktree_protection_missing: t("gpui-shell-settings-worktree-protection-missing"),
        settings_worktree_error_locked: t("gpui-shell-settings-worktree-error-locked"),
        settings_worktree_error_dirty: t("gpui-shell-settings-worktree-error-dirty"),
        settings_worktree_error_unpublished: t("gpui-shell-settings-worktree-error-unpublished"),
        settings_worktree_error_not_found: t("gpui-shell-settings-worktree-error-not-found"),
        settings_worktree_error_remote_unsupported: t(
            "gpui-shell-settings-worktree-error-remote-unsupported",
        ),
        settings_worktree_error_delete_failed: t(
            "gpui-shell-settings-worktree-error-delete-failed",
        ),
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

    #[test]
    fn shell_ftl_resources_parse_for_every_locale() {
        for (locale, source) in [
            ("zh-CN", include_str!("../../../locales/zh-CN.ftl")),
            ("zh-TW", include_str!("../../../locales/zh-TW.ftl")),
            ("en-US", include_str!("../../../locales/en-US.ftl")),
        ] {
            if let Err((_, errors)) = fluent_bundle::FluentResource::try_new(source.to_string()) {
                panic!("{locale} ftl failed to parse: {errors:?}");
            }
        }
    }

    /// The three locale files carry the same key set (top-level message ids;
    /// continuation lines are indented, so column-0 `id =` is the key). A
    /// locale missing a key falls back silently at runtime — this keeps the
    /// surface honest instead.
    #[test]
    fn shell_ftl_keys_align_across_locales() {
        fn keys(source: &str) -> Vec<&str> {
            let mut keys: Vec<&str> = source
                .lines()
                .filter_map(|line| line.split_once(" = ").map(|(key, _)| key.trim()))
                .filter(|key| !key.is_empty())
                .collect();
            keys.sort_unstable();
            keys.dedup();
            keys
        }
        let en = keys(include_str!("../../../locales/en-US.ftl"));
        let zh_cn = keys(include_str!("../../../locales/zh-CN.ftl"));
        let zh_tw = keys(include_str!("../../../locales/zh-TW.ftl"));
        let missing = |label: &str, keys: &[&str], base: &[&str]| {
            let missing: Vec<&str> = keys
                .iter()
                .filter(|key| !base.contains(key))
                .copied()
                .collect();
            assert!(missing.is_empty(), "{label} keys missing: {missing:?}");
        };
        missing("zh-CN", &zh_cn, &en);
        missing("zh-TW", &zh_tw, &en);
        missing("en-US", &en, &zh_cn);
    }

    /// The naming discipline (gpui-redesign E2): user-visible shell copy
    /// carries no third-party library identifiers — no `gpui-kit` /
    /// `gpui-component` / `gpui-pre`, and no standalone `ely` word.
    #[test]
    fn shell_ftl_carries_no_third_party_brand_names() {
        let forbidden = ["gpui-kit", "gpui-component", "gpui-pre", "gpui-rs"];
        for (locale, source) in [
            ("en-US", include_str!("../../../locales/en-US.ftl")),
            ("zh-CN", include_str!("../../../locales/zh-CN.ftl")),
            ("zh-TW", include_str!("../../../locales/zh-TW.ftl")),
        ] {
            let lowered = source.to_ascii_lowercase();
            for brand in forbidden {
                assert!(
                    !lowered.contains(brand),
                    "{locale} ftl names {brand}; user-visible copy stays library-free"
                );
            }
            // Word-boundary check for the component library's short name:
            // `ely` as its own token, not inside ordinary words.
            for word in lowered.split(|c: char| !c.is_ascii_alphanumeric()) {
                assert!(
                    word != "ely",
                    "{locale} ftl names the component library (ely); \
                     user-visible copy stays library-free"
                );
            }
        }
    }
}
