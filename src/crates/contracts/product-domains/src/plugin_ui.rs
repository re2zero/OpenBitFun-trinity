//! Declarative plugin UI contribution contracts (R7).
//!
//! Plugins contribute scenes as pure data: a contribution manifest
//! (`ui/contributions.json`) and per-scene UI trees (`ui/scenes/*.json`). The
//! shell renders the trees with native widgets through a closed component set;
//! there is no script, expression, HTML, or theme override anywhere in this
//! contract. Validation lives here as pure functions so the projection owner
//! (core) and the shell renderer share one gate. See
//! `docs/architecture/extensions/plugin-ui-contributions-design.md` for the
//! design; field names on the wire are camelCase (`schemaVersion`).

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Schema version of the contribution manifest and UI trees this contract
/// understands. `schemaVersion` only grows; a higher value means the package
/// needs a newer host and is never guessed at.
pub const PLUGIN_UI_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Limits (design §3.4 / §4.2)
// ---------------------------------------------------------------------------

/// Maximum number of scene contributions per plugin.
pub const MAX_SCENES_PER_PLUGIN: usize = 8;
/// Maximum locale entries per plugin across all its locale maps.
pub const MAX_LOCALE_KEYS_PER_PLUGIN: usize = 128;
/// Maximum characters of one locale entry value.
pub const MAX_LOCALE_VALUE_CHARS: usize = 256;
/// Maximum locales in one locale map.
pub const MAX_LOCALES_PER_MAP: usize = 8;
/// Maximum nodes in one UI tree.
pub const MAX_TREE_NODES: usize = 512;
/// Maximum depth of one UI tree.
pub const MAX_TREE_DEPTH: usize = 16;
/// Maximum serialized size of one UI tree frame.
pub const MAX_TREE_FRAME_BYTES: usize = 256 * 1024;
/// Maximum byte length of one text property or event payload string.
pub const MAX_TEXT_BYTES: usize = 4 * 1024;
/// Maximum options in a `select` control.
pub const MAX_SELECT_OPTIONS: usize = 32;
/// Maximum length of an event action name.
pub const MAX_ACTION_NAME_BYTES: usize = 128;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Copyable reference to one contributed scene: `(plugin slot, scene slot)`.
///
/// Slots are small integers handed out by the projection owner when a
/// contribution set is registered; the string identities (plugin id, in-package
/// scene id) stay in the owner's registry. The shell passes this value through
/// its whole tab chain (`SceneId::External`), so it must be `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneRef {
    pub plugin: u32,
    pub scene: u32,
}

impl SceneRef {
    pub fn new(plugin: u32, scene: u32) -> Self {
        Self { plugin, scene }
    }
}

/// Plugin-level copy table: `key → (locale tag → text)`. Tree nodes reference
/// entries with `@key`; the host resolves the active locale through the same
/// fallback chain as [`PluginLocaleMap`]. Plugin copy never enters the shell
/// ftl catalogs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginLocaleTable {
    entries: BTreeMap<String, BTreeMap<String, String>>,
}

