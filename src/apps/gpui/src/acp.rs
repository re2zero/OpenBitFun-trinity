//! The ACP agents page (`tools.external-agents`) — the shell's view of the
//! external-agent surface the web exposes at `settingsRegistry.ts:362`.
//!
//! The page has two views (the web has three; the SSH view is a registered
//! capability gap — see `.relay/design/gpui-ely/checklist-acp.md` §3.1):
//!
//! - **Local**: the agent registry — the five built-in presets plus every
//!   configured client, each with its detection status and the actions that
//!   status allows (install the CLI, prepare the adapter, add the config,
//!   pick the tool-confirmation mode).
//! - **Advanced JSON**: the raw `acpClients` document, the way the web edits
//!   an agent that the list cannot express.
//!
//! The status rules are ported from the web's `AcpAgentsConfig.tsx`
//! (`getAgentRowStatus` / `getIssueKind` / `classifyRequirementError`) so both
//! surfaces agree on what "installed", "partially available" and "invalid"
//! mean. The probes themselves come from the core ACP service
//! (`openbitfun_acp::AcpClientService`), not from this module.

use gpui_kit::App;
use gpui_kit::Global;

use openbitfun_acp::client::{
    AcpClientInfo, AcpClientPermissionMode, AcpClientRequirementProbe, AcpClientService,
};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

/// The core ACP client service. The shell owns one instance, built at
/// bootstrap the way the MCP service is built — the ACP crate is host
/// agnostic, so the same service the desktop host exposes over Tauri backs
/// this page.
static ACP_CLIENT_SERVICE: OnceLock<Arc<AcpClientService>> = OnceLock::new();

/// Publishes the service the page's actions run against.
pub(crate) fn set_acp_client_service(service: Arc<AcpClientService>) {
    if ACP_CLIENT_SERVICE.set(service).is_err() {
        tracing::warn!("ACP client service is already set");
    }
}

/// The service, when the shell finished booting.
pub(crate) fn acp_client_service() -> Option<Arc<AcpClientService>> {
    ACP_CLIENT_SERVICE.get().cloned()
}

/// One built-in agent the page offers, mirroring the web's `PRESETS` table
/// (`AcpAgentsConfig.tsx:116`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AcpPreset {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) command: &'static str,
    pub(crate) args: &'static [&'static str],
    /// Presets that speak ACP natively need no adapter package (their CLI is
    /// launched directly).
    pub(crate) native_acp: bool,
    /// Presets OpenBitFun cannot install for the user — the agent is
    /// installed manually, so the page hides the one-click install action.
    pub(crate) self_managed_install: bool,
}

/// The five built-in agents, in the web's order.
pub(crate) const ACP_PRESETS: &[AcpPreset] = &[
    AcpPreset {
        id: "opencode",
        name: "opencode",
        command: "opencode",
        args: &["acp"],
        native_acp: true,
        self_managed_install: false,
    },
    AcpPreset {
        id: "dsh",
        name: "DeepSeek Harness",
        command: "dsh",
        args: &["--profile", "openbitfun-acp"],
        native_acp: true,
        self_managed_install: false,
    },
    AcpPreset {
        id: "omp",
        name: "Oh My Pi",
        command: "omp",
        args: &["acp"],
        native_acp: true,
        self_managed_install: true,
    },
    AcpPreset {
        id: "claude-code",
        name: "Claude Code",
        command: "npx",
        args: &["--yes", "@agentclientprotocol/claude-agent-acp@latest"],
        native_acp: false,
        self_managed_install: false,
    },
    AcpPreset {
        id: "codex",
        name: "Codex",
        command: "npx",
        args: &["--yes", "@agentclientprotocol/codex-acp@latest"],
        native_acp: false,
        self_managed_install: false,
    },
];

/// The preset with this id, if it is a built-in one.
pub(crate) fn acp_preset(id: &str) -> Option<&'static AcpPreset> {
    ACP_PRESETS.iter().find(|preset| preset.id == id)
}

/// The view the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AcpView {
    #[default]
    Local,
    Json,
}

/// The registry filter (`registry.filters` in the web catalog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AcpFilter {
    #[default]
    All,
    /// Web label 「已启用」 — the rows whose status is enabled or ready.
    Enabled,
    NotInstalled,
    Invalid,
}

impl AcpFilter {
    /// The value the stored filter keeps.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Enabled => "enabled",
            Self::NotInstalled => "not_installed",
            Self::Invalid => "invalid",
        }
    }

    /// The filter a stored value names; unknown values fall back to `All`.
    pub(crate) fn from_str(value: &str) -> Self {
        match value {
            "enabled" => Self::Enabled,
            "not_installed" => Self::NotInstalled,
            "invalid" => Self::Invalid,
            _ => Self::All,
        }
    }
}

/// How the agent's requirements turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpIssue {
    None,
    CliMissing,
    AdapterMissing,
    ConnectionFailed,
    PermissionDenied,
    PathInvalid,
    VersionMismatch,
    ConfigInvalid,
}

/// The status the row shows (web `AgentRowStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpStatus {
    Enabled,
    Ready,
    Partial,
    NotInstalled,
    Invalid,
    Checking,
}

