//! Explicit subtitle acquisition. Provider code stays in Wasm; derivatives live in the DB.
use super::*;

const MAX_CONTENT: usize = 1024 * 1024;
const RESERVATION_SECONDS: i64 = 120;

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route("/api/v1/admin/subtitles/{asset}", get(list))
        .route(
            "/api/v1/admin/subtitles/{asset}/search",
            axum::routing::post(search),
        )
        .route(
            "/api/v1/admin/subtitles/{asset}/download",
            axum::routing::post(download),
        )
        .route(
            "/api/v1/admin/subtitles/{asset}/{id}",
            axum::routing::delete(remove),
        )
}

#[derive(sqlx::FromRow)]
struct Context {
    library_id: String,
    work_id: String,
    size: i64,
    modified_ns: String,
    options_json: String,
    active: i64,
}
impl Context {
    fn fingerprint(&self) -> String {
        format!("{}:{}", self.size, self.modified_ns)
    }
    fn options(&self) -> Result<crate::library_options::LibraryOptions, Error> {
        serde_json::from_str(&self.options_json)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_library_options"))
    }
}
async fn context(auth: &AuthState, asset: &str) -> Result<Context, Error> {
    let ctx = admin_context(auth, asset).await?;
    if ctx.active != 1 {
        return Err(err(StatusCode::NOT_FOUND, "subtitle_asset_unavailable"));
    }
    Ok(ctx)
}
// Retained derivatives must remain manageable by administrators when their
// source is missing or its library is disabled. This lookup grants no playback
// or provider authority; acquisition still uses the active context above.
async fn admin_context(auth: &AuthState, asset: &str) -> Result<Context, Error> {
    query_as("SELECT a.library_id,e.work_id,a.size,a.modified_ns,l.options_json,CAST(CASE WHEN a.available=1 AND l.enabled=1 AND l.root=s.root_snapshot AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0)) THEN 1 ELSE 0 END AS BIGINT) AS active FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=?")
        .bind(asset).fetch_optional(auth.store.pool()).await.map_err(db)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND,"subtitle_asset_unavailable"))
}
fn availability(options: &crate::library_options::LibraryOptions) -> Option<&'static str> {
    if !options.subtitle_download_enabled {
        Some("subtitle_download_disabled")
    } else if options.subtitle_require_hash {
        Some("subtitle_hash_required")
    } else {
        None
    }
}
async fn provider(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    ctx: &Context,
) -> Result<(Package, String), Error> {
    let options = ctx.options()?;
    if let Some(code) = availability(&options) {
        return Err(err(StatusCode::CONFLICT, code));
    }
    if !options.subtitle_providers.is_empty()
        && !options
            .subtitle_providers
            .iter()
            .any(|p| p.id == id && p.enabled)
    {
        return Err(err(StatusCode::FORBIDDEN, "subtitle_provider_disabled"));
    }
    let (stored, generation): (String, String) =
        query_as("SELECT package_json,generation FROM extensions WHERE id=?")
            .bind(id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_extension"))?;
    if !permitted(auth, headers, id, &ctx.library_id, &generation).await? {
        return Err(err(StatusCode::FORBIDDEN, "extension_permission_denied"));
    }
    let package: Package = serde_json::from_str(&stored)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
    adapters::authorize_operation(&package.manifest, "subtitles.search")?;
    Ok((package, generation))
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let ctx = admin_context(&auth, &asset).await?;
    let code = if ctx.active != 1 {
        Some("subtitle_asset_unavailable")
    } else {
        availability(&ctx.options()?)
    };
    let ids: Vec<String> =
        query_scalar("SELECT id FROM extensions WHERE enabled=1 ORDER BY id LIMIT 64")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let mut providers = Vec::new();
    if code.is_none() {
        for id in ids {
            if let Ok((package, _)) = provider(&auth, &headers, &id, &ctx).await {
                providers.push(json!({"id":id,"name":package.manifest.name}));
            }
        }
    }
    let rows:Vec<DownloadRow>=query_as("SELECT id,provider_id,file_id,track_json,status,error_code,created_at,fingerprint FROM subtitle_downloads WHERE asset_id=? ORDER BY created_at DESC,id LIMIT 16")
        .bind(&asset).fetch_all(auth.store.pool()).await.map_err(db)?;
    let downloads: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let DownloadRow {
                id,
                provider_id: p,
                file_id: f,
                track_json: t,
                status,
                error_code: error,
                created_at: created,
                fingerprint,
            } = row;
            let mut v: Value = serde_json::from_str(&t).unwrap_or_else(|_| json!({}));
            v["id"] = json!(id);
            v["providerId"] = json!(p);
            v["fileId"] = json!(f);
            v["status"] = json!(
                if status == "pending" && created + RESERVATION_SECONDS <= now() {
                    "interrupted"
                } else if status == "completed" && fingerprint != ctx.fingerprint() {
                    "stale"
                } else {
                    status.as_str()
                }
            );
            v["errorCode"] = json!(error);
            v
        })
        .collect();
    let quotas:Vec<QuotaRow>=query_as("SELECT provider_id,remaining,reset_time_utc,next_allowed_at FROM subtitle_download_limits ORDER BY provider_id")
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    let quotas:Vec<Value>=quotas.into_iter().filter(|row|providers.iter().any(|p|p["id"].as_str()==Some(&row.provider_id))).map(|row|json!({"providerId":row.provider_id,"remaining":row.remaining,"resetTimeUtc":row.reset_time_utc,"nextAllowedAt":row.next_allowed_at})).collect();
    Ok(Json(
        json!({"enabled":code.is_none(),"unavailableCode":code,"providers":providers,"downloads":downloads,"quotas":quotas}),
    ))
}

