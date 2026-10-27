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

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::{
    div, prelude::*, px, Anchor, AnyElement, App, Entity, FontWeight, IntoElement, ParentElement,
    RenderOnce, SharedString, Styled, Subscription, Window,
};
use std::rc::Rc;

use crate::i18n::ShellText;
use crate::settings::{
    body_json_valid, headers_json_valid, model_category_values, model_editor_presets_element,
    models_configured_list, proxy_enabled, proxy_text, set_ai_model_body_mode,
    set_ai_model_body_text, set_ai_model_category, set_ai_model_context_window,
    set_ai_model_header_mode, set_ai_model_headers_text, set_ai_model_inline_think,
    set_ai_model_max_tokens, set_ai_model_reasoning_default, set_ai_model_skip_ssl,
    set_default_primary_model, set_proxy_key, set_stream_timeout_value, ModelEditorState,
    MAX_STREAM_TIMEOUT_SECS, MODEL_EDITOR_DEFAULT_CONTEXT_WINDOW, MODEL_EDITOR_MIN_CONTEXT_WINDOW,
    STREAM_IDLE_TIMEOUT_PATH, STREAM_TTFT_TIMEOUT_PATH,
};
use crate::settings::{
    default_harness_facts, default_worktree_settings_document, entry_description, entry_label,
    flow_chat_fixed_mode, flow_chat_strategy, language_choices, object_bool, object_str,
    object_u64, session_title_model_options, set_autostart, set_flow_chat_fixed_mode,
    set_flow_chat_strategy, set_language, set_prevent_sleep, set_session_title_enabled,
    set_session_title_model, set_terminal_default_shell, set_terminal_font_family,
    set_terminal_font_size, set_terminal_scrollback, set_ui_font_custom_px, set_ui_font_level,
    set_worktree_auto_delete_limit, set_worktree_switch, set_worktree_text, shortcut_keycaps,
    terminal_profiles_field, theme_button, ui_font_base_px, ui_font_level_options, SettingsEntry,
    ShellConfig, CLOSE_BEHAVIOR_PATH, CLOSE_MINIMIZE, CLOSE_QUIT, GLOBAL_SHORTCUTS, MAX_FONT_SIZE,
    MAX_SCROLLBACK, MAX_UI_FONT_PX, MIN_FONT_SIZE, MIN_SCROLLBACK, MIN_UI_FONT_PX,
    UI_FONT_CUSTOM_LEVEL,
};
use crate::settings::{
    memory_actions_button, memory_advanced_toggle_button, memory_model_options,
    memory_notice_element, memory_policy_value, set_memory_btw, set_memory_concurrency,
    set_memory_consolidation_model, set_memory_enabled, set_memory_extract_model,
    set_memory_max_age_days, set_memory_min_idle_hours, set_memory_per_startup, set_memory_policy,
    set_memory_raw_limit, set_memory_retention_days, set_memory_scan_limit, MemoryPageState,
};
use crate::shell::ShellWindow;

/// One setting row: label + optional description on the left, the control in a
/// fixed trailing lane. The owning card supplies the divider above every row
/// but the first and rounds the hover tint at the card's corners.
pub(crate) struct SettingsCardRow {
    id: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    control: AnyElement,
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
            multiline: false,
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
        let label_block = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .when(self.danger, |label| label.text_color(theme.danger))
                    .child(label),
            )
            .children(self.description.map(|text| {
                div()
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
            .child(label_block);
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
struct DeferredControl<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    construct: F,
}

impl<F> RenderOnce for DeferredControl<F>
where
    F: FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.construct)(window, cx)
    }
}

/// The card-path renderer: `Some` once the entry migrated, `None` while it
/// still renders through the component `Settings` pipeline.
pub(crate) fn render_card_page(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    entry: SettingsEntry,
    cx: &mut App,
) -> Option<AnyElement> {
    let cards = match entry {
        SettingsEntry::General => general_cards(text, view, cx),
        SettingsEntry::Appearance => appearance_cards(text, cx),
        SettingsEntry::Terminal => terminal_cards(text, view, cx),
        SettingsEntry::SessionSearch => session_search_cards(text, view, cx),
        SettingsEntry::WorktreesGit => worktrees_git_cards(text, view, cx),
        SettingsEntry::Shortcuts => shortcuts_cards(text, cx),
        SettingsEntry::Models => models_cards(text, cx),
        SettingsEntry::Memory => memory_cards(text, view, cx),
        _ => return None,
    };
    Some(page_frame(
        entry,
        text,
        page_preamble(entry, text, cx),
        cards,
        cx,
    ))
}

