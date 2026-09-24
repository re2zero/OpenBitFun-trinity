//! Trinity cognitive engine TCP backend.
//!
//! Talks to the `trinityd` daemon over the Trinity frame protocol:
//! 4-byte big-endian u32 length prefix + UTF-8 JSON payload.
//!
//! A connection is reused across calls and only re-established while one is
//! being obtained, where nothing has been sent yet. A stalled exchange is
//! bounded by a timeout; its stream is dropped and the error returned, so a
//! non-idempotent method is never silently re-executed. All Trinity-specific
//! logic lives behind this backend so the rest of the desktop host only
//! depends on the generic cognitive hooks.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

/// Port the host probes when `TRINITY_DAEMON_PORT` is unset. The spawned
/// daemon takes its own port from its config, so this must match `daemon.port`.
pub(crate) const DEFAULT_DAEMON_PORT: u16 = 11656;

const FRAME_HEADER_LEN: usize = 4;
const MAX_FRAME_LEN: usize = 10_000_000;

/// Upper bound for one request/response exchange. A daemon that stalls must not
/// block a caller forever, so the transport read/write is bounded by this.
#[cfg(not(test))]
const RPC_TIMEOUT: Duration = Duration::from_secs(30);
/// Tests use a short bound so the timeout path runs without a long wait.
#[cfg(test)]
const RPC_TIMEOUT: Duration = Duration::from_millis(300);

/// How long to wait for a graceful `daemon.shutdown` before killing the child.
const SHUTDOWN_WAIT_STEPS: usize = 25;
const SHUTDOWN_WAIT_STEP: Duration = Duration::from_millis(100);

/// Connect bound for the exit-path shutdown frame. `TcpStream::connect` has no
/// timeout of its own and can block for minutes against a full accept backlog.
const SHUTDOWN_CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);

/// A daemon instance started by this process, tracked so host exit can stop it.
#[derive(Debug)]
struct OwnedDaemon {
    port: u16,
    child: std::process::Child,
}

/// Process-global slot for the daemon this process spawned. `Some` means the
/// child belongs to us and must be shut down on exit; a daemon discovered by
/// probe is never recorded here, so it stays independent of this process.
static OWNED_DAEMON: OnceLock<Mutex<Option<OwnedDaemon>>> = OnceLock::new();

fn owned_daemon_slot() -> &'static Mutex<Option<OwnedDaemon>> {
    OWNED_DAEMON.get_or_init(|| Mutex::new(None))
}

/// Resolve the daemon API key so the background and exit paths agree.
fn daemon_key() -> String {
    std::env::var("TRINITY_API_KEY").unwrap_or_else(|_| "trinity-local-dev-key".to_string())
}

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

