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
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt, WindowExt};
use gpui_kit::{
    canvas, div, point, prelude::*, px, size, Anchor, App, Background, BorderStyle, Bounds,
    Context, Corners, Edges, FontWeight, Global, Hsla, PathBuilder, SharedString, WeakEntity,
    Window,
};

use crate::i18n::{self, ShellText};
use crate::shell::ShellWindow;

/// Longest MCP-server target string rendered on a row before truncation.
const MCP_TARGET_LIMIT: usize = 120;
/// Longest interaction `params` JSON rendered in the dialog before truncation.
const INTERACTION_PARAMS_LIMIT: usize = 4_000;

// ---------------------------------------------------------------------------
// G-A6: harness overview zone
// ---------------------------------------------------------------------------

/// The four builtin harness profiles, in web `HARNESS_IDS` order.
pub(crate) const HARNESS_IDS: [&str; 4] = ["Minimal", "Standard", "Ultimate", "Creative"];

/// The web card's route viewBox (`STRATEGY_ROUTES`, `preserveAspectRatio`
/// stretched over the strip).
const HARNESS_ROUTE_WIDTH: f32 = 320.;
const HARNESS_ROUTE_HEIGHT: f32 = 44.;

/// One drawing command of a strategy route in the 320×44 route box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum HarnessRouteCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// Cubic Bézier: control A x/y, control B x/y, end point x/y.
    CurveTo(f32, f32, f32, f32, f32, f32),
}

/// One harness profile's route: the paths plus the node dots (web
/// `STRATEGY_ROUTES` path `d` strings and `nodes` tuples).
pub(crate) struct HarnessRouteSpec {
    pub(crate) paths: &'static [&'static [HarnessRouteCmd]],
    pub(crate) nodes: &'static [(f32, f32)],
}

const HARNESS_MINIMAL_PATH: &[HarnessRouteCmd] = &[
    HarnessRouteCmd::MoveTo(0., 22.),
    HarnessRouteCmd::LineTo(320., 22.),
];
const HARNESS_STANDARD_PATH: &[HarnessRouteCmd] = &[
    HarnessRouteCmd::MoveTo(0., 22.),
    HarnessRouteCmd::LineTo(320., 22.),
];
const HARNESS_ULTIMATE_PATH_MAIN: &[HarnessRouteCmd] = &[
    HarnessRouteCmd::MoveTo(0., 22.),
    HarnessRouteCmd::LineTo(48., 22.),
    HarnessRouteCmd::CurveTo(68., 22., 70., 7., 92., 7.),
    HarnessRouteCmd::LineTo(222., 7.),
    HarnessRouteCmd::CurveTo(242., 7., 245., 22., 267., 22.),
    HarnessRouteCmd::LineTo(320., 22.),
];
const HARNESS_ULTIMATE_PATH_BRANCH: &[HarnessRouteCmd] = &[
    HarnessRouteCmd::MoveTo(48., 22.),
    HarnessRouteCmd::CurveTo(68., 22., 70., 37., 92., 37.),
    HarnessRouteCmd::LineTo(222., 37.),
    HarnessRouteCmd::CurveTo(242., 37., 245., 22., 267., 22.),
];
const HARNESS_CREATIVE_PATH: &[HarnessRouteCmd] = &[
    HarnessRouteCmd::MoveTo(0., 22.),
    HarnessRouteCmd::CurveTo(20., 22., 24., 18., 48., 18.),
    HarnessRouteCmd::CurveTo(88., 18., 96., 34., 144., 34.),
    HarnessRouteCmd::CurveTo(184., 34., 192., 10., 224., 10.),
    HarnessRouteCmd::CurveTo(252., 10., 256., 22., 280., 22.),
    HarnessRouteCmd::LineTo(320., 22.),
];

const HARNESS_MINIMAL_ROUTE: HarnessRouteSpec = HarnessRouteSpec {
    paths: &[HARNESS_MINIMAL_PATH],
    nodes: &[(160., 22.)],
};
const HARNESS_STANDARD_ROUTE: HarnessRouteSpec = HarnessRouteSpec {
    paths: &[HARNESS_STANDARD_PATH],
    nodes: &[(64., 22.), (128., 22.), (192., 22.), (256., 22.)],
};
const HARNESS_ULTIMATE_ROUTE: HarnessRouteSpec = HarnessRouteSpec {
    paths: &[HARNESS_ULTIMATE_PATH_MAIN, HARNESS_ULTIMATE_PATH_BRANCH],
    nodes: &[
        (48., 22.),
        (116., 7.),
        (174., 7.),
        (124., 37.),
        (192., 37.),
        (267., 22.),
    ],
};
const HARNESS_CREATIVE_ROUTE: HarnessRouteSpec = HarnessRouteSpec {
    paths: &[HARNESS_CREATIVE_PATH],
    nodes: &[(48., 18.), (144., 34.), (280., 22.)],
};

/// The route spec for one harness id; unknown ids fall back to Minimal so the
/// zone never renders a blank strip.
pub(crate) fn harness_route_spec(profile: &str) -> &'static HarnessRouteSpec {
    match profile {
        "Standard" => &HARNESS_STANDARD_ROUTE,
        "Ultimate" => &HARNESS_ULTIMATE_ROUTE,
        "Creative" => &HARNESS_CREATIVE_ROUTE,
        _ => &HARNESS_MINIMAL_ROUTE,
    }
}

/// G-A6: the harness id of a catalog row, or `None` for a regular mode.
/// Builtin source only — a user agent may reuse the id (web `primaryAgentKind`).
pub(crate) fn harness_profile_of(row: &AgentCardRow) -> Option<&'static str> {
    if row.source != AgentSourceKind::Builtin {
        return None;
    }
    HARNESS_IDS.iter().copied().find(|id| *id == row.id)
}

/// The icon asset path of one harness profile (web `HARNESS_PRESENTATION`).
fn harness_icon_path(profile: &str) -> &'static str {
    match profile {
        "Standard" => "harness/standard.svg",
        "Ultimate" => "harness/ultimate.svg",
        "Creative" => "harness/creative.svg",
        _ => "harness/minimal.svg",
    }
}

/// The web card's strategy route: the stretched path plus its node dots.
fn harness_route(profile: &'static str, cx: &App) -> gpui_kit::AnyElement {
    let spec = harness_route_spec(profile);
    let line = cx.theme().border;
    let dot = cx.theme().muted_foreground;
    let stroke: Hsla = line;
    let dot_color: Hsla = dot;
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _cx| {
            let sx = bounds.size.width / px(HARNESS_ROUTE_WIDTH);
            let sy = bounds.size.height / px(HARNESS_ROUTE_HEIGHT);
            let map = move |x: f32, y: f32| {
                point(bounds.origin.x + px(x) * sx, bounds.origin.y + px(y) * sy)
            };
            for commands in spec.paths {
                let mut builder = PathBuilder::stroke(px(1.));
                for command in *commands {
                    match *command {
                        HarnessRouteCmd::MoveTo(x, y) => builder.move_to(map(x, y)),
                        HarnessRouteCmd::LineTo(x, y) => builder.line_to(map(x, y)),
                        HarnessRouteCmd::CurveTo(a_x, a_y, b_x, b_y, x, y) => {
                            builder.cubic_bezier_to(map(x, y), map(a_x, a_y), map(b_x, b_y));
                        }
                    }
                }
                if let Ok(path) = builder.build() {
                    window.paint_path(path, stroke);
                }
            }
            for (x, y) in spec.nodes {
                let center = map(*x, *y);
                let radius = px(3.5);
                window.paint_quad(gpui_kit::PaintQuad {
                    bounds: Bounds::new(
                        point(center.x - radius, center.y - radius),
                        size(radius * 2., radius * 2.),
                    ),
                    corner_radii: Corners::all(radius),
                    background: Background::from(dot_color),
                    border_widths: Edges::default(),
                    border_color: gpui_kit::transparent_black(),
                    border_style: BorderStyle::default(),
                });
            }
        },
    )
    .h(px(HARNESS_ROUTE_HEIGHT))
    .w_full()
    .into_any_element()
}

// ---------------------------------------------------------------------------
// Skill catalog
// ---------------------------------------------------------------------------

/// One skill row on the skills page: the scanned skill plus its global
/// enablement. Per-mode selection lives in the agents page's skills dialog.
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
    /// Built-in catalog group (`group_key`); R4 facet entries.
    pub(crate) group_key: Option<String>,
    /// Discovery ecosystem (`source_id`); empty or `openbitfun*` = native.
    pub(crate) source_id: String,
    /// Display name of the discovery source (facet label).
    pub(crate) source_label: String,
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
/// only (their effective enablement state). `skills_mode_id` is the id the
/// per-mode skill overrides persist under (profile representative for modes,
/// own id for subagents — the web `representativeModeId` fallback chain).
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
    pub(crate) skills_manageable: bool,
    pub(crate) skills_mode_id: String,
    /// G-A3: the subagent's stored model override (`None` = the default
    /// SubAgent model). Always `None` for modes and external subagents.
    pub(crate) model_override:
        Option<openbitfun_core::service::config::types::SubagentModelSelection>,
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
// Per-mode skill selection (T2-11)
// ---------------------------------------------------------------------------

/// One skill row inside the per-mode skills dialog — the shell projection of
/// the web `ModeSkillInfo`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModeSkillRow {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) project_level: bool,
    pub(crate) builtin: bool,
    pub(crate) shadowed: bool,
    /// Label of the discovery source (winner reporting for shadowed rows).
    pub(crate) source_label: String,
    pub(crate) shadowed_by_key: Option<String>,
    /// Enabled after all overrides are applied — the checked-set baseline.
    pub(crate) effective_enabled: bool,
    /// Actually selected at runtime after disable + priority resolution.
    pub(crate) selected_for_runtime: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeSkillStatus {
    Selected,
    Covered,
    Enabled,
    Disabled,
}

impl ModeSkillRow {
    /// Web `getModeSkillRuntimeStatus`: disabled wins, then runtime selection,
    /// then shadowing; plain effective enablement is the remainder.
    pub(crate) fn status(&self) -> ModeSkillStatus {
        if !self.effective_enabled {
            ModeSkillStatus::Disabled
        } else if self.selected_for_runtime {
            ModeSkillStatus::Selected
        } else if self.shadowed {
            ModeSkillStatus::Covered
        } else {
            ModeSkillStatus::Enabled
        }
    }

    /// Source label of the skill that shadows this row, if any.
    pub(crate) fn covered_by<'a>(&self, rows: &'a [ModeSkillRow]) -> Option<&'a str> {
        let winner_key = self.shadowed_by_key.as_deref()?;
        rows.iter()
            .find(|row| row.key == winner_key)
            .map(|row| row.source_label.as_str())
            .filter(|label| !label.is_empty())
    }
}

/// Render phase of the per-mode skills dialog. `Loading` covers the initial
/// fetch; save/reset land back in `View` with refreshed rows the way the web
/// detail dialog leaves edit mode after a successful write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeSkillsPhase {
    Loading,
    View,
    Edit,
}

/// Dialog scratch for one agent's skill selection. `mode_id` is the id the
/// overrides persist under; `pending` holds the edited selection while in
/// `Edit` phase.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModeSkillsDialogState {
    pub(crate) mode_id: String,
    pub(crate) mode_name: String,
    pub(crate) phase: ModeSkillsPhase,
    pub(crate) rows: Vec<ModeSkillRow>,
    pub(crate) pending: Option<Vec<String>>,
    pub(crate) submitting: bool,
    pub(crate) error: Option<String>,
}

impl ModeSkillsDialogState {
    /// Web `getConfiguredEnabledSkillKeys`: the selection is the effective
    /// enablement, with `pending` overriding once the user edits a row.
    pub(crate) fn is_selected(&self, key: &str) -> bool {
        match &self.pending {
            Some(keys) => keys.iter().any(|entry| entry == key),
            None => self
                .rows
                .iter()
                .any(|row| row.key == key && row.effective_enabled),
        }
    }

    pub(crate) fn selection_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| self.is_selected(&row.key))
            .count()
    }

    /// Flips one skill in the pending selection, seeding `pending` from the
    /// effective enablement on the first edit.
    pub(crate) fn toggle(&mut self, key: &str) {
        let keys = self.pending.get_or_insert_with(|| {
            self.rows
                .iter()
                .filter(|row| row.effective_enabled)
                .map(|row| row.key.clone())
                .collect()
        });
        if let Some(position) = keys.iter().position(|entry| entry == key) {
            keys.remove(position);
        } else {
            keys.push(key.to_string());
        }
    }
}

/// First-mode-wins representative per config profile (web `buildProfileMap`):
/// modes sharing a profile persist their overrides under the first mode in
/// list order; modes without a profile map to themselves.
pub(crate) fn representative_mode_ids(
    modes: &[(String, Option<String>)],
) -> std::collections::HashMap<String, String> {
    let mut first_by_profile: std::collections::HashMap<String, String> = Default::default();
    for (id, profile) in modes {
        let profile_id = profile.clone().unwrap_or_else(|| id.clone());
        first_by_profile
            .entry(profile_id)
            .or_insert_with(|| id.clone());
    }
    modes
        .iter()
        .map(|(id, profile)| {
            let profile_id = profile.clone().unwrap_or_else(|| id.clone());
            let representative = first_by_profile
                .get(&profile_id)
                .cloned()
                .unwrap_or_else(|| id.clone());
            (id.clone(), representative)
        })
        .collect()
}

/// Trims, drops empties, and deduplicates while preserving order (desktop
/// `normalize_skill_key_list`).
pub(crate) fn normalize_skill_key_list(keys: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut normalized = Vec::new();
    for key in keys {
        let trimmed = key.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            normalized.push(trimmed);
        }
    }
    normalized
}

/// Selection keys unknown to the scan (desktop `replace_mode_skill_selection`
/// rejects the whole batch when any key is unknown).
pub(crate) fn unknown_skill_keys(
    skills: &[openbitfun_core::agentic::tools::implementations::skills::SkillInfo],
    enabled_keys: &[String],
) -> Vec<String> {
    let known: std::collections::HashSet<&String> = skills.iter().map(|skill| &skill.key).collect();
    enabled_keys
        .iter()
        .filter(|key| !known.contains(*key))
        .cloned()
        .collect()
}

/// Desktop `persist_user_mode_skill_selection` diff: user skills whose
/// default-enabled state disagrees with the selection become the persisted
/// override sets (disable the defaults that are off, enable the rest).
pub(crate) fn user_mode_skill_override_diff(
    skills: &[openbitfun_core::agentic::tools::implementations::skills::SkillInfo],
    mode_id: &str,
    enabled_keys: &std::collections::HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    use openbitfun_core::agentic::tools::implementations::skills::resolver::resolve_skill_default_enabled_for_mode;
    let mut disabled_user_skills = Vec::new();
    let mut enabled_user_skills = Vec::new();
    for skill in skills.iter().filter(|skill| {
        skill.level == openbitfun_core::agentic::tools::implementations::skills::SkillLocation::User
    }) {
        let should_enable = enabled_keys.contains(&skill.key);
        let default_enabled = resolve_skill_default_enabled_for_mode(skill, mode_id);
        if default_enabled && !should_enable {
            disabled_user_skills.push(skill.key.clone());
        } else if !default_enabled && should_enable {
            enabled_user_skills.push(skill.key.clone());
        }
    }
    (disabled_user_skills, enabled_user_skills)
}

/// Project skills left out of the selection become the mode's project disable
/// set (desktop `build_disabled_project_skill_keys`).
pub(crate) fn disabled_project_skill_keys(
    skills: &[openbitfun_core::agentic::tools::implementations::skills::SkillInfo],
    enabled_keys: &std::collections::HashSet<String>,
) -> Vec<String> {
    skills
        .iter()
        .filter(|skill| {
            skill.level
                == openbitfun_core::agentic::tools::implementations::skills::SkillLocation::Project
        })
        .filter(|skill| !enabled_keys.contains(&skill.key))
        .map(|skill| skill.key.clone())
        .collect()
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

/// The prefilled response JSON for one interaction method, mirroring the web
/// dialog: sampling expects a message skeleton, everything else (elicitation)
/// an accept action.
pub(crate) fn default_interaction_result(method: &str) -> String {
    if method == "sampling/createMessage" {
        r#"{
  "role": "assistant",
  "content": [
    {
      "type": "text",
      "text": ""
    }
  ]
}"#
        .to_string()
    } else {
        "{\n  \"action\": \"accept\"\n}".to_string()
    }
}

