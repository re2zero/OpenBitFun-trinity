//! Plugin UI contribution projection (R7).
//!
//! Owns the installed-plugin UI facts the shell renders: the contribution
//! manifest per plugin, the current tree per scene, and the generation fencing
//! that retracts open scenes when a plugin is uninstalled. Providers produce
//! plain JSON documents (the same shapes the package files carry); this module
//! is the single validation and diagnostic funnel — nothing reaches the shell
//! unvalidated. The shell consumes snapshots through the accessors below and
//! never talks to a provider directly.
//!
//! The [`PluginUiProvider`] trait is deliberately synchronous: the R7b fixture
//! is in-process Rust, and the R7c Plugin Host channel (loopback TCP JSON-RPC)
//! will bridge through a background worker that keeps the last validated
//! snapshot, so the gpui render path never blocks on RPC.

// The shell consumes the contract types through this module (the shell never
// depends on product-domains directly — it sees the projection surface only).
// The fixture provider and the scene handle live in this module and are
// already public.
pub use openbitfun_product_domains::plugin_ui::{
    resolve_text, validate_contributions, validate_tree, PluginLocaleMap, PluginLocaleTable,
    PluginUiContributions, PluginUiDiagnostic, PluginUiDiagnosticCode, PluginUiEvent,
    PluginUiEventKind, PluginUiEventPayload, PluginUiNode, PluginUiNodeKind, PluginUiRejection,
    PluginUiSceneContribution, PluginUiSelectOption, PluginUiSlot, PluginUiTone, PluginUiTree,
    SceneRef,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Source of UI contributions. R7b ships the in-process fixture; R7c adds the
/// Plugin Host adapter.
pub trait PluginUiProvider: Send + Sync {
    /// The `ui/contributions.json` body.
    fn describe(&self) -> Result<String, String>;
    /// The package `files[]` inventory entries a scene `entry` may reference.
    fn package_files(&self) -> BTreeSet<String>;
    /// The initial tree document for a scene (`ui/scenes/<id>.json` body).
    fn tree(&self, scene_id: &str) -> Result<String, String>;
    /// Handle an interaction. `Ok(None)` keeps the current tree; `Ok(Some)`
    /// carries the replacement tree document.
    fn dispatch(
        &self,
        scene_id: &str,
        action: &str,
        payload: Option<&PluginUiEventPayload>,
    ) -> Result<Option<String>, String>;
}

/// Why a scene is not available. Rendered as an explicit in-tab state (never
/// silently swallowed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginUiSceneGate {
    /// The scene ref no longer resolves: the plugin was uninstalled or the
    /// scene retracted. Open tabs show "contribution removed" and disable the
    /// event loop; the tab stays until the user closes it.
    ContributionRemoved,
    /// The provider failed to produce a document.
    ProviderError(String),
    /// The document violated the schema/limits; carries the typed diagnostics.
    Rejected(PluginUiRejection),
}

impl PluginUiSceneGate {
    /// The first diagnostic code, for tests and log lines.
    pub fn diagnostic(&self) -> Option<&PluginUiDiagnostic> {
        match self {
            Self::Rejected(rejection) => Some(rejection.first()),
            _ => None,
        }
    }
}

/// Result of a dispatched interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginUiDispatchOutcome {
    /// The provider kept the current tree.
    Unchanged,
    /// A new tree passed validation and replaced the cached one.
    TreeReplaced,
}

struct InstalledPluginUi {
    source_key: String,
    provider: Arc<dyn PluginUiProvider>,
    contributions: PluginUiContributions,
    /// Scene id → scene slot. Slots are never reused within a projection, so
    /// a stale [`SceneRef`] after reinstall resolves to the new scene, not a
    /// look-alike of the retracted one.
    scene_slots: BTreeMap<String, u32>,
    trees: BTreeMap<u32, PluginUiTree>,
}

/// One handle for the shell's scene table: the ref plus its contribution row.
#[derive(Debug, Clone)]
pub struct PluginUiSceneHandle {
    pub scene_ref: SceneRef,
    pub contribution: PluginUiSceneContribution,
}

