//! Shell scheduled-jobs page: the management slice of the shared cron service
//! (list, enable/disable, run now, delete). Job creation stays with the agent
//! tool and the desktop for now; the page always shows the full shared store,
//! labelled with each job's target, so jobs from other surfaces are visible
//! instead of filtered away.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt as _};
use gpui_kit::{div, prelude::*, App, SharedString};
use openbitfun_core::service::cron::CronSchedule;
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};

/// State of the scheduled-jobs page; lives on the shell window and is updated
/// only through [`RuntimeEvent`] projections.
#[derive(Debug, Default)]
pub(crate) struct TasksPageState {
    pub jobs: Vec<openbitfun_core::service::cron::CronJob>,
    pub error: Option<String>,
    /// The job list was fetched at least once (drives reload-on-open).
    pub loaded: bool,
}

/// The full page: a refresh header plus one action row per job.
pub(crate) fn render_page(
    state: &TasksPageState,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let reload_tx = tx.clone();
    let mut column = div()
        .w_full()
        .h_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_lg()
                        .text_color(cx.theme().foreground)
                        .child(text.tasks_title.clone()),
                )
                .child(
                    Button::new("tasks-reload")
                        .icon(gpui_kit::assets::IconName::RefreshCw)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.sessions_reload.clone())
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.reload_cron_jobs(reload_tx.clone());
                            }
                        }),
                ),
        );

    if let Some(error) = &state.error {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(error.clone()),
        );
    }

    if state.jobs.is_empty() {
        if state.loaded {
            column = column.child(crate::shell::brand_empty(
                gpui_kit::assets::IconName::Calendar,
                &text.tasks_empty_title,
                &text.tasks_empty,
            ));
        } else {
            column = column.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text.insights_loading.clone()),
            );
        }
    }

    for job in &state.jobs {
        let row = render_job_row(job, tx, text, cx);
        column = column.child(row);
    }

    column.into_any_element()
}

/// One job row: name and schedule on the left, target plus actions on the
/// right. Disabled jobs stay visible at reduced opacity so the toggle is
/// discoverable.
fn render_job_row(
    job: &openbitfun_core::service::cron::CronJob,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let enabled_tx = tx.clone();
    let job_id_enabled = job.id.clone();
    let enabled = job.enabled;
    let run_tx = tx.clone();
    let job_id_run = job.id.clone();
    let delete_tx = tx.clone();
    let job_id_delete = job.id.clone();

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .p_2()
        .rounded(cx.theme().radius)
        .when(!job.enabled, |this| this.opacity(0.6))
        .child(
            div()
                .v_flex()
                .gap_0p5()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .truncate()
                        .child(job.name.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(schedule_label(&job.schedule)),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{:?}", job.target_kind())),
                )
                .child(
                    Button::new(SharedString::from(format!("task-toggle-{}", job.id)))
                        .label(if enabled {
                            text.tasks_disable.clone()
                        } else {
                            text.tasks_enable.clone()
                        })
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.set_cron_job_enabled(
                                    job_id_enabled.clone(),
                                    !enabled,
                                    enabled_tx.clone(),
                                );
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("task-run-{}", job.id)))
                        .label(text.tasks_run_now.clone())
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.run_cron_job_now(job_id_run.clone(), run_tx.clone());
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("task-delete-{}", job.id)))
                        .icon(gpui_kit::assets::IconName::X)
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .tooltip(text.session_delete.clone())
                        .on_click(move |_, _, _| {
                            if let Some(handle) = runtime::runtime_handle() {
                                handle.delete_cron_job(job_id_delete.clone(), delete_tx.clone());
                            }
                        }),
                ),
        )
}

/// Technical schedule notation; deliberately not localized because it mirrors
/// the wire values users set through the agent tool.
fn schedule_label(schedule: &CronSchedule) -> String {
    match schedule {
        CronSchedule::At { at } => format!("at {at}"),
        CronSchedule::Every { every_ms, .. } => format!("every {}", format_every(*every_ms)),
        CronSchedule::Cron { expr, tz } => match tz {
            Some(tz) => format!("cron {expr} ({tz})"),
            None => format!("cron {expr}"),
        },
    }
}

/// Milliseconds to a compact human interval (`90s`, `5m`, `2h`, `3d`).
fn format_every(every_ms: u64) -> String {
    const SECOND: u64 = 1_000;
    const MINUTE: u64 = 60 * SECOND;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    if every_ms >= DAY && every_ms % DAY == 0 {
        format!("{}d", every_ms / DAY)
    } else if every_ms >= HOUR && every_ms % HOUR == 0 {
        format!("{}h", every_ms / HOUR)
    } else if every_ms >= MINUTE && every_ms % MINUTE == 0 {
        format!("{}m", every_ms / MINUTE)
    } else {
        format!("{}s", every_ms / SECOND)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_labels_mirror_wire_values() {
        assert_eq!(
            schedule_label(&CronSchedule::At {
                at: "2026-10-12T09:00:00+08:00".to_string()
            }),
            "at 2026-10-12T09:00:00+08:00"
        );
        assert_eq!(
            schedule_label(&CronSchedule::Every {
                every_ms: 5 * 60 * 1_000,
                anchor_ms: None
            }),
            "every 5m"
        );
        assert_eq!(
            schedule_label(&CronSchedule::Cron {
                expr: "0 9 * * 1-5".to_string(),
                tz: Some("Asia/Shanghai".to_string())
            }),
            "cron 0 9 * * 1-5 (Asia/Shanghai)"
        );
    }

    #[test]
    fn every_intervals_compact_to_the_largest_whole_unit() {
        assert_eq!(format_every(90 * 1_000), "90s");
        assert_eq!(format_every(5 * 60 * 1_000), "5m");
        assert_eq!(format_every(2 * 60 * 60 * 1_000), "2h");
        assert_eq!(format_every(3 * 24 * 60 * 60 * 1_000), "3d");
    }
}