/// Raw JSON config snapshot the editor dialog edits (the Cursor-format
/// `app.json` the desktop and CLI hosts read and write).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpJsonSnapshot {
    pub(crate) json_config: String,
    pub(crate) fingerprint: String,
}

/// One pending or finished resource read in the browse dialog; the uri keys
/// the window state so a late reply cannot land on a newer selection.
#[derive(Debug, Clone)]
pub(crate) struct McpResourceView {
    pub(crate) uri: String,
    pub(crate) loading: bool,
    pub(crate) result:
        Option<Result<openbitfun_core::service::mcp::protocol::ResourcesReadResult, String>>,
}

/// The outcome of one resources/read call, routed back to the browse dialog.
#[derive(Debug)]
pub(crate) struct McpResourceReadOutcome {
    pub(crate) uri: String,
    pub(crate) result: Result<openbitfun_core::service::mcp::protocol::ResourcesReadResult, String>,
}

/// Flattens a resources/read result into display text, following the web
/// resource browser: one block per content entry separated by `---`, with
/// explicit notes for binary and empty payloads.
pub(crate) fn render_resource_content(
    result: &openbitfun_core::service::mcp::protocol::ResourcesReadResult,
    text: &ShellText,
) -> String {
    if result.contents.is_empty() {
        return text.manage_mcp_resource_empty.clone();
    }
    let blocks: Vec<String> = result
        .contents
        .iter()
        .enumerate()
        .map(|(index, content)| {
            let header = if result.contents.len() > 1 {
                let mut header = format!("#{} {}", index + 1, content.uri);
                if let Some(mime) = &content.mime_type {
                    header.push_str(&format!(" ({mime})"));
                }
                header
            } else {
                content
                    .mime_type
                    .as_ref()
                    .map(|mime| format!("[{mime}]"))
                    .unwrap_or_default()
            };
            let body = if content
                .content
                .as_ref()
                .is_some_and(|body| !body.is_empty())
            {
                content.content.clone().unwrap_or_default()
            } else if content.blob.is_some() {
                text.manage_mcp_resource_binary.clone()
            } else {
                text.manage_mcp_resource_failed.clone()
            };
            if header.is_empty() {
                body
            } else {
                format!("{header}\n\n{body}")
            }
        })
        .collect();
    blocks.join("\n\n---\n\n")
}

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

/// T2-12: one custom-agent edit request from the dialog. The id, kind, and
/// level are fixed once created (web edit form keeps them read-only); tools,
/// model, review, and the context policy pass `None` through the core update
/// so the existing definition is preserved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentEditDraft {
    pub(crate) agent_id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) prompt: String,
}

/// Shell projection of the core `CustomAgentDetail` for the edit dialog
/// prefill (the shell only edits name/description/prompt).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentDetailRow {
    pub(crate) agent_id: String,
    pub(crate) mode_kind: bool,
    pub(crate) project_level: bool,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) prompt: String,
}

/// Projects the core detail strings into the dialog's boolean shape
/// (`kind` = "mode"/"subagent", `level` = "user"/"project").
pub(crate) fn project_agent_detail(
    detail: &openbitfun_core::agentic::agents::CustomAgentDetail,
) -> AgentDetailRow {
    AgentDetailRow {
        agent_id: detail.agent_id.clone(),
        mode_kind: detail.kind == "mode",
        project_level: detail.level == "project",
        name: detail.name.clone(),
        description: detail.description.clone(),
        prompt: detail.prompt.clone(),
    }
}

/// T2-12: edit/delete are card actions for custom agents only — the web
/// `canManageCustomAgent` guard (`source` neither builtin nor external).
pub(crate) fn agent_card_manageable(source: AgentSourceKind) -> bool {
    matches!(source, AgentSourceKind::User | AgentSourceKind::Project)
}

/// Delete-confirmation scratch: the card target (id + display name).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentPendingDelete {
    pub(crate) agent_id: String,
    pub(crate) agent_name: String,
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

/// R4 facet selection for the skills page. A global because the settings
/// render closures only see `cx` (same reason as [`ManageNotice`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum SkillFacet {
    #[default]
    All,
    Builtin,
    User,
    Project,
    /// A built-in catalog group (`group_key` value).
    Group(String),
    /// A non-native discovery ecosystem (`source_id` value).
    Source(String),
    /// The skill market placeholder (T3-17): an explicit degrade, not a fake.
    Market,
}

#[derive(Debug, Default)]
pub(crate) struct SkillsFacetState(pub SkillFacet);

impl Global for SkillsFacetState {}

/// Counts and facet entries derived from the scanned rows. Pure so the facet
/// derivation stays testable against the registry scan shape.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SkillFacets {
    pub all: usize,
    pub builtin: usize,
    pub user: usize,
    pub project: usize,
    /// `(group_key, count)` over built-in skills, ordered by key. User-defined
    /// groups are a web-side configuration the shell does not manage yet, so
    /// they are absent here rather than faked.
    pub groups: Vec<(String, usize)>,
    /// `(source_id, label, count)` over non-native sources, ordered by label.
    pub sources: Vec<(String, String, usize)>,
}

/// Mirrors the web `installedSkillGroup` split: the native OpenBitFun
/// ecosystem folds into the level facets, everything else becomes a source.
pub(crate) fn is_native_source(source_id: &str) -> bool {
    matches!(
        source_id,
        "" | "openbitfun" | "openbitfun-system" | "openbitfun-user"
    )
}

pub(crate) fn skill_facets(rows: &[SkillRow]) -> SkillFacets {
    let mut facets = SkillFacets {
        all: rows.len(),
        ..Default::default()
    };
    let mut groups: std::collections::BTreeMap<String, usize> = Default::default();
    let mut sources: std::collections::BTreeMap<String, (String, usize)> = Default::default();
    for row in rows {
        if row.builtin {
            facets.builtin += 1;
            if let Some(group) = row
                .group_key
                .as_deref()
                .map(str::trim)
                .filter(|k| !k.is_empty())
            {
                *groups.entry(group.to_string()).or_default() += 1;
            }
            continue;
        }
        if row.project_level {
            facets.project += 1;
        } else {
            facets.user += 1;
        }
        if !is_native_source(&row.source_id) {
            let label = if row.source_label.trim().is_empty() {
                row.source_id.clone()
            } else {
                row.source_label.clone()
            };
            match sources.entry(row.source_id.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((label, 1));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().1 += 1;
                }
            }
        }
    }
    facets.groups = groups.into_iter().collect();
    facets.sources = sources
        .into_iter()
        .map(|(id, (label, count))| (id, label, count))
        .collect();
    facets
}

/// Whether one row belongs under the selected facet. Group and source facets
/// match on their id; a row shadowed out of the scan still shows under its
/// facet (the shadowed badge explains why it will not run).
pub(crate) fn skill_matches(row: &SkillRow, facet: &SkillFacet) -> bool {
    match facet {
        SkillFacet::All => true,
        SkillFacet::Builtin => row.builtin,
        SkillFacet::User => !row.builtin && !row.project_level,
        SkillFacet::Project => !row.builtin && row.project_level,
        SkillFacet::Group(group) => row.builtin && row.group_key.as_deref() == Some(group.as_str()),
        SkillFacet::Source(source) => {
            !row.builtin && !is_native_source(&row.source_id) && &row.source_id == source
        }
        SkillFacet::Market => false,
    }
}

// ---------------------------------------------------------------------------
// Page-local filters (G-K5 skills search + G-A5 agents search)
// ---------------------------------------------------------------------------

/// G-K5: the skills page's local filter row — the search box plus the web's
/// "Hide duplicates" toggle for shadowed rows. The page renders from globals,
/// so the query the input currently holds is mirrored here on every keystroke
/// (nothing else would repaint the list).
#[derive(Debug, Default)]
pub(crate) struct SkillsFilterState {
    pub(crate) query: String,
    pub(crate) hide_shadowed: bool,
    /// The lazily created search box; `None` until the window installs it.
    pub(crate) input: Option<gpui_kit::Entity<InputState>>,
}

impl Global for SkillsFilterState {}

/// G-A5: the agents page's search plus its source and type filters (web
/// `filters.source` / `filters.kind`).
#[derive(Debug, Default)]
pub(crate) struct AgentsFilterState {
    pub(crate) query: String,
    pub(crate) source: AgentSourceFilter,
    pub(crate) kind: AgentKindFilter,
    pub(crate) input: Option<gpui_kit::Entity<InputState>>,
}

impl Global for AgentsFilterState {}

/// Web `filters.source` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentSourceFilter {
    #[default]
    All,
    Builtin,
    User,
    Project,
    External,
}

impl AgentSourceFilter {
    pub(crate) fn matches(self, source: AgentSourceKind) -> bool {
        match self {
            Self::All => true,
            Self::Builtin => source == AgentSourceKind::Builtin,
            Self::User => source == AgentSourceKind::User,
            Self::Project => source == AgentSourceKind::Project,
            Self::External => source == AgentSourceKind::External,
        }
    }
}

/// Web `filters.kind` options: which of the page's two lists shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AgentKindFilter {
    #[default]
    All,
    Agent,
    SubAgent,
}

impl AgentKindFilter {
    pub(crate) fn matches(self, kind: AgentListKind) -> bool {
        match self {
            Self::All => true,
            Self::Agent => matches!(kind, AgentListKind::Modes),
            Self::SubAgent => matches!(kind, AgentListKind::Subagents),
        }
    }
}

/// G-K5: whether a skill row survives the local filter. The web matches name,
/// description and path; the shell row carries the skill key — the
/// path-derived identity the scan gives it — so the key stands in for the
/// path the row does not keep.
pub(crate) fn skill_matches_filter(row: &SkillRow, query: &str, hide_shadowed: bool) -> bool {
    if hide_shadowed && row.shadowed {
        return false;
    }
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    [
        row.name.as_str(),
        row.description.as_str(),
        row.key.as_str(),
    ]
    .iter()
    .any(|value| value.to_lowercase().contains(&query))
}

/// G-A5: whether an agent card survives the search and source filter (the
/// kind filter chooses the list, not the row). The web matches the name and
/// description only.
pub(crate) fn agent_matches_filter(
    card: &AgentCardRow,
    query: &str,
    source: AgentSourceFilter,
) -> bool {
    if !source.matches(card.source) {
        return false;
    }
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    [card.name.as_str(), card.description.as_str()]
        .iter()
        .any(|value| value.to_lowercase().contains(&query))
}

/// G-K5/G-A5: creates the two page-local search boxes, installs their filter
/// globals, and mirrors every keystroke into them. A keystroke has to repaint
/// the window that renders the list, which nothing else would notify — the
/// same subscription the settings nav search uses.
pub(crate) fn install_page_filters(window: &mut Window, cx: &mut Context<ShellWindow>) {
    let text = i18n::current(cx);
    let skills_input = cx.new(|cx| {
        InputState::new(window, cx).placeholder(text.manage_skills_search_placeholder.clone())
    });
    let agents_input = cx.new(|cx| {
        InputState::new(window, cx).placeholder(text.manage_agents_search_placeholder.clone())
    });
    let skills_placeholder = skills_input.clone();
    cx.set_global(SkillsFilterState {
        query: String::new(),
        hide_shadowed: false,
        input: Some(skills_placeholder),
    });
    let agents_placeholder = agents_input.clone();
    cx.set_global(AgentsFilterState {
        query: String::new(),
        source: AgentSourceFilter::All,
        kind: AgentKindFilter::All,
        input: Some(agents_placeholder),
    });
    cx.subscribe(
        &skills_input,
        |_: &mut ShellWindow, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = input.read(cx).value().trim().to_string();
                cx.update_global::<SkillsFilterState, _>(|state, _| state.query = query);
                cx.notify();
            }
        },
    )
    .detach();
    cx.subscribe(
        &agents_input,
        |_: &mut ShellWindow, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = input.read(cx).value().trim().to_string();
                cx.update_global::<AgentsFilterState, _>(|state, _| state.query = query);
                cx.notify();
            }
        },
    )
    .detach();
}

/// The skills page's filter row (G-K5): the search box and the shadowed-row
/// toggle, above the list the way the web toolbar sits.
fn skills_filter_row(text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let state = cx.try_global::<SkillsFilterState>();
    let input = state.and_then(|state| state.input.clone());
    let hide_shadowed = state.map(|state| state.hide_shadowed).unwrap_or(false);
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .pb_1();
    if let Some(input) = input {
        row = row.child(div().w(px(220.)).child(Input::new(&input).small()));
    }
    row = row.child(
        Checkbox::new("skills-hide-duplicates")
            .label(text.manage_skills_hide_shadowed.clone())
            .checked(hide_shadowed)
            .on_click(move |checked: &bool, _window, cx| {
                let checked = *checked;
                cx.update_global::<SkillsFilterState, _>(|state, _| state.hide_shadowed = checked);
                cx.refresh_windows();
            }),
    );
    row.into_any_element()
}

/// The agents page's control row (G-A5): search plus the source and type
/// menus, rendered once above both lists.
fn agents_controls(text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let state = cx.try_global::<AgentsFilterState>();
    let input = state.and_then(|state| state.input.clone());
    let source = state.map(|state| state.source).unwrap_or_default();
    let kind = state.map(|state| state.kind).unwrap_or_default();
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .w_full()
        .min_w_0()
        .pb_1();
    if let Some(input) = input {
        row = row.child(
            div()
                .flex_1()
                .min_w(px(220.))
                .child(Input::new(&input).small()),
        );
    }
    row = row
        .child(agents_source_menu(text, source))
        .child(agents_kind_menu(text, kind));
    row.into_any_element()
}

/// The source filter menu (web `filters.source`).
fn agents_source_menu(text: &ShellText, selected: AgentSourceFilter) -> gpui_kit::AnyElement {
    let options = [
        (
            AgentSourceFilter::All,
            text.manage_agents_source_any.clone(),
        ),
        (
            AgentSourceFilter::Builtin,
            text.manage_agent_source_builtin.clone(),
        ),
        (
            AgentSourceFilter::User,
            text.manage_agent_source_user.clone(),
        ),
        (
            AgentSourceFilter::Project,
            text.manage_agent_source_project.clone(),
        ),
        (
            AgentSourceFilter::External,
            text.manage_agent_source_external.clone(),
        ),
    ];
    let current = options
        .iter()
        .find(|(value, _)| *value == selected)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    Button::new("agents-source-filter")
        .label(format!("{}: {current}", text.manage_agents_source_label))
        .small()
        .with_variant(ButtonVariant::Secondary)
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
            let mut popup = menu;
            for (value, label) in options.clone() {
                popup = popup.item(
                    PopupMenuItem::new(label)
                        .checked(value == selected)
                        .on_click(move |_: &gpui_kit::ClickEvent, _window, cx| {
                            cx.update_global::<AgentsFilterState, _>(|state, _| {
                                state.source = value
                            });
                            cx.refresh_windows();
                        }),
                );
            }
            popup
        })
        .into_any_element()
}

/// The type filter menu (web `filters.kind`).
fn agents_kind_menu(text: &ShellText, selected: AgentKindFilter) -> gpui_kit::AnyElement {
    let options = [
        (AgentKindFilter::All, text.manage_agents_kind_any.clone()),
        (
            AgentKindFilter::Agent,
            text.manage_agents_kind_agent.clone(),
        ),
        (
            AgentKindFilter::SubAgent,
            text.manage_agents_kind_subagent.clone(),
        ),
    ];
    let current = options
        .iter()
        .find(|(value, _)| *value == selected)
        .map(|(_, label)| label.clone())
        .unwrap_or_default();
    Button::new("agents-kind-filter")
        .label(format!("{}: {current}", text.manage_agents_kind_label))
        .small()
        .with_variant(ButtonVariant::Secondary)
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
            let mut popup = menu;
            for (value, label) in options.clone() {
                popup = popup.item(
                    PopupMenuItem::new(label)
                        .checked(value == selected)
                        .on_click(move |_: &gpui_kit::ClickEvent, _window, cx| {
                            // The type filter drops a whole section, so the
                            // page has to re-render from the top.
                            cx.update_global::<AgentsFilterState, _>(|state, _| state.kind = value);
                            cx.refresh_windows();
                        }),
                );
            }
            popup
        })
        .into_any_element()
}

