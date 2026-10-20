//! Shell task board (R4): the management slice of the shared cron service
//! laid out as an overview card, a month calendar, and the job list (enable /
//! disable / run now / edit / delete). G-P1 adds the create/edit dialog, so
//! the board can schedule a job without leaving the shell; the board always
//! shows the full shared store, labelled with each job's target, so jobs from
//! other surfaces are visible instead of filtered away. The calendar only
//! marks instants the store actually carries (next scheduled run, latest
//! finished run) — the service keeps no run history, so past days stay blank
//! on purpose.

use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone as _};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt as _, WindowExt};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{
    div, prelude::*, px, Anchor, App, ClickEvent, IntoElement, SharedString, WeakEntity, Window,
};
use openbitfun_core::service::cron::{
    CreateCronJobRequest, CronJob, CronJobPayload, CronJobTarget, CronLaunchSpec, CronSchedule,
    CronWorkspaceRef, UpdateCronJobRequest,
};
use tokio::sync::mpsc;

use crate::i18n::ShellText;
use crate::runtime::{self, RuntimeEvent};
use crate::shell::ShellWindow;

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

/// G-P1: which schedule shape the editor is filling in (web `ScheduleKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskScheduleKind {
    At,
    Every,
    Cron,
}

/// G-P1: interval units offered by the editor. The wire format stays
/// milliseconds; the unit only decides how a value is entered and read back,
/// so a daily job reads "every 1 day" rather than "every 1440 minutes".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskIntervalUnit {
    Minute,
    Hour,
    Day,
}

/// G-P1: where a job runs — a fresh session in the current workspace or a
/// bound existing session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskTargetKind {
    Workspace,
    Session,
}

pub(crate) const TASK_MINUTE_MS: u64 = 60_000;
pub(crate) const TASK_HOUR_MS: u64 = 60 * TASK_MINUTE_MS;
pub(crate) const TASK_DAY_MS: u64 = 24 * TASK_HOUR_MS;
/// The web editor caps the prompt at 1000 characters (`TodoEditor.tsx`).
pub(crate) const TASK_PROMPT_MAX_CHARS: usize = 1_000;

impl TaskIntervalUnit {
    pub(crate) fn ms(self) -> u64 {
        match self {
            Self::Minute => TASK_MINUTE_MS,
            Self::Hour => TASK_HOUR_MS,
            Self::Day => TASK_DAY_MS,
        }
    }
}

impl TaskScheduleKind {
    fn from_schedule(schedule: &CronSchedule) -> Self {
        match schedule {
            CronSchedule::At { .. } => Self::At,
            CronSchedule::Every { .. } => Self::Every,
            CronSchedule::Cron { .. } => Self::Cron,
        }
    }
}

impl TaskTargetKind {
    fn from_target(target: &CronJobTarget) -> Self {
        match target {
            CronJobTarget::Session { .. } => Self::Session,
            CronJobTarget::Workspace { .. } => Self::Workspace,
        }
    }
}

