//! Administrator-only SQLite/PostgreSQL backups. Archives contain secrets and are encrypted
//! (`MDBK0003`) with the operator key from `MEDIA_BACKUP_KEY_FILE` / `MEDIA_BACKUP_KEY`; without
//! a key no archive is produced unless `MEDIA_BACKUP_ALLOW_UNENCRYPTED=true` is set explicitly.
use crate::{auth::AuthState, backup_bundle::Protection, identity::Role};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{
    collections::BTreeMap,
    path::{Path as DiskPath, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

struct Backups {
    auth: AuthState,
    source: Option<PathBuf>,
    postgres_url: Option<Arc<zeroize::Zeroizing<String>>>,
    directory: Option<PathBuf>,
    media_root: PathBuf,
    gate: Arc<Semaphore>,
    worker: Mutex<Option<tokio::task::AbortHandle>>,
    configuration: BTreeMap<String, String>,
    /// `None` when no valid key is configured: backups then fail closed.
    protection: Option<Protection>,
}

const KEY_REQUIRED: (&str, &str) = (
    "backup_key_required",
    "Aucune clé de sauvegarde valide n’est configurée. Définissez MEDIA_BACKUP_KEY_FILE (64 caractères hexadécimaux, par ex. `openssl rand -hex 32`) puis redémarrez Core.",
);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    id: String,
    size: u64,
    created_at: u64,
}

#[derive(Debug)]
struct BackupFailure {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}
impl BackupFailure {
    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        eprintln!("backup failed: {code}");
        Self {
            status,
            code,
            message,
        }
    }
}
impl From<StatusCode> for BackupFailure {
    fn from(status: StatusCode) -> Self {
        Self {
            status,
            code: "backup_request_failed",
            message: "Sauvegarde refusée : vérifiez vos droits et le dossier configuré.",
        }
    }
}
impl IntoResponse for BackupFailure {
    fn into_response(self) -> Response {
        (
            self.status,
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({"code":self.code,"message":self.message})),
        )
            .into_response()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    available: bool,
    reason: Option<&'static str>,
    running: bool,
    archives: Vec<Archive>,
    schedule: Schedule,
}

/// Configuration is resolved once from the actual open database, never from a request.
pub async fn router(
    auth: AuthState,
    media_root: PathBuf,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let (directory, configuration) = environment_inputs();
    router_with_configuration(auth, media_root, directory, configuration).await
}

