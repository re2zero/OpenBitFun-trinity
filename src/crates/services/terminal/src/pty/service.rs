//! PTY Service - High-level PTY management
//!
//! This module provides the main service interface for managing PTY processes,
//! including creation, input/output handling, and lifecycle management.
//!
//! The service uses the new component-based design from process.rs:
//! - Each process has independent writer, controller, and flow control
//! - Event streams are moved to dedicated tasks (no locks during event wait)
//! - Write and control operations don't require locks

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use log::{info, warn};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::{ShellConfig, TerminalConfig};
use crate::shell::ShellType;
use crate::{TerminalError, TerminalResult};

use super::data_bufferer::DataBufferer;
use super::process::{spawn_pty, FlowControl, PtyController, PtyEvent, PtyInfo, PtyWriter};

/// Events emitted by the PTY service
#[derive(Debug, Clone)]
pub enum PtyServiceEvent {
    /// Process data event
    ProcessData { id: u32, data: Vec<u8> },
    /// Process ready event
    ProcessReady { id: u32, pid: u32, cwd: String },
    /// Process exit event
    ProcessExit { id: u32, exit_code: Option<u32> },
    /// Process property changed
    ProcessProperty { id: u32, property: ProcessProperty },
    /// Resize completed (for frontend synchronization)
    ResizeCompleted { id: u32, cols: u16, rows: u16 },
}

/// Process properties that can change
#[derive(Debug, Clone)]
pub enum ProcessProperty {
    Title(String),
    Cwd(String),
    ShellType(ShellType),
    HasChildProcesses(bool),
}

/// Information about a PTY process
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process ID (internal)
    pub id: u32,
    /// OS process ID
    pub pid: Option<u32>,
    /// Current working directory
    pub cwd: String,
    /// Shell type
    pub shell_type: ShellType,
    /// Whether the process is running
    pub is_running: bool,
}

/// Internal state for a managed PTY process
struct ManagedProcess {
    /// Static information
    info: PtyInfo,
    /// Writer for sending data
    writer: PtyWriter,
    /// Controller for resize, signal, shutdown
    controller: PtyController,
    /// Flow control state
    flow_control: FlowControl,
}

/// PTY Service - manages multiple PTY processes
pub struct PtyService {
    /// Service configuration
    #[allow(dead_code)]
    config: TerminalConfig,

    /// Active PTY processes (using the new component-based design)
    processes: Arc<RwLock<HashMap<u32, ManagedProcess>>>,

    /// Data bufferer for each process
    bufferer: Arc<DataBufferer>,

    /// Next process ID
    next_id: AtomicU32,

    /// Whether this service's buffered-data forwarding task has started.
    /// Per instance on purpose: the bufferer and event sender it drains are
    /// instance state, so a process-wide flag would starve every later service
    /// in the same process (reproduced 2026-10-09: a second `PtyService`
    /// received no `ProcessData` at all).
    buffer_forwarding_started: AtomicBool,

    /// Event sender
    event_tx: mpsc::Sender<PtyServiceEvent>,

    /// Event receiver
    event_rx: Arc<Mutex<mpsc::Receiver<PtyServiceEvent>>>,
}

impl PtyService {
    /// Create a new PTY service
    pub fn new(config: TerminalConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        let bufferer = Arc::new(DataBufferer::new(config.buffering.clone()));

        Self {
            config,
            processes: Arc::new(RwLock::new(HashMap::new())),
            bufferer,
            next_id: AtomicU32::new(1),
            buffer_forwarding_started: AtomicBool::new(false),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
        }
    }

    /// Create a new PTY process
    pub async fn create_process(
        &self,
        shell_config: ShellConfig,
        shell_type: ShellType,
        cols: u16,
        rows: u16,
    ) -> TerminalResult<u32> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Spawn the PTY process using the new component-based API
        let result = spawn_pty(id, &shell_config, shell_type, cols, rows)?;

