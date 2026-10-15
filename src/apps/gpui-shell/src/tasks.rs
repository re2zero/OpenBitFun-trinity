//! Shell task board (R4): the management slice of the shared cron service
//! laid out as an overview card, a month calendar, and the job list (enable /
//! disable / run now / delete). Job creation stays with the agent tool and the
//! desktop for now; the board always shows the full shared store, labelled
//! with each job's target, so jobs from other surfaces are visible instead of
//! filtered away. The calendar only marks instants the store actually carries
//! (next scheduled run, latest finished run) — the service keeps no run
//! history, so past days stay blank on purpose.

use chrono::TimeZone as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt as _};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{div, prelude::*, px, App, SharedString};
use openbitfun_core::service::cron::{CronJob, CronSchedule};
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};

/// State of the task board; lives on the shell window and is updated only
/// through [`RuntimeEvent`] projections.
#[derive(Debug, Default)]
pub(crate) struct TasksPageState {
    pub jobs: Vec<CronJob>,
    pub error: Option<String>,
    /// The job list was fetched at least once (drives reload-on-open).
    pub loaded: bool,
    /// G-P2: the calendar day whose detail panel is open (web `selectedDayKey`).
    pub selected_day: Option<chrono::NaiveDate>,
}

/// Web `TodoItemRow` badges: the row flags this store can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobBadge {
    Running,
    NextRun,
    Overdue,
}

/// Web `inactive` reasons: why a job has no upcoming run. Precedence follows
/// the web row — a disabled job is paused first, then a finished one-shot is
/// done, and anything else the store cannot turn into a run is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InactiveReason {
    Paused,
    Done,
    InvalidSchedule,
}

pub(crate) fn job_running(job: &CronJob) -> bool {
    matches!(
        job.state.last_run_status,
        Some(openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus::Running)
    )
}

pub(crate) fn job_overdue(job: &CronJob, now_ms: i64) -> bool {
    job.enabled && job.state.next_run_at_ms.is_some_and(|at| at < now_ms)
}

/// The id of the job whose next run is the nearest upcoming one (web
/// `isNextRun`). Overdue instants do not qualify, and ties keep the first job.
pub(crate) fn next_run_job_id(jobs: &[CronJob], now_ms: i64) -> Option<String> {
    jobs.iter()
        .filter(|job| job.enabled)
        .filter_map(|job| {
            job.state
                .next_run_at_ms
                .filter(|at| *at >= now_ms)
                .map(|at| (at, job.id.clone()))
        })
        .min_by_key(|(at, _)| *at)
        .map(|(_, id)| id)
}

/// The inactive partition's reason for one job, or `None` when the job has an
/// upcoming run. The store cannot report a broken schedule expression
/// directly, so "invalid" is the honest reading of an enabled job whose next
/// run the service never computed.
pub(crate) fn inactive_reason(job: &CronJob) -> Option<InactiveReason> {
    if !job.enabled {
        return Some(InactiveReason::Paused);
    }
    if job.state.next_run_at_ms.is_some() {
        return None;
    }
    match (&job.schedule, job.state.last_run_status) {
        (CronSchedule::At { .. }, Some(_)) => Some(InactiveReason::Done),
        _ => Some(InactiveReason::InvalidSchedule),
    }
}

pub(crate) fn job_badges(job: &CronJob, next_id: Option<&str>, now_ms: i64) -> Vec<JobBadge> {
    let mut badges = Vec::new();
    if job_running(job) {
        badges.push(JobBadge::Running);
    }
    if next_id == Some(job.id.as_str()) {
        badges.push(JobBadge::NextRun);
    }
    if job_overdue(job, now_ms) {
        badges.push(JobBadge::Overdue);
    }
    badges
}

/// One occurrence the store carries on a calendar day (G-P2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DayEntry {
    pub job_name: String,
    pub at_ms: i64,
    pub kind: DayEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DayEntryKind {
    NextRun,
    RanOk,
    RanError,
}

