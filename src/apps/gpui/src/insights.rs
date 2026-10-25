//! Shell insights page: usage reports over recent sessions, generated through
//! the same core `InsightsService` the desktop insights page uses. The shell
//! renders the report's structured facts (headline numbers, at-a-glance
//! narratives, project areas) natively; the HTML report file the service also
//! writes stays a desktop-web concern.

use chrono::TimeZone as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt as _};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{div, prelude::*, Anchor, App, ClipboardItem, SharedString};
use openbitfun_core::agentic::insights::{InsightsReport, InsightsReportMeta, InsightsSuggestions};
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, ModelCatalog, RuntimeEvent};

/// Report ranges the generate control offers (days) — the web chip set.
pub(crate) const INSIGHTS_RANGES: [u32; 4] = [7, 14, 30, 90];
/// The range the control starts on (web `selectedDays`).
pub(crate) const INSIGHTS_DEFAULT_DAYS: u32 = 30;
/// The daemon's own model selector for "use the configured primary model"
/// (web `selectedModel` default).
pub(crate) const INSIGHTS_DEFAULT_MODEL: &str = "primary";

/// State of the insights page; lives on the shell window and is updated only
/// through [`RuntimeEvent`] projections.
#[derive(Debug)]
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
    /// Range the generate control would run with.
    pub selected_days: u32,
    /// Model selector for the next generation (web `selectedModel`).
    pub selected_model: String,
}

impl Default for InsightsPageState {
    fn default() -> Self {
        Self {
            latest: Vec::new(),
            report: None,
            generating: false,
            error: None,
            loaded: false,
            selected_days: INSIGHTS_DEFAULT_DAYS,
            selected_model: INSIGHTS_DEFAULT_MODEL.to_string(),
        }
    }
}

/// One entry of the model menu: the selector value plus its label. The
/// primary entry always comes first, then every enabled configured model
/// (web's combobox order).
pub(crate) fn model_options(
    catalog: &[crate::runtime::ModelChoice],
    text: &ShellText,
) -> Vec<(String, String)> {
    let mut options = vec![(
        INSIGHTS_DEFAULT_MODEL.to_string(),
        text.insights_model_primary.clone(),
    )];
    for model in catalog {
        if model.enabled {
            options.push((model.id.clone(), model.label()));
        }
    }
    options
}

/// The label to show for a selection, falling back to the primary entry when
/// the persisted selector no longer exists (web resets to `primary` too).
pub(crate) fn selected_model_label(options: &[(String, String)], selected: &str) -> String {
    options
        .iter()
        .find(|(id, _)| id == selected)
        .map(|(_, label)| label.clone())
        .unwrap_or_else(|| {
            options
                .first()
                .map(|(_, label)| label.clone())
                .unwrap_or_default()
        })
}

/// The history card's generation timestamp, in local time. Reports persist
/// `generated_at` as epoch **seconds** (`SystemTime::…as_secs()`; the web
/// multiplies by 1000), not milliseconds.
pub(crate) fn history_timestamp(generated_at_secs: u64) -> String {
    chrono::Local
        .timestamp_opt(generated_at_secs as i64, 0)
        .single()
        .map(|at| at.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| generated_at_secs.to_string())
}

/// The card's analysed window; the stored instants are ISO strings, so only
/// their date part is shown (web slices the first ten characters the same way).
pub(crate) fn history_range(start: &str, end: &str) -> String {
    format!("{} → {}", date_part(start), date_part(end))
}

fn date_part(value: &str) -> String {
    value.chars().take(10).collect()
}

/// The card's tag line: top goals then languages, the web order, deduplicated
/// so a goal that is also a language is not repeated.
pub(crate) fn history_tags(meta: &InsightsReportMeta) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in meta.top_goals.iter().chain(meta.languages.iter()) {
        let tag = tag.trim();
        if !tag.is_empty() && !tags.iter().any(|kept| kept == tag) {
            tags.push(tag.to_string());
        }
    }
    tags
}

