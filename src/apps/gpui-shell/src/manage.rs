//! Management surfaces embedded in the settings page (M21): skills, agents
//! (modes + subagents), and MCP servers.
//!
//! The catalogs are gpui globals loaded asynchronously by the runtime bridge
//! (the same projection pattern as the model catalog): the render path reads
//! the snapshot synchronously and never blocks on core services. Actions go
//! back through [`crate::runtime::runtime_handle`], which re-publishes a
//! catalog event when the mutation lands, so every page stays a projection of
//! the process-global truth the CLI and the desktop app read and write.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputState, Textarea};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, WindowExt};
use gpui_kit::{div, prelude::*, px, App, Global, SharedString, WeakEntity, Window};

use crate::i18n::{self, ShellText};
use crate::shell::ShellWindow;

/// Longest MCP-server target string rendered on a row before truncation.
const MCP_TARGET_LIMIT: usize = 120;
/// Longest interaction `params` JSON rendered in the dialog before truncation.
const INTERACTION_PARAMS_LIMIT: usize = 4_000;

// ---------------------------------------------------------------------------
// Skill catalog
// ---------------------------------------------------------------------------

/// One skill row on the skills page: the scanned skill plus its global
/// enablement. Per-mode skill selection is a deliberate deferred slice
/// (PLAN M21 后置清单).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SkillRow {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// Project-level skills live under `<workspace>/skills`; user skills in
    /// the shared user directory. Built-in skills cannot be disabled here.
    pub(crate) project_level: bool,
    pub(crate) builtin: bool,
    pub(crate) shadowed: bool,
    pub(crate) disabled: bool,
}

impl SkillRow {
    /// Global enablement of one scanned skill against the persisted disable
    /// sets. Keys match exactly (`user::foo` / `project::bar`), the same
    /// membership rule the core resolver applies.
    pub(crate) fn disabled_for(
        key: &str,
        user_level: bool,
        project_level: bool,
        disabled_user: &[String],
        disabled_project: &[String],
    ) -> bool {
        (user_level && disabled_user.iter().any(|entry| entry == key))
            || (project_level && disabled_project.iter().any(|entry| entry == key))
    }
}

/// Snapshot behind the skills page. `loaded` distinguishes "still scanning"
/// from "no skills installed" the way the model catalog does.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SkillCatalog {
    pub(crate) loaded: bool,
    pub(crate) skills: Vec<SkillRow>,
}

impl Global for SkillCatalog {}

impl SkillCatalog {
    pub(crate) fn try_current(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }
}

// ---------------------------------------------------------------------------
// Agent catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentSourceKind {
    Builtin,
    User,
    Project,
    External,
}

/// One card row for a mode or a subagent. `enabled` is `Some` for subagents
/// only (their effective enablement state).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentCardRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) tool_count: usize,
    pub(crate) readonly: bool,
    pub(crate) review: bool,
    pub(crate) source: AgentSourceKind,
    pub(crate) enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AgentCatalog {
    pub(crate) loaded: bool,
    pub(crate) modes: Vec<AgentCardRow>,
    pub(crate) subagents: Vec<AgentCardRow>,
}

impl Global for AgentCatalog {}

impl AgentCatalog {
    pub(crate) fn try_current(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }
}

// ---------------------------------------------------------------------------
// MCP catalog
// ---------------------------------------------------------------------------

/// Runtime status of one server, projected from the core status enum so the
/// shell types stay free of the MCP transport layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpServerState {
    Uninitialized,
    Starting,
    Connected,
    Healthy,
    NeedsAuth,
    Reconnecting,
    Failed,
    Stopping,
    Stopped,
}

impl McpServerState {
    /// Whether the server owns a live (or settling) connection right now.
    pub(crate) fn running(self) -> bool {
        matches!(
            self,
            Self::Connected | Self::Healthy | Self::Reconnecting | Self::Stopping
        )
    }

    pub(crate) fn label(self, text: &ShellText) -> String {
        match self {
            Self::Uninitialized => text.manage_mcp_state_uninitialized.clone(),
            Self::Starting => text.manage_mcp_state_starting.clone(),
            Self::Connected => text.manage_mcp_state_connected.clone(),
            Self::Healthy => text.manage_mcp_state_healthy.clone(),
            Self::NeedsAuth => text.manage_mcp_state_needs_auth.clone(),
            Self::Reconnecting => text.manage_mcp_state_reconnecting.clone(),
            Self::Failed => text.manage_mcp_state_failed.clone(),
            Self::Stopping => text.manage_mcp_state_stopping.clone(),
            Self::Stopped => text.manage_mcp_state_stopped.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpServerRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) remote: bool,
    /// `command args…` for local servers, the URL for remote ones.
    pub(crate) target: String,
    pub(crate) state: McpServerState,
    pub(crate) state_message: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) auto_start: bool,
    pub(crate) builtin: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct McpCatalog {
    pub(crate) loaded: bool,
    pub(crate) servers: Vec<McpServerRow>,
}

impl Global for McpCatalog {}

impl McpCatalog {
    pub(crate) fn try_current(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }
}

/// Resources/prompts snapshot for one server, shown in the browse dialog.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct McpServerCatalogView {
    pub(crate) server_id: String,
    pub(crate) server_name: String,
    pub(crate) resources: Vec<McpResourceRow>,
    pub(crate) prompts: Vec<McpPromptRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpResourceRow {
    pub(crate) uri: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpPromptRow {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) arguments: usize,
}

/// MCP lifecycle actions the page can request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum McpAction {
    Start,
    Stop,
    Restart,
    SetEnabled(bool),
    Delete,
}