/// The occurrences the store carries for one local day: each job's next
/// scheduled instant plus its latest finished run, in time order. A day can
/// never show more than those per job — the same limit the calendar's legend
/// states, because the service keeps no run history.
pub(crate) fn day_entries(jobs: &[CronJob], day: chrono::NaiveDate) -> Vec<DayEntry> {
    let mut entries = Vec::new();
    let local = |at: i64| chrono::Local.timestamp_millis_opt(at).single();
    for job in jobs {
        if job.enabled {
            if let Some(at) = job.state.next_run_at_ms {
                if local(at).is_some_and(|at| at.date_naive() == day) {
                    entries.push(DayEntry {
                        job_name: job.name.clone(),
                        at_ms: at,
                        kind: DayEntryKind::NextRun,
                    });
                }
            }
        }
        if let Some(at) = job.state.last_run_finished_at_ms {
            if local(at).is_some_and(|at| at.date_naive() == day) {
                entries.push(DayEntry {
                    job_name: job.name.clone(),
                    at_ms: at,
                    kind: if ran_ok(job) {
                        DayEntryKind::RanOk
                    } else {
                        DayEntryKind::RanError
                    },
                });
            }
        }
    }
    entries.sort_by_key(|entry| entry.at_ms);
    entries
}

/// Whether the job's latest finished run ended without an error (a running or
/// queued status is not a failure; anything else, including no status at all,
/// reads as the error the calendar marks).
fn ran_ok(job: &CronJob) -> bool {
    use openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus;
    matches!(
        job.state.last_run_status,
        Some(ScheduledJobRunStatus::Ok)
            | Some(ScheduledJobRunStatus::Running)
            | Some(ScheduledJobRunStatus::Queued)
    )
}

/// The full board: refresh header, overview card, month calendar, then the
/// job list.
pub(crate) fn render_page(
    state: &TasksPageState,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
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

    if !state.loaded {
        column = column.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.insights_loading.clone()),
        );
        return column.into_any_element();
    }

    // The overview card and calendar render even with no jobs ("0 pending"
    // is a fact worth showing); the empty state only replaces the list.
    column = column.child(render_overview(&state.jobs, text, cx));
    column = column.child(render_month_calendar(state, &view, text, cx));
    if let Some(day) = state.selected_day {
        column = column.child(render_day_detail(state, day, &view, text, cx));
    }

    if state.jobs.is_empty() {
        column = column.child(crate::shell::brand_empty(
            gpui_kit::assets::IconName::Calendar,
            &text.tasks_empty_title,
            &text.tasks_empty,
        ));
        return column.into_any_element();
    }

    column = column.child(render_job_list(state, tx, text, cx));

    column.into_any_element()
}

/// Headline counts of the overview card. Pure so the window state stays
/// projectable in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct OverviewStats {
    pub total: usize,
    pub enabled: usize,
    /// Enabled jobs scheduled inside the next 24 hours (`next_run_at_ms`).
    pub due_24h: usize,
    /// Jobs whose latest finished run ended in an error (or was cancelled).
    pub failed: usize,
}

pub(crate) fn overview_stats(
    jobs: &[openbitfun_core::service::cron::CronJob],
    now_ms: i64,
) -> OverviewStats {
    use openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus;
    const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
    let mut stats = OverviewStats {
        total: jobs.len(),
        ..Default::default()
    };
    for job in jobs {
        if !job.enabled {
            continue;
        }
        stats.enabled += 1;
        if job
            .state
            .next_run_at_ms
            .is_some_and(|at| at >= now_ms && at <= now_ms + DAY_MS)
        {
            stats.due_24h += 1;
        }
        if matches!(
            job.state.last_run_status,
            Some(ScheduledJobRunStatus::Error) | Some(ScheduledJobRunStatus::Cancelled)
        ) {
            stats.failed += 1;
        }
    }
    stats
}

