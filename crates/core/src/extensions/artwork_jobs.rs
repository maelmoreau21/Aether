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
    /// Automatic job started by the server itself: `single` (one image of each enabled
    /// kind) or `pool` (multi-image mode). Manual jobs leave it empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auto: Option<String>,
    /// Automatic pass started by an administrator from the task scheduler: every work
    /// not handled since this instant is asked again, instead of once a month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    since: Option<i64>,
}
/// A work handled by an automatic pass is asked again after this delay at most, so
/// images published later by the providers are still picked up.
const AUTO_RETRY_AFTER: i64 = 30 * 86_400;
/// Images already present for a work: provider pool, files beside the media, legacy poster.
async fn present_kinds(
    auth: &AuthState,
    work: &str,
) -> Result<std::collections::HashSet<String>, Error> {
    let kinds: Vec<String> = query_scalar("SELECT kind FROM work_image_entries WHERE work_id=? UNION SELECT i.role FROM inventory_images i LEFT JOIN work_image_hidden h ON h.work_id=i.work_id AND h.image_id=i.id WHERE i.work_id=? AND i.available=1 AND h.image_id IS NULL UNION SELECT 'poster' FROM work_posters WHERE work_id=?")
        .bind(work).bind(work).bind(work).fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(kinds.into_iter().collect())
}
const IMAGE_KINDS: [&str; 6] = ["poster", "backdrop", "logo", "thumb", "banner", "disc"];
/// Next identified film or series of an automatic job that still needs the providers.
/// Works with nothing missing are marked done here without any network call.
async fn next_auto_work(
    auth: &AuthState,
    job: &Job,
    mode: &str,
    since: Option<i64>,
) -> Result<(Option<String>, String), Error> {
    let handled_after = since.unwrap_or_else(|| now() - AUTO_RETRY_AFTER);
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(&job.library_id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_library_options"))?;
    let mut cursor = job.cursor_work.clone();
    for _ in 0..50 {
        let next: Option<String> = query_scalar("SELECT w.id FROM inventory_works w WHERE (w.library_id=? OR w.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND w.id>? AND w.kind IN ('film','series') AND w.external_ids_json NOT IN ('','{}') AND NOT EXISTS(SELECT 1 FROM artwork_auto_attempts a WHERE a.work_id=w.id AND a.mode=? AND a.attempted_at>?) ORDER BY w.id LIMIT 1")
            .bind(&job.library_id).bind(&job.library_id).bind(&cursor).bind(mode).bind(handled_after)
            .fetch_optional(auth.store.pool()).await.map_err(db)?;
        let Some(work) = next else {
            return Ok((None, cursor));
        };
        if mode == "single" {
            let present = present_kinds(auth, &work).await?;
            if IMAGE_KINDS
                .iter()
                .all(|kind| !options.image_enabled(kind) || present.contains(*kind))
            {
                mark_auto_attempt(auth, &work, mode).await?;
                cursor = work;
                continue;
            }
        }
        return Ok((Some(work), cursor));
    }
    Ok((None, cursor))
}
async fn mark_auto_attempt(auth: &AuthState, work: &str, mode: &str) -> Result<(), Error> {
    query("INSERT INTO artwork_auto_attempts(work_id,mode,attempted_at) VALUES(?,?,?) ON CONFLICT(work_id,mode) DO UPDATE SET attempted_at=excluded.attempted_at")
        .bind(work).bind(mode).bind(now()).execute(auth.store.pool()).await.map_err(db)?;
    Ok(())
}
static LAST_AUTO_CHECK: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// Asks the next worker tick to look for works to fill (new identification, saved settings).
pub(crate) fn request_automatic_check() {
    LAST_AUTO_CHECK.store(0, std::sync::atomic::Ordering::Relaxed);
}
/// Queues an automatic image pass for every film or series library that has identified
/// works not handled yet in the library's current mode. Runs at most every ten minutes
/// unless `force` (library settings just saved).
pub(crate) async fn ensure_automatic(auth: &AuthState, force: bool) -> Result<(), Error> {
    let checked = LAST_AUTO_CHECK.load(std::sync::atomic::Ordering::Relaxed);
    if !force && now() - checked < 600 {
        return Ok(());
    }
    LAST_AUTO_CHECK.store(now(), std::sync::atomic::Ordering::Relaxed);
    let owner: Option<String> = query_scalar("SELECT id FROM users WHERE role='administrator' AND disabled_at IS NULL ORDER BY id LIMIT 1")
        .fetch_optional(auth.store.pool()).await.map_err(db)?;
    let Some(owner) = owner else {
        return Ok(());
    };
    let libraries: Vec<(String, String)> = query_as("SELECT l.id,l.options_json FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.enabled=1 AND l.kind IN ('films','series') AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id AND f.active=1) AND NOT EXISTS(SELECT 1 FROM artwork_collection_jobs j WHERE j.library_id=l.id AND j.status IN ('queued','running','paused'))")
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    for (library, options) in libraries {
        let Ok(options) = serde_json::from_str::<crate::library_options::LibraryOptions>(&options)
        else {
            continue;
        };
        if !IMAGE_KINDS.iter().any(|kind| options.image_enabled(kind)) {
            continue;
        }
        let mode = library_mode(&options);
        let pending: i64 = query_scalar("SELECT COUNT(*) FROM (SELECT 1 FROM inventory_works w WHERE (w.library_id=? OR w.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND w.kind IN ('film','series') AND w.external_ids_json NOT IN ('','{}') AND NOT EXISTS(SELECT 1 FROM artwork_auto_attempts a WHERE a.work_id=w.id AND a.mode=? AND a.attempted_at>?) LIMIT 1) pending")
            .bind(&library).bind(&library).bind(mode).bind(now() - AUTO_RETRY_AFTER)
            .fetch_one(auth.store.pool()).await.map_err(db)?;
        if pending == 0 {
            continue;
        }
        let languages = if options.artwork_languages.is_empty() {
            vec!["fr".into(), "en".into()]
        } else {
            options.artwork_languages.clone()
        };
        let parameters = Input {
            library_id: library.clone(),
            after: None,
            languages,
            include_neutral: options.artwork_include_neutral,
            max_images: usize::from(options.artwork_max_images).clamp(1, 12),
            max_works: 100_000,
            auto: Some(mode.into()),
            since: None,
        };
        let parameters = serde_json::to_string(&parameters)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_parameters"))?;
        // The partial unique index keeps a single active job per library even if two
        // passes race; the loser's insert is simply ignored.
        let _ = query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,cursor_work,created_at,updated_at) VALUES(?,?,?,?,'',?,?)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&library).bind(&owner).bind(parameters).bind(now()).bind(now())
            .execute(auth.store.pool()).await;
    }
    query("DELETE FROM artwork_collection_jobs WHERE status IN ('completed','cancelled') AND id NOT IN (SELECT id FROM artwork_collection_jobs ORDER BY created_at DESC,id DESC LIMIT 1000)").execute(auth.store.pool()).await.map_err(db)?;
    Ok(())
}
/// Image mode of a library: `pool` (several images per kind) or `single`.
fn library_mode(options: &crate::library_options::LibraryOptions) -> &'static str {
    if options.artwork_pool_enabled {
        "pool"
    } else {
        "single"
    }
}
/// Queue, on an administrator's request, an automatic image pass over every identified
/// work of a library: the library's mode (pool or one image per kind), languages and
/// limits apply, and works handled earlier are asked again. `false` when a pass of
/// this library is already queued or running.
pub(crate) async fn queue_library_pass(
    auth: &AuthState,
    library: &str,
    owner: &str,
) -> Result<bool, Error> {
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(library)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options = crate::library_options::LibraryOptions::from_json(&options);
    let parameters = Input {
        library_id: library.to_owned(),
        after: None,
        languages: if options.artwork_languages.is_empty() {
            vec!["fr".into(), "en".into()]
        } else {
            options.artwork_languages.clone()
        },
        include_neutral: options.artwork_include_neutral,
        max_images: usize::from(options.artwork_max_images).clamp(1, 12),
        max_works: 100_000,
        auto: Some(library_mode(&options).into()),
        since: Some(now()),
    };
    let parameters = serde_json::to_string(&parameters)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_parameters"))?;
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x4152544a4f42)
        .await
        .map_err(db)?;
    let active: i64 = query_scalar("SELECT COUNT(*) FROM artwork_collection_jobs WHERE library_id=? AND status IN ('queued','running','paused')")
        .bind(library).fetch_one(&mut *tx).await.map_err(db)?;
    if active > 0 {
        return Ok(false);
    }
    query("INSERT INTO artwork_collection_jobs(id,library_id,owner,parameters,cursor_work,created_at,updated_at) VALUES(?,?,?,?,'',?,?)")
        .bind(uuid::Uuid::new_v4().to_string()).bind(library).bind(owner).bind(parameters).bind(now()).bind(now())
        .execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(true)
}
/// Stop the active image pass of a library. `false` when none is active.
pub(crate) async fn cancel_library_pass(auth: &AuthState, library: &str) -> Result<bool, Error> {
    Ok(query("UPDATE artwork_collection_jobs SET status='cancelled',lease_token='',lease_until=0,next_run_at=0,updated_at=? WHERE library_id=? AND status IN ('queued','running','paused')")
        .bind(now()).bind(library).execute(auth.store.pool()).await.map_err(db)?.rows_affected() > 0)
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
    ensure_automatic(auth, false).await?;
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
    let auto = parameters.auto.clone();
    let (work, cursor) = if let Some(mode) = auto.as_deref() {
        next_auto_work(auth, &job, mode, parameters.since).await?
    } else {
        let work: Option<String> = query_scalar("SELECT id FROM inventory_works WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND id>? ORDER BY id LIMIT 1")
            .bind(&job.library_id).bind(&job.library_id).bind(&job.cursor_work).fetch_optional(auth.store.pool()).await.map_err(db)?;
        (work, job.cursor_work.clone())
    };
    let Some(work) = work.filter(|_| job.processed < parameters.max_works) else {
        query("UPDATE artwork_collection_jobs SET status='completed',cursor_work=?,lease_until=0,updated_at=? WHERE id=? AND lease_token=? AND status='running'").bind(&cursor).bind(now()).bind(id).bind(token).execute(auth.store.pool()).await.map_err(db)?;
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
            one_per_kind: auto.as_deref() == Some("single"),
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
    // An automatic pass skips a work the providers cannot identify yet: it is taken
    // again by a later pass once its identifiers are known.
    let unidentified = error_code == "external_ids_required";
    let retry = !denied
        && !unidentified
        && job.work_attempts < 2
        && (value["partial"] == true
            || !error_code.is_empty()
            || value["errors"].as_array().is_some_and(|v| !v.is_empty()));
    query("UPDATE artwork_collection_jobs SET status=?,cursor_work=?,processed=processed+?,downloaded=downloaded+?,duplicates=duplicates+?,skipped=skipped+?,errors=errors+?,work_attempts=?,last_error=?,lease_until=0,next_run_at=?,updated_at=? WHERE id=? AND lease_token=? AND status='running'")
        .bind(if denied {"paused"} else {"queued"}).bind(if retry||denied {&cursor} else {&work}).bind(i64::from(!retry&&!denied))
        .bind(value["downloaded"].as_i64().unwrap_or(0)).bind(value["duplicates"].as_i64().unwrap_or(0)).bind(value["skipped"].as_i64().unwrap_or(0))
        .bind(i64::from(!error_code.is_empty())+value["errors"].as_array().map_or(0,|v|v.len() as i64)).bind(if retry {job.work_attempts+1} else {0}).bind(error_code)
        .bind(now()+15).bind(now()).bind(id).bind(token).execute(auth.store.pool()).await.map_err(db)?;
    if let Some(mode) = auto.as_deref()
        && !retry
        && !denied
        && !unidentified
    {
        mark_auto_attempt(auth, &work, mode).await?;
    }
    Ok(())
}
