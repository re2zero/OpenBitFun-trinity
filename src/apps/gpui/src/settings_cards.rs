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

use gpui_kit::component::button::Button;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    div, prelude::*, px, Anchor, AnyElement, App, FontWeight, IntoElement, ParentElement,
    RenderOnce, SharedString, Styled, Window,
};
use std::rc::Rc;

use crate::i18n::ShellText;
use crate::settings::{
    entry_description, entry_label, set_autostart, set_prevent_sleep, SettingsEntry, ShellConfig,
    CLOSE_BEHAVIOR_PATH, CLOSE_MINIMIZE, CLOSE_QUIT,
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

    fn render_row(self, divider: bool, edge_top: bool, edge_bottom: bool, cx: &App) -> AnyElement {
        let theme = cx.theme();
        // A softer derivative of the border token: the card outline keeps the
        // full-strength hairline (style.md 「卡内行间发丝线比卡描边更淡」).
        let divider_color = theme.border.opacity(0.6);
        let radius = theme.radius_lg;
        let label = self.label.clone();
        div()
            .id(self.id.clone())
            .debug_selector(move || format!("settings-row-{}", self.id))
            .flex()
            .flex_row()
            .items_center()
            .gap_4()
            .px_4()
            .py_3()
            .when(divider, |row| row.border_t_1().border_color(divider_color))
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
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().child(label))
                    .children(self.description.map(|text| {
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(text)
                    })),
            )
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
/// check row reads the shell's update state) defer like the component
/// pipeline's `SettingItem::render` callbacks do.
#[derive(IntoElement)]
struct DeferredControl<F>
where
    F: FnOnce(&mut App) -> AnyElement + 'static,
{
    construct: F,
}

impl<F> RenderOnce for DeferredControl<F>
where
    F: FnOnce(&mut App) -> AnyElement + 'static,
{
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.construct)(cx)
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
        _ => return None,
    };
    Some(page_frame(entry, text, cards, cx))
}

/// The page frame: the web `ConfigPageHeader` geometry over a scrollable card
/// column (the card path owns its scrolling; the component pipeline does not).
fn page_frame(
    entry: SettingsEntry,
    text: &ShellText,
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
) -> AnyElement {
    Switch::new(id.into())
        .checked(checked)
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
) -> AnyElement {
    Button::new(id.into())
        .label(label)
        .dropdown_caret(true)
        .outline()
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
            )),
        )
        .row(
            SettingsCardRow::new("general-prevent-sleep", text.settings_prevent_sleep.clone())
                .description(text.settings_prevent_sleep_description.clone())
                .control(switch_control(
                    "general-prevent-sleep-switch",
                    prevent_sleep_checked,
                    |value, cx| set_prevent_sleep(value, cx),
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
                        move |cx| crate::settings::update_check_field(view, &text, cx)
                    },
                }),
        )]
}
