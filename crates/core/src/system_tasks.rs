//! Scheduled system maintenance & plugin management tasks.
use crate::{auth::AuthState, identity::Role};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SystemTaskSchedule {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub interval_minutes: i64,
    pub enabled: i64,
    pub next_due_at: i64,
    pub last_started_at: Option<i64>,
    pub last_completed_at: Option<i64>,
    pub last_outcome: String,
    pub last_error_kind: Option<String>,
    pub last_error_scope: Option<String>,
    pub last_message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateScheduleInput {
    pub interval_minutes: i64,
    pub enabled: bool,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Stable failure classes persisted for administrators: `database`, `network`, `format`, `disk`, `config`.
#[derive(Debug, Clone)]
pub struct TaskFailure {
    pub kind: &'static str,
    pub scope: String,
    pub message: String,
}

impl TaskFailure {
    fn new(kind: &'static str, scope: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            scope: scope.into(),
            message: message.into(),
        }
    }
    fn database(scope: &str, error: sqlx::Error) -> Self {
        Self::new("database", scope, truncate(&error.to_string(), 200))
    }
    fn disk(scope: &str, error: &std::io::Error) -> Self {
        Self::new("disk", scope, format!("I/O error: {:?}", error.kind()))
    }
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Result of one task: informational summary plus every non-fatal failure, so none is silent.
#[derive(Debug, Default)]
pub struct TaskReport {
    pub summary: String,
    pub failures: Vec<TaskFailure>,
}

/// Name patterns of temporary files that Aether itself creates. Anything else is never touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempPattern {
    /// `.database-*.tmp`, `.backup-*.tmp`, `.restore-*.tmp`, `.media-backup-*.tmp`.
    BackupStaging,
    /// `<32 hex digits>.tmp` written by the image preview cache.
    ImageCache,
}

impl TempPattern {
    fn matches(self, name: &str) -> bool {
        match self {
            Self::BackupStaging => {
                name.ends_with(".tmp")
                    && [".database-", ".backup-", ".restore-", ".media-backup-"]
                        .iter()
                        .any(|prefix| name.starts_with(prefix))
            }
            Self::ImageCache => {
                name.len() == 36
                    && name.ends_with(".tmp")
                    && name.as_bytes()[..32].iter().all(u8::is_ascii_hexdigit)
            }
        }
    }
}

/// A directory explicitly owned by Aether where stale temporary files may be removed.
#[derive(Debug, Clone)]
pub struct OwnedDir {
    pub label: &'static str,
    pub path: PathBuf,
    pub pattern: TempPattern,
}

/// A temporary file must be at least this old: an active writer is never interrupted.
const TEMP_MIN_AGE: Duration = Duration::from_secs(3600);

/// Remove stale Aether temporary files, non-recursively, from one owned directory.
/// Refuses any directory that is, contains, or lies inside the media root, so an original
/// media file can never be reached, whatever the file-system permissions are.
async fn sweep_owned_dir(
    owned: &OwnedDir,
    media_root: &std::path::Path,
    min_age: Duration,
) -> (usize, Vec<TaskFailure>) {
    let mut failures = Vec::new();
    let canonical = match tokio::fs::canonicalize(&owned.path).await {
        Ok(path) => path,
        // An absent cache directory has nothing to clean and is not an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (0, failures),
        Err(e) => {
            failures.push(TaskFailure::disk(owned.label, &e));
            return (0, failures);
        }
    };
    let protected = tokio::fs::canonicalize(media_root)
        .await
        .unwrap_or_else(|_| media_root.to_path_buf());
    if canonical.parent().is_none()
        || canonical.starts_with(&protected)
        || protected.starts_with(&canonical)
    {
        failures.push(TaskFailure::new(
            "config",
            owned.label,
            "owned directory overlaps the media root or is a filesystem root; cleanup refused",
        ));
        return (0, failures);
    }
    let mut entries = match tokio::fs::read_dir(&canonical).await {
        Ok(entries) => entries,
        Err(e) => {
            failures.push(TaskFailure::disk(owned.label, &e));
            return (0, failures);
        }
    };
    let mut removed = 0;
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) => {
                failures.push(TaskFailure::disk(owned.label, &e));
                break;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !owned.pattern.matches(&name) {
            continue;
        }
        let metadata = match tokio::fs::symlink_metadata(entry.path()).await {
            Ok(metadata) => metadata,
            Err(e) => {
                failures.push(TaskFailure::disk(owned.label, &e));
                continue;
            }
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let old_enough = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age >= min_age);
        if !old_enough {
            continue;
        }
        match tokio::fs::remove_file(entry.path()).await {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => failures.push(TaskFailure::disk(owned.label, &e)),
        }
    }
    (removed, failures)
}