/// The skills page: a facet sidebar (library / groups / sources / explore)
/// beside the filtered skill list. The market entry renders an explicit
/// degrade card (T3-17) instead of a dead-end.
pub(crate) fn skills_group(text: &ShellText) -> SettingGroup {
    let page_text = text.clone();
    let page_field = SettingItem::render(move |_options, _window, cx| {
        skills_page(&page_text, cx).into_any_element()
    });

    SettingGroup::new().items(vec![page_field])
}

fn facet_row(
    id: String,
    label: String,
    count: Option<usize>,
    selected: bool,
    on_click: impl Fn(&mut App) + 'static,
    cx: &App,
) -> gpui_kit::AnyElement {
    // Selected style mirrors the settings sidebar row and the web skills
    // filters: the neutral sidebar selection surface with primary text (R5
    // parity; the accent solidVariant lost the side-by-side against desktop).
    let (label_color, count_color, weight) = if selected {
        (
            cx.theme().sidebar_accent_foreground,
            cx.theme().sidebar_accent_foreground,
            Some(FontWeight::MEDIUM),
        )
    } else {
        (cx.theme().foreground, cx.theme().muted_foreground, None)
    };
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_2()
        .py_1()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .when(selected, |this| {
            this.bg(cx.theme().sidebar_accent)
                .when_some(weight, |row, w| row.font_weight(w))
        })
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .on_click(move |_, _, cx| on_click(cx))
        .child(
            div()
                .text_sm()
                .text_color(label_color)
                .min_w_0()
                .truncate()
                .child(label),
        )
        .when_some(count, |row, count| {
            row.child(
                div()
                    .text_xs()
                    .text_color(count_color)
                    .flex_shrink_0()
                    .child(count.to_string()),
            )
        })
        .into_any_element()
}

fn facet_group_title(label: String, cx: &App) -> gpui_kit::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .px_2()
        .pt_1()
        .child(label)
}

fn skills_page(text: &ShellText, cx: &mut App) -> gpui_kit::Div {
    if let Some(message) = ManageNotice::current(cx) {
        let mut column = div().w_full();
        column = column.child(notice_banner(text, &message, cx));
        return column;
    }
    let Some(catalog) = SkillCatalog::try_current(cx) else {
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.manage_skills_loading.clone());
    };

    let facet = cx
        .try_global::<SkillsFacetState>()
        .map(|state| state.0.clone())
        .unwrap_or_default();
    let facets = skill_facets(&catalog.skills);

    // The facet rail: three fixed groups (library / explore) plus the
    // derived group and source sections when the scan produced any.
    let mut rail = div()
        .v_flex()
        .gap_0p5()
        .w(px(200.))
        .flex_shrink_0()
        .min_w_0();
    let set = |facet: SkillFacet| {
        move |cx: &mut App| {
            cx.set_global(SkillsFacetState(facet.clone()));
            cx.refresh_windows();
        }
    };
    rail = rail.child(facet_group_title(text.skills_facet_library.clone(), cx));
    for (candidate, label, count) in [
        (SkillFacet::All, text.skills_facet_all.clone(), facets.all),
        (
            SkillFacet::Builtin,
            text.manage_skill_builtin.clone(),
            facets.builtin,
        ),
        (
            SkillFacet::User,
            text.manage_skill_user.clone(),
            facets.user,
        ),
        (
            SkillFacet::Project,
            text.manage_skill_project.clone(),
            facets.project,
        ),
    ] {
        let selected = facet == candidate;
        rail = rail.child(facet_row(
            format!("skills-facet-{}", facet_key(&candidate)),
            label,
            Some(count),
            selected,
            set(candidate),
            cx,
        ));
    }
    if !facets.groups.is_empty() {
        rail = rail.child(facet_group_title(text.skills_facet_groups.clone(), cx));
        for (group, count) in &facets.groups {
            let candidate = SkillFacet::Group(group.clone());
            rail = rail.child(facet_row(
                format!("skills-facet-group-{}", group),
                group.clone(),
                Some(*count),
                facet == candidate,
                set(candidate),
                cx,
            ));
        }
    }
    if !facets.sources.is_empty() {
        rail = rail.child(facet_group_title(text.skills_facet_sources.clone(), cx));
        for (id, label, count) in &facets.sources {
            let candidate = SkillFacet::Source(id.clone());
            rail = rail.child(facet_row(
                format!("skills-facet-source-{}", id),
                label.clone(),
                Some(*count),
                facet == candidate,
                set(candidate),
                cx,
            ));
        }
    }
    rail = rail.child(facet_group_title(text.skills_facet_explore.clone(), cx));
    rail = rail.child(facet_row(
        "skills-facet-market".to_string(),
        text.skills_market_title.clone(),
        None,
        facet == SkillFacet::Market,
        set(SkillFacet::Market),
        cx,
    ));

    let mut content = div().v_flex().gap_2().flex_1().min_w_0();
    if facet == SkillFacet::Market {
        content = content.child(crate::shell::brand_empty(
            IconName::Globe,
            &text.skills_market_title,
            &text.skills_market_degraded,
        ));
        return div()
            .flex()
            .gap_3()
            .w_full()
            .min_w_0()
            .child(rail)
            .child(content);
    }

    // G-K5: the local filter row sits above the list, so the facet rail and
    // the search box narrow the same rows.
    content = content.child(skills_filter_row(text, cx));
    let filter = cx
        .try_global::<SkillsFilterState>()
        .map(|state| (state.query.clone(), state.hide_shadowed))
        .unwrap_or_default();
    let filtered = !filter.0.trim().is_empty() || filter.1;
    let rows: Vec<&SkillRow> = catalog
        .skills
        .iter()
        .filter(|row| skill_matches(row, &facet))
        .filter(|row| skill_matches_filter(row, &filter.0, filter.1))
        .collect();
    if rows.is_empty() {
        let (title, body) = if filtered {
            (text.manage_skills_no_match.clone(), String::new())
        } else {
            (
                text.manage_skills_empty_title.clone(),
                text.manage_skills_empty.clone(),
            )
        };
        content = content.child(crate::shell::brand_empty(IconName::BookOpen, &title, &body));
        return div()
            .flex()
            .gap_3()
            .w_full()
            .min_w_0()
            .child(rail)
            .child(content);
    }

    for skill in rows {
        content = content.child(render_skill_card(skill, text, cx));
    }
    div()
        .flex()
        .gap_3()
        .w_full()
        .min_w_0()
        .child(rail)
        .child(content)
}

/// Stable element id fragment per facet (source ids are ecosystem slugs).
fn facet_key(facet: &SkillFacet) -> String {
    match facet {
        SkillFacet::All => "all".to_string(),
        SkillFacet::Builtin => "builtin".to_string(),
        SkillFacet::User => "user".to_string(),
        SkillFacet::Project => "project".to_string(),
        SkillFacet::Group(group) => format!("group-{}", group),
        SkillFacet::Source(source) => format!("source-{}", source),
        SkillFacet::Market => "market".to_string(),
    }
}

/// One skill card on the right side: name, level/source/shadowed badges,
/// description, and the global enablement switch (built-ins stay on).
fn render_skill_card(row: &SkillRow, text: &ShellText, cx: &App) -> gpui_kit::Div {
    let level_label = if row.builtin {
        text.manage_skill_builtin.clone()
    } else if row.project_level {
        text.manage_skill_project.clone()
    } else {
        text.manage_skill_user.clone()
    };
    let card = div()
        .v_flex()
        .gap_1()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .flex_1()
        .min_w_0()
        .debug_selector(|| "skills-card".to_string())
        .child(
            div().flex().items_center().justify_between().gap_2().child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .min_w_0()
                            .truncate()
                            .child(row.name.clone()),
                    )
                    .child(badge(level_label, cx))
                    .when(!is_native_source(&row.source_id), |card| {
                        let label = if row.source_label.trim().is_empty() {
                            row.source_id.clone()
                        } else {
                            row.source_label.clone()
                        };
                        card.child(badge(label, cx))
                    })
                    .when(row.shadowed, |card| {
                        card.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .flex_shrink_0()
                                .child(text.manage_skill_shadowed.clone()),
                        )
                    }),
            ),
        )
        .when(!row.description.is_empty(), |card| {
            card.child(
                // min_w_0 + line_clamp over a bare truncate: the wrappable
                // line keeps min-content small so the card's shrink chain
                // constrains the text (same recipe as the agent cards).
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .min_w_0()
                    .whitespace_normal()
                    .line_clamp(2)
                    .debug_selector(|| "skills-card-desc".to_string())
                    .child(row.description.clone()),
            )
        });
    if row.builtin {
        card
    } else {
        let key = row.key.clone();
        let user_level = !row.project_level;
        card.child(
            div().flex().justify_end().child(
                Switch::new(SharedString::from(format!("skill-toggle-{}", row.key)))
                    .checked(!row.disabled)
                    .on_click(move |checked: &bool, _window, _cx| {
                        set_skill_disabled(key.clone(), user_level, !*checked);
                    })
                    .flex_shrink_0(),
            ),
        )
    }
}

fn badge(label: String, cx: &App) -> gpui_kit::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .bg(cx.theme().secondary)
        .px_1p5()
        .py_0p5()
        .rounded(cx.theme().radius)
        .flex_shrink_0()
        .child(label)
}

fn set_skill_disabled(key: String, user_level: bool, disabled: bool) {
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::error!("Runtime unavailable while toggling skill {key}");
        return;
    };
    handle.set_skill_disabled(key, user_level, disabled);
}

/// The agents page: mode cards, subagent cards, and the create dialog entry.
pub(crate) fn agents_group(
    text: &ShellText,
    view: WeakEntity<ShellWindow>,
    cx: &App,
) -> SettingGroup {
    let modes_text = text.clone();
    let modes_view = view.clone();
    let modes_field = SettingField::render(move |_options, _window, cx| {
        agent_cards(AgentListKind::Modes, &modes_text, modes_view.clone(), cx)
    });
    // G-A6: the harness profile zone (cloned here — `view` moves into the
    // create field below).
    let harness_text = text.clone();
    let harness_view = view.clone();
    let subagents_text = text.clone();
    let subagents_view = view.clone();
    let subagents_field = SettingField::render(move |_options, _window, cx| {
        agent_cards(
            AgentListKind::Subagents,
            &subagents_text,
            subagents_view.clone(),
            cx,
        )
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

    // G-A5: the search / source / type controls sit above both lists, and the
    // type filter drops the section it excludes — the same shape the web
    // gallery takes when its kind filter is set.
    let controls_text = text.clone();
    let controls_item =
        SettingItem::render(move |_options, _window, cx| agents_controls(&controls_text, cx));
    let kind = cx
        .try_global::<AgentsFilterState>()
        .map(|state| state.kind)
        .unwrap_or_default();

    let mut items = vec![controls_item];
    // G-A6: the harness profile zone leads the page (web renders
    // `AgentHarnessOverview` above the catalog zone, outside its filters).
    items.push(SettingItem::render(move |_options, _window, cx| {
        harness_zone(&harness_text, harness_view.clone(), cx)
    }));
    if kind.matches(AgentListKind::Modes) {
        items.push(SettingItem::new(
            text.manage_agents_modes.clone(),
            modes_field,
        ));
    }
    if kind.matches(AgentListKind::Subagents) {
        items.push(SettingItem::new(
            text.manage_agents_subagents.clone(),
            subagents_field,
        ));
    }
    items.push(SettingItem::new(
        text.manage_agents_create.clone(),
        create_field,
    ));
    SettingGroup::new().items(items)
}

/// Which of the agents page's two lists a field renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentListKind {
    Modes,
    Subagents,
}

// ---------------------------------------------------------------------------
// G-A3: subagent model override
// ---------------------------------------------------------------------------

/// The label of the menu entry that clears the override (web
/// `DEFAULT_SUBAGENT_MODEL_OVERRIDE_VALUE`); only used to name the row, the
/// stored value is the absence of the override.
pub(crate) const AGENT_MODEL_DEFAULT_CHOICE: &str = "__default_subagent_model__";

/// G-A3: the trigger label of one stored override (web
/// `subagentModelDisplayName`): the default entry, the two aliases, parent
/// inheritance, or the configured model's display name.
pub(crate) fn agent_model_label(
    model_override: Option<&openbitfun_core::service::config::types::SubagentModelSelection>,
    models: &[crate::runtime::ModelChoice],
    text: &ShellText,
) -> String {
    use openbitfun_core::service::config::types::SubagentModelSelection;
    match model_override {
        None => text.manage_agent_model_default.clone(),
        Some(SubagentModelSelection::Inherit) => text.settings_subagent_model_inherit.clone(),
        Some(SubagentModelSelection::Fixed { model_id }) => match model_id.as_str() {
            "fast" => text.settings_subagent_model_fast.clone(),
            "primary" => text.settings_subagent_model_primary.clone(),
            id => models
                .iter()
                .find(|model| model.id == id)
                .map(crate::runtime::ModelChoice::label)
                .unwrap_or_else(|| id.to_string()),
        },
    }
}

/// G-A3: the menu choices in web order — default, inherit, fast, primary,
/// then every enabled configured model. The value is the choice string the
/// runtime write path stores; [`AGENT_MODEL_DEFAULT_CHOICE`] means "clear".
/// Web additionally filters to `text_chat` capable models; the shell catalog
/// carries no capabilities (same recorded gap as G-I1).
pub(crate) fn agent_model_options(
    models: &[crate::runtime::ModelChoice],
    text: &ShellText,
) -> Vec<(String, String)> {
    let mut options = vec![
        (
            AGENT_MODEL_DEFAULT_CHOICE.to_string(),
            text.manage_agent_model_default.clone(),
        ),
        (
            "inherit".to_string(),
            text.settings_subagent_model_inherit.clone(),
        ),
        (
            "fast".to_string(),
            text.settings_subagent_model_fast.clone(),
        ),
        (
            "primary".to_string(),
            text.settings_subagent_model_primary.clone(),
        ),
    ];
    for model in models.iter().filter(|model| model.enabled) {
        options.push((model.id.clone(), model.label()));
    }
    options
}

/// G-A3: one subagent row's model menu (web `agentCard.modelSelector`, which
/// the web renders on the detail dialog's model tab; the shell has no detail
/// dialog, so the control sits on the row).
fn agent_model_menu(row: &AgentCardRow, text: &ShellText, cx: &App) -> gpui_kit::AnyElement {
    let models = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .map(|catalog| catalog.models.clone())
        .unwrap_or_default();
    let current_choice = row.model_override.as_ref().map(|selection| {
        use openbitfun_core::service::config::types::SubagentModelSelection;
        match selection {
            SubagentModelSelection::Inherit => "inherit".to_string(),
            SubagentModelSelection::Fixed { model_id } => model_id.clone(),
        }
    });
    let current_label = agent_model_label(row.model_override.as_ref(), &models, text);
    let options = agent_model_options(&models, text);
    let builtin = row.source == AgentSourceKind::Builtin;
    let row_id = row.id.clone();
    Button::new(SharedString::from(format!("agent-model-{}", row.id)))
        .label(format!("{}: {current_label}", text.manage_agent_model))
        .small()
        .with_variant(ButtonVariant::Secondary)
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
            let mut popup = menu;
            for (value, label) in options.clone() {
                let selected = if value == AGENT_MODEL_DEFAULT_CHOICE {
                    current_choice.is_none()
                } else {
                    current_choice.as_deref() == Some(value.as_str())
                };
                let row_id = row_id.clone();
                popup = popup.item(PopupMenuItem::new(label).checked(selected).on_click(
                    move |_: &gpui_kit::ClickEvent, _window, _cx| {
                        let choice = if value == AGENT_MODEL_DEFAULT_CHOICE {
                            None
                        } else {
                            Some(value.clone())
                        };
                        let Some(handle) = crate::runtime::runtime_handle() else {
                            tracing::error!("Runtime unavailable while setting a subagent model");
                            return;
                        };
                        handle.set_subagent_model(row_id.clone(), builtin, choice);
                    },
                ));
            }
            popup
        })
        .into_any_element()
}

