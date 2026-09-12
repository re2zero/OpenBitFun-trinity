//! Trinity being (cognitive being) identity registry — host read surface.
//!
//! The engine owns the unified identity layout:
//!
//! ```text
//! ~/.trinity/beings/
//! ├── core/being.toml                core cognitive being (engine template, never deleted)
//! └── assistants/<id>/being.toml     user assistants
//! ```
//!
//! `trinityd` is the only writer: `trinityd --init` and the `ceremony.awaken`
//! RPC both persist `core/being.toml`. This module only reads that layout and
//! deletes assistants, so the host never becomes a second identity source.
//!
//! The root resolves exactly like the engine's: `TRINITY_BEINGS_DIR` when set,
//! otherwise `~/.trinity/beings`, so host and daemon always agree.

mod template;

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use template::{CORE_BEING_ID, CORE_DEFAULT_PERSONA, CORE_SOUL};

/// Being kind: the core cognitive being, or a deletable user assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BeingKind {
    Core,
    Assistant,
}

impl Default for BeingKind {
    fn default() -> Self {
        Self::Assistant
    }
}

impl BeingKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Assistant => "assistant",
        }
    }
}

/// One `being.toml` record.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BeingConfig {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) kind: BeingKind,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) user_name: String,
    #[serde(default = "default_persona")]
    pub(crate) persona: String,
    #[serde(default)]
    pub(crate) awakened: bool,
    #[serde(default)]
    pub(crate) awakened_at: Option<f64>,
    #[serde(default)]
    pub(crate) created_at: f64,
}

fn default_persona() -> String {
    CORE_DEFAULT_PERSONA.to_string()
}

/// Read-only view of the being identity registry.
#[derive(Debug, Clone)]
pub(crate) struct BeingRegistry {
    root: PathBuf,
}

impl BeingRegistry {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Default root: `TRINITY_BEINGS_DIR`, otherwise `~/.trinity/beings`.
    pub(crate) fn default_root() -> PathBuf {
        if let Some(dir) = std::env::var_os("TRINITY_BEINGS_DIR") {
            if !dir.is_empty() {
                return PathBuf::from(dir);
            }
        }
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".trinity")
            .join("beings")
    }

    fn core_config_path(&self) -> PathBuf {
        self.root.join(CORE_BEING_ID).join("being.toml")
    }

    fn assistants_dir(&self) -> PathBuf {
        self.root.join("assistants")
    }

    /// Load the core cognitive being. `None` before the awaken ceremony.
    pub(crate) fn load_core(&self) -> Result<Option<BeingConfig>, String> {
        let path = self.core_config_path();
        if !path.exists() {
            return Ok(None);
        }
        let mut config = read_config(&path)?;
        // Pinned: the core record is always the core kind, whatever the file says.
        config.id = CORE_BEING_ID.to_string();
        config.kind = BeingKind::Core;
        Ok(Some(config))
    }

    /// List user assistants, sorted by id. A missing directory is an empty list.
    pub(crate) fn list_assistants(&self) -> Result<Vec<BeingConfig>, String> {
        let mut assistants = Vec::new();
        let Ok(entries) = fs::read_dir(self.assistants_dir()) else {
            return Ok(assistants);
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            let config_path = dir.join("being.toml");
            if !dir.is_dir() || !config_path.exists() {
                continue;
            }
            let mut config = read_config(&config_path)?;
            if config.id.is_empty() {
                config.id = entry.file_name().to_string_lossy().into_owned();
            }
            config.kind = BeingKind::Assistant;
            assistants.push(config);
        }
        assistants.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(assistants)
    }

    /// Delete a user assistant. The core being is never deletable.
    pub(crate) fn delete_assistant(&self, id: &str) -> Result<(), String> {
        let id = id.trim();
        if id == CORE_BEING_ID {
            return Err("the core cognitive being cannot be deleted".to_string());
        }
        if id.is_empty() || id.contains('/') || id.contains('\\') {
            return Err(format!("invalid assistant id '{id}'"));
        }
        let dir = self.assistants_dir().join(id);
        if !dir.is_dir() {
            return Err(format!("assistant '{id}' does not exist"));
        }
        fs::remove_dir_all(&dir).map_err(|e| format!("delete assistant '{id}': {e}"))
    }

    /// Core being soul text. Embedded, so it survives a deleted `beings/core/`.
    pub(crate) fn core_soul(&self) -> &'static str {
        CORE_SOUL
    }
}

