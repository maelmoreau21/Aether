//! Small local job runner; discovery and probing remain in the Media Node.
use crate::{auth::AuthState, identity::Role, inventory, node};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{sync::Semaphore, task::JoinHandle};
pub(crate) static LOCAL_STREAMS: std::sync::LazyLock<Arc<std::sync::atomic::AtomicU32>> =
    std::sync::LazyLock::new(|| Arc::new(std::sync::atomic::AtomicU32::new(0)));

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefreshMode {
    #[default]
    Incremental,
    NewOnly,
    MissingMetadata,
    Full,
}
impl RefreshMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::NewOnly => "new_only",
            Self::MissingMetadata => "missing_metadata",
            Self::Full => "full",
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefreshOptions {
    #[serde(default)]
    mode: RefreshMode,
}

pub struct ScanJobs {
    dispatch: tokio::sync::Mutex<()>,
    pub(crate) engine_busy: AtomicBool,
    pub(crate) local_resources: std::sync::OnceLock<media_node::resources::Sampler>,
    pub(crate) engine_tick: tokio::sync::Mutex<()>,
    pub(crate) scheduler_tick: tokio::sync::Mutex<()>,
    cancel: Arc<AtomicBool>,
    pub(crate) slots: Arc<Semaphore>,
    pub(crate) preparations: Arc<media_node::packaging::Preparations>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    pub(crate) cancellations: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
}
impl Default for ScanJobs {
    fn default() -> Self {
        Self {
            dispatch: tokio::sync::Mutex::new(()),
            engine_busy: AtomicBool::new(false),
            local_resources: std::sync::OnceLock::new(),
            engine_tick: tokio::sync::Mutex::new(()),
            scheduler_tick: tokio::sync::Mutex::new(()),
            cancel: Arc::new(AtomicBool::new(false)),
            slots: Arc::new(Semaphore::new(1)),
            preparations: Arc::new(media_node::packaging::Preparations::default()),
            tasks: Mutex::new(Vec::new()),
            cancellations: Mutex::new(Default::default()),
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
        if let Ok(tokens) = self.cancellations.lock() {
            for token in tokens.values() {
                token.store(true, Ordering::Release);
            }
        }
    }

    pub(crate) fn cancel_scan(&self, scan_id: &str) {
        if let Ok(tokens) = self.cancellations.lock()
            && let Some(token) = tokens.get(scan_id)
        {
            token.store(true, Ordering::Release);
        }
    }

    pub(crate) fn scan_active(&self, scan_id: &str) -> bool {
        self.cancellations
            .lock()
            .map(|tokens| tokens.contains_key(scan_id))
            .unwrap_or(true)
    }

    pub(crate) fn track_control(&self, task: JoinHandle<()>) {
        if let Ok(mut tasks) = self.tasks.lock() {
            tasks.retain(|task| !task.is_finished());
            tasks.push(task);
        } else {
            task.abort();
        }
    }

    pub fn reset(&self) {
        self.cancel.store(false, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub async fn shutdown(&self) {
        self.cancel_all();
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
    crate::database::query("DELETE FROM scan_batch_receipts")
        .execute(&auth.store.pool)
        .await?;
    crate::database::query("DELETE FROM active_scan_runs")
        .execute(&auth.store.pool)
        .await?;
    crate::database::query("DELETE FROM scan_staging_files")
        .execute(&auth.store.pool)
        .await?;
    crate::database::query("DELETE FROM scan_staging_issues")
        .execute(&auth.store.pool)
        .await?;
    Ok(Router::new()
        .route("/api/v1/admin/libraries/{id}/scan", get(status).post(start))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(ScanState { auth, root, jobs }))
}
async fn authorize(
    state: &ScanState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<String, StatusCode> {
    let identity = state.auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        state.auth.require_csrf(headers, &identity)?;
    }
    Ok(identity.user.id)
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
    let statuses: Vec<Status> = crate::database::query_as(
        "SELECT status,discovered,probed,reused,errors_json,errors_count,warnings_count FROM scan_jobs WHERE library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)",
    )
    .bind(&id)
    .bind(&id)
    .fetch_all(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut status = Status {
        status: "idle".into(),
        discovered: 0,
        probed: 0,
        reused: 0,
        errors: vec![],
        errors_json: "[]".into(),
        errors_count: 0,
        warnings_count: 0,
    };
    for item in statuses {
        let rank = |s: &str| match s {
            "running" => 5,
            "queued" => 4,
            "error" => 3,
            "completed" => 2,
            _ => 1,
        };
        if rank(&item.status) > rank(&status.status) {
            status.status = item.status;
        }
        status.discovered += item.discovered;
        status.probed += item.probed;
        status.reused += item.reused;
        status.errors_count += item.errors_count;
        status.warnings_count += item.warnings_count;
        status.errors.extend(
            serde_json::from_str::<Vec<String>>(&item.errors_json)
                .unwrap_or_else(|_| vec!["État de scan illisible".into()]),
        );
    }
    Ok(Json(status))
}
async fn start(
    State(state): State<ScanState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    let owner = authorize(&state, &headers, true).await?;
    let options: RefreshOptions = if body.is_empty() {
        RefreshOptions::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?
    };
    enqueue_for_owner(
        &state.auth,
        state.root,
        state.jobs,
        id,
        options.mode,
        &owner,
    )
    .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(
            serde_json::json!({"status":"queued","discovered":0,"probed":0,"reused":0,"errors":[],"errorsCount":0,"warningsCount":0}),
        ),
    ))
}

async fn enqueue_for_owner(
    auth: &AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
    id: String,
    mode: RefreshMode,
    owner: &str,
) -> Result<(), StatusCode> {
    if jobs.is_cancelled() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let dispatch = jobs.dispatch.lock().await;
    let mut scopes: Vec<String> = crate::database::query_scalar(
        "SELECT library_id FROM library_folders WHERE parent_id=? AND active=1 ORDER BY library_id",
    )
    .bind(&id)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    scopes.push(id.clone());
    let active: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND status IN ('queued','running','paused')")
        .bind(&id).bind(&id).fetch_one(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if active != 0 {
        return Err(StatusCode::CONFLICT);
    }
    let timestamp = media_contracts::protocol::unix_now();
    for scope in scopes {
        let key = uuid::Uuid::new_v4().to_string();
        crate::task_engine::enqueue_mode(
            &auth.store,
            &scope,
            owner,
            &key,
            200,
            timestamp,
            mode.as_str(),
        )
        .await
        .map_err(|err| match err {
            crate::task_engine::EngineError::Unavailable
            | crate::task_engine::EngineError::Invalid => StatusCode::BAD_REQUEST,
            crate::task_engine::EngineError::Capacity => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    }
    drop(dispatch);
    crate::scheduler::dispatch_once(auth, &root, &jobs, timestamp)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}
/// Execute one already leased physical scope; never expands the parent a second time.
pub(crate) async fn execute_leased(
    auth: &AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
    id: String,
    scan_id: String,
    mode: RefreshMode,
) -> Result<(), StatusCode> {
    let _dispatch = jobs.dispatch.lock().await;
    enqueue_one(auth, root, jobs.clone(), id, mode, Some(scan_id)).await
}
async fn enqueue_one(
    auth: &AuthState,
    root: PathBuf,
    jobs: Arc<ScanJobs>,
    id: String,
    mode: RefreshMode,
    supplied_scan_id: Option<String>,
) -> Result<(), StatusCode> {
    let state = ScanState {
        auth: auth.clone(),
        root,
        jobs,
    };
    if state.jobs.cancel.load(Ordering::Acquire) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let library: Option<(String, String, String, String)> = crate::database::query_as(
        "SELECT l.root, l.kind, l.node_id, l.excluded_patterns FROM libraries l JOIN media_nodes n ON n.id = l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films', 'series', 'music') AND n.status != 'revoked' AND l.id NOT IN ('fixture-films','fixture-series')",
    )
    .bind(&id)
    .fetch_optional(&state.auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (root, kind, node_id, excluded_patterns_raw) = library.ok_or(StatusCode::BAD_REQUEST)?;
    let excluded_patterns: Vec<String> = excluded_patterns_raw
        .split(&[',', ';', '\n', '\r'][..])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if node_id == "local" {
        crate::local_mounts::validate(std::path::Path::new(&root), &state.root)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    let result = crate::database::query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count,refresh_mode) VALUES(?,'queued',0,0,?) ON CONFLICT(library_id) DO UPDATE SET refresh_mode=excluded.refresh_mode,status='queued',discovered=0,probed=0,reused=0,errors_json='[]',errors_count=0,warnings_count=0 WHERE scan_jobs.status NOT IN ('queued','running')").bind(&id).bind(mode.as_str()).execute(&state.auth.store.pool).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if result.rows_affected() == 0 {
        return Err(StatusCode::CONFLICT);
    }
    let scan_id = supplied_scan_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    crate::database::query("INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES(?,?,?) ON CONFLICT(library_id) DO UPDATE SET scan_id=excluded.scan_id,node_id=excluded.node_id")
        .bind(&id).bind(&scan_id).bind(&node_id).execute(&state.auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let store = state.auth.store.clone();
    let slots = state.jobs.slots.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .jobs
        .cancellations
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .insert(scan_id.clone(), cancel.clone());
    let runner_jobs = state.jobs.clone();
    let job_id = id.clone();
    let allowed_root = state.root.clone();
    let core_origin = state.auth.origin().to_owned();
    let task = tokio::spawn(async move {
        let scan_id_err = scan_id.clone();
        let store_err = store.clone();
        if node_id != "local" {
            let _ =
                crate::database::query("UPDATE scan_jobs SET status='running' WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)")
                    .bind(&job_id).bind(&scan_id)
                    .execute(&store.pool)
                    .await;
            let excluded_for_remote = excluded_patterns.clone();
            if let Err(err_msg) = crate::nodes::dispatch_remote_scan(
                &store,
                &core_origin,
                &scan_id,
                &job_id,
                &node_id,
                &kind,
                &root,
                mode == RefreshMode::Full,
                excluded_for_remote,
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
                let _ = crate::database::query("UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1,warnings_count=0 WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)")
                    .bind(errors_json)
                    .bind(&job_id).bind(&scan_id)
                    .execute(&store_err.pool)
                    .await;
            }
            if let Ok(mut tokens) = runner_jobs.cancellations.lock() {
                tokens.remove(&scan_id);
            }
            return;
        }
        let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
            let _permit = slots.acquire().await?;
            if cancel.load(Ordering::Acquire) || runner_jobs.is_cancelled() { return Err("Scan arrêté".into()); }
            crate::database::query("UPDATE scan_jobs SET status='running' WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)").bind(&job_id).bind(&scan_id).execute(&store.pool).await?;
            let root = crate::local_mounts::validate(std::path::Path::new(&root), &allowed_root).await?;
            if kind == "music" {
                let count = crate::music::scan_fenced(&store, &job_id, &root, &cancel, Some(&scan_id)).await?;
                crate::database::query("UPDATE scan_jobs SET status='completed',discovered=?,probed=?,reused=0 WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)").bind(count as i64).bind(count as i64).bind(&job_id).bind(&scan_id).execute(&store.pool).await?;
                return Ok(());
            }
            let cache = if mode == RefreshMode::Full { Default::default() } else { inventory::load_scan_cache(&store, &job_id).await? };
            let images = if mode == RefreshMode::Full { Default::default() } else { inventory::load_image_cache(&store, &job_id).await? };

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
                &excluded_patterns,
                move |batch| {
                    let store = store_stage.clone();
                    let scan_id = scan_id_stage.clone();
                    let job_id = job_id_stage.clone();
                    async move {
                        let _effects = crate::task_engine::EFFECT_GATE.lock().await;
                        if !crate::nodes::scan_run_authorized(&store,&job_id,"local",&scan_id).await {
                            return Err(std::io::Error::other("Scan attempt fenced"));
                        }
                        inventory::stage_batch(&store, &scan_id, &job_id, &batch).await
                            .map_err(|e| std::io::Error::other(e.to_string()))
                    }
                }
            ).await?;

            if cancel.load(Ordering::Acquire) {
                let _ = inventory::clean_staging(&store, &scan_id).await;
                return Err("Scan stopped or preempted for playback".into());
            }

            let _effects = crate::task_engine::EFFECT_GATE.lock().await;
            if !crate::nodes::scan_run_authorized(&store, &job_id, "local", &scan_id).await {
                return Err("Tentative de scan expirée".into());
            }
            inventory::publish_staged_scan(&store, &scan_id, &job_id, &root, report.complete).await?;
            let conflicts = inventory::editorial_warnings(&store, &job_id).await?;
            let total_errors = (report.errors_count as i64) + (conflicts.len() as i64);
            let total_warnings = report.warnings_count as i64;
            let errors: Vec<String> = report.errors.iter().chain(report.warnings.iter()).map(|issue| format!("{}: {}", issue.relative_path, issue.message)).chain(conflicts).take(100).collect();

            // Purge staging tables to strictly bound SQLite storage growth
            let _ = inventory::clean_staging(&store, &scan_id).await;

            crate::database::query("UPDATE scan_jobs SET status=?,discovered=?,probed=?,reused=?,errors_json=?,errors_count=?,warnings_count=? WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)")
                .bind(if report.complete { "completed" } else { "error" })
                .bind(report.discovered as i64)
                .bind(report.probed as i64)
                .bind(report.reused as i64)
                .bind(serde_json::to_string(&errors)?)
                .bind(total_errors)
                .bind(total_warnings)
                .bind(&job_id).bind(&scan_id)
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
                crate::database::query("UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1,warnings_count=0 WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)")
                    .bind(errors_json)
                    .bind(&job_id).bind(&scan_id)
                    .execute(&store.pool)
                    .await;
        }
        if let Ok(mut tokens) = runner_jobs.cancellations.lock() {
            tokens.remove(&scan_id);
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
        crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)")
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
            start(
                State(state.clone()),
                Path("films".into()),
                headers.clone(),
                axum::body::Bytes::new()
            )
            .await
            .unwrap()
            .0,
            StatusCode::ACCEPTED
        );
        assert_eq!(
            start(
                State(state.clone()),
                Path("films".into()),
                headers.clone(),
                axum::body::Bytes::new()
            )
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
            start(
                State(state),
                Path("films".into()),
                headers,
                axum::body::Bytes::new()
            )
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