/// G-A6: the harness zone heading plus one card per builtin harness profile.
/// Web opens the agent detail dialog from a profile card; the shell has no
/// detail dialog, so a card keeps the shell's row action (the skills dialog)
/// and stays display-only otherwise.
fn harness_zone(
    text: &ShellText,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
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
    let heading = div()
        .debug_selector(|| "manage-harness-heading".to_string())
        .v_flex()
        .w_full()
        .gap_0p5()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(text.manage_harness_title.clone()),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text.manage_harness_subtitle.clone()),
        );
    let mut grid = div()
        .grid()
        .grid_cols(HARNESS_IDS.len() as u16)
        .gap_2()
        .w_full();
    for profile in HARNESS_IDS {
        let row = catalog
            .modes
            .iter()
            .find(|row| harness_profile_of(row) == Some(profile));
        grid = grid.child(harness_card(profile, row, text, view.clone(), cx));
    }
    div()
        .debug_selector(|| "manage-harness-zone".to_string())
        .v_flex()
        .w_full()
        .gap_3()
        .child(heading)
        .child(grid)
        .into_any_element()
}

/// G-A6: one harness profile card — glyph, localized name, purpose, and the
/// strategy route strip.
fn harness_card(
    profile: &'static str,
    row: Option<&AgentCardRow>,
    text: &ShellText,
    view: WeakEntity<ShellWindow>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let purpose = match profile {
        "Standard" => text.manage_harness_purpose_standard.clone(),
        "Ultimate" => text.manage_harness_purpose_ultimate.clone(),
        "Creative" => text.manage_harness_purpose_creative.clone(),
        _ => text.manage_harness_purpose_minimal.clone(),
    };
    let mut card = div()
        .debug_selector(move || format!("manage-harness-{profile}"))
        .v_flex()
        .min_w_0()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .min_w_0()
                .child(
                    gpui_kit::component::Icon::default()
                        .path(harness_icon_path(profile))
                        .with_size(px(20.))
                        .text_color(cx.theme().foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .min_w_0()
                        .line_clamp(1)
                        .text_color(cx.theme().foreground)
                        .child(i18n::shared_agent_name(profile, cx)),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .line_clamp(2)
                .child(purpose),
        )
        .child(harness_route(profile, cx));
    // The shell's existing per-mode action stays reachable now that the four
    // profiles moved out of the modes list.
    if let Some(row) = row {
        if row.skills_manageable {
            let mode_id = row.skills_mode_id.clone();
            let mode_name = row.name.clone();
            card = card.child(
                div().flex().flex_row().justify_end().child(
                    Button::new(SharedString::from(format!("harness-skills-{profile}")))
                        .label(text.mode_skills_open.clone())
                        .small()
                        .with_variant(ButtonVariant::Secondary)
                        .on_click(move |_, window, cx| {
                            open_mode_skills_dialog(
                                view.clone(),
                                mode_id.clone(),
                                mode_name.clone(),
                                window,
                                cx,
                            );
                        }),
                ),
            );
        }
    }
    card.into_any_element()
}

fn agent_cards(
    kind: AgentListKind,
    text: &ShellText,
    view: WeakEntity<ShellWindow>,
    cx: &mut App,
) -> gpui_kit::AnyElement {
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
        // G-A6: the four builtin harness profiles render in the harness zone,
        // not in the catalog list (web `isAgentInOverviewZone`).
        AgentListKind::Modes => catalog
            .modes
            .into_iter()
            .filter(|row| harness_profile_of(row).is_none())
            .collect(),
        AgentListKind::Subagents => catalog.subagents,
    };
    // G-A5: the search and source filters narrow whichever list is showing.
    let filter = cx
        .try_global::<AgentsFilterState>()
        .map(|state| (state.query.clone(), state.source))
        .unwrap_or_default();
    let filtered = !filter.0.trim().is_empty() || filter.1 != AgentSourceFilter::All;
    let rows: Vec<AgentCardRow> = rows
        .into_iter()
        .filter(|card| agent_matches_filter(card, &filter.0, filter.1))
        .collect();
    if rows.is_empty() {
        let (title, body) = if filtered {
            (text.manage_agents_no_match.clone(), String::new())
        } else {
            (
                text.manage_agents_empty_title.clone(),
                text.manage_agents_empty.clone(),
            )
        };
        return crate::shell::brand_empty(IconName::Bot, &title, &body);
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
        if card.skills_manageable {
            let mode_id = card.skills_mode_id.clone();
            let mode_name = card.name.clone();
            let skills_view = view.clone();
            badges = badges.child(
                Button::new(SharedString::from(format!("agent-skills-{}", card.id)))
                    .label(text.mode_skills_open.clone())
                    .small()
                    .with_variant(ButtonVariant::Secondary)
                    .on_click(move |_, window, cx| {
                        open_mode_skills_dialog(
                            skills_view.clone(),
                            mode_id.clone(),
                            mode_name.clone(),
                            window,
                            cx,
                        );
                    }),
            );
        }
        // G-A3: every non-external subagent gets the model-override menu
        // (web shows it on the detail dialog's model tab).
        if kind == AgentListKind::Subagents && card.source != AgentSourceKind::External {
            let anchor = SharedString::from(format!("agent-model-{}", card.id));
            badges = badges.child(
                div()
                    .debug_selector(move || anchor.to_string())
                    .child(agent_model_menu(&card, text, cx)),
            );
        }
        if agent_card_manageable(card.source) {
            let edit_view = view.clone();
            let edit_id = card.id.clone();
            badges = badges.child(
                Button::new(SharedString::from(format!("agent-edit-{}", card.id)))
                    .label(text.agent_card_edit.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, window, cx| {
                        open_agent_edit_dialog(edit_view.clone(), edit_id.clone(), window, cx);
                    }),
            );
            let delete_view = view.clone();
            let delete_id = card.id.clone();
            let delete_name = card.name.clone();
            badges = badges.child(
                Button::new(SharedString::from(format!("agent-delete-{}", card.id)))
                    .label(text.manage_agent_delete.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, window, cx| {
                        open_agent_delete_confirm(
                            delete_view.clone(),
                            delete_id.clone(),
                            delete_name.clone(),
                            window,
                            cx,
                        );
                    }),
            );
        }
        column = column.child(
            div()
                .flex()
                .w_full()
                .min_w_0()
                .items_start()
                .justify_between()
                .gap_2()
                .child(
                    // line_clamp over truncate: a nowrap text's intrinsic
                    // min-content keeps the settings field container at full
                    // text width (it pokes past the pane and pushes the badges
                    // column out of the clip); a wrappable line keeps the
                    // min-content small so the flex shrink chain actually
                    // constrains the row.
                    div()
                        .v_flex()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .min_w_0()
                                .line_clamp(1)
                                .child(card.name.clone()),
                        )
                        .when(!card.description.is_empty(), |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .min_w_0()
                                    .line_clamp(1)
                                    .child(card.description.clone()),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .min_w_0()
                                .line_clamp(1)
                                .child(facts),
                        ),
                )
                .child(badges),
        );
    }
    column.into_any_element()
}

// ---------------------------------------------------------------------------
// G-M2: MCP paste / file import
// ---------------------------------------------------------------------------

/// One parsed import candidate (web `McpImportCandidate` plus the entry the
/// merge writes back).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct McpImportCandidate {
    pub(crate) source_id: String,
    pub(crate) target_id: String,
    pub(crate) selected: bool,
    pub(crate) entry: serde_json::Value,
}

/// The import validation failures the shell renders (web `McpConfigFormError`
/// codes reachable through the import path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpImportError {
    InvalidJson,
    InvalidConfig,
    /// The entry uses fields the import path cannot carry or connect with
    /// (web `advancedRequired` / `commandRequired` / `urlInvalid`).
    Shape,
    SelectServers,
    IdRequired,
    InvalidId,
    IdConflict,
}

impl McpImportError {
    pub(crate) fn text(self, text: &ShellText) -> String {
        match self {
            Self::InvalidJson => text.manage_mcp_import_invalid_json.clone(),
            Self::InvalidConfig => text.manage_mcp_import_invalid_config.clone(),
            Self::Shape => text.manage_mcp_import_shape.clone(),
            Self::SelectServers => text.manage_mcp_import_select_servers.clone(),
            Self::IdRequired => text.manage_mcp_import_id_required.clone(),
            Self::InvalidId => text.manage_mcp_import_invalid_id.clone(),
            Self::IdConflict => text.manage_mcp_import_id_conflict.clone(),
        }
    }
}

/// The transport aliases the import understands (web `transportFromConfig`).
fn mcp_transport_alias(value: &str) -> Option<&'static str> {
    match value.to_lowercase().as_str() {
        "local" | "stdio" => Some("stdio"),
        "remote" | "http" | "streamable-http" | "streamable_http" | "streamablehttp" => {
            Some("streamable-http")
        }
        "sse" => Some("sse"),
        _ => None,
    }
}

/// The entry-shape gate for one imported server (web `transportFromConfig`
/// plus the connection checks `updateMcpServerConfig` runs): a recognizable
/// transport with its required connection input, and no conflicting fields.
fn validate_mcp_import_entry(entry: &serde_json::Value) -> Result<(), McpImportError> {
    let field = |key: &str| {
        entry
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let type_value = field("type").to_lowercase();
    let explicit_transport = field("transport").to_lowercase();
    let explicit_source = field("source").to_lowercase();
    for (declared, kind) in [
        (type_value.as_str(), "type"),
        (explicit_transport.as_str(), "transport"),
        (explicit_source.as_str(), "source"),
    ] {
        if declared.is_empty() {
            continue;
        }
        if mcp_transport_alias(declared).is_none()
            || (kind == "transport" && matches!(declared, "local" | "remote"))
        {
            return Err(McpImportError::Shape);
        }
    }
    let legacy_source = if type_value.is_empty() || type_value == "stdio" {
        None
    } else if type_value == "local" {
        Some("local")
    } else {
        Some("remote")
    };
    let source = if explicit_source.is_empty() {
        legacy_source
    } else {
        Some(explicit_source.as_str())
    };
    let has_command = !field("command").is_empty();
    let has_url = !field("url").is_empty();
    let declared_transport = if explicit_transport.is_empty() {
        type_value.as_str()
    } else {
        explicit_transport.as_str()
    };
    let transport =
        mcp_transport_alias(declared_transport).unwrap_or(if has_url || source == Some("remote") {
            "streamable-http"
        } else {
            "stdio"
        });
    if (has_command && has_url)
        || (source == Some("remote") && has_command)
        || (source == Some("local") && has_url)
        || (transport == "stdio" && (has_url || source == Some("remote")))
        || (transport != "stdio" && (has_command || source == Some("local")))
    {
        return Err(McpImportError::Shape);
    }
    // The rest of `updateMcpServerConfig`'s field checks: args must be a
    // string list, env/headers string maps, timeouts an object, and the
    // connection input present and usable.
    if let Some(args) = entry.get("args") {
        match args.as_array() {
            Some(items) if items.iter().all(serde_json::Value::is_string) => {}
            _ => return Err(McpImportError::Shape),
        }
    }
    for key in ["env", "headers"] {
        if let Some(value) = entry.get(key) {
            match value.as_object() {
                Some(map) if map.values().all(serde_json::Value::is_string) => {}
                _ => return Err(McpImportError::Shape),
            }
        }
    }
    if let Some(timeouts) = entry.get("timeouts") {
        if !timeouts.is_object() {
            return Err(McpImportError::Shape);
        }
    }
    if transport == "stdio" {
        if field("command").is_empty() || field("command").contains('\0') {
            return Err(McpImportError::Shape);
        }
        if let Some(args) = entry.get("args").and_then(serde_json::Value::as_array) {
            if args
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(|arg| arg.contains('\0'))
            {
                return Err(McpImportError::Shape);
            }
        }
    } else {
        let url = field("url");
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(McpImportError::Shape);
        }
    }
    Ok(())
}

/// Parses pasted or loaded MCP config into import candidates (web
/// `parseMcpImport`): the document must carry an `mcpServers` object of
/// object entries, every entry must pass the shape gate, and a candidate whose
/// id already exists starts unselected.
pub(crate) fn parse_mcp_import(
    text: &str,
    existing_ids: &[String],
) -> Result<Vec<McpImportCandidate>, McpImportError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| McpImportError::InvalidJson)?;
    let servers = match value.get("mcpServers") {
        Some(serde_json::Value::Object(map)) => map,
        _ => return Err(McpImportError::InvalidConfig),
    };
    if servers.is_empty() {
        return Err(McpImportError::SelectServers);
    }
    let mut candidates = Vec::with_capacity(servers.len());
    for (id, entry) in servers {
        if !entry.is_object() {
            return Err(McpImportError::InvalidConfig);
        }
        validate_mcp_import_entry(entry)?;
        candidates.push(McpImportCandidate {
            source_id: id.clone(),
            target_id: id.clone(),
            selected: !existing_ids.iter().any(|existing| existing == id),
            entry: entry.clone(),
        });
    }
    Ok(candidates)
}

/// Web `validateId`: 1-128 letters, digits, dots, hyphens, or underscores.
/// G-M2: the warning a preview row shows for the id currently in its input —
/// empty, malformed, or already taken (by the document or by a sibling row).
/// `None` means the row is importable under that id.
pub(crate) fn mcp_import_row_problem<'a>(
    current: &str,
    sibling_ids: &[String],
    existing_ids: &[String],
    text: &'a ShellText,
) -> Option<&'a str> {
    if current.is_empty() {
        Some(&text.manage_mcp_import_id_required)
    } else if !valid_mcp_server_id(current) {
        Some(&text.manage_mcp_import_invalid_id)
    } else if existing_ids.iter().any(|taken| taken == current)
        || sibling_ids.iter().any(|sibling| sibling == current)
    {
        Some(&text.manage_mcp_import_conflict)
    } else {
        None
    }
}

