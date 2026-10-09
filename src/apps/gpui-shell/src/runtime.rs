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

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result};
use gpui_kit::SharedString;
use openbitfun_agent_runtime::sdk::{
    AgentDialogTurnExecution, AgentDialogTurnRequest, AgentRuntime,
    AgentSessionArchiveStateRequest, AgentSessionCompactionRequest, AgentSessionCreateRequest,
    AgentSessionDeleteRequest, AgentSessionForkAtTurnRequest, AgentSessionForkBeforeTurnRequest,
    AgentSessionForkRequest, AgentSessionModelUpdateRequest, AgentSessionRenameRequest,
    AgentSessionRestoreRequest, AgentSessionUsageRequest, AgentTurnCancellationRequest,
    AgentUserAnswersRequest, PendingUserQuestion, PermissionReply, PermissionRequest,
    PermissionRequestEvent, SessionState, SessionTranscript, SessionTranscriptRequest,
    SessionUsageReport,
};
use openbitfun_agent_runtime::user_questions::{get_user_input_manager, UserInputManager};
use openbitfun_core::agentic::system::{
    init_agentic_system_for_profile_with_runtime_ownership, select_agentic_system_profile,
};
use openbitfun_core::infrastructure::ai::AIClientFactory;
use openbitfun_core::infrastructure::try_get_path_manager_arc;
use openbitfun_core::product_assembly::{DeliveryProfile, ProductAssembler, ProductAssemblyInput};
use openbitfun_core::product_runtime::{
    build_local_runtime_services, ensure_product_dialog_scheduler, CoreAgentRuntimeCompatibility,
    CoreProductAgentRuntime, CoreProductEventQueueOwner, CoreRuntimeServicesProvider,
};
use openbitfun_core::runtime_ownership::{CoreRuntimeOwnership, RuntimeDeployment};
use openbitfun_core::service::config::{
    get_app_language, get_global_config_service, initialize_global_config, types::GlobalConfig,
};
use openbitfun_core::service::workspace::{
    get_global_workspace_service, set_global_workspace_service, WorkspaceInfo, WorkspaceService,
};
use openbitfun_events::{AgenticEvent, ToolEventData};
use openbitfun_runtime_ports::{
    AgentInputAttachment, AgentSessionListRequest, AgentSessionSummary, AgentSubmissionSource,
    DialogSubmissionPolicy, GitPort, TranscriptContent, TranscriptMessage,
};
use openbitfun_services_core::session::SessionStatus;
use tokio::sync::mpsc;

/// Matches the CLI interactive buffer (`RUNTIME_EVENT_BUFFER`).
const RUNTIME_EVENT_BUFFER: usize = 256;
const ENTRYPOINT: &str = "gpui-shell";
/// Same default agent the CLI startup page resolves from `behavior.default_agent`.
const DEFAULT_AGENT_TYPE: &str = "Standard";
/// Tool card payload previews stay short; full values live in the runtime.
const TOOL_DETAIL_LIMIT: usize = 400;
/// Per-process workspace history kept for the picker (the shared catalog holds
/// the desktop/CLI recent list; this only remembers what this shell visited).
const VISITED_WORKSPACES_LIMIT: usize = 12;

static RUNTIME_HANDLE: OnceLock<RuntimeHandle> = OnceLock::new();