/// Elements rendered between the page header and the first card — the
/// shortcuts page's read-only note is the only consumer so far.
fn page_preamble(entry: SettingsEntry, text: &ShellText, cx: &App) -> Vec<AnyElement> {
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
                .children(cards),
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
fn dropdown_control(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    options: Rc<Vec<(SharedString, SharedString)>>,
    on_select: Rc<dyn Fn(SharedString, &mut App)>,
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
                    move |_, _, cx| {
                        on_select(value.clone(), cx);
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
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    value: f64,
    range: NumberRange,
    on_change: impl Fn(f64, &mut App) + 'static,
    disabled: bool,
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
                .w_32()
                .into_any_element()
        },
    }
}

/// The 通用 page's cards. Content 1:1 with the migrated component group — the
/// same seven rows, same order, same setters (web `application.general`).
fn general_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
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

    vec![SettingsCard::new("general")
        .row(
            SettingsCardRow::new("general-version", text.settings_version.clone()).control(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
            ),
        )
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
        )]
}

/// The 外观 page's cards, one per web `ConfigPageSection` the shell covers:
/// 界面 (language), 外观 (theme), 字体大小 (size level plus the conditional
/// custom size). The web page's session-panel section and the font preview
/// row are registered E3 gaps (design README §2.2).
fn appearance_cards(text: &ShellText, cx: &mut App) -> Vec<SettingsCard> {
    // 语言：the three shell locales, then the persisted id verbatim when it
    // is outside that domain — the same menu the component page renders.
    let current_language = ShellConfig::current(cx).language.as_str().to_string();
    let choices = language_choices();
    let language_label = choices
        .iter()
        .find(|(value, _)| *value == current_language)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current_language.clone().into());
    let on_language_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_language(&value, cx));

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
        SettingsCard::new("appearance-language")
            .title(text.settings_appearance_interface_title.clone())
            .description(text.settings_appearance_interface_description.clone())
            .row(
                SettingsCardRow::new("appearance-language", text.language.clone()).control(
                    dropdown_control(
                        "appearance-language-menu",
                        language_label,
                        Rc::new(choices),
                        on_language_select,
                        false,
                    ),
                ),
            ),
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

/// The 会话搜索 page's cards, one per web section: the default-Harness card,
/// the session-title card, and the headless jump row that opens the shell's
/// search overlay — this shell's session-search owner (design README §2.2).
fn session_search_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
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
    let on_strategy_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_flow_chat_strategy(&value, cx));

    let enabled = ShellConfig::current(cx).session_title_enabled;
    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();
    let current_model = ShellConfig::current(cx).session_title_model;
    let model_options = session_title_model_options(&catalog.models, &current_model, text);
    let model_label = model_options
        .iter()
        .find(|(value, _)| value.as_ref() == current_model.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current_model.clone().into());
    let model_description = if catalog.models.is_empty() {
        Some(text.settings_session_title_models_empty.clone())
    } else {
        None
    };
    let on_model_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_session_title_model(&value, cx));

    let mut harness_card = SettingsCard::new("session-search-harness")
        .title(text.settings_default_harness_section.clone())
        .description(text.settings_default_harness_section_note.clone())
        .row(
            SettingsCardRow::new(
                "session-search-harness-strategy",
                text.settings_default_harness_strategy.clone(),
            )
            .description(facts.strategy_description)
            .control(dropdown_control(
                "session-search-harness-strategy-menu",
                SharedString::from(current_strategy),
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
                "session-search-harness-fixed",
                text.settings_default_harness_fixed_mode.clone(),
            )
            .control(dropdown_control(
                "session-search-harness-fixed-menu",
                fixed_label,
                Rc::new(facts.fixed_options),
                Rc::new(|value: SharedString, cx| set_flow_chat_fixed_mode(&value, cx)),
                false,
            )),
        );
    }

    let mut title_card = SettingsCard::new("session-search-title")
        .title(text.settings_session_title_section.clone())
        .description(text.settings_session_title_section_note.clone())
        .row(
            SettingsCardRow::new(
                "session-search-title-enable",
                text.settings_session_title_enable.clone(),
            )
            .control(switch_control(
                "session-search-title-enable-switch",
                enabled,
                set_session_title_enabled,
                false,
            )),
        );
    let mut model_row = SettingsCardRow::new(
        "session-search-title-model",
        text.settings_session_title_model.clone(),
    )
    .control(dropdown_control(
        "session-search-title-model-menu",
        model_label,
        Rc::new(model_options),
        on_model_select,
        !enabled,
    ))
    .disabled(!enabled);
    if let Some(description) = model_description {
        model_row = model_row.description(description);
    }
    title_card = title_card.row(model_row);

    vec![
        harness_card,
        title_card,
        SettingsCard::new("session-search-open").row(
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
        ),
    ]
}