pub(crate) fn valid_mcp_server_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Merges the selected candidates into the config document (web
/// `importMcpServers`): ids are validated, a taken id is an error, the
/// untrusted-origin marker is stripped, and every imported entry lands
/// **disabled** — importing a configuration is never permission to run a
/// program.
pub(crate) fn merge_mcp_import(
    document: &serde_json::Value,
    selection: &[(String, serde_json::Value)],
) -> Result<serde_json::Value, McpImportError> {
    if selection.is_empty() {
        return Err(McpImportError::SelectServers);
    }
    let mut merged = document.clone();
    let servers = merged
        .get_mut("mcpServers")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or(McpImportError::InvalidConfig)?;
    for (target_id, entry) in selection {
        let id = target_id.trim();
        if id.is_empty() {
            return Err(McpImportError::IdRequired);
        }
        if !valid_mcp_server_id(id) {
            return Err(McpImportError::InvalidId);
        }
        if servers.contains_key(id) {
            return Err(McpImportError::IdConflict);
        }
        let mut entry = entry
            .as_object()
            .cloned()
            .ok_or(McpImportError::InvalidConfig)?;
        entry.remove("_openbitfunImport");
        entry.insert("enabled".to_string(), serde_json::Value::Bool(false));
        servers.insert(id.to_string(), serde_json::Value::Object(entry));
    }
    Ok(merged)
}

/// G-M2: opens the MCP import dialog (web `MCPServerConfigDialog`'s import
/// mode). The snapshot load goes through the JSON editor's pipeline so the
/// merge is written back under the same fingerprint guard.
pub(crate) fn open_mcp_import_dialog(
    view: WeakEntity<ShellWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (text_state, path_input) = {
        let shell = shell.read(cx);
        (shell.mcp_import_text.clone(), shell.mcp_import_path.clone())
    };
    text_state.update(cx, |state, cx| state.set_value("", window, cx));
    path_input.update(cx, |state, cx| state.set_value(String::new(), window, cx));
    shell.update(cx, |shell, cx| {
        shell.mcp_import_active = true;
        shell.mcp_import_loading = true;
        shell.mcp_import_error = None;
        shell.mcp_import_document = None;
        shell.mcp_import_fingerprint = None;
        shell.mcp_import_candidates.clear();
        shell.mcp_import_id_inputs.clear();
        cx.notify();
    });
    if let Some(handle) = crate::runtime::runtime_handle() {
        handle.load_mcp_json_config();
    }

    let cancel_view = view.clone();
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.mcp_import_active = false;
                shell.mcp_import_error = None;
                shell.mcp_import_candidates.clear();
                shell.mcp_import_id_inputs.clear();
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };
    let preview_view = view.clone();
    let preview = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        preview_mcp_import(&preview_view, window, cx);
    };
    let read_view = view.clone();
    let read = move |_: &gpui_kit::ClickEvent, _window: &mut Window, cx: &mut App| {
        let Some(shell) = read_view.upgrade() else {
            return;
        };
        let path = shell
            .read(cx)
            .mcp_import_path
            .read(cx)
            .value()
            .trim()
            .to_string();
        let Some(handle) = crate::runtime::runtime_handle() else {
            return;
        };
        shell.update(cx, |shell, cx| {
            shell.mcp_import_error = None;
            cx.notify();
        });
        handle.read_mcp_import_file(path);
    };
    let import_view = view.clone();
    let import = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = import_view.upgrade() else {
            return;
        };
        let (document, fingerprint, candidates) = {
            let shell = shell.read(cx);
            (
                shell.mcp_import_document.clone(),
                shell.mcp_import_fingerprint.clone(),
                shell.mcp_import_candidates.clone(),
            )
        };
        let text = i18n::current(cx);
        let Some(document) = document else {
            let message = text.manage_mcp_import_invalid_config.clone();
            shell.update(cx, |shell, cx| {
                shell.mcp_import_error = Some(message);
                cx.notify();
            });
            return;
        };
        // The edited target ids live in the per-row inputs, not in the
        // snapshot the preview stored.
        let mut selection = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            if !candidate.selected {
                continue;
            }
            let target_id = shell
                .read(cx)
                .mcp_import_id_inputs
                .get(index)
                .map(|input| input.read(cx).value().trim().to_string())
                .unwrap_or_else(|| candidate.target_id.clone());
            selection.push((target_id, candidate.entry.clone()));
        }
        match merge_mcp_import(&document, &selection) {
            Ok(merged) => {
                let Some(fingerprint) = fingerprint else {
                    let message = text.manage_mcp_import_invalid_config.clone();
                    shell.update(cx, |shell, cx| {
                        shell.mcp_import_error = Some(message);
                        cx.notify();
                    });
                    return;
                };
                let raw = serde_json::to_string_pretty(&merged).unwrap_or_default();
                if let Some(handle) = crate::runtime::runtime_handle() {
                    shell.update(cx, |shell, cx| {
                        shell.mcp_import_error = None;
                        cx.notify();
                    });
                    handle.save_mcp_json_config(raw, fingerprint);
                }
            }
            Err(error) => {
                let message = error.text(&text);
                shell.update(cx, |shell, cx| {
                    shell.mcp_import_error = Some(message);
                    cx.notify();
                });
            }
        }
        let _ = window;
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let Some(shell) = view.upgrade() else {
            return dialog;
        };
        let (loading, error, candidates) = {
            let shell = shell.read(cx);
            (
                shell.mcp_import_loading,
                shell.mcp_import_error.clone(),
                shell.mcp_import_candidates.clone(),
            )
        };
        let existing_ids: Vec<String> = shell
            .read(cx)
            .mcp_import_document
            .as_ref()
            .and_then(|document| document.get("mcpServers"))
            .and_then(serde_json::Value::as_object)
            .map(|servers| servers.keys().cloned().collect())
            .unwrap_or_default();
        let mut content = div().w(px(560.)).v_flex().gap_2();
        if loading {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_mcp_loading.clone()),
            );
        }
        content = content
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_mcp_import_hint.clone()),
            )
            .child(dialog_field(
                &text.manage_mcp_import_paste,
                cx.theme().muted_foreground,
                Textarea::new(&text_state).h(px(160.)),
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(dialog_field(
                        &text.manage_mcp_import_path,
                        cx.theme().muted_foreground,
                        Input::new(&path_input),
                    )))
                    .child(
                        Button::new("mcp-import-read")
                            .label(text.manage_mcp_import_read.clone())
                            .small()
                            .with_variant(ButtonVariant::Secondary)
                            .on_click(read.clone()),
                    ),
            )
            .child(
                div().flex().flex_row().justify_end().child(
                    Button::new("mcp-import-preview")
                        .label(text.manage_mcp_import_preview.clone())
                        .small()
                        .with_variant(ButtonVariant::Secondary)
                        .on_click(preview.clone()),
                ),
            );
        let live_ids: Vec<String> = shell
            .read(cx)
            .mcp_import_id_inputs
            .iter()
            .map(|input| input.read(cx).value().trim().to_string())
            .collect();
        for (index, candidate) in candidates.iter().enumerate() {
            // The rows validate the *live* id input, not the preview snapshot:
            // renaming a conflicted id has to clear its warning immediately
            // (web re-validates on every keystroke).
            let current = live_ids
                .get(index)
                .cloned()
                .unwrap_or_else(|| candidate.target_id.trim().to_string());
            let siblings: Vec<String> = live_ids
                .iter()
                .enumerate()
                .filter(|(other_index, _)| *other_index != index)
                .map(|(_, value)| value.clone())
                .collect();
            let problem = mcp_import_row_problem(&current, &siblings, &existing_ids, &text)
                .map(str::to_string);
            let toggle_view = view.clone();
            let mut row = div()
                .v_flex()
                .gap_1()
                .p_2()
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .child(
                    Checkbox::new(SharedString::from(format!(
                        "mcp-import-{}",
                        candidate.source_id
                    )))
                    .label(candidate.source_id.clone())
                    .checked(candidate.selected)
                    .on_click(move |checked: &bool, _window, cx| {
                        let checked = *checked;
                        if let Some(shell) = toggle_view.upgrade() {
                            shell.update(cx, |shell, cx| {
                                if let Some(target) = shell.mcp_import_candidates.get_mut(index) {
                                    target.selected = checked;
                                }
                                cx.notify();
                            });
                        }
                    }),
                );
            if let Some(input) = shell.read(cx).mcp_import_id_inputs.get(index) {
                row = row.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(i18n::manage_mcp_import_id(&candidate.source_id, cx)),
                        )
                        .child(Input::new(input).small()),
                );
            }
            if let Some(problem) = problem {
                row = row.child(div().text_xs().text_color(cx.theme().danger).child(problem));
            }
            content = content.child(row);
        }
        if let Some(error) = error {
            content = content.child(div().text_xs().text_color(cx.theme().danger).child(error));
        }

        dialog
            // The gpui-component dialog defaults to 448px and clips its body
            // (`overflow_hidden`), so the panel has to be told the content
            // width explicitly: this body is 560px wide, plus the 16px
            // paddings on both sides.
            .w(px(592.))
            .title(div().child(text.manage_mcp_import.clone()))
            .child(content)
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("mcp-import-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("mcp-import-confirm")
                            .label(text.manage_mcp_import_selected.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(import.clone()),
                    ),
            )
    });
}

/// G-M2: parses the paste box into preview candidates, creating one target-id
/// input per row.
fn preview_mcp_import(view: &WeakEntity<ShellWindow>, window: &mut Window, cx: &mut App) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (raw, existing_ids) = {
        let shell = shell.read(cx);
        (
            shell.mcp_import_text.read(cx).value().to_string(),
            shell
                .mcp_import_document
                .as_ref()
                .and_then(|document| document.get("mcpServers"))
                .and_then(serde_json::Value::as_object)
                .map(|servers| servers.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default(),
        )
    };
    let text = i18n::current(cx);
    match parse_mcp_import(&raw, &existing_ids) {
        Ok(candidates) => {
            let mut inputs = Vec::with_capacity(candidates.len());
            for candidate in &candidates {
                let target_id = candidate.target_id.clone();
                let input = cx.new(|cx| InputState::new(window, cx));
                input.update(cx, |state, cx| state.set_value(target_id, window, cx));
                inputs.push(input);
            }
            shell.update(cx, |shell, cx| {
                shell.mcp_import_error = None;
                shell.mcp_import_candidates = candidates;
                shell.mcp_import_id_inputs = inputs;
                cx.notify();
            });
        }
        Err(error) => {
            let message = error.text(&text);
            shell.update(cx, |shell, cx| {
                shell.mcp_import_candidates.clear();
                shell.mcp_import_id_inputs.clear();
                shell.mcp_import_error = Some(message);
                cx.notify();
            });
        }
    }
}

/// The MCP page: one row per configured server with lifecycle, enablement,/// browse, and delete actions, plus the raw JSON config editor.
pub(crate) fn mcp_group(text: &ShellText, view: WeakEntity<ShellWindow>) -> SettingGroup {
    let list_view = view.clone();
    let list_text = text.clone();
    let servers_field = SettingField::render(move |_options, _window, cx| {
        mcp_server_list(&list_text, list_view.clone(), cx)
    });
    // G-M2: the paste/file import entry (web `MCPServerConfigDialog`'s import
    // mode; the shell types the file path instead of using a native picker).
    let import_view = view.clone();
    let import_text = text.clone();
    let import_field = SettingField::render(move |_options, _window, _cx| {
        Button::new("mcp-import")
            .label(import_text.manage_mcp_import.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click({
                let view = import_view.clone();
                move |_, window, cx| open_mcp_import_dialog(view.clone(), window, cx)
            })
            .into_any_element()
    });
    let json_view = view;
    let json_text = text.clone();
    let json_field = SettingField::render(move |_options, _window, _cx| {
        Button::new("mcp-json-edit")
            .label(json_text.manage_mcp_json_edit.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click({
                let view = json_view.clone();
                move |_, window, cx| open_json_editor_dialog(view.clone(), window, cx)
            })
            .into_any_element()
    });

    SettingGroup::new().items(vec![
        SettingItem::new(text.manage_mcp_servers.clone(), servers_field)
            .description(text.manage_mcp_description.clone()),
        SettingItem::new(text.manage_mcp_import.clone(), import_field)
            .description(text.manage_mcp_import_hint.clone()),
        SettingItem::new(text.manage_mcp_json_item.clone(), json_field)
            .description(text.manage_mcp_json_description.clone()),
    ])
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
/// the runtime and the dialog closes when the catalog event lands. The same
/// dialog serves editing (`open_agent_edit_dialog` sets `agent_edit_target`,
/// the detail event prefills the fields, and id/kind/level render fixed).
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
        shell.agent_edit_target = None;
        shell.agent_edit_loading = false;
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, _window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let (kind_mode, project_level, edit_target) = {
            let shell = shell.read(cx);
            (
                shell.agent_create_kind_mode,
                shell.agent_create_project_level,
                shell.agent_edit_target.clone(),
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
        let Some(handle) = crate::runtime::runtime_handle() else {
            tracing::error!("Runtime unavailable while saving a custom agent");
            return;
        };
        if let Some(agent_id) = edit_target {
            // The id/kind/level are fixed in edit mode; tools, model,
            // review, and the context policy pass None and keep the
            // existing definition (desktop update_custom_agent parity).
            handle.update_custom_agent(crate::manage::AgentEditDraft {
                agent_id,
                name: draft.name,
                description: draft.description,
                prompt: draft.prompt,
            });
        } else {
            handle.create_custom_agent(draft);
        }
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.agent_create_active = false;
                shell.agent_create_error = None;
                shell.agent_edit_target = None;
                shell.agent_edit_loading = false;
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
        let (kind_mode, project_level, error, label_color, editing, loading) = {
            let shell = shell.read(cx);
            (
                shell.agent_create_kind_mode,
                shell.agent_create_project_level,
                shell.agent_create_error.clone(),
                cx.theme().muted_foreground,
                shell.agent_edit_target.clone(),
                shell.agent_edit_loading,
            )
        };
        let mut content = div().w(px(460.)).v_flex().gap_2();
        if editing.is_none() {
            content = content
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
                });
        }
        content = content
            .child(dialog_field(
                &text.manage_agent_id,
                label_color,
                Input::new(&id_input).disabled(editing.is_some()),
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
        if loading {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(label_color)
                    .child(text.manage_agents_loading.clone()),
            );
        }
        if let Some(error) = error {
            content = content.child(div().text_xs().text_color(cx.theme().danger).child(error));
        }

        let title = if editing.is_some() {
            text.manage_agent_edit_title.clone()
        } else {
            text.manage_agent_create_title.clone()
        };
        dialog.title(div().child(title)).child(content).footer(
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

/// Opens the same dialog in edit mode: the create dialog resets and opens
/// first, then the edit target is set and the detail fetch prefills the
/// fields (id/kind/level render fixed) while the confirm path goes through
/// the core update instead of file creation.
pub(crate) fn open_agent_edit_dialog(
    view: WeakEntity<ShellWindow>,
    agent_id: String,
    window: &mut Window,
    cx: &mut App,
) {
    open_agent_create_dialog(view.clone(), window, cx);
    let Some(shell) = view.upgrade() else {
        return;
    };
    shell.update(cx, |shell, cx| {
        shell.agent_edit_target = Some(agent_id.clone());
        shell.agent_edit_loading = true;
        cx.notify();
    });
    if let Some(handle) = crate::runtime::runtime_handle() {
        handle.load_custom_agent_detail(agent_id);
    } else {
        tracing::error!("Runtime unavailable while loading a custom agent detail");
    }
}

/// T2-12: delete confirmation for one custom agent (web confirmDanger
/// wording); the confirm path goes through the desktop `delete_custom_agent`
/// pipeline and the catalog event proves it landed.
pub(crate) fn open_agent_delete_confirm(
    view: WeakEntity<ShellWindow>,
    agent_id: String,
    agent_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    shell.update(cx, |shell, cx| {
        shell.agent_pending_delete = Some(crate::manage::AgentPendingDelete {
            agent_id: agent_id.clone(),
            agent_name,
        });
        cx.notify();
    });

    let confirm_view = view.clone();
    let cancel_view = view.clone();
    let confirm = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let pending = shell.read(cx).agent_pending_delete.clone();
        shell.update(cx, |shell, cx| {
            shell.agent_pending_delete = None;
            cx.notify();
        });
        if let Some(pending) = pending {
            if let Some(handle) = crate::runtime::runtime_handle() {
                handle.delete_custom_agent(pending.agent_id);
            } else {
                tracing::error!(
                    "Runtime unavailable while deleting custom agent {}",
                    pending.agent_id
                );
            }
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.agent_pending_delete = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let pending = view
            .upgrade()
            .and_then(|shell| shell.read(cx).agent_pending_delete.clone());
        let warning = i18n::manage_agent_delete_warning(
            pending
                .as_ref()
                .map(|p| p.agent_name.as_str())
                .unwrap_or(""),
            cx,
        );
        dialog
            .title(div().child(text.manage_agent_delete_title.clone()))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(warning),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("agent-delete-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(cancel.clone()),
                    )
                    .child(
                        Button::new("agent-delete-confirm")
                            .label(text.manage_agent_delete.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(confirm.clone()),
                    ),
            )
    });
}

/// Opens the per-mode skill selection dialog (T2-11, the web agents detail
/// skills tab): loads the mode's `ModeSkillInfo` projection, renders a
/// read-only status list, and edits the selection through the desktop
/// `replace_mode_skill_selection` pipeline. The dialog lands back in the view
/// phase when a save/reset refetch event arrives.
pub(crate) fn open_mode_skills_dialog(
    view: WeakEntity<ShellWindow>,
    mode_id: String,
    mode_name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    shell.update(cx, |shell, cx| {
        shell.mode_skills = Some(ModeSkillsDialogState {
            mode_id: mode_id.clone(),
            mode_name,
            phase: ModeSkillsPhase::Loading,
            rows: Vec::new(),
            pending: None,
            submitting: false,
            error: None,
        });
        cx.notify();
    });
    if let Some(handle) = crate::runtime::runtime_handle() {
        handle.reload_mode_skill_infos(mode_id);
    } else {
        tracing::error!("Runtime unavailable while loading mode skills {mode_id}");
    }

    let builder_view = view.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let Some(shell) = builder_view.upgrade() else {
            return dialog;
        };
        let Some(state) = shell.read(cx).mode_skills.clone() else {
            return dialog;
        };
        let muted = cx.theme().muted_foreground;

        let mut content = div().w(px(560.)).v_flex().gap_2();
        match state.phase {
            ModeSkillsPhase::Loading => {
                content = content.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(text.mode_skills_loading.clone()),
                );
            }
            ModeSkillsPhase::View | ModeSkillsPhase::Edit => {
                if let Some(error) = &state.error {
                    content = content.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(error.clone()),
                    );
                }
                if state.rows.is_empty() {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(text.mode_skills_empty.clone()),
                    );
                } else {
                    if state.phase == ModeSkillsPhase::Edit {
                        let count = i18n::mode_skills_selected_count(state.selection_count(), cx);
                        content = content.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{} · {}", count, text.mode_skills_save_hint)),
                        );
                    }
                    for row in &state.rows {
                        content = content.child(mode_skill_row(
                            builder_view.clone(),
                            &text,
                            &state,
                            row,
                            cx,
                        ));
                    }
                }
            }
        }

        let edit_view = builder_view.clone();
        let close_view = builder_view.clone();
        let reset_view = builder_view.clone();
        let save_view = builder_view.clone();
        let cancel_view = builder_view.clone();
        let footer = match state.phase {
            ModeSkillsPhase::Loading => div().flex().flex_row().justify_end().child(
                Button::new("mode-skills-close")
                    .label(text.session_action_cancel.clone())
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(move |_, window, cx| {
                        close_mode_skills_dialog(close_view.clone(), window, cx);
                    }),
            ),
            ModeSkillsPhase::View => div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("mode-skills-close")
                        .label(text.session_action_cancel.clone())
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, window, cx| {
                            close_mode_skills_dialog(close_view.clone(), window, cx);
                        }),
                )
                .child(
                    Button::new("mode-skills-edit")
                        .label(text.mode_skills_edit.clone())
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .disabled(state.rows.is_empty())
                        .on_click(move |_, _window, cx| {
                            if let Some(shell) = edit_view.upgrade() {
                                shell.update(cx, |shell, cx| {
                                    if let Some(state) = shell.mode_skills.as_mut() {
                                        state.phase = ModeSkillsPhase::Edit;
                                        state.error = None;
                                        cx.notify();
                                    }
                                });
                            }
                        }),
                ),
            ModeSkillsPhase::Edit => div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("mode-skills-reset")
                        .label(text.mode_skills_reset.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .disabled(state.submitting)
                        .on_click(move |_, _window, cx| {
                            let Some(shell) = reset_view.upgrade() else {
                                return;
                            };
                            let Some(handle) = crate::runtime::runtime_handle() else {
                                tracing::error!(
                                    "Runtime unavailable while resetting mode skill selection"
                                );
                                return;
                            };
                            let mode_id = shell.update(cx, |shell, cx| {
                                let mode_id = shell
                                    .mode_skills
                                    .as_ref()
                                    .map(|state| state.mode_id.clone());
                                if mode_id.is_some() {
                                    if let Some(state) = shell.mode_skills.as_mut() {
                                        state.submitting = true;
                                        state.error = None;
                                    }
                                    cx.notify();
                                }
                                mode_id
                            });
                            let Some(mode_id) = mode_id else {
                                return;
                            };
                            handle.reset_mode_skill_selection(mode_id);
                        }),
                )
                .child(
                    Button::new("mode-skills-cancel")
                        .label(text.session_action_cancel.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .disabled(state.submitting)
                        .on_click(move |_, _window, cx| {
                            if let Some(shell) = cancel_view.upgrade() {
                                shell.update(cx, |shell, cx| {
                                    if let Some(state) = shell.mode_skills.as_mut() {
                                        state.phase = ModeSkillsPhase::View;
                                        state.pending = None;
                                        state.error = None;
                                        cx.notify();
                                    }
                                });
                            }
                        }),
                )
                .child(
                    Button::new("mode-skills-save")
                        .label(text.session_action_confirm.clone())
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .disabled(state.submitting)
                        .on_click(move |_, _window, cx| {
                            save_mode_skills_selection(save_view.clone(), cx);
                        }),
                ),
        };

        // 448px default panel width clips a 560px body.
        dialog
            .w(px(592.))
            .title(div().child(format!("{} · {}", text.mode_skills_title, state.mode_name)))
            .child(content)
            .footer(footer)
    });
}