/// The web's `classifyRequirementError`: the probe error text decides which
/// issue the row reports.
pub(crate) fn acp_issue_from_error(error: Option<&str>) -> AcpIssue {
    let Some(error) = error else {
        return AcpIssue::ConfigInvalid;
    };
    let lower = error.to_ascii_lowercase();
    if lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("access denied")
    {
        return AcpIssue::PermissionDenied;
    }
    if lower.contains("ssh")
        || lower.contains("connection refused")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("network")
        || lower.contains("host key")
    {
        return AcpIssue::ConnectionFailed;
    }
    if lower.contains("version") || lower.contains("mismatch") || lower.contains("incompatible") {
        return AcpIssue::VersionMismatch;
    }
    if lower.contains("not found")
        || lower.contains("no such file or directory")
        || lower.contains("command -v")
        || lower.contains("path")
    {
        return AcpIssue::PathInvalid;
    }
    AcpIssue::ConfigInvalid
}

/// A probe failure caused by a timeout says nothing about the installation, so
/// the row keeps its configured status instead of claiming the CLI is missing
/// (web `hasTransientProbeFailure`).
fn acp_has_transient_probe_failure(probe: Option<&AcpClientRequirementProbe>) -> bool {
    let Some(probe) = probe else {
        return false;
    };
    [probe.tool.error.as_deref(), probe.adapter.as_ref().and_then(|a| a.error.as_deref())]
        .into_iter()
        .flatten()
        .any(|error| {
            let lower = error.to_ascii_lowercase();
            lower.contains("timeout") || lower.contains("timed out")
        })
}

/// The issue the row reports (web `getIssueKind`).
pub(crate) fn acp_issue_kind(
    probe: Option<&AcpClientRequirementProbe>,
    requires_adapter: bool,
) -> AcpIssue {
    let Some(probe) = probe else {
        return AcpIssue::ConfigInvalid;
    };
    let tool_issue = acp_issue_from_error(probe.tool.error.as_deref());
    if tool_issue != AcpIssue::ConfigInvalid {
        return tool_issue;
    }
    if !probe.tool.installed {
        return AcpIssue::CliMissing;
    }
    if requires_adapter {
        if let Some(adapter) = probe.adapter.as_ref() {
            let adapter_issue = acp_issue_from_error(adapter.error.as_deref());
            if adapter_issue != AcpIssue::ConfigInvalid {
                return adapter_issue;
            }
        }
        if probe.adapter.as_ref().is_some_and(|adapter| !adapter.installed) {
            return AcpIssue::AdapterMissing;
        }
    }
    if probe.runnable {
        AcpIssue::None
    } else {
        AcpIssue::ConfigInvalid
    }
}

/// The status the row shows (web `getAgentRowStatus`).
pub(crate) fn acp_status(
    configured: bool,
    enabled: bool,
    tool_installed: Option<bool>,
    adapter_installed: Option<bool>,
    requires_adapter: bool,
    probe_pending: bool,
    probe: Option<&AcpClientRequirementProbe>,
) -> AcpStatus {
    if probe_pending {
        return AcpStatus::Checking;
    }
    if tool_installed == Some(false) {
        if configured && enabled && acp_has_transient_probe_failure(probe) {
            return AcpStatus::Enabled;
        }
        return AcpStatus::NotInstalled;
    }
    if requires_adapter && adapter_installed == Some(false) {
        if configured && enabled && acp_has_transient_probe_failure(probe) {
            return AcpStatus::Enabled;
        }
        return AcpStatus::Partial;
    }
    if !configured {
        return AcpStatus::Ready;
    }
    if !enabled {
        return AcpStatus::Invalid;
    }
    AcpStatus::Enabled
}

/// The label the status badge reads (web `getStatusLabel`).
pub(crate) fn acp_status_label(status: AcpStatus, issue: AcpIssue) -> &'static str {
    match status {
        AcpStatus::Enabled => "enabled",
        AcpStatus::Ready => "ready",
        AcpStatus::Partial => "partial",
        AcpStatus::Checking => "checking",
        AcpStatus::NotInstalled => match issue {
            AcpIssue::ConnectionFailed => "connection_failed",
            AcpIssue::PermissionDenied => "permission_denied",
            AcpIssue::PathInvalid => "path_invalid",
            AcpIssue::VersionMismatch => "version_mismatch",
            AcpIssue::AdapterMissing => "acp_missing",
            AcpIssue::CliMissing => "cli_missing",
            _ => "config_invalid",
        },
        AcpStatus::Invalid => "config_invalid",
    }
}

