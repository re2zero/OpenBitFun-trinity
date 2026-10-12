//! Plugin-contributed scene rendering (R7b): the native renderer for
//! declarative UI trees published by [`openbitfun_core::plugin_ui`].
//!
//! The renderer is a pure tree walk over the validated snapshot the core
//! projection serves: no script, no HTML, no host component injection. Colors
//! exist only as semantic roles mapped onto `cx.theme()` tokens; icon names
//! resolve through the closed renderable set (`ShellAssets` = the component
//! default bundle plus the shell's extras). Interactions loop back through the
//! projection's `dispatch`, and every rejection lands as a visible in-tab
//! banner — never a silent swallow.
//!
//! The R7b fixture provider is synchronous, so the round trip completes inside
//! the click handler; the in-flight/timeout state machine below is kept (and
//! unit-tested) for the R7c async host channel, where events genuinely queue.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::input::{Enter, Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme, Disableable, Icon};
use gpui_kit::AssetSource as _;
use gpui_kit::{
    div, prelude::*, px, Anchor, AnyElement, Context, Entity, FontWeight, Hsla, IntoElement,
    SharedString, WeakEntity, Window,
};
use openbitfun_core::plugin_ui::{
    resolve_text, FixturePluginUi, PluginLocaleTable, PluginUiEvent, PluginUiEventKind,
    PluginUiEventPayload, PluginUiNode, PluginUiNodeKind, PluginUiProjection, PluginUiSceneGate,
    PluginUiTone, PluginUiTree, SceneRef,
};

use crate::i18n::ShellText;
use crate::shell::ShellWindow;
use crate::tabs::TabKind;

/// The event round trip's conservative timeout (design §4.3). A dispatch that
/// outlives it parks the scene's controls and shows the timeout banner.
const LOOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Renderable icon names → asset enums, built once from the shell's
/// composable asset sources (design §5.3: contributions may only reference
/// names this set actually embeds; everything else rejects at load time).
pub(crate) fn plugin_icon_catalog() -> &'static BTreeMap<String, IconName> {
    static CATALOG: OnceLock<BTreeMap<String, IconName>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let embeddable: BTreeSet<String> = crate::assets::ShellAssets
            .list("icons")
            .unwrap_or_default()
            .iter()
            .map(|path| {
                path.trim_start_matches("icons/")
                    .trim_end_matches(".svg")
                    .to_string()
            })
            .collect();
        let mut map = BTreeMap::new();
        for icon in IconName::ALL {
            let path = icon.path();
            let name = path.trim_start_matches("icons/").trim_end_matches(".svg");
            if embeddable.contains(name) {
                map.insert(name.to_string(), *icon);
            }
        }
        map
    })
}

/// A visible scene-level failure (design §4.2: diagnostics must surface in
/// the owning tab, not in a log alone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PluginUiBanner {
    /// The provider produced a document that failed validation; the previous
    /// tree (if any) stays on display.
    Rejected(String),
    /// The provider failed to produce a document at all.
    Provider(String),
    /// The async round trip outlived [`LOOP_TIMEOUT`] (R7c path; the R7b
    /// synchronous provider cannot produce it, the variant stays for the
    /// state-machine contract the tests pin).
    #[allow(dead_code)]
    Timeout,
}

impl PluginUiBanner {
    fn detail(&self) -> String {
        match self {
            Self::Rejected(detail) | Self::Provider(detail) => detail.clone(),
            Self::Timeout => "round trip timed out".to_string(),
        }
    }
}

/// Shell-side per-scene runtime: control entities keyed by tree path (draft
/// text survives full-tree replacement by design — the user's typing wins
/// until the next submit), the visible banner, and the retraction state.
#[derive(Clone, Default)]
pub(crate) struct PluginUiSceneRuntime {
    inputs: BTreeMap<Vec<usize>, Entity<InputState>>,
    textareas: BTreeMap<Vec<usize>, Entity<TextareaState>>,
    /// Started-at of the single in-flight round trip; `None` when idle. The
    /// R7b fixture dispatches synchronously, so this stays `Some` only inside
    /// the dispatch call — the timeout path is exercised for R7c.
    inflight_since: Option<Instant>,
    banner: Option<PluginUiBanner>,
    /// The scene ref stopped resolving (plugin uninstalled): the tab shows
    /// the removed state and disables its controls, but stays open for the
    /// user to close (design §4.2).
    removed: bool,
}

