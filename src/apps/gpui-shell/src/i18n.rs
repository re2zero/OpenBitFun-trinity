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
    pub tasks_enable: String,
    pub tasks_disable: String,
    pub tasks_run_now: String,
    /// R4 board chrome: overview card, month calendar, list section.
    pub tasks_overview: String,
    pub tasks_stat_total: String,
    pub tasks_stat_enabled: String,
    pub tasks_stat_due_24h: String,
    pub tasks_stat_failed: String,
    pub tasks_wd_mon: String,
    pub tasks_wd_tue: String,
    pub tasks_wd_wed: String,
    pub tasks_wd_thu: String,
    pub tasks_wd_fri: String,
    pub tasks_wd_sat: String,
    pub tasks_wd_sun: String,
    pub tasks_calendar_scheduled: String,
    pub tasks_calendar_ran: String,
    pub tasks_calendar_error: String,
    pub tasks_calendar_note: String,
    pub tasks_list_title: String,
    /// G-P3: the web row badges (running / nearest upcoming run / overdue).
    pub tasks_badge_running: String,
    pub tasks_badge_next_run: String,
    pub tasks_badge_overdue: String,
    /// G-P3: the inactive partition's per-job reason chips.
    pub tasks_inactive_paused: String,
    pub tasks_inactive_done: String,
    pub tasks_inactive_invalid: String,
    /// G-P2: the selected-day panel (title date and empty line are composed
    /// by the page; `tasks_day_title` is the parameterized helper).
    pub tasks_day_clear: String,
    pub tasks_day_empty: String,
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
    pub permission_full_access: String,
    pub permission_full_access_desc: String,
    pub models_title: String,
    pub models_default: String,
    pub models_default_description: String,
    pub models_configured: String,
    pub models_loading: String,
    pub models_empty: String,
    pub models_enabled: String,
    pub models_disabled: String,
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
    pub manage_mcp_servers: String,
    pub manage_mcp_description: String,
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
    pub manage_mcp_json_description: String,
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
    pub settings_git_coauthor: String,
    pub settings_git_coauthor_description: String,
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
    pub settings_page_worktrees_description: String,
    pub settings_page_execution_description: String,
    pub settings_page_automation_description: String,
    pub settings_page_websearch_description: String,
    pub settings_page_skills_description: String,
    pub settings_page_agents_description: String,
    pub settings_page_mcp_description: String,
    pub settings_page_usage_description: String,
    pub settings_page_archived_description: String,
    pub settings_page_diagnostics_description: String,
    pub settings_jump_session_label: String,
    pub settings_jump_session_description: String,
    pub settings_jump_session_action: String,
    pub settings_jump_worktrees_label: String,
    pub settings_jump_worktrees_description: String,
    pub settings_jump_worktrees_action: String,
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
    pub settings_usage_range: String,
    pub settings_usage_today: String,
    pub settings_usage_week: String,
    pub settings_usage_month: String,
    pub settings_usage_all: String,
    pub settings_usage_refresh: String,
    pub settings_usage_loading: String,
    pub settings_usage_empty: String,
    pub settings_usage_requests: String,
    pub settings_usage_cache_hit: String,
    pub usage_trend_title: String,
    pub usage_trend_partial: String,
    pub usage_trend_input: String,
    pub usage_trend_output: String,
    pub usage_activity_title: String,
    pub usage_activity_description: String,
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
    pub trinity_title: String,
    pub trinity_status_title: String,
    pub trinity_status_unavailable: String,
    pub trinity_phase_awake: String,
    pub trinity_phase_dormant: String,
    pub trinity_phase_offline: String,
    pub trinity_label_name: String,
    pub trinity_label_persona: String,
    pub trinity_label_user: String,
    pub trinity_label_birthday: String,
    pub trinity_label_emotion: String,
    pub trinity_label_focus: String,
    pub trinity_label_confidence: String,
    pub trinity_label_cycles: String,
    pub trinity_label_memory_nodes: String,
    pub trinity_label_memory_triples: String,
    /// G-Y6: the third memory-scale counter (web `trinity.status.vocabulary`).
    pub trinity_label_vocabulary: String,
    pub trinity_trend_title: String,
    pub trinity_memory_title: String,
    pub trinity_memory_add: String,
    pub trinity_memory_add_placeholder: String,
    pub trinity_memory_add_project: String,
    pub trinity_memory_submit: String,
    pub trinity_memory_search_placeholder: String,
    pub trinity_memory_search: String,
    pub trinity_memory_no_results: String,
    pub trinity_memory_empty: String,
    pub trinity_memory_load_earlier: String,
    pub trinity_memory_no_more: String,
    pub trinity_memory_forget: String,
    pub trinity_memory_forget_confirm: String,
    pub trinity_memory_reinforce: String,
    /// G-Y5: the inline correction affordances on a memory row.
    pub trinity_memory_edit: String,
    pub trinity_memory_edit_save: String,
    pub trinity_memory_edit_applied: String,
    pub trinity_memory_edit_missing: String,
    /// G-Y4: memory export/import (web T10 RPCs; the daemon reads/writes the
    /// file and the shell supplies a typed path).
    pub trinity_memory_export: String,
    pub trinity_memory_import: String,
    pub trinity_memory_export_title: String,
    pub trinity_memory_import_title: String,
    pub trinity_memory_path: String,
    pub trinity_memory_path_required: String,
    pub trinity_memory_import_confirm_title: String,
    pub trinity_memory_import_confirm: String,
    pub trinity_action_refresh: String,
    pub trinity_loading: String,
    pub trinity_action_failed: String,
    pub trinity_emotion_positive_high: String,
    pub trinity_emotion_positive_mild: String,
    pub trinity_emotion_neutral: String,
    pub trinity_emotion_curious: String,
    pub trinity_emotion_confused: String,
    pub trinity_emotion_negative_mild: String,
    pub trinity_emotion_negative_high: String,
    pub trinity_focus_respond: String,
    pub trinity_focus_reflect: String,
    pub trinity_focus_explore: String,
    pub trinity_focus_plan: String,
    pub trinity_focus_idle: String,
    pub trinity_need_competence: String,
    pub trinity_need_autonomy: String,
    pub trinity_need_relatedness: String,
    pub trinity_need_certainty: String,
    pub trinity_need_growth: String,
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
    pub settings_execution_section: String,
    pub settings_execution_section_note: String,
    pub settings_tool_timeout: String,
    pub settings_tool_timeout_description: String,
    pub settings_question_timeout: String,
    pub settings_question_timeout_description: String,
    pub settings_subagents_section: String,
    pub settings_subagents_section_note: String,
    pub settings_subagent_model: String,
    pub settings_subagent_model_description: String,
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
    pub settings_deferred_tools_section: String,
    pub settings_deferred_tools_section_note: String,
    pub settings_deferred_tools_enable: String,
    pub settings_deferred_tools_warning: String,
    pub settings_json_repair_section: String,
    pub settings_json_repair_section_note: String,
    pub settings_json_repair: String,
    pub settings_json_repair_description: String,
    pub settings_review_capacity_section: String,
    pub settings_review_capacity_section_note: String,
    pub settings_review_parallelism: String,
    pub settings_review_parallelism_description: String,
    pub settings_review_queue_wait: String,
    pub settings_review_queue_wait_description: String,
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
    // The tray labels are resolved through the same service; refresh them so
    // the menu follows the switch (M22).
    crate::tray::apply_locale(locale, cx);
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

/// G-G6: the ahead/behind badge tooltips of the branch chip.
pub(crate) fn git_ahead(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-ahead",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn git_behind(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-git-behind",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I6: the history card's session count (web `analyzed / total`).
pub(crate) fn insights_sessions_ratio(analyzed: usize, total: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-sessions-ratio",
        TranslationArgs::new()
            .with_number("analyzed", analyzed as f64)
            .with_number("total", total as f64),
        cx,
    )
}

/// G-I6: the history card's message count.
pub(crate) fn insights_messages_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-messages-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I6: the history card's tracked hours (already formatted, so the locale
/// keeps the number formatting the page chose).
pub(crate) fn insights_hours(hours: String, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-hours",
        TranslationArgs::new().with_string("hours", &hours),
        cx,
    )
}

