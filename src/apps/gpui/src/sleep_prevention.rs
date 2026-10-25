//! Shell host integration for the app-wide sleep-prevention preference
//! (G-SE1). The preference is intentionally independent of agent/session
//! activity: while enabled, the inhibitor lives for the shell process and is
//! released when the preference is disabled or the process exits.
//!
//! This is the desktop host's `sleep_prevention.rs` worker ported onto the
//! shell runtime: the same dedicated worker thread (the `keepawake` Linux
//! path is a blocking logind D-Bus round-trip that must never run on the UI
//! thread), the same bounded request timeout, and the same
//! apply-then-persist-then-rollback discipline around the shared
//! `app.prevent_sleep` config key. Both frontends write the same key, so the
//! last toggle wins and a config import flips both.

use std::sync::{mpsc, Arc, OnceLock};

use openbitfun_core::service::config::{subscribe_config_updates, ConfigUpdateEvent};

const PREVENT_SLEEP_CONFIG_PATH: &str = "app.prevent_sleep";
/// Ceiling for one worker round-trip. Acquiring an OS inhibitor is a local
/// call on every platform; anything slower than this is a stuck session
/// manager, not a slow one.
const WORKER_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

enum SleepPreventionRequest {
    SetEnabled {
        enabled: bool,
        response: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}

/// Owns the optional inhibitor worker. One instance serves the whole shell
/// process (see [`global`]); tests build their own so they never contend.
#[derive(Default)]
struct SleepPreventionState {
    worker: Arc<tokio::sync::Mutex<Option<mpsc::Sender<SleepPreventionRequest>>>>,
}

impl SleepPreventionState {
    /// Flips the runtime inhibitor. Must run on the tokio runtime the shell
    /// runtime handle carries; the OS call itself stays on the worker thread.
    async fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut worker = self.worker.lock().await;
        if !enabled && worker.is_none() {
            return Ok(());
        }

        if worker.is_none() {
            *worker = Some(start_worker()?);
        }

        let (response, result) = tokio::sync::oneshot::channel();
        let send_result = worker
            .as_ref()
            .ok_or_else(|| "Sleep-prevention worker failed to initialize".to_string())?
            .send(SleepPreventionRequest::SetEnabled { enabled, response });
        if send_result.is_err() {
            worker.take();
            return Err("Sleep-prevention worker is unavailable".to_string());
        }

        // Bounded on purpose. The worker's `keepawake` call is a blocking
        // D-Bus round-trip to logind on Linux, with no timeout of its own; a
        // hung or slow session manager would otherwise hold the worker slot
        // forever and wedge every later toggle, including the one that turns
        // the feature off.
        let outcome = match tokio::time::timeout(WORKER_REQUEST_TIMEOUT, result).await {
            Ok(received) => received
                .map_err(|_| "Sleep-prevention worker stopped unexpectedly".to_string())
                .and_then(|outcome| outcome),
            Err(_) => {
                // The thread is still stuck inside the OS call, so it cannot
                // be reused; dropping the sender retires it once that call
                // returns.
                Err(format!(
                    "Sleep-prevention request timed out after {} seconds",
                    WORKER_REQUEST_TIMEOUT.as_secs()
                ))
            }
        };

        // An inactive or failed worker owns no useful resources. Dropping the
        // final sender lets the thread exit instead of keeping one alive for
        // the entire application lifetime while the preference is off.
        if !enabled || outcome.is_err() {
            worker.take();
        }

        outcome
    }
}

/// The process-wide inhibitor owner.
fn global() -> &'static SleepPreventionState {
    static STATE: OnceLock<SleepPreventionState> = OnceLock::new();
    STATE.get_or_init(SleepPreventionState::default)
}

fn start_worker() -> Result<mpsc::Sender<SleepPreventionRequest>, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("openbitfun-sleep-prevention".to_string())
        .spawn(move || run_worker(receiver))
        .map_err(|error| format!("Failed to start sleep-prevention worker: {}", error))?;
    Ok(sender)
}

