//! R7c bridge between the async Plugin Host UI channel and the synchronous
//! [`PluginUiProvider`] trait (design §7.4).
//!
//! A dedicated worker thread owns a current-thread tokio runtime and drains
//! the host RPC (`host.ui.describe` / `host.ui.event` through the adapter
//! client). The provider methods never touch the RPC: they read the last
//! known-good caches, so the gpui render path stays zero-block. Dispatch is
//! two-phase — post to the worker, return immediately; the worker writes the
//! replacement tree into the cache and reports the outcome through the event
//! callback the host surface (shell) subscribes to.

use crate::plugin_ui::{PluginUiEventPayload, PluginUiProvider};
use openbitfun_opencode_plugin_host::{
    PluginDeclaration, PluginGenerationLease, PluginHost, PluginHostClient, PluginHostConfig,
    PluginHostShutdownPolicy, PluginHostShutdownReport, PluginInstanceOpenRequest, PluginUiSection,
    GENERATION_FENCING_V1, UI_CONTRIBUTIONS_V1,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Worker RPC deadline. Mirrors the shell's `LOOP_TIMEOUT` so a hung plugin
/// resolves on both sides within the same budget (design §7.4).
pub const UI_HOST_RPC_TIMEOUT: Duration = Duration::from_secs(2);

/// Async transport the worker drains. The production impl wraps the adapter
/// client; tests drive the bridge with in-memory fakes.
#[async_trait::async_trait]
pub trait UiHostTransport: Send + Sync + 'static {
    async fn describe(&self) -> Result<Vec<PluginUiSection>, String>;
    async fn dispatch_event(
        &self,
        plugin_id: Option<&str>,
        plugin_spec: &str,
        scene_id: &str,
        action: &str,
        payload: Option<&Value>,
    ) -> Result<Option<Value>, String>;
}

/// The adapter-client transport used in production.
pub struct PluginHostUiTransport {
    client: PluginHostClient,
    lease: PluginGenerationLease,
    deadline: Duration,
}