impl PluginLocaleTable {
    pub fn new(entries: BTreeMap<String, BTreeMap<String, String>>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &BTreeMap<String, BTreeMap<String, String>> {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn key_count(&self) -> usize {
        self.entries.len()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Resolve order (design §3.3): exact tag → same language family →
    /// `en-US` → any entry of that key.
    pub fn resolve(&self, key: &str, locale: &str) -> Option<&str> {
        let per_key = self.entries.get(key)?;
        if let Some(value) = per_key.get(locale) {
            return Some(value.as_str());
        }
        let primary = locale.split(['-', '_']).next().unwrap_or(locale);
        if let Some((_, value)) = per_key
            .iter()
            .find(|(tag, _)| tag.split(['-', '_']).next() == Some(primary))
        {
            return Some(value.as_str());
        }
        if let Some(value) = per_key.get("en-US") {
            return Some(value.as_str());
        }
        per_key.values().next().map(|value| value.as_str())
    }
}

/// Raw `locales` document shape (validated by [`validate_contributions`]).
type LocaleTableDoc = BTreeMap<String, BTreeMap<String, String>>;

/// One localized string: `locale tag → text`, as used by the scene title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginLocaleMap {
    entries: BTreeMap<String, String>,
}

impl PluginLocaleMap {
    /// Wraps a raw map after the size limits have been checked.
    pub fn new(entries: BTreeMap<String, String>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn key_count(&self) -> usize {
        self.entries.len()
    }

    /// Resolve order (design §3.3): exact tag → same language family
    /// (primary subtag match, e.g. `zh-TW` → `zh-CN`) → `en-US` → first
    /// entry. Always returns a value for a non-empty map.
    pub fn resolve(&self, locale: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if let Some(value) = self.entries.get(locale) {
            return Some(value.as_str());
        }
        let primary = locale.split(['-', '_']).next().unwrap_or(locale);
        if let Some((_, value)) = self
            .entries
            .iter()
            .find(|(tag, _)| tag.split(['-', '_']).next() == Some(primary))
        {
            return Some(value.as_str());
        }
        if let Some(value) = self.entries.get("en-US") {
            return Some(value.as_str());
        }
        self.entries.values().next().map(|value| value.as_str())
    }
}

/// Checks the plugin-level locale table against the §3.4 limits.
fn validate_locale_table(table: &LocaleTableDoc) -> Result<(), PluginUiRejection> {
    if table.len() > MAX_LOCALE_KEYS_PER_PLUGIN {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::LocaleMapTooLarge,
            format!(
                "locales: {} keys exceed the per-plugin limit of {MAX_LOCALE_KEYS_PER_PLUGIN}",
                table.len()
            ),
        ));
    }
    for (key, per_key) in table {
        if key.is_empty() || key.len() > MAX_ACTION_NAME_BYTES {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::InvalidLocaleTag,
                format!("locales: invalid copy key '{key}'"),
            ));
        }
        if per_key.len() > MAX_LOCALES_PER_MAP {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::LocaleMapTooLarge,
                format!(
                    "locales['{key}']: {} locales exceeds the limit of {MAX_LOCALES_PER_MAP}",
                    per_key.len()
                ),
            ));
        }
        for (tag, value) in per_key {
            if tag.is_empty() {
                return Err(PluginUiRejection::single(
                    PluginUiDiagnosticCode::InvalidLocaleTag,
                    format!("locales['{key}']: empty locale tag"),
                ));
            }
            if value.chars().count() > MAX_LOCALE_VALUE_CHARS {
                return Err(PluginUiRejection::single(
                    PluginUiDiagnosticCode::LocaleValueTooLong,
                    format!(
                        "locales['{key}']: value for '{tag}' exceeds {MAX_LOCALE_VALUE_CHARS} characters"
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Checks one locale map against the per-map limits (design §3.4: over-limit
/// locale maps reject the segment). The per-plugin key budget is enforced by
/// the caller summing across maps.
fn validate_locale_map(
    map: &BTreeMap<String, String>,
    what: &str,
) -> Result<(), PluginUiRejection> {
    if map.len() > MAX_LOCALES_PER_MAP {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::LocaleMapTooLarge,
            format!(
                "{what}: {} locales exceeds the limit of {MAX_LOCALES_PER_MAP}",
                map.len()
            ),
        ));
    }
    for (tag, value) in map {
        if tag.is_empty() {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::InvalidLocaleTag,
                format!("{what}: empty locale tag"),
            ));
        }
        if value.chars().count() > MAX_LOCALE_VALUE_CHARS {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::LocaleValueTooLong,
                format!("{what}: value for '{tag}' exceeds {MAX_LOCALE_VALUE_CHARS} characters"),
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Contribution manifest (ui/contributions.json)
// ---------------------------------------------------------------------------

/// Raw manifest document as it appears on the wire.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContributionsDoc {
    schema_version: u32,
    /// Plugin-level copy table referenced by tree `@key` texts (design §3.3).
    #[serde(default)]
    locales: LocaleTableDoc,
    #[serde(default)]
    scenes: Vec<SceneDoc>,
    /// Known field, not consumed in v0: a non-empty table is diagnosed and
    /// skipped without touching `scenes` (design §3.2).
    #[serde(default)]
    cards: Vec<Value>,
    #[serde(default)]
    panels: Vec<Value>,
    #[serde(default)]
    actions: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SceneDoc {
    id: String,
    title: BTreeMap<String, String>,
    icon: String,
    slot: SlotDoc,
    entry: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SlotDoc {
    /// The only v0 slot: a scene tab (host decides the final placement).
    Tab,
}

/// Validated contribution manifest.
#[derive(Debug, Clone)]
pub struct PluginUiContributions {
    pub schema_version: u32,
    pub locales: PluginLocaleTable,
    pub scenes: Vec<PluginUiSceneContribution>,
    /// Diagnostics produced while loading, currently the skipped v1
    /// contribution tables (design §3.2). They stay visible in the owning
    /// surface instead of being silently dropped.
    pub diagnostics: Vec<PluginUiDiagnostic>,
}

/// One scene contribution: identity, declarative copy, closed icon, slot
/// intent, and the tree file reference.
#[derive(Debug, Clone)]
pub struct PluginUiSceneContribution {
    /// Package-unique scene id (`^[a-z][a-z0-9-]{0,63}$`).
    pub scene_id: String,
    pub title: PluginLocaleMap,
    /// Member of the closed icon set passed to [`validate_contributions`].
    pub icon: String,
    pub slot: PluginUiSlot,
    /// Tree file path relative to the package root; must be listed in the
    /// manifest `files[]` inventory.
    pub entry: String,
}

/// Slot intent. The host owns the final placement decision (design §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginUiSlot {
    Tab,
}

/// Validates a contribution manifest.
///
/// `allowed_icons` is the closed icon set of the rendering surface and
/// `manifest_files` the sha256-locked `files[]` inventory of the package; both
/// come from the caller because this contract crate has no knowledge of icon
/// enums or package layouts.
pub fn validate_contributions(
    json: &str,
    allowed_icons: &BTreeSet<String>,
    manifest_files: &BTreeSet<String>,
) -> Result<PluginUiContributions, PluginUiRejection> {
    let mut diagnostics = Vec::new();
    let doc: ContributionsDoc = match serde_json::from_str(json) {
        Ok(doc) => doc,
        Err(err) => {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::MalformedJson,
                format!("contributions.json: {err}"),
            ));
        }
    };

    if doc.schema_version > PLUGIN_UI_SCHEMA_VERSION {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::SchemaVersionTooNew,
            format!(
                "contributions.json declares schemaVersion {}, this host understands {}",
                doc.schema_version, PLUGIN_UI_SCHEMA_VERSION
            ),
        ));
    }
    if doc.schema_version != PLUGIN_UI_SCHEMA_VERSION {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::InvalidSchemaVersion,
            format!(
                "contributions.json declares schemaVersion {}",
                doc.schema_version
            ),
        ));
    }

    // v0 consumes only `scenes`; the other tables are known fields whose host
    // wiring lands later. A non-empty table is diagnosed and skipped (the
    // scenes table is unaffected) instead of rejecting the whole segment.
    if !doc.cards.is_empty() {
        diagnostics.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::UnsupportedContributionTable,
            format!(
                "cards: {} entries skipped, not supported in this version",
                doc.cards.len()
            ),
        ));
    }
    if !doc.panels.is_empty() {
        diagnostics.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::UnsupportedContributionTable,
            format!(
                "panels: {} entries skipped, not supported in this version",
                doc.panels.len()
            ),
        ));
    }
    if !doc.actions.is_empty() {
        diagnostics.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::UnsupportedContributionTable,
            format!(
                "actions: {} entries skipped, not supported in this version",
                doc.actions.len()
            ),
        ));
    }

    if doc.scenes.len() > MAX_SCENES_PER_PLUGIN {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::SceneLimitExceeded,
            format!(
                "{} scenes exceeds the limit of {MAX_SCENES_PER_PLUGIN}",
                doc.scenes.len()
            ),
        ));
    }

    // The plugin-level copy table backs every `@key` reference in the trees;
    // validate its limits up front (design §3.4).
    validate_locale_table(&doc.locales)?;
    let locales = PluginLocaleTable::new(doc.locales);

    let mut title_keys = 0usize;
    let mut scenes = Vec::with_capacity(doc.scenes.len());
    let mut seen_ids = BTreeSet::new();
    for scene in doc.scenes {
        if !is_valid_scene_id(&scene.id) {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::InvalidSceneId,
                format!(
                    "scene id '{}' does not match ^[a-z][a-z0-9-]{{0,63}}$",
                    scene.id
                ),
            ));
        }
        if !seen_ids.insert(scene.id.clone()) {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::DuplicateSceneId,
                format!("duplicate scene id '{}'", scene.id),
            ));
        }
        validate_locale_map(&scene.title, &format!("scene '{}' title", scene.id))?;
        title_keys += scene.title.len();
        if !allowed_icons.contains(&scene.icon) {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::UnknownIcon,
                format!(
                    "scene '{}' icon '{}' is outside the closed icon set",
                    scene.id, scene.icon
                ),
            ));
        }
        if !manifest_files.contains(&scene.entry) {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::EntryNotInManifest,
                format!(
                    "scene '{}' entry '{}' is not listed in the package file inventory",
                    scene.id, scene.entry
                ),
            ));
        }
        let slot = match scene.slot {
            SlotDoc::Tab => PluginUiSlot::Tab,
        };
        scenes.push(PluginUiSceneContribution {
            scene_id: scene.id,
            title: PluginLocaleMap::new(scene.title),
            icon: scene.icon,
            slot,
            entry: scene.entry,
        });
    }
    if title_keys > MAX_LOCALE_KEYS_PER_PLUGIN {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::LocaleMapTooLarge,
            format!(
                "{} locale entries exceed the per-plugin limit of {MAX_LOCALE_KEYS_PER_PLUGIN}",
                title_keys
            ),
        ));
    }

    Ok(PluginUiContributions {
        schema_version: PLUGIN_UI_SCHEMA_VERSION,
        locales,
        scenes,
        diagnostics,
    })
}

