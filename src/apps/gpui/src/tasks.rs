//! Shell task board (E4-b2): the web `TodosScene` structure — an overview
//! card with two metrics, a job list where every active job takes one row (its
//! next run), a month calendar carrying the whole remaining agenda, and a
//! day-detail panel — laid out as quiet cards. Occurrences are derived the
//! same way the web panel derives them: the board expands each schedule over
//! the visible month, so a day cell marks runs the scheduler will actually
//! perform rather than only the next-run instant the store carries (G-P2's
//! store-truth marks retired with this batch). Jobs with nothing left to run
//! stay visible in the inactive partition; the create/edit dialog, the run-now
//! capability and the reload button are shell retainers.

use std::str::FromStr as _;

use chrono::{
    DateTime, Datelike as _, Local, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone as _,
};
use chrono_tz::Tz;
use cron::Schedule as CronExpr;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, StyledExt as _, WindowExt};
use gpui_kit::InteractiveElement as _;
use gpui_kit::{
    div, prelude::*, px, Anchor, App, ClickEvent, FontWeight, IntoElement, SharedString,
    WeakEntity, Window,
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
/// through [`RuntimeEvent`] projections and the board's own tick.
#[derive(Debug)]
pub(crate) struct TasksPageState {
    pub jobs: Vec<CronJob>,
    pub error: Option<String>,
    /// The job list was fetched at least once (drives reload-on-open).
    pub loaded: bool,
    /// The calendar day whose detail panel is open (web `selectedDayKey`).
    pub selected_day: Option<NaiveDate>,
    /// First day of the month the calendar is anchored on (web
    /// `monthAnchorMs`); shifted by the month navigation.
    pub month_anchor: NaiveDate,
    /// The tick's clock (web `nowMs`): countdowns and badges read this so a
    /// quiet board still ages honestly.
    pub now_ms: i64,
}

impl Default for TasksPageState {
    fn default() -> Self {
        let now = Local::now();
        Self {
            jobs: Vec::new(),
            error: None,
            loaded: false,
            selected_day: None,
            month_anchor: now.date_naive().with_day(1).expect("day one exists"),
            now_ms: now.timestamp_millis(),
        }
    }
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
            report_policy: Default::default(),
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
            report_policy: Default::default(),
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

/// Web `TodoItemRow` badges: the row flags this board can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobBadge {
    Running,
    NextRun,
    Overdue,
}

/// Web `inactive` reasons: why a job has no upcoming run. Precedence follows
/// the web row — a disabled job is paused first, then a finished one-shot is
/// done, and anything the board cannot turn into a run is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InactiveReason {
    Paused,
    Done,
    InvalidSchedule,
}

/// A schedule the service would also reject (web surfaces it as an inactive
/// job rather than dropping it silently).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CronScheduleError {
    Invalid,
}

/// Per-job caps that keep a high-frequency schedule from swamping either tier
/// (web `todoOccurrences.ts`, same values): the list takes the leading runs,
/// the calendar budget is per day so a frequent job still marks every day it
/// runs.
pub(crate) const MAX_LIST_OCCURRENCES_PER_JOB: usize = 3;
pub(crate) const MAX_CALENDAR_OCCURRENCES_PER_JOB_PER_DAY: usize = 4;
pub(crate) const MAX_OCCURRENCES_PER_JOB: usize = 200;

/// One dated run (web `TodoOccurrence`): the same recurring job can appear on
/// several rows with different times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TodoOccurrence {
    /// Index into the board's job list; render reads the job through it.
    pub job_index: usize,
    /// When this run fires.
    pub at_ms: i64,
    /// True when the scheduler has this exact run queued as its next action.
    pub is_next_run: bool,
    /// True when the run time has passed and the job is not currently running.
    pub is_overdue: bool,
    /// True while the scheduler is executing this run.
    pub is_running: bool,
}

/// An inactive entry: a job with nothing upcoming stays reachable (web
/// `InactiveTodo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InactiveTodo {
    pub job_index: usize,
    pub reason: InactiveReason,
}

/// The web scene's two tiers (web `TodoBuckets`): `visible` takes one row per
/// active job — its nearest upcoming run, soonest first — while `calendar`
/// carries every expanded occurrence for the month view, near-term runs
/// included, so a day cell is never mysteriously empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TodoBuckets {
    pub visible: Vec<TodoOccurrence>,
    pub calendar: Vec<TodoOccurrence>,
    pub inactive: Vec<InactiveTodo>,
    /// Distinct jobs with at least one run due within 24 hours (web
    /// `dueSoonTodoCount`), overdue and running runs included.
    pub due_soon_jobs: usize,
}

/// Whether the scheduler is executing this job right now (web
/// `isJobRunning`). A running job keeps its pending trigger timestamp in the
/// past until the turn finishes, so without this check an in-flight run reads
/// as overdue.
pub(crate) fn job_running(job: &CronJob) -> bool {
    use openbitfun_agent_runtime::scheduled_job::ScheduledJobRunStatus;
    job.state.active_turn_id.is_some()
        || matches!(
            job.state.last_run_status,
            Some(ScheduledJobRunStatus::Running) | Some(ScheduledJobRunStatus::Queued)
        )
}

/// A one-shot job whose run already happened is finished, not broken (web
/// `hasCompletedOneShot`): the backend clears `next_run_at_ms` once it
/// enqueues a one-shot run, so the absence of a next run plus a recorded run
/// is what marks it complete.
fn has_completed_one_shot(job: &CronJob) -> bool {
    if !matches!(job.schedule, CronSchedule::At { .. }) {
        return false;
    }
    job.state.last_enqueued_at_ms.is_some()
        || job.state.last_run_finished_at_ms.is_some()
        || job.state.last_run_started_at_ms.is_some()
}

/// Runs an anchored interval series produces in `(after_ms, until_ms]` (web
/// `everyOccurrencesInRange`): the series is anchored, not relative to "now",
/// so a job created at 08:03 with a 1h interval fires at :03.
fn every_occurrences_in_range(
    every_ms: u64,
    anchor_ms: i64,
    after_ms: i64,
    until_ms: i64,
    limit: usize,
) -> Vec<i64> {
    let every = every_ms as i64;
    if every <= 0 {
        return Vec::new();
    }
    let first = if anchor_ms > after_ms {
        anchor_ms
    } else {
        let steps = (after_ms - anchor_ms) / every + 1;
        anchor_ms + steps * every
    };
    let mut occurrences = Vec::new();
    let mut at = first;
    while at <= until_ms && occurrences.len() < limit {
        occurrences.push(at);
        at += every;
    }
    occurrences
}