impl PluginHostUiTransport {
    pub fn new(client: PluginHostClient, lease: PluginGenerationLease) -> Self {
        Self {
            client,
            lease,
            deadline: UI_HOST_RPC_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl UiHostTransport for PluginHostUiTransport {
    async fn describe(&self) -> Result<Vec<PluginUiSection>, String> {
        self.client
            .describe_ui(&self.lease, self.deadline)
            .await
            .map_err(|error| error.to_string())
    }

    async fn dispatch_event(
        &self,
        plugin_id: Option<&str>,
        plugin_spec: &str,
        scene_id: &str,
        action: &str,
        payload: Option<&Value>,
    ) -> Result<Option<Value>, String> {
        self.client
            .dispatch_ui_event(
                &self.lease,
                plugin_id,
                plugin_spec,
                scene_id,
                action,
                payload,
                self.deadline,
            )
            .await
            .map_err(|error| error.to_string())
    }
}

/// Why the worker woke the host surface.
#[derive(Debug, Clone, PartialEq)]
pub enum UiHostBridgeEvent {
    /// The initial describe landed; the named plugins have providers ready to
    /// install. Only then does the host surface install them (design §7.4:
    /// install happens after the initial snapshot, so projection reads always
    /// hit warm caches). An empty key list means the instance carries no UI
    /// contributions — not a failure.
    Ready { plugin_keys: Vec<String> },
    /// The initial describe failed; no UI contributions activate for this
    /// instance and the surface shows the explicit reason.
    Failed(String),
    /// A dispatched interaction settled.
    SceneUpdate(PluginUiSceneUpdate),
}

/// Result of one dispatched interaction, reported to the host surface.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginUiSceneOutcome {
    /// The plugin returned a replacement tree; the cache holds it.
    TreeReplaced,
    /// The plugin kept the current tree.
    Unchanged,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginUiSceneUpdate {
    pub plugin_key: String,
    pub scene_id: String,
    pub action: String,
    pub outcome: Result<PluginUiSceneOutcome, String>,
}

struct PluginSlice {
    plugin_key: String,
    plugin_id: Option<String>,
    plugin_spec: String,
    /// Serialized `contributions.json` body served by [`PluginUiProvider::describe`].
    describe_body: String,
    package_files: BTreeSet<String>,
    /// Scene id → tree body; seeded with the initial entry files, replaced
    /// whole on dispatched events.
    trees: Mutex<BTreeMap<String, String>>,
}

struct BridgeShared {
    stopped: AtomicBool,
    on_event: Mutex<Option<Arc<dyn Fn(UiHostBridgeEvent) + Send + Sync>>>,
    sections: Mutex<BTreeMap<String, Arc<PluginUiSection>>>,
}

/// The per-instance bridge: one worker, one RPC channel, one provider per
/// contributing plugin.
pub struct UiHostBridge {
    shared: Arc<BridgeShared>,
    command_tx: tokio::sync::mpsc::Sender<WorkerCommand>,
    rpc_timeout: Duration,
}

enum WorkerCommand {
    Dispatch {
        slice: Arc<PluginSlice>,
        scene_id: String,
        action: String,
        payload: Option<PluginUiEventPayload>,
    },
}

impl UiHostBridge {
    /// Spawns the worker with the event sink installed before the thread
    /// starts, so the initial [`UiHostBridgeEvent::Ready`] cannot race the
    /// subscription.
    pub fn spawn(transport: Arc<dyn UiHostTransport>) -> Self {
        Self::spawn_with_timeout(transport, UI_HOST_RPC_TIMEOUT, Arc::new(|_| {}))
    }

    pub fn spawn_with_timeout(
        transport: Arc<dyn UiHostTransport>,
        rpc_timeout: Duration,
        on_event: Arc<dyn Fn(UiHostBridgeEvent) + Send + Sync>,
    ) -> Self {
        let shared = Arc::new(BridgeShared {
            stopped: AtomicBool::new(false),
            on_event: Mutex::new(Some(on_event)),
            sections: Mutex::new(BTreeMap::new()),
        });
        let (command_tx, command_rx) = tokio::sync::mpsc::channel::<WorkerCommand>(32);
        let worker_shared = shared.clone();
        std::thread::Builder::new()
            .name("plugin-ui-host-bridge".to_string())
            .spawn(move || {
                run_worker(transport, worker_shared, command_rx, rpc_timeout);
            })
            .expect("plugin ui host worker thread should spawn");
        Self {
            shared,
            command_tx,
            rpc_timeout,
        }
    }

    /// The provider view for one contributing plugin; `None` before the
    /// worker's describe landed or after shutdown.
    pub fn provider(&self, plugin_key: &str) -> Option<Arc<dyn PluginUiProvider>> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return None;
        }
        let section = self
            .shared
            .sections
            .lock()
            .expect("bridge sections lock")
            .get(plugin_key)
            .cloned()?;
        let describe_body = serde_json::to_string(&section.contributions).ok()?;
        // Scene id → initial tree body (entry files keyed by path in the
        // registration; the projection addresses trees by scene id).
        let mut trees = BTreeMap::new();
        if let Some(scenes) = section
            .contributions
            .get("scenes")
            .and_then(Value::as_array)
        {
            for scene in scenes {
                let (Some(id), Some(entry)) = (
                    scene.get("id").and_then(Value::as_str),
                    scene.get("entry").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if let Some(body) = section.files.get(entry) {
                    trees.insert(id.to_string(), body.clone());
                }
            }
        }
        Some(Arc::new(HostPluginUi {
            slice: Arc::new(PluginSlice {
                plugin_key: plugin_key.to_string(),
                plugin_id: section.plugin_id.clone(),
                plugin_spec: section.plugin_spec.clone(),
                describe_body,
                package_files: section.files.keys().cloned().collect(),
                trees: Mutex::new(trees),
            }),
            shared: self.shared.clone(),
            command_tx: self.command_tx.clone(),
        }))
    }

    pub fn rpc_timeout(&self) -> Duration {
        self.rpc_timeout
    }

    /// Stops the worker. Installed providers turn into explicit errors
    /// (`ui host channel is stopped`), which the projection surfaces as
    /// provider-error states — the retract path for instance close
    /// (design §7.5). Never blocks on the worker thread.
    pub fn shutdown(&self) {
        self.shared.stopped.store(true, Ordering::Release);
    }
}

impl Drop for UiHostBridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct HostPluginUi {
    slice: Arc<PluginSlice>,
    shared: Arc<BridgeShared>,
    command_tx: tokio::sync::mpsc::Sender<WorkerCommand>,
}

impl PluginUiProvider for HostPluginUi {
    fn describe(&self) -> Result<String, String> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err("ui host channel is stopped".to_string());
        }
        Ok(self.slice.describe_body.clone())
    }

