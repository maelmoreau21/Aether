//! Jellyfin-style scheduled-task triggers (daily, weekly, interval, server startup) shared by
//! system tasks, library scans and the backup, plus the unified `/api/v1/admin/tasks` API.
//!
//! Triggers are the only scheduling configuration: `task_scheduler` fires them. The legacy
//! `next_due_at` / `next_run_at` / `enabled` columns are a mirror kept for older endpoints.
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query, query_as, query_scalar},
    identity::{IdentityStore, Role},
    task_scheduler::{Admission, TaskRef},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use jiff::{Span, Timestamp, civil, tz::TimeZone};
use serde::{Deserialize, Serialize};
use sqlx::AnyConnection;

/// Most triggers one task may carry.
pub const MAX_TRIGGERS_PER_TASK: i64 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TriggerType {
    Daily,
    Weekly,
    Interval,
    Startup,
}

impl TriggerType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Interval => "interval",
            Self::Startup => "startup",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "daily" => Self::Daily,
            "weekly" => Self::Weekly,
            "interval" => Self::Interval,
            "startup" => Self::Startup,
            _ => return None,
        })
    }
}

/// When a trigger fires. `day_of_week` counts from Sunday (0) to Saturday (6), like Jellyfin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerSpec {
    pub kind: TriggerType,
    pub time_of_day_minutes: Option<i64>,
    pub day_of_week: Option<i64>,
    pub interval_minutes: Option<i64>,
}

impl TriggerSpec {
    pub fn daily(time_of_day_minutes: i64) -> Self {
        Self {
            kind: TriggerType::Daily,
            time_of_day_minutes: Some(time_of_day_minutes),
            day_of_week: None,
            interval_minutes: None,
        }
    }
    pub fn weekly(day_of_week: i64, time_of_day_minutes: i64) -> Self {
        Self {
            kind: TriggerType::Weekly,
            time_of_day_minutes: Some(time_of_day_minutes),
            day_of_week: Some(day_of_week),
            interval_minutes: None,
        }
    }
    pub fn interval(minutes: i64) -> Self {
        Self {
            kind: TriggerType::Interval,
            time_of_day_minutes: None,
            day_of_week: None,
            interval_minutes: Some(minutes),
        }
    }
    pub fn startup() -> Self {
        Self {
            kind: TriggerType::Startup,
            time_of_day_minutes: None,
            day_of_week: None,
            interval_minutes: None,
        }
    }
    /// Keep only the fields meaningful for the type and check their ranges.
    pub fn normalized(self, min_interval: i64) -> Result<Self, &'static str> {
        let time = || {
            self.time_of_day_minutes
                .filter(|m| (0..1440).contains(m))
                .ok_or("timeOfDayMinutes must be between 0 and 1439")
        };
        Ok(match self.kind {
            TriggerType::Daily => Self::daily(time()?),
            TriggerType::Weekly => Self::weekly(
                self.day_of_week
                    .filter(|d| (0..7).contains(d))
                    .ok_or("dayOfWeek must be between 0 (Sunday) and 6")?,
                time()?,
            ),
            TriggerType::Interval => Self::interval(
                self.interval_minutes
                    .filter(|m| (min_interval..=10080).contains(m))
                    .ok_or("intervalMinutes is out of range")?,
            ),
            TriggerType::Startup => Self::startup(),
        })
    }
}

/// The server's time zone: `MEDIA_TIMEZONE` (IANA name), else the system zone (`TZ`,
/// `/etc/localtime`, Windows settings), else UTC. The tz database is bundled for minimal images.
pub fn server_time_zone() -> TimeZone {
    static ZONE: std::sync::LazyLock<TimeZone> = std::sync::LazyLock::new(|| {
        if let Some(name) = std::env::var("MEDIA_TIMEZONE")
            .ok()
            .filter(|name| !name.trim().is_empty())
        {
            match TimeZone::get(name.trim()) {
                Ok(zone) => return zone,
                Err(error) => eprintln!("MEDIA_TIMEZONE={name} ignored: {error}"),
            }
        }
        TimeZone::try_system().unwrap_or(TimeZone::UTC)
    });
    ZONE.clone()
}