/// The detail lines the status badge carries as its tooltip (web
/// `getStatusTitle`).
pub(crate) fn acp_status_detail(
    status: AcpStatus,
    issue: AcpIssue,
    probe: Option<&AcpClientRequirementProbe>,
    requires_adapter: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    match status {
        AcpStatus::Enabled => lines.push("enabled".to_string()),
        AcpStatus::Ready => lines.push("ready".to_string()),
        AcpStatus::Partial => lines.push("partial".to_string()),
        AcpStatus::Checking => lines.push("checking".to_string()),
        _ => {}
    }
    match issue {
        AcpIssue::ConnectionFailed => lines.push("connection_failed".to_string()),
        AcpIssue::PermissionDenied => lines.push("permission_denied".to_string()),
        AcpIssue::PathInvalid => lines.push("path_invalid".to_string()),
        AcpIssue::VersionMismatch => lines.push("version_mismatch".to_string()),
        AcpIssue::AdapterMissing => lines.push("acp_missing".to_string()),
        AcpIssue::CliMissing => lines.push("cli_missing".to_string()),
        _ => {}
    }
    if issue == AcpIssue::ConfigInvalid && status == AcpStatus::Invalid {
        lines.push("config_invalid".to_string());
    }
    let adapter_missing = requires_adapter
        && probe
            .and_then(|probe| probe.adapter.as_ref())
            .is_some_and(|adapter| !adapter.installed);
    if adapter_missing && !lines.iter().any(|line| line == "acp_missing") {
        lines.push("acp_missing".to_string());
    }
    if let Some(probe) = probe {
        if let Some(path) = probe.tool.path.as_ref() {
            lines.push(format!("path={path}"));
        }
        if let Some(version) = probe.tool.version.as_ref() {
            lines.push(format!("version={version}"));
        }
        if let Some(error) = probe.tool.error.as_ref() {
            lines.push(error.clone());
        }
        if let Some(error) = probe.adapter.as_ref().and_then(|adapter| adapter.error.as_ref()) {
            lines.push(error.clone());
        }
        lines.extend(probe.notes.iter().cloned());
    }
    lines
}

/// What the row's trailing control offers (web's confirmation cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcpRowAction {
    /// The tool-confirmation mode picker.
    PermissionMode,
    /// Install the agent CLI.
    InstallCli,
    /// Prepare the ACP adapter.
    ConfigureAcp,
    /// The self-managed agent needs a manual install — show the instructions.
    ViewInstructions,
    /// The row reports a problem — show the error.
    ViewError,
    /// The agent is not configured yet — add its default config.
    Add,
}

/// The action the row's state allows, in the web's order.
pub(crate) fn acp_row_action(
    status: AcpStatus,
    issue: AcpIssue,
    requires_adapter: bool,
    has_config: bool,
    self_managed_install: bool,
) -> Option<AcpRowAction> {
    let show_select = has_config && matches!(status, AcpStatus::Enabled | AcpStatus::Ready);
    if show_select {
        return Some(AcpRowAction::PermissionMode);
    }
    let can_install_cli =
        status == AcpStatus::NotInstalled && issue != AcpIssue::ConnectionFailed && !self_managed_install;
    if can_install_cli {
        return Some(AcpRowAction::InstallCli);
    }
    let can_configure_acp = requires_adapter
        && (issue == AcpIssue::AdapterMissing
            || (status == AcpStatus::Partial && issue == AcpIssue::ConfigInvalid));
    if can_configure_acp {
        return Some(AcpRowAction::ConfigureAcp);
    }
    let self_managed_cli_missing =
        self_managed_install && status == AcpStatus::NotInstalled && issue == AcpIssue::CliMissing;
    if self_managed_cli_missing && has_config {
        return Some(AcpRowAction::ViewInstructions);
    }
    let can_view_error = status == AcpStatus::Invalid
        || matches!(
            issue,
            AcpIssue::ConnectionFailed
                | AcpIssue::PermissionDenied
                | AcpIssue::PathInvalid
                | AcpIssue::VersionMismatch
        );
    if can_view_error {
        return Some(AcpRowAction::ViewError);
    }
    if !has_config {
        return Some(AcpRowAction::Add);
    }
    None
}

/// One row of the registry: a preset or a configured client.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AcpAgentRow {
    pub(crate) id: String,
    /// The name the row shows (the preset name, the configured name, or the id).
    pub(crate) display_name: String,
    /// The line under the name (the preset description, or the command line).
    pub(crate) description: String,
    pub(crate) status: AcpStatus,
    pub(crate) issue: AcpIssue,
    pub(crate) requires_adapter: bool,
    pub(crate) has_config: bool,
    pub(crate) enabled: bool,
    pub(crate) self_managed_install: bool,
    pub(crate) action: Option<AcpRowAction>,
    /// The stored tool-confirmation mode, for the picker.
    pub(crate) permission_mode: AcpClientPermissionMode,
    /// The status tooltip lines.
    pub(crate) detail: Vec<String>,
}

/// The clients and the probes the page renders.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AcpCatalog {
    pub(crate) loaded: bool,
    pub(crate) clients: Vec<AcpClientInfo>,
    pub(crate) probes: Vec<AcpClientRequirementProbe>,
    /// A probe that is still running — the rows that have no probe yet read as
    /// 「检测中」.
    pub(crate) probing: bool,
}

impl Global for AcpCatalog {}

impl AcpCatalog {
    pub(crate) fn try_current(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }

    /// The probe for one agent id.
    pub(crate) fn probe(&self, id: &str) -> Option<&AcpClientRequirementProbe> {
        self.probes.iter().find(|probe| probe.id == id)
    }
}

/// The page's own state: the active view, the registry search and filter, the
/// JSON draft and its baseline, and the last action receipt.
#[derive(Debug, Clone, Default)]
pub(crate) struct AcpPageState {
    pub(crate) view: AcpView,
    pub(crate) search: String,
    pub(crate) filter: AcpFilter,
    /// The JSON draft the user is editing.
    pub(crate) json_text: String,
    /// The JSON as it was loaded — 「还原」 returns to it.
    pub(crate) json_baseline: String,
    /// The draft differs from the baseline.
    pub(crate) json_dirty: bool,
    /// The last action's receipt (success or failure).
    pub(crate) notice: Option<AcpNotice>,
    /// An action is running (install, adapter download, save).
    pub(crate) busy: bool,
}