impl PluginUiSceneRuntime {
    /// Whether the round trip has outlived the conservative timeout.
    fn loop_timed_out(&self, now: Instant) -> bool {
        self.inflight_since
            .is_some_and(|started| now.duration_since(started) > LOOP_TIMEOUT)
    }

    /// Controls stay interactive while no failure state forbids it.
    fn events_enabled(&self, now: Instant) -> bool {
        !self.removed && !self.loop_timed_out(now)
    }
}

/// Env gate: the R7b fixture is a test contribution and never enters product
/// assembly (design §6.5); it installs only when the variable is present.
pub(crate) fn fixture_requested() -> bool {
    std::env::var_os("OPENBITFUN_PLUGIN_UI_FIXTURE").is_some()
}

/// Builds the shell-side projection (closed icon set from the renderable
/// catalog) and installs the fixture provider behind the env gate. Returns
/// the fixture scene ref when the fixture installed.
pub(crate) fn setup_projection() -> (PluginUiProjection, Option<SceneRef>) {
    let mut projection = PluginUiProjection::new(plugin_icon_catalog().keys().cloned().collect());
    if !fixture_requested() {
        return (projection, None);
    }
    match projection.install("fixture", FixturePluginUi::new_arc()) {
        Ok(handles) => {
            let scene_ref = handles.first().map(|handle| handle.scene_ref);
            tracing::info!("plugin UI fixture installed ({} scene(s))", handles.len());
            (projection, scene_ref)
        }
        Err(gate) => {
            tracing::warn!("plugin UI fixture rejected: {gate:?}");
            (projection, None)
        }
    }
}

impl ShellWindow {
    /// One-shot automation hook (`OPENBITFUN_PLUGIN_UI_AUTOOPEN`): opens the
    /// fixture scene tab once the window exists, so a smoke run can inspect
    /// the rendered tree without synthesizing clicks (same pattern as the
    /// terminal panel's auto-open).
    pub(crate) fn plugin_ui_auto_open_requested() -> bool {
        std::env::var_os("OPENBITFUN_PLUGIN_UI_AUTOOPEN").is_some()
    }

    /// Opens (or activates) a contributed scene tab and prepares its runtime.
    pub(crate) fn open_plugin_scene(&mut self, reference: SceneRef, cx: &mut Context<Self>) {
        self.tabs
            .open(TabKind::Scene(crate::tabs::SceneId::External(reference)));
        self.plugin_ui_scenes.entry(reference).or_default();
        cx.notify();
    }

    /// The tab-bar title of a contributed scene, resolved from the plugin
    /// locale map in the active shell locale.
    pub(crate) fn plugin_scene_label(&self, reference: SceneRef, text: &ShellText) -> String {
        let locale_tag = text.locale.as_str();
        let found = self
            .plugin_ui
            .scenes()
            .into_iter()
            .find(|handle| handle.scene_ref == reference);
        found
            .and_then(|handle| {
                handle
                    .contribution
                    .title
                    .resolve(locale_tag)
                    .map(str::to_string)
            })
            .unwrap_or_default()
    }

    /// The tab-bar icon of a contributed scene; names outside the closed set
    /// fall back to the same placeholder the builtin table uses.
    pub(crate) fn plugin_scene_icon(&self, reference: SceneRef) -> IconName {
        self.plugin_ui
            .scenes()
            .into_iter()
            .find(|handle| handle.scene_ref == reference)
            .and_then(|handle| plugin_icon_catalog().get(handle.contribution.icon.as_str()))
            .copied()
            .unwrap_or(IconName::Square)
    }

