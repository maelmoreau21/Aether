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
    /// False for deterministic failures (configuration, permissions): retrying cannot succeed,
    /// so the durable task is terminated instead of being replayed with backoff.
    pub retryable: bool,
}

impl TaskFailure {
    fn new(kind: &'static str, scope: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            scope: scope.into(),
            message: message.into(),
            retryable: matches!(kind, "database" | "network" | "disk"),
        }
    }
    fn database(scope: &str, error: sqlx::Error) -> Self {
        Self::new("database", scope, truncate(&error.to_string(), 200))
    }
    fn disk(scope: &str, error: &std::io::Error) -> Self {
        let mut failure = Self::new(
            "disk",
            scope,
            truncate(&format!("I/O error ({:?}): {error}", error.kind()), 200),
        );
        failure.retryable = !io_error_is_deterministic(error);
        failure
    }
}

/// A file that is locked, read-only, denied or already gone is skipped by cleanup, never fatal:
/// Windows reports open/antivirus-held files as access denied (5) or sharing/lock violations
/// (32/33); Unix reports EACCES/EPERM/EBUSY/EROFS.
pub fn io_error_is_skippable(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::PermissionDenied
            | ErrorKind::NotFound
            | ErrorKind::ResourceBusy
            | ErrorKind::ReadOnlyFilesystem
    ) || (cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33)))
}

/// Errors that a retry a few seconds later cannot fix.
pub fn io_error_is_deterministic(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::PermissionDenied
            | ErrorKind::ReadOnlyFilesystem
            | ErrorKind::NotADirectory
            | ErrorKind::IsADirectory
            | ErrorKind::InvalidInput
            | ErrorKind::InvalidFilename
    )
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