        // Create managed process (without the event stream)
        let managed = ManagedProcess {
            info: result.info,
            writer: result.writer,
            controller: result.controller,
            flow_control: result.flow_control.clone(),
        };

        // Start buffering
        self.bufferer.start_buffering(id).await;

        // Store the managed process
        {
            let mut processes = self.processes.write().await;
            processes.insert(id, managed);
        }

        // Start event forwarding for this process
        // The event stream is MOVED here (no locks needed during event wait!)
        self.start_event_forwarding(id, result.events, result.flow_control)
            .await;

        // Start buffered data forwarding
        self.start_buffer_forwarding().await;

        Ok(id)
    }

    /// Start forwarding events from a PTY process
    ///
    /// Note: The event stream is MOVED into this function, so no locks are needed
    /// during event waiting. This is the key improvement over the old design.
    async fn start_event_forwarding(
        &self,
        id: u32,
        mut events: super::process::PtyEventStream,
        flow_control: FlowControl,
    ) {
        let event_tx = self.event_tx.clone();
        let bufferer = self.bufferer.clone();

        tokio::spawn(async move {
            // Event loop - NO LOCKS needed here!
            while let Some(event) = events.recv().await {
                match event {
                    PtyEvent::Data(data) => {
                        // Track for flow control (no lock, uses atomic operations)
                        flow_control.track_output(data.len());

                        // Buffer the data
                        bufferer.buffer_data(id, &data).await;
                    }
                    PtyEvent::Ready { pid, cwd } => {
                        let _ = event_tx
                            .send(PtyServiceEvent::ProcessReady { id, pid, cwd })
                            .await;
                    }
                    PtyEvent::Exit { exit_code } => {
                        info!("PTY process {} exited with code {:?}", id, exit_code);
                        // Deliver the buffered tail on this task, before the
                        // exit event, so consumers see every byte the process
                        // wrote ahead of its exit (a plain flush would race,
                        // because the forwarding task is a separate hop).
                        let tail = bufferer.take_pending(id).await;
                        if !tail.is_empty() {
                            flow_control.track_output(tail.len());
                            let _ = event_tx
                                .send(PtyServiceEvent::ProcessData { id, data: tail })
                                .await;
                        }
                        bufferer.stop_buffering(id).await;
                        let _ = event_tx
                            .send(PtyServiceEvent::ProcessExit { id, exit_code })
                            .await;
                        break;
                    }
                    PtyEvent::TitleChanged(title) => {
                        let _ = event_tx
                            .send(PtyServiceEvent::ProcessProperty {
                                id,
                                property: ProcessProperty::Title(title),
                            })
                            .await;
                    }
                    PtyEvent::CwdChanged(cwd) => {
                        let _ = event_tx
                            .send(PtyServiceEvent::ProcessProperty {
                                id,
                                property: ProcessProperty::Cwd(cwd),
                            })
                            .await;
                    }
                    PtyEvent::ResizeCompleted { cols, rows } => {
                        let _ = event_tx
                            .send(PtyServiceEvent::ResizeCompleted { id, cols, rows })
                            .await;
                    }
                }
            }

            // Clean up buffering
            bufferer.stop_buffering(id).await;
        });
    }

    /// Start forwarding buffered data as events
    async fn start_buffer_forwarding(&self) {
        // Only start once per service (see the field comment: a process-wide
        // flag would disconnect every service created after the first).
        if self.buffer_forwarding_started.swap(true, Ordering::Relaxed) {
            return;
        }

        let bufferer = self.bufferer.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                if let Some(buffered) = bufferer.recv().await {
                    let _ = event_tx
                        .send(PtyServiceEvent::ProcessData {
                            id: buffered.process_id,
                            data: buffered.data,
                        })
                        .await;
                }
            }
        });
    }

    /// Write data to a PTY process
    ///
    /// Note: The writer uses a channel internally, so the process map lock is
    /// only held long enough to clone the writer handle.
    pub async fn write(&self, id: u32, data: &[u8]) -> TerminalResult<()> {
        let writer = {
            let processes = self.processes.read().await;
            processes
                .get(&id)
                .map(|process| process.writer.clone())
                .ok_or_else(|| TerminalError::SessionNotFound(id.to_string()))?
        };

        writer.write(data).await
    }

    /// Resize a PTY process
    ///
    /// Note: This no longer requires a lock on the process!
    pub async fn resize(&self, id: u32, cols: u16, rows: u16) -> TerminalResult<()> {
        // Flush buffer on resize
        self.bufferer.flush_buffer(id).await;

        let controller = {
            let processes = self.processes.read().await;
            processes
                .get(&id)
                .map(|process| process.controller.clone())
                .ok_or_else(|| TerminalError::SessionNotFound(id.to_string()))?
        };

        controller.resize(cols, rows).await
    }

    /// Send a signal to a PTY process
    pub async fn signal(&self, id: u32, signal: &str) -> TerminalResult<()> {
        let controller = {
            let processes = self.processes.read().await;
            processes
                .get(&id)
                .map(|process| process.controller.clone())
                .ok_or_else(|| TerminalError::SessionNotFound(id.to_string()))?
        };

        controller.signal(signal).await
    }

    /// Shutdown a PTY process
    pub async fn shutdown(&self, id: u32, immediate: bool) -> TerminalResult<()> {
        let controller = {
            let processes = self.processes.read().await;
            processes.get(&id).map(|process| process.controller.clone())
        };

        if let Some(controller) = controller {
            if !controller.is_running() {
                self.processes.write().await.remove(&id);
                return Ok(());
            }
            match controller.shutdown(immediate).await {
                Ok(()) => {
                    self.processes.write().await.remove(&id);
                }
                Err(TerminalError::ProcessNotRunning) if !controller.is_running() => {
                    self.processes.write().await.remove(&id);
                }
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }

    /// Shutdown all PTY processes
    pub async fn shutdown_all(&self) {
        let ids: Vec<u32> = {
            let processes = self.processes.read().await;
            processes.keys().cloned().collect()
        };

        for id in ids {
            if let Err(e) = self.shutdown(id, true).await {
                warn!("Failed to shutdown process {}: {}", id, e);
            }
        }
    }

    /// Acknowledge data received by frontend (for flow control)
    pub async fn acknowledge_data(&self, id: u32, char_count: usize) -> TerminalResult<()> {
        let flow_control = {
            let processes = self.processes.read().await;
            processes
                .get(&id)
                .map(|process| process.flow_control.clone())
                .ok_or_else(|| TerminalError::SessionNotFound(id.to_string()))?
        };

        flow_control.acknowledge_data(char_count);
        Ok(())
    }

    /// Get the next service event
    pub async fn recv_event(&self) -> Option<PtyServiceEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    /// Try to receive an event without blocking
    pub async fn try_recv_event(&self) -> Option<PtyServiceEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.try_recv().ok()
    }

    /// List all active process IDs
    pub async fn list_processes(&self) -> Vec<u32> {
        let processes = self.processes.read().await;
        processes.keys().cloned().collect()
    }

    /// Check if a process exists
    pub async fn has_process(&self, id: u32) -> bool {
        let processes = self.processes.read().await;
        processes.contains_key(&id)
    }

    /// Get process info
    pub async fn get_process_info(&self, id: u32) -> Option<ProcessInfo> {
        let processes = self.processes.read().await;
        let process = processes.get(&id)?;

        Some(ProcessInfo {
            id,
            pid: Some(process.info.pid),
            cwd: process.info.initial_cwd.clone(),
            shell_type: process.info.shell_type.clone(),
            is_running: process.controller.is_running(),
        })
    }
}

