//! Being (cognitive being) identity API — core being, assistants, cognitive framework.
//!
//! Reads the engine-owned identity registry (`~/.trinity/beings/`). The core
//! cognitive being is the system life form: read-only, never deletable. The
//! daemon owns every write, so this surface only reports what the engine
//! already persisted.
//!
//! Group copy (name / hint) stays in the Web UI i18n catalog; the command
//! returns identity facts, not user-visible prose.

use crate::beings::{BeingConfig, BeingKind, BeingRegistry};
use crate::trinity::backend::TrinityBackend;
use crate::trinity::numeric;
use crate::trinity::tools::trinity_tools;
use serde::Serialize;
use serde_json::{json, Value};

/// Core cognitive being (read-only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreBeingInfo {
    pub id: String,
    pub name: String,
    pub user_name: String,
    pub persona: String,
    pub awakened: bool,
    pub awakened_at: Option<f64>,
    pub created_at: f64,
    /// Soul excerpt (embedded template, display only).
    pub soul_excerpt: String,
}

/// Being summary — core and assistants in one list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeingSummary {
    pub id: String,
    /// `core` | `assistant`
    pub kind: String,
    pub name: String,
    pub user_name: String,
    pub awakened: bool,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAssistantBeingRequest {
    pub id: String,
}

/// One tool exposed by the cognitive framework group.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveFrameworkToolInfo {
    pub name: String,
    pub description: String,
}

/// Cognitive framework group: tool list + live state + core being.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CognitiveFrameworkInfo {
    /// Group id that enables the framework inside an agent tool configuration.
    pub id: String,
    pub tools: Vec<CognitiveFrameworkToolInfo>,
    /// Live PSI state (`None` while the daemon is unreachable).
    pub cognitive_state: Option<Value>,
    pub being: Option<CoreBeingInfo>,
}

fn registry() -> BeingRegistry {
    BeingRegistry::new(BeingRegistry::default_root())
}

fn core_being_info(core: BeingConfig) -> CoreBeingInfo {
    CoreBeingInfo {
        id: core.id,
        name: core.name,
        user_name: core.user_name,
        persona: core.persona,
        awakened: core.awakened,
        awakened_at: core.awakened_at,
        created_at: core.created_at,
        soul_excerpt: registry().core_soul().chars().take(140).collect(),
    }
}

/// Load the core cognitive being. Fails before the awaken ceremony ran.
#[tauri::command]
pub fn get_core_being() -> Result<CoreBeingInfo, String> {
    let core = registry().load_core()?.ok_or_else(|| {
    "core cognitive being is not initialized — run `trinityd --init` or finish the awaken ceremony"
      .to_string()
  })?;
    Ok(core_being_info(core))
}

/// List every being the engine knows: core first, then assistants by id.
#[tauri::command]
pub fn list_beings() -> Result<Vec<BeingSummary>, String> {
    let registry = registry();
    let mut beings = Vec::new();
    if let Some(core) = registry.load_core()? {
        beings.push(BeingSummary {
            id: core.id,
            kind: BeingKind::Core.as_str().to_string(),
            name: core.name,
            user_name: core.user_name,
            awakened: core.awakened,
        });
    }
    for assistant in registry.list_assistants()? {
        beings.push(BeingSummary {
            id: assistant.id,
            kind: BeingKind::Assistant.as_str().to_string(),
            name: assistant.name,
            user_name: assistant.user_name,
            awakened: assistant.awakened,
        });
    }
    Ok(beings)
}

/// Delete a user assistant. The core cognitive being is never deletable.
#[tauri::command]
pub fn delete_assistant_being(request: DeleteAssistantBeingRequest) -> Result<(), String> {
    registry().delete_assistant(&request.id)
}

/// Cognitive framework group: the five cognitive tools, the live PSI state, and
/// the core being. The tool list is read from the registered tool table, so it
/// never drifts from what the framework actually exposes.
#[tauri::command]
pub async fn get_cognitive_framework_info() -> Result<CognitiveFrameworkInfo, String> {
    let tools = trinity_tools(TrinityBackend::global())
        .into_iter()
        .map(|tool| CognitiveFrameworkToolInfo {
            name: tool.name.to_string(),
            description: tool.description.to_string(),
        })
        .collect();

    let cognitive_state = TrinityBackend::global()
        .call("get_cognitive_state", json!({}))
        .await
        .ok()
        .map(|state| numeric::round_to_one_decimal(&state));

    let being = registry().load_core()?.map(core_being_info);

    Ok(CognitiveFrameworkInfo {
        id: "trinity_cognitive".to_string(),
        tools,
        cognitive_state,
        being,
    })
}