// ---------------------------------------------------------------------------
// MCP interactive requests (elicitation / sampling)
// ---------------------------------------------------------------------------

/// Event name the core MCP manager emits on the process-global backend bus.
pub(crate) const MCP_INTERACTION_EVENT: &str = "backend-event-mcpinteractionrequest";

/// One pending ask from an MCP server (elicitation or sampling). The turn
/// parks server-side until the shell answers, so the queue drains front-first
/// through the interaction dialog.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpInteractionRequest {
    pub(crate) interaction_id: String,
    pub(crate) server_id: String,
    pub(crate) server_name: String,
    pub(crate) method: String,
    pub(crate) params: serde_json::Value,
}

/// Queued interaction requests on the shell window.
pub(crate) type McpInteractionQueue = std::collections::VecDeque<McpInteractionRequest>;

/// Pretty-printed `params` for the dialog, truncated the way tool-card
/// previews are (full values stay in the runtime).
pub(crate) fn interaction_params_preview(params: &serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(params).unwrap_or_else(|_| params.to_string());
    if pretty.chars().count() <= INTERACTION_PARAMS_LIMIT {
        return pretty;
    }
    let mut truncated: String = pretty.chars().take(INTERACTION_PARAMS_LIMIT).collect();
    truncated.push('…');
    truncated
}

/// Parses the optional result JSON from the interaction dialog. `None` when
/// the field is empty (a bare approve).
pub(crate) fn parse_interaction_result(raw: &str) -> Result<Option<serde_json::Value>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(trimmed)
        .map(Some)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// Agent draft (create dialog)
// ---------------------------------------------------------------------------

/// One custom-agent creation request from the dialog. Tools are left to the
/// core defaults for the kind; review stays off.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentDraft {
    /// `true` = custom mode, `false` = custom subagent.
    pub(crate) mode_kind: bool,
    pub(crate) project_level: bool,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) prompt: String,
}

/// Validation failures; the page localizes them, the runtime path logs the
/// English reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentDraftError {
    EmptyId,
    BadId,
    EmptyName,
    EmptyDescription,
    EmptyPrompt,
    ModeProjectLevel,
}

impl AgentDraft {
    pub(crate) fn validate(&self) -> Result<(), AgentDraftError> {
        let id = self.id.trim();
        if id.is_empty() {
            return Err(AgentDraftError::EmptyId);
        }
        // Same rule the desktop enforces: one leading letter, then letters,
        // digits, `-`, `_` (desktop regex `^[a-zA-Z][a-zA-Z0-9_-]*$`).
        let valid = id.starts_with(|c: char| c.is_ascii_alphabetic())
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid {
            return Err(AgentDraftError::BadId);
        }
        if self.name.trim().is_empty() {
            return Err(AgentDraftError::EmptyName);
        }
        if self.description.trim().is_empty() {
            return Err(AgentDraftError::EmptyDescription);
        }
        if self.prompt.trim().is_empty() {
            return Err(AgentDraftError::EmptyPrompt);
        }
        // Custom modes are user-level only (desktop parity).
        if self.mode_kind && self.project_level {
            return Err(AgentDraftError::ModeProjectLevel);
        }
        Ok(())
    }
}

impl AgentDraftError {
    pub(crate) fn text(self, text: &ShellText) -> String {
        match self {
            Self::EmptyId => text.manage_agent_error_empty_id.clone(),
            Self::BadId => text.manage_agent_error_bad_id.clone(),
            Self::EmptyName => text.manage_agent_error_empty_name.clone(),
            Self::EmptyDescription => text.manage_agent_error_empty_description.clone(),
            Self::EmptyPrompt => text.manage_agent_error_empty_prompt.clone(),
            Self::ModeProjectLevel => text.manage_agent_error_mode_project.clone(),
        }
    }
}

