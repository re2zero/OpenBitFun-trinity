//! Self-update for the standalone GPUI shell binary.
//!
//! The shell ships as one self-contained executable (a `tar.gz` archive plus a
//! published `.sha256`, mirrored by the Debian package), so updating in place
//! means: fetch the release manifest, verify the archive against its published
//! digest, extract it, and swap the binary inside its own directory. This all
//! runs before any window exists, so packagers and updaters can drive the
//! binary headlessly (`--check-update`, `--self-update`).
//!
//! Trust boundary, stated plainly: the digest is read from the same origin that
//! serves the archive, so it proves the transfer was not corrupted — not who
//! published it. The manifest reserves `sigUrl` for the signed-release
//! pipeline, which mirrors how the CLI only verifies signatures when a release
//! key is compiled into the build. Until this frontend is on the official
//! release feed (an M10 decision), the official default feed does not carry a
//! `gpuiShell` asset and the run says so instead of pretending.
//!
//! Failure policy: nothing is installed unless the digest matches, the
//! previous binary is kept under a hidden staging directory until the swapped-in
//! binary answers `--version`, and a rollback that itself fails is reported
//! with the location of the surviving copy rather than swallowed.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, Context, Result};
use flate2::read::GzDecoder;
use futures_util::StreamExt as _;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tar::Archive;
use tokio::io::AsyncWriteExt as _;

use crate::cli::{ProcessCommand, BIN_NAME, EXIT_UPDATE_AVAILABLE, EXIT_USAGE};

/// Official release feed: the same document the CLI reads. It carries a
/// `gpuiShell` asset per platform once this frontend is published through the
/// release pipeline; until then the key is absent and the run reports that.
pub(crate) const OFFICIAL_MANIFEST: &str = "https://openbitfun.com/release/linux-binaries-v1.json";

/// Manifests are a few KB, so a whole-request ceiling is safe here even though
/// archive downloads deliberately have none (a ceiling turns "slow" into
/// "impossible" for a 40 MB archive on a thin link).
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Stall guard for a single read, not a ceiling on the whole transfer.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// The archive is ~40 MB. The download is streamed to disk, so this bounds the
/// disk (and a hostile `Content-Length`) rather than memory.
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
/// A lock older than this is treated as abandoned by a killed updater.
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60 * 60);
const LOCK_FILE_NAME: &str = "gpui-shell-update.lock";

/// What `--check-update` / `--self-update` concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateOutcome {
    /// The feed offers nothing newer than this build.
    Current,
    /// `--check-update` found a newer release and was not asked to install it.
    Available,
    /// The running binary was replaced on disk.
    Updated,
    /// This build cannot update itself; the reason was printed.
    Unsupported,
}

/// Where the release manifest comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ManifestSource {
    Remote(String),
    Local(PathBuf),
}

impl ManifestSource {
    /// `None` means the official feed; `--update-manifest` accepts a URL or a
    /// file path so the whole flow is testable without the network.
    fn parse(raw: Option<&str>) -> Result<Self> {
        match raw {
            None => Ok(Self::Remote(OFFICIAL_MANIFEST.to_string())),
            Some(value) if value.starts_with("http://") || value.starts_with("https://") => {
                Ok(Self::Remote(value.to_string()))
            }
            Some(value) if !value.trim().is_empty() => Ok(Self::Local(PathBuf::from(value))),
            Some(_) => Err(anyhow!("--update-manifest needs a URL or a file path")),
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Remote(url) => url,
            Self::Local(path) => path.to_str().unwrap_or("the local release manifest"),
        }
    }
}

pub(crate) struct UpdateRequest {
    source: ManifestSource,
    check_only: bool,
}

impl UpdateRequest {
    pub(crate) fn from_args(manifest: Option<&str>, check_only: bool) -> Result<Self> {
        Ok(Self {
            source: ManifestSource::parse(manifest)?,
            check_only,
        })
    }
}

/// Release manifest, same shape as the CLI's (`linux-binaries-v1.json`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseManifest {
    schema_version: u32,
    version: String,
    platforms: HashMap<String, PlatformEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformEntry {
    target: String,
    /// The `gpuiShell` asset. Absent until the release pipeline publishes this
    /// frontend, which is a clear "not offered" state rather than a failure to
    /// fetch.
    #[serde(default)]
    gpui_shell: Option<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseAsset {
    filename: String,
    url: String,
    sha256_url: String,
    /// Reserved for the signed-release pipeline; see the module docs.
    #[serde(default)]
    sig_url: Option<String>,
}

/// Numeric release version. Build metadata is ignored; a pre-release tag ranks
/// below its release (`1.0.4-beta.1 < 1.0.4`), matching the feed contract.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseVersion {
    core: (u64, u64, u64),
    pre: Option<String>,
}