/// A local wall-clock value (`YYYY-MM-DDTHH:MM`) to RFC 3339. The editor
/// shows local time (web `LocalizedDateTimeField`); the store wants an
/// absolute instant. A time that does not exist in the local calendar (a DST
/// gap) is rejected rather than silently shifted.
pub(crate) fn local_input_to_rfc3339(value: &str) -> Option<String> {
    let value = value.trim();
    let naive = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"))
        .ok()?;
    let local = Local.from_local_datetime(&naive).earliest()?;
    Some(local.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The inverse used to prefill the editor, truncated to the minute the input
/// accepts. A value the wire could not have written is passed through so the
/// user sees (and can fix) it instead of an empty field.
pub(crate) fn rfc3339_to_local_input(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|at| {
            at.with_timezone(&Local)
                .format("%Y-%m-%dT%H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| at.to_string())
}

fn now_local_input() -> String {
    Local::now().format("%Y-%m-%dT%H:%M").to_string()
}

fn timestamp_ms_to_local_input(at_ms: i64) -> String {
    Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map(|at| at.format("%Y-%m-%dT%H:%M").to_string())
        .unwrap_or_default()
}

fn format_interval_value(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as u64)
    } else {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Milliseconds to the largest unit that divides them evenly, so a value the
/// editor saved reads back the way it was entered. Sub-minute intervals stay
/// fractional minutes (web `splitInterval`).
pub(crate) fn split_interval(every_ms: u64) -> (String, TaskIntervalUnit) {
    if every_ms > 0 && every_ms % TASK_DAY_MS == 0 {
        ((every_ms / TASK_DAY_MS).to_string(), TaskIntervalUnit::Day)
    } else if every_ms > 0 && every_ms % TASK_HOUR_MS == 0 {
        (
            (every_ms / TASK_HOUR_MS).to_string(),
            TaskIntervalUnit::Hour,
        )
    } else {
        (
            format_interval_value(every_ms as f64 / TASK_MINUTE_MS as f64),
            TaskIntervalUnit::Minute,
        )
    }
}

/// The editor's interval value in the selected unit, rounded to whole
/// milliseconds. `None` for anything the user could not have meant.
fn interval_value_to_ms(value: &str, unit: TaskIntervalUnit) -> Option<u64> {
    let value: f64 = value.trim().parse().ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let ms = (value * unit.ms() as f64).round();
    if ms.is_finite() && ms >= 1.0 {
        Some(ms as u64)
    } else {
        None
    }
}

/// G-P1 editor state as plain data, so schedule construction and validation
/// stay testable without a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskDraft {
    pub name: String,
    pub prompt: String,
    pub schedule_kind: TaskScheduleKind,
    /// Local `YYYY-MM-DDTHH:MM` for the `at` schedule.
    pub at: String,
    /// Free-form interval value in `interval_unit`.
    pub every_value: String,
    /// Local `YYYY-MM-DDTHH:MM` the interval counts from; blank means the
    /// creation time (web `anchorMs`).
    pub anchor: String,
    pub interval_unit: TaskIntervalUnit,
    pub cron_expr: String,
    pub timezone: String,
    pub target_kind: TaskTargetKind,
    pub session_id: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskDraftError {
    Name,
    Prompt,
    PromptTooLong,
    At,
    Every,
    Anchor,
    Cron,
    Session,
    Workspace,
}

impl TaskDraftError {
    pub(crate) fn message(self, text: &ShellText) -> String {
        match self {
            Self::Name => text.tasks_editor_error_name.clone(),
            Self::Prompt => text.tasks_editor_error_prompt.clone(),
            Self::PromptTooLong => text.tasks_editor_error_prompt_long.clone(),
            Self::At => text.tasks_editor_error_at.clone(),
            Self::Every => text.tasks_editor_error_every.clone(),
            Self::Anchor => text.tasks_editor_error_anchor.clone(),
            Self::Cron => text.tasks_editor_error_cron.clone(),
            Self::Session => text.tasks_editor_error_session.clone(),
            Self::Workspace => text.tasks_editor_error_workspace.clone(),
        }
    }
}

impl TaskDraft {
    /// The create dialog's starting point (web `createEmptyDraft`): one-shot
    /// at the current local minute, an hourly interval, and the daily cron
    /// example.
    pub(crate) fn blank() -> Self {
        Self {
            name: String::new(),
            prompt: String::new(),
            schedule_kind: TaskScheduleKind::At,
            at: now_local_input(),
            every_value: "1".to_string(),
            anchor: String::new(),
            interval_unit: TaskIntervalUnit::Hour,
            cron_expr: "0 8 * * *".to_string(),
            timezone: String::new(),
            target_kind: TaskTargetKind::Workspace,
            session_id: None,
            enabled: true,
        }
    }

    /// Prefill from an existing job (web `jobToDraft`), preserving the
    /// interval unit the value was entered in.
    pub(crate) fn from_job(job: &CronJob) -> Self {
        let mut draft = Self::blank();
        draft.name = job.name.clone();
        draft.prompt = job.payload.text.clone();
        draft.enabled = job.enabled;
        draft.target_kind = TaskTargetKind::from_target(&job.target);
        draft.session_id = job.session_id().map(str::to_string);
        draft.schedule_kind = TaskScheduleKind::from_schedule(&job.schedule);
        match &job.schedule {
            CronSchedule::At { at } => draft.at = rfc3339_to_local_input(at),
            CronSchedule::Every {
                every_ms,
                anchor_ms,
            } => {
                let (value, unit) = split_interval(*every_ms);
                draft.every_value = value;
                draft.interval_unit = unit;
                draft.anchor = (*anchor_ms)
                    .map(timestamp_ms_to_local_input)
                    .unwrap_or_default();
            }
            CronSchedule::Cron { expr, tz } => {
                draft.cron_expr = expr.clone();
                draft.timezone = tz.clone().unwrap_or_default();
            }
        }
        draft
    }

    pub(crate) fn schedule(&self) -> Result<CronSchedule, TaskDraftError> {
        match self.schedule_kind {
            TaskScheduleKind::At => Ok(CronSchedule::At {
                at: local_input_to_rfc3339(&self.at).ok_or(TaskDraftError::At)?,
            }),
            TaskScheduleKind::Every => {
                let every_ms = interval_value_to_ms(&self.every_value, self.interval_unit)
                    .ok_or(TaskDraftError::Every)?;
                let anchor = self.anchor.trim();
                let anchor_ms = if anchor.is_empty() {
                    None
                } else {
                    Some(
                        local_input_to_rfc3339(anchor)
                            .and_then(|at| DateTime::parse_from_rfc3339(&at).ok())
                            .map(|at| at.timestamp_millis())
                            .ok_or(TaskDraftError::Anchor)?,
                    )
                };
                Ok(CronSchedule::Every {
                    every_ms,
                    anchor_ms,
                })
            }
            TaskScheduleKind::Cron => {
                let expr = self.cron_expr.trim();
                if expr.is_empty() {
                    return Err(TaskDraftError::Cron);
                }
                let tz = self.timezone.trim();
                Ok(CronSchedule::Cron {
                    expr: expr.to_string(),
                    tz: (!tz.is_empty()).then(|| tz.to_string()),
                })
            }
        }
    }

    pub(crate) fn validate(&self) -> Result<(), TaskDraftError> {
        if self.name.trim().is_empty() {
            return Err(TaskDraftError::Name);
        }
        if self.prompt.trim().is_empty() {
            return Err(TaskDraftError::Prompt);
        }
        if self.prompt.chars().count() > TASK_PROMPT_MAX_CHARS {
            return Err(TaskDraftError::PromptTooLong);
        }
        if self.target_kind == TaskTargetKind::Session
            && self
                .session_id
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
        {
            return Err(TaskDraftError::Session);
        }
        self.schedule().map(|_| ())
    }

    pub(crate) fn target(
        &self,
        workspace: CronWorkspaceRef,
    ) -> Result<CronJobTarget, TaskDraftError> {
        match self.target_kind {
            TaskTargetKind::Workspace => Ok(CronJobTarget::Workspace {
                workspace,
                launch: CronLaunchSpec::default(),
            }),
            TaskTargetKind::Session => {
                let session_id = self
                    .session_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .ok_or(TaskDraftError::Session)?;
                Ok(CronJobTarget::Session {
                    session_id: session_id.to_string(),
                    workspace,
                })
            }
        }
    }

    pub(crate) fn create_request(
        &self,
        workspace: CronWorkspaceRef,
    ) -> Result<CreateCronJobRequest, TaskDraftError> {
        self.validate()?;
        Ok(CreateCronJobRequest {
            name: self.name.trim().to_string(),
            schedule: self.schedule()?,
            payload: CronJobPayload {
                text: self.prompt.trim().to_string(),
            },
            enabled: self.enabled,
            target: self.target(workspace)?,
        })
    }

    /// Every field the editor can change, including the interval anchor.
    pub(crate) fn update_request(
        &self,
        workspace: CronWorkspaceRef,
    ) -> Result<UpdateCronJobRequest, TaskDraftError> {
        self.validate()?;
        Ok(UpdateCronJobRequest {
            name: Some(self.name.trim().to_string()),
            schedule: Some(self.schedule()?),
            payload: Some(CronJobPayload {
                text: self.prompt.trim().to_string(),
            }),
            enabled: Some(self.enabled),
            target: Some(self.target(workspace)?),
        })
    }
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
    let new_view = view.clone();
    let new_tx = tx.clone();
    // Natural height: `h_full()` capped the page inside its scroll wrapper at
    // the viewport height, so a long job list had no scroll extent.
    let mut column = div()
        .w_full()
        .flex()
        .flex_col()
        .debug_selector(|| "tasks-page-content".to_string())
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
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("tasks-new")
                                .label(text.tasks_new.clone())
                                .small()
                                .with_variant(ButtonVariant::Primary)
                                .on_click(move |_, window, cx| {
                                    open_task_editor(
                                        new_view.clone(),
                                        None,
                                        new_tx.clone(),
                                        window,
                                        cx,
                                    );
                                }),
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

    column = column.child(render_job_list(state, &view, tx, text, cx));

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
    view: &WeakEntity<ShellWindow>,
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
                active = active.child(render_job_row(job, &badges, None, view, tx, text, cx));
            }
            Some(reason) => {
                inactive.push(render_job_row(job, &[], Some(reason), view, tx, text, cx))
            }
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
    view: &WeakEntity<ShellWindow>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let enabled_tx = tx.clone();
    let job_id_enabled = job.id.clone();
    let enabled = job.enabled;
    let run_tx = tx.clone();
    let job_id_run = job.id.clone();
    let edit_view = view.clone();
    let edit_job = job.clone();
    let edit_tx = tx.clone();
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
                    Button::new(SharedString::from(format!("task-edit-{}", job.id)))
                        .label(crate::i18n::shared_term("tools.edit", cx))
                        .small()
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(move |_, window, cx| {
                            open_task_editor(
                                edit_view.clone(),
                                Some(edit_job.clone()),
                                edit_tx.clone(),
                                window,
                                cx,
                            );
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

fn tasks_editor_field(
    label: &str,
    label_color: gpui_kit::Hsla,
    field: impl IntoElement,
) -> gpui_kit::AnyElement {
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(label_color)
                .child(label.to_string()),
        )
        .child(field)
        .into_any_element()
}

fn schedule_kind_label(kind: TaskScheduleKind, text: &ShellText) -> String {
    match kind {
        TaskScheduleKind::At => text.tasks_editor_kind_at.clone(),
        TaskScheduleKind::Every => text.tasks_editor_kind_every.clone(),
        TaskScheduleKind::Cron => text.tasks_editor_kind_cron.clone(),
    }
}

fn interval_unit_label(unit: TaskIntervalUnit, text: &ShellText) -> String {
    match unit {
        TaskIntervalUnit::Minute => text.tasks_editor_unit_minute.clone(),
        TaskIntervalUnit::Hour => text.tasks_editor_unit_hour.clone(),
        TaskIntervalUnit::Day => text.tasks_editor_unit_day.clone(),
    }
}

fn target_kind_label(kind: TaskTargetKind, text: &ShellText) -> String {
    match kind {
        TaskTargetKind::Workspace => text.tasks_editor_target_workspace.clone(),
        TaskTargetKind::Session => text.tasks_editor_target_session.clone(),
    }
}

/// A dialog choice button that rewrites one shell flag and repaints the
/// window (the dialog builder rebuilds its content from those flags, the same
/// contract the custom-agent kind selector uses).
fn task_choice_button(
    id: &'static str,
    label: String,
    active: bool,
    view: WeakEntity<ShellWindow>,
    select: impl Fn(&mut ShellWindow) + 'static,
) -> gpui_kit::AnyElement {
    Button::new(id)
        .label(label)
        .small()
        .with_variant(if active {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Ghost
        })
        .on_click(move |_, _window, cx| {
            if let Some(shell) = view.upgrade() {
                shell.update(cx, |shell, cx| {
                    select(shell);
                    cx.notify();
                });
            }
        })
        .into_any_element()
}

fn task_schedule_kind_row(
    view: WeakEntity<ShellWindow>,
    active: TaskScheduleKind,
    text: &ShellText,
) -> gpui_kit::AnyElement {
    let mut row = div().flex().flex_row().items_center().gap_2();
    for kind in [
        TaskScheduleKind::At,
        TaskScheduleKind::Every,
        TaskScheduleKind::Cron,
    ] {
        let id = match kind {
            TaskScheduleKind::At => "task-kind-at",
            TaskScheduleKind::Every => "task-kind-every",
            TaskScheduleKind::Cron => "task-kind-cron",
        };
        row = row.child(task_choice_button(
            id,
            schedule_kind_label(kind, text),
            kind == active,
            view.clone(),
            move |shell| shell.task_editor_kind = kind,
        ));
    }
    row.into_any_element()
}

/// Opens the create (or edit) dialog. This mirrors the web `TodoEditor`
/// minimum: name, prompt, the three schedule shapes, and the run target. The
/// inputs live on the window because the dialog builder runs every frame; the
/// selection flags live on the shell for the same reason.
pub(crate) fn open_task_editor(
    view: WeakEntity<ShellWindow>,
    job: Option<CronJob>,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(shell) = view.upgrade() else {
        return;
    };
    let (name_input, prompt_input, at_input, every_input, anchor_input, cron_input, tz_input) = {
        let shell = shell.read(cx);
        (
            shell.task_name_input.clone(),
            shell.task_prompt_input.clone(),
            shell.task_at_input.clone(),
            shell.task_every_input.clone(),
            shell.task_anchor_input.clone(),
            shell.task_cron_input.clone(),
            shell.task_tz_input.clone(),
        )
    };
    let draft = job
        .as_ref()
        .map(TaskDraft::from_job)
        .unwrap_or_else(TaskDraft::blank);
    name_input.update(cx, |input, cx| {
        input.set_value(draft.name.clone(), window, cx);
    });
    prompt_input.update(cx, |input, cx| {
        input.set_value(draft.prompt.clone(), window, cx);
    });
    at_input.update(cx, |input, cx| {
        input.set_value(draft.at.clone(), window, cx);
    });
    every_input.update(cx, |input, cx| {
        input.set_value(draft.every_value.clone(), window, cx);
    });
    anchor_input.update(cx, |input, cx| {
        input.set_value(draft.anchor.clone(), window, cx);
    });
    cron_input.update(cx, |input, cx| {
        input.set_value(draft.cron_expr.clone(), window, cx);
    });
    tz_input.update(cx, |input, cx| {
        input.set_value(draft.timezone.clone(), window, cx);
    });
    shell.update(cx, |shell, cx| {
        shell.task_editor_target = job.as_ref().map(|job| job.id.clone());
        shell.task_editor_kind = draft.schedule_kind;
        shell.task_editor_unit = draft.interval_unit;
        shell.task_editor_target_kind = draft.target_kind;
        shell.task_editor_session = draft.session_id.clone();
        shell.task_editor_enabled = draft.enabled;
        shell.task_editor_error = None;
        cx.notify();
    });

    let confirm_view = view.clone();
    let confirm_tx = tx.clone();
    let confirm_name = name_input.clone();
    let confirm_prompt = prompt_input.clone();
    let confirm_at = at_input.clone();
    let confirm_every = every_input.clone();
    let confirm_anchor = anchor_input.clone();
    let confirm_cron = cron_input.clone();
    let confirm_tz = tz_input.clone();
    let confirm = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
        let Some(shell) = confirm_view.upgrade() else {
            return;
        };
        let (schedule_kind, interval_unit, target_kind, session_id, enabled, edit_id) = {
            let shell = shell.read(cx);
            (
                shell.task_editor_kind,
                shell.task_editor_unit,
                shell.task_editor_target_kind,
                shell.task_editor_session.clone(),
                shell.task_editor_enabled,
                shell.task_editor_target.clone(),
            )
        };
        let draft = TaskDraft {
            name: confirm_name.read(cx).value().trim().to_string(),
            prompt: confirm_prompt.read(cx).value().trim().to_string(),
            schedule_kind,
            at: confirm_at.read(cx).value().to_string(),
            every_value: confirm_every.read(cx).value().to_string(),
            anchor: confirm_anchor.read(cx).value().to_string(),
            interval_unit,
            cron_expr: confirm_cron.read(cx).value().to_string(),
            timezone: confirm_tz.read(cx).value().to_string(),
            target_kind,
            session_id,
            enabled,
        };
        let text = crate::i18n::current(cx);
        let fail = |message: String, cx: &mut App| {
            shell.update(cx, |shell, cx| {
                shell.task_editor_error = Some(message);
                cx.notify();
            });
        };
        if let Err(error) = draft.validate() {
            fail(error.message(&text), cx);
            return;
        }
        let Some(handle) = runtime::runtime_handle() else {
            fail(TaskDraftError::Workspace.message(&text), cx);
            return;
        };
        let workspace = handle.workspace();
        if workspace.root_path.as_os_str().is_empty() {
            fail(TaskDraftError::Workspace.message(&text), cx);
            return;
        }
        let workspace_ref = CronWorkspaceRef {
            workspace_id: Some(workspace.id.clone()),
            workspace_path: workspace.root_path.to_string_lossy().to_string(),
            project_workspace_path: None,
            execution_target: None,
            remote_connection_id: None,
            remote_ssh_host: None,
        };
        let request = match edit_id {
            Some(job_id) => match draft.update_request(workspace_ref) {
                Ok(request) => {
                    handle.update_cron_job(job_id, request, confirm_tx.clone());
                    Ok(())
                }
                Err(error) => Err(error),
            },
            None => match draft.create_request(workspace_ref) {
                Ok(request) => {
                    handle.create_cron_job(request, confirm_tx.clone());
                    Ok(())
                }
                Err(error) => Err(error),
            },
        };
        if let Err(error) = request {
            fail(error.message(&text), cx);
            return;
        }
        shell.update(cx, |shell, cx| {
            shell.task_editor_target = None;
            shell.task_editor_error = None;
            cx.notify();
        });
        window.close_dialog(cx);
    };

    let cancel_view = view.clone();
    let cancel = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(shell) = cancel_view.upgrade() {
            shell.update(cx, |shell, cx| {
                shell.task_editor_target = None;
                shell.task_editor_error = None;
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = crate::i18n::current(cx);
        let Some(shell) = view.upgrade() else {
            return dialog;
        };
        let (
            schedule_kind,
            interval_unit,
            target_kind,
            session_id,
            enabled,
            error,
            editing,
            sessions,
            workspace_name,
        ) = {
            let shell = shell.read(cx);
            (
                shell.task_editor_kind,
                shell.task_editor_unit,
                shell.task_editor_target_kind,
                shell.task_editor_session.clone(),
                shell.task_editor_enabled,
                shell.task_editor_error.clone(),
                shell.task_editor_target.is_some(),
                shell
                    .sessions
                    .iter()
                    .filter(|session| !session.archived)
                    .map(|session| (session.id.clone(), session.name.clone()))
                    .collect::<Vec<_>>(),
                runtime::runtime_handle().map(|handle| handle.workspace().name),
            )
        };
        let label_color = cx.theme().muted_foreground;
        let mut content = div()
            .w(px(520.))
            .v_flex()
            .gap_3()
            .debug_selector(|| "task-editor".to_string())
            .child(tasks_editor_field(
                &text.tasks_editor_name,
                label_color,
                Input::new(&name_input),
            ))
            .child(tasks_editor_field(
                &text.tasks_editor_prompt,
                label_color,
                Textarea::new(&prompt_input),
            ))
            .child(tasks_editor_field(
                &text.tasks_editor_schedule,
                label_color,
                task_schedule_kind_row(view.clone(), schedule_kind, &text),
            ));

        match schedule_kind {
            TaskScheduleKind::At => {
                content = content
                    .child(tasks_editor_field(
                        &text.tasks_editor_at,
                        label_color,
                        Input::new(&at_input),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(label_color)
                            .child(text.tasks_editor_at_hint.clone()),
                    );
            }
            TaskScheduleKind::Every => {
                let unit_view = view.clone();
                let unit_label = interval_unit_label(interval_unit, &text);
                let unit_menu_text = text.clone();
                let unit_button = Button::new("task-unit-picker")
                    .label(unit_label)
                    .small()
                    .with_variant(ButtonVariant::Secondary)
                    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                        let mut menu = menu;
                        for unit in [
                            TaskIntervalUnit::Minute,
                            TaskIntervalUnit::Hour,
                            TaskIntervalUnit::Day,
                        ] {
                            let pick_view = unit_view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(interval_unit_label(unit, &unit_menu_text))
                                    .checked(unit == interval_unit)
                                    .on_click(move |_, _window, cx| {
                                        if let Some(shell) = pick_view.upgrade() {
                                            shell.update(cx, |shell, cx| {
                                                shell.task_editor_unit = unit;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            );
                        }
                        menu
                    });
                content = content
                    .child(tasks_editor_field(
                        &text.tasks_editor_every,
                        label_color,
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(div().flex_1().child(Input::new(&every_input)))
                            .child(unit_button),
                    ))
                    .child(tasks_editor_field(
                        &text.tasks_editor_anchor,
                        label_color,
                        Input::new(&anchor_input),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(label_color)
                            .child(text.tasks_editor_anchor_hint.clone()),
                    );
            }
            TaskScheduleKind::Cron => {
                content = content
                    .child(tasks_editor_field(
                        &text.tasks_editor_cron_expr,
                        label_color,
                        Input::new(&cron_input),
                    ))
                    .child(tasks_editor_field(
                        &text.tasks_editor_cron_tz,
                        label_color,
                        Input::new(&tz_input),
                    ));
            }
        }

        let mut target_row = div().flex().flex_row().items_center().gap_2();
        for kind in [TaskTargetKind::Workspace, TaskTargetKind::Session] {
            let id = match kind {
                TaskTargetKind::Workspace => "task-target-workspace",
                TaskTargetKind::Session => "task-target-session",
            };
            target_row = target_row.child(task_choice_button(
                id,
                target_kind_label(kind, &text),
                kind == target_kind,
                view.clone(),
                move |shell| shell.task_editor_target_kind = kind,
            ));
        }
        content = content.child(tasks_editor_field(
            &text.tasks_editor_target,
            label_color,
            target_row,
        ));
        match target_kind {
            TaskTargetKind::Workspace => {
                let name = workspace_name.unwrap_or_default();
                content = content.child(div().text_xs().text_color(label_color).child(
                    if name.is_empty() {
                        text.tasks_editor_error_workspace.clone()
                    } else {
                        name
                    },
                ));
            }
            TaskTargetKind::Session => {
                let selected_label = session_id
                    .as_ref()
                    .and_then(|id| {
                        sessions
                            .iter()
                            .find(|(session_id, _)| session_id == id)
                            .map(|(_, name)| name.clone())
                    })
                    .unwrap_or_else(|| text.tasks_editor_session_placeholder.clone());
                let picker: gpui_kit::AnyElement = if sessions.is_empty() {
                    Button::new("task-session-picker")
                        .label(selected_label)
                        .small()
                        .with_variant(ButtonVariant::Secondary)
                        .disabled(true)
                        .into_any_element()
                } else {
                    let picker_view = view.clone();
                    let picker_sessions = sessions.clone();
                    Button::new("task-session-picker")
                        .label(selected_label)
                        .small()
                        .with_variant(ButtonVariant::Secondary)
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _window, _cx| {
                            let mut menu = menu;
                            for (id, name) in &picker_sessions {
                                let pick_view = picker_view.clone();
                                let value = id.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(name.clone())
                                        .checked(session_id.as_deref() == Some(id.as_str()))
                                        .on_click(move |_, _window, cx| {
                                            if let Some(shell) = pick_view.upgrade() {
                                                shell.update(cx, |shell, cx| {
                                                    shell.task_editor_session = Some(value.clone());
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                );
                            }
                            menu
                        })
                        .into_any_element()
                };
                content =
                    content
                        .child(div().flex().flex_row().items_center().child(picker))
                        .child(div().text_xs().text_color(label_color).child(
                            if sessions.is_empty() {
                                text.tasks_editor_no_sessions.clone()
                            } else {
                                String::new()
                            },
                        ));
            }
        }

        let enabled_view = view.clone();
        content = content.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(Switch::new("task-enabled").checked(enabled).on_click(
                    move |checked: &bool, _window, cx| {
                        if let Some(shell) = enabled_view.upgrade() {
                            let next = *checked;
                            shell.update(cx, |shell, cx| {
                                shell.task_editor_enabled = next;
                                cx.notify();
                            });
                        }
                    },
                ))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(text.tasks_editor_enabled.clone()),
                ),
        );
        if let Some(error) = error {
            content = content.child(div().text_xs().text_color(cx.theme().danger).child(error));
        }

        let title = if editing {
            text.tasks_editor_edit_title.clone()
        } else {
            text.tasks_editor_create_title.clone()
        };
        dialog.title(div().child(title)).child(content).footer(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("task-editor-cancel")
                        .label(text.session_action_cancel.clone())
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(cancel.clone()),
                )
                .child(
                    Button::new("task-editor-confirm")
                        .label(text.session_action_confirm.clone())
                        .with_variant(ButtonVariant::Primary)
                        .on_click(confirm.clone()),
                ),
        )
    });
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

    fn task_workspace() -> CronWorkspaceRef {
        CronWorkspaceRef {
            workspace_id: Some("ws-1".to_string()),
            workspace_path: "/tmp/ws".to_string(),
            project_workspace_path: None,
            execution_target: None,
            remote_connection_id: None,
            remote_ssh_host: None,
        }
    }

    /// G-P1: the editor's local wall clock is what the store reads back, and
    /// a malformed value is rejected instead of being silently shifted.
    #[test]
    fn local_datetime_round_trips_and_rejects_garbage() {
        let rfc3339 = local_input_to_rfc3339("2030-01-02T03:04").expect("valid local time");
        assert_eq!(rfc3339_to_local_input(&rfc3339), "2030-01-02T03:04");
        // The wire value keeps the local offset; the store parses RFC 3339.
        assert!(chrono::DateTime::parse_from_rfc3339(&rfc3339).is_ok());
        assert_eq!(
            rfc3339_to_local_input(&local_input_to_rfc3339(" 2030-01-02T03:04:05 ").unwrap()),
            "2030-01-02T03:04"
        );
        assert!(local_input_to_rfc3339("").is_none());
        assert!(local_input_to_rfc3339("2030-01-02").is_none());
        assert!(local_input_to_rfc3339("tomorrow").is_none());
    }

    /// G-P1: milliseconds read back in the largest whole unit, and a
    /// sub-minute interval stays fractional minutes.
    #[test]
    fn split_interval_picks_the_largest_whole_unit() {
        assert_eq!(
            split_interval(TASK_DAY_MS),
            ("1".to_string(), TaskIntervalUnit::Day)
        );
        assert_eq!(
            split_interval(3 * TASK_HOUR_MS),
            ("3".to_string(), TaskIntervalUnit::Hour)
        );
        assert_eq!(
            split_interval(90 * TASK_MINUTE_MS),
            ("90".to_string(), TaskIntervalUnit::Minute)
        );
        assert_eq!(
            split_interval(5_400_000),
            ("90".to_string(), TaskIntervalUnit::Minute)
        );
        assert_eq!(
            split_interval(90_000),
            ("1.5".to_string(), TaskIntervalUnit::Minute)
        );
    }

    /// G-P1: each schedule shape builds the wire value the service validates,
    /// including the blank-timezone and fractional-interval cases.
    #[test]
    fn task_draft_builds_each_schedule_shape() {
        let mut draft = TaskDraft::blank();
        draft.schedule_kind = TaskScheduleKind::At;
        draft.at = "2030-01-02T03:04".to_string();
        match draft.schedule().expect("at schedule") {
            CronSchedule::At { at } => {
                assert_eq!(at, local_input_to_rfc3339("2030-01-02T03:04").unwrap())
            }
            other => panic!("expected at, got {other:?}"),
        }

        draft.schedule_kind = TaskScheduleKind::Every;
        draft.every_value = "1.5".to_string();
        draft.interval_unit = TaskIntervalUnit::Hour;
        assert_eq!(
            draft.schedule().expect("every schedule"),
            CronSchedule::Every {
                every_ms: 5_400_000,
                anchor_ms: None,
            }
        );
        draft.anchor = "2030-01-02T03:04".to_string();
        let anchored = draft.schedule().expect("anchored every schedule");
        let expected_anchor =
            DateTime::parse_from_rfc3339(&local_input_to_rfc3339("2030-01-02T03:04").unwrap())
                .unwrap()
                .timestamp_millis();
        assert_eq!(
            anchored,
            CronSchedule::Every {
                every_ms: 5_400_000,
                anchor_ms: Some(expected_anchor),
            }
        );
        draft.anchor = "not a time".to_string();
        assert_eq!(draft.schedule(), Err(TaskDraftError::Anchor));
        draft.anchor.clear();

        draft.schedule_kind = TaskScheduleKind::Cron;
        draft.cron_expr = " 0 9 * * * ".to_string();
        draft.timezone = "  ".to_string();
        assert_eq!(
            draft.schedule().expect("cron schedule"),
            CronSchedule::Cron {
                expr: "0 9 * * *".to_string(),
                tz: None,
            }
        );
        draft.timezone = "Asia/Shanghai".to_string();
        assert_eq!(
            draft.schedule().expect("cron schedule with a zone"),
            CronSchedule::Cron {
                expr: "0 9 * * *".to_string(),
                tz: Some("Asia/Shanghai".to_string()),
            }
        );
    }

    /// G-P1: the required fields are checked before the runtime sees them.
    #[test]
    fn task_draft_rejects_missing_required_fields() {
        let mut draft = TaskDraft::blank();
        draft.name = "  ".to_string();
        assert_eq!(draft.validate(), Err(TaskDraftError::Name));

        draft.name = "Nightly".to_string();
        draft.prompt = String::new();
        assert_eq!(draft.validate(), Err(TaskDraftError::Prompt));

        draft.prompt = "x".repeat(TASK_PROMPT_MAX_CHARS + 1);
        assert_eq!(draft.validate(), Err(TaskDraftError::PromptTooLong));

        draft.prompt = "Summarize".to_string();
        draft.at = "not a time".to_string();
        assert_eq!(draft.validate(), Err(TaskDraftError::At));

        draft.schedule_kind = TaskScheduleKind::Every;
        draft.every_value = "0".to_string();
        assert_eq!(draft.validate(), Err(TaskDraftError::Every));
        draft.every_value = "-2".to_string();
        assert_eq!(draft.validate(), Err(TaskDraftError::Every));

        draft.schedule_kind = TaskScheduleKind::Cron;
        draft.cron_expr = "  ".to_string();
        assert_eq!(draft.validate(), Err(TaskDraftError::Cron));

        draft.cron_expr = "0 9 * * *".to_string();
        draft.target_kind = TaskTargetKind::Session;
        draft.session_id = None;
        assert_eq!(draft.validate(), Err(TaskDraftError::Session));
        draft.session_id = Some("  ".to_string());
        assert_eq!(draft.validate(), Err(TaskDraftError::Session));
        draft.session_id = Some("session-1".to_string());
        assert_eq!(draft.validate(), Ok(()));
    }

    /// G-P1: the target keeps the workspace reference the store validates and
    /// binds the picked session only in session mode.
    #[test]
    fn task_draft_targets_the_workspace_or_a_session() {
        let mut draft = TaskDraft::blank();
        draft.name = "Nightly".to_string();
        draft.prompt = "Summarize".to_string();
        draft.cron_expr = "0 9 * * *".to_string();
        draft.schedule_kind = TaskScheduleKind::Cron;
        let workspace = task_workspace();
        let request = draft
            .create_request(workspace.clone())
            .expect("workspace job");
        assert_eq!(request.name, "Nightly");
        assert_eq!(request.payload.text, "Summarize");
        assert!(request.enabled);
        assert_eq!(
            request.target,
            CronJobTarget::Workspace {
                workspace: workspace.clone(),
                launch: CronLaunchSpec::default(),
            }
        );

        draft.target_kind = TaskTargetKind::Session;
        draft.session_id = Some(" session-1 ".to_string());
        assert_eq!(
            draft.target(workspace.clone()).expect("session target"),
            CronJobTarget::Session {
                session_id: "session-1".to_string(),
                workspace: workspace.clone(),
            }
        );
    }

    /// G-P1: editing prefills from the stored job and preserves an existing
    /// interval anchor, which the dialog does not expose.
    #[test]
    fn task_draft_prefills_from_a_job_and_keeps_its_anchor() {
        let job = CronJob {
            id: "job-1".to_string(),
            name: "Hourly".to_string(),
            schedule: CronSchedule::Every {
                every_ms: 2 * TASK_HOUR_MS,
                anchor_ms: Some(1_000),
            },
            payload: CronJobPayload {
                text: "Check the build".to_string(),
            },
            enabled: false,
            target: CronJobTarget::Session {
                session_id: "session-1".to_string(),
                workspace: task_workspace(),
            },
            created_at_ms: 0,
            config_updated_at_ms: 0,
            updated_at_ms: 0,
            state: Default::default(),
        };
        let draft = TaskDraft::from_job(&job);
        assert_eq!(draft.name, "Hourly");
        assert_eq!(draft.prompt, "Check the build");
        assert!(!draft.enabled);
        assert_eq!(draft.schedule_kind, TaskScheduleKind::Every);
        assert_eq!(draft.every_value, "2");
        assert_eq!(draft.interval_unit, TaskIntervalUnit::Hour);
        assert_eq!(draft.target_kind, TaskTargetKind::Session);
        assert_eq!(draft.session_id.as_deref(), Some("session-1"));
        assert_eq!(draft.anchor, timestamp_ms_to_local_input(1_000));

        let update = draft
            .update_request(task_workspace())
            .expect("update request");
        // The editor works in whole local minutes, so the stored anchor reads
        // back truncated to the minute (web `toLocalDateTimeInput`).
        let expected_anchor = DateTime::parse_from_rfc3339(
            &local_input_to_rfc3339(&draft.anchor).expect("anchor input"),
        )
        .expect("anchor rfc3339")
        .timestamp_millis();
        assert_eq!(
            update.schedule,
            Some(CronSchedule::Every {
                every_ms: 2 * TASK_HOUR_MS,
                anchor_ms: Some(expected_anchor),
            })
        );
        assert_eq!(
            update.target,
            Some(CronJobTarget::Session {
                session_id: "session-1".to_string(),
                workspace: task_workspace(),
            })
        );
    }
}