/// The overview card: four at-a-glance numbers, the insights headline shape.
fn render_overview(
    jobs: &[openbitfun_core::service::cron::CronJob],
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let stats = overview_stats(jobs, chrono::Utc::now().timestamp_millis());
    let failed_color = if stats.failed > 0 {
        cx.theme().danger
    } else {
        cx.theme().muted_foreground
    };
    let cells = [
        (
            text.tasks_stat_total.clone(),
            stats.total.to_string(),
            cx.theme().foreground,
        ),
        (
            text.tasks_stat_enabled.clone(),
            stats.enabled.to_string(),
            cx.theme().foreground,
        ),
        (
            text.tasks_stat_due_24h.clone(),
            stats.due_24h.to_string(),
            cx.theme().foreground,
        ),
        (
            text.tasks_stat_failed.clone(),
            stats.failed.to_string(),
            failed_color,
        ),
    ];
    let mut row = div()
        .flex()
        .flex_wrap()
        .gap_4()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(text.tasks_overview.clone()),
        );
    for (label, value, color) in cells {
        row = row.child(
            div()
                .v_flex()
                .gap_0p5()
                .min_w(px(72.))
                .child(div().text_lg().text_color(color).child(value))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label),
                ),
        );
    }
    row
}

/// One day cell of the month calendar. Marks are booleans rather than counts:
/// the store has no per-day history, only the next scheduled instant and the
/// latest finished run per job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonthCell {
    /// The full local date the cell stands for (G-P2 keys the day panel on it).
    pub date: chrono::NaiveDate,
    pub day: u32,
    pub in_month: bool,
    pub is_today: bool,
    pub scheduled: bool,
    pub ran_ok: bool,
    pub ran_error: bool,
}

/// Monday-first month grid, 7 columns; leading/trailing days of adjacent
/// months pad the first and last rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonthGrid {
    pub cells: Vec<MonthCell>,
    pub week_count: usize,
}

pub(crate) fn build_month_grid(
    jobs: &[openbitfun_core::service::cron::CronJob],
    today: chrono::DateTime<chrono::Local>,
) -> MonthGrid {
    use chrono::{Datelike, Duration};
    let today_date = today.date_naive();
    let first = today_date.with_day(1).expect("first day exists");
    let month_end = if first.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .and_then(|next| next.pred_opt())
    .expect("month end exists");

    let mut scheduled: std::collections::HashSet<chrono::NaiveDate> = Default::default();
    let mut ran_ok_days: std::collections::HashSet<chrono::NaiveDate> = Default::default();
    let mut ran_error_days: std::collections::HashSet<chrono::NaiveDate> = Default::default();
    for job in jobs {
        if job.enabled {
            if let Some(at) = job.state.next_run_at_ms {
                if let Some(local) = chrono::Local.timestamp_millis_opt(at).single() {
                    scheduled.insert(local.date_naive());
                }
            }
        }
        if let Some(at) = job.state.last_run_finished_at_ms {
            let local = chrono::Local.timestamp_millis_opt(at).single();
            if let Some(local) = local {
                if ran_ok(job) {
                    ran_ok_days.insert(local.date_naive());
                } else {
                    ran_error_days.insert(local.date_naive());
                }
            }
        }
    }

    let lead = first.weekday().num_days_from_monday() as i64;
    let day_count = (month_end - first).num_days() + 1;
    let week_count = ((day_count + lead) as usize).div_ceil(7);
    let grid_start = first - Duration::days(lead);
    let mut cells = Vec::with_capacity(week_count * 7);
    for index in 0..week_count * 7 {
        let day = grid_start + Duration::days(index as i64);
        cells.push(MonthCell {
            date: day,
            day: day.day(),
            in_month: day >= first && day <= month_end,
            is_today: day == today_date,
            scheduled: scheduled.contains(&day),
            ran_ok: ran_ok_days.contains(&day) && !ran_error_days.contains(&day),
            ran_error: ran_error_days.contains(&day),
        });
    }
    MonthGrid { cells, week_count }
}

