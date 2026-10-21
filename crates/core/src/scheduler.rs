//! Persistent scan intervals. The process owns the worker guard; no worker is created by routers.
use crate::{auth::AuthState, identity::Role, scans::ScanJobs};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub library_id: String,
    pub interval_minutes: i64,
    // DB flags remain integers, matching the shared SQLite/PostgreSQL schema.
    pub enabled: i64,
    pub next_run_at: i64,
    pub last_run_at: Option<i64>,
    pub last_outcome: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    interval_minutes: i64,
    enabled: bool,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub async fn router(auth: AuthState) -> Result<Router, sqlx::Error> {
    crate::database::query(
        "UPDATE scheduled_scans SET last_outcome='interrupted' WHERE last_outcome='dispatching'",
    )
    .execute(&auth.store.pool)
    .await?;
    // Scan-router startup has already marked abandoned jobs as errors. Preserve
    // that terminal result immediately, even before the first worker tick.
    reconcile(&auth).await?;
    Ok(Router::new()
        .route("/api/v1/admin/tasks/schedules", get(list))
        .route(
            "/api/v1/admin/tasks/schedules/{id}",
            axum::routing::put(save).delete(remove),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth))
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
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Schedule>>, StatusCode> {
    authorize(&auth, &headers, false).await?;
    crate::database::query_as("SELECT library_id,interval_minutes,enabled,next_run_at,last_run_at,last_outcome FROM scheduled_scans ORDER BY library_id")
        .fetch_all(&auth.store.pool).await.map(Json).map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)
}
async fn save(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<StatusCode, StatusCode> {
    authorize(&auth, &headers, true).await?;
    if !(5..=10080).contains(&input.interval_minutes) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let exists:i64 = crate::database::query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.kind IN ('films','series','music') AND l.id NOT IN ('fixture-films','fixture-series') AND n.status!='revoked'")
        .bind(&id).fetch_one(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    crate::database::query("INSERT INTO scheduled_scans(library_id,interval_minutes,enabled,next_run_at) VALUES(?,?,?,?) ON CONFLICT(library_id) DO UPDATE SET interval_minutes=excluded.interval_minutes,enabled=excluded.enabled,next_run_at=excluded.next_run_at")
        .bind(id).bind(input.interval_minutes).bind(i64::from(input.enabled)).bind(now()+input.interval_minutes*60)
        .execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    authorize(&auth, &headers, true).await?;
    crate::database::query("DELETE FROM scheduled_scans WHERE library_id=?")
        .bind(id)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
/// Dropping the guard cancels and releases the worker; test routers never spawn background loops.
pub struct Scheduler(tokio::task::JoinHandle<()>);
impl Drop for Scheduler {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl Scheduler {
    pub fn start(auth: AuthState, root: PathBuf, jobs: Arc<ScanJobs>) -> Self {
        Self(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                if jobs.is_cancelled() {
                    break;
                }
                if run_due(&auth, &root, &jobs, now()).await.is_err() {
                    eprintln!("Scheduled scan tick failed; retrying on next tick");
                }
            }
        }))
    }
}
async fn reconcile(auth: &AuthState) -> Result<(), sqlx::Error> {
    crate::database::query("UPDATE scheduled_scans SET last_outcome=(SELECT status FROM scan_jobs WHERE scan_jobs.library_id=scheduled_scans.library_id) WHERE last_outcome IN ('queued','running') AND EXISTS(SELECT 1 FROM scan_jobs WHERE scan_jobs.library_id=scheduled_scans.library_id AND status IN ('completed','error'))")
        .execute(&auth.store.pool).await?;
    crate::database::query("UPDATE scheduled_scans SET last_outcome='interrupted' WHERE last_outcome IN ('queued','running') AND NOT EXISTS(SELECT 1 FROM scan_jobs WHERE scan_jobs.library_id=scheduled_scans.library_id)")
        .execute(&auth.store.pool).await?;
    Ok(())
}

/// One bounded pass, exported for deterministic integration tests and operational checks.
pub async fn run_due(
    auth: &AuthState,
    root: &std::path::Path,
    jobs: &Arc<ScanJobs>,
    timestamp: i64,
) -> Result<(), sqlx::Error> {
    let _tick = jobs.scheduler_tick.lock().await;
    // Completion is retained even when the plan is disabled after dispatch.
    reconcile(auth).await?;
    if jobs.is_cancelled() {
        return Ok(());
    }
    // At most one dispatch per pass and no queue growth behind a busy scan, local or distant.
    let active: i64 = crate::database::query_scalar(
        "SELECT COUNT(*) FROM scan_jobs WHERE status IN ('queued','running')",
    )
    .fetch_one(&auth.store.pool)
    .await?;
    if active > 0 {
        return Ok(());
    }
    let due:Option<(String,i64,i64)>=crate::database::query_as("SELECT s.library_id,s.interval_minutes,s.next_run_at FROM scheduled_scans s JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE s.enabled=1 AND s.next_run_at<=? AND l.enabled=1 AND n.status!='revoked' ORDER BY s.next_run_at,s.library_id LIMIT 1")
        .bind(timestamp).fetch_optional(&auth.store.pool).await?;
    let Some((id, minutes, previous)) = due else {
        let due: Option<(String,String,i64)> = crate::database::query_as("SELECT l.id,l.options_json,l.next_metadata_refresh_at FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.enabled=1 AND l.id NOT IN ('fixture-films','fixture-series') AND l.kind!='music' AND n.status!='revoked' AND l.next_metadata_refresh_at<=? AND CAST(COALESCE(json_extract(l.options_json,'$.automaticRefreshIntervalDays'),'0') AS BIGINT)>0 ORDER BY l.next_metadata_refresh_at,l.id LIMIT 1")
            .bind(timestamp).fetch_optional(&auth.store.pool).await?;
        if let Some((id, encoded, previous)) = due {
            let options: crate::library_options::LibraryOptions = serde_json::from_str(&encoded)
                .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
            let claimed = crate::database::query("UPDATE libraries SET next_metadata_refresh_at=? WHERE id=? AND next_metadata_refresh_at=?")
                .bind(timestamp+i64::from(options.automatic_refresh_interval_days)*86400).bind(&id).bind(previous).execute(&auth.store.pool).await?;
            if claimed.rows_affected() != 0 {
                crate::scans::enqueue(auth, root.to_owned(), jobs.clone(), id)
                    .await
                    .map_err(|_| {
                        sqlx::Error::Protocol("metadata refresh dispatch failed".into())
                    })?;
            }
        }
        return Ok(());
    };
    // Compare-and-set prevents duplicate launches even if a tick is called concurrently.
    let claim=crate::database::query("UPDATE scheduled_scans SET next_run_at=?,last_run_at=?,last_outcome='dispatching' WHERE library_id=? AND enabled=1 AND next_run_at=?")
        .bind(timestamp+minutes*60).bind(timestamp).bind(&id).bind(previous).execute(&auth.store.pool).await?;
    if claim.rows_affected() == 0 {
        return Ok(());
    }
    let outcome = match crate::scans::enqueue(auth, root.to_owned(), jobs.clone(), id.clone()).await
    {
        Ok(()) => "queued",
        Err(StatusCode::CONFLICT) => "already_running",
        Err(StatusCode::BAD_REQUEST) => "unavailable",
        Err(StatusCode::SERVICE_UNAVAILABLE) => "stopped",
        Err(_) => "error",
    };
    crate::database::query("UPDATE scheduled_scans SET last_outcome=? WHERE library_id=? AND last_run_at=? AND last_outcome='dispatching'")
        .bind(outcome).bind(id).bind(timestamp).execute(&auth.store.pool).await?;
    Ok(())
}