/// The outcome of the last action, shown as the page's receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpNotice {
    pub(crate) success: bool,
    /// The message key (without the `gpui-shell-acp-` prefix) or the raw text
    /// for a runtime error.
    pub(crate) message: String,
}

impl Global for AcpPageState {}

impl AcpPageState {
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

/// The configured clients that are not built-in presets, sorted by id — the
/// web's `customClientRows`.
pub(crate) fn acp_custom_client_ids(catalog: &AcpCatalog) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for client in &catalog.clients {
        if acp_preset(&client.id).is_none() && !ids.contains(&client.id) {
            ids.push(client.id.clone());
        }
    }
    ids.sort();
    ids
}

/// Whether the row survives the active filter (web's filter rules).
pub(crate) fn acp_matches_filter(status: AcpStatus, filter: AcpFilter) -> bool {
    match filter {
        AcpFilter::All => true,
        AcpFilter::Enabled => matches!(status, AcpStatus::Enabled | AcpStatus::Ready),
        AcpFilter::NotInstalled => status == AcpStatus::NotInstalled,
        AcpFilter::Invalid => status == AcpStatus::Invalid,
    }
}

/// Whether the row survives the search box (web matches the name, the id, the
/// description and the command).
pub(crate) fn acp_matches_search(row: &AcpAgentRow, search: &str) -> bool {
    let search = search.trim().to_ascii_lowercase();
    if search.is_empty() {
        return true;
    }
    [row.id.as_str(), row.display_name.as_str(), row.description.as_str()]
        .join(" ")
        .to_ascii_lowercase()
        .contains(&search)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(installed: bool) -> AcpClientRequirementProbe {
        AcpClientRequirementProbe {
            id: "opencode".to_string(),
            tool: openbitfun_acp::client::AcpRequirementProbeItem {
                name: "opencode".to_string(),
                installed,
                version: None,
                path: None,
                error: None,
            },
            adapter: None,
            runnable: installed,
            notes: Vec::new(),
        }
    }

    #[test]
    fn probe_errors_map_to_issues() {
        assert_eq!(
            acp_issue_from_error(Some("permission denied")),
            AcpIssue::PermissionDenied
        );
        assert_eq!(
            acp_issue_from_error(Some("ssh: connect to host")),
            AcpIssue::ConnectionFailed
        );
        assert_eq!(
            acp_issue_from_error(Some("command -v opencode: not found")),
            AcpIssue::PathInvalid
        );
        assert_eq!(
            acp_issue_from_error(Some("version mismatch")),
            AcpIssue::VersionMismatch
        );
        assert_eq!(acp_issue_from_error(None), AcpIssue::ConfigInvalid);
    }

    #[test]
    fn a_missing_cli_reads_as_not_installed() {
        let probe = probe(false);
        let status = acp_status(
            true,
            true,
            Some(false),
            None,
            false,
            false,
            Some(&probe),
        );
        assert_eq!(status, AcpStatus::NotInstalled);
        assert_eq!(
            acp_issue_kind(Some(&probe), false),
            AcpIssue::PathInvalid
        );
    }

    #[test]
    fn a_configured_agent_reads_as_enabled() {
        let probe = probe(true);
        let status = acp_status(true, true, Some(true), None, false, false, Some(&probe));
        assert_eq!(status, AcpStatus::Enabled);
    }

    #[test]
    fn an_unconfigured_agent_reads_as_ready() {
        let status = acp_status(false, false, Some(true), None, false, false, None);
        assert_eq!(status, AcpStatus::Ready);
    }

    #[test]
    fn a_disabled_agent_reads_as_invalid() {
        let probe = probe(true);
        let status = acp_status(true, false, Some(true), None, false, false, Some(&probe));
        assert_eq!(status, AcpStatus::Invalid);
    }

    #[test]
    fn a_missing_adapter_reads_as_partial() {
        let mut probe = probe(true);
        probe.adapter = Some(openbitfun_acp::client::AcpRequirementProbeItem {
            name: "codex-acp".to_string(),
            installed: false,
            version: None,
            path: None,
            error: None,
        });
        let status = acp_status(
            true,
            true,
            Some(true),
            Some(false),
            true,
            false,
            Some(&probe),
        );
        assert_eq!(status, AcpStatus::Partial);
        assert_eq!(
            acp_issue_kind(Some(&probe), true),
            AcpIssue::AdapterMissing
        );
    }

    #[test]
    fn the_row_action_follows_the_web_order() {
        // A configured, working agent offers the permission-mode picker.
        assert_eq!(
            acp_row_action(AcpStatus::Enabled, AcpIssue::None, false, true, false),
            Some(AcpRowAction::PermissionMode)
        );
        // An unconfigured agent offers 「添加」.
        assert_eq!(
            acp_row_action(AcpStatus::Ready, AcpIssue::None, false, false, false),
            Some(AcpRowAction::Add)
        );
        // A missing CLI offers the install action.
        assert_eq!(
            acp_row_action(
                AcpStatus::NotInstalled,
                AcpIssue::CliMissing,
                false,
                true,
                false
            ),
            Some(AcpRowAction::InstallCli)
        );
        // A self-managed agent never offers the install action.
        assert_eq!(
            acp_row_action(
                AcpStatus::NotInstalled,
                AcpIssue::CliMissing,
                false,
                true,
                true
            ),
            Some(AcpRowAction::ViewInstructions)
        );
        // A missing adapter offers the adapter action.
        assert_eq!(
            acp_row_action(
                AcpStatus::Partial,
                AcpIssue::AdapterMissing,
                true,
                true,
                false
            ),
            Some(AcpRowAction::ConfigureAcp)
        );
        // A broken configuration offers the error view.
        assert_eq!(
            acp_row_action(
                AcpStatus::Invalid,
                AcpIssue::ConfigInvalid,
                false,
                true,
                false
            ),
            Some(AcpRowAction::ViewError)
        );
    }

    #[test]
    fn the_filter_selects_the_same_rows_as_the_web() {
        assert!(acp_matches_filter(AcpStatus::Enabled, AcpFilter::Enabled));
        assert!(acp_matches_filter(AcpStatus::Ready, AcpFilter::Enabled));
        assert!(!acp_matches_filter(
            AcpStatus::NotInstalled,
            AcpFilter::Enabled
        ));
        assert!(acp_matches_filter(
            AcpStatus::NotInstalled,
            AcpFilter::NotInstalled
        ));
        assert!(acp_matches_filter(AcpStatus::Invalid, AcpFilter::Invalid));
        assert!(acp_matches_filter(AcpStatus::Enabled, AcpFilter::All));
    }

    #[test]
    fn the_search_matches_the_name_and_the_command() {
        let row = AcpAgentRow {
            id: "codex".to_string(),
            display_name: "Codex".to_string(),
            description: "通过官方 ACP 适配器连接".to_string(),
            status: AcpStatus::Enabled,
            issue: AcpIssue::None,
            requires_adapter: false,
            has_config: true,
            enabled: true,
            self_managed_install: false,
            action: None,
            permission_mode: AcpClientPermissionMode::Ask,
            detail: Vec::new(),
        };
        assert!(acp_matches_search(&row, "codex"));
        assert!(acp_matches_search(&row, "适配器"));
        assert!(!acp_matches_search(&row, "opencode"));
        assert!(acp_matches_search(&row, "  "));
    }
}