/// Validates and writes the draft, then reloads the registry so the new
/// definition is visible to sessions immediately. Mirrors the desktop
/// `create_custom_agent` flow: same directory layout, same defaults (kind
/// default tools, review off, subagents read-only).
pub(crate) async fn persist_agent_draft(
    draft: AgentDraft,
    workspace_id: &str,
    root: &Path,
) -> Result<(), String> {
    use openbitfun_agent_runtime::custom_agent::{
        custom_agent_model_or_default, default_custom_agent_tools, CustomAgentKind,
        CustomAgentLevel,
    };
    use openbitfun_agent_runtime::prompt::UserContextPolicy;
    use openbitfun_core::agentic::agents::{CustomMode, CustomSubagent};

    draft
        .validate()
        .map_err(|error| format!("invalid agent definition: {error:?}"))?;

    let id = draft.id.trim().to_lowercase();
    let kind = if draft.mode_kind {
        CustomAgentKind::Mode
    } else {
        CustomAgentKind::Subagent
    };
    let path_manager = openbitfun_core::infrastructure::try_get_path_manager_arc()
        .map_err(|error| format!("path manager unavailable: {error}"))?;
    let agents_dir = if draft.project_level {
        path_manager.project_agents_dir(root)
    } else {
        path_manager.user_agents_dir()
    };
    let file_path = agents_dir.join(format!("{id}.md"));
    if file_path.exists() {
        return Err(format!(
            "agent file '{}' already exists",
            file_path.display()
        ));
    }
    // Registry-wide duplicate guard (the desktop checks ids across levels and
    // built-ins before writing).
    let registry = openbitfun_core::agentic::agents::get_agent_registry();
    let taken = {
        let modes = registry.get_modes_info_for_workspace(None, false).await;
        let subagents = registry.get_subagents_info(None).await;
        modes
            .iter()
            .chain(subagents.iter())
            .any(|info| info.id.eq_ignore_ascii_case(&id))
    };
    if taken {
        return Err(format!("agent id '{id}' conflicts with an existing agent"));
    }

    let path_string = file_path.to_string_lossy().to_string();
    let tools = default_custom_agent_tools(kind);
    let model = custom_agent_model_or_default(kind, None).to_string();
    let readonly = !draft.mode_kind;
    let level = if draft.project_level {
        CustomAgentLevel::Project
    } else {
        CustomAgentLevel::User
    };
    if draft.mode_kind {
        let mode = CustomMode::new(
            id.clone(),
            draft.name.trim().to_string(),
            draft.description.trim().to_string(),
            tools,
            draft.prompt.trim().to_string(),
            readonly,
            path_string,
            model,
            UserContextPolicy::default(),
        );
        mode.save_to_file(None)
            .map_err(|error| format!("failed to write agent file: {error}"))?;
    } else {
        let mut subagent = CustomSubagent::new_with_id_and_model_explicit(
            id.clone(),
            draft.name.trim().to_string(),
            draft.description.trim().to_string(),
            tools,
            draft.prompt.trim().to_string(),
            readonly,
            path_string,
            level,
            model,
            false,
            UserContextPolicy::default(),
        );
        subagent.set_review(false);
        subagent
            .save_to_file(None)
            .map_err(|error| format!("failed to write agent file: {error}"))?;
    }
    registry.load_custom_agents(Some(workspace_id)).await;
    tracing::info!("Created custom agent {id} (workspace {workspace_id})");
    Ok(())
}

// ---------------------------------------------------------------------------
// Action failure notice
// ---------------------------------------------------------------------------

/// Last management-action failure, shown as a dismissible banner on the
/// management pages. A global because the settings render closures only see
/// `cx`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ManageNotice(pub Option<String>);

impl Global for ManageNotice {}

impl ManageNotice {
    pub(crate) fn current(cx: &App) -> Option<String> {
        cx.try_global::<Self>().and_then(|notice| notice.0.clone())
    }

    pub(crate) fn set(message: Option<String>, cx: &mut App) {
        cx.set_global(Self(message));
    }
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

/// The skills page: the scanned catalog with per-skill global enablement.
pub(crate) fn skills_page(text: &ShellText) -> SettingPage {
    let list_text = text.clone();
    let list_field = SettingField::render(move |_options, _window, cx| skills_list(&list_text, cx));

    SettingPage::new(text.manage_skills_title.clone())
        .icon(IconName::Sparkles)
        .resettable(false)
        .group(SettingGroup::new().items(vec![SettingItem::new(
            text.manage_skills_list.clone(),
            list_field,
        )
        .description(text.manage_skills_description.clone())]))
}

fn skills_list(text: &ShellText, cx: &mut App) -> gpui_kit::AnyElement {
    if let Some(message) = ManageNotice::current(cx) {
        return notice_banner(text, &message, cx);
    }
    let Some(catalog) = SkillCatalog::try_current(cx) else {
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.manage_skills_loading.clone())
            .into_any_element();
    };
    if catalog.skills.is_empty() {
        return crate::shell::brand_empty(
            IconName::BookOpen,
            &text.manage_skills_empty_title,
            &text.manage_skills_empty,
        );
    }

    let mut column = div().v_flex().gap_2().w_full();
    for skill in catalog.skills {
        let level_label = if skill.builtin {
            text.manage_skill_builtin.clone()
        } else if skill.project_level {
            text.manage_skill_project.clone()
        } else {
            text.manage_skill_user.clone()
        };
        let row = div().flex().items_center().justify_between().gap_2().child(
            div()
                .v_flex()
                .min_w_0()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(skill.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .flex_shrink_0()
                                .child(level_label),
                        )
                        .when(skill.shadowed, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .flex_shrink_0()
                                    .child(text.manage_skill_shadowed.clone()),
                            )
                        }),
                )
                .when(!skill.description.is_empty(), |row| {
                    row.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(skill.description.clone()),
                    )
                }),
        );
        let row = if skill.builtin {
            // Built-in skills ship with the product and stay enabled.
            row
        } else {
            let key = skill.key.clone();
            let user_level = !skill.project_level;
            row.child(
                Switch::new(SharedString::from(format!("skill-toggle-{}", skill.key)))
                    .checked(!skill.disabled)
                    .on_click(move |checked: &bool, _window, _cx| {
                        set_skill_disabled(key.clone(), user_level, !*checked);
                    })
                    .flex_shrink_0(),
            )
        };
        column = column.child(row);
    }
    column.into_any_element()
}