/// The month calendar card: weekday header, the grid, and a legend that also
/// states the data honestly (no run history in the shell). Clicking a day
/// toggles the day panel below it (web `onSelectDay` toggles the same way).
fn render_month_calendar(
    state: &TasksPageState,
    view: &gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    const CELL: f32 = 30.;
    const GAP: f32 = 3.;
    let grid = build_month_grid(&state.jobs, chrono::Local::now());
    let muted = cx.theme().muted_foreground;
    let label = |value: String| div().text_xs().text_color(muted).child(value);

    let weekdays = [
        text.tasks_wd_mon.clone(),
        text.tasks_wd_tue.clone(),
        text.tasks_wd_wed.clone(),
        text.tasks_wd_thu.clone(),
        text.tasks_wd_fri.clone(),
        text.tasks_wd_sat.clone(),
        text.tasks_wd_sun.clone(),
    ];
    let mut header = div().flex().gap(px(GAP));
    for name in weekdays {
        header = header.child(div().w(px(CELL)).flex().justify_center().child(label(name)));
    }

    let mut grid_element = div().v_flex().gap(px(GAP)).min_w_0();
    for week in 0..grid.week_count {
        let mut row = div().flex().gap(px(GAP));
        for col in 0..7usize {
            let cell = &grid.cells[week * 7 + col];
            let text_color = if !cell.in_month {
                cx.theme().muted_foreground.opacity(0.5)
            } else if cell.is_today {
                cx.theme().info
            } else {
                cx.theme().foreground
            };
            let mut day = div()
                .w(px(CELL))
                .h(px(CELL))
                .v_flex()
                .items_center()
                .justify_center()
                .gap_0p5()
                .rounded(cx.theme().radius)
                .child(
                    div()
                        .text_xs()
                        .text_color(text_color)
                        .child(cell.day.to_string()),
                );
            if cell.scheduled || cell.ran_ok || cell.ran_error {
                let mut dots = div().flex().gap(px(2.));
                if cell.scheduled {
                    dots = dots.child(dot(cx.theme().info));
                }
                if cell.ran_ok {
                    dots = dots.child(dot(cx.theme().success));
                }
                if cell.ran_error {
                    dots = dots.child(dot(cx.theme().danger));
                }
                day = day.child(dots);
            }
            if cell.is_today {
                day = day.border_1().border_color(cx.theme().info);
            }
            if state.selected_day == Some(cell.date) {
                day = day.bg(cx.theme().sidebar_accent);
            }
            let toggle_view = view.clone();
            let day_date = cell.date;
            let selector = format!("tasks-day-{day_date}");
            row = row.child(
                day.id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .cursor_pointer()
                    .on_click(move |_, _window, cx| {
                        let Some(shell) = toggle_view.upgrade() else {
                            return;
                        };
                        shell.update(cx, |shell, cx| {
                            shell.tasks.selected_day = if shell.tasks.selected_day == Some(day_date)
                            {
                                None
                            } else {
                                Some(day_date)
                            };
                            cx.notify();
                        });
                    }),
            );
        }
        grid_element = grid_element.child(row);
    }

    let mut legend = div().flex().flex_wrap().items_center().gap_2();
    legend = legend
        .child(legend_item(
            cx.theme().info,
            text.tasks_calendar_scheduled.clone(),
            cx,
        ))
        .child(legend_item(
            cx.theme().success,
            text.tasks_calendar_ran.clone(),
            cx,
        ))
        .child(legend_item(
            cx.theme().danger,
            text.tasks_calendar_error.clone(),
            cx,
        ));

    div()
        .v_flex()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(chrono::Local::now().format("%Y-%m").to_string()),
        )
        .child(header)
        .child(grid_element)
        .child(legend)
        .child(label(text.tasks_calendar_note.clone()))
}

/// G-P2: the selected day's panel — the title with the picked date, the web
/// panel's Clear action, and one row per instant the store carries that day
/// (or its empty line when the day is blank).
fn render_day_detail(
    state: &TasksPageState,
    day: chrono::NaiveDate,
    view: &gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let clear_view = view.clone();
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(crate::i18n::tasks_day_title(&day.to_string(), cx)),
        )
        .child(
            Button::new("tasks-day-clear")
                .label(text.tasks_day_clear.clone())
                .small()
                .with_variant(ButtonVariant::Ghost)
                .on_click(move |_, _window, cx| {
                    let Some(shell) = clear_view.upgrade() else {
                        return;
                    };
                    shell.update(cx, |shell, cx| {
                        shell.tasks.selected_day = None;
                        cx.notify();
                    });
                }),
        );

    let entries = day_entries(&state.jobs, day);
    let mut card = div()
        .v_flex()
        .gap_2()
        .p_3()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .debug_selector(|| "tasks-day-detail".to_string())
        .child(header);
    if entries.is_empty() {
        card = card.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.tasks_day_empty.clone()),
        );
    } else {
        for entry in entries {
            let at = chrono::Local
                .timestamp_millis_opt(entry.at_ms)
                .single()
                .map(|at| at.format("%H:%M").to_string())
                .unwrap_or_else(|| entry.at_ms.to_string());
            let (chip_label, chip_color) = match entry.kind {
                DayEntryKind::NextRun => (text.tasks_badge_next_run.clone(), cx.theme().info),
                DayEntryKind::RanOk => (text.tasks_calendar_ran.clone(), cx.theme().success),
                DayEntryKind::RanError => (text.tasks_calendar_error.clone(), cx.theme().danger),
            };
            card = card.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(at),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(entry.job_name),
                    )
                    .child(status_badge(chip_label, chip_color, cx)),
            );
        }
    }
    card
}