    /// The plugin scene tab's main area: ensure the snapshot and control
    /// entities, then render the tree (or the explicit failure state).
    pub(crate) fn render_external_scene(
        &mut self,
        reference: SceneRef,
        window: &mut Window,
        text: &ShellText,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let locale_tag = text.locale.as_str().to_string();

        // Gate the scene on the projection state first; a retracted
        // contribution renders the explicit removed state (tab stays open,
        // controls disabled — design §4.2).
        let gate = self.plugin_ui.tree(reference).err();
        {
            let runtime = self.plugin_ui_scenes.entry(reference).or_default();
            match &gate {
                Some(PluginUiSceneGate::ContributionRemoved) => runtime.removed = true,
                Some(PluginUiSceneGate::ProviderError(detail)) => {
                    let detail = detail.clone();
                    if runtime
                        .banner
                        .as_ref()
                        .map(PluginUiBanner::detail)
                        .as_deref()
                        != Some(detail.as_str())
                    {
                        runtime.banner = Some(PluginUiBanner::Provider(detail));
                    }
                }
                Some(PluginUiSceneGate::Rejected(rejection)) => {
                    let detail = rejection.first().detail.clone();
                    if runtime
                        .banner
                        .as_ref()
                        .map(PluginUiBanner::detail)
                        .as_deref()
                        != Some(detail.as_str())
                    {
                        runtime.banner = Some(PluginUiBanner::Rejected(detail));
                    }
                }
                None => runtime.removed = false,
            }
        }

        if self
            .plugin_ui_scenes
            .get(&reference)
            .is_some_and(|runtime| runtime.removed)
        {
            return removed_face(text).into_any_element();
        }

        // Owned copy first: the copy table survives the mutable tree fetch.
        let locale_table = self.plugin_ui.locales(reference);
        // The tree is present (a fresh rejection keeps the previous tree);
        // sync control entities with it, then render.
        let tree = match self.plugin_ui.tree(reference) {
            Ok(tree) => tree,
            Err(_) => {
                // No tree ever arrived: the banner explains why.
                let banner = self
                    .plugin_ui_scenes
                    .get(&reference)
                    .and_then(|runtime| runtime.banner.clone());
                let mut column = div()
                    .id(format!("plugin-scene-{}", scene_ref_id(reference)))
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .px_4()
                    .py_3();
                if let Some(banner) = banner {
                    column = column.child(render_banner(banner, text, cx));
                }
                return column.into_any_element();
            }
        };
        let runtime = self.plugin_ui_scenes.entry(reference).or_default();
        Self::sync_scene_widgets(runtime, tree, &locale_table, &locale_tag, window, cx);
        let runtime = self
            .plugin_ui_scenes
            .get(&reference)
            .cloned()
            .unwrap_or_default();
        let events_enabled = runtime.events_enabled(Instant::now());

        let mut column = div()
            .id(format!("plugin-scene-{}", scene_ref_id(reference)))
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .text_color(cx.theme().foreground);
        if let Some(banner) = runtime.banner.clone() {
            column = column.child(render_banner(banner, text, cx));
        }
        let weak = cx.entity().downgrade();
        column = column.child(render_node(
            &tree.root,
            weak,
            reference,
            &mut Vec::new(),
            &locale_table,
            &locale_tag,
            &runtime,
            events_enabled,
            cx,
        ));
        column.into_any_element()
    }

    /// One interaction, dispatched through the projection (the only arbiter
    /// of event ownership). Rejections land in the scene banner.
    pub(crate) fn dispatch_plugin_event(
        &mut self,
        reference: SceneRef,
        action: String,
        payload: Option<PluginUiEventPayload>,
        cx: &mut Context<Self>,
    ) {
        let runtime = self.plugin_ui_scenes.entry(reference).or_default();
        runtime.inflight_since = Some(Instant::now());
        let event = PluginUiEvent {
            scene: reference,
            action,
            payload,
        };
        let outcome = self.plugin_ui.dispatch(&event);
        let runtime = self.plugin_ui_scenes.entry(reference).or_default();
        runtime.inflight_since = None;
        match outcome {
            Ok(_) => runtime.banner = None,
            Err(PluginUiSceneGate::ContributionRemoved) => runtime.removed = true,
            Err(PluginUiSceneGate::ProviderError(detail)) => {
                runtime.banner = Some(PluginUiBanner::Provider(detail));
            }
            Err(PluginUiSceneGate::Rejected(rejection)) => {
                runtime.banner = Some(PluginUiBanner::Rejected(rejection.first().detail.clone()));
            }
        }
        cx.notify();
    }