/// Published image-cache entries (`<32 or 64 hex digits>.img`) eligible for eviction.
fn is_cached_image(name: &str) -> bool {
    name.strip_suffix(".img").is_some_and(|stem| {
        matches!(stem.len(), 32 | 64) && stem.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

/// Eviction of published entries in a cache directory: oldest first, by age then by total size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheEviction {
    /// Entries older than this are removed (`None`: no age limit).
    pub max_age: Option<Duration>,
    /// Oldest entries are removed until the directory is at most this large (`None`: no cap).
    pub max_bytes: Option<u64>,
}

impl CacheEviction {
    /// `MEDIA_IMAGE_CACHE_MAX_AGE_DAYS` (default 30) and `MEDIA_IMAGE_CACHE_MAX_MB` (default 2048);
    /// `0` disables the corresponding limit.
    pub fn image_cache_from_env() -> Self {
        let read = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(default)
        };
        let days = read("MEDIA_IMAGE_CACHE_MAX_AGE_DAYS", 30);
        let megabytes = read("MEDIA_IMAGE_CACHE_MAX_MB", 2048);
        Self {
            max_age: (days > 0).then(|| Duration::from_secs(days.saturating_mul(86_400))),
            max_bytes: (megabytes > 0).then(|| megabytes.saturating_mul(1024 * 1024)),
        }
    }
}

/// A directory explicitly owned by Aether where stale temporary files may be removed.
#[derive(Debug, Clone)]
pub struct OwnedDir {
    pub label: &'static str,
    pub path: PathBuf,
    pub pattern: TempPattern,
    /// Only the image cache evicts its published `.img` entries.
    pub eviction: Option<CacheEviction>,
}

/// A temporary file must be at least this old: an active writer is never interrupted.
const TEMP_MIN_AGE: Duration = Duration::from_secs(3600);

/// At most this many cache entries are considered for size eviction in one pass (bounded memory).
const MAX_EVICTION_CANDIDATES: usize = 200_000;

/// Counts of one owned-directory sweep. Locked, denied or vanished files are `skipped`, never failures.
#[derive(Debug, Default)]
pub struct SweepOutcome {
    pub removed: usize,
    pub evicted: usize,
    pub skipped: usize,
    /// The directory itself could not be listed (permission denied); logged, not a failure.
    pub unreadable: bool,
    pub failures: Vec<TaskFailure>,
}

/// Remove one file; `Ok(false)` when it is locked/denied/gone and must simply be skipped.
async fn remove_tolerant(path: &std::path::Path) -> Result<bool, std::io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(true),
        Err(error) if io_error_is_skippable(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Remove stale Aether temporary files, non-recursively, from one owned directory.
/// Refuses any directory that is, contains, or lies inside the media root, so an original
/// media file can never be reached, whatever the file-system permissions are.
async fn sweep_owned_dir(
    owned: &OwnedDir,
    media_root: &std::path::Path,
    min_age: Duration,
) -> SweepOutcome {
    let mut outcome = SweepOutcome::default();
    let canonical = match tokio::fs::canonicalize(&owned.path).await {
        Ok(path) => path,
        // An absent cache directory has nothing to clean and is not an error.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return outcome,
        Err(e) if io_error_is_skippable(&e) => {
            eprintln!(
                "clean-cache: {} skipped, directory not accessible: {e}",
                owned.label
            );
            outcome.unreadable = true;
            return outcome;
        }
        Err(e) => {
            outcome.failures.push(TaskFailure::disk(owned.label, &e));
            return outcome;
        }
    };
    let protected = tokio::fs::canonicalize(media_root)
        .await
        .unwrap_or_else(|_| media_root.to_path_buf());
    if canonical.parent().is_none()
        || canonical.starts_with(&protected)
        || protected.starts_with(&canonical)
    {
        outcome.failures.push(TaskFailure::new(
            "config",
            owned.label,
            "owned directory overlaps the media root or is a filesystem root; cleanup refused",
        ));
        return outcome;
    }
    let mut entries = match tokio::fs::read_dir(&canonical).await {
        Ok(entries) => entries,
        Err(e) if io_error_is_skippable(&e) => {
            eprintln!(
                "clean-cache: {} skipped, directory not readable: {e}",
                owned.label
            );
            outcome.unreadable = true;
            return outcome;
        }
        Err(e) => {
            outcome.failures.push(TaskFailure::disk(owned.label, &e));
            return outcome;
        }
    };
    // (modified, size, path) of published cache entries kept after age eviction.
    let mut cached: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(e) if io_error_is_skippable(&e) => {
                outcome.skipped += 1;
                break;
            }
            Err(e) => {
                outcome.failures.push(TaskFailure::disk(owned.label, &e));
                break;
            }
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let temporary = owned.pattern.matches(&name);
        let evictable = owned.eviction.is_some() && is_cached_image(&name);
        if !temporary && !evictable {
            continue;
        }
        let metadata = match tokio::fs::symlink_metadata(entry.path()).await {
            Ok(metadata) => metadata,
            Err(e) if io_error_is_skippable(&e) => {
                outcome.skipped += 1;
                continue;
            }
            Err(e) => {
                outcome.failures.push(TaskFailure::disk(owned.label, &e));
                continue;
            }
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let modified = metadata.modified().ok();
        let age = modified.and_then(|modified| modified.elapsed().ok());
        let expired = if temporary {
            age.is_some_and(|age| age >= min_age)
        } else {
            owned
                .eviction
                .and_then(|eviction| eviction.max_age)
                .is_some_and(|max_age| age.is_some_and(|age| age >= max_age))
        };
        if !expired {
            if evictable && cached.len() < MAX_EVICTION_CANDIDATES {
                cached.push((
                    modified.unwrap_or(std::time::UNIX_EPOCH),
                    metadata.len(),
                    entry.path(),
                ));
            }
            continue;
        }
        match remove_tolerant(&entry.path()).await {
            Ok(true) if temporary => outcome.removed += 1,
            Ok(true) => outcome.evicted += 1,
            Ok(false) => outcome.skipped += 1,
            Err(e) => outcome.failures.push(TaskFailure::disk(owned.label, &e)),
        }
    }
    if let Some(max_bytes) = owned.eviction.and_then(|eviction| eviction.max_bytes) {
        let mut total: u64 = cached.iter().map(|(_, size, _)| *size).sum();
        if total > max_bytes {
            cached.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
            for (_, size, path) in cached {
                if total <= max_bytes {
                    break;
                }
                match remove_tolerant(&path).await {
                    Ok(true) => {
                        outcome.evicted += 1;
                        total = total.saturating_sub(size);
                    }
                    Ok(false) => outcome.skipped += 1,
                    Err(e) => outcome.failures.push(TaskFailure::disk(owned.label, &e)),
                }
            }
        }
    }
    outcome
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
        eviction: Some(CacheEviction::image_cache_from_env()),
    }];
    if let Some(path) = backups {
        dirs.push(OwnedDir {
            label: "backup-staging",
            path,
            pattern: TempPattern::BackupStaging,
            eviction: None,
        });
    }
    dirs
}