impl ReleaseVersion {
    fn parse(raw: &str) -> Option<Self> {
        let without_metadata = raw.split('+').next()?;
        let (numeric, pre) = match without_metadata.split_once('-') {
            Some((numeric, tag)) => (numeric, Some(tag.to_string())),
            None => (without_metadata, None),
        };
        let mut parts = numeric.split('.');
        let core = (
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        if parts.next().is_some() {
            return None;
        }
        Some(Self { core, pre })
    }

    fn is_newer_than(&self, other: &Self) -> bool {
        if self.core != other.core {
            return self.core > other.core;
        }
        match (&self.pre, &other.pre) {
            (None, Some(_)) => true,
            (Some(candidate), Some(current)) => candidate > current,
            _ => false,
        }
    }
}

/// `--check-update` / `--self-update` entry point. Returns the process exit
/// code when the caller should exit instead of opening a window.
pub fn answer_update_command(
    command: &ProcessCommand,
    handle: &tokio::runtime::Handle,
) -> Option<i32> {
    let (manifest, check_only) = match command {
        ProcessCommand::CheckUpdate { manifest } => (manifest.as_deref(), true),
        ProcessCommand::SelfUpdate { manifest } => (manifest.as_deref(), false),
        _ => return None,
    };
    let request = match UpdateRequest::from_args(manifest, check_only) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("{error:#}");
            return Some(EXIT_USAGE);
        }
    };
    match handle.block_on(run(request)) {
        Ok(UpdateOutcome::Current | UpdateOutcome::Updated) => Some(0),
        Ok(UpdateOutcome::Available) => Some(EXIT_UPDATE_AVAILABLE),
        Ok(UpdateOutcome::Unsupported) => Some(EXIT_USAGE),
        Err(error) => {
            eprintln!("{error:#}");
            Some(1)
        }
    }
}

async fn run(request: UpdateRequest) -> Result<UpdateOutcome> {
    let current_exe = std::env::current_exe().context("resolve the running shell executable")?;
    if let Some(reason) = unsupported_reason(&current_exe) {
        eprintln!("{reason}");
        return Ok(UpdateOutcome::Unsupported);
    }
    let platform_key = platform_key().expect("a supported platform has a manifest key");

    let client = build_client()?;
    let manifest = load_manifest(&client, &request.source).await?;
    let current = ReleaseVersion::parse(env!("CARGO_PKG_VERSION"))
        .ok_or_else(|| anyhow!("this build's version is not a release version"))?;
    let offered = ReleaseVersion::parse(&manifest.version)
        .ok_or_else(|| anyhow!("the manifest announces a non-release version"))?;
    if !offered.is_newer_than(&current) {
        println!(
            "{BIN_NAME} {} is up to date (manifest {}, {})",
            env!("CARGO_PKG_VERSION"),
            manifest.version,
            request.source.label()
        );
        return Ok(UpdateOutcome::Current);
    }

    // The asset matters only once there is something to install: a feed that
    // predates this frontend must not fail an otherwise clean check.
    let asset = shell_asset(&manifest, platform_key)?;
    if request.check_only {
        println!(
            "{BIN_NAME} {} is available (current {}, from {})",
            manifest.version,
            env!("CARGO_PKG_VERSION"),
            request.source.label()
        );
        return Ok(UpdateOutcome::Available);
    }

    let sig_url = asset.sig_url.clone();
    let filename = asset.filename.clone();
    match (release_pubkey(), &sig_url) {
        (Some(_), Some(_)) => {}
        (Some(_), None) => bail!(
            "this build requires signed releases but {} publishes no signature for {filename}; refusing to install",
            request.source.label()
        ),
        (None, Some(_)) => tracing::warn!(
            "Release {} publishes a signature, but this build verifies only the SHA256 digest (no release public key was injected at build time)",
            manifest.version
        ),
        (None, None) => {}
    }

    let _lock = InstallLock::acquire()?;
    let work = tempfile::tempdir().context("create the update work directory")?;
    let archive_path = work.path().join(asset_file_name(&asset.filename)?);
    let digest = download_archive(&client, asset, &archive_path).await?;
    let checksum_text = fetch_text(&client, &asset.sha256_url).await?;
    verify_sha256(&digest, &checksum_text, &filename)?;

    // The signature covers the archive itself, so it is checked against the
    // downloaded bytes after the transfer (the digest gate above has already
    // bounded what those bytes can be).
    if let (Some(pubkey), Some(sig_url)) = (release_pubkey(), sig_url) {
        let signature = fetch_text(&client, &sig_url)
            .await
            .with_context(|| format!("fetch release signature {sig_url}"))?;
        let archive = fs::read(&archive_path).with_context(|| {
            format!("read {} for signature verification", archive_path.display())
        })?;
        verify_signature(&archive, &signature, pubkey)
            .context("release signature verification failed")?;
    }

    let package_dir = extract_package(&archive_path, work.path())?;
    let new_binary = package_dir.join(BIN_NAME);
    let verify = |path: &Path| validate_shell_binary(path, &manifest.version);
    install_binary(&new_binary, &current_exe, &verify)?;
    println!(
        "{BIN_NAME} was updated to {}. Restart the shell to use it.",
        manifest.version
    );
    Ok(UpdateOutcome::Updated)
}

/// Why this build cannot replace itself, or `None` when it can. Stated as an
/// explicit unsupported state instead of a fake success.
fn unsupported_reason(current_exe: &Path) -> Option<String> {
    if platform_key().is_none() {
        return Some(format!(
            "{BIN_NAME} self-update supports Linux x86_64 and ARM64 builds only."
        ));
    }
    if current_exe.file_name() != Some(OsStr::new(BIN_NAME)) {
        return Some(format!(
            "{BIN_NAME} self-update requires the official executable name `{BIN_NAME}` \
             (running as `{}`); reinstall it from the package you installed.",
            current_exe.display()
        ));
    }
    if is_development_binary(current_exe) {
        return Some(format!(
            "{BIN_NAME} self-update refuses to replace a build artifact inside a cargo `target` \
             directory; copy the binary out of the build tree first."
        ));
    }
    None
}

fn platform_key() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    match std::env::consts::ARCH {
        "x86_64" => Some("linux-x86_64"),
        "aarch64" => Some("linux-aarch64"),
        _ => None,
    }
}

