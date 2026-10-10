//! Shell insights page: usage reports over recent sessions, generated through
//! the same core `InsightsService` the desktop insights page uses. The shell
//! renders the report's structured facts (headline numbers, at-a-glance
//! narratives, project areas) natively; the HTML report file the service also
//! writes stays a desktop-web concern.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt as _};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{div, prelude::*, Anchor, App, SharedString};
use openbitfun_core::agentic::insights::{InsightsReport, InsightsReportMeta};
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};

/// Report ranges the generate action offers (days).
pub(crate) const INSIGHTS_RANGES: [u32; 3] = [7, 30, 90];

/// State of the insights page; lives on the shell window and is updated only
/// through [`RuntimeEvent`] projections.
#[derive(Debug, Default)]
pub(crate) struct InsightsPageState {
    pub latest: Vec<InsightsReportMeta>,
    /// The report currently shown, if any (generated or opened from the
    /// latest list).
    pub report: Option<Box<InsightsReport>>,
    /// A generate call is in flight; the page disables the action and offers
    /// cancellation.
    pub generating: bool,
    pub error: Option<String>,
    /// The latest list was fetched at least once (drives reload-on-open).
    pub loaded: bool,
}

/// The full page: a generate header, the latest-reports list, and the
/// currently open report.
pub(crate) fn render_page(
    state: &InsightsPageState,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut column = div().w_full().h_full().flex().flex_col().gap_3().p_4();

    let selected_days = state.report.as_ref().map(|report| report.days_covered);
    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_lg()
                .text_color(cx.theme().foreground)
                .child(text.insights_title.clone()),
        );
    let mut actions = div().flex().items_center().gap_2();
    if state.generating {
        actions = actions.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.insights_generating.clone()),
        );
        let cancel_tx = tx.clone();
        actions = actions.child(
            Button::new("insights-cancel")
                .label(text.session_action_cancel.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _, _| {
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.cancel_insights(cancel_tx.clone());
                    }
                }),
        );
    } else {
        let days = selected_days.unwrap_or(30);
        let menu_tx = tx.clone();
        actions = actions.child(
            Button::new("insights-generate")
                .label(format!("{} · {days} d", text.insights_generate))
                .small()
                .with_variant(ButtonVariant::Primary)
                .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, cx| {
                    let label = i18n_generate_label(cx);
                    let mut popup = menu;
                    for range in INSIGHTS_RANGES {
                        let menu_tx = menu_tx.clone();
                        popup = popup.item(
                            PopupMenuItem::new(format!("{label} · {range} d")).on_click(
                                move |_: &gpui_kit::ClickEvent, _window, _cx| {
                                    if let Some(handle) = runtime::runtime_handle() {
                                        handle.generate_insights(range, menu_tx.clone());
                                    }
                                },
                            ),
                        );
                    }
                    popup
                }),
        );
    }
    header = header.child(actions);
    column = column.child(header);

    if let Some(error) = &state.error {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone()),
        );
    }

    if !state.latest.is_empty() {
        let mut list = div().v_flex().gap_1();
        for meta in &state.latest {
            let path = meta.path.clone();
            let load_tx = tx.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!(
                        "insight-{}",
                        markdown_id_key(&meta.path)
                    )))
                    .on_click(move |_, _, _| {
                        if let Some(handle) = runtime::runtime_handle() {
                            handle.load_insights_report(path.clone(), load_tx.clone());
                        }
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .hover(|style| style.bg(cx.theme().sidebar_accent))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(format!(
                                "{} → {}",
                                meta.date_range.start, meta.date_range.end
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} / {} · {}",
                                meta.analyzed_sessions, meta.total_sessions, meta.total_messages
                            )),
                    ),
            );
        }
        column = column.child(list);
    } else if !state.loaded {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.insights_loading.clone()),
        );
    } else {
        column = column.child(crate::shell::brand_empty(
            gpui_kit::assets::IconName::ChartPie,
            &text.insights_empty_title,
            &text.insights_empty,
        ));
    }

    if let Some(report) = &state.report {
        column = column.child(render_report(report, text, cx));
    }

    column.into_any_element()
}

/// The open report: headline numbers, at-a-glance narratives, interaction
/// style, and project areas.
fn render_report(report: &InsightsReport, text: &ShellText, cx: &App) -> gpui_kit::Div {
    let headline = vec![
        (
            text.insights_sessions.clone(),
            report.total_sessions.to_string(),
        ),
        (
            text.insights_analyzed.clone(),
            report.analyzed_sessions.to_string(),
        ),
        (
            text.insights_messages.clone(),
            report.total_messages.to_string(),
        ),
        (
            text.insights_days_covered.clone(),
            report.days_covered.to_string(),
        ),
    ];
    let mut numbers = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_4()
        .p_3()
        .rounded(cx.theme().radius);
    for (label, value) in headline {
        numbers = numbers.child(
            div()
                .v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_lg()
                        .text_color(cx.theme().foreground)
                        .child(value),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                ),
        );
    }

    let glance = vec![
        (
            text.insights_whats_working.clone(),
            &report.at_a_glance.whats_working,
        ),
        (
            text.insights_whats_hindering.clone(),
            &report.at_a_glance.whats_hindering,
        ),
        (
            text.insights_quick_wins.clone(),
            &report.at_a_glance.quick_wins,
        ),
        (
            text.insights_looking_ahead.clone(),
            &report.at_a_glance.looking_ahead,
        ),
    ];
    let mut has_sections = false;
    let mut sections = div().v_flex().gap_3();
    for (title, body) in glance {
        if body.trim().is_empty() {
            continue;
        }
        has_sections = true;
        sections = sections.child(
            div()
                .v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(body.clone()),
                ),
        );
    }

    let mut report_view = div()
        .v_flex()
        .gap_3()
        .mt_2()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(numbers);
    if has_sections {
        report_view = report_view.child(sections);
    }

    if !report.project_areas.is_empty() {
        let mut areas = div().v_flex().gap_1();
        for area in &report.project_areas {
            areas = areas.child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(format!("{} ({})", area.name, area.session_count)),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(area.description.clone()),
                    ),
            );
        }
        report_view = report_view.child(areas);
    }

    report_view
}

fn i18n_generate_label(cx: &App) -> String {
    crate::i18n::current(cx).insights_generate
}

/// Stable element id key for report rows (paths are unique already, but stay
/// consistent with the transcript id scheme).
fn markdown_id_key(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_ranges_are_ordered_and_non_empty() {
        assert_eq!(INSIGHTS_RANGES, [7, 30, 90]);
    }
}
