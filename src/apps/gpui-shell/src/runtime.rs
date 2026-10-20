//! Embedded OpenBitFun runtime bootstrap for the GPUI shell.
//!
//! Mirrors the CLI composition path (`src/apps/cli`:
//! `initialize_core_services_for_deployment` + `CliRuntimeContext::build`):
//! `DeliveryProfile::Cli`, the Embedded runtime deployment, the reviewed CLI
//! owner-feature closure, and `CoreProductAgentRuntime`. The desktop
//! `src/apps/desktop` api modules are intentionally not imported — they bind
//! Tauri state and cannot be reused here.
//!
//! Core async work runs on a process-lifetime tokio runtime owned by this
//! module; results cross into GPUI through an mpsc channel of [`RuntimeEvent`]
//! so the render path stays synchronous.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use gpui_kit::SharedString;
use openbitfun_agent_runtime::event_router::EventSubscriber as _;
use openbitfun_agent_runtime::sdk::{
    AgentDialogTurnExecution, AgentDialogTurnRequest, AgentRuntime,
    AgentSessionArchiveStateRequest, AgentSessionCompactionRequest, AgentSessionCreateRequest,
    AgentSessionDeleteRequest, AgentSessionForkAtTurnRequest, AgentSessionForkBeforeTurnRequest,
    AgentSessionForkRequest, AgentSessionLineageRequest, AgentSessionLineageSnapshot,
    AgentSessionModelUpdateRequest, AgentSessionRenameRequest, AgentSessionRestoreRequest,
    AgentSessionRollbackToTurnOutcome, AgentSessionRollbackToTurnRequest, AgentSessionUsageRequest,
    AgentTurnCancellationRequest, AgentUserAnswersRequest, PendingUserQuestion, PermissionReply,
    PermissionRequest, PermissionRequestEvent, SessionState, SessionTranscript,
    SessionTranscriptRequest, SessionUsageReport,
};
use openbitfun_agent_runtime::user_questions::{get_user_input_manager, UserInputManager};
use openbitfun_core::agentic::insights::{InsightsReport, InsightsReportMeta, InsightsService};
use openbitfun_core::agentic::memories::db::MemoryDatabase;
use openbitfun_core::agentic::memories::workspace::reset_memory_workspace;
use openbitfun_core::agentic::system::{
    init_agentic_system_for_profile_with_runtime_ownership, select_agentic_system_profile,
};
use openbitfun_core::infrastructure::ai::AIClientFactory;
use openbitfun_core::infrastructure::events::event_system::get_global_event_system;
use openbitfun_core::infrastructure::try_get_path_manager_arc;
use openbitfun_core::product_assembly::{DeliveryProfile, ProductAssembler, ProductAssemblyInput};
use openbitfun_core::product_runtime::{
    build_local_runtime_services, ensure_product_dialog_scheduler, CoreAgentRuntimeCompatibility,
    CoreProductAgentRuntime, CoreProductEventQueueOwner, CoreRuntimeServicesProvider,
};
use openbitfun_core::runtime_ownership::{CoreRuntimeOwnership, RuntimeDeployment};
use openbitfun_core::service::config::{
    get_app_language, get_global_config_service, initialize_global_config, types::GlobalConfig,
    types::MemoriesConfig,
};
use openbitfun_core::service::cron::CronJob;
use openbitfun_core::service::workspace::{
    get_global_workspace_service, set_global_workspace_service, WorkspaceInfo, WorkspaceService,
};
use openbitfun_events::{AgenticEvent, ToolEventData};
use openbitfun_runtime_ports::{
    AgentInputAttachment, AgentSessionListRequest, AgentSessionSummary, AgentSubmissionSource,
    DialogSubmissionPolicy, GitPort, SessionContentSearchRequest, SessionContentSearchResponse,
    SessionSearchHit, TranscriptContent, TranscriptMessage,
};
use openbitfun_services_core::session::SessionStatus;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::transcript;

/// Matches the CLI interactive buffer (`RUNTIME_EVENT_BUFFER`).
const RUNTIME_EVENT_BUFFER: usize = 256;
const ENTRYPOINT: &str = "gpui-shell";
/// Same default agent the CLI startup page resolves from `behavior.default_agent`.
const DEFAULT_AGENT_TYPE: &str = "Standard";
/// Tool card payload previews stay short; full values live in the runtime.
/// Per-process workspace history kept for the picker (the shared catalog holds
/// the desktop/CLI recent list; this only remembers what this shell visited).
const VISITED_WORKSPACES_LIMIT: usize = 12;

static RUNTIME_HANDLE: OnceLock<RuntimeHandle> = OnceLock::new();

/// Event sender parked at bootstrap so management actions fired from settings
/// closures (which hold no runtime handles) can publish catalog refreshes.
static SHELL_EVENT_TX: OnceLock<mpsc::UnboundedSender<RuntimeEvent>> = OnceLock::new();

/// Publishes one event into the shell pump from anywhere in the process
/// (tray thread, settings closures, spawned service tasks). Events fired
/// before the pump is wired are dropped with a warning.
pub(crate) fn publish(event: RuntimeEvent) {
    match SHELL_EVENT_TX.get() {
        Some(tx) => {
            let _ = tx.send(event);
        }
        None => tracing::warn!("Dropping a runtime event before the shell pump is wired"),
    }
}

/// Returns the embedded runtime handle once the bootstrap has finished.
pub(crate) fn runtime_handle() -> Option<&'static RuntimeHandle> {
    RUNTIME_HANDLE.get()
}

/// G-SE12: builds a disposable client for one stored model and probes it.
/// Mirrors the desktop command: stream options plus subscription auth/profile
/// come from the live shared config, so a test never writes the config back.
/// Multimodal models also run the image-input leg and merge both receipts.
async fn test_model_connection_once(
    model_id: &str,
) -> Result<openbitfun_core::util::types::ConnectionTestResult, String> {
    let service = get_global_config_service()
        .await
        .map_err(|error| format!("config service unavailable: {error}"))?;
    let config: GlobalConfig = service
        .get_config(None)
        .await
        .map_err(|error| format!("failed to read configuration: {error}"))?;
    let model_config = config
        .ai
        .models
        .iter()
        .find(|model| model.id == model_id)
        .cloned()
        .ok_or_else(|| format!("model '{model_id}' is not configured"))?;

    let supports_image_input = model_config.capabilities.iter().any(|capability| {
        matches!(
            capability,
            openbitfun_core::service::config::types::ModelCapability::ImageUnderstanding
        )
    }) || matches!(
        model_config.category,
        openbitfun_core::service::config::types::ModelCategory::Multimodal
    );

    let auth = model_config.auth.clone();
    let stream_options = openbitfun_core::infrastructure::ai::build_stream_options_for_model(
        &config.ai,
        Some(&model_config),
    );
    let mut ai_config: openbitfun_core::util::types::AIConfig = model_config
        .try_into()
        .map_err(|error| format!("failed to convert configuration: {error}"))?;
    let skip_ssl_verify = ai_config.skip_ssl_verify;
    let proxy_config = config.ai.proxy.enabled.then(|| config.ai.proxy.clone());
    let subscription_options =
        openbitfun_core::infrastructure::subscription_auth::SubscriptionHttpOptions::new(
            proxy_config.clone(),
            skip_ssl_verify,
        );

    openbitfun_core::infrastructure::ai::client_factory::apply_subscription_auth_with_options(
        &auth,
        &mut ai_config,
        &subscription_options,
    )
    .await
    .map_err(|error| format!("failed to resolve subscription auth: {error}"))?;

    let client =
        openbitfun_core::infrastructure::ai::client_factory::apply_subscription_request_profile(
            &auth,
            openbitfun_core::infrastructure::ai::AIClient::new_with_runtime_options(
                ai_config,
                proxy_config,
                stream_options,
            ),
        );

    let result = client
        .test_connection()
        .await
        .map_err(|error| format!("connection test failed: {error}"))?;
    if !result.success || !supports_image_input {
        return Ok(result);
    }

    let image_result = client
        .test_image_input_connection()
        .await
        .map_err(|error| format!("connection test failed: {error}"))?;
    let response_time_ms = result.response_time_ms + image_result.response_time_ms;
    Ok(openbitfun_core::util::types::ConnectionTestResult {
        success: image_result.success,
        response_time_ms,
        model_response: image_result.model_response.or(result.model_response),
        message_code: if image_result.success {
            result.message_code
        } else {
            image_result.message_code
        },
        error_details: if image_result.success {
            result.error_details
        } else {
            image_result.error_details
        },
    })
}

/// One-way notifications from the embedded runtime into the shell UI.
// Turn ids ride along on turn events for the M5 permission/steering work;
// the M4 view keys everything on session ids and round ids.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum RuntimeEvent {
    /// Shared-configuration snapshot (`app.language`, `appearance.selection`,
    /// tool permissions) read once at bootstrap. Replaces the earlier
    /// locale-only event: the shell applies language, theme, and the settings
    /// page mirror from it.
    Config(Box<crate::settings::ShellConfig>),
    /// Embedded runtime assembled successfully; sessions may be loaded.
    Ready,
    /// Session list snapshot (workspace-scoped, read-only).
    Sessions(Vec<SessionListRow>),
    /// Reload failed while the runtime itself stays usable.
    SessionsFailed(String),
    /// Per-root session lineage snapshots (R1c): the subagent tree under each
    /// listed session, fetched alongside the list. A root whose fetch failed
    /// simply has no snapshot and renders without children.
    SessionLineages(Vec<AgentSessionLineageSnapshot>),
    /// Bootstrap failed; the shell stays degraded with the error shown.
    Failed(String),
    /// A session was created or restored and its transcript is ready.
    SessionOpened(OpenedSession),
    /// Opening (creating or restoring) a session failed.
    SessionOpenFailed(String),
    /// A submitted dialog turn was accepted by the runtime; the shell shows
    /// the submitted message on this event (the composer clears on click).
    TurnAccepted {
        session_id: String,
        turn_id: String,
        message: String,
    },
    /// Submitting a message failed; the turn never started.
    SendFailed {
        session_id: String,
        error: String,
    },
    /// A dialog turn started executing (busy marker for the open session).
    TurnStarted {
        session_id: String,
        turn_id: String,
    },
    /// Streaming assistant text for the open session.
    TextChunk {
        session_id: String,
        turn_id: String,
        round_id: String,
        text: String,
    },
    /// Tool lifecycle update; upserted by the tool id in the open session.
    ToolUpdate {
        session_id: String,
        card: ToolCard,
    },
    /// A dialog turn reached a terminal state.
    TurnFinished {
        session_id: String,
        error: Option<String>,
        /// G-S1: the runtime-reported turn duration (`DialogTurnCompleted`);
        /// the failure and cancel paths have none.
        duration_ms: Option<u64>,
        /// G-S1: the user stopped the turn; the footer meta says `Stopped`
        /// instead of `Completed` (web `ModelRoundItem` status).
        cancelled: bool,
    },
    /// The authoritative history changed (undo/restore/repair); reload it.
    HistoryChanged {
        session_id: String,
    },
    /// A tool asked for permission (process-local `PermissionRequestManager`
    /// broadcast). The turn parks until the shell answers it through
    /// [`RuntimeHandle::respond_permission`].
    PermissionAsked {
        request: PermissionRequest,
    },
    /// A pending request was answered or cancelled; `request_id` drops out of
    /// the shell's pending queue. This includes echoes of the shell's own
    /// replies, which keeps the queue a pure projection of the runtime state.
    PermissionResolved {
        request_id: String,
    },
    /// Full pending-request snapshot replacing the queue, sent when the
    /// permission broadcast lags so no ask can be stranded unanswered.
    PermissionSnapshot(Vec<PermissionRequest>),
    /// A session rename was applied by the runtime; adopt the new name.
    SessionRenamed {
        session_id: String,
        name: String,
    },
    /// A fork of an existing session was created; the runtime opens it right
    /// away, so the chat pane switches to the fork when its transcript
    /// arrives through [`RuntimeEvent::SessionOpened`].
    SessionForked {
        new_session_id: String,
        new_session_name: String,
    },
    /// A manual context-compaction maintenance turn was accepted for the open
    /// session; completion flows through the normal turn-settlement events.
    CompactionStarted {
        session_id: String,
    },
    /// A transcript export was written to `path`.
    ExportFinished {
        session_id: String,
        path: String,
    },
    /// Session usage report for the usage dialog.
    SessionUsage(Box<SessionUsageSummary>),
    /// Insight report metadata (latest first) for the insights page.
    InsightsLatest(Vec<InsightsReportMeta>),
    /// One full insight report (generated or loaded by path).
    InsightsReport(Box<InsightsReport>),
    /// An insights action failed; the page shows the reason inline.
    InsightsFailed(String),
    /// A generate call was dispatched; the page shows its in-flight state.
    InsightsGenerating,
    /// Scheduled-jobs snapshot for the tasks page.
    CronJobs(Vec<CronJob>),
    /// A scheduled-jobs action failed; the page shows the reason inline.
    CronFailed(String),
    /// Files-tab results for the global search page (M16).
    SearchContent {
        hits: Vec<openbitfun_core::service::search::WorkspaceSearchHit>,
        truncated: bool,
    },
    /// Sessions-tab results for the global search page (M16).
    SearchSessions {
        hits: Vec<SessionSearchHit>,
        truncated: bool,
    },
    /// Files search cannot run in this host: the payload is the managed
    /// daemon's install hint when that is the gate that failed, `None` for
    /// the config gate.
    SearchUnavailable(Option<String>),
    /// A search action failed; the page shows the reason inline.
    SearchFailed(String),
    /// A session was deleted; the chat pane must close when it shows it.
    SessionDeleted {
        session_id: String,
    },
    /// A session action (rename / archive / delete / model change) failed;
    /// surfaced as a shell notice line, never silently dropped.
    SessionActionFailed {
        message: String,
    },
    /// A hidden subagent session was linked to the parent tool call that
    /// launched it (M20). The parent's tool card grows the "view run" entry
    /// point; the child opens directly as the detail view.
    SubagentLinked {
        parent_session_id: String,
        parent_tool_call_id: String,
        subagent_session_id: String,
        agent_type: Option<String>,
    },
    /// A `/btw` side-question turn started (M20); the child session opens as
    /// the detail view with a return path to the parent.
    BtwStarted {
        parent_session_id: String,
        child_session_id: String,
        /// The coordinator dialog turn, kept so the pane's stop button can
        /// cancel the turn it belongs to (desktop side_question semantics).
        turn_id: String,
    },
    /// A session's model selector was updated successfully.
    SessionModelChanged {
        session_id: String,
        model_id: String,
    },
    /// Configured-model catalog for the pickers (session header + models
    /// settings page); sent once after bootstrap.
    ModelCatalog(Vec<ModelChoice>),
    /// One model's connection test finished (G-SE12); the models page shows
    /// the receipt inline. A setup failure is reported as `Err` so the row
    /// keeps a usable message instead of losing the test.
    ModelTestFinished {
        model_id: String,
        result: Result<openbitfun_core::util::types::ConnectionTestResult, String>,
    },
    /// G-SE25: the Hook-import projection for the automation page (imports,
    /// catalog, corrupt-store diagnostics and the revision mutations expect).
    HookImports(
        Box<openbitfun_product_domains::external_hook_import::ExternalHookImportSnapshotV1>,
    ),
    /// G-SE25: a review plan for one import source, shown before applying.
    HookImportPlan(Box<openbitfun_product_domains::external_hook_import::ExternalHookImportPlanV1>),
    /// G-SE25: the reviewed plan went stale because the source changed; the
    /// page swaps in the refreshed plan instead of applying the old one.
    HookImportPlanStale(
        Box<openbitfun_product_domains::external_hook_import::ExternalHookImportPlanV1>,
    ),
    /// G-SE25: a Hook-import action failed; the page shows the reason inline.
    HookImportFailed(String),
    /// Workspaces the shell can switch to: the shared core catalog (recent
    /// first, then the rest of the opened set) plus workspaces visited by
    /// this process, with the active one always present.
    Workspaces(Vec<WorkspaceChoice>),
    /// One other workspace's session list for the sidebar groups (web
    /// SessionsSection): one event per recent workspace that answered.
    OtherWorkspaceSessions {
        name: String,
        root_path: String,
        rows: Vec<SessionListRow>,
    },
    /// The archived-sessions projection for the settings page (G-SE30): every
    /// recent workspace's archived rows, one group each. The read answers as a
    /// whole so the page's loading state has a single boundary; ordering is
    /// the page's job (web sorts all entries by `lastActiveAt` descending).
    ArchivedSessions(Vec<ArchivedWorkspaceGroup>),
    /// An archive-state change or a delete landed for some workspace; the
    /// archived page reloads its projection while it is open.
    ArchivedChanged,
    /// The active workspace changed; workspace-scoped surfaces reset and the
    /// list reloads for the new root.
    WorkspaceSwitched(WorkspaceChoice),
    /// A switch was rejected (unreadable path, another runtime owning the
    /// workspace, service assembly failure); the previous workspace stays
    /// active and the reason is surfaced.
    WorkspaceSwitchFailed(String),
    /// Full pending-user-question snapshot from the process-local mailbox
    /// (`ask_user` tool). The mailbox publishes only a coalesced revision, so
    /// the shell rebuilds its queue from this on every change: additions,
    /// answers, cancellations, timeouts, and turn cancellations all arrive
    /// through this one path, and questions that predate the subscription
    /// still surface.
    QuestionsChanged(Vec<PendingUserQuestion>),
    /// M21 management catalogs for the settings pages (skills, agents, MCP
    /// servers). Each replaces the corresponding gpui global wholesale.
    SkillCatalog(crate::manage::SkillCatalog),
    /// G-K3: the add-skill source path passed the filesystem/SKILL.md check.
    SkillValidationFinished(Result<crate::manage::SkillValidationRow, String>),
    /// G-K3: an add/delete write settled; the success arm also refreshes the
    /// catalog.
    SkillAdded(Result<String, String>),
    SkillDeleted(Result<String, String>),
    AgentCatalog(crate::manage::AgentCatalog),
    McpCatalog(crate::manage::McpCatalog),
    /// T2-11: the skill selection projection for one agent (mode profile
    /// representative or subagent id); the dialog renders it per phase.
    ModeSkillInfos {
        mode_id: String,
        rows: Vec<crate::manage::ModeSkillRow>,
    },
    /// T2-12: the custom-agent definition for the edit dialog prefill.
    AgentDetail(Result<crate::manage::AgentDetailRow, String>),
    /// Resources/prompts snapshot for one MCP server (browse dialog).
    McpServerCatalog(Box<crate::manage::McpServerCatalogView>),
    /// An MCP server asked for an interactive decision (elicitation or
    /// sampling); the turn parks until the shell answers through
    /// [`RuntimeHandle::submit_mcp_interaction`].
    McpInteractionAsked(Box<crate::manage::McpInteractionRequest>),
    /// T2-7: the user JSON config snapshot (Cursor format) for the editor
    /// dialog; the fingerprint guards concurrent edits.
    McpJsonConfig(Result<crate::manage::McpJsonSnapshot, String>),
    /// T2-7: a JSON config save finished; `Ok` means the file landed and the
    /// running servers were reconciled.
    McpJsonSaved(Result<(), String>),
    /// G-M2: one typed import path's file content, for the paste box.
    McpImportFile(Result<String, String>),
    /// T2-7: one resources/read call finished for the browse dialog.
    McpResourceRead(Box<crate::manage::McpResourceReadOutcome>),
    /// A management action (agent creation, MCP lifecycle) failed; the
    /// settings pages surface it as a dismissible banner.
    ManageFailed(String),
    /// G-A3: a subagent model-override write failed (its own banner line so
    /// the page can name the operation, web `modelSelector.updateFailed`).
    AgentModelFailed(String),
    /// G-Y4: one trinity memory transfer finished (web T10 RPCs).
    TrinityTransfer(Box<crate::trinity::MemoryTransferOutcome>),
    /// M22 tray: show (and focus) the main window. Left click or the tray
    /// menu; there is no per-window hide in gpui, so this is activate-only.
    TrayShowWindow,
    /// The window close button or the tray asked to quit. The pump shuts the
    /// terminal PTYs down before the process exits — closing the window
    /// directly would orphan them.
    QuitRequested,
    /// Restart after a successful self-update: shut PTYs down, hand off to
    /// the freshly installed binary, and exit.
    RestartRequested,
    /// An update check was requested (settings button or tray menu).
    UpdateCheckRequested,
    /// A download+install of the offered release was requested (settings
    /// button).
    UpdateInstallRequested,
    /// An update check finished (manual or the startup auto-check).
    UpdateCheckFinished {
        result: Result<crate::update::UpdateCheckReport, String>,
    },
    /// A self-update download+install finished; `Ok` carries the installed
    /// version. The running process keeps the old image until it restarts.
    UpdateInstallFinished {
        result: Result<String, String>,
    },
    /// Launch-at-login state read from the platform backend at bootstrap.
    AutostartState(bool),
    /// Global token-usage statistics for the settings usage page.
    UsageReady(Result<Box<openbitfun_services_core::token_usage::UsageStatistics>, String>),
    /// T2-8 activity heatmap snapshot: the fixed six-month custom-range
    /// window at day granularity, same service as [`RuntimeEvent::UsageReady`].
    UsageActivityReady(Result<Box<openbitfun_services_core::token_usage::UsageStatistics>, String>),
    /// Web-search credential state for one provider (`provider`, `configured`).
    WebSearchStatus(Result<(String, bool), String>),
    /// T2-16 tray mark-all-read: clears every session's unread turn count.
    TrayMarkAllRead,
    /// T2-15 Trinity console: identity + live PSI state + the cognition
    /// trend, fetched together so the page flips atomically.
    TrinityStatusReady {
        psi: Box<crate::trinity::PsiState>,
        status: Box<crate::trinity::StatusInfo>,
        history: Vec<f64>,
    },
    /// The daemon is unreachable (or the call failed); the console shows the
    /// offline state with the reason.
    TrinityStatusFailed(String),
    /// One memory-timeline page; `append` marks a `before_ts` continuation
    /// of the existing list (a first page or a post-mutation reload replaces).
    TrinityMemoriesReady {
        items: Vec<crate::trinity::MemoryEntry>,
        exhausted: bool,
        append: bool,
    },
    /// The timeline load failed; the page keeps the previous list.
    TrinityMemoriesFailed(String),
    /// Recall results for the current search query.
    TrinityRecallReady(Vec<crate::trinity::MemoryEntry>),
    /// A memory mutation (memorize / forget / reinforce) or the recall
    /// failed; surfaced as the page's inline action error.
    TrinityActionFailed(String),
    /// G-Y5: the daemon's verdict on a correction (`memorize` with a
    /// `target_id`). `false` means the original memory no longer existed.
    TrinityCorrection {
        applied: bool,
    },
    /// G-SE14 engine page: the daemon's current LLM configuration
    /// (`llm.get_config`).
    TrinityEngineConfigReady {
        config: Box<crate::trinity::EngineLlmConfig>,
    },
    /// G-SE14: the engine configuration could not be read (daemon
    /// unreachable); the page shows the reason instead of the fields.
    TrinityEngineConfigFailed(String),
    /// G-SE14: `llm.set_config` settled and the daemon hot-swapped it.
    TrinityEngineSaved,
    /// G-SE14: the save failed; surfaced as the page's inline notice.
    TrinityEngineSaveFailed(String),
    /// G-SE14: `llm.test_connection` settled; `error` carries the daemon's
    /// reason on failure (the shell composes the display line).
    TrinityEngineTested {
        ok: bool,
        error: Option<String>,
    },
    /// G-SE13 memory page: the `memories` settings document was restored to
    /// its defaults; `Ok` carries the re-read section so the page repaints
    /// from stored facts rather than from what it just wrote.
    MemorySettingsReset(Result<Box<MemoriesConfig>, String>),
    /// G-SE13: the memory database and the memory workspace files were
    /// cleared (`Ok(())` on success).
    MemoryCleared(Result<(), String>),
    /// R7c: the plugin UI host channel is live and its initial describe
    /// landed; the named providers install into the projection now (each
    /// entry is the source key plus its provider).
    PluginUiBridgeReady {
        providers: Vec<PluginUiProviderInstall>,
    },
    /// R7c: one dispatched interaction settled on the host channel; the
    /// shell clears the scene's in-flight state and applies the outcome.
    PluginUiSceneUpdate {
        update: openbitfun_core::plugin_ui_host::PluginUiSceneUpdate,
    },
    /// R7c: the plugin UI host channel failed to open or its initial
    /// describe failed; no contributions install from it and the reason is
    /// logged (the test channel surfaces it in the smoke output).
    PluginUiBridgeFailed(String),
    /// R7c: the plugin UI test channel shut down; its installed providers
    /// retract and open tabs fall into the contribution-removed state.
    PluginUiChannelClosed,
}