pub struct SystemTasksState {
    pub auth: AuthState,
    /// Never modified by any task; only used to refuse overlapping cleanup scopes.
    pub media_root: PathBuf,
    pub owned_dirs: Vec<OwnedDir>,
    pub gate: Arc<Mutex<()>>,
}

/// Directories Aether owns and may clean, resolved from configuration only.
fn default_owned_dirs(media_root: &std::path::Path) -> Vec<OwnedDir> {
    let backups = std::env::var_os("MEDIA_BACKUP_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            crate::identity::configured_path(media_root)
                .ok()
                .and_then(|db| db.parent().map(|parent| parent.join("backups")))
        });
    let images = std::env::var_os("MEDIA_IMAGE_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("media-image-cache"));
    let mut dirs = vec![OwnedDir {
        label: "image-cache",
        path: images,
        pattern: TempPattern::ImageCache,
    }];
    if let Some(path) = backups {
        dirs.push(OwnedDir {
            label: "backup-staging",
            path,
            pattern: TempPattern::BackupStaging,
        });
    }
    dirs
}

pub async fn router(auth: AuthState, media_root: PathBuf) -> Result<Router, sqlx::Error> {
    let state = Arc::new(execution_state(auth, media_root));

    Ok(Router::new()
        .route("/api/v1/admin/tasks/system", get(list_tasks))
        .route(
            "/api/v1/admin/tasks/system/{id}/schedule",
            put(update_schedule),
        )
        .route(
            "/api/v1/admin/tasks/system/{id}/run",
            post(run_task_endpoint),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(state))
}

async fn authorize(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let user = auth.authenticate(headers).await?;
    if user.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &user)?;
    }
    Ok(())
}

macro_rules! select_tasks {
    ($suffix:literal) => {
        concat!(
            "SELECT id, name, description, category, interval_minutes, enabled, next_due_at, last_started_at, last_completed_at, last_outcome, last_error_kind, last_error_scope, last_message FROM system_task_schedules",
            $suffix
        )
    };
}