fn set_skill_disabled(key: String, user_level: bool, disabled: bool) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::error!("Runtime unavailable while toggling skill {key}");
        return;
    };
    handle.set_skill_disabled(key, user_level, disabled);
}

/// The agents page: mode cards, subagent cards, and the create dialog entry.
pub(crate) fn agents_page(text: &ShellText, view: WeakEntity<ShellWindow>) -> SettingPage {
    let modes_text = text.clone();
    let modes_field = SettingField::render(move |_options, _window, cx| {
        agent_cards(AgentListKind::Modes, &modes_text, cx)
    });
    let subagents_text = text.clone();
    let subagents_field = SettingField::render(move |_options, _window, cx| {
        agent_cards(AgentListKind::Subagents, &subagents_text, cx)
    });
    let create_view = view;
    let create_text = text.clone();
    let create_field = SettingField::render(move |_options, _window, _cx| {
        let view = create_view.clone();
        Button::new("agent-create")
            .label(create_text.manage_agents_create.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click(move |_, window, cx| {
                open_agent_create_dialog(view.clone(), window, cx);
            })
            .into_any_element()
    });

    SettingPage::new(text.manage_agents_title.clone())
        .icon(IconName::Bot)
        .resettable(false)
        .group(SettingGroup::new().items(vec![
            SettingItem::new(text.manage_agents_modes.clone(), modes_field),
            SettingItem::new(text.manage_agents_subagents.clone(), subagents_field),
            SettingItem::new(text.manage_agents_create.clone(), create_field),
        ]))
}

enum AgentListKind {
    Modes,
    Subagents,
}

fn agent_cards(kind: AgentListKind, text: &ShellText, cx: &mut App) -> gpui_kit::AnyElement {
    if let Some(message) = ManageNotice::current(cx) {
        return notice_banner(text, &message, cx);
    }
    let Some(catalog) = AgentCatalog::try_current(cx) else {
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.manage_agents_loading.clone())
            .into_any_element();
    };
    let rows = match kind {
        AgentListKind::Modes => catalog.modes,
        AgentListKind::Subagents => catalog.subagents,
    };
    if rows.is_empty() {
        return crate::shell::brand_empty(
            IconName::Bot,
            &text.manage_agents_empty_title,
            &text.manage_agents_empty,
        );
    }

    let mut column = div().v_flex().gap_2().w_full();
    for card in rows {
        let source_label = match card.source {
            AgentSourceKind::Builtin => text.manage_agent_source_builtin.clone(),
            AgentSourceKind::User => text.manage_agent_source_user.clone(),
            AgentSourceKind::Project => text.manage_agent_source_project.clone(),
            AgentSourceKind::External => text.manage_agent_source_external.clone(),
        };
        let mut facts = format!("{} · {}", card.tool_count, source_label);
        if let Some(enabled) = card.enabled {
            facts.push_str(" · ");
            facts.push_str(if enabled {
                &text.manage_agent_enabled
            } else {
                &text.manage_agent_disabled
            });
        }
        let mut badges = div().flex().items_center().gap_2().flex_shrink_0();
        if card.readonly {
            badges = badges.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_agent_readonly.clone()),
            );
        }
        if card.review {
            badges = badges.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_agent_review.clone()),
            );
        }
        column = column.child(
            div()
                .flex()
                .items_start()
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
                                .child(card.name.clone()),
                        )
                        .when(!card.description.is_empty(), |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(card.description.clone()),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(facts),
                        ),
                )
                .child(badges),
        );
    }
    column.into_any_element()
}

/// The MCP page: one row per configured server with lifecycle, enablement,
/// browse, and delete actions.
pub(crate) fn mcp_page(text: &ShellText, view: WeakEntity<ShellWindow>) -> SettingPage {
    let list_view = view;
    let list_text = text.clone();
    let servers_field = SettingField::render(move |_options, _window, cx| {
        mcp_server_list(&list_text, list_view.clone(), cx)
    });

    SettingPage::new(text.manage_mcp_title.clone())
        .icon(IconName::Plug)
        .resettable(false)
        .group(SettingGroup::new().items(vec![SettingItem::new(
            text.manage_mcp_servers.clone(),
            servers_field,
        )
        .description(text.manage_mcp_description.clone())]))
}

fn mcp_server_list(
    text: &ShellText,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    if let Some(message) = ManageNotice::current(cx) {
        return notice_banner(text, &message, cx);
    }
    let Some(catalog) = McpCatalog::try_current(cx) else {
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.manage_mcp_loading.clone())
            .into_any_element();
    };
    if catalog.servers.is_empty() {
        return crate::shell::brand_empty(
            IconName::Plug,
            &text.manage_mcp_empty_title,
            &text.manage_mcp_empty,
        );
    }

    let mut column = div().v_flex().gap_3().w_full();
    for server in catalog.servers {
        column = column.child(mcp_server_row(&server, text, view.clone(), cx));
    }
    column.into_any_element()
}

