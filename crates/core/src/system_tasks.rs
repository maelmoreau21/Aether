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

pub struct SystemTasksState {
    pub auth: AuthState,
    pub media_root: PathBuf,
    pub gate: Arc<Mutex<()>>,
}

pub async fn router(auth: AuthState, media_root: PathBuf) -> Result<Router, sqlx::Error> {
    // On startup, reset interrupted tasks
    crate::database::query(
        "UPDATE system_task_schedules SET last_outcome='interrupted', last_completed_at=? WHERE last_outcome='running'",
    )
    .bind(now())
    .execute(&auth.store.pool)
    .await?;

    let state = Arc::new(SystemTasksState {
        auth,
        media_root,
        gate: Arc::new(Mutex::new(())),
    });

    start_worker(&state);

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

async fn list_tasks(
    State(state): State<Arc<SystemTasksState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SystemTaskSchedule>>, StatusCode> {
    authorize(&state.auth, &headers, false).await?;
    let tasks: Vec<SystemTaskSchedule> = crate::database::query_as(
        "SELECT id, name, description, category, interval_minutes, enabled, next_due_at, last_started_at, last_completed_at, last_outcome FROM system_task_schedules ORDER BY category, name"
    )
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

    // Run execution in the background under the gate so we don't block the HTTP request
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        let _ = execute_task(&state_clone, &id).await;
    });

    Ok(StatusCode::ACCEPTED)
}