/// Scene ids are lowercase kebab: first char a letter, then lowercase letters,
/// digits, or hyphens, up to 64 characters.
fn is_valid_scene_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    if id.len() > 64 {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// ---------------------------------------------------------------------------
// UI tree (ui/scenes/<scene_id>.json and event responses)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TreeDoc {
    schema_version: u32,
    root: NodeDoc,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeDoc {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    props: serde_json::Map<String, Value>,
    #[serde(default)]
    children: Vec<NodeDoc>,
    /// Interaction bindings: `press` / `submit` / `change` → action name.
    #[serde(default)]
    on: BTreeMap<String, String>,
}

/// Closed component set of the v0 renderer (design §5.1). Every variant has a
/// gpui-component counterpart verified against gpui-component 0.7.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginUiNodeKind {
    Column,
    Row,
    Scroll,
    Divider,
    Spacer,
    Text,
    Heading,
    Label,
    Button,
    Input,
    Textarea,
    Select,
    Checkbox,
    Switch,
    Badge,
    Tag,
    Icon,
    Card,
    Progress,
    Spinner,
    Empty,
    List,
    Group,
}

impl PluginUiNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Column => "column",
            Self::Row => "row",
            Self::Scroll => "scroll",
            Self::Divider => "divider",
            Self::Spacer => "spacer",
            Self::Text => "text",
            Self::Heading => "heading",
            Self::Label => "label",
            Self::Button => "button",
            Self::Input => "input",
            Self::Textarea => "textarea",
            Self::Select => "select",
            Self::Checkbox => "checkbox",
            Self::Switch => "switch",
            Self::Badge => "badge",
            Self::Tag => "tag",
            Self::Icon => "icon",
            Self::Card => "card",
            Self::Progress => "progress",
            Self::Spinner => "spinner",
            Self::Empty => "empty",
            Self::List => "list",
            Self::Group => "group",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "column" => Self::Column,
            "row" => Self::Row,
            "scroll" => Self::Scroll,
            "divider" => Self::Divider,
            "spacer" => Self::Spacer,
            "text" => Self::Text,
            "heading" => Self::Heading,
            "label" => Self::Label,
            "button" => Self::Button,
            "input" => Self::Input,
            "textarea" => Self::Textarea,
            "select" => Self::Select,
            "checkbox" => Self::Checkbox,
            "switch" => Self::Switch,
            "badge" => Self::Badge,
            "tag" => Self::Tag,
            "icon" => Self::Icon,
            "card" => Self::Card,
            "progress" => Self::Progress,
            "spinner" => Self::Spinner,
            "empty" => Self::Empty,
            "list" => Self::List,
            "group" => Self::Group,
            _ => return None,
        })
    }
}

/// Semantic color roles a node may reference (design §3.3 / §5.2). Raw color
/// values do not exist in this contract; the renderer maps roles onto
/// `cx.theme()` tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginUiTone {
    Primary,
    Accent,
    Text,
    Muted,
    Danger,
    Surface,
    Border,
}

impl PluginUiTone {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "primary" => Self::Primary,
            "accent" => Self::Accent,
            "text" => Self::Text,
            "muted" => Self::Muted,
            "danger" => Self::Danger,
            "surface" => Self::Surface,
            "border" => Self::Border,
            _ => return None,
        })
    }
}

/// Interaction events a node can bind (design §5.4). Hover, focus, scroll, and
/// keyboard shortcuts are host-owned and not exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PluginUiEventKind {
    Press,
    Submit,
    Change,
}

impl PluginUiEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Submit => "submit",
            Self::Change => "change",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "press" => Self::Press,
            "submit" => Self::Submit,
            "change" => Self::Change,
            _ => return None,
        })
    }
}

/// One select option after validation.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginUiSelectOption {
    pub value: String,
    /// Display text, possibly an `@key` reference resolved by the renderer.
    pub label: String,
}

/// A validated UI tree node. Props stay as a JSON map (the renderer reads them
/// through typed accessors) but every entry has already passed the per-component
/// whitelist, so the renderer can trust shapes and never rejects at render time.
#[derive(Debug, Clone)]
pub struct PluginUiNode {
    pub kind: PluginUiNodeKind,
    pub props: PluginUiProps,
    pub children: Vec<PluginUiNode>,
    pub on: BTreeMap<PluginUiEventKind, String>,
}

impl PluginUiNode {
    /// Resolves the node's text for `name`: `@key` references go through the
    /// plugin copy table, literals pass through.
    pub fn text(&self, name: &str, locales: &PluginLocaleTable, locale: &str) -> Option<String> {
        let raw = self.props.get_str(name)?;
        Some(resolve_text(raw, locales, locale))
    }
}

/// Resolves one text value: a leading `@` marks a copy-table key; anything
/// else is a literal.
pub fn resolve_text(raw: &str, locales: &PluginLocaleTable, locale: &str) -> String {
    match raw.strip_prefix('@') {
        Some(key) => locales.resolve(key, locale).unwrap_or_default().to_string(),
        None => raw.to_string(),
    }
}

/// Validated props of one node.
#[derive(Debug, Clone, Default)]
pub struct PluginUiProps {
    map: serde_json::Map<String, Value>,
    options: Vec<PluginUiSelectOption>,
}

impl PluginUiProps {
    pub fn get_str(&self, name: &str) -> Option<&str> {
        self.map.get(name).and_then(|value| value.as_str())
    }

    pub fn get_u32(&self, name: &str) -> Option<u32> {
        self.map
            .get(name)
            .and_then(|value| value.as_u64())
            .map(|value| value as u32)
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.map.get(name).and_then(|value| value.as_bool())
    }

    pub fn get_tone(&self, name: &str) -> Option<PluginUiTone> {
        self.get_str(name).and_then(PluginUiTone::parse)
    }

    pub fn options(&self) -> &[PluginUiSelectOption] {
        &self.options
    }
}

/// A validated UI tree.
#[derive(Debug, Clone)]
pub struct PluginUiTree {
    pub schema_version: u32,
    pub root: PluginUiNode,
}