/// A compact status chip: tinted text on the tint's weak surface, the shell's
/// reading of the web row's badge classes.
fn status_badge(label: String, tone: gpui_kit::Hsla, cx: &App) -> gpui_kit::Div {
    div()
        .flex_shrink_0()
        .px_1p5()
        .py_0p5()
        .rounded(cx.theme().radius)
        .bg(tone.opacity(0.12))
        .text_xs()
        .text_color(tone)
        .child(label)
}

fn dot(color: gpui_kit::Hsla) -> gpui_kit::Div {
    div().w(px(4.)).h(px(4.)).rounded_full().bg(color)
}

fn legend_item(color: gpui_kit::Hsla, label: String, cx: &App) -> gpui_kit::Div {
    div().flex().items_center().gap_1().child(dot(color)).child(
        div()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(label),
    )
}

/// The list section: every job under its partition. Jobs with an upcoming run
/// come first (their row carrying the web badges), then the inactive
/// partition under the web's own subheader, so a job with no next run is
/// visible as such instead of silently missing.
fn render_job_list(
    state: &TasksPageState,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let next_id = next_run_job_id(&state.jobs, now_ms);
    let mut active = div().v_flex().gap_2().child(
        div()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.tasks_list_title.clone()),
    );
    let mut inactive = Vec::new();

    for job in &state.jobs {
        match inactive_reason(job) {
            None => {
                let badges = job_badges(job, next_id.as_deref(), now_ms);
                active = active.child(render_job_row(job, &badges, None, tx, text, cx));
            }
            Some(reason) => inactive.push(render_job_row(job, &[], Some(reason), tx, text, cx)),
        }
    }

    if !inactive.is_empty() {
        active = active.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(crate::i18n::tasks_inactive_title(inactive.len(), cx)),
        );
        for row in inactive {
            active = active.child(row);
        }
    }
    active
}

