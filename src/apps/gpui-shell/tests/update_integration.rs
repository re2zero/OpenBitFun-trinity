//! End-to-end self-update test.
//!
//! A local HTTP/1.1 fixture serves a release manifest, its `.sha256`, and the
//! archive; the real binary runs `--check-update` / `--self-update` against it
//! and replaces itself. No external network and no display are involved: the
//! updater answers before any window exists.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use flate2::write::GzEncoder;
use flate2::Compression;
use openbitfun_gpui_shell::cli::{BIN_NAME, EXIT_UPDATE_AVAILABLE};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

fn target() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64-unknown-linux-gnu",
        _ => "x86_64-unknown-linux-gnu",
    }
}

fn platform() -> (&'static str, &'static str) {
    match std::env::consts::ARCH {
        "aarch64" => ("linux-aarch64", target()),
        _ => ("linux-x86_64", target()),
    }
}

/// A release the fixture publishes: the served files plus what the test expects
/// to end up installed.
struct Release {
    version: String,
    archive_name: String,
    artifact: Vec<u8>,
    files: HashMap<String, Vec<u8>>,
}

/// Builds the release. `archive_corrupted` keeps the published digest pointing
/// at the original bytes while the archive on the wire is damaged.
fn release_fixture(version: &str, archive_corrupted: bool) -> Release {
    let artifact = stub_artifact(version);
    let archive_name = format!("{BIN_NAME}-{version}-{}.tar.gz", target());
    let archive = tar_gz(&format!("{BIN_NAME}-{version}-{}", target()), &artifact);
    let digest = format!("{:x}", Sha256::digest(&archive));
    let served_archive = if archive_corrupted {
        let mut corrupted = archive;
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0xff;
        corrupted
    } else {
        archive
    };

    let mut files = HashMap::new();
    files.insert(format!("/{archive_name}"), served_archive);
    files.insert(
        format!("/{archive_name}.sha256"),
        format!("{digest}  {archive_name}\n").into_bytes(),
    );
    Release {
        version: version.to_string(),
        archive_name,
        artifact,
        files,
    }
}

/// The artifact a release ships. A script is enough for the engine: what the
/// updater checks is that the installed file is executable and reports the
/// announced version.
fn stub_artifact(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho \"{BIN_NAME} {version}\"\n").into_bytes()
}

fn tar_gz(stage: &str, artifact: &[u8]) -> Vec<u8> {
    let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut binary = tar::Header::new_gnu();
    binary.set_size(artifact.len() as u64);
    binary.set_mode(0o755);
    binary.set_cksum();
    archive
        .append_data(&mut binary, format!("{stage}/{BIN_NAME}"), artifact)
        .expect("append the fixture binary");
    let mut license = tar::Header::new_gnu();
    license.set_size(3);
    license.set_mode(0o644);
    license.set_cksum();
    archive
        .append_data(&mut license, format!("{stage}/LICENSE"), &b"MIT"[..])
        .expect("append the fixture license");
    archive
        .into_inner()
        .expect("finish the fixture archive")
        .finish()
        .expect("finish the fixture archive")
}

fn manifest_json(version: &str, release: &Release, base_url: &str, offer_asset: bool) -> Vec<u8> {
    let (platform_key, target) = platform();
    let asset = if offer_asset {
        serde_json::json!({
            "filename": release.archive_name,
            "url": format!("{base_url}/{}", release.archive_name),
            "sha256Url": format!("{base_url}/{}.sha256", release.archive_name),
            "sigUrl": null,
        })
    } else {
        serde_json::Value::Null
    };
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "version": version,
        "platforms": { platform_key: { "target": target, "gpuiShell": asset } }
    });
    serde_json::to_vec_pretty(&manifest).expect("the fixture manifest serializes")
}

/// Minimal HTTP/1.1 server: one request per connection, exact-path routing. The
/// served files are built from the bound address, so manifests can reference
/// their own fixture.
struct Server {
    base_url: String,
    _task: tokio::task::JoinHandle<()>,
}