fn environment_inputs() -> (Option<PathBuf>, BTreeMap<String, String>) {
    let directory = std::env::var_os("MEDIA_BACKUP_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    // Key settings are consumed (and stripped from the archived configuration) by `build_state`.
    let configuration = std::env::vars()
        .filter(|(name, _)| name.starts_with("MEDIA_") || name.starts_with("OIDC_"))
        .collect();
    (directory, configuration)
}

/// Explicit operator/test configuration; never exposed as an HTTP payload.
pub async fn router_with_configuration(
    auth: AuthState,
    media_root: PathBuf,
    directory: Option<PathBuf>,
    configuration: BTreeMap<String, String>,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let state = build_state(auth, media_root, directory, configuration).await?;
    Ok(router_with_state(state))
}

/// Resolve engine, directory and configuration from the open database.
async fn build_state(
    auth: AuthState,
    media_root: PathBuf,
    directory: Option<PathBuf>,
    mut configuration: BTreeMap<String, String>,
) -> Result<Backups, Box<dyn std::error::Error + Send + Sync>> {
    let protection = {
        let lookup = |name: &str| configuration.get(name).cloned();
        match crate::backup_bundle::protection_from_settings(&lookup) {
            Ok(Protection::Unencrypted) => {
                eprintln!(
                    "admin_backups: WARNING backups will be UNENCRYPTED (MEDIA_BACKUP_ALLOW_UNENCRYPTED)"
                );
                Some(Protection::Unencrypted)
            }
            Ok(protection) => Some(protection),
            Err(error) => {
                eprintln!("admin_backups: backups disabled: {error}");
                None
            }
        }
    };
    for name in [
        "MEDIA_BACKUP_KEY",
        "MEDIA_BACKUP_KEY_FILE",
        "MEDIA_BACKUP_ALLOW_UNENCRYPTED",
    ] {
        configuration.remove(name);
    }
    let mut connection = auth.store.pool().acquire().await?;
    let source = if connection.backend_name() == "SQLite" {
        let rows = crate::database::query("PRAGMA database_list")
            .fetch_all(&mut *connection)
            .await?;
        rows.into_iter().find_map(|row| {
            let name: String = row.try_get("name").ok()?;
            let file: String = row.try_get("file").ok()?;
            (name == "main" && !file.is_empty()).then(|| PathBuf::from(file))
        })
    } else {
        None
    };
    let postgres_url = if connection.backend_name() == "PostgreSQL" {
        Some(Arc::new(zeroize::Zeroizing::new(
            auth.store
                .pool()
                .connect_options()
                .database_url
                .as_str()
                .to_owned(),
        )))
    } else {
        None
    };
    drop(connection);
    let default_directory = source
        .as_ref()
        .and_then(|source| source.parent())
        .map(|parent| parent.join("backups"))
        .unwrap_or_else(|| {
            if DiskPath::new("/backups").is_dir() {
                PathBuf::from("/backups")
            } else if DiskPath::new("/data").is_dir() {
                PathBuf::from("/data/backups")
            } else {
                PathBuf::from("backups")
            }
        });
    let directory = Some(directory.unwrap_or(default_directory));
    if let Some(directory) = &directory {
        configuration.insert(
            "MEDIA_BACKUP_DIR".into(),
            directory.to_string_lossy().into_owned(),
        );
    }
    configuration.insert(
        "MEDIA_DB_ENGINE".into(),
        if postgres_url.is_some() {
            "postgres"
        } else {
            "sqlite"
        }
        .into(),
    );
    if let Some(url) = &postgres_url {
        configuration.insert("MEDIA_DB_URL".into(), url.as_str().to_owned());
    }
    if let Some(source) = &source {
        configuration.insert(
            "MEDIA_DB_PATH".into(),
            source.to_string_lossy().into_owned(),
        );
    }
    configuration.insert("MEDIA_PUBLIC_URL".into(), auth.origin().into());
    Ok(Backups {
        auth,
        source,
        postgres_url,
        directory,
        media_root,
        gate: Arc::new(Semaphore::new(1)),
        worker: Mutex::new(None),
        configuration,
        protection,
    })
}

fn router_with_state(state: Backups) -> Router {
    let state = Arc::new(state);
    start_worker(&state);
    Router::new()
        .route("/api/v1/admin/backups", get(list).post(create))
        .route(
            "/api/v1/admin/backups/schedule",
            axum::routing::put(update_schedule),
        )
        .route(
            "/api/v1/admin/backups/{id}/download",
            get(download).head(download),
        )
        .with_state(state)
}

async fn administrator(
    state: &Backups,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let identity = state.auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        state.auth.require_csrf(headers, &identity)?;
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id)
        .is_ok_and(|value| value.to_string() == id && value.get_version_num() == 4)
}

// Walk every existing ancestor, including Windows junctions, before touching disk.
fn safe_directory(directory: &DiskPath, media_root: &DiskPath) -> Result<(), StatusCode> {
    // Refuse missing intermediate parents: policy must resolve the destination before mkdir.
    let parent_is_dir = directory
        .parent()
        .map(|p| {
            if p.as_os_str().is_empty() {
                DiskPath::new(".").is_dir()
            } else {
                p.is_dir()
            }
        })
        .unwrap_or(false);
    if !directory.exists() && !parent_is_dir {
        return Err(StatusCode::CONFLICT);
    }
    #[cfg(unix)]
    if let Ok(metadata) = std::fs::symlink_metadata(directory) {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            eprintln!(
                "admin_backups: backup directory {:?} must have mode 0700 (current {:o})",
                directory,
                metadata.permissions().mode() & 0o777
            );
            return Err(StatusCode::FORBIDDEN);
        }
    }
    for ancestor in directory.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        crate::backup::check_path_safety(ancestor, "backup directory")
            .map_err(|_| StatusCode::CONFLICT)?;
    }
    crate::backup::check_not_in_media_root(
        &directory.join("archive.media-backup"),
        Some(media_root),
        "backup directory",
    )
    .map_err(|_| StatusCode::CONFLICT)
}