fn mcp_server_row(
    server: &McpServerRow,
    text: &ShellText,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
    let mut target = server.target.clone();
    if target.chars().count() > MCP_TARGET_LIMIT {
        target = format!(
            "{}…",
            target.chars().take(MCP_TARGET_LIMIT).collect::<String>()
        );
    }
    let state_label = match &server.state_message {
        Some(message) if !message.is_empty() => {
            format!("{} ({message})", server.state.label(text))
        }
        _ => server.state.label(text),
    };
    let mut facts = vec![target, state_label];
    if !server.enabled {
        facts.push(text.manage_mcp_disabled.clone());
    }
    if server.builtin {
        facts.push(text.manage_mcp_location_builtin.clone());
    }

    let mut actions = div().flex().items_center().gap_1().flex_shrink_0();
    if !server.state.running() {
        let action_id = server.id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("mcp-start-{}", server.id)))
                .label(text.manage_mcp_start.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _window, _cx| {
                    run_mcp_action(McpAction::Start, action_id.clone());
                }),
        );
    }
    if server.state.running() {
        let stop_id = server.id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("mcp-stop-{}", server.id)))
                .label(text.manage_mcp_stop.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _window, _cx| {
                    run_mcp_action(McpAction::Stop, stop_id.clone());
                }),
        );
        let restart_id = server.id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("mcp-restart-{}", server.id)))
                .label(text.manage_mcp_restart.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _window, _cx| {
                    run_mcp_action(McpAction::Restart, restart_id.clone());
                }),
        );
    }
    if !server.builtin {
        let toggle_id = server.id.clone();
        let enable = !server.enabled;
        actions = actions.child(
            Button::new(SharedString::from(format!("mcp-toggle-{}", server.id)))
                .label(if enable {
                    text.manage_mcp_enable.clone()
                } else {
                    text.manage_mcp_disable.clone()
                })
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _window, _cx| {
                    run_mcp_action(McpAction::SetEnabled(enable), toggle_id.clone());
                }),
        );
        let browse_view = view.clone();
        let browse_id = server.id.clone();
        let browse_name = server.name.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("mcp-browse-{}", server.id)))
                .label(text.manage_mcp_browse.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, window, cx| {
                    open_browse_dialog(
                        browse_view.clone(),
                        browse_id.clone(),
                        browse_name.clone(),
                        window,
                        cx,
                    );
                }),
        );
        let delete_view = view.clone();
        let delete_id = server.id.clone();
        actions = actions.child(
            Button::new(SharedString::from(format!("mcp-delete-{}", server.id)))
                .label(text.manage_mcp_delete.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, window, cx| {
                    open_delete_confirm(delete_view.clone(), delete_id.clone(), window, cx);
                }),
        );
    }

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
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(server.name.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .flex_shrink_0()
                                .child(if server.remote { "remote" } else { "local" }),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(facts.join(" · ")),
                ),
        )
        .child(actions)
        .into_any_element()
}

fn run_mcp_action(action: McpAction, server_id: String) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::error!("Runtime unavailable while running an MCP action for {server_id}");
        return;
    };
    handle.mcp_server_action(action, server_id);
}

/// Dismissible error banner shared by the three management pages.
fn notice_banner(text: &ShellText, message: &str, cx: &mut App) -> gpui_kit::AnyElement {
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().danger)
                .truncate()
                .child(message.to_string()),
        )
        .child(
            Button::new("manage-notice-dismiss")
                .label(text.manage_error_dismiss.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(|_, _window, cx| ManageNotice::set(None, cx)),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

/// Shared field row for the dialogs: a muted label above the control.
fn dialog_field(
    label: &str,
    label_color: gpui_kit::Hsla,
    field: impl IntoElement,
) -> gpui_kit::AnyElement {
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(label_color)
                .child(label.to_string()),
        )
        .child(field)
        .into_any_element()
}

/// Kind selector for the create dialog (rebuilt each frame by the dialog
/// builder, so it takes a fresh view clone per call).
fn agent_kind_button(
    view: WeakEntity<ShellWindow>,
    label: String,
    active: bool,
    next_kind: bool,
) -> gpui_kit::AnyElement {
    Button::new(SharedString::from(format!(
        "agent-kind-{}",
        if next_kind { "mode" } else { "subagent" }
    )))
    .label(label)
    .small()
    .with_variant(if active {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    })
    .on_click(move |_, _window, cx| {
        if let Some(shell) = view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.agent_create_kind_mode = next_kind;
                cx.notify();
            });
        }
    })
    .into_any_element()
}

/// Level selector for the create dialog (subagents only).
fn agent_level_button(
    view: WeakEntity<ShellWindow>,
    label: String,
    active: bool,
    next_level: bool,
) -> gpui_kit::AnyElement {
    Button::new(SharedString::from(format!(
        "agent-level-{}",
        if next_level { "project" } else { "user" }
    )))
    .label(label)
    .small()
    .with_variant(if active {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    })
    .on_click(move |_, _window, cx| {
        if let Some(shell) = view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.agent_create_project_level = next_level;
                cx.notify();
            });
        }
    })
    .into_any_element()
}

