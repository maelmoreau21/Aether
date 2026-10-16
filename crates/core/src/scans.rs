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
    cancel: Arc<AtomicBool>,
    slots: Arc<Semaphore>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}
impl Default for ScanJobs {
    fn default() -> Self {
        Self {
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
struct Status {
    status: String,
    discovered: i64,
    probed: i64,
    reused: i64,
    #[sqlx(skip)]
    errors: Vec<String>,
    #[serde(skip)]
    errors_json: String,
}
pub async fn router(
    auth: AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
) -> Result<Router, sqlx::Error> {
    // A stopped process cannot resume a probe. Keep its last inventory and report interruption.
    sqlx::query("UPDATE scan_jobs SET status='error',errors_json='[\"Scan interrompu par un redémarrage\"]' WHERE status IN ('queued','running')").execute(&auth.store.pool).await?;
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
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM libraries WHERE id=?)")
        .bind(&id)
        .fetch_one(&state.auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut status: Status = sqlx::query_as(
        "SELECT status,discovered,probed,reused,errors_json FROM scan_jobs WHERE library_id=?",
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
    if state.jobs.cancel.load(Ordering::Acquire) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let root: Option<String> = sqlx::query_scalar("SELECT root FROM libraries WHERE id=? AND enabled=1 AND kind='films' AND node_id='local' AND id NOT IN ('fixture-films','fixture-series')").bind(&id).fetch_optional(&state.auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let root = root.ok_or(StatusCode::BAD_REQUEST)?;
    node::validate_library_root(std::path::Path::new(&root), &state.root)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let result = sqlx::query("INSERT INTO scan_jobs(library_id,status) VALUES(?,'queued') ON CONFLICT(library_id) DO UPDATE SET status='queued',discovered=0,probed=0,reused=0,errors_json='[]' WHERE scan_jobs.status NOT IN ('queued','running')").bind(&id).execute(&state.auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() == 0 {
        return Err(StatusCode::CONFLICT);
    }
    let store = state.auth.store.clone();
    let slots = state.jobs.slots.clone();
    let cancel = state.jobs.cancel.clone();
    let job_id = id.clone();
    let allowed_root = state.root.clone();
    let task = tokio::spawn(async move {
        let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
            let _permit = slots.acquire().await?;
            if cancel.load(Ordering::Acquire) { return Err("Scan arrêté".into()); }
            sqlx::query("UPDATE scan_jobs SET status='running' WHERE library_id=?").bind(&job_id).execute(&store.pool).await?;
            let root = node::validate_library_root(std::path::Path::new(&root), &allowed_root).await?;
            let cache = inventory::load_cache(&store, &job_id).await?;
            let report = node::scanner::scan(&root, &cache, &cancel).await?;
            if cancel.load(Ordering::Acquire) { return Err("Scan arrêté".into()); }
            inventory::ingest(&store, &job_id, &root, &report).await?;
            let conflicts = inventory::editorial_warnings(&store, &job_id).await?;
            let errors: Vec<String> = report.errors.iter().chain(report.warnings.iter()).map(|issue| format!("{}: {}", issue.relative_path, issue.message)).chain(conflicts).take(100).collect();
            sqlx::query("UPDATE scan_jobs SET status=?,discovered=?,probed=?,reused=?,errors_json=? WHERE library_id=?")
                .bind(if report.complete { "completed" } else { "error" }).bind(report.discovered as i64).bind(report.probed as i64).bind(report.reused as i64).bind(serde_json::to_string(&errors)?).bind(&job_id).execute(&store.pool).await?;
            Ok(())
        }.await;
        if result.is_err() {
            let _ = sqlx::query("UPDATE scan_jobs SET status='error',errors_json='[\"Scan échoué ou interrompu ; inventaire précédent conservé\"]' WHERE library_id=?").bind(&job_id).execute(&store.pool).await;
        }
    });
    let mut tasks = state
        .jobs
        .tasks
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tasks.retain(|task| !task.is_finished());
    tasks.push(task);
    Ok((
        StatusCode::ACCEPTED,
        Json(
            serde_json::json!({"status":"queued","discovered":0,"probed":0,"reused":0,"errors":[]}),
        ),
    ))
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
        sqlx::query("INSERT INTO libraries VALUES('films','Films','films','local',?,1)")
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
        sqlx::query("UPDATE scan_jobs SET status='running'")
            .execute(&store.pool)
            .await
            .unwrap();
        let _ = router(auth, root, Arc::new(ScanJobs::default()))
            .await
            .unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM scan_jobs WHERE library_id='films'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(status, "error");
        store.close().await;
    }
}
