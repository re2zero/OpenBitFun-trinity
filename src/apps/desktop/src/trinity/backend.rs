//! Trinity cognitive engine TCP backend.
//!
//! Talks to the `trinityd` daemon over the Trinity frame protocol:
//! 4-byte big-endian u32 length prefix + UTF-8 JSON payload.
//!
//! Connection is reused across calls and reconnects once on failure. All
//! Trinity-specific logic lives behind this backend so the rest of the
//! desktop host only depends on the generic cognitive hooks.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

/// Default daemon port when neither env nor config provides one.
pub(crate) const DEFAULT_DAEMON_PORT: u16 = 11656;

const FRAME_HEADER_LEN: usize = 4;
const MAX_FRAME_LEN: usize = 10_000_000;

#[derive(Debug, serde::Serialize)]
struct DaemonRequest {
    id: u64,
    method: String,
    #[serde(alias = "api_key")]
    key: Option<String>,
    #[serde(default)]
    params: Value,
}

fn encode_frame(value: &Value) -> Vec<u8> {
    let json = serde_json::to_vec(value).expect("serialize frame payload");
    let len = json.len() as u32;
    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);
    frame
}

/// Reusable TCP backend: first call connects, later calls reuse the stream,
/// and a single reconnect is attempted when the stream is stale.
#[derive(Debug)]
pub(crate) struct TrinityBackend {
    host: String,
    port: u16,
    key: String,
    conn: tokio::sync::Mutex<Option<tokio::net::TcpStream>>,
}

impl TrinityBackend {
    pub(crate) fn from_env() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: std::env::var("TRINITY_DAEMON_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_DAEMON_PORT),
            key: std::env::var("TRINITY_API_KEY")
                .unwrap_or_else(|_| "trinity-local-dev-key".to_string()),
            conn: tokio::sync::Mutex::new(None),
        }
    }

    /// Global shared backend (single connection shared by tools and injectors).
    pub(crate) fn global() -> Arc<Self> {
        static GLOBAL: std::sync::OnceLock<Arc<TrinityBackend>> = std::sync::OnceLock::new();
        GLOBAL.get_or_init(|| Arc::new(TrinityBackend::from_env())).clone()
    }

    pub(crate) async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let addr = (self.host.as_str(), self.port);
        let mut guard = self.conn.lock().await;
        let mut attempt = 0u8;
        loop {
            let mut stream = match guard.take() {
                Some(s) => s,
                None => match tokio::net::TcpStream::connect(addr).await {
                    Ok(s) => s,
                    Err(e) => return Err(format!("connect trinityd {addr:?}: {e}")),
                },
            };

            let req = DaemonRequest {
                id: 1,
                method: method.to_string(),
                key: Some(self.key.clone()),
                params: params.clone(),
            };
            let frame = encode_frame(&serde_json::to_value(&req).unwrap());

            match write_request_read_response(&mut stream, &frame).await {
                Ok(v) => {
                    *guard = Some(stream);
                    return v;
                }
                Err(_e) if attempt == 0 => {
                    attempt = 1;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}

async fn write_request_read_response(
    stream: &mut tokio::net::TcpStream,
    frame: &[u8],
) -> Result<Result<Value, String>, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(frame)
        .await
        .map_err(|e| format!("write: {e}"))?;
    let mut header = [0u8; FRAME_HEADER_LEN];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|e| format!("read header: {e}"))?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        return Ok(Err(format!("invalid frame length: {len}")));
    }
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("read body: {e}"))?;
    let v: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;

    if let Some(result) = v.get("result") {
        Ok(Ok(result.clone()))
    } else if let Some(err) = v.get("error") {
        Ok(Err(err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string()))
    } else {
        Ok(Err(format!("unexpected daemon response: {v}")))
    }
}

/// Probe whether a Trinity daemon is already listening on `port`.
pub(crate) async fn probe_port(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
}

/// Locate the `trinityd` binary: `TRINITYD_BIN` env → exe dir → PATH.
pub(crate) fn locate_trinityd() -> Option<std::path::PathBuf> {
    let names = ["trinityd", "trinityd.exe"];
    if let Ok(p) = std::env::var("TRINITYD_BIN") {
        let pb = std::path::PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in &names {
                let cand = dir.join(name);
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH")?) {
        for name in &names {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Ensure a Trinity daemon is running. Reuses an already-listening daemon;
/// otherwise spawns `trinityd` and waits for the port (up to ~15s).
pub(crate) async fn ensure_trinityd_running() -> Result<u16, String> {
    let port = std::env::var("TRINITY_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_DAEMON_PORT);

    if probe_port(port).await {
        log::info!("[trinity] daemon already running on port {port}, reusing");
        return Ok(port);
    }

    let bin = locate_trinityd()
        .ok_or_else(|| "trinityd not found (set TRINITYD_BIN or build it)".to_string())?;
    log::info!("[trinity] spawning trinityd: {}", bin.display());
    // Force daemon mode on the port we probe, so a user config that disables
    // `[daemon]` cannot leave the spawned process silent on stdio.
    let child = std::process::Command::new(&bin)
        .arg("--daemon")
        .arg("--port")
        .arg(port.to_string())
        .spawn()
        .map_err(|e| format!("spawn trinityd {}: {e}", bin.display()))?;
    // Keep the child alive for the lifetime of this process; drop does not kill.
    std::mem::forget(child);

    for _ in 0..60 {
        if probe_port(port).await {
            log::info!("[trinity] daemon ready on port {port}");
            return Ok(port);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!("trinityd did not become ready on port {port} within 15s"))
}

/// Fetch the static cognitive identity + NAP protocol block (cached by caller).
pub(crate) async fn static_prompt() -> Option<String> {
    let backend = TrinityBackend::global();
    match backend.call("get_static_prompt", json!({})).await {
        Ok(v) => v
            .get("prompt")
            .and_then(|p| p.as_str())
            .map(str::to_owned),
        Err(e) => {
            log::warn!("[trinity] get_static_prompt failed: {e}");
            None
        }
    }
}

/// Fetch the per-turn cognitive state block (prepended to the user message).
pub(crate) async fn cognitive_state_block() -> Option<String> {
    let backend = TrinityBackend::global();
    match backend.call("before_turn", json!({})).await {
        Ok(v) => v
            .get("cognitive_state")
            .map(|s| s.to_string()),
        Err(e) => {
            log::warn!("[trinity] before_turn failed: {e}");
            None
        }
    }
}

/// Fetch the per-turn sampling temperature from the PSI engine.
pub(crate) async fn sampling_temperature() -> Option<f64> {
    let backend = TrinityBackend::global();
    match backend.call("before_turn", json!({})).await {
        Ok(v) => v
            .get("sampling_params")
            .and_then(|p| p.get("temperature"))
            .and_then(|t| t.as_f64()),
        Err(e) => {
            log::warn!("[trinity] sampling temperature fetch failed: {e}");
            None
        }
    }
}