pub async fn router(auth: AuthState, media_root: PathBuf) -> Result<Router, sqlx::Error> {
    ensure_activity_cleanup(&auth.store).await?;
    // Tasks seeded after the trigger migration ran get their default trigger now.
    crate::task_scheduler::adopt_legacy_system_tasks(&auth.store, now()).await?;
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

/// Also install the maintenance schedule on existing databases without replacing user settings.
async fn ensure_activity_cleanup(
    store: &crate::identity::IdentityStore,
) -> Result<(), sqlx::Error> {
    crate::database::query(
        "CREATE INDEX IF NOT EXISTS extension_audit_retention ON extension_audit(created_at,id)",
    )
    .execute(store.pool())
    .await?;
    crate::database::query("INSERT INTO system_task_schedules(id,name,description,category,interval_minutes,enabled,next_due_at) VALUES('clean-activity-log','Nettoyer le journal des extensions','Supprime au plus 500 événements du journal des extensions antérieurs à la rétention configurée.','Maintenance',1440,1,?) ON CONFLICT(id) DO NOTHING")
        .bind(now().saturating_add(86400)).execute(store.pool()).await?;
    Ok(())
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

    let timestamp = now();
    let next_due = timestamp + (input.interval_minutes * 60);
    let _writer = state.auth.store.task_writes.lock().await;
    let mut conn = state
        .auth
        .store
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let changed = crate::database::query(
        "UPDATE system_task_schedules SET interval_minutes=?, enabled=?, next_due_at=? WHERE id=?",
    )
    .bind(input.interval_minutes)
    .bind(if input.enabled { 1 } else { 0 })
    .bind(next_due)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if changed.rows_affected() == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    // Compatibility endpoint: the task keeps exactly one interval trigger.
    crate::task_triggers::replace_with_interval(
        &mut tx,
        "system",
        &id,
        input.interval_minutes,
        input.enabled,
        timestamp,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::database::commit(tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
        "clean-statistics" => crate::statistics::maintain(&state.auth.store)
            .await
            .map(|count| TaskReport {
                summary: format!("{count} expired statistics event(s) removed"),
                failures: Vec::new(),
            })
            .map_err(|e| TaskFailure::database("statistics", e)),
        "clean-activity-log" => task_clean_activity_log(state, job).await,
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
    let permanent = failed && report.failures.iter().all(|failure| !failure.retryable);
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
    if permanent {
        // Deterministic failure: the queue terminates this run instead of retrying it.
        Err(StatusCode::UNPROCESSABLE_ENTITY)
    } else if failed {
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
    // Session expiry must follow authentication policy, never log retention.
    let inactivity_threshold = current_time.saturating_sub(crate::auth::IDLE_SECONDS);
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

/// One bounded, replayable deletion; current leases fence every persisted change.
async fn task_clean_activity_log(
    state: &SystemTasksState,
    job: Option<&crate::task_engine::Job>,
) -> Result<TaskReport, TaskFailure> {
    let settings = crate::server_settings::load(&state.auth.store)
        .await
        .map_err(|_| {
            TaskFailure::new(
                "config",
                "activityLogRetentionDays",
                "server settings could not be loaded; cleanup refused",
            )
        })?;
    if !(1..=3650).contains(&settings.activity_log_retention_days) {
        return Err(TaskFailure::new(
            "config",
            "activityLogRetentionDays",
            "invalid retention; cleanup refused",
        ));
    }
    let current_time = now();
    let cutoff =
        current_time.saturating_sub(i64::from(settings.activity_log_retention_days) * 86400);
    let task_id = job.map_or("", |j| j.id.as_str());
    let attempt = job.map_or(0, |j| j.attempt);
    let done = crate::database::query("DELETE FROM extension_audit WHERE id IN (SELECT id FROM extension_audit WHERE created_at<? ORDER BY created_at,id LIMIT 500) AND created_at<? AND (?='' OR EXISTS(SELECT 1 FROM durable_tasks WHERE id=? AND attempt=? AND status='running' AND lease_until>unixepoch()))")
        .bind(cutoff).bind(cutoff).bind(task_id).bind(task_id).bind(attempt)
        .execute(state.auth.store.pool()).await.map_err(|e| TaskFailure::database("extension_audit", e))?;
    Ok(TaskReport {
        summary: format!(
            "{} extension audit event(s) removed; retention {} day(s), maximum 500 per run",
            done.rows_affected(),
            settings.activity_log_retention_days
        ),
        failures: Vec::new(),
    })
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
    let (mut files, mut evicted, mut skipped) = (0, 0, 0);
    let mut unreadable = Vec::new();
    for owned in &state.owned_dirs {
        let outcome = sweep_owned_dir(owned, &state.media_root, TEMP_MIN_AGE).await;
        files += outcome.removed;
        evicted += outcome.evicted;
        skipped += outcome.skipped;
        if outcome.unreadable {
            unreadable.push(owned.label);
        }
        report.failures.extend(outcome.failures);
    }
    let mut summary = format!(
        "{previews} orphan preview(s), {files} stale temporary file(s) and {evicted} expired cached image(s) removed"
    );
    if skipped > 0 {
        summary.push_str(&format!("; {skipped} locked or protected file(s) skipped"));
    }
    if !unreadable.is_empty() {
        summary.push_str(&format!(
            "; not readable, skipped: {}",
            unreadable.join(", ")
        ));
    }
    report.summary = summary;
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
        ensure_activity_cleanup(&auth.store).await.unwrap();
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
    async fn settings_activity_retention_is_bounded_fenced_and_independent_of_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let state = state_in(temp.path(), Vec::new()).await;
        let pool = state.auth.store.pool();
        let current = now();
        crate::database::query("UPDATE system_task_schedules SET enabled=0,interval_minutes=60 WHERE id='clean-activity-log'")
            .execute(pool).await.unwrap();
        ensure_activity_cleanup(&state.auth.store).await.unwrap();
        let saved = schedule(&state, "clean-activity-log").await;
        assert_eq!((saved.enabled, saved.interval_minutes), (0, 60));
        crate::database::query("INSERT INTO server_settings(singleton,settings_json) VALUES(1,?)")
            .bind(r#"{"serverName":"test","welcomeMessage":"","activityLogRetentionDays":1}"#)
            .execute(pool)
            .await
            .unwrap();
        crate::database::query("WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM sequence WHERE n<501) INSERT INTO extension_audit(id,actor_id,extension_id,action,created_at) SELECT CAST(n AS TEXT),'admin','test','install',? FROM sequence")
            .bind(current - 10 * 86400).execute(pool).await.unwrap();
        crate::database::query("INSERT INTO extension_audit(id,actor_id,extension_id,action,created_at) VALUES('recent','admin','test','install',?)")
            .bind(current - 3600).execute(pool).await.unwrap();
        crate::database::query("INSERT INTO durable_tasks(id,owner,idempotency_key,node_id,category,target_id,status,attempt,created_at,updated_at,available_at,lease_until) VALUES('audit-cleanup','system','audit-cleanup','local','system','clean-activity-log','running',1,?,?,?,?)")
            .bind(current).bind(current).bind(current).bind(current - 1).execute(pool).await.unwrap();
        let job = crate::task_engine::get(&state.auth.store, "audit-cleanup")
            .await
            .unwrap()
            .unwrap();
        task_clean_activity_log(&state, Some(&job)).await.unwrap();
        let count = || {
            crate::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM extension_audit")
                .fetch_one(pool)
        };
        assert_eq!(count().await.unwrap(), 502, "expired lease must not delete");
        crate::database::query("UPDATE durable_tasks SET lease_until=? WHERE id='audit-cleanup'")
            .bind(current + 60)
            .execute(pool)
            .await
            .unwrap();
        task_clean_activity_log(&state, Some(&job)).await.unwrap();
        assert_eq!(count().await.unwrap(), 2, "one pass removes at most 500");
        task_clean_activity_log(&state, Some(&job)).await.unwrap();
        assert_eq!(count().await.unwrap(), 1, "replay retains recent events");
        crate::database::query("INSERT INTO users(id,role) VALUES('reader','user')")
            .execute(pool)
            .await
            .unwrap();
        for (id, idle_days) in [("valid", 2), ("idle-expired", 8)] {
            crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'reader',?,?,?)")
                .bind(id).bind(current - 9 * 86400).bind(current - idle_days * 86400).bind(current + 86400).execute(pool).await.unwrap();
        }
        task_clean_sessions(&state).await.unwrap();
        let remaining: String = crate::database::query_scalar("SELECT token_hash FROM sessions")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(
            remaining, "valid",
            "one-day log retention must not revoke a two-day idle valid session"
        );
        crate::database::query("UPDATE server_settings SET settings_json='invalid'")
            .execute(pool)
            .await
            .unwrap();
        assert!(
            task_clean_activity_log(&state, Some(&job)).await.is_err(),
            "settings errors must fail closed"
        );
        assert_eq!(count().await.unwrap(), 1);
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
        assert_eq!(tasks.len(), 7);
        for id in [
            "update-extensions",
            "optimize-database",
            "clean-sessions",
            "clean-statistics",
            "clean-activity-log",
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
            "clean-activity-log",
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
            eviction: Some(CacheEviction {
                max_age: Some(Duration::from_secs(30 * 86_400)),
                max_bytes: None,
            }),
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
                eviction: Some(CacheEviction {
                    max_age: Some(Duration::ZERO),
                    max_bytes: Some(0),
                }),
            };
            let outcome = sweep_owned_dir(&owned, &media, Duration::ZERO).await;
            assert_eq!(outcome.removed + outcome.evicted, 0);
            assert_eq!(outcome.failures.len(), 1);
            assert_eq!(outcome.failures[0].kind, "config");
            assert!(!outcome.failures[0].retryable);
            assert!(victim.exists());
        }
        // A missing directory is not an error and removes nothing.
        let missing = OwnedDir {
            label: "image-cache",
            path: temp.path().join("absent"),
            pattern: TempPattern::ImageCache,
            eviction: None,
        };
        let outcome = sweep_owned_dir(&missing, &media, Duration::ZERO).await;
        assert_eq!((outcome.removed, outcome.failures.len()), (0, 0));
        assert!(!outcome.unreadable);
    }

    fn aged(path: &std::path::Path, seconds: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(seconds))
            .unwrap();
    }

    #[tokio::test]
    async fn image_cache_evicts_expired_then_oldest_entries_over_the_size_cap() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let cache = temp.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let entry = |c: &str| cache.join(format!("{}.img", c.repeat(64)));
        let (expired, oldest, middle, newest) = (entry("a"), entry("b"), entry("c"), entry("d"));
        let foreign = cache.join("keep.img");
        for path in [&expired, &oldest, &middle, &newest, &foreign] {
            std::fs::write(path, vec![0u8; 100]).unwrap();
        }
        aged(&expired, 40 * 86_400);
        aged(&foreign, 40 * 86_400);
        aged(&oldest, 3 * 86_400);
        aged(&middle, 2 * 86_400);
        aged(&newest, 86_400);
        let owned = OwnedDir {
            label: "image-cache",
            path: cache.clone(),
            pattern: TempPattern::ImageCache,
            eviction: Some(CacheEviction {
                max_age: Some(Duration::from_secs(30 * 86_400)),
                max_bytes: Some(200),
            }),
        };
        let outcome = sweep_owned_dir(&owned, &media, TEMP_MIN_AGE).await;
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        assert_eq!(outcome.evicted, 2);
        assert!(!expired.exists(), "entries past the age limit are evicted");
        assert!(!oldest.exists(), "the oldest entry goes first over the cap");
        assert!(middle.exists() && newest.exists());
        assert!(
            foreign.exists(),
            "names Aether does not own are never removed"
        );
    }

    /// A file held open without delete sharing (Windows: antivirus, indexer, viewer) or inside a
    /// read-only directory (Unix) is skipped and the task still succeeds.
    #[tokio::test]
    async fn clean_cache_skips_locked_or_denied_files_without_failing() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let cache = temp.path().join("cache");
        std::fs::create_dir(&cache).unwrap();
        let locked = cache.join(format!("{}.tmp", "ab".repeat(16)));
        std::fs::write(&locked, b"x").unwrap();
        old(&locked);
        #[cfg(windows)]
        let _guard = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&locked)
                .unwrap()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o500)).unwrap();
        }
        let owned = OwnedDir {
            label: "image-cache",
            path: cache.clone(),
            pattern: TempPattern::ImageCache,
            eviction: None,
        };
        let state = state_in(temp.path(), vec![owned.clone()]).await;
        let outcome = sweep_owned_dir(&owned, &media, TEMP_MIN_AGE).await;
        // Root bypasses Unix permissions: the file is then simply removed.
        if locked.exists() {
            assert_eq!(outcome.skipped, 1);
        }
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
        execute_task(&state, "clean-cache").await.unwrap();
        let row = schedule(&state, "clean-cache").await;
        assert_eq!(row.last_outcome, "succeeded", "{:?}", row.last_message);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    #[test]
    fn io_errors_are_classified_for_skipping_and_retrying() {
        use std::io::{Error, ErrorKind};
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::NotFound,
            ErrorKind::ResourceBusy,
            ErrorKind::ReadOnlyFilesystem,
        ] {
            assert!(io_error_is_skippable(&Error::from(kind)), "{kind:?}");
        }
        assert!(!io_error_is_skippable(&Error::from(ErrorKind::StorageFull)));
        assert!(!io_error_is_skippable(&Error::other("unexpected")));
        #[cfg(windows)]
        {
            assert!(
                io_error_is_skippable(&Error::from_raw_os_error(32)),
                "sharing violation"
            );
            assert!(
                io_error_is_skippable(&Error::from_raw_os_error(33)),
                "lock violation"
            );
            assert!(
                io_error_is_skippable(&Error::from_raw_os_error(5)),
                "access denied"
            );
        }
        let denied = TaskFailure::disk("image-cache", &Error::from(ErrorKind::PermissionDenied));
        assert!(!denied.retryable, "permission errors must not be retried");
        assert!(denied.message.contains("PermissionDenied"));
        assert!(TaskFailure::disk("x", &Error::from(ErrorKind::TimedOut)).retryable);
        assert!(!TaskFailure::new("config", "x", "y").retryable);
        assert!(TaskFailure::new("network", "x", "y").retryable);
    }

    #[tokio::test]
    async fn deterministic_failures_are_reported_as_permanent() {
        let temp = tempfile::tempdir().unwrap();
        let state = state_in(temp.path(), Vec::new()).await;
        crate::database::query(
            "INSERT INTO system_task_schedules(id,name,description,category,interval_minutes,enabled,next_due_at) VALUES('bogus','b','b','b',5,0,0)",
        )
        .execute(&state.auth.store.pool)
        .await
        .unwrap();
        assert_eq!(
            execute_task(&state, "bogus").await,
            Err(StatusCode::UNPROCESSABLE_ENTITY)
        );
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
