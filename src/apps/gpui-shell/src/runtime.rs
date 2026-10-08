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
use openbitfun_agent_runtime::sdk::AgentRuntime;
use openbitfun_core::agentic::system::{
    init_agentic_system_for_profile_with_runtime_ownership, select_agentic_system_profile,
};
use openbitfun_core::infrastructure::ai::AIClientFactory;
use openbitfun_core::infrastructure::try_get_path_manager_arc;
use openbitfun_core::product_assembly::{DeliveryProfile, ProductAssembler, ProductAssemblyInput};
use openbitfun_core::product_runtime::{
    CoreProductAgentRuntime, CoreProductEventQueueOwner, CoreRuntimeServicesProvider,
    build_local_runtime_services, ensure_product_dialog_scheduler,
};
use openbitfun_core::runtime_ownership::{CoreRuntimeOwnership, RuntimeDeployment};
use openbitfun_core::service::config::{get_app_language, initialize_global_config};
use openbitfun_core::service::i18n::LocaleId;
use openbitfun_core::service::workspace::{
    WorkspaceInfo, WorkspaceService, get_global_workspace_service, set_global_workspace_service,
};
use openbitfun_runtime_ports::{AgentSessionListRequest, AgentSessionSummary};
use tokio::sync::mpsc;

/// Matches the CLI interactive buffer (`RUNTIME_EVENT_BUFFER`).
const RUNTIME_EVENT_BUFFER: usize = 256;
const ENTRYPOINT: &str = "gpui-shell";

static RUNTIME_HANDLE: OnceLock<RuntimeHandle> = OnceLock::new();

/// Returns the embedded runtime handle once the bootstrap has finished.
pub(crate) fn runtime_handle() -> Option<&'static RuntimeHandle> {
    RUNTIME_HANDLE.get()
}

/// One-way notifications from the embedded runtime into the shell UI.
#[derive(Debug)]
pub(crate) enum RuntimeEvent {
    /// Active locale resolved from the shared config (`app.language`).
    Locale(LocaleId),
    /// Embedded runtime assembled successfully; sessions may be loaded.
    Ready,
    /// Session list snapshot (workspace-scoped, read-only).
    Sessions(Vec<AgentSessionSummary>),
    /// Reload failed while the runtime itself stays usable.
    SessionsFailed(String),
    /// Bootstrap failed; the shell stays degraded with the error shown.
    Failed(String),
}

/// Process-lifetime handle to the embedded runtime, safe to hold from UI code.
pub(crate) struct RuntimeHandle {
    agent_runtime: AgentRuntime,
    workspace: WorkspaceInfo,
    tokio: tokio::runtime::Handle,
    // Keeps the runtime event source attached for the process lifetime.
    _event_queue_owner: CoreProductEventQueueOwner,
}

impl RuntimeHandle {
    /// Workspace record the runtime was assembled against (canonical root).
    pub(crate) fn workspace(&self) -> &WorkspaceInfo {
        &self.workspace
    }

    /// Requests a fresh read-only session snapshot; the result arrives as a
    /// [`RuntimeEvent::Sessions`] (or [`RuntimeEvent::SessionsFailed`]).
    pub(crate) fn reload_sessions(&self, tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let request = AgentSessionListRequest {
            workspace_id: Some(self.workspace.id.clone()),
            workspace_path: String::new(),
            remote_connection_id: None,
            remote_ssh_host: None,
        };
        let runtime = self.agent_runtime.clone();
        self.tokio.spawn(async move {
            match runtime.list_sessions(request).await {
                Ok(mut sessions) => {
                    sort_sessions(&mut sessions);
                    tracing::info!("Session list loaded: {} sessions", sessions.len());
                    let _ = tx.send(RuntimeEvent::Sessions(sessions));
                }
                Err(error) => {
                    let message = error.into_message();
                    tracing::error!("Failed to list sessions: {message}");
                    let _ = tx.send(RuntimeEvent::SessionsFailed(message));
                }
            }
        });
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

    // M3 i18n upgrade: the shared config owns the shell locale (`app.language`)
    // instead of the `$LANG` fallback used before the config service existed.
    let locale = get_app_language().await;
    let _ = tx.send(RuntimeEvent::Locale(locale));

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
    tracing::info!(
        "Agent runtime ownership acquired for workspace {}",
        workspace_root.display()
    );

    AIClientFactory::initialize_global()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to initialize global AIClientFactory: {error}"))?;

    let agentic_system = init_agentic_system_for_profile_with_runtime_ownership(
        DeliveryProfile::Cli,
        runtime_ownership,
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
    let (_, services) = build_local_runtime_services(&workspace.root_path, RUNTIME_EVENT_BUFFER)
        .context("Failed to build local runtime services")?;
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
        workspace,
        tokio: tokio::runtime::Handle::current(),
        _event_queue_owner: event_queue_owner,
    };
    let _ = RUNTIME_HANDLE.set(handle);
    let _ = tx.send(RuntimeEvent::Ready);
    tracing::info!("Embedded runtime ready");

    if let Some(handle) = runtime_handle() {
        handle.reload_sessions(tx);
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