    fn package_files(&self) -> BTreeSet<String> {
        self.slice.package_files.clone()
    }

    fn tree(&self, scene_id: &str) -> Result<String, String> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err("ui host channel is stopped".to_string());
        }
        self.slice
            .trees
            .lock()
            .expect("bridge tree cache lock")
            .get(scene_id)
            .cloned()
            .ok_or_else(|| format!("ui host channel has no tree for scene {scene_id}"))
    }

    fn dispatch(
        &self,
        scene_id: &str,
        action: &str,
        payload: Option<&PluginUiEventPayload>,
    ) -> Result<Option<String>, String> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err("ui host channel is stopped".to_string());
        }
        self.command_tx
            .try_send(WorkerCommand::Dispatch {
                slice: self.slice.clone(),
                scene_id: scene_id.to_string(),
                action: action.to_string(),
                payload: payload.cloned(),
            })
            .map_err(|error| format!("ui host worker is not accepting dispatches: {error}"))?;
        // Two-phase dispatch (design §7.4): the worker owns the RPC; the
        // shell stays in its in-flight state until the SceneUpdate event.
        Ok(None)
    }

    fn dispatch_deferred(&self) -> bool {
        true
    }
}

fn run_worker(
    transport: Arc<dyn UiHostTransport>,
    shared: Arc<BridgeShared>,
    mut command_rx: tokio::sync::mpsc::Receiver<WorkerCommand>,
    rpc_timeout: Duration,
) {
    // The worker thread owns its reactor: no tokio context exists on a bare
    // thread, and current-thread keeps the bridge footprint minimal.
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            emit(
                &shared,
                UiHostBridgeEvent::Failed(format!(
                    "ui host worker failed to build its runtime: {error}"
                )),
            );
            return;
        }
    };

    let described =
        runtime.block_on(async { tokio::time::timeout(rpc_timeout, transport.describe()).await });
    let sections = match described {
        Ok(Ok(sections)) => sections,
        Ok(Err(error)) => {
            emit(
                &shared,
                UiHostBridgeEvent::Failed(format!("ui host describe failed: {error}")),
            );
            return;
        }
        Err(_) => {
            emit(
                &shared,
                UiHostBridgeEvent::Failed(format!(
                    "ui host describe timed out after {} ms",
                    rpc_timeout.as_millis()
                )),
            );
            return;
        }
    };
    let ready_keys = sections
        .iter()
        .map(|section| plugin_key_of(section))
        .collect::<Vec<_>>();
    {
        let mut stored = shared.sections.lock().expect("bridge sections lock");
        for section in sections {
            stored.insert(plugin_key_of(&section), Arc::new(section));
        }
    }
    // Empty describe still means "no UI contributions" — a Ready with no
    // providers, not a failure.
    emit(
        &shared,
        UiHostBridgeEvent::Ready {
            plugin_keys: ready_keys,
        },
    );

    while let Some(command) = command_rx.blocking_recv() {
        if shared.stopped.load(Ordering::Acquire) {
            return;
        }
        match command {
            WorkerCommand::Dispatch {
                slice,
                scene_id,
                action,
                payload,
            } => {
                let wire_payload = payload.as_ref().map(payload_to_wire);
                let dispatched = runtime.block_on(async {
                    tokio::time::timeout(
                        rpc_timeout,
                        transport.dispatch_event(
                            slice.plugin_id.as_deref(),
                            &slice.plugin_spec,
                            &scene_id,
                            &action,
                            wire_payload.as_ref(),
                        ),
                    )
                    .await
                });
                let outcome = match dispatched {
                    Ok(Ok(Some(tree))) => match serde_json::to_string(&tree) {
                        Ok(body) => {
                            slice
                                .trees
                                .lock()
                                .expect("bridge tree cache lock")
                                .insert(scene_id.clone(), body);
                            Ok(PluginUiSceneOutcome::TreeReplaced)
                        }
                        Err(error) => Err(format!(
                            "ui host event returned a tree that cannot be serialized: {error}"
                        )),
                    },
                    Ok(Ok(None)) => Ok(PluginUiSceneOutcome::Unchanged),
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(format!(
                        "ui host event timed out after {} ms",
                        rpc_timeout.as_millis()
                    )),
                };
                emit(
                    &shared,
                    UiHostBridgeEvent::SceneUpdate(PluginUiSceneUpdate {
                        plugin_key: slice.plugin_key.clone(),
                        scene_id,
                        action,
                        outcome,
                    }),
                );
            }
        }
    }
}