/// One skill row in the dialog: checkbox + level in edit phase, name/status
/// summary in view phase. Status labels follow the web runtime-status rules
/// (disabled rows show no label at all).
fn mode_skill_row(
    view: WeakEntity<ShellWindow>,
    text: &ShellText,
    state: &ModeSkillsDialogState,
    row: &ModeSkillRow,
    cx: &App,
) -> gpui_kit::AnyElement {
    let muted = cx.theme().muted_foreground;
    let level_label = if row.builtin {
        text.manage_skill_builtin.clone()
    } else if row.project_level {
        text.manage_skill_project.clone()
    } else {
        text.manage_skill_user.clone()
    };
    let status_label = match row.status() {
        ModeSkillStatus::Selected => Some(text.mode_skills_status_selected.clone()),
        ModeSkillStatus::Covered => {
            let source = row
                .covered_by(&state.rows)
                .unwrap_or(text.mode_skills_source_other.as_str());
            Some(i18n::mode_skills_covered(source, cx))
        }
        ModeSkillStatus::Enabled => Some(text.mode_skills_status_enabled.clone()),
        ModeSkillStatus::Disabled => None,
    };
    let mut head = div().flex().items_center().gap_2().min_w_0();
    match state.phase {
        ModeSkillsPhase::Edit => {
            let key = row.key.clone();
            let toggle_view = view.clone();
            head = head.child(
                Checkbox::new(SharedString::from(format!("mode-skill-{}", row.key)))
                    .label(row.name.clone())
                    .checked(state.is_selected(&row.key))
                    .on_click(move |_: &bool, _window, cx| {
                        if let Some(shell) = toggle_view.upgrade() {
                            shell.update(cx, |shell, cx| {
                                if let Some(state) = shell.mode_skills.as_mut() {
                                    state.toggle(&key);
                                    cx.notify();
                                }
                            });
                        }
                    }),
            );
        }
        _ => {
            head = head.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .min_w_0()
                    .line_clamp(1)
                    .child(row.name.clone()),
            );
        }
    }
    head = head.child(
        div()
            .text_xs()
            .text_color(muted)
            .flex_shrink_0()
            .child(level_label),
    );
    if let Some(status) = status_label {
        head = head.child(
            div()
                .text_xs()
                .text_color(muted)
                .flex_shrink_0()
                .child(status),
        );
    }
    let mut column = div().v_flex().gap_0p5().min_w_0().child(head);
    if !row.description.is_empty() {
        column = column.child(
            div()
                .text_xs()
                .text_color(muted)
                .min_w_0()
                .line_clamp(1)
                .child(row.description.clone()),
        );
    }
    column.into_any_element()
}

/// Leaves the edit phase without a write (web cancel path).
fn close_mode_skills_dialog(view: WeakEntity<ShellWindow>, window: &mut Window, cx: &mut App) {
    if let Some(shell) = view.upgrade() {
        shell.update(cx, |shell, cx| {
            shell.mode_skills = None;
            cx.notify();
        });
    }
    window.close_dialog(cx);
}

/// Submits the edited selection through the desktop replace pipeline. A
/// no-op edit (nothing touched) just leaves the edit phase, the same way the
/// web save path skips the call when `pendingSkills` is null.
fn save_mode_skills_selection(view: WeakEntity<ShellWindow>, cx: &mut App) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let request = shell.read(cx).mode_skills.as_ref().and_then(|state| {
        if state.phase != ModeSkillsPhase::Edit {
            return None;
        }
        let pending = state.pending.clone()?;
        Some((state.mode_id.clone(), pending))
    });
    let Some((mode_id, keys)) = request else {
        shell.update(cx, |shell, cx| {
            if let Some(state) = shell.mode_skills.as_mut() {
                state.phase = ModeSkillsPhase::View;
                cx.notify();
            }
        });
        return;
    };
    if let Some(handle) = crate::runtime::runtime_handle() {
        shell.update(cx, |shell, cx| {
            if let Some(state) = shell.mode_skills.as_mut() {
                state.submitting = true;
                state.error = None;
                cx.notify();
            }
        });
        handle.replace_mode_skill_selection(mode_id, keys);
    } else {
        tracing::error!("Runtime unavailable while saving mode skill selection {mode_id}");
    }
}

/// Opens the raw JSON config editor: loads the user `app.json` snapshot with
/// its fingerprint, edits it in a multi-line field, and saves through the
/// same pipeline the desktop uses (validate, fingerprint check, then runtime
/// reconcile). The dialog stays open on a failed save so the error can be
/// fixed in place.
pub(crate) fn open_json_editor_dialog(
    view: WeakEntity<ShellWindow>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let state = {
        let shell = shell.read(cx);
        shell.mcp_json_state.clone()
    };
    shell.update(cx, |shell, cx| {
        shell.mcp_json_fingerprint = None;
        shell.mcp_json_loading = true;
        shell.mcp_json_error = None;
        cx.notify();
    });
    state.update(cx, |state, cx| state.set_value("", window, cx));
    if let Some(handle) = crate::runtime::runtime_handle() {
        handle.load_mcp_json_config();
    }

    let reload_view = view.clone();
    let save_view = view.clone();
    let reload = move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = reload_view.upgrade() else {
            return;
        };
        shell.update(cx, |shell, cx| {
            shell.mcp_json_loading = true;
            shell.mcp_json_error = None;
            cx.notify();
        });
        if let Some(handle) = crate::runtime::runtime_handle() {
            handle.load_mcp_json_config();
        }
        let _ = window;
    };
    let save = move |_: &gpui_kit::ClickEvent, _window: &mut Window, cx: &mut App| {
        let Some(shell) = save_view.upgrade() else {
            return;
        };
        let (fingerprint, loading) = {
            let shell = shell.read(cx);
            (shell.mcp_json_fingerprint.clone(), shell.mcp_json_loading)
        };
        // Saving before the snapshot landed has no fingerprint to check
        // against; the loading line in the dialog explains the wait.
        let (Some(fingerprint), false) = (fingerprint, loading) else {
            return;
        };
        let raw = shell.read(cx).mcp_json_state.read(cx).value().to_string();
        if let Some(handle) = crate::runtime::runtime_handle() {
            handle.save_mcp_json_config(raw, fingerprint);
        }
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = i18n::current(cx);
        let Some(shell) = view.upgrade() else {
            return dialog;
        };
        let (state, loading, error) = {
            let shell = shell.read(cx);
            (
                shell.mcp_json_state.clone(),
                shell.mcp_json_loading,
                shell.mcp_json_error.clone(),
            )
        };
        let mut content = div().w(px(560.)).v_flex().gap_2();
        if loading {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_mcp_json_loading.clone()),
            );
        }
        content = content.child(Textarea::new(&state).h(px(320.)));
        if let Some(error) = error {
            content = content.child(div().text_xs().text_color(cx.theme().danger).child(error));
        }

        // 448px default panel width clips a 560px body.
        dialog
            .w(px(592.))
            .title(div().child(text.manage_mcp_json_item.clone()))
            .child(content)
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("mcp-json-reload")
                            .label(text.manage_mcp_refresh.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(reload.clone()),
                    )
                    .child(
                        Button::new("mcp-json-save")
                            .label(text.manage_mcp_json_save.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(save.clone()),
                    ),
            )
    });
}