impl Drop for PtyService {
    fn drop(&mut self) {
        // Note: Processes should be shut down explicitly before dropping
    }
}

#[cfg(test)]
mod tests {
    use super::PtyService;
    use crate::config::{ShellConfig, TerminalConfig};
    use crate::pty::service::PtyServiceEvent;
    use crate::shell::ShellType;

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_evicts_a_process_whose_controller_already_confirmed_exit() {
        let service = PtyService::new(TerminalConfig::default());
        let (executable, shell_type) = if cfg!(windows) {
            ("cmd.exe", ShellType::Cmd)
        } else {
            ("/bin/sh", ShellType::Sh)
        };
        let process_id = service
            .create_process(
                ShellConfig {
                    executable: executable.to_string(),
                    ..ShellConfig::default()
                },
                shell_type,
                80,
                24,
            )
            .await
            .expect("create test PTY");
        let controller = service
            .processes
            .read()
            .await
            .get(&process_id)
            .expect("managed process")
            .controller
            .clone();

        controller
            .shutdown(true)
            .await
            .expect("confirm child exit before service eviction");
        assert!(service.has_process(process_id).await);

        service
            .shutdown(process_id, true)
            .await
            .expect("stale process eviction must be idempotent");
        assert!(!service.has_process(process_id).await);
    }