/// One session list row: the runtime summary plus its persisted archive state.
/// The summary DTO carries no archive flag, so the bootstrap-side metadata read
/// supplies it (the sidebar filters archived sessions out by default).
#[derive(Debug)]
pub(crate) struct SessionListRow {
    pub summary: AgentSessionSummary,
    pub archived: bool,
}

/// One workspace's archived sessions for the settings archived page (G-SE30).
/// The page groups by workspace, so each workspace keeps its own read result:
/// `Err` is that workspace's load failure (web
/// `loadArchivedFailedForWorkspace`), never a page-level error.
#[derive(Debug)]
pub(crate) struct ArchivedWorkspaceGroup {
    pub workspace_id: String,
    pub name: String,
    pub root_path: String,
    pub rows: Result<Vec<ArchivedSessionRow>, String>,
}

/// One archived session row: what the page renders plus the workspace id the
/// per-workspace restore/delete calls need.
#[derive(Debug, Clone)]
pub(crate) struct ArchivedSessionRow {
    pub session_id: String,
    pub name: String,
    /// Unix ms; the page sorts groups and rows by it (the web collects every
    /// workspace's entries and sorts by `lastActiveAt` descending before
    /// grouping).
    pub last_active_at: u64,
}

/// R7c: one host-channel provider ready to install into the shell projection.
/// The trait object has no `Debug`, so the event enum derives around it.
pub(crate) struct PluginUiProviderInstall {
    pub source_key: String,
    pub provider: std::sync::Arc<dyn openbitfun_core::plugin_ui::PluginUiProvider>,
}

impl std::fmt::Debug for PluginUiProviderInstall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginUiProviderInstall")
            .field("source_key", &self.source_key)
            .finish_non_exhaustive()
    }
}

/// Dialog-ready projection of a session usage report: the headline numbers
/// the shell shows, without the full report schema.
#[derive(Debug, Clone)]
pub(crate) struct SessionUsageSummary {
    pub session_id: String,
    pub turn_count: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub wall_time_ms: Option<u64>,
    pub active_turn_ms: Option<u64>,
    pub models: Vec<UsageModelLine>,
}

/// One model's usage line in the usage dialog.
#[derive(Debug, Clone)]
pub(crate) struct UsageModelLine {
    pub model_id: String,
    pub calls: u64,
    pub total_tokens: Option<u64>,
}

impl SessionUsageSummary {
    fn from_report(session_id: String, report: SessionUsageReport) -> Self {
        Self {
            session_id,
            turn_count: report.scope.turn_count,
            input_tokens: report.tokens.input_tokens,
            output_tokens: report.tokens.output_tokens,
            total_tokens: report.tokens.total_tokens,
            cached_tokens: report.tokens.cached_tokens,
            wall_time_ms: report.time.wall_time_ms,
            active_turn_ms: report.time.active_turn_ms,
            models: report
                .models
                .iter()
                .map(|model| UsageModelLine {
                    model_id: model.model_id.clone(),
                    calls: model.call_count,
                    total_tokens: model.total_tokens,
                })
                .collect(),
        }
    }
}

/// One model from the shared catalog, pre-shaped for the pickers.
#[derive(Debug, Clone)]
pub(crate) struct ModelChoice {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub enabled: bool,
}

impl ModelChoice {
    /// Display label: model name plus its provider, so duplicates across
    /// providers stay distinguishable.
    pub(crate) fn label(&self) -> String {
        if self.provider.is_empty() {
            self.name.clone()
        } else {
            format!("{} · {}", self.name, self.provider)
        }
    }
}

/// One workspace the shell can switch to, pre-shaped for the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceChoice {
    pub id: String,
    pub name: String,
    pub root_path: String,
}

impl WorkspaceChoice {
    fn from_info(info: &WorkspaceInfo) -> Self {
        Self {
            id: info.id.clone(),
            name: info.name.clone(),
            root_path: info.root_path.to_string_lossy().to_string(),
        }
    }
}

/// Configured-model catalog as a gpui global: written once from
/// [`RuntimeEvent::ModelCatalog`], read by the chat-header picker and the
/// models settings page.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelCatalog {
    pub loaded: bool,
    pub models: Vec<ModelChoice>,
}

impl gpui_kit::Global for ModelCatalog {}

/// A session ready to be shown in the chat pane.
#[derive(Debug)]
pub(crate) struct OpenedSession {
    pub session_id: String,
    pub name: String,
    pub agent_type: String,
    /// The session's model selector, when the runtime recorded one.
    pub model_id: Option<String>,
    pub entries: Vec<ChatEntry>,
    /// G-S1: per-turn wall-clock timing read from the transcript's message
    /// timestamps, so a historical turn's footer can show its completion time
    /// and duration (live turns extend it through the turn events).
    pub turn_times: HashMap<String, TurnTimes>,
    /// The transcript was re-read for an already-open pane (turn settlement
    /// refreshed it) rather than opened by the user. A reload is not an open:
    /// it must not count as "reading it" for the unread-turn badge, so the
    /// shell keeps a session's unread completions across refreshes.
    pub reload: bool,
    /// The session was mid-turn when opened; live events continue to arrive.
    pub busy: bool,
}

/// One turn's wall-clock timing for the transcript footer (G-S1, web
/// `turnStartedAt` / `turnEndedAt` / `turnDurationMs`). A transcript read
/// fills both ends from the persisted message timestamps; a live turn fills
/// the start when it starts, and the end plus the runtime's authoritative
/// duration when it settles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TurnTimes {
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    /// Runtime-reported duration (`DialogTurnCompleted`).
    pub duration_ms: Option<u64>,
    /// The user stopped the turn (live path only; a transcript read cannot
    /// tell a stop from a success). The footer says `Stopped` for these.
    pub cancelled: bool,
}

impl TurnTimes {
    /// The duration the footer shows: the runtime's number when it reported
    /// one, else the wall-clock span between the two recorded ends (the web
    /// falls back the same way).
    pub(crate) fn effective_duration_ms(&self) -> Option<u64> {
        self.duration_ms
            .or_else(|| match (self.started_at_ms, self.ended_at_ms) {
                (Some(start), Some(end)) => Some(end.saturating_sub(start)),
                _ => None,
            })
    }

    /// Whether the footer can show anything at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.started_at_ms.is_none() && self.ended_at_ms.is_none() && self.duration_ms.is_none()
    }
}

/// Chat transcript entry as rendered by the shell.
#[derive(Debug, Clone)]
pub(crate) enum ChatEntry {
    /// A submitted user prompt. `turn_id` is the persisted turn identity the
    /// entry belongs to (turn echo or transcript read), and is the fork
    /// boundary for the per-turn fork menu.
    User {
        text: String,
        turn_id: Option<String>,
    },
    /// Assistant markdown (streamed chunks accumulate into one entry per
    /// model round, keyed by `round_id`).
    Assistant {
        round_id: String,
        text: String,
    },
    Tool(ToolCard),
    /// Turn-level failures, cancellations, and system errors.
    Notice {
        text: String,
    },
    /// Non-error status lines (compaction started, export finished).
    Info {
        text: String,
    },
}

/// Tool invocation card state; `key` doubles as the stable element id.
#[derive(Debug, Clone)]
pub(crate) struct ToolCard {
    pub key: SharedString,
    /// The runtime's tool call id: the join key for the subagent-link map and
    /// the background-task pairing (the `key` adds the `tool-` prefix).
    pub call_id: String,
    pub tool_name: String,
    pub status: ToolStatus,
    /// Display facts parsed once from the call's input (and result) — see
    /// [`crate::transcript`]; the row renderer never sees raw JSON.
    pub facts: crate::transcript::ToolFacts,
    /// The result reported `success: false`, when it reported anything at all
    /// (web `toolResult.success`); grouping excludes such calls.
    pub result_success: Option<bool>,
    /// Hidden subagent session linked to this tool call (M20): set when the
    /// runtime's `SubagentSessionLinked` event arrives, and the chat renders
    /// the "view run" entry point on the card.
    pub subagent_session: Option<String>,
    /// The tool started with a `run_in_background` input (T2-6, the web
    /// backgroundSubagentActivity signal): only the `Started` event carries
    /// params, so this is the one chance to record it.
    pub background: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolStatus {
    Running,
    WaitingConfirm,
    Done,
    Failed,
    Cancelled,
}

/// Process-lifetime handle to the embedded runtime, safe to hold from UI code.
///
/// The process-wide Agent Runtime (coordinator, event queue, tool registry)
/// is assembled once and cannot be rebuilt in-process — core registers the
/// coordinator in a `OnceLock` and skips a second `set_global`. The active
/// workspace is therefore a swappable fact of this handle: every request the
/// shell sends carries the active workspace id/path, and core resolves
/// per-turn workspace services from that binding (the same shape the desktop
/// uses when it switches its active workspace).
pub(crate) struct RuntimeHandle {
    agent_runtime: AgentRuntime,
    /// Coordinator-backed compatibility owner (M11): persistence-level session
    /// metadata, which the runtime ports do not expose (archive state, for
    /// example, is absent from `AgentSessionSummary`).
    compat: CoreAgentRuntimeCompatibility,
    /// Workspace the shell's requests are addressed to. Shared with the
    /// short-lived action clones so a switch is visible to in-flight reloads.
    workspace: Arc<RwLock<WorkspaceInfo>>,
    /// Workspaces visited in this process, newest first. Core keeps the
    /// shared catalog (desktop recent/opened); this is the per-process extra
    /// so a workspace opened by path stays reachable from the picker without
    /// touching the desktop's selection.
    visited: Arc<RwLock<Vec<WorkspaceInfo>>>,
    /// Process ownership guard: acquiring another local workspace lease is the
    /// same check the runtime performs before a turn, done here so a switch
    /// fails loudly instead of on the first turn.
    ownership: Arc<CoreRuntimeOwnership>,
    tokio: tokio::runtime::Handle,
    /// Workspace-level git port from the local runtime services (M8 changes
    /// view); `None` when the assembly did not register one. Rebuilt for the
    /// active workspace on every switch.
    git: Arc<RwLock<Option<Arc<dyn GitPort>>>>,
    // Keeps the runtime event source attached for the process lifetime.
    _event_queue_owner: CoreProductEventQueueOwner,
}

use std::sync::RwLock;

impl RuntimeHandle {
    /// Active workspace record (canonical root).
    pub(crate) fn workspace(&self) -> WorkspaceInfo {
        self.workspace
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Process-lifetime tokio handle for spawning service-side tasks (the
    /// terminal pane's PTY pump and writes).
    pub(crate) fn tokio_handle(&self) -> tokio::runtime::Handle {
        self.tokio.clone()
    }

    /// Workspace-level git port (M8 changes view) for the active workspace,
    /// when the assembly registered one.
    pub(crate) fn git(&self) -> Option<Arc<dyn GitPort>> {
        self.git
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Requests a fresh session snapshot; the result arrives as a
    /// [`RuntimeEvent::Sessions`] (or [`RuntimeEvent::SessionsFailed`]).
    /// Archive state comes from the persisted metadata in the same pass, so
    /// the sidebar can hide archived sessions instead of inventing a local
    /// flag that the desktop side would not see.
    pub(crate) fn reload_sessions(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        spawn_session_reload(
            self.agent_runtime.clone(),
            self.compat.clone(),
            self.workspace.clone(),
            self.tokio.clone(),
            tx,
        );
    }

    /// Sidebar session groups beyond the current workspace (web
    /// SessionsSection): one list request per recent workspace, each answering
    /// through [`RuntimeEvent::OtherWorkspaceSessions`]. A workspace that
    /// fails (deleted root, unreachable remote) simply sends no group — the
    /// sidebar renders the workspaces it can see, and the failure lands in
    /// the log rather than a broken row.
    pub(crate) fn load_other_workspace_sessions(
        &self,
        choices: Vec<WorkspaceChoice>,
        current_workspace_id: &str,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        for choice in choices {
            if choice.id == current_workspace_id {
                continue;
            }
            let runtime = self.agent_runtime.clone();
            let compat = self.compat.clone();
            let tx = tx.clone();
            let name = choice.name.clone();
            let root_path = choice.root_path.clone();
            let workspace_id = choice.id.clone();
            let root = std::path::PathBuf::from(&choice.root_path);
            self.tokio.spawn(async move {
                let request = AgentSessionListRequest {
                    workspace_id: Some(workspace_id),
                    workspace_path: String::new(),
                    remote_connection_id: None,
                    remote_ssh_host: None,
                };
                let archived = archived_session_ids(&compat, &root).await;
                match runtime.list_sessions(request).await {
                    Ok(mut sessions) => {
                        if sessions.is_empty() {
                            return;
                        }
                        sort_sessions(&mut sessions);
                        let rows = sessions
                            .into_iter()
                            .map(|summary| SessionListRow {
                                archived: archived.contains(&summary.session_id),
                                summary,
                            })
                            .collect::<Vec<_>>();
                        let _ = tx.send(RuntimeEvent::OtherWorkspaceSessions {
                            name,
                            root_path,
                            rows,
                        });
                    }
                    Err(error) => {
                        tracing::warn!(
                            "Sidebar group skipped for workspace {root_path}: {}",
                            error.into_message()
                        );
                    }
                }
            });
        }
    }

    /// The settings archived page's cross-workspace read (G-SE30): one
    /// metadata read per recent workspace, archived rows only. The page is the
    /// only consumer and asks on open and after every mutation, so this stays
    /// a page-scoped projection rather than a second live list.
    pub(crate) fn load_archived_sessions(
        &self,
        choices: Vec<WorkspaceChoice>,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let compat = self.compat.clone();
        self.tokio.spawn(async move {
            let mut groups = Vec::with_capacity(choices.len());
            for choice in choices {
                let root = std::path::PathBuf::from(&choice.root_path);
                let rows = match compat.list_persisted_sessions(&root).await {
                    Ok(metadatas) => Ok(metadatas
                        .into_iter()
                        .filter(|metadata| matches!(metadata.status, SessionStatus::Archived))
                        .map(|metadata| ArchivedSessionRow {
                            session_id: metadata.session_id,
                            name: metadata.session_name,
                            last_active_at: metadata.last_active_at,
                        })
                        .collect::<Vec<_>>()),
                    Err(error) => {
                        tracing::warn!(
                            "Archived sessions for {} could not be read: {error}",
                            choice.root_path
                        );
                        Err(error.to_string())
                    }
                };
                groups.push(ArchivedWorkspaceGroup {
                    workspace_id: choice.id,
                    name: choice.name,
                    root_path: choice.root_path,
                    rows,
                });
            }
            let _ = tx.send(RuntimeEvent::ArchivedSessions(groups));
        });
    }

    /// Renames one session; a successful call reports
    /// [`RuntimeEvent::SessionRenamed`] and refreshes the list.
    pub(crate) fn rename_session(
        &self,
        session_id: String,
        name: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionRenameRequest {
                workspace_id,
                workspace_path: String::new(),
                session_id: session_id.clone(),
                session_name: name.clone(),
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.rename_session(request).await {
                Ok(()) => {
                    tracing::info!("Renamed session {session_id}");
                    let _ = tx.send(RuntimeEvent::SessionRenamed { session_id, name });
                    handle.reload_sessions(tx);
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to rename session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("rename: {message}"),
                    });
                }
            }
        });
    }

    /// Sets one session's persisted archive state; refreshes the list so the
    /// row appears or disappears according to the same source the desktop
    /// reads.
    pub(crate) fn set_session_archived(
        &self,
        session_id: String,
        archived: bool,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.set_session_archived_in(self.workspace().id, session_id, archived, tx);
    }