/// The 工作区与 Git page's cards: the web isolation defaults (G-SE16) as one
/// titled card plus the headless jump row that opens worktree management in
/// the Git panel, which remains this shell's worktree owner.
fn worktrees_git_cards(
    text: &ShellText,
    view: gpui_kit::WeakEntity<ShellWindow>,
    cx: &mut App,
) -> Vec<SettingsCard> {
    let document = ShellConfig::current(cx).worktrees.clone();
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

    vec![
        SettingsCard::new("worktrees-isolation")
            .title(text.settings_worktree_section.clone())
            .description(text.settings_worktree_section_note.clone())
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
            ),
        SettingsCard::new("worktrees-open").row(
            SettingsCardRow::new(
                "worktrees-open",
                text.settings_jump_worktrees_description.clone(),
            )
            .control(
                Button::new("open-worktrees-git")
                    .label(text.settings_jump_worktrees_action.clone())
                    .with_variant(ButtonVariant::Ghost)
                    .on_click({
                        let view = view.clone();
                        move |_: &gpui_kit::ClickEvent, window, cx| {
                            if let Some(shell) = view.upgrade() {
                                shell.update(cx, |shell, cx| {
                                    shell.open_git_panel(window, cx);
                                    shell.set_git_view(crate::git::GitView::Worktrees, cx);
                                });
                            }
                        }
                    }),
            ),
        ),
    ]
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