/// The full page: a generate header, the report history, and the currently
/// open report.
///
/// The column keeps its natural height: forcing `h_full()` capped the scroll
/// container's content at the viewport, so a long report had no scroll extent
/// and everything past the fold was unreachable.
pub(crate) fn render_page(
    state: &InsightsPageState,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut column = div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .debug_selector(|| "insights-page-content".to_string());

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
        // G-I1: the range chips (web 7/14/30/90 buttons).
        let mut ranges = div().flex().items_center().gap_1();
        ranges = ranges.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text.insights_range_label.clone()),
        );
        for range in INSIGHTS_RANGES {
            let selected = state.selected_days == range;
            let set_view = view.clone();
            ranges = ranges.child(
                Button::new(SharedString::from(format!("insights-range-{range}")))
                    .label(crate::i18n::insights_range_days(range, cx))
                    .small()
                    .with_variant(if selected {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Ghost
                    })
                    .on_click(move |_, _window, cx| {
                        let Some(shell) = set_view.upgrade() else {
                            return;
                        };
                        shell.update(cx, |shell, cx| {
                            shell.insights.selected_days = range;
                            cx.notify();
                        });
                    }),
            );
        }
        actions = actions.child(ranges);

        // G-I1: the model selector (web combobox over the configured models).
        let catalog = cx
            .try_global::<ModelCatalog>()
            .map(|catalog| catalog.models.clone())
            .unwrap_or_default();
        let options = model_options(&catalog, text);
        let selected_label = selected_model_label(&options, &state.selected_model);
        let selected = state.selected_model.clone();
        let options_for_menu = options.clone();
        actions = actions.child(
            Button::new("insights-model")
                .label(format!("{}: {selected_label}", text.insights_model_label))
                .small()
                .with_variant(ButtonVariant::Secondary)
                .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _window, _cx| {
                    let mut popup = menu;
                    for (id, label) in &options_for_menu {
                        let pick_view = view.clone();
                        let pick_id = id.clone();
                        let is_selected = *id == selected;
                        popup = popup.item(
                            PopupMenuItem::new(label.clone())
                                .checked(is_selected)
                                .on_click(move |_: &gpui_kit::ClickEvent, _window, cx| {
                                    let Some(shell) = pick_view.upgrade() else {
                                        return;
                                    };
                                    shell.update(cx, |shell, cx| {
                                        shell.insights.selected_model = pick_id.clone();
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    popup
                }),
        );

        let generate_tx = tx.clone();
        let days = state.selected_days;
        let model = state.selected_model.clone();
        actions = actions.child(
            Button::new("insights-generate")
                .label(text.insights_generate.clone())
                .small()
                .with_variant(ButtonVariant::Primary)
                .on_click(move |_, _, _| {
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.generate_insights(days, Some(model.clone()), generate_tx.clone());
                    }
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
        // G-I6: the history section header states the retention the backend
        // enforces (web `Report History` + `Only the 5 most recent …`).
        column = column.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(text.insights_history.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.insights_keep_latest.clone()),
                ),
        );
        let mut list = div().v_flex().gap_1();
        for meta in &state.latest {
            list = list.child(render_history_row(meta, tx, cx));
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

/// G-I6: one history entry with the web card's facts — generation time, the
/// analysed window, the session/message counts, hours and days covered,
/// goals/languages tags, and the model plus generation tokens when the report
/// recorded them. Clicking loads the report.
fn render_history_row(
    meta: &InsightsReportMeta,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let path = meta.path.clone();
    let load_tx = tx.clone();
    let tags = history_tags(meta);
    let mut facts = vec![
        crate::i18n::insights_sessions_ratio(
            meta.analyzed_sessions as usize,
            meta.total_sessions as usize,
            cx,
        ),
        crate::i18n::insights_messages_count(meta.total_messages as usize, cx),
        crate::i18n::insights_hours(format!("{:.1}", meta.total_hours), cx),
        crate::i18n::insights_days_count(meta.days_covered as usize, cx),
    ];
    if meta.generation_usage.total_tokens > 0 {
        facts.push(crate::i18n::insights_generation_tokens(
            meta.generation_usage.total_tokens,
            cx,
        ));
    }
    if !meta.generation_models.is_empty() {
        facts.push(meta.generation_models.join(" + "));
    }

    let mut details = div().flex().flex_row().flex_wrap().items_center().gap_2();
    for fact in facts {
        details = details.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(fact),
        );
    }

    let mut row = div()
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
        .v_flex()
        .gap_1()
        .p_2()
        .rounded(cx.theme().radius)
        .debug_selector(|| "insights-history-row".to_string())
        .hover(|style| style.bg(cx.theme().sidebar_accent))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(history_timestamp(meta.generated_at)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(history_range(&meta.date_range.start, &meta.date_range.end)),
                ),
        )
        .child(details);
    if !tags.is_empty() {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(tags.join(" · ")),
        );
    }
    row.into_any_element()
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

    if let Some(suggestions) = render_suggestions(&report.suggestions, text, cx) {
        report_view = report_view.child(suggestions);
    }

    report_view
}

/// G-I5: the report's three suggestion lists with the web copy affordances.
/// Each copy button puts exactly what the web copies on the clipboard — the
/// OPENBITFUN.md addition's content, a feature's example usage, a usage
/// pattern's suggested prompt — and a section disappears when its list is
/// empty, the same conditional the web section has.
fn render_suggestions(
    suggestions: &InsightsSuggestions,
    text: &ShellText,
    cx: &App,
) -> Option<gpui_kit::Div> {
    let mut block = div().v_flex().gap_3();
    let mut rendered = false;

    if !suggestions.openbitfun_md_additions.is_empty() {
        rendered = true;
        let mut section = div().v_flex().gap_2();
        section = section.child(section_title(text.insights_md_additions.clone(), cx));
        for addition in &suggestions.openbitfun_md_additions {
            section = section.child(suggestion_row(
                addition.section.clone(),
                addition.rationale.clone(),
                addition.content.clone(),
                text.insights_copy.clone(),
                cx,
            ));
        }
        block = block.child(section);
    }

    if !suggestions.features_to_try.is_empty() {
        rendered = true;
        let mut section = div().v_flex().gap_2();
        section = section.child(section_title(text.insights_features_to_try.clone(), cx));
        for feature in &suggestions.features_to_try {
            // The web only offers a copy action when the feature carries an
            // example, so an empty one renders without a button.
            let copy =
                (!feature.example_usage.trim().is_empty()).then(|| feature.example_usage.clone());
            section = section.child(suggestion_row_maybe(
                feature.feature.clone(),
                feature.description.clone(),
                copy,
                text.insights_copy.clone(),
                cx,
            ));
        }
        block = block.child(section);
    }

    if !suggestions.usage_patterns.is_empty() {
        rendered = true;
        let mut section = div().v_flex().gap_2();
        section = section.child(section_title(text.insights_usage_patterns.clone(), cx));
        for pattern in &suggestions.usage_patterns {
            let copy = (!pattern.suggested_prompt.trim().is_empty())
                .then(|| pattern.suggested_prompt.clone());
            section = section.child(suggestion_row_maybe(
                pattern.pattern.clone(),
                pattern.description.clone(),
                copy,
                text.insights_try_this_prompt.clone(),
                cx,
            ));
        }
        block = block.child(section);
    }

    rendered.then_some(block)
}

fn section_title(label: String, cx: &App) -> gpui_kit::Div {
    div()
        .text_sm()
        .text_color(cx.theme().foreground)
        .child(label)
}

fn suggestion_row(
    label: String,
    detail: String,
    copy_text: String,
    copy_label: String,
    cx: &App,
) -> gpui_kit::Div {
    suggestion_row_maybe(label, detail, Some(copy_text), copy_label, cx)
}

fn suggestion_row_maybe(
    label: String,
    detail: String,
    copy_text: Option<String>,
    copy_label: String,
    cx: &App,
) -> gpui_kit::Div {
    let mut row = div().v_flex().gap_1().p_2().rounded(cx.theme().radius);
    let mut head = div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap_2();
    head = head.child(
        div()
            .flex_1()
            .min_w_0()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(label),
    );
    if let Some(copy_text) = copy_text {
        head = head.child(
            Button::new(SharedString::from(format!(
                "insights-copy-{}",
                markdown_id_key(&copy_text)
            )))
            .label(copy_label)
            .small()
            .with_variant(ButtonVariant::Ghost)
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
            }),
        );
    }
    row = row.child(head);
    if !detail.trim().is_empty() {
        row = row.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .min_w_0()
                .whitespace_normal()
                .line_clamp(3)
                .child(detail),
        );
    }
    row
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
        assert_eq!(INSIGHTS_RANGES, [7, 14, 30, 90]);
        assert_eq!(INSIGHTS_DEFAULT_DAYS, 30);
        assert!(INSIGHTS_RANGES.contains(&INSIGHTS_DEFAULT_DAYS));
    }

    /// G-I1: the model menu leads with the primary selector and lists enabled
    /// models after it; a vanished selection falls back to the lead entry.
    #[test]
    fn model_options_lead_with_primary_and_skip_disabled_models() {
        use crate::runtime::ModelChoice;
        let text = crate::i18n::fallback_text(openbitfun_core::service::i18n::LocaleId::EnUS);
        let catalog = vec![
            ModelChoice {
                id: "claude-sonnet".to_string(),
                name: "Sonnet".to_string(),
                provider: "anthropic".to_string(),
                enabled: true,
            },
            ModelChoice {
                id: "retired".to_string(),
                name: "Retired".to_string(),
                provider: "local".to_string(),
                enabled: false,
            },
        ];
        let options = model_options(&catalog, &text);
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].0, INSIGHTS_DEFAULT_MODEL);
        assert_eq!(options[0].1, text.insights_model_primary);
        assert_eq!(options[1].0, "claude-sonnet");
        assert!(options[1].1.contains("Sonnet"));
        assert_eq!(
            selected_model_label(&options, "claude-sonnet"),
            options[1].1
        );
        assert_eq!(selected_model_label(&options, "gone"), options[0].1);
    }

    /// G-I6: the history card trims the stored ISO instants to their date and
    /// renders the generation instant in local time.
    #[test]
    fn history_meta_formats_the_web_card_fields() {
        assert_eq!(
            history_range("2026-09-15T00:00:00+08:00", "2026-10-15T23:59:59+08:00"),
            "2026-09-15 → 2026-10-15"
        );
        assert_eq!(history_range("n/a", "n/a"), "n/a → n/a");
        // The report persists epoch seconds; the same instant goes through the
        // local calendar, so the assertion holds wherever the suite runs.
        let stamp = chrono::Local
            .with_ymd_and_hms(2026, 10, 15, 9, 30, 0)
            .single()
            .expect("valid local time")
            .timestamp() as u64;
        assert_eq!(history_timestamp(stamp), "2026-10-15 09:30");
    }

    /// G-I6: the tag line keeps the web order (goals then languages) and does
    /// not repeat a tag that is both.
    #[test]
    fn history_tags_deduplicate_goals_then_languages() {
        use openbitfun_core::agentic::insights::DateRange;
        let meta = InsightsReportMeta {
            generated_at: 0,
            total_sessions: 0,
            analyzed_sessions: 0,
            date_range: DateRange {
                start: String::new(),
                end: String::new(),
            },
            path: String::new(),
            total_messages: 0,
            days_covered: 0,
            total_hours: 0.0,
            top_goals: vec!["Rust".to_string(), "  ".to_string(), "docs".to_string()],
            languages: vec!["docs".to_string(), "TypeScript".to_string()],
            session_usage: Default::default(),
            generation_usage: Default::default(),
            generation_models: Vec::new(),
        };
        assert_eq!(history_tags(&meta), vec!["Rust", "docs", "TypeScript"]);
    }
}