fn run_worker(receiver: mpsc::Receiver<SleepPreventionRequest>) {
    let mut inhibitor = None;

    while let Ok(request) = receiver.recv() {
        match request {
            SleepPreventionRequest::SetEnabled { enabled, response } => {
                let _ = response.send(set_inhibitor_enabled(&mut inhibitor, enabled));
            }
        }
    }
}

fn set_inhibitor_enabled(
    inhibitor: &mut Option<keepawake::KeepAwake>,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        if inhibitor.is_none() {
            let guard = keepawake::Builder::default()
                .idle(true)
                .reason("Prevent sleep is enabled in OpenBitFun")
                .app_name("OpenBitFun")
                .app_reverse_domain("com.openbitfun.desktop")
                .create()
                .map_err(|error| {
                    let message = format!("Failed to prevent system sleep: {}", error);
                    #[cfg(target_os = "linux")]
                    {
                        format!(
                            "{}. Linux sleep prevention requires the system D-Bus and \
                             org.freedesktop.login1",
                            message
                        )
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        message
                    }
                })?;
            *inhibitor = Some(guard);
            tracing::info!("System sleep prevention enabled");
        }
    } else if inhibitor.take().is_some() {
        tracing::info!("System sleep prevention disabled");
    }

    Ok(())
}

async fn configured_enabled() -> Result<bool, String> {
    let service = openbitfun_core::service::config::get_global_config_service()
        .await
        .map_err(|error| format!("Failed to read prevent-sleep preference: {}", error))?;
    service
        .get_config::<bool>(Some(PREVENT_SLEEP_CONFIG_PATH))
        .await
        .map_err(|error| format!("Failed to read prevent-sleep preference: {}", error))
}

/// Applies the given runtime state, then persists it, undoing the runtime
/// change if persisting fails.
///
/// Order matters: applying first means a rejected inhibitor never reaches
/// disk, and rolling back means a failed save never leaves the running shell
/// in a state the config does not describe. Written against injected closures
/// so the rollback branch can be tested without a failing filesystem.
async fn apply_then_persist<Apply, ApplyFut, Persist, PersistFut>(
    previous: bool,
    enabled: bool,
    apply: Apply,
    persist: Persist,
) -> Result<(), String>
where
    Apply: Fn(bool) -> ApplyFut,
    ApplyFut: std::future::Future<Output = Result<(), String>>,
    Persist: FnOnce() -> PersistFut,
    PersistFut: std::future::Future<Output = Result<(), String>>,
{
    apply(enabled).await?;

    let Err(error) = persist().await else {
        return Ok(());
    };
    if let Err(rollback_error) = apply(previous).await {
        return Err(format!(
            "Failed to save prevent-sleep preference: {}; runtime rollback also failed: {}",
            error, rollback_error
        ));
    }
    Err(format!(
        "Failed to save prevent-sleep preference: {}",
        error
    ))
}

/// The settings toggle: apply the runtime state, persist the shared config
/// key, and roll both back when the save fails.
pub(crate) async fn set_prevent_sleep(enabled: bool) -> Result<(), String> {
    let previous = configured_enabled().await?;
    apply_then_persist(
        previous,
        enabled,
        |enabled| global().set_enabled(enabled),
        || async {
            let service = openbitfun_core::service::config::get_global_config_service()
                .await
                .map_err(|error| error.to_string())?;
            service
                .set_config(PREVENT_SLEEP_CONFIG_PATH, enabled)
                .await
                .map_err(|error| error.to_string())
        },
    )
    .await
}

/// Events that can change `app.prevent_sleep`.
///
/// `ConfigReloaded` covers a config import or an external edit: the preference
/// can flip without any command running, and without re-reading it the runtime
/// state would silently disagree with the saved one until the next restart.
fn config_event_requires_sync(event: &ConfigUpdateEvent) -> bool {
    matches!(
        event,
        ConfigUpdateEvent::AppUpdated | ConfigUpdateEvent::ConfigReloaded
    )
}

async fn sync_from_config() {
    match configured_enabled().await {
        Ok(enabled) => {
            if let Err(error) = global().set_enabled(enabled).await {
                tracing::warn!(
                    "Failed to apply prevent-sleep preference: enabled={}, error={}",
                    enabled,
                    error
                );
            }
            crate::runtime::publish(crate::runtime::RuntimeEvent::PreventSleepState(enabled));
        }
        Err(error) => {
            tracing::warn!("{}", error);
        }
    }
}

