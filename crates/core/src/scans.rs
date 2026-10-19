//! Small local job runner; discovery and probing remain in the Media Node.
use crate::{auth::AuthState, identity::Role, inventory, node};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::Semaphore, task::JoinHandle};

pub struct ScanJobs {
    pub(crate) scheduler_tick: tokio::sync::Mutex<()>,
    cancel: Arc<AtomicBool>,
    slots: Arc<Semaphore>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}
impl Default for ScanJobs {
    fn default() -> Self {
        Self {
            scheduler_tick: tokio::sync::Mutex::new(()),
            cancel: Arc::new(AtomicBool::new(false)),
            slots: Arc::new(Semaphore::new(1)),
            tasks: Mutex::new(Vec::new()),
        }
    }
}
impl Drop for ScanJobs {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Ok(tasks) = self.tasks.get_mut() {
            for task in tasks {
                task.abort();
            }
        }
    }
}
impl ScanJobs {
    pub fn cancel_all(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub fn reset(&self) {
        self.cancel.store(false, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub async fn shutdown(&self) {
        self.cancel.store(true, Ordering::Release);
        self.slots.close();
        let tasks = self
            .tasks
            .lock()
            .map(|mut tasks| std::mem::take(&mut *tasks))
            .unwrap_or_default();
        for task in tasks {
            let _ = task.await;
        }
    }
}
#[derive(Clone)]
struct ScanState {
    auth: AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
}
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Status {
    status: String,
    discovered: i64,
    probed: i64,
    reused: i64,
    #[sqlx(skip)]
    errors: Vec<String>,
    #[serde(skip)]
    errors_json: String,
    errors_count: i64,
    warnings_count: i64,
}
pub async fn router(
    auth: AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
) -> Result<Router, sqlx::Error> {
    // A stopped process cannot resume a probe. Keep its last inventory and report interruption.
    crate::database::query("UPDATE scan_jobs SET status='error',errors_json='[\"Scan interrompu par un redémarrage\"]',errors_count=1,warnings_count=0 WHERE status IN ('queued','running')").execute(&auth.store.pool).await?;
    crate::database::query("DELETE FROM scan_staging_files")
        .execute(&auth.store.pool)
        .await?;
    crate::database::query("DELETE FROM scan_staging_issues")
        .execute(&auth.store.pool)
        .await?;
    Ok(Router::new()
        .route("/api/v1/admin/libraries/{id}/scan", get(status).post(start))
        .with_state(ScanState { auth, root, jobs }))
}
async fn authorize(
    state: &ScanState,
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
async fn status(
    State(state): State<ScanState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Status>, StatusCode> {
    authorize(&state, &headers, false).await?;
    let exists: i64 = crate::database::query_scalar(
        "SELECT CAST(EXISTS(SELECT 1 FROM libraries WHERE id=?) AS INTEGER)",
    )
    .bind(&id)
    .fetch_one(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut status: Status = crate::database::query_as(
        "SELECT status,discovered,probed,reused,errors_json,errors_count,warnings_count FROM scan_jobs WHERE library_id=?",
    )
    .bind(id)
    .fetch_optional(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .unwrap_or(Status {
        status: "idle".into(),
        discovered: 0,
        probed: 0,
        reused: 0,
        errors: vec![],
        errors_json: "[]".into(),
        errors_count: 0,
        warnings_count: 0,
    });
    status.errors = serde_json::from_str(&status.errors_json)
        .unwrap_or_else(|_| vec!["État de scan illisible".into()]);
    Ok(Json(status))
}
async fn start(
    State(state): State<ScanState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    authorize(&state, &headers, true).await?;
    enqueue(&state.auth, state.root, state.jobs, id).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(
            serde_json::json!({"status":"queued","discovered":0,"probed":0,"reused":0,"errors":[],"errorsCount":0,"warningsCount":0}),
        ),
    ))
}

/// Shared dispatch path for authenticated manual scans and the internal scheduler.
pub(crate) async fn enqueue(
    auth: &AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
    id: String,
) -> Result<(), StatusCode> {
    let state = ScanState {
        auth: auth.clone(),
        root,
        jobs,
    };
    if state.jobs.cancel.load(Ordering::Acquire) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let library: Option<(String, String, String)> = crate::database::query_as(
        "SELECT l.root, l.kind, l.node_id FROM libraries l JOIN media_nodes n ON n.id = l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND l.id NOT IN ('fixture-films','fixture-series')",
    )
    .bind(&id)
    .fetch_optional(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (root, kind, node_id) = library.ok_or(StatusCode::BAD_REQUEST)?;
    if node_id == "local" {
        node::LocalNode
            .validate_root(std::path::Path::new(&root), &state.root)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    let result = crate::database::query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count) VALUES(?,'queued',0,0) ON CONFLICT(library_id) DO UPDATE SET status='queued',discovered=0,probed=0,reused=0,errors_json='[]',errors_count=0,warnings_count=0 WHERE scan_jobs.status NOT IN ('queued','running')").bind(&id).execute(&state.auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() == 0 {
        return Err(StatusCode::CONFLICT);
    }
    let store = state.auth.store.clone();
    let slots = state.jobs.slots.clone();
    let cancel = state.jobs.cancel.clone();
    let job_id = id.clone();
    let allowed_root = state.root.clone();
    let core_origin = state.auth.origin().to_owned();
    let task = tokio::spawn(async move {
        let scan_id = uuid::Uuid::new_v4().to_string();
        let scan_id_err = scan_id.clone();
        let store_err = store.clone();
        if node_id != "local" {
            let _ =
                crate::database::query("UPDATE scan_jobs SET status='running' WHERE library_id=?")
                    .bind(&job_id)
                    .execute(&store.pool)
                    .await;
            if let Err(err_msg) = crate::nodes::dispatch_remote_scan(
                &store,
                &core_origin,
                &scan_id,
                &job_id,
                &node_id,
                &kind,
                &root,
            )
            .await
            {
                let detailed = if err_msg.contains("os error 13")
                    || err_msg.to_lowercase().contains("permission denied")
                {
                    format!(
                        "{err_msg} (le conteneur UID 10001 n'a pas accès à ce dossier ou ses fichiers ; vérifiez que les dossiers possèdent les droits d'exécution +x et lecture hôte, ex: chmod -R o+rX)"
                    )
                } else {
                    err_msg
                };
                let errors_json = serde_json::to_string(&vec![format!("Scan échoué: {detailed}")])
                    .unwrap_or_else(|_| "[\"Scan échoué\"]".into());
                let _ = crate::database::query("UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1,warnings_count=0 WHERE library_id=?")
                    .bind(errors_json)
                    .bind(&job_id)
                    .execute(&store_err.pool)
                    .await;
            }
            return;
        }
        let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
            let _permit = slots.acquire().await?;
            if cancel.load(Ordering::Acquire) { return Err("Scan arrêté".into()); }
            crate::database::query("UPDATE scan_jobs SET status='running' WHERE library_id=?").bind(&job_id).execute(&store.pool).await?;
            let root = node::LocalNode.validate_root(std::path::Path::new(&root), &allowed_root).await?;
            let cache = inventory::load_scan_cache(&store, &job_id).await?;
            let images = inventory::load_image_cache(&store, &job_id).await?;

            let store_stage = store.clone();
            let scan_id_stage = scan_id.clone();
            let job_id_stage = job_id.clone();
            let report = node::LocalNode.scan_library_streaming(
                &root,
                &kind,
                &cache,
                &images,
                &cancel,
                100,
                move |batch| {
                    let store = store_stage.clone();
                    let scan_id = scan_id_stage.clone();
                    let job_id = job_id_stage.clone();
                    async move {
                        inventory::stage_batch(&store, &scan_id, &job_id, &batch).await
                            .map_err(|e| std::io::Error::other(e.to_string()))
                    }
                }
            ).await?;

            if cancel.load(Ordering::Acquire) {
                let _ = inventory::clean_staging(&store, &scan_id).await;
                return Err("Scan arrêté".into());
            }

            inventory::publish_staged_scan(&store, &scan_id, &job_id, &root, report.complete).await?;
            let conflicts = inventory::editorial_warnings(&store, &job_id).await?;
            let total_errors = (report.errors_count as i64) + (conflicts.len() as i64);
            let total_warnings = report.warnings_count as i64;
            let errors: Vec<String> = report.errors.iter().chain(report.warnings.iter()).map(|issue| format!("{}: {}", issue.relative_path, issue.message)).chain(conflicts).take(100).collect();

            // Purge staging tables to strictly bound SQLite storage growth
            let _ = inventory::clean_staging(&store, &scan_id).await;

            crate::database::query("UPDATE scan_jobs SET status=?,discovered=?,probed=?,reused=?,errors_json=?,errors_count=?,warnings_count=? WHERE library_id=?")
                .bind(if report.complete { "completed" } else { "error" })
                .bind(report.discovered as i64)
                .bind(report.probed as i64)
                .bind(report.reused as i64)
                .bind(serde_json::to_string(&errors)?)
                .bind(total_errors)
                .bind(total_warnings)
                .bind(&job_id)
                .execute(&store.pool)
                .await?;
            Ok(())
        }.await;
        if let Err(e) = &result {
            eprintln!("[SCAN ERROR in job {}]: {:?}", job_id, e);
            let _ = inventory::clean_staging(&store_err, &scan_id_err).await;
            let err_str = e.to_string();
            let sanitized = if let Some(root_str) = allowed_root.to_str() {
                err_str.replace(root_str, "/media")
            } else {
                err_str
            };
            let detailed_msg = if sanitized.contains("os error 13")
                || sanitized.to_lowercase().contains("permission denied")
            {
                format!(
                    "{sanitized} (le conteneur UID 10001 n'a pas accès à ce dossier ou ses fichiers ; vérifiez que les dossiers possèdent les droits d'exécution +x et lecture hôte, ex: chmod -R o+rX)"
                )
            } else {
                sanitized
            };
            let err_msg = format!("Scan échoué: {detailed_msg}");
            let errors_json = serde_json::to_string(&vec![err_msg])
                .unwrap_or_else(|_| "[\"Scan échoué ou interrompu\"]".into());
            let _ =
                crate::database::query("UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1,warnings_count=0 WHERE library_id=?")
                    .bind(errors_json)
                    .bind(&job_id)
                    .execute(&store.pool)
                    .await;
        }
    });
    let mut tasks = state
        .jobs
        .tasks
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tasks.retain(|task| !task.is_finished());
    tasks.push(task);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, header},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use zeroize::Zeroizing;

    #[tokio::test]
    async fn queued_scan_is_exclusive_and_shutdown_retains_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
        let store = crate::identity::IdentityStore::open(&root.join("db.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap(
                "scan-admin",
                Zeroizing::new("synthetic job test password".into()),
            )
            .await
            .unwrap();
        crate::database::query("INSERT INTO libraries VALUES('films','Films','films','local',?,1)")
            .bind(root.to_str().unwrap())
            .execute(&store.pool)
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let response = crate::auth::router(auth.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header(header::ORIGIN, "http://127.0.0.1:3000")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"login":"scan-admin","password":"synthetic job test password"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let login: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        headers.insert(header::ORIGIN, "http://127.0.0.1:3000".parse().unwrap());
        headers.insert(
            "x-csrf-token",
            login["csrfToken"].as_str().unwrap().parse().unwrap(),
        );
        let jobs = Arc::new(ScanJobs::default());
        // Hold the only probe slot to prove queued jobs cannot race a second launch.
        let permit = jobs.slots.clone().acquire_owned().await.unwrap();
        let state = ScanState {
            auth: auth.clone(),
            root: root.clone(),
            jobs: jobs.clone(),
        };
        assert_eq!(
            start(State(state.clone()), Path("films".into()), headers.clone())
                .await
                .unwrap()
                .0,
            StatusCode::ACCEPTED
        );
        assert_eq!(
            start(State(state.clone()), Path("films".into()), headers.clone())
                .await
                .err(),
            Some(StatusCode::CONFLICT)
        );
        assert_eq!(
            status(State(state.clone()), Path("films".into()), headers.clone())
                .await
                .unwrap()
                .0
                .status,
            "queued"
        );
        jobs.shutdown().await;
        drop(permit);
        assert_eq!(
            status(State(state.clone()), Path("films".into()), headers.clone())
                .await
                .unwrap()
                .0
                .status,
            "error"
        );
        assert_eq!(
            start(State(state), Path("films".into()), headers)
                .await
                .err(),
            Some(StatusCode::SERVICE_UNAVAILABLE)
        );
        crate::database::query("UPDATE scan_jobs SET status='running'")
            .execute(&store.pool)
            .await
            .unwrap();
        let _ = router(auth, root, Arc::new(ScanJobs::default()))
            .await
            .unwrap();
        let status: String =
            crate::database::query_scalar("SELECT status FROM scan_jobs WHERE library_id='films'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(status, "error");
        store.close().await;
    }
}
