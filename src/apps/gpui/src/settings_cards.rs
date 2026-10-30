//! Card primitives for the redesigned settings scene (gpui-redesign E2).
//!
//! Geometry follows the design spec (`.relay/design/gpui-ely/style.md`,
//! 「静谧卡片」): one flat card per product section — the theme panel ground, a
//! hairline border, the theme's large-surface radius — holding edge-to-edge
//! setting rows separated by hairlines. Every color reads the active theme's
//! semantic tokens; no raw palette values (theme color audit contract).
//!
//! Pages migrate one at a time: [`render_card_page`] returns `Some` for a page
//! that renders through this path and `None` for one still on the component
//! `Settings` pipeline (`settings::render_page`).

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, WindowExt};
use gpui_kit::{
    div, prelude::*, px, Anchor, AnyElement, App, Entity, FontWeight, IntoElement, ParentElement,
    RenderOnce, SharedString, Styled, Subscription, Window,
};
use std::rc::Rc;

use crate::i18n::ShellText;
use crate::settings::{
    add_quick_action, apply_hook_import_plan, delete_quick_action, dismiss_hook_import_plan,
    fetch_hook_imports, hook_import_state_label, quick_action_builtin_default, remove_hook_import,
    reset_hook_import_store, review_hook_import, save_quick_action_edit, set_git_commit_coauthor,
    set_hook_import_enabled, set_hooks_enabled, set_project_hooks_enabled, start_quick_action_edit,
    toggle_quick_action, HookImportState, QuickActionsEditorState,
};
use crate::settings::{
    archived_sessions_field, diagnostics_export_button, diagnostics_logs_field,
    previous_exit_notice, set_logging_level, usage_activity_field, usage_distributions_body,
    usage_endpoint_body, usage_overview_body, usage_trend_body, DiagnosticsNotice,
};
use crate::settings::{
    body_json_valid, headers_json_valid, model_category_values, model_editor_presets_element,
    model_effective_capabilities, model_provider_groups, model_provider_key,
    model_supports_capability, model_test_receipt, proxy_enabled, proxy_text,
    set_ai_model_body_mode, set_ai_model_body_text, set_ai_model_category,
    set_ai_model_connection_text, set_ai_model_context_window, set_ai_model_header_mode,
    set_ai_model_headers_text, set_ai_model_inline_think, set_ai_model_max_tokens,
    set_ai_model_provider, set_ai_model_reasoning_default, set_ai_model_skip_ssl,
    set_default_model_slot, set_default_primary_model, set_proxy_key, set_stream_timeout_value,
    start_model_test, ModelEditorState, ModelProviderGroup, ModelTestReceipt, ModelTestState,
    ModelsPageState, DEFAULT_MODEL_FAST_SLOT, DEFAULT_MODEL_IMAGE_SLOT, DEFAULT_MODEL_SPEECH_SLOT,
    MAX_STREAM_TIMEOUT_SECS, MODEL_CREATE_MIN_CONTEXT_WINDOW, MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW,
    MODEL_EDITOR_MIN_CONTEXT_WINDOW, MODEL_PROVIDER_FORMATS, STREAM_IDLE_TIMEOUT_PATH,
    STREAM_TTFT_TIMEOUT_PATH,
};
use crate::settings::{
    default_harness_facts, default_worktree_settings_document, entry_description, entry_label,
    flow_chat_fixed_mode, flow_chat_strategy, language_choices, object_bool, object_str,
    object_u64, session_title_model_options, set_autostart, set_flow_chat_fixed_mode,
    set_flow_chat_strategy, set_language, set_prevent_sleep, set_session_title_enabled,
    set_session_title_model, set_terminal_default_shell, set_terminal_font_family,
    set_terminal_font_size, set_terminal_scrollback, set_ui_font_custom_px, set_ui_font_level,
    set_workspace_search, set_worktree_auto_delete_limit, set_worktree_switch, set_worktree_text,
    shortcut_keycaps, terminal_profiles_field, theme_button, ui_font_base_px,
    ui_font_level_options, worktree_error_label, worktree_settings_are_default, SettingsEntry,
    ShellConfig, WorktreeProjectsState, CLOSE_BEHAVIOR_PATH, CLOSE_MINIMIZE, CLOSE_QUIT,
    GLOBAL_SHORTCUTS, MAX_FONT_SIZE, MAX_SCROLLBACK, MAX_UI_FONT_PX, MIN_FONT_SIZE, MIN_SCROLLBACK,
    MIN_UI_FONT_PX, UI_FONT_CUSTOM_LEVEL,
};
use crate::settings::{
    memory_actions_button, memory_advanced_toggle_button, memory_model_options,
    memory_notice_element, memory_policy_value, set_memory_btw, set_memory_concurrency,
    set_memory_consolidation_model, set_memory_enabled, set_memory_extract_model,
    set_memory_max_age_days, set_memory_min_idle_hours, set_memory_per_startup, set_memory_policy,
    set_memory_raw_limit, set_memory_retention_days, set_memory_scan_limit, MemoryPageState,
};
use crate::settings::{
    permission_mode_description, permission_mode_options, permission_mode_value,
    permission_rules_field, set_deferred_tool_loading, set_permission_mode,
    set_review_capacity_value, set_show_permission_mode_control, set_subagent_batch_policy,
    set_subagent_default_model, set_subagent_max_concurrency, set_swarm_max_concurrency,
    set_tool_execution_timeout, set_tool_json_repair, set_tool_presentation_mode,
    set_user_question_timeout, subagent_batch_description, subagent_batch_options,
    subagent_model_choice, subagent_model_options,
};
use crate::settings::{
    set_websearch_http_auth_mode, set_websearch_http_endpoint, set_websearch_http_header_name,
    set_websearch_provider, websearch_credential_field, websearch_http_auth_mode_display,
};
use crate::settings::{
    DEFAULT_REVIEW_PARALLELISM, DEFAULT_REVIEW_QUEUE_WAIT_SECS, MAX_REVIEW_PARALLELISM,
    MAX_REVIEW_QUEUE_WAIT_SECS, MAX_RUNTIME_TIMEOUT_SECS, MAX_SUBAGENT_CONCURRENCY,
    MAX_SWARM_CONCURRENCY, MIN_REVIEW_PARALLELISM, MIN_SUBAGENT_CONCURRENCY,
};
use crate::shell::{resolve_quick_action_text, ShellWindow};
use openbitfun_core::service::worktree::WorktreeProjectSummary;
use openbitfun_core_types::{WorktreeLifecycle, WorktreeSummary};

/// One setting row: label + optional description on the left, the control in a
/// fixed trailing lane. The owning card supplies the divider above every row
/// but the first and rounds the hover tint at the card's corners.
pub(crate) struct SettingsCardRow {
    id: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    control: AnyElement,
    /// An inline affordance after the label text (web `ConfigPageRow`'s
    /// label-adjacent icon button, e.g. the subagent-model settings jump).
    label_action: Option<AnyElement>,
    /// Web `ConfigPageRow`'s multiline mode: the label block sits above and
    /// the content spans the full width below (collection rows — lists with
    /// their own actions — read better than a crushed trailing lane).
    multiline: bool,
    /// The component pipeline's disabled item: reduced opacity.
    disabled: bool,
    /// Danger emphasis for statement rows (invalid-JSON notes and the like):
    /// the label reads in the theme's danger color.
    danger: bool,
}

impl SettingsCardRow {
    /// A row keyed by a domain id (`ElementId` uniqueness scope: the page).
    /// The id never derives from the (translated) label.
    pub(crate) fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            control: div().into_any_element(),
            label_action: None,
            multiline: false,
            disabled: false,
            danger: false,
        }
    }

    /// A label-less body row: the card head speaks for the section and the
    /// content spans the card edge to edge (dashboards and lists — page
    /// bodies, not labeled controls). Implies [`Self::multiline`].
    pub(crate) fn body(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: SharedString::from(""),
            description: None,
            control: div().into_any_element(),
            label_action: None,
            multiline: true,
            disabled: false,
            danger: false,
        }
    }

    /// The line on what the setting does.
    pub(crate) fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The control in the trailing lane.
    pub(crate) fn control(mut self, control: impl IntoElement) -> Self {
        self.control = control.into_any_element();
        self
    }

    /// An inline affordance after the label text (web label-adjacent icon).
    pub(crate) fn label_action(mut self, action: impl IntoElement) -> Self {
        self.label_action = Some(action.into_any_element());
        self
    }

    /// Stack the label over full-width content instead of the trailing lane.
    #[allow(dead_code)]
    pub(crate) fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// The component pipeline's disabled item: reduced opacity. Controls also
    /// take the flag so the interaction (not just the look) turns off.
    #[allow(dead_code)]
    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Read the label in the theme's danger color (statement rows only).
    #[allow(dead_code)]
    pub(crate) fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    fn render_row(self, divider: bool, edge_top: bool, edge_bottom: bool, cx: &App) -> AnyElement {
        let theme = cx.theme();
        // A softer derivative of the border token: the card outline keeps the
        // full-strength hairline (style.md 「卡内行间发丝线比卡描边更淡」).
        let divider_color = theme.border.opacity(0.6);
        let radius = theme.radius_lg;
        let label = self.label.clone();
        let row_id = self.id.clone();
        let has_label = !label.is_empty() || self.description.is_some();
        let label_action = self.label_action;
        let label_block = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(self.danger, |label| label.text_color(theme.danger))
                    .child(label)
                    .children(label_action),
            )
            .children(self.description.map(|text| {
                div()
                    .debug_selector(move || format!("settings-row-{row_id}-desc"))
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text)
            }));
        let row = div()
            .id(self.id.clone())
            .debug_selector(move || format!("settings-row-{}", self.id))
            .flex()
            .gap_4()
            .px_4()
            .py_3()
            .when(divider, |row| row.border_t_1().border_color(divider_color))
            .when(self.disabled, |row| row.opacity(0.5))
            .hover(move |mut style| {
                if edge_top {
                    style.corner_radii.top_left = Some(radius.into());
                    style.corner_radii.top_right = Some(radius.into());
                }
                if edge_bottom {
                    style.corner_radii.bottom_left = Some(radius.into());
                    style.corner_radii.bottom_right = Some(radius.into());
                }
                style.bg(theme.list_hover)
            })
            .when(has_label, |row| row.child(label_block));
        if self.multiline {
            row.flex_col()
                .child(div().w_full().child(self.control))
                .into_any_element()
        } else {
            row.items_center()
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .child(self.control),
                )
                .into_any_element()
        }
    }
}

/// One flat card: optional head (title, description, trailing action slot)
/// over edge-to-edge setting rows. Ground, border, and radius all come from
/// the active theme.
#[derive(IntoElement)]
pub(crate) struct SettingsCard {
    anchor: SharedString,
    title: Option<SharedString>,
    description: Option<SharedString>,
    extra: Option<AnyElement>,
    rows: Vec<SettingsCardRow>,
}

impl SettingsCard {
    /// A card whose test anchor is `anchor` (the migrated pages keep the
    /// `settings-section-*` selector contract).
    pub(crate) fn new(anchor: impl Into<SharedString>) -> Self {
        Self {
            anchor: anchor.into(),
            title: None,
            description: None,
            extra: None,
            rows: Vec::new(),
        }
    }

    /// The card head title.
    // Head builders serve the P2 page migrations; the untitled General card
    // (web has no sections on `application.general`) is the only consumer
    // until then.
    #[allow(dead_code)]
    pub(crate) fn title(mut self, text: impl Into<SharedString>) -> Self {
        self.title = Some(text.into());
        self
    }

    /// The line of context under the title.
    #[allow(dead_code)]
    pub(crate) fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// A quiet action in the head's trailing corner (「管理…」/「重置」).
    #[allow(dead_code)]
    pub(crate) fn extra(mut self, extra: impl IntoElement) -> Self {
        self.extra = Some(extra.into_any_element());
        self
    }

    /// A row after the others.
    pub(crate) fn row(mut self, row: SettingsCardRow) -> Self {
        self.rows.push(row);
        self
    }
}

impl RenderOnce for SettingsCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let has_head = self.title.is_some() || self.description.is_some() || self.extra.is_some();
        let row_count = self.rows.len();
        div()
            .debug_selector({
                let anchor = self.anchor.clone();
                move || format!("settings-card-{anchor}")
            })
            .w_full()
            .flex()
            .flex_col()
            .bg(theme.group_box)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .when(has_head, |card| {
                card.child(
                    div().px_4().pt_4().pb_3().child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .children(self.title.map(|title| {
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(title)
                                    }))
                                    .children(self.description.map(|text| {
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child(text)
                                    })),
                            )
                            .children(self.extra),
                    ),
                )
            })
            .children(
                self.rows
                    .into_iter()
                    .enumerate()
                    .map(|(ix, row)| row.render_row(ix > 0, ix == 0, ix + 1 == row_count, cx)),
            )
    }
}

/// Runs an element constructor during the element-tree pass instead of
/// `ShellWindow::render`. The shell entity cannot be read while its own
/// render is on the stack; controls that need the window state (the update
/// check row reads the shell's update state) or window-keyed input state
/// (the text and number controls) defer like the component pipeline's
/// `SettingItem::render` callbacks do.
#[derive(IntoElement)]
pub(crate) struct DeferredControl<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    pub(crate) construct: F,
}

impl<F> RenderOnce for DeferredControl<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.construct)(window, cx)
    }
}

/// The card-path renderer: every settings entry renders through the shell
/// card primitives (the component `Settings` pipeline is retired).
pub(crate) fn render_card_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    entry: SettingsEntry,
    cx: &mut App,
) -> AnyElement {
    let preamble = page_preamble(entry, text, view.clone(), cx);
    let cards = match entry {
        SettingsEntry::General => general_cards(text, view, cx),
        SettingsEntry::Appearance => appearance_cards(text, cx),
        SettingsEntry::Terminal => terminal_cards(text, view, cx),
        SettingsEntry::SessionSearch => session_search_cards(text, view),
        SettingsEntry::WorktreesGit => worktrees_git_cards(text, cx),
        SettingsEntry::Shortcuts => shortcuts_cards(text, cx),
        SettingsEntry::Models => models_cards(text, cx),
        SettingsEntry::Memory => memory_cards(text, view, cx),
        SettingsEntry::WebSearch => websearch_cards(text, view, cx),
        SettingsEntry::Usage => usage_cards(text, view),
        SettingsEntry::Archived => archived_cards(text, view),
        SettingsEntry::Diagnostics => diagnostics_cards(text, cx),
        SettingsEntry::Permissions => permissions_cards(text, view, cx),
        SettingsEntry::Automation => automation_cards(text, cx),
        SettingsEntry::Mcp => crate::manage::mcp_cards(text, view, cx),
        // The manage pages keep their scene layouts: skills renders the facet
        // rail beside its list card under the shared header; agents keeps the
        // standard card column with its controls row as the preamble.
        SettingsEntry::Skills => {
            let body = crate::manage::skills_body(text, view, cx);
            return page_frame_body(entry, text, preamble, body, cx);
        }
        SettingsEntry::Agents => crate::manage::agents_cards(text, view, cx),
        // The ACP page switches views above its cards: the local registry and
        // the advanced-JSON editor are two states of one page (web TabGroup),
        // so the whole body goes through the custom-body frame.
        SettingsEntry::ExternalAgents => {
            let body = crate::acp::acp_page_body(text, cx);
            return page_frame_body(entry, text, preamble, body, cx);
        }
    };
    page_frame(entry, text, preamble, cards, cx)
}

/// Elements rendered between the page header and the first card — the
/// shortcuts page's read-only note, the memory scope notice, the diagnostics
/// receipts, and the agents page's control row.
fn page_preamble(
    entry: SettingsEntry,
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &App,
) -> Vec<AnyElement> {
    match entry {
        SettingsEntry::Shortcuts => {
            vec![crate::settings::info_note(&text.shortcuts_fixed_note, cx)]
        }
        // The memory page's scope notice, then the inline receipt standing in
        // for the web toasts while one is up.
        SettingsEntry::Memory => {
            let mut parts = vec![crate::settings::info_note(&text.memory_scope_notice, cx)];
            if let Some(receipt) = memory_notice_element(cx) {
                parts.push(receipt);
            }
            parts
        }
        // The diagnostics page's export receipt (the web toasts it; this
        // surface shows it above the card), then the previous-run banner when
        // the last run ended badly — the web's order too (message, alert).
        SettingsEntry::Diagnostics => {
            let mut parts: Vec<AnyElement> = Vec::new();
            if let Some((succeeded, message)) = DiagnosticsNotice::current(cx) {
                parts.push(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_color(if succeeded {
                            cx.theme().foreground
                        } else {
                            cx.theme().danger
                        })
                        .child(message)
                        .into_any_element(),
                );
            }
            if let Some(previous) = crate::diagnostics::previous_unexpected_exit() {
                parts.push(previous_exit_notice(&previous, cx));
            }
            parts
        }
        // The agents page's search / source / type / create controls (web's
        // catalog toolbar sits in the same position).
        SettingsEntry::Agents => vec![crate::manage::agents_controls(text, view, cx)],
        _ => Vec::new(),
    }
}

/// The page frame: the web `ConfigPageHeader` geometry over a scrollable card
/// column (the card path owns its scrolling; the component pipeline does not).
fn page_frame(
    entry: SettingsEntry,
    text: &ShellText,
    preamble: Vec<AnyElement>,
    cards: Vec<SettingsCard>,
    cx: &App,
) -> AnyElement {
    page_frame_body(
        entry,
        text,
        preamble,
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children(cards)
            .into_any_element(),
        cx,
    )
}

