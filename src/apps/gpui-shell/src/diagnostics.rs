//! Shell-side diagnostics: per-session file logs, run state, panic reports,
//! and the support bundle export. Mirrors the desktop app's
//! `crash_diagnostics` bundle layout so a shell bundle looks familiar to the
//! same support tooling, with `gpui-`-prefixed state files so the two
//! frontends sharing one logs root do not stomp each other's run state.

use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const RUN_STATE_FILE: &str = "gpui-run-state.json";
const CRASH_REPORT_FILE: &str = "gpui-crash-report.json";
const DIAGNOSTIC_METADATA_FILE: &str = "diagnostic-metadata.json";
pub(crate) const SHELL_LOG_FILE: &str = "shell.log";
const ARCHIVE_RUN_STATE_FILE: &str = "run-state.json";
const MAX_BUNDLED_LOG_SESSIONS: usize = 5;

static CURRENT_RUN_CONTEXT: OnceLock<RunContext> = OnceLock::new();
static PREVIOUS_UNEXPECTED_EXIT: OnceLock<Option<UnexpectedExitInfo>> = OnceLock::new();
static EFFECTIVE_LEVEL: Mutex<String> = Mutex::new(String::new());

#[derive(Debug, Clone)]
struct RunContext {
    logs_root: PathBuf,
    session_log_dir: PathBuf,
    run_state_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnexpectedExitInfo {
    pub detected: bool,
    pub started_at: Option<String>,
    pub session_log_dir: Option<String>,
    pub crash_report_path: Option<String>,
    pub category: UnexpectedExitCategory,
    pub notify_on_startup: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnexpectedExitCategory {
    Crash,
    UncleanShutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunState {
    app_version: String,
    pid: u32,
    started_at: String,
    updated_at: String,
    session_log_dir: String,
    clean_shutdown: bool,
    shutdown_at: Option<String>,
    exit_reason: Option<String>,
    startup_trace_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsBundleInfo {
    pub bundle_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellLoggingInfo {
    effective_level: String,
    session_log_dir: String,
    shell_log_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticMetadata {
    exported_at: String,
    app_version: String,
    os: String,
    arch: String,
    debug_assertions: bool,
    current_pid: u32,
    current_log_info: ShellLoggingInfo,
    previous_unexpected_exit: Option<UnexpectedExitInfo>,
    platform_crash_report_hints: Vec<String>,
}

/// Resolves the shared logs root: explicit overrides first, then the core
/// path manager (`~/.config/openbitfun/logs/`). Mirrors the desktop
/// resolution order minus its webdriver mode.
pub fn resolve_logs_root() -> PathBuf {
    if let Some(path) = std::env::var_os("OPENBITFUN_LOG_DIR").map(PathBuf::from) {
        return path;
    }
    if let Some(path) = std::env::var_os("OPENBITFUN_E2E_LOG_DIR").map(PathBuf::from) {
        return path;
    }
    openbitfun_core::infrastructure::get_path_manager_arc().logs_dir()
}

/// Creates (or reuses) this run's session directory under the logs root,
/// named like the desktop sessions so bundle discovery treats them alike.
pub fn create_session_log_dir() -> PathBuf {
    let logs_root = resolve_logs_root();
    let timestamp = Local::now().format("%Y%m%dT%H%M%S").to_string();
    let session_dir = logs_root.join(&timestamp);

    if let Err(error) = fs::create_dir_all(&session_dir) {
        eprintln!("Warning: Failed to create log session directory: {error}");
        return logs_root;
    }
    session_dir
}

/// Path of this run's file log, when file logging is active.
pub fn session_log_path() -> Option<PathBuf> {
    CURRENT_RUN_CONTEXT
        .get()
        .map(|context| context.session_log_dir.join(SHELL_LOG_FILE))
}

/// Records this run's context and detects an unclean previous shutdown.
/// Called once from `init_tracing`, before any window exists.
pub fn initialize_run_state(session_log_dir: PathBuf, startup_trace_id: &str) {
    let logs_root = session_log_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| session_log_dir.clone());
    let run_state_path = logs_root.join(RUN_STATE_FILE);
    let previous = detect_previous_unexpected_exit(&run_state_path);
    let _ = PREVIOUS_UNEXPECTED_EXIT.set(previous);

    let context = RunContext {
        logs_root,
        session_log_dir: session_log_dir.clone(),
        run_state_path,
    };

    let state = RunState {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        pid: std::process::id(),
        started_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
        session_log_dir: session_log_dir.to_string_lossy().to_string(),
        clean_shutdown: false,
        shutdown_at: None,
        exit_reason: None,
        startup_trace_id: startup_trace_id.to_string(),
    };

    if let Err(error) = write_json_file(&context.run_state_path, &state) {
        eprintln!("Warning: Failed to write run state: {error}");
    }

    let _ = CURRENT_RUN_CONTEXT.set(context);
}

pub fn previous_unexpected_exit() -> Option<UnexpectedExitInfo> {
    PREVIOUS_UNEXPECTED_EXIT.get().cloned().flatten()
}

/// Tracks the live log filter for the bundle metadata; kept in sync by
/// `shell::init_tracing` and `shell::apply_log_level`.
pub(crate) fn set_effective_level(level: &str) {
    if let Ok(mut current) = EFFECTIVE_LEVEL.lock() {
        *current = level.to_string();
    }
}

fn effective_level() -> String {
    EFFECTIVE_LEVEL
        .lock()
        .map(|level| level.clone())
        .unwrap_or_else(|_| "info".to_string())
}

pub fn mark_clean_shutdown(reason: &str) {
    let Some(context) = CURRENT_RUN_CONTEXT.get() else {
        return;
    };

    let now = Utc::now().to_rfc3339();
    let mut state =
        read_json_file::<RunState>(&context.run_state_path).unwrap_or_else(|_| RunState {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            started_at: now.clone(),
            updated_at: now.clone(),
            session_log_dir: context.session_log_dir.to_string_lossy().to_string(),
            clean_shutdown: false,
            shutdown_at: None,
            exit_reason: None,
            startup_trace_id: String::new(),
        });
    state.updated_at = now.clone();
    state.clean_shutdown = true;
    state.shutdown_at = Some(now);
    state.exit_reason = Some(reason.to_string());

    if let Err(error) = write_json_file(&context.run_state_path, &state) {
        tracing::warn!("Failed to mark clean shutdown: {error}");
    }
}

/// Installs a panic hook that records a crash report into the session log
/// directory, then defers to whatever hook was installed before (gpui or the
/// default runtime hook).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| location.to_string())
            .unwrap_or_default();
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string());
        let thread = std::thread::current();
        write_panic_report(
            location,
            message,
            thread.name().map(str::to_string),
            format!("{:?}", thread.id()),
        );
        previous(info);
    }));
}