/// Validates one UI tree document against the tree limits, the closed
/// component set, per-component prop whitelists, and the locale map of the
/// owning plugin (for `@key` closure). `expected_schema_version` must match
/// the tree's own `schemaVersion` (design §4.2: a mismatch rejects the tree).
pub fn validate_tree(
    json: &str,
    expected_schema_version: u32,
    locale_keys: &BTreeSet<String>,
    allowed_icons: &BTreeSet<String>,
) -> Result<PluginUiTree, PluginUiRejection> {
    if json.len() > MAX_TREE_FRAME_BYTES {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::TreeTooLarge,
            format!(
                "tree frame of {} bytes exceeds the {MAX_TREE_FRAME_BYTES}-byte limit",
                json.len()
            ),
        ));
    }
    let doc: TreeDoc = match serde_json::from_str(json) {
        Ok(doc) => doc,
        Err(err) => {
            return Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::MalformedJson,
                format!("tree: {err}"),
            ));
        }
    };
    if doc.schema_version != expected_schema_version {
        return Err(PluginUiRejection::single(
            PluginUiDiagnosticCode::SchemaVersionMismatch,
            format!(
                "tree schemaVersion {} does not match the contribution schemaVersion {expected_schema_version}",
                doc.schema_version
            ),
        ));
    }
    let mut diagnostics = Vec::new();
    let mut state = TreeCheckState {
        node_count: 0,
        locale_keys,
        allowed_icons,
    };
    let root = validate_node(&doc.root, 0, &mut state, &mut diagnostics);
    if state.node_count > MAX_TREE_NODES {
        diagnostics.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::TreeTooLarge,
            format!(
                "{} nodes exceeds the limit of {MAX_TREE_NODES}",
                state.node_count
            ),
        ));
    }
    if diagnostics.is_empty() {
        Ok(PluginUiTree {
            schema_version: doc.schema_version,
            root,
        })
    } else {
        Err(PluginUiRejection { diagnostics })
    }
}

struct TreeCheckState<'a> {
    node_count: usize,
    locale_keys: &'a BTreeSet<String>,
    allowed_icons: &'a BTreeSet<String>,
}

fn validate_node(
    doc: &NodeDoc,
    depth: usize,
    state: &mut TreeCheckState,
    out: &mut Vec<PluginUiDiagnostic>,
) -> PluginUiNode {
    let fail = |code: PluginUiDiagnosticCode, detail: String, out: &mut Vec<PluginUiDiagnostic>| {
        out.push(PluginUiDiagnostic::new(code, detail));
        PluginUiNode {
            kind: PluginUiNodeKind::Group,
            props: PluginUiProps::default(),
            children: Vec::new(),
            on: BTreeMap::new(),
        }
    };

    if depth > MAX_TREE_DEPTH {
        return fail(
            PluginUiDiagnosticCode::TreeTooDeep,
            format!("tree depth exceeds {MAX_TREE_DEPTH}"),
            out,
        );
    }
    state.node_count += 1;

    let Some(kind) = PluginUiNodeKind::parse(&doc.kind) else {
        return fail(
            PluginUiDiagnosticCode::UnknownComponent,
            format!("unknown component type '{}'", doc.kind),
            out,
        );
    };

    let mut props = PluginUiProps::default();
    validate_props(kind, &doc.props, state, &mut props, out);

    let mut on: BTreeMap<PluginUiEventKind, String> = BTreeMap::new();
    // Event bindings: only the events a component supports, with well-formed
    // action names (design §5.4).
    for (event, action) in &doc.on {
        let Some(event_kind) = PluginUiEventKind::parse(event) else {
            out.push(PluginUiDiagnostic::new(
                PluginUiDiagnosticCode::UnknownEventKind,
                format!("component '{}': unknown event '{event}'", kind.as_str()),
            ));
            continue;
        };
        if !component_supports_event(kind, event_kind) {
            out.push(PluginUiDiagnostic::new(
                PluginUiDiagnosticCode::UnsupportedEventBinding,
                format!(
                    "component '{}' does not support '{}'",
                    kind.as_str(),
                    event_kind.as_str()
                ),
            ));
            continue;
        }
        if !is_valid_action_name(action) {
            out.push(PluginUiDiagnostic::new(
                PluginUiDiagnosticCode::InvalidActionName,
                format!("component '{}': invalid action name", kind.as_str()),
            ));
            continue;
        }
        props_check_text_length(action, out, || {
            format!(
                "component '{}': action name exceeds {MAX_TEXT_BYTES} bytes",
                kind.as_str()
            )
        });
        on.insert(event_kind, action.clone());
    }

    // Children arity (design §5.1).
    let max_children = match kind {
        PluginUiNodeKind::Scroll => 1,
        PluginUiNodeKind::Column
        | PluginUiNodeKind::Row
        | PluginUiNodeKind::Card
        | PluginUiNodeKind::Group
        | PluginUiNodeKind::List => usize::MAX,
        _ => 0,
    };
    if doc.children.len() > max_children {
        out.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::InvalidChildren,
            format!(
                "component '{}' accepts at most {} children, got {}",
                kind.as_str(),
                max_children,
                doc.children.len()
            ),
        ));
    }

    let children = doc
        .children
        .iter()
        .map(|child| validate_node(child, depth + 1, state, out))
        .collect();

    PluginUiNode {
        kind,
        props,
        children,
        on,
    }
}

fn props_check_text_length(
    value: &str,
    out: &mut Vec<PluginUiDiagnostic>,
    detail: impl FnOnce() -> String,
) {
    if value.len() > MAX_TEXT_BYTES {
        out.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::TextTooLong,
            detail(),
        ));
    }
}