// ── rendering ─────────────────────────────────────────────────────────────

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt};
use gpui_kit::{
    div, prelude::*, px, AnyElement, Entity, IntoElement, ParentElement, SharedString, Styled,
    Subscription, Window,
};

use crate::i18n::ShellText;
use crate::settings_cards::{dropdown_control, search_input_control, DeferredControl, SettingsCard,
    SettingsCardRow};

/// The page body: the view switcher, the last action's receipt, and the
/// active view (web `AcpAgentsConfig`'s view controls and section stack).
pub(crate) fn acp_page_body(text: &ShellText, cx: &mut App) -> AnyElement {
    let state = AcpPageState::current(cx);
    let mut body = div().flex().flex_col().gap_4().w_full();
    body = body.child(acp_view_switcher(text, state.view));
    if let Some(notice) = state.notice.clone() {
        body = body.child(acp_notice(text, &notice, cx));
    }
    body = body.child(match state.view {
        AcpView::Local => acp_local_card(text, cx),
        AcpView::Json => acp_json_card(text, cx),
    });
    body.into_any_element()
}

/// The view switcher (web's `TabGroup` over the three views).
fn acp_view_switcher(text: &ShellText, active: AcpView) -> AnyElement {
    let mut row = div().flex().flex_row().items_center().gap_2();
    for (view, label, id) in [
        (AcpView::Local, text.acp_view_local.clone(), "acp-view-local"),
        (AcpView::Json, text.acp_view_json.clone(), "acp-view-json"),
    ] {
        let is_active = view == active;
        let button = Button::new(id)
            .label(label)
            .small()
            .with_variant(if is_active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            })
            .on_click(move |_, _, cx| {
                AcpPageState::mutate(cx, |state| state.view = view);
            });
        row = row.child(button);
    }
    row.into_any_element()
}

/// The receipt of the last action (the web's toast, in the settings face's
/// inline form).
fn acp_notice(text: &ShellText, notice: &AcpNotice, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let message = if notice.success {
        acp_receipt_text(text, &notice.message)
    } else {
        notice.message.clone()
    };
    div()
        .text_sm()
        .text_color(if notice.success {
            theme.success
        } else {
            theme.danger
        })
        .child(message)
        .into_any_element()
}

/// The localized receipt for a successful action.
fn acp_receipt_text(text: &ShellText, key: &str) -> String {
    match key {
        "save_success" | "json_saved" => text.acp_notice_save_success.clone(),
        "install_success" => text.acp_notice_install_success.clone(),
        "configure_success" => text.acp_notice_configure_success.clone(),
        "config_added" => text.acp_notice_config_added.clone(),
        other => other.to_string(),
    }
}

