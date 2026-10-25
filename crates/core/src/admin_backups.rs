//! Administrator-only encrypted SQLite/PostgreSQL backups. Archive names and source paths are server owned.
use crate::{auth::AuthState, identity::Role};
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

pub use crate::backup_bundle::DEFAULT_BACKUP_KEY;

struct Backups {
    auth: AuthState,
    source: Option<PathBuf>,
    postgres_url: Option<Arc<zeroize::Zeroizing<String>>>,
    directory: Option<PathBuf>,
    media_root: PathBuf,
    gate: Arc<Semaphore>,
    worker: Mutex<Option<tokio::task::AbortHandle>>,
    key: Option<Arc<zeroize::Zeroizing<String>>>,
    configuration: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Archive {
    id: String,
    size: u64,
    created_at: u64,
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
    let key = std::env::var("MEDIA_BACKUP_KEY").ok();
    let directory = std::env::var_os("MEDIA_BACKUP_DIR").map(PathBuf::from);
    let configuration = std::env::vars()
        .filter(|(name, _)| {
            (name.starts_with("MEDIA_") || name.starts_with("OIDC_"))
                && !matches!(name.as_str(), "MEDIA_BACKUP_KEY" | "MEDIA_BACKUP_KEY_FILE")
        })
        .collect();
    router_with_configuration(auth, media_root, directory, key, configuration).await
}

/// Explicit operator/test configuration; never exposed as an HTTP payload.
pub async fn router_with_configuration(
    auth: AuthState,
    media_root: PathBuf,
    directory: Option<PathBuf>,
    key: Option<String>,
    mut configuration: BTreeMap<String, String>,
) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    configuration.remove("MEDIA_BACKUP_KEY");
    configuration.remove("MEDIA_BACKUP_KEY_FILE");
    let key = key
        .filter(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| DEFAULT_BACKUP_KEY.to_string());
    let key = Some(Arc::new(zeroize::Zeroizing::new(key)));
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
        .unwrap_or(std::env::current_dir()?.join("backups"));
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
    Ok(router_with_state(Backups {
        auth,
        source,
        postgres_url,
        directory,
        media_root,
        gate: Arc::new(Semaphore::new(1)),
        worker: Mutex::new(None),
        key,
        configuration,
    }))
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
            return Err(StatusCode::CONFLICT);
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
                    tokio::task::spawn_blocking(move || -> Result<Vec<Archive>, StatusCode> {
                        let entries = match std::fs::read_dir(&directory) {
                            Ok(entries) => entries,
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                return Ok(Vec::new());
                            }
                            Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
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
                                let metadata = std::fs::symlink_metadata(entry.path())
                                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
                    _ => (
                        false,
                        Some("Impossible de lire les archives de sauvegarde."),
                        Vec::new(),
                    ),
                }
            }
            Err(_) => (
                false,
                Some(
                    "Le dossier de sauvegarde n'est pas accessible ou son emplacement est refusé.",
                ),
                Vec::new(),
            ),
        }
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

async fn perform_backup(state: &Backups) -> Result<Archive, StatusCode> {
    let fallback_key = DEFAULT_BACKUP_KEY.to_string();
    let key = state
        .key
        .as_ref()
        .map(|k| k.as_str())
        .unwrap_or(&fallback_key);
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
        let builder = std::fs::DirBuilder::new();
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
        result
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        "postgres"
    } else if let Some(source) = &state.source {
        crate::backup::backup_database(source, &temporary.0, Some(&state.media_root))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        "sqlite"
    } else {
        return Err(StatusCode::NOT_IMPLEMENTED);
    };
    let size = crate::backup_bundle::create(
        &temporary.0,
        state.configuration.clone(),
        &directory.join(format!("{id}.media-backup")),
        key,
        engine,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Archive {
        id,
        size,
        created_at: now() as u64,
    })
}

