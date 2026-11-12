//! Administrator controls feed the same persistent automation outbox.
use crate::{
    auth::AuthState,
    automation,
    database::{begin_write, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

async fn admin(auth: &AuthState, headers: &HeaderMap, write: bool) -> Result<(), StatusCode> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if write {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(())
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/tasks/automation/policies",
            get(list).put(update),
        )
        .route("/api/v1/admin/tasks/automation/run", post(run))
        .route("/api/v1/admin/tasks/automation/results", get(results))
        .route("/api/v1/admin/tasks/automation/history", get(history))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    admin(&auth, &headers, false).await?;
    Ok(Json(
        json!({"policies":automation::policies(&auth.store).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?}),
    ))
}
async fn update(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(policy): Json<automation::Policy>,
) -> Result<StatusCode, StatusCode> {
    admin(&auth, &headers, true).await?;
    automation::set_policy(&auth.store, &policy, media_contracts::protocol::unix_now())
        .await
        .map_err(|error| match error {
            sqlx::Error::Protocol(_) => StatusCode::UNPROCESSABLE_ENTITY,
            sqlx::Error::Database(ref e) if e.is_foreign_key_violation() => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manual {
    library_id: String,
    extension_id: String,
    idempotency_key: String,
    #[serde(default)]
    missing_only: bool,
}
async fn run(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Manual>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    admin(&auth, &headers, true).await?;
    if input.library_id.len() > 128
        || input.extension_id.len() > 80
        || input.idempotency_key.is_empty()
        || input.idempotency_key.len() > 128
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let _writer = auth.store.task_writes.lock().await;
    let mut conn = auth
        .store
        .pool()
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let allowed: i64 = query_scalar(
        "SELECT COUNT(*) \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            JOIN extensions e ON e.id=? \
            JOIN extension_publishers p ON p.id=e.publisher_id \
            WHERE l.id=? AND e.enabled=1 AND p.active=1 AND n.status!='revoked' AND (l.enabled=1 \
            OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent \
            ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) \
            AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g \
            WHERE g.extension_id=e.id AND (g.library_id=l.id OR EXISTS(SELECT 1 \
            FROM library_folders f WHERE f.library_id=l.id AND f.parent_id=g.library_id \
            AND f.active=1))))",
    )
    .bind(&input.extension_id)
    .bind(&input.library_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if allowed != 1 {
        return Err(StatusCode::NOT_FOUND);
    }
    let id = automation::schedule_library(
        &mut tx,
        &input.library_id,
        if input.missing_only {
            "metadata.missing"
        } else {
            "manual"
        },
        &input.idempotency_key,
        Some(&input.extension_id),
        media_contracts::protocol::unix_now(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    crate::task_engine::WAKE.notify_one();
    Ok((StatusCode::ACCEPTED, Json(json!({"requestId":id}))))
}
async fn results(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    admin(&auth, &headers, false).await?;
    let rows:Vec<(String,String,String,String,i64)>=query_as("SELECT extension_id,asset_id,task_id,result_json,updated_at FROM automation_results ORDER BY updated_at DESC,task_id LIMIT 100")
        .fetch_all(auth.store.pool()).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        json!({"results":rows.into_iter().map(|(extension,asset,task,result,updated)|json!({"extensionId":extension,"assetId":asset,"taskId":task,"result":serde_json::from_str::<Value>(&result).unwrap_or(Value::Null),"updatedAt":updated})).collect::<Vec<_>>()}),
    ))
}

#[derive(sqlx::FromRow)]
struct TaskHistoryRow {
    id: String,
    extension_id: String,
    library_id: String,
    root_library_id: String,
    library_name: String,
    status: String,
    operation: String,
    provider: String,
    priority: i64,
    attempt: i64,
    max_attempts: i64,
    created_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    error: String,
    asset_path: String,
    track_title: String,
}

/// File name of an inventory path, whichever separator the node used.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

async fn history(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    admin(&auth, &headers, false).await?;
    // Folder libraries (extra roots of a library) have their own id: report the
    // library the administrator knows, and the file or track that was processed.
    let tasks: Vec<TaskHistoryRow> = query_as(
        "SELECT t.id AS id,l.extension_id AS extension_id,t.library_id AS \
            library_id,COALESCE(parent.id,lib.id,t.library_id) AS \
            root_library_id,COALESCE(parent.name,lib.name,'') AS library_name,t.status AS \
            status,l.operation AS operation,l.provider AS provider,t.priority AS \
            priority,t.attempt AS attempt,t.max_attempts AS max_attempts,t.created_at AS \
            created_at,t.started_at AS started_at,t.finished_at AS finished_at,t.error AS \
            error,COALESCE(a.relative_path,'') AS asset_path,COALESCE(m.title,'') AS track_title \
            FROM durable_tasks t \
            JOIN automation_task_links l ON l.task_id=t.id \
            LEFT \
            JOIN libraries lib ON lib.id=t.library_id \
            LEFT \
            JOIN library_folders f ON f.library_id=t.library_id \
            LEFT \
            JOIN libraries parent ON parent.id=f.parent_id \
            LEFT \
            JOIN inventory_assets a ON a.id=t.target_id \
            LEFT \
            JOIN music_tracks m ON m.id=t.target_id \
            ORDER BY t.created_at DESC,t.id \
            LIMIT 200",
    )
    .fetch_all(auth.store.pool())
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let quotas:Vec<(String,i64,i64,i64)>=query_as("SELECT provider,next_allowed_at,min_interval_seconds,max_concurrent FROM automation_provider_limits ORDER BY provider LIMIT 100")
        .fetch_all(auth.store.pool()).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let sources:Vec<(String,String,String,String,String,i64,i64)>=query_as("SELECT p.work_id,COALESCE(w.title,''),p.field_name,p.extension_id,p.provider_id,p.priority,p.updated_at \
            FROM automatic_metadata_provenance p \
            LEFT \
            JOIN inventory_works w ON w.id=p.work_id \
            ORDER BY p.updated_at DESC,p.work_id,p.field_name \
            LIMIT 100")
        .fetch_all(auth.store.pool()).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "now": media_contracts::protocol::unix_now(),
        "tasks":tasks.into_iter().map(|task| {
            let target = if task.track_title.is_empty() { file_name(&task.asset_path).to_owned() } else { task.track_title.clone() };
            json!({
                "id":task.id,"extensionId":task.extension_id,"libraryId":task.library_id,"rootLibraryId":task.root_library_id,
                "libraryName":task.library_name,"status":task.status,"operation":task.operation,"provider":task.provider,
                "priority":task.priority,"attempt":task.attempt,"maxAttempts":task.max_attempts,"target":target,
                "createdAt":task.created_at,"startedAt":task.started_at,"finishedAt":task.finished_at,
                "durationSeconds":task.started_at.zip(task.finished_at).map(|(start,end)|end.saturating_sub(start)),
                "error":task.error
            })
        }).collect::<Vec<_>>(),
        "quotas":quotas.into_iter().map(|(provider,next,min_interval,max_concurrent)|json!({"provider":provider,"nextAllowedAt":next,"minIntervalSeconds":min_interval,"maxConcurrent":max_concurrent})).collect::<Vec<_>>(),
        "sources":sources.into_iter().map(|(work,title,field,extension,provider,priority,updated)|json!({"workId":work,"workTitle":title,"field":field,"extensionId":extension,"providerId":provider,"priority":priority,"updatedAt":updated})).collect::<Vec<_>>()
    })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_names_ignore_the_node_separator() {
        assert_eq!(super::file_name("Films/Dune (2021)/Dune.mkv"), "Dune.mkv");
        assert_eq!(super::file_name(r"Films\Dune.mkv"), "Dune.mkv");
        assert_eq!(super::file_name("Dune.mkv"), "Dune.mkv");
        assert_eq!(super::file_name(""), "");
    }
}