/// The 模型 page's cards: the untitled primary card (default model + the
/// configured list, the web page's top block), the stream and proxy cards
/// (the web page's same-named sections), and — while a row is opened — the
/// editor and advanced cards standing in for the web page's model editor.
fn models_cards(text: &ShellText, cx: &mut App) -> Vec<SettingsCard> {
    let catalog = cx
        .try_global::<crate::runtime::ModelCatalog>()
        .cloned()
        .unwrap_or_default();
    let current_default = SharedString::from(
        ShellConfig::current(cx)
            .default_primary_model
            .unwrap_or_default(),
    );
    let default_options: Vec<(SharedString, SharedString)> = catalog
        .models
        .iter()
        .filter(|model| model.enabled)
        .map(|model| (model.id.clone().into(), model.label().into()))
        .collect();
    let default_label = default_options
        .iter()
        .find(|(value, _)| value.as_ref() == current_default.as_str())
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| current_default.clone().into());
    let on_default_select: Rc<dyn Fn(SharedString, &mut App)> =
        Rc::new(|value: SharedString, cx| set_default_primary_model(&value, cx));

    let mut cards = vec![SettingsCard::new("models")
        .row(
            SettingsCardRow::new("models-default", text.models_default.clone())
                .description(if catalog.models.is_empty() {
                    text.models_empty.clone()
                } else {
                    text.models_default_description.clone()
                })
                .control(dropdown_control(
                    "models-default-menu",
                    default_label,
                    Rc::new(default_options),
                    on_default_select,
                    false,
                )),
        )
        .row(
            SettingsCardRow::new("models-configured", text.models_configured.clone())
                .multiline()
                .control(DeferredControl {
                    construct: move |_window, cx| models_configured_list(cx),
                }),
        )];

    // The inline editor: the card pair stands in for the web page's model
    // editor while a row is opened from the list.
    if let Some(model_id) = ModelEditorState::current(cx).model_id.clone() {
        let Some(model) = ShellConfig::current(cx)
            .ai_models
            .iter()
            .find(|model| model.id == model_id)
            .cloned()
        else {
            return cards;
        };
        let editor_text = text.clone();
        let close_button = Button::new("models-editor-close")
            .label(text.models_edit_close.clone())
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(|_, _window, cx| {
                ModelEditorState::mutate(cx, |state| state.model_id = None);
                cx.refresh_windows();
            });

        // Category: the web's four choices, plus a value written by a newer
        // build kept verbatim so it is never silently remapped.
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
        let category_current = ShellConfig::current(cx)
            .ai_models
            .iter()
            .find(|model| model.id == model_id)
            .map(|model| {
                serde_json::to_value(&model.category)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
                    .unwrap_or_else(|| "general_chat".to_string())
            })
            .unwrap_or_else(|| "general_chat".to_string());
        let category_label = category_options
            .iter()
            .find(|(value, _)| value.as_ref() == category_current)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| category_current.clone().into());
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
                // The shared contract caps an explicit output limit at 40% of
                // the context window; an out-of-range value is ignored rather
                // than persisted invalid.
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

        // Reasoning: the stored presets, then a stored default this build
        // cannot resolve kept verbatim.
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
            .title(format!("{} · {}", text.models_edit_title, model.name))
            .extra(close_button)
            .row(
                SettingsCardRow::new("models-editor-category", text.models_category.clone())
                    .control(dropdown_control(
                        "models-editor-category-menu",
                        category_label,
                        Rc::new(category_options),
                        on_category_select,
                        false,
                    )),
            )
            .row(
                SettingsCardRow::new("models-editor-context", text.models_context_window.clone())
                    .description(text.models_context_window_hint.clone())
                    .control(number_input_control(
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
                    )),
            )
            .row(
                SettingsCardRow::new("models-editor-max-tokens", text.models_max_tokens.clone())
                    .description(text.models_max_tokens_hint.clone())
                    .control(number_input_control(
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
        cards.push(editor_card);

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
        let drafts_here =
            editor.drafts_ready && editor.model_id.as_deref() == Some(model_id.as_str());
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
                SettingsCardRow::new("models-advanced-format", text.models_request_format.clone())
                    .description(model.provider.clone()),
            )
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
                            move |value, cx| {
                                set_ai_model_headers_text(&model_id, value.to_string(), cx)
                            }
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
                            move |value, cx| {
                                set_ai_model_body_text(&model_id, value.to_string(), cx)
                            }
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
        // Invalid JSON stays in the draft (the stored row is untouched); say
        // so instead of silently showing a field that is not saving.
        if invalid {
            advanced = advanced.row(
                SettingsCardRow::new(
                    "models-advanced-json-invalid",
                    editor_text.models_json_invalid.clone(),
                )
                .danger(true),
            );
        }
        cards.push(advanced);
    }

    // G-SE10: the streaming-timeout pair; zero means "no timeout".
    let ttft_value = ShellConfig::current(cx)
        .stream_ttft_timeout_secs
        .unwrap_or(0) as f64;
    let idle_value = ShellConfig::current(cx)
        .stream_idle_timeout_secs
        .unwrap_or(0) as f64;
    cards.push(
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
            ),
    );

    // G-SE11: the proxy document; the three credential rows turn off with the
    // switch.
    let proxy = ShellConfig::current(cx).proxy;
    let proxy_on = proxy_enabled(&proxy);
    let proxy_url = SharedString::from(proxy_text(&proxy, "url"));
    let proxy_username = SharedString::from(proxy_text(&proxy, "username"));
    let proxy_password = SharedString::from(proxy_text(&proxy, "password"));
    cards.push(
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
            ),
    );
    cards
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

/// The 会话与记忆 page's cards, one per web section: 基础 (the enable pair,
/// BTW source, external-context policy, the actions menu), 模型 (the
/// extraction and consolidation followers), and 高级设置 — head-only while
/// collapsed, its seven tuned numbers as rows while open. The scope notice
/// and the inline receipt render above the cards (page preamble).
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
    let mut cards = vec![basic, models];
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