/// Opens the custom-agent creation dialog. The input states live on the
/// window (a dialog builder runs every frame); the submission goes through
/// the runtime and the dialog closes when the catalog event lands.
pub(crate) fn open_agent_create_dialog(
    view: WeakEntity<ShellWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (id_input, name_input, description_input, prompt_input) = {
        let shell = shell.read(cx);
        (
            shell.agent_id_input.clone(),
            shell.agent_name_input.clone(),
            shell.agent_description_input.clone(),
            shell.agent_prompt_input.clone(),
        )
    };
    let (confirm_id, confirm_name, confirm_description, confirm_prompt) = (
        id_input.clone(),
        name_input.clone(),
        description_input.clone(),
        prompt_input.clone(),
    );
    for input in [&id_input, &name_input, &description_input] {
        input.update(cx, |state, cx| state.set_value(String::new(), window, cx));
    }
    prompt_input.update(cx, |state, cx| state.set_value(String::new(), window, cx));
    shell.update(cx, |shell, cx| {
        shell.agent_create_kind_mode = false;
        shell.agent_create_project_level = false;
        shell.agent_create_error = None;
        shell.agent_create_active = true;
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, _window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let (kind_mode, project_level) = {
            let shell = shell.read(cx);
            (
                shell.agent_create_kind_mode,
                shell.agent_create_project_level,
            )
        };
        let read_input = |input: &gpui_kit::Entity<InputState>, cx: &App| {
            input.read(cx).value().trim().to_string()
        };
        let draft = AgentDraft {
            mode_kind: kind_mode,
            project_level,
            id: read_input(&confirm_id, cx),
            name: read_input(&confirm_name, cx),
            description: read_input(&confirm_description, cx),
            prompt: confirm_prompt.read(cx).value().trim().to_string(),
        };
        if let Err(error) = draft.validate() {
            let text = i18n::current(cx);
            let message = error.text(&text);
            shell.update(cx, |shell, cx| {
                shell.agent_create_error = Some(message);
                cx.notify();
            });
            return;
        }
        shell.update(cx, |shell, cx| {
            shell.agent_create_error = None;
            cx.notify();
        });
        if let Some(handle) = crate::runtime::runtime_handle() {
            handle.create_custom_agent(draft);
        } else {
            tracing::error!("Runtime unavailable while creating a custom agent");
        }
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.agent_create_active = false;
                shell.agent_create_error = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let Some(shell) = view.upgrade() else {
            return dialog;
        };
        let (kind_mode, project_level, error, label_color) = {
            let shell = shell.read(cx);
            (
                shell.agent_create_kind_mode,
                shell.agent_create_project_level,
                shell.agent_create_error.clone(),
                cx.theme().muted_foreground,
            )
        };
        let mut content = div()
            .w(px(460.))
            .v_flex()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(agent_kind_button(
                        view.clone(),
                        text.manage_agent_kind_mode.clone(),
                        kind_mode,
                        true,
                    ))
                    .child(agent_kind_button(
                        view.clone(),
                        text.manage_agent_kind_subagent.clone(),
                        !kind_mode,
                        false,
                    )),
            )
            // Custom modes are user-level only, so the level selector only
            // exists for subagents.
            .when(!kind_mode, |row| {
                row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(agent_level_button(
                            view.clone(),
                            text.manage_agent_level_user.clone(),
                            !project_level,
                            false,
                        ))
                        .child(agent_level_button(
                            view.clone(),
                            text.manage_agent_level_project.clone(),
                            project_level,
                            true,
                        )),
                )
            })
            .child(dialog_field(
                &text.manage_agent_id,
                label_color,
                Input::new(&id_input),
            ))
            .child(dialog_field(
                &text.manage_agent_name,
                label_color,
                Input::new(&name_input),
            ))
            .child(dialog_field(
                &text.manage_agent_description,
                label_color,
                Input::new(&description_input),
            ))
            .child(dialog_field(
                &text.manage_agent_prompt,
                label_color,
                Textarea::new(&prompt_input),
            ));
        if let Some(error) = error {
            content = content.child(div().text_xs().text_color(cx.theme().danger).child(error));
        }

        dialog
            .title(div().child(text.manage_agent_create_title.clone()))
            .child(content)
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("agent-create-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("agent-create-confirm")
                            .label(text.session_action_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the resources/prompts browse dialog for one server and kicks off the
/// first catalog load. The dialog body renders from the window's latest
/// snapshot, so the refresh button just re-publishes a load.
pub(crate) fn open_browse_dialog(
    view: WeakEntity<ShellWindow>,
    server_id: String,
    server_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    shell.update(cx, |shell, cx| {
        shell.mcp_browse = None;
        shell.mcp_browse_loading = Some((server_id.clone(), server_name));
        cx.notify();
    });
    if let Some(handle) = crate::runtime::runtime_handle() {
        handle.reload_mcp_server_catalog(server_id, false);
    }
    let refresh_view = view.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let Some(shell) = refresh_view.upgrade() else {
            return dialog;
        };
        let (loading, snapshot) = {
            let shell = shell.read(cx);
            (shell.mcp_browse_loading.clone(), shell.mcp_browse.clone())
        };
        let mut content = div().w(px(520.)).v_flex().gap_2();
        if let Some(view) = snapshot {
            let section_label = |label: String| {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label)
            };
            let mut entries = div().v_flex().gap_2();
            entries = entries.child(section_label(text.manage_mcp_resources.clone()));
            if view.resources.is_empty() {
                entries = entries.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.manage_mcp_empty.clone()),
                );
            }
            for resource in &view.resources {
                let mut label = resource.name.clone();
                if resource.name != resource.uri {
                    label.push_str(" · ");
                    label.push_str(&resource.uri);
                }
                entries = entries.child(
                    div()
                        .v_flex()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(label),
                        )
                        .when_some(
                            resource.description.clone().filter(|d| !d.is_empty()),
                            |row, description| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(description),
                                )
                            },
                        ),
                );
            }
            entries = entries.child(section_label(text.manage_mcp_prompts.clone()));
            if view.prompts.is_empty() {
                entries = entries.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.manage_mcp_empty.clone()),
                );
            }
            for prompt in &view.prompts {
                let arguments = if prompt.arguments == 1 {
                    format!(" (1)")
                } else if prompt.arguments > 1 {
                    format!(" ({})", prompt.arguments)
                } else {
                    String::new()
                };
                entries = entries.child(
                    div()
                        .v_flex()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(prompt.name.clone()),
                        )
                        .when_some(
                            prompt.description.clone().filter(|d| !d.is_empty()),
                            |row, description| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(format!("{description}{arguments}")),
                                )
                            },
                        ),
                );
            }
            content = content.child(entries);
            content = content.child(
                Button::new("mcp-browse-refresh")
                    .label(text.manage_mcp_refresh.clone())
                    .small()
                    .with_variant(ButtonVariant::Secondary)
                    .on_click({
                        let server_id = view.server_id.clone();
                        move |_, _window, _cx| {
                            if let Some(handle) = crate::runtime::runtime_handle() {
                                handle.reload_mcp_server_catalog(server_id.clone(), true);
                            }
                        }
                    }),
            );
        } else {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_mcp_loading.clone()),
            );
            let _ = loading;
        }

        dialog
            .title(div().child(text.manage_mcp_browse_title.clone()))
            .child(content)
    });
}

