//! Daemon configuration ownership for the Trinity cognitive engine.
//!
//! `trinityd` is started without CLI overrides and reads `[daemon]` from its own
//! config file, so this host has to resolve the same file the daemon will: it
//! installs the packaged factory template on first run and then derives the port
//! and API key from it. Without that, a fresh install has no config at all, the
//! daemon falls back to stdio mode, and no TCP port ever opens.
//!
//! An existing config is never rewritten. It holds the user's model endpoints,
//! identity, and memory settings, and rewriting it would discard them.

use std::path::{Path, PathBuf};

/// Factory template, embedded in the binary so dev and packaged builds resolve
/// it identically (no resource-directory lookup). Its `{{DAEMON_KEY}}` is
/// replaced by [`install_factory_config`].
const FACTORY_TEMPLATE: &str = include_str!("../../resources/trinity-factory.toml");
const DAEMON_KEY_PLACEHOLDER: &str = "{{DAEMON_KEY}}";

/// Port assumed when the config cannot supply one. Matches the template.
pub(crate) const DEFAULT_DAEMON_PORT: u16 = 11656;
/// Key assumed when the config supplies none. A config without `[daemon] keys`
/// runs trinityd without authentication, so any value is accepted there.
pub(crate) const DEFAULT_DAEMON_KEY: &str = "trinity-local-dev-key";

/// `$XDG_CONFIG_HOME/trinity/trinity.toml` (`~/.config/trinity/...` by default).
/// This is the path trinityd resolves on its own when started without arguments.
pub(crate) fn user_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("trinity").join("trinity.toml"))
}

/// Daemon listen port: `TRINITY_DAEMON_PORT`, then the user config, then the
/// factory default. The host must agree with the daemon, which reads the config
/// it resolved itself.
pub(crate) fn daemon_port() -> u16 {
    env_port_override()
        .or_else(|| user_config_path().and_then(|p| read_daemon_port(&p)))
        .unwrap_or(DEFAULT_DAEMON_PORT)
}

/// `TRINITY_DAEMON_PORT`, the host-side port override.
///
/// trinityd takes its port from its config file and reads no environment, so an
/// override has to be forwarded on the command line. It is returned separately
/// from [`daemon_port`] for exactly that purpose.
pub(crate) fn env_port_override() -> Option<u16> {
    std::env::var("TRINITY_DAEMON_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
}

/// Daemon API key, resolved in the same order as [`daemon_port`].
pub(crate) fn daemon_key() -> String {
    daemon_key_env()
        .or_else(|| user_config_path().and_then(|p| read_daemon_key(&p)))
        .unwrap_or_else(|| DEFAULT_DAEMON_KEY.to_string())
}

/// `TRINITY_API_KEY`, the host-side key override.
///
/// trinityd reads its keys from the config file only, so this can never be
/// honoured on its own: it is used as the key installed on first run, and any
/// later change has to be made in the config as well.
fn daemon_key_env() -> Option<String> {
    std::env::var("TRINITY_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
}

/// Make sure trinityd will find a usable config, installing the factory template
/// when the user has none. Returns the path the daemon will read.
pub(crate) fn ensure_user_config() -> Result<PathBuf, String> {
    let path = user_config_path().ok_or_else(|| {
        "cannot determine the Trinity config directory (no HOME/XDG_CONFIG_HOME)".to_string()
    })?;
    ensure_user_config_at(&path)?;
    Ok(path)
}

fn ensure_user_config_at(path: &Path) -> Result<(), String> {
    if path.is_file() {
        check_daemon_usable(path)?;
        log::info!("[trinity] using daemon config {}", path.display());
        return Ok(());
    }

    let Some(parent) = path.parent() else {
        return Err(format!("invalid config path: {}", path.display()));
    };
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("create config dir {}: {e}", parent.display()))?;
    install_factory_config(path)?;
    log::info!(
        "[trinity] installed factory daemon config {}",
        path.display()
    );
    Ok(())
}

/// Write the factory config with a freshly generated daemon key.
///
/// The key is unique to this device and this install: trinityd authenticates
/// against `[daemon] keys`, so a value shipped in the package would be the same
/// secret on every machine. It is written through a temporary file so a reader
/// never sees a half-written config.
fn install_factory_config(path: &Path) -> Result<(), String> {
    let key = daemon_key_env().unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let config = FACTORY_TEMPLATE.replace(DAEMON_KEY_PLACEHOLDER, &key);
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, config).map_err(|e| format!("write config {}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("install config {}: {e}", path.display())
    })
}

/// Reject a config the daemon cannot serve TCP on. It is reported rather than
/// repaired: an existing file may hold user values that must not be guessed at.
fn check_daemon_usable(path: &Path) -> Result<(), String> {
    if read_daemon_enabled(path) != Some(true) {
        return Err(format!(
            "{} does not enable the Trinity daemon (`[daemon] enabled = true` missing), so no \
             TCP port would open; enable it in that file, or delete the file to reinstall the \
             factory configuration",
            path.display()
        ));
    }
    if read_daemon_port(path).is_none() {
        return Err(format!(
            "{} has no `[daemon] port`, so trinityd would listen on its own default while this \
             host probes {}; set the port in that file",
            path.display(),
            DEFAULT_DAEMON_PORT
        ));
    }
    Ok(())
}

