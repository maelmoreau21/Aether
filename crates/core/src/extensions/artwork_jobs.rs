//! Durable progressive collection on the existing Core worker/runtime and AssetStore quotas.
//! No browser credentials are persisted or manufactured for background execution.
use super::*;
use serde::Serialize;
static WORKER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[cfg(test)]
mod tests;

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Input {
    library_id: String,
    #[serde(default)]
    after: Option<String>,
    languages: Vec<String>,
    include_neutral: bool,
    max_images: usize,
    #[serde(default = "default_work_limit")]
    max_works: i64,
}
fn default_work_limit() -> i64 {
    1000
}
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(super) struct Job {
    pub id: String,
    pub library_id: String,
    pub owner: String,
    pub parameters: String,
    pub cursor_work: String,
    pub status: String,
    #[serde(skip)]
    pub lease_token: String,
    pub processed: i64,
    pub downloaded: i64,
    pub duplicates: i64,
    pub skipped: i64,
    pub errors: i64,
    pub work_attempts: i64,
    pub last_error: String,
}
const SELECT: &str = "SELECT id,library_id,owner,parameters,cursor_work,status,lease_token,processed,downloaded,duplicates,skipped,errors,work_attempts,last_error FROM artwork_collection_jobs";

pub(super) async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>), Error> {
    let actor = admin(&auth, &headers, true).await?;
    if input
        .after
        .as_ref()
        .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
        || input.max_images == 0
        || input.max_images > 12
        || !(1..=100_000).contains(&input.max_works)
        || input.languages.is_empty()
        || input.languages.len() > 5
        || input
            .languages
            .iter()
            .any(|v| !image_collection::valid_language(v))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_collection_limits"));
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND l.kind IN ('films','series') AND n.status!='revoked'")
        .bind(&input.library_id).fetch_one(auth.store.pool()).await.map_err(db)?;
    if count != 1 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_library"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x4152544a4f42)
        .await
        .map_err(db)?;
    let active: i64 = query_scalar("SELECT COUNT(*) FROM artwork_collection_jobs WHERE status IN ('queued','running','paused')").fetch_one(&mut *tx).await.map_err(db)?;
    if active >= 32 {
        return Err(err(StatusCode::CONFLICT, "collection_queue_full"));
    }
    let existing: Option<String> = query_scalar("SELECT id FROM artwork_collection_jobs WHERE library_id=? AND status IN ('queued','running','paused')")
        .bind(&input.library_id).fetch_optional(&mut *tx).await.map_err(db)?;
    if let Some(id) = existing {
        return Ok((StatusCode::ACCEPTED, Json(json!({"id":id,"existing":true}))));
    }
    query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,cursor_work,created_at,updated_at) VALUES(?,?,?,?,?,?,?)")
        .bind(&id).bind(&input.library_id).bind(&actor.user.id).bind(serde_json::to_string(&input).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_parameters"))?).bind(input.after.as_deref().unwrap_or("")).bind(now()).bind(now()).execute(&mut *tx).await.map_err(db)?;
    query("DELETE FROM artwork_collection_jobs WHERE status IN ('completed','cancelled') AND id NOT IN (SELECT id FROM artwork_collection_jobs ORDER BY created_at DESC,id DESC LIMIT 1000)").execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::ACCEPTED, Json(json!({"id":id}))))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Filter {
    library_id: Option<String>,
}
pub(super) async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<Job>>, Error> {
    admin(&auth, &headers, false).await?;
    query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE (CAST(? AS TEXT) IS NULL OR library_id=?) ORDER BY created_at DESC,id DESC LIMIT 32")))
        .bind(&filter.library_id).bind(&filter.library_id).fetch_all(auth.store.pool()).await.map(Json).map_err(db)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Control {
    action: String,
}
pub(super) async fn control(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Control>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let status = match input.action.as_str() {
        "pause" => "paused",
        "cancel" => "cancelled",
        "resume" => "queued",
        _ => return Err(err(StatusCode::BAD_REQUEST, "invalid_action")),
    };
    let changed = query("UPDATE artwork_collection_jobs SET status=?,lease_token='',lease_until=0,next_run_at=0,updated_at=? WHERE id=? AND status IN ('queued','running','paused')")
        .bind(status).bind(now()).bind(id).execute(auth.store.pool()).await.map_err(db)?.rows_affected();
    if changed == 0 {
        return Err(err(StatusCode::CONFLICT, "collection_not_active"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn authorize_work(auth: &AuthState, job: &Job, work: &str) -> Result<(), Error> {
    let valid: i64 = query_scalar("SELECT COUNT(*) FROM artwork_collection_jobs j JOIN users u ON u.id=j.owner JOIN inventory_works w ON w.id=? JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE j.id=? AND j.lease_token=? AND j.status='running' AND j.lease_until>? AND u.role='administrator' AND u.disabled_at IS NULL AND l.enabled=1 AND n.status!='revoked' AND (l.id=j.library_id OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.parent_id=j.library_id AND f.active=1 AND parent.enabled=1))")
        .bind(work).bind(&job.id).bind(&job.lease_token).bind(now()).fetch_one(auth.store.pool()).await.map_err(db)?;
    if valid != 1
        || !crate::product::target_allowed(&auth.store, &job.owner, "work", work)
            .await
            .map_err(|s| err(s, "work_denied"))?
    {
        return Err(err(StatusCode::FORBIDDEN, "collection_access_revoked"));
    }
    Ok(())
}
pub(super) async fn provider_permitted(
    auth: &AuthState,
    job: &Job,
    id: &str,
    library: &str,
    generation: &str,
) -> Result<bool, Error> {
    let valid: i64 = query_scalar("SELECT COUNT(*) FROM artwork_collection_jobs j JOIN users u ON u.id=j.owner JOIN extensions e ON e.id=? JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON l.id=? JOIN media_nodes n ON n.id=l.node_id WHERE j.id=? AND j.lease_token=? AND j.status='running' AND j.lease_until>? AND u.role='administrator' AND u.disabled_at IS NULL AND e.generation=? AND e.enabled=1 AND p.active=1 AND l.enabled=1 AND n.status!='revoked' AND (l.id=j.library_id OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.parent_id=j.library_id AND f.active=1 AND parent.enabled=1)) AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) AND NOT EXISTS(SELECT 1 FROM automation_policies policy WHERE policy.extension_id=e.id AND policy.library_id=l.id AND policy.mode='disabled')")
        .bind(id).bind(library).bind(&job.id).bind(&job.lease_token).bind(now()).bind(generation).fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(valid == 1)
}

pub(crate) async fn tick(auth: &AuthState) -> Result<(), Error> {
    let Ok(_worker) = WORKER.try_lock() else {
        return Ok(());
    };
    let token = uuid::Uuid::new_v4().to_string();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x4152544a4f42)
        .await
        .map_err(db)?;
    let next: Option<String> = query_scalar("SELECT id FROM artwork_collection_jobs WHERE (status='queued' OR (status='running' AND lease_until<=?)) AND next_run_at<=? ORDER BY updated_at,id LIMIT 1")
        .bind(now()).bind(now()).fetch_optional(&mut *tx).await.map_err(db)?;
    let Some(id) = next else {
        return Ok(());
    };
    query("UPDATE artwork_collection_jobs SET status='running',lease_token=?,lease_until=?,updated_at=? WHERE id=?")
        .bind(&token).bind(now()+120).bind(now()).bind(&id).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    drop(conn);
    let job: Job = query_as(sqlx::AssertSqlSafe(format!("{SELECT} WHERE id=?")))
        .bind(&id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let parameters: Input = serde_json::from_str(&job.parameters)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_collection_parameters"))?;
    let work: Option<String> = query_scalar("SELECT id FROM inventory_works WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND id>? ORDER BY id LIMIT 1")
        .bind(&job.library_id).bind(&job.library_id).bind(&job.cursor_work).fetch_optional(auth.store.pool()).await.map_err(db)?;
    let Some(work) = work.filter(|_| job.processed < parameters.max_works) else {
        query("UPDATE artwork_collection_jobs SET status='completed',lease_until=0,updated_at=? WHERE id=? AND lease_token=? AND status='running'").bind(now()).bind(id).bind(token).execute(auth.store.pool()).await.map_err(db)?;
        return Ok(());
    };
    let result = image_collection::collect_with_context(
        auth,
        &HeaderMap::new(),
        image_collection::Collect {
            work_id: work.clone(),
            languages: Some(parameters.languages),
            include_neutral: Some(parameters.include_neutral),
            max_images: Some(parameters.max_images),
        },
        Some(super::run::Background::Artwork(&job)),
    )
    .await;
    let (value, error_code, denied) = match result {
        Ok(value) => (value.0, String::new(), false),
        Err((status, body)) => (
            json!({}),
            body.0["error"]
                .as_str()
                .unwrap_or("collection_failed")
                .to_owned(),
            status == StatusCode::FORBIDDEN,
        ),
    };
    let retry = !denied
        && job.work_attempts < 2
        && (value["partial"] == true
            || !error_code.is_empty()
            || value["errors"].as_array().is_some_and(|v| !v.is_empty()));
    query("UPDATE artwork_collection_jobs SET status=?,cursor_work=?,processed=processed+?,downloaded=downloaded+?,duplicates=duplicates+?,skipped=skipped+?,errors=errors+?,work_attempts=?,last_error=?,lease_until=0,next_run_at=?,updated_at=? WHERE id=? AND lease_token=? AND status='running'")
        .bind(if denied {"paused"} else {"queued"}).bind(if retry||denied {&job.cursor_work} else {&work}).bind(i64::from(!retry&&!denied))
        .bind(value["downloaded"].as_i64().unwrap_or(0)).bind(value["duplicates"].as_i64().unwrap_or(0)).bind(value["skipped"].as_i64().unwrap_or(0))
        .bind(i64::from(!error_code.is_empty())+value["errors"].as_array().map_or(0,|v|v.len() as i64)).bind(if retry {job.work_attempts+1} else {0}).bind(error_code)
        .bind(now()+15).bind(now()).bind(id).bind(token).execute(auth.store.pool()).await.map_err(db)?;
    Ok(())
}