/// G-I6: the history card's covered-day count.
pub(crate) fn insights_days_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-days-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I6: the history card's generation-token line.
pub(crate) fn insights_generation_tokens(count: u64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-generation-tokens",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-I1: one chip of the range control (web `7 Days`).
pub(crate) fn insights_range_days(count: u32, cx: &App) -> String {
    resolve_with(
        "gpui-shell-insights-range-days",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-P2: the selected-day panel's heading, naming the picked local date.
pub(crate) fn tasks_day_title(date: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-day-title",
        TranslationArgs::new().with_string("date", date),
        cx,
    )
}

/// G-P3: the inactive partition's subheader with its job count (web
/// `inactive.title`).
pub(crate) fn tasks_inactive_title(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tasks-inactive-title",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// G-A6: the localized name of a builtin agent (web `agentName` over the
/// shared `shared:agents.*` terms).
pub(crate) fn shared_agent_name(id: &str, cx: &App) -> String {
    resolve_with(&format!("shared.agents.{id}"), TranslationArgs::new(), cx)
}

/// G-M2: the per-candidate target-id field label (web `visual.importId`).
pub(crate) fn manage_mcp_import_id(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-mcp-import-id",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// G-Y4: the export success line (web `trinity.memory.exportDone`), naming the
/// exported node/triple counts and the file path.
pub(crate) fn trinity_memory_export_done(nodes: u64, triples: u64, path: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-trinity-memory-export-done",
        TranslationArgs::new()
            .with_number("nodes", nodes as f64)
            .with_number("triples", triples as f64)
            .with_string("path", path),
        cx,
    )
}

/// G-Y4: the import success line (web `trinity.memory.importDone`).
pub(crate) fn trinity_memory_import_done(added: u64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-trinity-memory-import-done",
        TranslationArgs::new().with_number("added", added as f64),
        cx,
    )
}

/// G-Y4: the cross-identity import warning (web
/// `trinity.memory.importConfirmMessage`).
pub(crate) fn trinity_memory_import_confirm_message(
    container: &str,
    local: &str,
    cx: &App,
) -> String {
    resolve_with(
        "gpui-shell-trinity-memory-import-confirm-message",
        TranslationArgs::new()
            .with_string("container", container)
            .with_string("local", local),
        cx,
    )
}

/// T2-11: the selected-count line in the per-mode skills dialog.
pub(crate) fn mode_skills_selected_count(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-mode-skills-selected-count",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// T2-11: the covered-by status label naming the shadowing source.
pub(crate) fn mode_skills_covered(source: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-mode-skills-status-covered",
        TranslationArgs::new().with_string("source", source),
        cx,
    )
}

/// T2-12: the custom-agent delete confirmation naming the target.
pub(crate) fn manage_agent_delete_warning(name: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-manage-agent-delete-warning",
        TranslationArgs::new().with_string("name", name),
        cx,
    )
}

/// Resolves the localized notice for a `/btw` command without a question.
pub(crate) fn btw_empty(cx: &App) -> String {
    resolve_with("gpui-shell-btw-empty", TranslationArgs::new(), cx)
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

/// Resolves the localized trend range line for the usage chart.
pub(crate) fn usage_trend_range(start: &str, end: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-usage-trend-range",
        TranslationArgs::new()
            .with_string("start", start)
            .with_string("end", end),
        cx,
    )
}

/// Resolves the localized activity-window total line.
pub(crate) fn usage_activity_total(tokens: u64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-usage-activity-total",
        TranslationArgs::new().with_number("tokens", tokens as f64),
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

/// Resolves the delete-all-archived confirmation body for the counts the
/// dialog was opened with (G-SE30: every workspace the page groups).
pub(crate) fn archived_delete_all_confirm_message(
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
pub(crate) fn archived_load_failed(workspace: &str, path: &str, cx: &App) -> String {
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
pub(crate) fn notifications_row(name: &str, count: u32, cx: &App) -> String {
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
pub(crate) fn terminal_resizer_title(mode: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-terminal-resizer-title",
        TranslationArgs::new().with_string("mode", mode),
        cx,
    )
}

/// G-T1: one terminal size tier's label (web `layout.panelMode.*`).
pub(crate) fn terminal_tier_label(tier: crate::shell::TerminalTier, text: &ShellText) -> String {
    match tier {
        crate::shell::TerminalTier::Compact => text.terminal_mode_compact.clone(),
        crate::shell::TerminalTier::Comfortable => text.terminal_mode_comfortable.clone(),
        crate::shell::TerminalTier::Expanded => text.terminal_mode_expanded.clone(),
    }
}

/// Resolves the archived row actions' inline receipts (web
/// `restoreSucceeded` / `deleteArchivedSucceeded` / `deleteAllArchivedSucceeded`)
/// with the row's name and its workspace label.
pub(crate) fn archived_restore_succeeded(name: &str, workspace: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-restore-succeeded",
        TranslationArgs::new()
            .with_string("name", name)
            .with_string("workspace", workspace),
        cx,
    )
}

pub(crate) fn archived_delete_succeeded(name: &str, workspace: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-succeeded",
        TranslationArgs::new()
            .with_string("name", name)
            .with_string("workspace", workspace),
        cx,
    )
}

pub(crate) fn archived_delete_all_succeeded(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-all-succeeded",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// Resolves the single-row delete confirmation body, which names the session
/// and the workspace it is deleted from (web `deleteArchivedConfirmMessage`).
pub(crate) fn archived_delete_message(name: &str, workspace: &str, cx: &App) -> String {
    resolve_with(
        "gpui-shell-settings-archived-delete-message",
        TranslationArgs::new()
            .with_string("name", name)
            .with_string("workspace", workspace),
        cx,
    )
}

/// Tool-row chips that take their number at render time (the pattern lives in
/// the locale file, so word order stays locale-owned).
pub(crate) fn tool_meta_exit_code(code: i64, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-exit-code",
        TranslationArgs::new().with_number("code", code as f64),
        cx,
    )
}

pub(crate) fn tool_meta_lines(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-lines",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn tool_meta_replacements(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-replacements",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn tool_meta_matches(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-matches",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn tool_meta_agents(total: usize, running: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-agents",
        TranslationArgs::new()
            .with_number("total", total as f64)
            .with_number("running", running as f64),
        cx,
    )
}

pub(crate) fn tool_meta_failed(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-tool-meta-failed",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

/// The exploration/execution group chip (web `workspaceGroup.summary`) and
/// the category counts its description lists.
pub(crate) fn transcript_group_summary(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-summary",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn transcript_group_reads(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-reads",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn transcript_group_searches(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-searches",
        TranslationArgs::new().with_number("count", count as f64),
        cx,
    )
}

pub(crate) fn transcript_group_commands(count: usize, cx: &App) -> String {
    resolve_with(
        "gpui-shell-transcript-group-commands",
        TranslationArgs::new().with_number("count", count as f64),
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
        tasks_enable: "Enable".to_string(),
        tasks_disable: "Disable".to_string(),
        tasks_run_now: "Run now".to_string(),
        tasks_overview: "Plan overview".to_string(),
        tasks_stat_total: "Total jobs".to_string(),
        tasks_stat_enabled: "Enabled".to_string(),
        tasks_stat_due_24h: "Due in 24h".to_string(),
        tasks_stat_failed: "Last run failed".to_string(),
        tasks_wd_mon: "Mo".to_string(),
        tasks_wd_tue: "Tu".to_string(),
        tasks_wd_wed: "We".to_string(),
        tasks_wd_thu: "Th".to_string(),
        tasks_wd_fri: "Fr".to_string(),
        tasks_wd_sat: "Sa".to_string(),
        tasks_wd_sun: "Su".to_string(),
        tasks_calendar_scheduled: "Scheduled".to_string(),
        tasks_calendar_ran: "Ran".to_string(),
        tasks_calendar_error: "Error".to_string(),
        tasks_calendar_note: "Marks the next scheduled run and the latest run only; run history is not kept in this shell.".to_string(),
        tasks_list_title: "All jobs".to_string(),
        tasks_badge_running: "Running now".to_string(),
        tasks_badge_next_run: "Next run".to_string(),
        tasks_badge_overdue: "Overdue".to_string(),
        tasks_inactive_paused: "Paused".to_string(),
        tasks_inactive_done: "Finished".to_string(),
        tasks_inactive_invalid: "Invalid schedule".to_string(),
        tasks_day_clear: "Clear".to_string(),
        tasks_day_empty: "Nothing scheduled on this day.".to_string(),
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
        permission_full_access: "Full access".to_string(),
        permission_full_access_desc: "Tools are allowed by default without confirmation."
            .to_string(),
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
        manage_mcp_servers: "Configured servers".to_string(),
        manage_mcp_description: "Lifecycle, enablement, and the resources and prompts catalog of this machine's MCP servers.".to_string(),
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
        manage_mcp_json_description: {
            "Edit the raw MCP server configuration (the same app.json the \
             desktop and CLI read)."
                .to_string()
        },
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
        settings_git_coauthor: "Git co-author".to_string(),
        settings_git_coauthor_description: {
            "Add OpenBitFun as a co-author in built-in commit workflows.".to_string()
        },
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
        settings_page_worktrees_description: "Set session isolation defaults and manage workspace worktrees".to_string(),
        settings_page_execution_description: "Set tool approval rules, timeouts, and task concurrency".to_string(),
        settings_page_automation_description: "Set up reusable AI prompts and commands triggered by events".to_string(),
        settings_page_websearch_description: "Choose the service agents use to search the web".to_string(),
        settings_page_skills_description: "Review installed skills and control their global enabled state".to_string(),
        settings_page_agents_description: "Create and manage agents with a name, description, and system prompt".to_string(),
        settings_page_mcp_description: "Connect MCP servers to give agents access to external tools and data".to_string(),
        settings_page_usage_description: "See model request counts, token use, and cache hit rate".to_string(),
        settings_page_archived_description: "Review, restore, or permanently delete archived sessions".to_string(),
        settings_page_diagnostics_description: "Adjust logging and export troubleshooting information".to_string(),
        settings_jump_session_label: "Session & Workspace".to_string(),
        settings_jump_session_description: {
            "Covered by the global search overlay in this shell: search sessions and workspace content from the command bar."
                .to_string()
        },
        settings_jump_session_action: "Open Global Search".to_string(),
        settings_jump_worktrees_label: "Worktrees".to_string(),
        settings_jump_worktrees_description: {
            "Covered by the Git panel in this shell: manage workspace worktrees there."
                .to_string()
        },
        settings_jump_worktrees_action: "Open Git Panel".to_string(),
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
        settings_usage_range: "Time range".to_string(),
        settings_usage_today: "Today".to_string(),
        settings_usage_week: "This week".to_string(),
        settings_usage_month: "This month".to_string(),
        settings_usage_all: "All time".to_string(),
        settings_usage_refresh: "Refresh".to_string(),
        settings_usage_loading: "Loading usage statistics…".to_string(),
        settings_usage_empty: "No usage recorded in this range.".to_string(),
        settings_usage_requests: "Requests".to_string(),
        settings_usage_cache_hit: "Cache hit".to_string(),
        usage_trend_title: "Detailed trend".to_string(),
        usage_trend_partial: {
            "Only the most recent trend is shown; totals still cover the full \
             range."
                .to_string()
        },
        usage_trend_input: "Input".to_string(),
        usage_trend_output: "Output".to_string(),
        usage_activity_title: "Daily usage, last six months".to_string(),
        usage_activity_description: "Fixed last six months (UTC).".to_string(),
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
        trinity_title: "Trinity".to_string(),
        trinity_status_title: "Trinity Cognitive State".to_string(),
        trinity_status_unavailable: {
            "The Trinity cognitive engine is unavailable. Start trinityd or              check the daemon port."
                .to_string()
        },
        trinity_phase_awake: "Awake".to_string(),
        trinity_phase_dormant: "Dormant".to_string(),
        trinity_phase_offline: "Disconnected".to_string(),
        trinity_label_name: "Being name".to_string(),
        trinity_label_persona: "Persona".to_string(),
        trinity_label_user: "Your name".to_string(),
        trinity_label_birthday: "Birthday".to_string(),
        trinity_label_emotion: "Emotion".to_string(),
        trinity_label_focus: "Focus".to_string(),
        trinity_label_confidence: "Confidence".to_string(),
        trinity_label_cycles: "Cycles".to_string(),
        trinity_label_memory_nodes: "Memory nodes".to_string(),
        trinity_label_memory_triples: "Triples".to_string(),
        trinity_label_vocabulary: "Vocabulary".to_string(),
        trinity_trend_title: "Cognitive trend".to_string(),
        trinity_memory_title: "Her diary".to_string(),
        trinity_memory_add: "Add memory".to_string(),
        trinity_memory_add_placeholder: "Write something to remember…".to_string(),
        trinity_memory_add_project: "Project (optional)".to_string(),
        trinity_memory_submit: "Save".to_string(),
        trinity_memory_search_placeholder: "Search memories…".to_string(),
        trinity_memory_search: "Search".to_string(),
        trinity_memory_no_results: "No matching memories.".to_string(),
        trinity_memory_empty: "No memories yet.".to_string(),
        trinity_memory_load_earlier: "Load earlier".to_string(),
        trinity_memory_no_more: "Reached the earliest".to_string(),
        trinity_memory_forget: "Forget".to_string(),
        trinity_memory_forget_confirm: "Click again to confirm".to_string(),
        trinity_memory_reinforce: "Reinforce".to_string(),
        trinity_memory_edit: "Correct".to_string(),
        trinity_memory_edit_save: "Save correction".to_string(),
        trinity_memory_edit_applied: "Correction applied.".to_string(),
        trinity_memory_edit_missing: "The original memory no longer exists (it may have been forgotten).".to_string(),
        trinity_memory_export: "Export".to_string(),
        trinity_memory_import: "Import".to_string(),
        trinity_memory_export_title: "Export memories".to_string(),
        trinity_memory_import_title: "Import memories".to_string(),
        trinity_memory_path: "File path".to_string(),
        trinity_memory_path_required: "Enter a file path.".to_string(),
        trinity_memory_import_confirm_title: "Cross-identity memory import".to_string(),
        trinity_memory_import_confirm: "Merge anyway".to_string(),
        trinity_action_refresh: "Refresh".to_string(),
        trinity_loading: "Loading…".to_string(),
        trinity_action_failed: "Action failed".to_string(),
        trinity_emotion_positive_high: "Elated".to_string(),
        trinity_emotion_positive_mild: "Mellow".to_string(),
        trinity_emotion_neutral: "Calm".to_string(),
        trinity_emotion_curious: "Curious".to_string(),
        trinity_emotion_confused: "Confused".to_string(),
        trinity_emotion_negative_mild: "Down".to_string(),
        trinity_emotion_negative_high: "Dejected".to_string(),
        trinity_focus_respond: "Responding".to_string(),
        trinity_focus_reflect: "Reflecting".to_string(),
        trinity_focus_explore: "Exploring".to_string(),
        trinity_focus_plan: "Planning".to_string(),
        trinity_focus_idle: "Idle".to_string(),
        trinity_need_competence: "Competence".to_string(),
        trinity_need_autonomy: "Autonomy".to_string(),
        trinity_need_relatedness: "Relatedness".to_string(),
        trinity_need_certainty: "Certainty".to_string(),
        trinity_need_growth: "Growth".to_string(),
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
        settings_execution_section: "Tool execution".to_string(),
        settings_execution_section_note: "Tool execution and waiting for an answer have separate timers; 0 removes that limit.".to_string(),
        settings_tool_timeout: "Tool timeout (seconds)".to_string(),
        settings_tool_timeout_description: "Longest a single tool operation may run; 0 waits indefinitely.".to_string(),
        settings_question_timeout: "Question timeout (seconds)".to_string(),
        settings_question_timeout_description: "Longest wait for you to start answering a question; 0 waits indefinitely.".to_string(),
        settings_subagents_section: "Subagents".to_string(),
        settings_subagents_section_note: "Regular subagents and Swarm use separate concurrency limits.".to_string(),
        settings_subagent_model: "Default subagent model".to_string(),
        settings_subagent_model_description: "Used by subagents without an individual model override.".to_string(),
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
        settings_deferred_tools_section: "Load tools as needed".to_string(),
        settings_deferred_tools_section_note: "Provide tool details to the model on demand to reduce context usage.".to_string(),
        settings_deferred_tools_enable: "Enable".to_string(),
        settings_deferred_tools_warning: "Turning this off loads every tool definition and may increase model usage.".to_string(),
        settings_json_repair_section: "Tool Argument JSON Repair".to_string(),
        settings_json_repair_section_note: "Applies to the next model round.".to_string(),
        settings_json_repair: "Repair malformed tool arguments".to_string(),
        settings_json_repair_description: "Attempt to repair argument JSON only after the provider reports a completed tool call; repair runs before normal tool validation.".to_string(),
        settings_review_capacity_section: "Review capacity".to_string(),
        settings_review_capacity_section_note: "Control how supplemental reviews use model capacity; changing concurrency keeps the review scope unchanged.".to_string(),
        settings_review_parallelism: "Review parallelism".to_string(),
        settings_review_parallelism_description: "Run supplemental review tasks one at a time or two at once.".to_string(),
        settings_review_queue_wait: "Capacity wait time".to_string(),
        settings_review_queue_wait_description: "How long optional reviews wait for provider capacity; excludes checks already running.".to_string(),
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
        theme: service.t("gpui-shell-theme").await,
        language: service.t("gpui-shell-language").await,
        status_starting: service.t("gpui-shell-status-starting").await,
        sessions_title: service.t("gpui-shell-sessions-title").await,
        sessions_empty: service.t("gpui-shell-sessions-empty").await,
        nav_assistant: service.t("gpui-shell-nav-assistant").await,
        nav_tasks: service.t("gpui-shell-nav-tasks").await,
        sessions_reload: service.t("gpui-shell-sessions-reload").await,
        sessions_new: service.t("gpui-shell-sessions-new").await,
        sessions_add_group: service.t("gpui-shell-sessions-add-group").await,
        sessions_show_archived: service.t("gpui-shell-sessions-show-archived").await,
        sessions_show_more: service.t("gpui-shell-sessions-show-more").await,
        sessions_collapse: service.t("gpui-shell-sessions-collapse").await,
        miniapps_title: service.t("gpui-shell-miniapps-title").await,
        miniapps_empty: service.t("gpui-shell-miniapps-empty").await,
        extensions_title: service.t("gpui-shell-extensions-title").await,
        extensions_empty: service.t("gpui-shell-extensions-empty").await,
        sessions_show_less: service.t("gpui-shell-sessions-show-less").await,
        sessions_view_all_tooltip: service.t("gpui-shell-sessions-view-all-tooltip").await,
        sessions_view_grouped_tooltip: service.t("gpui-shell-sessions-view-grouped-tooltip").await,
        sessions_filter_tooltip: service.t("gpui-shell-sessions-filter-tooltip").await,
        sessions_ordering: service.t("gpui-shell-sessions-ordering").await,
        sessions_order_updated: service.t("gpui-shell-sessions-order-updated").await,
        sessions_order_created: service.t("gpui-shell-sessions-order-created").await,
        sessions_order_name: service.t("gpui-shell-sessions-order-name").await,
        sessions_show: service.t("gpui-shell-sessions-show").await,
        sessions_show_all: service.t("gpui-shell-sessions-show-all").await,
        sessions_show_unread: service.t("gpui-shell-sessions-show-unread").await,
        sessions_filters: service.t("gpui-shell-sessions-filters").await,
        // "Running" is a shared term; a dedicated key would trip the
        // shared-term duplicate ratchet (same alias the web
        // `$t(shared:statuses.running)` uses).
        sessions_status_running: service.t("shared.statuses.running").await,
        sessions_reset_filters: service.t("gpui-shell-sessions-reset-filters").await,
        sessions_no_matches: service.t("gpui-shell-sessions-no-matches").await,
        sessions_collapse_all: service.t("gpui-shell-sessions-collapse-all").await,
        sessions_mark_all_read: service.t("gpui-shell-sessions-mark-all-read").await,
        sessions_copy_path: service.t("gpui-shell-sessions-copy-path").await,
        sessions_reveal: service.t("gpui-shell-sessions-reveal").await,
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
        insights_empty_title: service.t("gpui-shell-insights-empty-title").await,
        insights_empty: service.t("gpui-shell-insights-empty").await,
        insights_sessions: service.t("gpui-shell-insights-sessions").await,
        insights_analyzed: service.t("gpui-shell-insights-analyzed").await,
        insights_messages: service.t("gpui-shell-insights-messages").await,
        insights_days_covered: service.t("gpui-shell-insights-days-covered").await,
        insights_whats_working: service.t("gpui-shell-insights-whats-working").await,
        insights_whats_hindering: service.t("gpui-shell-insights-whats-hindering").await,
        insights_quick_wins: service.t("gpui-shell-insights-quick-wins").await,
        insights_looking_ahead: service.t("gpui-shell-insights-looking-ahead").await,
        insights_model_label: service.t("gpui-shell-insights-model-label").await,
        insights_model_primary: service.t("gpui-shell-insights-model-primary").await,
        insights_range_label: service.t("gpui-shell-insights-range-label").await,
        insights_history: service.t("gpui-shell-insights-history").await,
        insights_keep_latest: service.t("gpui-shell-insights-keep-latest").await,
        insights_md_additions: service.t("gpui-shell-insights-md-additions").await,
        insights_features_to_try: service.t("gpui-shell-insights-features-to-try").await,
        insights_usage_patterns: service.t("gpui-shell-insights-usage-patterns").await,
        insights_try_this_prompt: service.t("gpui-shell-insights-try-this-prompt").await,
        insights_copy: service.t("gpui-shell-insights-copy").await,
        tasks_title: service.t("gpui-shell-tasks-title").await,
        tasks_empty_title: service.t("gpui-shell-tasks-empty-title").await,
        tasks_empty: service.t("gpui-shell-tasks-empty").await,
        tasks_enable: service.t("gpui-shell-tasks-enable").await,
        tasks_disable: service.t("gpui-shell-tasks-disable").await,
        tasks_run_now: service.t("gpui-shell-tasks-run-now").await,
        tasks_overview: service.t("gpui-shell-tasks-overview").await,
        tasks_stat_total: service.t("gpui-shell-tasks-stat-total").await,
        tasks_stat_enabled: service.t("gpui-shell-tasks-stat-enabled").await,
        tasks_stat_due_24h: service.t("gpui-shell-tasks-stat-due-24h").await,
        tasks_stat_failed: service.t("gpui-shell-tasks-stat-failed").await,
        tasks_wd_mon: service.t("gpui-shell-tasks-wd-mon").await,
        tasks_wd_tue: service.t("gpui-shell-tasks-wd-tue").await,
        tasks_wd_wed: service.t("gpui-shell-tasks-wd-wed").await,
        tasks_wd_thu: service.t("gpui-shell-tasks-wd-thu").await,
        tasks_wd_fri: service.t("gpui-shell-tasks-wd-fri").await,
        tasks_wd_sat: service.t("gpui-shell-tasks-wd-sat").await,
        tasks_wd_sun: service.t("gpui-shell-tasks-wd-sun").await,
        tasks_calendar_scheduled: service.t("gpui-shell-tasks-calendar-scheduled").await,
        tasks_calendar_ran: service.t("gpui-shell-tasks-calendar-ran").await,
        tasks_calendar_error: service.t("gpui-shell-tasks-calendar-error").await,
        tasks_calendar_note: service.t("gpui-shell-tasks-calendar-note").await,
        tasks_list_title: service.t("gpui-shell-tasks-list-title").await,
        tasks_badge_running: service.t("gpui-shell-tasks-badge-running").await,
        tasks_badge_next_run: service.t("gpui-shell-tasks-badge-next-run").await,
        tasks_badge_overdue: service.t("gpui-shell-tasks-badge-overdue").await,
        tasks_inactive_paused: service.t("gpui-shell-tasks-inactive-paused").await,
        tasks_inactive_done: service.t("gpui-shell-tasks-inactive-done").await,
        tasks_inactive_invalid: service.t("gpui-shell-tasks-inactive-invalid").await,
        tasks_day_clear: service.t("gpui-shell-tasks-day-clear").await,
        tasks_day_empty: service.t("gpui-shell-tasks-day-empty").await,
        panel_pop_out: service.t("gpui-shell-panel-pop-out").await,
        panel_close: service.t("gpui-shell-panel-close").await,
        panel_maximize: service.t("gpui-shell-panel-maximize").await,
        panel_restore: service.t("gpui-shell-panel-restore").await,
        panel_swap: service.t("gpui-shell-panel-swap").await,
        panel_open: service.t("gpui-shell-panel-open").await,
        sidebar_toggle: service.t("gpui-shell-sidebar-toggle").await,
        files_title: service.t("gpui-shell-files-title").await,
        files_loading: service.t("gpui-shell-files-loading").await,
        files_empty: service.t("gpui-shell-files-empty").await,
        files_empty_title: service.t("gpui-shell-files-empty-title").await,
        files_reload: service.t("gpui-shell-files-reload").await,
        files_filter_placeholder: service.t("gpui-shell-files-filter-placeholder").await,
        files_no_matches: service.t("gpui-shell-files-no-matches").await,
        files_too_large: service.t("gpui-shell-files-too-large").await,
        files_binary: service.t("gpui-shell-files-binary").await,
        files_truncated: service.t("gpui-shell-files-truncated").await,
        search_trigger: service.t("gpui-shell-search-trigger").await,
        search_files_tab: service.t("gpui-shell-search-files-tab").await,
        search_sessions_tab: service.t("gpui-shell-search-sessions-tab").await,
        search_run: service.t("shared.tools.search").await,
        search_running: service.t("gpui-shell-search-running").await,
        search_empty: service.t("gpui-shell-search-empty").await,
        search_empty_title: service.t("gpui-shell-search-empty-title").await,
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
        terminal_profile_edit: service.t("shared.tools.edit").await,
        terminal_profile_delete: service.t("gpui-shell-terminal-profile-delete").await,
        terminal_profile_dialog_title: service.t("gpui-shell-terminal-profile-title").await,
        terminal_profile_name: service.t("gpui-shell-terminal-profile-name").await,
        terminal_profile_working_dir: service.t("gpui-shell-terminal-profile-dir").await,
        terminal_profile_startup_command: service.t("gpui-shell-terminal-profile-command").await,
        model_picker: service.t("gpui-shell-model-picker").await,
        model_picker_empty: service.t("gpui-shell-model-picker-empty").await,
        model_default: service.t("gpui-shell-model-default").await,
        composer_local: service.t("gpui-shell-composer-local").await,
        permission_ask: service.t("gpui-shell-permission-ask").await,
        permission_ask_desc: service.t("gpui-shell-permission-ask-desc").await,
        permission_full_access: service.t("gpui-shell-permission-full-access").await,
        permission_full_access_desc: service.t("gpui-shell-permission-full-access-desc").await,
        models_title: service.t("gpui-shell-models-title").await,
        models_default: service.t("gpui-shell-models-default").await,
        models_default_description: service.t("gpui-shell-models-default-desc").await,
        models_configured: service.t("gpui-shell-models-configured").await,
        models_loading: service.t("gpui-shell-models-loading").await,
        models_empty: service.t("gpui-shell-models-empty").await,
        models_enabled: service.t("gpui-shell-models-enabled").await,
        models_disabled: service.t("gpui-shell-models-disabled").await,
        chat_empty: service.t("gpui-shell-chat-empty").await,
        chat_empty_title: service.t("gpui-shell-chat-empty-title").await,
        welcome_title: service.t("gpui-shell-welcome-title").await,
        welcome_slogan: service.t("gpui-shell-welcome-slogan").await,
        chat_input_placeholder: service.t("gpui-shell-chat-input-placeholder").await,
        chat_jump_latest: service.t("gpui-shell-chat-jump-latest").await,
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
        workspace_open_project: service.t("gpui-shell-workspace-open-project").await,
        workspace_new_project: service.t("gpui-shell-workspace-new-project").await,
        workspace_open_path_description: service
            .t("gpui-shell-workspace-open-path-description")
            .await,
        new_project_title: service.t("gpui-shell-new-project-title").await,
        new_project_subtitle: service.t("gpui-shell-new-project-subtitle").await,
        new_project_parent: service.t("gpui-shell-new-project-parent").await,
        new_project_parent_placeholder: service
            .t("gpui-shell-new-project-parent-placeholder")
            .await,
        new_project_name: service.t("gpui-shell-new-project-name").await,
        new_project_name_placeholder: service.t("gpui-shell-new-project-name-placeholder").await,
        new_project_full_path: service.t("gpui-shell-new-project-full-path").await,
        new_project_create: service.t("gpui-shell-new-project-create").await,
        new_project_error_parent: service.t("gpui-shell-new-project-error-parent").await,
        new_project_error_name: service.t("gpui-shell-new-project-error-name").await,
        new_project_error_missing: service.t("gpui-shell-new-project-error-missing").await,
        new_project_error_create: service.t("gpui-shell-new-project-error-create").await,
        terminal_title: service.t("gpui-shell-terminal-title").await,
        terminal_close: service.t("gpui-shell-terminal-close").await,
        terminal_reveal: service.t("gpui-shell-terminal-reveal").await,
        notifications_title: service.t("gpui-shell-notifications-title").await,
        notifications_empty: service.t("gpui-shell-notifications-empty").await,
        notifications_clear_all: service.t("gpui-shell-notifications-clear-all").await,
        terminal_mode_compact: service.t("gpui-shell-terminal-mode-compact").await,
        terminal_mode_comfortable: service.t("gpui-shell-terminal-mode-comfortable").await,
        terminal_mode_expanded: service.t("gpui-shell-terminal-mode-expanded").await,
        terminal_exited: service.t("gpui-shell-terminal-exited").await,
        settings_open: service.t("gpui-shell-settings-open").await,
        settings_title: service.t("shared.features.settings").await,
        tab_close: service.t("gpui-shell-tab-close").await,
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
        git_empty_title: service.t("gpui-shell-git-empty-title").await,
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
        git_discard: service.t("gpui-shell-git-discard").await,
        git_discard_confirm: service.t("gpui-shell-git-discard-confirm").await,
        git_delete_file: service.t("gpui-shell-git-delete-file").await,
        git_delete_confirm: service.t("gpui-shell-git-delete-confirm").await,
        git_commit: service.t("gpui-shell-git-commit").await,
        git_commit_placeholder: service.t("gpui-shell-git-commit-placeholder").await,
        git_push: service.t("gpui-shell-git-push").await,
        git_pull: service.t("gpui-shell-git-pull").await,
        git_not_repo_title: service.t("gpui-shell-git-not-repo-title").await,
        git_not_repo_detail: service.t("gpui-shell-git-not-repo-detail").await,
        git_init_button: service.t("gpui-shell-git-init-button").await,
        git_init_hint: service.t("gpui-shell-git-init-hint").await,
        git_init_prompt: service.t("gpui-shell-git-init-prompt").await,
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
        turn_copy: service.t("gpui-shell-turn-copy").await,
        turn_meta_completed: service.t("gpui-shell-turn-meta-completed").await,
        turn_meta_stopped: service.t("gpui-shell-turn-meta-stopped").await,
        turn_meta_duration: service.t("gpui-shell-turn-meta-duration").await,
        turn_copy_full: service.t("gpui-shell-turn-copy-full").await,
        turn_copy_result: service.t("gpui-shell-turn-copy-result").await,
        code_copy: service.t("gpui-shell-code-copy").await,
        code_copied: service.t("gpui-shell-code-copied").await,
        tool_open_details: service.t("gpui-shell-tool-open-details").await,
        tool_copy: service.t("gpui-shell-tool-copy").await,
        tool_copied: service.t("gpui-shell-tool-copied").await,
        turn_copy_user_label: service.t("gpui-shell-turn-copy-user-label").await,
        subagent_badge: service.t("gpui-shell-subagent-badge").await,
        subagent_open: service.t("gpui-shell-subagent-open").await,
        subagent_names: fetch_subagent_names(&service).await,
        tool_action_command: service.t("gpui-shell-tool-action-command").await,
        tool_action_read: service.t("gpui-shell-tool-action-read").await,
        tool_action_search: service.t("gpui-shell-tool-action-search").await,
        tool_action_web: service.t("gpui-shell-tool-action-web").await,
        tool_action_write: service.t("gpui-shell-tool-action-write").await,
        tool_action_edit: service.t("gpui-shell-tool-action-edit").await,
        tool_action_tool_spec: service.t("gpui-shell-tool-action-tool-spec").await,
        tool_action_tool_call: service.t("gpui-shell-tool-action-tool-call").await,
        tool_action_agent_spawn: service.t("gpui-shell-tool-action-agent-spawn").await,
        tool_action_agent_wait: service.t("gpui-shell-tool-action-agent-wait").await,
        tool_action_agent_list: service.t("gpui-shell-tool-action-agent-list").await,
        tool_action_agent_send: service.t("gpui-shell-tool-action-agent-send").await,
        tool_action_agent_delete: service.t("gpui-shell-tool-action-agent-delete").await,
        btw_badge: service.t("gpui-shell-btw-badge").await,
        sidebar_review_running: service.t("gpui-shell-sidebar-review-running").await,
        child_back: service.t("gpui-shell-child-back").await,
        manage_skills_title: service.t("gpui-shell-manage-skills-title").await,
        manage_skills_loading: service.t("gpui-shell-manage-skills-loading").await,
        manage_skills_empty_title: service.t("gpui-shell-manage-skills-empty-title").await,
        manage_skills_empty: service.t("gpui-shell-manage-skills-empty").await,
        manage_skills_search_placeholder: service
            .t("gpui-shell-manage-skills-search-placeholder")
            .await,
        manage_skills_hide_shadowed: service.t("gpui-shell-manage-skills-hide-shadowed").await,
        manage_skills_no_match: service.t("gpui-shell-manage-skills-no-match").await,
        manage_skill_builtin: service.t("gpui-shell-manage-skill-builtin").await,
        manage_skill_user: service.t("gpui-shell-manage-skill-user").await,
        manage_skill_project: service.t("gpui-shell-manage-skill-project").await,
        manage_skill_shadowed: service.t("gpui-shell-manage-skill-shadowed").await,
        skills_facet_library: service.t("gpui-shell-skills-facet-library").await,
        skills_facet_all: service.t("gpui-shell-skills-facet-all").await,
        skills_facet_groups: service.t("gpui-shell-skills-facet-groups").await,
        skills_facet_sources: service.t("gpui-shell-skills-facet-sources").await,
        skills_facet_explore: service.t("gpui-shell-skills-facet-explore").await,
        skills_market_title: service.t("gpui-shell-skills-market-title").await,
        skills_market_degraded: service.t("gpui-shell-skills-market-degraded").await,
        manage_agents_title: service.t("gpui-shell-manage-agents-title").await,
        manage_agents_modes: service.t("gpui-shell-manage-agents-modes").await,
        manage_agents_subagents: service.t("gpui-shell-manage-agents-subagents").await,
        manage_agents_create: service.t("gpui-shell-manage-agents-create").await,
        manage_agents_loading: service.t("gpui-shell-manage-agents-loading").await,
        manage_agents_empty_title: service.t("gpui-shell-manage-agents-empty-title").await,
        manage_agents_empty: service.t("gpui-shell-manage-agents-empty").await,
        manage_agents_search_placeholder: service
            .t("gpui-shell-manage-agents-search-placeholder")
            .await,
        manage_agents_source_label: service.t("gpui-shell-manage-agents-source-label").await,
        manage_agents_source_any: service.t("gpui-shell-manage-agents-source-any").await,
        manage_agents_kind_label: service.t("gpui-shell-manage-agents-kind-label").await,
        manage_agents_kind_any: service.t("gpui-shell-manage-agents-kind-any").await,
        manage_agents_kind_agent: service.t("gpui-shell-manage-agents-kind-agent").await,
        manage_agents_kind_subagent: service.t("gpui-shell-manage-agents-kind-subagent").await,
        manage_agents_no_match: service.t("gpui-shell-manage-agents-no-match").await,
        manage_agent_create_title: service.t("gpui-shell-manage-agent-create-title").await,
        manage_agent_kind_mode: service.t("gpui-shell-manage-agent-kind-mode").await,
        manage_agent_kind_subagent: service.t("gpui-shell-manage-agent-kind-subagent").await,
        manage_agent_level_user: service.t("gpui-shell-manage-agent-level-user").await,
        manage_agent_level_project: service.t("gpui-shell-manage-agent-level-project").await,
        manage_agent_id: service.t("gpui-shell-manage-agent-id").await,
        manage_agent_name: service.t("gpui-shell-manage-agent-name").await,
        manage_agent_description: service.t("gpui-shell-manage-agent-description").await,
        manage_agent_prompt: service.t("gpui-shell-manage-agent-prompt").await,
        manage_agent_readonly: service.t("gpui-shell-manage-agent-readonly").await,
        manage_agent_review: service.t("gpui-shell-manage-agent-review").await,
        manage_agent_enabled: service.t("gpui-shell-manage-agent-enabled").await,
        manage_agent_disabled: service.t("gpui-shell-manage-agent-disabled").await,
        manage_agent_source_builtin: service.t("gpui-shell-manage-agent-source-builtin").await,
        manage_agent_source_user: service.t("gpui-shell-manage-agent-source-user").await,
        manage_agent_source_project: service.t("gpui-shell-manage-agent-source-project").await,
        manage_agent_source_external: service.t("gpui-shell-manage-agent-source-external").await,
        manage_agent_error_empty_id: service.t("gpui-shell-manage-agent-error-empty-id").await,
        manage_agent_error_bad_id: service.t("gpui-shell-manage-agent-error-bad-id").await,
        manage_agent_error_empty_name: service.t("gpui-shell-manage-agent-error-empty-name").await,
        manage_agent_error_empty_description: service
            .t("gpui-shell-manage-agent-error-empty-description")
            .await,
        manage_agent_error_empty_prompt: service
            .t("gpui-shell-manage-agent-error-empty-prompt")
            .await,
        manage_agent_error_mode_project: service
            .t("gpui-shell-manage-agent-error-mode-project")
            .await,
        manage_agent_edit_title: service.t("gpui-shell-manage-agent-edit-title").await,
        manage_agent_model: service.t("gpui-shell-manage-agent-model").await,
        manage_agent_model_default: service.t("gpui-shell-manage-agent-model-default").await,
        manage_agent_model_failed: service.t("gpui-shell-manage-agent-model-failed").await,
        manage_harness_title: service.t("gpui-shell-manage-harness-title").await,
        manage_harness_subtitle: service.t("gpui-shell-manage-harness-subtitle").await,
        manage_harness_purpose_minimal: service
            .t("gpui-shell-manage-harness-purpose-minimal")
            .await,
        manage_harness_purpose_standard: service
            .t("gpui-shell-manage-harness-purpose-standard")
            .await,
        manage_harness_purpose_ultimate: service
            .t("gpui-shell-manage-harness-purpose-ultimate")
            .await,
        manage_harness_purpose_creative: service
            .t("gpui-shell-manage-harness-purpose-creative")
            .await,
        manage_agent_delete: service.t("gpui-shell-manage-agent-delete").await,
        manage_agent_delete_title: service.t("gpui-shell-manage-agent-delete-title").await,
        // "Edit" is a shared term; a dedicated key would trip the shared-term
        // duplicate ratchet (same alias the terminal profile uses).
        agent_card_edit: service.t("shared.tools.edit").await,
        mode_skills_open: service.t("gpui-shell-mode-skills-open").await,
        mode_skills_title: service.t("gpui-shell-mode-skills-title").await,
        mode_skills_loading: service.t("gpui-shell-mode-skills-loading").await,
        mode_skills_empty: service.t("gpui-shell-mode-skills-empty").await,
        // "Edit" is a shared term; a dedicated key would trip the shared-term
        // duplicate ratchet (same alias the terminal profile uses).
        mode_skills_edit: service.t("shared.tools.edit").await,
        mode_skills_save_hint: service.t("gpui-shell-mode-skills-save-hint").await,
        mode_skills_status_selected: service.t("gpui-shell-mode-skills-status-selected").await,
        mode_skills_status_enabled: service.t("gpui-shell-mode-skills-status-enabled").await,
        mode_skills_reset: service.t("gpui-shell-mode-skills-reset").await,
        mode_skills_source_other: service.t("gpui-shell-mode-skills-source-other").await,
        manage_mcp_title: service.t("gpui-shell-manage-mcp-title").await,
        manage_mcp_servers: service.t("gpui-shell-manage-mcp-servers").await,
        manage_mcp_description: service.t("gpui-shell-manage-mcp-description").await,
        manage_mcp_loading: service.t("gpui-shell-manage-mcp-loading").await,
        manage_mcp_empty_title: service.t("gpui-shell-manage-mcp-empty-title").await,
        manage_mcp_empty: service.t("gpui-shell-manage-mcp-empty").await,
        manage_mcp_start: service.t("gpui-shell-manage-mcp-start").await,
        manage_mcp_stop: service.t("gpui-shell-manage-mcp-stop").await,
        manage_mcp_restart: service.t("gpui-shell-manage-mcp-restart").await,
        manage_mcp_enable: service.t("gpui-shell-manage-mcp-enable").await,
        manage_mcp_disable: service.t("gpui-shell-manage-mcp-disable").await,
        manage_mcp_delete: service.t("gpui-shell-manage-mcp-delete").await,
        manage_mcp_delete_title: service.t("gpui-shell-manage-mcp-delete-title").await,
        manage_mcp_delete_warning: service.t("gpui-shell-manage-mcp-delete-warning").await,
        manage_mcp_browse: service.t("gpui-shell-manage-mcp-browse").await,
        manage_mcp_browse_title: service.t("gpui-shell-manage-mcp-browse-title").await,
        manage_mcp_resources: service.t("gpui-shell-manage-mcp-resources").await,
        manage_mcp_prompts: service.t("gpui-shell-manage-mcp-prompts").await,
        manage_mcp_refresh: service.t("gpui-shell-manage-mcp-refresh").await,
        manage_mcp_disabled: service.t("gpui-shell-manage-mcp-disabled").await,
        manage_mcp_location_builtin: service.t("gpui-shell-manage-mcp-location-builtin").await,
        manage_mcp_state_uninitialized: service
            .t("gpui-shell-manage-mcp-state-uninitialized")
            .await,
        manage_mcp_state_starting: service.t("gpui-shell-manage-mcp-state-starting").await,
        manage_mcp_state_connected: service.t("gpui-shell-manage-mcp-state-connected").await,
        manage_mcp_state_healthy: service.t("gpui-shell-manage-mcp-state-healthy").await,
        manage_mcp_state_needs_auth: service.t("gpui-shell-manage-mcp-state-needs-auth").await,
        manage_mcp_state_reconnecting: service.t("gpui-shell-manage-mcp-state-reconnecting").await,
        manage_mcp_state_failed: service.t("shared.statuses.failed").await,
        manage_mcp_state_stopping: service.t("gpui-shell-manage-mcp-state-stopping").await,
        manage_mcp_state_stopped: service.t("gpui-shell-manage-mcp-state-stopped").await,
        manage_interaction_title: service.t("gpui-shell-manage-interaction-title").await,
        manage_interaction_params: service.t("gpui-shell-manage-interaction-params").await,
        manage_interaction_result_hint: service
            .t("gpui-shell-manage-interaction-result-hint")
            .await,
        manage_interaction_approve: service.t("gpui-shell-manage-interaction-approve").await,
        manage_interaction_reject: service.t("gpui-shell-manage-interaction-reject").await,
        manage_mcp_json_item: service.t("gpui-shell-manage-mcp-json-item").await,
        manage_mcp_json_edit: service.t("gpui-shell-manage-mcp-json-edit").await,
        manage_mcp_json_description: service.t("gpui-shell-manage-mcp-json-description").await,
        manage_mcp_json_loading: service.t("gpui-shell-manage-mcp-json-loading").await,
        manage_mcp_json_save: service.t("gpui-shell-manage-mcp-json-save").await,
        manage_mcp_json_saved: service.t("gpui-shell-manage-mcp-json-saved").await,
        manage_mcp_import: service.t("gpui-shell-manage-mcp-import").await,
        manage_mcp_import_paste: service.t("gpui-shell-manage-mcp-import-paste").await,
        manage_mcp_import_path: service.t("gpui-shell-manage-mcp-import-path").await,
        manage_mcp_import_read: service.t("gpui-shell-manage-mcp-import-read").await,
        manage_mcp_import_file_failed: service.t("gpui-shell-manage-mcp-import-file-failed").await,
        manage_mcp_import_preview: service.t("gpui-shell-manage-mcp-import-preview").await,
        manage_mcp_import_hint: service.t("gpui-shell-manage-mcp-import-hint").await,
        manage_mcp_import_selected: service.t("gpui-shell-manage-mcp-import-selected").await,
        manage_mcp_import_conflict: service.t("gpui-shell-manage-mcp-import-conflict").await,
        manage_mcp_import_select_servers: service
            .t("gpui-shell-manage-mcp-import-select-servers")
            .await,
        manage_mcp_import_invalid_json: service
            .t("gpui-shell-manage-mcp-import-invalid-json")
            .await,
        manage_mcp_import_invalid_config: service
            .t("gpui-shell-manage-mcp-import-invalid-config")
            .await,
        manage_mcp_import_id_required: service.t("gpui-shell-manage-mcp-import-id-required").await,
        manage_mcp_import_invalid_id: service.t("gpui-shell-manage-mcp-import-invalid-id").await,
        manage_mcp_import_id_conflict: service.t("gpui-shell-manage-mcp-import-id-conflict").await,
        manage_mcp_import_shape: service.t("gpui-shell-manage-mcp-import-shape").await,
        manage_mcp_resource_hint: service.t("gpui-shell-manage-mcp-resource-hint").await,
        manage_mcp_resource_loading: service.t("gpui-shell-manage-mcp-resource-loading").await,
        manage_mcp_resource_binary: service.t("gpui-shell-manage-mcp-resource-binary").await,
        manage_mcp_resource_empty: service.t("gpui-shell-manage-mcp-resource-empty").await,
        manage_mcp_resource_failed: service.t("gpui-shell-manage-mcp-resource-failed").await,
        manage_error_dismiss: service.t("gpui-shell-manage-error-dismiss").await,
        value_unsupported: service.t("gpui-shell-value-unsupported").await,
        settings_general: service.t("gpui-shell-settings-general").await,
        settings_version: service.t("gpui-shell-settings-version").await,
        settings_launch_login: service.t("gpui-shell-settings-launch-login").await,
        settings_launch_login_description: service
            .t("gpui-shell-settings-launch-login-description")
            .await,
        settings_close_behavior: service.t("gpui-shell-settings-close-behavior").await,
        settings_close_behavior_description: service
            .t("gpui-shell-settings-close-behavior-description")
            .await,
        settings_close_quit: service.t("gpui-shell-settings-close-quit").await,
        settings_close_minimize: service.t("gpui-shell-settings-close-minimize").await,
        settings_auto_update: service.t("gpui-shell-settings-auto-update").await,
        settings_auto_update_description: service
            .t("gpui-shell-settings-auto-update-description")
            .await,
        settings_notify_turn_done: service.t("gpui-shell-settings-notify-turn-done").await,
        settings_notify_turn_done_description: service
            .t("gpui-shell-settings-notify-turn-done-description")
            .await,
        settings_check_updates: service.t("gpui-shell-settings-check-updates").await,
        settings_update_checking: service.t("gpui-shell-settings-update-checking").await,
        settings_up_to_date: service.t("gpui-shell-settings-up-to-date").await,
        settings_update_available: service.t("gpui-shell-settings-update-available").await,
        settings_update_install: service.t("gpui-shell-settings-update-install").await,
        settings_update_installing: service.t("gpui-shell-settings-update-installing").await,
        settings_update_installed: service.t("gpui-shell-settings-update-installed").await,
        settings_update_restart: service.t("gpui-shell-settings-update-restart").await,
        settings_update_unsupported: service.t("gpui-shell-settings-update-unsupported").await,
        settings_update_failed: service.t("gpui-shell-settings-update-failed").await,
        settings_automation: service.t("gpui-shell-settings-automation").await,
        settings_hooks: service.t("gpui-shell-settings-hooks").await,
        settings_hooks_description: service.t("gpui-shell-settings-hooks-description").await,
        settings_hooks_project: service.t("gpui-shell-settings-hooks-project").await,
        settings_hooks_project_description: service
            .t("gpui-shell-settings-hooks-project-description")
            .await,
        settings_git_coauthor: service.t("gpui-shell-settings-git-coauthor").await,
        settings_git_coauthor_description: service
            .t("gpui-shell-settings-git-coauthor-description")
            .await,
        settings_websearch: service.t("gpui-shell-settings-websearch").await,
        settings_websearch_provider: service.t("gpui-shell-settings-websearch-provider").await,
        settings_websearch_provider_description: service
            .t("gpui-shell-settings-websearch-provider-description")
            .await,
        settings_category_application: service.t("gpui-shell-settings-category-application").await,
        settings_category_ai: service.t("gpui-shell-settings-category-ai").await,
        settings_category_workspace: service.t("gpui-shell-settings-category-workspace").await,
        settings_category_tools: service.t("gpui-shell-settings-category-tools").await,
        settings_category_data: service.t("gpui-shell-settings-category-data").await,
        settings_nav_search_placeholder: service
            .t("gpui-shell-settings-nav-search-placeholder")
            .await,
        settings_nav_search_empty: service.t("gpui-shell-settings-nav-search-empty").await,
        settings_page_general_description: service
            .t("gpui-shell-settings-page-general-description")
            .await,
        settings_page_appearance_description: service
            .t("gpui-shell-settings-page-appearance-description")
            .await,
        settings_page_terminal_description: service
            .t("gpui-shell-settings-page-terminal-description")
            .await,
        settings_page_models_description: service
            .t("gpui-shell-settings-page-models-description")
            .await,
        settings_page_session_description: service
            .t("gpui-shell-settings-page-session-description")
            .await,
        settings_page_worktrees_description: service
            .t("gpui-shell-settings-page-worktrees-description")
            .await,
        settings_page_execution_description: service
            .t("gpui-shell-settings-page-execution-description")
            .await,
        settings_page_automation_description: service
            .t("gpui-shell-settings-page-automation-description")
            .await,
        settings_page_websearch_description: service
            .t("gpui-shell-settings-page-websearch-description")
            .await,
        settings_page_skills_description: service
            .t("gpui-shell-settings-page-skills-description")
            .await,
        settings_page_agents_description: service
            .t("gpui-shell-settings-page-agents-description")
            .await,
        settings_page_mcp_description: service.t("gpui-shell-settings-page-mcp-description").await,
        settings_page_usage_description: service
            .t("gpui-shell-settings-page-usage-description")
            .await,
        settings_page_archived_description: service
            .t("gpui-shell-settings-page-archived-description")
            .await,
        settings_page_diagnostics_description: service
            .t("gpui-shell-settings-page-diagnostics-description")
            .await,
        settings_jump_session_label: service.t("gpui-shell-settings-jump-session-label").await,
        settings_jump_session_description: service
            .t("gpui-shell-settings-jump-session-description")
            .await,
        settings_jump_session_action: service.t("gpui-shell-settings-jump-session-action").await,
        settings_jump_worktrees_label: service.t("gpui-shell-settings-jump-worktrees-label").await,
        settings_jump_worktrees_description: service
            .t("gpui-shell-settings-jump-worktrees-description")
            .await,
        settings_jump_worktrees_action: service
            .t("gpui-shell-settings-jump-worktrees-action")
            .await,
        websearch_provider_free: service.t("gpui-shell-websearch-provider-free").await,
        websearch_provider_exa: service.t("gpui-shell-websearch-provider-exa").await,
        websearch_provider_tavily: service.t("gpui-shell-websearch-provider-tavily").await,
        websearch_provider_http: service.t("gpui-shell-websearch-provider-http").await,
        websearch_http_endpoint: service.t("gpui-shell-websearch-http-endpoint").await,
        websearch_http_endpoint_description: service
            .t("gpui-shell-websearch-http-endpoint-description")
            .await,
        websearch_http_auth_mode: service.t("gpui-shell-websearch-http-auth-mode").await,
        websearch_auth_none: service.t("gpui-shell-websearch-auth-none").await,
        websearch_auth_bearer: service.t("gpui-shell-websearch-auth-bearer").await,
        websearch_auth_header: service.t("gpui-shell-websearch-auth-header").await,
        websearch_http_header_name: service.t("gpui-shell-websearch-http-header-name").await,
        websearch_http_header_name_description: service
            .t("gpui-shell-websearch-http-header-name-description")
            .await,
        websearch_credential: service.t("gpui-shell-websearch-credential").await,
        websearch_credential_description: service
            .t("gpui-shell-websearch-credential-description")
            .await,
        websearch_configured: service.t("gpui-shell-websearch-configured").await,
        websearch_not_configured: service.t("gpui-shell-websearch-not-configured").await,
        websearch_save: service.t("gpui-shell-websearch-save").await,
        websearch_no_credential: service.t("gpui-shell-websearch-no-credential").await,
        quick_action_title: service.t("gpui-shell-quick-action-title").await,
        quick_action_commit_label: service.t("gpui-shell-quick-action-commit-label").await,
        quick_action_commit_prompt: service.t("gpui-shell-quick-action-commit-prompt").await,
        quick_action_create_pr_label: service.t("gpui-shell-quick-action-create-pr-label").await,
        quick_action_create_pr_prompt: service.t("gpui-shell-quick-action-create-pr-prompt").await,
        settings_usage: service.t("gpui-shell-settings-usage").await,
        settings_usage_range: service.t("gpui-shell-settings-usage-range").await,
        settings_usage_today: service.t("gpui-shell-settings-usage-today").await,
        settings_usage_week: service.t("gpui-shell-settings-usage-week").await,
        settings_usage_month: service.t("gpui-shell-settings-usage-month").await,
        settings_usage_all: service.t("gpui-shell-settings-usage-all").await,
        settings_usage_refresh: service.t("gpui-shell-settings-usage-refresh").await,
        settings_usage_loading: service.t("gpui-shell-settings-usage-loading").await,
        settings_usage_empty: service.t("gpui-shell-settings-usage-empty").await,
        settings_usage_requests: service.t("gpui-shell-settings-usage-requests").await,
        settings_usage_cache_hit: service.t("gpui-shell-settings-usage-cache-hit").await,
        usage_trend_title: service.t("gpui-shell-usage-trend-title").await,
        usage_trend_partial: service.t("gpui-shell-usage-trend-partial").await,
        usage_trend_input: service.t("gpui-shell-usage-trend-input").await,
        usage_trend_output: service.t("gpui-shell-usage-trend-output").await,
        usage_activity_title: service.t("gpui-shell-usage-activity-title").await,
        usage_activity_description: service.t("gpui-shell-usage-activity-description").await,
        usage_activity_less: service.t("gpui-shell-usage-activity-less").await,
        usage_activity_more: service.t("gpui-shell-usage-activity-more").await,
        usage_activity_mon: service.t("gpui-shell-usage-activity-mon").await,
        usage_activity_wed: service.t("gpui-shell-usage-activity-wed").await,
        usage_activity_fri: service.t("gpui-shell-usage-activity-fri").await,
        usage_activity_sun: service.t("gpui-shell-usage-activity-sun").await,
        settings_archived: service.t("gpui-shell-settings-archived").await,
        settings_archived_list: service.t("gpui-shell-settings-archived-list").await,
        settings_archived_description: service.t("gpui-shell-settings-archived-description").await,
        settings_archived_empty: service.t("gpui-shell-settings-archived-empty").await,
        settings_archived_loading: service.t("gpui-shell-settings-archived-loading").await,
        settings_archived_refresh: service.t("gpui-shell-settings-archived-refresh").await,
        settings_archived_restore: service.t("gpui-shell-settings-archived-restore").await,
        settings_archived_delete_title: service
            .t("gpui-shell-settings-archived-delete-title")
            .await,
        settings_archived_delete_all: service.t("gpui-shell-settings-archived-delete-all").await,
        settings_archived_delete_all_confirm_title: service
            .t("gpui-shell-settings-archived-delete-all-confirm-title")
            .await,
        settings_diagnostics: service.t("gpui-shell-settings-diagnostics").await,
        settings_logging_level: service.t("gpui-shell-settings-logging-level").await,
        settings_logging_level_description: service
            .t("gpui-shell-settings-logging-level-description")
            .await,
        settings_logs_dir: service.t("gpui-shell-settings-logs-dir").await,
        settings_logs_open: service.t("gpui-shell-settings-logs-open").await,
        settings_diagnostics_note: service.t("gpui-shell-settings-diagnostics-note").await,
        settings_diagnostics_export: service.t("gpui-shell-settings-diagnostics-export").await,
        settings_diagnostics_export_description: service
            .t("gpui-shell-settings-diagnostics-export-description")
            .await,
        settings_diagnostics_export_action: service
            .t("gpui-shell-settings-diagnostics-export-action")
            .await,
        settings_diagnostics_export_title: service
            .t("gpui-shell-settings-diagnostics-export-title")
            .await,
        settings_diagnostics_export_warning: service
            .t("gpui-shell-settings-diagnostics-export-warning")
            .await,
        settings_diagnostics_export_confirm: service
            .t("gpui-shell-settings-diagnostics-export-confirm")
            .await,
        settings_diagnostics_export_success: service
            .t("gpui-shell-settings-diagnostics-export-success")
            .await,
        settings_diagnostics_export_failed: service
            .t("gpui-shell-settings-diagnostics-export-failed")
            .await,
        trinity_title: service.t("gpui-shell-trinity-title").await,
        trinity_status_title: service.t("gpui-shell-trinity-status-title").await,
        trinity_status_unavailable: service.t("gpui-shell-trinity-status-unavailable").await,
        trinity_phase_awake: service.t("gpui-shell-trinity-phase-awake").await,
        trinity_phase_dormant: service.t("gpui-shell-trinity-phase-dormant").await,
        trinity_phase_offline: service.t("gpui-shell-trinity-phase-offline").await,
        trinity_label_name: service.t("gpui-shell-trinity-label-name").await,
        trinity_label_persona: service.t("gpui-shell-trinity-label-persona").await,
        trinity_label_user: service.t("gpui-shell-trinity-label-user").await,
        trinity_label_birthday: service.t("gpui-shell-trinity-label-birthday").await,
        trinity_label_emotion: service.t("gpui-shell-trinity-label-emotion").await,
        trinity_label_focus: service.t("gpui-shell-trinity-label-focus").await,
        trinity_label_confidence: service.t("gpui-shell-trinity-label-confidence").await,
        trinity_label_cycles: service.t("gpui-shell-trinity-label-cycles").await,
        trinity_label_memory_nodes: service.t("gpui-shell-trinity-label-memory-nodes").await,
        trinity_label_memory_triples: service.t("gpui-shell-trinity-label-memory-triples").await,
        trinity_label_vocabulary: service.t("gpui-shell-trinity-label-vocabulary").await,
        trinity_trend_title: service.t("gpui-shell-trinity-trend-title").await,
        trinity_memory_title: service.t("gpui-shell-trinity-memory-title").await,
        trinity_memory_add: service.t("gpui-shell-trinity-memory-add").await,
        trinity_memory_add_placeholder: service
            .t("gpui-shell-trinity-memory-add-placeholder")
            .await,
        trinity_memory_add_project: service.t("gpui-shell-trinity-memory-add-project").await,
        trinity_memory_submit: service.t("gpui-shell-trinity-memory-submit").await,
        trinity_memory_search_placeholder: service
            .t("gpui-shell-trinity-memory-search-placeholder")
            .await,
        trinity_memory_search: service.t("shared.tools.search").await,
        trinity_memory_no_results: service.t("gpui-shell-trinity-memory-no-results").await,
        trinity_memory_empty: service.t("gpui-shell-trinity-memory-empty").await,
        trinity_memory_load_earlier: service.t("gpui-shell-trinity-memory-load-earlier").await,
        trinity_memory_no_more: service.t("gpui-shell-trinity-memory-no-more").await,
        trinity_memory_forget: service.t("gpui-shell-trinity-memory-forget").await,
        trinity_memory_forget_confirm: service.t("gpui-shell-trinity-memory-forget-confirm").await,
        trinity_memory_reinforce: service.t("gpui-shell-trinity-memory-reinforce").await,
        trinity_memory_edit: service.t("gpui-shell-trinity-memory-edit").await,
        trinity_memory_edit_save: service.t("gpui-shell-trinity-memory-edit-save").await,
        trinity_memory_edit_applied: service.t("gpui-shell-trinity-memory-edit-applied").await,
        trinity_memory_edit_missing: service.t("gpui-shell-trinity-memory-edit-missing").await,
        trinity_memory_export: service.t("gpui-shell-trinity-memory-export").await,
        trinity_memory_import: service.t("gpui-shell-trinity-memory-import").await,
        trinity_memory_export_title: service.t("gpui-shell-trinity-memory-export-title").await,
        trinity_memory_import_title: service.t("gpui-shell-trinity-memory-import-title").await,
        trinity_memory_path: service.t("gpui-shell-trinity-memory-path").await,
        trinity_memory_path_required: service.t("gpui-shell-trinity-memory-path-required").await,
        trinity_memory_import_confirm_title: service
            .t("gpui-shell-trinity-memory-import-confirm-title")
            .await,
        trinity_memory_import_confirm: service.t("gpui-shell-trinity-memory-import-confirm").await,
        trinity_action_refresh: service.t("gpui-shell-trinity-action-refresh").await,
        trinity_loading: service.t("gpui-shell-trinity-loading").await,
        trinity_action_failed: service.t("gpui-shell-trinity-action-failed").await,
        trinity_emotion_positive_high: service.t("gpui-shell-trinity-emotion-positive-high").await,
        trinity_emotion_positive_mild: service.t("gpui-shell-trinity-emotion-positive-mild").await,
        trinity_emotion_neutral: service.t("gpui-shell-trinity-emotion-neutral").await,
        trinity_emotion_curious: service.t("gpui-shell-trinity-emotion-curious").await,
        trinity_emotion_confused: service.t("gpui-shell-trinity-emotion-confused").await,
        trinity_emotion_negative_mild: service.t("gpui-shell-trinity-emotion-negative-mild").await,
        trinity_emotion_negative_high: service.t("gpui-shell-trinity-emotion-negative-high").await,
        trinity_focus_respond: service.t("gpui-shell-trinity-focus-respond").await,
        trinity_focus_reflect: service.t("gpui-shell-trinity-focus-reflect").await,
        trinity_focus_explore: service.t("gpui-shell-trinity-focus-explore").await,
        trinity_focus_plan: service.t("gpui-shell-trinity-focus-plan").await,
        trinity_focus_idle: service.t("gpui-shell-trinity-focus-idle").await,
        trinity_need_competence: service.t("gpui-shell-trinity-need-competence").await,
        trinity_need_autonomy: service.t("gpui-shell-trinity-need-autonomy").await,
        trinity_need_relatedness: service.t("gpui-shell-trinity-need-relatedness").await,
        trinity_need_certainty: service.t("gpui-shell-trinity-need-certainty").await,
        trinity_need_growth: service.t("gpui-shell-trinity-need-growth").await,
        plugin_ui_removed: service.t("gpui-shell-plugin-ui-removed").await,
        plugin_ui_rejected: service.t("gpui-shell-plugin-ui-rejected").await,
        plugin_ui_timeout: service.t("gpui-shell-plugin-ui-timeout").await,
        settings_ui_font: service.t("gpui-shell-settings-ui-font").await,
        settings_ui_font_description: service.t("gpui-shell-settings-ui-font-desc").await,
        ui_font_level_compact: service.t("gpui-shell-ui-font-level-compact").await,
        ui_font_level_small: service.t("gpui-shell-ui-font-level-small").await,
        ui_font_level_default: service.t("gpui-shell-ui-font-level-default").await,
        ui_font_level_medium: service.t("gpui-shell-ui-font-level-medium").await,
        ui_font_level_large: service.t("gpui-shell-ui-font-level-large").await,
        ui_font_level_custom: service.t("gpui-shell-ui-font-level-custom").await,
        settings_ui_font_custom: service.t("gpui-shell-settings-ui-font-custom").await,
        settings_ui_font_custom_description: service
            .t("gpui-shell-settings-ui-font-custom-desc")
            .await,
        settings_stream_section: service.t("gpui-shell-settings-stream-section").await,
        settings_stream_section_note: service.t("gpui-shell-settings-stream-section-note").await,
        settings_stream_ttft: service.t("gpui-shell-settings-stream-ttft").await,
        settings_stream_ttft_description: service.t("gpui-shell-settings-stream-ttft-desc").await,
        settings_stream_idle: service.t("gpui-shell-settings-stream-idle").await,
        settings_stream_idle_description: service.t("gpui-shell-settings-stream-idle-desc").await,
        settings_proxy_section: service.t("gpui-shell-settings-proxy-section").await,
        settings_proxy_section_note: service.t("gpui-shell-settings-proxy-section-note").await,
        settings_proxy_enable: service.t("gpui-shell-settings-proxy-enable").await,
        settings_proxy_enable_description: service.t("gpui-shell-settings-proxy-enable-desc").await,
        settings_proxy_url: service.t("gpui-shell-settings-proxy-url").await,
        settings_proxy_url_description: service.t("gpui-shell-settings-proxy-url-desc").await,
        settings_proxy_username: service.t("gpui-shell-settings-proxy-username").await,
        settings_proxy_password: service.t("gpui-shell-settings-proxy-password").await,
        settings_proxy_password_description: service
            .t("gpui-shell-settings-proxy-password-desc")
            .await,
        settings_execution_section: service.t("gpui-shell-settings-execution-section").await,
        settings_execution_section_note: service
            .t("gpui-shell-settings-execution-section-note")
            .await,
        settings_tool_timeout: service.t("gpui-shell-settings-tool-timeout").await,
        settings_tool_timeout_description: service.t("gpui-shell-settings-tool-timeout-desc").await,
        settings_question_timeout: service.t("gpui-shell-settings-question-timeout").await,
        settings_question_timeout_description: service
            .t("gpui-shell-settings-question-timeout-desc")
            .await,
        settings_subagents_section: service.t("gpui-shell-settings-subagents-section").await,
        settings_subagents_section_note: service
            .t("gpui-shell-settings-subagents-section-note")
            .await,
        settings_subagent_model: service.t("gpui-shell-settings-subagent-model").await,
        settings_subagent_model_description: service
            .t("gpui-shell-settings-subagent-model-desc")
            .await,
        settings_subagent_model_inherit: service
            .t("gpui-shell-settings-subagent-model-inherit")
            .await,
        settings_subagent_model_fast: service.t("gpui-shell-settings-subagent-model-fast").await,
        settings_subagent_model_primary: service
            .t("gpui-shell-settings-subagent-model-primary")
            .await,
        settings_subagent_batch: service.t("gpui-shell-settings-subagent-batch").await,
        settings_subagent_batch_safe: service.t("gpui-shell-settings-subagent-batch-safe").await,
        settings_subagent_batch_safe_description: service
            .t("gpui-shell-settings-subagent-batch-safe-desc")
            .await,
        settings_subagent_batch_parallel: service
            .t("gpui-shell-settings-subagent-batch-parallel")
            .await,
        settings_subagent_batch_parallel_description: service
            .t("gpui-shell-settings-subagent-batch-parallel-desc")
            .await,
        settings_subagent_concurrency: service.t("gpui-shell-settings-subagent-concurrency").await,
        settings_subagent_concurrency_description: service
            .t("gpui-shell-settings-subagent-concurrency-desc")
            .await,
        settings_swarm_concurrency: service.t("gpui-shell-settings-swarm-concurrency").await,
        settings_swarm_concurrency_description: service
            .t("gpui-shell-settings-swarm-concurrency-desc")
            .await,
        settings_deferred_tools_section: service
            .t("gpui-shell-settings-deferred-tools-section")
            .await,
        settings_deferred_tools_section_note: service
            .t("gpui-shell-settings-deferred-tools-section-note")
            .await,
        settings_deferred_tools_enable: service
            .t("gpui-shell-settings-deferred-tools-enable")
            .await,
        settings_deferred_tools_warning: service
            .t("gpui-shell-settings-deferred-tools-warning")
            .await,
        settings_json_repair_section: service.t("gpui-shell-settings-json-repair-section").await,
        settings_json_repair_section_note: service
            .t("gpui-shell-settings-json-repair-section-note")
            .await,
        settings_json_repair: service.t("gpui-shell-settings-json-repair").await,
        settings_json_repair_description: service.t("gpui-shell-settings-json-repair-desc").await,
        settings_review_capacity_section: service
            .t("gpui-shell-settings-review-capacity-section")
            .await,
        settings_review_capacity_section_note: service
            .t("gpui-shell-settings-review-capacity-section-note")
            .await,
        settings_review_parallelism: service.t("gpui-shell-settings-review-parallelism").await,
        settings_review_parallelism_description: service
            .t("gpui-shell-settings-review-parallelism-desc")
            .await,
        settings_review_queue_wait: service.t("gpui-shell-settings-review-queue-wait").await,
        settings_review_queue_wait_description: service
            .t("gpui-shell-settings-review-queue-wait-desc")
            .await,
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