    /// Same as [`Self::set_session_archived`] but scoped to an explicit
    /// workspace: the archived settings page acts on rows from every recent
    /// workspace (web `sessionAPI.unarchiveSession(id, workspaceId)`), and the
    /// archive state lives in the owning workspace's store. Only the active
    /// workspace's session list reloads; the archived projection reloads
    /// through [`RuntimeEvent::ArchivedChanged`] either way.
    pub(crate) fn set_session_archived_in(
        &self,
        workspace_id: String,
        session_id: String,
        archived: bool,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let active_id = self.workspace().id;
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionArchiveStateRequest {
                workspace_id: Some(workspace_id.clone()),
                workspace_path: String::new(),
                session_id: session_id.clone(),
                archived,
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.set_session_archived(request).await {
                Ok(()) => {
                    tracing::info!("Session {session_id} archived={archived}");
                    let _ = tx.send(RuntimeEvent::ArchivedChanged);
                    if workspace_id == active_id {
                        handle.reload_sessions(tx);
                    }
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!(
                        "Failed to set archive state for session {session_id}: {message}"
                    );
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("archive: {message}"),
                    });
                }
            }
        });
    }

    /// Deletes one session and reports it so the chat pane can close.
    pub(crate) fn delete_session(
        &self,
        session_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.delete_session_in(self.workspace().id, session_id, tx);
    }

    /// Deletes one session of an explicit workspace (the archived page's rows
    /// come from every recent workspace); the active workspace's list reloads,
    /// the archived projection rides [`RuntimeEvent::ArchivedChanged`].
    pub(crate) fn delete_session_in(
        &self,
        workspace_id: String,
        session_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let active_id = self.workspace().id;
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionDeleteRequest {
                workspace_id: Some(workspace_id.clone()),
                workspace_path: String::new(),
                session_id: session_id.clone(),
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.delete_session(request).await {
                Ok(()) => {
                    tracing::info!("Deleted session {session_id}");
                    let _ = tx.send(RuntimeEvent::SessionDeleted {
                        session_id: session_id.clone(),
                    });
                    let _ = tx.send(RuntimeEvent::ArchivedChanged);
                    if workspace_id == active_id {
                        handle.reload_sessions(tx);
                    }
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to delete session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("delete: {message}"),
                    });
                }
            }
        });
    }

    /// Forks a session at its latest persisted turn and opens the fork. The
    /// copy shares no mutable history with the source, so the source session
    /// keeps running untouched.
    pub(crate) fn fork_session(&self, session_id: String, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionForkRequest {
                workspace_id,
                workspace_path: String::new(),
                source_session_id: session_id.clone(),
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.fork_session(request).await {
                Ok(forked) => {
                    tracing::info!(
                        "Forked session {session_id} -> {} ({})",
                        forked.session_id,
                        forked.session_name
                    );
                    let _ = tx.send(RuntimeEvent::SessionForked {
                        new_session_id: forked.session_id.clone(),
                        new_session_name: forked.session_name,
                    });
                    handle.reload_sessions(tx.clone());
                    handle.open_session(forked.session_id, tx);
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to fork session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("fork: {message}"),
                    });
                }
            }
        });
    }

    /// Forks a session at an explicit turn boundary (`before` skips the
    /// selected turn, otherwise it is included) and opens the fork.
    pub(crate) fn fork_session_turn(
        &self,
        session_id: String,
        turn_id: String,
        before: bool,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let forked = if before {
                let request = AgentSessionForkBeforeTurnRequest {
                    workspace_id,
                    workspace_path: String::new(),
                    source_session_id: session_id.clone(),
                    source_turn_id: turn_id.clone(),
                    remote_connection_id: None,
                    remote_ssh_host: None,
                };
                runtime.fork_session_before_turn(request).await
            } else {
                let request = AgentSessionForkAtTurnRequest {
                    workspace_id,
                    workspace_path: String::new(),
                    source_session_id: session_id.clone(),
                    source_turn_id: turn_id.clone(),
                    remote_connection_id: None,
                    remote_ssh_host: None,
                };
                runtime.fork_session_at_turn(request).await
            };
            match forked {
                Ok(forked) => {
                    tracing::info!(
                        "Forked session {session_id} at turn {turn_id} (before={before}) -> {}",
                        forked.session_id
                    );
                    let _ = tx.send(RuntimeEvent::SessionForked {
                        new_session_id: forked.session_id.clone(),
                        new_session_name: forked.session_name,
                    });
                    handle.reload_sessions(tx.clone());
                    handle.open_session(forked.session_id, tx);
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!(
                        "Failed to fork session {session_id} at turn {turn_id}: {message}"
                    );
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("fork: {message}"),
                    });
                }
            }
        });
    }

    /// Starts a `/btw` side-question child turn for the session (M20): the
    /// coordinator forks the parent context into a hidden child session and
    /// runs the question there. The child never appears in the session list;
    /// the shell opens it directly as the detail view when `BtwStarted`
    /// confirms the turn.
    pub(crate) fn start_btw(
        &self,
        session_id: String,
        question: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            let Some(coordinator) =
                openbitfun_core::agentic::coordination::get_global_coordinator()
            else {
                tracing::error!("Coordinator not initialized; /btw unavailable");
                let _ = tx.send(RuntimeEvent::SendFailed {
                    session_id: session_id.clone(),
                    error: "coordinator not ready".to_string(),
                });
                return;
            };
            let request_id = uuid::Uuid::new_v4().to_string();
            let child_session_id = format!("btw-{}", uuid::Uuid::new_v4().simple());
            match coordinator
                .start_btw_turn(
                    &request_id,
                    &session_id,
                    &child_session_id,
                    None,
                    &question,
                    DialogSubmissionPolicy::for_source(AgentSubmissionSource::Cli),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await
            {
                Ok(turn_id) => {
                    tracing::info!("Started /btw turn for {session_id} -> {child_session_id}");
                    let _ = tx.send(RuntimeEvent::BtwStarted {
                        parent_session_id: session_id,
                        child_session_id,
                        turn_id,
                    });
                }
                Err(error) => {
                    tracing::error!("Failed to start /btw turn: {}", error);
                    let _ = tx.send(RuntimeEvent::SendFailed {
                        session_id,
                        error: error.to_string(),
                    });
                }
            }
        });
    }

    /// Cancels a running `/btw` side-question turn. The turn lives in the
    /// coordinator as a dialog turn (the D68 direct-call pipeline), not in
    /// the agent runtime's session loop, so the pane's stop button must come
    /// here instead of [`Self::cancel_turn`], which would be a silent no-op.
    pub(crate) fn cancel_btw(&self, child_session_id: String, turn_id: String) {
        self.tokio.spawn(async move {
            let Some(coordinator) =
                openbitfun_core::agentic::coordination::get_global_coordinator()
            else {
                tracing::error!("Coordinator not initialized; /btw cancel unavailable");
                return;
            };
            match coordinator
                .cancel_dialog_turn(&child_session_id, &turn_id)
                .await
            {
                Ok(()) => tracing::info!("Cancelled /btw turn {turn_id} for {child_session_id}"),
                Err(error) => tracing::error!(
                    "Failed to cancel /btw turn {turn_id} for {child_session_id}: {error}"
                ),
            }
        });
    }

    /// Rolls a session back to a persisted turn boundary: the runtime reverts
    /// the transcript and the workspace files together (M19) and emits
    /// SessionHistoryChanged, which refreshes the open transcript through the
    /// regular event pump. Idle is required up front so an in-flight turn
    /// rejects the rollback instead of interleaving with it.
    pub(crate) fn rollback_session_to_turn(
        &self,
        session_id: String,
        turn_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        self.tokio.spawn(async move {
            let request = AgentSessionRollbackToTurnRequest {
                workspace_id,
                workspace_path: String::new(),
                workspace_hostname: None,
                session_id: session_id.clone(),
                target_turn_id: turn_id.clone(),
                require_idle: true,
                expected_storage_turn_index: None,
                expected_catalog_revision: None,
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.rollback_session_to_turn(request).await {
                Ok(AgentSessionRollbackToTurnOutcome::Completed { .. }) => {
                    tracing::info!("Rolled back session {session_id} to turn {turn_id}");
                    // Transcript refresh rides the runtime's
                    // SessionHistoryChanged event; nothing to send here.
                }
                Ok(AgentSessionRollbackToTurnOutcome::RecoveryRequired {
                    reason,
                    affected_files,
                    ..
                }) => {
                    tracing::error!(
                        "Rollback of session {session_id} to turn {turn_id} needs recovery: {reason}"
                    );
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!(
                            "rollback: {reason} ({} files affected)",
                            affected_files.len()
                        ),
                    });
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!(
                        "Failed to roll back session {session_id} to turn {turn_id}: {message}"
                    );
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("rollback: {message}"),
                    });
                }
            }
        });
    }

    /// Starts one manual context-compaction maintenance turn. The turn
    /// identity is caller-generated (the CLI runtime-client precedent): the
    /// runtime reserves it for cancellation ownership, it does not reference
    /// an existing turn. Completion surfaces through the normal
    /// turn-settlement and history-changed events.
    pub(crate) fn start_compaction(
        &self,
        session_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.tokio.spawn(async move {
            let request = AgentSessionCompactionRequest {
                session_id: session_id.clone(),
                turn_id,
            };
            match runtime.start_session_compaction(request).await {
                Ok(_) => {
                    tracing::info!("Compaction started for session {session_id}");
                    let _ = tx.send(RuntimeEvent::CompactionStarted { session_id });
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to compact session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("compact: {message}"),
                    });
                }
            }
        });
    }

    /// Exports the full session transcript as a Markdown file. Image payloads
    /// stay in the runtime; multimodal entries report attachment counts.
    pub(crate) fn export_session(
        &self,
        session_id: String,
        path: PathBuf,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        self.tokio.spawn(async move {
            let transcript = match runtime
                .read_session_transcript(SessionTranscriptRequest {
                    session_id: session_id.clone(),
                    turn_id: None,
                })
                .await
            {
                Ok(transcript) => transcript,
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to read transcript for export: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("export: {message}"),
                    });
                    return;
                }
            };
            let markdown = transcript_to_markdown(&transcript);
            match tokio::fs::write(&path, markdown).await {
                Ok(()) => {
                    let path_display = path.display().to_string();
                    tracing::info!("Exported session {session_id} to {path_display}");
                    let _ = tx.send(RuntimeEvent::ExportFinished {
                        session_id,
                        path: path_display,
                    });
                }
                Err(error) => {
                    tracing::error!("Failed to write export {}: {error}", path.display());
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("export: {error}"),
                    });
                }
            }
        });
    }

    /// Generates the session usage report for the usage dialog.
    pub(crate) fn session_usage(
        &self,
        session_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        self.tokio.spawn(async move {
            let request = AgentSessionUsageRequest {
                session_id: session_id.clone(),
                workspace_id,
                workspace_path: None,
                remote_connection_id: None,
                remote_ssh_host: None,
                include_hidden_subagents: false,
            };
            match runtime.generate_session_usage(request).await {
                Ok(report) => {
                    let _ = tx.send(RuntimeEvent::SessionUsage(Box::new(
                        SessionUsageSummary::from_report(session_id, report),
                    )));
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to generate usage for session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("usage: {message}"),
                    });
                }
            }
        });
    }

    /// Loads the latest insight report metadata for the insights page.
    pub(crate) fn reload_insights(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            match InsightsService::load_latest_reports().await {
                Ok(metas) => {
                    let _ = tx.send(RuntimeEvent::InsightsLatest(metas));
                }
                Err(error) => {
                    tracing::error!("Failed to load latest insights: {error}");
                    let _ = tx.send(RuntimeEvent::InsightsFailed(format!("insights: {error}")));
                }
            }
        });
    }

    /// Generates a fresh insight report over the last `days` days with the
    /// page's model selector (`None` = the daemon's own default). The call is
    /// long-running; the page shows its own generating state and can cancel
    /// through [`RuntimeHandle::cancel_insights`].
    pub(crate) fn generate_insights(
        &self,
        days: u32,
        model_id: Option<String>,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            let _ = tx.send(RuntimeEvent::InsightsGenerating);
            // Boxed: the generation future's concrete type is deep enough to
            // overflow the compiler's Send evaluation without the pin.
            let generation = Box::pin(InsightsService::generate(days, model_id));
            match generation.await {
                Ok(report) => {
                    let _ = tx.send(RuntimeEvent::InsightsReport(Box::new(report)));
                }
                Err(error) => {
                    tracing::error!("Failed to generate insights: {error}");
                    let _ = tx.send(RuntimeEvent::InsightsFailed(format!("insights: {error}")));
                }
            }
        });
    }

    /// Loads one previously generated insight report by its stored path.
    pub(crate) fn load_insights_report(
        &self,
        path: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            match InsightsService::load_report(&path).await {
                Ok(report) => {
                    let _ = tx.send(RuntimeEvent::InsightsReport(Box::new(report)));
                }
                Err(error) => {
                    tracing::error!("Failed to load insight report {path}: {error}");
                    let _ = tx.send(RuntimeEvent::InsightsFailed(format!("insights: {error}")));
                }
            }
        });
    }

    /// Cancels an in-flight insight report generation.
    pub(crate) fn cancel_insights(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            if let Err(error) = InsightsService::cancel().await {
                tracing::warn!("Failed to cancel insights generation: {error}");
                let _ = tx.send(RuntimeEvent::InsightsFailed(format!("insights: {error}")));
            }
        });
    }

    /// Loads the scheduled-jobs snapshot for the tasks page.
    pub(crate) fn reload_cron_jobs(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::cron::get_global_cron_service() else {
                tracing::error!("Scheduled jobs unavailable: cron service missing");
                let _ = tx.send(RuntimeEvent::CronFailed(
                    "cron: service is not initialized".to_string(),
                ));
                return;
            };
            let _ = tx.send(RuntimeEvent::CronJobs(service.list_jobs().await));
        });
    }

    /// Enables or disables one scheduled job; the updated row replaces the
    /// stale one through [`RuntimeEvent::CronJobs`] refresh.
    pub(crate) fn set_cron_job_enabled(
        &self,
        job_id: String,
        enabled: bool,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::cron::get_global_cron_service() else {
                let _ = tx.send(RuntimeEvent::CronFailed(
                    "cron: service is not initialized".to_string(),
                ));
                return;
            };
            match service.set_job_enabled(&job_id, enabled).await {
                Ok(_) => {
                    tracing::info!("Scheduled job {job_id} enabled={enabled}");
                    let _ = tx.send(RuntimeEvent::CronJobs(service.list_jobs().await));
                }
                Err(error) => {
                    tracing::error!("Failed to update scheduled job {job_id}: {error}");
                    let _ = tx.send(RuntimeEvent::CronFailed(format!("cron: {error}")));
                }
            }
        });
    }

    /// Triggers one scheduled job immediately.
    pub(crate) fn run_cron_job_now(&self, job_id: String, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::cron::get_global_cron_service() else {
                let _ = tx.send(RuntimeEvent::CronFailed(
                    "cron: service is not initialized".to_string(),
                ));
                return;
            };
            match service.run_job_now(&job_id).await {
                Ok(_) => {
                    tracing::info!("Scheduled job {job_id} triggered manually");
                    let _ = tx.send(RuntimeEvent::CronJobs(service.list_jobs().await));
                }
                Err(error) => {
                    tracing::error!("Failed to trigger scheduled job {job_id}: {error}");
                    let _ = tx.send(RuntimeEvent::CronFailed(format!("cron: {error}")));
                }
            }
        });
    }

    /// G-P1: creates one scheduled job from the task editor. The editor
    /// already validated the schedule shape; core remains the authority on
    /// cron/timezone parsing and rejects with [`RuntimeEvent::CronFailed`].
    pub(crate) fn create_cron_job(
        &self,
        request: openbitfun_core::service::cron::CreateCronJobRequest,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::cron::get_global_cron_service() else {
                let _ = tx.send(RuntimeEvent::CronFailed(
                    "cron: service is not initialized".to_string(),
                ));
                return;
            };
            match service.create_job(request).await {
                Ok(job) => {
                    tracing::info!("Scheduled job {} created", job.id);
                    let _ = tx.send(RuntimeEvent::CronJobs(service.list_jobs().await));
                }
                Err(error) => {
                    tracing::error!("Failed to create a scheduled job: {error}");
                    let _ = tx.send(RuntimeEvent::CronFailed(format!("cron: {error}")));
                }
            }
        });
    }

    /// G-P1: applies one editor save to an existing job. Fields left `None`
    /// keep their stored value (core's update contract).
    pub(crate) fn update_cron_job(
        &self,
        job_id: String,
        request: openbitfun_core::service::cron::UpdateCronJobRequest,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::cron::get_global_cron_service() else {
                let _ = tx.send(RuntimeEvent::CronFailed(
                    "cron: service is not initialized".to_string(),
                ));
                return;
            };
            match service.update_job(&job_id, request).await {
                Ok(job) => {
                    tracing::info!("Scheduled job {} updated", job.id);
                    let _ = tx.send(RuntimeEvent::CronJobs(service.list_jobs().await));
                }
                Err(error) => {
                    tracing::error!("Failed to update scheduled job {job_id}: {error}");
                    let _ = tx.send(RuntimeEvent::CronFailed(format!("cron: {error}")));
                }
            }
        });
    }

    /// Deletes one scheduled job.
    pub(crate) fn delete_cron_job(&self, job_id: String, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::cron::get_global_cron_service() else {
                let _ = tx.send(RuntimeEvent::CronFailed(
                    "cron: service is not initialized".to_string(),
                ));
                return;
            };
            match service.delete_job(&job_id).await {
                Ok(_) => {
                    tracing::info!("Scheduled job {job_id} deleted");
                    let _ = tx.send(RuntimeEvent::CronJobs(service.list_jobs().await));
                }
                Err(error) => {
                    tracing::error!("Failed to delete scheduled job {job_id}: {error}");
                    let _ = tx.send(RuntimeEvent::CronFailed(format!("cron: {error}")));
                }
            }
        });
    }

    /// Files-content search for the global search page (M16). The core gate
    /// (config feature + managed daemon) is checked per run and reported as
    /// [`RuntimeEvent::SearchUnavailable`] instead of failing silently.
    pub(crate) fn search_workspace_content(
        &self,
        query: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            if !openbitfun_core::service::search::workspace_search_runtime_available().await {
                let detail = if !openbitfun_core::service::search::workspace_search_feature_enabled(
                )
                .await
                {
                    None
                } else if !openbitfun_core::service::search::workspace_search_daemon_available() {
                    Some(openbitfun_core::service::search::workspace_search_daemon_missing_hint())
                } else {
                    None
                };
                let _ = tx.send(RuntimeEvent::SearchUnavailable(detail));
                return;
            }
            let Some(service) =
                openbitfun_core::service::search::get_global_workspace_search_service()
            else {
                tracing::error!("Workspace search unavailable: service is not initialized");
                let _ = tx.send(RuntimeEvent::SearchFailed(
                    "search: service is not initialized".to_string(),
                ));
                return;
            };
            let request = openbitfun_core::service::search::ContentSearchRequest {
                repo_root: workspace.root_path.clone(),
                search_path: None,
                pattern: query,
                output_mode: openbitfun_core::service::search::ContentSearchOutputMode::Content,
                case_sensitive: false,
                use_regex: false,
                whole_word: false,
                multiline: false,
                max_results: Some(crate::search::SEARCH_LIMIT),
                globs: Vec::new(),
                file_types: Vec::new(),
                exclude_file_types: Vec::new(),
            };
            match service.search_content(request).await {
                Ok(result) => {
                    let _ = tx.send(RuntimeEvent::SearchContent {
                        hits: result.hits,
                        truncated: result.outcome.truncated,
                    });
                }
                Err(error) => {
                    tracing::error!("Workspace content search failed: {error}");
                    let _ = tx.send(RuntimeEvent::SearchFailed(format!("search: {error}")));
                }
            }
        });
    }

    /// Session-content search for the global search page (M16), through the
    /// `ProductSearchPort` the compatibility owner implements.
    pub(crate) fn search_sessions(&self, query: String, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let workspace = self.workspace();
        let compat = self.compat.clone();
        self.tokio.spawn(async move {
            let request = SessionContentSearchRequest {
                workspace_id: Some(workspace.id.clone()),
                workspace_path: workspace.root_path.to_string_lossy().to_string(),
                remote_connection_id: None,
                remote_ssh_host: None,
                query,
                limit: crate::search::SEARCH_LIMIT,
                include_archived: false,
            };
            match search_session_content_via(&compat, request).await {
                Ok(response) => {
                    let _ = tx.send(RuntimeEvent::SearchSessions {
                        hits: response.hits,
                        truncated: response.truncated,
                    });
                }
                Err(error) => {
                    tracing::error!("Session search failed: {error}");
                    let _ = tx.send(RuntimeEvent::SearchFailed(format!("sessions: {error}")));
                }
            }
        });
    }

    /// Updates one session's model selector (model only; the reasoning preset
    /// stays whatever the session already recorded, matching the desktop's
    /// model-only path).
    pub(crate) fn update_session_model(
        &self,
        session_id: String,
        model_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionModelUpdateRequest {
                session_id: session_id.clone(),
                model_id: model_id.clone(),
            };
            match runtime.update_session_model(request).await {
                Ok(()) => {
                    tracing::info!("Session {session_id} model set to {model_id}");
                    let _ = tx.send(RuntimeEvent::SessionModelChanged {
                        session_id,
                        model_id,
                    });
                    handle.reload_sessions(tx);
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to update model for session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionActionFailed {
                        message: format!("model: {message}"),
                    });
                }
            }
        });
    }

    /// Loads the configured-model catalog for the pickers. Read once at
    /// bootstrap; the runtime owns refresh through the same core path the
    /// desktop models page uses.
    pub(crate) fn reload_model_catalog(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            match openbitfun_core::get_remote_model_catalog().await {
                Ok(catalog) => {
                    let models = catalog
                        .models
                        .into_iter()
                        .map(|model| ModelChoice {
                            id: model.id,
                            name: model.name,
                            provider: model.provider,
                            enabled: model.enabled,
                        })
                        .collect::<Vec<_>>();
                    tracing::info!("Model catalog loaded: {} models", models.len());
                    let _ = tx.send(RuntimeEvent::ModelCatalog(models));
                }
                Err(error) => {
                    tracing::warn!("Failed to load model catalog: {error}");
                    let _ = tx.send(RuntimeEvent::ModelCatalog(Vec::new()));
                }
            }
        });
    }

    /// G-SE12: probes one configured model through the same transient-client
    /// path the desktop `test_ai_config_connection` command uses, including
    /// the image-input leg for multimodal models. Runs off the UI thread; the
    /// result comes back as [`RuntimeEvent::ModelTestFinished`].
    pub(crate) fn test_model_connection(&self, model_id: String) {
        self.tokio.spawn(async move {
            let result = test_model_connection_once(&model_id).await;
            match &result {
                Ok(receipt) => tracing::info!(
                    "Model connection test finished: model={}, success={}, response_time={}ms",
                    model_id,
                    receipt.success,
                    receipt.response_time_ms
                ),
                Err(error) => {
                    tracing::warn!(
                        "Model connection test failed to run: model={model_id}, error={error}"
                    )
                }
            }
            publish(RuntimeEvent::ModelTestFinished { model_id, result });
        });
    }

    /// G-SE25: reads the Hook-import projection for the active workspace.
    /// `refresh_updates` re-probes source updates (the page's Refresh button);
    /// a plain read uses the catalog cache.
    pub(crate) fn reload_hook_imports(&self, refresh_updates: bool) {
        let workspace = self.workspace().id;
        self.tokio.spawn(async move {
            match openbitfun_core::external_hook_import::external_hook_import_snapshot(
                Some(&workspace),
                refresh_updates,
            )
            .await
            {
                Ok(snapshot) => publish(RuntimeEvent::HookImports(Box::new(snapshot))),
                Err(error) => publish(RuntimeEvent::HookImportFailed(error.to_string())),
            }
        });
    }

    /// G-SE25: applies one import mutation (toggle, remove, corrupt-store
    /// reset) against the revision the page rendered.
    pub(crate) fn mutate_hook_import(
        &self,
        expected_revision: String,
        action: openbitfun_product_domains::external_hook_import::ExternalHookImportMutationV1,
    ) {
        use openbitfun_product_domains::external_hook_import::ExternalHookImportMutationRequestV1;
        let workspace = self.workspace().id;
        self.tokio.spawn(async move {
            let request = ExternalHookImportMutationRequestV1 {
                schema_version:
                    openbitfun_product_domains::external_hook_import::EXTERNAL_HOOK_IMPORT_SCHEMA_V1,
                expected_revision,
                action,
            };
            match openbitfun_core::external_hook_import::mutate_external_hook_import(
                Some(&workspace),
                request,
            )
            .await
            {
                Ok(snapshot) => publish(RuntimeEvent::HookImports(Box::new(snapshot))),
                Err(error) => publish(RuntimeEvent::HookImportFailed(error.to_string())),
            }
        });
    }

    /// G-SE25: prepares the review plan for one source's Update action.
    pub(crate) fn plan_hook_import(
        &self,
        source: openbitfun_product_domains::external_sources::SourceKey,
    ) {
        let workspace = self.workspace().id;
        self.tokio.spawn(async move {
            match openbitfun_core::external_hook_import::plan_external_hook_import(
                Some(&workspace),
                source,
            )
            .await
            {
                Ok(plan) => publish(RuntimeEvent::HookImportPlan(Box::new(plan))),
                Err(error) => publish(RuntimeEvent::HookImportFailed(error.to_string())),
            }
        });
    }

    /// G-SE25: applies a reviewed plan. A stale fingerprint returns the
    /// refreshed plan instead of writing anything.
    pub(crate) fn apply_hook_import(
        &self,
        source: openbitfun_product_domains::external_sources::SourceKey,
        plan_fingerprint: String,
    ) {
        use openbitfun_product_domains::external_hook_import::{
            ExternalHookImportApplyOutcomeV1, ExternalHookImportApplyRequestV1,
        };
        let workspace = self.workspace().id;
        self.tokio.spawn(async move {
            let request = ExternalHookImportApplyRequestV1 {
                schema_version:
                    openbitfun_product_domains::external_hook_import::EXTERNAL_HOOK_IMPORT_SCHEMA_V1,
                source,
                plan_fingerprint,
            };
            match openbitfun_core::external_hook_import::apply_external_hook_import(
                Some(&workspace),
                request,
            )
            .await
            {
                Ok(result) => match result.outcome {
                    ExternalHookImportApplyOutcomeV1::Applied { snapshot }
                    | ExternalHookImportApplyOutcomeV1::Unchanged { snapshot } => {
                        publish(RuntimeEvent::HookImports(Box::new(snapshot)));
                    }
                    ExternalHookImportApplyOutcomeV1::Stale { refreshed_plan } => {
                        publish(RuntimeEvent::HookImportPlanStale(Box::new(refreshed_plan)));
                    }
                },
                Err(error) => publish(RuntimeEvent::HookImportFailed(error.to_string())),
            }
        });
    }

    /// M21: the skills catalog plus the global disable sets, for the skills
    /// management page.
    pub(crate) fn reload_skill_catalog(&self) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let catalog = fetch_skill_catalog(&workspace.root_path, &workspace.id).await;
            publish(RuntimeEvent::SkillCatalog(catalog));
        });
    }

    /// G-K3: checks an add-skill source directory without writing anything.
    pub(crate) fn validate_skill_path(&self, path: String) {
        self.tokio.spawn(async move {
            publish(RuntimeEvent::SkillValidationFinished(
                validate_skill_source(path).await,
            ));
        });
    }

    /// G-K3: copies one validated skill directory into the user or project
    /// skills root, then re-reads the catalog.
    pub(crate) fn add_skill(&self, source_path: String, project_level: bool) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let result = add_skill_folder(&workspace, source_path, project_level).await;
            if result.is_ok() {
                let catalog = fetch_skill_catalog(&workspace.root_path, &workspace.id).await;
                publish(RuntimeEvent::SkillCatalog(catalog));
            }
            publish(RuntimeEvent::SkillAdded(result));
        });
    }

    /// G-K3: removes one OpenBitFun-owned, non-built-in skill directory after
    /// checking it really sits under the managed skills root.
    pub(crate) fn delete_skill(&self, skill_key: String) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let result = delete_skill_folder(&workspace, &skill_key).await;
            if result.is_ok() {
                let catalog = fetch_skill_catalog(&workspace.root_path, &workspace.id).await;
                publish(RuntimeEvent::SkillCatalog(catalog));
            }
            publish(RuntimeEvent::SkillDeleted(result));
        });
    }

    /// M21: mode and subagent cards for the agents management page.
    pub(crate) fn reload_agent_catalog(&self) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let catalog = fetch_agent_catalog(Some(&workspace.id)).await;
            publish(RuntimeEvent::AgentCatalog(catalog));
        });
    }

    /// M21: configured MCP servers with their runtime status.
    pub(crate) fn reload_mcp_catalog(&self) {
        self.tokio.spawn(async move {
            publish(RuntimeEvent::McpCatalog(fetch_mcp_catalog().await));
        });
    }

    /// M21: resources/prompts snapshot for one server (browse dialog).
    pub(crate) fn reload_mcp_server_catalog(&self, server_id: String, force: bool) {
        self.tokio.spawn(async move {
            match fetch_mcp_server_catalog(&server_id, force).await {
                Ok(view) => publish(RuntimeEvent::McpServerCatalog(Box::new(view))),
                Err(message) => {
                    tracing::warn!("Failed to load MCP catalog for {server_id}: {message}");
                    publish(RuntimeEvent::ManageFailed(message));
                }
            }
        });
    }

    /// M21: flips one skill's global enablement and reloads the catalog.
    /// Desktop parity: every skill-settings write is followed by a global
    /// config reload so resolution picks the new set up.
    pub(crate) fn set_skill_disabled(&self, key: String, user_level: bool, disabled: bool) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            use openbitfun_core::agentic::tools::implementations::skills::mode_overrides;
            let result = if user_level {
                mode_overrides::set_global_user_skill_disabled(&key, disabled).await
            } else {
                mode_overrides::set_global_project_skill_disabled(
                    openbitfun_core::agentic::tools::implementations::skills::mode_overrides::SkillPolicyWorkspace::from_record(&workspace),
                    &key,
                    disabled,
                )
                .await
            };
            if let Err(error) = result {
                tracing::error!("Failed to set skill {key} disabled={disabled}: {error}");
                publish(RuntimeEvent::ManageFailed(format!("skill {key}: {error}")));
            }
            if let Err(error) = openbitfun_core::service::config::reload_global_config().await {
                tracing::warn!("Failed to reload global config after skill change: {error}");
            }
            let catalog = fetch_skill_catalog(&workspace.root_path, &workspace.id).await;
            publish(RuntimeEvent::SkillCatalog(catalog));
        });
    }

    /// M21: writes a custom agent definition from the create dialog and
    /// reloads the catalog. The dialog closes when the catalog event lands;
    /// a failure surfaces as the dismissible banner (and inline on submit).
    pub(crate) fn create_custom_agent(&self, draft: crate::manage::AgentDraft) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            match crate::manage::persist_agent_draft(draft, &workspace.id, &workspace.root_path)
                .await
            {
                Ok(()) => {
                    let catalog = fetch_agent_catalog(Some(&workspace.id)).await;
                    publish(RuntimeEvent::AgentCatalog(catalog));
                }
                Err(message) => {
                    tracing::warn!("Agent creation failed: {message}");
                    publish(RuntimeEvent::ManageFailed(message));
                }
            }
        });
    }

    /// T2-11: the skill selection projection for one agent id (mode profile
    /// representative or subagent id), for the agents page skills dialog.
    pub(crate) fn reload_mode_skill_infos(&self, mode_id: String) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let rows = fetch_mode_skill_rows(&workspace, &mode_id).await;
            publish(RuntimeEvent::ModeSkillInfos { mode_id, rows });
        });
    }

    /// T2-11: replaces one agent's skill selection through the desktop
    /// `replace_mode_skill_selection` pipeline (validate, persist the user
    /// override diff, persist the project disable set), reloads the global
    /// config, and refetches. A successful refetch lands the dialog back in
    /// the view phase; a failure surfaces as the dialog's inline error.
    pub(crate) fn replace_mode_skill_selection(&self, mode_id: String, enabled_keys: Vec<String>) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            if let Err(message) =
                persist_mode_skill_selection(&workspace, &mode_id, &enabled_keys).await
            {
                tracing::error!("Failed to replace skill selection for {mode_id}: {message}");
                publish(RuntimeEvent::ManageFailed(format!("skills: {message}")));
                return;
            }
            if let Err(error) = openbitfun_core::service::config::reload_global_config().await {
                tracing::warn!(
                    "Failed to reload global config after skill selection update: {error}"
                );
            }
            let rows = fetch_mode_skill_rows(&workspace, &mode_id).await;
            publish(RuntimeEvent::ModeSkillInfos { mode_id, rows });
        });
    }

    /// T2-11: clears one agent's user overrides and project disable set (the
    /// desktop `reset_mode_skill_selection` semantics), then refetches.
    pub(crate) fn reset_mode_skill_selection(&self, mode_id: String) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            if let Err(message) = clear_mode_skill_selection(&workspace, &mode_id).await {
                tracing::error!("Failed to reset skill selection for {mode_id}: {message}");
                publish(RuntimeEvent::ManageFailed(format!("skills: {message}")));
                return;
            }
            if let Err(error) = openbitfun_core::service::config::reload_global_config().await {
                tracing::warn!(
                    "Failed to reload global config after resetting skill selection: {error}"
                );
            }
            let rows = fetch_mode_skill_rows(&workspace, &mode_id).await;
            publish(RuntimeEvent::ModeSkillInfos { mode_id, rows });
        });
    }

    /// T2-12: the file-backed custom agent definition for the edit dialog
    /// prefill (the shell only edits name/description/prompt).
    pub(crate) fn load_custom_agent_detail(&self, agent_id: String) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let registry = openbitfun_core::agentic::agents::get_agent_registry();
            let result = match registry
                .get_custom_agent_detail(&agent_id, Some(&workspace.id))
                .await
            {
                Ok(detail) => Ok(crate::manage::project_agent_detail(&detail)),
                Err(error) => Err(error.to_string()),
            };
            publish(RuntimeEvent::AgentDetail(result));
        });
    }

    /// T2-12: updates the editable fields of one custom agent. Tools, model,
    /// review, and the context policy pass `None` so the core update keeps
    /// the existing definition; the catalog refetch closes the dialog and
    /// proves the write landed (the same echo the create flow uses).
    pub(crate) fn update_custom_agent(&self, draft: crate::manage::AgentEditDraft) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let registry = openbitfun_core::agentic::agents::get_agent_registry();
            let result = registry
                .update_custom_agent_definition(
                    &draft.agent_id,
                    Some(&workspace.id),
                    draft.name.trim().to_string(),
                    draft.description.trim().to_string(),
                    draft.prompt.trim().to_string(),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await;
            if let Err(error) = result {
                tracing::error!("Failed to update custom agent {}: {error}", draft.agent_id);
                publish(RuntimeEvent::ManageFailed(error.to_string()));
                return;
            }
            tracing::info!("Updated custom agent {}", draft.agent_id);
            let catalog = fetch_agent_catalog(Some(&workspace.id)).await;
            publish(RuntimeEvent::AgentCatalog(catalog));
        });
    }

    /// G-A3: stores one subagent's model override (web
    /// `updateSubagentConfig`). Builtin subagents persist under the shared
    /// `ai.agent_model_defaults.subagents.builtin` map; custom file-backed
    /// subagents rewrite their own definition. `choice == None` clears the
    /// override back to the default SubAgent model.
    pub(crate) fn set_subagent_model(
        &self,
        subagent_id: String,
        builtin: bool,
        choice: Option<String>,
    ) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let result: Result<(), String> = if builtin {
                match get_global_config_service().await {
                    Ok(service) => {
                        let mut builtin_models: std::collections::HashMap<
                            String,
                            openbitfun_core::service::config::types::SubagentModelSelection,
                        > = service
                            .get_config(Some("ai.agent_model_defaults.subagents.builtin"))
                            .await
                            .unwrap_or_default();
                        match &choice {
                            None => {
                                builtin_models.remove(&subagent_id);
                            }
                            Some(value) => {
                                builtin_models.insert(
                                    subagent_id.clone(),
                                    crate::settings::subagent_model_selection(value),
                                );
                            }
                        }
                        service
                            .set_config(
                                "ai.agent_model_defaults.subagents.builtin",
                                &builtin_models,
                            )
                            .await
                            .map_err(|error| error.to_string())
                    }
                    Err(error) => Err(error.to_string()),
                }
            } else {
                let (model, clear) = match &choice {
                    None => (None, true),
                    Some(value) => (Some(value.clone()), false),
                };
                openbitfun_core::agentic::agents::get_agent_registry()
                    .update_and_save_custom_subagent_config(
                        &subagent_id,
                        model,
                        clear,
                        Some(&workspace.id),
                    )
                    .map_err(|error| error.to_string())
            };
            if let Err(message) = result {
                tracing::warn!("Failed to set the model of subagent {subagent_id}: {message}");
                publish(RuntimeEvent::AgentModelFailed(message));
                return;
            }
            if let Err(error) = openbitfun_core::service::config::reload_global_config().await {
                tracing::warn!(
                    "Failed to reload global config after a subagent model change: {error}"
                );
            }
            let catalog = fetch_agent_catalog(Some(&workspace.id)).await;
            publish(RuntimeEvent::AgentCatalog(catalog));
        });
    }

    /// T2-12: the desktop `delete_custom_agent` pipeline — remove the
    /// mode references from the shared config, reload the global config,
    /// then refetch the catalog (the delete confirm closes immediately;
    /// failures surface as the banner).
    pub(crate) fn delete_custom_agent(&self, agent_id: String) {
        let workspace = self.workspace();
        self.tokio.spawn(async move {
            let registry = openbitfun_core::agentic::agents::get_agent_registry();
            let removed = match registry.remove_custom_agent(&agent_id) {
                Ok(path) => path,
                Err(error) => {
                    tracing::error!("Failed to remove custom agent {agent_id}: {error}");
                    publish(RuntimeEvent::ManageFailed(error.to_string()));
                    return;
                }
            };
            if let Some(path) = removed {
                if let Err(error) = std::fs::remove_file(&path) {
                    tracing::warn!(
                        "Failed to delete custom agent file: path={path}, error={error}"
                    );
                }
            }
            match openbitfun_core::service::config::get_global_config_service().await {
                Ok(service) => {
                    if let Err(error) = service
                        .update_config(
                            "",
                            |config: &mut openbitfun_core::service::config::GlobalConfig| {
                                config.ai.agent_profiles.remove(&agent_id);
                                config.app.flow_chat.remove_mode_reference(&agent_id);
                                Ok(())
                            },
                        )
                        .await
                    {
                        tracing::warn!(
                            "Failed to clean up config after custom agent deletion: agent_id={agent_id}, error={error}"
                        );
                    }
                }
                Err(error) => tracing::warn!(
                    "Config service unavailable after custom agent deletion: {error}"
                ),
            }
            if let Err(error) = openbitfun_core::service::config::reload_global_config().await {
                tracing::warn!(
                    "Failed to reload global config after custom agent deletion: agent_id={agent_id}, error={error}"
                );
            }
            let catalog = fetch_agent_catalog(Some(&workspace.id)).await;
            publish(RuntimeEvent::AgentCatalog(catalog));
        });
    }

    /// M21: one MCP server lifecycle action; the catalog reloads either way.
    pub(crate) fn mcp_server_action(&self, action: crate::manage::McpAction, server_id: String) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::mcp::get_global_mcp_service() else {
                publish(RuntimeEvent::ManageFailed(
                    "MCP service is not initialized".to_string(),
                ));
                return;
            };
            let manager = service.server_manager();
            let result = match action {
                crate::manage::McpAction::Start => manager
                    .start_server(&server_id)
                    .await
                    .map_err(|error| error.to_string()),
                crate::manage::McpAction::Stop => manager
                    .stop_server(&server_id)
                    .await
                    .map_err(|error| error.to_string()),
                crate::manage::McpAction::Restart => manager
                    .restart_server(&server_id)
                    .await
                    .map_err(|error| error.to_string()),
                crate::manage::McpAction::SetEnabled(enabled) => {
                    set_mcp_server_enabled(&service, &server_id, enabled).await
                }
                crate::manage::McpAction::Delete => manager
                    .remove_server(&server_id)
                    .await
                    .map_err(|error| error.to_string()),
            };
            if let Err(message) = result {
                tracing::warn!("MCP action failed for {server_id}: {message}");
                publish(RuntimeEvent::ManageFailed(format!(
                    "MCP {server_id}: {message}"
                )));
            }
            publish(RuntimeEvent::McpCatalog(fetch_mcp_catalog().await));
        });
    }

    /// M21: answers one pending MCP interaction (elicitation/sampling). A
    /// rejection carries the same message the desktop UI sends.
    pub(crate) fn submit_mcp_interaction(
        &self,
        interaction_id: String,
        approve: bool,
        result: Option<serde_json::Value>,
    ) {
        self.tokio.spawn(async move {
            let Some(service) = openbitfun_core::service::mcp::get_global_mcp_service() else {
                tracing::error!("MCP service unavailable while answering {interaction_id}");
                return;
            };
            let (error_message, error_code) = if approve {
                (None, None)
            } else {
                (
                    Some("Rejected by the user in the OpenBitFun shell".to_string()),
                    None,
                )
            };
            if let Err(error) = service
                .server_manager()
                .submit_interaction_response(
                    &interaction_id,
                    approve,
                    result,
                    error_message,
                    error_code,
                    None,
                )
                .await
            {
                tracing::warn!("Failed to submit MCP interaction response: {error}");
            }
        });
    }

    /// T2-7: the user JSON config snapshot (Cursor format) for the editor
    /// dialog. Same core read the desktop `load_mcp_json_config` command
    /// serves.
    pub(crate) fn load_mcp_json_config(&self) {
        self.tokio.spawn(async move {
            let result = match openbitfun_core::service::mcp::get_global_mcp_service() {
                Some(service) => service
                    .config_service()
                    .load_mcp_json_config()
                    .await
                    .map(|snapshot| crate::manage::McpJsonSnapshot {
                        json_config: snapshot.json_config,
                        fingerprint: snapshot.fingerprint,
                    })
                    .map_err(|error| error.to_string()),
                None => Err("MCP service is not initialized".to_string()),
            };
            publish(RuntimeEvent::McpJsonConfig(result));
        });
    }

    /// T2-7: saves the edited JSON config through the desktop pipeline —
    /// parse + validate + fingerprint check, then reconcile the running
    /// servers with the previous/current config sets. The catalog refreshes
    /// either way so statuses follow the file.
    pub(crate) fn save_mcp_json_config(&self, json_config: String, expected_fingerprint: String) {
        self.tokio.spawn(async move {
            let result = save_mcp_json_config(&json_config, &expected_fingerprint).await;
            if let Err(message) = &result {
                tracing::warn!("Failed to save MCP JSON config: {message}");
            }
            publish(RuntimeEvent::McpJsonSaved(result));
            publish(RuntimeEvent::McpCatalog(fetch_mcp_catalog().await));
        });
    }

    /// G-M2: reads one typed import path's file content (the shell's stand-in
    /// for the web file picker) and routes it into the paste box.
    pub(crate) fn read_mcp_import_file(&self, path: String) {
        self.tokio.spawn(async move {
            if path.trim().is_empty() {
                publish(RuntimeEvent::McpImportFile(Err(
                    "The import file path is empty".to_string(),
                )));
                return;
            }
            let result = tokio::fs::read_to_string(&path)
                .await
                .map_err(|error| format!("{error}"));
            publish(RuntimeEvent::McpImportFile(result));
        });
    }

    /// G-Y4: exports the whole MindGraph through the daemon (`memory.export`),
    /// which reads the graph and writes the file at `path` (web T10).
    pub(crate) fn trinity_memory_export(&self, path: String) {
        self.tokio.spawn(async move {
            let result = crate::trinity::client()
                .call("memory.export", serde_json::json!({ "path": path.clone() }))
                .await;
            let outcome = match result {
                Ok(value) => crate::trinity::MemoryTransferOutcome::Exported {
                    nodes: value
                        .get("nodes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    triples: value
                        .get("triples")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    path,
                },
                Err(error) => {
                    tracing::warn!("Trinity memory.export failed: {error}");
                    crate::trinity::MemoryTransferOutcome::Failed(error)
                }
            };
            publish(RuntimeEvent::TrinityTransfer(Box::new(outcome)));
        });
    }

    /// G-Y4: imports a trinity-graph container through the daemon
    /// (`memory.import`). A cross-identity container is rejected with both
    /// being ids; the retry after explicit confirmation sets
    /// `confirm_being_id` (web T10 migration gate).
    pub(crate) fn trinity_memory_import(&self, path: String, confirm_being_id: bool) {
        self.tokio.spawn(async move {
            let mut params = serde_json::json!({ "path": path.clone() });
            if confirm_being_id {
                params["confirm_being_id"] = serde_json::json!(true);
            }
            let result = crate::trinity::client().call("memory.import", params).await;
            let outcome = match result {
                Ok(value) => crate::trinity::MemoryTransferOutcome::Imported {
                    added: value
                        .get("added")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                },
                Err(error) => match crate::trinity::parse_being_id_mismatch(&error) {
                    Some((container, local)) => crate::trinity::MemoryTransferOutcome::Mismatch {
                        container,
                        local,
                        path,
                    },
                    None => {
                        tracing::warn!("Trinity memory.import failed: {error}");
                        crate::trinity::MemoryTransferOutcome::Failed(error)
                    }
                },
            };
            publish(RuntimeEvent::TrinityTransfer(Box::new(outcome)));
        });
    }

    /// T2-7: reads one resource through the server connection and routes the
    /// outcome back to the browse dialog. Mirrors the desktop
    /// `read_mcp_resource` command, including the surface-availability gate.
    pub(crate) fn read_mcp_resource(&self, server_id: String, uri: String) {
        self.tokio.spawn(async move {
            let result = match openbitfun_core::service::mcp::get_global_mcp_service() {
                Some(service) => {
                    let manager = service.server_manager();
                    if manager
                        .server_available_for_context(&server_id, None, false)
                        .await
                    {
                        match manager.get_connection(&server_id).await {
                            Some(connection) => connection
                                .read_resource(&uri)
                                .await
                                .map_err(|error| error.to_string()),
                            None => Err(format!("MCP server not connected: {server_id}")),
                        }
                    } else {
                        Err("MCP server is unavailable in this product surface".to_string())
                    }
                }
                None => Err("MCP service is not initialized".to_string()),
            };
            publish(RuntimeEvent::McpResourceRead(Box::new(
                crate::manage::McpResourceReadOutcome { uri, result },
            )));
        });
    }

    /// M22: fetches global token-usage statistics for the settings usage
    /// page. Same core service the desktop dashboard reads; the result rides
    /// the pump as [`RuntimeEvent::UsageReady`].
    pub(crate) fn fetch_usage_stats(&self, time_range: String) {
        self.tokio.spawn(async move {
            let result =
                match openbitfun_core::service::token_usage::get_global_token_usage_service() {
                    Some(service) => {
                        let request =
                            openbitfun_services_core::token_usage::TokenUsageStatisticsRequest {
                                time_range: time_range.clone(),
                                granularity: "day".to_string(),
                                start: None,
                                end: None,
                                time_zone: None,
                                include_subagent: true,
                                filter_kind: Default::default(),
                                filter_query: None,
                            };
                        match service.get_statistics_for_request(request).await {
                            Ok(stats) => Ok(Box::new(stats)),
                            Err(error) => Err(format!("{error:#}")),
                        }
                    }
                    None => Err("token usage service is not available".to_string()),
                };
            if let Err(message) = &result {
                tracing::warn!("Usage statistics for {time_range} failed: {message}");
            }
            publish(RuntimeEvent::UsageReady(result));
        });
    }

    /// T2-8: fetches the activity heatmap window — a fixed six-month
    /// custom-range request at day granularity (web
    /// `usageActivityRequestRange` semantics, UTC-anchored). The result rides
    /// the pump as [`RuntimeEvent::UsageActivityReady`].
    pub(crate) fn fetch_usage_activity(&self) {
        self.tokio.spawn(async move {
            let end = chrono::Utc::now();
            let start = crate::settings::activity_window_start(end);
            let result =
                match openbitfun_core::service::token_usage::get_global_token_usage_service() {
                    Some(service) => {
                        let request =
                            openbitfun_services_core::token_usage::TokenUsageStatisticsRequest {
                                time_range: "custom".to_string(),
                                granularity: "day".to_string(),
                                start: Some(start),
                                end: Some(end),
                                time_zone: None,
                                include_subagent: true,
                                filter_kind: Default::default(),
                                filter_query: None,
                            };
                        match service.get_statistics_for_request(request).await {
                            Ok(stats) => Ok(Box::new(stats)),
                            Err(error) => Err(format!("{error:#}")),
                        }
                    }
                    None => Err("token usage service is not available".to_string()),
                };
            if let Err(message) = &result {
                tracing::warn!("Usage activity statistics failed: {message}");
            }
            publish(RuntimeEvent::UsageActivityReady(result));
        });
    }

    /// T2-15 Trinity console: pulls identity/PSI/history in one pass so the
    /// page flips atomically. The daemon is never spawned here — an absent
    /// daemon surfaces as the console's offline state.
    pub(crate) fn trinity_refresh(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            let client = crate::trinity::client();
            let (status, psi, history) = tokio::join!(
                client.call("get_status", serde_json::json!({})),
                client.call("get_cognitive_state", serde_json::json!({})),
                client.call("cognition_history", serde_json::json!({ "limit": 120 })),
            );
            // A successful get_status RPC is the definition of "connected".
            let status = match status {
                Ok(value) => crate::trinity::parse_status(&value),
                Err(error) => {
                    tracing::info!("Trinity daemon unreachable: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityStatusFailed(error));
                    return;
                }
            };
            // PSI/history failures degrade to defaults while the identity
            // still renders — the daemon answered, so it is connected.
            let psi = match psi {
                Ok(value) => crate::trinity::parse_psi(&value),
                Err(error) => {
                    tracing::warn!("Trinity get_cognitive_state failed: {error}");
                    Default::default()
                }
            };
            let history = match history {
                Ok(value) => crate::trinity::parse_history(&value),
                Err(error) => {
                    tracing::warn!("Trinity cognition_history failed: {error}");
                    Vec::new()
                }
            };
            let _ = tx.send(RuntimeEvent::TrinityStatusReady {
                psi: Box::new(psi),
                status: Box::new(status),
                history,
            });
        });
    }

    /// One timeline page; `before_ts` continues an existing list (web
    /// load-earlier semantics), `None` restarts it. A short page marks the
    /// timeline exhausted.
    pub(crate) fn trinity_load_memories(
        &self,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
        before_ts: Option<i64>,
    ) {
        self.tokio.spawn(async move {
            let mut params =
                serde_json::json!({ "limit": crate::trinity::TIMELINE_LIMIT, "full": true });
            if let Some(before) = before_ts {
                params["before_ts"] = serde_json::json!(before);
            }
            match crate::trinity::client()
                .call("memory.timeline", params)
                .await
            {
                Ok(value) => {
                    let items = crate::trinity::parse_memory_items(&value);
                    let exhausted = (items.len() as i64) < crate::trinity::TIMELINE_LIMIT;
                    let _ = tx.send(RuntimeEvent::TrinityMemoriesReady {
                        items,
                        exhausted,
                        append: before_ts.is_some(),
                    });
                }
                Err(error) => {
                    tracing::warn!("Trinity memory.timeline failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityMemoriesFailed(error));
                }
            }
        });
    }

    /// Recall memories matching `query` (web `recallMemory({query, limit:20})`).
    pub(crate) fn trinity_recall(&self, tx: mpsc::UnboundedSender<RuntimeEvent>, query: String) {
        self.tokio.spawn(async move {
            match crate::trinity::client()
                .call(
                    "recall_memory",
                    serde_json::json!({ "query": query, "limit": 20 }),
                )
                .await
            {
                Ok(value) => {
                    let _ = tx.send(RuntimeEvent::TrinityRecallReady(
                        crate::trinity::parse_recall_items(&value),
                    ));
                }
                Err(error) => {
                    tracing::warn!("Trinity recall_memory failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityActionFailed(error));
                }
            }
        });
    }

    /// Store a memory (`kind: "note"` like the web add form). On success the
    /// timeline reloads so the new entry shows up immediately.
    pub(crate) fn trinity_memorize(
        &self,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
        content: String,
        project: Option<String>,
        target_id: Option<String>,
    ) {
        self.tokio.spawn(async move {
            // Only a correction is judged: the daemon answers a plain
            // memorize without a `corrected` flag.
            let correction = target_id.is_some();
            let mut params = serde_json::json!({ "content": content, "kind": "note" });
            if let Some(project) = project {
                params["project"] = serde_json::json!(project);
            }
            if let Some(target_id) = target_id {
                params["target_id"] = serde_json::json!(target_id);
            }
            match crate::trinity::client().call("memorize", params).await {
                Ok(value) => {
                    if correction {
                        let applied = value
                            .get("corrected")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true);
                        let _ = tx.send(RuntimeEvent::TrinityCorrection { applied });
                    }
                    Self::load_memories_for(tx).await
                }
                Err(error) => {
                    tracing::warn!("Trinity memorize failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityActionFailed(error));
                }
            }
        });
    }

    /// Forget one memory by id, then reload the timeline.
    pub(crate) fn trinity_forget(&self, tx: mpsc::UnboundedSender<RuntimeEvent>, id: String) {
        self.tokio.spawn(async move {
            match crate::trinity::client()
                .call("forget_memory", serde_json::json!({ "id": id }))
                .await
            {
                Ok(_) => Self::load_memories_for(tx).await,
                Err(error) => {
                    tracing::warn!("Trinity forget_memory failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityActionFailed(error));
                }
            }
        });
    }

    /// Reinforce one memory by id, then reload the timeline.
    pub(crate) fn trinity_reinforce(&self, tx: mpsc::UnboundedSender<RuntimeEvent>, id: String) {
        self.tokio.spawn(async move {
            match crate::trinity::client()
                .call("reinforce_memory", serde_json::json!({ "id": id }))
                .await
            {
                Ok(_) => Self::load_memories_for(tx).await,
                Err(error) => {
                    tracing::warn!("Trinity reinforce_memory failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityActionFailed(error));
                }
            }
        });
    }

    /// G-SE14 engine page: reads the daemon LLM configuration
    /// (`llm.get_config`). A daemon that cannot answer settles the page's
    /// offline state with the reason.
    pub(crate) fn trinity_engine_load(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            match crate::trinity::client()
                .call("llm.get_config", serde_json::json!({}))
                .await
            {
                Ok(value) => {
                    let config = crate::trinity::parse_llm_config(&value);
                    let _ = tx.send(RuntimeEvent::TrinityEngineConfigReady {
                        config: Box::new(config),
                    });
                }
                Err(error) => {
                    tracing::info!("Trinity llm.get_config failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityEngineConfigFailed(error));
                }
            }
        });
    }

    /// G-SE14: writes the engine LLM configuration (`llm.set_config`); the
    /// daemon persists its toml and hot-swaps. An empty api key keeps the
    /// stored one (daemon contract).
    pub(crate) fn trinity_engine_save(
        &self,
        api_url: String,
        model: String,
        api_key: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            match crate::trinity::client()
                .call(
                    "llm.set_config",
                    serde_json::json!({
                        "api_url": api_url,
                        "model": model,
                        "api_key": api_key,
                    }),
                )
                .await
            {
                Ok(_) => {
                    let _ = tx.send(RuntimeEvent::TrinityEngineSaved);
                }
                Err(error) => {
                    tracing::warn!("Trinity llm.set_config failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityEngineSaveFailed(error));
                }
            }
        });
    }

    /// G-SE14: probes the engine LLM connection with the draft values
    /// (`llm.test_connection`); an empty api key uses the saved one.
    pub(crate) fn trinity_engine_test(
        &self,
        api_url: String,
        model: String,
        api_key: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.tokio.spawn(async move {
            match crate::trinity::client()
                .call(
                    "llm.test_connection",
                    serde_json::json!({
                        "api_url": api_url,
                        "model": model,
                        "api_key": api_key,
                    }),
                )
                .await
            {
                Ok(value) => {
                    let (ok, error) = crate::trinity::parse_test_outcome(&value);
                    let _ = tx.send(RuntimeEvent::TrinityEngineTested { ok, error });
                }
                Err(error) => {
                    tracing::warn!("Trinity llm.test_connection failed: {error}");
                    let _ = tx.send(RuntimeEvent::TrinityEngineTested {
                        ok: false,
                        error: Some(error),
                    });
                }
            }
        });
    }

    /// G-SE13 memory page: restores the stored `memories` document to the
    /// shipped defaults (`configManager.resetConfig('memories')` on the web)
    /// and re-reads the typed section; existing memory data is untouched.
    pub(crate) fn reset_memories_config(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            let result = async {
                let service = get_global_config_service()
                    .await
                    .map_err(|error| error.to_string())?;
                service
                    .reset_config(Some("memories"))
                    .await
                    .map_err(|error| error.to_string())?;
                let config: MemoriesConfig = service
                    .get_config(Some("memories"))
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Box::new(config))
            }
            .await;
            if let Err(error) = &result {
                tracing::warn!("Failed to reset memory settings: {}", error);
            }
            let _ = tx.send(RuntimeEvent::MemorySettingsReset(result));
        });
    }

    /// G-SE13: clears the memory database and the memory workspace files, the
    /// two halves of the desktop `reset_memory` command.
    pub(crate) fn clear_memory_data(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.tokio.spawn(async move {
            let result = async {
                let manager = try_get_path_manager_arc().map_err(|error| error.to_string())?;
                MemoryDatabase::new(manager.clone())
                    .reset_memory_state()
                    .await
                    .map_err(|error| format!("Failed to reset memory database: {}", error))?;
                reset_memory_workspace(&manager.memories_root_dir())
                    .await
                    .map_err(|error| format!("Failed to reset memory workspace: {}", error))?;
                Ok(())
            }
            .await;
            if let Err(error) = &result {
                tracing::warn!("Failed to clear memory data: {}", error);
            }
            let _ = tx.send(RuntimeEvent::MemoryCleared(result));
        });
    }

    /// Reloads the first timeline page (shared tail of the mutation calls).
    async fn load_memories_for(tx: mpsc::UnboundedSender<RuntimeEvent>) {
        match crate::trinity::client()
            .call(
                "memory.timeline",
                serde_json::json!({ "limit": crate::trinity::TIMELINE_LIMIT, "full": true }),
            )
            .await
        {
            Ok(value) => {
                let items = crate::trinity::parse_memory_items(&value);
                let exhausted = (items.len() as i64) < crate::trinity::TIMELINE_LIMIT;
                let _ = tx.send(RuntimeEvent::TrinityMemoriesReady {
                    items,
                    exhausted,
                    append: false,
                });
            }
            Err(error) => {
                let _ = tx.send(RuntimeEvent::TrinityMemoriesFailed(error));
            }
        }
    }

    /// M22: saves (or clears, with an empty secret refused by core) the
    /// web-search credential for one provider, then reports the new state.
    pub(crate) fn save_web_search_credential(&self, provider: String, secret: String) {
        self.tokio.spawn(async move {
            let result = openbitfun_core::service::web_search::save_web_search_credential(
                openbitfun_core::service::web_search::SaveWebSearchCredentialRequest {
                    provider: provider.clone(),
                    secret,
                },
            )
            .await;
            match &result {
                Ok(status) => tracing::info!(
                    "WebSearch credential saved for {} (configured: {})",
                    status.provider,
                    status.configured
                ),
                Err(error) => {
                    tracing::warn!("Failed to save WebSearch credential for {provider}: {error}")
                }
            }
            publish(RuntimeEvent::WebSearchStatus(
                result
                    .map(|status| (status.provider, status.configured))
                    .map_err(|error| error.to_string()),
            ));
        });
    }

    /// M22: reads whether one web-search provider has a stored credential.
    pub(crate) fn load_web_search_credential_status(&self, provider: String) {
        self.tokio.spawn(async move {
            let result =
                openbitfun_core::service::web_search::get_web_search_credential_status(&provider)
                    .await;
            publish(RuntimeEvent::WebSearchStatus(
                result
                    .map(|status| (status.provider, status.configured))
                    .map_err(|error| error.to_string()),
            ));
        });
    }

    /// A short-lived handle clone for follow-up work inside a spawned task
    /// (`reload_sessions` needs the workspace + tokio facts only).
    fn clone_handle_after_action(&self) -> RuntimeHandleLite {
        RuntimeHandleLite {
            agent_runtime: self.agent_runtime.clone(),
            compat: self.compat.clone(),
            workspace: self.workspace.clone(),
            tokio: self.tokio.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// M21 management catalog loaders (core service reads, projected into the
// shell-owned row types so the UI layer stays free of MCP/registry types)
// ---------------------------------------------------------------------------

async fn fetch_skill_catalog(root: &Path, workspace_id: &str) -> crate::manage::SkillCatalog {
    use openbitfun_agent_runtime::skills::SkillLocation;
    use openbitfun_core::agentic::tools::implementations::skills::{mode_overrides, SkillRegistry};

    let registry = SkillRegistry::global();
    let skills = registry.get_all_skills_for_workspace(Some(root)).await;
    let disabled_user = mode_overrides::load_globally_disabled_user_skills()
        .await
        .unwrap_or_else(|error| {
            tracing::warn!("Failed to load disabled user skills: {error}");
            Vec::new()
        });
    let disabled_project = mode_overrides::load_globally_disabled_project_skills(
        mode_overrides::SkillPolicyWorkspace { workspace_id, root },
    )
    .await
    .unwrap_or_else(|error| {
        tracing::warn!("Failed to load disabled project skills: {error}");
        Vec::new()
    });
    let rows = skills
        .into_iter()
        .map(|skill| {
            let project_level = matches!(skill.level, SkillLocation::Project);
            let deletable = !skill.is_builtin && skill.is_native();
            crate::manage::SkillRow {
                disabled: crate::manage::SkillRow::disabled_for(
                    &skill.key,
                    !project_level,
                    project_level,
                    &disabled_user,
                    &disabled_project,
                ),
                key: skill.key,
                name: skill.name,
                description: skill.description,
                project_level,
                builtin: skill.is_builtin,
                shadowed: skill.is_shadowed,
                group_key: skill.group_key.filter(|key| !key.trim().is_empty()),
                source_id: skill.source_id,
                source_label: skill.source_label,
                path: skill.path,
                deletable,
            }
        })
        .collect();
    crate::manage::SkillCatalog {
        loaded: true,
        skills: rows,
    }
}

/// G-K3: checks a folder the user typed into the add-skill dialog. The
/// directory must exist, be a directory, and contain a parseable `SKILL.md`;
/// only unexpected IO failures surface as `Err`.
async fn validate_skill_source(path: String) -> Result<crate::manage::SkillValidationRow, String> {
    use openbitfun_core::agentic::tools::implementations::skills::{SkillData, SkillLocation};

    use crate::manage::{SkillValidationError, SkillValidationRow};

    let invalid = |error| {
        Ok(SkillValidationRow {
            valid: false,
            name: None,
            description: None,
            error: Some(error),
        })
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return invalid(SkillValidationError::Empty);
    }
    let skill_path = Path::new(trimmed);
    let metadata = match tokio::fs::metadata(skill_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return invalid(SkillValidationError::NotFound)
        }
        Err(error) => {
            tracing::warn!("Failed to inspect skill path {trimmed}: {error}");
            return invalid(SkillValidationError::Unreadable);
        }
    };
    if !metadata.is_dir() {
        return invalid(SkillValidationError::NotDirectory);
    }
    let content = match tokio::fs::read_to_string(skill_path.join("SKILL.md")).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return invalid(SkillValidationError::MissingSkillFile)
        }
        Err(error) => {
            tracing::warn!("Failed to read SKILL.md under {trimmed}: {error}");
            return invalid(SkillValidationError::Unreadable);
        }
    };
    match SkillData::from_markdown(trimmed.to_string(), &content, SkillLocation::User, false) {
        Ok(data) => Ok(SkillValidationRow {
            valid: true,
            name: Some(data.name),
            description: Some(data.description),
            error: None,
        }),
        Err(error) => invalid(SkillValidationError::Parse(error.to_string())),
    }
}