/// Per-component prop whitelist (design §4.1 "whitelisted props, validated per
/// component"). Unknown names and wrong shapes reject the node.
fn validate_props(
    kind: PluginUiNodeKind,
    doc: &serde_json::Map<String, Value>,
    state: &TreeCheckState,
    props: &mut PluginUiProps,
    out: &mut Vec<PluginUiDiagnostic>,
) {
    let what = kind.as_str();
    for (name, value) in doc {
        let allowed = matches!(
            (kind, name.as_str()),
            (
                PluginUiNodeKind::Column | PluginUiNodeKind::Row | PluginUiNodeKind::Group,
                "gap" | "padding"
            ) | (PluginUiNodeKind::Spacer, "size")
                | (PluginUiNodeKind::Text, "text" | "tone" | "truncate")
                | (PluginUiNodeKind::Heading, "text" | "level")
                | (PluginUiNodeKind::Label, "text" | "tone")
                | (PluginUiNodeKind::Button, "label" | "tone" | "disabled")
                | (
                    PluginUiNodeKind::Input | PluginUiNodeKind::Textarea,
                    "placeholder" | "value" | "disabled"
                )
                | (PluginUiNodeKind::Select, "options" | "value" | "disabled")
                | (PluginUiNodeKind::Checkbox, "label" | "checked")
                | (PluginUiNodeKind::Switch, "checked")
                | (
                    PluginUiNodeKind::Badge | PluginUiNodeKind::Tag,
                    "label" | "tone"
                )
                | (PluginUiNodeKind::Icon, "name" | "size")
                | (PluginUiNodeKind::Card, "padding")
                | (PluginUiNodeKind::Progress, "value")
                | (PluginUiNodeKind::Spinner, "size")
                | (PluginUiNodeKind::Empty, "title" | "description")
        );
        if !allowed {
            out.push(PluginUiDiagnostic::new(
                PluginUiDiagnosticCode::UnknownProp,
                format!("component '{what}': unknown prop '{name}'"),
            ));
            continue;
        }
        match (name.as_str(), value) {
            ("gap" | "padding" | "size", Value::Number(n)) => match n.as_u64() {
                Some(n) => {
                    props.map.insert(name.clone(), Value::from(n as u32));
                }
                None => out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': prop '{name}' must be a non-negative integer"),
                )),
            },
            ("level", Value::Number(n)) => match n.as_u64() {
                Some(n @ 1..=3) => {
                    props.map.insert(name.clone(), Value::from(n as u32));
                }
                _ => out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': level must be 1-3"),
                )),
            },
            ("value", Value::Number(n)) if kind == PluginUiNodeKind::Progress => match n.as_u64() {
                Some(n @ 0..=100) => {
                    props.map.insert(name.clone(), Value::from(n as u32));
                }
                _ => out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': progress value must be 0-100"),
                )),
            },
            ("value", _) if kind == PluginUiNodeKind::Select => match value.as_str() {
                Some(value) => {
                    props_check_text_length(value, out, || {
                        format!("component '{what}': select value exceeds {MAX_TEXT_BYTES} bytes")
                    });
                    props.map.insert(name.clone(), Value::from(value));
                }
                None => out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': select value must be a string"),
                )),
            },
            // input/textarea control values are plain strings (not locale
            // references): the tree's value is the control state itself.
            ("value", Value::String(text)) => {
                props_check_text_length(text, out, || {
                    format!("component '{what}': prop 'value' exceeds {MAX_TEXT_BYTES} bytes")
                });
                props.map.insert(name.clone(), Value::from(text.as_str()));
            }
            ("options", Value::Array(items)) => {
                if items.len() > MAX_SELECT_OPTIONS {
                    out.push(PluginUiDiagnostic::new(
                        PluginUiDiagnosticCode::InvalidPropValue,
                        format!(
                            "component '{what}': {} options exceed the limit of {MAX_SELECT_OPTIONS}",
                            items.len()
                        ),
                    ));
                    continue;
                }
                let mut options = Vec::with_capacity(items.len());
                for item in items {
                    let Some(obj) = item.as_object() else {
                        out.push(PluginUiDiagnostic::new(
                            PluginUiDiagnosticCode::InvalidPropValue,
                            format!("component '{what}': options entries must be objects"),
                        ));
                        options.clear();
                        break;
                    };
                    let (Some(value), Some(label)) = (
                        obj.get("value").and_then(Value::as_str),
                        obj.get("label").and_then(Value::as_str),
                    ) else {
                        out.push(PluginUiDiagnostic::new(
                            PluginUiDiagnosticCode::InvalidPropValue,
                            format!("component '{what}': option entries need string 'value' and 'label'"),
                        ));
                        options.clear();
                        break;
                    };
                    if obj.len() != 2 {
                        out.push(PluginUiDiagnostic::new(
                            PluginUiDiagnosticCode::InvalidPropValue,
                            format!("component '{what}': option entries accept only 'value' and 'label'"),
                        ));
                        options.clear();
                        break;
                    }
                    props_check_text_length(value, out, || {
                        format!("component '{what}': option value exceeds {MAX_TEXT_BYTES} bytes")
                    });
                    props_check_text_length(label, out, || {
                        format!("component '{what}': option label exceeds {MAX_TEXT_BYTES} bytes")
                    });
                    check_text_reference(label, what, state, out);
                    options.push(PluginUiSelectOption {
                        value: value.to_string(),
                        label: label.to_string(),
                    });
                }
                if !options.is_empty() || items.is_empty() {
                    props.options = options;
                }
            }
            ("checked" | "disabled" | "truncate", Value::Bool(flag)) => {
                props.map.insert(name.clone(), Value::from(*flag));
            }
            ("checked" | "disabled" | "truncate", _) => {
                out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': prop '{name}' must be a boolean"),
                ));
            }
            ("tone", Value::String(tone)) => {
                if PluginUiTone::parse(tone).is_none() {
                    out.push(PluginUiDiagnostic::new(
                        PluginUiDiagnosticCode::InvalidPropValue,
                        format!("component '{what}': unknown tone '{tone}'"),
                    ));
                    continue;
                }
                props.map.insert(name.clone(), Value::from(tone.as_str()));
            }
            ("tone", _) => {
                out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': tone must be a string"),
                ));
            }
            ("name", Value::String(icon)) if kind == PluginUiNodeKind::Icon => {
                props_check_text_length(icon, out, || {
                    format!("component '{what}': icon name exceeds {MAX_TEXT_BYTES} bytes")
                });
                if !state.allowed_icons.contains(icon) {
                    out.push(PluginUiDiagnostic::new(
                        PluginUiDiagnosticCode::UnknownIcon,
                        format!("component '{what}': icon '{icon}' is outside the closed icon set"),
                    ));
                    continue;
                }
                props.map.insert(name.clone(), Value::from(icon.as_str()));
            }
            (text_name, Value::String(text))
                if matches!(
                    text_name,
                    "text" | "label" | "placeholder" | "title" | "description"
                ) =>
            {
                props_check_text_length(text, out, || {
                    format!("component '{what}': prop '{text_name}' exceeds {MAX_TEXT_BYTES} bytes")
                });
                check_text_reference(text, what, state, out);
                props.map.insert(name.clone(), Value::from(text.as_str()));
            }
            (_, _) => {
                out.push(PluginUiDiagnostic::new(
                    PluginUiDiagnosticCode::InvalidPropValue,
                    format!("component '{what}': prop '{name}' has the wrong shape"),
                ));
            }
        }
    }
}

/// `@key` references must exist in the plugin's copy table (design §4.1:
/// closure check); literals pass through.
fn check_text_reference(
    text: &str,
    what: &str,
    state: &TreeCheckState,
    out: &mut Vec<PluginUiDiagnostic>,
) {
    let Some(key) = text.strip_prefix('@') else {
        return;
    };
    if key.is_empty() || !state.locale_keys.contains(key) {
        out.push(PluginUiDiagnostic::new(
            PluginUiDiagnosticCode::UnknownTextKey,
            format!("component '{what}': text reference '{text}' is not in the plugin locale map"),
        ));
    }
}

fn component_supports_event(kind: PluginUiNodeKind, event: PluginUiEventKind) -> bool {
    matches!(
        (kind, event),
        (PluginUiNodeKind::Button, PluginUiEventKind::Press)
            | (
                PluginUiNodeKind::Input | PluginUiNodeKind::Textarea,
                PluginUiEventKind::Submit
            )
            | (
                PluginUiNodeKind::Select | PluginUiNodeKind::Checkbox | PluginUiNodeKind::Switch,
                PluginUiEventKind::Change
            )
    )
}

fn is_valid_action_name(action: &str) -> bool {
    !action.is_empty() && action.len() <= MAX_ACTION_NAME_BYTES
}

// ---------------------------------------------------------------------------
// Events (shell → provider round trip)
// ---------------------------------------------------------------------------

/// Control values only: no files, no arbitrary JSON trees (design §4.3).
#[derive(Debug, Clone, PartialEq)]
pub enum PluginUiEventPayload {
    Text(String),
    Number(f64),
    Bool(bool),
}