pub fn write_panic_report(
    location: String,
    message: String,
    thread_name: Option<String>,
    thread_id: String,
) {
    let Some(context) = CURRENT_RUN_CONTEXT.get() else {
        return;
    };

    let report = CrashReport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        pid: std::process::id(),
        crashed_at: Utc::now().to_rfc3339(),
        session_log_dir: context.session_log_dir.to_string_lossy().to_string(),
        thread_name,
        thread_id,
        location,
        message,
        backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        debug_assertions: cfg!(debug_assertions),
    };

    let crash_report_path = context.session_log_dir.join(CRASH_REPORT_FILE);
    if let Err(error) = write_json_file(&crash_report_path, &report) {
        eprintln!("Warning: Failed to write panic report: {error}");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CrashReport {
    app_version: String,
    pid: u32,
    crashed_at: String,
    session_log_dir: String,
    thread_name: Option<String>,
    thread_id: String,
    location: String,
    message: String,
    backtrace: String,
    os: String,
    arch: String,
    debug_assertions: bool,
}

/// Exports the support bundle next to the desktop app's
/// (`<logs-root>/diagnostics/openbitfun-gpui-diagnostics-<timestamp>.zip`),
/// holding the diagnostic metadata, the shell run state, and the five most
/// recent log session directories.
pub fn export_diagnostics_bundle() -> Result<DiagnosticsBundleInfo, String> {
    let context = CURRENT_RUN_CONTEXT
        .get()
        .cloned()
        .ok_or_else(|| "Shell diagnostics are not initialized".to_string())?;
    export_bundle_in(&context)
}

fn export_bundle_in(context: &RunContext) -> Result<DiagnosticsBundleInfo, String> {
    let diagnostics_dir = context.logs_root.join("diagnostics");
    fs::create_dir_all(&diagnostics_dir)
        .map_err(|error| format!("Failed to create diagnostics directory: {error}"))?;

    let filename = format!(
        "openbitfun-gpui-diagnostics-{}.zip",
        Local::now().format("%Y%m%dT%H%M%S")
    );
    let bundle_path = diagnostics_dir.join(filename);
    let file = File::create(&bundle_path)
        .map_err(|error| format!("Failed to create diagnostics bundle: {error}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let metadata = DiagnosticMetadata {
        exported_at: Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        debug_assertions: cfg!(debug_assertions),
        current_pid: std::process::id(),
        current_log_info: ShellLoggingInfo {
            effective_level: effective_level(),
            session_log_dir: context.session_log_dir.to_string_lossy().to_string(),
            shell_log_path: context
                .session_log_dir
                .join(SHELL_LOG_FILE)
                .to_string_lossy()
                .to_string(),
        },
        previous_unexpected_exit: previous_unexpected_exit(),
        platform_crash_report_hints: platform_crash_report_hints(),
    };
    add_json_entry(&mut zip, DIAGNOSTIC_METADATA_FILE, &metadata, options)?;

    if context.run_state_path.exists() {
        add_file_entry(
            &mut zip,
            &context.run_state_path,
            ARCHIVE_RUN_STATE_FILE,
            options,
        )?;
    }

    for session_dir in recent_session_dirs(&context.logs_root, MAX_BUNDLED_LOG_SESSIONS)? {
        let name = session_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown-session");
        add_directory_entries(&mut zip, &session_dir, &format!("sessions/{name}"), options)?;
    }

    zip.finish()
        .map_err(|error| format!("Failed to finish diagnostics bundle: {error}"))?;

    Ok(DiagnosticsBundleInfo {
        bundle_path: bundle_path.to_string_lossy().to_string(),
    })
}

fn detect_previous_unexpected_exit(run_state_path: &Path) -> Option<UnexpectedExitInfo> {
    let state = read_json_file::<RunState>(run_state_path).ok()?;
    if state.clean_shutdown {
        return None;
    }

    let session_log_dir = if state.session_log_dir.is_empty() {
        None
    } else {
        Some(state.session_log_dir.clone())
    };
    let crash_report_path = session_log_dir
        .as_ref()
        .map(PathBuf::from)
        .map(|path| path.join(CRASH_REPORT_FILE))
        .filter(|path| path.exists())
        .map(|path| path.to_string_lossy().to_string());

    let category = if crash_report_path.is_some() {
        UnexpectedExitCategory::Crash
    } else {
        UnexpectedExitCategory::UncleanShutdown
    };
    let notify_on_startup = matches!(category, UnexpectedExitCategory::Crash);
    let reason = match category {
        UnexpectedExitCategory::Crash => {
            "Previous run wrote a crash report before shutdown".to_string()
        }
        UnexpectedExitCategory::UncleanShutdown => {
            "Previous run state was not marked as clean shutdown".to_string()
        }
    };

    Some(UnexpectedExitInfo {
        detected: true,
        started_at: Some(state.started_at),
        session_log_dir,
        crash_report_path,
        category,
        notify_on_startup,
        reason,
    })
}

/// Session directories follow the desktop naming (`YYYYMMDDTHHMMSS`); matched
/// by hand so the shell does not pull in a regex engine for one pattern.
fn is_session_dir_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 15
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 8 {
                *byte == b'T'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn recent_session_dirs(logs_root: &Path, limit: usize) -> Result<Vec<PathBuf>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(logs_root)
        .map_err(|error| format!("Failed to read logs directory: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Failed to read logs entry: {error}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if is_session_dir_name(name) {
            entries.push(path);
        }
    }

    entries.sort();
    entries.reverse();
    entries.truncate(limit);
    entries.reverse();
    Ok(entries)
}

fn add_directory_entries(
    zip: &mut zip::ZipWriter<File>,
    dir: &Path,
    archive_prefix: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    for entry in fs::read_dir(dir)
        .map_err(|error| format!("Failed to read directory {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| format!("Failed to read directory entry: {error}"))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let archive_path = format!("{archive_prefix}/{name}");

        if path.is_dir() {
            add_directory_entries(zip, &path, &archive_path, options)?;
        } else if path.is_file() {
            add_file_entry(zip, &path, &archive_path, options)?;
        }
    }

    Ok(())
}

fn add_json_entry<T: Serialize>(
    zip: &mut zip::ZipWriter<File>,
    archive_path: &str,
    value: &T,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let content = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize {archive_path}: {error}"))?;
    zip.start_file(normalize_archive_path(archive_path), options)
        .map_err(|error| format!("Failed to add {archive_path} to diagnostics bundle: {error}"))?;
    zip.write_all(&content)
        .map_err(|error| format!("Failed to write {archive_path} to diagnostics bundle: {error}"))
}

fn add_file_entry(
    zip: &mut zip::ZipWriter<File>,
    source_path: &Path,
    archive_path: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let mut file = File::open(source_path)
        .map_err(|error| format!("Failed to open {}: {error}", source_path.display()))?;
    zip.start_file(normalize_archive_path(archive_path), options)
        .map_err(|error| format!("Failed to add {archive_path} to diagnostics bundle: {error}"))?;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read {}: {error}", source_path.display()))?;
        if read == 0 {
            break;
        }
        zip.write_all(&buffer[..read]).map_err(|error| {
            format!("Failed to write {archive_path} to diagnostics bundle: {error}")
        })?;
    }
    Ok(())
}

fn normalize_archive_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
    }
    let content = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to serialize {}: {error}", path.display()))?;
    fs::write(path, content).map_err(|error| format!("Failed to write {}: {error}", path.display()))
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let content =
        fs::read(path).map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
    serde_json::from_slice(&content)
        .map_err(|error| format!("Failed to parse {}: {error}", path.display()))
}