/// Cron runs in `(after_ms, until_ms]`, expanded where the schedule executes:
/// in its own timezone when it carries one, local wall clock otherwise (web
/// `cronOccurrencesInRange`).
fn cron_occurrences_in_range(
    expr: &str,
    tz: Option<&str>,
    after_ms: i64,
    until_ms: i64,
    limit: usize,
) -> Result<Vec<i64>, CronScheduleError> {
    let normalized = match expr.split_whitespace().count() {
        5 => format!("0 {expr}"),
        6 | 7 => expr.to_string(),
        _ => return Err(CronScheduleError::Invalid),
    };
    let schedule = CronExpr::from_str(&normalized).map_err(|_| CronScheduleError::Invalid)?;
    let zone = tz
        .map(|tz| Tz::from_str(tz))
        .transpose()
        .map_err(|_| CronScheduleError::Invalid)?;

    fn collect<Tz: chrono::TimeZone>(
        schedule: &CronExpr,
        after: chrono::DateTime<Tz>,
        after_ms: i64,
        until_ms: i64,
        limit: usize,
        out: &mut Vec<i64>,
    ) {
        for at in schedule.after(&after) {
            let at_ms = at.with_timezone(&chrono::Utc).timestamp_millis();
            if at_ms > until_ms {
                break;
            }
            if at_ms > after_ms {
                out.push(at_ms);
                if out.len() >= limit {
                    break;
                }
            }
        }
    }

    let mut occurrences = Vec::new();
    match zone {
        Some(zone) => {
            let after = zone
                .timestamp_millis_opt(after_ms)
                .single()
                .ok_or(CronScheduleError::Invalid)?;
            collect(
                &schedule,
                after,
                after_ms,
                until_ms,
                limit,
                &mut occurrences,
            );
        }
        None => {
            let after = Local
                .timestamp_millis_opt(after_ms)
                .single()
                .ok_or(CronScheduleError::Invalid)?;
            collect(
                &schedule,
                after,
                after_ms,
                until_ms,
                limit,
                &mut occurrences,
            );
        }
    }
    Ok(occurrences)
}

/// Lists the runs a schedule produces in `(after_ms, until_ms]` (web
/// `scheduleOccurrencesInRange`); the requested limit is clamped to the web
/// default cap, so a caller can lower it but never expand past 200 per call.
pub(crate) fn schedule_occurrences_in_range(
    schedule: &CronSchedule,
    created_at_ms: i64,
    after_ms: i64,
    until_ms: i64,
    limit: usize,
) -> Result<Vec<i64>, CronScheduleError> {
    let limit = limit.min(MAX_OCCURRENCES_PER_JOB);
    match schedule {
        CronSchedule::At { at } => {
            let at_ms = DateTime::parse_from_rfc3339(at)
                .map(|at| at.timestamp_millis())
                .map_err(|_| CronScheduleError::Invalid)?;
            Ok(if at_ms > after_ms && at_ms <= until_ms {
                vec![at_ms]
            } else {
                Vec::new()
            })
        }
        CronSchedule::Every {
            every_ms,
            anchor_ms,
        } => {
            if *every_ms == 0 {
                return Err(CronScheduleError::Invalid);
            }
            Ok(every_occurrences_in_range(
                *every_ms,
                anchor_ms.unwrap_or(created_at_ms),
                after_ms,
                until_ms,
                limit,
            ))
        }
        CronSchedule::Cron { expr, tz } => {
            cron_occurrences_in_range(expr, tz.as_deref(), after_ms, until_ms, limit)
        }
    }
}

/// Expands the calendar tier one local day at a time (web
/// `expandCalendarOccurrences`): budgeting per day rather than across the
/// whole window keeps a frequent job visible on every day it runs.
fn expand_calendar_occurrences(
    schedule: &CronSchedule,
    created_at_ms: i64,
    from_ms: i64,
    until_ms: i64,
) -> Result<Vec<i64>, CronScheduleError> {
    if until_ms <= from_ms {
        return Ok(Vec::new());
    }
    let first_day = Local
        .timestamp_millis_opt(from_ms)
        .single()
        .map(|at| at.date_naive());
    let last_day = Local
        .timestamp_millis_opt(until_ms)
        .single()
        .map(|at| at.date_naive());
    let (Some(first_day), Some(last_day)) = (first_day, last_day) else {
        return Ok(Vec::new());
    };

    let mut occurrences = Vec::new();
    let mut cursor = first_day;
    while cursor <= last_day {
        let day_start = Local
            .with_ymd_and_hms(cursor.year(), cursor.month(), cursor.day(), 0, 0, 0)
            .single();
        let next_day = cursor.succ_opt().and_then(|next| {
            Local
                .with_ymd_and_hms(next.year(), next.month(), next.day(), 0, 0, 0)
                .single()
        });
        if let (Some(day_start), Some(next_start)) = (day_start, next_day) {
            let day_end = next_start.timestamp_millis() - 1;
            occurrences.extend(schedule_occurrences_in_range(
                schedule,
                created_at_ms,
                day_start.timestamp_millis().max(from_ms) - 1,
                day_end.min(until_ms),
                MAX_CALENDAR_OCCURRENCES_PER_JOB_PER_DAY,
            )?);
        }
        cursor = cursor.succ_opt().unwrap_or(last_day);
    }
    Ok(occurrences)
}

/// Groups every job into the visible list, the calendar tier, or the inactive
/// partition (web `buildTodoBuckets`). `range_end_ms` bounds calendar
/// expansion; the list window always covers the next 24 hours even when the
/// calendar shows a later month.
pub(crate) fn build_todo_buckets(jobs: &[CronJob], now_ms: i64, range_end_ms: i64) -> TodoBuckets {
    let list_boundary_ms = now_ms + TASK_DAY_MS as i64;
    let expansion_end_ms = range_end_ms.max(list_boundary_ms);
    let mut runs: Vec<TodoOccurrence> = Vec::new();
    let mut inactive: Vec<InactiveTodo> = Vec::new();

    for (job_index, job) in jobs.iter().enumerate() {
        if !job.enabled {
            inactive.push(InactiveTodo {
                job_index,
                reason: InactiveReason::Paused,
            });
            continue;
        }
        let scheduled_at_ms = job.state.next_run_at_ms;
        // Expand from just before the scheduler's own next run when that is in
        // the past, so an overdue or retrying run still shows up.
        let expand_after_ms = scheduled_at_ms.map_or(now_ms, |at| (at - 1).min(now_ms));

        let mut occurrence_times = match schedule_occurrences_in_range(
            &job.schedule,
            job.created_at_ms,
            expand_after_ms,
            list_boundary_ms,
            MAX_LIST_OCCURRENCES_PER_JOB,
        )
        .and_then(|mut times| {
            times.extend(expand_calendar_occurrences(
                &job.schedule,
                job.created_at_ms,
                list_boundary_ms,
                expansion_end_ms,
            )?);
            Ok(times)
        }) {
            Ok(times) => times,
            Err(_) => {
                inactive.push(InactiveTodo {
                    job_index,
                    reason: InactiveReason::InvalidSchedule,
                });
                continue;
            }
        };

        // A one-shot that already ran expands to nothing; report it as
        // finished rather than as an unexplained empty schedule.
        if occurrence_times.is_empty() && scheduled_at_ms.is_none() {
            inactive.push(InactiveTodo {
                job_index,
                reason: if has_completed_one_shot(job) {
                    InactiveReason::Done
                } else {
                    InactiveReason::InvalidSchedule
                },
            });
            continue;
        }

        // The scheduler's pending trigger wins over a recomputed time: a
        // retry sits off the nominal series and is still the run that will
        // actually happen.
        if let Some(at) = scheduled_at_ms {
            if !occurrence_times.contains(&at) {
                occurrence_times.push(at);
                occurrence_times.sort_unstable();
            }
        }

        let running = job_running(job);
        runs.extend(occurrence_times.into_iter().map(|at_ms| TodoOccurrence {
            job_index,
            at_ms,
            is_next_run: scheduled_at_ms == Some(at_ms),
            is_overdue: at_ms < now_ms && !running,
            is_running: running && scheduled_at_ms == Some(at_ms),
        }));
    }

    let by_time = |a: &TodoOccurrence, b: &TodoOccurrence| {
        a.at_ms
            .cmp(&b.at_ms)
            .then_with(|| jobs[a.job_index].name.cmp(&jobs[b.job_index].name))
    };
    runs.sort_by(by_time);
    let calendar = runs.clone();

    // One row per job: the run the scheduler has queued wins over the nearest
    // recomputed one (web `visibleTodos`).
    let mut nearest: Vec<Option<TodoOccurrence>> = vec![None; jobs.len()];
    for occurrence in &runs {
        let slot = &mut nearest[occurrence.job_index];
        match slot {
            None => *slot = Some(*occurrence),
            Some(current) if occurrence.is_next_run && !current.is_next_run => {
                *slot = Some(*occurrence)
            }
            _ => {}
        }
    }
    let mut visible: Vec<TodoOccurrence> = nearest.into_iter().flatten().collect();
    visible.sort_by(by_time);

    let due_soon_jobs = runs
        .iter()
        .filter(|occurrence| occurrence.at_ms <= list_boundary_ms)
        .map(|occurrence| occurrence.job_index)
        .collect::<std::collections::HashSet<_>>()
        .len();

    inactive.sort_by(|a, b| {
        jobs[b.job_index]
            .config_updated_at_ms
            .cmp(&jobs[a.job_index].config_updated_at_ms)
    });

    TodoBuckets {
        visible,
        calendar,
        inactive,
        due_soon_jobs,
    }
}

