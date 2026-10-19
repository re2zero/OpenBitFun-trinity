//! Single-instance guard for the shell.
//!
//! Core's Embedded runtime intentionally allows parallel hosts on one user
//! data root (desktop + CLI coexistence), so the kernel never blocks a second
//! shell. The shell still wants one window per data root: a second launch
//! with the same `OPENBITFUN_USER_ROOT` (or the default root) would boot a
//! second embedded runtime against the same sessions, which reads as a
//! confusing "empty sessions" window rather than an error. This guard makes
//! that case an explicit, script-readable exit instead. Set
//! `OPENBITFUN_GPUI_ALLOW_MULTI=1` to opt out for development and testing.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const LOCK_FILE_NAME: &str = "gpui-shell-app.lock";

/// Held for the process lifetime; dropping it releases the lock.
#[derive(Debug)]
pub struct InstanceLock {
    path: PathBuf,
}

/// Why a second launch was refused.
#[derive(Debug)]
pub struct AlreadyRunning {
    pub pid: u32,
    pub lock_path: PathBuf,
}

/// Acquires the per-user-root single-instance lock, taking over only when the
/// recorded owner is provably dead (crash leftovers).
pub fn acquire_instance_lock() -> Result<InstanceLock, AlreadyRunning> {
    let path = lock_path();
    acquire_at(&path, process_alive)
}

fn lock_path() -> PathBuf {
    // The runtime bootstrap initializes the path manager later in the launch
    // path; the manager itself resolves `OPENBITFUN_USER_ROOT` lazily, so an
    // isolated test root never contends with a real install.
    match openbitfun_core::infrastructure::try_get_path_manager_arc() {
        Ok(manager) => manager.user_config_dir().join(LOCK_FILE_NAME),
        Err(_) => std::env::temp_dir().join(LOCK_FILE_NAME),
    }
}

/// Linux liveness via procfs; a pid directory that is gone means the owner
/// crashed without cleanup.
fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn acquire_at(path: &Path, is_alive: impl Fn(u32) -> bool) -> Result<InstanceLock, AlreadyRunning> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            let _ = write!(file, "{}", std::process::id());
            Ok(InstanceLock {
                path: path.to_path_buf(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let owner = fs::read_to_string(path)
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok());
            match owner {
                Some(pid) if is_alive(pid) => Err(AlreadyRunning {
                    pid,
                    lock_path: path.to_path_buf(),
                }),
                // Missing pid, unparseable content, or a dead owner: the file
                // is crash debris, not a running shell.
                _ => {
                    let _ = fs::remove_file(path);
                    match fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                    {
                        Ok(mut file) => {
                            let _ = write!(file, "{}", std::process::id());
                            Ok(InstanceLock {
                                path: path.to_path_buf(),
                            })
                        }
                        Err(_) => Err(AlreadyRunning {
                            pid: owner.unwrap_or(0),
                            lock_path: path.to_path_buf(),
                        }),
                    }
                }
            }
        }
        Err(_) => Err(AlreadyRunning {
            pid: 0,
            lock_path: path.to_path_buf(),
        }),
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lock_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gpui-shell-instance-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_launch_is_refused_while_owner_is_alive() {
        let dir = temp_lock_dir("refused");
        let path = dir.join(LOCK_FILE_NAME);
        let _first = acquire_at(&path, |_| true).expect("first launch acquires");
        let second = acquire_at(&path, |_| true).unwrap_err();
        assert_eq!(second.pid, std::process::id());
        assert_eq!(second.lock_path, path);
    }

    #[test]
    fn dead_owner_lock_is_taken_over() {
        let dir = temp_lock_dir("takeover");
        let path = dir.join(LOCK_FILE_NAME);
        let first = acquire_at(&path, |_| true).expect("first launch acquires");
        assert_eq!(first.path, path);
        // Simulate a crashed owner: release without unlinking.
        std::mem::forget(first);
        assert!(path.exists(), "forgotten lock file stays on disk");
        let _second = acquire_at(&path, |_| false).expect("dead owner is taken over");
    }

    #[test]
    fn unparseable_lock_is_taken_over() {
        let dir = temp_lock_dir("garbage");
        let path = dir.join(LOCK_FILE_NAME);
        fs::write(&path, "not-a-pid").unwrap();
        let _lock = acquire_at(&path, |_| true).expect("garbage lock is crash debris");
    }

    #[test]
    fn drop_releases_the_lock() {
        let dir = temp_lock_dir("release");
        let path = dir.join(LOCK_FILE_NAME);
        {
            let _lock = acquire_at(&path, |_| true).expect("acquire");
        }
        assert!(!path.exists(), "dropping the guard unlinks the lock");
    }
}