/// G-K3: copies one validated skill folder into the managed skills root. The
/// source is canonicalized first so a folder cannot be copied into itself, and
/// the target must not already exist (desktop `add_skill`).
async fn add_skill_folder(
    workspace: &WorkspaceInfo,
    source_path: String,
    project_level: bool,
) -> Result<String, String> {
    let validation = validate_skill_source(source_path.clone()).await?;
    if !validation.valid {
        return Err("The selected folder is not a valid skill".to_string());
    }
    let source = PathBuf::from(source_path.trim());
    let paths = try_get_path_manager_arc().map_err(|error| error.to_string())?;
    let target_dir = if project_level {
        paths.project_root(&workspace.root_path).join("skills")
    } else {
        paths.user_skills_dir()
    };
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|error| format!("Failed to create the skills directory: {error}"))?;
    let folder_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Unable to read the skill folder name".to_string())?
        .to_string();
    let canonical_source = tokio::fs::canonicalize(&source)
        .await
        .map_err(|error| format!("Failed to read the skill folder: {error}"))?;
    let canonical_target_dir = tokio::fs::canonicalize(&target_dir)
        .await
        .map_err(|error| format!("Failed to read the skills directory: {error}"))?;
    let target_path = canonical_target_dir.join(&folder_name);
    if target_path.exists() {
        return Err(format!(
            "Skill '{folder_name}' already exists in the {} skills directory",
            if project_level { "project" } else { "user" }
        ));
    }
    if canonical_target_dir.starts_with(&canonical_source) {
        return Err("Cannot copy a skill folder into itself".to_string());
    }
    copy_dir_all(&canonical_source, &target_path)
        .await
        .map_err(|error| format!("Failed to copy the skill folder: {error}"))?;
    Ok(format!(
        "Skill '{}' added successfully",
        validation.name.unwrap_or(folder_name)
    ))
}