#[derive(sqlx::FromRow)]
struct DownloadRow {
    id: String,
    provider_id: String,
    file_id: String,
    track_json: String,
    status: String,
    error_code: Option<String>,
    created_at: i64,
    fingerprint: String,
}

#[derive(sqlx::FromRow)]
struct QuotaRow {
    provider_id: String,
    remaining: Option<i64>,
    reset_time_utc: Option<String>,
    next_allowed_at: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SearchInput {
    provider_id: String,
    language: String,
    forced: Option<bool>,
    sdh: Option<bool>,
    page: Option<u32>,
}
fn valid_language(s: &str) -> bool {
    (2..=16).contains(&s.len())
        && s.split('-')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphabetic()))
}
pub(super) fn validate_result(operation: &str, result: Value) -> Result<Value, Error> {
    let bad = || err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_subtitle_output");
    if operation == "subtitles.search" {
        let items = result["items"].as_array().ok_or_else(bad)?;
        if items.len() > 60 || !result["totalPages"].as_u64().is_some_and(|n| n <= 10000) {
            return Err(bad());
        }
        let mut seen = std::collections::HashSet::new();
        for item in items {
            let id = item["id"].as_str().ok_or_else(bad)?;
            if id.is_empty()
                || id.len() > 20
                || !id.bytes().all(|b| b.is_ascii_digit())
                || !seen.insert(id)
                || !item["language"].as_str().is_some_and(valid_language)
                || !item["label"].as_str().is_some_and(|s| {
                    !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
                })
                || item["forced"].as_bool().is_none()
                || item["sdh"].as_bool().is_none()
                || !matches!(item["match"].as_str(), Some("identity" | "title"))
            {
                return Err(bad());
            }
        }
        // Strip unknown fields, including any provider-private token or URL.
        let items:Vec<Value>=items.iter().map(|i|json!({"id":i["id"],"language":i["language"],"label":i["label"],"forced":i["forced"],"sdh":i["sdh"],"match":i["match"],"downloadCount":i["downloadCount"].as_u64()})).collect();
        return Ok(json!({"items":items,"totalPages":result["totalPages"]}));
    }
    if operation == "subtitles.download"
        && result["url"]
            .as_str()
            .is_some_and(|s| s.len() <= 4096 && reqwest::Url::parse(s).is_ok())
    {
        return Ok(
            json!({"url":result["url"],"remaining":result["remaining"].as_u64(),"resetTimeUtc":result["resetTimeUtc"].as_str().filter(|s|s.len()<=80)}),
        );
    }
    Err(bad())
}

