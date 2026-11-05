//! Jellyfin-style scheduled-task triggers (daily, weekly, interval, server startup) shared by
//! system tasks, library scans and the backup, plus the unified `/api/v1/admin/tasks` API.
//!
//! Triggers are the configuration; the legacy `next_due_at` / `next_run_at` / `enabled` columns
//! stay the dispatch cache ("earliest next run") read by the existing schedulers, so every
//! runner, lease and fencing rule is unchanged.
use crate::{
    auth::AuthState,
    database::{begin_write, commit, query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
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
    fn spec(&self) -> Option<TriggerSpec> {
        Some(TriggerSpec {
            kind: TriggerType::parse(&self.trigger_type)?,
            time_of_day_minutes: self.time_of_day_minutes,
            day_of_week: self.day_of_week,
            interval_minutes: self.interval_minutes,
        })
    }
    fn timed(&self) -> bool {
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

/// Result of a task firing: its new earliest next run and the time limit of this run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Advance {
    pub next: Option<i64>,
    pub max_runtime_seconds: Option<i64>,
}

/// The task fired at `now`: every timed trigger is recomputed from `now`, and the time limit of
/// this run is the largest one among the triggers that were due. A task without any trigger row
/// keeps its legacy interval (`legacy_interval_minutes`), so pre-trigger rows behave as before.
pub async fn advance(
    conn: &mut AnyConnection,
    kind: &str,
    id: &str,
    now: i64,
    legacy_interval_minutes: i64,
) -> Result<Advance, sqlx::Error> {
    let rows = load(conn, kind, id).await?;
    if rows.is_empty() {
        return Ok(Advance {
            next: Some(now.saturating_add(legacy_interval_minutes.max(1).saturating_mul(60))),
            max_runtime_seconds: None,
        });
    }
    let zone = server_time_zone();
    let mut advance = Advance::default();
    for row in rows.iter().filter(|row| row.timed()) {
        let Some(spec) = row.spec() else { continue };
        if row.next_fire_at.is_none_or(|at| at <= now)
            && let Some(limit) = row.max_runtime_seconds
        {
            advance.max_runtime_seconds =
                Some(advance.max_runtime_seconds.map_or(limit, |l| l.max(limit)));
        }
        let next = next_fire(&spec, now, &zone);
        query("UPDATE task_triggers SET next_fire_at=? WHERE id=?")
            .bind(next)
            .bind(&row.id)
            .execute(&mut *conn)
            .await?;
        advance.next = match (advance.next, next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    Ok(advance)
}

/// After a trigger edit: fill missing next instants and write the legacy dispatch cache.
pub async fn sync(
    conn: &mut AnyConnection,
    kind: &str,
    id: &str,
    now: i64,
) -> Result<Option<i64>, sqlx::Error> {
    let rows = load(conn, kind, id).await?;
    let zone = server_time_zone();
    let mut earliest: Option<i64> = None;
    let mut first_interval = None;
    for row in rows.iter().filter(|row| row.timed()) {
        let Some(spec) = row.spec() else { continue };
        let next = match row.next_fire_at {
            Some(at) if at > now => Some(at),
            _ => {
                let next = next_fire(&spec, now, &zone);
                query("UPDATE task_triggers SET next_fire_at=? WHERE id=?")
                    .bind(next)
                    .bind(&row.id)
                    .execute(&mut *conn)
                    .await?;
                next
            }
        };
        if first_interval.is_none() {
            first_interval = spec.interval_minutes;
        }
        earliest = match (earliest, next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    let enabled = i64::from(earliest.is_some());
    match kind {
        "system" => {
            query("UPDATE system_task_schedules SET enabled=?,next_due_at=COALESCE(?,next_due_at),interval_minutes=COALESCE(?,interval_minutes) WHERE id=?")
                .bind(enabled).bind(earliest).bind(first_interval.filter(|m| (5..=10080).contains(m))).bind(id)
                .execute(&mut *conn).await?;
        }
        "backup" => {
            query("UPDATE backup_schedule SET enabled=?,next_due_at=COALESCE(?,next_due_at),interval_minutes=COALESCE(?,interval_minutes) WHERE singleton=1")
                .bind(enabled).bind(earliest).bind(first_interval.filter(|m| (60..=10080).contains(m)))
                .execute(&mut *conn).await?;
        }
        "scan" => {
            if let Some(next) = earliest {
                query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at) VALUES(?,?,1,?) ON CONFLICT(library_id) DO UPDATE SET enabled=1,next_run_at=excluded.next_run_at,interval_minutes=excluded.interval_minutes")
                    .bind(id).bind(first_interval.unwrap_or(1440).clamp(5, 10080)).bind(next)
                    .execute(&mut *conn).await?;
            } else {
                query("UPDATE scheduled_scans SET enabled=0 WHERE library_id=?")
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
        _ => {}
    }
    mark_migrated(conn, kind, id, now).await?;
    Ok(earliest)
}

async fn mark_migrated(
    conn: &mut AnyConnection,
    kind: &str,
    id: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    query("INSERT INTO task_trigger_migrations(task_kind,task_id,migrated_at) VALUES(?,?,?) ON CONFLICT(task_kind,task_id) DO NOTHING")
        .bind(kind).bind(id).bind(now).execute(conn).await?;
    Ok(())
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
    mark_migrated(conn, kind, id, now).await
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

/// Enqueue, once per process start, every task carrying an enabled "server startup" trigger.
pub async fn fire_startup(auth: &AuthState, now: i64) -> Result<usize, sqlx::Error> {
    let rows: Vec<(String, String, Option<i64>)> = query_as("SELECT task_kind,task_id,MAX(max_runtime_seconds) FROM task_triggers WHERE trigger_type='startup' AND enabled=1 GROUP BY task_kind,task_id ORDER BY task_kind,task_id")
        .fetch_all(auth.store.pool()).await?;
    let mut fired = 0;
    for (kind, id, limit) in rows {
        let key = format!("startup:{now}");
        let result = match kind.as_str() {
            "system" => {
                crate::system_task_queue::enqueue_triggered(&auth.store, &id, &key, now, limit)
                    .await
                    .map(|_| ())
            }
            "backup" => crate::admin_backups::trigger_backup(&auth.store, &key, now, limit)
                .await
                .map(|_| ()),
            "scan" => {
                crate::scheduler::queue_library_scan(auth, &id, &format!("startup:{id}:{now}"), now)
                    .await
            }
            _ => Ok(()),
        };
        match result {
            Ok(()) => fired += 1,
            Err(error) => eprintln!("startup trigger for {kind}:{id} failed: {error}"),
        }
    }
    Ok(fired)
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum TaskRef {
    System(String),
    Backup,
    Scan(String),
}

impl TaskRef {
    fn parse(id: &str) -> Self {
        if id == "backup" {
            Self::Backup
        } else if let Some(library) = id.strip_prefix("scan-") {
            Self::Scan(library.to_owned())
        } else {
            Self::System(id.to_owned())
        }
    }
    fn kind(&self) -> &'static str {
        match self {
            Self::System(_) => "system",
            Self::Backup => "backup",
            Self::Scan(_) => "scan",
        }
    }
    fn target(&self) -> &str {
        match self {
            Self::System(id) | Self::Scan(id) => id,
            Self::Backup => "backup",
        }
    }
}

fn now() -> i64 {
    media_contracts::protocol::unix_now()
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

fn internal<E: std::fmt::Display>(error: E) -> StatusCode {
    eprintln!("task API: {error}");
    StatusCode::INTERNAL_SERVER_ERROR
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

fn active_state(status: Option<&str>) -> &'static str {
    match status {
        Some("running") => "running",
        Some("queued" | "paused") => "queued",
        _ => "idle",
    }
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
        "never" | "" => return None,
        // `queued` only reflects admission; the previous run is not known separately.
        "queued" | "dispatching" if start.is_none() => return None,
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
    let active: Vec<(String, String, String, String, i64)> = query_as("SELECT category,target_id,COALESCE(library_id,''),status,progress FROM durable_tasks WHERE status IN ('queued','running','paused') ORDER BY created_at")
        .fetch_all(pool).await?;
    let mut tasks = Vec::new();

    let wants = |candidate: &TaskRef| only.is_none_or(|only| only == candidate);

    // System maintenance tasks.
    let systems: Vec<SystemRow> = query_as("SELECT id,name,description,category,last_started_at,last_completed_at,last_outcome,last_error_kind,last_error_scope,last_message FROM system_task_schedules ORDER BY category,name")
        .fetch_all(pool).await?;
    for (id, name, description, category, started, completed, outcome, kind, scope, message) in
        systems
    {
        if !wants(&TaskRef::System(id.clone())) {
            continue;
        }
        let current = active
            .iter()
            .filter(|(c, t, ..)| c == "system" && t == &id)
            .max_by_key(|(.., status, _)| status == "running");
        let mut state = active_state(current.map(|(.., s, _)| s.as_str()));
        if state == "idle" && outcome == "running" {
            state = "running";
        }
        tasks.push(TaskView {
            id: id.clone(),
            kind: "system",
            name,
            description,
            category,
            state,
            progress: current.filter(|_| state == "running").map(|(.., p)| *p),
            next_run_at: next_of("system", &id),
            last_run: if outcome == "running" || outcome == "queued" {
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
            let current = active
                .iter()
                .find(|(c, t, ..)| c == "system" && t == "backup");
            let mut state = active_state(current.map(|(.., s, _)| s.as_str()));
            if state == "idle" && outcome == "running" {
                state = "running";
            }
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
                progress: current.filter(|_| state == "running").map(|(.., p)| *p),
                next_run_at: next_of("backup", "backup"),
                last_run: if outcome == "running" {
                    None
                } else {
                    let message = (outcome == "failed" || outcome == "interrupted")
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
        let current: Vec<_> = active
            .iter()
            .filter(|(c, _, scope, ..)| c == "scan" && in_scope(scope))
            .collect();
        let state = if current.iter().any(|(.., s, _)| s == "running") {
            "running"
        } else if current.is_empty() {
            "idle"
        } else {
            "queued"
        };
        let progress = (state == "running")
            .then(|| current.iter().map(|(.., p)| *p).sum::<i64>() / current.len().max(1) as i64);
        let last: Option<(Option<i64>, Option<i64>, String, String)> = query_as("SELECT started_at,finished_at,status,error FROM durable_tasks WHERE category='scan' AND library_id=? AND status IN ('completed','error','cancelled') ORDER BY COALESCE(finished_at,updated_at) DESC,id LIMIT 1")
            .bind(&library).fetch_optional(pool).await?;
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

async fn find(store: &IdentityStore, id: &str) -> Result<(TaskRef, TaskView), StatusCode> {
    let task = TaskRef::parse(id);
    let mut views = collect(store, Some(&task)).await.map_err(internal)?;
    views
        .pop()
        .map(|view| (task, view))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TaskView>>, StatusCode> {
    authorize(&auth, &headers, false).await?;
    collect(&auth.store, None).await.map(Json).map_err(internal)
}

async fn detail(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<TaskView>, StatusCode> {
    authorize(&auth, &headers, false).await?;
    find(&auth.store, &id).await.map(|(_, view)| Json(view))
}

async fn run(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    let actor = authorize(&auth, &headers, true).await?;
    let (task, view) = find(&auth.store, &id).await?;
    if view.state != "idle" {
        return Err(StatusCode::CONFLICT);
    }
    if view.reason.is_some() {
        return Err(StatusCode::CONFLICT);
    }
    let timestamp = now();
    match &task {
        TaskRef::System(id) => {
            crate::system_task_queue::enqueue_manual(&auth.store, id, &actor, timestamp)
                .await
                .map_err(internal)?;
        }
        TaskRef::Backup => {
            let key = format!("manual:{}", uuid::Uuid::new_v4());
            if !crate::admin_backups::trigger_backup(&auth.store, &key, timestamp, None)
                .await
                .map_err(internal)?
            {
                return Err(StatusCode::CONFLICT);
            }
        }
        TaskRef::Scan(library) => {
            let key = format!("manual:{library}:{}", uuid::Uuid::new_v4());
            crate::scheduler::queue_library_scan(&auth, library, &key, timestamp)
                .await
                .map_err(internal)?;
        }
    }
    crate::task_engine::WAKE.notify_one();
    Ok(StatusCode::ACCEPTED)
}

async fn cancel(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&auth, &headers, true).await?;
    let (task, _) = find(&auth.store, &id).await?;
    let ids: Vec<String> = match &task {
        TaskRef::System(target) => query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND target_id=? AND status IN ('queued','running','paused')")
            .bind(target).fetch_all(auth.store.pool()).await,
        TaskRef::Backup => query_scalar("SELECT id FROM durable_tasks WHERE category='system' AND target_id='backup' AND status IN ('queued','running','paused')")
            .fetch_all(auth.store.pool()).await,
        TaskRef::Scan(library) => query_scalar("SELECT id FROM durable_tasks WHERE category='scan' AND (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?)) AND status IN ('queued','running','paused')")
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
        Err(StatusCode::CONFLICT)
    }
}

async fn list_triggers(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<TriggerView>>, StatusCode> {
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
) -> Result<(StatusCode, Json<TriggerView>), StatusCode> {
    authorize(&auth, &headers, true).await?;
    let (task, _) = find(&auth.store, &id).await?;
    let min_interval = if task == TaskRef::Backup { 60 } else { 5 };
    let spec = TriggerSpec {
        kind: input.trigger_type,
        time_of_day_minutes: input.time_of_day_minutes,
        day_of_week: input.day_of_week,
        interval_minutes: input.interval_minutes,
    }
    .normalized(min_interval)
    .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if input
        .max_runtime_seconds
        .is_some_and(|limit| !(60..=604_800).contains(&limit))
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let timestamp = now();
    let trigger_id = uuid::Uuid::new_v4().to_string();
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth.store.pool().acquire().await.map_err(internal)?;
    let mut tx = begin_write(&mut conn).await.map_err(internal)?;
    let (kind, target) = (task.kind(), task.target());
    let existing = load(&mut tx, kind, target).await.map_err(internal)?;
    if existing.len() as i64 >= MAX_TRIGGERS_PER_TASK {
        return Err(StatusCode::CONFLICT);
    }
    let duplicate = existing.iter().any(|row| {
        row.spec() == Some(spec) && row.max_runtime_seconds == input.max_runtime_seconds
    });
    if duplicate {
        return Err(StatusCode::CONFLICT);
    }
    query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,time_of_day_minutes,day_of_week,interval_minutes,max_runtime_seconds,enabled,next_fire_at,created_at) VALUES(?,?,?,?,?,?,?,?,1,?,?)")
        .bind(&trigger_id).bind(kind).bind(target).bind(spec.kind.as_str())
        .bind(spec.time_of_day_minutes).bind(spec.day_of_week).bind(spec.interval_minutes)
        .bind(input.max_runtime_seconds).bind(next_fire(&spec, timestamp, &server_time_zone())).bind(timestamp)
        .execute(&mut *tx).await.map_err(internal)?;
    sync(&mut tx, kind, target, timestamp)
        .await
        .map_err(internal)?;
    let created = load(&mut tx, kind, target)
        .await
        .map_err(internal)?
        .into_iter()
        .find(|row| row.id == trigger_id)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    commit(tx).await.map_err(internal)?;
    Ok((StatusCode::CREATED, Json(TriggerView::from(&created))))
}

async fn delete_trigger(
    State(auth): State<AuthState>,
    Path((id, trigger)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
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
        return Err(StatusCode::NOT_FOUND);
    }
    sync(&mut tx, kind, target, now()).await.map_err(internal)?;
    commit(tx).await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_references_round_trip() {
        assert_eq!(TaskRef::parse("backup"), TaskRef::Backup);
        assert_eq!(TaskRef::parse("scan-lib-1"), TaskRef::Scan("lib-1".into()));
        assert_eq!(
            TaskRef::parse("clean-cache"),
            TaskRef::System("clean-cache".into())
        );
        assert_eq!(TaskRef::parse("scan-lib-1").target(), "lib-1");
    }

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
}