fn read_config(path: &Path) -> Result<BeingConfig, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    toml::from_str(&content).map_err(|e| format!("parse {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Isolated root under the OS temp dir (this crate has no tempfile dev-dep).
    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("openbitfun-beings-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create temp root");
            Self { path }
        }

        fn registry(&self) -> BeingRegistry {
            BeingRegistry::new(self.path.join("beings"))
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_core(root: &TempRoot, body: &str) {
        let dir = root.path.join("beings").join("core");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("being.toml"), body).unwrap();
    }

    fn write_assistant(root: &TempRoot, id: &str, body: &str) {
        let dir = root.path.join("beings").join("assistants").join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("being.toml"), body).unwrap();
    }

    #[test]
    fn missing_core_is_not_an_error() {
        let root = TempRoot::new();
        assert!(root.registry().load_core().unwrap().is_none());
    }

    #[test]
    fn core_record_is_pinned_to_core_kind() {
        let root = TempRoot::new();
        write_core(
      &root,
      "id = \"renamed\"\nkind = \"assistant\"\nname = \"银月\"\nuser_name = \"公子\"\npersona = \"sage\"\nawakened = true\nawakened_at = 12.5\ncreated_at = 1.0\n",
    );

        let core = root.registry().load_core().unwrap().unwrap();
        assert_eq!(core.id, CORE_BEING_ID);
        assert_eq!(core.kind, BeingKind::Core);
        assert_eq!(core.name, "银月");
        assert_eq!(core.user_name, "公子");
        assert!(core.awakened);
        assert_eq!(core.awakened_at, Some(12.5));
    }

    #[test]
    fn assistants_are_listed_sorted_with_forced_kind() {
        let root = TempRoot::new();
        write_assistant(
            &root,
            "beta",
            "id = \"beta\"\nkind = \"core\"\nname = \"B\"\n",
        );
        write_assistant(&root, "alpha", "id = \"alpha\"\nname = \"A\"\n");

        let assistants = root.registry().list_assistants().unwrap();
        assert_eq!(
            assistants.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        assert!(assistants.iter().all(|a| a.kind == BeingKind::Assistant));
        // A missing persona falls back to the engine default.
        assert_eq!(assistants[0].persona, CORE_DEFAULT_PERSONA);
    }

    #[test]
    fn core_being_cannot_be_deleted() {
        let root = TempRoot::new();
        let err = root.registry().delete_assistant("core").unwrap_err();
        assert!(err.contains("cannot be deleted"));
    }

    #[test]
    fn assistant_delete_rejects_paths_and_unknown_ids() {
        let root = TempRoot::new();
        let registry = root.registry();
        assert!(registry.delete_assistant("../core").is_err());
        assert!(registry.delete_assistant("  ").is_err());
        assert!(registry
            .delete_assistant("ghost")
            .unwrap_err()
            .contains("does not exist"));
    }

    #[test]
    fn assistant_delete_removes_the_record() {
        let root = TempRoot::new();
        write_assistant(&root, "alpha", "id = \"alpha\"\nname = \"A\"\n");
        root.registry().delete_assistant("alpha").unwrap();
        assert!(root.registry().list_assistants().unwrap().is_empty());
    }

    #[test]
    fn core_soul_is_embedded() {
        let root = TempRoot::new();
        assert!(root.registry().core_soul().contains("银月"));
    }
}