/// Opens the delete confirmation for one server; the actual removal runs on
/// confirm (destructive, so it never fires from the row directly).
pub(crate) fn open_delete_confirm(
    view: WeakEntity<ShellWindow>,
    server_id: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    shell.update(cx, |shell, cx| {
        shell.mcp_pending_delete = Some(server_id);
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let pending = shell.read(cx).mcp_pending_delete.clone();
        shell.update(cx, |shell, cx| {
            shell.mcp_pending_delete = None;
            cx.notify();
        });
        if let Some(server_id) = pending {
            run_mcp_action(McpAction::Delete, server_id);
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.mcp_pending_delete = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let target = view
            .upgrade()
            .and_then(|shell| shell.read(cx).mcp_pending_delete.clone())
            .unwrap_or_default();
        dialog
            .title(div().child(text.manage_mcp_delete_title.clone()))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(format!("{} ({target})", text.manage_mcp_delete_warning)),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("mcp-delete-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("mcp-delete-confirm")
                            .label(text.manage_mcp_delete.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens (or refreshes) the interaction dialog for the front of the pending
/// queue. Called from the event pump when an ask arrives; the queue keeps
/// later asks parked until this one is answered.
pub(crate) fn open_interaction_dialog(
    view: WeakEntity<ShellWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let result_input = {
        let shell = shell.read(cx);
        shell.mcp_interaction_result_input.clone()
    };
    result_input.update(cx, |state, cx| state.set_value(String::new(), window, cx));
    shell.update(cx, |shell, cx| {
        shell.mcp_interaction_error = None;
        cx.notify();
    });

    let approve_view = view.clone();
    let reject_view = view.clone();
    let approve = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        answer_interaction(approve_view.clone(), true, window, cx);
    };
    let reject = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        answer_interaction(reject_view.clone(), false, window, cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let Some(shell) = view.upgrade() else {
            return dialog;
        };
        let (request, error, label_color) = {
            let shell = shell.read(cx);
            (
                shell.mcp_interactions.front().cloned(),
                shell.mcp_interaction_error.clone(),
                cx.theme().muted_foreground,
            )
        };
        let Some(request) = request else {
            return dialog;
        };
        let mut content = div()
            .w(px(520.))
            .v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(format!("{} · {}", request.server_name, request.method)),
            )
            .child(dialog_field(
                &text.manage_interaction_params,
                label_color,
                div()
                    .h(px(200.))
                    .overflow_hidden()
                    .rounded_sm()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(interaction_params_preview(&request.params)),
            ))
            .child(dialog_field(
                &text.manage_interaction_result_hint,
                label_color,
                Input::new(&result_input),
            ));
        if let Some(error) = error {
            content = content.child(div().text_xs().text_color(cx.theme().danger).child(error));
        }

        dialog
            .title(div().child(text.manage_interaction_title.clone()))
            .child(content)
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("mcp-interaction-reject")
                            .label(text.manage_interaction_reject.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(reject.clone()),
                    )
                    .child(
                        Button::new("mcp-interaction-approve")
                            .label(text.manage_interaction_approve.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(approve.clone()),
                    ),
            )
    });
}

/// Answers the front interaction: parses the optional result JSON on approve,
/// submits through the runtime, and pops the queue (closing the dialog when
/// nothing is left).
fn answer_interaction(
    view: WeakEntity<ShellWindow>,
    approve: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (request, raw_result) = {
        let shell = shell.read(cx);
        (
            shell.mcp_interactions.front().cloned(),
            shell
                .mcp_interaction_result_input
                .read(cx)
                .value()
                .trim()
                .to_string(),
        )
    };
    let Some(request) = request else {
        window.close_dialog(cx);
        return;
    };
    let result = if approve {
        match parse_interaction_result(&raw_result) {
            Ok(result) => {
                shell.update(cx, |shell, cx| {
                    shell.mcp_interaction_error = None;
                    cx.notify();
                });
                result
            }
            Err(message) => {
                shell.update(cx, |shell, cx| {
                    shell.mcp_interaction_error = Some(message);
                    cx.notify();
                });
                return;
            }
        }
    } else {
        None
    };
    shell.update(cx, |shell, cx| {
        shell.mcp_interactions.pop_front();
        cx.notify();
    });
    if let Some(handle) = crate::runtime::runtime_handle() {
        handle.submit_mcp_interaction(request.interaction_id.clone(), approve, result);
    } else {
        tracing::error!(
            "Runtime unavailable while answering MCP interaction {}",
            request.interaction_id
        );
    }
    if shell.read(cx).mcp_interactions.is_empty() {
        window.close_dialog(cx);
    } else {
        // The dialog builder renders the next ask on the following frame.
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_disabled_membership_follows_level_sets() {
        let user = vec!["user::alpha".to_string()];
        let project = vec!["project::beta".to_string()];
        assert!(SkillRow::disabled_for(
            "user::alpha",
            true,
            false,
            &user,
            &project
        ));
        // The wrong level's set never disables a key.
        assert!(!SkillRow::disabled_for(
            "user::alpha",
            false,
            true,
            &user,
            &project
        ));
        assert!(SkillRow::disabled_for(
            "project::beta",
            false,
            true,
            &user,
            &project
        ));
        assert!(!SkillRow::disabled_for(
            "user::other",
            true,
            false,
            &user,
            &project
        ));
        assert!(!SkillRow::disabled_for("user::alpha", true, true, &[], &[]));
    }

    #[test]
    fn agent_draft_validation_mirrors_the_desktop_rules() {
        let valid = AgentDraft {
            mode_kind: false,
            project_level: true,
            id: "writer-1".to_string(),
            name: "Writer".to_string(),
            description: "d".to_string(),
            prompt: "p".to_string(),
        };
        assert_eq!(valid.validate(), Ok(()));

        let patch = |change: fn(&mut AgentDraft)| {
            let mut draft = valid.clone();
            change(&mut draft);
            draft
        };
        assert_eq!(
            patch(|d| d.id = "  ".into()).validate(),
            Err(AgentDraftError::EmptyId)
        );
        assert_eq!(
            patch(|d| d.id = "1bad".into()).validate(),
            Err(AgentDraftError::BadId)
        );
        assert_eq!(
            patch(|d| d.id = "has space".into()).validate(),
            Err(AgentDraftError::BadId)
        );
        assert_eq!(
            patch(|d| d.name = String::new()).validate(),
            Err(AgentDraftError::EmptyName)
        );
        assert_eq!(
            patch(|d| d.description = " ".into()).validate(),
            Err(AgentDraftError::EmptyDescription)
        );
        assert_eq!(
            patch(|d| d.prompt = String::new()).validate(),
            Err(AgentDraftError::EmptyPrompt)
        );
        // Custom modes are user-level only, subagents take both levels.
        assert_eq!(
            patch(|d| d.mode_kind = true).validate(),
            Err(AgentDraftError::ModeProjectLevel)
        );
        assert_eq!(
            patch(|d| {
                d.mode_kind = true;
                d.project_level = false;
            })
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn mcp_running_states_drive_the_row_actions() {
        use McpServerState::{
            Connected, Failed, Healthy, NeedsAuth, Reconnecting, Starting, Stopped, Stopping,
            Uninitialized,
        };
        for state in [Connected, Healthy, Reconnecting, Stopping] {
            assert!(state.running(), "{state:?} counts as running");
        }
        for state in [Uninitialized, Starting, NeedsAuth, Failed, Stopped] {
            assert!(!state.running(), "{state:?} does not count as running");
        }
    }

    #[test]
    fn interaction_payload_round_trips_and_previews() {
        let raw = serde_json::json!({
            "interactionId": "mcp_interaction_1",
            "serverId": "srv",
            "serverName": "Demo",
            "method": "elicitation/create",
            "params": { "message": "hello" },
        });
        let request: McpInteractionRequest =
            serde_json::from_value(raw).expect("camelCase payload parses");
        assert_eq!(request.interaction_id, "mcp_interaction_1");
        assert_eq!(request.server_name, "Demo");
        assert_eq!(request.method, "elicitation/create");

        let big = serde_json::json!({ "blob": "x".repeat(INTERACTION_PARAMS_LIMIT + 10) });
        let preview = interaction_params_preview(&big);
        assert!(preview.chars().count() <= INTERACTION_PARAMS_LIMIT + 1);
        assert!(preview.ends_with('…'));

        assert_eq!(parse_interaction_result("  ").unwrap(), None);
        assert_eq!(
            parse_interaction_result("{\"ok\":true}").unwrap(),
            Some(serde_json::json!({ "ok": true }))
        );
        assert!(parse_interaction_result("{nope").is_err());
    }
}