/// Row badges straight off the occurrence flags (web row order: running,
/// next run, overdue).
pub(crate) fn occurrence_badges(occurrence: &TodoOccurrence) -> Vec<JobBadge> {
    let mut badges = Vec::new();
    if occurrence.is_running {
        badges.push(JobBadge::Running);
    }
    if occurrence.is_next_run {
        badges.push(JobBadge::NextRun);
    }
    if occurrence.is_overdue {
        badges.push(JobBadge::Overdue);
    }
    badges
}

/// One day cell of the month calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonthCell {
    /// The full local date the cell stands for (the day panel keys on it).
    pub date: NaiveDate,
    pub day: u32,
    pub in_month: bool,
    pub is_today: bool,
    /// Indexes into the board's calendar occurrence list, in time order.
    pub occurrences: Vec<usize>,
}

/// Six-week Sunday-first grid covering the anchor's month (web
/// `buildMonthGrid`): 42 fixed cells so the calendar never reflows between
/// months, and a run happening today is exactly what someone opening a
/// calendar expects to see.
pub(crate) fn build_month_grid(
    anchor: NaiveDate,
    calendar: &[TodoOccurrence],
    today: NaiveDate,
) -> Vec<MonthCell> {
    let first = anchor.with_day(1).expect("day one exists");
    let lead = i64::from(first.weekday().num_days_from_sunday());
    let grid_start = first - chrono::Duration::days(lead);
    let mut by_day: std::collections::HashMap<NaiveDate, Vec<usize>> = Default::default();
    for (index, occurrence) in calendar.iter().enumerate() {
        if let Some(at) = Local.timestamp_millis_opt(occurrence.at_ms).single() {
            by_day.entry(at.date_naive()).or_default().push(index);
        }
    }
    (0..42)
        .map(|index| {
            let date = grid_start + chrono::Duration::days(index);
            MonthCell {
                date,
                day: date.day(),
                in_month: date.month() == anchor.month(),
                is_today: date == today,
                occurrences: by_day.get(&date).cloned().unwrap_or_default(),
            }
        })
        .collect()
}

/// Last millisecond of the anchor month's six-week grid (web `monthRangeMs`
/// bounds the calendar expansion with the same window). An unusable local
/// conversion yields zero, which simply narrows the expansion to the 24-hour
/// list window.
pub(crate) fn month_range_end_ms(anchor: NaiveDate) -> i64 {
    let first = anchor.with_day(1).expect("day one exists");
    let lead = i64::from(first.weekday().num_days_from_sunday());
    let last = first - chrono::Duration::days(lead) + chrono::Duration::days(41);
    Local
        .with_ymd_and_hms(last.year(), last.month(), last.day(), 23, 59, 59)
        .single()
        .map(|end| end.timestamp_millis() + 999)
        .unwrap_or(0)
}

/// `HH:mm` in the viewer's local wall clock (web `formatTimeOfDay`).
pub(crate) fn format_occurrence_time(at_ms: i64) -> String {
    Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map(|at| at.format("%H:%M").to_string())
        .unwrap_or_else(|| at_ms.to_string())
}

/// `MM/DD HH:mm` (web `formatDateTime`): the day-detail title and the
/// one-shot summary render this compact shape.
pub(crate) fn format_occurrence_datetime(at_ms: i64) -> String {
    Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map(|at| at.format("%m/%d %H:%M").to_string())
        .unwrap_or_else(|| at_ms.to_string())
}

/// The RFC 3339 `at` value of a one-shot schedule in the same compact shape;
/// a value the wire could not have written passes through untouched.
fn local_compact_datetime(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|at| at.with_timezone(&Local).format("%m/%d %H:%M").to_string())
        .unwrap_or_else(|_| at.to_string())
}

/// Which relative bucket a countdown falls into (web `formatCountdown`); the
/// split is pure so tests can pin it without a locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Countdown {
    Overdue,
    Now,
    InMinutes(i64),
    InHours(i64),
    InOneDay,
    InDays(i64),
}

pub(crate) fn countdown_choice(target_ms: i64, now_ms: i64) -> Countdown {
    let delta_ms = target_ms - now_ms;
    if delta_ms < 0 {
        return Countdown::Overdue;
    }
    let minutes = delta_ms / TASK_MINUTE_MS as i64;
    if minutes < 1 {
        return Countdown::Now;
    }
    if minutes < 60 {
        return Countdown::InMinutes(minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return Countdown::InHours(hours);
    }
    let days = hours / 24;
    if days == 1 {
        Countdown::InOneDay
    } else {
        Countdown::InDays(days)
    }
}

/// Names the workspace a job runs in (web `resolveJobWorkspaceLabel`): the
/// matched workspace's name, else the stored path's tail. A job outlives its
/// workspace list, and the path never picks another workspace.
pub(crate) fn job_workspace_label(job: &CronJob, current: Option<(&str, &str)>) -> String {
    let reference = match &job.target {
        CronJobTarget::Workspace { workspace, .. } | CronJobTarget::Session { workspace, .. } => {
            workspace
        }
    };
    if let Some((current_id, current_name)) = current {
        if reference.workspace_id.as_deref() == Some(current_id) && !current_name.is_empty() {
            return current_name.to_string();
        }
    }
    let path = reference.workspace_path.trim();
    let tail = path.split('/').filter(|segment| !segment.is_empty()).last();
    tail.unwrap_or(path).to_string()
}

/// The quiet-card container every board section renders on (style.md §3): the
/// settings-page card geometry — panel surface, hairline boundary, the theme's
/// large radius — with an arbitrary body instead of label/control rows.
fn scene_card(anchor: &'static str, cx: &App) -> gpui_kit::Div {
    div()
        .debug_selector(move || format!("tasks-card-{anchor}"))
        .w_full()
        .flex()
        .flex_col()
        .bg(cx.theme().group_box)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
}

/// A card head: title and optional muted description on the left, the extra
/// lane on the right (the `SettingsCard` head shape).
fn scene_card_head(
    title: String,
    description: Option<String>,
    extra: Option<gpui_kit::AnyElement>,
    cx: &App,
) -> gpui_kit::Div {
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
                    .min_w_0()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(title),
                    )
                    .children(description.map(|text| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(text)
                    })),
            )
            .children(extra),
    )
}

