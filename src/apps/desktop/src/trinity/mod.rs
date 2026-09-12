//! Trinity cognitive engine integration (desktop host).
//!
//! Self-contained integration point: spawns/reuses the `trinityd` daemon,
//! registers the `trinity_*` cognitive tools, and wires the generic cognitive
//! injector (static protocol -> system prompt, live PSI state -> latest user
//! message). The daemon's static prompt is used as-is; nothing is extracted or
//! rewritten into workspace files. All Trinity-specific code lives in this
//! module; removing it leaves the host exactly as upstream.
//!
//! Env contract:
//!   TRINITYD_BIN         — path to the trinityd binary (spawn fallback)
//!   TRINITY_DAEMON_PORT  — daemon TCP port (default 11656)
//!   TRINITY_API_KEY      — daemon API key (default trinity-local-dev-key)

pub(crate) mod backend;
pub(crate) mod injector;
pub(crate) mod numeric;
pub(crate) mod tools;

/// Bring up the Trinity cognitive engine and register its injector/tools.
///
/// Safe to call multiple times: daemon reuse and registrations are idempotent.
/// Failures are logged and swallowed so a missing daemon never blocks the host.
pub(crate) async fn init() {
    match backend::ensure_trinityd_running().await {
        Ok(port) => log::info!("[trinity] cognitive engine ready on port {port}"),
        Err(e) => {
            log::warn!("[trinity] cognitive engine unavailable: {e}");
            return;
        }
    }
    injector::register_trinity_injector();
    let registered = tools::register_cognitive_tools().await;
    log::info!("[trinity] integration ready ({registered} tools)");
}