/// The page frame over a custom body — the skills page lays out its facet
/// rail beside the list card under the shared header.
fn page_frame_body(
    entry: SettingsEntry,
    text: &ShellText,
    preamble: Vec<AnyElement>,
    body: AnyElement,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    div()
        .id("settings-card-page")
        .debug_selector(|| "settings-card-page".to_string())
        .flex_1()
        .min_h_0()
        .min_w_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_4()
                .pt_5()
                .pb_0()
                .mb_5()
                .child(
                    div()
                        .text_size(px(24.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.foreground)
                        .child(entry_label(entry, text).to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(entry_description(entry, text).to_string()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_4()
                .px_4()
                .pb_8()
                .children(preamble)
                .child(body),
        )
        .into_any_element()
}

/// A controlled switch wired to the page's getter/setter pair — the same
/// semantics the component fields use (checked from the getter each frame,
/// the click reports the next value; the setter owns persistence/refresh).
fn switch_control(
    id: impl Into<SharedString>,
    checked: bool,
    on_change: impl Fn(bool, &mut App) + 'static,
    disabled: bool,
) -> AnyElement {
    Switch::new(id.into())
        .checked(checked)
        .disabled(disabled)
        .on_click(move |checked: &bool, _, cx: &mut App| on_change(*checked, cx))
        .into_any_element()
}

/// A dropdown button in the shell's `Button + caret + menu` shape, showing
/// `current`'s label and reporting the picked value.
pub(crate) fn dropdown_control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    options: Rc<Vec<(SharedString, SharedString)>>,
    on_select: Rc<dyn Fn(SharedString, &mut App)>,
    disabled: bool,
) -> AnyElement {
    dropdown_control_windowed(
        id,
        label,
        options,
        Rc::new(move |value, _window, cx| on_select(value, cx)),
        disabled,
    )
}

/// [`dropdown_control`] with the window handed to the selection callback, for
/// choices that open a confirmation dialog (the full-access permission mode).
fn dropdown_control_windowed(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    options: Rc<Vec<(SharedString, SharedString)>>,
    on_select: Rc<dyn Fn(SharedString, &mut Window, &mut App)>,
    disabled: bool,
) -> AnyElement {
    Button::new(id.into())
        .label(label)
        .dropdown_caret(true)
        .outline()
        .disabled(disabled)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _window, _cx| {
            let mut menu = menu;
            for (value, option_label) in options.iter() {
                let value = value.clone();
                let on_select = on_select.clone();
                menu = menu.item(PopupMenuItem::new(option_label.clone()).on_click(
                    move |_, window, cx| {
                        on_select(value.clone(), window, cx);
                    },
                ));
            }
            menu
        })
        .into_any_element()
}

/// Bounds and stepping for [`number_input_control`] — the same engine config
/// the component pipeline's number field applies: `+`/`-` and Up/Down step by
/// `step`, out-of-range text is tolerated while typing and clamped on blur.
#[derive(Clone, Copy)]
pub(crate) struct NumberRange {
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) step: f64,
}

struct StringFieldState {
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// A text input wired to the row's value — the component pipeline's string
/// field semantics: the engine state is keyed by the row id, every change
/// reports, and the displayed text follows the setting when it changes
/// elsewhere. The row label doubles as the input's accessible name.
/// [`text_input_control`] with a visible placeholder (the web search field).
pub(crate) fn search_input_control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    value: SharedString,
    on_change: impl Fn(SharedString, &mut App) + 'static,
) -> DeferredControl<impl FnOnce(&mut Window, &mut App) -> AnyElement> {
    let key = SharedString::from(format!("card-string-{}", id.into()));
    let label: SharedString = label.into();
    let placeholder: SharedString = placeholder.into();
    DeferredControl {
        construct: move |window, cx| {
            let state_entity = window.use_keyed_state(key.clone(), cx, {
                let value = value.clone();
                let placeholder = placeholder.clone();
                let on_change = Rc::new(on_change);
                move |window, cx| {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value.clone())
                            .placeholder(placeholder.clone())
                    });
                    let _subscription = cx.subscribe(&input, {
                        move |_, input, event: &InputEvent, cx| match event {
                            InputEvent::Change => {
                                let value = input.read(cx).value();
                                on_change(value.into(), cx);
                            }
                            _ => {}
                        }
                    });
                    StringFieldState {
                        input,
                        _subscription,
                    }
                }
            });
            state_entity.update(cx, |state, cx| {
                if state.input.read(cx).value() != value {
                    state.input.update(cx, |input, cx| {
                        input.set_value(value.clone(), window, cx);
                    });
                }
            });
            let state = state_entity.read(cx);
            Input::new(&state.input)
                .aria_label(label)
                .w_64()
                .into_any_element()
        },
    }
}

fn text_input_control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    value: SharedString,
    on_change: impl Fn(SharedString, &mut App) + 'static,
) -> DeferredControl<impl FnOnce(&mut Window, &mut App) -> AnyElement> {
    let key = SharedString::from(format!("card-string-{}", id.into()));
    let label: SharedString = label.into();
    DeferredControl {
        construct: move |window, cx| {
            let state_entity = window.use_keyed_state(key.clone(), cx, {
                let value = value.clone();
                let on_change = Rc::new(on_change);
                move |window, cx| {
                    let input =
                        cx.new(|cx| InputState::new(window, cx).default_value(value.clone()));
                    let _subscription = cx.subscribe(&input, {
                        move |_, input, event: &InputEvent, cx| match event {
                            InputEvent::Change => {
                                let value = input.read(cx).value();
                                on_change(value.into(), cx);
                            }
                            _ => {}
                        }
                    });
                    StringFieldState {
                        input,
                        _subscription,
                    }
                }
            });
            state_entity.update(cx, |state, cx| {
                if state.input.read(cx).value() != value {
                    state.input.update(cx, |input, cx| {
                        input.set_value(value.clone(), window, cx);
                    });
                }
            });
            let state = state_entity.read(cx);
            Input::new(&state.input)
                .aria_label(label)
                .w_64()
                .into_any_element()
        },
    }
}

struct NumberFieldState {
    input: Entity<InputState>,
    initial_value: f64,
    _subscriptions: Vec<Subscription>,
}

/// A number input wired to the row's value — the component pipeline's number
/// field semantics: unparsable intermediates (`-`, ``, `1.`) stay so the next
/// keystroke can complete them, and the setting only ever receives a value
/// inside `min..=max`.
fn number_input_control(
    id: impl Into<SharedString> + 'static,
    label: impl Into<SharedString> + 'static,
    value: f64,
    range: NumberRange,
    on_change: impl Fn(f64, &mut App) + 'static,
    disabled: bool,
) -> DeferredControl<impl FnOnce(&mut Window, &mut App) -> AnyElement> {
    number_input_control_with_width(id, label, value, range, on_change, disabled, 128.)
}

/// [`number_input_control`] at a stated width. Six-digit context windows and
/// output caps do not fit the default lane: the field clips the head of the
/// value, so the row would show `30000` for a stored `300000`.
fn number_input_control_with_width(
    id: impl Into<SharedString> + 'static,
    label: impl Into<SharedString> + 'static,
    value: f64,
    range: NumberRange,
    on_change: impl Fn(f64, &mut App) + 'static,
    disabled: bool,
    width: f32,
) -> DeferredControl<impl FnOnce(&mut Window, &mut App) -> AnyElement> {
    let key = SharedString::from(format!("card-number-{}", id.into()));
    let label: SharedString = label.into();
    DeferredControl {
        construct: move |window, cx| {
            let state_entity = window.use_keyed_state(key.clone(), cx, {
                move |window, cx| {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value.to_string())
                            .step(range.step)
                            .min(range.min)
                            .max(range.max)
                    });
                    let _subscriptions = vec![cx.subscribe_in(&input, window, {
                        move |state: &mut NumberFieldState,
                              input,
                              event: &InputEvent,
                              _window,
                              cx| {
                            match event {
                                InputEvent::Change => {
                                    input.update(cx, |input, cx| {
                                        let text = input.value();
                                        if text == state.initial_value.to_string() {
                                            return;
                                        }
                                        // Out-of-range text stays too (the engine
                                        // clamps it on blur), but the setting only
                                        // ever receives a value inside min..=max.
                                        if let Ok(parsed) = text.parse::<f64>() {
                                            let clamped = parsed.clamp(range.min, range.max);
                                            on_change(clamped, cx);
                                            state.initial_value = clamped;
                                        }
                                    });
                                }
                                _ => {}
                            }
                        }
                    })];
                    NumberFieldState {
                        input,
                        initial_value: value,
                        _subscriptions,
                    }
                }
            });
            // Sync engine config and displayed value when options or the
            // underlying setting changed externally.
            state_entity.update(cx, |state, cx| {
                state.input.update(cx, |input, cx| {
                    input.set_step(Some(range.step.into()), window, cx);
                    input.set_min(Some(range.min), window, cx);
                    input.set_max(Some(range.max), window, cx);
                });
                if state.initial_value != value {
                    state.initial_value = value;
                    state.input.update(cx, |input, cx| {
                        input.set_value(SharedString::from(value.to_string()), window, cx);
                    });
                }
            });
            let state = state_entity.read(cx);
            NumberInput::new(&state.input)
                .accessibility_label(label)
                .disabled(disabled)
                .w(px(width))
                .into_any_element()
        },
    }
}

/// The 通用 page's cards, one per web `ConfigPageSection` in the web page's
/// order: 界面 (language), 启动与更新, 窗口与通知 (design README §2.4: card
/// boundary = section). The permission-request notification row lives on the
/// permissions page (§2.2). The version and manual update-check rows are shell
/// capabilities the web general page does not carry (§2.4); they close the
/// 启动与更新 card.
fn general_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
    // 语言：the three shell locales, then the persisted id verbatim when it is
    // outside that domain — the same menu the web interface section renders.
    let current_language = ShellConfig::current(cx).language.as_str().to_string();
    let choices = language_choices();
    let language_label = choices
        .iter()
        .find(|(value, _)| *value == current_language)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current_language.clone().into());
    let on_language_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_language(&value, cx));

    // Close behavior: the two values the shell renders, plus the persisted
    // value verbatim when it is outside that domain (the desktop's `ask`).
    let current_close = ShellConfig::current(cx).close_behavior_raw.clone();
    let mut close_options = vec![
        (
            SharedString::from(CLOSE_QUIT),
            SharedString::from(text.settings_close_quit.clone()),
        ),
        (
            SharedString::from(CLOSE_MINIMIZE),
            SharedString::from(text.settings_close_minimize.clone()),
        ),
    ];
    if current_close != CLOSE_QUIT && current_close != CLOSE_MINIMIZE {
        close_options.insert(
            0,
            (
                SharedString::from(current_close.clone()),
                SharedString::from(format!(
                    "{current_close} ({})",
                    text.value_unsupported.clone()
                )),
            ),
        );
    }
    let close_label = close_options
        .iter()
        .find(|(value, _)| *value == current_close)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current_close.clone().into());
    let close_options = Rc::new(close_options);
    let on_close_select: Rc<dyn Fn(SharedString, &mut App)> = Rc::new(|value: SharedString, cx| {
        let value = value.to_string();
        if value != CLOSE_QUIT && value != CLOSE_MINIMIZE {
            tracing::warn!("Ignoring unsupported close behavior: {value}");
            return;
        }
        cx.global_mut::<ShellConfig>().close_behavior_raw = value.clone();
        crate::shell::sync_close_quits(cx);
        crate::settings::persist(CLOSE_BEHAVIOR_PATH, serde_json::Value::String(value));
        cx.refresh_windows();
    });

    let autostart_checked = cx
        .try_global::<crate::settings::AutostartState>()
        .map(|state| state.0)
        .unwrap_or(false);
    let prevent_sleep_checked = cx
        .try_global::<crate::settings::PreventSleepState>()
        .map(|state| state.0)
        .unwrap_or(false);
    let auto_update_checked = ShellConfig::current(cx).auto_update;
    let notify_checked = ShellConfig::current(cx).dialog_completion_notify;

    vec![
        SettingsCard::new("general-interface")
            .title(text.settings_appearance_interface_title.clone())
            .description(text.settings_appearance_interface_description.clone())
            .row(
                SettingsCardRow::new("general-language", text.language.clone()).control(
                    dropdown_control(
                        "general-language-menu",
                        language_label,
                        Rc::new(choices),
                        on_language_select,
                        false,
                    ),
                ),
            ),
        SettingsCard::new("general-startup")
            .title(text.settings_general_startup_title.clone())
            .description(text.settings_general_startup_description.clone())
            .row(
                SettingsCardRow::new("general-launch-login", text.settings_launch_login.clone())
                    .description(text.settings_launch_login_description.clone())
                    .control(switch_control(
                        "general-launch-login-switch",
                        autostart_checked,
                        |value, cx| set_autostart(value, cx),
                        false,
                    )),
            )
            .row(
                SettingsCardRow::new("general-prevent-sleep", text.settings_prevent_sleep.clone())
                    .description(text.settings_prevent_sleep_description.clone())
                    .control(switch_control(
                        "general-prevent-sleep-switch",
                        prevent_sleep_checked,
                        |value, cx| set_prevent_sleep(value, cx),
                        false,
                    )),
            )
            .row(
                SettingsCardRow::new("general-auto-update", text.settings_auto_update.clone())
                    .description(text.settings_auto_update_description.clone())
                    .control(switch_control(
                        "general-auto-update-switch",
                        auto_update_checked,
                        |value, cx| {
                            cx.global_mut::<ShellConfig>().auto_update = value;
                            crate::settings::persist(
                                crate::settings::AUTO_UPDATE_PATH,
                                serde_json::Value::Bool(value),
                            );
                            cx.refresh_windows();
                        },
                        false,
                    )),
            )
            .row(
                SettingsCardRow::new("general-version", text.settings_version.clone()).control(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                ),
            )
            .row(
                SettingsCardRow::new("general-check-updates", text.settings_check_updates.clone())
                    // DeferredControl::render runs after `ShellWindow::render`
                    // has returned, so reading the shell state there is legal; the
                    // constructor captures owned clones because the closure is
                    // `'static` while `text` is only a borrow.
                    .control(DeferredControl {
                        construct: {
                            let view = view.clone();
                            let text = text.clone();
                            move |_, cx| crate::settings::update_check_field(view, &text, cx)
                        },
                    }),
            ),
        SettingsCard::new("general-window")
            .title(text.settings_general_window_title.clone())
            .description(text.settings_general_window_description.clone())
            .row(
                SettingsCardRow::new(
                    "general-close-behavior",
                    text.settings_close_behavior.clone(),
                )
                .description(text.settings_close_behavior_description.clone())
                .control(dropdown_control(
                    "general-close-behavior-menu",
                    close_label,
                    close_options,
                    on_close_select,
                    false,
                )),
            )
            .row(
                SettingsCardRow::new(
                    "general-notify-turn-done",
                    text.settings_notify_turn_done.clone(),
                )
                .description(text.settings_notify_turn_done_description.clone())
                .control(switch_control(
                    "general-notify-turn-done-switch",
                    notify_checked,
                    |value, cx| {
                        cx.global_mut::<ShellConfig>().dialog_completion_notify = value;
                        crate::settings::persist(
                            crate::settings::NOTIFY_TURN_DONE_PATH,
                            serde_json::Value::Bool(value),
                        );
                        cx.refresh_windows();
                    },
                    false,
                )),
            ),
    ]
}

/// The 外观 page's cards, one per web `ConfigPageSection` the shell covers:
/// 外观 (theme) and 字体大小 (size level plus the conditional custom size). The
/// web page's interface-language section belongs to 通用; the session-panel
/// section and the font preview row are registered E3 gaps (§2.2).
fn appearance_cards(text: &ShellText, cx: &mut App) -> Vec<SettingsCard> {
    // 字体大小：the shared levels, then the custom entry, then a stored id
    // outside the list so the menu shows what is persisted.
    let current_level = ShellConfig::current(cx).ui_font_level.clone();
    let level_options = ui_font_level_options(&current_level, text);
    let level_label = level_options
        .iter()
        .find(|(value, _)| *value == current_level)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current_level.clone().into());
    let on_level_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_ui_font_level(&value, cx));

    let mut font_card = SettingsCard::new("appearance-font")
        .title(text.settings_appearance_font_title.clone())
        .row(
            SettingsCardRow::new("appearance-font-level", text.settings_ui_font.clone())
                .description(text.settings_ui_font_description.clone())
                .control(dropdown_control(
                    "appearance-font-level-menu",
                    level_label,
                    Rc::new(level_options),
                    on_level_select,
                    false,
                )),
        );
    // The custom size row is the web page's conditional second row: only the
    // custom level reads it.
    if ShellConfig::current(cx).ui_font_level == UI_FONT_CUSTOM_LEVEL {
        font_card = font_card.row(
            SettingsCardRow::new(
                "appearance-font-custom",
                text.settings_ui_font_custom.clone(),
            )
            .description(text.settings_ui_font_custom_description.clone())
            .control(number_input_control(
                "appearance-font-custom-input",
                text.settings_ui_font_custom.clone(),
                f64::from(ui_font_base_px(
                    &ShellConfig::current(cx).ui_font_level,
                    ShellConfig::current(cx).ui_font_custom_px,
                )),
                NumberRange {
                    min: f64::from(MIN_UI_FONT_PX),
                    max: f64::from(MAX_UI_FONT_PX),
                    step: 1.,
                },
                set_ui_font_custom_px,
                false,
            )),
        );
    }

    vec![
        SettingsCard::new("appearance-theme")
            .title(text.settings_appearance.clone())
            .row(
                SettingsCardRow::new("appearance-theme", text.theme.clone())
                    .control(theme_button(text, cx)),
            ),
        font_card,
    ]
}

/// The 终端 page's card: the web page has a single `ConfigPageSection` (终端，
/// the same string as the page header), so the card stays untitled like the
/// General page's — and holds the web row (default shell) plus the shell-only
/// rows the component page carries: font size, family, scrollback, and the
/// launch-profile collection (design README §2.2).
fn terminal_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
    let font_size = f64::from(ShellConfig::current(cx).terminal.font_size);
    let scrollback = f64::from(ShellConfig::current(cx).terminal.scrollback);
    let font_family = SharedString::from(ShellConfig::current(cx).terminal.font_family.clone());
    let default_shell = SharedString::from(ShellConfig::current(cx).terminal.default_shell.clone());
    vec![SettingsCard::new("terminal")
        .row(
            SettingsCardRow::new(
                "terminal-font-size",
                text.settings_terminal_font_size.clone(),
            )
            .description(text.settings_terminal_font_size_description.clone())
            .control(number_input_control(
                "terminal-font-size-input",
                text.settings_terminal_font_size.clone(),
                font_size,
                NumberRange {
                    min: f64::from(MIN_FONT_SIZE),
                    max: f64::from(MAX_FONT_SIZE),
                    step: 1.,
                },
                set_terminal_font_size,
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "terminal-font-family",
                text.settings_terminal_font_family.clone(),
            )
            .description(text.settings_terminal_font_family_description.clone())
            .control(text_input_control(
                "terminal-font-family-input",
                text.settings_terminal_font_family.clone(),
                font_family,
                |value, cx| set_terminal_font_family(&value, cx),
            )),
        )
        .row(
            SettingsCardRow::new(
                "terminal-scrollback",
                text.settings_terminal_scrollback.clone(),
            )
            .description(text.settings_terminal_scrollback_description.clone())
            .control(number_input_control(
                "terminal-scrollback-input",
                text.settings_terminal_scrollback.clone(),
                scrollback,
                NumberRange {
                    min: f64::from(MIN_SCROLLBACK),
                    max: f64::from(MAX_SCROLLBACK),
                    step: 1000.,
                },
                set_terminal_scrollback,
                false,
            )),
        )
        .row(
            SettingsCardRow::new("terminal-shell", text.settings_terminal_shell.clone())
                .description(text.settings_terminal_shell_description.clone())
                .control(text_input_control(
                    "terminal-shell-input",
                    text.settings_terminal_shell.clone(),
                    default_shell,
                    |value, cx| set_terminal_default_shell(&value, cx),
                )),
        )
        .row(
            SettingsCardRow::new("terminal-profiles", text.settings_terminal_profiles.clone())
                .description(text.settings_terminal_profiles_description.clone())
                .multiline()
                .control(DeferredControl {
                    construct: {
                        let view = view.clone();
                        let text = text.clone();
                        move |window, cx| terminal_profiles_field(view, &text, window, cx)
                    },
                }),
        )]
}