pub async fn execute_task(state: &SystemTasksState, id: &str) -> Result<(), StatusCode> {
    let _guard = state.gate.lock().await;

    let started = now();
    // Claim task execution
    let claimed = crate::database::query(
        "UPDATE system_task_schedules SET last_started_at=?, last_completed_at=NULL, last_outcome='running' WHERE id=? AND last_outcome!='running'"
    )
    .bind(started)
    .bind(id)
    .execute(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if claimed.rows_affected() == 0 {
        return Ok(());
    }

    let outcome_result = match id {
        "update-extensions" => task_update_extensions(state).await,
        "optimize-database" => task_optimize_database(state).await,
        "clean-sessions" => task_clean_sessions(state).await,
        "clean-cache" => task_clean_cache(state).await,
        "clean-playlists" => task_clean_playlists(state).await,
        _ => Err(StatusCode::NOT_FOUND),
    };

    let outcome = if outcome_result.is_ok() {
        "succeeded"
    } else {
        "failed"
    };

    let _ = crate::database::query(
        "UPDATE system_task_schedules SET last_completed_at=?, last_outcome=? WHERE id=? AND last_started_at=?"
    )
    .bind(now())
    .bind(outcome)
    .bind(id)
    .bind(started)
    .execute(&state.auth.store.pool)
    .await;

    outcome_result
}

/// 1. update-extensions: Refresh extension repositories and catalogs
async fn task_update_extensions(state: &SystemTasksState) -> Result<(), StatusCode> {
    let repos: Vec<(String, String)> =
        crate::database::query_as("SELECT id, url FROM extension_repositories ORDER BY id")
            .fetch_all(&state.auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    for (id, url) in repos {
        if let Ok(bytes) = crate::extensions::repositories::fetch(&url, 1024 * 1024).await {
            let manifest_str = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let _ = crate::database::query(
                "UPDATE extension_repositories SET manifest_json=?, refreshed_at=? WHERE id=?",
            )
            .bind(manifest_str)
            .bind(now())
            .bind(id)
            .execute(&state.auth.store.pool)
            .await;
        }
    }

    Ok(())
}

/// 2. optimize-database: SQLite PRAGMA optimize / WAL checkpoint or PostgreSQL ANALYZE
async fn task_optimize_database(state: &SystemTasksState) -> Result<(), StatusCode> {
    if let Ok(conn) = state.auth.store.pool.acquire().await {
        if conn.backend_name() == "SQLite" {
            let _ = crate::database::query("PRAGMA optimize")
                .execute(&state.auth.store.pool)
                .await;
            let _ = crate::database::query("PRAGMA wal_checkpoint(PASSIVE)")
                .execute(&state.auth.store.pool)
                .await;
        } else {
            let _ = crate::database::query("ANALYZE")
                .execute(&state.auth.store.pool)
                .await;
        }
    }
    Ok(())
}

/// 3. clean-sessions: Delete expired and revoked sessions and tokens
async fn task_clean_sessions(state: &SystemTasksState) -> Result<(), StatusCode> {
    let current_time = now();
    // Inactivity threshold: 30 days
    let inactivity_threshold = current_time - (30 * 86400);

    let _ = crate::database::query("DELETE FROM sessions WHERE expires_at<=? OR last_seen_at<=?")
        .bind(current_time)
        .bind(inactivity_threshold)
        .execute(&state.auth.store.pool)
        .await;

    // Purge expired oidc flows if table exists
    let _ = crate::database::query("DELETE FROM oidc_flows WHERE created_at<=?")
        .bind(current_time - 3600)
        .execute(&state.auth.store.pool)
        .await;

    Ok(())
}

/// 4. clean-cache: Remove stale poster previews and orphaned temporary files
async fn task_clean_cache(state: &SystemTasksState) -> Result<(), StatusCode> {
    // Delete poster_previews for works that no longer have a poster or no longer exist
    let _ = crate::database::query(
        "DELETE FROM poster_previews WHERE work_id NOT IN (SELECT work_id FROM work_posters)",
    )
    .execute(&state.auth.store.pool)
    .await;

    // Clean .tmp files in media root or app caches if any
    if let Ok(mut entries) = tokio::fs::read_dir(&state.media_root).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if (file_name.starts_with('.') && file_name.ends_with(".tmp"))
                || file_name.ends_with(".part")
            {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }

    Ok(())
}

/// 5. clean-playlists: Remove references in playlists to missing works or deleted music tracks
async fn task_clean_playlists(state: &SystemTasksState) -> Result<(), StatusCode> {
    // Remove personal_list_items for works that do not exist in inventory or works table
    let _ = crate::database::query(
        "DELETE FROM personal_list_items WHERE work_id NOT IN (SELECT id FROM works)",
    )
    .execute(&state.auth.store.pool)
    .await;

    // Remove playlist_music_items for tracks that no longer exist
    let _ = crate::database::query(
        "DELETE FROM playlist_music_items WHERE track_id NOT IN (SELECT id FROM music_tracks)",
    )
    .execute(&state.auth.store.pool)
    .await;

    Ok(())
}

async fn scheduled_tick(state: &SystemTasksState) {
    let current_time = now();
    // Query due tasks that are enabled
    let due_tasks: Vec<(String, i64)> = match crate::database::query_as(
        "SELECT id, interval_minutes FROM system_task_schedules WHERE enabled=1 AND next_due_at<=? AND last_outcome!='running' ORDER BY next_due_at"
    )
    .bind(current_time)
    .fetch_all(&state.auth.store.pool)
    .await
    {
        Ok(t) => t,
        Err(_) => return,
    };

    for (id, interval_minutes) in due_tasks {
        let next_due = current_time + (interval_minutes * 60);
        let _ = crate::database::query("UPDATE system_task_schedules SET next_due_at=? WHERE id=?")
            .bind(next_due)
            .bind(&id)
            .execute(&state.auth.store.pool)
            .await;

        let _ = execute_task(state, &id).await;
    }
}

fn start_worker(state: &Arc<SystemTasksState>) {
    let weak = Arc::downgrade(state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let Some(state) = weak.upgrade() else {
                break;
            };
            scheduled_tick(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_system_tasks_seeded_and_claimable() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("test.sqlite3");
        let store = crate::identity::IdentityStore::open(&db_path)
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), "http://localhost:3000".to_string())
            .await
            .unwrap();

        let tasks: Vec<SystemTaskSchedule> = crate::database::query_as(
            "SELECT id, name, description, category, interval_minutes, enabled, next_due_at, last_started_at, last_completed_at, last_outcome FROM system_task_schedules ORDER BY id"
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();

        assert_eq!(tasks.len(), 5);
        assert!(
            tasks
                .iter()
                .any(|t| t.id == "update-extensions" && t.enabled == 1)
        );
        assert!(
            tasks
                .iter()
                .any(|t| t.id == "optimize-database" && t.enabled == 1)
        );
        assert!(
            tasks
                .iter()
                .any(|t| t.id == "clean-sessions" && t.enabled == 1)
        );
        assert!(
            tasks
                .iter()
                .any(|t| t.id == "clean-cache" && t.enabled == 1)
        );
        assert!(
            tasks
                .iter()
                .any(|t| t.id == "clean-playlists" && t.enabled == 1)
        );

        let state = SystemTasksState {
            auth,
            media_root: temp.path().to_path_buf(),
            gate: Arc::new(Mutex::new(())),
        };

        // Execution of optimize-database
        let res = execute_task(&state, "optimize-database").await;
        assert!(res.is_ok());

        let updated: SystemTaskSchedule = crate::database::query_as(
            "SELECT id, name, description, category, interval_minutes, enabled, next_due_at, last_started_at, last_completed_at, last_outcome FROM system_task_schedules WHERE id='optimize-database'"
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();

        assert_eq!(updated.last_outcome, "succeeded");
        assert!(updated.last_completed_at.is_some());
    }
}