async fn list_tasks(
    State(state): State<Arc<SystemTasksState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SystemTaskSchedule>>, StatusCode> {
    authorize(&state.auth, &headers, false).await?;
    let tasks: Vec<SystemTaskSchedule> =
        crate::database::query_as(select_tasks!(" ORDER BY category, name"))
            .fetch_all(&state.auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(tasks))
}

async fn update_schedule(
    State(state): State<Arc<SystemTasksState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<UpdateScheduleInput>,
) -> Result<StatusCode, StatusCode> {
    authorize(&state.auth, &headers, true).await?;
    if !(5..=10080).contains(&input.interval_minutes) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let next_due = now() + (input.interval_minutes * 60);
    let changed = crate::database::query(
        "UPDATE system_task_schedules SET interval_minutes=?, enabled=?, next_due_at=? WHERE id=?",
    )
    .bind(input.interval_minutes)
    .bind(if input.enabled { 1 } else { 0 })
    .bind(next_due)
    .bind(&id)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if changed.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn run_task_endpoint(
    State(state): State<Arc<SystemTasksState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&state.auth, &headers, true).await?;

    let exists: i64 =
        crate::database::query_scalar("SELECT COUNT(*) FROM system_task_schedules WHERE id=?")
            .bind(&id)
            .fetch_one(&state.auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    let actor = state.auth.authenticate(&headers).await?;
    crate::system_task_queue::enqueue_manual(&state.auth.store, &id, &actor.user.id, now())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::ACCEPTED)
}

pub(crate) fn execution_state(auth: AuthState, media_root: PathBuf) -> SystemTasksState {
    let owned_dirs = default_owned_dirs(&media_root);
    SystemTasksState {
        auth,
        media_root,
        owned_dirs,
        gate: Arc::new(Mutex::new(())),
    }
}

pub async fn execute_task(state: &SystemTasksState, id: &str) -> Result<(), StatusCode> {
    execute_inner(state, id, None).await
}

pub(crate) async fn execute_leased(
    state: &SystemTasksState,
    id: &str,
    job: &crate::task_engine::Job,
) -> Result<(), StatusCode> {
    execute_inner(state, id, Some(job)).await
}

async fn execute_inner(
    state: &SystemTasksState,
    id: &str,
    job: Option<&crate::task_engine::Job>,
) -> Result<(), StatusCode> {
    if id == "backup" {
        // Scheduled backup: state lives in `backup_schedule`; the lease is the only exclusion.
        let job = job.ok_or(StatusCode::BAD_REQUEST)?;
        return crate::admin_backups::execute_scheduled(
            state.auth.clone(),
            state.media_root.clone(),
            job,
        )
        .await;
    }
    let _guard = state.gate.lock().await;
    let task_id = job.map_or("", |job| job.id.as_str());
    let attempt = job.map_or(0, |job| job.attempt);

    let started = now();
    // Claim task execution
    let claimed = crate::database::query(
        "UPDATE system_task_schedules SET last_started_at=?, last_completed_at=NULL, last_outcome='running', last_error_kind=NULL, last_error_scope=NULL, last_message=NULL WHERE id=? AND (last_outcome!='running' OR (?!='' AND NOT EXISTS(SELECT 1 FROM durable_tasks previous WHERE previous.category='system' AND previous.target_id=? AND previous.id<>? AND previous.status='running' AND previous.lease_until>?))) AND (?='' OR EXISTS(SELECT 1 FROM durable_tasks WHERE id=? AND attempt=? AND status='running' AND lease_until>?))"
    )
    .bind(started)
    .bind(id)
    .bind(task_id).bind(id).bind(task_id).bind(started)
    .bind(task_id).bind(task_id).bind(attempt).bind(started)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if claimed.rows_affected() == 0 {
        return Ok(());
    }

    let result = match id {
        "update-extensions" => task_update_extensions(state).await,
        "optimize-database" => task_optimize_database(state).await,
        "clean-sessions" => task_clean_sessions(state).await,
        "clean-cache" => task_clean_cache(state).await,
        "clean-playlists" => task_clean_playlists(state).await,
        _ => Err(TaskFailure::new("config", id, "unknown system task")),
    };
    let report = match result {
        Ok(report) => report,
        Err(failure) => TaskReport {
            summary: String::new(),
            failures: vec![failure],
        },
    };
    let failed = !report.failures.is_empty();
    let (kind, scope, message) = match report.failures.first() {
        Some(first) => {
            let extra = report.failures.len() - 1;
            let suffix = if extra > 0 {
                format!(" (+{extra} more)")
            } else {
                String::new()
            };
            (
                Some(first.kind),
                Some(first.scope.clone()),
                Some(format!("{}{suffix}", first.message)),
            )
        }
        None => (None, None, Some(report.summary).filter(|m| !m.is_empty())),
    };

    let recorded = crate::database::query(
        "UPDATE system_task_schedules SET last_completed_at=?, last_outcome=?, last_error_kind=?, last_error_scope=?, last_message=? WHERE id=? AND last_started_at=? AND (?='' OR EXISTS(SELECT 1 FROM durable_tasks WHERE id=? AND attempt=? AND status='running' AND lease_until>?))"
    )
    .bind(now())
    .bind(if failed { "failed" } else { "succeeded" })
    .bind(kind)
    .bind(scope)
    .bind(message)
    .bind(id)
    .bind(started)
    .bind(task_id).bind(task_id).bind(attempt).bind(now())
    .execute(&state.auth.store.pool)
    .await;
    if let Err(error) = recorded {
        eprintln!("system task {id}: result could not be persisted: {error}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    if failed {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    } else {
        Ok(())
    }
}

/// `update-extensions`: Refresh extension repositories and catalogs
async fn task_update_extensions(state: &SystemTasksState) -> Result<TaskReport, TaskFailure> {
    let repos: Vec<(String, String)> =
        crate::database::query_as("SELECT id, url FROM extension_repositories ORDER BY id")
            .fetch_all(&state.auth.store.pool)
            .await
            .map_err(|e| TaskFailure::database("extension_repositories", e))?;

    let mut report = TaskReport::default();
    let mut refreshed = 0;
    for (id, url) in repos {
        let scope = format!("repository:{id}");
        let bytes = match crate::extensions::repositories::fetch(&url, 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => {
                report.failures.push(TaskFailure::new(
                    "network",
                    scope,
                    "repository manifest could not be fetched",
                ));
                continue;
            }
        };
        let manifest_str = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                report.failures.push(TaskFailure::new(
                    "format",
                    scope,
                    "repository manifest is not valid UTF-8",
                ));
                continue;
            }
        };
        match crate::database::query(
            "UPDATE extension_repositories SET manifest_json=?, refreshed_at=? WHERE id=?",
        )
        .bind(manifest_str)
        .bind(now())
        .bind(id)
        .execute(&state.auth.store.pool)
        .await
        {
            Ok(_) => refreshed += 1,
            Err(e) => report.failures.push(TaskFailure::database(&scope, e)),
        }
    }
    report.summary = format!("{refreshed} repository manifest(s) refreshed");
    Ok(report)
}