fn expected_target(platform_key: &str) -> Option<&'static str> {
    match platform_key {
        "linux-x86_64" => Some("x86_64-unknown-linux-gnu"),
        "linux-aarch64" => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// The `gpuiShell` asset for this platform, after checking the manifest's
/// `target` really names the platform the binary was built for.
fn shell_asset<'a>(manifest: &'a ReleaseManifest, platform_key: &str) -> Result<&'a ReleaseAsset> {
    let expected = expected_target(platform_key)
        .ok_or_else(|| anyhow!("no updater platform is defined for {platform_key}"))?;
    let platform = manifest
        .platforms
        .get(platform_key)
        .ok_or_else(|| anyhow!("the manifest does not contain {platform_key}"))?;
    if platform.target != expected {
        bail!(
            "the manifest target {} does not match {expected} for {platform_key}",
            platform.target
        );
    }
    let asset = platform.gpui_shell.as_ref().ok_or_else(|| {
        anyhow!("the manifest does not offer a gpuiShell asset for {platform_key}")
    })?;
    if !asset.filename.ends_with(".tar.gz") {
        bail!("the gpuiShell release asset is not a tar.gz archive");
    }
    Ok(asset)
}

/// True for a path inside a cargo build tree; replacing those breaks the
/// developer's own build output (and the hardlinked copies cargo keeps).
fn is_development_binary(executable: &Path) -> bool {
    executable
        .components()
        .any(|component| component.as_os_str() == "target")
}

fn build_client() -> Result<Client> {
    openbitfun_services_core::tls_provider::ensure_ring_crypto_provider();
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .context("build the shell updater HTTP client")
}

async fn load_manifest(client: &Client, source: &ManifestSource) -> Result<ReleaseManifest> {
    let manifest = match source {
        ManifestSource::Remote(url) => {
            let body = tokio::time::timeout(MANIFEST_TIMEOUT, async {
                client
                    .get(url)
                    .send()
                    .await
                    .with_context(|| format!("request {url}"))?
                    .error_for_status()
                    .with_context(|| format!("fetch {url}"))?
                    .bytes()
                    .await
                    .with_context(|| format!("read {url}"))
            })
            .await
            .map_err(|_| anyhow!("{url} did not answer within {MANIFEST_TIMEOUT:?}"))??;
            serde_json::from_slice::<ReleaseManifest>(&body)
                .with_context(|| format!("parse {url}"))?
        }
        ManifestSource::Local(path) => {
            let body = fs::read(path)
                .with_context(|| format!("read the release manifest {}", path.display()))?;
            serde_json::from_slice::<ReleaseManifest>(&body)
                .with_context(|| format!("parse {}", path.display()))?
        }
    };
    if manifest.schema_version != 1 {
        bail!(
            "unsupported release manifest schema {}",
            manifest.schema_version
        );
    }
    Ok(manifest)
}

/// The archive name from the manifest is used as a path component, so it must
/// be a plain file name and nothing else.
fn asset_file_name(filename: &str) -> Result<&str> {
    let name = Path::new(filename)
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            anyhow!("the release asset filename `{filename}` is not a plain file name")
        })?;
    if name != filename {
        bail!("the release asset filename `{filename}` is not a plain file name");
    }
    Ok(name)
}

/// Streams the archive to `destination` while hashing it, so memory stays at
/// one chunk and a failed digest never leaves an installed binary behind.
async fn download_archive(
    client: &Client,
    asset: &ReleaseAsset,
    destination: &Path,
) -> Result<String> {
    let response = client
        .get(&asset.url)
        .send()
        .await
        .with_context(|| format!("request {}", asset.url))?
        .error_for_status()
        .with_context(|| format!("fetch {}", asset.url))?;
    if let Some(length) = response.content_length() {
        if length > MAX_ARCHIVE_BYTES {
            bail!(
                "{} announces {length} bytes; the updater stops at {} MB",
                asset.url,
                MAX_ARCHIVE_BYTES / (1024 * 1024)
            );
        }
    }

    let mut file = tokio::fs::File::create(destination)
        .await
        .with_context(|| format!("create {}", destination.display()))?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("read {}", asset.url))?;
        received += chunk.len() as u64;
        if received > MAX_ARCHIVE_BYTES {
            bail!(
                "{} exceeds the {} MB archive ceiling; refusing to keep reading",
                asset.url,
                MAX_ARCHIVE_BYTES / (1024 * 1024)
            );
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", destination.display()))?;
    }
    file.flush()
        .await
        .with_context(|| format!("write {}", destination.display()))?;
    Ok(format!("{:x}", hasher.finalize()))
}

async fn fetch_text(client: &Client, url: &str) -> Result<String> {
    tokio::time::timeout(MANIFEST_TIMEOUT, async {
        client
            .get(url)
            .send()
            .await
            .with_context(|| format!("request {url}"))?
            .error_for_status()
            .with_context(|| format!("fetch {url}"))?
            .text()
            .await
            .with_context(|| format!("read {url}"))
    })
    .await
    .map_err(|_| anyhow!("{url} did not answer within {MANIFEST_TIMEOUT:?}"))?
}

/// Standard `sha256sum` output: `<digest>  <filename>`.
/// The trust root for release archives: the same `OPENBITFUN_RELEASE_PUBKEY`
/// the CLI updater trusts (official minisign key `50F47CBE6CC0A376`), injected
/// at release-build time. Builds without the injection verify the digest only;
/// the packaging script warns loudly so an official build cannot forget the
/// key quietly.
const RELEASE_PUBKEY: Option<&str> = option_env!("OPENBITFUN_RELEASE_PUBKEY");