fn plugin_key_of(section: &PluginUiSection) -> String {
    section
        .plugin_id
        .clone()
        .unwrap_or_else(|| section.plugin_spec.clone())
}

fn emit(shared: &BridgeShared, event: UiHostBridgeEvent) {
    let sink = shared.on_event.lock().expect("bridge event sink lock");
    if let Some(on_event) = sink.as_ref() {
        on_event(event);
    }
}

fn payload_to_wire(payload: &PluginUiEventPayload) -> Value {
    match payload {
        PluginUiEventPayload::Text(text) => json!({ "kind": "text", "value": text }),
        PluginUiEventPayload::Number(number) => json!({ "kind": "number", "value": number }),
        PluginUiEventPayload::Bool(value) => json!({ "kind": "boolean", "value": value }),
    }
}

static NEXT_UI_TEST_CHANNEL: AtomicU64 = AtomicU64::new(1);
const UI_TEST_HOST_LOG_LEVEL: &str = "info";

/// A self-contained plugin host + UI bridge pair for host-driven UI smoke
/// verification (design §7.7). It bypasses the global plugin configuration:
/// the channel starts a dedicated extension host, opens one local file plugin
/// directory directly, and hands out the bridge so the host surface can
/// install providers once [`UiHostBridgeEvent::Ready`] lands.
pub struct UiTestChannel {
    bridge: Arc<UiHostBridge>,
    host: PluginHost,
}

impl UiTestChannel {
    pub fn bridge(&self) -> Arc<UiHostBridge> {
        self.bridge.clone()
    }

    /// Retracts installed providers first (bridge shutdown turns provider
    /// reads into explicit errors) and then reaps the host process tree.
    /// The [`UiHostBridge`] worker thread is kept alive by the host surface
    /// until this runs; a dropped channel stops the bridge via `Drop` but the
    /// host process must always be reaped here.
    pub async fn shutdown(self) -> PluginHostShutdownReport {
        self.bridge.shutdown();
        self.host
            .shutdown(PluginHostShutdownPolicy::default())
            .await
    }
}