/// The registry card: the search field, the filter, and one row per agent.
fn acp_local_card(text: &ShellText, cx: &mut App) -> AnyElement {
    let catalog = AcpCatalog::try_current(cx).unwrap_or_default();
    let state = AcpPageState::current(cx);

    let mut card = SettingsCard::new("acp-registry")
        .title(text.acp_registry_title.clone())
        .description(text.acp_registry_description.clone())
        .extra(acp_refresh_button(text));

    card = card.row(
        SettingsCardRow::body("acp-toolbar").control(acp_toolbar(text, cx)),
    );

    if !catalog.loaded {
        card = card.row(SettingsCardRow::body("acp-loading").control(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.acp_loading.clone()),
        ));
        return div().child(card).into_any_element();
    }

    let rows = acp_agent_rows(text, &catalog);
    let visible = rows
        .iter()
        .filter(|row| acp_matches_filter(row.status, state.filter))
        .filter(|row| acp_matches_search(row, &state.search))
        .collect::<Vec<_>>();
    if visible.is_empty() {
        card = card.row(SettingsCardRow::body("acp-empty").control(
            crate::shell::brand_empty(
                IconName::Bot,
                &text.acp_empty_title,
                &text.acp_empty,
            ),
        ));
    } else {
        for row in visible {
            card = card.row(acp_agent_row(text, row, cx));
        }
    }
    div().child(card).into_any_element()
}

/// The card head's re-check action (web `ConfigRefreshButton`).
fn acp_refresh_button(text: &ShellText) -> AnyElement {
    Button::new("acp-refresh")
        .label(text.acp_action_refresh.clone())
        .small()
        .with_variant(ButtonVariant::Secondary)
        .on_click(|_, _, cx| {
            if let Some(handle) = crate::runtime::runtime_handle() {
                handle.probe_acp_requirements(true);
            }
        })
        .into_any_element()
}

/// The search field and the filter (web's registry toolbar).
fn acp_toolbar(text: &ShellText, cx: &mut App) -> AnyElement {
    let state = AcpPageState::current(cx);
    let search = search_input_control(
        "acp-search",
        text.acp_search_placeholder.clone(),
        text.acp_search_placeholder.clone(),
        SharedString::from(state.search.clone()),
        Rc::new(|value: SharedString, cx: &mut App| {
            AcpPageState::mutate(cx, |state| state.search = value.to_string());
        }),
    );
    let filter = dropdown_control(
        "acp-filter",
        acp_filter_label(text, state.filter),
        Rc::new(acp_filter_options(text)),
        Rc::new(|value: SharedString, cx: &mut App| {
            let filter = AcpFilter::from_str(&value);
            AcpPageState::mutate(cx, |state| state.filter = filter);
        }),
        false,
    );
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .w_full()
        .child(div().flex_1().min_w(px(220.)).child(search))
        .child(filter)
        .into_any_element()
}

/// The filter options (web `registryFilterOptions`).
fn acp_filter_options(text: &ShellText) -> Vec<(SharedString, SharedString)> {
    vec![
        (
            SharedString::from("all"),
            text.acp_filter_all.clone(),
        ),
        (
            SharedString::from("enabled"),
            text.acp_filter_enabled.clone(),
        ),
        (
            SharedString::from("not_installed"),
            text.acp_filter_not_installed.clone(),
        ),
        (
            SharedString::from("invalid"),
            text.acp_filter_invalid.clone(),
        ),
    ]
}

/// The filter's closed-state label — the option's own text, never the stored
/// value.
fn acp_filter_label(text: &ShellText, filter: AcpFilter) -> SharedString {
    match filter {
        AcpFilter::All => text.acp_filter_all.clone(),
        AcpFilter::Enabled => text.acp_filter_enabled.clone(),
        AcpFilter::NotInstalled => text.acp_filter_not_installed.clone(),
        AcpFilter::Invalid => text.acp_filter_invalid.clone(),
    }
}

/// One registry row.
fn acp_agent_row(text: &ShellText, row: &AcpAgentRow, cx: &mut App) -> SettingsCardRow {
    SettingsCardRow::new(format!("acp-agent-{}", row.id), row.display_name.clone())
        .description(row.description.clone())
        .control(acp_row_control(text, row, cx))
}

/// The trailing lane: the status badge and the action the status allows.
fn acp_row_control(text: &ShellText, row: &AcpAgentRow, cx: &mut App) -> AnyElement {
    let mut lane = div()
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap_2();
    lane = lane.child(acp_status_badge(text, row, cx));
    if let Some(action) = row.action {
        lane = lane.child(acp_action_control(text, row, action));
    }
    lane.into_any_element()
}

/// The status badge (web `AgentStatusPill`).
fn acp_status_badge(text: &ShellText, row: &AcpAgentRow, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let key = acp_status_label(row.status, row.issue);
    let (label, color) = match key {
        "enabled" => (text.acp_status_enabled.clone(), theme.success),
        "ready" => (text.acp_status_ready.clone(), theme.success),
        "partial" => (text.acp_status_partial.clone(), theme.warning),
        "checking" => (text.acp_status_checking.clone(), theme.info),
        "cli_missing" => (text.acp_status_cli_missing.clone(), theme.muted_foreground),
        "acp_missing" => (text.acp_status_acp_missing.clone(), theme.muted_foreground),
        "path_invalid" => (text.acp_status_path_invalid.clone(), theme.danger),
        "version_mismatch" => (text.acp_status_version_mismatch.clone(), theme.danger),
        "connection_failed" => (text.acp_status_connection_failed.clone(), theme.danger),
        "permission_denied" => (text.acp_status_permission_denied.clone(), theme.danger),
        _ => (text.acp_status_config_invalid.clone(), theme.danger),
    };
    div()
        .flex_shrink_0()
        .rounded_md()
        .border_1()
        .border_color(color)
        .px_2()
        .py_0p5()
        .text_xs()
        .text_color(color)
        .child(label)
        .into_any_element()
}