/// Applies the saved preference at startup and after config imports/reloads.
/// Also publishes the current value to the UI so the settings switch starts
/// from the persisted truth instead of a default.
pub(crate) fn spawn_config_listener() {
    let Some(handle) = crate::runtime::runtime_handle() else {
        tracing::warn!("Runtime unavailable; sleep-prevention listener not started");
        return;
    };
    handle.tokio_handle().spawn(async move {
        let Some(mut receiver) = subscribe_config_updates() else {
            tracing::warn!("Config update subscription unavailable for sleep-prevention listener");
            sync_from_config().await;
            return;
        };

        sync_from_config().await;

        loop {
            match receiver.recv().await {
                Ok(event) if config_event_requires_sync(&event) => {
                    sync_from_config().await;
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::warn!("Sleep-prevention config listener channel closed");
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(
                        "Sleep-prevention config listener lagged by {} messages",
                        count
                    );
                    sync_from_config().await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        apply_then_persist, config_event_requires_sync, start_worker, SleepPreventionState,
    };
    use openbitfun_core::service::config::ConfigUpdateEvent;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn inactive_state_does_not_start_a_worker() {
        let state = SleepPreventionState::default();
        assert!(state.worker.lock().await.is_none());

        state.set_enabled(false).await.unwrap();
        state.set_enabled(false).await.unwrap();

        assert!(state.worker.lock().await.is_none());
    }

    #[tokio::test]
    async fn disabling_stops_an_existing_worker() {
        let state = SleepPreventionState::default();
        *state.worker.lock().await = Some(start_worker().unwrap());

        state.set_enabled(false).await.unwrap();

        assert!(state.worker.lock().await.is_none());
    }

    #[tokio::test]
    async fn a_failed_save_puts_the_runtime_state_back() {
        let applied = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&applied);

        let error = apply_then_persist(
            false,
            true,
            move |enabled| {
                let recorder = Arc::clone(&recorder);
                async move {
                    recorder.lock().unwrap().push(enabled);
                    Ok(())
                }
            },
            || async { Err("disk is full".to_string()) },
        )
        .await
        .expect_err("a failed save must surface as an error");

        assert!(error.contains("disk is full"), "unexpected error: {error}");
        assert!(
            !error.contains("rollback also failed"),
            "rollback succeeded, so it must not be reported as failed: {error}"
        );
        assert_eq!(
            *applied.lock().unwrap(),
            vec![true, false],
            "the new state must be applied, then reverted to the previous one"
        );
    }

    #[tokio::test]
    async fn a_failed_save_and_a_failed_rollback_report_both() {
        let error = apply_then_persist(
            false,
            true,
            |enabled| async move {
                if enabled {
                    Ok(())
                } else {
                    Err("inhibitor stuck".to_string())
                }
            },
            || async { Err("disk is full".to_string()) },
        )
        .await
        .expect_err("a failed save must surface as an error");

        assert!(error.contains("disk is full"), "unexpected error: {error}");
        assert!(
            error.contains("inhibitor stuck"),
            "a failed rollback must be reported too: {error}"
        );
    }

    #[tokio::test]
    async fn a_successful_save_does_not_roll_back() {
        let applied = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&applied);

        apply_then_persist(
            false,
            true,
            move |enabled| {
                let recorder = Arc::clone(&recorder);
                async move {
                    recorder.lock().unwrap().push(enabled);
                    Ok(())
                }
            },
            || async { Ok(()) },
        )
        .await
        .expect("saving succeeded");

        assert_eq!(*applied.lock().unwrap(), vec![true]);
    }

    #[test]
    fn config_reload_re_reads_the_preference() {
        assert!(config_event_requires_sync(
            &ConfigUpdateEvent::ConfigReloaded
        ));
        assert!(config_event_requires_sync(&ConfigUpdateEvent::AppUpdated));
        assert!(!config_event_requires_sync(
            &ConfigUpdateEvent::ModelConfigurationUpdated
        ));
    }
}