    /// Creates missing control entities for the tree's inputs (placeholder
    /// seeds from the tree on first mount) and drops the ones whose nodes
    /// disappeared (full-tree replacement semantics). A free function over
    /// split borrows: the tree reference pins the projection, the runtime map
    /// is a separate field.
    fn sync_scene_widgets(
        runtime: &mut PluginUiSceneRuntime,
        tree: &PluginUiTree,
        locales: &PluginLocaleTable,
        locale_tag: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut inputs = BTreeMap::new();
        let mut textareas = BTreeMap::new();
        collect_widget_paths(&tree.root, Vec::new(), &mut inputs, &mut textareas);
        let stale_inputs: Vec<Vec<usize>> = runtime
            .inputs
            .keys()
            .filter(|path| !inputs.contains_key(*path))
            .cloned()
            .collect();
        let stale_textareas: Vec<Vec<usize>> = runtime
            .textareas
            .keys()
            .filter(|path| !textareas.contains_key(*path))
            .cloned()
            .collect();
        for path in stale_inputs {
            runtime.inputs.remove(&path);
        }
        for path in stale_textareas {
            runtime.textareas.remove(&path);
        }
        for (path, placeholder) in inputs {
            if runtime.inputs.contains_key(&path) {
                continue;
            }
            let placeholder = placeholder
                .map(|raw| resolve_text(&raw, &locales, locale_tag))
                .unwrap_or_default();
            let entity = cx.new(|cx| {
                let state = InputState::new(window, cx);
                if placeholder.is_empty() {
                    state
                } else {
                    state.placeholder(placeholder)
                }
            });
            runtime.inputs.insert(path, entity);
        }
        for (path, placeholder) in textareas {
            if runtime.textareas.contains_key(&path) {
                continue;
            }
            let placeholder = placeholder
                .map(|raw| resolve_text(&raw, &locales, locale_tag))
                .unwrap_or_default();
            let entity = cx.new(|cx| {
                let state = TextareaState::new(window, cx);
                if placeholder.is_empty() {
                    state
                } else {
                    state.placeholder(placeholder)
                }
            });
            runtime.textareas.insert(path, entity);
        }
    }
}

/// The red failure banner at the top of the scene.
fn render_banner(
    banner: PluginUiBanner,
    text: &ShellText,
    cx: &Context<ShellWindow>,
) -> impl IntoElement {
    let detail = banner.detail();
    let headline = match banner {
        PluginUiBanner::Timeout => text.plugin_ui_timeout.clone(),
        PluginUiBanner::Rejected(_) | PluginUiBanner::Provider(_) => {
            text.plugin_ui_rejected.clone()
        }
    };
    let mut tint = cx.theme().danger;
    tint.a = 0.08;
    div()
        .mt_1()
        .mb_2()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().danger)
        .bg(tint)
        .px_3()
        .py_2()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().danger)
                .child(SharedString::from(headline)),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(detail)),
        )
}

/// The retracted-contribution face: the tab stays open, controls die.
fn removed_face(text: &ShellText) -> impl IntoElement {
    Empty::new().header(
        EmptyHeader::new()
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .child(Icon::new(IconName::Plug)),
            )
            .title(EmptyTitle::new().child(text.plugin_ui_removed.clone())),
    )
}

/// Event dispatch from non-listener callbacks (component on_click closures
/// run against `&mut App`, not the window's context).
fn dispatch_via_weak(
    weak: &WeakEntity<ShellWindow>,
    reference: SceneRef,
    action: String,
    payload: Option<PluginUiEventPayload>,
    cx: &mut gpui_kit::App,
) {
    let _ = weak.update(cx, |this, cx| {
        this.dispatch_plugin_event(reference, action, payload, cx);
    });
}