impl PluginUiEventPayload {
    pub fn validate(&self) -> Result<(), PluginUiRejection> {
        match self {
            Self::Text(text) if text.len() > MAX_TEXT_BYTES => Err(PluginUiRejection::single(
                PluginUiDiagnosticCode::PayloadTooLarge,
                format!("payload string exceeds {MAX_TEXT_BYTES} bytes"),
            )),
            _ => Ok(()),
        }
    }
}

/// One interaction dispatched back to the provider. `scene` must belong to the
/// contributing plugin; the projection owner is the only arbiter of that.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginUiEvent {
    pub scene: SceneRef,
    pub action: String,
    pub payload: Option<PluginUiEventPayload>,
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Typed rejection reasons. Every one must surface in the owning scene tab
/// (design §4.2: diagnostics are visible, never silently swallowed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginUiDiagnosticCode {
    MalformedJson,
    InvalidSchemaVersion,
    SchemaVersionTooNew,
    SchemaVersionMismatch,
    SceneLimitExceeded,
    InvalidSceneId,
    DuplicateSceneId,
    LocaleMapTooLarge,
    LocaleValueTooLong,
    InvalidLocaleTag,
    UnknownIcon,
    EntryNotInManifest,
    UnsupportedContributionTable,
    UnknownComponent,
    TreeTooLarge,
    TreeTooDeep,
    TextTooLong,
    UnknownTextKey,
    UnknownProp,
    InvalidPropValue,
    InvalidChildren,
    UnknownEventKind,
    UnsupportedEventBinding,
    InvalidActionName,
    PayloadTooLarge,
}

/// A single typed diagnostic with human-readable detail (English, log-safe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUiDiagnostic {
    pub code: PluginUiDiagnosticCode,
    pub detail: String,
}

impl PluginUiDiagnostic {
    pub fn new(code: PluginUiDiagnosticCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

/// A validation rejection carrying all collected diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUiRejection {
    pub diagnostics: Vec<PluginUiDiagnostic>,
}

impl PluginUiRejection {
    fn single(code: PluginUiDiagnosticCode, detail: impl Into<String>) -> Self {
        Self {
            diagnostics: vec![PluginUiDiagnostic::new(code, detail)],
        }
    }

    pub fn first(&self) -> &PluginUiDiagnostic {
        &self.diagnostics[0]
    }
}

// ---------------------------------------------------------------------------
// Tests: the validation matrix (design §6 step 1)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use PluginUiDiagnosticCode as Code;

