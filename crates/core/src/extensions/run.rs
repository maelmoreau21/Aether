use super::*;
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use media_extension_sdk::{ExtensionRequest, ExtensionResponse, Package};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct RunGuard {
    pub(super) auth: AuthState,
    pub(super) lease: run_ownership::Lease,
    pub(super) finished: bool,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let auth = self.auth.clone();
        let lease = self.lease.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = lease
                    .finish(&auth.store, "interrupted", &json!({}), 0)
                    .await;
            });
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApplyMetadata {
    pub(super) work_id: String,
    pub(super) metadata: Value,
}

pub(super) async fn apply_metadata(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ApplyMetadata>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;

    let exists: Option<(String, String)> =
        query_as("SELECT library_id, title FROM inventory_works WHERE id=?")
            .bind(&input.work_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (library, _) = exists.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    let stored: Option<(String, String)> =
        query_as("SELECT package_json,generation FROM extensions WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (stored, generation) =
        stored.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_extension"))?;
    if !permitted(&auth, &headers, &id, &library, &generation).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }
    let package: Package = serde_json::from_str(&stored)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
    if !package
        .manifest
        .permissions
        .iter()
        .any(|permission| permission == "metadata.write")
    {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }

    let mut normalized = adapters::catalogue_metadata(input.metadata)?;
    normalized.insert("provenance".into(), json!(format!("extension:{id}")));
    let previous: Option<String> =
        query_scalar("SELECT manual_json FROM inventory_works WHERE id=?")
            .bind(&input.work_id)
            .fetch_one(auth.store.pool())
            .await
            .map_err(db)?;
    let merged = adapters::merge_metadata(previous, normalized);
    let meta_str = merged.to_string();
    if meta_str.len() > 65_536 {
        return Err(err(StatusCode::BAD_REQUEST, "metadata_too_large"));
    }

    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("UPDATE inventory_works SET manual_json=? WHERE id=?")
        .bind(&meta_str)
        .bind(&input.work_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    audit(&mut tx, &actor.user.id, &id, "metadata.applied").await?;
    tx.commit().await.map_err(db)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Run {
    pub(super) work_id: String,
    #[serde(default = "default_operation")]
    pub(super) operation: String,
    #[serde(default)]
    pub(super) input: Value,
}

pub(super) fn default_operation() -> String {
    "metadata.propose".into()
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    #[serde(default)]
    pub(super) q: String,
    #[serde(default)]
    pub(super) offset: u32,
}

pub(super) async fn works(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(search): Query<Search>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    if search.q.len() > 120 || search.offset > 1_000_000 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_search"));
    }
    let rows: Vec<(String,String,String)> = query_as("SELECT w.id,w.title,w.library_id FROM inventory_works w JOIN extensions e ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=w.library_id)) JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE e.id=? AND e.enabled=1 AND p.active=1 AND l.enabled=1 AND n.status!='revoked' AND LOWER(w.title) LIKE ? ORDER BY w.title,w.id LIMIT 50 OFFSET ?")
        .bind(id).bind(format!("%{}%",search.q.to_lowercase())).bind(i64::from(search.offset)).fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"items":rows.into_iter().map(|(id,title,library)| json!({"id":id,"title":title,"libraryId":library})).collect::<Vec<_>>(),"offset":search.offset,"limit":50}),
    ))
}

pub(super) async fn permitted(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
) -> Result<bool, Error> {
    if library.is_empty() {
        auth.authenticate(headers)
            .await
            .map_err(|status| err(status, "authentication_required"))?;
        let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? AND e.generation=? AND e.enabled=1 AND p.active=1")
            .bind(id).bind(generation).fetch_one(auth.store.pool()).await.map_err(db)?;
        return Ok(count == 1);
    }
    admin(auth, headers, false).await?;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) JOIN media_nodes n ON n.id=l.node_id WHERE e.id=? AND l.id=? AND e.generation=? AND e.enabled=1 AND p.active=1 AND l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))")
        .bind(id).bind(library).bind(generation).fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(count == 1)
}