impl Server {
    async fn start(build_files: impl FnOnce(&str) -> HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the fixture server");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("fixture server address")
        );
        let routes = Arc::new(build_files(&base_url));
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let routes = Arc::clone(&routes);
                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    let read = socket.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                    match routes.get(&path) {
                        Some(body) => {
                            let head = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = socket.write_all(head.as_bytes()).await;
                            let _ = socket.write_all(body).await;
                        }
                        None => {
                            let _ = socket
                                .write_all(
                                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                                )
                                .await;
                        }
                    }
                    let _ = socket.flush().await;
                });
            }
        });
        Self {
            base_url,
            _task: task,
        }
    }

    fn manifest_url(&self) -> String {
        format!("{}/manifest.json", self.base_url)
    }
}

/// The updater refuses to replace a binary inside a cargo `target` directory, so
/// the test needs the real executable elsewhere. A hard link into the home cache
/// keeps that instant and disk-free (both paths are on the same filesystem); the
/// swap only moves directory entries, so the artifact cargo owns stays intact.
fn installed_shell() -> (TempDir, PathBuf) {
    let artifact = PathBuf::from(env!("CARGO_BIN_EXE_openbitfun-gpui-shell"));
    let cache = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache"))
        .filter(|cache| cache.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let dir = tempfile::Builder::new()
        .prefix("gpui-shell-update-test-")
        .tempdir_in(cache)
        .expect("create the test install directory");
    let installed = dir.path().join(BIN_NAME);
    if fs::hard_link(&artifact, &installed).is_err() {
        fs::copy(&artifact, &installed).expect("install the shell copy");
    }
    (dir, installed)
}

/// Isolation roots: the update lock lives under the shared config directory, and
/// each test gets its own so runs never contend with each other or a real
/// install.
struct Isolation {
    _dir: TempDir,
    user_root: PathBuf,
    home: PathBuf,
}

impl Isolation {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create the isolation root");
        let user_root = dir.path().join("config");
        let home = dir.path().join("home");
        Self {
            _dir: dir,
            user_root,
            home,
        }
    }

    /// Where the updater's install lock is expected to live.
    fn lock_path(&self) -> PathBuf {
        self.user_root.join("config").join("gpui-shell-update.lock")
    }
}

async fn run_shell(binary: &Path, args: &[&str], isolation: &Isolation) -> std::process::Output {
    let mut command = openbitfun_services_core::process_manager::create_tokio_command(binary);
    command
        .args(args)
        .env("OPENBITFUN_USER_ROOT", &isolation.user_root)
        .env("OPENBITFUN_HOME", &isolation.home)
        .stdin(Stdio::null());
    command.output().await.expect("the shell answers")
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn sha256_of(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(fs::read(path).expect("read the file"))
    )
}