/// The recursive tree walk: one node → one element. Paths address nodes for
/// control entity reuse across full-tree replacements.
#[allow(clippy::too_many_arguments)]
fn render_node(
    node: &PluginUiNode,
    weak: WeakEntity<ShellWindow>,
    reference: SceneRef,
    path: &mut Vec<usize>,
    locales: &PluginLocaleTable,
    locale_tag: &str,
    runtime: &PluginUiSceneRuntime,
    events_enabled: bool,
    cx: &mut Context<ShellWindow>,
) -> AnyElement {
    let text_of = |name: &str| node.text(name, locales, locale_tag).unwrap_or_default();
    let flex = |column: bool, node: &PluginUiNode| {
        let base = if column {
            div().flex().flex_col()
        } else {
            div().flex().flex_row()
        };
        let gap = node.props.get_u32("gap").unwrap_or(0);
        let padding = node.props.get_u32("padding").unwrap_or(0);
        base.gap(px(gap as f32 * 4.0))
            .p(px(padding as f32 * 4.0))
            .min_w_0()
    };
    let element_id = |kind: &str, path: &[usize]| {
        SharedString::from(format!(
            "pu-{kind}-{}-{}",
            scene_ref_id(reference),
            path.iter()
                .map(|step| step.to_string())
                .collect::<Vec<_>>()
                .join("-")
        ))
    };

    match node.kind {
        PluginUiNodeKind::Column | PluginUiNodeKind::Group => {
            let mut container = flex(true, node).w_full();
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                container = container.child(render_node(
                    child,
                    weak.clone(),
                    reference,
                    path,
                    locales,
                    locale_tag,
                    runtime,
                    events_enabled,
                    cx,
                ));
                path.pop();
            }
            container.into_any_element()
        }
        PluginUiNodeKind::Row | PluginUiNodeKind::List => {
            let mut container = flex(false, node).flex_wrap().w_full();
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                container = container.child(render_node(
                    child,
                    weak.clone(),
                    reference,
                    path,
                    locales,
                    locale_tag,
                    runtime,
                    events_enabled,
                    cx,
                ));
                path.pop();
            }
            container.into_any_element()
        }
        PluginUiNodeKind::Card => {
            let padding = node.props.get_u32("padding").unwrap_or(3);
            let mut card = div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .p(px(padding as f32 * 4.0))
                .min_w_0();
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                card = card.child(render_node(
                    child,
                    weak.clone(),
                    reference,
                    path,
                    locales,
                    locale_tag,
                    runtime,
                    events_enabled,
                    cx,
                ));
                path.pop();
            }
            card.into_any_element()
        }
        PluginUiNodeKind::Scroll => {
            let mut scroller = div()
                .id(element_id("scroll", path))
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scroll()
                .flex()
                .flex_col();
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                scroller = scroller.child(render_node(
                    child,
                    weak.clone(),
                    reference,
                    path,
                    locales,
                    locale_tag,
                    runtime,
                    events_enabled,
                    cx,
                ));
                path.pop();
            }
            scroller.into_any_element()
        }
        PluginUiNodeKind::Divider => Separator::horizontal().into_any_element(),
        PluginUiNodeKind::Spacer => {
            let size = node.props.get_u32("size").unwrap_or(2);
            div()
                .w(px(size as f32 * 4.0))
                .h(px(size as f32 * 4.0))
                .into_any_element()
        }
        PluginUiNodeKind::Text => {
            let mut line = div().text_sm().min_w_0();
            if let Some(tone) = node.props.get_tone("tone") {
                line = line.text_color(tone_color(tone, cx));
            }
            if node.props.get_bool("truncate").unwrap_or(false) {
                line = line.truncate();
            }
            line.child(SharedString::from(text_of("text")))
                .into_any_element()
        }
        PluginUiNodeKind::Heading => {
            let level = node.props.get_u32("level").unwrap_or(1);
            let mut heading = div().min_w_0().font_weight(FontWeight::SEMIBOLD);
            heading = match level {
                1 => heading.text_xl(),
                2 => heading.text_lg(),
                _ => heading.text_base(),
            };
            heading
                .child(SharedString::from(text_of("text")))
                .into_any_element()
        }
        PluginUiNodeKind::Label => {
            let mut label = div().text_xs().min_w_0();
            if let Some(tone) = node.props.get_tone("tone") {
                label = label.text_color(tone_color(tone, cx));
            } else {
                label = label.text_color(cx.theme().muted_foreground);
            }
            label
                .child(SharedString::from(text_of("text")))
                .into_any_element()
        }
        PluginUiNodeKind::Button => {
            let label = text_of("label");
            let disabled = node.props.get_bool("disabled").unwrap_or(false) || !events_enabled;
            let tone = node.props.get_tone("tone").unwrap_or(PluginUiTone::Primary);
            let variant = match tone {
                PluginUiTone::Danger => ButtonVariant::Danger,
                PluginUiTone::Muted
                | PluginUiTone::Text
                | PluginUiTone::Surface
                | PluginUiTone::Border => ButtonVariant::Secondary,
                _ => ButtonVariant::Primary,
            };
            let button = Button::new(element_id("button", path))
                .label(label)
                .with_variant(variant)
                .disabled(disabled);
            match node.on.get(&PluginUiEventKind::Press) {
                None => button.into_any_element(),
                Some(action) => {
                    let action = action.clone();
                    button
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.dispatch_plugin_event(reference, action.clone(), None, cx);
                        }))
                        .into_any_element()
                }
            }
        }
        PluginUiNodeKind::Input => {
            let entity = runtime.inputs.get(path).cloned();
            let Some(entity) = entity else {
                return div().into_any_element();
            };
            let disabled = node.props.get_bool("disabled").unwrap_or(false) || !events_enabled;
            let mut input = Input::new(&entity).w_full();
            let _ = locales;
            if disabled {
                input = input.disabled(true);
            }
            match node.on.get(&PluginUiEventKind::Submit) {
                None => input.into_any_element(),
                Some(action) => {
                    let action = action.clone();
                    div()
                        .id(element_id("input-wrap", path))
                        .w_full()
                        .on_action(move |_: &Enter, _, cx: &mut gpui_kit::App| {
                            let value = entity.read(cx).value().to_string();
                            dispatch_via_weak(
                                &weak,
                                reference,
                                action.clone(),
                                Some(PluginUiEventPayload::Text(value)),
                                cx,
                            );
                        })
                        .child(input)
                        .into_any_element()
                }
            }
        }
        PluginUiNodeKind::Textarea => {
            let entity = runtime.textareas.get(path).cloned();
            let Some(entity) = entity else {
                return div().into_any_element();
            };
            let disabled = node.props.get_bool("disabled").unwrap_or(false) || !events_enabled;
            let mut area = Textarea::new(&entity).w_full().h(px(96.0));
            if disabled {
                area = area.disabled(true);
            }
            match node.on.get(&PluginUiEventKind::Submit) {
                None => area.into_any_element(),
                Some(action) => {
                    let action = action.clone();
                    div()
                        .id(element_id("textarea-wrap", path))
                        .w_full()
                        .on_action(move |_: &Enter, _, cx: &mut gpui_kit::App| {
                            let value = entity.read(cx).value().to_string();
                            dispatch_via_weak(
                                &weak,
                                reference,
                                action.clone(),
                                Some(PluginUiEventPayload::Text(value)),
                                cx,
                            );
                        })
                        .child(area)
                        .into_any_element()
                }
            }
        }
        PluginUiNodeKind::Select => {
            let options = node.props.options();
            let current = node.props.get_str("value").unwrap_or_default();
            let current_label = options
                .iter()
                .find(|option| option.value == current)
                .map(|option| resolve_text(&option.label, locales, locale_tag))
                .unwrap_or_else(|| current.to_string());
            let disabled = node.props.get_bool("disabled").unwrap_or(false) || !events_enabled;
            let action = node.on.get(&PluginUiEventKind::Change).cloned();
            let trigger = Button::new(element_id("select", path))
                .label(current_label)
                .with_variant(ButtonVariant::Secondary)
                .disabled(disabled);
            let Some(action) = action else {
                return trigger.into_any_element();
            };
            let entries: Vec<(String, String)> = options
                .iter()
                .map(|option| {
                    (
                        option.value.clone(),
                        resolve_text(&option.label, locales, locale_tag),
                    )
                })
                .collect();
            let weak_menu = weak.clone();
            trigger
                .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                    let mut popup = menu;
                    for (value, label) in &entries {
                        let action = action.clone();
                        let value = value.clone();
                        let weak = weak_menu.clone();
                        popup = popup.item(PopupMenuItem::new(label.clone()).on_click(
                            move |_: &gpui_kit::ClickEvent, _, cx| {
                                dispatch_via_weak(
                                    &weak,
                                    reference,
                                    action.clone(),
                                    Some(PluginUiEventPayload::Text(value.clone())),
                                    cx,
                                );
                            },
                        ));
                    }
                    popup
                })
                .into_any_element()
        }
        PluginUiNodeKind::Checkbox => {
            let label = text_of("label");
            let checked = node.props.get_bool("checked").unwrap_or(false);
            let action = node.on.get(&PluginUiEventKind::Change).cloned();
            let mut checkbox = Checkbox::new(element_id("checkbox", path))
                .label(label)
                .checked(checked);
            match action {
                None => checkbox.into_any_element(),
                Some(action) => {
                    let weak = weak.clone();
                    checkbox = checkbox.on_click(move |checked: &bool, _, cx| {
                        if events_enabled {
                            dispatch_via_weak(
                                &weak,
                                reference,
                                action.clone(),
                                Some(PluginUiEventPayload::Bool(*checked)),
                                cx,
                            );
                        }
                    });
                    checkbox.into_any_element()
                }
            }
        }
        PluginUiNodeKind::Switch => {
            let checked = node.props.get_bool("checked").unwrap_or(false);
            let action = node.on.get(&PluginUiEventKind::Change).cloned();
            let mut switch = Switch::new(element_id("switch", path)).checked(checked);
            match action {
                None => switch.into_any_element(),
                Some(action) => {
                    let weak = weak.clone();
                    switch = switch.on_click(move |checked: &bool, _, cx| {
                        if events_enabled {
                            dispatch_via_weak(
                                &weak,
                                reference,
                                action.clone(),
                                Some(PluginUiEventPayload::Bool(*checked)),
                                cx,
                            );
                        }
                    });
                    switch.into_any_element()
                }
            }
        }
        PluginUiNodeKind::Badge | PluginUiNodeKind::Tag => {
            let label = text_of("label");
            let tone = node.props.get_tone("tone").unwrap_or(PluginUiTone::Muted);
            Tag::color(tag_color(tone))
                .child(SharedString::from(label))
                .into_any_element()
        }
        PluginUiNodeKind::Icon => {
            let name = node.props.get_str("name").unwrap_or_default();
            match plugin_icon_catalog().get(name).copied() {
                Some(icon) => Icon::new(icon).into_any_element(),
                None => div().into_any_element(),
            }
        }
        PluginUiNodeKind::Progress => {
            let value = node.props.get_u32("value").unwrap_or(0);
            Progress::new(element_id("progress", path))
                .value(value as f32)
                .into_any_element()
        }
        PluginUiNodeKind::Spinner => Spinner::new().into_any_element(),
        PluginUiNodeKind::Empty => {
            let title = text_of("title");
            let description = text_of("description");
            Empty::new()
                .header(
                    EmptyHeader::new()
                        .media(
                            EmptyMedia::new()
                                .with_variant(EmptyMediaVariant::Icon)
                                .child(Icon::new(IconName::Square)),
                        )
                        .title(EmptyTitle::new().child(title))
                        .description(EmptyDescription::new().child(description)),
                )
                .into_any_element()
        }
    }
}