#[derive(Clone, Copy)]
pub(super) enum Background<'a> {
    Task(&'a crate::task_engine::Job),
    Artwork(&'a super::artwork_jobs::Job),
}
impl Background<'_> {
    fn owner(self) -> String {
        match self {
            Self::Task(j) => j.owner.clone(),
            Self::Artwork(j) => j.owner.clone(),
        }
    }
}

pub(super) async fn run(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Run>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    run_with_context(&auth, &headers, &id, input, None).await
}
pub(super) async fn run_with_context(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    input: Run,
    background: Option<Background<'_>>,
) -> Result<Json<Value>, Error> {
    let actor_id = if let Some(context) = background {
        context.owner()
    } else {
        admin(auth, headers, true).await?.user.id
    };
    let _slot = crate::extensions::runtime::try_acquire_execution(
        id,
        if background.is_some() {
            crate::extensions::runtime::ExecutionClass::Background
        } else {
            crate::extensions::runtime::ExecutionClass::Interactive
        },
    )
    .ok_or_else(|| err(StatusCode::CONFLICT, "extension_busy"))?;
    let resource: Option<adapters::WorkContext> =
        query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?")
            .bind(&input.work_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let context = resource.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_work"))?;
    let library = context.library_id.clone();
    let stored: Option<(String, String)> =
        query_as("SELECT package_json,generation FROM extensions WHERE id=?")
            .bind(id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let (stored, generation) =
        stored.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_extension"))?;
    if !worker_permitted(auth, headers, id, &library, &generation, background).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }
    let package: Package = serde_json::from_str(&stored)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
    let settings_str: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = settings_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));

    if !settings.is_object() {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_settings"));
    }
    let mut settings = resolve_effective_extension_settings(id, settings);
    // The work's library owns metadata language, including historical plugin settings.
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(&library)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_library_options"))?;
    // The first ABI's simple proposal rejects any extra field, including config.
    // Keep that no-settings payload compatible; provider operations receive the library language.
    let simple_legacy = input.operation == "metadata.propose"
        && package
            .manifest
            .config_schema
            .as_ref()
            .is_none_or(Vec::is_empty)
        && settings.as_object().is_some_and(|fields| fields.is_empty());
    let s_settings = crate::server_settings::load(&auth.store)
        .await
        .unwrap_or_default();
    let preferred_lang = if !options.preferred_metadata_language.trim().is_empty() {
        options.preferred_metadata_language.as_str()
    } else {
        s_settings.preferred_metadata_language.as_str()
    };
    let country_code = if !options.metadata_country_code.trim().is_empty() {
        options.metadata_country_code.as_str()
    } else {
        s_settings.metadata_country_code.as_str()
    };
    if !simple_legacy {
        settings["language"] = json!(preferred_lang);
        settings["country"] = json!(country_code);
        settings["countryCode"] = json!(country_code);
    }
    if input.operation == "metadata.images" {
        image_collection::apply_language_settings(&input.input, &mut settings)?;
    }
    adapters::authorize_operation(&package.manifest, &input.operation)?;
    let mut payload =
        adapters::metadata_input(&context, &input.work_id, &input.operation, &input.input)?;
    if input.operation != "metadata.propose" {
        payload["language"] = settings["language"].clone();
        payload["country"] = settings["country"].clone();
        payload["countryCode"] = settings["countryCode"].clone();
    }
    if input.operation == "metadata.details" {
        // Plugins on the first contract ignore this field and answer in one language.
        let wanted = crate::translations::wanted_locales(&auth.store, preferred_lang, 6)
            .await
            .map_err(db)?;
        payload["languages"] = json!(wanted);
    }
    payload = metadata_input_with_settings(payload, settings);
    let operation = input.operation.clone();
    let invocation = crate::extensions::runtime::Invocation {
        wasm: crate::extensions::storage::wasm(&auth.store, &package)
            .await
            .map_err(storage_error)?,
        input: ExtensionRequest {
            api_version: 1,
            operation: operation.clone(),
            input: payload,
        },
        allowed_hosts: package.manifest.http_hosts.clone(),
        auth_headers: std::collections::BTreeMap::new(),
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    let lease = run_ownership::Lease::begin(
        &auth.store,
        run_id.clone(),
        id,
        &library,
        &input.work_id,
        &actor_id,
    )
    .await
    .map_err(db)?;
    let mut guard = RunGuard {
        auth: auth.clone(),
        lease: lease.clone(),
        finished: false,
    };
    let started = Instant::now();
    let result = invoke_worker_context(
        auth,
        headers,
        id,
        &library,
        &generation,
        invocation,
        background,
        Some(&lease),
    )
    .await;
    let duration = started.elapsed().as_millis() as i64;
    let (status, mut details) = match &result {
        Ok(proposal) => (
            "completed",
            if operation == "metadata.propose" {
                proposal.clone()
            } else {
                json!({"operation":operation,"output":proposal})
            },
        ),
        Err((_, body)) => ("error", body.0.clone()),
    };
    details["version"] = json!(package.manifest.version);
    details["sha256"] = json!(package.sha256);
    if !lease
        .finish(&auth.store, status, &details, duration)
        .await
        .map_err(db)?
    {
        return Err(err(StatusCode::CONFLICT, "extension_run_lease_lost"));
    }
    guard.finished = true;
    query("DELETE FROM extension_runs WHERE status!='running' AND id NOT IN (SELECT id FROM extension_runs ORDER BY started_at DESC,id DESC LIMIT 1000)")
        .execute(auth.store.pool()).await.map_err(db)?;
    let proposal = result?;
    Ok(Json(
        json!({"runId":run_id,"extensionId":id,"version":package.manifest.version,"sha256":package.sha256,"durationMs":duration,"operation":operation,"proposal":proposal}),
    ))
}

