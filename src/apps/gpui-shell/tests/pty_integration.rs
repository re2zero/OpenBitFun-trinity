//! Real-PTY integration tests for the terminal pane's process contract.
//!
//! These drive `terminal-core`'s `PtyService` exactly the way
//! `terminal::pane` does (spawn, read, acknowledge, resize, shutdown) so the
//! service round trip is covered without a window: sustained output with
//! acknowledgements, window-size changes reaching the child, and lifecycle
//! cleanup. The emulator runs alongside to prove the grid survives the load.

#![cfg(unix)]

use std::collections::HashMap;
use std::time::Duration;

use openbitfun_gpui_shell::terminal::emulator::TerminalEmulator;
use terminal_core::{PtyService, PtyServiceEvent, ShellConfig, ShellType, TerminalConfig};

/// Generous ceiling: the assertions are about delivery, not throughput.
const DEADLINE: Duration = Duration::from_secs(60);

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .expect("build test runtime")
}

fn service() -> PtyService {
    PtyService::new(TerminalConfig::default())
}

fn shell(script: &str) -> ShellConfig {
    ShellConfig {
        executable: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        env: HashMap::new(),
        cwd: None,
        login: false,
    }
}

fn row_text(emulator: &TerminalEmulator, row: usize) -> String {
    let text: String = emulator
        .visible_line(row)
        .iter()
        .map(|cell| cell.c)
        .collect();
    text.trim_end().to_string()
}

/// Last grid row that has any content.
fn last_content_row(emulator: &TerminalEmulator) -> String {
    (0..emulator.rows())
        .rev()
        .map(|row| row_text(emulator, row))
        .find(|text| !text.is_empty())
        .unwrap_or_default()
}

fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Direct children of `pid`, read from `/proc/<p>/stat` (field 4).
fn children_of(pid: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut children = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(candidate) = name.to_str().and_then(|name| name.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{candidate}/stat")) else {
            continue;
        };
        // The comm field can contain spaces and parentheses: parse after the
        // closing one.
        let Some(rest) = stat.rsplit_once(") ") else {
            continue;
        };
        let mut fields = rest.1.split_whitespace();
        let _state = fields.next();
        let ppid = fields.next().and_then(|value| value.parse::<u32>().ok());
        if ppid == Some(pid) {
            children.push(candidate);
        }
    }
    children
}

/// Consumes service events into `emulator` (acknowledging every chunk, as the
/// pane does) until `marker` shows up in the collected output.
async fn read_until(
    service: &PtyService,
    id: u32,
    emulator: &mut TerminalEmulator,
    collected: &mut Vec<u8>,
    marker: &str,
) {
    let deadline = tokio::time::Instant::now() + DEADLINE;
    loop {
        let event = tokio::time::timeout_at(deadline, service.recv_event())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {marker:?}"))
            .expect("service event stream ended early");
        match event {
            PtyServiceEvent::ProcessData { data, .. } => {
                collected.extend_from_slice(&data);
                emulator.advance(&data);
                service
                    .acknowledge_data(id, data.len())
                    .await
                    .expect("acknowledge pty data");
                let text = String::from_utf8_lossy(collected);
                if text.contains(marker) {
                    return;
                }
            }
            PtyServiceEvent::ProcessExit { .. } => {
                panic!(
                    "process exited before {marker:?} arrived; output so far: {:?}",
                    String::from_utf8_lossy(collected)
                );
            }
            _ => {}
        }
    }
}

/// Acceptance criterion 6: `seq 1 200000` streams to completion with the
/// pane's ack loop, no chunk stalling the consumer.
#[test]
fn large_output_streams_through_with_acknowledgements() {
    let runtime = runtime();
    runtime.block_on(async {
        let service = service();
        let id = service
            .create_process(shell("seq 1 200000"), ShellType::Bash, 80, 24)
            .await
            .expect("spawn seq");

        let mut emulator = TerminalEmulator::new(80, 24);
        let mut bytes = 0usize;
        let mut chunks = 0usize;
        let mut slowest = Duration::ZERO;
        let deadline = tokio::time::Instant::now() + DEADLINE;
        loop {
            let event = tokio::time::timeout_at(deadline, service.recv_event())
                .await
                .expect("`seq 1 200000` must finish inside the deadline")
                .expect("service event stream ended early");
            match event {
                PtyServiceEvent::ProcessData { data, .. } => {
                    bytes += data.len();
                    chunks += 1;
                    let started = std::time::Instant::now();
                    emulator.advance(&data);
                    slowest = slowest.max(started.elapsed());
                    service
                        .acknowledge_data(id, data.len())
                        .await
                        .expect("acknowledge pty data");
                }
                PtyServiceEvent::ProcessExit { exit_code, .. } => {
                    assert_eq!(exit_code, Some(0), "seq must exit cleanly");
                    break;
                }
                _ => {}
            }
        }

        // 1,288,895 bytes of digits and newlines, plus CRLF expansion: the
        // stream must arrive whole, not truncated at the 100k high-water mark.
        assert!(
            bytes >= 1_288_895,
            "expected the full stream, got {bytes} bytes in {chunks} chunks"
        );
        println!(
            "{bytes} bytes in {chunks} chunks, slowest chunk applied in {:?}",
            slowest
        );
        // The grid stays coherent: the tail of the stream is on screen.
        assert_eq!(last_content_row(&emulator), "200000");
        assert!(
            slowest < Duration::from_millis(100),
            "a single chunk must not stall the UI thread for {:?}",
            slowest
        );
    });
}