/// A row inside a scene card: the settings-card row geometry (12px block,
/// hairline between rows) for list-shaped bodies.
fn scene_card_row(divider: bool, cx: &App) -> gpui_kit::Div {
    div().flex().flex_col().px_4().py_3().when(divider, |row| {
        row.border_t_1()
            .border_color(cx.theme().border.opacity(0.6))
    })
}

/// The full board: page header, overview card, month calendar card, the job
/// list card, and — when a day is selected — the day-detail card. With
/// nothing scheduled at all the board collapses to the scene's branded empty
/// card; the overview and calendar still render around a non-empty but fully
/// inactive store ("0 upcoming" is a fact worth showing).
pub(crate) fn render_page(
    state: &TasksPageState,
    view: gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::AnyElement {
    let now_ms = if state.now_ms > 0 {
        state.now_ms
    } else {
        chrono::Utc::now().timestamp_millis()
    };
    let buckets = build_todo_buckets(&state.jobs, now_ms, month_range_end_ms(state.month_anchor));

    let reload_tx = tx.clone();
    let new_view = view.clone();
    let new_tx = tx.clone();
    let header = div()
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
                .min_w_0()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(text.tasks_title.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.tasks_subtitle.clone()),
                ),
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
                            open_task_editor(new_view.clone(), None, new_tx.clone(), window, cx);
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
        );

    // Natural height: `h_full()` capped the page inside its scroll wrapper at
    // the viewport height, so a long job list had no scroll extent.
    let mut column = div()
        .w_full()
        .flex()
        .flex_col()
        .debug_selector(|| "tasks-page-content".to_string())
        .gap_3()
        .p_4()
        .child(header);

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

    if state.jobs.is_empty() {
        let empty_view = view.clone();
        let empty_tx = tx.clone();
        let action = Button::new("tasks-new-empty")
            .label(text.tasks_new.clone())
            .small()
            .with_variant(ButtonVariant::Primary)
            .on_click(move |_, window, cx| {
                open_task_editor(empty_view.clone(), None, empty_tx.clone(), window, cx);
            })
            .into_any_element();
        column = column.child(crate::shell::brand_empty_card(
            gpui_kit::assets::IconName::Calendar,
            &text.tasks_empty_title,
            &text.tasks_empty,
            Some(action),
            cx,
        ));
        return column.into_any_element();
    }

    column = column.child(render_overview_card(&buckets, text, cx));
    column = column.child(render_month_calendar(state, &buckets, &view, text, cx));
    if let Some(day) = state.selected_day {
        column = column.child(render_day_detail(
            state, &buckets, day, now_ms, &view, tx, text, cx,
        ));
    }
    column = column.child(render_job_list(
        state, &buckets, now_ms, &view, tx, text, cx,
    ));

    column.into_any_element()
}

/// One overview metric: the big value with its unit, the muted label under it
/// (web `overview.total` / `overview.dueSoon` shape).
fn overview_metric(value: usize, label: String, text: &ShellText, cx: &App) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .min_w_0()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(value.to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(text.tasks_overview_unit.clone()),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
}

/// The overview card: the web scene's two metrics — visible jobs and jobs due
/// within 24 hours — under the 计划概览 head with its calendar glyph.
fn render_overview_card(buckets: &TodoBuckets, text: &ShellText, cx: &App) -> gpui_kit::Div {
    scene_card("overview", cx)
        .child(scene_card_head(
            text.tasks_overview.clone(),
            None,
            Some(
                Icon::new(gpui_kit::assets::IconName::CalendarDays)
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element(),
            ),
            cx,
        ))
        .child(
            div()
                .px_4()
                .pb_4()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap_8()
                .child(overview_metric(
                    buckets.visible.len(),
                    text.tasks_overview_total.clone(),
                    text,
                    cx,
                ))
                .child(overview_metric(
                    buckets.due_soon_jobs,
                    text.tasks_overview_due_soon.clone(),
                    text,
                    cx,
                )),
        )
}