/// Reusable TCP backend: the first call connects, later calls reuse the stream.
/// A stream is reconnected only when a new one is being obtained; once a
/// request may have been written, a failure ends the call.
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
            key: daemon_key(),
            conn: tokio::sync::Mutex::new(None),
        }
    }

    /// Test-only constructor that targets an explicit port.
    #[cfg(test)]
    fn for_port(port: u16) -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port,
            key: daemon_key(),
            conn: tokio::sync::Mutex::new(None),
        }
    }

    /// Global shared backend (single connection shared by tools and injectors).
    pub(crate) fn global() -> Arc<Self> {
        static GLOBAL: std::sync::OnceLock<Arc<TrinityBackend>> = std::sync::OnceLock::new();
        GLOBAL
            .get_or_init(|| Arc::new(TrinityBackend::from_env()))
            .clone()
    }

    /// Send one request and return its `result`.
    ///
    /// A stream is only (re)connected while one is being obtained, where
    /// nothing has been sent yet. Once a request may be on the wire, a failure
    /// is final: the stream is dropped and the error returned. Retrying there
    /// would re-execute a non-idempotent method (`memorize`, `apply_feedback`)
    /// against a daemon that was merely slow rather than dead.
    pub(crate) async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let addr = (self.host.as_str(), self.port);
        let mut guard = self.conn.lock().await;

        let req = DaemonRequest {
            id: 1,
            method: method.to_string(),
            key: Some(self.key.clone()),
            params,
        };
        let frame = encode_frame(&serde_json::to_value(&req).unwrap());

        let mut connect_attempt = 0u8;
        loop {
            let mut stream = match guard.take() {
                Some(s) => s,
                None => match tokio::net::TcpStream::connect(addr).await {
                    Ok(s) => s,
                    Err(e) if connect_attempt == 0 => {
                        connect_attempt = 1;
                        log::debug!("[trinity] connect to {addr:?} failed, retrying once: {e}");
                        continue;
                    }
                    Err(e) => return Err(format!("connect trinityd {addr:?}: {e}")),
                },
            };

            // Bound the exchange so a stalled daemon cannot hang the caller.
            let result = match tokio::time::timeout(
                RPC_TIMEOUT,
                write_request_read_response(&mut stream, &frame),
            )
            .await
            {
                Ok(result) => result,
                Err(_elapsed) => Err(format!(
                    "trinityd '{method}' timed out after {RPC_TIMEOUT:?}"
                )),
            };

            match result {
                // Only a healthy stream is pooled; protocol errors keep it.
                Ok(v) => {
                    *guard = Some(stream);
                    return v;
                }
                // The request may already have been executed, so never resend
                // it here. Dropping `stream` forces a fresh connect next call.
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

/// Ask the kernel to terminate the daemon when this host dies.
///
/// The host only reaches its shutdown path on its own exit routes. A crash or an
/// external `SIGTERM` ends the process without running any of that code and used
/// to leave the daemon holding the port, after which every later host instance
/// found a listener it did not own and could never shut down again. Arming
/// `PR_SET_PDEATHSIG` covers every kind of host death, including abnormal ones.
///
/// `SIGINT` rather than `SIGTERM`: the daemon only installs a handler for the
/// former, and that handler runs its graceful path (`server.stop()` followed by
/// `save_rsi_state()`), the same one the `daemon.shutdown` frame triggers.
/// `SIGTERM` has no handler and would kill it outright.
/// Linux-only: no other platform offers an equivalent.
fn bind_daemon_to_host_lifetime(command: &mut std::process::Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;

        let host_pid = std::process::id() as libc::pid_t;
        // SAFETY: the closure runs between `fork` and `exec`, where only
        // async-signal-safe calls are sound. `prctl`, `getppid` and `_exit` are
        // raw syscalls: no allocation, no locking.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGINT) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // The host may have died before the signal was armed, in which
                // case nothing will ever be delivered.
                if libc::getppid() != host_pid {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = command;
    }
}

/// Ensure a Trinity daemon is running. Reuses an already-listening daemon;
/// otherwise spawns `trinityd` and waits for the port (up to ~15s).
pub(crate) async fn ensure_trinityd_running() -> Result<u16, String> {
    let port = std::env::var("TRINITY_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_DAEMON_PORT);

    if probe_port(port).await {
        // A listener that already exists is not ours: it may have been started
        // by the user or another client, so it is never recorded as owned and
        // host exit leaves it running. An already tracked child is preserved so
        // repeated calls do not orphan the daemon this process spawned.
        log::info!("[trinity] daemon already running on port {port}, reusing");
        return Ok(port);
    }

    let bin = locate_trinityd()
        .ok_or_else(|| "trinityd not found (set TRINITYD_BIN or build it)".to_string())?;

    // Never overwrite a tracked child without reaping it: dropping the handle
    // would orphan an unreaped process. The probe above already showed it is not
    // serving, so it is reaped before a replacement starts. This keeps
    // `trinity::init()` safe to call more than once.
    {
        let mut slot = owned_daemon_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = slot.as_mut() {
            match existing.child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    log::warn!(
                        "[trinity] reaping the tracked daemon child on port {} before respawn",
                        existing.port
                    );
                    let _ = existing.child.kill();
                    let _ = existing.child.wait();
                }
                Err(e) => {
                    log::warn!("[trinity] could not check the tracked daemon child: {e}");
                    let _ = existing.child.kill();
                    let _ = existing.child.wait();
                }
            }
            *slot = None;
        }
    }

    log::info!("[trinity] spawning trinityd: {}", bin.display());
    // No CLI overrides: the daemon reads its own config (`[daemon]` in
    // `~/.config/trinity/trinity.toml`). The `port` probed above therefore has
    // to match the configured `daemon.port`.
    let mut command = openbitfun_core::util::process_manager::create_command(&bin);
    bind_daemon_to_host_lifetime(&mut command);
    let child = command
        .spawn()
        .map_err(|e| format!("spawn trinityd {}: {e}", bin.display()))?;
    // Track the child handle (instead of forgetting it) so host exit can shut
    // down exactly the daemon this process spawned.
    *owned_daemon_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(OwnedDaemon { port, child });

    for _ in 0..60 {
        if probe_port(port).await {
            log::info!("[trinity] daemon ready on port {port}");
            return Ok(port);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!(
        "trinityd did not become ready on port {port} within 15s"
    ))
}

/// Shut down the Trinity daemon this process spawned (graceful, kill fallback).
///
/// Ownership contract: only a child tracked in [`OWNED_DAEMON`] is managed. A
/// daemon found by probe was started by the user or another client; it is an
/// independent service and is left untouched, so host exit never terminates a
/// process this host does not own.
///
/// Synchronous by design: it runs on the exit path, where async is unavailable.
/// The liveness of the tracked child is checked first: a child that already
/// exited means the port may now hold an independent daemon, so nothing is
/// signalled. While the child is confirmed alive it receives `daemon.shutdown`
/// (the daemon persists its state and exits), is waited on for up to ~2.5s, and
/// is killed if it did not exit in time.
pub(crate) fn shutdown_owned_trinityd() {
    let owned = owned_daemon_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    let Some(OwnedDaemon { port, mut child }) = owned else {
        return;
    };

    // Confirm the tracked child is still alive before signalling the port. If
    // it already exited, the listener (if any) belongs to an independent daemon
    // that this host must not stop.
    match child.try_wait() {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(e) => {
            log::warn!("[trinity] could not check daemon liveness: {e}; killing the child");
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
    }

    if let Err(e) = send_shutdown_frame(port) {
        log::warn!(
            "[trinity] graceful shutdown signal failed (port {port}): {e}; will kill the child"
        );
    }

    for _ in 0..SHUTDOWN_WAIT_STEPS {
        match child.try_wait() {
            Ok(Some(_)) => {
                log::info!("[trinity] daemon shut down gracefully with the desktop host");
                return;
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("[trinity] waiting for daemon exit failed: {e}");
                break;
            }
        }
        std::thread::sleep(SHUTDOWN_WAIT_STEP);
    }

    log::warn!("[trinity] graceful shutdown timed out, killing the daemon child");
    let _ = child.kill();
    let _ = child.wait();
}

/// Kill the owned daemon immediately, without the graceful wait.
///
/// Used by the emergency exit path (panic hooks), which must stay fast and
/// cannot afford the 2.5s shutdown budget. Only a child this process spawned is
/// affected; a probed/independent daemon is never touched.
pub(crate) fn kill_owned_trinityd_now() {
    let owned = owned_daemon_slot()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    let Some(OwnedDaemon { mut child, .. }) = owned else {
        return;
    };
    log::warn!("[trinity] killing the owned daemon child during emergency exit");
    let _ = child.kill();
    let _ = child.wait();
}

/// Send a `daemon.shutdown` frame to `127.0.0.1:port` (synchronous; the exit
/// path cannot use async). Uses the same API key source as [`TrinityBackend`].
fn send_shutdown_frame(port: u16) -> std::io::Result<()> {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};

    let payload = serde_json::to_vec(&json!({
        "id": 0,
        "method": "daemon.shutdown",
        "key": daemon_key(),
        "params": {},
    }))
    .map_err(std::io::Error::other)?;

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, SHUTDOWN_CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    let mut frame = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    stream.write_all(&frame)?;

    // Read the acknowledgement frame; a missing ack is not fatal because the
    // wait/kill fallback below still guarantees the child is stopped.
    let mut header = [0u8; FRAME_HEADER_LEN];
    let _ = stream.read_exact(&mut header);
    Ok(())
}

/// Fetch the static cognitive identity + NAP protocol block (cached by caller).
pub(crate) async fn static_prompt() -> Option<String> {
    let backend = TrinityBackend::global();
    match backend.call("get_static_prompt", json!({})).await {
        Ok(v) => v
            .get("static_system_prompt")
            .and_then(|p| p.as_str())
            .map(str::to_owned),
        Err(e) => {
            log::warn!("[trinity] get_static_prompt failed: {e}");
            None
        }
    }
}

/// Fetch the per-turn cognitive state (prepended to the latest user message).
///
/// Carries the current user message and turn id so the engine can classify the
/// real input and deduplicate its work across the tool rounds of one turn.
pub(crate) async fn cognitive_state_block(user_message: &str, turn_id: &str) -> Option<String> {
    let backend = TrinityBackend::global();
    match backend
        .call(
            "before_turn",
            json!({ "user_message": user_message, "turn_id": turn_id }),
        )
        .await
    {
        Ok(v) => v.get("cognitive_state_text").map(|s| s.to_string()),
        Err(e) => {
            log::warn!("[trinity] before_turn failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captures log records whose target is this module, so tests can assert on
    /// the no-op path without a real daemon. Filtering by target keeps records
    /// from other concurrently running tests out of the buffer.
    struct CaptureLogger;

    static LOG_RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    impl log::Log for CaptureLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if record.target().contains("trinity") {
                LOG_RECORDS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(format!("{} {}", record.level(), record.args()));
            }
        }

        fn flush(&self) {}
    }

    static CAPTURE_LOGGER: CaptureLogger = CaptureLogger;
    static INSTALL_LOGGER: std::sync::Once = std::sync::Once::new();

    fn install_capture_logger() {
        INSTALL_LOGGER.call_once(|| {
            // Ignored when another test already installed a logger.
            let _ = log::set_logger(&CAPTURE_LOGGER);
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    fn captured_count() -> usize {
        LOG_RECORDS.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// (a) With no owned child the shutdown is a silent no-op.
    #[test]
    fn shutdown_is_noop_without_owned_daemon() {
        install_capture_logger();
        // The slot is a process-global OnceLock<Mutex<Option<..>>>; take() and
        // assert the precondition instead of leaving stale state behind.
        let previous = owned_daemon_slot()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        assert!(previous.is_none(), "precondition: no owned daemon tracked");

        let before = captured_count();
        shutdown_owned_trinityd();
        let after = captured_count();

        assert_eq!(before, after, "no-op shutdown must not log");
        assert!(
            owned_daemon_slot()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none(),
            "no-op shutdown must not track a daemon"
        );
    }

    /// (b) A daemon that accepts and never replies makes `call` time out, and
    /// the timed-out stream is dropped instead of being pooled for reuse.
    #[tokio::test]
    async fn call_times_out_and_does_not_pool_the_stream() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Accept connections but never write a response, so the client read
        // blocks until its timeout fires. The accepted sockets stay open.
        let acceptor = tokio::spawn(async move {
            let mut held = Vec::new();
            while held.len() < 2 {
                match listener.accept().await {
                    Ok((stream, _)) => held.push(stream),
                    Err(_) => break,
                }
            }
            held
        });

        let backend = TrinityBackend::for_port(port);

        let started = std::time::Instant::now();
        let first = backend.call("noop", json!({})).await.unwrap_err();
        assert!(first.contains("timed out"), "unexpected error: {first}");
        assert!(
            started.elapsed() >= RPC_TIMEOUT,
            "call returned before the timeout elapsed"
        );

        let second = backend.call("noop", json!({})).await.unwrap_err();
        assert!(second.contains("timed out"), "unexpected error: {second}");

        // A pooled stream would have been reused, so no second accept would
        // happen. Two accepted sockets prove the stream was dropped.
        let held = acceptor.await.unwrap();
        assert_eq!(held.len(), 2, "each call must open its own connection");
    }
}