/// Acceptance criterion 3: a PTY resize reaches the running process
/// (SIGWINCH + new winsize), which is what makes full-screen apps reflow.
#[test]
fn resize_reaches_the_running_process() {
    let runtime = runtime();
    runtime.block_on(async {
        let service = service();
        let id = service
            .create_process(
                shell("trap 'stty size' WINCH; printf 'ready\\n'; while :; do sleep 0.05; done"),
                ShellType::Bash,
                80,
                24,
            )
            .await
            .expect("spawn reporter");
        let mut emulator = TerminalEmulator::new(80, 24);
        let mut collected = Vec::new();
        read_until(&service, id, &mut emulator, &mut collected, "ready").await;

        service.resize(id, 100, 30).await.expect("resize pty");
        // The trap prints "rows cols" on the next SIGWINCH.
        read_until(&service, id, &mut emulator, &mut collected, "30 100").await;

        service.shutdown(id, true).await.expect("shutdown");
        assert!(!service.has_process(id).await);
    });
}

/// Acceptance criterion 1: closing the pane reaps the child *and* the
/// processes it started; nothing is left orphaned.
#[test]
fn shutdown_reaps_the_child_and_its_children() {
    let runtime = runtime();
    runtime.block_on(async {
        let service = service();
        let id = service
            .create_process(shell("sleep 300 & wait"), ShellType::Bash, 80, 24)
            .await
            .expect("spawn sleeper");

        let mut emulator = TerminalEmulator::new(80, 24);
        let mut collected = Vec::new();
        let pid = wait_for_ready(&service, id, &mut emulator, &mut collected).await;
        assert!(process_alive(pid), "pty child {pid} must be running");
        let grandchildren = wait_for_children(pid).await;
        assert!(
            !grandchildren.is_empty(),
            "fixture must have a background child to orphan"
        );

        // The pane's close path: graceful shutdown.
        service
            .shutdown(id, false)
            .await
            .expect("shutdown must confirm termination");
        assert!(
            !service.has_process(id).await,
            "service must forget the pty"
        );
        assert!(!process_alive(pid), "pty child {pid} must be reaped");
        for grandchild in grandchildren {
            assert!(
                !process_alive(grandchild),
                "grandchild {grandchild} must not be orphaned"
            );
        }
    });
}

/// Acceptance criterion 1: a process that exits on its own reports the exit
/// and stops the event stream (the pane turns this into the "exited" label).
#[test]
fn normal_exit_reports_code_and_ends_the_stream() {
    let runtime = runtime();
    runtime.block_on(async {
        let service = service();
        let id = service
            .create_process(shell("printf 'bye\\n'; exit 7"), ShellType::Bash, 80, 24)
            .await
            .expect("spawn exiting shell");

        let mut emulator = TerminalEmulator::new(80, 24);
        let mut exit_code = None;
        let deadline = tokio::time::Instant::now() + DEADLINE;
        while let Ok(Some(event)) = tokio::time::timeout_at(deadline, service.recv_event()).await {
            match event {
                PtyServiceEvent::ProcessData { data, .. } => {
                    emulator.advance(&data);
                    service.acknowledge_data(id, data.len()).await.ok();
                }
                PtyServiceEvent::ProcessExit {
                    exit_code: code, ..
                } => {
                    exit_code = code;
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(exit_code, Some(7), "exit code must be carried through");
    });
}

/// Waits for `ProcessReady` (which carries the pid) and returns it.
async fn wait_for_ready(
    service: &PtyService,
    id: u32,
    emulator: &mut TerminalEmulator,
    collected: &mut Vec<u8>,
) -> u32 {
    let deadline = tokio::time::Instant::now() + DEADLINE;
    loop {
        let event = tokio::time::timeout_at(deadline, service.recv_event())
            .await
            .expect("timed out waiting for the pty to become ready")
            .expect("service event stream ended early");
        match event {
            PtyServiceEvent::ProcessReady {
                id: ready_id, pid, ..
            } if ready_id == id => {
                return pid;
            }
            PtyServiceEvent::ProcessData { data, .. } => {
                collected.extend_from_slice(&data);
                emulator.advance(&data);
                service.acknowledge_data(id, data.len()).await.ok();
            }
            PtyServiceEvent::ProcessExit { .. } => panic!("process exited before ready"),
            _ => {}
        }
    }
}

/// Polls briefly for the fixture's background child to show up.
async fn wait_for_children(pid: u32) -> Vec<u32> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let children = children_of(pid);
        if !children.is_empty() {
            return children;
        }
        if tokio::time::Instant::now() >= deadline {
            return children;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Acceptance criterion 1: the panel can be closed and opened again — a fresh
/// pane (and therefore a fresh `PtyService`) still streams output. Regression
/// for the process-wide forwarding flag that used to starve every later
/// service in the same process.
#[test]
fn reopened_pane_streams_output_again() {
    let runtime = runtime();
    runtime.block_on(async {
        for round in 1..=2 {
            let service = service();
            let id = service
                .create_process(shell("printf 'round\\n'"), ShellType::Bash, 80, 24)
                .await
                .expect("spawn");
            let mut emulator = TerminalEmulator::new(80, 24);
            let mut collected = Vec::new();
            read_until(&service, id, &mut emulator, &mut collected, "round").await;
            // Closing the pane tears the pty down before the next round.
            service
                .shutdown(id, false)
                .await
                .unwrap_or_else(|error| panic!("round {round} shutdown failed: {error}"));
            assert!(!service.has_process(id).await);
        }
    });
}