async fn validate_directory(state: &Backups) -> Result<PathBuf, StatusCode> {
    let directory = state.directory.clone().ok_or(StatusCode::NOT_IMPLEMENTED)?;
    let roots: Vec<String> =
        crate::database::query_scalar("SELECT root FROM libraries WHERE node_id = 'local'")
            .fetch_all(state.auth.store.pool())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let media_root = state.media_root.clone();
    tokio::task::spawn_blocking(move || {
        safe_directory(&directory, &media_root)?;
        for root in roots {
            safe_directory(&directory, DiskPath::new(&root))?;
        }
        Ok(directory)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

async fn list(
    State(state): State<Arc<Backups>>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    administrator(&state, &headers, false).await?;
    let db_available = state.source.is_some() || state.postgres_url.is_some();
    let (available, reason, archives) = if !db_available {
        let reason = if state.postgres_url.is_some() {
            "PostgreSQL requires pg_dump 18 on the server."
        } else {
            "Database is unavailable."
        };
        (false, Some(reason), Vec::new())
    } else {
        match validate_directory(&state).await {
            Ok(directory) => {
                let listing_result =
                    tokio::task::spawn_blocking(move || -> Result<Vec<Archive>, std::io::Error> {
                        if !directory.exists() {
                            return Ok(Vec::new());
                        }
                        let entries = match std::fs::read_dir(&directory) {
                            Ok(entries) => entries,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                return Ok(Vec::new());
                            }
                            Err(err) => {
                                eprintln!(
                                    "admin_backups: read_dir error in {:?}: {:?}",
                                    directory, err
                                );
                                return Err(err);
                            }
                        };
                        let mut archives = Vec::new();
                        for entry in entries.take(10_000).flatten() {
                            let name = entry.file_name();
                            let Some(id) = name
                                .to_str()
                                .and_then(|name| name.strip_suffix(".media-backup"))
                            else {
                                continue;
                            };
                            if !valid_id(id) {
                                continue;
                            }
                            if let Ok(file) = media_node::OpenedFile::open_in_root_sync(
                                &directory,
                                DiskPath::new(&name),
                            ) {
                                let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                                    continue;
                                };
                                let created_at = metadata
                                    .modified()
                                    .ok()
                                    .and_then(|date| {
                                        date.duration_since(std::time::UNIX_EPOCH).ok()
                                    })
                                    .map_or(0, |duration| duration.as_secs());
                                archives.push(Archive {
                                    id: id.into(),
                                    size: file.size(),
                                    created_at,
                                });
                            }
                        }
                        archives
                            .sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
                        archives.truncate(1000);
                        Ok(archives)
                    })
                    .await;
                match listing_result {
                    Ok(Ok(archives)) => (true, None, archives),
                    Ok(Err(error)) => {
                        let reason = match error.kind() {
                            std::io::ErrorKind::PermissionDenied => {
                                "Le processus du serveur ne peut pas lire le dossier de sauvegarde. Vérifiez le propriétaire et les permissions du volume de sauvegardes pour l’UID/GID du conteneur."
                            }
                            std::io::ErrorKind::NotADirectory => {
                                "Le chemin de sauvegarde configuré ne correspond pas à un dossier. Vérifiez le montage du volume de sauvegardes."
                            }
                            _ => {
                                "Impossible de lire les archives de sauvegarde. Vérifiez le volume et les journaux du serveur."
                            }
                        };
                        (false, Some(reason), Vec::new())
                    }
                    Err(join_err) => {
                        eprintln!("admin_backups: join error on listing: {:?}", join_err);
                        (
                            false,
                            Some("Impossible de lire les archives de sauvegarde."),
                            Vec::new(),
                        )
                    }
                }
            }
            Err(StatusCode::FORBIDDEN) => (
                false,
                Some(
                    "Le dossier de sauvegarde doit être privé (permissions 0700). Vérifiez aussi qu’il appartient à l’UID/GID du conteneur.",
                ),
                Vec::new(),
            ),
            Err(_) => (
                false,
                Some(
                    "Le dossier de sauvegarde n'est pas accessible ou son emplacement est refusé.",
                ),
                Vec::new(),
            ),
        }
    };
    let (available, reason) = if available && state.protection.is_none() {
        (false, Some(KEY_REQUIRED.1))
    } else {
        (available, reason)
    };
    let schedule = load_schedule(&state).await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(Listing {
            schedule,
            available,
            reason,
            running: state.gate.available_permits() == 0,
            archives,
        }),
    )
        .into_response())
}