/// Returns the embedded runtime handle once the bootstrap has finished.
pub(crate) fn runtime_handle() -> Option<&'static RuntimeHandle> {
    RUNTIME_HANDLE.get()
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
    SendFailed { session_id: String, error: String },
    /// A dialog turn started executing (busy marker for the open session).
    TurnStarted { session_id: String, turn_id: String },
    /// Streaming assistant text for the open session.
    TextChunk {
        session_id: String,
        turn_id: String,
        round_id: String,
        text: String,
    },
    /// Tool lifecycle update; upserted by the tool id in the open session.
    ToolUpdate { session_id: String, card: ToolCard },
    /// A dialog turn reached a terminal state.
    TurnFinished {
        session_id: String,
        error: Option<String>,
    },
    /// The authoritative history changed (undo/restore/repair); reload it.
    HistoryChanged { session_id: String },
    /// A tool asked for permission (process-local `PermissionRequestManager`
    /// broadcast). The turn parks until the shell answers it through
    /// [`RuntimeHandle::respond_permission`].
    PermissionAsked { request: PermissionRequest },
    /// A pending request was answered or cancelled; `request_id` drops out of
    /// the shell's pending queue. This includes echoes of the shell's own
    /// replies, which keeps the queue a pure projection of the runtime state.
    PermissionResolved { request_id: String },
    /// Full pending-request snapshot replacing the queue, sent when the
    /// permission broadcast lags so no ask can be stranded unanswered.
    PermissionSnapshot(Vec<PermissionRequest>),
    /// A session rename was applied by the runtime; adopt the new name.
    SessionRenamed { session_id: String, name: String },
    /// A fork of an existing session was created; the runtime opens it right
    /// away, so the chat pane switches to the fork when its transcript
    /// arrives through [`RuntimeEvent::SessionOpened`].
    SessionForked {
        new_session_id: String,
        new_session_name: String,
    },
    /// A manual context-compaction maintenance turn was accepted for the open
    /// session; completion flows through the normal turn-settlement events.
    CompactionStarted { session_id: String },
    /// A transcript export was written to `path`.
    ExportFinished { session_id: String, path: String },
    /// Session usage report for the usage dialog.
    SessionUsage(Box<SessionUsageSummary>),
    /// A session was deleted; the chat pane must close when it shows it.
    SessionDeleted { session_id: String },
    /// A session action (rename / archive / delete / model change) failed;
    /// surfaced as a shell notice line, never silently dropped.
    SessionActionFailed { message: String },
    /// A session's model selector was updated successfully.
    SessionModelChanged {
        session_id: String,
        model_id: String,
    },
    /// Configured-model catalog for the pickers (session header + models
    /// settings page); sent once after bootstrap.
    ModelCatalog(Vec<ModelChoice>),
    /// Workspaces the shell can switch to: the shared core catalog (recent
    /// first, then the rest of the opened set) plus workspaces visited by
    /// this process, with the active one always present.
    Workspaces(Vec<WorkspaceChoice>),
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
}

