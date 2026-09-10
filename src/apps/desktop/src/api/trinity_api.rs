//! Trinity cognitive engine API (Tauri commands).
//!
//! Thin wrappers over the `trinityd` daemon RPC surface. Every command
//! forwards to `TrinityBackend::global().call(method, params)` so the desktop
//! host never owns cognitive state; the daemon is the single source of truth.
//!
//! All commands degrade gracefully: when the daemon is unreachable they return
//! an error string the frontend can surface without crashing the host.

use crate::trinity::backend::TrinityBackend;
use serde_json::{json, Value};
use tauri::State;

/// Optional passthrough params for a daemon method.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrinityRequest {
    #[serde(default)]
    pub params: Value,
}

/// Forward one call to the daemon; `params` defaults to `{}`.
async fn forward(method: &str, request: Option<TrinityRequest>) -> Result<Value, String> {
    let params = request
        .map(|r| r.params)
        .unwrap_or_else(|| json!({}));
    TrinityBackend::global().call(method, params).await
}

// ── Cognitive state ─────────────────────────────────────────────

/// Current PSI cognitive state (emotion / needs / focus / confidence).
#[tauri::command]
pub async fn trinity_get_cognitive_state(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("get_cognitive_state", request).await
}

/// Express current emotion and bond status.
#[tauri::command]
pub async fn trinity_express(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("express", request).await
}

/// Deep introspection snapshot (emotion / bond / memory stats).
#[tauri::command]
pub async fn trinity_self_perception(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("self_perception", request).await
}

/// Daemon health status.
#[tauri::command]
pub async fn trinity_get_status(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("get_status", request).await
}

/// Cognitive history (emotion / needs curve over recent turns).
#[tauri::command]
pub async fn trinity_cognition_history(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cognition_history", request).await
}

// ── Memory (MindGraph) ──────────────────────────────────────────

/// Memory timeline entries.
#[tauri::command]
pub async fn trinity_memory_timeline(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("memory.timeline", request).await
}

/// Memory statistics.
#[tauri::command]
pub async fn trinity_memory_stats(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("memory_stats", request).await
}

/// Recall memories matching a query.
#[tauri::command]
pub async fn trinity_recall_memory(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("recall_memory", request).await
}

/// Store a memory into MindGraph.
#[tauri::command]
pub async fn trinity_memorize(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("memorize", request).await
}

/// Forget a memory by id.
#[tauri::command]
pub async fn trinity_forget_memory(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("forget_memory", request).await
}

/// Reinforce a memory by id.
#[tauri::command]
pub async fn trinity_reinforce_memory(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("reinforce_memory", request).await
}

// ── Awakening ceremony ──────────────────────────────────────────

/// Run the awakening ceremony (name + persona + bond).
#[tauri::command]
pub async fn trinity_awaken(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("ceremony.awaken", request).await
}

// ── Cognitive engine LLM config ─────────────────────────────────

/// Read the cognitive engine LLM configuration.
#[tauri::command]
pub async fn trinity_llm_get_config(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("llm.get_config", request).await
}

/// Write the cognitive engine LLM configuration (daemon owns the toml).
#[tauri::command]
pub async fn trinity_llm_set_config(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("llm.set_config", request).await
}

/// Test the cognitive engine LLM connection.
#[tauri::command]
pub async fn trinity_llm_test_connection(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("llm.test_connection", request).await
}

// ── Cloud memory ────────────────────────────────────────────────

/// Cloud-memory status (registration / key / sync cursors / pending ops).
#[tauri::command]
pub async fn trinity_cloud_status(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.status", request).await
}

/// Register a cloud account and bind this device.
#[tauri::command]
pub async fn trinity_cloud_signup(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.signup", request).await
}

/// Log in to an existing cloud account and bind this device.
#[tauri::command]
pub async fn trinity_cloud_login(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.login", request).await
}

/// Wrap the local master key with a passphrase (cloud key ceremony).
#[tauri::command]
pub async fn trinity_cloud_setup_key(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.setup_key", request).await
}

/// Trigger an immediate cloud sync.
#[tauri::command]
pub async fn trinity_cloud_sync_now(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.sync_now", request).await
}

/// Export → seal → upload a full memory snapshot.
#[tauri::command]
pub async fn trinity_cloud_backup(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.backup", request).await
}

/// Download → decrypt → merge the latest cloud snapshot.
#[tauri::command]
pub async fn trinity_cloud_restore(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("cloud.restore", request).await
}

// ── Shutdown ────────────────────────────────────────────────────

/// Ask the daemon to shut down gracefully (used on host exit).
#[tauri::command]
pub async fn trinity_shutdown(
    _state: State<'_, crate::api::app_state::AppState>,
    request: Option<TrinityRequest>,
) -> Result<Value, String> {
    forward("daemon.shutdown", request).await
}