async fn invoke(
    auth: &AuthState,
    headers: &HeaderMap,
    ctx: &Context,
    package: &Package,
    generation: &str,
    operation: &str,
    mut input: Value,
) -> Result<Value, Error> {
    let id = &package.manifest.id;
    let _slot = crate::extensions::runtime::try_acquire_execution(
        id,
        crate::extensions::runtime::ExecutionClass::Interactive,
    )
    .ok_or_else(|| err(StatusCode::CONFLICT, "extension_busy"))?;
    let limited: Option<i64> =
        query_scalar("SELECT next_allowed_at FROM subtitle_provider_limits WHERE provider_id=?")
            .bind(id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    if let Some(until) = limited.filter(|until| *until > now()) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"provider_rate_limited","retryAfterSeconds":until-now()})),
        ));
    }
    if operation == "subtitles.download" {
        check_download_limit(&auth.store, id).await?;
    }
    let work:adapters::WorkContext=query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?").bind(&ctx.work_id).fetch_one(auth.store.pool()).await.map_err(db)?;
    let metadata = adapters::metadata_input(&work, &ctx.work_id, "metadata.search", &json!({}))?;
    for key in [
        "title",
        "year",
        "mediaType",
        "externalIds",
        "workId",
        "libraryId",
    ] {
        input[key] = metadata[key].clone();
    }
    if let Some((season,episode,title))=query_as::<_,(i64,i64,String)>("SELECT s.season_number,e.episode_number,COALESCE(json_extract(w.manual_json,'$.title'),w.title) FROM inventory_episodes e JOIN inventory_seasons s ON s.id=e.season_id JOIN inventory_works w ON w.id=s.series_id WHERE e.work_id=?")
        .bind(&ctx.work_id).fetch_optional(auth.store.pool()).await.map_err(db)? {
        input["seasonNumber"]=json!(season); input["episodeNumber"]=json!(episode);
        // Episode provider IDs remain episode IDs. Without one, search the series title.
        if metadata["externalIds"]["imdb"].is_null() && metadata["externalIds"]["tmdb"].is_null() {
            input["title"]=json!(title);
        }
    }
    let settings: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = settings
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let result = invoke_worker(
        auth,
        headers,
        id,
        &ctx.library_id,
        generation,
        crate::extensions::runtime::Invocation {
            wasm: crate::extensions::storage::wasm(&auth.store, package)
                .await
                .map_err(storage_error)?,
            input: ExtensionRequest {
                api_version: 1,
                operation: operation.into(),
                input: metadata_input_with_settings(input, settings),
            },
            allowed_hosts: package.manifest.http_hosts.clone(),
            auth_headers: std::collections::BTreeMap::new(),
        },
    )
    .await;
    if let Err((StatusCode::TOO_MANY_REQUESTS, body)) = &result {
        let wait = body.0["retryAfterSeconds"]
            .as_i64()
            .unwrap_or(60)
            .clamp(1, 86400);
        query("INSERT INTO subtitle_provider_limits(provider_id,next_allowed_at) VALUES(?,?) ON CONFLICT(provider_id) DO UPDATE SET next_allowed_at=excluded.next_allowed_at")
            .bind(id).bind(now()+wait).execute(auth.store.pool()).await.map_err(db)?;
    }
    result
}