async fn perform_backup(state: &Backups) -> Result<Archive, BackupFailure> {
    // Fail closed before any snapshot: no key means no plaintext copy of the secrets on disk.
    let protection = state
        .protection
        .clone()
        .ok_or_else(|| BackupFailure::new(StatusCode::CONFLICT, KEY_REQUIRED.0, KEY_REQUIRED.1))?;
    let directory = validate_directory(state).await?;
    let path = directory.clone();
    tokio::task::spawn_blocking(move || {
        #[cfg(unix)]
        let builder = {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = std::fs::DirBuilder::new();
            builder.mode(0o700);
            builder
        };
        #[cfg(not(unix))]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(not(unix))]
        builder.recursive(true);
        let result = match builder.create(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {
                Ok(())
            }
            Err(error) => Err(error),
        };
        #[cfg(unix)]
        if result.is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
        }
        result?;
        // A pre-existing private directory can belong to a different deployment UID.
        // Test real access before starting an expensive SQL snapshot.
        let probe = path.join(format!(".backup-access-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&probe)?;
        drop(file);
        std::fs::remove_file(probe)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|error| BackupFailure::new(
        if error.kind() == std::io::ErrorKind::PermissionDenied { StatusCode::FORBIDDEN } else { StatusCode::INTERNAL_SERVER_ERROR },
        "backup_directory_unwritable",
        "Le processus du serveur ne peut pas écrire dans le dossier de sauvegarde. Vérifiez le propriétaire et les permissions du volume de sauvegardes pour l’UID/GID du conteneur.",
    ))?;
    validate_directory(state).await?;
    let id = uuid::Uuid::new_v4().to_string();
    struct TemporaryDatabase(PathBuf);
    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let temporary = TemporaryDatabase(directory.join(format!(".database-{id}.tmp")));
    let engine = if let Some(url) = &state.postgres_url {
        crate::postgres_backup::backup(url.as_str(), &temporary.0, Some(&state.media_root))
            .await
            .map_err(|error| {
                eprintln!("backup PostgreSQL snapshot failed: {error}");
                BackupFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "backup_postgres_snapshot_failed", "Le snapshot PostgreSQL a échoué. Vérifiez pg_dump 18 et la connexion du serveur dans les journaux.")
            })?;
        "postgres"
    } else if let Some(source) = &state.source {
        crate::backup::backup_database(source, &temporary.0, Some(&state.media_root))
            .await
            .map_err(|_| BackupFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "backup_sqlite_snapshot_failed", "Le snapshot SQLite a échoué. Vérifiez l’accès à la base et l’espace disponible."))?;
        "sqlite"
    } else {
        return Err(StatusCode::NOT_IMPLEMENTED.into());
    };
    // Assets are listed after the database snapshot: every row present in the snapshot has its
    // file (file-before-row writes, deferred deletion), so a failure here is a real defect.
    let assets = state
        .auth
        .store
        .assets()
        .export_source()
        .await
        .map_err(|error| {
            eprintln!("backup: cannot list durable assets: {error}");
            BackupFailure::new(StatusCode::INTERNAL_SERVER_ERROR, "backup_assets_unavailable", "L’inventaire des fichiers durables est indisponible. La sauvegarde n’a pas été créée.")
        })?;
    let size = crate::backup_bundle::create_with_assets(
        &temporary.0,
        state.configuration.clone(),
        &directory.join(format!("{id}.media-backup")),
        engine,
        Some(assets),
        protection,
    )
    .await
    .map_err(|error| {
        eprintln!("backup: archive creation failed: {error}");
        let (code, message) = if matches!(error, crate::backup_bundle::BundleError::AssetIntegrity) {
            ("backup_asset_integrity_failed", "Un fichier durable est absent ou corrompu. Vérifiez le montage du volume des assets ; la sauvegarde incomplète a été refusée.")
        } else {
            ("backup_archive_failed", "La création de l’archive a échoué. Vérifiez l’espace disque, les permissions et les fichiers de certificats configurés.")
        };
        BackupFailure::new(StatusCode::INTERNAL_SERVER_ERROR, code, message)
    })?;
    Ok(Archive {
        id,
        size,
        created_at: now() as u64,
    })
}