/// The projection owner: installs providers, assigns scene refs, validates
/// every document, and serves the current tree per scene.
#[derive(Default)]
pub struct PluginUiProjection {
    plugins: BTreeMap<u32, InstalledPluginUi>,
    /// Monotonic; incremented on every install/uninstall.
    generation: u64,
    next_plugin_slot: u32,
    next_scene_slot: u32,
    allowed_icons: BTreeSet<String>,
}

impl PluginUiProjection {
    /// `allowed_icons` is the closed icon set of the rendering surface (the
    /// shell derives it from its asset icon enum).
    pub fn new(allowed_icons: BTreeSet<String>) -> Self {
        Self {
            allowed_icons,
            ..Self::default()
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Installs one plugin's UI contributions. Fails (and installs nothing)
    /// when the manifest does not validate; diagnostics from skipped v1
    /// contribution tables stay attached to the contributions.
    pub fn install(
        &mut self,
        source_key: &str,
        provider: Arc<dyn PluginUiProvider>,
    ) -> Result<Vec<PluginUiSceneHandle>, PluginUiSceneGate> {
        let document = provider
            .describe()
            .map_err(PluginUiSceneGate::ProviderError)?;
        let files = provider.package_files();
        let contributions = openbitfun_product_domains::plugin_ui::validate_contributions(
            &document,
            &self.allowed_icons,
            &files,
        )
        .map_err(PluginUiSceneGate::Rejected)?;

        let plugin_slot = self.next_plugin_slot;
        self.next_plugin_slot += 1;
        let mut scene_slots = BTreeMap::new();
        let mut handles = Vec::new();
        for contribution in &contributions.scenes {
            let scene_slot = self.next_scene_slot;
            self.next_scene_slot += 1;
            scene_slots.insert(contribution.scene_id.clone(), scene_slot);
            handles.push(PluginUiSceneHandle {
                scene_ref: SceneRef::new(plugin_slot, scene_slot),
                contribution: contribution.clone(),
            });
        }

        self.plugins.insert(
            plugin_slot,
            InstalledPluginUi {
                source_key: source_key.to_string(),
                provider,
                contributions,
                scene_slots,
                trees: BTreeMap::new(),
            },
        );
        self.generation += 1;
        Ok(handles)
    }

    /// Uninstalls the named plugin. Open scene refs stop resolving
    /// ([`PluginUiSceneGate::ContributionRemoved`]); the slots are not reused.
    pub fn uninstall(&mut self, source_key: &str) -> bool {
        let removed = self
            .plugins
            .iter()
            .find(|(_, installed)| installed.source_key == source_key)
            .map(|(slot, _)| *slot);
        if let Some(slot) = removed {
            self.plugins.remove(&slot);
            self.generation += 1;
            true
        } else {
            false
        }
    }

    /// Every installed scene for the shell's scene table.
    pub fn scenes(&self) -> Vec<PluginUiSceneHandle> {
        let mut handles = Vec::new();
        for (plugin_slot, installed) in &self.plugins {
            for contribution in &installed.contributions.scenes {
                let scene_slot = installed.scene_slots[&contribution.scene_id];
                handles.push(PluginUiSceneHandle {
                    scene_ref: SceneRef::new(*plugin_slot, scene_slot),
                    contribution: contribution.clone(),
                });
            }
        }
        handles
    }

    /// The copy table of the owning plugin, for `@key` resolution at render
    /// time. A dangling ref yields an empty table (the tab shows the removed
    /// state instead of rendering).
    pub fn locales(
        &self,
        scene_ref: SceneRef,
    ) -> openbitfun_product_domains::plugin_ui::PluginLocaleTable {
        self.installed(scene_ref)
            .map(|installed| installed.contributions.locales.clone())
            .unwrap_or_default()
    }

    /// The current tree of a scene, pulling it through the provider on first
    /// open. The validated tree is cached; later dispatches replace it whole.
    pub fn tree(&mut self, scene_ref: SceneRef) -> Result<&PluginUiTree, PluginUiSceneGate> {
        if !self.has_tree(scene_ref)? {
            self.fetch_tree(scene_ref)?;
        }
        self.plugins
            .get(&scene_ref.plugin)
            .and_then(|installed| installed.trees.get(&scene_ref.scene))
            .ok_or(PluginUiSceneGate::ContributionRemoved)
    }

    /// Dispatches one interaction through the provider, validates the
    /// replacement tree, and swaps the cache. The event ref must resolve to
    /// the contributing plugin (the only arbiter of event ownership).
    pub fn dispatch(
        &mut self,
        event: &PluginUiEvent,
    ) -> Result<PluginUiDispatchOutcome, PluginUiSceneGate> {
        let scene_id = {
            let installed = self
                .plugins
                .get(&event.scene.plugin)
                .ok_or(PluginUiSceneGate::ContributionRemoved)?;
            installed
                .scene_id_of(event.scene)
                .ok_or(PluginUiSceneGate::ContributionRemoved)?
                .clone()
        };
        if let Some(payload) = &event.payload {
            payload.validate().map_err(PluginUiSceneGate::Rejected)?;
        }

        let installed = self
            .plugins
            .get(&event.scene.plugin)
            .expect("checked above");
        let response = installed
            .provider
            .dispatch(&scene_id, &event.action, event.payload.as_ref())
            .map_err(PluginUiSceneGate::ProviderError)?;

        let Some(document) = response else {
            return Ok(PluginUiDispatchOutcome::Unchanged);
        };

        // Validate before the cache sees it: over-limit or schema-mismatched
        // trees keep the previous one (degrade loudly, never break the shell).
        let locales = installed.contributions.locales.clone();
        let tree = openbitfun_product_domains::plugin_ui::validate_tree(
            &document,
            installed.contributions.schema_version,
            &locale_keys(&locales),
            &self.allowed_icons,
        )
        .map_err(PluginUiSceneGate::Rejected)?;
        let installed = self
            .plugins
            .get_mut(&event.scene.plugin)
            .expect("checked above");
        installed.trees.insert(event.scene.scene, tree);
        Ok(PluginUiDispatchOutcome::TreeReplaced)
    }

    fn installed(&self, scene_ref: SceneRef) -> Option<&InstalledPluginUi> {
        self.plugins.get(&scene_ref.plugin)
    }

    fn has_tree(&self, scene_ref: SceneRef) -> Result<bool, PluginUiSceneGate> {
        let installed = self
            .plugins
            .get(&scene_ref.plugin)
            .ok_or(PluginUiSceneGate::ContributionRemoved)?;
        installed
            .scene_id_of(scene_ref)
            .map(|_| installed.trees.contains_key(&scene_ref.scene))
            .ok_or(PluginUiSceneGate::ContributionRemoved)
    }

    fn fetch_tree(&mut self, scene_ref: SceneRef) -> Result<(), PluginUiSceneGate> {
        let (scene_id, schema_version, locales) = {
            let installed = self
                .plugins
                .get(&scene_ref.plugin)
                .ok_or(PluginUiSceneGate::ContributionRemoved)?;
            (
                installed
                    .scene_id_of(scene_ref)
                    .ok_or(PluginUiSceneGate::ContributionRemoved)?
                    .clone(),
                installed.contributions.schema_version,
                installed.contributions.locales.clone(),
            )
        };
        let installed = self.plugins.get(&scene_ref.plugin).expect("checked above");
        let document = installed
            .provider
            .tree(&scene_id)
            .map_err(PluginUiSceneGate::ProviderError)?;
        let tree = openbitfun_product_domains::plugin_ui::validate_tree(
            &document,
            schema_version,
            &locale_keys(&locales),
            &self.allowed_icons,
        )
        .map_err(PluginUiSceneGate::Rejected)?;
        let installed = self
            .plugins
            .get_mut(&scene_ref.plugin)
            .expect("checked above");
        installed.trees.insert(scene_ref.scene, tree);
        Ok(())
    }
}

fn locale_keys(
    locales: &openbitfun_product_domains::plugin_ui::PluginLocaleTable,
) -> BTreeSet<String> {
    locales.entries().keys().cloned().collect()
}

impl InstalledPluginUi {
    fn scene_id_of(&self, scene_ref: SceneRef) -> Option<&String> {
        self.scene_slots
            .iter()
            .find(|(_, slot)| **slot == scene_ref.scene)
            .map(|(id, _)| id)
    }
}

// ---------------------------------------------------------------------------
// Fixture provider (R7b): a two-control counter scene proving the contract.
// Test contribution only — never wired into product assembly.
// ---------------------------------------------------------------------------

/// In-process fixture: one `board` scene with a heading, a name input, and a
/// counter button. `dispatch` returns replacement trees so the shell's
/// full-replace state machine is exercised end to end.
#[derive(Default)]
pub struct FixturePluginUi {
    count: std::sync::Mutex<u32>,
    name: std::sync::Mutex<String>,
}

const FIXTURE_CONTRIBUTIONS: &str = r#"{
    "schemaVersion": 1,
    "locales": {
        "title": { "zh-CN": "插件看板", "en-US": "Plugin Board" },
        "btn.increment": { "zh-CN": "加一", "en-US": "Increment" },
        "name.hint": { "zh-CN": "输入名字回车", "en-US": "Type a name and press Enter" }
    },
    "scenes": [{
        "id": "board",
        "title": { "zh-CN": "插件场景", "en-US": "Plugin Scene" },
        "icon": "layout-dashboard",
        "slot": { "kind": "tab" },
        "entry": "ui/scenes/board.json"
    }],
    "cards": [],
    "panels": [],
    "actions": []
}"#;

impl FixturePluginUi {
    pub fn new_arc() -> Arc<dyn PluginUiProvider> {
        Arc::new(Self::default())
    }