/// `optimize-database`: SQLite PRAGMA optimize / WAL checkpoint or PostgreSQL ANALYZE
async fn task_optimize_database(state: &SystemTasksState) -> Result<TaskReport, TaskFailure> {
    let pool = &state.auth.store.pool;
    let sqlite = pool
        .acquire()
        .await
        .map_err(|e| TaskFailure::database("connection", e))?
        .backend_name()
        == "SQLite";
    let statements: &'static [&'static str] = if sqlite {
        &[
            "PRAGMA optimize",
            "PRAGMA wal_checkpoint(PASSIVE)",
            "PRAGMA shrink_memory",
        ]
    } else {
        &["ANALYZE"]
    };
    let mut report = TaskReport::default();
    for &statement in statements {
        if let Err(e) = crate::database::query(statement).execute(pool).await {
            report.failures.push(TaskFailure::database(statement, e));
        }
    }
    crate::system_memory::trim_memory();
    report.summary = format!("{} statement(s) executed", statements.len());
    Ok(report)
}

/// `clean-sessions`: Delete expired and revoked sessions and tokens
async fn task_clean_sessions(state: &SystemTasksState) -> Result<TaskReport, TaskFailure> {
    let current_time = now();
    let s_settings = crate::server_settings::load(&state.auth.store)
        .await
        .unwrap_or_default();
    let retention_days = s_settings.activity_log_retention_days as i64;
    let inactivity_threshold = current_time - (retention_days * 86400);
    let mut report = TaskReport::default();
    let mut sessions = 0;
    let mut flows = 0;

    match crate::database::query("DELETE FROM sessions WHERE expires_at<=? OR last_seen_at<=?")
        .bind(current_time)
        .bind(inactivity_threshold)
        .execute(&state.auth.store.pool)
        .await
    {
        Ok(done) => sessions = done.rows_affected(),
        Err(e) => report.failures.push(TaskFailure::database("sessions", e)),
    }
    // Flows carry their own expiry; the former `created_at` column never existed.
    match crate::database::query("DELETE FROM oidc_flows WHERE expires_at<=?")
        .bind(current_time)
        .execute(&state.auth.store.pool)
        .await
    {
        Ok(done) => flows = done.rows_affected(),
        Err(e) => report.failures.push(TaskFailure::database("oidc_flows", e)),
    }
    report.summary = format!("{sessions} session(s) and {flows} OIDC flow(s) removed");
    Ok(report)
}

/// `clean-cache`: orphan poster previews and stale temporary files in Aether-owned directories.
/// The media root and any library path are never read or modified by this task.
async fn task_clean_cache(state: &SystemTasksState) -> Result<TaskReport, TaskFailure> {
    let mut report = TaskReport::default();
    let mut previews = 0;
    match crate::database::query(
        "DELETE FROM poster_previews WHERE work_id NOT IN (SELECT work_id FROM work_posters)",
    )
    .execute(&state.auth.store.pool)
    .await
    {
        Ok(done) => previews = done.rows_affected(),
        Err(e) => report
            .failures
            .push(TaskFailure::database("poster_previews", e)),
    }
    let mut files = 0;
    for owned in &state.owned_dirs {
        let (removed, failures) = sweep_owned_dir(owned, &state.media_root, TEMP_MIN_AGE).await;
        files += removed;
        report.failures.extend(failures);
    }
    report.summary =
        format!("{previews} orphan preview(s) and {files} stale temporary file(s) removed");
    Ok(report)
}