/// Every fixture test drives the real binary, whose startup builds a runtime;
/// the extra worker keeps the fixture server responsive while the child runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_update_downloads_verifies_and_replaces_the_running_binary() {
    let (_install_dir, installed) = installed_shell();
    let isolation = Isolation::new();
    let release = release_fixture("9.9.9", false);
    let expected_artifact = release.artifact.clone();
    let server = Server::start(|base_url| {
        let mut files = release.files.clone();
        files.insert(
            "/manifest.json".to_string(),
            manifest_json(&release.version, &release, base_url, true),
        );
        files
    })
    .await;

    let before = sha256_of(&installed);

    let check = run_shell(
        &installed,
        &[
            "--check-update",
            "--update-manifest",
            &server.manifest_url(),
        ],
        &isolation,
    )
    .await;
    assert_eq!(
        check.status.code(),
        Some(EXIT_UPDATE_AVAILABLE),
        "stdout: {} stderr: {}",
        stdout_of(&check),
        stderr_of(&check)
    );
    assert!(
        stdout_of(&check).contains("9.9.9 is available"),
        "{}",
        stdout_of(&check)
    );
    assert_eq!(
        sha256_of(&installed),
        before,
        "a check must not touch the installed binary"
    );

    let update = run_shell(
        &installed,
        &["--self-update", "--update-manifest", &server.manifest_url()],
        &isolation,
    )
    .await;
    assert_eq!(
        update.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        stdout_of(&update),
        stderr_of(&update)
    );
    assert!(
        stdout_of(&update).contains("was updated to 9.9.9"),
        "{}",
        stdout_of(&update)
    );

    // The installed file is exactly the served artifact, and it answers with the
    // announced version: the release really landed.
    assert_eq!(
        sha256_of(&installed),
        format!("{:x}", Sha256::digest(&expected_artifact)),
        "the installed binary must be the release artifact"
    );
    let version = run_shell(&installed, &["--version"], &isolation).await;
    assert_eq!(version.status.code(), Some(0));
    assert!(
        stdout_of(&version).contains("9.9.9"),
        "{}",
        stdout_of(&version)
    );

    // No staging leftovers next to the binary.
    let leftovers: Vec<String> = fs::read_dir(installed.parent().expect("install dir"))
        .expect("read the install directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != BIN_NAME)
        .collect();
    assert!(leftovers.is_empty(), "leftovers: {leftovers:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_update_refuses_an_archive_that_fails_its_digest() {
    let (_install_dir, installed) = installed_shell();
    let isolation = Isolation::new();
    let release = release_fixture("9.9.9", true);
    let server = Server::start(|base_url| {
        let mut files = release.files.clone();
        files.insert(
            "/manifest.json".to_string(),
            manifest_json(&release.version, &release, base_url, true),
        );
        files
    })
    .await;
    let before = sha256_of(&installed);

    let update = run_shell(
        &installed,
        &["--self-update", "--update-manifest", &server.manifest_url()],
        &isolation,
    )
    .await;
    assert_eq!(update.status.code(), Some(1), "{}", stdout_of(&update));
    assert!(
        stderr_of(&update).contains("SHA256 mismatch"),
        "{}",
        stderr_of(&update)
    );
    assert_eq!(
        sha256_of(&installed),
        before,
        "a rejected archive must leave the installed binary untouched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_update_says_so_when_the_feed_offers_no_shell_asset() {
    let (_install_dir, installed) = installed_shell();
    let isolation = Isolation::new();
    let release = release_fixture("9.9.9", false);
    let server = Server::start(|_| {
        let mut files = HashMap::new();
        files.insert(
            "/manifest.json".to_string(),
            manifest_json("9.9.9", &release, "http://127.0.0.1:1", false),
        );
        files
    })
    .await;

    let update = run_shell(
        &installed,
        &["--self-update", "--update-manifest", &server.manifest_url()],
        &isolation,
    )
    .await;
    assert_eq!(update.status.code(), Some(1));
    assert!(
        stderr_of(&update).contains("gpuiShell"),
        "{}",
        stderr_of(&update)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn check_update_reports_an_older_feed_without_needing_an_asset() {
    let (_install_dir, installed) = installed_shell();
    let isolation = Isolation::new();
    let release = release_fixture("0.0.1", false);
    let server = Server::start(|_| {
        let mut files = HashMap::new();
        files.insert(
            "/manifest.json".to_string(),
            manifest_json("0.0.1", &release, "http://127.0.0.1:1", false),
        );
        files
    })
    .await;

    let check = run_shell(
        &installed,
        &[
            "--check-update",
            "--update-manifest",
            &server.manifest_url(),
        ],
        &isolation,
    )
    .await;
    assert_eq!(
        check.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&check)
    );
    assert!(
        stdout_of(&check).contains("up to date"),
        "{}",
        stdout_of(&check)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_updater_is_refused_while_the_lock_is_held() {
    let (_install_dir, installed) = installed_shell();
    let isolation = Isolation::new();
    let lock = isolation.lock_path();
    fs::create_dir_all(lock.parent().expect("lock directory"))
        .expect("create the config directory");
    fs::write(&lock, std::process::id().to_string()).expect("write the lock");

    let release = release_fixture("9.9.9", false);
    let server = Server::start(|base_url| {
        let mut files = release.files.clone();
        files.insert(
            "/manifest.json".to_string(),
            manifest_json(&release.version, &release, base_url, true),
        );
        files
    })
    .await;

    let update = run_shell(
        &installed,
        &["--self-update", "--update-manifest", &server.manifest_url()],
        &isolation,
    )
    .await;
    assert_eq!(update.status.code(), Some(1));
    assert!(
        stderr_of(&update).contains("already running"),
        "{}",
        stderr_of(&update)
    );
}