async fn search(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
    Json(input): Json<SearchInput>,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;
    if !valid_language(&input.language) || !input.page.is_none_or(|p| (1..=100).contains(&p)) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_subtitle_search"));
    }
    let ctx = context(&auth, &asset).await?;
    let (package, generation) = provider(&auth, &headers, &input.provider_id, &ctx).await?;
    let result=invoke(&auth,&headers,&ctx,&package,&generation,"subtitles.search",json!({"assetId":asset,"language":input.language.to_ascii_lowercase(),"forced":input.forced,"sdh":input.sdh,"page":input.page.unwrap_or(1)})).await?;
    provider(&auth, &headers, &input.provider_id, &ctx).await?;
    let fresh = context(&auth, &asset).await?;
    if ctx.fingerprint() != fresh.fingerprint() {
        return Err(err(StatusCode::CONFLICT, "subtitle_source_changed"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("DELETE FROM subtitle_searches WHERE expires_at<=? OR (actor_id=? AND id NOT IN (SELECT id FROM subtitle_searches WHERE actor_id=? ORDER BY expires_at DESC LIMIT 15))")
        .bind(now()).bind(&actor.user.id).bind(&actor.user.id).execute(auth.store.pool()).await.map_err(db)?;
    query("INSERT INTO subtitle_searches(id,actor_id,asset_id,provider_id,generation,fingerprint,result_json,expires_at) VALUES(?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&actor.user.id).bind(&asset).bind(&input.provider_id).bind(generation).bind(ctx.fingerprint()).bind(result.to_string()).bind(now()+900).execute(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"searchId":id,"items":result["items"],"totalPages":result["totalPages"]}),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DownloadInput {
    search_id: String,
    file_id: String,
}

async fn download(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(asset): Path<String>,
    Json(input): Json<DownloadInput>,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let ctx = context(&auth, &asset).await?;
    let (provider_id,generation,fingerprint,results):(String,String,String,String)=query_as("SELECT provider_id,generation,fingerprint,result_json FROM subtitle_searches WHERE id=? AND actor_id=? AND asset_id=? AND expires_at>?")
        .bind(&input.search_id).bind(&actor.user.id).bind(&asset).bind(now()).fetch_optional(auth.store.pool()).await.map_err(db)?
        .ok_or_else(||err(StatusCode::CONFLICT,"subtitle_search_expired"))?;
    let (package, current) = provider(&auth, &headers, &provider_id, &ctx).await?;
    if current != generation || fingerprint != ctx.fingerprint() {
        return Err(err(StatusCode::CONFLICT, "subtitle_source_changed"));
    }
    let result: Value = serde_json::from_str(&results)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_subtitle_output"))?;
    let track = result["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|i| i["id"].as_str() == Some(&input.file_id))
        })
        .cloned()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "subtitle_candidate_required"))?;
    let id = uuid::Uuid::new_v4().to_string();
    check_download_limit(&auth.store, &provider_id).await?;
    reserve(
        &auth.store,
        &id,
        &asset,
        &provider_id,
        &input.file_id,
        &fingerprint,
        &track,
    )
    .await?;
    // The reservation survives cancellation/crash. Never retry a quota-consuming request implicitly.
    let mut quota = json!({});
    let acquired=async {
        let output=invoke(&auth,&headers,&ctx,&package,&generation,"subtitles.download",json!({"assetId":asset,"fileId":input.file_id})).await?;
        record_quota(&auth.store,&provider_id,&output).await?;
        quota=json!({"remaining":output["remaining"],"resetTimeUtc":output["resetTimeUtc"]});
        let url=output["url"].as_str().ok_or_else(||err(StatusCode::UNPROCESSABLE_ENTITY,"invalid_subtitle_output"))?;
        let (bytes,_)=crate::provider_download::download(&package.manifest.http_hosts,url,MAX_CONTENT).await?;
        let text=std::str::from_utf8(&bytes).map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"subtitle_encoding_unsupported"))?;
        let format=if text.trim_start_matches('\u{feff}').starts_with("WEBVTT") {"vtt"} else {"srt"};
        let vtt=media_node::subtitles::to_webvtt(text,format).map_err(|_|err(StatusCode::UNPROCESSABLE_ENTITY,"subtitle_format_unsupported"))?;
        if vtt.len()>MAX_CONTENT {return Err(err(StatusCode::PAYLOAD_TOO_LARGE,"subtitle_too_large"));}
        admin(&auth,&headers,true).await?;
        let (_,current)=provider(&auth,&headers,&provider_id,&context(&auth,&asset).await?).await?;
        if current!=generation || context(&auth,&asset).await?.fingerprint()!=fingerprint {return Err(err(StatusCode::CONFLICT,"subtitle_source_changed"));}
        let updated=query("UPDATE subtitle_downloads SET status='completed',content=? WHERE id=? AND status='pending'").bind(vtt).bind(&id).execute(auth.store.pool()).await.map_err(db)?;
        if updated.rows_affected()!=1 {return Err(err(StatusCode::CONFLICT,"subtitle_attempt_removed"));}
        Ok::<_,Error>(())
    }.await;
    if let Err(error) = acquired {
        if error.1.0["pluginError"].as_str() == Some("download_quota_exceeded") {
            // A quota rejection need not contain a reset timestamp. Remember it
            // conservatively instead of spending further calls on other files.
            record_quota(&auth.store, &provider_id, &json!({"remaining":0})).await?;
        }
        let code = error.1.0["error"]
            .as_str()
            .unwrap_or("subtitle_download_failed");
        query("UPDATE subtitle_downloads SET status='failed',error_code=? WHERE id=? AND status='pending'").bind(code).bind(&id).execute(auth.store.pool()).await.map_err(db)?;
        return Err(error);
    }
    Ok(Json(
        json!({"id":id,"status":"completed","remaining":quota["remaining"],"resetTimeUtc":quota["resetTimeUtc"]}),
    ))
}