/// `clean-playlists`: Remove references in playlists to missing works or deleted music tracks
async fn task_clean_playlists(state: &SystemTasksState) -> Result<TaskReport, TaskFailure> {
    let mut report = TaskReport::default();
    let mut works = 0;
    let mut tracks = 0;
    match crate::database::query(
        "DELETE FROM personal_list_items WHERE work_id NOT IN (SELECT id FROM inventory_works)",
    )
    .execute(&state.auth.store.pool)
    .await
    {
        Ok(done) => works = done.rows_affected(),
        Err(e) => report
            .failures
            .push(TaskFailure::database("personal_list_items", e)),
    }
    match crate::database::query(
        "DELETE FROM playlist_music_items WHERE track_id NOT IN (SELECT id FROM music_tracks)",
    )
    .execute(&state.auth.store.pool)
    .await
    {
        Ok(done) => tracks = done.rows_affected(),
        Err(e) => report
            .failures
            .push(TaskFailure::database("playlist_music_items", e)),
    }
    report.summary = format!("{works} work reference(s) and {tracks} track reference(s) removed");
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;

    async fn state_in(temp: &std::path::Path, owned_dirs: Vec<OwnedDir>) -> SystemTasksState {
        let store = crate::identity::IdentityStore::open(&temp.join("test.sqlite3"))
            .await
            .unwrap();
        let auth = AuthState::new(store, "http://localhost:3000".to_string())
            .await
            .unwrap();
        SystemTasksState {
            auth,
            media_root: temp.join("media"),
            owned_dirs,
            gate: Arc::new(Mutex::new(())),
        }
    }

    async fn schedule(state: &SystemTasksState, id: &str) -> SystemTaskSchedule {
        crate::database::query_as(select_tasks!(" WHERE id=?"))
            .bind(id)
            .fetch_one(&state.auth.store.pool)
            .await
            .unwrap()
    }

    fn old(path: &std::path::Path) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(7200))
            .unwrap();
    }

    #[tokio::test]
    async fn test_system_tasks_seeded_and_claimable() {
        let temp = tempfile::tempdir().unwrap();
        let state = state_in(temp.path(), Vec::new()).await;
        let tasks: Vec<SystemTaskSchedule> =
            crate::database::query_as(select_tasks!(" ORDER BY id"))
                .fetch_all(&state.auth.store.pool)
                .await
                .unwrap();
        assert_eq!(tasks.len(), 5);
        for id in [
            "update-extensions",
            "optimize-database",
            "clean-sessions",
            "clean-cache",
            "clean-playlists",
        ] {
            assert!(tasks.iter().any(|t| t.id == id && t.enabled == 1), "{id}");
        }
        assert!(execute_task(&state, "optimize-database").await.is_ok());
        let updated = schedule(&state, "optimize-database").await;
        assert_eq!(updated.last_outcome, "succeeded");
        assert!(updated.last_completed_at.is_some());
        assert!(updated.last_error_kind.is_none());
    }

    #[tokio::test]
    async fn expired_system_attempt_can_resume_a_running_schedule() {
        let temp = tempfile::tempdir().unwrap();
        let state = state_in(temp.path(), Vec::new()).await;
        let started = now();
        crate::database::query(
            "UPDATE system_task_schedules SET last_outcome='running',last_started_at=? WHERE id='optimize-database'",
        )
        .bind(started - 120)
        .execute(&state.auth.store.pool)
        .await
        .unwrap();
        crate::database::query(
            "INSERT INTO durable_tasks(id,owner,idempotency_key,node_id,category,target_id,status,attempt,created_at,updated_at,available_at,lease_until) VALUES('recovered-system-job','system','recovered-system-job','local','system','optimize-database','running',2,?,?,?,?)",
        )
        .bind(started - 120)
        .bind(started)
        .bind(started)
        .bind(started + 60)
        .execute(&state.auth.store.pool)
        .await
        .unwrap();
        let job = crate::task_engine::get(&state.auth.store, "recovered-system-job")
            .await
            .unwrap()
            .unwrap();
        execute_leased(&state, "optimize-database", &job)
            .await
            .unwrap();
        assert_eq!(
            schedule(&state, "optimize-database").await.last_outcome,
            "succeeded"
        );
    }

    /// Every maintenance statement must actually run: before this fix `clean-playlists` queried the
    /// nonexistent `works` table and `clean-sessions` a nonexistent `oidc_flows.created_at`,
    /// both silently ignored.
    #[tokio::test]
    async fn every_task_executes_without_hidden_sql_errors() {
        let temp = tempfile::tempdir().unwrap();
        let state = state_in(temp.path(), Vec::new()).await;
        for id in [
            "optimize-database",
            "clean-sessions",
            "clean-cache",
            "clean-playlists",
        ] {
            execute_task(&state, id).await.unwrap_or_else(|e| {
                panic!("{id} failed: {e}");
            });
            let row = schedule(&state, id).await;
            assert_eq!(
                row.last_outcome, "succeeded",
                "{id}: {:?}",
                row.last_message
            );
            assert!(row.last_error_kind.is_none(), "{id}");
        }
    }

    #[tokio::test]
    async fn failures_are_reported_with_kind_and_scope() {
        let temp = tempfile::tempdir().unwrap();
        let state = state_in(temp.path(), Vec::new()).await;
        crate::database::query("DROP TABLE playlist_music_items")
            .execute(&state.auth.store.pool)
            .await
            .unwrap();
        assert!(execute_task(&state, "clean-playlists").await.is_err());
        let row = schedule(&state, "clean-playlists").await;
        assert_eq!(row.last_outcome, "failed");
        assert_eq!(row.last_error_kind.as_deref(), Some("database"));
        assert_eq!(
            row.last_error_scope.as_deref(),
            Some("playlist_music_items")
        );
        assert!(row.last_message.is_some());
        // Unknown identifiers are reported too, not silently accepted.
        crate::database::query(
            "INSERT INTO system_task_schedules(id,name,description,category,interval_minutes,enabled,next_due_at) VALUES('bogus','b','b','b',5,0,0)",
        )
        .execute(&state.auth.store.pool)
        .await
        .unwrap();
        assert!(execute_task(&state, "bogus").await.is_err());
        let row = schedule(&state, "bogus").await;
        assert_eq!(row.last_error_kind.as_deref(), Some("config"));
    }

    #[tokio::test]
    async fn clean_cache_removes_only_stale_owned_temporaries_and_never_media() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let cache = temp.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        // Originals that the former implementation could delete.
        let originals = [
            media.join("film.mkv.part"),
            media.join(".hidden.tmp"),
            media.join("download.part"),
        ];
        for path in &originals {
            std::fs::write(path, b"original").unwrap();
            old(path);
        }
        let stale = cache.join(format!("{}.tmp", "ab".repeat(16)));
        let fresh = cache.join(format!("{}.tmp", "cd".repeat(16)));
        let foreign = cache.join("notes.tmp");
        let cached = cache.join(format!("{}.img", "ef".repeat(16)));
        for path in [&stale, &fresh, &foreign, &cached] {
            std::fs::write(path, b"x").unwrap();
        }
        for path in [&stale, &foreign, &cached] {
            old(path);
        }
        let owned = |path: PathBuf| OwnedDir {
            label: "image-cache",
            path,
            pattern: TempPattern::ImageCache,
        };
        let state = state_in(temp.path(), vec![owned(cache.clone())]).await;
        execute_task(&state, "clean-cache").await.unwrap();
        assert!(!stale.exists(), "stale owned temporary must be removed");
        assert!(fresh.exists(), "an active writer's file is kept");
        assert!(foreign.exists(), "unknown names are never removed");
        assert!(cached.exists(), "published cache entries are kept");
        for path in &originals {
            assert!(path.exists(), "{} must never be touched", path.display());
        }
    }

    #[tokio::test]
    async fn owned_directory_overlapping_media_root_is_refused() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let victim = media.join(format!("{}.tmp", "ab".repeat(16)));
        std::fs::write(&victim, b"original").unwrap();
        old(&victim);
        let nested = media.join("cache");
        std::fs::create_dir(&nested).unwrap();
        for path in [media.clone(), nested, temp.path().to_path_buf()] {
            let owned = OwnedDir {
                label: "image-cache",
                path,
                pattern: TempPattern::ImageCache,
            };
            let (removed, failures) = sweep_owned_dir(&owned, &media, Duration::ZERO).await;
            assert_eq!(removed, 0);
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0].kind, "config");
            assert!(victim.exists());
        }
        // A missing directory is not an error and removes nothing.
        let missing = OwnedDir {
            label: "image-cache",
            path: temp.path().join("absent"),
            pattern: TempPattern::ImageCache,
        };
        let (removed, failures) = sweep_owned_dir(&missing, &media, Duration::ZERO).await;
        assert_eq!((removed, failures.len()), (0, 0));
    }

    #[test]
    fn temp_patterns_are_narrow() {
        assert!(TempPattern::BackupStaging.matches(".database-1.tmp"));
        assert!(TempPattern::BackupStaging.matches(".restore-1-2.tmp"));
        assert!(!TempPattern::BackupStaging.matches("movie.part"));
        assert!(!TempPattern::BackupStaging.matches("a.tmp"));
        assert!(!TempPattern::ImageCache.matches("short.tmp"));
        assert!(!TempPattern::ImageCache.matches(&format!("{}.tmp", "zz".repeat(16))));
    }
}