/// G-K3: removes one skill bundle after proving the registry path sits under
/// the managed user/project skills root, so stale or hostile records cannot
/// delete anything outside it.
async fn delete_skill_folder(workspace: &WorkspaceInfo, skill_key: &str) -> Result<String, String> {
    use openbitfun_agent_runtime::skills::SkillLocation;
    use openbitfun_core::agentic::tools::implementations::skills::SkillRegistry;

    let registry = SkillRegistry::global();
    let skill = registry
        .find_skill_by_key_for_workspace(skill_key, Some(&workspace.root_path))
        .await
        .ok_or_else(|| format!("Skill '{skill_key}' not found"))?;
    if skill.is_builtin || !skill.is_native() {
        return Err(
            "Only OpenBitFun-owned, non-built-in Skills can be deleted from OpenBitFun".to_string(),
        );
    }
    let paths = try_get_path_manager_arc().map_err(|error| error.to_string())?;
    let managed_root = match skill.level {
        SkillLocation::User => paths.user_skills_dir(),
        SkillLocation::Project => paths.project_root(&workspace.root_path).join("skills"),
    };
    let canonical_root = tokio::fs::canonicalize(&managed_root)
        .await
        .map_err(|error| format!("Failed to read the skills directory: {error}"))?;
    let skill_path = PathBuf::from(&skill.path);
    let metadata = tokio::fs::metadata(&skill_path)
        .await
        .map_err(|error| format!("Failed to read the skill folder: {error}"))?;
    if !metadata.is_dir() {
        return Err("The skill is not stored as a directory bundle".to_string());
    }
    let canonical_skill = tokio::fs::canonicalize(&skill_path)
        .await
        .map_err(|error| format!("Failed to read the skill folder: {error}"))?;
    if !canonical_skill.starts_with(&canonical_root) {
        return Err("Refusing to delete a skill outside the managed skills directory".to_string());
    }
    tokio::fs::remove_dir_all(&canonical_skill)
        .await
        .map_err(|error| format!("Failed to delete the skill folder: {error}"))?;
    Ok(format!("Skill '{}' deleted", skill.name))
}

/// Recursive directory copy for skill bundles (desktop `copy_dir_all`; files
/// follow symlinks the same way `tokio::fs::copy` does).
async fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let metadata = entry.metadata().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if metadata.is_dir() {
            Box::pin(copy_dir_all(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

/// T2-11: the per-agent skill selection projection (web `ModeSkillInfo[]`).
async fn fetch_mode_skill_rows(
    workspace: &openbitfun_core::service::workspace::WorkspaceInfo,
    mode_id: &str,
) -> Vec<crate::manage::ModeSkillRow> {
    use openbitfun_agent_runtime::skills::SkillLocation;
    use openbitfun_core::agentic::tools::implementations::skills::{
        mode_overrides::SkillPolicyWorkspace, SkillRegistry,
    };

    let registry = SkillRegistry::global();
    let policy = SkillPolicyWorkspace {
        workspace_id: &workspace.id,
        root: workspace.root_path.as_path(),
    };
    registry
        .get_mode_skill_infos_for_workspace(Some(policy), mode_id)
        .await
        .into_iter()
        .map(|info| crate::manage::ModeSkillRow {
            key: info.skill.key,
            name: info.skill.name,
            description: info.skill.description,
            project_level: matches!(info.skill.level, SkillLocation::Project),
            builtin: info.skill.is_builtin,
            shadowed: info.skill.is_shadowed,
            source_label: info.skill.source_label,
            shadowed_by_key: info.skill.shadowed_by_key,
            effective_enabled: info.effective_enabled,
            selected_for_runtime: info.selected_for_runtime,
        })
        .collect()
}

/// T2-11: the desktop `replace_mode_skill_selection` write path — validate
/// the keys against the scan, persist the minimal user-override diff, then
/// persist the project disable set for the skills left out.
async fn persist_mode_skill_selection(
    workspace: &openbitfun_core::service::workspace::WorkspaceInfo,
    mode_id: &str,
    enabled_keys: &[String],
) -> Result<(), String> {
    use openbitfun_core::agentic::tools::implementations::skills::{mode_overrides, SkillRegistry};

    let registry = SkillRegistry::global();
    let all_skills = registry
        .get_all_skills_for_workspace(Some(workspace.root_path.as_path()))
        .await;
    let enabled = crate::manage::normalize_skill_key_list(enabled_keys.to_vec());
    let unknown = crate::manage::unknown_skill_keys(&all_skills, &enabled);
    if !unknown.is_empty() {
        return Err(format!(
            "Unknown skill keys for mode '{mode_id}': {}",
            unknown.join(", ")
        ));
    }
    let enabled_set: std::collections::HashSet<String> = enabled.into_iter().collect();
    let (disabled_user, enabled_user) =
        crate::manage::user_mode_skill_override_diff(&all_skills, mode_id, &enabled_set);
    openbitfun_core::service::config::mode_config_canonicalizer::persist_agent_profile_from_value(
        mode_id,
        serde_json::json!({
            "disabled_user_skills": crate::manage::normalize_skill_key_list(disabled_user),
            "enabled_user_skills": crate::manage::normalize_skill_key_list(enabled_user),
        }),
    )
    .await
    .map_err(|error| format!("Failed to update user skill overrides: {error}"))?;
    let disabled_project = crate::manage::disabled_project_skill_keys(&all_skills, &enabled_set);
    let mut document =
        mode_overrides::load_project_mode_skills_document_local(&workspace.root_path)
            .await
            .map_err(|error| format!("Failed to load project mode skills: {error}"))?;
    mode_overrides::set_disabled_mode_skills_in_document(&mut document, mode_id, disabled_project)
        .map_err(|error| format!("Failed to update project skill overrides: {error}"))?;
    mode_overrides::save_project_mode_skills_document_local(&workspace.root_path, &document)
        .await
        .map_err(|error| format!("Failed to save project mode skills: {error}"))?;
    Ok(())
}

/// T2-11: the desktop `reset_mode_skill_selection` clear path — drop the
/// user overrides, then clear the mode's project disable set, removing the
/// project file when the document is left empty.
async fn clear_mode_skill_selection(
    workspace: &openbitfun_core::service::workspace::WorkspaceInfo,
    mode_id: &str,
) -> Result<(), String> {
    use openbitfun_core::agentic::tools::implementations::skills::mode_overrides;
    use openbitfun_core::infrastructure::get_path_manager_arc;

    mode_overrides::clear_user_mode_skill_overrides(mode_id)
        .await
        .map_err(|error| format!("Failed to reset user skill overrides: {error}"))?;
    let path = get_path_manager_arc().project_agent_profiles_file(&workspace.root_path);
    let exists = tokio::fs::try_exists(&path)
        .await
        .map_err(|error| format!("Failed to check project mode skills file: {error}"))?;
    if !exists {
        return Ok(());
    }
    let mut document =
        mode_overrides::load_project_mode_skills_document_local(&workspace.root_path)
            .await
            .map_err(|error| format!("Failed to load project mode skills: {error}"))?;
    mode_overrides::set_disabled_mode_skills_in_document(&mut document, mode_id, Vec::new())
        .map_err(|error| format!("Failed to clear project skill overrides: {error}"))?;
    if document.is_empty() {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Failed to remove project mode skills file: {error}"
            )),
        }
    } else {
        mode_overrides::save_project_mode_skills_document_local(&workspace.root_path, &document)
            .await
            .map_err(|error| format!("Failed to save project mode skills: {error}"))
    }
}

async fn fetch_agent_catalog(workspace_id: Option<&str>) -> crate::manage::AgentCatalog {
    use openbitfun_core::agentic::agents::{get_agent_registry, AgentSource};

    let registry = get_agent_registry();
    let map_source = |source: &AgentSource| match source {
        AgentSource::Builtin => crate::manage::AgentSourceKind::Builtin,
        AgentSource::User => crate::manage::AgentSourceKind::User,
        AgentSource::Project => crate::manage::AgentSourceKind::Project,
        AgentSource::External => crate::manage::AgentSourceKind::External,
    };
    let modes_info = registry
        .get_modes_info_for_workspace(workspace_id, false)
        .await;
    // G-A3: builtin subagents keep their model override in the shared config
    // map (web reads `ai.agent_model_defaults.subagents.builtin`).
    let builtin_model_overrides: std::collections::HashMap<
        String,
        openbitfun_core::service::config::types::SubagentModelSelection,
    > = match openbitfun_core::service::config::get_global_config_service().await {
        Ok(service) => service
            .get_config(Some("ai.agent_model_defaults.subagents.builtin"))
            .await
            .unwrap_or_default(),
        Err(error) => {
            tracing::warn!(
                "Config service unavailable while reading builtin model overrides: {error}"
            );
            Default::default()
        }
    };
    // T2-11: overrides persist under the profile representative — the first
    // mode in list order for each config profile (web `buildProfileMap`).
    let profiles: Vec<(String, Option<String>)> = modes_info
        .iter()
        .map(|info| (info.id.clone(), info.config_profile_id.clone()))
        .collect();
    let representatives = crate::manage::representative_mode_ids(&profiles);
    let modes = modes_info
        .into_iter()
        .map(|info| {
            let skills_mode_id = representatives
                .get(&info.id)
                .cloned()
                .unwrap_or_else(|| info.id.clone());
            crate::manage::AgentCardRow {
                id: info.id,
                name: info.name,
                description: info.description,
                tool_count: info.tool_count,
                readonly: info.is_readonly,
                review: info.is_review,
                source: map_source(&info.source),
                enabled: None,
                // The shell has no mode tool-config editor, so the default
                // tool set is the shell-visible truth (web hasSkillTool).
                skills_manageable: info.default_tools.iter().any(|tool| tool == "Skill"),
                skills_mode_id,
                // G-A3: model overrides apply to subagents only.
                model_override: None,
            }
        })
        .collect();
    let subagents = registry
        .get_subagents_info(workspace_id)
        .await
        .into_iter()
        .map(|info| {
            let external = matches!(info.source, AgentSource::External);
            // G-A3: the override the row menu shows — builtin overrides live
            // in the shared config map, custom ones in the agent file (web
            // `subagentModelOverride`).
            let model_override = if external {
                None
            } else if matches!(info.source, AgentSource::Builtin) {
                builtin_model_overrides.get(&info.id).cloned()
            } else if info.model_is_explicit.unwrap_or(false) {
                info.model
                    .as_deref()
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(crate::settings::subagent_model_selection)
            } else {
                None
            };
            crate::manage::AgentCardRow {
                id: info.id.clone(),
                name: info.name,
                description: info.description,
                tool_count: info.tool_count,
                readonly: info.is_readonly,
                review: info.is_review,
                source: map_source(&info.source),
                enabled: Some(info.effective_enabled),
                skills_manageable: !external
                    && info.default_tools.iter().any(|tool| tool == "Skill"),
                // Subagents persist under their own id (web falls back to
                // the agent id when no mode profile exists).
                skills_mode_id: info.id,
                model_override,
            }
        })
        .collect();
    crate::manage::AgentCatalog {
        loaded: true,
        modes,
        subagents,
    }
}

/// Projects a core MCP status onto the shell-side enum (exhaustive so a new
/// core status fails the build here instead of rendering blank).
fn map_mcp_status(
    status: openbitfun_core::service::mcp::MCPServerStatus,
) -> crate::manage::McpServerState {
    use openbitfun_core::service::mcp::MCPServerStatus;
    match status {
        MCPServerStatus::Uninitialized => crate::manage::McpServerState::Uninitialized,
        MCPServerStatus::Starting => crate::manage::McpServerState::Starting,
        MCPServerStatus::Connected => crate::manage::McpServerState::Connected,
        MCPServerStatus::Healthy => crate::manage::McpServerState::Healthy,
        MCPServerStatus::NeedsAuth => crate::manage::McpServerState::NeedsAuth,
        MCPServerStatus::Reconnecting => crate::manage::McpServerState::Reconnecting,
        MCPServerStatus::Failed => crate::manage::McpServerState::Failed,
        MCPServerStatus::Stopping => crate::manage::McpServerState::Stopping,
        MCPServerStatus::Stopped => crate::manage::McpServerState::Stopped,
    }
}

async fn fetch_mcp_catalog() -> crate::manage::McpCatalog {
    use openbitfun_core::service::mcp::{ConfigLocation, MCPServerType};

    let Some(service) = openbitfun_core::service::mcp::get_global_mcp_service() else {
        // No service means the manager never came up; the page shows the
        // empty state instead of pretending servers exist.
        return crate::manage::McpCatalog {
            loaded: true,
            servers: Vec::new(),
        };
    };
    let configs = service
        .config_service()
        .load_all_configs()
        .await
        .unwrap_or_else(|error| {
            tracing::warn!("Failed to load MCP server configs: {error}");
            Vec::new()
        });
    let manager = service.server_manager();
    let mut servers = Vec::with_capacity(configs.len());
    for config in configs {
        let state = match manager.get_server_status(&config.id).await {
            Ok(status) => map_mcp_status(status),
            Err(error) => {
                tracing::debug!("MCP status unavailable for {}: {error}", config.id);
                crate::manage::McpServerState::Uninitialized
            }
        };
        let state_message = manager
            .get_server_status_message(&config.id)
            .await
            .ok()
            .flatten();
        let remote = config.server_type == MCPServerType::Remote;
        let target = if remote {
            config.url.clone().unwrap_or_default()
        } else {
            let mut command = config.command.clone().unwrap_or_default();
            for argument in &config.args {
                command.push(' ');
                command.push_str(argument);
            }
            command
        };
        servers.push(crate::manage::McpServerRow {
            id: config.id,
            name: config.name,
            remote,
            target,
            state,
            state_message,
            enabled: config.enabled,
            auto_start: config.auto_start,
            builtin: config.location == ConfigLocation::BuiltIn,
        });
    }
    crate::manage::McpCatalog {
        loaded: true,
        servers,
    }
}

/// Flips one server's persisted `enabled` flag and reconciles the runtimes
/// (the same previous/current diff the desktop JSON save path applies).
async fn set_mcp_server_enabled(
    service: &openbitfun_core::service::mcp::MCPService,
    server_id: &str,
    enabled: bool,
) -> Result<(), String> {
    let config_service = service.config_service();
    let previous = config_service
        .load_all_configs()
        .await
        .map_err(|error| error.to_string())?;
    let mut config = config_service
        .get_server_config(server_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("MCP server config not found: {server_id}"))?;
    config.enabled = enabled;
    config_service
        .save_server_config(&config)
        .await
        .map_err(|error| error.to_string())?;
    let current = config_service
        .load_all_configs()
        .await
        .map_err(|error| error.to_string())?;
    service
        .server_manager()
        .reconcile_persisted_configs(previous, current)
        .await
        .map_err(|error| error.to_string())
}

async fn fetch_mcp_server_catalog(
    server_id: &str,
    force: bool,
) -> Result<crate::manage::McpServerCatalogView, String> {
    let Some(service) = openbitfun_core::service::mcp::get_global_mcp_service() else {
        return Err("MCP service is not initialized".to_string());
    };
    let config = service
        .config_service()
        .get_server_config(server_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("MCP server config not found: {server_id}"))?;
    let manager = service.server_manager();
    let mut resources = manager.get_cached_resources(server_id).await;
    let mut prompts = manager.get_cached_prompts(server_id).await;
    // Desktop convention: refresh on request, or when both caches are cold.
    if force || (resources.is_empty() && prompts.is_empty()) {
        if let Err(error) = manager.refresh_server_resource_catalog(server_id).await {
            tracing::warn!("Failed to refresh MCP resources for {server_id}: {error}");
        }
        if let Err(error) = manager.refresh_server_prompt_catalog(server_id).await {
            tracing::warn!("Failed to refresh MCP prompts for {server_id}: {error}");
        }
        resources = manager.get_cached_resources(server_id).await;
        prompts = manager.get_cached_prompts(server_id).await;
    }
    Ok(crate::manage::McpServerCatalogView {
        server_id: server_id.to_string(),
        server_name: config.name,
        resources: resources
            .into_iter()
            .map(|resource| crate::manage::McpResourceRow {
                name: resource.title.clone().unwrap_or(resource.name),
                uri: resource.uri,
                description: resource.description,
            })
            .collect(),
        prompts: prompts
            .into_iter()
            .map(|prompt| crate::manage::McpPromptRow {
                arguments: prompt.arguments.map(|args| args.len()).unwrap_or(0),
                name: prompt.title.clone().unwrap_or(prompt.name),
                description: prompt.description,
            })
            .collect(),
    })
}