/// One session list row: the runtime summary plus its persisted archive state.
/// The summary DTO carries no archive flag, so the bootstrap-side metadata read
/// supplies it (the sidebar filters archived sessions out by default).
#[derive(Debug)]
pub(crate) struct SessionListRow {
    pub summary: AgentSessionSummary,
    pub archived: bool,
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
    /// The session was mid-turn when opened; live events continue to arrive.
    pub busy: bool,
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
    pub tool_name: String,
    pub status: ToolStatus,
    pub detail: String,
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
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionArchiveStateRequest {
                workspace_id,
                workspace_path: String::new(),
                session_id: session_id.clone(),
                archived,
                remote_connection_id: None,
                remote_ssh_host: None,
            };
            match runtime.set_session_archived(request).await {
                Ok(()) => {
                    tracing::info!("Session {session_id} archived={archived}");
                    handle.reload_sessions(tx);
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
        let runtime = self.agent_runtime.clone();
        let workspace_id = Some(self.workspace().id);
        let handle = self.clone_handle_after_action();
        self.tokio.spawn(async move {
            let request = AgentSessionDeleteRequest {
                workspace_id,
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
                    handle.reload_sessions(tx);
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
/// flags the summary DTO omits.
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
                let _ = tx.send(RuntimeEvent::Sessions(rows));
            }
            Err(error) => {
                let message = error.into_message();
                tracing::error!("Failed to list sessions: {message}");
                let _ = tx.send(RuntimeEvent::SessionsFailed(message));
            }
        }
    });
}

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
                workspace_id,
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
                        created.session_id,
                        created.session_name,
                        created.agent_type,
                        created.model_id,
                        false,
                        tx.clone(),
                    )
                    .await;
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
            let turn_id = uuid::Uuid::new_v4().to_string();
            // The turn carries the session's own agent; the default only
            // covers sessions that predate agent recording.
            let agent_type = runtime
                .resolve_session_agent_type(&session_id)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| DEFAULT_AGENT_TYPE.to_string());
            // Ask semantics: the metadata stays empty so the turn inherits the
            // persisted approval preference (same as the CLI interactive
            // default); M5 wires the explicit approval surface on top.
            let request = AgentDialogTurnRequest {
                session_id: session_id.clone(),
                message: message.clone(),
                output_schema: None,
                original_message: None,
                turn_id: Some(turn_id.clone()),
                execution: AgentDialogTurnExecution::Standard,
                agent_type,
                workspace_path: Some(workspace_path),
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
        });
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
            let entries: Vec<ChatEntry> = transcript.messages.iter().map(convert_message).collect();
            let _ = tx.send(RuntimeEvent::SessionOpened(OpenedSession {
                session_id,
                // Reload keeps the current pane title; the list refresh on turn
                // settlement picks up any rename.
                name: String::new(),
                agent_type: String::new(),
                model_id: None,
                entries,
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
    let entries: Vec<ChatEntry> = transcript.messages.iter().map(convert_message).collect();
    let _ = tx.send(RuntimeEvent::SessionOpened(OpenedSession {
        session_id,
        name,
        agent_type,
        model_id,
        entries,
        busy,
    }));
}

/// Converts one persisted transcript message into shell chat entries.
/// Mixed assistant messages keep their order: text first, then tool calls
/// (result details arrive in their own `ToolResult` messages).
fn convert_message(message: &TranscriptMessage) -> ChatEntry {
    match &message.content {
        TranscriptContent::Text(text) => match message.role.as_str() {
            "assistant" => ChatEntry::Assistant {
                round_id: message.turn_id.clone().unwrap_or_default(),
                text: text.clone(),
            },
            _ => ChatEntry::User {
                text: text.clone(),
                turn_id: message.turn_id.clone(),
            },
        },
        TranscriptContent::Multimodal { text, image_count } => {
            let mut text = text.clone();
            if *image_count > 0 {
                text.push_str(&format!("\n\n[{image_count} image(s) attached]"));
            }
            ChatEntry::User {
                text,
                turn_id: message.turn_id.clone(),
            }
        }
        TranscriptContent::ToolResult {
            tool_id,
            tool_name,
            result,
            is_error,
            ..
        } => ChatEntry::Tool(ToolCard {
            key: SharedString::from(format!("tool-{tool_id}")),
            tool_name: tool_name.clone(),
            status: if *is_error {
                ToolStatus::Failed
            } else {
                ToolStatus::Done
            },
            detail: truncate_detail(&result.to_string()),
        }),
        TranscriptContent::Mixed {
            text, tool_calls, ..
        } => {
            // Historical Mixed messages collapse into an assistant entry plus
            // one Done card per call; per-call results land in ToolResult
            // entries and refresh the same keyed cards when they follow.
            if let Some(card) = tool_calls.first().map(|call| {
                ChatEntry::Tool(ToolCard {
                    key: SharedString::from(format!("tool-{}", call.tool_id)),
                    tool_name: call.tool_name.clone(),
                    status: ToolStatus::Done,
                    detail: truncate_detail(&call.arguments.to_string()),
                })
            }) {
                if text.trim().is_empty() {
                    return card;
                }
            }
            ChatEntry::Assistant {
                round_id: message.turn_id.clone().unwrap_or_default(),
                text: text.clone(),
            }
        }
    }
}

fn truncate_detail(value: &str) -> String {
    truncate_chars(value, TOOL_DETAIL_LIMIT)
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
        loop {
            match rx.recv().await {
                Ok(envelope) => {
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
            ..
        } => vec![RuntimeEvent::TurnFinished {
            session_id,
            error: match success {
                Some(false) => Some("turn completed unsuccessfully".to_string()),
                _ => None,
            },
        }],
        AgenticEvent::DialogTurnFailed {
            session_id, error, ..
        } => vec![RuntimeEvent::TurnFinished {
            session_id,
            error: Some(error),
        }],
        AgenticEvent::DialogTurnCancelled { session_id, .. }
        | AgenticEvent::DialogTurnInterrupted { session_id, .. } => {
            vec![RuntimeEvent::TurnFinished {
                session_id,
                error: None,
            }]
        }
        AgenticEvent::SessionHistoryChanged { session_id, .. } => {
            vec![RuntimeEvent::HistoryChanged { session_id }]
        }
        AgenticEvent::SystemError {
            session_id: Some(session_id),
            error,
            ..
        } => vec![RuntimeEvent::TurnFinished {
            session_id,
            error: Some(error),
        }],
        other => {
            tracing::debug!("Dropping runtime event for UI: {:?}", other.session_id());
            Vec::new()
        }
    }
}

/// Collapses the tool lifecycle onto the four card states the shell renders.
fn convert_tool_event(event: ToolEventData) -> ToolCard {
    let (tool_id, tool_name) = {
        let identity = event.identity();
        (identity.tool_id.clone(), identity.tool_name.clone())
    };
    let key = SharedString::from(format!("tool-{tool_id}"));
    match event {
        ToolEventData::Started { params, .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Running,
            detail: truncate_detail(&params.to_string()),
        },
        ToolEventData::Progress { message, .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Running,
            detail: truncate_detail(&message),
        },
        ToolEventData::ConfirmationNeeded { params, .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::WaitingConfirm,
            detail: truncate_detail(&params.to_string()),
        },
        ToolEventData::Rejected { .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Cancelled,
            detail: String::new(),
        },
        ToolEventData::Completed { result, .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Done,
            detail: truncate_detail(&result.to_string()),
        },
        ToolEventData::Failed { error, .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Failed,
            detail: truncate_detail(&error),
        },
        ToolEventData::Cancelled { reason, .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Cancelled,
            detail: truncate_detail(&reason),
        },
        ToolEventData::EarlyDetected { .. }
        | ToolEventData::ParamsPartial { .. }
        | ToolEventData::Queued { .. }
        | ToolEventData::Waiting { .. }
        | ToolEventData::Streaming { .. }
        | ToolEventData::StreamChunk { .. }
        | ToolEventData::Confirmed { .. } => ToolCard {
            key,
            tool_name,
            status: ToolStatus::Running,
            detail: String::new(),
        },
    }
}

/// Spawns the embedded runtime bootstrap on the shell's tokio runtime.
/// Progress and outcomes are reported through `tx`.
pub(crate) fn spawn_bootstrap(
    tokio: tokio::runtime::Handle,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
) {
    tokio.spawn(async move {
        if let Err(error) = bootstrap(tx.clone()).await {
            tracing::error!("Embedded runtime bootstrap failed: {error:#}");
            let _ = tx.send(RuntimeEvent::Failed(format!("{error:#}")));
        }
    });
}

/// Assembles the embedded runtime following the CLI order of operations.
async fn bootstrap(tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<()> {
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
    match get_global_config_service().await {
        Ok(service) => match service.get_config::<GlobalConfig>(None).await {
            Ok(config) => {
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
    let agent_runtime = CoreProductAgentRuntime::build_with_event_source(
        agentic_system.coordinator.clone(),
        scheduler,
        agentic_system.token_usage_service.clone(),
        event_queue_owner.runtime_source(),
        services,
    )
    .map_err(anyhow::Error::msg)
    .context("Failed to build Agent Runtime SDK")?;

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
    if let Some(handle) = runtime_handle() {
        handle.reload_sessions(tx.clone());
        handle.reload_model_catalog(tx.clone());
        handle.reload_workspaces(tx.clone());
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
        let user = convert_message(&message(
            "user",
            TranscriptContent::Text("hello".to_string()),
        ));
        assert!(matches!(user, ChatEntry::User { .. }));

        let assistant = convert_message(&message(
            "assistant",
            TranscriptContent::Text("**done**".to_string()),
        ));
        assert!(matches!(assistant, ChatEntry::Assistant { .. }));

        let non_user_text_stays_user = convert_message(&message(
            "system",
            TranscriptContent::Text("note".to_string()),
        ));
        assert!(matches!(non_user_text_stays_user, ChatEntry::User { .. }));

        let tool = convert_message(&message(
            "tool",
            TranscriptContent::ToolResult {
                tool_id: "t1".to_string(),
                tool_name: "Read".to_string(),
                effective_tool_name: None,
                result: serde_json::json!({"ok": true}),
                is_error: false,
            },
        ));
        match tool {
            ChatEntry::Tool(card) => {
                assert_eq!(card.tool_name, "Read");
                assert_eq!(card.status, ToolStatus::Done);
                assert!(card.detail.contains("ok"));
            }
            other => panic!("expected tool card, got {other:?}"),
        }
    }

    #[test]
    fn multimodal_and_mixed_transcripts_convert() {
        let images = convert_message(&message(
            "user",
            TranscriptContent::Multimodal {
                text: "see this".to_string(),
                image_count: 2,
            },
        ));
        match images {
            ChatEntry::User { text, .. } => assert!(text.contains("[2 image(s) attached]")),
            other => panic!("expected user entry, got {other:?}"),
        }

        let empty_text_mixed = convert_message(&message(
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
        ));
        match empty_text_mixed {
            ChatEntry::Tool(card) => {
                assert_eq!(card.status, ToolStatus::Done);
                assert!(card.detail.contains("ls"));
            }
            other => panic!("expected tool card for text-less mixed, got {other:?}"),
        }
    }

    #[test]
    fn transcript_carries_turn_ids_for_fork_boundaries() {
        let user = convert_message(&message(
            "user",
            TranscriptContent::Text("hello".to_string()),
        ));
        match user {
            ChatEntry::User { turn_id, .. } => {
                assert_eq!(turn_id.as_deref(), Some("turn-1"));
            }
            other => panic!("expected user entry, got {other:?}"),
        }

        let unnamed = TranscriptMessage {
            turn_id: None,
            ..message("user", TranscriptContent::Text("legacy".to_string()))
        };
        match convert_message(&unnamed) {
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
        assert!(started.detail.contains("ls"));

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
        assert_eq!(failed.detail, "boom");

        let queued = convert_tool_event(ToolEventData::Queued {
            identity: id,
            position: 1,
        });
        assert_eq!(queued.status, ToolStatus::Running);
    }

    #[test]
    fn detail_preview_is_truncated_with_ellipsis() {
        let long = "x".repeat(TOOL_DETAIL_LIMIT + 10);
        let truncated = truncate_detail(&long);
        assert_eq!(truncated.chars().count(), TOOL_DETAIL_LIMIT + 1);
        assert!(truncated.ends_with('…'));
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