fn read_daemon_table(path: &Path) -> Option<toml::Table> {
    let text = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    table.get("daemon")?.as_table().cloned()
}

fn read_daemon_enabled(path: &Path) -> Option<bool> {
    read_daemon_table(path)?.get("enabled")?.as_bool()
}

fn read_daemon_port(path: &Path) -> Option<u16> {
    let port = read_daemon_table(path)?.get("port")?.as_integer()?;
    u16::try_from(port).ok()
}

fn read_daemon_key(path: &Path) -> Option<String> {
    read_daemon_table(path)?
        .get("keys")?
        .as_array()?
        .iter()
        .find_map(|key| key.as_str().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Isolated directory per test: these tests must not touch the real user
    /// config, and must not depend on process-wide environment overrides that
    /// other tests in this binary also read.
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openbitfun-trinity-config-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn installs_factory_config_when_missing() {
        let dir = temp_dir("install");
        let path = dir.join("trinity").join("trinity.toml");
        assert!(!path.exists(), "precondition: no config yet");

        ensure_user_config_at(&path).expect("first run installs the factory config");

        assert_eq!(
            read_daemon_enabled(&path),
            Some(true),
            "daemon must be enabled"
        );
        assert_eq!(read_daemon_port(&path), Some(DEFAULT_DAEMON_PORT));

        // The key is generated per install, not shipped in the package.
        let key = read_daemon_key(&path).expect("installed config carries a key");
        assert_eq!(key.len(), 32, "unexpected key: {key}");
        assert!(
            key.chars().all(|c| c.is_ascii_hexdigit()),
            "unexpected key: {key}"
        );
        assert_ne!(
            key, DEFAULT_DAEMON_KEY,
            "the factory key must not be a constant"
        );

        // Slot binaries are not shipped in the package, so the template must not
        // ask the daemon to start any, and no placeholder may survive install.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text
                .lines()
                .any(|line| line.trim_start().starts_with("[slot")),
            "factory config must not enable slots"
        );
        assert!(
            !text.contains(DAEMON_KEY_PLACEHOLDER),
            "the key placeholder must be substituted on install"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generates_a_distinct_key_per_install() {
        let first = temp_dir("key-first");
        let second = temp_dir("key-second");
        let first_path = first.join("trinity.toml");
        let second_path = second.join("trinity.toml");

        ensure_user_config_at(&first_path).expect("first install");
        ensure_user_config_at(&second_path).expect("second install");

        assert_ne!(
            read_daemon_key(&first_path),
            read_daemon_key(&second_path),
            "each install must get its own key"
        );

        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }

    #[test]
    fn reinstalling_reuses_the_installed_config_and_key() {
        let dir = temp_dir("reinstall");
        let path = dir.join("trinity.toml");

        ensure_user_config_at(&path).expect("first install");
        let key = read_daemon_key(&path).expect("generated key");
        let installed = std::fs::read_to_string(&path).unwrap();

        // A reinstall, upgrade, or later start must keep the same file, so the
        // key the daemon authenticates with never changes underneath a running
        // setup.
        ensure_user_config_at(&path).expect("later run reuses the config");
        assert_eq!(read_daemon_key(&path).as_deref(), Some(key.as_str()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn never_overwrites_an_existing_config() {
        let dir = temp_dir("existing");
        let path = dir.join("trinity.toml");
        let user_config = "[daemon]\nenabled = true\nport = 12656\nkeys = [\"user-key\"]\n";
        std::fs::write(&path, user_config).unwrap();

        ensure_user_config_at(&path).expect("an existing config is reused");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), user_config);
        assert_eq!(read_daemon_port(&path), Some(12656), "user port survives");
        assert_eq!(read_daemon_key(&path).as_deref(), Some("user-key"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_config_that_cannot_start_the_daemon() {
        let dir = temp_dir("disabled");
        let path = dir.join("trinity.toml");
        let disabled = "[daemon]\nenabled = false\nport = 11556\n";
        std::fs::write(&path, disabled).unwrap();

        let error = ensure_user_config_at(&path).expect_err("a disabled daemon is not usable");
        assert!(error.contains("enabled"), "unexpected error: {error}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            disabled,
            "the user file must be left untouched"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_enabled_config_without_a_port() {
        let dir = temp_dir("no-port");
        let path = dir.join("trinity.toml");
        std::fs::write(&path, "[daemon]\nenabled = true\n").unwrap();

        let error = ensure_user_config_at(&path).expect_err("a port is required");
        assert!(error.contains("port"), "unexpected error: {error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_daemon_values_from_a_config_file() {
        let dir = temp_dir("read");
        let path = dir.join("trinity.toml");
        std::fs::write(
            &path,
            "[daemon]\nport = 23231\nkeys = [\"first\", \"second\"]\n",
        )
        .unwrap();

        assert_eq!(read_daemon_port(&path), Some(23231));
        assert_eq!(read_daemon_key(&path).as_deref(), Some("first"));
        assert_eq!(read_daemon_enabled(&path), None, "enabled is absent here");
        assert_eq!(read_daemon_port(&dir.join("absent.toml")), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