/// Opens one local file plugin's UI contributions on a dedicated extension
/// host (design §7.7 test channel). The host must negotiate
/// `generation-fencing-v1` + `ui-contributions-v1` and the open result must
/// carry a non-empty `ui` section — anything else is an explicit error and
/// the host is reaped, never a silent empty channel. `on_event` receives the
/// bridge lifecycle events; install providers on [`UiHostBridgeEvent::Ready`]
/// and follow dispatch settlements on [`UiHostBridgeEvent::SceneUpdate`].
pub async fn open_ui_test_channel(
    plugin_dir: impl Into<PathBuf>,
    on_event: Arc<dyn Fn(UiHostBridgeEvent) + Send + Sync>,
) -> crate::OpenBitFunResult<UiTestChannel> {
    let plugin_dir = plugin_dir.into();
    if !plugin_dir.is_dir() {
        return Err(crate::OpenBitFunError::Validation(format!(
            "Plugin UI test channel requires an existing plugin directory: {}",
            plugin_dir.display()
        )));
    }
    let directory = dunce::canonicalize(&plugin_dir)
        .map_err(|error| {
            crate::OpenBitFunError::Io(std::io::Error::other(format!(
                "Failed to canonicalize the plugin UI test channel directory {}: {error}",
                plugin_dir.display()
            )))
        })?
        .to_string_lossy()
        .into_owned();

    let launch_spec = crate::plugin_host::PluginHostLaunchSpec::bun();
    let entry = crate::plugin_host::resolve_host_entry(launch_spec)?;
    let working_directory = entry.parent().ok_or_else(|| {
        crate::OpenBitFunError::config(format!(
            "{} plugin host entry has no parent directory: {}",
            launch_spec.runtime_name,
            entry.display()
        ))
    })?;
    let host = PluginHost::start(PluginHostConfig {
        runtime_command: std::env::var_os(launch_spec.command_env)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(launch_spec.default_command)),
        entry: entry.clone(),
        working_directory: working_directory.to_path_buf(),
        cache_directory: std::env::temp_dir().join("openbitfun-plugin-ui-test-channel"),
        log_file: std::env::temp_dir().join("openbitfun-plugin-ui-test-channel-host.log"),
        log_level: UI_TEST_HOST_LOG_LEVEL.to_string(),
    })
    .await
    .map_err(|error| {
        crate::OpenBitFunError::ProcessError(format!(
            "Failed to start the plugin UI test channel host from {}: {error}",
            entry.display()
        ))
    })?;
    match open_ui_test_channel_on_host(&host, &directory, on_event).await {
        Ok(bridge) => Ok(UiTestChannel {
            bridge: Arc::new(bridge),
            host,
        }),
        Err(error) => {
            let report = host.shutdown(PluginHostShutdownPolicy::default()).await;
            log::warn!(
                "Plugin UI test channel open failed; host reaped: disposition={:?}, error={}",
                report.disposition,
                error
            );
            Err(error)
        }
    }
}

async fn open_ui_test_channel_on_host(
    host: &PluginHost,
    directory: &str,
    on_event: Arc<dyn Fn(UiHostBridgeEvent) + Send + Sync>,
) -> crate::OpenBitFunResult<UiHostBridge> {
    let client = host.client();
    for capability in [GENERATION_FENCING_V1, UI_CONTRIBUTIONS_V1] {
        if !client.capabilities().supports(capability) {
            return Err(crate::OpenBitFunError::ProcessError(format!(
                "Plugin UI test channel host did not negotiate {capability}"
            )));
        }
    }
    let sequence = NEXT_UI_TEST_CHANNEL.fetch_add(1, Ordering::Relaxed);
    let instance_id = format!("openbitfun:ui-test:{sequence}");
    let open_result = client
        .open_instance(
            PluginInstanceOpenRequest {
                instance_id: instance_id.clone(),
                generation_key: format!("ui-test-generation-{sequence}"),
                revision: format!("revision-{sequence}"),
                project: json!({ "id": "openbitfun-plugin-ui-test", "worktree": directory }),
                config: serde_json::Map::new(),
                directory: directory.to_string(),
                worktree: directory.to_string(),
                plugins: vec![PluginDeclaration {
                    spec: directory.to_string(),
                    options: None,
                    base_directory: None,
                }],
                configuration_fingerprint: None,
                expected_content_digests: None,
                expected_review_digest: None,
            },
            Duration::from_secs(30),
        )
        .await
        .map_err(|error| {
            crate::OpenBitFunError::ProcessError(format!(
                "Plugin UI test channel instance open failed: {error}"
            ))
        })?;
    let ui_count = open_result
        .get("ui")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if ui_count == 0 {
        return Err(crate::OpenBitFunError::Validation(format!(
            "Plugin {directory} carries no UI contributions; the test channel refuses to open an empty UI surface"
        )));
    }
    let lease = lease_from_open_result(&open_result, &instance_id)?;
    let transport = Arc::new(PluginHostUiTransport::new(client, lease));
    Ok(UiHostBridge::spawn_with_timeout(
        transport,
        UI_HOST_RPC_TIMEOUT,
        on_event,
    ))
}