async fn check_download_limit(
    store: &crate::identity::IdentityStore,
    provider: &str,
) -> Result<(), Error> {
    let until: Option<i64> =
        query_scalar("SELECT next_allowed_at FROM subtitle_download_limits WHERE provider_id=?")
            .bind(provider)
            .fetch_optional(store.pool())
            .await
            .map_err(db)?;
    if let Some(until) = until.filter(|until| *until > now()) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"subtitle_download_quota","retryAfterSeconds":until-now()})),
        ));
    }
    Ok(())
}

async fn record_quota(
    store: &crate::identity::IdentityStore,
    provider: &str,
    output: &Value,
) -> Result<(), Error> {
    let remaining = output["remaining"]
        .as_u64()
        .and_then(|n| i64::try_from(n).ok());
    let reset = output["resetTimeUtc"].as_str().filter(|s| s.len() <= 80);
    let next = if remaining == Some(0) {
        reset
            .and_then(|s| {
                time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
            })
            .map(|t| t.unix_timestamp())
            .filter(|t| *t > now())
            .unwrap_or_else(|| now() + 86400)
    } else {
        0
    };
    query("INSERT INTO subtitle_download_limits(provider_id,remaining,reset_time_utc,next_allowed_at) VALUES(?,?,?,?) ON CONFLICT(provider_id) DO UPDATE SET remaining=excluded.remaining,reset_time_utc=excluded.reset_time_utc,next_allowed_at=excluded.next_allowed_at")
        .bind(provider).bind(remaining).bind(reset).bind(next).execute(store.pool()).await.map_err(db)?;
    Ok(())
}