/// Opens the resources/prompts browse dialog for one server and kicks off the
/// first catalog load. The dialog body renders from the window's latest
/// snapshot, so the refresh button just re-publishes a load. Resource rows
/// read on click; the rendered content shows in a panel above the list.
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
        shell.mcp_resource_view = None;
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
        let (loading, snapshot, resource_view) = {
            let shell = shell.read(cx);
            (
                shell.mcp_browse_loading.clone(),
                shell.mcp_browse.clone(),
                shell.mcp_resource_view.clone(),
            )
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
            entries = entries.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.manage_mcp_resource_hint.clone()),
            );
            if view.resources.is_empty() {
                entries = entries.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.manage_mcp_empty.clone()),
                );
            }
            for (index, resource) in view.resources.iter().enumerate() {
                let mut label = resource.name.clone();
                if resource.name != resource.uri {
                    label.push_str(" · ");
                    label.push_str(&resource.uri);
                }
                let read_view = refresh_view.clone();
                let read_uri = resource.uri.clone();
                let read_server_id = view.server_id.clone();
                entries = entries.child(
                    div()
                        .id(SharedString::from(format!(
                            "mcp-resource-{index}-{}",
                            view.server_id
                        )))
                        .on_click(move |_, _window, cx| {
                            if let Some(shell) = read_view.upgrade() {
                                shell.update(cx, |shell, cx| {
                                    shell.mcp_resource_view =
                                        Some(crate::manage::McpResourceView {
                                            uri: read_uri.clone(),
                                            loading: true,
                                            result: None,
                                        });
                                    cx.notify();
                                });
                                if let Some(handle) = crate::runtime::runtime_handle() {
                                    handle.read_mcp_resource(
                                        read_server_id.clone(),
                                        read_uri.clone(),
                                    );
                                }
                            }
                        })
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
            let mut resource_panel = div().v_flex().gap_1();
            if let Some(read) = &resource_view {
                let close_view = refresh_view.clone();
                resource_panel = resource_panel.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(read.uri.clone()),
                        )
                        .child(
                            Button::new("mcp-resource-close")
                                .icon(IconName::Close)
                                .small()
                                .with_variant(ButtonVariant::Ghost)
                                .on_click(move |_, _window, cx| {
                                    if let Some(shell) = close_view.upgrade() {
                                        shell.update(cx, |shell, cx| {
                                            shell.mcp_resource_view = None;
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                );
                if read.loading {
                    resource_panel = resource_panel.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(text.manage_mcp_resource_loading.clone()),
                    );
                } else if let Some(result) = &read.result {
                    resource_panel = resource_panel.child(match result {
                        Ok(content) => div()
                            .id("mcp-resource-content")
                            .h(px(220.))
                            .overflow_y_scroll()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_2()
                            .text_xs()
                            .child(render_resource_content(content, &text))
                            .into_any_element(),
                        Err(_) => div()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(text.manage_mcp_resource_failed.clone())
                            .into_any_element(),
                    });
                }
            }
            content = content.child(resource_panel);
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

        // 448px default panel width clips a 520px body.
        dialog
            .w(px(552.))
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
    let (result_input, default_result) = {
        let shell = shell.read(cx);
        let default_result = shell
            .mcp_interactions
            .front()
            .map(|request| default_interaction_result(&request.method))
            .unwrap_or_default();
        (shell.mcp_interaction_result_input.clone(), default_result)
    };
    result_input.update(cx, |state, cx| state.set_value(default_result, window, cx));
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
                    .rounded(cx.theme().radius)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(interaction_params_preview(&request.params)),
            ))
            .child(dialog_field(
                &text.manage_interaction_result_hint,
                label_color,
                Textarea::new(&result_input).h(px(160.)),
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
    // The dialog stays open for the next ask: prefill its default response so
    // a stale result never rides into the following request.
    let next_default = {
        let shell = shell.read(cx);
        shell
            .mcp_interactions
            .front()
            .map(|request| default_interaction_result(&request.method))
    };
    if let Some(default) = next_default {
        let result_input = shell.read(cx).mcp_interaction_result_input.clone();
        result_input.update(cx, |state, cx| state.set_value(default, window, cx));
    }
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
    use openbitfun_core::agentic::tools::implementations::skills::SkillLocation;
    use std::collections::HashSet;

    fn sample_skill(
        key: &str,
        level: SkillLocation,
        builtin: bool,
        dir_name: &str,
    ) -> openbitfun_core::agentic::tools::implementations::skills::SkillInfo {
        openbitfun_core::agentic::tools::implementations::skills::SkillInfo {
            key: key.to_string(),
            name: key.to_string(),
            description: String::new(),
            path: format!("/skills/{key}"),
            entry_file: None,
            level,
            source_slot: "openbitfun".to_string(),
            source_id: "openbitfun".to_string(),
            source_label: "OpenBitFun".to_string(),
            installation_source: None,
            import_origin: None,
            dir_name: dir_name.to_string(),
            is_builtin: builtin,
            group_key: None,
            is_shadowed: false,
            shadowed_by_key: None,
            allow_implicit_invocation: true,
            allow_user_invocation: true,
            argument_hint: None,
        }
    }

    fn mode_skill_row(
        key: &str,
        effective_enabled: bool,
        selected_for_runtime: bool,
        shadowed: bool,
        shadowed_by_key: Option<String>,
        source_label: &str,
    ) -> ModeSkillRow {
        ModeSkillRow {
            key: key.to_string(),
            name: key.to_string(),
            description: String::new(),
            project_level: false,
            builtin: false,
            shadowed,
            source_label: source_label.to_string(),
            shadowed_by_key,
            effective_enabled,
            selected_for_runtime,
        }
    }

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

    #[test]
    fn interaction_defaults_follow_the_method() {
        let sampling = default_interaction_result("sampling/createMessage");
        let parsed: serde_json::Value =
            serde_json::from_str(&sampling).expect("sampling default parses");
        assert_eq!(parsed["role"], "assistant");
        assert_eq!(parsed["content"][0]["type"], "text");

        let elicitation = default_interaction_result("elicitation/create");
        let parsed: serde_json::Value =
            serde_json::from_str(&elicitation).expect("elicitation default parses");
        assert_eq!(parsed["action"], "accept");
    }

    #[test]
    fn resource_content_rendering_mirrors_the_web_browser() {
        use openbitfun_core::service::mcp::protocol::{MCPResourceContent, ResourcesReadResult};
        let text = crate::i18n::fallback_text(openbitfun_core::service::i18n::LocaleId::EnUS);
        let content = |uri: &str,
                       mime: Option<&str>,
                       body: Option<&str>,
                       blob: Option<&str>|
         -> MCPResourceContent {
            MCPResourceContent {
                uri: uri.to_string(),
                content: body.map(str::to_string),
                blob: blob.map(str::to_string),
                mime_type: mime.map(str::to_string),
                annotations: None,
                meta: None,
            }
        };

        // A single entry renders with its mime header when present and as
        // plain text without one (web resource-browser semantics).
        let single = ResourcesReadResult {
            contents: vec![content(
                "file:///a.txt",
                Some("text/plain"),
                Some("hello"),
                None,
            )],
        };
        assert_eq!(
            render_resource_content(&single, &text),
            "[text/plain]\n\nhello"
        );
        let bare = ResourcesReadResult {
            contents: vec![content("file:///a.txt", None, Some("hello"), None)],
        };
        assert_eq!(render_resource_content(&bare, &text), "hello");

        // Binary payloads render the localized binary note.
        let binary = ResourcesReadResult {
            contents: vec![content(
                "file:///img.png",
                Some("image/png"),
                None,
                Some("aGk="),
            )],
        };
        assert!(render_resource_content(&binary, &text).contains("Binary content"));

        // Multiple entries get numbered headers and `---` separators.
        let multi = ResourcesReadResult {
            contents: vec![
                content("file:///one.txt", Some("text/plain"), Some("one"), None),
                content("file:///two.txt", None, Some("two"), None),
            ],
        };
        let rendered = render_resource_content(&multi, &text);
        assert!(rendered.contains("#1 file:///one.txt (text/plain)"));
        assert!(rendered.contains("two"));
        assert!(rendered.contains("---"));

        // Empty contents render the empty note.
        let empty = ResourcesReadResult { contents: vec![] };
        assert_eq!(
            render_resource_content(&empty, &text),
            text.manage_mcp_resource_empty
        );
    }
    #[test]
    fn representative_mode_ids_follow_the_first_mode_per_profile() {
        let modes = vec![
            ("Standard".to_string(), Some("Standard".to_string())),
            ("Cowork".to_string(), Some("Cowork".to_string())),
            ("coding_shared".to_string(), Some("Standard".to_string())),
            ("Custom".to_string(), None),
        ];
        let map = representative_mode_ids(&modes);
        // The first mode in list order owns the profile's overrides.
        assert_eq!(map["Standard"].as_str(), "Standard");
        assert_eq!(map["coding_shared"].as_str(), "Standard");
        assert_eq!(map["Cowork"].as_str(), "Cowork");
        // A mode without a profile persists under itself.
        assert_eq!(map["Custom"].as_str(), "Custom");
    }

    #[test]
    fn agent_card_manageable_follows_the_web_source_guard() {
        // Web canManageCustomAgent: source neither builtin nor external.
        assert!(agent_card_manageable(AgentSourceKind::User));
        assert!(agent_card_manageable(AgentSourceKind::Project));
        assert!(!agent_card_manageable(AgentSourceKind::Builtin));
        assert!(!agent_card_manageable(AgentSourceKind::External));
    }

    #[test]
    fn project_agent_detail_projects_kind_and_level_strings() {
        let detail = openbitfun_core::agentic::agents::CustomAgentDetail {
            agent_id: "helper".to_string(),
            kind: "subagent".to_string(),
            name: "Helper".to_string(),
            description: "d".to_string(),
            prompt: "p".to_string(),
            tools: vec![],
            readonly: true,
            review: false,
            model: String::new(),
            path: "/tmp/helper.md".to_string(),
            user_context_policy: vec![],
            level: "project".to_string(),
        };
        let row = project_agent_detail(&detail);
        assert!(!row.mode_kind);
        assert!(row.project_level);
        assert_eq!(row.agent_id, "helper");
        assert_eq!(row.name, "Helper");
    }

    #[test]
    fn mode_skill_status_and_covered_by_follow_the_web_rules() {
        let rows = vec![
            mode_skill_row("user::a", true, true, false, None, "OpenBitFun"),
            mode_skill_row("user::b", true, false, false, None, "OpenBitFun"),
            mode_skill_row(
                "user::c",
                true,
                false,
                true,
                Some("user::d".into()),
                "Claude Code",
            ),
            mode_skill_row("user::d", true, true, false, None, "Claude Code"),
            mode_skill_row("user::e", false, false, false, None, "OpenBitFun"),
        ];
        assert_eq!(rows[0].status(), ModeSkillStatus::Selected);
        assert_eq!(rows[1].status(), ModeSkillStatus::Enabled);
        // Shadowing only matters for enabled rows the runtime did not pick.
        assert_eq!(rows[2].status(), ModeSkillStatus::Covered);
        assert_eq!(rows[2].covered_by(&rows), Some("Claude Code"));
        assert_eq!(rows[4].status(), ModeSkillStatus::Disabled);
        // A winner key without a row (or with an empty label) leaves the
        // source unnamed; the render falls back to the generic source label.
        let orphan = mode_skill_row("user::f", true, false, true, Some("user::gone".into()), "x");
        assert_eq!(orphan.covered_by(&rows), None);
        let blank_winner = vec![
            mode_skill_row("user::d", true, true, false, None, ""),
            mode_skill_row("user::g", true, false, true, Some("user::d".into()), "x"),
        ];
        assert_eq!(blank_winner[1].covered_by(&blank_winner), None);
    }

    #[test]
    fn mode_skills_dialog_selection_seeds_from_effective_and_toggles() {
        let mut state = ModeSkillsDialogState {
            mode_id: "Standard".to_string(),
            mode_name: "Standard".to_string(),
            phase: ModeSkillsPhase::Edit,
            rows: vec![
                mode_skill_row("user::on", true, true, false, None, "OpenBitFun"),
                mode_skill_row("user::off", false, false, false, None, "OpenBitFun"),
            ],
            pending: None,
            submitting: false,
            error: None,
        };
        // The selection starts at the effective enablement.
        assert!(state.is_selected("user::on"));
        assert!(!state.is_selected("user::off"));
        assert_eq!(state.selection_count(), 1);
        // The first toggle seeds pending from the effective set.
        state.toggle("user::off");
        assert_eq!(state.selection_count(), 2);
        state.toggle("user::on");
        assert_eq!(state.selection_count(), 1);
        assert_eq!(state.pending, Some(vec!["user::off".to_string()]));
    }

    #[test]
    fn user_mode_skill_override_diff_mirrors_the_desktop_rules() {
        let skills = vec![
            sample_skill("user::custom", SkillLocation::User, false, "custom"),
            // Real builtin default-off for the Standard profile.
            sample_skill(
                "user::agent-browser",
                SkillLocation::User,
                true,
                "agent-browser",
            ),
        ];
        let enabled: HashSet<String> = ["user::agent-browser".to_string()].into();
        let (disabled, enabled_user) = user_mode_skill_override_diff(&skills, "Standard", &enabled);
        // A default-on skill left out lands in the disable set; a default-off
        // builtin picked by the user lands in the enable set.
        assert_eq!(disabled, vec!["user::custom".to_string()]);
        assert_eq!(enabled_user, vec!["user::agent-browser".to_string()]);

        // Everything at its default produces an empty diff.
        let enabled: HashSet<String> = ["user::custom".to_string()].into();
        let (disabled, enabled_user) = user_mode_skill_override_diff(&skills, "Standard", &enabled);
        assert!(disabled.is_empty());
        assert!(enabled_user.is_empty());
    }

    #[test]
    fn selection_key_helpers_trim_dedupe_and_split_levels() {
        let normalized = normalize_skill_key_list(vec![
            "  user::a  ".to_string(),
            String::new(),
            "user::a".to_string(),
            "user::b".to_string(),
        ]);
        assert_eq!(normalized, vec!["user::a", "user::b"]);

        let skills = vec![
            sample_skill("user::a", SkillLocation::User, false, "a"),
            sample_skill("project::b", SkillLocation::Project, false, "b"),
        ];
        let enabled: HashSet<String> = ["user::a".to_string()].into();
        assert!(unknown_skill_keys(&skills, &["user::a".to_string()]).is_empty());
        assert_eq!(
            unknown_skill_keys(&skills, &["user::x".to_string()]),
            vec!["user::x".to_string()]
        );
        // Project skills left out of the selection form the disable set.
        assert_eq!(
            disabled_project_skill_keys(&skills, &enabled),
            vec!["project::b".to_string()]
        );
    }

    fn facet_row(
        key: &str,
        builtin: bool,
        project: bool,
        group: Option<&str>,
        source: (&str, &str),
    ) -> SkillRow {
        SkillRow {
            key: key.to_string(),
            name: key.to_string(),
            description: String::new(),
            project_level: project,
            builtin,
            shadowed: false,
            disabled: false,
            group_key: group.map(str::to_string),
            source_id: source.0.to_string(),
            source_label: source.1.to_string(),
        }
    }

    #[test]
    fn facets_split_levels_groups_and_non_native_sources() {
        let rows = vec![
            facet_row(
                "b1",
                true,
                false,
                Some("core"),
                ("openbitfun", "OpenBitFun"),
            ),
            facet_row(
                "b2",
                true,
                false,
                Some("core"),
                ("openbitfun", "OpenBitFun"),
            ),
            facet_row("b3", true, false, None, ("openbitfun", "OpenBitFun")),
            facet_row("u1", false, false, None, ("", "")),
            facet_row("p1", false, true, None, ("codex", "Codex")),
            facet_row("u2", false, false, None, (".agents", ".agents")),
        ];
        let facets = skill_facets(&rows);
        assert_eq!(facets.all, 6);
        assert_eq!(facets.builtin, 3);
        assert_eq!(facets.user, 2);
        assert_eq!(facets.project, 1);
        // Built-in groups only, keyed, with counts.
        assert_eq!(facets.groups, vec![("core".to_string(), 2)]);
        // Native sources fold into the level facets; only Codex/.agents show.
        assert_eq!(
            facets.sources,
            vec![
                (".agents".to_string(), ".agents".to_string(), 1),
                ("codex".to_string(), "Codex".to_string(), 1),
            ]
        );
    }

    #[test]
    fn facet_filters_route_rows_by_their_facet_id() {
        let rows = vec![
            facet_row(
                "b1",
                true,
                false,
                Some("core"),
                ("openbitfun", "OpenBitFun"),
            ),
            facet_row("p1", false, true, None, ("codex", "Codex")),
            facet_row("u1", false, false, None, ("", "")),
        ];
        let matches = |facet: &SkillFacet| -> Vec<String> {
            rows.iter()
                .filter(|row| skill_matches(row, facet))
                .map(|row| row.key.clone())
                .collect()
        };
        assert_eq!(matches(&SkillFacet::All), vec!["b1", "p1", "u1"]);
        assert_eq!(matches(&SkillFacet::Builtin), vec!["b1"]);
        assert_eq!(matches(&SkillFacet::User), vec!["u1"]);
        assert_eq!(matches(&SkillFacet::Project), vec!["p1"]);
        assert_eq!(matches(&SkillFacet::Group("core".to_string())), vec!["b1"]);
        assert_eq!(
            matches(&SkillFacet::Source("codex".to_string())),
            vec!["p1"]
        );
        assert!(matches(&SkillFacet::Source("openbitfun".to_string())).is_empty());
        assert!(matches(&SkillFacet::Market).is_empty());
    }

    #[test]
    fn empty_group_keys_do_not_become_facets() {
        let rows = vec![facet_row(
            "b1",
            true,
            false,
            Some("  "),
            ("openbitfun", "OpenBitFun"),
        )];
        // runtime.rs normalizes on the way in; the derivation stays honest
        // when a blank key slips through anyway.
        assert!(skill_facets(&rows).groups.is_empty());
        assert!(is_native_source(""));
        assert!(is_native_source("openbitfun-user"));
        assert!(!is_native_source("codex"));
    }

    #[gpui_kit::test]
    fn skills_card_text_wraps_inside_the_settings_content_column(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let presets = crate::theme::load_presets();
            presets
                .iter()
                .find(|preset| preset.id == crate::theme::DEFAULT_PRESET_ID)
                .or_else(|| presets.first())
                .expect("theme preset exists")
                .apply(cx);
            cx.set_global(SkillCatalog {
                loaded: true,
                skills: vec![SkillRow {
                    key: "user::probe".into(),
                    name: "Probe skill".into(),
                    description: "A deliberately long description so the wrap probe has more text than one line can hold inside the narrow settings column.".repeat(3),
                    project_level: false,
                    builtin: false,
                    shadowed: false,
                    disabled: false,
                    group_key: None,
                    source_id: String::new(),
                    source_label: String::new(),
                }],
            });
            cx.set_global(crate::i18n::fallback_text(
                openbitfun_core::service::i18n::LocaleId::EnUS,
            ));
        });
        let window = cx.add_empty_window();
        struct Probe;
        impl gpui_kit::Render for Probe {
            fn render(
                &mut self,
                _window: &mut gpui_kit::Window,
                cx: &mut gpui_kit::Context<Self>,
            ) -> impl gpui_kit::IntoElement {
                use gpui_kit::component::setting::{SettingPage, Settings};
                let text = crate::i18n::current(cx);
                // The real container: Settings drives h_resizable +
                // container_query + the virtualized group list around the
                // facet page; that environment is where the wrap broke.
                Settings::new("skills-wrap-probe")
                    .sidebar_width(gpui_kit::px(180.))
                    .pages(vec![SettingPage::new("Tools").group(skills_group(&text))])
                    .into_any_element()
            }
        }
        let view = window.update(|_, cx| cx.new(|_| Probe));
        window.draw(
            gpui::point(gpui::px(0.), gpui::px(0.)),
            gpui::size(gpui::px(1200.), gpui::px(800.)),
            {
                let view = view.clone();
                move |_, _| view.clone().into_any_element()
            },
        );
        window.run_until_parked();
        let card = window.debug_bounds("skills-card").expect("card laid out");
        let desc = window
            .debug_bounds("skills-card-desc")
            .expect("description laid out");
        // The probe description exceeds one line at any pane width: the clamp
        // must wrap it (two text_xs lines tall) and keep it inside the card,
        // not poke a single long line past the settings column.
        assert!(
            desc.size.height > gpui::px(24.),
            "description wraps to two lines: desc {:?}",
            desc
        );
        assert!(
            desc.origin.x + desc.size.width <= card.origin.x + card.size.width,
            "description stays inside the card: desc {:?} card {:?}",
            desc,
            card
        );
        assert!(
            card.origin.x + card.size.width <= gpui::px(1200.),
            "card stays inside the window: card {:?}",
            card
        );
    }

    fn skill_row(key: &str, name: &str, description: &str, shadowed: bool) -> SkillRow {
        SkillRow {
            key: key.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            project_level: false,
            builtin: false,
            shadowed,
            disabled: false,
            group_key: None,
            source_id: "openbitfun".to_string(),
            source_label: "OpenBitFun".to_string(),
        }
    }

    fn agent_card(name: &str, description: &str, source: AgentSourceKind) -> AgentCardRow {
        AgentCardRow {
            id: name.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            tool_count: 0,
            readonly: false,
            review: false,
            source,
            enabled: None,
            skills_manageable: false,
            skills_mode_id: String::new(),
            model_override: None,
        }
    }

    /// G-K5: the local skill filter matches the web fields — name,
    /// description, and the path-derived key — case-insensitively and after
    /// trimming, and the shadowed toggle drops the duplicate rows.
    #[test]
    fn skill_filter_matches_name_description_and_key() {
        let row = skill_row("user::alpha", "Alpha", "Summarises long diffs", false);
        assert!(skill_matches_filter(&row, "", false));
        assert!(skill_matches_filter(&row, "  ", false));
        assert!(skill_matches_filter(&row, "alpha", false));
        assert!(skill_matches_filter(&row, "ALPHA", false));
        assert!(skill_matches_filter(&row, " long diffs ", false));
        // The key stands in for the path the row does not keep.
        assert!(skill_matches_filter(&row, "user::", false));
        assert!(!skill_matches_filter(&row, "missing", false));

        let shadowed = skill_row("user::beta", "Beta", "", true);
        assert!(skill_matches_filter(&shadowed, "", false));
        assert!(!skill_matches_filter(&shadowed, "", true));
        // Hiding duplicates only drops shadowed rows, never the query match.
        assert!(!skill_matches_filter(&shadowed, "beta", true));
    }

    /// G-A5: the agents filter follows the web — the search covers the name
    /// and description, the source filter compares the resolved source, and
    /// both compose.
    #[test]
    fn agent_filter_follows_search_and_source() {
        let user_agent = agent_card("Reviewer", "Checks diffs", AgentSourceKind::User);
        let builtin_agent = agent_card("Explorer", "Reads the tree", AgentSourceKind::Builtin);
        assert!(agent_matches_filter(
            &user_agent,
            "",
            AgentSourceFilter::All
        ));
        assert!(agent_matches_filter(
            &user_agent,
            "diffs",
            AgentSourceFilter::All
        ));
        assert!(agent_matches_filter(
            &user_agent,
            "REVIEWER",
            AgentSourceFilter::All
        ));
        assert!(!agent_matches_filter(
            &user_agent,
            "reviewer",
            AgentSourceFilter::Builtin
        ));
        assert!(agent_matches_filter(
            &builtin_agent,
            "",
            AgentSourceFilter::Builtin
        ));
        assert!(!agent_matches_filter(
            &builtin_agent,
            "tree",
            AgentSourceFilter::User
        ));
    }

    /// G-A5: the type filter picks exactly one of the page's two lists, and
    /// `Any type` keeps both.
    #[test]
    fn agent_kind_filter_selects_the_lists() {
        assert!(AgentKindFilter::All.matches(AgentListKind::Modes));
        assert!(AgentKindFilter::All.matches(AgentListKind::Subagents));
        assert!(AgentKindFilter::Agent.matches(AgentListKind::Modes));
        assert!(!AgentKindFilter::Agent.matches(AgentListKind::Subagents));
        assert!(!AgentKindFilter::SubAgent.matches(AgentListKind::Modes));
        assert!(AgentKindFilter::SubAgent.matches(AgentListKind::Subagents));
    }

    /// G-A6: the harness zone reads web `STRATEGY_ROUTES` — one route per
    /// profile with the same node counts, every node inside the 320x44 box,
    /// and an unknown profile falling back to Minimal.
    #[test]
    fn harness_routes_mirror_the_web_specs() {
        let expected_nodes: [(&str, usize); 4] = [
            ("Minimal", 1),
            ("Standard", 4),
            ("Ultimate", 6),
            ("Creative", 3),
        ];
        for (profile, nodes) in expected_nodes {
            let spec = harness_route_spec(profile);
            assert_eq!(spec.nodes.len(), nodes, "{profile} node count");
            assert!(!spec.paths.is_empty(), "{profile} has a path");
            for (x, y) in spec.nodes {
                assert!(
                    (0.0..=HARNESS_ROUTE_WIDTH).contains(x)
                        && (0.0..=HARNESS_ROUTE_HEIGHT).contains(y),
                    "{profile} node ({x},{y}) stays in the route box"
                );
            }
            // The main path spans the box from its left edge; Ultimate's
            // second path branches off mid-box (web `M 48 22 C ...`).
            assert!(matches!(
                spec.paths[0].first(),
                Some(HarnessRouteCmd::MoveTo(0., _))
            ));
        }
        // Ultimate is the only two-path profile; a cubic keeps its two
        // controls (web `C c1 c2 end`).
        assert_eq!(harness_route_spec("Ultimate").paths.len(), 2);
        assert!(matches!(
            harness_route_spec("Ultimate").paths[1].first(),
            Some(HarnessRouteCmd::MoveTo(48., 22.))
        ));
        assert!(harness_route_spec("Ultimate")
            .paths
            .iter()
            .flat_map(|path| path.iter())
            .any(|command| matches!(command, HarnessRouteCmd::CurveTo(..))));
        assert_eq!(
            harness_route_spec("Nope").nodes,
            harness_route_spec("Minimal").nodes
        );
    }

    /// G-A6: only builtin harness ids count as harness profiles — a user
    /// agent reusing the id stays in the modes list.
    #[test]
    fn harness_profile_requires_builtin_source() {
        assert_eq!(
            harness_profile_of(&agent_card("Minimal", "", AgentSourceKind::Builtin)),
            Some("Minimal")
        );
        assert_eq!(
            harness_profile_of(&agent_card("Minimal", "", AgentSourceKind::User)),
            None
        );
        assert_eq!(
            harness_profile_of(&agent_card("Claw", "", AgentSourceKind::Builtin)),
            None
        );
    }

    /// G-A3: the model menu labels the stored override the web way — the
    /// default entry, the two aliases, inheritance, and the configured
    /// model's display name.
    #[test]
    fn agent_model_labels_follow_the_selection() {
        use openbitfun_core::service::config::types::SubagentModelSelection;
        let text = crate::i18n::fallback_text(openbitfun_core::service::i18n::LocaleId::EnUS);
        let models = vec![
            crate::runtime::ModelChoice {
                id: "claude-sonnet".to_string(),
                name: "Claude Sonnet".to_string(),
                provider: "anthropic".to_string(),
                enabled: true,
            },
            crate::runtime::ModelChoice {
                id: "gpt-5".to_string(),
                name: "GPT-5".to_string(),
                provider: "openai".to_string(),
                enabled: false,
            },
        ];
        assert_eq!(
            agent_model_label(None, &models, &text),
            text.manage_agent_model_default
        );
        assert_eq!(
            agent_model_label(Some(&SubagentModelSelection::Inherit), &models, &text),
            text.settings_subagent_model_inherit
        );
        assert_eq!(
            agent_model_label(Some(&SubagentModelSelection::fixed("fast")), &models, &text),
            text.settings_subagent_model_fast
        );
        assert_eq!(
            agent_model_label(
                Some(&SubagentModelSelection::fixed("claude-sonnet")),
                &models,
                &text
            ),
            "Claude Sonnet · anthropic"
        );
        // A model that left the catalog keeps its id readable.
        assert_eq!(
            agent_model_label(Some(&SubagentModelSelection::fixed("gone")), &models, &text),
            "gone"
        );
    }

    /// G-A3: the menu lists the default entry, the three aliases, then only
    /// enabled configured models (same `enabled` filter the shell model menus
    /// use).
    #[test]
    fn agent_model_options_skip_disabled_models() {
        let text = crate::i18n::fallback_text(openbitfun_core::service::i18n::LocaleId::EnUS);
        let models = vec![
            crate::runtime::ModelChoice {
                id: "claude-sonnet".to_string(),
                name: "Claude Sonnet".to_string(),
                provider: String::new(),
                enabled: true,
            },
            crate::runtime::ModelChoice {
                id: "gpt-5".to_string(),
                name: "GPT-5".to_string(),
                provider: String::new(),
                enabled: false,
            },
        ];
        let options = agent_model_options(&models, &text);
        let values: Vec<&str> = options.iter().map(|(value, _)| value.as_str()).collect();
        assert_eq!(
            values,
            vec![
                AGENT_MODEL_DEFAULT_CHOICE,
                "inherit",
                "fast",
                "primary",
                "claude-sonnet"
            ]
        );
    }

    /// G-M2: the paste parser mirrors web `parseMcpImport` — the document
    /// shape, the per-entry shape gate, and the "existing id starts
    /// unselected" rule.
    #[test]
    fn mcp_import_parser_mirrors_the_web_rules() {
        let existing = vec!["taken".to_string()];
        let candidates = parse_mcp_import(
            r#"{"mcpServers":{"fresh":{"command":"npx","args":["-y","srv"]},
                "taken":{"url":"https://example.com/mcp"}}}"#,
            &existing,
        )
        .expect("valid document");
        assert_eq!(candidates.len(), 2);
        let fresh = candidates
            .iter()
            .find(|candidate| candidate.source_id == "fresh")
            .expect("fresh candidate");
        assert!(fresh.selected);
        assert_eq!(fresh.target_id, "fresh");
        let taken = candidates
            .iter()
            .find(|candidate| candidate.source_id == "taken")
            .expect("taken candidate");
        assert!(!taken.selected, "an existing id imports unselected");

        assert_eq!(
            parse_mcp_import("not json", &existing).unwrap_err(),
            McpImportError::InvalidJson
        );
        assert_eq!(
            parse_mcp_import("[]", &existing).unwrap_err(),
            McpImportError::InvalidConfig
        );
        assert_eq!(
            parse_mcp_import(r#"{"servers":{}}"#, &existing).unwrap_err(),
            McpImportError::InvalidConfig
        );
        assert_eq!(
            parse_mcp_import(r#"{"mcpServers":{}}"#, &existing).unwrap_err(),
            McpImportError::SelectServers
        );
        assert_eq!(
            parse_mcp_import(r#"{"mcpServers":{"a":"nope"}}"#, &existing).unwrap_err(),
            McpImportError::InvalidConfig
        );
        // Shape gate: no transport and no connection input.
        assert_eq!(
            parse_mcp_import(r#"{"mcpServers":{"a":{}}}"#, &existing).unwrap_err(),
            McpImportError::Shape
        );
        // Shape gate: web `advancedRequired` conflicts.
        assert_eq!(
            parse_mcp_import(
                r#"{"mcpServers":{"a":{"command":"npx","url":"https://example.com"}}}"#,
                &existing
            )
            .unwrap_err(),
            McpImportError::Shape
        );
        assert_eq!(
            parse_mcp_import(
                r#"{"mcpServers":{"a":{"transport":"carrier","command":"npx"}}}"#,
                &existing
            )
            .unwrap_err(),
            McpImportError::Shape
        );
        assert_eq!(
            parse_mcp_import(r#"{"mcpServers":{"a":{"url":"ftp://x"}}}"#, &existing).unwrap_err(),
            McpImportError::Shape
        );
        assert_eq!(
            parse_mcp_import(
                r#"{"mcpServers":{"a":{"command":"npx","env":{"TOKEN":1}}}}"#,
                &existing
            )
            .unwrap_err(),
            McpImportError::Shape
        );
        // A declared stdio transport still needs its command.
        assert_eq!(
            parse_mcp_import(r#"{"mcpServers":{"a":{"transport":"stdio"}}}"#, &existing)
                .unwrap_err(),
            McpImportError::Shape
        );
    }

    /// G-M2: each preview row warns about the id it currently holds — the
    /// live input, so renaming a conflicted id clears the warning (the smoke
    /// found the snapshot-based version leaving a stale one).
    #[test]
    fn mcp_import_row_problem_follows_the_live_id() {
        let text = crate::i18n::fallback_text(openbitfun_core::service::i18n::LocaleId::EnUS);
        let existing = vec!["kept".to_string()];
        let none: Vec<String> = Vec::new();
        assert_eq!(
            mcp_import_row_problem("fresh", &none, &existing, &text),
            None
        );
        assert_eq!(
            mcp_import_row_problem("", &none, &existing, &text),
            Some(text.manage_mcp_import_id_required.as_str())
        );
        assert_eq!(
            mcp_import_row_problem("bad id", &none, &existing, &text),
            Some(text.manage_mcp_import_invalid_id.as_str())
        );
        assert_eq!(
            mcp_import_row_problem("kept", &none, &existing, &text),
            Some(text.manage_mcp_import_conflict.as_str())
        );
        // A sibling row holding the same id collides too, and the renamed id
        // that no longer collides is clean again.
        assert_eq!(
            mcp_import_row_problem("twin", &["twin".to_string()], &existing, &text),
            Some(text.manage_mcp_import_conflict.as_str())
        );
        assert_eq!(
            mcp_import_row_problem("smoke-http-2", &["smoke-fs".to_string()], &existing, &text),
            None
        );
    }

    /// G-M2: the merge mirrors web `importMcpServers` — ids validated, taken
    /// ids rejected, the untrusted marker stripped, and every imported entry
    /// forced disabled while the rest of the document survives.
    #[test]
    fn mcp_import_merge_forces_disabled_and_keeps_the_rest() {
        let document = serde_json::json!({
            "mcpServers": {"kept": {"command": "srv"}},
            "otherKey": {"untouched": true},
        });
        let entry = serde_json::json!({
            "command": "npx",
            "args": ["-y", "srv"],
            "enabled": true,
            "_openbitfunImport": {"from": "elsewhere"},
        });
        let merged =
            merge_mcp_import(&document, &[("fresh".to_string(), entry)]).expect("merge succeeds");
        let fresh = &merged["mcpServers"]["fresh"];
        assert_eq!(fresh["enabled"], serde_json::Value::Bool(false));
        assert!(fresh.get("_openbitfunImport").is_none());
        assert_eq!(fresh["args"], serde_json::json!(["-y", "srv"]));
        assert!(merged["mcpServers"].get("kept").is_some());
        assert_eq!(merged["otherKey"], document["otherKey"]);

        let err = |document: &serde_json::Value, selection: Vec<(String, serde_json::Value)>| {
            merge_mcp_import(document, &selection).unwrap_err()
        };
        assert_eq!(err(&document, vec![]), McpImportError::SelectServers);
        assert_eq!(
            err(&document, vec![("  ".to_string(), serde_json::json!({}))]),
            McpImportError::IdRequired
        );
        assert_eq!(
            err(
                &document,
                vec![("bad id".to_string(), serde_json::json!({}))]
            ),
            McpImportError::InvalidId
        );
        assert_eq!(
            err(&document, vec![("kept".to_string(), serde_json::json!({}))]),
            McpImportError::IdConflict
        );
        // The same id selected twice collides with itself.
        assert_eq!(
            err(
                &document,
                vec![
                    ("fresh".to_string(), serde_json::json!({})),
                    ("fresh".to_string(), serde_json::json!({})),
                ]
            ),
            McpImportError::IdConflict
        );
        assert!(valid_mcp_server_id("a.b-c_d1"));
        assert!(!valid_mcp_server_id("a b"));
        assert!(!valid_mcp_server_id(""));
        assert!(!valid_mcp_server_id(&"a".repeat(129)));
    }
}