/// T2-7: the desktop `save_mcp_json_config` pipeline — parse/validate +
/// fingerprint-checked replace, then reconcile the running servers between
/// the previous and current config sets.
async fn save_mcp_json_config(json_config: &str, expected_fingerprint: &str) -> Result<(), String> {
    let Some(service) = openbitfun_core::service::mcp::get_global_mcp_service() else {
        return Err("MCP service is not initialized".to_string());
    };
    let config_service = service.config_service();
    let previous_configs = config_service
        .load_all_configs()
        .await
        .map_err(|error| error.to_string())?;
    config_service
        .save_mcp_json_config(json_config, expected_fingerprint)
        .await
        .map_err(|error| error.to_string())?;
    let current_configs = config_service
        .load_all_configs()
        .await
        .map_err(|error| error.to_string())?;
    service
        .server_manager()
        .reconcile_persisted_configs(previous_configs, current_configs)
        .await
        .map_err(|error| {
            format!("MCP config was saved, but runtime reconciliation failed: {error}")
        })
}

/// Installs the shell's backend-bus emitter: MCP interactive requests
/// (elicitation/sampling) are forwarded into the UI pump; every other event
/// keeps the previous behavior (dropped — nothing else consumed this bus in
/// the shell process).
fn install_backend_emitter(
    handle: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    handle.spawn(async move {
        get_global_event_system()
            .set_emitter(Arc::new(ShellBackendEmitter { tx }))
            .await;
        tracing::info!("Backend event emitter installed for MCP interactions");
    });
}

/// Forwards MCP interaction asks from the process-global backend bus into the
/// shell event pump.
struct ShellBackendEmitter {
    tx: mpsc::UnboundedSender<RuntimeEvent>,
}

#[async_trait::async_trait]
impl openbitfun_events::EventEmitter for ShellBackendEmitter {
    async fn emit(&self, event_name: &str, payload: serde_json::Value) -> anyhow::Result<()> {
        if event_name == crate::manage::MCP_INTERACTION_EVENT {
            match serde_json::from_value::<crate::manage::McpInteractionRequest>(payload) {
                Ok(request) => {
                    tracing::info!(
                        "MCP interaction requested: server={} method={}",
                        request.server_name,
                        request.method
                    );
                    let _ = self
                        .tx
                        .send(RuntimeEvent::McpInteractionAsked(Box::new(request)));
                }
                Err(error) => {
                    tracing::warn!("Failed to parse MCP interaction request: {error}");
                }
            }
        }
        Ok(())
    }
}

/// The subset of [`RuntimeHandle`] a post-action reload needs. Spawned action
/// tasks take this instead of the whole process-lifetime handle, because the
/// handle itself is owned by the `OnceLock` global.
struct RuntimeHandleLite {
    agent_runtime: AgentRuntime,
    compat: CoreAgentRuntimeCompatibility,
    /// Shared with the owning handle: a reload spawned before a workspace
    /// switch reads the workspace that is active when it runs.
    workspace: Arc<RwLock<WorkspaceInfo>>,
    tokio: tokio::runtime::Handle,
}

/// Session-content search through the compatibility owner's port impl,
/// factored out so callers only map the error shape.
async fn search_session_content_via(
    compat: &CoreAgentRuntimeCompatibility,
    request: SessionContentSearchRequest,
) -> Result<SessionContentSearchResponse, String> {
    use openbitfun_runtime_ports::ProductSearchPort as _;
    compat
        .search_session_content(request)
        .await
        .map_err(|error| error.to_string())
}

impl RuntimeHandleLite {
    fn reload_sessions(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        spawn_session_reload(
            self.agent_runtime.clone(),
            self.compat.clone(),
            self.workspace.clone(),
            self.tokio.clone(),
            tx,
        );
    }

    /// Restores a session transcript into the chat pane. Shared by the owning
    /// handle and by post-action flows that open a fresh session (fork).
    fn open_session(&self, session_id: String, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let runtime = self.agent_runtime.clone();
        let (workspace_id, workspace_path) = {
            let workspace = self
                .workspace
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                Some(workspace.id.clone()),
                workspace.root_path.to_string_lossy().to_string(),
            )
        };
        self.tokio.spawn(async move {
            let request = AgentSessionRestoreRequest {
                workspace_id,
                workspace_path,
                session_id: session_id.clone(),
                include_internal: false,
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.restore_session(request).await {
                Ok(restored) => {
                    let busy = matches!(restored.state, SessionState::Processing { .. });
                    tracing::info!(
                        "Restored session {} (busy={busy})",
                        restored.session.session_id
                    );
                    emit_opened(
                        &runtime,
                        restored.session.session_id,
                        restored.session.session_name,
                        restored.session.agent_type,
                        restored.session.model_id,
                        busy,
                        tx,
                    )
                    .await;
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to restore session {session_id}: {message}");
                    let _ = tx.send(RuntimeEvent::SessionOpenFailed(message));
                }
            }
        });
    }
}

/// Spawns one session-list read: runtime summaries plus the persisted archive
/// flags the summary DTO omits, then one lineage snapshot per listed session
/// (R1c). Lineage reads walk the same workspace metadata, so they run bounded;
/// a failed snapshot logs and renders that root childless instead of failing
/// the whole list.
fn spawn_session_reload(
    runtime: AgentRuntime,
    compat: CoreAgentRuntimeCompatibility,
    workspace: Arc<RwLock<WorkspaceInfo>>,
    tokio: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    let workspace = workspace
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let request = AgentSessionListRequest {
        workspace_id: Some(workspace.id.clone()),
        workspace_path: String::new(),
        remote_connection_id: None,
        remote_ssh_host: None,
    };
    tokio.spawn(async move {
        let archived = archived_session_ids(&compat, &workspace.root_path).await;
        match runtime.list_sessions(request).await {
            Ok(mut sessions) => {
                sort_sessions(&mut sessions);
                let rows = sessions
                    .into_iter()
                    .map(|summary| SessionListRow {
                        archived: archived.contains(&summary.session_id),
                        summary,
                    })
                    .collect::<Vec<_>>();
                tracing::info!("Session list loaded: {} sessions", rows.len());
                let anchors: Vec<String> = rows
                    .iter()
                    .map(|row| row.summary.session_id.clone())
                    .collect();
                let _ = tx.send(RuntimeEvent::Sessions(rows));
                let lineages = fetch_session_lineages(&runtime, &workspace.id, &anchors).await;
                let _ = tx.send(RuntimeEvent::SessionLineages(lineages));
            }
            Err(error) => {
                let message = error.into_message();
                tracing::error!("Failed to list sessions: {message}");
                let _ = tx.send(RuntimeEvent::SessionsFailed(message));
            }
        }
    });
}

/// Lineage snapshots for every listed session (R1c sidebar tree): one
/// subagent-tree snapshot per anchor, fetched with bounded concurrency so a
/// large workspace cannot flood the metadata store with parallel scans. A
/// failed anchor logs and drops out; the sidebar renders that root childless.
///
/// Sessions listed by the runtime are the visible top level (subagent and
/// ephemeral children stay hidden from the index), so anchoring at each listed
/// session covers every tree the sidebar can show.
async fn fetch_session_lineages(
    runtime: &AgentRuntime,
    workspace_id: &str,
    anchors: &[String],
) -> Vec<AgentSessionLineageSnapshot> {
    use futures_util::StreamExt;

    futures_util::stream::iter(anchors.iter().cloned().map(|anchor_session_id| {
        let runtime = runtime.clone();
        let workspace_id = workspace_id.to_string();
        async move {
            let request = AgentSessionLineageRequest {
                workspace_id: Some(workspace_id),
                workspace_path: String::new(),
                anchor_session_id,
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.get_session_lineage(request).await {
                Ok(Some(snapshot)) => Some(snapshot),
                Ok(None) => None,
                Err(error) => {
                    tracing::warn!("Session lineage fetch failed: {error}");
                    None
                }
            }
        }
    }))
    .buffered(LINEAGE_FETCH_CONCURRENCY)
    .filter_map(|snapshot| async move { snapshot })
    .collect()
    .await
}

/// Upper bound on concurrent lineage reads per reload (R1c).
const LINEAGE_FETCH_CONCURRENCY: usize = 8;

/// Persisted ids of archived sessions for one workspace. A failed metadata
/// read degrades to "nothing is archived" with an explicit warning: the list
/// then shows every session rather than hiding rows on a guess.
async fn archived_session_ids(
    compat: &CoreAgentRuntimeCompatibility,
    workspace_root: &std::path::Path,
) -> std::collections::HashSet<String> {
    match compat.list_persisted_sessions(workspace_root).await {
        Ok(metadatas) => metadatas
            .into_iter()
            .filter(|metadata| matches!(metadata.status, SessionStatus::Archived))
            .map(|metadata| metadata.session_id)
            .collect(),
        Err(error) => {
            tracing::warn!("Failed to read session archive states: {error}");
            std::collections::HashSet::new()
        }
    }
}

impl RuntimeHandle {
    /// Creates a new durable session for the shell workspace.
    pub(crate) fn create_session(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.spawn_create_session(None, tx);
    }

    /// Creates a new durable session and submits `first_message` as its
    /// opening turn (R2b, the welcome face's type-to-start input).
    /// `SessionOpened` is queued ahead of the turn's events, so the shell
    /// lands on the new tab before the reply stream starts.
    pub(crate) fn create_session_with_first_message(
        &self,
        first_message: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        self.spawn_create_session(Some(first_message), tx);
    }

    fn spawn_create_session(
        &self,
        first_message: Option<String>,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let workspace = self.workspace();
        let workspace_path = workspace.root_path.to_string_lossy().to_string();
        let workspace_id = Some(workspace.id);
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionCreateRequest {
                session_name: default_session_name(),
                agent_type: DEFAULT_AGENT_TYPE.to_string(),
                agent_route_key: None,
                workspace_path: Some(workspace_path.clone()),
                project_workspace_path: None,
                execution_target: None,
                workspace_id: workspace_id.clone(),
                remote_connection_id: None,
                remote_ssh_host: None,
                model_id: None,
                metadata: Default::default(),
            };
            match runtime.create_session(request).await {
                Ok(created) => {
                    tracing::info!(
                        "Created session {} ({})",
                        created.session_id,
                        created.agent_type
                    );
                    emit_opened(
                        &runtime,
                        created.session_id.clone(),
                        created.session_name,
                        created.agent_type.clone(),
                        created.model_id,
                        false,
                        tx.clone(),
                    )
                    .await;
                    if let Some(message) = first_message {
                        // The fresh session has no recorded agent history to
                        // resolve, so the creation-time agent type is exact.
                        Self::submit_dialog_turn(
                            runtime.clone(),
                            created.session_id,
                            message,
                            Vec::new(),
                            created.agent_type,
                            Some(workspace_path),
                            workspace_id,
                            tx.clone(),
                        )
                        .await;
                    }
                    // The fresh session belongs in the sidebar right away, not
                    // only after its first turn settles.
                    handle.reload_sessions(tx);
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to create session: {message}");
                    let _ = tx.send(RuntimeEvent::SessionOpenFailed(message));
                }
            }
        });
    }

    /// Restores a persisted session into the runtime and reads its transcript.
    pub(crate) fn open_session(&self, session_id: String, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        self.clone_handle_after_action()
            .open_session(session_id, tx)
    }

    /// Submits one dialog turn; streaming output arrives through the event pump.
    /// `attachments` rides along as `remote_image` input attachments — the core
    /// turn pipeline decodes data URLs, persists the bytes, and builds the
    /// model content (see `prepare_inline_image_attachments`).
    pub(crate) fn send_message(
        &self,
        session_id: String,
        message: String,
        attachments: Vec<AgentInputAttachment>,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        let workspace = self.workspace();
        let workspace_path = workspace.root_path.to_string_lossy().to_string();
        let workspace_id = Some(workspace.id);
        self.tokio.spawn(async move {
            // The turn carries the session's own agent; the default only
            // covers sessions that predate agent recording.
            let agent_type = runtime
                .resolve_session_agent_type(&session_id)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| DEFAULT_AGENT_TYPE.to_string());
            Self::submit_dialog_turn(
                runtime,
                session_id,
                message,
                attachments,
                agent_type,
                Some(workspace_path),
                workspace_id,
                tx,
            )
            .await;
        });
    }

    /// One dialog-turn submission shared by the composer send path and the
    /// welcome face's create-then-send path (R2b): builds the request with the
    /// ask-inherit metadata (the CLI interactive default) and reports the
    /// outcome as [`RuntimeEvent::TurnAccepted`] / [`RuntimeEvent::SendFailed`].
    async fn submit_dialog_turn(
        runtime: AgentRuntime,
        session_id: String,
        message: String,
        attachments: Vec<AgentInputAttachment>,
        agent_type: String,
        workspace_path: Option<String>,
        workspace_id: Option<String>,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let turn_id = uuid::Uuid::new_v4().to_string();
        let request = AgentDialogTurnRequest {
            session_id: session_id.clone(),
            message: message.clone(),
            output_schema: None,
            original_message: None,
            turn_id: Some(turn_id.clone()),
            execution: AgentDialogTurnExecution::Standard,
            agent_type,
            workspace_path,
            workspace_id,
            remote_connection_id: None,
            remote_ssh_host: None,
            policy: DialogSubmissionPolicy::for_source(AgentSubmissionSource::Cli),
            reply_route: None,
            prepended_reminders: Vec::new(),
            attachments,
            metadata: Default::default(),
        };
        match runtime.submit_dialog_turn(request).await {
            Ok(_) => {
                let _ = tx.send(RuntimeEvent::TurnAccepted {
                    session_id,
                    turn_id,
                    message,
                });
            }
            Err(error) => {
                let message = error.into_message();
                tracing::error!("Failed to submit turn: {message}");
                let _ = tx.send(RuntimeEvent::SendFailed {
                    session_id,
                    error: message,
                });
            }
        }
    }

    /// Requests cancellation of the session's current turn. The terminal
    /// state arrives as a [`RuntimeEvent::TurnFinished`] through the pump.
    pub(crate) fn cancel_turn(&self, session_id: String) {
        let runtime = self.agent_runtime.clone();
        self.tokio.spawn(async move {
            let request = AgentTurnCancellationRequest {
                session_id,
                turn_id: None,
                source: Some(AgentSubmissionSource::Cli),
                requester_session_id: None,
                reason: Some("user requested cancellation from the GPUI shell".to_string()),
                wait_timeout_ms: None,
                cancel_descendants: true,
            };
            match runtime.cancel_turn(request).await {
                Ok(_) => tracing::info!("Turn cancellation accepted"),
                Err(error) => tracing::error!(
                    "Failed to request turn cancellation: {}",
                    error.into_message()
                ),
            }
        });
    }

    /// Answers one permission request on the user's behalf. The approval UI
    /// clears its prompt when the [`PermissionRequestEvent::Replied`]
    /// broadcast echoes back through the pump, so a failed answer keeps the
    /// prompt on screen (the request is still pending) instead of desyncing
    /// the shell's queue from the runtime.
    pub(crate) fn respond_permission(&self, request_id: String, reply: PermissionReply) {
        let runtime = self.agent_runtime.clone();
        self.tokio.spawn(async move {
            if let Err(error) = runtime.respond_permission(&request_id, reply).await {
                tracing::error!(
                    "Failed to respond to permission request {request_id}: {}",
                    error.into_message()
                );
            }
        });
    }

    /// Submits answers for one pending user question (`ask_user` tool). The
    /// question dialog advances on the mailbox rebuild that follows the
    /// answer, not here, so a lost answer leaves the question on screen
    /// instead of desyncing the shell's queue from the runtime.
    pub(crate) fn respond_user_question(&self, tool_id: String, answers: serde_json::Value) {
        let runtime = self.agent_runtime.clone();
        self.tokio.spawn(async move {
            let request = AgentUserAnswersRequest {
                tool_id: tool_id.clone(),
                answers,
            };
            if let Err(error) = runtime.submit_user_answers(request).await {
                tracing::error!(
                    "Failed to submit answers for question {tool_id}: {}",
                    error.into_message()
                );
            }
        });
    }

    /// Declines one pending user question; the tool reports the cancellation
    /// back to the model. A question that was answered or expired already
    /// reports a missing channel, which is logged and otherwise ignored: the
    /// mailbox rebuild has already removed it from the dialog.
    pub(crate) fn cancel_user_question(&self, session_id: String, tool_id: String) {
        if let Err(error) = self
            .agent_runtime
            .cancel_user_question(&session_id, &tool_id)
        {
            tracing::error!("Failed to cancel question {tool_id}: {error}");
        }
    }

    /// Reads the workspace catalog for the picker: shared recent/opened
    /// records first, then workspaces visited by this process, with the
    /// active workspace always included so a hidden record (the cwd-launched
    /// workspace, or one opened by path) stays selectable.
    pub(crate) fn reload_workspaces(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let active = self.workspace();
        let visited = self
            .visited
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        self.tokio.spawn(async move {
            let mut choices = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let push = |info: &WorkspaceInfo,
                        choices: &mut Vec<WorkspaceChoice>,
                        seen: &mut std::collections::HashSet<String>| {
                if seen.insert(info.id.clone()) {
                    choices.push(WorkspaceChoice::from_info(info));
                }
            };
            push(&active, &mut choices, &mut seen);
            for info in visited {
                push(&info, &mut choices, &mut seen);
            }
            if let Some(service) = get_global_workspace_service() {
                for info in service.get_recent_workspaces().await {
                    push(&info, &mut choices, &mut seen);
                }
                for info in service.get_opened_workspaces().await {
                    push(&info, &mut choices, &mut seen);
                }
            } else {
                tracing::warn!("Workspace catalog unavailable; picker shows this process only");
            }
            let _ = tx.send(RuntimeEvent::Workspaces(choices));
        });
    }

    /// Switches the shell's active workspace. The process-wide Agent Runtime
    /// is not rebuilt (core registers its coordinator once); instead the
    /// workspace facts every request carries are swapped, after the same
    /// ownership check the runtime performs before running a turn, and the
    /// workspace-bound services the shell itself uses (the changes view git
    /// port) are rebuilt for the new root. The shared core catalog's
    /// current/opened/recent selection is left untouched — this is the
    /// per-process workspace semantics, the same seam the CLI and ACP use.
    pub(crate) fn switch_workspace(&self, path: PathBuf, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let ownership = self.ownership.clone();
        let workspace = self.workspace.clone();
        let visited = self.visited.clone();
        let git = self.git.clone();
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let record = match register_local_workspace_record(path.clone()).await {
                Ok(record) => record,
                Err(error) => {
                    let message = format!("failed to open workspace: {error}");
                    tracing::error!("Workspace switch failed to resolve {path:?}: {message}");
                    let _ = tx.send(RuntimeEvent::WorkspaceSwitchFailed(message));
                    return;
                }
            };
            // Same lease the runtime takes before a turn, so a workspace owned
            // by a Shared runtime is rejected here instead of on first use.
            if let Err(error) = ownership.ensure_local_workspace(&record.root_path) {
                let message = ownership.error_message(&error);
                tracing::error!("Workspace switch rejected for {}: {message}", record.name);
                let _ = tx.send(RuntimeEvent::WorkspaceSwitchFailed(message));
                return;
            }
            let git_port =
                match build_local_runtime_services(&record.root_path, RUNTIME_EVENT_BUFFER) {
                    Ok((_, services)) => services.git.clone(),
                    Err(error) => {
                        let message = format!("failed to bind workspace services: {error:#}");
                        tracing::error!("Workspace switch failed for {}: {message}", record.name);
                        let _ = tx.send(RuntimeEvent::WorkspaceSwitchFailed(message));
                        return;
                    }
                };
            {
                let mut active = workspace
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *active = record.clone();
            }
            {
                let mut visited = visited
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                visited.retain(|info| info.id != record.id);
                visited.insert(0, record.clone());
                visited.truncate(VISITED_WORKSPACES_LIMIT);
            }
            {
                let mut slot = git.write().unwrap_or_else(|poisoned| poisoned.into_inner());
                *slot = git_port;
            }
            tracing::info!(
                "Active workspace switched: id={}, root={}",
                record.id,
                record.root_path.display()
            );
            let _ = tx.send(RuntimeEvent::WorkspaceSwitched(WorkspaceChoice::from_info(
                &record,
            )));
            handle.reload_sessions(tx.clone());
            if let Some(handle) = runtime_handle() {
                handle.reload_workspaces(tx);
                // The management catalogs are workspace-scoped (project skills
                // and agents); refresh them together with the sessions.
                handle.reload_skill_catalog();
                handle.reload_agent_catalog();
                handle.reload_mcp_catalog();
            }
        });
    }

    /// Re-reads the transcript of an already-restored session after
    /// [`RuntimeEvent::HistoryChanged`]; arrives as [`RuntimeEvent::SessionOpened`].
    pub(crate) fn reload_transcript(
        &self,
        session_id: String,
        tx: mpsc::UnboundedSender<RuntimeEvent>,
    ) {
        let runtime = self.agent_runtime.clone();
        self.tokio.spawn(async move {
            // The session is already loaded in this process; only the
            // projection is stale. Name/agent facts come from the summary list
            // snapshot the UI already holds, so reuse the last-known values
            // through the transcript-only path below.
            let transcript = match runtime
                .read_session_transcript(SessionTranscriptRequest {
                    session_id: session_id.clone(),
                    turn_id: None,
                })
                .await
            {
                Ok(transcript) => transcript,
                Err(error) => {
                    tracing::warn!(
                        "Failed to reload session transcript: {}",
                        error.into_message()
                    );
                    SessionTranscript {
                        session_id: session_id.clone(),
                        messages: Vec::new(),
                    }
                }
            };
            let entries = convert_transcript(&transcript.messages);
            let turn_times = transcript_turn_times(&transcript.messages);
            let _ = tx.send(RuntimeEvent::SessionOpened(OpenedSession {
                session_id,
                // Reload keeps the current pane title; the list refresh on turn
                // settlement picks up any rename.
                name: String::new(),
                agent_type: String::new(),
                model_id: None,
                entries,
                turn_times,
                reload: true,
                busy: false,
            }));
        });
    }
}

/// Reads the transcript for `session_id` and reports it as an opened session.
/// Chunks streamed between the transcript read and this call are dropped —
/// the shell accepts that small gap for sessions opened mid-turn (same as the
/// CLI, which re-attaches to the live stream without replaying history).
async fn emit_opened(
    runtime: &AgentRuntime,
    session_id: String,
    name: String,
    agent_type: String,
    model_id: Option<String>,
    busy: bool,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    let transcript = match runtime
        .read_session_transcript(SessionTranscriptRequest {
            session_id: session_id.clone(),
            turn_id: None,
        })
        .await
    {
        Ok(transcript) => transcript,
        Err(error) => {
            tracing::warn!(
                "Failed to read session transcript: {}",
                error.into_message()
            );
            SessionTranscript {
                session_id: session_id.clone(),
                messages: Vec::new(),
            }
        }
    };
    let entries = convert_transcript(&transcript.messages);
    let turn_times = transcript_turn_times(&transcript.messages);
    let _ = tx.send(RuntimeEvent::SessionOpened(OpenedSession {
        session_id,
        name,
        agent_type,
        model_id,
        entries,
        turn_times,
        reload: false,
        busy,
    }));
}

/// G-S1: per-turn wall-clock timing from a persisted transcript — the first
/// and last message timestamps of each turn (the web reads the same facts off
/// its persisted dialog turns). A turn whose messages carry no timestamps
/// stays absent; its footer then shows no meta.
pub(crate) fn transcript_turn_times(messages: &[TranscriptMessage]) -> HashMap<String, TurnTimes> {
    let mut times: HashMap<String, TurnTimes> = HashMap::new();
    for message in messages {
        let Some(turn_id) = message.turn_id.as_deref() else {
            continue;
        };
        let Some(at) = message.timestamp_ms else {
            continue;
        };
        let entry = times.entry(turn_id.to_string()).or_default();
        entry.started_at_ms = Some(entry.started_at_ms.map_or(at, |first| first.min(at)));
        entry.ended_at_ms = Some(entry.ended_at_ms.map_or(at, |last| last.max(at)));
    }
    times
}