async fn create(
    State(state): State<Arc<Backups>>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    administrator(&state, &headers, true).await?;
    if state.source.is_none() && state.postgres_url.is_none() {
        return Err(StatusCode::NOT_IMPLEMENTED);
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
    crate::database::query(
        "UPDATE backup_schedule SET enabled=?,interval_minutes=?,next_due_at=? WHERE singleton=1",
    )
    .bind(i64::from(input.enabled))
    .bind(input.interval_minutes)
    .bind(now() + input.interval_minutes * 60)
    .execute(state.auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(load_schedule(&state).await?),
    )
        .into_response())
}

async fn scheduled_tick(state: &Backups) -> Result<(), StatusCode> {
    if state.source.is_none() && state.postgres_url.is_none() {
        return Ok(());
    }
    let schedule = load_schedule(state).await?;
    let started = now();
    if !schedule.enabled || schedule.next_due_at > started {
        return Ok(());
    }
    let Ok(permit) = state.gate.clone().try_acquire_owned() else {
        return Ok(());
    };
    // Atomic claim across router instances; advance from now to avoid missed-run storms.
    let claimed = crate::database::query("UPDATE backup_schedule SET next_due_at=?,last_started_at=?,last_completed_at=NULL,last_outcome='running' WHERE singleton=1 AND enabled=1 AND next_due_at=? AND next_due_at<=?")
        .bind(started + schedule.interval_minutes * 60).bind(started).bind(schedule.next_due_at).bind(started)
        .execute(state.auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if claimed.rows_affected() == 0 {
        return Ok(());
    }
    let result = perform_backup(state).await;
    crate::database::query("UPDATE backup_schedule SET last_completed_at=?,last_outcome=? WHERE singleton=1 AND last_started_at=?")
        .bind(now()).bind(if result.is_ok() {"succeeded"} else {"failed"}).bind(started)
        .execute(state.auth.store.pool()).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    drop(permit);
    Ok(())
}

fn start_worker(state: &Arc<Backups>) {
    let weak = Arc::downgrade(state);
    let task = tokio::spawn(async move {
        // On startup distinguish an interrupted scheduled attempt from completion.
        if let Some(state) = weak.upgrade() {
            let _ = crate::database::query("UPDATE backup_schedule SET last_outcome='interrupted',last_completed_at=? WHERE singleton=1 AND last_outcome='running'")
                .bind(now()).execute(state.auth.store.pool()).await;
        }
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            let Some(state) = weak.upgrade() else {
                break;
            };
            let _ = scheduled_tick(&state).await;
            // Drop strong state before sleeping; worker never owns its own lifetime.
        }
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
            key: Some(Arc::new(zeroize::Zeroizing::new("11".repeat(32)))),
            configuration: BTreeMap::new(),
        };
        assert!(!load_schedule(&state).await.unwrap().enabled);
        crate::database::query("UPDATE backup_schedule SET enabled=1,interval_minutes=60,next_due_at=0 WHERE singleton=1").execute(store.pool()).await.unwrap();
        let permit = state.gate.clone().try_acquire_owned().unwrap();
        scheduled_tick(&state).await.unwrap();
        assert_eq!(load_schedule(&state).await.unwrap().next_due_at, 0);
        drop(permit);
        scheduled_tick(&state).await.unwrap();
        let schedule = load_schedule(&state).await.unwrap();
        assert_eq!(schedule.last_outcome, "succeeded");
        assert!(schedule.next_due_at > now());
        scheduled_tick(&state).await.unwrap();
        assert_eq!(
            std::fs::read_dir(temp.path().join("backups"))
                .unwrap()
                .count(),
            1
        );
        // Failed attempts are terminal until the next interval, not retried every tick.
        state.source = Some(temp.path().join("missing.media-backup"));
        crate::database::query("UPDATE backup_schedule SET next_due_at=0 WHERE singleton=1")
            .execute(store.pool())
            .await
            .unwrap();
        scheduled_tick(&state).await.unwrap();
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
    fn directory_policy_rejects_symlink_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let linked = temp.path().join("linked");
        std::os::unix::fs::symlink(&media, &linked).unwrap();
        assert!(safe_directory(&linked.join("backups"), &media).is_err());
    }
}