/// Stable string id for a scene ref (element ids and selectors).
fn scene_ref_id(reference: SceneRef) -> String {
    format!("p{}s{}", reference.plugin, reference.scene)
}

/// Semantic role → theme token (design §5.2). The only place the mapping
/// exists; adding a role means editing this function and the design table.
fn tone_color(tone: PluginUiTone, cx: &Context<ShellWindow>) -> Hsla {
    let theme = cx.theme();
    match tone {
        PluginUiTone::Primary | PluginUiTone::Accent => theme.primary,
        PluginUiTone::Text => theme.foreground,
        PluginUiTone::Muted => theme.muted_foreground,
        PluginUiTone::Danger => theme.danger,
        PluginUiTone::Surface => theme.popover,
        PluginUiTone::Border => theme.border,
    }
}

/// Semantic role → tag color name (the closest theme-family match; the tag
/// component's palette is name-based, not token-based).
fn tag_color(tone: PluginUiTone) -> gpui_kit::component::theme::ColorName {
    use gpui_kit::component::theme::ColorName;
    match tone {
        PluginUiTone::Danger => ColorName::Red,
        PluginUiTone::Accent | PluginUiTone::Primary => ColorName::Blue,
        PluginUiTone::Muted => ColorName::Gray,
        PluginUiTone::Text | PluginUiTone::Surface | PluginUiTone::Border => ColorName::Neutral,
    }
}