fn at_local(date: civil::Date, minutes: i64, zone: &TimeZone) -> Option<i64> {
    let time = civil::Time::new((minutes / 60) as i8, (minutes % 60) as i8, 0, 0).ok()?;
    // DST gaps resolve forward and folds to the first occurrence ("compatible").
    let zoned = date.to_datetime(time).to_zoned(zone.clone()).ok()?;
    Some(zoned.timestamp().as_second())
}

/// Next firing instant strictly after `now` (Unix seconds), in the given local time zone.
/// Interval triggers count from `now`, i.e. from the moment the task last fired or the trigger
/// was saved. Startup triggers have no scheduled instant.
pub fn next_fire(spec: &TriggerSpec, now: i64, zone: &TimeZone) -> Option<i64> {
    match spec.kind {
        TriggerType::Startup => None,
        TriggerType::Interval => spec
            .interval_minutes
            .filter(|m| *m > 0)
            .map(|m| now.saturating_add(m.saturating_mul(60))),
        TriggerType::Daily | TriggerType::Weekly => {
            let minutes = spec.time_of_day_minutes.filter(|m| (0..1440).contains(m))?;
            let today = Timestamp::from_second(now)
                .ok()?
                .to_zoned(zone.clone())
                .date();
            for offset in 0..=8_i64 {
                let date = today.checked_add(Span::new().days(offset)).ok()?;
                if spec.kind == TriggerType::Weekly
                    && Some(i64::from(date.weekday().to_sunday_zero_offset())) != spec.day_of_week
                {
                    continue;
                }
                if let Some(at) = at_local(date, minutes, zone)
                    && at > now
                {
                    return Some(at);
                }
            }
            None
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct TriggerRow {
    pub id: String,
    pub task_kind: String,
    pub task_id: String,
    pub trigger_type: String,
    pub time_of_day_minutes: Option<i64>,
    pub day_of_week: Option<i64>,
    pub interval_minutes: Option<i64>,
    pub max_runtime_seconds: Option<i64>,
    pub enabled: i64,
    pub next_fire_at: Option<i64>,
    pub created_at: i64,
}

impl TriggerRow {
    pub(crate) fn spec(&self) -> Option<TriggerSpec> {
        Some(TriggerSpec {
            kind: TriggerType::parse(&self.trigger_type)?,
            time_of_day_minutes: self.time_of_day_minutes,
            day_of_week: self.day_of_week,
            interval_minutes: self.interval_minutes,
        })
    }
    pub(crate) fn timed(&self) -> bool {
        self.enabled == 1 && self.spec().is_some_and(|s| s.kind != TriggerType::Startup)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerView {
    pub id: String,
    #[serde(rename = "type")]
    pub trigger_type: String,
    pub time_of_day_minutes: Option<i64>,
    pub day_of_week: Option<i64>,
    pub interval_minutes: Option<i64>,
    pub max_runtime_seconds: Option<i64>,
    pub enabled: bool,
    pub next_fire_at: Option<i64>,
}

impl From<&TriggerRow> for TriggerView {
    fn from(row: &TriggerRow) -> Self {
        Self {
            id: row.id.clone(),
            trigger_type: row.trigger_type.clone(),
            time_of_day_minutes: row.time_of_day_minutes,
            day_of_week: row.day_of_week,
            interval_minutes: row.interval_minutes,
            max_runtime_seconds: row.max_runtime_seconds,
            enabled: row.enabled == 1,
            next_fire_at: row.next_fire_at,
        }
    }
}

const SELECT_TRIGGERS: &str = "SELECT id,task_kind,task_id,trigger_type,time_of_day_minutes,day_of_week,interval_minutes,max_runtime_seconds,enabled,next_fire_at,created_at FROM task_triggers";

pub async fn load(
    conn: &mut AnyConnection,
    kind: &str,
    id: &str,
) -> Result<Vec<TriggerRow>, sqlx::Error> {
    query_as("SELECT id,task_kind,task_id,trigger_type,time_of_day_minutes,day_of_week,interval_minutes,max_runtime_seconds,enabled,next_fire_at,created_at FROM task_triggers WHERE task_kind=? AND task_id=? ORDER BY created_at,id")
        .bind(kind)
        .bind(id)
        .fetch_all(conn)
        .await
}

/// Legacy `PUT .../schedule` endpoints: the task gets exactly one interval trigger (or none).
pub async fn replace_with_interval(
    conn: &mut AnyConnection,
    kind: &str,
    id: &str,
    interval_minutes: i64,
    enabled: bool,
    now: i64,
) -> Result<(), sqlx::Error> {
    query("DELETE FROM task_triggers WHERE task_kind=? AND task_id=?")
        .bind(kind)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if enabled {
        query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at) VALUES(?,?,?,'interval',?,1,?,?)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(kind).bind(id).bind(interval_minutes)
            .bind(now.saturating_add(interval_minutes.saturating_mul(60))).bind(now)
            .execute(&mut *conn).await?;
    }
    crate::task_scheduler::mirror(conn, kind, id).await?;
    Ok(())
}

/// Legacy `DELETE` of a library scan plan.
pub async fn remove_all(conn: &mut AnyConnection, kind: &str, id: &str) -> Result<(), sqlx::Error> {
    query("DELETE FROM task_triggers WHERE task_kind=? AND task_id=?")
        .bind(kind)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Unified administration API
// ---------------------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastRun {
    pub start: Option<i64>,
    pub end: Option<i64>,
    /// `succeeded`, `failed`, `interrupted`, `cancelled` (or `running` for the current run).
    pub outcome: String,
    pub message: Option<String>,
    pub error_kind: Option<String>,
    pub error_scope: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub description: String,
    pub category: String,
    /// `idle`, `queued` or `running`.
    pub state: &'static str,
    pub progress: Option<i64>,
    pub next_run_at: Option<i64>,
    pub last_run: Option<LastRun>,
    /// Why the task cannot be started now (disabled library, offline node), if any.
    pub reason: Option<String>,
    pub library_id: Option<String>,
    pub library_kind: Option<String>,
    pub warnings_count: i64,
    pub issues: Vec<String>,
    pub triggers: Vec<TriggerView>,
}

fn now() -> i64 {
    media_contracts::protocol::unix_now()
}

/// An API refusal. With a code, the body is `{"error": code}` so the interface can explain it;
/// without one only the status is sent (the interface then uses its generic status messages).
#[derive(Debug)]
pub struct ApiError(StatusCode, Option<&'static str>);

impl ApiError {
    fn coded(status: StatusCode, code: &'static str) -> Self {
        Self(status, Some(code))
    }
}

impl From<StatusCode> for ApiError {
    fn from(status: StatusCode) -> Self {
        Self(status, None)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self.1 {
            Some(code) => (self.0, Json(serde_json::json!({ "error": code }))).into_response(),
            None => self.0.into_response(),
        }
    }
}

async fn authorize(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<String, StatusCode> {
    let user = auth.authenticate(headers).await?;
    if user.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &user)?;
    }
    Ok(user.user.id)
}

fn internal<E: std::fmt::Display>(error: E) -> ApiError {
    eprintln!("task API: {error}");
    StatusCode::INTERNAL_SERVER_ERROR.into()
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/tasks", get(list))
        .route("/api/v1/admin/tasks/{id}", get(detail))
        .route("/api/v1/admin/tasks/{id}/run", post(run))
        .route("/api/v1/admin/tasks/{id}/cancel", post(cancel))
        .route(
            "/api/v1/admin/tasks/{id}/triggers",
            get(list_triggers).post(create_trigger),
        )
        .route(
            "/api/v1/admin/tasks/{id}/triggers/{trigger}",
            delete(delete_trigger),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}

type SystemRow = (
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<i64>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Display state from the durable runs of a task.
fn active_state<'a>(statuses: impl Iterator<Item = &'a str>) -> &'static str {
    let mut state = "idle";
    for status in statuses {
        match status {
            "running" => return "running",
            "queued" | "paused" => state = "queued",
            _ => {}
        }
    }
    state
}

fn last_run(
    start: Option<i64>,
    end: Option<i64>,
    outcome: &str,
    message: Option<String>,
    kind: Option<String>,
    scope: Option<String>,
) -> Option<LastRun> {
    let outcome = match outcome {
        // No completed run is known (never run, or a legacy "queued" marker).
        "never" | "" | "queued" | "dispatching" => return None,
        // A "running" marker without an active run: the server stopped during that run.
        "running" => "interrupted",
        "completed" => "succeeded",
        "error" => "failed",
        other => other,
    };
    Some(LastRun {
        start,
        end,
        outcome: outcome.to_owned(),
        message,
        error_kind: kind,
        error_scope: scope,
    })
}

/// One active durable run: (category, target, scope, owner, status, progress).
type ActiveRow = (String, String, String, String, String, i64);

/// Build every task view (or one task when `only` is set) from current database state.
async fn collect(
    store: &IdentityStore,
    only: Option<&TaskRef>,
) -> Result<Vec<TaskView>, sqlx::Error> {
    let pool = store.pool();
    let triggers: Vec<TriggerRow> = query_as(SELECT_TRIGGERS).fetch_all(pool).await?;
    let triggers_of = |kind: &str, id: &str| -> Vec<TriggerView> {
        triggers
            .iter()
            .filter(|t| t.task_kind == kind && t.task_id == id)
            .map(TriggerView::from)
            .collect()
    };
    let next_of = |kind: &str, id: &str| -> Option<i64> {
        triggers
            .iter()
            .filter(|t| t.task_kind == kind && t.task_id == id && t.timed())
            .filter_map(|t| t.next_fire_at)
            .min()
    };
    let active: Vec<ActiveRow> = query_as("SELECT category,COALESCE(target_id,''),COALESCE(library_id,''),owner,status,progress FROM durable_tasks WHERE status IN ('queued','running','paused') ORDER BY created_at")
        .fetch_all(pool).await?;
    let mut tasks = Vec::new();

    let wants = |candidate: &TaskRef| only.is_none_or(|only| only == candidate);
    let system_runs = |target: &str| -> Vec<&ActiveRow> {
        active
            .iter()
            .filter(|(category, t, ..)| category == "system" && t == target)
            .collect()
    };
    let progress_of = |runs: &[&ActiveRow]| -> Option<i64> {
        runs.iter()
            .find(|(.., status, _)| status == "running")
            .map(|(.., progress)| *progress)
    };

    // System maintenance tasks.
    let systems: Vec<SystemRow> = query_as("SELECT id,name,description,category,last_started_at,last_completed_at,last_outcome,last_error_kind,last_error_scope,last_message FROM system_task_schedules ORDER BY category,name")
        .fetch_all(pool).await?;
    for (id, name, description, category, started, completed, outcome, kind, scope, message) in
        systems
    {
        if !wants(&TaskRef::System(id.clone())) {
            continue;
        }
        let runs = system_runs(&id);
        let state = active_state(runs.iter().map(|(.., status, _)| status.as_str()));
        tasks.push(TaskView {
            id: id.clone(),
            kind: "system",
            name,
            description,
            category,
            state,
            progress: progress_of(&runs).filter(|_| state == "running"),
            next_run_at: next_of("system", &id),
            // While a run is active its outcome is not known yet.
            last_run: if state == "running" {
                None
            } else {
                last_run(started, completed, &outcome, message, kind, scope)
            },
            reason: None,
            library_id: None,
            library_kind: None,
            warnings_count: 0,
            issues: Vec::new(),
            triggers: triggers_of("system", &id),
        });
    }

    // Backup.
    if wants(&TaskRef::Backup) {
        let row: Option<(Option<i64>, Option<i64>, String)> = query_as("SELECT last_started_at,last_completed_at,last_outcome FROM backup_schedule WHERE singleton=1")
            .fetch_optional(pool).await?;
        if let Some((started, completed, outcome)) = row {
            let runs = system_runs("backup");
            let state = active_state(runs.iter().map(|(.., status, _)| status.as_str()));
            let failure: Option<(String, String)> = query_as("SELECT error,result_json FROM durable_tasks WHERE category='system' AND target_id='backup' AND status IN ('queued','completed','error','cancelled') AND (error!='' OR result_json LIKE '%\"message\"%') ORDER BY updated_at DESC,id LIMIT 1")
                .fetch_optional(pool).await?;
            let failure = failure.map(|(error, detail)| {
                serde_json::from_str::<serde_json::Value>(&detail)
                    .ok()
                    .and_then(|v| v.get("message")?.as_str().map(str::to_owned))
                    .unwrap_or(error)
            });
            tasks.push(TaskView {
                id: "backup".into(),
                kind: "backup",
                name: "Sauvegarder le serveur".into(),
                description: "Crée une archive non chiffrée de la base et de la configuration du serveur. Les fichiers médias ne sont pas inclus.".into(),
                category: "Maintenance".into(),
                state,
                progress: progress_of(&runs).filter(|_| state == "running"),
                next_run_at: next_of("backup", "backup"),
                last_run: if state == "running" {
                    None
                } else {
                    let message = matches!(outcome.as_str(), "failed" | "interrupted" | "running")
                        .then(|| failure.filter(|e| !e.is_empty()))
                        .flatten();
                    last_run(started, completed, &outcome, message, None, None)
                },
                reason: None,
                library_id: None,
                library_kind: None,
                warnings_count: 0,
                issues: Vec::new(),
                triggers: triggers_of("backup", "backup"),
            });
        }
    }

    // Library scans: one task per visible library (physical folders are its scopes).
    let libraries: Vec<(String, String, String, i64, String, String)> = query_as("SELECT l.id,l.name,l.kind,l.enabled,l.node_id,n.status FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.kind IN ('films','series','music') AND l.id NOT IN ('fixture-films','fixture-series') AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id) ORDER BY l.name,l.id")
        .fetch_all(pool).await?;
    for (library, name, library_kind, enabled, node, node_status) in libraries {
        if !wants(&TaskRef::Scan(library.clone())) {
            continue;
        }
        let scopes: Vec<String> =
            query_scalar("SELECT library_id FROM library_folders WHERE parent_id=?")
                .bind(&library)
                .fetch_all(pool)
                .await?;
        let in_scope = |scope: &str| scope == library || scopes.iter().any(|s| s == scope);
        // Real-time monitoring polls are background housekeeping, not runs of this task.
        let current: Vec<&ActiveRow> = active
            .iter()
            .filter(|(category, _, scope, owner, ..)| {
                category == "scan" && owner != "library-monitor" && in_scope(scope)
            })
            .collect();
        let state = active_state(current.iter().map(|(.., status, _)| status.as_str()));
        let progress = (state == "running")
            .then(|| current.iter().map(|(.., p)| *p).sum::<i64>() / current.len().max(1) as i64);
        // The most recent finished run over every scope of the library.
        let last: Option<(Option<i64>, Option<i64>, String, String)> = query_as("SELECT started_at,finished_at,status,error FROM durable_tasks WHERE category='scan' AND owner<>'library-monitor' AND (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?)) AND status IN ('completed','error','cancelled') ORDER BY COALESCE(finished_at,updated_at) DESC,id LIMIT 1")
            .bind(&library).bind(&library).fetch_optional(pool).await?;
        let job: Option<(String, String, i64, i64)> = query_as("SELECT status,errors_json,errors_count,warnings_count FROM scan_jobs WHERE library_id=?")
            .bind(&library).fetch_optional(pool).await?;
        let issues: Vec<String> = job
            .as_ref()
            .and_then(|(_, json, ..)| serde_json::from_str::<Vec<String>>(json).ok())
            .unwrap_or_default()
            .into_iter()
            .take(20)
            .collect();
        let warnings_count = job.as_ref().map_or(0, |(.., warnings)| *warnings);
        let last_run = match last {
            Some((start, end, status, error)) => {
                let message = if status == "error" {
                    // Scan errors are stored as the JSON list of scan issues.
                    let error = serde_json::from_str::<Vec<String>>(&error)
                        .ok()
                        .and_then(|list| list.into_iter().next())
                        .unwrap_or(error);
                    Some(if error.is_empty() {
                        issues.first().cloned().unwrap_or_default()
                    } else {
                        error
                    })
                    .filter(|m| !m.is_empty())
                } else if warnings_count > 0 {
                    Some(format!("{warnings_count} avertissement(s)"))
                } else {
                    None
                };
                last_run(start, end, &status, message, None, None)
            }
            None => {
                let legacy: Option<(Option<i64>, String)> = query_as(
                    "SELECT last_run_at,last_outcome FROM scheduled_scans WHERE library_id=?",
                )
                .bind(&library)
                .fetch_optional(pool)
                .await?;
                legacy.and_then(|(start, outcome)| {
                    if matches!(outcome.as_str(), "queued" | "running" | "dispatching") {
                        None
                    } else {
                        last_run(start, None, &outcome, None, None, None)
                    }
                })
            }
        };
        let reason = if enabled != 1 {
            Some("Bibliothèque désactivée.".to_owned())
        } else if node != "local" && node_status != "connected" {
            Some("Media Node indisponible.".to_owned())
        } else {
            None
        };
        tasks.push(TaskView {
            id: format!("scan-{library}"),
            kind: "scan",
            description: match library_kind.as_str() {
                "series" => "Analyse les fichiers de séries et actualise le catalogue. Les médias restent en lecture seule.",
                "music" => "Analyse les fichiers de musique et actualise le catalogue. Les médias restent en lecture seule.",
                _ => "Analyse les fichiers de films et actualise le catalogue. Les médias restent en lecture seule.",
            }
            .into(),
            name: format!("Analyser {name}"),
            category: "Bibliothèques".into(),
            state,
            progress,
            next_run_at: next_of("scan", &library),
            last_run,
            reason,
            library_id: Some(library.clone()),
            library_kind: Some(library_kind),
            warnings_count,
            issues,
            triggers: triggers_of("scan", &library),
        });
    }
    Ok(tasks)
}

async fn find(store: &IdentityStore, id: &str) -> Result<(TaskRef, TaskView), ApiError> {
    let task = TaskRef::parse(id);
    let mut views = collect(store, Some(&task)).await.map_err(internal)?;
    views
        .pop()
        .map(|view| (task, view))
        .ok_or(ApiError::coded(StatusCode::NOT_FOUND, "task_not_found"))
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TaskView>>, ApiError> {
    authorize(&auth, &headers, false).await?;
    collect(&auth.store, None).await.map(Json).map_err(internal)
}

async fn detail(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<TaskView>, ApiError> {
    authorize(&auth, &headers, false).await?;
    find(&auth.store, &id).await.map(|(_, view)| Json(view))
}

async fn run(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let actor = authorize(&auth, &headers, true).await?;
    let (task, view) = find(&auth.store, &id).await?;
    if view.state != "idle" {
        return Err(ApiError::coded(StatusCode::CONFLICT, "task_already_active"));
    }
    if view.reason.is_some() {
        return Err(ApiError::coded(StatusCode::CONFLICT, "task_unavailable"));
    }
    let timestamp = now();
    let admission = match &task {
        TaskRef::System(id) => {
            crate::system_task_queue::enqueue_manual(&auth.store, id, &actor, timestamp)
                .await
                .map_err(internal)?;
            Admission::Queued
        }
        TaskRef::Backup => crate::task_scheduler::admit(
            &auth,
            &task,
            &format!("manual:{}", uuid::Uuid::new_v4()),
            timestamp,
            None,
        )
        .await
        .map_err(internal)?,
        TaskRef::Scan(library) => crate::task_scheduler::admit(
            &auth,
            &task,
            &format!("manual:{library}:{}", uuid::Uuid::new_v4()),
            timestamp,
            None,
        )
        .await
        .map_err(internal)?,
    };
    match admission {
        Admission::Queued => {
            crate::task_engine::WAKE.notify_one();
            Ok(StatusCode::ACCEPTED)
        }
        Admission::AlreadyActive => {
            Err(ApiError::coded(StatusCode::CONFLICT, "task_already_active"))
        }
        Admission::Unavailable => Err(ApiError::coded(StatusCode::CONFLICT, "task_unavailable")),
    }
}

async fn cancel(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authorize(&auth, &headers, true).await?;
    let (task, _) = find(&auth.store, &id).await?;
    let ids: Vec<String> = match &task {
        TaskRef::System(target) => query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND target_id=? AND status IN ('queued','running','paused')")
            .bind(target).fetch_all(auth.store.pool()).await,
        TaskRef::Backup => query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND target_id='backup' AND status IN ('queued','running','paused')")
            .fetch_all(auth.store.pool()).await,
        TaskRef::Scan(library) => query_scalar("SELECT id FROM durable_tasks WHERE category='scan' AND owner<>'library-monitor' AND (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?)) AND status IN ('queued','running','paused')")
            .bind(library).bind(library).fetch_all(auth.store.pool()).await,
    }
    .map_err(internal)?;
    let mut stopped = false;
    for id in ids {
        stopped |= crate::task_engine::control(&auth.store, &id, "cancel", now())
            .await
            .map_err(internal)?;
    }
    if stopped {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::coded(StatusCode::CONFLICT, "task_not_active"))
    }
}

async fn list_triggers(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<TriggerView>>, ApiError> {
    authorize(&auth, &headers, false).await?;
    let (_, view) = find(&auth.store, &id).await?;
    Ok(Json(view.triggers))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TriggerInput {
    #[serde(rename = "type")]
    trigger_type: TriggerType,
    time_of_day_minutes: Option<i64>,
    day_of_week: Option<i64>,
    interval_minutes: Option<i64>,
    max_runtime_seconds: Option<i64>,
}

async fn create_trigger(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<TriggerInput>,
) -> Result<(StatusCode, Json<TriggerView>), ApiError> {
    authorize(&auth, &headers, true).await?;
    let (task, _) = find(&auth.store, &id).await?;
    let invalid = || ApiError::coded(StatusCode::UNPROCESSABLE_ENTITY, "trigger_invalid");
    let spec = TriggerSpec {
        kind: input.trigger_type,
        time_of_day_minutes: input.time_of_day_minutes,
        day_of_week: input.day_of_week,
        interval_minutes: input.interval_minutes,
    }
    .normalized(task.min_interval())
    .map_err(|_| invalid())?;
    if input
        .max_runtime_seconds
        .is_some_and(|limit| !(60..=604_800).contains(&limit))
    {
        return Err(invalid());
    }
    let timestamp = now();
    let trigger_id = uuid::Uuid::new_v4().to_string();
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth.store.pool().acquire().await.map_err(internal)?;
    let mut tx = begin_write(&mut conn).await.map_err(internal)?;
    let (kind, target) = (task.kind(), task.target());
    let existing = load(&mut tx, kind, target).await.map_err(internal)?;
    if existing.len() as i64 >= MAX_TRIGGERS_PER_TASK {
        return Err(ApiError::coded(StatusCode::CONFLICT, "trigger_limit"));
    }
    let duplicate = existing.iter().any(|row| {
        row.spec() == Some(spec) && row.max_runtime_seconds == input.max_runtime_seconds
    });
    if duplicate {
        return Err(ApiError::coded(StatusCode::CONFLICT, "trigger_duplicate"));
    }
    query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,time_of_day_minutes,day_of_week,interval_minutes,max_runtime_seconds,enabled,next_fire_at,created_at) VALUES(?,?,?,?,?,?,?,?,1,?,?)")
        .bind(&trigger_id).bind(kind).bind(target).bind(spec.kind.as_str())
        .bind(spec.time_of_day_minutes).bind(spec.day_of_week).bind(spec.interval_minutes)
        .bind(input.max_runtime_seconds).bind(next_fire(&spec, timestamp, &server_time_zone())).bind(timestamp)
        .execute(&mut *tx).await.map_err(internal)?;
    crate::task_scheduler::mirror(&mut tx, kind, target)
        .await
        .map_err(internal)?;
    let created = load(&mut tx, kind, target)
        .await
        .map_err(internal)?
        .into_iter()
        .find(|row| row.id == trigger_id)
        .ok_or_else(|| ApiError::from(StatusCode::INTERNAL_SERVER_ERROR))?;
    commit(tx).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(TriggerView::from(&created))))
}

async fn delete_trigger(
    State(auth): State<AuthState>,
    Path((id, trigger)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authorize(&auth, &headers, true).await?;
    let (task, _) = find(&auth.store, &id).await?;
    let (kind, target) = (task.kind(), task.target());
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth.store.pool().acquire().await.map_err(internal)?;
    let mut tx = begin_write(&mut conn).await.map_err(internal)?;
    let removed = query("DELETE FROM task_triggers WHERE id=? AND task_kind=? AND task_id=?")
        .bind(&trigger)
        .bind(kind)
        .bind(target)
        .execute(&mut *tx)
        .await
        .map_err(internal)?
        .rows_affected();
    if removed == 0 {
        return Err(ApiError::coded(StatusCode::NOT_FOUND, "trigger_not_found"));
    }
    crate::task_scheduler::mirror(&mut tx, kind, target)
        .await
        .map_err(internal)?;
    commit(tx).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_rejects_out_of_range_fields() {
        assert!(TriggerSpec::daily(1440).normalized(5).is_err());
        assert!(TriggerSpec::weekly(7, 60).normalized(5).is_err());
        assert!(TriggerSpec::interval(30).normalized(60).is_err());
        assert_eq!(
            TriggerSpec {
                kind: TriggerType::Startup,
                time_of_day_minutes: Some(3),
                day_of_week: Some(1),
                interval_minutes: Some(10),
            }
            .normalized(5),
            Ok(TriggerSpec::startup())
        );
    }

    #[test]
    fn display_state_prefers_the_running_run() {
        assert_eq!(active_state(["queued", "running"].into_iter()), "running");
        assert_eq!(active_state(["paused"].into_iter()), "queued");
        assert_eq!(active_state(std::iter::empty()), "idle");
    }

    #[test]
    fn stale_markers_never_hide_the_last_run() {
        assert!(last_run(Some(1), Some(2), "queued", None, None, None).is_none());
        let interrupted = last_run(Some(1), None, "running", None, None, None).unwrap();
        assert_eq!(interrupted.outcome, "interrupted");
        assert_eq!(
            last_run(Some(1), Some(2), "completed", None, None, None)
                .unwrap()
                .outcome,
            "succeeded"
        );
    }
}