async fn reserve(
    store: &crate::identity::IdentityStore,
    id: &str,
    asset: &str,
    provider: &str,
    file: &str,
    fingerprint: &str,
    track: &Value,
) -> Result<(), Error> {
    let mut conn = store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, 0x5355425449544c45)
        .await
        .map_err(db)?;
    // Lock a durable row on PostgreSQL as well as BEGIN IMMEDIATE on SQLite.
    query("UPDATE inventory_assets SET available=available WHERE id=?")
        .bind(asset)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some((status,)) = query_as::<_, (String,)>(
        "SELECT status FROM subtitle_downloads WHERE asset_id=? AND provider_id=? AND file_id=?",
    )
    .bind(asset)
    .bind(provider)
    .bind(file)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?
    {
        return Err(err(
            StatusCode::CONFLICT,
            if status == "completed" {
                "subtitle_already_downloaded"
            } else {
                "subtitle_attempt_exists"
            },
        ));
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM subtitle_downloads WHERE asset_id=?")
        .bind(asset)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    let active:i64=query_scalar("SELECT COUNT(*) FROM subtitle_downloads WHERE provider_id=? AND status='pending' AND created_at>?")
        .bind(provider).bind(now()-RESERVATION_SECONDS).fetch_one(&mut *tx).await.map_err(db)?;
    if active > 0 {
        return Err(err(StatusCode::CONFLICT, "subtitle_provider_busy"));
    }
    // Bound DB storage including pending reservations: 16 MiB per asset, 256 MiB total.

    let total: i64 = query_scalar("SELECT COUNT(*) FROM subtitle_downloads")
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if count >= 16 || total >= 256 {
        return Err(err(StatusCode::CONFLICT, "subtitle_storage_quota"));
    }
    query("INSERT INTO subtitle_downloads(id,asset_id,provider_id,file_id,fingerprint,track_json,status,created_at) VALUES(?,?,?,?,?,?,'pending',?)")
        .bind(id).bind(asset).bind(provider).bind(file).bind(fingerprint).bind(track.to_string()).bind(now()).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)
}

async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((asset, id)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    admin_context(&auth, &asset).await?;
    let removed=query("DELETE FROM subtitle_downloads WHERE id=? AND asset_id=? AND (status!='pending' OR created_at<=?)")
        .bind(id).bind(asset).bind(now()-RESERVATION_SECONDS).execute(auth.store.pool()).await.map_err(db)?;
    if removed.rows_affected() == 0 {
        return Err(err(StatusCode::CONFLICT, "subtitle_active_or_unknown"));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn descriptors(
    store: &crate::identity::IdentityStore,
    asset: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows:Vec<(String,String)>=query_as("SELECT d.id,d.track_json FROM subtitle_downloads d JOIN inventory_assets a ON a.id=d.asset_id WHERE d.asset_id=? AND d.status='completed' AND d.fingerprint=CAST(a.size AS TEXT)||':'||a.modified_ns ORDER BY d.created_at,d.id LIMIT 16")
        .bind(asset).fetch_all(store.pool()).await?;
    rows.into_iter()
        .map(|(id, text)| {
            let mut v: Value =
                serde_json::from_str(&text).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            v["id"] = json!(format!("downloaded-{id}"));
            v["sourceFormat"] = json!("vtt");
            v["url"] = json!(format!("/api/v1/assets/{asset}/subtitles/downloaded-{id}"));
            Ok(v)
        })
        .collect()
}

/// Called only behind the common live media/library/parental middleware.
pub(crate) async fn content(
    store: &crate::identity::IdentityStore,
    asset: &str,
    id: &str,
    head: bool,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let result:Result<Option<String>,_>=query_scalar("SELECT d.content FROM subtitle_downloads d JOIN inventory_assets a ON a.id=d.asset_id JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE d.id=? AND d.asset_id=? AND d.status='completed' AND d.fingerprint=CAST(a.size AS TEXT)||':'||a.modified_ns AND a.available=1 AND l.enabled=1 AND l.root=s.root_snapshot AND n.status!='revoked'")
        .bind(id).bind(asset).fetch_optional(store.pool()).await;
    match result {
        Ok(Some(text)) => {
            let length = text.len();
            (
                [
                    (header::CONTENT_TYPE, "text/vtt; charset=utf-8"),
                    (header::CACHE_CONTROL, "private, no-store"),
                    (header::CONTENT_LENGTH, &length.to_string()),
                ],
                if head { String::new() } else { text },
            )
                .into_response()
        }
        Ok(None) => crate::error(StatusCode::NOT_FOUND, "subtitle_unavailable"),
        Err(_) => crate::error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
    }
}

#[cfg(test)]
mod tests;