/// Collects the paths (and raw placeholders) of input-like nodes so control
/// entities can follow the tree across full replacements.
fn collect_widget_paths(
    node: &PluginUiNode,
    path: Vec<usize>,
    inputs: &mut BTreeMap<Vec<usize>, Option<String>>,
    textareas: &mut BTreeMap<Vec<usize>, Option<String>>,
) {
    match node.kind {
        PluginUiNodeKind::Input => {
            inputs.insert(path, node.props.get_str("placeholder").map(str::to_string));
            return;
        }
        PluginUiNodeKind::Textarea => {
            textareas.insert(path, node.props.get_str("placeholder").map(str::to_string));
            return;
        }
        _ => {}
    }
    for (index, child) in node.children.iter().enumerate() {
        let mut child_path = path.clone();
        child_path.push(index);
        collect_widget_paths(child, child_path, inputs, textareas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_catalog_only_contains_embedded_names() {
        let catalog = plugin_icon_catalog();
        // The fixture's icon and common shell icons must resolve.
        assert!(catalog.contains_key("layout-dashboard"));
        assert!(catalog.contains_key("settings"));
        assert!(!catalog.contains_key("definitely-not-an-icon"));
        // Every listed name maps to an icon whose path round-trips.
        for (name, icon) in catalog {
            let path = icon.path().to_string();
            assert_eq!(path, format!("icons/{name}.svg"));
        }
    }

    #[test]
    fn loop_timeout_disables_events_but_only_past_the_deadline() {
        let mut runtime = PluginUiSceneRuntime::default();
        let now = Instant::now();
        assert!(runtime.events_enabled(now));
        // Inside the window: still enabled.
        runtime.inflight_since = Some(now - LOOP_TIMEOUT + std::time::Duration::from_millis(500));
        assert!(runtime.events_enabled(now));
        // Past the deadline: conservative disable.
        runtime.inflight_since = Some(now - LOOP_TIMEOUT - std::time::Duration::from_millis(1));
        assert!(!runtime.events_enabled(now));
        assert!(runtime.loop_timed_out(now));
        runtime.inflight_since = None;
        assert!(runtime.events_enabled(now));
    }

    #[test]
    fn retraction_disables_events_regardless_of_inflight() {
        let mut runtime = PluginUiSceneRuntime::default();
        runtime.removed = true;
        assert!(!runtime.events_enabled(Instant::now()));
        assert_eq!(runtime.banner, None);
    }

    #[test]
    fn banners_carry_details() {
        assert_eq!(PluginUiBanner::Timeout.detail(), "round trip timed out");
        assert_eq!(
            PluginUiBanner::Rejected("boom".to_string()).detail(),
            "boom"
        );
        assert_eq!(
            PluginUiBanner::Provider("down".to_string()).detail(),
            "down"
        );
    }

    #[test]
    fn fixture_gate_reads_the_environment() {
        // The gate itself is env-driven; this only pins the helper's shape.
        let requested = fixture_requested();
        let expected = std::env::var_os("OPENBITFUN_PLUGIN_UI_FIXTURE").is_some();
        assert_eq!(requested, expected);
    }
}