/// The action the row's status allows.
fn acp_action_control(text: &ShellText, row: &AcpAgentRow, action: AcpRowAction) -> AnyElement {
    match action {
        AcpRowAction::PermissionMode => acp_permission_control(text, row),
        AcpRowAction::InstallCli => {
            let id = row.id.clone();
            Button::new(format!("acp-install-{}", row.id))
                .label(text.acp_action_install_cli.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _, cx| {
                    if let Some(handle) = crate::runtime::runtime_handle() {
                        handle.install_acp_client_cli(id.clone());
                    }
                })
                .into_any_element()
        }
        AcpRowAction::ConfigureAcp => {
            let id = row.id.clone();
            Button::new(format!("acp-configure-{}", row.id))
                .label(text.acp_action_configure.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _, cx| {
                    if let Some(handle) = crate::runtime::runtime_handle() {
                        handle.configure_acp_adapter(id.clone());
                    }
                })
                .into_any_element()
        }
        AcpRowAction::ViewInstructions => Button::new(format!("acp-instructions-{}", row.id))
            .label(text.acp_action_view_instructions.clone())
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click(move |_, _, cx| {
                let message = text.acp_cli_missing_detail.clone();
                AcpPageState::mutate(cx, |state| {
                    state.notice = Some(AcpNotice {
                        success: false,
                        message,
                    });
                });
            })
            .into_any_element(),
        AcpRowAction::ViewError => {
            let message = row.detail.join("\n");
            Button::new(format!("acp-error-{}", row.id))
                .label(text.acp_action_view_error.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _, cx| {
                    AcpPageState::mutate(cx, |state| {
                        state.notice = Some(AcpNotice {
                            success: false,
                            message: message.clone(),
                        });
                    });
                })
                .into_any_element()
        }
        AcpRowAction::Add => {
            let id = row.id.clone();
            let label = if row.self_managed_install {
                text.acp_action_add_config.clone()
            } else {
                text.acp_action_add.clone()
            };
            Button::new(format!("acp-add-{}", row.id))
                .label(label)
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _, cx| {
                    if let Some(handle) = crate::runtime::runtime_handle() {
                        handle.add_acp_preset(id.clone());
                    }
                })
                .into_any_element()
        }
    }
}

/// The tool-confirmation picker (web `permissionOptions`).
fn acp_permission_control(text: &ShellText, row: &AcpAgentRow) -> AnyElement {
    let options = Rc::new(vec![
        (
            SharedString::from("ask"),
            text.acp_permission_ask.clone(),
        ),
        (
            SharedString::from("allow_once"),
            text.acp_permission_allow_once.clone(),
        ),
    ]);
    let label = match row.permission_mode {
        AcpClientPermissionMode::AllowOnce => text.acp_permission_allow_once.clone(),
        _ => text.acp_permission_ask.clone(),
    };
    let id = row.id.clone();
    dropdown_control(
        format!("acp-permission-{}", row.id),
        label,
        options,
        Rc::new(move |value: SharedString, cx: &mut App| {
            let mode = value.to_string();
            if let Some(handle) = crate::runtime::runtime_handle() {
                handle.set_acp_permission_mode(id.clone(), mode);
            }
        }),
        false,
    )
}

/// The advanced-JSON card: the security warning, the editor, and the actions.
fn acp_json_card(text: &ShellText, cx: &mut App) -> AnyElement {
    let state = AcpPageState::current(cx);
    let mut card = SettingsCard::new("acp-json")
        .title(text.acp_json_title.clone())
        .description(text.acp_json_description.clone());
    card = card.row(SettingsCardRow::body("acp-json-warning").control(
        div()
            .text_sm()
            .text_color(cx.theme().warning)
            .child(text.acp_json_warning.clone()),
    ));
    card = card.row(SettingsCardRow::body("acp-json-editor").control(acp_json_editor(
        SharedString::from(state.json_text.clone()),
    )));
    card = card.row(SettingsCardRow::body("acp-json-actions").control(acp_json_actions(
        text,
        state.json_dirty,
    )));
    div().child(card).into_any_element()
}

