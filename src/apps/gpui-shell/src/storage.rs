//! Storage-root isolation for the GPUI shell.
//!
//! The shell is a second surface on the same machine as the desktop app, and
//! both write the same persisted shapes under one user root — most sharply the
//! workspace registry. A shell built from a different revision can therefore
//! write a record the desktop build cannot read back; on 2026-10-15 exactly that
//! happened (records without `rootPath`) and the desktop app refused to start
//! until the catalog was repaired by hand.
//!
//! So the shell resolves its own root by default: a shell build can only damage
//! its own catalog. Working against the shared, real roots stays possible on
//! purpose — set `OPENBITFUN_GPUI_SHARED_STORAGE=1`, or point
//! `OPENBITFUN_USER_ROOT` / `OPENBITFUN_HOME` at them explicitly.
//!
//! This must run before anything resolves a path, including the single-instance
//! lock, the update lock, and the session log directory.

use std::path::{Path, PathBuf};

/// Opt-in switch: keep the shell on the shared product roots.
pub const SHARED_STORAGE_ENV: &str = "OPENBITFUN_GPUI_SHARED_STORAGE";
/// Overrides the base directory of the shell's own storage.
pub const STORAGE_ROOT_ENV: &str = "OPENBITFUN_GPUI_STORAGE_ROOT";

const USER_ROOT_ENV: &str = "OPENBITFUN_USER_ROOT";
const E2E_USER_ROOT_ENV: &str = "OPENBITFUN_E2E_USER_ROOT";
const PRODUCT_HOME_ENV: &str = "OPENBITFUN_HOME";
const DEFAULT_DIRECTORY_NAME: &str = "openbitfun-gpui-shell";

/// What the shell decided about its storage roots.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StorageDecision {
    /// The operator asked for the shared roots.
    Shared,
    /// An explicit root is already configured; leave the environment alone.
    ExplicitRoot,
    /// Point the shell at its own roots.
    Isolated {
        user_root: PathBuf,
        product_home: PathBuf,
    },
}

fn configured(env: &impl Fn(&str) -> Option<String>, name: &str) -> bool {
    env(name).is_some_and(|value| !value.trim().is_empty())
}

fn decide_storage(env: impl Fn(&str) -> Option<String>, isolated_base: PathBuf) -> StorageDecision {
    if env(SHARED_STORAGE_ENV).as_deref() == Some("1") {
        return StorageDecision::Shared;
    }
    // An explicit root outranks the default in either direction: scripts and
    // smoke runs set these to pick their own sandbox.
    if configured(&env, USER_ROOT_ENV) || configured(&env, E2E_USER_ROOT_ENV) {
        return StorageDecision::ExplicitRoot;
    }
    StorageDecision::Isolated {
        user_root: isolated_base.join("config"),
        product_home: isolated_base.join("home"),
    }
}

/// Base directory the shell's `config` and `home` roots live under.
fn isolated_base(env: impl Fn(&str) -> Option<String>) -> (PathBuf, bool) {
    if let Some(value) = env(STORAGE_ROOT_ENV).filter(|value| !value.trim().is_empty()) {
        return (PathBuf::from(value), false);
    }
    match dirs::data_dir() {
        Some(data_dir) => (data_dir.join(DEFAULT_DIRECTORY_NAME), false),
        None => (std::env::temp_dir().join(DEFAULT_DIRECTORY_NAME), true),
    }
}

/// Keeps this shell off the shared product roots unless the operator asks for
/// them. Call first in `main`, before any path resolution.
pub fn apply_isolated_storage_defaults() {
    let (base, temporary_base) = isolated_base(|name| std::env::var(name).ok());
    let fallback_notice = temporary_base.then(|| display(&base));
    match decide_storage(|name| std::env::var(name).ok(), base) {
        StorageDecision::Shared => {
            eprintln!(
                "OpenBitFun GPUI shell: {SHARED_STORAGE_ENV}=1, using the shared product roots"
            );
        }
        StorageDecision::ExplicitRoot => {
            eprintln!("OpenBitFun GPUI shell: storage root already configured, leaving it as is");
        }
        StorageDecision::Isolated {
            user_root,
            product_home,
        } => {
            std::env::set_var(USER_ROOT_ENV, &user_root);
            std::env::set_var(PRODUCT_HOME_ENV, &product_home);
            eprintln!(
                "OpenBitFun GPUI shell: storage isolated to {} (config) and {} (home); set {SHARED_STORAGE_ENV}=1 to use the shared roots",
                display(&user_root),
                display(&product_home)
            );
            if let Some(base) = fallback_notice {
                eprintln!(
                    "OpenBitFun GPUI shell: no platform data directory; falling back to {base}"
                );
            }
        }
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn defaults_to_the_shells_own_roots() {
        let base = PathBuf::from("/state/openbitfun-gpui-shell");
        assert_eq!(
            decide_storage(env_from(&[]), base.clone()),
            StorageDecision::Isolated {
                user_root: base.join("config"),
                product_home: base.join("home"),
            }
        );
    }

    #[test]
    fn shared_storage_switch_keeps_the_operator_roots() {
        let decision = decide_storage(
            env_from(&[(SHARED_STORAGE_ENV, "1")]),
            PathBuf::from("/state/openbitfun-gpui-shell"),
        );
        assert_eq!(decision, StorageDecision::Shared);
    }

    #[test]
    fn an_explicit_user_root_outranks_the_default() {
        for name in [USER_ROOT_ENV, E2E_USER_ROOT_ENV] {
            let decision = decide_storage(
                env_from(&[(name, "/tmp/smoke-root")]),
                PathBuf::from("/state/openbitfun-gpui-shell"),
            );
            assert_eq!(decision, StorageDecision::ExplicitRoot, "{name}");
        }
        // A blank value is not a configured root.
        let decision = decide_storage(
            env_from(&[(USER_ROOT_ENV, "  ")]),
            PathBuf::from("/state/openbitfun-gpui-shell"),
        );
        assert!(matches!(decision, StorageDecision::Isolated { .. }));
    }

    #[test]
    fn storage_root_env_moves_both_roots() {
        let (base, temporary) = isolated_base(env_from(&[(STORAGE_ROOT_ENV, "/tmp/shell-state")]));
        assert_eq!(base, PathBuf::from("/tmp/shell-state"));
        assert!(!temporary);
    }
}