/// One job row: name and schedule on the left, target plus actions on the
/// right. Disabled jobs stay visible at reduced opacity so the toggle is
/// discoverable; an inactive job also carries its reason chip in place of the
/// countdown the web row shows.
fn render_job_row(
    job: &CronJob,
    badges: &[JobBadge],
    inactive: Option<InactiveReason>,
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

    let mut name_row = div().flex().flex_row().items_center().gap_2().min_w_0();
    name_row = name_row.child(
        div()
            .text_sm()
            .text_color(cx.theme().foreground)
            .truncate()
            .child(job.name.clone()),
    );
    for badge in badges {
        let (label, tone) = match badge {
            JobBadge::Running => (text.tasks_badge_running.clone(), cx.theme().success),
            JobBadge::NextRun => (text.tasks_badge_next_run.clone(), cx.theme().info),
            JobBadge::Overdue => (text.tasks_badge_overdue.clone(), cx.theme().danger),
        };
        name_row = name_row.child(status_badge(label, tone, cx));
    }
    if let Some(reason) = inactive {
        let label = match reason {
            InactiveReason::Paused => text.tasks_inactive_paused.clone(),
            InactiveReason::Done => text.tasks_inactive_done.clone(),
            InactiveReason::InvalidSchedule => text.tasks_inactive_invalid.clone(),
        };
        name_row = name_row.child(status_badge(label, cx.theme().muted_foreground, cx));
    }

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
            div().v_flex().gap_0p5().min_w_0().child(name_row).child(
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
    use openbitfun_agent_runtime::scheduled_job::ScheduledJobRuntimeState;
    use openbitfun_core::service::cron::{CronJobPayload, CronJobTarget, CronWorkspaceRef};

    fn job(
        name: &str,
        enabled: bool,
        state: ScheduledJobRuntimeState,
    ) -> openbitfun_core::service::cron::CronJob {
        job_with(
            name,
            enabled,
            CronSchedule::Cron {
                expr: "0 9 * * *".to_string(),
                tz: None,
            },
            state,
        )
    }

    fn job_with(
        name: &str,
        enabled: bool,
        schedule: CronSchedule,
        state: ScheduledJobRuntimeState,
    ) -> openbitfun_core::service::cron::CronJob {
        openbitfun_core::service::cron::CronJob {
            id: name.to_string(),
            name: name.to_string(),
            schedule,
            payload: CronJobPayload {
                text: String::new(),
            },
            enabled,
            target: CronJobTarget::Workspace {
                workspace: CronWorkspaceRef {
                    workspace_id: None,
                    workspace_path: String::new(),
                    project_workspace_path: None,
                    execution_target: None,
                    remote_connection_id: None,
                    remote_ssh_host: None,
                },
                launch: Default::default(),
            },
            created_at_ms: 0,
            config_updated_at_ms: 0,
            updated_at_ms: 0,
            state,
        }
    }

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

    #[test]
    fn overview_counts_only_enabled_jobs_and_the_24h_window() {
        let now: i64 = 1_760_000_000_000;
        let due = ScheduledJobRuntimeState {
            next_run_at_ms: Some(now + 3_600_000),
            ..Default::default()
        };
        let later = ScheduledJobRuntimeState {
            next_run_at_ms: Some(now + 25 * 3_600_000),
            ..Default::default()
        };
        let failed = ScheduledJobRuntimeState {
            last_run_status: Some(
                openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus::Error,
            ),
            ..Default::default()
        };
        let jobs = vec![
            job("due", true, due.clone()),
            job("later", true, later),
            job("off", false, due),
            job("broke", true, failed),
        ];
        let stats = overview_stats(&jobs, now);
        assert_eq!(stats.total, 4);
        assert_eq!(stats.enabled, 3);
        assert_eq!(stats.due_24h, 1);
        assert_eq!(stats.failed, 1);
    }

    #[test]
    fn month_grid_covers_the_month_monday_first_and_marks_days() {
        use chrono::Datelike;
        // A Wednesday: 2026-10-07 local, so the first row pads Mon+Tue.
        let today = chrono::Local
            .timestamp_millis_opt(1_760_000_000_000)
            .single()
            .expect("valid local time");
        let due = ScheduledJobRuntimeState {
            next_run_at_ms: Some(today.timestamp_millis()),
            ..Default::default()
        };
        let jobs = vec![job("daily", true, due)];
        let grid = build_month_grid(&jobs, today);
        assert_eq!(grid.cells.len() % 7, 0);
        assert_eq!(grid.cells[0].day, {
            let first = today.date_naive().with_day(1).unwrap();
            let lead = first.weekday().num_days_from_monday() as i64;
            (first - chrono::Duration::days(lead)).day()
        });
        let today_cell = grid
            .cells
            .iter()
            .find(|cell| cell.is_today)
            .expect("today is in the grid");
        assert!(today_cell.in_month);
        assert!(today_cell.scheduled);
        assert!(!today_cell.ran_ok && !today_cell.ran_error);
    }

    /// G-P3: the badges follow the web row — running first, the nearest
    /// upcoming run marked, an overdue instant flagged, and nothing else.
    #[test]
    fn badges_follow_running_next_run_and_overdue() {
        use openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus;
        let now: i64 = 1_760_000_000_000;
        let busy = ScheduledJobRuntimeState {
            last_run_status: Some(ScheduledJobRunStatus::Running),
            next_run_at_ms: Some(now + 3_600_000),
            ..Default::default()
        };
        let later = ScheduledJobRuntimeState {
            next_run_at_ms: Some(now + 7_200_000),
            ..Default::default()
        };
        let overdue = ScheduledJobRuntimeState {
            next_run_at_ms: Some(now - 3_600_000),
            ..Default::default()
        };
        let idle = ScheduledJobRuntimeState {
            next_run_at_ms: None,
            last_run_status: Some(ScheduledJobRunStatus::Ok),
            ..Default::default()
        };
        let jobs = vec![
            job("busy", true, busy),
            job("later", true, later),
            job("late", true, overdue),
            job("idle", false, idle),
        ];
        let next = next_run_job_id(&jobs, now);
        assert_eq!(next.as_deref(), Some("busy"));
        assert_eq!(
            job_badges(&jobs[0], next.as_deref(), now),
            vec![JobBadge::Running, JobBadge::NextRun]
        );
        assert!(job_badges(&jobs[1], next.as_deref(), now).is_empty());
        assert_eq!(
            job_badges(&jobs[2], next.as_deref(), now),
            vec![JobBadge::Overdue]
        );
        // A paused job never carries a badge: it belongs to the partition.
        assert!(job_badges(&jobs[3], next.as_deref(), now).is_empty());
    }

    /// G-P3: the inactive partition reads paused, done and invalid the way
    /// the web row does — disabled first, then a finished one-shot, then the
    /// schedule the store could not turn into a run.
    #[test]
    fn inactive_partition_reads_paused_done_and_invalid() {
        use openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus;
        let now: i64 = 1_760_000_000_000;
        let next = ScheduledJobRuntimeState {
            next_run_at_ms: Some(now + 3_600_000),
            ..Default::default()
        };
        let paused = job("off", false, next.clone());
        assert_eq!(inactive_reason(&paused), Some(InactiveReason::Paused));

        let scheduled = job("daily", true, next);
        assert_eq!(inactive_reason(&scheduled), None);

        let done = job_with(
            "once",
            true,
            CronSchedule::At {
                at: "2026-10-01T09:00:00+08:00".to_string(),
            },
            ScheduledJobRuntimeState {
                next_run_at_ms: None,
                last_run_status: Some(ScheduledJobRunStatus::Ok),
                ..Default::default()
            },
        );
        assert_eq!(inactive_reason(&done), Some(InactiveReason::Done));

        let never_ran = job_with(
            "broken",
            true,
            CronSchedule::Cron {
                expr: "not a cron expression".to_string(),
                tz: None,
            },
            ScheduledJobRuntimeState::default(),
        );
        assert_eq!(
            inactive_reason(&never_ran),
            Some(InactiveReason::InvalidSchedule)
        );
    }

    /// G-P2: the day panel lists exactly the instants the store carries for
    /// that local day — the next scheduled run and the latest finished run —
    /// in time order, and nothing for a day it knows nothing about.
    #[test]
    fn day_entries_cover_next_and_latest_run_in_time_order() {
        use openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus;
        let day = chrono::Local
            .with_ymd_and_hms(2026, 10, 15, 0, 0, 0)
            .single()
            .expect("valid local date")
            .date_naive();
        let at = |hour: u32, minute: u32| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, 15, hour, minute, 0)
                .single()
                .expect("valid local time")
                .timestamp_millis()
        };
        let jobs = vec![
            job(
                "nine",
                true,
                ScheduledJobRuntimeState {
                    next_run_at_ms: Some(at(9, 0)),
                    ..Default::default()
                },
            ),
            job(
                "eight",
                true,
                ScheduledJobRuntimeState {
                    last_run_finished_at_ms: Some(at(8, 0)),
                    last_run_status: Some(ScheduledJobRunStatus::Error),
                    ..Default::default()
                },
            ),
            // Tomorrow's run does not leak into today.
            job(
                "tomorrow",
                true,
                ScheduledJobRuntimeState {
                    next_run_at_ms: Some(at(9, 0) + 24 * 3_600_000),
                    ..Default::default()
                },
            ),
        ];
        let entries = day_entries(&jobs, day);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].job_name, "eight");
        assert_eq!(entries[0].kind, DayEntryKind::RanError);
        assert_eq!(entries[1].job_name, "nine");
        assert_eq!(entries[1].kind, DayEntryKind::NextRun);
        assert!(day_entries(&jobs, day + chrono::Duration::days(2)).is_empty());
    }
}