/// Converts a persisted transcript into shell chat entries.
///
/// Assistant rounds persist as one `Mixed` message (text plus every call with
/// its arguments) followed by one `ToolResult` message per call. The shell
/// emits one assistant entry plus one card per call, so a historical card
/// carries the same input facts as a live one — the pairing pass below holds
/// the call arguments until the result arrives.
pub(crate) fn convert_transcript(messages: &[TranscriptMessage]) -> Vec<ChatEntry> {
    let mut calls: HashMap<String, (String, serde_json::Value)> = HashMap::new();
    let mut entries = Vec::with_capacity(messages.len());
    for message in messages {
        match &message.content {
            TranscriptContent::Text(text) => entries.push(match message.role.as_str() {
                "assistant" => ChatEntry::Assistant {
                    round_id: message.turn_id.clone().unwrap_or_default(),
                    text: text.clone(),
                },
                _ => ChatEntry::User {
                    text: text.clone(),
                    turn_id: message.turn_id.clone(),
                },
            }),
            TranscriptContent::Multimodal { text, image_count } => {
                let mut text = text.clone();
                if *image_count > 0 {
                    text.push_str(&format!("\n\n[{image_count} image(s) attached]"));
                }
                entries.push(ChatEntry::User {
                    text,
                    turn_id: message.turn_id.clone(),
                });
            }
            TranscriptContent::Mixed {
                text, tool_calls, ..
            } => {
                if !text.trim().is_empty() {
                    entries.push(ChatEntry::Assistant {
                        round_id: message.turn_id.clone().unwrap_or_default(),
                        text: text.clone(),
                    });
                }
                for call in tool_calls {
                    calls.insert(
                        call.tool_id.clone(),
                        (call.tool_name.clone(), call.arguments.clone()),
                    );
                }
            }
            TranscriptContent::ToolResult {
                tool_id,
                tool_name,
                effective_tool_name,
                result,
                is_error,
            } => {
                let (call_name, arguments) = calls
                    .remove(tool_id)
                    .unwrap_or_else(|| (tool_name.clone(), serde_json::Value::Null));
                // The deferred gateway reports the inner tool as its effective
                // name; that is the identity a reader recognizes.
                let display_name = effective_tool_name.clone().unwrap_or(call_name);
                let mut facts = transcript::facts_from_input(&display_name, &arguments);
                transcript::apply_result(&mut facts, &display_name, result);
                if *is_error {
                    facts.error = Some(transcript::first_line(&result.to_string()));
                }
                entries.push(ChatEntry::Tool(ToolCard {
                    key: SharedString::from(format!("tool-{tool_id}")),
                    call_id: tool_id.clone(),
                    tool_name: display_name,
                    status: if *is_error {
                        ToolStatus::Failed
                    } else {
                        ToolStatus::Done
                    },
                    facts,
                    result_success: result.get("success").and_then(serde_json::Value::as_bool),
                    subagent_session: None,
                    background: false,
                }));
            }
        }
    }
    entries
}

/// Renders a persisted transcript as Markdown for the export flow: roles
/// become sections, tool results fenced JSON blocks. Multimodal entries carry
/// attachment counts, matching the in-shell projection.
pub(crate) fn transcript_to_markdown(transcript: &SessionTranscript) -> String {
    let mut out = format!("# Session {}\n\n", transcript.session_id);
    for message in &transcript.messages {
        match &message.content {
            TranscriptContent::Text(text) => {
                let role = if message.role == "assistant" {
                    "Assistant"
                } else {
                    "User"
                };
                push_markdown_section(&mut out, role, text);
            }
            TranscriptContent::Multimodal { text, image_count } => {
                let mut text = text.clone();
                if *image_count > 0 {
                    text.push_str(&format!("\n\n[{image_count} image(s) attached]"));
                }
                push_markdown_section(&mut out, "User", &text);
            }
            TranscriptContent::ToolResult {
                tool_name, result, ..
            } => {
                push_markdown_tool(&mut out, tool_name, &result.to_string());
            }
            TranscriptContent::Mixed {
                text, tool_calls, ..
            } => {
                push_markdown_section(&mut out, "Assistant", text);
                for call in tool_calls {
                    push_markdown_tool(&mut out, &call.tool_name, &call.arguments.to_string());
                }
            }
        }
    }
    out
}

fn push_markdown_section(out: &mut String, role: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    out.push_str(&format!("## {role}\n\n{text}\n\n"));
}

fn push_markdown_tool(out: &mut String, tool_name: &str, body: &str) {
    out.push_str(&format!(
        "### Tool `{tool_name}`\n\n```json\n{body}\n```\n\n"
    ));
}

/// Char-based preview truncation shared by tool cards and permission prompts.
pub(crate) fn truncate_chars(value: &str, limit: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        let truncated: String = value.chars().take(limit).collect();
        format!("{truncated}…")
    }
}

/// Subscribes to the runtime event broadcast once and forwards translated
/// events to the UI for the process lifetime.
pub(crate) fn spawn_event_pump(
    tokio: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    tokio.spawn(async move {
        let Some(handle) = runtime_handle() else {
            tracing::error!("Event pump skipped: runtime handle missing");
            return;
        };
        let mut rx = match handle.agent_runtime.subscribe_events() {
            Ok(rx) => rx,
            Err(error) => {
                tracing::error!(
                    "Failed to subscribe to runtime events: {}",
                    error.into_message()
                );
                return;
            }
        };
        tracing::info!("Runtime event pump attached");
        // Turn-outcome bookkeeping for scheduled jobs (run state, history).
        // Resolved once: the pump starts after the bootstrap has installed
        // the global service.
        let cron_feeder = openbitfun_core::service::cron::get_global_cron_service()
            .map(openbitfun_core::service::cron::CronEventSubscriber::new);
        loop {
            match rx.recv().await {
                Ok(envelope) => {
                    if let Some(feeder) = cron_feeder.as_ref() {
                        let _ = feeder.on_event(&envelope.event).await;
                    }
                    for event in translate_event(envelope.event) {
                        let _ = tx.send(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("Runtime event pump lagged; {skipped} events dropped");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Runtime event source closed");
                    break;
                }
            }
        }
    });
}

/// Subscribes to the process-local permission broadcast and forwards the
/// lifecycle events the shell must answer for the process lifetime. A lagging
/// receiver cannot be trusted (dropped `Asked`/`Resolved` events strand
/// prompts or turns), so it rebuilds from the pending snapshot instead.
pub(crate) fn spawn_permission_pump(
    tokio: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    tokio.spawn(async move {
        let Some(handle) = runtime_handle() else {
            tracing::error!("Permission pump skipped: runtime handle missing");
            return;
        };
        let mut rx = match handle.agent_runtime.subscribe_permission_requests() {
            Ok(rx) => rx,
            Err(error) => {
                tracing::error!(
                    "Failed to subscribe to permission requests: {}",
                    error.into_message()
                );
                return;
            }
        };
        tracing::info!("Permission request pump attached");
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Some(event) = translate_permission_event(event) {
                        let _ = tx.send(event);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("Permission pump lagged; {skipped} events dropped");
                    match handle.agent_runtime.pending_permission_requests() {
                        Ok(requests) => {
                            let _ = tx.send(RuntimeEvent::PermissionSnapshot(requests));
                        }
                        Err(error) => {
                            tracing::error!(
                                "Failed to snapshot pending permissions: {}",
                                error.into_message()
                            );
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Permission request source closed");
                    break;
                }
            }
        }
    });
}

/// Watches the process-local user-question mailbox and republishes the full
/// pending set on every revision. The mailbox only publishes a coalesced
/// invalidation (no payloads), so the shell rebuilds from the snapshot: an
/// answer, a cancellation, a timeout, and a turn cancellation all remove the
/// question through the same path, and a question that predates the
/// subscription still surfaces on the first pass.
pub(crate) fn spawn_question_pump(
    tokio: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    tokio.spawn(async move {
        let manager = get_user_input_manager();
        let mut changes = manager.subscribe_changes();
        let mut announced: std::collections::HashSet<String> = std::collections::HashSet::new();
        tracing::info!("User question pump attached");
        loop {
            publish_pending_questions(manager, &mut announced, &tx);
            if changes.changed().await.is_err() {
                tracing::info!("User question mailbox closed");
                break;
            }
        }
    });
}

/// Reads every session's pending questions (the mailbox exposes no
/// process-wide list), deduplicates by tool id, and marks newly seen
/// questions as attended: displaying one in the shell removes its unattended
/// timeout, so it cannot expire while the user is deciding.
fn publish_pending_questions(
    manager: &UserInputManager,
    announced: &mut std::collections::HashSet<String>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
) {
    let mut session_ids = manager
        .pending_question_counts()
        .into_keys()
        .collect::<Vec<_>>();
    // Sorted so the snapshot is deterministic even when one question is
    // visible from both its own session and a controller session.
    session_ids.sort();
    let mut seen = std::collections::HashSet::new();
    let mut questions = Vec::new();
    for session_id in session_ids {
        for question in manager.pending_question_snapshot(&session_id).questions {
            if seen.insert(question.tool_id.clone()) {
                questions.push(question);
            }
        }
    }
    questions.sort_by(|a, b| {
        a.registered_at_ms
            .cmp(&b.registered_at_ms)
            .then_with(|| a.tool_id.cmp(&b.tool_id))
    });
    for question in &questions {
        if announced.insert(question.tool_id.clone()) {
            if let Err(error) = manager.start_interaction(&question.session_id, &question.tool_id) {
                // Raced with a turn cancellation between snapshot and mark.
                tracing::debug!(
                    "Could not mark question {} as attended: {}",
                    question.tool_id,
                    error
                );
            }
        }
    }
    announced.retain(|tool_id| {
        questions
            .iter()
            .any(|question| &question.tool_id == tool_id)
    });
    let _ = tx.send(RuntimeEvent::QuestionsChanged(questions));
}

/// Maps one permission lifecycle event onto the shell's view-facing events.
/// Every ask is surfaced without session filtering: this process's manager
/// only serves shell-driven turns, and an unanswered ask parks its turn at
/// the tool pipeline until cancellation (subagent asks included — the parent
/// turn waits on the child).
fn translate_permission_event(event: PermissionRequestEvent) -> Option<RuntimeEvent> {
    match event {
        PermissionRequestEvent::Asked { request } => {
            Some(RuntimeEvent::PermissionAsked { request })
        }
        PermissionRequestEvent::Replied { request_id, .. }
        | PermissionRequestEvent::Cancelled { request_id, .. } => {
            Some(RuntimeEvent::PermissionResolved { request_id })
        }
    }
}

/// Maps runtime events onto the shell's view-facing event set. Everything the
/// chat pane cannot render (token usage, compression, round diagnostics,
/// subagent-internal sessions) is dropped here with a debug trace.
fn translate_event(event: AgenticEvent) -> Vec<RuntimeEvent> {
    match event {
        AgenticEvent::DialogTurnStarted {
            session_id,
            turn_id,
            ..
        } => vec![RuntimeEvent::TurnStarted {
            session_id,
            turn_id,
        }],
        AgenticEvent::TextChunk {
            session_id,
            turn_id,
            round_id,
            text,
            ..
        } => vec![RuntimeEvent::TextChunk {
            session_id,
            turn_id,
            round_id,
            text,
        }],
        AgenticEvent::ToolEvent {
            session_id,
            tool_event,
            ..
        } => vec![RuntimeEvent::ToolUpdate {
            session_id,
            card: convert_tool_event(tool_event),
        }],
        AgenticEvent::DialogTurnCompleted {
            session_id,
            success,
            duration_ms,
            ..
        } => vec![RuntimeEvent::TurnFinished {
            session_id,
            error: match success {
                Some(false) => Some("turn completed unsuccessfully".to_string()),
                _ => None,
            },
            duration_ms: Some(duration_ms),
            cancelled: false,
        }],
        AgenticEvent::DialogTurnFailed {
            session_id, error, ..
        } => vec![RuntimeEvent::TurnFinished {
            session_id,
            error: Some(error),
            duration_ms: None,
            cancelled: false,
        }],
        AgenticEvent::DialogTurnCancelled { session_id, .. }
        | AgenticEvent::DialogTurnInterrupted { session_id, .. } => {
            vec![RuntimeEvent::TurnFinished {
                session_id,
                error: None,
                duration_ms: None,
                cancelled: true,
            }]
        }
        AgenticEvent::SessionHistoryChanged { session_id, .. } => {
            vec![RuntimeEvent::HistoryChanged { session_id }]
        }
        AgenticEvent::SubagentSessionLinked {
            session_id,
            parent_session_id,
            parent_tool_call_id,
            agent_type,
            ..
        } => vec![RuntimeEvent::SubagentLinked {
            parent_session_id,
            parent_tool_call_id,
            subagent_session_id: session_id,
            agent_type,
        }],
        AgenticEvent::SystemError {
            session_id: Some(session_id),
            error,
            ..
        } => vec![RuntimeEvent::TurnFinished {
            session_id,
            error: Some(error),
            duration_ms: None,
            cancelled: false,
        }],
        other => {
            tracing::debug!("Dropping runtime event for UI: {:?}", other.session_id());
            Vec::new()
        }
    }
}

/// Collapses the tool lifecycle onto the four card states the shell renders.
/// The input-side facts come from the `Started`/`Confirmed` params (the only
/// events that carry them); result-side counts are folded in on completion.
fn convert_tool_event(event: ToolEventData) -> ToolCard {
    let (tool_id, tool_name) = {
        let identity = event.identity();
        (identity.tool_id.clone(), identity.tool_name.clone())
    };
    let key = SharedString::from(format!("tool-{tool_id}"));
    let card = |status: ToolStatus, facts: transcript::ToolFacts| ToolCard {
        key: key.clone(),
        call_id: tool_id.clone(),
        tool_name: tool_name.clone(),
        status,
        facts,
        result_success: None,
        subagent_session: None,
        background: false,
    };
    // Result-side events carry no input: the facts they build are empty and
    // the shell merges them into the card the `Started` event opened.
    let result_facts = |result: &serde_json::Value| {
        let mut facts = transcript::facts_from_input(&tool_name, &serde_json::Value::Null);
        transcript::apply_result(&mut facts, &tool_name, result);
        facts
    };
    match event {
        ToolEventData::Started { params, .. } => ToolCard {
            // The web backgroundSubagentActivity signal: a task tool that
            // started with `run_in_background` outlives its parent's turn.
            background: params.get("run_in_background") == Some(&serde_json::Value::Bool(true)),
            ..card(
                ToolStatus::Running,
                transcript::facts_from_input(&tool_name, &params),
            )
        },
        ToolEventData::Progress { message, .. } => {
            let mut facts = transcript::ToolFacts::empty();
            if !message.trim().is_empty() {
                facts
                    .meta
                    .push(transcript::ToolMeta::Text(transcript::first_line(&message)));
            }
            card(ToolStatus::Running, facts)
        }
        ToolEventData::ConfirmationNeeded { params, .. } => card(
            ToolStatus::WaitingConfirm,
            transcript::facts_from_input(&tool_name, &params),
        ),
        ToolEventData::Rejected { .. } => {
            card(ToolStatus::Cancelled, transcript::ToolFacts::empty())
        }
        ToolEventData::Completed { result, .. } => {
            let mut card = card(ToolStatus::Done, result_facts(&result));
            card.result_success = result.get("success").and_then(serde_json::Value::as_bool);
            card
        }
        ToolEventData::Failed { error, .. } => {
            let mut facts = transcript::ToolFacts::empty();
            facts.error = Some(transcript::first_line(&error));
            card(ToolStatus::Failed, facts)
        }
        ToolEventData::Cancelled { reason, .. } => {
            let mut facts = transcript::ToolFacts::empty();
            if !reason.trim().is_empty() {
                facts.error = Some(transcript::first_line(&reason));
            }
            card(ToolStatus::Cancelled, facts)
        }
        ToolEventData::EarlyDetected { .. }
        | ToolEventData::ParamsPartial { .. }
        | ToolEventData::Queued { .. }
        | ToolEventData::Waiting { .. }
        | ToolEventData::Streaming { .. }
        | ToolEventData::StreamChunk { .. }
        | ToolEventData::Confirmed { .. } => {
            card(ToolStatus::Running, transcript::ToolFacts::empty())
        }
    }
}

/// Spawns the embedded runtime bootstrap on the shell's tokio runtime.
/// Progress and outcomes are reported through `tx`.
pub(crate) fn spawn_bootstrap(
    tokio: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    let bootstrap_tx = tx.clone();
    tokio.spawn(async move {
        if let Err(error) = bootstrap(bootstrap_tx.clone()).await {
            tracing::error!("Embedded runtime bootstrap failed: {error:#}");
            let _ = bootstrap_tx.send(RuntimeEvent::Failed(format!("{error:#}")));
        }
    });
    // R7c test channel (design §7.7): an env-named file plugin opens on a
    // dedicated extension host next to the main bootstrap. Its bridge events
    // ride the same RuntimeEvent pump.
    if let Some(plugin_dir) = crate::plugin_ui::host_channel_plugin_dir() {
        crate::plugin_ui::spawn_host_channel(tokio, plugin_dir, tx);
    }
}

/// Assembles the embedded runtime following the CLI order of operations.
async fn bootstrap(tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<()> {
    // Management actions publish through this sender; wire it before any
    // reload can fire so nothing is dropped on the startup path.
    let _ = SHELL_EVENT_TX.set(tx.clone());
    // Delivery profile must be selected before configuration canonicalization
    // initializes the global tool registry (same as CLI).
    select_agentic_system_profile(DeliveryProfile::Cli)
        .context("Failed to select agentic system delivery profile")?;

    initialize_global_config()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to initialize global config service: {error}"))?;
    tracing::info!("Global config service initialized");

    // The shared config owns the shell locale (`app.language`), the theme
    // (`appearance.selection`), and the tool permission policy. Read one
    // snapshot; the UI thread applies it (theme and locale need gpui state).
    // A failed read keeps the shell on its defaults instead of blocking the
    // bootstrap.
    let mut auto_update_enabled = true;
    match get_global_config_service().await {
        Ok(service) => match service.get_config::<GlobalConfig>(None).await {
            Ok(config) => {
                auto_update_enabled = config.app.auto_update;
                let _ = tx.send(RuntimeEvent::Config(Box::new(
                    crate::settings::ShellConfig::from_global(&config),
                )));
            }
            Err(error) => tracing::warn!("Failed to read settings snapshot: {}", error),
        },
        Err(error) => tracing::warn!(
            "Config service unavailable for settings snapshot: {}",
            error
        ),
    }
    // Locale from the config for any consumer that runs before the snapshot
    // is applied on the UI thread (same source, same fallback).
    let locale = get_app_language().await;
    tracing::debug!("Bootstrap locale resolved: {}", locale.as_str());

    // M22: the tray comes up early so it is present while the runtime boots.
    // Label resolution needs the i18n service (already initialized on the UI
    // thread before the runtime bootstrap starts).
    crate::tray::spawn(locale, tx.clone()).await;
    // M22: launch-at-login state for the general settings page; one stat call
    // against the platform autostart entry.
    let autostart = crate::tray::autostart::is_enabled().unwrap_or(false);
    let _ = tx.send(RuntimeEvent::AutostartState(autostart));

    let workspace_root = std::env::current_dir().context("Failed to read current directory")?;
    let path_manager =
        try_get_path_manager_arc().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let runtime_ownership = CoreRuntimeOwnership::fixed_workspace(
        path_manager.as_ref(),
        ENTRYPOINT,
        &workspace_root,
        RuntimeDeployment::Embedded,
    )
    .map_err(|error| {
        anyhow::anyhow!(error.startup_message(RuntimeDeployment::Embedded, ENTRYPOINT))
    })?;
    let runtime_ownership = Arc::new(runtime_ownership);
    // The same guard is kept on the handle so a workspace switch takes the
    // identical lease check the runtime applies before a turn.
    tracing::info!(
        "Agent runtime ownership acquired for workspace {}",
        workspace_root.display()
    );

    AIClientFactory::initialize_global()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to initialize global AIClientFactory: {error}"))?;

    let agentic_system = init_agentic_system_for_profile_with_runtime_ownership(
        DeliveryProfile::Cli,
        runtime_ownership.clone(),
    )
    .await
    .context("Failed to initialize agentic system")?;
    // Same coordinator ports the CLI wrapper installs for execution paths.
    agentic_system
        .coordinator
        .set_terminal_port(CoreRuntimeServicesProvider::terminal_port());
    agentic_system
        .coordinator
        .set_remote_exec_port(CoreRuntimeServicesProvider::remote_exec_port());
    tracing::info!("Agentic system initialized");

    let workspace = register_local_workspace_record(workspace_root).await?;
    tracing::info!(
        "Workspace record registered: id={}, root={}",
        workspace.id,
        workspace.root_path.display()
    );

    let scheduler = ensure_product_dialog_scheduler(&agentic_system);
    // Persistence-level session facts (archive state) for the sidebar; the
    // runtime ports surface no archive flag on session summaries.
    let compatibility =
        CoreAgentRuntimeCompatibility::build(agentic_system.coordinator.clone(), scheduler.clone());
    let (_, services) = build_local_runtime_services(&workspace.root_path, RUNTIME_EVENT_BUFFER)
        .context("Failed to build local runtime services")?;
    // M8: keep the workspace git port for the changes view before the runtime
    // consumes the rest of the service set. The port is registered by the same
    // local provider that serves the agent's git tools.
    let git_port = services.git.clone();
    tracing::info!(
        "Workspace git port: {}",
        if git_port.is_some() {
            "available"
        } else {
            "unavailable"
        }
    );
    let parts = ProductAssembler::new()
        .assemble(ProductAssemblyInput::new(DeliveryProfile::Cli, services))
        .context("Failed to assemble GPUI shell product runtime")?;
    tracing::info!(
        "Product runtime assembled: profile={}, services={}, plugin_runtime={:?}",
        parts.plan().profile().id(),
        parts.service_availability().len(),
        parts.plugin_runtime().availability(),
    );
    let (services, _disabled_plugin_runtime) = parts.into_runtime_parts();

    let event_queue_owner = CoreProductEventQueueOwner::new(agentic_system.event_queue.clone());
    let cron_scheduler = scheduler.clone();
    let agent_runtime = CoreProductAgentRuntime::build_with_event_source(
        agentic_system.coordinator.clone(),
        scheduler,
        agentic_system.token_usage_service.clone(),
        event_queue_owner.runtime_source(),
        services,
    )
    .map_err(anyhow::Error::msg)
    .context("Failed to build Agent Runtime SDK")?;

    // M14: the shell hosts the same cron service the desktop hosts — the
    // shared jobs.json store guarded by the scheduling lease, the management
    // API for the scheduled-jobs page, and the runner loop for due jobs.
    // Turn-outcome bookkeeping rides the shell's own event pump
    // (CronEventSubscriber has no registration surface on AgentRuntime).
    let cron_service = openbitfun_core::service::cron::CronService::new(
        path_manager.clone(),
        agentic_system.coordinator.clone(),
        cron_scheduler,
    )
    .await
    .context("Failed to initialize cron service")?;
    openbitfun_core::service::cron::set_global_cron_service(cron_service.clone());
    cron_service.start();
    tracing::info!("Cron service initialized and started");

    // M16: the shell hosts the workspace search service the desktop hosts —
    // the managed daemon lifecycle (with ripgrep/scan fallbacks) behind the
    // same config gate; availability is checked per search, not here.
    openbitfun_core::service::search::set_global_workspace_search_service(Arc::new(
        openbitfun_core::service::search::WorkspaceSearchService::new(),
    ));
    tracing::info!("Workspace search service initialized");

    // M21: the shell hosts the MCP service the CLI host hosts — the global
    // manager over the shared config store, servers started off the bootstrap
    // path so a slow spawn never blocks the window. Without it, MCP tools
    // never register and there is nothing to manage.
    match get_global_config_service().await {
        Ok(config_service) => {
            match openbitfun_core::service::mcp::MCPService::new(config_service) {
                Ok(service) => {
                    let service = Arc::new(service);
                    openbitfun_core::service::mcp::set_global_mcp_service(service.clone());
                    tracing::info!("MCP service initialized");
                    tokio::spawn(async move {
                        match service.server_manager().initialize_all().await {
                            Ok(()) => tracing::info!("MCP servers initialized"),
                            Err(error) => {
                                tracing::warn!("Failed to initialize MCP servers: {error}")
                            }
                        }
                    });
                }
                Err(error) => {
                    tracing::warn!("Failed to initialize MCP service: {error}");
                }
            }
        }
        Err(error) => tracing::warn!("Config service unavailable for MCP service: {error}"),
    }

    let handle = RuntimeHandle {
        agent_runtime,
        compat: compatibility,
        workspace: Arc::new(RwLock::new(workspace.clone())),
        visited: Arc::new(RwLock::new(vec![workspace])),
        ownership: runtime_ownership.clone(),
        tokio: tokio::runtime::Handle::current(),
        git: Arc::new(RwLock::new(git_port)),
        _event_queue_owner: event_queue_owner,
    };
    let _ = RUNTIME_HANDLE.set(handle);
    let _ = tx.send(RuntimeEvent::Ready);
    tracing::info!("Embedded runtime ready");

    // Attach before any session work so turn events for sessions opened later
    // are always visible to the pump. The permission pump rides along, and the
    // initial snapshot surfaces any request that predates the subscription.
    spawn_event_pump(tokio::runtime::Handle::current(), tx.clone());
    spawn_permission_pump(tokio::runtime::Handle::current(), tx.clone());
    spawn_question_pump(tokio::runtime::Handle::current(), tx.clone());
    // M21: MCP interactive requests ride the process-global backend event bus.
    // The shell installs its emitter so an eliciting server cannot strand a
    // running turn with an invisible ask.
    install_backend_emitter(tokio::runtime::Handle::current(), tx.clone());
    if let Some(handle) = runtime_handle() {
        handle.reload_sessions(tx.clone());
        handle.reload_model_catalog(tx.clone());
        handle.reload_workspaces(tx.clone());
        handle.reload_skill_catalog();
        handle.reload_agent_catalog();
        handle.reload_mcp_catalog();
        match handle.agent_runtime.pending_permission_requests() {
            Ok(requests) if !requests.is_empty() => {
                tracing::info!("{} permission requests pending at startup", requests.len());
                let _ = tx.send(RuntimeEvent::PermissionSnapshot(requests));
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    "Failed to read pending permissions at startup: {}",
                    error.into_message()
                );
            }
        }
    }

    // M22: the auto-update preference gates the automatic check the same way
    // the desktop store does — the toggle only silences this startup probe,
    // never the manual button. Delayed so the check never competes with the
    // boot path; a failure stays a log line for the automatic case.
    // T1-3: the probe itself runs at most once a day, anchored to the
    // persisted snapshot (the desktop storage contract); inside the window
    // the recorded verdict is restored into the UI instead of re-asking the
    // feed.
    if auto_update_enabled {
        let tx = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            match crate::update::startup_check_decision(crate::update::unix_time_ms()) {
                crate::update::StartupCheckDecision::Due => {
                    match crate::update::check_for_update(None).await {
                        Ok(report) => {
                            let _ =
                                tx.send(RuntimeEvent::UpdateCheckFinished { result: Ok(report) });
                        }
                        Err(error) => tracing::info!("Startup update check skipped: {error:#}"),
                    }
                }
                crate::update::StartupCheckDecision::Cached(report) => {
                    let _ = tx.send(RuntimeEvent::UpdateCheckFinished { result: Ok(report) });
                }
            }
        });
    }
    Ok(())
}