/// The 会话与工作区 page's single card: the headless jump row that opens the
/// shell's search overlay — this shell's session-search owner (design README
/// §2.2). The default-execution-mode and session-title sections live on the
/// 会话与记忆 page, where the web `ai.session-memory` page keeps them.
fn session_search_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
) -> Vec<SettingsCard> {
    vec![SettingsCard::new("session-search-open").row(
        SettingsCardRow::new(
            "session-search-open",
            text.settings_jump_session_description.clone(),
        )
        .control(
            Button::new("open-session-search")
                .label(text.settings_jump_session_action.clone())
                .with_variant(ButtonVariant::Ghost)
                .on_click({
                    let view = view.clone();
                    move |_: &gpui_kit::ClickEvent, window, cx| {
                        if let Some(shell) = view.upgrade() {
                            shell.update(cx, |shell, cx| shell.open_search_overlay(window, cx));
                        }
                    }
                }),
        ),
    )]
}

/// The 工作区与 Git page's cards, one per web section
/// (`WorkspaceGitSettingsPage`): accelerated workspace search, commit
/// attribution, the worktree isolation defaults (G-SE16), and the host's
/// project/worktree catalog with its delete path. The catalog read is
/// triggered when the page is selected (see `ShellWindow::set_settings_page`).
fn worktrees_git_cards(text: &ShellText, cx: &mut App) -> Vec<SettingsCard> {
    let config = ShellConfig::current(cx);

    let search = SettingsCard::new("worktrees-search")
        .title(text.settings_workspace_search_title.clone())
        .description(text.settings_workspace_search_description.clone())
        .row(
            SettingsCardRow::new(
                "worktrees-search-enable",
                text.settings_workspace_search_enable.clone(),
            )
            .control(switch_control(
                "worktrees-search-enable-switch",
                config.workspace_search_enabled,
                set_workspace_search,
                false,
            )),
        );

    let attribution = SettingsCard::new("worktrees-attribution")
        .title(text.settings_git_attribution_title.clone())
        .description(text.settings_git_attribution_scope.clone())
        .row(
            SettingsCardRow::new(
                "worktrees-attribution-coauthor",
                text.settings_git_attribution_label.clone(),
            )
            .description(crate::settings::git_attribution_description(cx))
            .control(switch_control(
                "worktrees-attribution-coauthor-switch",
                config.git_commit_coauthor,
                set_git_commit_coauthor,
                false,
            )),
        );

    let document = config.worktrees.clone();
    let defaults = default_worktree_settings_document();
    let root_default = object_str(&defaults, "rootPath", "").to_string();
    let prefix_default = object_str(&defaults, "branchPrefix", "").to_string();
    let auto_delete_default = object_bool(&defaults, "autoDeleteEnabled", true);
    let limit_default = object_u64(&defaults, "autoDeleteLimit", 15);

    let root_value =
        SharedString::from(object_str(&document, "rootPath", &root_default).to_string());
    let prefix_value =
        SharedString::from(object_str(&document, "branchPrefix", &prefix_default).to_string());
    let copy_checked = object_bool(&document, "copyLocalChanges", false);
    let auto_delete_checked = object_bool(&document, "autoDeleteEnabled", auto_delete_default);
    let limit_value = object_u64(&document, "autoDeleteLimit", limit_default) as f64;

    let isolation = SettingsCard::new("worktrees-isolation")
        .title(text.settings_worktree_section.clone())
        .description(text.settings_worktree_section_note.clone())
        .extra(
            Button::new("worktrees-reset")
                .label(text.settings_worktree_reset.clone())
                .small()
                .outline()
                .disabled(worktree_settings_are_default(cx))
                .on_click(|_, _, cx| crate::settings::reset_worktree_settings(cx)),
        )
        .row(
            SettingsCardRow::new("worktrees-root", text.settings_worktree_root.clone())
                .description(text.settings_worktree_root_description.clone())
                .control(text_input_control(
                    "worktrees-root-input",
                    text.settings_worktree_root.clone(),
                    root_value,
                    |value, cx| set_worktree_text("rootPath", &value, cx),
                )),
        )
        .row(
            SettingsCardRow::new(
                "worktrees-branch-prefix",
                text.settings_worktree_branch_prefix.clone(),
            )
            .description(text.settings_worktree_branch_prefix_description.clone())
            .control(text_input_control(
                "worktrees-branch-prefix-input",
                text.settings_worktree_branch_prefix.clone(),
                prefix_value,
                |value, cx| set_worktree_text("branchPrefix", &value, cx),
            )),
        )
        .row(
            SettingsCardRow::new(
                "worktrees-copy-changes",
                text.settings_worktree_copy_changes.clone(),
            )
            .description(text.settings_worktree_copy_changes_description.clone())
            .control(switch_control(
                "worktrees-copy-changes-switch",
                copy_checked,
                |value, cx| set_worktree_switch("copyLocalChanges", value, cx),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "worktrees-auto-delete",
                text.settings_worktree_auto_delete.clone(),
            )
            .description(text.settings_worktree_auto_delete_description.clone())
            .control(switch_control(
                "worktrees-auto-delete-switch",
                auto_delete_checked,
                |value, cx| set_worktree_switch("autoDeleteEnabled", value, cx),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "worktrees-auto-delete-limit",
                text.settings_worktree_auto_delete_limit.clone(),
            )
            .description(text.settings_worktree_auto_delete_limit_description.clone())
            .disabled(!auto_delete_checked)
            .control(number_input_control(
                "worktrees-auto-delete-limit-input",
                text.settings_worktree_auto_delete_limit.clone(),
                limit_value,
                NumberRange {
                    min: f64::from(crate::settings::WORKTREE_AUTO_DELETE_MIN),
                    max: f64::from(crate::settings::WORKTREE_AUTO_DELETE_MAX),
                    step: 1.,
                },
                set_worktree_auto_delete_limit,
                !auto_delete_checked,
            )),
        );

    vec![
        search,
        attribution,
        isolation,
        worktrees_management_card(text, cx),
    ]
}

/// The web worktree management section: a refresh action in the head, then one
/// project block per open project with its managed worktrees. Empty, loading,
/// and failed reads each keep the section's single-card shape.
fn worktrees_management_card(text: &ShellText, cx: &App) -> SettingsCard {
    let state = WorktreeProjectsState::current(cx);
    let theme = cx.theme();
    let mut card = SettingsCard::new("worktrees-management")
        .title(text.settings_worktree_management_title.clone())
        .description(text.settings_worktree_management_description.clone())
        .extra(
            Button::new("worktrees-management-refresh")
                .icon(IconName::RefreshCw)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.settings_worktree_management_refresh.clone())
                .disabled(state.loading || state.removing.is_some())
                .on_click(|_, _, cx| crate::settings::fetch_worktree_projects(cx)),
        );

    if let Some((code, _message)) = state.error.clone() {
        let message: SharedString = if code.is_empty() {
            text.settings_worktree_management_load_failed.clone().into()
        } else {
            worktree_error_label(&code, text)
        };
        card = card.row(
            SettingsCardRow::new("worktrees-management-error", message)
                .danger(true)
                .control(
                    Button::new("worktrees-management-retry")
                        .label(text.settings_worktree_management_retry.clone())
                        .small()
                        .outline()
                        .on_click(|_, _, cx| crate::settings::fetch_worktree_projects(cx)),
                ),
        );
    }

    if state.projects.is_empty() {
        // The failed read already rendered its message and retry action; the
        // quiet empty state stays away until a read succeeds (web keeps the
        // same gate).
        if state.error.is_none() {
            if state.loading && !state.loaded {
                card = card.row(
                    SettingsCardRow::body("worktrees-management-loading").control(
                        div()
                            .w_full()
                            .py_6()
                            .flex()
                            .justify_center()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(text.settings_worktree_management_loading.clone()),
                    ),
                );
            } else if state.loaded {
                card = card.row(
                    SettingsCardRow::body("worktrees-management-empty").control(
                        div()
                            .w_full()
                            .py_6()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(IconName::Folder)
                                    .small()
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(text.settings_worktree_management_empty.clone()),
                            ),
                    ),
                );
            }
        }
        return card;
    }

    for project in &state.projects {
        card = card.row(
            SettingsCardRow::body(format!(
                "worktrees-project-{}",
                project.project_workspace_path
            ))
            .control(worktree_project_header(project, cx)),
        );
        for worktree in &project.worktrees {
            card = card.row(
                SettingsCardRow::body(format!("worktrees-row-{}", worktree.worktree_id))
                    .control(worktree_management_row(project, worktree, &state, text, cx)),
            );
        }
    }
    card
}

/// One project block head: the directory name, the full path (the web's code
/// line), and the managed-worktree count.
fn worktree_project_header(project: &WorktreeProjectSummary, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let name = project
        .project_workspace_path
        .replace('\\', "/")
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| project.project_workspace_path.clone());
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(project.project_workspace_path.clone()),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::worktree_count_label(
                    project.worktrees.len(),
                    cx,
                )),
        )
        .into_any_element()
}

/// One worktree row: the branch (or the detached-commit label), the lifecycle
/// and state facts, the checkout path, the bound-session count, and the
/// delete action. Locked and missing checkouts refuse deletion with the web's
/// block reason instead of failing after the confirmation.
fn worktree_management_row(
    project: &WorktreeProjectSummary,
    worktree: &WorktreeSummary,
    state: &WorktreeProjectsState,
    text: &ShellText,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let title = worktree
        .branch
        .clone()
        .unwrap_or_else(|| crate::i18n::worktree_detached_label(&worktree.head, cx));

    let mut badges: Vec<String> = Vec::new();
    match worktree.lifecycle {
        WorktreeLifecycle::Managed => {}
        WorktreeLifecycle::Permanent => {
            badges.push(text.settings_worktree_lifecycle_permanent.clone())
        }
        WorktreeLifecycle::External => {
            badges.push(text.settings_worktree_lifecycle_external.clone())
        }
    }
    if worktree.dirty {
        badges.push(text.settings_worktree_state_dirty.clone());
    }
    if worktree.has_unpublished_commits {
        badges.push(text.settings_worktree_state_unpublished.clone());
    }
    if worktree.locked {
        badges.push(text.settings_worktree_state_locked.clone());
    }
    if worktree.missing {
        badges.push(text.settings_worktree_state_missing.clone());
    }

    let block_reason = if worktree.locked {
        Some(text.settings_worktree_protection_locked.clone())
    } else if worktree.missing {
        Some(text.settings_worktree_protection_missing.clone())
    } else {
        None
    };
    let delete_disabled = block_reason.is_some() || state.removing.is_some();

    let project_id = project.project_workspace_id.clone();
    let project_path = project.project_workspace_path.clone();
    let facts = WorktreeDeleteFacts {
        worktree_id: worktree.worktree_id.clone(),
        path: worktree.path.clone(),
        discard_local_work: worktree.dirty || worktree.has_unpublished_commits,
        has_sessions: worktree.associated_session_count > 0,
    };
    let delete = Button::new(format!("worktrees-delete-{}", worktree.worktree_id))
        .icon(IconName::Trash)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .tooltip(
            block_reason
                .clone()
                .unwrap_or_else(|| text.settings_worktree_delete_action.clone()),
        )
        .disabled(delete_disabled)
        .on_click(move |_, window, cx| {
            open_worktree_delete_confirm(
                window,
                cx,
                project_id.clone(),
                project_path.clone(),
                facts.clone(),
            );
        });

    div()
        .w_full()
        .flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.foreground)
                                .truncate()
                                .child(title),
                        )
                        .when(!badges.is_empty(), |line| {
                            line.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(badges.join(" · ")),
                            )
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(worktree.path.clone()),
                )
                .when(worktree.associated_session_count > 0, |column| {
                    column.child(div().text_xs().text_color(theme.muted_foreground).child(
                        crate::i18n::worktree_management_sessions_label(
                            worktree.associated_session_count,
                            cx,
                        ),
                    ))
                }),
        )
        .child(div().flex_shrink_0().child(delete))
        .into_any_element()
}

/// The facts one row's delete confirmation needs after the render pass ends.
#[derive(Clone)]
struct WorktreeDeleteFacts {
    worktree_id: String,
    path: String,
    /// The worktree carries local work: the web confirmation switches to the
    /// discard wording and the removal forces through.
    discard_local_work: bool,
    has_sessions: bool,
}