    fn tree_document(&self) -> String {
        let count = self.count.lock().expect("fixture count lock").to_string();
        let name = self.name.lock().expect("fixture name lock").clone();
        format!(
            r#"{{
                "schemaVersion": 1,
                "root": {{
                    "type": "column",
                    "props": {{"gap": 3, "padding": 4}},
                    "children": [
                        {{"type": "heading", "props": {{"text": "@title", "level": 1}}}},
                        {{"type": "text", "props": {{"text": "count = {count}", "tone": "muted"}}}},
                        {{"type": "text", "props": {{"text": "name = {name}"}}}},
                        {{"type": "input", "props": {{"placeholder": "@name.hint", "value": "{name}"}},
                          "on": {{"submit": "set-name"}}}},
                        {{"type": "button", "props": {{"label": "@btn.increment", "tone": "primary"}},
                          "on": {{"press": "increment"}}}}
                    ]
                }}
            }}"#
        )
    }
}

impl PluginUiProvider for FixturePluginUi {
    fn describe(&self) -> Result<String, String> {
        Ok(FIXTURE_CONTRIBUTIONS.to_string())
    }

    fn package_files(&self) -> BTreeSet<String> {
        ["ui/contributions.json", "ui/scenes/board.json"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    fn tree(&self, scene_id: &str) -> Result<String, String> {
        match scene_id {
            "board" => Ok(self.tree_document()),
            other => Err(format!("unknown fixture scene '{other}'")),
        }
    }

    fn dispatch(
        &self,
        scene_id: &str,
        action: &str,
        payload: Option<&PluginUiEventPayload>,
    ) -> Result<Option<String>, String> {
        if scene_id != "board" {
            return Err(format!("unknown fixture scene '{scene_id}'"));
        }
        match (action, payload) {
            ("increment", _) => {
                *self.count.lock().expect("fixture count lock") += 1;
                Ok(Some(self.tree_document()))
            }
            ("set-name", Some(PluginUiEventPayload::Text(name))) => {
                *self.name.lock().expect("fixture name lock") = name.clone();
                Ok(Some(self.tree_document()))
            }
            ("set-name", None) => Ok(None),
            _ => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests: the projection state machine (install → open → dispatch → retract).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn icon_set() -> BTreeSet<String> {
        ["layout-dashboard", "refresh", "settings"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    fn board_ref(projection: &PluginUiProjection) -> SceneRef {
        let scenes = projection.scenes();
        assert_eq!(scenes.len(), 1);
        scenes[0].scene_ref
    }

    #[test]
    fn install_publishes_scenes_and_diagnostics_stay_attached() {
        let mut projection = PluginUiProjection::new(icon_set());
        let handles = projection
            .install("fixture", FixturePluginUi::new_arc())
            .expect("fixture installs");
        assert_eq!(handles.len(), 1);
        assert_eq!(handles[0].contribution.scene_id, "board");
        assert_eq!(handles[0].contribution.icon, "layout-dashboard");
        assert_eq!(projection.generation(), 1);
    }

    #[test]
    fn tree_fetch_validates_and_caches() {
        let mut projection = PluginUiProjection::new(icon_set());
        projection
            .install("fixture", FixturePluginUi::new_arc())
            .unwrap();
        let scene = board_ref(&projection);
        let tree = projection.tree(scene).expect("fixture tree resolves");
        let version = tree.schema_version;
        let children = tree.root.children.len();
        // Cached: a second read returns the same tree without provider churn.
        let again = projection.tree(scene).expect("cached tree");
        assert_eq!(again.schema_version, version);
        assert_eq!(again.root.children.len(), children);
    }

    #[test]
    fn dispatch_replaces_the_tree_through_validation() {
        let mut projection = PluginUiProjection::new(icon_set());
        projection
            .install("fixture", FixturePluginUi::new_arc())
            .unwrap();
        let scene = board_ref(&projection);
        projection.tree(scene).unwrap();

        let outcome = projection
            .dispatch(&PluginUiEvent {
                scene,
                action: "increment".to_string(),
                payload: None,
            })
            .expect("increment dispatches");
        assert_eq!(outcome, PluginUiDispatchOutcome::TreeReplaced);
        let tree = projection.tree(scene).unwrap();
        let counter = tree
            .root
            .children
            .iter()
            .find(|node| node.kind == PluginUiNodeKind::Text)
            .expect("counter text node");
        assert_eq!(counter.props.get_str("text"), Some("count = 1"));

        // A payload event flows through to the provider.
        projection
            .dispatch(&PluginUiEvent {
                scene,
                action: "set-name".to_string(),
                payload: Some(PluginUiEventPayload::Text("公子".to_string())),
            })
            .expect("set-name dispatches");
        let tree = projection.tree(scene).unwrap();
        let name = tree
            .root
            .children
            .iter()
            .find(|node| {
                node.kind == PluginUiNodeKind::Text
                    && node
                        .props
                        .get_str("text")
                        .unwrap_or_default()
                        .starts_with("name")
            })
            .expect("name text node");
        assert_eq!(name.props.get_str("text"), Some("name = 公子"));

        // An over-limit payload rejects before the provider sees it.
        let rejection = projection
            .dispatch(&PluginUiEvent {
                scene,
                action: "set-name".to_string(),
                payload: Some(PluginUiEventPayload::Text("x".repeat(4 * 1024 + 1))),
            })
            .expect_err("oversized payload must reject");
        assert_eq!(
            rejection.diagnostic().map(|diagnostic| diagnostic.code),
            Some(PluginUiDiagnosticCode::PayloadTooLarge)
        );
    }

    #[test]
    fn unknown_actions_keep_the_current_tree() {
        let mut projection = PluginUiProjection::new(icon_set());
        projection
            .install("fixture", FixturePluginUi::new_arc())
            .unwrap();
        let scene = board_ref(&projection);
        projection.tree(scene).unwrap();
        let outcome = projection
            .dispatch(&PluginUiEvent {
                scene,
                action: "no-such-action".to_string(),
                payload: None,
            })
            .expect("unknown actions are not provider errors");
        assert_eq!(outcome, PluginUiDispatchOutcome::Unchanged);
    }

    #[test]
    fn uninstall_retracts_open_scene_refs() {
        let mut projection = PluginUiProjection::new(icon_set());
        projection
            .install("fixture", FixturePluginUi::new_arc())
            .unwrap();
        let scene = board_ref(&projection);
        projection.tree(scene).unwrap();
        assert!(projection.uninstall("fixture"));
        assert_eq!(projection.generation(), 2);
        assert!(projection.scenes().is_empty());
        assert!(matches!(
            projection.tree(scene),
            Err(PluginUiSceneGate::ContributionRemoved)
        ));
        // Events into the retracted scene are refused, not dispatched.
        assert!(matches!(
            projection.dispatch(&PluginUiEvent {
                scene,
                action: "increment".to_string(),
                payload: None,
            }),
            Err(PluginUiSceneGate::ContributionRemoved)
        ));
    }

    #[test]
    fn reinstalled_plugins_get_fresh_slots() {
        let mut projection = PluginUiProjection::new(icon_set());
        projection
            .install("fixture", FixturePluginUi::new_arc())
            .unwrap();
        let first = board_ref(&projection);
        assert!(projection.uninstall("fixture"));
        projection
            .install("fixture", FixturePluginUi::new_arc())
            .expect("reinstall succeeds");
        let second = board_ref(&projection);
        assert_ne!(first, second, "slots never repeat within a projection");
    }

    /// A provider whose manifest breaks the schema is refused whole.
    struct BrokenManifestProvider;

    impl PluginUiProvider for BrokenManifestProvider {
        fn describe(&self) -> Result<String, String> {
            Ok(r#"{"schemaVersion": 2, "scenes": []}"#.to_string())
        }

        fn package_files(&self) -> BTreeSet<String> {
            BTreeSet::new()
        }

        fn tree(&self, _scene_id: &str) -> Result<String, String> {
            Ok("{}".to_string())
        }

        fn dispatch(
            &self,
            _scene_id: &str,
            _action: &str,
            _payload: Option<&PluginUiEventPayload>,
        ) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    #[test]
    fn broken_manifests_reject_the_install() {
        let mut projection = PluginUiProjection::new(icon_set());
        let gate = projection
            .install("broken", Arc::new(BrokenManifestProvider))
            .expect_err("newer schema must not install");
        assert!(matches!(gate, PluginUiSceneGate::Rejected(_)));
        assert!(projection.scenes().is_empty());
        assert_eq!(
            projection.generation(),
            0,
            "failed installs do not bump the generation"
        );
    }

    /// A provider whose tree breaks the limits refuses to open, keeping the
    /// scene reachable for diagnostics but treeless.
    struct BrokenTreeProvider;

    impl PluginUiProvider for BrokenTreeProvider {
        fn describe(&self) -> Result<String, String> {
            Ok(r#"{
                "schemaVersion": 1,
                "scenes": [{
                    "id": "board", "title": {"en-US": "B"}, "icon": "refresh",
                    "slot": {"kind": "tab"}, "entry": "ui/scenes/board.json"
                }]
            }"#
            .to_string())
        }

        fn package_files(&self) -> BTreeSet<String> {
            ["ui/scenes/board.json"]
                .into_iter()
                .map(str::to_string)
                .collect()
        }

        fn tree(&self, _scene_id: &str) -> Result<String, String> {
            // References a copy key that does not exist.
            Ok(
                r#"{"schemaVersion": 1, "root": {"type": "text", "props": {"text": "@missing"}}}"#
                    .to_string(),
            )
        }

        fn dispatch(
            &self,
            _scene_id: &str,
            _action: &str,
            _payload: Option<&PluginUiEventPayload>,
        ) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    #[test]
    fn broken_trees_surface_typed_diagnostics() {
        let mut projection = PluginUiProjection::new(icon_set());
        projection
            .install("broken-tree", Arc::new(BrokenTreeProvider))
            .expect("the manifest itself is fine");
        let scene = board_ref(&projection);
        let gate = projection.tree(scene).expect_err("tree must refuse");
        assert_eq!(
            gate.diagnostic().map(|diagnostic| diagnostic.code),
            Some(PluginUiDiagnosticCode::UnknownTextKey)
        );
    }
}