/// The multi-line JSON editor (web's `Textarea`).
fn acp_json_editor(value: SharedString) -> DeferredControl<impl FnOnce(&mut Window, &mut App) -> AnyElement>
{
    let key = SharedString::from("card-textarea-acp-json");
    DeferredControl {
        construct: move |window, cx| {
            let state_entity = window.use_keyed_state(key.clone(), cx, {
                let value = value.clone();
                move |window, cx| {
                    let area =
                        cx.new(|cx| TextareaState::new(window, cx).default_value(value.clone()));
                    let _subscription = cx.subscribe(&area, {
                        move |_, area, event: &InputEvent, cx| match event {
                            InputEvent::Change => {
                                let value = area.read(cx).value();
                                AcpPageState::mutate(cx, |state| {
                                    state.json_text = value.to_string();
                                    state.json_dirty = state.json_text != state.json_baseline;
                                });
                            }
                            _ => {}
                        }
                    });
                    AcpTextareaState {
                        area,
                        _subscription,
                    }
                }
            });
            state_entity.update(cx, |state, cx| {
                if state.area.read(cx).value() != value {
                    state.area.update(cx, |area, cx| {
                        area.set_value(value.clone(), window, cx);
                    });
                }
            });
            let state = state_entity.read(cx);
            Textarea::new(&state.area)
                .h(px(240.))
                .w_full()
                .into_any_element()
        },
    }
}

/// The keyed state behind the JSON editor.
struct AcpTextareaState {
    area: Entity<TextareaState>,
    _subscription: Subscription,
}

/// The editor's actions (web's revert and save buttons).
fn acp_json_actions(text: &ShellText, dirty: bool) -> AnyElement {
    let revert = Button::new("acp-json-revert")
        .label(text.acp_action_revert.clone())
        .small()
        .with_variant(ButtonVariant::Secondary)
        .on_click(|_, _, cx| {
            AcpPageState::mutate(cx, |state| {
                state.json_text = state.json_baseline.clone();
                state.json_dirty = false;
            });
        });
    let save = Button::new("acp-json-save")
        .label(text.acp_action_save_json.clone())
        .small()
        .with_variant(ButtonVariant::Primary)
        .disabled(!dirty)
        .on_click(|_, _, cx| {
            let json = AcpPageState::current(cx).json_text;
            if let Some(handle) = crate::runtime::runtime_handle() {
                handle.save_acp_json_config(json);
            }
        });
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(revert)
        .child(save)
        .into_any_element()
}

/// The registry rows: the five presets first, then the configured clients
/// that are not presets (web `registryPresets` + `visibleCustomClientRows`).
fn acp_agent_rows(text: &ShellText, catalog: &AcpCatalog) -> Vec<AcpAgentRow> {
    let mut rows = Vec::new();
    for preset in ACP_PRESETS {
        let probe = catalog.probe(preset.id);
        let client = catalog.clients.iter().find(|client| client.id == preset.id);
        let has_config = client.is_some();
        let enabled = client.map(|client| client.enabled).unwrap_or(false);
        let requires_adapter = probe
            .map(|probe| probe.adapter.is_some())
            .unwrap_or(!preset.native_acp);
        let probe_pending = catalog.probing && probe.is_none();
        let status = acp_status(
            has_config,
            enabled,
            probe.map(|probe| probe.tool.installed),
            probe.and_then(|probe| probe.adapter.as_ref().map(|adapter| adapter.installed)),
            requires_adapter,
            probe_pending,
            probe,
        );
        let issue = acp_issue_kind(probe, requires_adapter);
        let permission_mode = client
            .map(|client| client.permission_mode)
            .unwrap_or(AcpClientPermissionMode::Ask);
        rows.push(AcpAgentRow {
            id: preset.id.to_string(),
            display_name: preset.name.to_string(),
            description: acp_preset_description(text, preset.id),
            status,
            issue,
            requires_adapter,
            has_config,
            enabled,
            self_managed_install: preset.self_managed_install,
            action: acp_row_action(
                status,
                issue,
                requires_adapter,
                has_config,
                preset.self_managed_install,
            ),
            permission_mode,
            detail: acp_status_detail(status, issue, probe, requires_adapter),
        });
    }
    for id in acp_custom_client_ids(catalog) {
        let client = catalog
            .clients
            .iter()
            .find(|client| client.id == id)
            .expect("the id came from the client list");
        let probe = catalog.probe(&id);
        let requires_adapter = probe.map(|probe| probe.adapter.is_some()).unwrap_or(false);
        let probe_pending = catalog.probing && probe.is_none();
        let status = acp_status(
            true,
            client.enabled,
            probe.map(|probe| probe.tool.installed),
            probe.and_then(|probe| probe.adapter.as_ref().map(|adapter| adapter.installed)),
            requires_adapter,
            probe_pending,
            probe,
        );
        let issue = acp_issue_kind(probe, requires_adapter);
        rows.push(AcpAgentRow {
            id: id.clone(),
            display_name: if client.name.is_empty() {
                id.clone()
            } else {
                client.name.clone()
            },
            description: std::iter::once(client.command.clone())
                .chain(client.args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
            status,
            issue,
            requires_adapter,
            has_config: true,
            enabled: client.enabled,
            self_managed_install: false,
            action: acp_row_action(status, issue, requires_adapter, true, false),
            permission_mode: client.permission_mode,
            detail: acp_status_detail(status, issue, probe, requires_adapter),
        });
    }
    rows
}

/// The preset's description (web `presets.*.description`).
fn acp_preset_description(text: &ShellText, preset_id: &str) -> String {
    match preset_id {
        "opencode" => text.acp_preset_opencode.clone(),
        "dsh" => text.acp_preset_dsh.clone(),
        "omp" => text.acp_preset_omp.clone(),
        "claude-code" => text.acp_preset_claude_code.clone(),
        "codex" => text.acp_preset_codex.clone(),
        other => other.to_string(),
    }
}