/// Opens the web delete confirmation (`ConfirmDialog` in
/// `WorktreeSettingsSection`). Only the confirm path removes; dismissing the
/// dialog leaves the checkout untouched.
fn open_worktree_delete_confirm(
    window: &mut Window,
    cx: &mut App,
    project_workspace_id: Option<String>,
    project_workspace_path: String,
    facts: WorktreeDeleteFacts,
) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = crate::i18n::current(cx);
        let force = facts.discard_local_work;
        let title = if force {
            text.settings_worktree_delete_force_title.clone()
        } else {
            text.settings_worktree_delete_title.clone()
        };
        let message = match (force, facts.has_sessions) {
            (true, true) => text.settings_worktree_delete_force_message_sessions.clone(),
            (true, false) => text.settings_worktree_delete_force_message.clone(),
            (false, true) => text.settings_worktree_delete_message_sessions.clone(),
            (false, false) => text.settings_worktree_delete_message.clone(),
        };
        let confirm_label = if force {
            text.settings_worktree_delete_force_action.clone()
        } else {
            text.settings_worktree_delete_action.clone()
        };
        let confirm_id = project_workspace_id.clone();
        let confirm_path = project_workspace_path.clone();
        let confirm_worktree = facts.worktree_id.clone();
        dialog
            .title(div().child(title))
            .child(
                div()
                    .w(px(380.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(message),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().muted_foreground)
                            .child(facts.path.clone()),
                    ),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("worktree-delete-cancel")
                            .label(text.settings_worktree_delete_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("worktree-delete-confirm")
                            .label(confirm_label)
                            .with_variant(ButtonVariant::Danger)
                            .on_click(move |_, window, cx| {
                                crate::settings::remove_worktree(
                                    confirm_id.clone(),
                                    confirm_path.clone(),
                                    confirm_worktree.clone(),
                                    force,
                                    cx,
                                );
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}

/// The 快捷键 page's cards, one per web section (global / overlays / terminal
/// / editing), each a read-only keycap table. The page-level read-only note
/// renders above the cards via [`page_preamble`].
fn shortcuts_cards(text: &ShellText, cx: &App) -> Vec<SettingsCard> {
    let mut global =
        SettingsCard::new("shortcuts-global").title(text.shortcuts_section_global.clone());
    for shortcut in GLOBAL_SHORTCUTS {
        global = global.row(
            SettingsCardRow::new(
                format!("shortcut-{}", shortcut.stroke),
                (shortcut.label)(text),
            )
            .control(shortcut_keycaps(&[shortcut.stroke.to_string()], cx)),
        );
    }

    let overlays = SettingsCard::new("shortcuts-overlays")
        .title(text.shortcuts_section_overlays.clone())
        .row(
            SettingsCardRow::new("shortcut-escape", text.shortcuts_overlay_dismiss.clone())
                .control(shortcut_keycaps(&["escape".to_string()], cx)),
        )
        .row(
            SettingsCardRow::new("shortcut-up", text.shortcuts_overlay_move.clone()).control(
                shortcut_keycaps(&["up".to_string(), "down".to_string()], cx),
            ),
        )
        .row(
            SettingsCardRow::new("shortcut-enter", text.shortcuts_overlay_confirm.clone())
                .control(shortcut_keycaps(&["enter".to_string()], cx)),
        );

    let terminal = SettingsCard::new("shortcuts-terminal")
        .title(text.shortcuts_section_terminal.clone())
        .row(
            SettingsCardRow::new(
                "shortcut-ctrl-shift-c",
                text.shortcuts_terminal_copy.clone(),
            )
            .control(shortcut_keycaps(&["ctrl-shift-c".to_string()], cx)),
        )
        .row(
            SettingsCardRow::new(
                "shortcut-ctrl-shift-v",
                text.shortcuts_terminal_paste.clone(),
            )
            .control(shortcut_keycaps(&["ctrl-shift-v".to_string()], cx)),
        )
        .row(SettingsCardRow::new(
            "shortcuts-terminal-note",
            text.shortcuts_terminal_note.clone(),
        ));

    vec![
        global,
        overlays,
        terminal,
        SettingsCard::new("shortcuts-editing")
            .title(text.shortcuts_section_editing.clone())
            .row(SettingsCardRow::new(
                "shortcuts-editing-note",
                text.shortcuts_editing_note.clone(),
            )),
    ]
}

/// The 模型 page's cards, one per web `ConfigPageSection` (README §2.4):
/// 添加或导入模型 / 模型选用模式 / 全部模型 / 更多设置, plus the inline editor
/// (the web opens a modal) and the new-model form. The web `ModelPoolsSection`
/// (routing pools) and the subscription manager have no shell capability yet;
/// `checklist-models.md` §3 registers both.
fn models_cards(text: &ShellText, cx: &mut App) -> Vec<SettingsCard> {
    let config = ShellConfig::current(cx);
    let models = config.ai_models.clone();
    let state = ModelsPageState::current(cx);

    let mut cards = vec![
        models_acquisition_card(text, &models, cx),
        models_selection_modes_card(text, cx),
        models_pool_card(text, &models, &state, cx),
    ];

    if state.create.is_some() {
        cards.push(models_create_card(text, &state));
    }
    cards.extend(models_editor_cards(text, &models, cx));

    cards.push(models_more_card(text, &state));
    if state.more_open {
        cards.push(models_stream_card(text, cx));
        cards.push(models_proxy_card(text, cx));
    }
    cards
}

/// The row title the web builds for a model (`getModelDisplayName`): the
/// provider's config name over the model slug, falling back to whichever half
/// exists.
fn model_display_label(model: &openbitfun_core::service::config::types::AIModelConfig) -> String {
    let name = model.name.trim();
    let model_name = model.model_name.trim();
    match (name.is_empty(), model_name.is_empty()) {
        (false, false) => format!("{name}/{model_name}"),
        (false, true) => name.to_string(),
        (true, false) => model_name.to_string(),
        (true, true) => model.id.clone(),
    }
}

/// The category badge text (web `category.*`).
fn model_category_label(
    model: &openbitfun_core::service::config::types::AIModelConfig,
    text: &ShellText,
) -> String {
    let category = serde_json::to_value(&model.category)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    match category.as_str() {
        "multimodal" => text.models_category_multimodal.clone(),
        "image_generation" => text.models_category_image_generation.clone(),
        "speech_recognition" => text.models_category_speech_recognition.clone(),
        "general_chat" => text.models_category_general_chat.clone(),
        other => other.to_string(),
    }
}

/// A trailing-lane neutral badge (the web's `StatusPill` for 「尚未开放」).
fn models_muted_badge(label: SharedString, cx: &App) -> AnyElement {
    div()
        .flex_shrink_0()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .px_2()
        .py_0p5()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(label)
        .into_any_element()
}

/// Opens the inline editor on one model row.
fn open_model_editor(model_id: String, cx: &mut App) {
    ModelEditorState::mutate(cx, |state| {
        state.model_id = Some(model_id);
        state.drafts_ready = false;
        state.headers_text.clear();
        state.body_text.clear();
    });
    cx.refresh_windows();
}

/// The 添加或导入模型 section: the API connection pills (click opens the
/// service's first model, the trailing icon deletes the whole service) with
/// the add button, and the subscriptions row. Subscription sign-in and quotas
/// have no shell manager yet, so that row says so instead of faking an import.
fn models_acquisition_card(
    text: &ShellText,
    models: &[openbitfun_core::service::config::types::AIModelConfig],
    cx: &App,
) -> SettingsCard {
    let groups = model_provider_groups(models);
    let summary = if groups.is_empty() {
        text.models_acquisition_none.clone()
    } else {
        crate::i18n::models_acquisition_summary(groups.len(), models.len(), cx)
    };

    let mut body = div().flex().flex_col().items_start().gap_2().w_full();
    if !groups.is_empty() {
        let mut pills = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_2()
            .w_full();
        for group in &groups {
            pills = pills.child(models_provider_pill(group, text, cx));
        }
        body = body.child(pills);
    }
    body = body.child(
        Button::new("models-add-model")
            .label(text.models_add.clone())
            .icon(IconName::Plus)
            .small()
            .with_variant(ButtonVariant::Secondary)
            .on_click(|_, _, cx| crate::settings::open_model_create(cx)),
    );

    SettingsCard::new("models-acquisition")
        .title(text.models_acquisition_title.clone())
        .row(
            SettingsCardRow::new(
                "models-acquisition-api",
                text.models_acquisition_api.clone(),
            )
            .description(summary)
            .multiline()
            .control(body),
        )
        .row(
            SettingsCardRow::new(
                "models-acquisition-subscriptions",
                text.models_acquisition_subscriptions.clone(),
            )
            .description(text.models_subscriptions_unavailable.clone()),
        )
}

/// One configured service: name button + delete icon (web
/// `openbitfun-model-settings__source-pill`).
fn models_provider_pill(group: &ModelProviderGroup, text: &ShellText, cx: &App) -> AnyElement {
    let open_id = group.model_ids.first().cloned();
    let delete_group = group.clone();
    let delete_tooltip = text.models_delete_provider.clone();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .pl_2()
        .pr_1()
        .py_0p5()
        .child(
            Button::new(SharedString::from(format!(
                "models-provider-open-{}",
                group.key
            )))
            .label(group.name.clone())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(group.base_url.clone())
            .on_click(move |_, _, cx| {
                if let Some(model_id) = open_id.clone() {
                    open_model_editor(model_id, cx);
                }
            }),
        )
        .child(
            Button::new(SharedString::from(format!(
                "models-provider-delete-{}",
                group.key
            )))
            .icon(IconName::Trash)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .tooltip(delete_tooltip)
            .on_click(move |_, window, cx| {
                open_provider_delete_confirm(window, cx, delete_group.clone());
            }),
        )
        .into_any_element()
}

/// The web `requestFormatOptions` labels for the provider picker, plus a
/// stored value this build does not know (kept verbatim).
fn model_provider_options(stored: &str) -> Vec<(SharedString, SharedString)> {
    let mut options: Vec<(SharedString, SharedString)> = MODEL_PROVIDER_FORMATS
        .iter()
        .map(|(value, label)| (SharedString::from(*value), SharedString::from(*label)))
        .collect();
    if !stored.is_empty() && !options.iter().any(|(value, _)| value.as_ref() == stored) {
        options.push((
            SharedString::from(stored.to_string()),
            SharedString::from(stored.to_string()),
        ));
    }
    options
}

/// The 模型选用模式 section: the web renders only the smart-mode row, with a
/// neutral 「尚未开放」 pill (battery copy exists but is not rendered).
fn models_selection_modes_card(text: &ShellText, cx: &App) -> SettingsCard {
    SettingsCard::new("models-selection-modes")
        .title(text.models_selection_modes_title.clone())
        .row(
            SettingsCardRow::new(
                "models-selection-modes-smart",
                text.models_mode_smart.clone(),
            )
            .description(text.models_mode_smart_hint.clone())
            .control(models_muted_badge(
                text.models_mode_unavailable.clone().into(),
                cx,
            )),
        )
}

/// One default-model slot (web `DefaultModelConfig`): enabled models with the
/// slot's capability; the optional slots carry a 「未设置」 entry.
fn model_default_slot_row(
    id: &'static str,
    label: String,
    description: String,
    clear_label: Option<String>,
    capability: openbitfun_core::service::config::types::ModelCapability,
    stored: Option<String>,
    models: &[openbitfun_core::service::config::types::AIModelConfig],
) -> SettingsCardRow {
    let current = stored.unwrap_or_default();
    let mut options: Vec<(SharedString, SharedString)> = Vec::new();
    if let Some(clear_label) = clear_label {
        options.push((SharedString::from(""), clear_label.into()));
    }
    for model in models
        .iter()
        .filter(|model| model_supports_capability(model, capability.clone()))
    {
        options.push((
            SharedString::from(model.id.clone()),
            SharedString::from(model_display_label(model)),
        ));
    }
    if !current.is_empty() && !options.iter().any(|(value, _)| value.as_ref() == current) {
        options.push((
            SharedString::from(current.clone()),
            SharedString::from(current.clone()),
        ));
    }
    let shown = options
        .iter()
        .find(|(value, _)| value.as_ref() == current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current.clone().into());
    let on_select: Rc<dyn Fn(SharedString, &mut App)> = match id {
        "models-pool-primary" => {
            Rc::new(|value: SharedString, cx: &mut App| set_default_primary_model(&value, cx))
        }
        "models-pool-fast" => Rc::new(|value: SharedString, cx: &mut App| {
            set_default_model_slot(DEFAULT_MODEL_FAST_SLOT, &value, cx)
        }),
        "models-pool-image" => Rc::new(|value: SharedString, cx: &mut App| {
            set_default_model_slot(DEFAULT_MODEL_IMAGE_SLOT, &value, cx)
        }),
        _ => Rc::new(|value: SharedString, cx: &mut App| {
            set_default_model_slot(DEFAULT_MODEL_SPEECH_SLOT, &value, cx)
        }),
    };
    SettingsCardRow::new(id, label.clone())
        .description(description)
        .control(dropdown_control(
            SharedString::from(format!("{id}-menu")),
            shown,
            Rc::new(options),
            on_select,
            false,
        ))
}

/// The 全部模型 section: the four default slots, the search/filter toolbar,
/// and the model rows (web `sections.pool` + `DefaultModelConfig`).
fn models_pool_card(
    text: &ShellText,
    models: &[openbitfun_core::service::config::types::AIModelConfig],
    state: &ModelsPageState,
    cx: &App,
) -> SettingsCard {
    let config = ShellConfig::current(cx);
    SettingsCard::new("models-pool")
        .title(text.models_pool_title.clone())
        .row(model_default_slot_row(
            "models-pool-primary",
            text.models_default_primary.clone(),
            text.models_default_primary_desc.clone(),
            None,
            openbitfun_core::service::config::types::ModelCapability::TextChat,
            config.default_primary_model.clone(),
            models,
        ))
        .row(model_default_slot_row(
            "models-pool-fast",
            text.models_default_fast.clone(),
            text.models_default_fast_desc.clone(),
            Some(text.models_default_not_set.clone()),
            openbitfun_core::service::config::types::ModelCapability::TextChat,
            config.default_fast_model.clone(),
            models,
        ))
        .row(model_default_slot_row(
            "models-pool-image",
            text.models_default_image.clone(),
            text.models_default_image_desc.clone(),
            Some(text.models_default_not_set.clone()),
            openbitfun_core::service::config::types::ModelCapability::ImageUnderstanding,
            config.default_image_model.clone(),
            models,
        ))
        .row(model_default_slot_row(
            "models-pool-speech",
            text.models_default_speech.clone(),
            text.models_default_speech_desc.clone(),
            Some(text.models_default_not_set.clone()),
            openbitfun_core::service::config::types::ModelCapability::SpeechRecognition,
            config.default_speech_model.clone(),
            models,
        ))
        .row(
            SettingsCardRow::body("models-pool-toolbar")
                .control(models_pool_toolbar(text, models, state)),
        )
        .row(
            SettingsCardRow::body("models-pool-list")
                .control(models_pool_list(text, models, state, cx)),
        )
}

/// The capability filter value → the shared capability enum.
fn model_capability_filter(
    value: &str,
) -> Option<openbitfun_core::service::config::types::ModelCapability> {
    use openbitfun_core::service::config::types::ModelCapability as Capability;
    match value {
        "text_chat" => Some(Capability::TextChat),
        "image_understanding" => Some(Capability::ImageUnderstanding),
        "speech_recognition" => Some(Capability::SpeechRecognition),
        "function_calling" => Some(Capability::FunctionCalling),
        _ => None,
    }
}

/// The web pool filter: capability, provider group, then the query over the
/// model name, provider name, request format, and endpoint.
fn model_matches_pool_filter(
    model: &openbitfun_core::service::config::types::AIModelConfig,
    query: &str,
    capability: &str,
    provider: &str,
) -> bool {
    if let Some(wanted) = model_capability_filter(capability) {
        if !model_effective_capabilities(model).contains(&wanted) {
            return false;
        }
    }
    if !provider.is_empty() && model_provider_key(model) != provider {
        return false;
    }
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    [
        model.name.as_str(),
        model.model_name.as_str(),
        model.provider.as_str(),
        model.base_url.as_str(),
    ]
    .join(" ")
    .to_lowercase()
    .contains(&query)
}

/// The `pool` section's toolbar: search field, capability filter, provider
/// filter (web `pool.search` / `pool.capabilityFilter` / `pool.providerFilter`).
fn models_pool_toolbar(
    text: &ShellText,
    models: &[openbitfun_core::service::config::types::AIModelConfig],
    state: &ModelsPageState,
) -> AnyElement {
    let capability_options: Vec<(SharedString, SharedString)> = vec![
        ("", text.models_filter_all_capabilities.clone()),
        ("text_chat", text.models_capability_text.clone()),
        ("image_understanding", text.models_capability_image.clone()),
        ("speech_recognition", text.models_capability_speech.clone()),
        ("function_calling", text.models_capability_tools.clone()),
    ]
    .into_iter()
    .map(|(value, label)| (SharedString::from(value), SharedString::from(label)))
    .collect();
    let capability_shown = capability_options
        .iter()
        .find(|(value, _)| value.as_ref() == state.capability)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| text.models_filter_all_capabilities.clone().into());

    let mut provider_options: Vec<(SharedString, SharedString)> = vec![(
        SharedString::from(""),
        SharedString::from(text.models_filter_all_providers.clone()),
    )];
    for group in model_provider_groups(models) {
        provider_options.push((
            SharedString::from(group.key),
            SharedString::from(group.name),
        ));
    }
    let provider_shown = provider_options
        .iter()
        .find(|(value, _)| value.as_ref() == state.provider)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| text.models_filter_all_providers.clone().into());

    let query_value = SharedString::from(state.query.clone());
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_wrap()
        .w_full()
        .child(search_input_control(
            "models-pool-search",
            text.models_search.clone(),
            text.models_search.clone(),
            query_value,
            |value: SharedString, cx: &mut App| {
                ModelsPageState::mutate(cx, |state| state.query = value.to_string());
                cx.refresh_windows();
            },
        ))
        .child(dropdown_control(
            "models-pool-capability-menu",
            capability_shown,
            Rc::new(capability_options),
            Rc::new(|value: SharedString, cx: &mut App| {
                ModelsPageState::mutate(cx, |state| state.capability = value.to_string());
                cx.refresh_windows();
            }),
            false,
        ))
        .child(dropdown_control(
            "models-pool-provider-menu",
            provider_shown,
            Rc::new(provider_options),
            Rc::new(|value: SharedString, cx: &mut App| {
                ModelsPageState::mutate(cx, |state| state.provider = value.to_string());
                cx.refresh_windows();
            }),
            false,
        ))
        .into_any_element()
}

/// The filtered model collection: loading / empty / no-match states, then one
/// row per model with its enable switch, test, edit, and delete actions (the
/// web pool card). Default primary and fast rows sort first, like the web.
fn models_pool_list(
    text: &ShellText,
    models: &[openbitfun_core::service::config::types::AIModelConfig],
    state: &ModelsPageState,
    cx: &App,
) -> AnyElement {
    let config = ShellConfig::current(cx);
    let catalog_loaded = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .map(|catalog| catalog.loaded)
        .unwrap_or(false);
    if models.is_empty() {
        let message = if catalog_loaded {
            text.models_empty.clone()
        } else {
            text.models_loading.clone()
        };
        return div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(message)
            .into_any_element();
    }

    let mut visible: Vec<&openbitfun_core::service::config::types::AIModelConfig> = models
        .iter()
        .filter(|model| {
            model_matches_pool_filter(model, &state.query, &state.capability, &state.provider)
        })
        .collect();
    visible.sort_by_key(|model| {
        if config.default_primary_model.as_deref() == Some(model.id.as_str()) {
            0
        } else if config.default_fast_model.as_deref() == Some(model.id.as_str()) {
            1
        } else {
            2
        }
    });

    if visible.is_empty() {
        return div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.models_no_matches.clone()),
            )
            .child(
                Button::new("models-pool-clear-filter")
                    .label(text.models_clear_filter.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(|_, _, cx| {
                        ModelsPageState::mutate(cx, |state| {
                            state.query.clear();
                            state.capability.clear();
                            state.provider.clear();
                        });
                        cx.refresh_windows();
                    }),
            )
            .into_any_element();
    }

    let receipts = ModelTestState::current(cx).receipts;
    let mut list = div().flex().flex_col().gap_1().w_full();
    let mut first_row = true;
    for model in visible {
        let model_id = model.id.clone();
        let testing = matches!(receipts.get(&model_id), Some(ModelTestReceipt::Testing));
        let test_label = if testing {
            text.models_testing.clone()
        } else {
            text.models_test.clone()
        };
        let description = format!(
            "{} · {}",
            model_category_label(model, text),
            model.base_url.trim()
        );
        let actions = div()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(switch_control(
                SharedString::from(format!("models-enable-{model_id}")),
                model.enabled,
                {
                    let model_id = model_id.clone();
                    move |value, cx| crate::settings::set_ai_model_enabled(&model_id, value, cx)
                },
                false,
            ))
            .child(
                Button::new(SharedString::from(format!("models-test-{model_id}")))
                    .label(test_label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .loading(testing)
                    .on_click({
                        let model_id = model_id.clone();
                        move |_, _, cx| {
                            start_model_test(model_id.clone(), cx);
                        }
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("models-edit-{model_id}")))
                    .icon(IconName::Pencil)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(crate::i18n::shared_term("tools.edit", cx))
                    .on_click({
                        let model_id = model_id.clone();
                        move |_, _, cx| open_model_editor(model_id.clone(), cx)
                    }),
            )
            .child(
                Button::new(SharedString::from(format!("models-delete-{model_id}")))
                    .icon(IconName::Trash)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .tooltip(text.models_delete.clone())
                    .on_click({
                        let model_id = model_id.clone();
                        let label = model_display_label(model);
                        move |_, window, cx| {
                            open_model_delete_confirm(window, cx, model_id.clone(), label.clone());
                        }
                    }),
            );

        let line = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(model_display_label(model)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .child(actions);
        let mut row = div().flex().flex_col().gap_1().w_full().child(line);
        if let Some(ModelTestReceipt::Done(result)) = receipts.get(&model_id) {
            row = row.child(model_test_receipt(result, text, cx));
        }
        if !first_row {
            row = row
                .border_t_1()
                .border_color(cx.theme().border.opacity(0.6))
                .pt_2();
        }
        first_row = false;
        list = list.child(row);
    }
    list.into_any_element()
}

/// Deletes a whole configured service after the danger confirmation; the
/// dialog names the service and the number of model rows that go with it.
fn open_provider_delete_confirm(window: &mut Window, cx: &mut App, group: ModelProviderGroup) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = crate::i18n::current(cx);
        let title = crate::i18n::models_delete_provider_title(&group.name, cx);
        let message = crate::i18n::models_delete_provider_message(group.model_ids.len(), cx);
        let ids = group.model_ids.clone();
        dialog
            .title(div().child(title))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(message),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("models-provider-delete-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("models-provider-delete-confirm")
                            .label(text.models_delete_provider.clone())
                            .with_variant(ButtonVariant::Danger)
                            .on_click(move |_, window, cx| {
                                crate::settings::delete_ai_models(ids.clone(), cx);
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}

/// Deletes one model row after the danger confirmation.
fn open_model_delete_confirm(window: &mut Window, cx: &mut App, model_id: String, label: String) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = crate::i18n::current(cx);
        let title = crate::i18n::models_delete_model_title(&label, cx);
        let ids = vec![model_id.clone()];
        dialog
            .title(div().child(title))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(text.models_delete_message.clone()),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("models-delete-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("models-delete-confirm")
                            .label(text.models_delete_confirm.clone())
                            .with_variant(ButtonVariant::Danger)
                            .on_click(move |_, window, cx| {
                                crate::settings::delete_ai_models(ids.clone(), cx);
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}

/// The new-model form (web `handleCreateNew` + `handleSave`'s required
/// fields). One deliberate draft: nothing is written to `ai.models` until the
/// whole row validates.
fn models_create_card(text: &ShellText, state: &ModelsPageState) -> SettingsCard {
    let draft = state.create.clone().unwrap_or_default();
    let provider_options = model_provider_options(&draft.provider);
    let provider_shown = provider_options
        .iter()
        .find(|(value, _)| value.as_ref() == draft.provider)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| draft.provider.clone().into());
    let category_options: Vec<(SharedString, SharedString)> = vec![
        ("general_chat", text.models_category_general_chat.clone()),
        ("multimodal", text.models_category_multimodal.clone()),
        (
            "speech_recognition",
            text.models_category_speech_recognition.clone(),
        ),
    ]
    .into_iter()
    .map(|(value, label)| (SharedString::from(value), SharedString::from(label)))
    .collect();
    let category_shown = category_options
        .iter()
        .find(|(value, _)| value.as_ref() == draft.category)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| draft.category.clone().into());

    let mut card = SettingsCard::new("models-create")
        .title(text.models_create_title.clone())
        .extra(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    Button::new("models-create-cancel")
                        .label(text.models_create_cancel.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(|_, _, cx| crate::settings::cancel_model_create(cx)),
                )
                .child(
                    Button::new("models-create-save")
                        .label(text.models_create_save.clone())
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .on_click(|_, _, cx| crate::settings::save_model_create(cx)),
                ),
        );

    card = card
        .row(
            SettingsCardRow::new("models-create-name", text.models_editor_name.clone()).control(
                text_input_control(
                    "models-create-name",
                    text.models_editor_name.clone(),
                    SharedString::from(draft.name.clone()),
                    |value: SharedString, cx: &mut App| {
                        crate::settings::update_model_create_draft(cx, |draft| {
                            draft.name = value.to_string()
                        });
                    },
                ),
            ),
        )
        .row(
            SettingsCardRow::new(
                "models-create-provider",
                text.models_editor_provider.clone(),
            )
            .control(dropdown_control(
                "models-create-provider-menu",
                provider_shown,
                Rc::new(provider_options),
                Rc::new(|value: SharedString, cx: &mut App| {
                    crate::settings::update_model_create_draft(cx, |draft| {
                        draft.provider = value.to_string()
                    });
                }),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "models-create-model-name",
                text.models_editor_model_name.clone(),
            )
            .control(text_input_control(
                "models-create-model-name",
                text.models_editor_model_name.clone(),
                SharedString::from(draft.model_name.clone()),
                |value: SharedString, cx: &mut App| {
                    crate::settings::update_model_create_draft(cx, |draft| {
                        draft.model_name = value.to_string()
                    });
                },
            )),
        )
        .row(
            SettingsCardRow::new(
                "models-create-base-url",
                text.models_editor_base_url.clone(),
            )
            .control(text_input_control(
                "models-create-base-url",
                text.models_editor_base_url.clone(),
                SharedString::from(draft.base_url.clone()),
                |value: SharedString, cx: &mut App| {
                    crate::settings::update_model_create_draft(cx, |draft| {
                        draft.base_url = value.to_string()
                    });
                },
            )),
        )
        .row(
            SettingsCardRow::new("models-create-api-key", text.models_editor_api_key.clone())
                .control(text_input_control(
                    "models-create-api-key",
                    text.models_editor_api_key.clone(),
                    SharedString::from(draft.api_key.clone()),
                    |value: SharedString, cx: &mut App| {
                        crate::settings::update_model_create_draft(cx, |draft| {
                            draft.api_key = value.to_string()
                        });
                    },
                )),
        )
        .row(
            SettingsCardRow::new("models-create-context", text.models_context_window.clone())
                .description(text.models_context_window_hint.clone())
                .control(number_input_control_with_width(
                    "models-create-context-input",
                    text.models_context_window.clone(),
                    f64::from(draft.context_window),
                    NumberRange {
                        min: f64::from(MODEL_CREATE_MIN_CONTEXT_WINDOW),
                        max: f64::MAX,
                        step: 1_000.,
                    },
                    |value: f64, cx: &mut App| {
                        if !value.is_finite() {
                            return;
                        }
                        let tokens =
                            (value.round().max(0.) as u32).max(MODEL_CREATE_MIN_CONTEXT_WINDOW);
                        crate::settings::update_model_create_draft(cx, |draft| {
                            draft.context_window = tokens
                        });
                    },
                    false,
                    224.,
                )),
        )
        .row(
            SettingsCardRow::new("models-create-category", text.models_category.clone()).control(
                dropdown_control(
                    "models-create-category-menu",
                    category_shown,
                    Rc::new(category_options),
                    Rc::new(|value: SharedString, cx: &mut App| {
                        crate::settings::update_model_create_draft(cx, |draft| {
                            draft.category = value.to_string()
                        });
                    }),
                    false,
                ),
            ),
        );

    if let Some(error) = draft.error.as_ref() {
        card = card.row(SettingsCardRow::new("models-create-error", error.clone()).danger(true));
    }
    card
}

/// The inline editor cards (web modal): the connection fields, the model
/// parameters, and the advanced request-shaping block. Empty while no row is
/// open, or when the open row no longer exists (deleted meanwhile).
fn models_editor_cards(
    text: &ShellText,
    models: &[openbitfun_core::service::config::types::AIModelConfig],
    cx: &App,
) -> Vec<SettingsCard> {
    let Some(model_id) = ModelEditorState::current(cx).model_id.clone() else {
        return Vec::new();
    };
    let Some(model) = models.iter().find(|model| model.id == model_id).cloned() else {
        return Vec::new();
    };

    let close_button = Button::new("models-editor-close")
        .label(text.models_edit_close.clone())
        .small()
        .with_variant(ButtonVariant::Ghost)
        .on_click(|_, _, cx| {
            ModelEditorState::mutate(cx, |state| state.model_id = None);
            cx.refresh_windows();
        });
    let delete_button = Button::new("models-editor-delete")
        .icon(IconName::Trash)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .tooltip(text.models_editor_delete.clone())
        .on_click({
            let model_id = model_id.clone();
            let label = model_display_label(&model);
            move |_, window, cx| {
                open_model_delete_confirm(window, cx, model_id.clone(), label.clone());
            }
        });
    let head_actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(delete_button)
        .child(close_button);

    let provider_options = model_provider_options(&model.provider);
    let provider_shown = provider_options
        .iter()
        .find(|(value, _)| value.as_ref() == model.provider)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| model.provider.clone().into());

    // Category: the web's four choices, plus a value written by a newer build
    // kept verbatim so it is never silently remapped.
    let stored_category = serde_json::to_value(&model.category)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "general_chat".to_string());
    let mut category_options: Vec<(SharedString, SharedString)> = Vec::new();
    for value in model_category_values(&stored_category) {
        let (value, label) = if value == "__stored_unknown" {
            (stored_category.clone(), stored_category.clone())
        } else {
            let label = match value {
                "general_chat" => text.models_category_general_chat.clone(),
                "multimodal" => text.models_category_multimodal.clone(),
                "image_generation" => text.models_category_image_generation.clone(),
                "speech_recognition" => text.models_category_speech_recognition.clone(),
                other => other.to_string(),
            };
            (value.to_string(), label)
        };
        category_options.push((value.into(), label.into()));
    }
    let category_label = category_options
        .iter()
        .find(|(value, _)| value.as_ref() == stored_category)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| stored_category.clone().into());
    let on_category_select: Rc<dyn Fn(SharedString, &mut App)> = {
        let model_id = model_id.clone();
        Rc::new(move |value: SharedString, cx| {
            set_ai_model_category(&model_id, value.to_string(), cx);
        })
    };

    let context_value = model
        .context_window
        .unwrap_or(MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW) as f64;
    let on_context_change = {
        let model_id = model_id.clone();
        move |value: f64, cx: &mut App| {
            if value.is_finite() && value >= f64::from(MODEL_EDITOR_MIN_CONTEXT_WINDOW) {
                set_ai_model_context_window(&model_id, value.round() as u32, cx);
            }
        }
    };

    let max_tokens_value = model.max_tokens.unwrap_or(0) as f64;
    let on_max_tokens_change = {
        let model_id = model_id.clone();
        move |value: f64, cx: &mut App| {
            if !value.is_finite() {
                return;
            }
            let tokens = value.round().max(0.) as u32;
            if tokens == 0 {
                set_ai_model_max_tokens(&model_id, None, cx);
                return;
            }
            // The shared contract caps an explicit output limit at 40% of the
            // context window; an out-of-range value is ignored rather than
            // persisted invalid.
            let context = ShellConfig::current(cx)
                .ai_models
                .iter()
                .find(|model| model.id == model_id)
                .and_then(|model| model.context_window)
                .unwrap_or(MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW);
            if openbitfun_core::service::config::types::is_valid_configured_max_output_tokens(
                context, tokens,
            ) {
                set_ai_model_max_tokens(&model_id, Some(tokens), cx);
            }
        }
    };

    // Reasoning: the stored presets, then a stored default this build cannot
    // resolve kept verbatim.
    let stored_default = model
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.default_preset.clone())
        .unwrap_or_default();
    let mut reasoning_options: Vec<(SharedString, SharedString)> = vec![(
        SharedString::from(""),
        text.models_reasoning_auto.clone().into(),
    )];
    if let Some(reasoning) = model.reasoning.as_ref() {
        for preset in &reasoning.presets {
            if preset.disabled || preset.actions.is_empty() {
                continue;
            }
            let label = preset
                .label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| preset.id.clone());
            reasoning_options.push((preset.id.clone().into(), label.into()));
        }
    }
    if !stored_default.is_empty()
        && !reasoning_options
            .iter()
            .any(|(value, _)| value.as_ref() == stored_default)
    {
        reasoning_options.push((stored_default.clone().into(), stored_default.clone().into()));
    }
    let reasoning_label = reasoning_options
        .iter()
        .find(|(value, _)| value.as_ref() == stored_default.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| stored_default.clone().into());
    let on_reasoning_select: Rc<dyn Fn(SharedString, &mut App)> = {
        let model_id = model_id.clone();
        Rc::new(move |value: SharedString, cx: &mut App| {
            let preset = (!value.is_empty()).then_some(value.to_string());
            set_ai_model_reasoning_default(&model_id, preset, cx);
        })
    };

    let editor_card = SettingsCard::new("models-editor")
        .title(format!(
            "{} · {}",
            text.models_edit_title,
            model_display_label(&model)
        ))
        .extra(head_actions)
        .row(
            SettingsCardRow::new("models-editor-name", text.models_editor_name.clone()).control(
                text_input_control(
                    "models-editor-name",
                    text.models_editor_name.clone(),
                    SharedString::from(model.name.clone()),
                    {
                        let model_id = model_id.clone();
                        move |value: SharedString, cx: &mut App| {
                            set_ai_model_connection_text(&model_id, "name", &value, cx)
                        }
                    },
                ),
            ),
        )
        .row(
            SettingsCardRow::new(
                "models-editor-provider",
                text.models_editor_provider.clone(),
            )
            .control(dropdown_control(
                "models-editor-provider-menu",
                provider_shown,
                Rc::new(provider_options),
                {
                    let model_id = model_id.clone();
                    Rc::new(move |value: SharedString, cx: &mut App| {
                        set_ai_model_provider(&model_id, value.to_string(), cx);
                    })
                },
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "models-editor-model-name",
                text.models_editor_model_name.clone(),
            )
            .control(text_input_control(
                "models-editor-model-name",
                text.models_editor_model_name.clone(),
                SharedString::from(model.model_name.clone()),
                {
                    let model_id = model_id.clone();
                    move |value: SharedString, cx: &mut App| {
                        set_ai_model_connection_text(&model_id, "model_name", &value, cx)
                    }
                },
            )),
        )
        .row(
            SettingsCardRow::new(
                "models-editor-base-url",
                text.models_editor_base_url.clone(),
            )
            .control(text_input_control(
                "models-editor-base-url",
                text.models_editor_base_url.clone(),
                SharedString::from(model.base_url.clone()),
                {
                    let model_id = model_id.clone();
                    move |value: SharedString, cx: &mut App| {
                        set_ai_model_connection_text(&model_id, "base_url", &value, cx)
                    }
                },
            )),
        )
        .row(
            SettingsCardRow::new("models-editor-api-key", text.models_editor_api_key.clone())
                .control(text_input_control(
                    "models-editor-api-key",
                    text.models_editor_api_key.clone(),
                    SharedString::from(model.api_key.clone()),
                    {
                        let model_id = model_id.clone();
                        move |value: SharedString, cx: &mut App| {
                            set_ai_model_connection_text(&model_id, "api_key", &value, cx)
                        }
                    },
                )),
        )
        .row(
            SettingsCardRow::new("models-editor-enabled", text.models_editor_enabled.clone())
                .control(switch_control(
                    "models-editor-enabled-switch",
                    model.enabled,
                    {
                        let model_id = model_id.clone();
                        move |value, cx| crate::settings::set_ai_model_enabled(&model_id, value, cx)
                    },
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("models-editor-category", text.models_category.clone()).control(
                dropdown_control(
                    "models-editor-category-menu",
                    category_label,
                    Rc::new(category_options),
                    on_category_select,
                    false,
                ),
            ),
        )
        .row(
            SettingsCardRow::new("models-editor-context", text.models_context_window.clone())
                .description(text.models_context_window_hint.clone())
                .control(number_input_control_with_width(
                    "models-editor-context-input",
                    text.models_context_window.clone(),
                    context_value,
                    NumberRange {
                        min: f64::from(MODEL_EDITOR_MIN_CONTEXT_WINDOW),
                        max: f64::MAX,
                        step: 1_000.,
                    },
                    on_context_change,
                    false,
                    224.,
                )),
        )
        .row(
            SettingsCardRow::new("models-editor-max-tokens", text.models_max_tokens.clone())
                .description(text.models_max_tokens_hint.clone())
                .control(number_input_control_with_width(
                    "models-editor-max-tokens-input",
                    text.models_max_tokens.clone(),
                    max_tokens_value,
                    NumberRange {
                        min: 0.,
                        max: f64::MAX,
                        step: 1_000.,
                    },
                    on_max_tokens_change,
                    false,
                    224.,
                )),
        )
        .row(
            SettingsCardRow::new(
                "models-editor-reasoning",
                text.models_reasoning_default.clone(),
            )
            .control(dropdown_control(
                "models-editor-reasoning-menu",
                reasoning_label,
                Rc::new(reasoning_options),
                on_reasoning_select,
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "models-editor-presets",
                text.models_reasoning_presets.clone(),
            )
            .multiline()
            .control(DeferredControl {
                construct: {
                    let presets = model
                        .reasoning
                        .as_ref()
                        .map(|reasoning| reasoning.presets.clone())
                        .unwrap_or_default();
                    move |_window, cx| model_editor_presets_element(&presets, cx)
                },
            }),
        );

    // Advanced: request shaping over the fixed provider format.
    let stored_headers_mode = model
        .custom_headers_mode
        .clone()
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "replace".to_string());
    let stored_body_mode = model
        .custom_request_body_mode
        .clone()
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "merge".to_string());
    let headers_mode_options = verbatim_pair_options(
        vec![
            (
                SharedString::from("replace"),
                text.models_headers_replace.clone().into(),
            ),
            (
                SharedString::from("merge"),
                text.models_headers_merge.clone().into(),
            ),
        ],
        &stored_headers_mode,
    );
    let headers_mode_label = headers_mode_options
        .iter()
        .find(|(value, _)| value.as_ref() == stored_headers_mode)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| stored_headers_mode.clone().into());
    let body_mode_options = verbatim_pair_options(
        vec![
            (
                SharedString::from("merge"),
                text.models_body_merge.clone().into(),
            ),
            (
                SharedString::from("trim"),
                text.models_body_trim.clone().into(),
            ),
        ],
        &stored_body_mode,
    );
    let body_mode_label = body_mode_options
        .iter()
        .find(|(value, _)| value.as_ref() == stored_body_mode)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| stored_body_mode.clone().into());
    let headers_description = if stored_headers_mode == "merge" {
        text.models_headers_merge_hint.clone()
    } else {
        text.models_headers_replace_hint.clone()
    };
    let body_description = if stored_body_mode == "trim" {
        text.models_body_trim_hint.clone()
    } else {
        text.models_body_hint.clone()
    };
    let editor = ModelEditorState::current(cx);
    let drafts_here = editor.drafts_ready && editor.model_id.as_deref() == Some(model_id.as_str());
    let headers_value = if drafts_here {
        SharedString::from(editor.headers_text.clone())
    } else {
        SharedString::from(
            serde_json::to_string(&model.custom_headers.clone().unwrap_or_default())
                .unwrap_or_else(|_| "{}".to_string()),
        )
    };
    let body_value = if drafts_here {
        SharedString::from(editor.body_text.clone())
    } else {
        SharedString::from(model.custom_request_body.clone().unwrap_or_default())
    };
    let invalid = drafts_here
        && (!headers_json_valid(&editor.headers_text) || !body_json_valid(&editor.body_text));

    let mut advanced = SettingsCard::new("models-advanced")
        .title(text.models_advanced.clone())
        .description(text.models_advanced_note.clone())
        .row(
            SettingsCardRow::new(
                "models-advanced-inline-think",
                text.models_inline_think.clone(),
            )
            .description(text.models_inline_think_hint.clone())
            .control(switch_control(
                "models-advanced-inline-think-switch",
                model.inline_think_in_text,
                {
                    let model_id = model_id.clone();
                    move |value, cx| set_ai_model_inline_think(&model_id, value, cx)
                },
                false,
            )),
        )
        .row(
            SettingsCardRow::new("models-advanced-skip-ssl", text.models_skip_ssl.clone())
                .description(text.models_skip_ssl_warning.clone())
                .control(switch_control(
                    "models-advanced-skip-ssl-switch",
                    model.skip_ssl_verify,
                    {
                        let model_id = model_id.clone();
                        move |value, cx| set_ai_model_skip_ssl(&model_id, value, cx)
                    },
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("models-advanced-headers", text.models_headers.clone())
                .description(format!(
                    "{} {}",
                    text.models_headers_hint, headers_description
                ))
                .multiline()
                .control(text_input_control(
                    "models-advanced-headers-input",
                    text.models_headers.clone(),
                    headers_value,
                    {
                        let model_id = model_id.clone();
                        move |value, cx| set_ai_model_headers_text(&model_id, value.to_string(), cx)
                    },
                )),
        )
        .row(
            SettingsCardRow::new(
                "models-advanced-headers-mode",
                text.models_headers_mode.clone(),
            )
            .control(dropdown_control(
                "models-advanced-headers-mode-menu",
                headers_mode_label,
                Rc::new(headers_mode_options),
                {
                    let model_id = model_id.clone();
                    Rc::new(move |value: SharedString, cx: &mut App| {
                        set_ai_model_header_mode(&model_id, value.to_string(), cx);
                    })
                },
                false,
            )),
        )
        .row(
            SettingsCardRow::new("models-advanced-body", text.models_body.clone())
                .description(body_description)
                .multiline()
                .control(text_input_control(
                    "models-advanced-body-input",
                    text.models_body.clone(),
                    body_value,
                    {
                        let model_id = model_id.clone();
                        move |value, cx| set_ai_model_body_text(&model_id, value.to_string(), cx)
                    },
                )),
        )
        .row(
            SettingsCardRow::new("models-advanced-body-mode", text.models_body_mode.clone())
                .control(dropdown_control(
                    "models-advanced-body-mode-menu",
                    body_mode_label,
                    Rc::new(body_mode_options),
                    {
                        let model_id = model_id.clone();
                        Rc::new(move |value: SharedString, cx: &mut App| {
                            set_ai_model_body_mode(&model_id, value.to_string(), cx);
                        })
                    },
                    false,
                )),
        );
    // Invalid JSON stays in the draft (the stored row is untouched); say so
    // instead of silently showing a field that is not saving.
    if invalid {
        advanced = advanced.row(
            SettingsCardRow::new(
                "models-advanced-json-invalid",
                text.models_json_invalid.clone(),
            )
            .danger(true),
        );
    }
    vec![editor_card, advanced]
}

/// The 更多设置 disclosure (web `sections.more`): a head-only card whose
/// toggle reveals the two advanced sections below it.
fn models_more_card(text: &ShellText, state: &ModelsPageState) -> SettingsCard {
    let open = state.more_open;
    SettingsCard::new("models-more")
        .title(text.models_more.clone())
        .extra(
            Button::new("models-more-toggle")
                .label(if open {
                    text.models_more_collapse.clone()
                } else {
                    text.models_more_expand.clone()
                })
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(|_, _, cx| {
                    ModelsPageState::mutate(cx, |state| state.more_open = !state.more_open);
                    cx.refresh_windows();
                }),
        )
}

/// G-SE10: the streaming-timeout pair; zero means "no timeout".
fn models_stream_card(text: &ShellText, cx: &App) -> SettingsCard {
    let ttft_value = ShellConfig::current(cx)
        .stream_ttft_timeout_secs
        .unwrap_or(0) as f64;
    let idle_value = ShellConfig::current(cx)
        .stream_idle_timeout_secs
        .unwrap_or(0) as f64;
    SettingsCard::new("models-stream")
        .title(text.settings_stream_section.clone())
        .description(text.settings_stream_section_note.clone())
        .row(
            SettingsCardRow::new("models-stream-ttft", text.settings_stream_ttft.clone())
                .description(text.settings_stream_ttft_description.clone())
                .control(number_input_control(
                    "models-stream-ttft-input",
                    text.settings_stream_ttft.clone(),
                    ttft_value,
                    NumberRange {
                        min: 0.,
                        max: MAX_STREAM_TIMEOUT_SECS,
                        step: 30.,
                    },
                    |value, cx| {
                        set_stream_timeout_value(
                            STREAM_TTFT_TIMEOUT_PATH,
                            |config, value| config.stream_ttft_timeout_secs = value,
                            value,
                            cx,
                        );
                    },
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("models-stream-idle", text.settings_stream_idle.clone())
                .description(text.settings_stream_idle_description.clone())
                .control(number_input_control(
                    "models-stream-idle-input",
                    text.settings_stream_idle.clone(),
                    idle_value,
                    NumberRange {
                        min: 0.,
                        max: MAX_STREAM_TIMEOUT_SECS,
                        step: 30.,
                    },
                    |value, cx| {
                        set_stream_timeout_value(
                            STREAM_IDLE_TIMEOUT_PATH,
                            |config, value| config.stream_idle_timeout_secs = value,
                            value,
                            cx,
                        );
                    },
                    false,
                )),
        )
}

/// G-SE11: the proxy document; the three credential rows turn off with the
/// switch.
fn models_proxy_card(text: &ShellText, cx: &App) -> SettingsCard {
    let proxy = ShellConfig::current(cx).proxy;
    let proxy_on = proxy_enabled(&proxy);
    let proxy_url = SharedString::from(proxy_text(&proxy, "url"));
    let proxy_username = SharedString::from(proxy_text(&proxy, "username"));
    let proxy_password = SharedString::from(proxy_text(&proxy, "password"));
    SettingsCard::new("models-proxy")
        .title(text.settings_proxy_section.clone())
        .description(text.settings_proxy_section_note.clone())
        .row(
            SettingsCardRow::new("models-proxy-enable", text.settings_proxy_enable.clone())
                .description(text.settings_proxy_enable_description.clone())
                .control(switch_control(
                    "models-proxy-enable-switch",
                    proxy_on,
                    |value, cx| set_proxy_key("enabled", serde_json::json!(value), cx),
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("models-proxy-url", text.settings_proxy_url.clone())
                .description(text.settings_proxy_url_description.clone())
                .disabled(!proxy_on)
                .control(text_input_control(
                    "models-proxy-url-input",
                    text.settings_proxy_url.clone(),
                    proxy_url,
                    |value, cx| set_proxy_key("url", serde_json::json!(value.to_string()), cx),
                )),
        )
        .row(
            SettingsCardRow::new(
                "models-proxy-username",
                text.settings_proxy_username.clone(),
            )
            .disabled(!proxy_on)
            .control(text_input_control(
                "models-proxy-username-input",
                text.settings_proxy_username.clone(),
                proxy_username,
                |value, cx| {
                    set_proxy_key("username", serde_json::json!(value.to_string()), cx);
                },
            )),
        )
        .row(
            SettingsCardRow::new(
                "models-proxy-password",
                text.settings_proxy_password.clone(),
            )
            .description(text.settings_proxy_password_description.clone())
            .disabled(!proxy_on)
            .control(text_input_control(
                "models-proxy-password-input",
                text.settings_proxy_password.clone(),
                proxy_password,
                |value, cx| {
                    set_proxy_key("password", serde_json::json!(value.to_string()), cx);
                },
            )),
        )
}

/// A two-value option list plus a stored value outside it, kept verbatim so
/// a newer build's value is never silently remapped (the model editor's
/// header/body mode menus).
fn verbatim_pair_options(
    mut options: Vec<(SharedString, SharedString)>,
    stored: &str,
) -> Vec<(SharedString, SharedString)> {
    if !options.iter().any(|(value, _)| value.as_ref() == stored) {
        options.push((stored.to_string().into(), stored.to_string().into()));
    }
    options
}

/// The 会话与记忆 page's cards, one per web section: 默认执行模式 (the
/// strategy and its conditional fixed-mode row), 会话标题自动生成 (the enable
/// switch and, while enabled, the model selector), 记忆 (the enable pair, BTW
/// source, external-context policy, the actions menu), 记忆模型 (the extraction
/// and consolidation followers), and 记忆处理 — head-only while collapsed, its
/// seven tuned numbers as rows while open. The scope notice and the inline
/// receipt render above the cards (page preamble).
fn memory_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
    let memories = ShellConfig::current(cx).memories.clone();
    let memory_enabled = memories.generate_memories && memories.use_memories;
    let work_disabled = !memory_enabled;
    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();

    let policy_options: Vec<(SharedString, SharedString)> = [
        (
            "clear_tool_results",
            text.memory_policy_clear_tool_results.clone(),
        ),
        ("allow", text.memory_policy_allow.clone()),
        ("skip_session", text.memory_policy_skip_session.clone()),
    ]
    .into_iter()
    .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
    .collect();
    let policy_current =
        memory_policy_value(ShellConfig::current(cx).memories.external_context_policy);
    let policy_label = policy_options
        .iter()
        .find(|(value, _)| value.as_ref() == policy_current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| policy_current.into());

    let extract_options = memory_model_options(
        &catalog,
        text.memory_model_follow_primary.clone(),
        memories.extract_model.as_deref(),
        cx,
    );
    let extract_current = memories.extract_model.clone().unwrap_or_default();
    let extract_label = extract_options
        .iter()
        .find(|(value, _)| value.as_ref() == extract_current.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| extract_current.clone().into());
    let consolidation_options = memory_model_options(
        &catalog,
        text.memory_model_follow_extraction.clone(),
        memories.consolidation_model.as_deref(),
        cx,
    );
    let consolidation_current = memories.consolidation_model.clone().unwrap_or_default();
    let consolidation_label = consolidation_options
        .iter()
        .find(|(value, _)| value.as_ref() == consolidation_current.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| consolidation_current.clone().into());

    // The web page opens with its default-execution-mode and session-title
    // sections; E3-B4 moved both off the shell's search page.
    let facts = default_harness_facts(text, cx);
    let strategy_options = Rc::new(vec![
        (
            SharedString::from("follow_last"),
            SharedString::from(text.settings_default_harness_follow_last.clone()),
        ),
        (
            SharedString::from("fixed"),
            SharedString::from(text.settings_default_harness_fixed.clone()),
        ),
    ]);
    let current_strategy = flow_chat_strategy(&ShellConfig::current(cx).flow_chat);
    // The closed control shows the localized option label, never the stored id.
    let strategy_label = strategy_options
        .iter()
        .find(|(value, _)| value.as_ref() == current_strategy)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| SharedString::from(current_strategy));
    let on_strategy_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_flow_chat_strategy(&value, cx));

    let mut harness_card = SettingsCard::new("memory-harness")
        .title(text.settings_default_harness_section.clone())
        .description(text.settings_default_harness_section_note.clone())
        .row(
            SettingsCardRow::new(
                "memory-harness-strategy",
                text.settings_default_harness_strategy.clone(),
            )
            .description(facts.strategy_description)
            .control(dropdown_control(
                "memory-harness-strategy-menu",
                strategy_label,
                strategy_options,
                on_strategy_select,
                false,
            )),
        );
    if facts.strategy == "fixed" {
        let fixed_mode = flow_chat_fixed_mode(&ShellConfig::current(cx).flow_chat);
        let fixed_label = facts
            .fixed_options
            .iter()
            .find(|(value, _)| value.as_ref() == fixed_mode.as_str())
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| fixed_mode.clone().into());
        harness_card = harness_card.row(
            SettingsCardRow::new(
                "memory-harness-fixed",
                text.settings_default_harness_fixed_mode.clone(),
            )
            .control(dropdown_control(
                "memory-harness-fixed-menu",
                fixed_label,
                Rc::new(facts.fixed_options),
                Rc::new(|value: SharedString, cx| set_flow_chat_fixed_mode(&value, cx)),
                false,
            )),
        );
    }

    let title_enabled = ShellConfig::current(cx).session_title_enabled;
    let title_model_current = ShellConfig::current(cx).session_title_model;
    let title_model_options =
        session_title_model_options(&catalog.models, &title_model_current, text);
    let title_model_label = title_model_options
        .iter()
        .find(|(value, _)| value.as_ref() == title_model_current.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| title_model_current.clone().into());
    let mut title_card = SettingsCard::new("memory-session-title")
        .title(text.settings_session_title_section.clone())
        .description(text.settings_session_title_section_note.clone())
        .row(
            SettingsCardRow::new(
                "memory-session-title-enable",
                text.settings_session_title_enable.clone(),
            )
            .control(switch_control(
                "memory-session-title-enable-switch",
                title_enabled,
                set_session_title_enabled,
                false,
            )),
        );
    // The web page renders the model row only while generation is on.
    if title_enabled {
        let mut model_row = SettingsCardRow::new(
            "memory-session-title-model",
            text.settings_session_title_model.clone(),
        )
        .control(dropdown_control(
            "memory-session-title-model-menu",
            title_model_label,
            Rc::new(title_model_options),
            Rc::new(|value: SharedString, cx| set_session_title_model(&value, cx)),
            false,
        ));
        if !catalog.models.iter().any(|model| model.enabled) {
            model_row = model_row.description(text.settings_session_title_models_empty.clone());
        }
        title_card = title_card.row(model_row);
    }

    let basic = SettingsCard::new("memory-basic")
        .title(text.memory_section_basic.clone())
        .description(text.memory_section_basic_note.clone())
        .row(
            SettingsCardRow::new("memory-basic-enabled", text.memory_enabled.clone())
                .description(text.memory_enabled_description.clone())
                .control(switch_control(
                    "memory-basic-enabled-switch",
                    memory_enabled,
                    set_memory_enabled,
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("memory-basic-btw", text.memory_generate_btw.clone())
                .description(text.memory_generate_btw_description.clone())
                .disabled(work_disabled)
                .control(switch_control(
                    "memory-basic-btw-switch",
                    memories.generate_for_btw_sessions,
                    set_memory_btw,
                    work_disabled,
                )),
        )
        .row(
            SettingsCardRow::new("memory-basic-policy", text.memory_external_context.clone())
                .description(text.memory_external_context_description.clone())
                .disabled(work_disabled)
                .control(dropdown_control(
                    "memory-basic-policy-menu",
                    policy_label,
                    Rc::new(policy_options),
                    Rc::new(|value: SharedString, cx: &mut App| set_memory_policy(&value, cx)),
                    work_disabled,
                )),
        )
        .row(
            SettingsCardRow::new("memory-basic-actions", text.memory_actions_label.clone())
                .description(text.memory_actions_description.clone())
                .control(DeferredControl {
                    construct: {
                        let view = view.clone();
                        move |_, cx| memory_actions_button(view, cx)
                    },
                }),
        );

    let models = SettingsCard::new("memory-models")
        .title(text.memory_section_models.clone())
        .description(text.memory_section_models_note.clone())
        .row(
            SettingsCardRow::new("memory-models-extract", text.memory_extract_model.clone())
                .description(text.memory_extract_model_description.clone())
                .disabled(work_disabled)
                .control(dropdown_control(
                    "memory-models-extract-menu",
                    extract_label,
                    Rc::new(extract_options),
                    Rc::new(|value: SharedString, cx: &mut App| {
                        set_memory_extract_model(&value, cx)
                    }),
                    work_disabled,
                )),
        )
        .row(
            SettingsCardRow::new(
                "memory-models-consolidation",
                text.memory_consolidation_model.clone(),
            )
            .description(text.memory_consolidation_model_description.clone())
            .disabled(work_disabled)
            .control(dropdown_control(
                "memory-models-consolidation-menu",
                consolidation_label,
                Rc::new(consolidation_options),
                Rc::new(|value: SharedString, cx: &mut App| {
                    set_memory_consolidation_model(&value, cx)
                }),
                work_disabled,
            )),
        );

    let advanced_open = MemoryPageState::current(cx).advanced_open;
    let advanced = SettingsCard::new("memory-advanced")
        .title(text.memory_section_advanced.clone())
        .description(text.memory_section_advanced_note.clone())
        .extra(DeferredControl {
            construct: {
                let view = view.clone();
                move |_, cx| memory_advanced_toggle_button(view, cx)
            },
        });
    let mut cards = vec![harness_card, title_card, basic, models];
    if !advanced_open {
        cards.push(advanced);
        return cards;
    }

    let age_max = memories.max_unused_days.max(0) as f64;
    let retention_min = memories.max_rollout_age_days.max(0) as f64;
    let idle_value = memories.min_rollout_idle_hours as f64;
    let age_value = memories.max_rollout_age_days as f64;
    let per_startup_value = memories.max_rollouts_per_startup as f64;
    let scan_value = memories.max_rollouts_scan_limit as f64;
    let concurrency_value = memories.phase1_max_concurrency as f64;
    let raw_value = memories.max_raw_memories_for_consolidation as f64;
    let retention_value = memories.max_unused_days as f64;

    let advanced = advanced
        .row(
            SettingsCardRow::new(
                "memory-advanced-idle-hours",
                text.memory_min_idle_hours.clone(),
            )
            .description(text.memory_min_idle_hours_description.clone())
            .control(number_input_control(
                "memory-advanced-idle-hours-input",
                text.memory_min_idle_hours.clone(),
                idle_value,
                NumberRange {
                    min: 1.,
                    max: 48.,
                    step: 1.,
                },
                set_memory_min_idle_hours,
                false,
            )),
        )
        .row(
            SettingsCardRow::new("memory-advanced-age-days", text.memory_max_age_days.clone())
                .description(text.memory_max_age_days_description.clone())
                .control(number_input_control(
                    "memory-advanced-age-days-input",
                    text.memory_max_age_days.clone(),
                    age_value,
                    NumberRange {
                        min: 0.,
                        max: age_max,
                        step: 1.,
                    },
                    set_memory_max_age_days,
                    false,
                )),
        )
        .row(
            SettingsCardRow::new(
                "memory-advanced-per-startup",
                text.memory_per_startup.clone(),
            )
            .description(text.memory_per_startup_description.clone())
            .control(number_input_control(
                "memory-advanced-per-startup-input",
                text.memory_per_startup.clone(),
                per_startup_value,
                NumberRange {
                    min: 1.,
                    max: 128.,
                    step: 1.,
                },
                set_memory_per_startup,
                false,
            )),
        )
        .row(
            SettingsCardRow::new("memory-advanced-scan-limit", text.memory_scan_limit.clone())
                .description(text.memory_scan_limit_description.clone())
                .control(number_input_control(
                    "memory-advanced-scan-limit-input",
                    text.memory_scan_limit.clone(),
                    scan_value,
                    NumberRange {
                        min: 1.,
                        max: 50_000.,
                        step: 100.,
                    },
                    set_memory_scan_limit,
                    false,
                )),
        )
        .row(
            SettingsCardRow::new(
                "memory-advanced-concurrency",
                text.memory_concurrency.clone(),
            )
            .description(text.memory_concurrency_description.clone())
            .control(number_input_control(
                "memory-advanced-concurrency-input",
                text.memory_concurrency.clone(),
                concurrency_value,
                NumberRange {
                    min: 1.,
                    max: 16.,
                    step: 1.,
                },
                set_memory_concurrency,
                false,
            )),
        )
        .row(
            SettingsCardRow::new("memory-advanced-raw-limit", text.memory_raw_limit.clone())
                .description(text.memory_raw_limit_description.clone())
                .control(number_input_control(
                    "memory-advanced-raw-limit-input",
                    text.memory_raw_limit.clone(),
                    raw_value,
                    NumberRange {
                        min: 1.,
                        max: 4096.,
                        step: 1.,
                    },
                    set_memory_raw_limit,
                    false,
                )),
        )
        .row(
            SettingsCardRow::new(
                "memory-advanced-retention",
                text.memory_retention_days.clone(),
            )
            .description(text.memory_retention_days_description.clone())
            .control(number_input_control(
                "memory-advanced-retention-input",
                text.memory_retention_days.clone(),
                retention_value,
                NumberRange {
                    min: retention_min,
                    max: 365.,
                    step: 1.,
                },
                set_memory_retention_days,
                false,
            )),
        );
    cards.push(advanced);
    cards
}

/// The 网页搜索 page's cards, one per web section: 搜索服务 (the active
/// provider plus its credential), and — while the custom HTTP service is
/// selected — 自定义搜索服务 (endpoint, auth mode, and the credential header
/// name for the header mode).
fn websearch_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
    let provider_options: Vec<(SharedString, SharedString)> = [
        ("exa_mcp_free", text.websearch_provider_free.clone()),
        ("exa_search_api", text.websearch_provider_exa.clone()),
        ("tavily", text.websearch_provider_tavily.clone()),
        (
            "openbitfun_search_http",
            text.websearch_provider_http.clone(),
        ),
    ]
    .into_iter()
    .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
    .collect();
    let provider_current = ShellConfig::current(cx).web_search_provider.clone();
    let provider_label = provider_options
        .iter()
        .find(|(value, _)| value.as_ref() == provider_current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| provider_current.clone().into());
    let on_provider_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_websearch_provider(&value, cx));

    let mut cards = vec![SettingsCard::new("websearch")
        .title(text.settings_websearch_provider_title.clone())
        .row(
            SettingsCardRow::new(
                "websearch-provider",
                text.settings_websearch_provider.clone(),
            )
            .description(text.settings_websearch_provider_description.clone())
            .control(dropdown_control(
                "websearch-provider-menu",
                provider_label,
                Rc::new(provider_options),
                on_provider_select,
                false,
            )),
        )
        .row(
            SettingsCardRow::new("websearch-credential", text.websearch_credential.clone())
                .description(text.websearch_credential_description.clone())
                .control(DeferredControl {
                    construct: {
                        let view = view.clone();
                        let text = text.clone();
                        move |_, cx| websearch_credential_field(view, &text, cx)
                    },
                }),
        )];

    // T2-9: the custom HTTP service's endpoint/auth rows follow its provider.
    if ShellConfig::current(cx).web_search_provider == "openbitfun_search_http" {
        let snapshot = ShellConfig::current(cx);
        let endpoint_value = SharedString::from(snapshot.web_search_http_endpoint.clone());
        let auth_current = websearch_http_auth_mode_display(&snapshot.web_search_http_auth_mode);
        let auth_options: Vec<(SharedString, SharedString)> = [
            ("none", text.websearch_auth_none.clone()),
            ("bearer", text.websearch_auth_bearer.clone()),
            ("header", text.websearch_auth_header.clone()),
        ]
        .into_iter()
        .map(|(id, label)| (SharedString::from(id), SharedString::from(label)))
        .collect();
        let auth_label = auth_options
            .iter()
            .find(|(value, _)| *value == auth_current)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| auth_current.clone());

        let mut http = SettingsCard::new("websearch-http")
            .title(text.settings_websearch_http_title.clone())
            .description(text.settings_websearch_http_description.clone())
            .row(
                SettingsCardRow::new(
                    "websearch-http-endpoint",
                    text.websearch_http_endpoint.clone(),
                )
                .description(text.websearch_http_endpoint_description.clone())
                .control(text_input_control(
                    "websearch-http-endpoint-input",
                    text.websearch_http_endpoint.clone(),
                    endpoint_value,
                    |value, cx| set_websearch_http_endpoint(&value, cx),
                )),
            )
            .row(
                SettingsCardRow::new(
                    "websearch-http-auth-mode",
                    text.websearch_http_auth_mode.clone(),
                )
                .control(dropdown_control(
                    "websearch-http-auth-mode-menu",
                    auth_label,
                    Rc::new(auth_options),
                    {
                        let view = view.clone();
                        Rc::new(move |value: SharedString, cx: &mut App| {
                            set_websearch_http_auth_mode(&view, &value, cx)
                        })
                    },
                    false,
                )),
            );
        if snapshot.web_search_http_auth_mode.trim() == "header" {
            http = http.row(
                SettingsCardRow::new(
                    "websearch-http-header-name",
                    text.websearch_http_header_name.clone(),
                )
                .description(text.websearch_http_header_name_description.clone())
                .control(text_input_control(
                    "websearch-http-header-name-input",
                    text.websearch_http_header_name.clone(),
                    SharedString::from(snapshot.web_search_http_header_name.clone()),
                    |value, cx| set_websearch_http_header_name(&value, cx),
                )),
            );
        }
        cards.push(http);
    }
    cards
}

/// The usage page's cards, one per web section: the activity heatmap
/// (近半年每日用量), 所选时段用量 (range controls and totals), 用量来源 (the
/// filtered model/provider distributions), and 更多明细 (trend and endpoint
/// rows). The bodies are the component pipeline's own; the cards only re-seat
/// them. While the stats are absent the three data cards render heads only —
/// the primitive's head-only card shape (the memory page's closed advanced
/// card), never an empty husk with rows.
fn usage_cards(text: &ShellText, view: gpui_kit::WeakEntity<ShellWindow>) -> Vec<SettingsCard> {
    let activity_view = view.clone();
    let activity_text = text.clone();
    let activity = SettingsCard::new("usage-activity")
        .title(text.usage_activity_title.clone())
        .description(text.usage_activity_description.clone())
        .row(
            SettingsCardRow::body("usage-activity-grid").control(DeferredControl {
                construct: move |_, cx| usage_activity_field(activity_view, &activity_text, cx),
            }),
        );

    let overview_view = view.clone();
    let overview_text = text.clone();
    let overview = SettingsCard::new("usage-overview")
        .title(text.settings_usage_overview_title.clone())
        .row(
            SettingsCardRow::body("usage-overview-body").control(DeferredControl {
                construct: move |_, cx| usage_overview_body(overview_view, &overview_text, cx),
            }),
        );

    let distributions_view = view.clone();
    let distributions_text = text.clone();
    let distributions = SettingsCard::new("usage-distributions")
        .title(text.settings_usage_distributions_title.clone())
        .row(
            SettingsCardRow::body("usage-distributions-body").control(DeferredControl {
                construct: move |window, cx| {
                    usage_distributions_body(distributions_view, &distributions_text, window, cx)
                },
            }),
        );

    let details_view = view;
    let details_text = text.clone();
    let details = SettingsCard::new("usage-details")
        .title(text.settings_usage_details_title.clone())
        .row(
            SettingsCardRow::body("usage-details-body").control(DeferredControl {
                construct: move |_, cx| {
                    let mut column = div().flex().flex_col().gap_2().w_full();
                    column =
                        column.child(usage_trend_body(details_view.clone(), &details_text, cx));
                    column = column.child(usage_endpoint_body(details_view, &details_text, cx));
                    column.into_any_element()
                },
            }),
        );

    vec![activity, overview, distributions, details]
}

/// The archived page's single card: the web page has one section whose title
/// repeats the page title, so the card stays headless and the row carries the
/// list's own label. The cross-workspace list, its actions, states, and the
/// inline receipt all come from the shared body.
fn archived_cards(text: &ShellText, view: gpui_kit::WeakEntity<ShellWindow>) -> Vec<SettingsCard> {
    let owned_text = text.clone();
    vec![SettingsCard::new("archived").row(
        SettingsCardRow::new("archived-list", text.settings_archived_list.clone())
            .description(text.settings_archived_description.clone())
            .multiline()
            .control(DeferredControl {
                construct: move |window, cx| archived_sessions_field(view, &owned_text, window, cx),
            }),
    )]
}

/// The diagnostics page's single card (web has one logging section, so the
/// card stays headless): the log level, the log directory line, and the
/// bundle export. All three controls construct eagerly — none reads the shell
/// entity; the export receipt and the previous-run banner live in the page
/// preamble.
fn diagnostics_cards(text: &ShellText, cx: &mut App) -> Vec<SettingsCard> {
    let level = ShellConfig::current(cx).logging_level.clone();
    let options: Rc<Vec<(SharedString, SharedString)>> = Rc::new(
        ["trace", "debug", "info", "warn", "error", "off"]
            .iter()
            .map(|level| {
                // The web capitalizes the level identifier in every locale.
                let mut label = (*level).to_string();
                label[..1].make_ascii_uppercase();
                (SharedString::from(*level), SharedString::from(label))
            })
            .collect(),
    );
    let current_label = options
        .iter()
        .find(|(value, _)| value.as_ref() == level.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| level.clone().into());

    vec![SettingsCard::new("diagnostics")
        .row(
            SettingsCardRow::new("diagnostics-level", text.settings_logging_level.clone())
                .description(text.settings_logging_level_description.clone())
                .control(dropdown_control(
                    "diagnostics-level-menu",
                    current_label,
                    options,
                    Rc::new(|value: SharedString, cx: &mut App| set_logging_level(&value, cx)),
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("diagnostics-logs", text.settings_logs_dir.clone())
                .description(text.settings_diagnostics_note.clone())
                .multiline()
                .control(diagnostics_logs_field(text, cx)),
        )
        .row(
            SettingsCardRow::new(
                "diagnostics-export",
                text.settings_diagnostics_export.clone(),
            )
            .description(text.settings_diagnostics_export_description.clone())
            .control(diagnostics_export_button(text)),
        )]
}

/// The permissions page — the shell entry that carries both web pages'
/// content (`permissions` policy and `execution` runtime), one card per web
/// section: tool permissions (the three-state mode, the chat-input selector
/// switch, and the global rules block), deferred tool loading, tool
/// presentation, tool execution timers, subagents, tool argument JSON repair,
/// and review capacity. Row controls are the component pipeline's own fields
/// re-seated; every write goes through the shared setters both paths call.
fn permissions_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &App,
) -> Vec<SettingsCard> {
    let config = ShellConfig::current(cx);

    let mode_current = permission_mode_value(&config);
    let mode_options = permission_mode_options(text);
    let mode_label = mode_options
        .iter()
        .find(|(value, _)| value.as_ref() == mode_current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| SharedString::from(mode_current));
    let mode_row = SettingsCardRow::new("permissions-mode", text.settings_permissions_mode.clone())
        .description(permission_mode_description(mode_current, text))
        .control(dropdown_control_windowed(
            "permissions-mode-menu",
            mode_label,
            Rc::new(mode_options),
            Rc::new(
                move |value: SharedString, window: &mut Window, cx: &mut App| {
                    if value.as_ref() == "full_access"
                        && permission_mode_value(&ShellConfig::current(cx)) != "full_access"
                    {
                        open_full_access_confirm(window, cx);
                    } else {
                        set_permission_mode(&value, cx);
                    }
                },
            ),
            false,
        ));
    let show_in_chat_input = SettingsCardRow::new(
        "permissions-show-in-chat-input",
        text.settings_permissions_show_in_chat_input.clone(),
    )
    .description(
        text.settings_permissions_show_in_chat_input_description
            .clone(),
    )
    .control(switch_control(
        "permissions-show-in-chat-input-switch",
        object_bool(&config.flow_chat, "show_permission_mode_control", true),
        set_show_permission_mode_control,
        false,
    ));
    let policy = SettingsCard::new("permissions-policy")
        .title(text.settings_permissions_policy_title.clone())
        .description(text.settings_permissions_policy_desc.clone())
        .row(mode_row)
        .row(show_in_chat_input)
        .row(
            SettingsCardRow::new(
                "permissions-rules",
                text.settings_permission_rules_section.clone(),
            )
            .description(text.settings_permission_rules_description.clone())
            .multiline()
            .control(DeferredControl {
                construct: {
                    let text = text.clone();
                    move |window, cx| permission_rules_field(&text, window, cx)
                },
            }),
        );

    let mut deferred_enable = SettingsCardRow::new(
        "permissions-deferred-enable",
        text.settings_deferred_tools_enable.clone(),
    )
    .control(switch_control(
        "permissions-deferred-switch",
        config.deferred_tool_loading,
        |value, cx| set_deferred_tool_loading(value, cx),
        false,
    ));
    if let Some(warning) =
        (!config.deferred_tool_loading).then(|| text.settings_deferred_tools_warning.clone())
    {
        deferred_enable = deferred_enable.description(warning);
    }
    let deferred = SettingsCard::new("permissions-deferred")
        .title(text.settings_permissions_deferred_title.clone())
        .description(text.settings_permissions_deferred_desc.clone())
        .row(deferred_enable);

    let presentation_options = vec![
        (
            SharedString::from("native"),
            SharedString::from(text.settings_tool_presentation_native.clone()),
        ),
        (
            SharedString::from("ptc"),
            SharedString::from(text.settings_tool_presentation_ptc.clone()),
        ),
    ];
    let presentation_current = config.tool_presentation_mode.as_str();
    let presentation_label = presentation_options
        .iter()
        .find(|(value, _)| value.as_ref() == presentation_current)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| presentation_options[0].1.clone());
    // The web explains PTC through the option tooltip; the shell's menu items
    // carry no tooltip, so the hint joins the row description while PTC is
    // the selected mode.
    let presentation_description = if presentation_current == "ptc" {
        format!(
            "{} {}",
            text.settings_tool_presentation_description, text.settings_tool_presentation_ptc_hint
        )
    } else {
        text.settings_tool_presentation_description.clone()
    };
    let tool_presentation = SettingsCard::new("permissions-tool-presentation")
        .title(text.settings_permissions_tool_presentation_title.clone())
        .description(text.settings_permissions_tool_presentation_desc.clone())
        .row(
            SettingsCardRow::new(
                "permissions-tool-presentation-mode",
                text.settings_tool_presentation_mode.clone(),
            )
            .description(presentation_description)
            .control(dropdown_control(
                "permissions-tool-presentation-menu",
                presentation_label,
                Rc::new(presentation_options),
                Rc::new(|value: SharedString, cx: &mut App| set_tool_presentation_mode(&value, cx)),
                false,
            )),
        );

    let execution = SettingsCard::new("permissions-execution")
        .title(text.settings_permissions_execution_title.clone())
        .description(text.settings_permissions_execution_desc.clone())
        .row(
            SettingsCardRow::new(
                "permissions-tool-timeout",
                text.settings_tool_timeout.clone(),
            )
            .description(text.settings_tool_timeout_description.clone())
            .control(number_input_control(
                "permissions-tool-timeout-input",
                text.settings_tool_timeout.clone(),
                config.tool_execution_timeout_secs.unwrap_or(0) as f64,
                NumberRange {
                    min: 0.,
                    max: MAX_RUNTIME_TIMEOUT_SECS,
                    step: 5.,
                },
                |value, cx| set_tool_execution_timeout(value, cx),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "permissions-question-timeout",
                text.settings_question_timeout.clone(),
            )
            .description(text.settings_question_timeout_description.clone())
            .control(number_input_control(
                "permissions-question-timeout-input",
                text.settings_question_timeout.clone(),
                f64::from(config.user_question_timeout_secs.unwrap_or(0)),
                NumberRange {
                    min: 0.,
                    max: MAX_RUNTIME_TIMEOUT_SECS,
                    step: 5.,
                },
                |value, cx| set_user_question_timeout(value, cx),
                false,
            )),
        );

    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();
    let current_model = subagent_model_choice(&config.subagent_default_model);
    let model_options = subagent_model_options(&catalog.models, &current_model, text);
    let model_label = model_options
        .iter()
        .find(|(value, _)| value.as_ref() == current_model.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| SharedString::from(current_model.clone()));
    let batch_policy = config.subagent_batch_policy.clone();
    let batch_options = subagent_batch_options(&batch_policy, text);
    let batch_label = batch_options
        .iter()
        .find(|(value, _)| value.as_ref() == batch_policy.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| SharedString::from(batch_policy.clone()));

    let agents_view = view;
    let subagent_model_settings = Button::new("permissions-subagent-model-settings")
        .icon(gpui_kit::assets::IconName::Settings)
        .small()
        .with_variant(ButtonVariant::Ghost)
        .tooltip(text.settings_subagent_model_settings.clone())
        .on_click(move |_, _, cx| {
            if let Some(shell) = agents_view.upgrade() {
                shell.update(cx, |shell, cx| {
                    shell.set_settings_page(SettingsEntry::Agents, cx);
                });
            }
        });

    let subagents = SettingsCard::new("permissions-subagents")
        .title(text.settings_permissions_subagents_title.clone())
        .description(text.settings_permissions_subagents_desc.clone())
        .row(
            SettingsCardRow::new(
                "permissions-subagent-model",
                text.settings_subagent_model.clone(),
            )
            .description(text.settings_subagent_model_description.clone())
            .label_action(subagent_model_settings)
            .control(dropdown_control(
                "permissions-subagent-model-menu",
                model_label,
                Rc::new(model_options),
                Rc::new(|value: SharedString, cx: &mut App| set_subagent_default_model(&value, cx)),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "permissions-subagent-batch",
                text.settings_subagent_batch.clone(),
            )
            .description(subagent_batch_description(text, cx))
            .control(dropdown_control(
                "permissions-subagent-batch-menu",
                batch_label,
                Rc::new(batch_options),
                Rc::new(|value: SharedString, cx: &mut App| set_subagent_batch_policy(&value, cx)),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "permissions-subagent-concurrency",
                text.settings_subagent_concurrency.clone(),
            )
            .description(text.settings_subagent_concurrency_description.clone())
            .control(number_input_control(
                "permissions-subagent-concurrency-input",
                text.settings_subagent_concurrency.clone(),
                f64::from(config.subagent_max_concurrency),
                NumberRange {
                    min: f64::from(MIN_SUBAGENT_CONCURRENCY),
                    max: f64::from(MAX_SUBAGENT_CONCURRENCY),
                    step: 1.,
                },
                |value, cx| set_subagent_max_concurrency(value, cx),
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "permissions-swarm-concurrency",
                text.settings_swarm_concurrency.clone(),
            )
            .description(text.settings_swarm_concurrency_description.clone())
            .control(number_input_control(
                "permissions-swarm-concurrency-input",
                text.settings_swarm_concurrency.clone(),
                f64::from(config.swarm_max_concurrency),
                NumberRange {
                    min: f64::from(MIN_SUBAGENT_CONCURRENCY),
                    max: f64::from(MAX_SWARM_CONCURRENCY),
                    step: 1.,
                },
                |value, cx| set_swarm_max_concurrency(value, cx),
                false,
            )),
        );

    let json_repair = SettingsCard::new("permissions-json-repair")
        .title(text.settings_permissions_json_repair_title.clone())
        .description(text.settings_permissions_json_repair_desc.clone())
        .row(
            SettingsCardRow::new("permissions-json-repair", text.settings_json_repair.clone())
                .description(text.settings_json_repair_description.clone())
                .control(switch_control(
                    "permissions-json-repair-switch",
                    config.allow_tool_json_repair,
                    |value, cx| set_tool_json_repair(value, cx),
                    false,
                )),
        );

    let review = SettingsCard::new("permissions-review")
        .title(text.settings_permissions_review_title.clone())
        .description(text.settings_permissions_review_desc.clone())
        .row(
            SettingsCardRow::new(
                "permissions-review-parallelism",
                text.settings_review_parallelism.clone(),
            )
            .description(text.settings_review_parallelism_description.clone())
            .control(number_input_control(
                "permissions-review-parallelism-input",
                text.settings_review_parallelism.clone(),
                object_u64(
                    &config.review_team,
                    "max_parallel_reviewers",
                    DEFAULT_REVIEW_PARALLELISM,
                ) as f64,
                NumberRange {
                    min: f64::from(MIN_REVIEW_PARALLELISM),
                    max: f64::from(MAX_REVIEW_PARALLELISM),
                    step: 1.,
                },
                |value, cx| {
                    set_review_capacity_value(
                        "max_parallel_reviewers",
                        value,
                        MIN_REVIEW_PARALLELISM,
                        MAX_REVIEW_PARALLELISM,
                        cx,
                    )
                },
                false,
            )),
        )
        .row(
            SettingsCardRow::new(
                "permissions-review-queue",
                text.settings_review_queue_wait.clone(),
            )
            .description(text.settings_review_queue_wait_description.clone())
            .control(number_input_control(
                "permissions-review-queue-input",
                text.settings_review_queue_wait.clone(),
                object_u64(
                    &config.review_team,
                    "max_queue_wait_seconds",
                    DEFAULT_REVIEW_QUEUE_WAIT_SECS,
                ) as f64,
                NumberRange {
                    min: 0.,
                    max: f64::from(MAX_REVIEW_QUEUE_WAIT_SECS),
                    step: 60.,
                },
                |value, cx| {
                    set_review_capacity_value(
                        "max_queue_wait_seconds",
                        value,
                        0,
                        MAX_REVIEW_QUEUE_WAIT_SECS,
                        cx,
                    )
                },
                false,
            )),
        );

    vec![
        policy,
        deferred,
        tool_presentation,
        execution,
        subagents,
        json_repair,
        review,
    ]
}

/// Opens the full-access confirmation dialog (web `confirmDanger` on the
/// permission-mode select). Only the confirm path writes the mode; dismissing
/// the dialog leaves the stored mode untouched.
fn open_full_access_confirm(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = crate::i18n::current(cx);
        dialog
            .title(div().child(text.settings_permissions_full_access_title.clone()))
            .child(
                div()
                    .w(px(380.))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(text.settings_permissions_full_access_message.clone()),
            )
            .footer(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("permissions-full-access-cancel")
                            .label(text.session_action_cancel.clone())
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("permissions-full-access-confirm")
                            .label(text.settings_permissions_full_access_confirm.clone())
                            .with_variant(ButtonVariant::Primary)
                            .on_click(|_, window, cx| {
                                set_permission_mode("full_access", cx);
                                window.close_dialog(cx);
                            }),
                    ),
            )
    });
}

/// The 自动化 page's cards, one per web `ConfigPageSection`: the hooks
/// activation switches, the built-in and custom quick-action collections, the
/// hook declaration locations, the imported-hook manager, and the Codex
/// contract reference. The quick-action editor and the import-review block
/// stay inline (the shell has no dialog host here; registered divergence from
/// the web modals).
fn automation_cards(text: &ShellText, cx: &App) -> Vec<SettingsCard> {
    let config = ShellConfig::current(cx);
    let editor = QuickActionsEditorState::current(cx);
    let actions = config.quick_actions.clone();

    let activation = SettingsCard::new("automation-hooks")
        .title(text.settings_automation_hooks_title.clone())
        .description(text.settings_automation_hooks_desc.clone())
        .row(
            SettingsCardRow::new("automation-hooks-enabled", text.settings_hooks.clone())
                .description(text.settings_hooks_description.clone())
                .control(switch_control(
                    "automation-hooks-enabled-switch",
                    config.hooks_enabled,
                    set_hooks_enabled,
                    false,
                )),
        )
        .row(
            SettingsCardRow::new(
                "automation-project-hooks",
                text.settings_hooks_project.clone(),
            )
            .description(text.settings_hooks_project_description.clone())
            .disabled(!config.hooks_enabled)
            .control(switch_control(
                "automation-project-hooks-switch",
                config.project_hooks_enabled,
                set_project_hooks_enabled,
                !config.hooks_enabled,
            )),
        );

    let add_label = text.settings_quick_actions_add.clone();
    let mut builtin_card = SettingsCard::new("automation-qa-builtin")
        .title(text.settings_automation_qa_builtin_title.clone())
        .description(text.settings_automation_qa_builtin_desc.clone());
    let mut custom_card = SettingsCard::new("automation-qa-custom")
        .title(text.settings_automation_qa_custom_title.clone())
        .description(text.settings_automation_qa_custom_desc.clone())
        .extra(
            Button::new("quick-action-add")
                .label(add_label)
                .small()
                .outline()
                .on_click(|_, _, cx| add_quick_action(cx)),
        );

    // The inline editor follows the action it edits, so it lands in the card
    // that owns the row.
    let edited_builtin = editor.index.and_then(|index| {
        actions
            .get(index)
            .map(|action| quick_action_builtin_default(&action.id).is_some())
    });

    for (index, action) in actions.iter().enumerate() {
        let builtin = quick_action_builtin_default(&action.id).is_some();
        let row = quick_action_row(
            text,
            action,
            index,
            !builtin,
            editor.confirm_delete == Some(index),
            cx,
        );
        if builtin {
            builtin_card = builtin_card.row(row);
        } else {
            custom_card = custom_card.row(row);
        }
    }

    let editor_rows = quick_action_editor_rows(text, &editor, cx);
    match edited_builtin {
        Some(true) => {
            for row in editor_rows {
                builtin_card = builtin_card.row(row);
            }
        }
        Some(false) => {
            for row in editor_rows {
                custom_card = custom_card.row(row);
            }
        }
        None => {}
    }

    let locations = SettingsCard::new("automation-hook-locations")
        .title(text.settings_automation_hook_locations_title.clone())
        .description(text.settings_automation_hook_locations_desc.clone())
        .row(
            SettingsCardRow::new(
                "automation-hook-user-file",
                text.settings_hooks_user_file.clone(),
            )
            .description(text.settings_hooks_user_file_desc.clone()),
        )
        .row(
            SettingsCardRow::new(
                "automation-hook-project-file",
                text.settings_hooks_project_file.clone(),
            )
            .description(text.settings_hooks_project_file_desc.clone()),
        );

    let imports = automation_imports_card(text, cx);

    let compat = SettingsCard::new("automation-hooks-compat")
        .title(text.settings_automation_hooks_compat_title.clone())
        .description(text.settings_automation_hooks_compat_desc.clone())
        .row(
            SettingsCardRow::new(
                "automation-hooks-compat-reference",
                text.settings_hooks_compat_reference.clone(),
            )
            .control(
                Button::new("hooks-compat-open")
                    .label(text.settings_hooks_compat_open.clone())
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .on_click(|_, _, _| {
                        crate::markdown::open_external("https://learn.chatgpt.com/docs/hooks");
                    }),
            ),
        );

    vec![
        activation,
        builtin_card,
        custom_card,
        locations,
        imports,
        compat,
    ]
}

/// One quick-action row: label and prompt on the left, the switch plus the
/// edit (and, for custom actions, delete) buttons in the trailing lane — the
/// web `ActionRow`. Deleting an action takes the legacy two-click confirm
/// (the web opens a dialog).
fn quick_action_row(
    text: &ShellText,
    action: &openbitfun_core::service::config::types::AiExperienceQuickAction,
    index: usize,
    deletable: bool,
    confirm_delete: bool,
    cx: &App,
) -> SettingsCardRow {
    let (label, prompt) = resolve_quick_action_text(action, text);
    let row_id = format!("quick-action-{}", action.id);
    let mut controls = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(switch_control(
            format!("{row_id}-switch"),
            action.enabled,
            move |value, cx| toggle_quick_action(index, value, cx),
            false,
        ))
        .child(
            Button::new(SharedString::from(format!("{row_id}-edit")))
                .label(crate::i18n::shared_term("tools.edit", cx))
                .small()
                .outline()
                .on_click(move |_, _, cx| start_quick_action_edit(index, cx)),
        );
    if deletable {
        let delete_label = if confirm_delete {
            text.settings_quick_actions_delete_confirm.clone()
        } else {
            text.settings_quick_actions_delete.clone()
        };
        controls = controls.child(
            Button::new(SharedString::from(format!("{row_id}-delete")))
                .label(delete_label)
                .small()
                .with_variant(ButtonVariant::Danger)
                .on_click(move |_, _, cx| {
                    if QuickActionsEditorState::current(cx).confirm_delete == Some(index) {
                        delete_quick_action(index, cx);
                    } else {
                        QuickActionsEditorState::mutate(cx, |state| {
                            state.confirm_delete = Some(index)
                        });
                        cx.refresh_windows();
                    }
                }),
        );
    }
    SettingsCardRow::new(row_id, label)
        .description(prompt)
        .control(controls)
}

/// The inline quick-action editor (web `ActionFormModal`): name and prompt
/// inputs over the save/cancel line, appended to the edited action's card.
fn quick_action_editor_rows(
    text: &ShellText,
    editor: &QuickActionsEditorState,
    cx: &App,
) -> Vec<SettingsCardRow> {
    if editor.index.is_none() {
        return Vec::new();
    }
    let label_title = text.settings_quick_actions_label.clone();
    let prompt_title = text.settings_quick_actions_prompt.clone();
    let label_value = SharedString::from(editor.label.clone());
    let prompt_value = SharedString::from(editor.prompt.clone());
    let valid = !editor.label.trim().is_empty() && !editor.prompt.trim().is_empty();
    let invalid_label = text.settings_quick_actions_invalid.clone();
    let save_label = text.settings_quick_actions_save.clone();
    let cancel_label = text.settings_quick_actions_cancel.clone();

    let actions_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .w_full()
        .when(!valid, |row| {
            row.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(invalid_label),
            )
        })
        .child(
            Button::new("quick-action-save")
                .label(save_label)
                .small()
                .with_variant(ButtonVariant::Primary)
                .disabled(!valid)
                .on_click(|_, _, cx| save_quick_action_edit(cx)),
        )
        .child(
            Button::new("quick-action-cancel")
                .label(cancel_label)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(|_, _, cx| {
                    QuickActionsEditorState::mutate(cx, |state| state.index = None);
                    cx.refresh_windows();
                }),
        );

    vec![
        SettingsCardRow::new("quick-action-editor-label", label_title.clone())
            .multiline()
            .control(text_input_control(
                "quick-action-editor-label-input",
                label_title,
                label_value,
                |value: SharedString, cx| {
                    QuickActionsEditorState::mutate(cx, |state| state.label = value.to_string());
                    cx.refresh_windows();
                },
            )),
        SettingsCardRow::new("quick-action-editor-prompt", prompt_title.clone())
            .multiline()
            .control(text_input_control(
                "quick-action-editor-prompt-input",
                prompt_title,
                prompt_value,
                |value: SharedString, cx| {
                    QuickActionsEditorState::mutate(cx, |state| state.prompt = value.to_string());
                    cx.refresh_windows();
                },
            )),
        SettingsCardRow::body("quick-action-editor-actions").control(actions_row),
    ]
}

/// The imported-hook manager (web `HooksSettingsSection` imports section):
/// refresh in the card head, one row per imported source, the corrupt-store
/// diagnostics with their reset action, the empty state, and the inline
/// review block that stands in for the web dialog.
fn automation_imports_card(text: &ShellText, cx: &App) -> SettingsCard {
    use openbitfun_product_domains::external_sources::ExternalSourceScope;

    let master_on = ShellConfig::current(cx).hooks_enabled;
    let page = HookImportState::current(cx);
    let snapshot = page.snapshot.clone();
    let imports = snapshot
        .as_ref()
        .map(|snapshot| snapshot.imports.clone())
        .unwrap_or_default();
    let diagnostics = snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic
                        .code
                        .starts_with("external_hook.import_store_corrupt.")
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut card = SettingsCard::new("automation-hook-imports")
        .title(text.settings_automation_hook_imports_title.clone())
        .description(if master_on {
            text.settings_automation_hook_imports_desc.clone()
        } else {
            text.settings_hooks_imports_master_disabled.clone()
        })
        .extra(
            Button::new("hook-imports-refresh")
                .label(text.settings_hooks_imports_refresh.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .loading(page.loading)
                .on_click(|_, _, cx| fetch_hook_imports(true, cx)),
        );

    if let Some(error) = page.error.clone() {
        card = card.row(
            SettingsCardRow::body("hook-imports-status").control(
                div()
                    .min_w_0()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .child(error),
            ),
        );
    } else if page.loading {
        card = card.row(
            SettingsCardRow::body("hook-imports-status").control(
                div()
                    .min_w_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.settings_hooks_imports_loading.clone()),
            ),
        );
    }

    let imports_empty = imports.is_empty();
    let diagnostics_empty = diagnostics.is_empty();
    let busy = page.busy || page.loading;
    for import in imports {
        let import_id = import.import_id.clone();
        let name = import.source.display_name.clone();
        let state_label =
            hook_import_state_label(&import.state, &import.source.location_hint, text);
        let enabled = import.enabled;
        let can_update = !matches!(
            import.state,
            openbitfun_product_domains::external_hook_import::ImportedHookSourceStateV1::SourceMissing
        );
        let armed = page.confirm.as_deref() == Some(&format!("remove:{import_id}"));
        let update_label = text.settings_hooks_imports_update.clone();
        let remove_label = text.settings_hooks_imports_remove.clone();
        let toggle_label = text.settings_hooks_import_toggle.clone();
        let cancel_label = text.settings_quick_actions_cancel.clone();
        let confirm_title = text.settings_hooks_import_remove_title.clone();
        let confirm_note = text.settings_hooks_import_remove_confirm_note.clone();
        let confirm_label = text.settings_hooks_import_remove_confirm.clone();
        let update_source = import.source.key.clone();
        let click_id = import_id.clone();
        let update_id = import_id.clone();
        let remove_id = import_id.clone();
        let confirm_id = import_id.clone();
        let cancel_id = import_id.clone();

        let mut column = div().flex().flex_col().gap_2().w_full().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap_2()
                .w_full()
                .child(
                    Switch::new(SharedString::from(format!("hook-import-toggle-{click_id}")))
                        .checked(enabled)
                        .disabled(busy || !master_on)
                        .accessibility_label(toggle_label)
                        .on_click({
                            let id = click_id.clone();
                            move |checked: &bool, _, cx| {
                                set_hook_import_enabled(id.clone(), *checked, cx);
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "hook-import-update-{update_id}"
                    )))
                    .label(update_label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(busy || !can_update)
                    .on_click({
                        let source = update_source.clone();
                        move |_, _, cx| review_hook_import(source.clone(), cx)
                    }),
                )
                .child(
                    Button::new(SharedString::from(format!(
                        "hook-import-remove-{remove_id}"
                    )))
                    .label(remove_label)
                    .small()
                    .with_variant(ButtonVariant::Ghost)
                    .disabled(busy)
                    .on_click({
                        let id = remove_id.clone();
                        move |_, _, cx| {
                            HookImportState::mutate(cx, |state| {
                                state.confirm = Some(format!("remove:{id}"))
                            });
                            cx.refresh_windows();
                        }
                    }),
                ),
        );
        if armed {
            column = column
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(format!("{confirm_title} {confirm_note}")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!(
                                "hook-import-remove-confirm-{confirm_id}"
                            )))
                            .label(confirm_label)
                            .small()
                            .with_variant(ButtonVariant::Primary)
                            .on_click({
                                let id = confirm_id.clone();
                                move |_, _, cx| remove_hook_import(id.clone(), cx)
                            }),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "hook-import-remove-cancel-{cancel_id}"
                            )))
                            .label(cancel_label)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, _, cx| {
                                HookImportState::mutate(cx, |state| state.confirm = None);
                                cx.refresh_windows();
                            }),
                        ),
                );
        }
        card = card.row(
            SettingsCardRow::new(format!("hook-import-{import_id}"), name)
                .description(state_label)
                .multiline()
                .control(column),
        );
    }

    for diagnostic in diagnostics {
        let scope = if diagnostic.code.ends_with(".user_global") {
            ExternalSourceScope::UserGlobal
        } else {
            ExternalSourceScope::Project
        };
        let message = diagnostic.message.clone();
        let code = diagnostic.code.clone();
        let invalid_text = text.settings_hooks_import_store_invalid.clone();
        let reset_label = text.settings_hooks_import_reset.clone();
        let reset_title = text.settings_hooks_import_reset_title.clone();
        let reset_note = text.settings_hooks_import_reset_note.clone();
        let reset_confirm = text.settings_hooks_import_reset_confirm.clone();
        let cancel_label = text.settings_quick_actions_cancel.clone();
        let armed_key = format!("reset:{code}");
        let armed = page.confirm.as_deref() == Some(armed_key.as_str());
        let arm_code = code.clone();
        let run_code = code.clone();
        let cancel_code = code.clone();

        let mut column = div().flex().flex_col().gap_2().w_full().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap_2()
                .w_full()
                .child(
                    Button::new(SharedString::from(format!("hook-import-reset-{arm_code}")))
                        .label(reset_label)
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .on_click({
                            let key = format!("reset:{arm_code}");
                            move |_, _, cx| {
                                HookImportState::mutate(cx, |state| {
                                    state.confirm = Some(key.clone())
                                });
                                cx.refresh_windows();
                            }
                        }),
                ),
        );
        if armed {
            column = column
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(format!("{reset_title} {reset_note}")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!(
                                "hook-import-reset-confirm-{run_code}"
                            )))
                            .label(reset_confirm)
                            .small()
                            .with_variant(ButtonVariant::Primary)
                            .on_click(move |_, _, cx| reset_hook_import_store(scope, cx)),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "hook-import-reset-cancel-{cancel_code}"
                            )))
                            .label(cancel_label)
                            .small()
                            .with_variant(ButtonVariant::Ghost)
                            .on_click(|_, _, cx| {
                                HookImportState::mutate(cx, |state| state.confirm = None);
                                cx.refresh_windows();
                            }),
                        ),
                );
        }
        card = card.row(
            SettingsCardRow::new(format!("hook-import-reset-{code}"), invalid_text)
                .description(message)
                .danger(true)
                .multiline()
                .control(column),
        );
    }

    if page.loaded && imports_empty && diagnostics_empty {
        card = card.row(
            SettingsCardRow::body("hook-imports-empty").control(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.settings_hooks_imports_empty.clone()),
            ),
        );
    }

    if let Some(plan) = page.plan.clone() {
        let is_update = snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot.imports.iter().any(|import| {
                    import.source.key.provider_id == plan.source.key.provider_id
                        && import.source.key.source_id == plan.source.key.source_id
                })
            })
            .unwrap_or(false);
        let title = format!(
            "{} · {}",
            text.settings_hooks_import_review_title, plan.source.display_name
        );
        let warning = text.settings_hooks_import_review_warning.clone();
        let stale = page
            .plan_stale
            .then(|| text.settings_hooks_import_stale.clone());
        let timeout_label = text.settings_hooks_import_timeout.clone();
        let skipped_label = text.settings_hooks_import_skipped.clone();
        let handlers = plan.handlers.clone();
        let skipped = plan.skipped.clone();
        let confirm_label = if is_update {
            text.settings_hooks_import_confirm_update.clone()
        } else {
            text.settings_hooks_import_confirm.clone()
        };
        let cancel_label = text.settings_quick_actions_cancel.clone();
        let busy = page.busy;

        let mut column = div()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().text_xs().text_color(cx.theme().danger).child(warning));
        if let Some(stale) = stale {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(stale),
            );
        }
        for handler in &handlers {
            let matcher = handler
                .matcher
                .as_deref()
                .map(|matcher| format!(" · {matcher}"))
                .unwrap_or_default();
            let mut line = format!("{} · {}{}", handler.event, handler.command, matcher);
            if let Some(timeout) = handler.timeout_seconds {
                line.push_str(&format!(" · {timeout_label} {timeout}"));
            }
            column = column.child(
                div()
                    .min_w_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(line),
            );
        }
        for skipped in &skipped {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{skipped_label} {} · {}",
                        skipped.count, skipped.reason_code
                    )),
            );
        }
        column = column.child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    Button::new("hook-import-review-confirm")
                        .label(confirm_label)
                        .small()
                        .with_variant(ButtonVariant::Primary)
                        .loading(busy)
                        .on_click(|_, _, cx| apply_hook_import_plan(cx)),
                )
                .child(
                    Button::new("hook-import-review-cancel")
                        .label(cancel_label)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(|_, _, cx| dismiss_hook_import_plan(cx)),
                ),
        );
        card = card.row(SettingsCardRow::body("hook-import-review").control(column));
    }

    card
}