fn release_pubkey() -> Option<&'static str> {
    RELEASE_PUBKEY.filter(|key| !key.trim().is_empty())
}

/// Verify a Tauri-format `.sig` (base64 of a minisign signature file) over the
/// archive. The public key accepts both current raw Tauri values and the
/// legacy base64 wrapper. Same trust semantics as the CLI updater: a checksum
/// only proves the transfer was not corrupted — whoever serves the archive can
/// serve a matching `.sha256`; a signature is what a mirror cannot forge.
fn verify_signature(archive: &[u8], signature_b64: &str, pubkey: &str) -> Result<()> {
    use base64::Engine as _;

    let public_key_text = if pubkey.trim().starts_with("untrusted comment:") {
        pubkey.trim().to_owned()
    } else {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(pubkey.trim().as_bytes())
            .context("decode release public key")?;
        String::from_utf8(bytes).context("decode release public key as UTF-8")?
    };
    let public_key = minisign_verify::PublicKey::decode(&public_key_text)
        .map_err(|error| anyhow!("invalid release public key: {error}"))?;
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_b64.trim().as_bytes())
        .context("decode release signature")?;
    let signature_text =
        String::from_utf8(signature_bytes).context("decode release signature as UTF-8")?;
    let signature = minisign_verify::Signature::decode(&signature_text)
        .map_err(|error| anyhow!("invalid release signature: {error}"))?;
    public_key
        .verify(archive, &signature, false)
        .map_err(|error| anyhow!("release signature does not match the archive: {error}"))
}

fn verify_sha256(actual_digest: &str, checksum_text: &str, filename: &str) -> Result<()> {
    let expected = checksum_text.split_whitespace().next().unwrap_or_default();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("the published checksum for {filename} is not a SHA256 digest");
    }
    if !actual_digest.eq_ignore_ascii_case(expected) {
        bail!("SHA256 mismatch for {filename}: refusing to install this archive");
    }
    Ok(())
}

/// Unpacks the archive and returns the directory that holds the binary. The
/// archive's top-level name is not trusted; the directory that actually carries
/// the binary wins.
fn extract_package(archive_path: &Path, work_dir: &Path) -> Result<PathBuf> {
    let extract_dir = work_dir.join("package");
    fs::create_dir_all(&extract_dir).context("create the update extraction directory")?;
    let archive_file =
        fs::File::open(archive_path).with_context(|| format!("open {}", archive_path.display()))?;
    Archive::new(GzDecoder::new(archive_file))
        .unpack(&extract_dir)
        .context("extract the release archive")?;

    if extract_dir.join(BIN_NAME).is_file() {
        return Ok(extract_dir);
    }
    for entry in fs::read_dir(&extract_dir).context("inspect the release archive")? {
        let path = entry?.path();
        if path.is_dir() && path.join(BIN_NAME).is_file() {
            return Ok(path);
        }
    }
    bail!("the release archive does not contain {BIN_NAME}")
}