fn platform_crash_report_hints() -> Vec<String> {
    match std::env::consts::OS {
        "macos" => vec![
            "~/Library/Logs/DiagnosticReports".to_string(),
            "Console.app Diagnostic Reports".to_string(),
        ],
        "windows" => vec![
            "Windows Error Reporting".to_string(),
            "Event Viewer > Windows Logs > Application".to_string(),
        ],
        "linux" => vec!["coredumpctl".to_string(), "systemd-coredump".to_string()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "openbitfun-gpui-diagnostics-{tag}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        ))
    }

    fn run_state(_root: &Path, session_dir: &Path, clean: bool) -> RunState {
        RunState {
            app_version: "test".to_string(),
            pid: 123,
            started_at: "2026-10-11T09:00:00Z".to_string(),
            updated_at: "2026-10-11T09:00:01Z".to_string(),
            session_log_dir: session_dir.to_string_lossy().to_string(),
            clean_shutdown: clean,
            shutdown_at: clean.then(|| "2026-10-11T09:00:02Z".to_string()),
            exit_reason: clean.then(|| "test".to_string()),
            startup_trace_id: "test-trace".to_string(),
        }
    }

    #[test]
    fn session_dir_names_match_the_desktop_pattern() {
        assert!(is_session_dir_name("20261011T090000"));
        assert!(!is_session_dir_name("20261011T0900"));
        assert!(!is_session_dir_name("2026101XT090000"));
        assert!(!is_session_dir_name("diagnostics"));
    }

    #[test]
    fn detects_previous_unclean_run_state_with_crash_report() {
        let root = temp_root("unclean-crash");
        let session_dir = root.join("20261011T090000");
        fs::create_dir_all(&session_dir).expect("test session directory should be created");
        fs::write(session_dir.join(CRASH_REPORT_FILE), "{}")
            .expect("test crash report should be written");
        let run_state_path = root.join(RUN_STATE_FILE);
        write_json_file(&run_state_path, &run_state(&root, &session_dir, false))
            .expect("test run state should be written");

        let info = detect_previous_unexpected_exit(&run_state_path)
            .expect("unclean run state should be detected");
        assert!(info.detected);
        assert_eq!(info.category, UnexpectedExitCategory::Crash);
        assert!(info.crash_report_path.is_some());
        assert!(info.notify_on_startup);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn records_unclean_shutdown_without_notification_when_no_crash_report() {
        let root = temp_root("unclean-plain");
        let session_dir = root.join("20261011T090000");
        fs::create_dir_all(&session_dir).expect("test session directory should be created");
        let run_state_path = root.join(RUN_STATE_FILE);
        write_json_file(&run_state_path, &run_state(&root, &session_dir, false))
            .expect("test run state should be written");

        let info = detect_previous_unexpected_exit(&run_state_path)
            .expect("unclean run state should still be recorded");
        assert_eq!(info.category, UnexpectedExitCategory::UncleanShutdown);
        assert!(info.crash_report_path.is_none());
        assert!(!info.notify_on_startup);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ignores_previous_clean_run_state() {
        let root = temp_root("clean");
        let session_dir = root.join("20261011T090000");
        fs::create_dir_all(&session_dir).expect("test session directory should be created");
        let run_state_path = root.join(RUN_STATE_FILE);
        write_json_file(&run_state_path, &run_state(&root, &session_dir, true))
            .expect("test run state should be written");

        assert!(detect_previous_unexpected_exit(&run_state_path).is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exports_bundle_with_metadata_state_and_recent_sessions() {
        let root = temp_root("export");
        // Six session dirs: only the five most recent are bundled.
        for day in 10..16 {
            let dir = root.join(format!("202610{day:02}T090000"));
            fs::create_dir_all(&dir).expect("test session directory should be created");
            fs::write(dir.join(SHELL_LOG_FILE), format!("session {day} log"))
                .expect("test session log should be written");
        }
        let session_dir = root.join("20261016T090000");
        let run_state_path = root.join(RUN_STATE_FILE);
        write_json_file(&run_state_path, &run_state(&root, &session_dir, false))
            .expect("test run state should be written");

        let context = RunContext {
            logs_root: root.clone(),
            session_log_dir: session_dir.clone(),
            run_state_path: run_state_path.clone(),
        };
        let info = export_bundle_in(&context).expect("bundle export should succeed");
        let bundle = File::open(&info.bundle_path).expect("bundle should exist");
        let mut archive = zip::ZipArchive::new(bundle).expect("bundle should be a valid zip");

        assert!(
            archive
                .by_name(DIAGNOSTIC_METADATA_FILE)
                .expect("metadata entry should exist")
                .size()
                > 0
        );
        assert!(archive.by_name(ARCHIVE_RUN_STATE_FILE).is_ok());
        // Six session dirs exist; only the five most recent are bundled.
        assert!(archive
            .by_name("sessions/20261010T090000/shell.log")
            .is_err());
        for day in 11..16 {
            let entry = archive
                .by_name(&format!("sessions/202610{day:02}T090000/shell.log"))
                .unwrap_or_else(|_| panic!("session {day} should be bundled"));
            assert_eq!(entry.size(), format!("session {day} log").len() as u64);
        }

        let _ = fs::remove_dir_all(root);
    }
}