pub(super) async fn invoke_worker(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
    invocation: crate::extensions::runtime::Invocation,
) -> Result<Value, Error> {
    invoke_worker_context(
        auth, headers, id, library, generation, invocation, None, None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn invoke_worker_context(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
    invocation: crate::extensions::runtime::Invocation,
    background: Option<Background<'_>>,
    lease: Option<&run_ownership::Lease>,
) -> Result<Value, Error> {
    let requested_operation = invocation.input.operation.clone();
    if let Some(lease) = lease
        && !lease.live(&auth.store).await.map_err(db)?
    {
        return Err(err(StatusCode::CONFLICT, "extension_run_lease_lost"));
    }
    let exe = std::env::current_exe()
        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "worker_unavailable"))?;
    let mut dir = exe
        .parent()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "worker_unavailable"))?;
    if dir.file_name().is_some_and(|n| n == "deps") {
        dir = dir
            .parent()
            .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "worker_unavailable"))?;
    }
    let worker = dir.join(if cfg!(windows) {
        "media-extension-worker.exe"
    } else {
        "media-extension-worker"
    });
    let mut command = tokio::process::Command::new(worker);
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .spawn()
        .map_err(|_| err(StatusCode::SERVICE_UNAVAILABLE, "worker_unavailable"))?;
    let payload = serde_json::to_vec(&invocation)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_input"))?;
    let user_id = if let Some(job) = background {
        job.owner()
    } else {
        auth.authenticate(headers)
            .await
            .map_err(|status| err(status, "authentication_required"))?
            .user
            .id
    };
    let work_id = invocation
        .input
        .input
        .get("workId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let episode: Option<(String, String)> =
        query_as("SELECT id,season_id FROM inventory_episodes WHERE work_id=?")
            .bind(&work_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let context = crate::extensions::data::Context {
        extension_id: id.to_owned(),
        generation: generation.to_owned(),
        library_id: library.to_owned(),
        work_id,
        season_id: episode.as_ref().map(|v| v.1.clone()).unwrap_or_default(),
        episode_id: episode.map(|v| v.0).unwrap_or_default(),
        user_id,
    };
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        stdin
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        stdin
            .write_all(&payload)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        let mut count = 0;
        loop {
            let mut header = [0; 4];
            stdout
                .read_exact(&mut header)
                .await
                .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "extension_failed"))?;
            let len = u32::from_be_bytes(header) as usize;
            if len > media_extension_sdk::MAX_JSON + 4096 {
                return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"));
            }
            let mut bytes = vec![0; len];
            stdout
                .read_exact(&mut bytes)
                .await
                .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "extension_failed"))?;
            let frame: Value = serde_json::from_slice(&bytes)
                .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"))?;
            let kind = frame["kind"].as_str().unwrap_or("");
            if kind == "failed" {
                let code = match frame["error"].as_str() {
                    Some("execution_trapped") => "extension_execution_trapped",
                    Some("invalid_output_size") => "extension_output_too_large",
                    Some("invalid_module" | "instantiation_failed") => "extension_module_invalid",
                    _ => "extension_failed",
                };
                eprintln!("Extension '{id}' failed: {code}");
                return Err(err(StatusCode::UNPROCESSABLE_ENTITY, code));
            }
            if kind == "provider_retry" {
                let delay = frame["retryAfterSeconds"]
                    .as_i64()
                    .unwrap_or(60)
                    .clamp(1, 86400);
                if let Some(Background::Task(job)) = background {
                    query("UPDATE automation_provider_limits SET next_allowed_at=CASE WHEN next_allowed_at>? THEN next_allowed_at ELSE ? END WHERE provider=(SELECT provider FROM automation_task_links WHERE task_id=?)")
                        .bind(now().saturating_add(delay)).bind(now().saturating_add(delay)).bind(&job.id)
                        .execute(auth.store.pool()).await.map_err(db)?;
                }
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({"error":"provider_rate_limited","retryAfterSeconds":delay})),
                ));
            }
            if kind == "finished" {
                drop(stdin);
                let status = child
                    .wait()
                    .await
                    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
                if !status.success() {
                    return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "extension_failed"));
                }
                let response: ExtensionResponse = serde_json::from_value(frame["response"].clone())
                    .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"))?;
                if response.api_version != 1 {
                    return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"));
                }
                return adapters::validate_result(&requested_operation, response.result);
            }
            count += 1;
            if count > 128 {
                return Err(err(
                    StatusCode::TOO_MANY_REQUESTS,
                    "storage_operation_quota",
                ));
            }
            if !worker_permitted(auth, headers, id, library, generation, background).await? {
                return Err(err(StatusCode::FORBIDDEN, "extension_revoked"));
            }
            if let Some(lease) = lease
                && !lease.live(&auth.store).await.map_err(db)?
            {
                return Err(err(StatusCode::CONFLICT, "extension_run_lease_lost"));
            }
            let result = match kind {
                "storage" => {
                    let request = serde_json::from_value(frame["request"].clone())
                        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_storage_request"))?;
                    serde_json::to_value(
                        crate::extensions::data::execute(&auth.store, &context, request, now())
                            .await
                            .map_err(media_extension_sdk::storage::StorageError::from),
                    )
                    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "storage_failed"))?
                }
                "objects" => {
                    let request = serde_json::from_value(frame["request"].clone())
                        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_object_request"))?;
                    serde_json::to_value(
                        crate::extensions::objects::execute(&auth.store, &context, request, now())
                            .await,
                    )
                    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "storage_failed"))?
                }
                _ => return Err(err(StatusCode::BAD_REQUEST, "invalid_worker_frame")),
            };
            let bytes = serde_json::to_vec(&result)
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "storage_failed"))?;
            if bytes.len() > media_extension_sdk::MAX_JSON {
                return Err(err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage_response_too_large",
                ));
            }
            stdin
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
            stdin
                .write_all(&bytes)
                .await
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        }
    };
    tokio::pin!(operation);
    let deadline = tokio::time::sleep(Duration::from_secs(
        crate::extensions::runtime::INVOCATION_TIMEOUT_SECS,
    ));
    tokio::pin!(deadline);
    let mut check = tokio::time::interval(Duration::from_millis(30));
    let renewal = async {
        if let Some(lease) = lease {
            loop {
                tokio::time::sleep(Duration::from_secs(run_ownership::HEARTBEAT_SECONDS)).await;
                if !lease.renew(&auth.store).await.map_err(db)? {
                    return Err::<(), Error>(err(StatusCode::CONFLICT, "extension_run_lease_lost"));
                }
            }
        }
        std::future::pending::<Result<(), Error>>().await
    };
    tokio::pin!(renewal);
    loop {
        tokio::select! {
            result = &mut operation => {
                if !worker_permitted(auth, headers, id, library, generation, background).await? { return Err(err(StatusCode::FORBIDDEN, "extension_revoked")); }
                return result;
            }
            _ = &mut deadline => return Err(err(StatusCode::REQUEST_TIMEOUT, "extension_timeout")),
            _ = check.tick() => {
                if !worker_permitted(auth, headers, id, library, generation, background).await? { return Err(err(StatusCode::FORBIDDEN, "extension_revoked")); }
            }
            result = &mut renewal => return result.map(|_| Value::Null),
        }
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct RunRow {
    pub(super) id: String,
    pub(super) extension_id: String,
    pub(super) library_id: String,
    pub(super) work_id: String,
    pub(super) started_at: i64,
    pub(super) duration_ms: i64,
    pub(super) status: String,
    pub(super) result_json: String,
}

pub(super) async fn runs(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    run_ownership::recover(&auth.store).await.map_err(db)?;
    let rows: Vec<RunRow> = query_as("SELECT id,extension_id,library_id,work_id,started_at,duration_ms,status,result_json FROM extension_runs ORDER BY started_at DESC,id DESC LIMIT 100").fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"runs":rows.into_iter().map(|r| json!({"id":r.id,"extensionId":r.extension_id,"libraryId":r.library_id,"workId":r.work_id,"startedAt":r.started_at,"durationMs":r.duration_ms,"status":r.status,"result":serde_json::from_str::<Value>(&r.result_json).unwrap_or(Value::Null)})).collect::<Vec<_>>()}),
    ))
}