/// A release binary is only usable if it runs and reports the version the
/// manifest announced; anything else is a wrong or stale artifact.
fn validate_shell_binary(binary: &Path, expected_version: &str) -> Result<()> {
    let output = openbitfun_services_core::process_manager::create_command(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("run {}", binary.display()))?;
    if !output.status.success() {
        bail!("{} --version failed ({})", binary.display(), output.status);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains(BIN_NAME) {
        bail!(
            "{} does not identify itself as {BIN_NAME}",
            binary.display()
        );
    }
    if !stdout.contains(expected_version) {
        bail!(
            "{} reports a version other than the announced {expected_version}",
            binary.display()
        );
    }
    Ok(())
}

fn install_binary(
    new_binary: &Path,
    current_exe: &Path,
    verify: &dyn Fn(&Path) -> Result<()>,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let install_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", current_exe.display()))?;
    // Staging lives next to the target so the final swap is a rename on one
    // filesystem; the hidden prefix keeps a crashed update out of sight.
    let prefix = format!(
        "{}-update.",
        openbitfun_services_core::product_identity::hidden_data_directory()
    );
    let stage = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir_in(install_dir)
        .with_context(|| {
            format!(
                "create the update staging directory in {}",
                install_dir.display()
            )
        })?;
    let stage_path = stage.path().to_path_buf();
    let staged = stage_path.join(BIN_NAME);
    let backup = stage_path.join(format!("previous-{BIN_NAME}"));

    fs::copy(new_binary, &staged).context("stage the new binary")?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
        .context("make the staged binary executable")?;
    verify(&staged).context("the staged binary is not usable")?;

    if let Err(failure) = replace_binary(verify, current_exe, &staged, &backup) {
        if failure.previous_kept.is_some() {
            // The rollback failed, so this directory holds the only remaining
            // copy of the previous binary: keep it and say where it is instead
            // of letting the temp-dir guard delete it.
            let kept = stage.keep();
            tracing::error!(
                "Update failed and the previous binary could not be restored; it is kept at {}",
                kept.display()
            );
        }
        return Err(failure.error);
    }
    Ok(())
}

/// A failed swap: `previous_kept` names the directory still holding the
/// previous binary when putting it back did not work.
#[derive(Debug)]
struct SwapFailure {
    error: anyhow::Error,
    previous_kept: Option<PathBuf>,
}

/// Swaps `staged` into `current`'s place with the previous binary as a backup
/// until `verify` passes on the installed path. Any failure restores the
/// previous binary; a restore that fails is reported explicitly instead of
/// leaving "no working binary" as an unexplained error.
fn replace_binary(
    verify: &dyn Fn(&Path) -> Result<()>,
    current: &Path,
    staged: &Path,
    backup: &Path,
) -> std::result::Result<(), SwapFailure> {
    let backup_error = |error: std::io::Error, step: String| SwapFailure {
        error: anyhow!(error).context(step),
        previous_kept: None,
    };
    fs::rename(current, backup)
        .map_err(|error| backup_error(error, format!("back up {}", current.display())))?;

    let mut rollback_failures: Vec<String> = Vec::new();
    let restore = |rollback_failures: &mut Vec<String>| {
        if let Err(error) = fs::rename(backup, current) {
            rollback_failures.push(format!("restore {}: {error}", current.display()));
        }
    };
    let failed = |error: anyhow::Error, rollback_failures: Vec<String>| {
        if rollback_failures.is_empty() {
            return SwapFailure {
                error,
                previous_kept: None,
            };
        }
        SwapFailure {
            error: error.context(format!(
                "the previous {BIN_NAME} could NOT be put back ({}); reinstall {BIN_NAME} from \
                 a release archive before using it again",
                rollback_failures.join("; ")
            )),
            previous_kept: backup.parent().map(Path::to_path_buf),
        }
    };

    if let Err(error) = fs::rename(staged, current) {
        restore(&mut rollback_failures);
        return Err(failed(
            anyhow!(error).context(format!("install {}", current.display())),
            rollback_failures,
        ));
    }
    if let Err(error) = verify(current) {
        // A binary that failed its own version probe must not stay installed:
        // the previous one goes back in its place.
        if let Err(remove_error) = fs::remove_file(current) {
            rollback_failures.push(format!(
                "remove the failed {}: {remove_error}",
                current.display()
            ));
        }
        restore(&mut rollback_failures);
        return Err(failed(
            error.context(format!("validate {}", current.display())),
            rollback_failures,
        ));
    }
    Ok(())
}

/// One updater at a time. Two concurrent runs interleaving their backup and
/// swap renames can leave no working binary at all, and the download is large
/// enough that a duplicate run is worth refusing rather than racing.
#[derive(Debug)]
struct InstallLock {
    path: PathBuf,
}

impl InstallLock {
    fn acquire() -> Result<Self> {
        let path = Self::default_path()?;
        Self::acquire_at(&path, LOCK_STALE_AFTER)
    }

    /// The shell's shared config directory, so an isolated `OPENBITFUN_USER_ROOT`
    /// never contends with a real install.
    fn default_path() -> Result<PathBuf> {
        let manager = openbitfun_core::infrastructure::try_get_path_manager_arc()
            .map_err(|error| anyhow!("cannot resolve the update lock location: {error}"))?;
        Ok(manager.user_config_dir().join(LOCK_FILE_NAME))
    }

    fn acquire_at(path: &Path, stale_after: Duration) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        // `create_new` is the whole point: a check-then-write leaves a window in
        // which two updaters both see no lock. Only a stale lock is cleared, and
        // only then is the create retried.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                let _ = write!(file, "{}", std::process::id());
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !lock_is_stale(path, stale_after) {
                    bail!(
                        "another {BIN_NAME} update is already running (lock {})",
                        path.display()
                    );
                }
                fs::remove_file(path)
                    .with_context(|| format!("clear stale lock {}", path.display()))?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .with_context(|| format!("create {}", path.display()))?;
                let _ = write!(file, "{}", std::process::id());
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(error) => Err(error).with_context(|| format!("create {}", path.display())),
        }
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_is_stale(path: &Path, stale_after: Duration) -> bool {
    match fs::metadata(path).and_then(|meta| meta.modified()) {
        Ok(modified) => SystemTime::now()
            .duration_since(modified)
            .map(|age| age > stale_after)
            .unwrap_or(false),
        // Unreadable or already gone: nothing is holding it.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;

    const MANIFEST: &str = r#"{
        "schemaVersion": 1,
        "version": "9.9.9",
        "platforms": {
            "linux-x86_64": {
                "target": "x86_64-unknown-linux-gnu",
                "gpuiShell": {
                    "filename": "openbitfun-gpui-shell-9.9.9-x86_64-unknown-linux-gnu.tar.gz",
                    "url": "https://example.invalid/openbitfun-gpui-shell-9.9.9.tar.gz",
                    "sha256Url": "https://example.invalid/openbitfun-gpui-shell-9.9.9.tar.gz.sha256",
                    "sigUrl": null
                },
                "cli": {
                    "filename": "openbitfun-9.9.9-x86_64-unknown-linux-gnu.tar.gz",
                    "url": "https://example.invalid/openbitfun.tar.gz",
                    "sha256Url": "https://example.invalid/openbitfun.tar.gz.sha256"
                }
            },
            "linux-aarch64": {
                "target": "aarch64-unknown-linux-gnu"
            }
        }
    }"#;

    fn parsed_manifest() -> ReleaseManifest {
        serde_json::from_str(MANIFEST).expect("the sample manifest parses")
    }

    /// A stand-in release binary: a script the validator accepts and the
    /// version probe can read.
    fn fake_binary(dir: &Path, version: &str) -> PathBuf {
        let path = dir.join(BIN_NAME);
        fs::write(&path, format!("#!/bin/sh\necho \"{BIN_NAME} {version}\"\n"))
            .expect("write the fake binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("make the fake binary executable");
        }
        path
    }

    fn tar_gz_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (name, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive
                .append_data(&mut header, name, *contents)
                .expect("append the fixture entry");
        }
        archive
            .into_inner()
            .expect("finish the fixture archive")
            .finish()
            .expect("finish the fixture archive")
    }

    /// Fixture produced with the Tauri signer CLI in its base64-wrapped
    /// minisign wire format, pinning the exact on-disk format CI must emit
    /// (same fixture as the CLI updater's).
    const FIXTURE_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IERENTQzQUM5RUY0NTIzRTMKUldUakkwWHZ5VHBVM1NOMXJWMHhLVlljSDBOY2x4YlpxVHA2clN1NEJPMWcyY2Qvd2U4VUR2b3AK";
    const FIXTURE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUakkwWHZ5VHBVM2RVVFdoR3FNZDltSWNUeEQ1K2ZnNWRUSnYxWk5lUkZzd0h0MkdzSUhUSlV6a0haUTdNZm1aemM5QVBQWW50UWgvaWpFcEp1Zkp4SERWdnhIc1g2YUFrPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzg4NDg2NTU4CWZpbGU6Lm9wZW5iaXRmdW4tbWluaXNpZ24tZml4dHVyZS50eHQKa1QxdDQ3bWtLVlhaZUdFSjR4R0V5R1Z3REVnUlI0RGJqbHFoZkVHdkdLSlFyTGJ5Z05JRTI5V3dwdXRkSFpZckUrK0RaUVVJYUJod1dzcmVydHZnQXc9PQo=";
    const FIXTURE_DATA: &[u8] = b"hello-openbitfun\n";

    #[test]
    fn release_signature_accepts_the_tauri_wire_format() {
        use base64::Engine as _;

        verify_signature(FIXTURE_DATA, FIXTURE_SIGNATURE, FIXTURE_PUBKEY)
            .expect("minisign signature in Tauri's base64 wrapper must verify");
        let raw_pubkey = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(FIXTURE_PUBKEY)
                .expect("decode fixture public key"),
        )
        .expect("fixture public key is UTF-8");
        verify_signature(FIXTURE_DATA, FIXTURE_SIGNATURE, &raw_pubkey)
            .expect("raw minisign public key must verify");
    }

    #[test]
    fn release_signature_rejects_tampered_bytes() {
        // The whole point: a mirror that alters the archive cannot also forge
        // this, unlike the checksum it serves alongside it.
        let tampered = b"hello-openbitfun-tampered\n";
        assert!(verify_signature(tampered, FIXTURE_SIGNATURE, FIXTURE_PUBKEY).is_err());
        assert!(verify_signature(FIXTURE_DATA, "bm90LWEtc2lnbmF0dXJl", FIXTURE_PUBKEY).is_err());
    }

    #[test]
    fn manifest_source_accepts_the_official_feed_a_url_or_a_file() {
        assert_eq!(
            ManifestSource::parse(None).expect("the default source"),
            ManifestSource::Remote(OFFICIAL_MANIFEST.to_string())
        );
        assert_eq!(
            ManifestSource::parse(Some("http://127.0.0.1:9/m.json")).expect("an HTTP source"),
            ManifestSource::Remote("http://127.0.0.1:9/m.json".to_string())
        );
        assert_eq!(
            ManifestSource::parse(Some("https://example.invalid/m.json")).expect("an HTTPS source"),
            ManifestSource::Remote("https://example.invalid/m.json".to_string())
        );
        assert_eq!(
            ManifestSource::parse(Some("/tmp/m.json")).expect("a file source"),
            ManifestSource::Local(PathBuf::from("/tmp/m.json"))
        );
        assert_eq!(
            ManifestSource::parse(Some("relative/m.json")).expect("a relative file source"),
            ManifestSource::Local(PathBuf::from("relative/m.json"))
        );
        assert!(ManifestSource::parse(Some("   ")).is_err());
    }

    #[test]
    fn manifest_parses_the_cli_shaped_document() {
        let manifest = parsed_manifest();
        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.version, "9.9.9");
        let asset = shell_asset(&manifest, "linux-x86_64").expect("the shell asset is offered");
        assert!(asset.filename.ends_with(".tar.gz"));
        assert!(asset.sha256_url.ends_with(".sha256"));
        assert_eq!(asset.sig_url, None);
    }

    #[test]
    fn manifest_platform_gates_are_explicit() {
        let manifest = parsed_manifest();
        // A platform without the asset is "not offered", not "up to date".
        let error = shell_asset(&manifest, "linux-aarch64").unwrap_err();
        assert!(error.to_string().contains("gpuiShell"), "{error}");
        // A platform that exists but is not the one this binary was built for.
        let mut mismatched = parsed_manifest();
        mismatched
            .platforms
            .get_mut("linux-x86_64")
            .expect("platform")
            .target = "aarch64-unknown-linux-gnu".to_string();
        let error = shell_asset(&mismatched, "linux-x86_64").unwrap_err();
        assert!(error.to_string().contains("does not match"), "{error}");
        let mut missing = parsed_manifest();
        missing.platforms.remove("linux-x86_64");
        let error = shell_asset(&missing, "linux-x86_64").unwrap_err();
        assert!(error.to_string().contains("does not contain"), "{error}");
    }

    #[test]
    fn asset_filenames_stay_plain_file_names() {
        assert_eq!(
            asset_file_name("openbitfun-gpui-shell-9.9.9.tar.gz").unwrap(),
            "openbitfun-gpui-shell-9.9.9.tar.gz"
        );
        assert!(asset_file_name("../../etc/passwd").is_err());
        assert!(asset_file_name("nested/archive.tar.gz").is_err());
        assert!(asset_file_name("").is_err());
    }

    #[test]
    fn version_comparison_follows_the_feed_contract() {
        let version = |raw: &str| ReleaseVersion::parse(raw).expect("a release version parses");
        assert!(version("1.0.4").is_newer_than(&version("1.0.3")));
        assert!(version("1.0.10").is_newer_than(&version("1.0.9")));
        assert!(version("1.1.0").is_newer_than(&version("1.0.99")));
        assert!(!version("1.0.3").is_newer_than(&version("1.0.3")));
        assert!(!version("1.0.3").is_newer_than(&version("1.0.4")));
        // Build metadata is not part of the comparison; a pre-release ranks
        // below its release.
        assert!(!version("1.0.3+build.7").is_newer_than(&version("1.0.3")));
        assert!(version("1.0.4").is_newer_than(&version("1.0.4-beta.1")));
        assert!(version("1.0.4-beta.2").is_newer_than(&version("1.0.4-beta.1")));
        assert!(!version("1.0.4-beta.1").is_newer_than(&version("1.0.4")));
        assert!(ReleaseVersion::parse("nonsense").is_none());
        assert!(ReleaseVersion::parse("1.0").is_none());
        assert!(ReleaseVersion::parse("1.0.3.4").is_none());
    }

    #[test]
    fn checksum_contract_matches_sha256sum_output() {
        let digest = "a".repeat(64);
        verify_sha256(
            &digest,
            &format!("{digest}  openbitfun-gpui-shell.tar.gz\n"),
            "openbitfun-gpui-shell.tar.gz",
        )
        .expect("a sha256sum line verifies");
        verify_sha256(&digest, &digest.to_uppercase(), "archive").expect("case is irrelevant");

        // Any deviation refuses the archive instead of installing it.
        let other = "b".repeat(64);
        assert!(verify_sha256(&digest, &other, "archive").is_err());
        assert!(verify_sha256(&digest, "not-a-digest archive", "archive").is_err());
        assert!(verify_sha256(&digest, &"a".repeat(63), "archive").is_err());
        assert!(verify_sha256(&digest, &format!("{}z", &digest[..63]), "archive").is_err());
        assert!(verify_sha256(&digest, "", "archive").is_err());
    }

    #[test]
    fn unsupported_builds_are_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        // Official name, outside a cargo build tree: supported on Linux.
        let official = dir.path().join(BIN_NAME);
        assert_eq!(unsupported_reason(&official), None);
        // Wrong name: refused with the reason.
        let renamed = dir.path().join("openbitfun-gpui-shell-copy");
        let reason = unsupported_reason(&renamed).expect("a renamed binary is refused");
        assert!(reason.contains("official executable name"), "{reason}");
        // Development build: refused even under the official name.
        let development = dir.path().join("target").join("release").join(BIN_NAME);
        let reason = unsupported_reason(&development).expect("a cargo artifact is refused");
        assert!(reason.contains("target"), "{reason}");
    }

    fn executable_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("write the script");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod the script");
        path
    }

    #[test]
    fn installed_binary_is_validated_by_running_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let good = executable_script(
            dir.path(),
            "good",
            &format!("#!/bin/sh\necho \"{BIN_NAME} 9.9.9\"\n"),
        );
        validate_shell_binary(&good, "9.9.9").expect("the matching version validates");

        let wrong_version = executable_script(
            dir.path(),
            "old",
            &format!("#!/bin/sh\necho \"{BIN_NAME} 1.0.3\"\n"),
        );
        assert!(validate_shell_binary(&wrong_version, "9.9.9").is_err());

        let failing = executable_script(dir.path(), "failing", "#!/bin/sh\nexit 3\n");
        assert!(validate_shell_binary(&failing, "9.9.9").is_err());

        let silent = executable_script(dir.path(), "silent", "#!/bin/sh\nexit 0\n");
        assert!(validate_shell_binary(&silent, "9.9.9").is_err());
    }

    #[test]
    fn extracting_looks_for_the_binary_not_the_archive_name() {
        let work = tempfile::tempdir().expect("temp dir");
        let archive_path = work.path().join("release.tar.gz");
        let archive = tar_gz_with(&[
            (
                "openbitfun-gpui-shell-9.9.9-x86_64-unknown-linux-gnu/LICENSE",
                b"MIT",
            ),
            (
                "openbitfun-gpui-shell-9.9.9-x86_64-unknown-linux-gnu/openbitfun-gpui-shell",
                b"binary",
            ),
        ]);
        fs::write(&archive_path, archive).expect("write the fixture archive");
        let package = extract_package(&archive_path, work.path()).expect("the package is found");
        assert!(package.join(BIN_NAME).is_file());

        let other_work = tempfile::tempdir().expect("temp dir");
        let empty_path = other_work.path().join("empty.tar.gz");
        fs::write(&empty_path, tar_gz_with(&[("notes.txt", b"nothing")]))
            .expect("write the empty fixture");
        let error = extract_package(&empty_path, other_work.path()).unwrap_err();
        assert!(error.to_string().contains("does not contain"), "{error}");
    }

    #[test]
    fn a_verified_swap_replaces_the_binary_and_keeps_the_previous_one_for_the_caller() {
        let dir = tempfile::tempdir().expect("temp dir");
        let current = fake_binary(dir.path(), "1.0.3");
        let staging = tempfile::tempdir_in(dir.path()).expect("staging dir");
        let staged = fake_binary(staging.path(), "9.9.9");
        let backup = staging.path().join("previous");

        replace_binary(
            &|path| validate_shell_binary(path, "9.9.9"),
            &current,
            &staged,
            &backup,
        )
        .expect("the swap succeeds");
        let installed = fs::read_to_string(&current).expect("read the installed binary");
        assert!(installed.contains("9.9.9"));
        // The previous binary stays in the caller's staging directory until the
        // guard that owns it removes it, so a crash right after the swap still
        // leaves a way back.
        assert!(fs::read_to_string(&backup)
            .expect("read the backup")
            .contains("1.0.3"));
    }

    #[test]
    fn a_failed_install_restores_the_previous_binary() {
        let dir = tempfile::tempdir().expect("temp dir");
        let current = fake_binary(dir.path(), "1.0.3");
        let before = fs::read_to_string(&current).expect("read the current binary");
        let staging = tempfile::tempdir_in(dir.path()).expect("staging dir");
        let backup = staging.path().join("previous");
        // A staged path that does not exist: the install rename fails and the
        // backup has to come back.
        let missing = staging.path().join("vanished");

        let failure =
            replace_binary(&|_| Ok(()), &current, &missing, &backup).expect_err("the swap fails");
        assert!(failure.previous_kept.is_none());
        assert!(
            failure.error.to_string().contains("install"),
            "{:?}",
            failure.error
        );
        assert_eq!(
            fs::read_to_string(&current).expect("read the restored binary"),
            before
        );
    }

    #[test]
    fn a_failed_validation_rolls_back_and_reports_a_failed_rollback() {
        let dir = tempfile::tempdir().expect("temp dir");
        let current = fake_binary(dir.path(), "1.0.3");
        let before = fs::read_to_string(&current).expect("read the current binary");
        let staging = tempfile::tempdir_in(dir.path()).expect("staging dir");
        let staged = fake_binary(staging.path(), "9.9.9");
        let backup = staging.path().join("previous");

        // Validation refuses the installed file: the previous binary returns.
        let failure = replace_binary(&|_| bail!("not usable"), &current, &staged, &backup)
            .expect_err("the swap fails");
        assert!(failure.previous_kept.is_none());
        assert_eq!(
            fs::read_to_string(&current).expect("read the restored binary"),
            before
        );

        // Now make the rollback itself impossible and check the failure is
        // reported with the directory that still holds the previous binary.
        let staged = fake_binary(staging.path(), "9.9.9");
        let failure = replace_binary(
            &|_| {
                fs::remove_dir_all(staging.path()).expect("remove the staging directory");
                bail!("not usable")
            },
            &current,
            &staged,
            &backup,
        )
        .expect_err("the swap fails");
        let kept = failure
            .previous_kept
            .as_ref()
            .expect("the surviving copy is named");
        assert_eq!(kept, staging.path());
        let message = format!("{:#}", failure.error);
        assert!(message.contains("could NOT be put back"), "{message}");
        assert!(
            !current.exists(),
            "the failed install left no binary behind for the caller to trust"
        );
    }

    #[test]
    fn install_binary_swaps_in_a_staged_copy() {
        let dir = tempfile::tempdir().expect("temp dir");
        let current = fake_binary(dir.path(), "1.0.3");
        let release_dir = tempfile::tempdir().expect("release dir");
        let new_binary = fake_binary(release_dir.path(), "9.9.9");

        install_binary(&new_binary, &current, &|path| {
            validate_shell_binary(path, "9.9.9")
        })
        .expect("the install succeeds");
        assert!(fs::read_to_string(&current)
            .expect("read the installed binary")
            .contains("9.9.9"));
        // No staging leftovers: the hidden directory is gone after the swap.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read the install directory")
            .map(|entry| entry.expect("entry").file_name())
            .filter(|name| name.to_string_lossy().starts_with(".openbitfun"))
            .collect();
        assert!(leftovers.is_empty(), "staging leftovers: {leftovers:?}");

        // A staged binary that does not validate leaves the install untouched.
        let bad_release = tempfile::tempdir().expect("release dir");
        let bad_binary = fake_binary(bad_release.path(), "0.0.1");
        let before = fs::read_to_string(&current).expect("read the current binary");
        assert!(install_binary(&bad_binary, &current, &|path| {
            validate_shell_binary(path, "9.9.9")
        })
        .is_err());
        assert_eq!(
            fs::read_to_string(&current).expect("read the current binary"),
            before
        );
    }

    #[test]
    fn install_lock_is_exclusive_and_reusable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("update.lock");
        let lock = InstallLock::acquire_at(&path, LOCK_STALE_AFTER).expect("first lock");
        let error =
            InstallLock::acquire_at(&path, LOCK_STALE_AFTER).expect_err("a held lock is refused");
        assert!(error.to_string().contains("already running"), "{error}");
        drop(lock);
        assert!(!path.exists(), "the lock is released on drop");
        InstallLock::acquire_at(&path, LOCK_STALE_AFTER).expect("the lock is reusable");

        // A lock left behind by a killed updater is taken over.
        let stale = dir.path().join("stale.lock");
        fs::write(&stale, "1").expect("write the stale lock");
        InstallLock::acquire_at(&stale, Duration::ZERO).expect("a stale lock is taken over");
    }
}