/// The host echoes the lease it activated; those echoed values are the
/// authoritative fencing identity for every later `host.ui.*` call.
fn lease_from_open_result(
    result: &Value,
    fallback_instance_id: &str,
) -> crate::OpenBitFunResult<PluginGenerationLease> {
    let missing = |field: &str| {
        crate::OpenBitFunError::ProcessError(format!(
            "Plugin UI test channel open result is missing {field}"
        ))
    };
    Ok(PluginGenerationLease {
        instance_id: result
            .get("instanceID")
            .and_then(Value::as_str)
            .unwrap_or(fallback_instance_id)
            .to_string(),
        generation_key: result
            .get("generationKey")
            .and_then(Value::as_str)
            .ok_or_else(|| missing("generationKey"))?
            .to_string(),
        revision: result
            .get("revision")
            .and_then(Value::as_str)
            .ok_or_else(|| missing("revision"))?
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbitfun_opencode_plugin_host::PluginUiSection;
    use std::sync::mpsc;
    use std::time::Instant;

    const BOARD_KEY: &str = "fixture.ui-board";
    const INITIAL_TREE: &str =
        r#"{"schemaVersion":1,"root":{"type":"text","props":{"text":"hello"}}}"#;

    fn board_section() -> PluginUiSection {
        PluginUiSection {
            plugin_id: Some(BOARD_KEY.to_string()),
            plugin_spec: "/pkg/board.ts".to_string(),
            contributions: json!({
                "schemaVersion": 1,
                "locales": {},
                "scenes": [
                    { "id": "board", "title": { "en-US": "Board" }, "icon": "layout-dashboard", "slot": { "kind": "tab" }, "entry": "ui/scenes/board.json" }
                ]
            }),
            files: BTreeMap::from([("ui/scenes/board.json".to_string(), INITIAL_TREE.to_string())]),
        }
    }

    /// Scripted transport: describe answers `sections`; each dispatch pops the
    /// next scripted response (default: unchanged) after `dispatch_delay`.
    struct ScriptedTransport {
        sections: Vec<PluginUiSection>,
        describe_error: Option<String>,
        responses: Mutex<std::collections::VecDeque<Result<Option<Value>, String>>>,
        dispatch_delay: Duration,
        observed_payloads: Mutex<Vec<Option<Value>>>,
    }

    impl ScriptedTransport {
        fn new(sections: Vec<PluginUiSection>) -> Self {
            Self {
                sections,
                describe_error: None,
                responses: Mutex::new(std::collections::VecDeque::new()),
                dispatch_delay: Duration::ZERO,
                observed_payloads: Mutex::new(Vec::new()),
            }
        }

        fn with_response(mut self, response: Result<Option<Value>, String>) -> Self {
            self.responses
                .lock()
                .expect("responses")
                .push_back(response);
            self
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.dispatch_delay = delay;
            self
        }
    }

    #[async_trait::async_trait]
    impl UiHostTransport for ScriptedTransport {
        async fn describe(&self) -> Result<Vec<PluginUiSection>, String> {
            if let Some(error) = &self.describe_error {
                return Err(error.clone());
            }
            Ok(self.sections.clone())
        }

        async fn dispatch_event(
            &self,
            plugin_id: Option<&str>,
            plugin_spec: &str,
            scene_id: &str,
            action: &str,
            payload: Option<&Value>,
        ) -> Result<Option<Value>, String> {
            assert_eq!(plugin_id, Some(BOARD_KEY));
            assert_eq!(plugin_spec, "/pkg/board.ts");
            assert_eq!(scene_id, "board");
            self.observed_payloads
                .lock()
                .expect("payloads")
                .push(payload.cloned());
            // Async delay: a hung host must stay cancelable by the worker's
            // tokio timeout, exactly like the real adapter client.
            if !self.dispatch_delay.is_zero() {
                tokio::time::sleep(self.dispatch_delay).await;
            }
            self.responses
                .lock()
                .expect("responses")
                .pop_front()
                .unwrap_or(Ok(None))
                .map(|response| {
                    assert_eq!(action, "increment");
                    response
                })
        }
    }

    fn event_channel() -> (
        Arc<dyn Fn(UiHostBridgeEvent) + Send + Sync>,
        mpsc::Receiver<UiHostBridgeEvent>,
    ) {
        let (tx, rx) = mpsc::channel();
        let sink = Arc::new(move |event: UiHostBridgeEvent| {
            let _ = tx.send(event);
        }) as Arc<dyn Fn(UiHostBridgeEvent) + Send + Sync>;
        (sink, rx)
    }

    fn wait_for_ready(rx: &mpsc::Receiver<UiHostBridgeEvent>) -> UiHostBridgeEvent {
        let event = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the bridge should report ready");
        assert!(
            matches!(event, UiHostBridgeEvent::Ready { .. }),
            "{event:?}"
        );
        event
    }

    #[test]
    fn bridge_reports_ready_and_serves_cached_documents() {
        let (sink, rx) = event_channel();
        let bridge = UiHostBridge::spawn_with_timeout(
            Arc::new(ScriptedTransport::new(vec![board_section()])),
            Duration::from_secs(1),
            sink,
        );
        let ready = wait_for_ready(&rx);
        assert_eq!(
            ready,
            UiHostBridgeEvent::Ready {
                plugin_keys: vec![BOARD_KEY.to_string()]
            }
        );

        let provider = bridge
            .provider(BOARD_KEY)
            .expect("the ready plugin should expose a provider");
        let describe = provider
            .describe()
            .expect("describe should serve the cache");
        assert!(describe.contains("board"));
        assert_eq!(
            provider.package_files(),
            BTreeSet::from(["ui/scenes/board.json".to_string()])
        );
        let tree = provider
            .tree("board")
            .expect("the initial tree should be cached");
        assert_eq!(tree, INITIAL_TREE);
        assert!(provider.tree("missing").is_err());
    }

    #[test]
    fn dispatch_is_deferred_and_replaces_the_tree_on_settlement() {
        let replacement = json!({
            "schemaVersion": 1,
            "root": { "type": "text", "props": { "text": "replaced" } }
        });
        let transport = Arc::new(
            ScriptedTransport::new(vec![board_section()])
                .with_response(Ok(Some(replacement.clone()))),
        );
        let (sink, rx) = event_channel();
        let bridge =
            UiHostBridge::spawn_with_timeout(transport.clone(), Duration::from_secs(1), sink);
        wait_for_ready(&rx);
        let provider = bridge.provider(BOARD_KEY).expect("provider");

        assert!(provider.dispatch_deferred());
        let posted = provider
            .dispatch(
                "board",
                "increment",
                Some(&PluginUiEventPayload::Text("hi".into())),
            )
            .expect("posting should succeed");
        assert_eq!(posted, None);

        let event = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the settlement should reach the callback");
        assert_eq!(
            event,
            UiHostBridgeEvent::SceneUpdate(PluginUiSceneUpdate {
                plugin_key: BOARD_KEY.to_string(),
                scene_id: "board".to_string(),
                action: "increment".to_string(),
                outcome: Ok(PluginUiSceneOutcome::TreeReplaced),
            })
        );
        let tree = provider
            .tree("board")
            .expect("the replacement should be cached");
        assert_eq!(tree, serde_json::to_string(&replacement).unwrap());
        assert_eq!(
            transport
                .observed_payloads
                .lock()
                .expect("payloads")
                .as_slice(),
            &[Some(json!({ "kind": "text", "value": "hi" }))]
        );
    }

    #[test]
    fn unchanged_and_failed_settlements_keep_the_current_tree() {
        let transport = Arc::new(
            ScriptedTransport::new(vec![board_section()])
                .with_response(Ok(None))
                .with_response(Err("plugin handler exploded".to_string())),
        );
        let (sink, rx) = event_channel();
        let bridge = UiHostBridge::spawn_with_timeout(transport, Duration::from_secs(1), sink);
        wait_for_ready(&rx);
        let provider = bridge.provider(BOARD_KEY).expect("provider");
        provider
            .dispatch("board", "increment", None)
            .expect("posting should succeed");
        provider
            .dispatch("board", "increment", None)
            .expect("posting should succeed");

        let unchanged = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the unchanged settlement should arrive");
        assert_eq!(
            unchanged,
            UiHostBridgeEvent::SceneUpdate(PluginUiSceneUpdate {
                plugin_key: BOARD_KEY.to_string(),
                scene_id: "board".to_string(),
                action: "increment".to_string(),
                outcome: Ok(PluginUiSceneOutcome::Unchanged),
            })
        );
        let failed = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the failed settlement should arrive");
        assert_eq!(
            failed,
            UiHostBridgeEvent::SceneUpdate(PluginUiSceneUpdate {
                plugin_key: BOARD_KEY.to_string(),
                scene_id: "board".to_string(),
                action: "increment".to_string(),
                outcome: Err("plugin handler exploded".to_string()),
            })
        );
        assert_eq!(
            provider.tree("board").expect("the tree must survive"),
            INITIAL_TREE
        );
    }

    #[test]
    fn a_hung_transport_times_out_with_a_diagnostic() {
        let transport = Arc::new(
            ScriptedTransport::new(vec![board_section()])
                .with_response(Ok(None))
                .with_delay(Duration::from_millis(300)),
        );
        let (sink, rx) = event_channel();
        let bridge = UiHostBridge::spawn_with_timeout(transport, Duration::from_millis(50), sink);
        wait_for_ready(&rx);
        let provider = bridge.provider(BOARD_KEY).expect("provider");
        let started_at = Instant::now();
        provider
            .dispatch("board", "increment", None)
            .expect("posting should succeed");

        let event = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the timeout settlement should arrive");
        assert!(
            started_at.elapsed() < Duration::from_millis(250),
            "the worker must not block past the rpc timeout"
        );
        assert_eq!(
            event,
            UiHostBridgeEvent::SceneUpdate(PluginUiSceneUpdate {
                plugin_key: BOARD_KEY.to_string(),
                scene_id: "board".to_string(),
                action: "increment".to_string(),
                outcome: Err("ui host event timed out after 50 ms".to_string()),
            })
        );
    }

    #[test]
    fn a_failed_describe_reports_failure_and_stays_providerless() {
        let mut transport = ScriptedTransport::new(Vec::new());
        transport.describe_error = Some("host offline".to_string());
        let (sink, rx) = event_channel();
        let bridge =
            UiHostBridge::spawn_with_timeout(Arc::new(transport), Duration::from_secs(1), sink);

        let event = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("the failure should reach the callback");
        assert_eq!(
            event,
            UiHostBridgeEvent::Failed("ui host describe failed: host offline".to_string())
        );
        assert!(bridge.provider(BOARD_KEY).is_none());
    }

    #[test]
    fn shutdown_turns_installed_providers_into_explicit_errors() {
        let (sink, rx) = event_channel();
        let bridge = UiHostBridge::spawn_with_timeout(
            Arc::new(ScriptedTransport::new(vec![board_section()])),
            Duration::from_secs(1),
            sink,
        );
        wait_for_ready(&rx);
        let provider = bridge.provider(BOARD_KEY).expect("provider");

        bridge.shutdown();
        assert_eq!(
            provider
                .describe()
                .expect_err("a stopped channel must fail loudly"),
            "ui host channel is stopped"
        );
        assert_eq!(
            provider
                .dispatch("board", "increment", None)
                .expect_err("a stopped channel must fail loudly"),
            "ui host channel is stopped"
        );
        assert!(bridge.provider(BOARD_KEY).is_none());
    }

    #[test]
    fn lease_extraction_takes_the_echoed_generation_identity() {
        let lease = lease_from_open_result(
            &json!({
                "instanceID": "openbitfun:ui-test:9",
                "generationKey": "ui-test-generation-9",
                "revision": "revision-9",
            }),
            "openbitfun:ui-test:1",
        )
        .expect("a complete open result carries a lease");
        assert_eq!(lease.instance_id, "openbitfun:ui-test:9");
        assert_eq!(lease.generation_key, "ui-test-generation-9");
        assert_eq!(lease.revision, "revision-9");

        let missing_revision =
            lease_from_open_result(&json!({ "generationKey": "gen" }), "openbitfun:ui-test:1")
                .expect_err("a missing revision must fail the open");
        assert!(missing_revision.to_string().contains("revision"));
    }

    #[tokio::test]
    async fn open_ui_test_channel_rejects_a_missing_plugin_directory_without_a_host() {
        let missing =
            std::env::temp_dir().join(format!("openbitfun-ui-test-missing-{}", std::process::id()));
        let error = match open_ui_test_channel(missing, Arc::new(|_| {})).await {
            Err(error) => error,
            Ok(_) => panic!("a missing plugin directory must fail before any host spawn"),
        };
        assert!(error.to_string().contains("plugin directory"));
    }
}