pub(super) async fn audit_log(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String,String,String,i64)> = query_as("SELECT actor_id,extension_id,action,created_at FROM extension_audit ORDER BY created_at DESC,id DESC LIMIT 100").fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"audit":rows.into_iter().map(|(actor,id,action,time)| json!({"actorId":actor,"extensionId":id,"action":action,"createdAt":time})).collect::<Vec<_>>()}),
    ))
}

// Background authority is the persisted initiating administrator, rechecked together
// with grants and the current fenced lease; it never fabricates a browser session.
pub(super) async fn worker_permitted(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
    background: Option<Background<'_>>,
) -> Result<bool, Error> {
    let job = match background {
        None => return permitted(auth, headers, id, library, generation).await,
        Some(Background::Artwork(job)) => {
            return super::artwork_jobs::provider_permitted(auth, job, id, library, generation)
                .await;
        }
        Some(Background::Task(job)) => job,
    };
    let valid: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks j JOIN users u ON u.id=j.owner JOIN extensions e ON e.id=? JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON l.id=j.library_id JOIN media_nodes n ON n.id=j.node_id WHERE j.id=? AND j.attempt=? AND j.status='running' AND j.lease_until>? AND u.role='administrator' AND u.disabled_at IS NULL AND NOT EXISTS(SELECT 1 FROM automation_policies policy WHERE policy.extension_id=e.id AND policy.library_id=l.id AND policy.mode='disabled') AND e.generation=? AND e.enabled=1 AND p.active=1 AND l.id=? AND l.node_id=j.node_id AND l.enabled=1 AND n.status!='revoked' AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))")
        .bind(id).bind(&job.id).bind(job.attempt).bind(now()).bind(generation).bind(library)
        .fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(valid == 1)
}
