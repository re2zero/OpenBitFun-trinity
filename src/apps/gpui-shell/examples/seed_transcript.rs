//! Seeds one short session whose transcript exercises the rich render path:
//! a relative file link with a line spec, an external link, a fenced Rust
//! block, a three-call work run (command / read / failed command) and a
//! `Write` row that stays outside the run.
//!
//! Writes through the sanctioned offline import path, so desktop, CLI and the
//! GPUI shell all read the same bundle. Pass a scratch workspace path; the
//! example also writes the `src/demo.rs` the transcript links to. Delete the
//! seeded project directory afterwards.
//!
//! Usage:
//! `cargo run -p openbitfun-gpui-shell --example seed_transcript -- <workspace_path>`

// The turn literal nests deeper than the default serde_json macro budget.
#![recursion_limit = "512"]

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

/// The assistant round: every markdown feature the transcript has to render.
const ASSISTANT_MARKDOWN: &str = "\
### 渲染验收

相对路径：[src/demo.rs](src/demo.rs#L40)　外部链接：[example.com](https://example.com)

```rust
fn demo() -> u32 {
    7
}
```
";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let workspace_path = args
        .next()
        .expect("usage: seed_transcript <workspace_path>");

    select_agentic_system_profile(DeliveryProfile::Cli)
        .map_err(|error| anyhow::anyhow!("select delivery profile: {error}"))?;
    initialize_global_config()
        .await
        .map_err(|error| anyhow::anyhow!("initialize global config: {error}"))?;
    let path_manager =
        try_get_path_manager_arc().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let sessions_root = path_manager.project_sessions_dir(Path::new(&workspace_path));

    // The file the transcript links to and the Read/Write rows name.
    let demo_dir = Path::new(&workspace_path).join("src");
    std::fs::create_dir_all(&demo_dir)?;
    let demo_file = demo_dir.join("demo.rs");
    // Long enough that a `#L40` reveal is observable in a screenshot.
    let mut source = String::new();
    for line in 1..=60 {
        source.push_str(&format!("// line {line}\n"));
    }
    source.push_str("fn demo() -> u32 {\n    7\n}\n");
    std::fs::write(&demo_file, source)?;
    let demo_file = demo_file.to_string_lossy().to_string();

    let session_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    let metadata = build_session_metadata(SessionMetadataBuildFacts {
        session_id: &session_id,
        session_name: "GPUI shell transcript render",
        agent_type: "Standard",
        last_user_dialog_agent_type: Some("Standard"),
        last_submitted_agent_type: Some("Standard"),
        created_by: Some("gpui-shell-seed-transcript"),
        session_kind: SessionKind::Standard,
        model_name: None,
        created_at_ms: now,
        last_active_at_ms: now,
        turn_count: 1,
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

    let turn_id = uuid::Uuid::new_v4().to_string();
    let command = "cargo test -p openbitfun-gpui-shell --lib\ncat src/demo.rs";
    let turn: DialogTurnData = serde_json::from_value(serde_json::json!({
        "turnId": turn_id,
        "turnIndex": 0,
        "sessionId": session_id,
        "timestamp": now,
        "agentType": "Standard",
        "userMessage": {
            "id": "user-0",
            "content": "渲染验收：检查工具行、代码块与链接。",
            "timestamp": now,
        },
        "modelRounds": [{
            "id": "round-0",
            "turnId": turn_id,
            "roundIndex": 0,
            "timestamp": now + 1,
            "textItems": [{
                "id": "text-0",
                "content": ASSISTANT_MARKDOWN,
                "isStreaming": false,
                "timestamp": now + 1,
                "isMarkdown": true,
                "orderIndex": 0,
            }],
            "toolItems": [
                {
                    "id": "call-cmd-ok",
                    "toolName": "ExecCommand",
                    "toolCall": { "id": "call-cmd-ok", "input": { "cmd": command } },
                    "startTime": now + 1,
                    "endTime": now + 1300,
                    "durationMs": 1210,
                    "toolResult": {
                        "success": true,
                        "durationMs": 1210,
                        "result": {
                            "chunk_id": "seed-cmd-ok",
                            "wall_time_seconds": 1.21,
                            "output": "running 289 tests\n\ntest result: ok. 289 passed; 0 failed\n",
                            "session_id": null,
                            "exit_code": 0,
                            "original_output_chars": 54,
                            "completion": { "source": "process", "status": "exited" },
                            "remote": false,
                            "workdir": workspace_path,
                            "tty": false,
                            "shell": {
                                "name": "bash", "kind": "bash", "path": "/bin/bash",
                                "invocation": "-c", "pipeline_failure_policy": "fail_fast"
                            }
                        }
                    }
                },
                {
                    "id": "call-read",
                    "toolName": "Read",
                    "toolCall": {
                        "id": "call-read",
                        "input": { "file_path": demo_file, "offset": 0, "limit": 40 }
                    },
                    "startTime": now + 1300,
                    "endTime": now + 1330,
                    "durationMs": 21,
                    "toolResult": {
                        "success": true,
                        "durationMs": 21,
                        "result": {
                            "file_path": demo_file,
                            "content": "fn demo() -> u32 {\n    7\n}\n",
                            "lines_read": 3,
                            "offset": 0
                        }
                    }
                },
                {
                    "id": "call-cmd-fail",
                    "toolName": "ExecCommand",
                    "toolCall": {
                        "id": "call-cmd-fail",
                        "input": { "cmd": "cargo test --features nope" }
                    },
                    "startTime": now + 1330,
                    "endTime": now + 1700,
                    "durationMs": 340,
                    "toolResult": {
                        "success": false,
                        "result": {},
                        "durationMs": 340,
                        "error": "error: none of the selected packages contains these features: nope"
                    }
                },
                {
                    "id": "call-write",
                    "toolName": "Write",
                    "toolCall": {
                        "id": "call-write",
                        "input": { "file_path": demo_file, "content": "fn demo() -> u32 {\n    7\n}\n" }
                    },
                    "startTime": now + 1700,
                    "endTime": now + 1720,
                    "durationMs": 12,
                    "toolResult": {
                        "success": true,
                        "durationMs": 12,
                        "result": { "file_path": demo_file, "lines_written": 3, "bytes_written": 30 }
                    }
                }
            ],
            "startTime": now + 1,
            "endTime": now + 1500,
            "attemptCount": 1,
            "status": "completed",
        }],
        "startTime": now,
        "endTime": now + 1500,
        "status": "completed",
        "hasFinalResponse": true,
    }))?;

    let store = OfflineSessionImportStore::new(&sessions_root);
    store
        .write_bundle(&OfflineSessionBundle {
            metadata,
            turns: vec![turn],
        })
        .await
        .map_err(|error| anyhow::anyhow!("write bundle: {error}"))?;
    store
        .rebuild_index()
        .await
        .map_err(|error| anyhow::anyhow!("rebuild index: {error}"))?;
    println!(
        "seeded transcript session {session_id}\n  workspace: {workspace_path}\n  linked file: {demo_file}"
    );
    Ok(())
}