/// Registers a hidden workspace record for the shell's working directory,
/// sharing the workspace catalog with the desktop and CLI without touching
/// their opened/recent selection (same contract as the CLI).
async fn register_local_workspace_record(path: PathBuf) -> Result<WorkspaceInfo> {
    let service = if let Some(service) = get_global_workspace_service() {
        service
    } else {
        let service = Arc::new(
            WorkspaceService::new()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
        );
        set_global_workspace_service(service.clone());
        service
    };
    service
        .register_local_workspace_record(path)
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn default_session_name() -> String {
    format!(
        "GPUI Session - {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )
}

/// Most recent first; stable on the session id for identical timestamps.
pub(crate) fn sort_sessions(sessions: &mut [AgentSessionSummary]) {
    sessions.sort_by(|a, b| {
        b.last_active_at_ms
            .cmp(&a.last_active_at_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
}

/// Local-time rendering of a session's `last_active_at_ms`.
pub(crate) fn format_last_active(last_active_at_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(last_active_at_ms as i64)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Wall-clock milliseconds, the unit the shell's timing rows use (the same
/// clock the session metadata records `last_active_at` in).
pub(crate) fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// G-S1: the turn footer's completion time, local `HH:MM:SS` (the web
/// `ModelRoundItem` formats the same clock shape).
pub(crate) fn format_clock_time(at_ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms as i64)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

/// G-S1: the turn footer's duration, shaped like the web's
/// `formatElapsedDuration` (`820ms`, `12.3s`, `5m30s`, `2h5m`).
pub(crate) fn format_elapsed_duration(duration_ms: u64) -> String {
    if duration_ms < 1000 {
        return format!("{duration_ms}ms");
    }
    let seconds = duration_ms as f64 / 1000.0;
    if seconds < 60.0 {
        let rounded = (seconds * 10.0).round() / 10.0;
        if rounded.fract() == 0.0 {
            return format!("{}s", rounded as u64);
        }
        return format!("{rounded}s");
    }
    let whole_seconds = seconds.round() as u64;
    let minutes = whole_seconds / 60;
    let remaining_seconds = whole_seconds % 60;
    if minutes < 60 {
        return if remaining_seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m{remaining_seconds}s")
        };
    }
    let hours = minutes / 60;
    let remaining_minutes = minutes % 60;
    if remaining_minutes == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h{remaining_minutes}m")
    }
}

/// Minimal valid permission request for pump/UI tests; shared with the
/// `main.rs` test module.
#[cfg(test)]
pub(crate) fn test_permission_request(request_id: &str) -> PermissionRequest {
    use openbitfun_agent_runtime::sdk::{PermissionRequestSource, PermissionRequestSourceKind};
    PermissionRequest {
        request_id: request_id.to_string(),
        round_id: "round-1".to_string(),
        order: 0,
        tool_call_id: Some("call-1".to_string()),
        project_path: None,
        project_id: "project".to_string(),
        session_id: "session".to_string(),
        agent_id: "Standard".to_string(),
        action: "test.action".to_string(),
        resources: vec!["resource-a".to_string()],
        save_resources: Vec::new(),
        source: PermissionRequestSource {
            kind: PermissionRequestSourceKind::ToolCall,
            identity: "test".to_string(),
        },
        delegation: None,
        display_metadata: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbitfun_events::ToolEventIdentity;

    #[test]
    fn tool_started_records_the_run_in_background_flag() {
        let started = |params: serde_json::Value| ToolEventData::Started {
            identity: ToolEventIdentity::direct("call-1", "task"),
            params,
            timeout_seconds: None,
        };
        // The web backgroundSubagentActivity signal rides exactly this input.
        assert!(
            convert_tool_event(started(serde_json::json!({
                "prompt": "research",
                "run_in_background": true,
            })))
            .background
        );
        // Foreground task tools (and every other event) stay unflagged.
        assert!(
            !convert_tool_event(started(serde_json::json!({
                "prompt": "research",
            })))
            .background
        );
        assert!(
            !convert_tool_event(ToolEventData::Progress {
                identity: ToolEventIdentity::direct("call-1", "task"),
                message: "working".into(),
                percentage: 0.5,
            })
            .background
        );
    }

    fn summary(id: &str, last_active_at_ms: u64) -> AgentSessionSummary {
        AgentSessionSummary {
            session_id: id.to_string(),
            session_name: format!("session {id}"),
            agent_type: "code".to_string(),
            model_id: None,
            reasoning_preset: None,
            last_user_dialog_agent_type: None,
            last_submitted_agent_type: None,
            turn_count: 1,
            created_at_ms: 0,
            last_active_at_ms,
        }
    }

    #[test]
    fn sessions_sort_by_last_active_desc_then_id() {
        let mut sessions = vec![
            summary("b", 200),
            summary("a", 300),
            summary("c", 200),
            summary("d", 100),
        ];
        sort_sessions(&mut sessions);
        let ids: Vec<&str> = sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }

    /// G-S1: turn timing comes from the persisted message timestamps — the
    /// first and last of each turn, whatever the role order; a message
    /// without a timestamp (or turn id) contributes nothing.
    #[test]
    fn transcript_turn_times_bracket_each_turn() {
        let timed = |turn_id: &str, at: u64| TranscriptMessage {
            id: None,
            role: "assistant".to_string(),
            turn_id: Some(turn_id.to_string()),
            timestamp_ms: Some(at),
            content: TranscriptContent::Text("text".to_string()),
        };
        let mut untimed = message("user", TranscriptContent::Text("hello".to_string()));
        untimed.turn_id = None;
        let times = transcript_turn_times(&[
            timed("turn-1", 1_000),
            untimed,
            timed("turn-2", 9_000),
            timed("turn-1", 5_000),
            timed("turn-2", 4_000),
        ]);
        let first = times.get("turn-1").expect("turn-1 timed");
        assert_eq!(first.started_at_ms, Some(1_000));
        assert_eq!(first.ended_at_ms, Some(5_000));
        assert_eq!(first.effective_duration_ms(), Some(4_000));
        let second = times.get("turn-2").expect("turn-2 timed");
        assert_eq!(second.started_at_ms, Some(4_000));
        assert_eq!(second.ended_at_ms, Some(9_000));
        assert_eq!(times.len(), 2, "timeless messages add no rows");

        // The runtime's own duration wins over the wall-clock span.
        let mut with_runtime_duration = *first;
        with_runtime_duration.duration_ms = Some(3_200);
        assert_eq!(with_runtime_duration.effective_duration_ms(), Some(3_200));
    }

    /// G-S1: the duration shapes the footer shows follow the web's
    /// `formatElapsedDuration`.
    #[test]
    fn elapsed_duration_shapes_match_the_web() {
        assert_eq!(format_elapsed_duration(0), "0ms");
        assert_eq!(format_elapsed_duration(820), "820ms");
        assert_eq!(format_elapsed_duration(12_000), "12s");
        assert_eq!(format_elapsed_duration(12_300), "12.3s");
        assert_eq!(format_elapsed_duration(90_000), "1m30s");
        assert_eq!(format_elapsed_duration(120_000), "2m");
        assert_eq!(format_elapsed_duration(7_500_000), "2h5m");
        assert_eq!(format_elapsed_duration(7_200_000), "2h");
    }

    /// G-S1: the completion clock is a local `HH:MM:SS`; an out-of-range
    /// timestamp renders empty rather than panicking.
    #[test]
    fn clock_time_formats_hh_mm_ss_or_empty() {
        assert_eq!(format_clock_time(0).len(), 8);
        assert_eq!(format_clock_time(0).matches(':').count(), 2);
        assert_eq!(format_clock_time(i64::MAX as u64), "");
    }

    #[test]
    fn last_active_formats_yyyy_mm_dd_hh_mm_or_empty() {
        // Timestamp rendering is timezone-dependent; assert the fixed layout.
        assert_eq!(format_last_active(0).len(), 16);
        assert!(format_last_active(0).contains(':'));
        assert_eq!(format_last_active(i64::MAX as u64), "");
    }

    #[test]
    fn model_choice_labels_stay_distinguishable_across_providers() {
        let choice = |provider: &str| ModelChoice {
            id: "model-a".into(),
            name: "Model A".into(),
            provider: provider.into(),
            enabled: true,
        };
        assert_eq!(choice("Provider X").label(), "Model A · Provider X");
        assert_eq!(choice("").label(), "Model A");
    }

    fn message(role: &str, content: TranscriptContent) -> TranscriptMessage {
        TranscriptMessage {
            id: None,
            role: role.to_string(),
            turn_id: Some("turn-1".to_string()),
            timestamp_ms: None,
            content,
        }
    }

    #[test]
    fn transcript_converts_roles_and_tool_results() {
        let entries = convert_transcript(&[
            message("user", TranscriptContent::Text("hello".to_string())),
            message("assistant", TranscriptContent::Text("**done**".to_string())),
            message("system", TranscriptContent::Text("note".to_string())),
        ]);
        assert!(matches!(entries[0], ChatEntry::User { .. }));
        assert!(matches!(entries[1], ChatEntry::Assistant { .. }));
        assert!(matches!(entries[2], ChatEntry::User { .. }));

        // A result without a preceding call still renders a card, with the
        // result-side facts only.
        let entries = convert_transcript(&[message(
            "tool",
            TranscriptContent::ToolResult {
                tool_id: "t1".to_string(),
                tool_name: "Read".to_string(),
                effective_tool_name: None,
                result: serde_json::json!({"file_path": "src/lib.rs", "lines_read": 12}),
                is_error: false,
            },
        )]);
        match &entries[0] {
            ChatEntry::Tool(card) => {
                assert_eq!(card.tool_name, "Read");
                assert_eq!(card.status, ToolStatus::Done);
                assert_eq!(card.facts.kind, transcript::ToolKind::Read);
                assert_eq!(card.facts.title, "src/lib.rs");
                assert_eq!(card.facts.meta, vec![transcript::ToolMeta::Lines(12)]);
            }
            other => panic!("expected tool card, got {other:?}"),
        }
    }

    #[test]
    fn transcript_pairs_tool_calls_with_their_results() {
        // The persisted shape: one Mixed round carrying every call with its
        // arguments, then one ToolResult message per call. The card must show
        // the input facts (path, window) plus the result counts.
        let call = |tool_id: &str, tool_name: &str, arguments: serde_json::Value| {
            openbitfun_runtime_ports::TranscriptToolCall {
                tool_id: tool_id.to_string(),
                tool_name: tool_name.to_string(),
                arguments,
            }
        };
        let entries = convert_transcript(&[
            message(
                "assistant",
                TranscriptContent::Mixed {
                    reasoning_content: None,
                    text: "reading the file".to_string(),
                    tool_calls: vec![call(
                        "t1",
                        "Read",
                        serde_json::json!({"file_path": "src/main.rs", "offset": 0, "limit": 200}),
                    )],
                },
            ),
            message(
                "tool",
                TranscriptContent::ToolResult {
                    tool_id: "t1".to_string(),
                    tool_name: "Read".to_string(),
                    effective_tool_name: None,
                    result: serde_json::json!({"file_path": "src/main.rs", "lines_read": 200}),
                    is_error: false,
                },
            ),
        ]);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], ChatEntry::Assistant { .. }));
        match &entries[1] {
            ChatEntry::Tool(card) => {
                assert_eq!(card.facts.title, "src/main.rs");
                assert_eq!(
                    card.facts.meta,
                    vec![
                        transcript::ToolMeta::Text("1-200".to_string()),
                        transcript::ToolMeta::Lines(200),
                    ]
                );
            }
            other => panic!("expected tool card, got {other:?}"),
        }
    }

    #[test]
    fn deferred_tool_cards_use_the_projected_tool_identity() {
        // The gateway call is what the transcript stores; the inner tool is
        // what the row must name.
        let entries = convert_transcript(&[
            message(
                "assistant",
                TranscriptContent::Mixed {
                    reasoning_content: None,
                    text: String::new(),
                    tool_calls: vec![openbitfun_runtime_ports::TranscriptToolCall {
                        tool_id: "t4".to_string(),
                        tool_name: "CallDeferredTool".to_string(),
                        arguments: serde_json::json!({"tool_name": "ListModels", "args": {}}),
                    }],
                },
            ),
            message(
                "tool",
                TranscriptContent::ToolResult {
                    tool_id: "t4".to_string(),
                    tool_name: "CallDeferredTool".to_string(),
                    effective_tool_name: Some("ListModels".to_string()),
                    result: serde_json::json!({"match_count": 7}),
                    is_error: false,
                },
            ),
        ]);
        match &entries[0] {
            ChatEntry::Tool(card) => {
                assert_eq!(card.tool_name, "ListModels");
                assert_eq!(card.facts.kind, transcript::ToolKind::Other);
                assert_eq!(card.facts.title, "ListModels");
                assert_eq!(card.facts.meta, vec![transcript::ToolMeta::Matches(7)]);
            }
            other => panic!("expected tool card, got {other:?}"),
        }
    }

    #[test]
    fn multimodal_and_mixed_transcripts_convert() {
        let images = convert_transcript(&[message(
            "user",
            TranscriptContent::Multimodal {
                text: "see this".to_string(),
                image_count: 2,
            },
        )]);
        match &images[0] {
            ChatEntry::User { text, .. } => assert!(text.contains("[2 image(s) attached]")),
            other => panic!("expected user entry, got {other:?}"),
        }

        // A text-less Mixed round emits no assistant entry: its calls are
        // the round's content, and they arrive through their results.
        let empty_text_mixed = convert_transcript(&[message(
            "assistant",
            TranscriptContent::Mixed {
                reasoning_content: None,
                text: String::new(),
                tool_calls: vec![openbitfun_runtime_ports::TranscriptToolCall {
                    tool_id: "t9".to_string(),
                    tool_name: "Bash".to_string(),
                    arguments: serde_json::json!({"cmd": "ls"}),
                }],
            },
        )]);
        assert!(empty_text_mixed.is_empty());
    }

    #[test]
    fn transcript_carries_turn_ids_for_fork_boundaries() {
        let entries = convert_transcript(&[message(
            "user",
            TranscriptContent::Text("hello".to_string()),
        )]);
        match &entries[0] {
            ChatEntry::User { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some("turn-1"));
            }
            other => panic!("expected user entry, got {other:?}"),
        }

        let unnamed = TranscriptMessage {
            turn_id: None,
            ..message("user", TranscriptContent::Text("legacy".to_string()))
        };
        match &convert_transcript(&[unnamed])[0] {
            ChatEntry::User { turn_id, .. } => assert!(turn_id.is_none()),
            other => panic!("expected user entry, got {other:?}"),
        }
    }

    #[test]
    fn transcript_export_renders_sections_tools_and_attachments() {
        let transcript = SessionTranscript {
            session_id: "s-1".to_string(),
            messages: vec![
                message("user", TranscriptContent::Text("hello".to_string())),
                message("assistant", TranscriptContent::Text("**done**".to_string())),
                message(
                    "tool",
                    TranscriptContent::ToolResult {
                        tool_id: "t1".to_string(),
                        tool_name: "Read".to_string(),
                        effective_tool_name: None,
                        result: serde_json::json!({"ok": true}),
                        is_error: false,
                    },
                ),
                message(
                    "user",
                    TranscriptContent::Multimodal {
                        text: "see this".to_string(),
                        image_count: 2,
                    },
                ),
                message(
                    "assistant",
                    TranscriptContent::Mixed {
                        reasoning_content: None,
                        text: "  ".to_string(),
                        tool_calls: vec![openbitfun_runtime_ports::TranscriptToolCall {
                            tool_id: "t2".to_string(),
                            tool_name: "Bash".to_string(),
                            arguments: serde_json::json!({"cmd": "ls"}),
                        }],
                    },
                ),
            ],
        };
        let markdown = transcript_to_markdown(&transcript);
        assert!(markdown.starts_with("# Session s-1\n\n"));
        assert!(markdown.contains("## User\n\nhello\n\n"));
        assert!(markdown.contains("## Assistant\n\n**done**\n\n"));
        assert!(markdown.contains("### Tool `Read`"));
        assert!(markdown.contains("### Tool `Bash`"));
        assert!(markdown.contains("```json\n{\"ok\":true}\n```"));
        assert!(markdown.contains("[2 image(s) attached]"));
        // Whitespace-only assistant text collapses into its tool calls
        // instead of emitting an empty section.
        assert!(!markdown.contains("## Assistant\n\n \n"));
    }

    #[test]
    fn tool_events_collapse_onto_card_states() {
        let id = ToolEventIdentity::direct("id-1", "Bash");
        let started = convert_tool_event(ToolEventData::Started {
            identity: id.clone(),
            params: serde_json::json!({"command": "ls"}),
            timeout_seconds: None,
        });
        assert_eq!(started.status, ToolStatus::Running);
        assert_eq!(started.facts.kind, transcript::ToolKind::Command);
        assert_eq!(started.facts.title, "ls");

        let confirm = convert_tool_event(ToolEventData::ConfirmationNeeded {
            identity: id.clone(),
            params: serde_json::json!({}),
            timeout_at: None,
        });
        assert_eq!(confirm.status, ToolStatus::WaitingConfirm);

        let done = convert_tool_event(ToolEventData::Completed {
            identity: id.clone(),
            result: serde_json::json!("out"),
            result_for_assistant: None,
            image_attachments: None,
            duration_ms: 5,
            queue_wait_ms: None,
            preflight_ms: None,
            confirmation_wait_ms: None,
            execution_ms: None,
        });
        assert_eq!(done.status, ToolStatus::Done);

        let failed = convert_tool_event(ToolEventData::Failed {
            identity: id.clone(),
            error: "boom".to_string(),
            error_detail: None,
            duration_ms: None,
            queue_wait_ms: None,
            preflight_ms: None,
            confirmation_wait_ms: None,
            execution_ms: None,
        });
        assert_eq!(failed.status, ToolStatus::Failed);
        assert_eq!(failed.facts.error.as_deref(), Some("boom"));

        let queued = convert_tool_event(ToolEventData::Queued {
            identity: id,
            position: 1,
        });
        assert_eq!(queued.status, ToolStatus::Running);
    }

    #[test]
    fn sparse_lifecycle_updates_mark_success_and_failure() {
        // The Completed event carries the result out of band: the shell reads
        // `success` off it and merges the result facts into the started card.
        let id = ToolEventIdentity::direct("id-2", "Edit");
        let done = convert_tool_event(ToolEventData::Completed {
            identity: id,
            result: serde_json::json!({"success": false, "match_count": 0}),
            result_for_assistant: None,
            image_attachments: None,
            duration_ms: 5,
            queue_wait_ms: None,
            preflight_ms: None,
            confirmation_wait_ms: None,
            execution_ms: None,
        });
        assert_eq!(done.result_success, Some(false));
        assert_eq!(done.facts.meta, vec![transcript::ToolMeta::Replacements(0)]);
    }

    #[test]
    fn tool_fact_merge_keeps_input_identity_and_appends_results() {
        let stored = transcript::facts_from_input(
            "AgentSpawn",
            &serde_json::json!({
                "agent_id": "parser-review",
                "agent_type": "SwarmWorker",
                "model_id": "glm-5.3",
                "prompt": "port one slice",
            }),
        );
        let mut update = transcript::facts_from_input("AgentSpawn", &serde_json::Value::Null);
        transcript::apply_result(
            &mut update,
            "AgentSpawn",
            &serde_json::json!({"agent_id": "parser-review", "bg_task_id": "t_bg1", "status": "started"}),
        );
        let merged = transcript::merge_facts(&stored, &update);
        assert_eq!(merged.kind, transcript::ToolKind::AgentSpawn);
        assert_eq!(merged.title, "parser-review");
        assert_eq!(merged.agents.len(), 1);
        assert_eq!(merged.agents[0].alias, "parser-review");
        assert_eq!(merged.agents[0].agent_type.as_deref(), Some("SwarmWorker"));
        assert_eq!(merged.agents[0].model.as_deref(), Some("glm-5.3"));
        assert_eq!(merged.agents[0].task_id.as_deref(), Some("t_bg1"));
        assert_eq!(merged.agents[0].status.as_deref(), Some("started"));
    }

    #[test]
    fn translated_events_cover_the_chat_surface() {
        use openbitfun_events::AgenticEvent;
        let started = translate_event(AgenticEvent::DialogTurnStarted {
            session_id: "s1".to_string(),
            turn_id: "t1".to_string(),
            turn_index: 0,
            user_input: "hi".to_string(),
            original_user_input: None,
            user_message_metadata: None,
        });
        assert!(matches!(
            started.as_slice(),
            [RuntimeEvent::TurnStarted { session_id, .. }] if session_id == "s1"
        ));

        let dropped = translate_event(AgenticEvent::TokenUsageUpdated {
            session_id: "s1".to_string(),
            turn_id: "t1".to_string(),
            model_config_id: "m".to_string(),
            effective_model_name: "m".to_string(),
            input_tokens: 1,
            output_tokens: None,
            total_tokens: 1,
            max_context_tokens: None,
            is_subagent: false,
            cached_tokens: None,
            token_details: None,
        });
        assert!(dropped.is_empty());
    }

    #[test]
    fn permission_events_translate_to_view_events() {
        let asked = translate_permission_event(PermissionRequestEvent::Asked {
            request: test_permission_request("req-1"),
        });
        assert!(matches!(asked, Some(RuntimeEvent::PermissionAsked { .. })));

        let replied = translate_permission_event(PermissionRequestEvent::Replied {
            request_id: "req-2".to_string(),
            reply: PermissionReply::Once,
            source: openbitfun_agent_runtime::sdk::PermissionReplySource::User,
        });
        assert!(matches!(
            replied,
            Some(RuntimeEvent::PermissionResolved { ref request_id }) if request_id == "req-2"
        ));

        let cancelled = translate_permission_event(PermissionRequestEvent::Cancelled {
            request_id: "req-3".to_string(),
            reason: "turn cancelled".to_string(),
        });
        assert!(matches!(
            cancelled,
            Some(RuntimeEvent::PermissionResolved { ref request_id }) if request_id == "req-3"
        ));
    }
}