async fn create(
    State(state): State<Arc<Backups>>,
    headers: HeaderMap,
) -> Result<Response, BackupFailure> {
    administrator(&state, &headers, true).await?;
    if state.source.is_none() && state.postgres_url.is_none() {
        return Err(StatusCode::NOT_IMPLEMENTED.into());
    }
    let permit = state
        .gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::CONFLICT)?;
    // Own the permit across disconnects; scheduler and manual requests share this gate.
    let result = tokio::spawn(async move {
        let _permit = permit;
        perform_backup(&state).await
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(result),
    )
        .into_response())
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Schedule {
    #[sqlx(try_from = "crate::database::DbBool")]
    enabled: bool,
    interval_minutes: i64,
    next_due_at: i64,
    last_started_at: Option<i64>,
    last_completed_at: Option<i64>,
    last_outcome: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScheduleInput {
    enabled: bool,
    interval_minutes: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

async fn load_schedule(state: &Backups) -> Result<Schedule, StatusCode> {
    crate::database::query_as("SELECT enabled,interval_minutes,next_due_at,last_started_at,last_completed_at,last_outcome FROM backup_schedule WHERE singleton=1")
        .fetch_one(state.auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn update_schedule(
    State(state): State<Arc<Backups>>,
    headers: HeaderMap,
    Json(input): Json<ScheduleInput>,
) -> Result<Response, StatusCode> {
    administrator(&state, &headers, true).await?;
    if !(60..=10080).contains(&input.interval_minutes) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    if input.enabled {
        if state.source.is_none() && state.postgres_url.is_none() {
            return Err(StatusCode::NOT_IMPLEMENTED);
        }
        validate_directory(&state).await?;
    }
    {
        let timestamp = now();
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
        // Compatibility endpoint: the backup keeps exactly one interval trigger.
        crate::task_triggers::replace_with_interval(
            &mut tx,
            "backup",
            "backup",
            input.interval_minutes,
            input.enabled,
            timestamp,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::database::query(
            "UPDATE backup_schedule SET enabled=?,interval_minutes=?,next_due_at=? WHERE singleton=1",
        )
        .bind(i64::from(input.enabled))
        .bind(input.interval_minutes)
        .bind(timestamp + input.interval_minutes * 60)
        .execute(&mut *tx)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        crate::database::commit(tx)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(load_schedule(&state).await?),
    )
        .into_response())
}

/// Queue one backup run (backup triggers via `task_scheduler`, the unified "run" action).
/// Returns `false` when a backup is already queued or running. The durable task's lease, fencing
/// token and playback rules then govern the execution; `backup_schedule` records its outcome.
pub(crate) async fn trigger_backup(
    store: &crate::identity::IdentityStore,
    key: &str,
    timestamp: i64,
    max_runtime_seconds: Option<i64>,
) -> Result<bool, sqlx::Error> {
    let active: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND target_id='backup' AND status IN ('queued','running','paused')")
        .fetch_one(store.pool())
        .await?;
    if active > 0 {
        return Ok(false);
    }
    crate::system_task_queue::enqueue_backup(store, key, timestamp, max_runtime_seconds).await?;
    Ok(true)
}

/// Execute a queued scheduled backup under its durable-task lease. The outcome is recorded only
/// while this attempt still owns the lease, so a superseded worker never overwrites the result.
pub(crate) async fn execute_scheduled(
    auth: AuthState,
    media_root: PathBuf,
    job: &crate::task_engine::Job,
) -> Result<(), StatusCode> {
    let (directory, configuration) = environment_inputs();
    let state = build_state(auth, media_root, directory, configuration)
        .await
        .map_err(|error| {
            eprintln!("scheduled backup: configuration unavailable: {error}");
            StatusCode::SERVICE_UNAVAILABLE
        })?;
    run_scheduled(&state, Some(job)).await
}

async fn run_scheduled(
    state: &Backups,
    fence: Option<&crate::task_engine::Job>,
) -> Result<(), StatusCode> {
    // `running` is set when the run really starts, never while it only waits in the queue.
    crate::database::query("UPDATE backup_schedule SET last_started_at=?,last_completed_at=NULL,last_outcome='running' WHERE singleton=1")
        .bind(now())
        .execute(state.auth.store.pool())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let result = perform_backup(state).await;
    if let Err(error) = &result {
        eprintln!(
            "scheduled backup failed: {} HTTP {}",
            error.code, error.status
        );
    }
    if let Some(job) = fence {
        let current = crate::task_engine::get(&state.auth.store, &job.id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let owns = current.is_some_and(|c| {
            c.status == "running"
                && c.attempt == job.attempt
                && c.lease_until.is_some_and(|u| u > now())
        });
        if !owns {
            eprintln!("scheduled backup: lease lost, result not recorded");
            return Err(StatusCode::CONFLICT);
        }
    }
    crate::database::query("UPDATE backup_schedule SET last_completed_at=?,last_outcome=? WHERE singleton=1 AND last_outcome='running'")
        .bind(now()).bind(if result.is_ok() {"succeeded"} else {"failed"})
        .execute(state.auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let (Some(job), Err(error)) = (fence, &result) {
        // The administrator-facing reason, shown as the task's last-run message.
        let mut detail = serde_json::from_str::<serde_json::Value>(&job.result_json)
            .ok()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        detail["code"] = error.code.into();
        detail["message"] = error.message.into();
        let _ = crate::database::query(
            "UPDATE durable_tasks SET result_json=? WHERE id=? AND attempt=?",
        )
        .bind(detail.to_string())
        .bind(&job.id)
        .bind(job.attempt)
        .execute(state.auth.store.pool())
        .await;
    }
    result.map(|_| ()).map_err(|error| error.status)
}

fn start_worker(state: &Arc<Backups>) {
    let weak = Arc::downgrade(state);
    let task = tokio::spawn(async move {
        // On startup distinguish an interrupted scheduled attempt from completion.
        if let Some(state) = weak.upgrade() {
            let _ = crate::database::query("UPDATE backup_schedule SET last_outcome='interrupted',last_completed_at=? WHERE singleton=1 AND last_outcome='running'")
                .bind(now()).execute(state.auth.store.pool()).await;
        }
        // Scheduled backups fire from their triggers (`task_scheduler`), like every task.
    });
    match state.worker.lock() {
        Ok(mut handle) => *handle = Some(task.abort_handle()),
        Err(_) => task.abort(),
    }
}

impl Drop for Backups {
    fn drop(&mut self) {
        if let Ok(mut handle) = self.worker.lock()
            && let Some(handle) = handle.take()
        {
            handle.abort();
        }
    }
}
async fn download(
    State(state): State<Arc<Backups>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    method: axum::http::Method,
) -> Result<Response, StatusCode> {
    administrator(&state, &headers, false).await?;
    if !valid_id(&id) {
        return Err(StatusCode::NOT_FOUND);
    }
    let directory = validate_directory(&state).await?;
    let filename = format!("{id}.media-backup");
    let opened = media_node::OpenedFile::open_in_root(&directory, DiskPath::new(&filename))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let response = media_node::stream::serve_opened_parts(
        opened,
        &method,
        &headers,
        "application/octet-stream",
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut response = response.map(Body::new);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scheduled_backup_persists_shares_gate_and_records_failure_without_catchup_storm() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let source = temp.path().join("live.media-backup");
        let store = crate::identity::IdentityStore::open(&source).await.unwrap();
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let mut state = Backups {
            auth,
            source: Some(source.clone()),
            postgres_url: None,
            directory: Some(temp.path().join("backups")),
            media_root: media,
            gate: Arc::new(Semaphore::new(1)),
            worker: Mutex::new(None),
            configuration: BTreeMap::new(),
            protection: Some(Protection::Encrypted(
                crate::backup_bundle::BackupKey::parse(
                    "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12",
                )
                .unwrap(),
            )),
        };
        assert!(!load_schedule(&state).await.unwrap().enabled);
        // A due hourly trigger: the scheduler pass only queues a durable task.
        crate::database::query("INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at) VALUES('hourly','backup','backup','interval',60,1,0,0)").execute(store.pool()).await.unwrap();
        let queued_runs = || async {
            crate::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM durable_tasks WHERE category='system' AND target_id='backup' AND status='queued'")
                .fetch_one(store.pool()).await.unwrap()
        };
        assert_eq!(
            crate::task_scheduler::fire_due(&state.auth, now())
                .await
                .unwrap(),
            1
        );
        assert_eq!(queued_runs().await, 1);
        // Queued is not running: the last outcome stays the previous one until the run starts.
        assert_ne!(load_schedule(&state).await.unwrap().last_outcome, "running");
        assert!(!temp.path().join("backups").exists());
        // The trigger moved on: another pass right away does not queue a second run.
        crate::task_scheduler::fire_due(&state.auth, now())
            .await
            .unwrap();
        assert_eq!(queued_runs().await, 1);
        run_scheduled(&state, None).await.unwrap();
        let schedule = load_schedule(&state).await.unwrap();
        assert_eq!(schedule.last_outcome, "succeeded");
        assert!(schedule.next_due_at > now(), "the mirror follows the trigger");
        assert_eq!(
            std::fs::read_dir(temp.path().join("backups"))
                .unwrap()
                .count(),
            1
        );
        // Failed attempts are terminal until the next firing, not retried every pass.
        state.source = Some(temp.path().join("missing.media-backup"));
        assert!(run_scheduled(&state, None).await.is_err());
        let schedule = load_schedule(&state).await.unwrap();
        assert_eq!(schedule.last_outcome, "failed");
        assert!(schedule.last_completed_at.is_some());
        assert!(schedule.next_due_at > now());
        drop(state);
        store.close().await;
        let reopened = crate::identity::IdentityStore::open(&source).await.unwrap();
        let persisted: String = crate::database::query_scalar(
            "SELECT last_outcome FROM backup_schedule WHERE singleton=1",
        )
        .fetch_one(reopened.pool())
        .await
        .unwrap();
        assert_eq!(persisted, "failed");
        reopened.close().await;
    }

    #[test]
    fn directory_policy_rejects_media_traversal_and_missing_intermediate_parents() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        assert!(safe_directory(&media.join("backups"), &media).is_err());
        assert!(safe_directory(&temp.path().join("missing").join("backups"), &media).is_err());
        assert!(safe_directory(&temp.path().join("..").join("backups"), &media).is_err());
        assert!(safe_directory(&temp.path().join("backups"), &media).is_ok());
        assert!(safe_directory(DiskPath::new("backups"), &media).is_ok());
        assert!(!valid_id("../secret"));
        assert!(!valid_id("00000000-0000-0000-0000-000000000000"));
        assert!(valid_id(&uuid::Uuid::new_v4().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn directory_policy_reports_non_private_mount_without_changing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let backups = root.path().join("backups");
        let media = root.path().join("media");
        std::fs::create_dir(&backups).unwrap();
        std::fs::create_dir(&media).unwrap();
        std::fs::set_permissions(&backups, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(safe_directory(&backups, &media), Err(StatusCode::FORBIDDEN));
        assert_eq!(
            std::fs::metadata(&backups).unwrap().permissions().mode() & 0o777,
            0o755
        );
        std::fs::set_permissions(&backups, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(safe_directory(&backups, &media).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn directory_policy_rejects_symlink_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let linked = temp.path().join("linked");
        std::os::unix::fs::symlink(&media, &linked).unwrap();
        assert!(safe_directory(&linked.join("backups"), &media).is_err());
    }
}