    fn icon_set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn files(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    const ICONS: &[&str] = &["layout-dashboard", "refresh", "settings"];
    const TREE_ENTRY: &str = "ui/scenes/board.json";

    fn contributions_json(scenes_json: &str) -> String {
        format!(
            r#"{{
                "schemaVersion": 1,
                "locales": {{
                    "title": {{"zh-CN": "看板", "en-US": "Board"}},
                    "btn.refresh": {{"zh-CN": "刷新", "en-US": "Refresh"}}
                }},
                "scenes": {scenes_json},
                "cards": [],
                "panels": [],
                "actions": []
            }}"#
        )
    }

    fn one_scene(scenes_extra: &str) -> String {
        contributions_json(&format!(
            r#"[{{
                "id": "board",
                "title": {{"zh-CN": "看板", "en-US": "Board"}},
                "icon": "layout-dashboard",
                "slot": {{"kind": "tab"}},
                "entry": "{TREE_ENTRY}"
            }}{scenes_extra}]"#
        ))
    }

    fn valid_contributions() -> PluginUiContributions {
        validate_contributions(&one_scene(""), &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect("fixture contributions validate")
    }

    fn tree_json(root: &str) -> String {
        format!(r#"{{"schemaVersion": 1, "root": {root}}}"#)
    }

    fn locale_keys() -> BTreeSet<String> {
        ["title", "btn.refresh", "greeting"]
            .iter()
            .map(|key| key.to_string())
            .collect()
    }

    fn copy_table() -> PluginLocaleTable {
        let mut entries = BTreeMap::new();
        let mut title = BTreeMap::new();
        title.insert("zh-CN".to_string(), "看板".to_string());
        title.insert("en-US".to_string(), "Board".to_string());
        entries.insert("title".to_string(), title);
        PluginLocaleTable::new(entries)
    }

    fn valid_tree() -> PluginUiTree {
        validate_tree(
            &tree_json(
                r#"{"type": "column", "props": {"gap": 2}, "children": [
                    {"type": "heading", "props": {"text": "@title", "level": 2}},
                    {"type": "button", "props": {"label": "@btn.refresh", "tone": "primary"},
                     "on": {"press": "refresh"}}
                ]}"#,
            ),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect("fixture tree validates")
    }

    fn code_of(rejection: &PluginUiRejection) -> PluginUiDiagnosticCode {
        rejection.first().code
    }

    #[test]
    fn contributions_parse_and_expose_scenes() {
        let contributions = valid_contributions();
        assert_eq!(contributions.schema_version, 1);
        assert_eq!(contributions.scenes.len(), 1);
        assert_eq!(contributions.locales.key_count(), 2);
        assert!(contributions.locales.contains_key("btn.refresh"));
        let scene = &contributions.scenes[0];
        assert_eq!(scene.scene_id, "board");
        assert_eq!(scene.slot, PluginUiSlot::Tab);
        assert_eq!(scene.entry, TREE_ENTRY);
        assert_eq!(scene.title.resolve("zh-TW"), Some("看板"));
        assert_eq!(scene.title.resolve("en-US"), Some("Board"));
        assert!(contributions.diagnostics.is_empty());
    }

    #[test]
    fn contributions_reject_unknown_fields() {
        let json = r#"{"schemaVersion": 1, "scenes": [], "widgets": []}"#;
        let rejection = validate_contributions(json, &icon_set(ICONS), &files(&[]))
            .expect_err("unknown field must reject");
        assert_eq!(code_of(&rejection), Code::MalformedJson);
    }

    #[test]
    fn contributions_reject_newer_schema_version() {
        let json = r#"{"schemaVersion": 2, "scenes": []}"#;
        let rejection = validate_contributions(json, &icon_set(ICONS), &files(&[]))
            .expect_err("newer schema must reject");
        assert_eq!(code_of(&rejection), Code::SchemaVersionTooNew);
    }

    #[test]
    fn contributions_reject_scene_limit() {
        let scenes: Vec<String> = (0..MAX_SCENES_PER_PLUGIN + 1)
            .map(|index| {
                format!(
                    r#"{{"id": "scene-{index}", "title": {{"en-US": "S"}}, "icon": "refresh",
                        "slot": {{"kind": "tab"}}, "entry": "{TREE_ENTRY}"}}"#
                )
            })
            .collect();
        let json = contributions_json(&format!("[{}]", scenes.join(",")));
        let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect_err("scene limit must reject");
        assert_eq!(code_of(&rejection), Code::SceneLimitExceeded);
    }

    #[test]
    fn contributions_reject_invalid_scene_ids() {
        for id in ["Board", "1board", "-board", "board_x", "", "BOARD"] {
            let json = contributions_json(&format!(
                r#"[{{"id": "{id}", "title": {{"en-US": "S"}}, "icon": "refresh",
                    "slot": {{"kind": "tab"}}, "entry": "{TREE_ENTRY}"}}]"#
            ));
            let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
                .expect_err("invalid scene id must reject");
            assert_eq!(code_of(&rejection), Code::InvalidSceneId, "id: {id:?}");
        }
    }

    #[test]
    fn contributions_reject_duplicate_scene_ids() {
        let scene = r#"{"id": "board", "title": {"en-US": "S"}, "icon": "refresh",
            "slot": {"kind": "tab"}, "entry": "ui/scenes/board.json"}"#;
        let json = contributions_json(&format!("[{scene},{scene}]"));
        let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect_err("duplicate ids must reject");
        assert_eq!(code_of(&rejection), Code::DuplicateSceneId);
    }

    #[test]
    fn contributions_reject_icons_outside_the_closed_set() {
        let json = contributions_json(&format!(
            r#"[{{"id": "board", "title": {{"en-US": "S"}}, "icon": "rocket",
                "slot": {{"kind": "tab"}}, "entry": "{TREE_ENTRY}"}}]"#
        ));
        let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect_err("unknown icon must reject");
        assert_eq!(code_of(&rejection), Code::UnknownIcon);
    }

    #[test]
    fn contributions_reject_entries_missing_from_the_file_inventory() {
        let rejection =
            validate_contributions(&one_scene(""), &icon_set(ICONS), &files(&["other.json"]))
                .expect_err("unlisted entry must reject");
        assert_eq!(code_of(&rejection), Code::EntryNotInManifest);
    }

    #[test]
    fn contributions_skip_unsupported_tables_with_diagnostics_but_keep_scenes() {
        let json = r#"{
            "schemaVersion": 1,
            "scenes": [{
                "id": "board", "title": {"en-US": "Board"}, "icon": "layout-dashboard",
                "slot": {"kind": "tab"}, "entry": "ui/scenes/board.json"
            }],
            "cards": [{"slot": "workbench"}],
            "actions": [{"action": "do"}]
        }"#;
        let contributions = validate_contributions(json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect("scenes survive unsupported tables");
        assert_eq!(contributions.scenes.len(), 1);
        assert_eq!(contributions.diagnostics.len(), 2);
        assert!(contributions
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code == Code::UnsupportedContributionTable));
    }

    #[test]
    fn contributions_reject_locale_budget_breaches() {
        // More than 8 locales in one title map rejects the segment (§3.4).
        let locales: serde_json::Map<String, Value> = (0..MAX_LOCALES_PER_MAP + 1)
            .map(|index| (format!("en-{index:03}"), Value::from("x")))
            .collect();
        let json = format!(
            r#"{{"schemaVersion": 1, "scenes": [{{"id": "board",
                "title": {}, "icon": "refresh", "slot": {{"kind": "tab"}},
                "entry": "{TREE_ENTRY}"}}]}}"#,
            Value::Object(locales)
        );
        let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect_err("locale map over the per-map limit must reject");
        assert_eq!(code_of(&rejection), Code::LocaleMapTooLarge);

        // One over-long value rejects the segment.
        let long = "x".repeat(MAX_LOCALE_VALUE_CHARS + 1);
        let json = contributions_json(&format!(
            r#"[{{"id": "board", "title": {{"en-US": "{long}"}}, "icon": "refresh",
                "slot": {{"kind": "tab"}}, "entry": "{TREE_ENTRY}"}}]"#
        ));
        let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[TREE_ENTRY]))
            .expect_err("over-long locale value must reject");
        assert_eq!(code_of(&rejection), Code::LocaleValueTooLong);

        // A copy table over the per-plugin 128-key budget rejects.
        let mut keys = serde_json::Map::new();
        for index in 0..=MAX_LOCALE_KEYS_PER_PLUGIN {
            keys.insert(
                format!("key-{index:03}"),
                serde_json::from_str(r#"{"en-US": "x"}"#).unwrap(),
            );
        }
        let json = format!(
            r#"{{"schemaVersion": 1, "locales": {}, "scenes": []}}"#,
            Value::Object(keys)
        );
        let rejection = validate_contributions(&json, &icon_set(ICONS), &files(&[]))
            .expect_err("copy table over the key budget must reject");
        assert_eq!(code_of(&rejection), Code::LocaleMapTooLarge);
    }

    #[test]
    fn locale_table_resolution_follows_the_fallback_chain() {
        let table = copy_table();
        // Exact match.
        assert_eq!(table.resolve("title", "zh-CN"), Some("看板"));
        assert_eq!(table.resolve("title", "en-US"), Some("Board"));
        // Language-family fallback (zh-TW → zh-CN).
        assert_eq!(table.resolve("title", "zh-TW"), Some("看板"));
        // en-US fallback.
        assert_eq!(table.resolve("title", "fr-FR"), Some("Board"));
        // Unknown key.
        assert_eq!(table.resolve("missing", "en-US"), None);
        assert_eq!(PluginLocaleTable::default().resolve("title", "en-US"), None);
    }

    #[test]
    fn locale_map_resolution_follows_the_fallback_chain() {
        let mut entries = BTreeMap::new();
        entries.insert("zh-CN".to_string(), "看板".to_string());
        entries.insert("en-US".to_string(), "Board".to_string());
        let map = PluginLocaleMap::new(entries);
        // Exact match.
        assert_eq!(map.resolve("zh-CN"), Some("看板"));
        // Language-family fallback (zh-TW → zh-CN).
        assert_eq!(map.resolve("zh-TW"), Some("看板"));
        // en-US fallback.
        assert_eq!(map.resolve("fr-FR"), Some("Board"));
        // First entry when nothing matches.
        let mut single = BTreeMap::new();
        single.insert("ja-JP".to_string(), "ボード".to_string());
        let map = PluginLocaleMap::new(single);
        assert_eq!(map.resolve("zh-CN"), Some("ボード"));
        assert_eq!(PluginLocaleMap::default().resolve("en-US"), None);
    }

    #[test]
    fn tree_parses_nodes_props_and_bindings() {
        let tree = valid_tree();
        assert_eq!(tree.schema_version, 1);
        assert_eq!(tree.root.kind, PluginUiNodeKind::Column);
        assert_eq!(tree.root.props.get_u32("gap"), Some(2));
        assert_eq!(tree.root.children.len(), 2);
        let button = &tree.root.children[1];
        assert_eq!(button.kind, PluginUiNodeKind::Button);
        assert_eq!(
            button.on.get(&PluginUiEventKind::Press).map(String::as_str),
            Some("refresh")
        );
        assert_eq!(button.props.get_tone("tone"), Some(PluginUiTone::Primary));
    }

    #[test]
    fn tree_resolves_locale_references_but_passes_literals() {
        let tree = valid_tree();
        let table = copy_table();
        assert_eq!(
            tree.root.children[0].text("text", &table, "zh-TW"),
            Some("看板".to_string())
        );
        assert_eq!(
            tree.root.children[0].text("text", &table, "en-US"),
            Some("Board".to_string())
        );
        let literal = validate_tree(
            &tree_json(r#"{"type": "text", "props": {"text": "hello"}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect("literal tree validates");
        assert_eq!(
            literal.root.text("text", &table, "zh-CN"),
            Some("hello".to_string())
        );
    }

    #[test]
    fn tree_rejects_unknown_components() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "table", "props": {}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("unknown component must reject");
        assert_eq!(code_of(&rejection), Code::UnknownComponent);
    }

    #[test]
    fn tree_rejects_schema_version_mismatch() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "group"}"#),
            2,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("mismatched tree version must reject");
        assert_eq!(code_of(&rejection), Code::SchemaVersionMismatch);
    }

    #[test]
    fn tree_rejects_frames_over_the_size_limit() {
        let padding = " ".repeat(MAX_TREE_FRAME_BYTES + 1);
        let rejection = validate_tree(
            &format!("{{\"schemaVersion\": 1{padding}}}"),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("oversized frame must reject");
        assert_eq!(code_of(&rejection), Code::TreeTooLarge);
    }

    #[test]
    fn tree_rejects_too_many_nodes() {
        let leaf = r#"{"type": "divider"}"#;
        let children: Vec<String> = (0..MAX_TREE_NODES).map(|_| leaf.to_string()).collect();
        let root = format!(
            r#"{{"type": "column", "children": [{{"type": "row", "children": [{}]}}]}}"#,
            children.join(",")
        );
        let rejection = validate_tree(&tree_json(&root), 1, &locale_keys(), &icon_set(ICONS))
            .expect_err("node limit must reject");
        assert!(rejection
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == Code::TreeTooLarge));
    }

    #[test]
    fn tree_rejects_too_deep_nesting() {
        let mut root = r#"{"type": "divider"}"#.to_string();
        for _ in 0..=MAX_TREE_DEPTH {
            root = format!(r#"{{"type": "group", "children": [{root}]}}"#);
        }
        let rejection = validate_tree(&tree_json(&root), 1, &locale_keys(), &icon_set(ICONS))
            .expect_err("depth limit must reject");
        assert_eq!(code_of(&rejection), Code::TreeTooDeep);
    }

    #[test]
    fn tree_rejects_unknown_text_references() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "text", "props": {"text": "@missing"}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("unresolved @key must reject");
        assert_eq!(code_of(&rejection), Code::UnknownTextKey);
    }

    #[test]
    fn tree_rejects_unknown_props_and_wrong_shapes() {
        let rejection = validate_tree(
            &tree_json(r##"{"type": "text", "props": {"color": "#ff0000"}}"##),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("raw color prop must reject");
        assert_eq!(code_of(&rejection), Code::UnknownProp);

        let rejection = validate_tree(
            &tree_json(r#"{"type": "text", "props": {"text": "hi", "truncate": "yes"}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("wrong prop shape must reject");
        assert_eq!(code_of(&rejection), Code::InvalidPropValue);

        let rejection = validate_tree(
            &tree_json(r#"{"type": "heading", "props": {"text": "t", "level": 9}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("heading level range must hold");
        assert_eq!(code_of(&rejection), Code::InvalidPropValue);
    }

    #[test]
    fn tree_rejects_unknown_tones() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "text", "props": {"text": "hi", "tone": "chartreuse"}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("tone whitelist must hold");
        assert_eq!(code_of(&rejection), Code::InvalidPropValue);
    }

    #[test]
    fn tree_rejects_children_on_leaves_and_extra_scroll_children() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "divider", "children": [{"type": "divider"}]}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("leaf children must reject");
        assert_eq!(code_of(&rejection), Code::InvalidChildren);

        let rejection = validate_tree(
            &tree_json(
                r#"{"type": "scroll", "children": [{"type": "divider"}, {"type": "divider"}]}"#,
            ),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("scroll takes a single child");
        assert_eq!(code_of(&rejection), Code::InvalidChildren);
    }

    #[test]
    fn tree_rejects_event_bindings_outside_the_component_contract() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "divider", "on": {"press": "x"}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("divider cannot press");
        assert_eq!(code_of(&rejection), Code::UnsupportedEventBinding);

        let rejection = validate_tree(
            &tree_json(r#"{"type": "button", "props": {"label": "b"}, "on": {"hover": "x"}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("hover is host-owned");
        assert_eq!(code_of(&rejection), Code::UnknownEventKind);

        let rejection = validate_tree(
            &tree_json(r#"{"type": "button", "props": {"label": "b"}, "on": {"press": ""}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("empty action names must reject");
        assert_eq!(code_of(&rejection), Code::InvalidActionName);
    }

    #[test]
    fn tree_validates_select_options() {
        let tree = validate_tree(
            &tree_json(
                r#"{"type": "select", "props": {"options": [
                    {"value": "a", "label": "@greeting"},
                    {"value": "b", "label": "Beta"}
                ], "value": "a"}}"#,
            ),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect("options tree validates");
        assert_eq!(tree.root.props.options().len(), 2);
        assert_eq!(tree.root.props.options()[0].label, "@greeting");

        let options: Vec<String> = (0..MAX_SELECT_OPTIONS + 1)
            .map(|index| format!(r#"{{"value": "v{index}", "label": "L{index}"}}"#))
            .collect();
        let rejection = validate_tree(
            &tree_json(&format!(
                r#"{{"type": "select", "props": {{"options": [{}]}}}}"#,
                options.join(",")
            )),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("option limit must reject");
        assert_eq!(code_of(&rejection), Code::InvalidPropValue);
    }

    #[test]
    fn tree_validates_progress_range() {
        let rejection = validate_tree(
            &tree_json(r#"{"type": "progress", "props": {"value": 101}}"#),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("progress range must hold");
        assert_eq!(code_of(&rejection), Code::InvalidPropValue);
    }

    #[test]
    fn tree_rejects_text_over_the_length_limit() {
        let long = "x".repeat(MAX_TEXT_BYTES + 1);
        let rejection = validate_tree(
            &tree_json(&format!(
                r#"{{"type": "text", "props": {{"text": "{long}"}}}}"#
            )),
            1,
            &locale_keys(),
            &icon_set(ICONS),
        )
        .expect_err("text limit must reject");
        assert_eq!(code_of(&rejection), Code::TextTooLong);
    }

    #[test]
    fn event_payloads_hold_control_values_only() {
        assert!(PluginUiEventPayload::Text("ok".to_string())
            .validate()
            .is_ok());
        assert!(PluginUiEventPayload::Number(1.5).validate().is_ok());
        assert!(PluginUiEventPayload::Bool(true).validate().is_ok());
        let long = "x".repeat(MAX_TEXT_BYTES + 1);
        assert!(PluginUiEventPayload::Text(long).validate().is_err());
    }

    #[test]
    fn scene_refs_are_copy_and_orderable() {
        let reference = SceneRef::new(1, 2);
        let copied = reference;
        assert_eq!(reference, copied);
        assert!(SceneRef::new(1, 1) < SceneRef::new(1, 2));
    }
}