/// The month calendar card: month navigation (previous / label / next plus
/// 今天), a Sunday-first six-week grid whose cells carry up to three occurrence
/// chips, and the web cell states (today outlined, selected filled, outside
/// month dimmed). Clicking a cell toggles the day panel below (web
/// `onSelectDay` toggles the same way).
fn render_month_calendar(
    state: &TasksPageState,
    buckets: &TodoBuckets,
    view: &gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    const CELL_HEIGHT: f32 = 68.;
    let today = Local::now().date_naive();
    let grid = build_month_grid(state.month_anchor, &buckets.calendar, today);
    let muted = cx.theme().muted_foreground;

    let prev_view = view.clone();
    let next_view = view.clone();
    let today_view = view.clone();
    let actions = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            Button::new("tasks-cal-prev")
                .icon(gpui_kit::assets::IconName::ChevronLeft)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.tasks_cal_prev.clone())
                .on_click(move |_, _, cx| {
                    if let Some(shell) = prev_view.upgrade() {
                        shell.update(cx, |shell, cx| {
                            shell.shift_month(-1);
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(Icon::new(gpui_kit::assets::IconName::CalendarDays).text_color(muted))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(state.month_anchor.format("%Y-%m").to_string()),
                ),
        )
        .child(
            Button::new("tasks-cal-next")
                .icon(gpui_kit::assets::IconName::ChevronRight)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.tasks_cal_next.clone())
                .on_click(move |_, _, cx| {
                    if let Some(shell) = next_view.upgrade() {
                        shell.update(cx, |shell, cx| {
                            shell.shift_month(1);
                            cx.notify();
                        });
                    }
                }),
        )
        .child(
            Button::new("tasks-cal-today")
                .label(text.tasks_cal_today.clone())
                .small()
                .with_variant(ButtonVariant::Secondary)
                .on_click(move |_, _, cx| {
                    if let Some(shell) = today_view.upgrade() {
                        shell.update(cx, |shell, cx| {
                            shell.tasks.month_anchor = Local::now()
                                .date_naive()
                                .with_day(1)
                                .expect("day one exists");
                            shell.tasks.selected_day = None;
                            cx.notify();
                        });
                    }
                }),
        );

    // Sunday-first to match the web grid (`buildMonthGrid` starts each week
    // on Sunday); the seven weekday keys render in that order.
    let weekdays = [
        text.tasks_wd_sun.clone(),
        text.tasks_wd_mon.clone(),
        text.tasks_wd_tue.clone(),
        text.tasks_wd_wed.clone(),
        text.tasks_wd_thu.clone(),
        text.tasks_wd_fri.clone(),
        text.tasks_wd_sat.clone(),
    ];
    let mut header = div().flex().gap(px(3.));
    for name in weekdays {
        header = header.child(
            div()
                .flex_1()
                .flex()
                .justify_center()
                .child(div().text_xs().text_color(muted).child(name)),
        );
    }

    let mut grid_element = div().v_flex().gap(px(3.)).min_w_0();
    for week in 0..6usize {
        let mut row = div().flex().gap(px(3.));
        for col in 0..7usize {
            let cell = &grid[week * 7 + col];
            let selected = state.selected_day == Some(cell.date);
            let mut day = div()
                .flex_1()
                .h(px(CELL_HEIGHT))
                .v_flex()
                .items_center()
                .justify_center()
                .gap_0p5()
                .rounded(cx.theme().radius)
                .px_1()
                .overflow_hidden()
                .child(
                    div()
                        .text_xs()
                        .text_color(if cell.in_month {
                            cx.theme().foreground
                        } else {
                            muted.opacity(0.5)
                        })
                        .child(cell.day.to_string()),
                );
            if !cell.occurrences.is_empty() {
                let mut chips = div().v_flex().gap(px(2.)).min_w_0().w_full();
                for index in cell.occurrences.iter().take(MAX_CHIPS_PER_DAY) {
                    let occurrence = &buckets.calendar[*index];
                    chips = chips.child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .gap_0p5()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format_occurrence_time(occurrence.at_ms)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(job_name_of(&state.jobs, occurrence.job_index)),
                            ),
                    );
                }
                if cell.occurrences.len() > MAX_CHIPS_PER_DAY {
                    chips = chips.child(div().w_full().text_xs().text_color(muted).child(
                        crate::i18n::tasks_cal_more(cell.occurrences.len() - MAX_CHIPS_PER_DAY, cx),
                    ));
                }
                day = day.child(chips);
            }
            if cell.is_today {
                day = day.border_1().border_color(cx.theme().info);
            }
            if selected {
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

    scene_card("calendar", cx)
        .child(scene_card_head(
            text.tasks_cal_title.clone(),
            None,
            Some(actions.into_any_element()),
            cx,
        ))
        .child(
            div()
                .px_4()
                .pb_4()
                .v_flex()
                .gap_2()
                .child(header)
                .child(grid_element),
        )
}

/// `Shifts` a month anchor by whole months (web `shiftMonth`), clamped to a
/// real first-of-month; pure so tests can pin the year rollover.
pub(crate) fn shift_month_anchor(anchor: NaiveDate, delta: i32) -> NaiveDate {
    let total = i32::from(anchor.year()) * 12 + anchor.month() as i32 - 1 + delta;
    let year = total.div_euclid(12);
    let month = total.rem_euclid(12) as u32 + 1;
    chrono::NaiveDate::from_ymd_opt(year, month, 1).expect("first of month exists")
}

/// The board's month navigation on the window state: shift the anchor and
/// drop the day selection (web clears the selection on the same move).
impl crate::shell::ShellWindow {
    pub(crate) fn shift_month(&mut self, delta: i32) {
        self.tasks.month_anchor = shift_month_anchor(self.tasks.month_anchor, delta);
        self.tasks.selected_day = None;
    }
}

/// The selected day's card (web day-detail): the title is the day's first
/// occurrence in the compact `MM/DD HH:mm` shape, the web panel's clear
/// action, and one row per run that day — the same job can appear several
/// times — or the web empty line.
fn render_day_detail(
    state: &TasksPageState,
    buckets: &TodoBuckets,
    day: NaiveDate,
    now_ms: i64,
    view: &gpui_kit::WeakEntity<crate::shell::ShellWindow>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let clear_view = view.clone();
    let day_occurrences: Vec<usize> = buckets
        .calendar
        .iter()
        .enumerate()
        .filter(|(_, occurrence)| {
            Local
                .timestamp_millis_opt(occurrence.at_ms)
                .single()
                .is_some_and(|at| at.date_naive() == day)
        })
        .map(|(index, _)| index)
        .collect();
    let title = day_occurrences
        .first()
        .map(|index| format_occurrence_datetime(buckets.calendar[*index].at_ms))
        .unwrap_or_else(|| text.tasks_day_detail_title.clone());

    // The card head is the shared scene-card shape; the web panel's clear
    // action rides in the extra lane.
    let clear = Button::new("tasks-day-clear")
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
        })
        .into_any_element();

    let mut body = scene_card_row(false, cx);
    if day_occurrences.is_empty() {
        body = body.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(text.tasks_day_empty.clone()),
        );
    } else {
        let current_workspace = runtime::runtime_handle().map(|handle| {
            let workspace = handle.workspace();
            (workspace.id, workspace.name)
        });
        for (row_index, occurrence_index) in day_occurrences.iter().enumerate() {
            let occurrence = buckets.calendar[*occurrence_index];
            let Some(job) = state.jobs.get(occurrence.job_index) else {
                continue;
            };
            body = body.child(render_job_row(
                job,
                Some(&occurrence),
                None,
                row_index > 0,
                now_ms,
                "day",
                view,
                tx,
                text,
                current_workspace
                    .as_ref()
                    .map(|(id, name)| (id.as_str(), name.as_str())),
                cx,
            ));
        }
    }

    scene_card("day-detail", cx)
        .debug_selector(|| "tasks-day-detail".to_string())
        .child(scene_card_head(title, None, Some(clear), cx))
        .child(body)
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

/// Chips rendered inside a calendar day cell before collapsing into a "+N"
/// counter (web `MAX_CHIPS_PER_DAY`).
const MAX_CHIPS_PER_DAY: usize = 3;

/// The job list card: one row per active job — its nearest upcoming run,
/// soonest first — then the inactive partition under the web's own subheader,
/// so a job with no next run is visible as such instead of silently missing.
fn render_job_list(
    state: &TasksPageState,
    buckets: &TodoBuckets,
    now_ms: i64,
    view: &WeakEntity<ShellWindow>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    cx: &App,
) -> gpui_kit::Div {
    let current_workspace = runtime::runtime_handle().map(|handle| {
        let workspace = handle.workspace();
        (workspace.id, workspace.name)
    });

    let mut card = scene_card("jobs", cx).child(scene_card_head(
        crate::i18n::tasks_list_count_title(buckets.visible.len(), cx),
        None,
        None,
        cx,
    ));
    let mut body = scene_card_row(false, cx);
    if buckets.visible.is_empty() {
        body = body.child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "tasks-list-empty".to_string())
                .child(text.tasks_list_empty.clone()),
        );
    } else {
        for (index, occurrence) in buckets.visible.iter().enumerate() {
            let Some(job) = state.jobs.get(occurrence.job_index) else {
                continue;
            };
            body = body.child(render_job_row(
                job,
                Some(occurrence),
                None,
                index > 0,
                now_ms,
                "list",
                view,
                tx,
                text,
                current_workspace
                    .as_ref()
                    .map(|(id, name)| (id.as_str(), name.as_str())),
                cx,
            ));
        }
    }
    card = card.child(body);

    if !buckets.inactive.is_empty() {
        let mut inactive_body = scene_card_row(true, cx).child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(crate::i18n::tasks_inactive_title(
                    buckets.inactive.len(),
                    cx,
                )),
        );
        for (index, entry) in buckets.inactive.iter().enumerate() {
            let Some(job) = state.jobs.get(entry.job_index) else {
                continue;
            };
            inactive_body = inactive_body.child(render_job_row(
                job,
                None,
                Some(entry.reason),
                index > 0,
                now_ms,
                "list",
                view,
                tx,
                text,
                current_workspace
                    .as_ref()
                    .map(|(id, name)| (id.as_str(), name.as_str())),
                cx,
            ));
        }
        card = card.child(inactive_body);
    }
    card
}