    /// Platform shell plus the arguments that run a one-line script.
    fn scripted_shell(script: &str) -> (String, Vec<String>, ShellType) {
        if cfg!(windows) {
            (
                "cmd.exe".to_string(),
                vec!["/C".to_string(), script.to_string()],
                ShellType::Cmd,
            )
        } else {
            (
                "/bin/sh".to_string(),
                vec!["-c".to_string(), script.to_string()],
                ShellType::Sh,
            )
        }
    }

    /// Regression: nothing observes the child, so a process that exits on its
    /// own (no shutdown request) used to never report `ProcessExit` at all.
    #[tokio::test(flavor = "current_thread")]
    async fn a_process_that_exits_on_its_own_reports_its_exit() {
        let service = PtyService::new(TerminalConfig::default());
        let (executable, args, shell_type) = scripted_shell("exit 7");
        let process_id = service
            .create_process(
                ShellConfig {
                    executable,
                    args,
                    ..ShellConfig::default()
                },
                shell_type,
                80,
                24,
            )
            .await
            .expect("create test PTY");

        let mut exit_code = None;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, service.recv_event()).await {
            match event {
                PtyServiceEvent::ProcessExit {
                    id,
                    exit_code: code,
                } if id == process_id => {
                    exit_code = code;
                    break;
                }
                PtyServiceEvent::ProcessData { id, data } => {
                    let _ = service.acknowledge_data(id, data.len()).await;
                }
                _ => {}
            }
        }
        assert_eq!(
            exit_code,
            Some(7),
            "a self-exiting process must report its exit code"
        );
    }

    /// Regression: the buffer-forwarding task was gated by a process-wide
    /// flag, so every `PtyService` created after the first one in the same
    /// process silently received no `ProcessData` at all.
    #[tokio::test(flavor = "current_thread")]
    async fn a_second_service_in_the_same_process_still_forwards_data() {
        for round in 1..=2 {
            let service = PtyService::new(TerminalConfig::default());
            let (executable, args, shell_type) = scripted_shell("echo round");
            let process_id = service
                .create_process(
                    ShellConfig {
                        executable,
                        args,
                        ..ShellConfig::default()
                    },
                    shell_type,
                    80,
                    24,
                )
                .await
                .expect("create test PTY");

            let mut collected = Vec::new();
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);
            while let Ok(Some(event)) =
                tokio::time::timeout_at(deadline, service.recv_event()).await
            {
                match event {
                    PtyServiceEvent::ProcessData { id, data } if id == process_id => {
                        collected.extend_from_slice(&data);
                        let _ = service.acknowledge_data(id, data.len()).await;
                        if String::from_utf8_lossy(&collected).contains("round") {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            assert!(
                String::from_utf8_lossy(&collected).contains("round"),
                "service instance {round} must forward process data, got {:?}",
                String::from_utf8_lossy(&collected)
            );
        }
    }
}
