//! Seeds a synthetic long session for the M4 1k-message stress test through
//! the sanctioned offline import path, so the bundle is immediately readable
//! by desktop, CLI, and the GPUI shell (same storage owners as a live
//! session). Nothing in the live runtime is touched: pass a scratch workspace
//! path and delete the seeded project directory afterwards.
//!
//! Usage:
//! `cargo run -p openbitfun-gpui-shell --example seed_session -- <workspace_path> [turns]`

use openbitfun_core::agentic::system::select_agentic_system_profile;
use openbitfun_core::infrastructure::try_get_path_manager_arc;
use openbitfun_core::product_assembly::DeliveryProfile;
use openbitfun_core::service::config::initialize_global_config;
use openbitfun_services_core::session::types::SessionMemoryMode;
use openbitfun_services_core::session::{
    build_session_metadata, DialogTurnData, OfflineSessionBundle, OfflineSessionImportStore,
    SessionKind, SessionMetadataBuildFacts,
};
use std::path::Path;

fn assistant_markdown(index: usize) -> String {
    let paragraph = format!(
        "Answer **{index}** with a [link](https://example.com/{index}) and `inline code`.\n\n\
         - first point for round {index}\n- second point with a longer line of text that wraps\n\n\
         | column a | column b |\n|---|---|\n| {index} | value |"
    );
    if index % 10 == 0 {
        format!("{paragraph}\n\n```rust\nfn round_{index}() -> usize {{\n    {index} * 2\n}}\n```")
    } else {
        paragraph
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let workspace_path = args
        .next()
        .expect("usage: seed_session <workspace_path> [turns]");
    let turns: usize = args
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|error| anyhow::anyhow!("turn count must be a number: {error}"))?
        .unwrap_or(500);

    // Same order as the embedded bootstrap: delivery profile before config.
    select_agentic_system_profile(DeliveryProfile::Cli).context("select delivery profile")?;
    initialize_global_config()
        .await
        .map_err(|error| anyhow::anyhow!("initialize global config: {error}"))?;
    let path_manager =
        try_get_path_manager_arc().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let sessions_root = path_manager.project_sessions_dir(Path::new(&workspace_path));

    let session_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    let metadata = build_session_metadata(SessionMetadataBuildFacts {
        session_id: &session_id,
        session_name: "GPUI shell 1k stress",
        agent_type: "Standard",
        last_user_dialog_agent_type: Some("Standard"),
        last_submitted_agent_type: Some("Standard"),
        created_by: Some("gpui-shell-seed"),
        session_kind: SessionKind::Standard,
        model_name: None,
        created_at_ms: now,
        last_active_at_ms: now,
        turn_count: turns,
        snapshot_session_id: None,
        workspace_path: &workspace_path,
        project_workspace_path: None,
        execution_target: None,
        workspace_hostname: None,
        workspace_id: None,
        project_workspace_id: None,
        new_session_memory_mode: SessionMemoryMode::Enabled,
        existing: None,
    });

    let mut seeded = Vec::with_capacity(turns);
    for index in 0..turns {
        let turn_id = uuid::Uuid::new_v4().to_string();
        let stamp = now + index as u64 * 2;
        let turn: DialogTurnData = serde_json::from_value(serde_json::json!({
            "turnId": turn_id,
            "turnIndex": index,
            "sessionId": session_id,
            "timestamp": stamp,
            "agentType": "Standard",
            "userMessage": {
                "id": format!("user-{index}"),
                "content": format!("Stress round {index}: summarize item {index} in markdown."),
                "timestamp": stamp,
            },
            "modelRounds": [{
                "id": format!("round-{index}"),
                "turnId": turn_id,
                "roundIndex": 0,
                "timestamp": stamp + 1,
                "textItems": [{
                    "id": format!("text-{index}"),
                    "content": assistant_markdown(index),
                    "isStreaming": false,
                    "timestamp": stamp + 1,
                    "isMarkdown": true,
                }],
                "startTime": stamp + 1,
                "endTime": stamp + 2,
                "status": "completed",
            }],
            "startTime": stamp,
            "endTime": stamp + 2,
            "status": "completed",
            "hasFinalResponse": true,
        }))?;
        seeded.push(turn);
    }

    let store = OfflineSessionImportStore::new(&sessions_root);
    store
        .write_bundle(&OfflineSessionBundle {
            metadata,
            turns: seeded,
        })
        .await
        .map_err(|error| anyhow::anyhow!("write bundle: {error}"))?;
    store
        .rebuild_index()
        .await
        .map_err(|error| anyhow::anyhow!("rebuild index: {error}"))?;
    println!(
        "seeded session {session_id} with {turns} turns at {}",
        sessions_root.display()
    );
    Ok(())
}

use anyhow::Context;