/// One job row (web `TodoItemRow`): the calendar-clock glyph, the name with
/// its badges, a meta line (workspace · schedule summary · target ·
/// countdown), the stored last error when the run failed, and the action lane
/// — the web Switch plus the shell-retained run-now, edit and delete. The
/// row click opens the editor; the lane stops propagation so its controls
/// never double-fire. Disabled jobs stay visible at reduced opacity so the
/// toggle is discoverable; an inactive row carries its reason chip in place
/// of the countdown.
#[allow(clippy::too_many_arguments)]
fn render_job_row(
    job: &CronJob,
    occurrence: Option<&TodoOccurrence>,
    inactive: Option<InactiveReason>,
    divider: bool,
    now_ms: i64,
    scope: &'static str,
    view: &WeakEntity<ShellWindow>,
    tx: &mpsc::UnboundedSender<RuntimeEvent>,
    text: &ShellText,
    current_workspace: Option<(&str, &str)>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let enabled_tx = tx.clone();
    let job_id_enabled = job.id.clone();
    let run_tx = tx.clone();
    let job_id_run = job.id.clone();
    let edit_view = view.clone();
    let edit_job = job.clone();
    let edit_tx = tx.clone();
    let row_view = view.clone();
    let row_job = job.clone();
    let row_tx = tx.clone();
    let delete_view = view.clone();
    let delete_job = job.clone();
    let delete_tx = tx.clone();

    // The same job+instant renders in both the list card and the day-detail
    // card; element ids are unique per frame, so the scope keeps them apart.
    let row_id = match occurrence {
        Some(occurrence) => format!("{scope}-{}-{}", job.id, occurrence.at_ms),
        None => format!("{scope}-{}", job.id),
    };

    let mut name_row = div().flex().flex_row().items_center().gap_2().min_w_0();
    name_row = name_row.child(
        div()
            .text_sm()
            .text_color(cx.theme().foreground)
            .truncate()
            .child(job.name.clone()),
    );
    if let Some(occurrence) = occurrence {
        for badge in occurrence_badges(occurrence) {
            let (label, tone) = match badge {
                JobBadge::Running => (text.tasks_badge_running.clone(), cx.theme().success),
                JobBadge::NextRun => (text.tasks_badge_next_run.clone(), cx.theme().info),
                JobBadge::Overdue => (text.tasks_badge_overdue.clone(), cx.theme().danger),
            };
            name_row = name_row.child(status_badge(label, tone, cx));
        }
    }
    if let Some(reason) = inactive {
        let label = match reason {
            InactiveReason::Paused => text.tasks_inactive_paused.clone(),
            InactiveReason::Done => text.tasks_inactive_done.clone(),
            InactiveReason::InvalidSchedule => text.tasks_inactive_invalid.clone(),
        };
        name_row = name_row.child(status_badge(label, cx.theme().muted_foreground, cx));
    }

    let mut meta_parts = vec![
        job_workspace_label(job, current_workspace),
        schedule_summary(&job.schedule, cx),
        job_target_label(job, text, cx),
    ];
    if let Some(occurrence) = occurrence {
        meta_parts.push(countdown_label(occurrence.at_ms, now_ms, cx));
    }
    let mut body = div()
        .flex_1()
        .min_w_0()
        .v_flex()
        .gap_0p5()
        .child(name_row)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .truncate()
                .child(meta_parts.join(" · ")),
        );
    if let Some(error) = job.state.last_error.as_deref().filter(|e| !e.is_empty()) {
        body = body.child(
            div()
                .w_full()
                .min_w_0()
                .text_xs()
                .text_color(cx.theme().danger)
                .child(error.to_string()),
        );
    }

    let actions = div()
        .flex()
        .items_center()
        .gap_1()
        .child(
            Switch::new(SharedString::from(format!("task-toggle-{row_id}")))
                .checked(job.enabled)
                .on_click(move |checked: &bool, _window, cx| {
                    cx.stop_propagation();
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.set_cron_job_enabled(
                            job_id_enabled.clone(),
                            *checked,
                            enabled_tx.clone(),
                        );
                    }
                }),
        )
        .child(
            Button::new(SharedString::from(format!("task-run-{row_id}")))
                .icon(gpui_kit::assets::IconName::Play)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.tasks_run_now.clone())
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    if let Some(handle) = runtime::runtime_handle() {
                        handle.run_cron_job_now(job_id_run.clone(), run_tx.clone());
                    }
                }),
        )
        .child(
            Button::new(SharedString::from(format!("task-edit-{row_id}")))
                .icon(gpui_kit::assets::IconName::Pencil)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(crate::i18n::shared_term("tools.edit", cx))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
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
            Button::new(SharedString::from(format!("task-delete-{row_id}")))
                .icon(gpui_kit::assets::IconName::Trash)
                .small()
                .with_variant(ButtonVariant::Ghost)
                .tooltip(text.session_delete.clone())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    open_delete_confirm(
                        delete_view.clone(),
                        delete_job.clone(),
                        delete_tx.clone(),
                        window,
                        cx,
                    );
                }),
        );

    div()
        .id(SharedString::from(format!("tasks-row-{row_id}")))
        .debug_selector({
            let row_id = row_id.clone();
            move || format!("tasks-row-{row_id}")
        })
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_3()
        .when(divider, |row| {
            row.border_t_1()
                .border_color(cx.theme().border.opacity(0.6))
        })
        .when(!job.enabled, |row| row.opacity(0.5))
        .cursor_pointer()
        .on_click(move |_, window, cx| {
            open_task_editor(
                row_view.clone(),
                Some(row_job.clone()),
                row_tx.clone(),
                window,
                cx,
            );
        })
        .child(
            div()
                .flex_shrink_0()
                .w(px(36.))
                .h(px(36.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .child(
                    Icon::new(gpui_kit::assets::IconName::CalendarClock)
                        .text_color(cx.theme().muted_foreground),
                ),
        )
        .child(body)
        .child(actions)
        .into_any_element()
}

/// The human summary of the recurrence rule (web `formatScheduleSummary`):
/// the one-shot time, the interval in its largest unit, or the expression
/// with an optional timezone.
fn schedule_summary(schedule: &CronSchedule, cx: &App) -> String {
    match schedule {
        CronSchedule::At { at } => {
            crate::i18n::tasks_schedule_once(&local_compact_datetime(at), cx)
        }
        CronSchedule::Every { every_ms, .. } => {
            let (value, unit) = split_interval(*every_ms);
            match unit {
                TaskIntervalUnit::Minute => crate::i18n::tasks_schedule_every_minutes(&value, cx),
                TaskIntervalUnit::Hour => crate::i18n::tasks_schedule_every_hours(&value, cx),
                TaskIntervalUnit::Day => crate::i18n::tasks_schedule_every_days(&value, cx),
            }
        }
        CronSchedule::Cron { expr, tz } => match tz {
            Some(tz) => crate::i18n::tasks_schedule_cron_tz(expr, tz, cx),
            None => crate::i18n::tasks_schedule_cron(expr, cx),
        },
    }
}

/// What the job launches into: a specific session, or a fresh one named by
/// its agent type (web `formatJobTargetLabel`).
fn job_target_label(job: &CronJob, text: &ShellText, cx: &App) -> String {
    match &job.target {
        CronJobTarget::Session { .. } => text.tasks_editor_target_session.clone(),
        CronJobTarget::Workspace { launch, .. } => crate::i18n::tasks_target_new_session(
            &crate::i18n::shared_agent_name(&launch.agent_type, cx),
            cx,
        ),
    }
}

fn job_name_of(jobs: &[CronJob], job_index: usize) -> String {
    jobs.get(job_index)
        .map(|job| job.name.clone())
        .unwrap_or_default()
}

/// The localized countdown line for one occurrence (web `formatCountdown`).
fn countdown_label(at_ms: i64, now_ms: i64, cx: &App) -> String {
    match countdown_choice(at_ms, now_ms) {
        Countdown::Overdue => crate::i18n::tasks_relative_overdue(cx),
        Countdown::Now => crate::i18n::tasks_relative_now(cx),
        Countdown::InMinutes(value) => crate::i18n::tasks_relative_in_minutes(value, cx),
        Countdown::InHours(value) => crate::i18n::tasks_relative_in_hours(value, cx),
        Countdown::InOneDay => crate::i18n::tasks_relative_in_one_day(cx),
        Countdown::InDays(value) => crate::i18n::tasks_relative_in_days(value, cx),
    }
}

/// The delete confirmation (web `confirmDanger`): the dialog names the job,
/// and only the danger action deletes. Deleting the job being edited also
/// closes the editor, as the web panel does.
fn open_delete_confirm(
    view: WeakEntity<ShellWindow>,
    job: CronJob,
    tx: mpsc::UnboundedSender<RuntimeEvent>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = crate::i18n::tasks_delete_confirm_title(&job.name, cx);
    let confirm_view = view.clone();
    let confirm_job_id = job.id.clone();
    let confirm_tx = tx.clone();
    let confirm = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
        if let Some(handle) = runtime::runtime_handle() {
            handle.delete_cron_job(confirm_job_id.clone(), confirm_tx.clone());
        }
        if let Some(shell) = confirm_view.upgrade() {
            let confirm_job_id = confirm_job_id.clone();
            shell.update(cx, |shell, cx| {
                if shell.task_editor_target.as_deref() == Some(confirm_job_id.as_str()) {
                    shell.task_editor_target = None;
                }
                cx.notify();
            });
        }
        window.close_dialog(cx);
    };
    let cancel = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
        window.close_dialog(cx);
    };
    window.open_dialog(cx, move |dialog, _window, cx| {
        let text = crate::i18n::current(cx);
        dialog.title(div().child(title.clone())).footer(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("task-delete-cancel")
                        .label(text.session_action_cancel.clone())
                        .with_variant(ButtonVariant::Ghost)
                        .on_click(cancel.clone()),
                )
                .child(
                    Button::new("task-delete-accept")
                        .label(text.session_delete.clone())
                        .with_variant(ButtonVariant::Danger)
                        .on_click(confirm.clone()),
                ),
        )
    });
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
        let description = if editing {
            text.tasks_editor_edit_desc.clone()
        } else {
            text.tasks_editor_create_desc.clone()
        };
        let mut content = div()
            .w(px(520.))
            .v_flex()
            .gap_3()
            .debug_selector(|| "task-editor".to_string())
            // The web editor's intro (icon + what this dialog does); the title
            // itself lives in the dialog chrome.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(px(36.))
                            .h(px(36.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(gpui_kit::assets::IconName::CalendarClock)
                                    .text_color(label_color),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_color(label_color)
                            .child(description),
                    ),
            )
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
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(label_color)
                            .child(text.tasks_editor_cron_hint.clone()),
                    );
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
                        .label(if editing {
                            text.tasks_editor_btn_save.clone()
                        } else {
                            text.tasks_editor_btn_create.clone()
                        })
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
            report_policy: Default::default(),
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

    /// E4-b2: the one-shot occurrence appears inside its window and not
    /// outside it.
    #[test]
    fn at_occurrences_land_inside_the_window_only() {
        let at = "2026-11-10T09:00:00+08:00".to_string();
        let schedule = CronSchedule::At { at: at.clone() };
        let inside = schedule_occurrences_in_range(
            &schedule,
            0,
            0,
            DateTime::parse_from_rfc3339(&at)
                .unwrap()
                .timestamp_millis(),
            MAX_OCCURRENCES_PER_JOB,
        )
        .expect("at expansion");
        assert_eq!(inside.len(), 1);
        let before = schedule_occurrences_in_range(&schedule, 0, 0, 1, MAX_OCCURRENCES_PER_JOB)
            .expect("at expansion");
        assert!(before.is_empty());
    }

    /// E4-b2: the anchored interval series stays on its anchor (a job created
    /// at 08:03 with a 1h interval fires at :03) and honors the cap.
    #[test]
    fn every_occurrences_keep_the_anchor_alignment() {
        let times = every_occurrences_in_range(60_000, 1_000, 181_000, 1_000_000, 2);
        assert_eq!(times, vec![241_000, 301_000]);
    }

    /// E4-b2: cron expansion runs where the schedule executes — a `0 9 * * *`
    /// job with an Asia/Shanghai zone yields 09:00 in that zone, converted to
    /// the absolute instants the calendar plots.
    #[test]
    fn cron_occurrences_expand_in_the_schedule_timezone() {
        use chrono::TimeZone as _;
        let zone = chrono_tz::Tz::from_str("Asia/Shanghai").expect("zone");
        let after = zone
            .with_ymd_and_hms(2026, 11, 3, 0, 0, 0)
            .single()
            .expect("valid local time")
            .timestamp_millis();
        let until = zone
            .with_ymd_and_hms(2026, 11, 4, 0, 0, 0)
            .single()
            .expect("valid local time")
            .timestamp_millis();
        let times = schedule_occurrences_in_range(
            &CronSchedule::Cron {
                expr: "0 9 * * *".to_string(),
                tz: Some("Asia/Shanghai".to_string()),
            },
            0,
            after,
            until,
            MAX_OCCURRENCES_PER_JOB,
        )
        .expect("cron expansion");
        let expected = zone
            .with_ymd_and_hms(2026, 11, 3, 9, 0, 0)
            .single()
            .expect("valid local time")
            .timestamp_millis();
        assert_eq!(times, vec![expected]);
    }

    /// E4-b2: the two tiers — one row per active job with its nearest run,
    /// the calendar carrying every expanded occurrence — plus the inactive
    /// partition reading paused, done and invalid the way the web row does.
    #[test]
    fn buckets_split_visible_calendar_and_inactive() {
        let now: i64 = 1_760_000_000_000;
        let hourly = ScheduledJobRuntimeState {
            next_run_at_ms: Some(now + 1_800_000),
            ..Default::default()
        };
        let jobs = vec![
            job_with(
                "hourly",
                true,
                CronSchedule::Every {
                    every_ms: 3_600_000,
                    anchor_ms: Some(now - 1_800_000),
                },
                hourly,
            ),
            job("paused", false, ScheduledJobRuntimeState::default()),
            job_with(
                "finished",
                true,
                CronSchedule::At {
                    at: "2026-10-01T09:00:00+08:00".to_string(),
                },
                ScheduledJobRuntimeState {
                    last_enqueued_at_ms: Some(now - 86_400_000),
                    ..Default::default()
                },
            ),
            job_with(
                "broken",
                true,
                CronSchedule::Cron {
                    expr: "not a cron expression".to_string(),
                    tz: None,
                },
                ScheduledJobRuntimeState::default(),
            ),
        ];
        let buckets = build_todo_buckets(&jobs, now, now + TASK_DAY_MS as i64);
        assert_eq!(buckets.visible.len(), 1);
        assert_eq!(buckets.visible[0].job_index, 0);
        assert!(buckets.visible[0].is_next_run);
        assert!(!buckets.calendar.is_empty());
        assert_eq!(buckets.due_soon_jobs, 1);
        let reasons: Vec<InactiveReason> =
            buckets.inactive.iter().map(|entry| entry.reason).collect();
        assert_eq!(
            reasons,
            vec![
                InactiveReason::Paused,
                InactiveReason::Done,
                InactiveReason::InvalidSchedule
            ]
        );
    }

    /// E4-b2: the scheduler's pending trigger wins over the recomputed
    /// series; a running job is not overdue, and a passed run without a turn
    /// is.
    #[test]
    fn occurrence_flags_follow_running_overdue_and_next_run() {
        let now: i64 = 1_760_000_000_000;
        let schedule = CronSchedule::Every {
            every_ms: 3_600_000,
            anchor_ms: Some(now - 7_200_000),
        };
        let overdue = job_with(
            "overdue",
            true,
            schedule.clone(),
            ScheduledJobRuntimeState {
                next_run_at_ms: Some(now - 3_600_000),
                ..Default::default()
            },
        );
        let running = job_with(
            "running",
            true,
            schedule,
            ScheduledJobRuntimeState {
                next_run_at_ms: Some(now - 3_600_000),
                active_turn_id: Some("turn-1".to_string()),
                ..Default::default()
            },
        );
        let jobs = vec![overdue, running];
        let buckets = build_todo_buckets(&jobs, now, now + TASK_DAY_MS as i64);
        let first = buckets
            .visible
            .iter()
            .find(|occurrence| occurrence.job_index == 0)
            .expect("overdue row");
        assert!(first.is_next_run);
        assert!(first.is_overdue);
        assert!(!first.is_running);
        let second = buckets
            .visible
            .iter()
            .find(|occurrence| occurrence.job_index == 1)
            .expect("running row");
        assert!(second.is_running);
        assert!(!second.is_overdue);
        assert_eq!(
            occurrence_badges(first),
            vec![JobBadge::NextRun, JobBadge::Overdue]
        );
        assert_eq!(
            occurrence_badges(second),
            vec![JobBadge::Running, JobBadge::NextRun]
        );
    }

    /// E4-b2: the grid is always six weeks and starts on Sunday, and a day's
    /// occurrences index back into the calendar list.
    #[test]
    fn month_grid_is_sunday_first_and_indexes_day_occurrences() {
        use chrono::TimeZone as _;
        let now: i64 = 1_760_000_000_000;
        let at = chrono::Local
            .with_ymd_and_hms(2026, 11, 10, 9, 0, 0)
            .single()
            .expect("valid local time")
            .timestamp_millis();
        let job = job_with(
            "daily",
            true,
            CronSchedule::At {
                at: "2026-11-10T09:00:00+08:00".to_string(),
            },
            ScheduledJobRuntimeState {
                next_run_at_ms: Some(at),
                ..Default::default()
            },
        );
        let buckets = build_todo_buckets(&[job], now, now + 30 * TASK_DAY_MS as i64);
        let grid = build_month_grid(
            NaiveDate::from_ymd_opt(2026, 11, 15).expect("date"),
            &buckets.calendar,
            NaiveDate::from_ymd_opt(2026, 11, 15).expect("date"),
        );
        assert_eq!(grid.len(), 42);
        assert_eq!(grid[0].date.weekday(), chrono::Weekday::Sun);
        let tenth = grid
            .iter()
            .find(|cell| cell.date == NaiveDate::from_ymd_opt(2026, 11, 10).expect("date"))
            .expect("the day cell");
        assert_eq!(tenth.occurrences.len(), 1);
        assert_eq!(
            buckets.calendar[tenth.occurrences[0]].at_ms,
            buckets.calendar[0].at_ms
        );
    }

    /// E4-b2: the expansion window covers the anchor month's six-week grid,
    /// so a late-month job beyond the visible week still lands on its cell.
    #[test]
    fn month_range_end_covers_the_six_week_grid() {
        let anchor = NaiveDate::from_ymd_opt(2026, 11, 15).expect("date");
        let end = month_range_end_ms(anchor);
        let first = anchor.with_day(1).expect("day one");
        let grid_end_day = first
            - chrono::Duration::days(i64::from(first.weekday().num_days_from_sunday()))
            + chrono::Duration::days(41);
        let expected_end = chrono::Local
            .with_ymd_and_hms(
                grid_end_day.year(),
                grid_end_day.month(),
                grid_end_day.day(),
                23,
                59,
                59,
            )
            .single()
            .expect("valid local time")
            .timestamp_millis()
            + 999;
        assert_eq!(end, expected_end);
    }

    /// E4-b2: the countdown buckets mirror the web wording split — overdue,
    /// imminent, and the minute/hour/day ladders with the singular day.
    #[test]
    fn countdown_choices_follow_the_web_ladder() {
        let now: i64 = 1_760_000_000_000;
        assert_eq!(countdown_choice(now - 1, now), Countdown::Overdue);
        assert_eq!(countdown_choice(now + 30_000, now), Countdown::Now);
        assert_eq!(
            countdown_choice(now + 5 * 60_000, now),
            Countdown::InMinutes(5)
        );
        assert_eq!(
            countdown_choice(now + 2 * 3_600_000, now),
            Countdown::InHours(2)
        );
        assert_eq!(
            countdown_choice(now + 24 * 3_600_000, now),
            Countdown::InOneDay
        );
        assert_eq!(
            countdown_choice(now + 3 * 24 * 3_600_000, now),
            Countdown::InDays(3)
        );
    }

    /// E4-b2: a job's workspace label is the matched workspace's name, or the
    /// stored path's tail when the workspace list no longer knows it.
    #[test]
    fn workspace_label_matches_by_id_and_falls_back_to_the_path_tail() {
        let mut job = job("named", true, ScheduledJobRuntimeState::default());
        job.target = CronJobTarget::Workspace {
            workspace: CronWorkspaceRef {
                workspace_id: Some("ws-1".to_string()),
                workspace_path: "/home/dev/project".to_string(),
                project_workspace_path: None,
                execution_target: None,
                remote_connection_id: None,
                remote_ssh_host: None,
            },
            launch: Default::default(),
        };
        assert_eq!(
            job_workspace_label(&job, Some(("ws-1", "My Project"))),
            "My Project"
        );
        assert_eq!(
            job_workspace_label(&job, Some(("ws-2", "Other"))),
            "project"
        );
        assert_eq!(job_workspace_label(&job, None), "project");
    }

    /// E4-b2: month navigation rolls over the year boundary and stays pinned
    /// to the first of the month.
    #[test]
    fn month_anchor_shifts_across_the_year_boundary() {
        let anchor = NaiveDate::from_ymd_opt(2026, 12, 15).expect("date");
        assert_eq!(
            shift_month_anchor(anchor, 1),
            NaiveDate::from_ymd_opt(2027, 1, 1).expect("date")
        );
        assert_eq!(
            shift_month_anchor(anchor, -1),
            NaiveDate::from_ymd_opt(2026, 11, 1).expect("date")
        );
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
            report_policy: Default::default(),
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
