//! Read-only, resumable migration. Imported records never enqueue acquisition or notifications.
use crate::{
    acquisition::transport::{self, Connection},
    applications::{self, Error, db, err},
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

const PAGE_SIZE: i64 = 10;
const MAX_RECORDS: i64 = 1_000_000;
fn now() -> i64 {
    media_contracts::protocol::unix_now()
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/imports", get(list))
        .route("/api/v1/admin/imports/preview", post(preview))
        .route("/api/v1/admin/imports/{id}/start", post(start))
        .route("/api/v1/admin/imports/{id}/run", post(run))
        .route("/api/v1/admin/imports/{id}/pause", post(pause))
        .layer(DefaultBodyLimit::max(65536))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}

async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = auth
        .authenticate(headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    if actor.user.role != Role::Administrator {
        return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
    }
    if mutation {
        auth.require_csrf(headers, &actor)
            .map_err(|s| err(s, "csrf_required"))?;
    }
    if !applications::enabled(&auth.store, "requests").await?
        && !applications::enabled(&auth.store, "reports").await?
    {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "extension_disabled"));
    }
    Ok(actor)
}

#[derive(sqlx::FromRow)]
struct Import {
    id: String,
    source_url: String,
    connection_json: String,
    status: String,
    phase: String,
    page_offset: i64,
    processed: i64,
    errors: i64,
    mappings_json: String,
    preview_json: String,
    lease_token: Option<String>,
    lease_until: i64,
    updated_at: i64,
    last_error: Option<String>,
}
impl Import {
    fn public(&self) -> Value {
        let preview: Value = serde_json::from_str(&self.preview_json).unwrap_or(Value::Null);
        json!({"id":self.id,"sourceUrl":self.source_url,"status":self.status,
        "phase":self.phase,"processed":self.processed,"errors":self.errors,"updatedAt":self.updated_at,"lastError":self.last_error,
        "failures":preview["failures"].as_array().cloned().unwrap_or_default()})
    }
}
async fn record(store: &IdentityStore, id: &str) -> Result<Import, Error> {
    query_as("SELECT * FROM application_imports WHERE id=?")
        .bind(id)
        .fetch_optional(store.pool())
        .await
        .map_err(db)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "import_not_found"))
}

async fn list(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    administrator(&auth, &headers, false).await?;
    let rows: Vec<Import> =
        query_as("SELECT * FROM application_imports ORDER BY updated_at DESC,id LIMIT 100")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    Ok(Json(
        json!({"imports":rows.iter().map(Import::public).collect::<Vec<_>>()}),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewInput {
    base_url: String,
    api_key: String,
}
fn source_id(connection: &Connection) -> String {
    format!("{:x}", Sha256::digest(connection.base_url.as_bytes()))
}
fn source_identifier(value: &Value) -> Option<String> {
    value
        .as_i64()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .or_else(|| {
            value
                .as_str()
                .filter(|id| {
                    !id.is_empty()
                        && id.len() <= 128
                        && id
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                })
                .map(str::to_owned)
        })
}
fn label(value: &Value) -> String {
    value["displayName"]
        .as_str()
        .or_else(|| value["username"].as_str())
        .or_else(|| value["name"].as_str())
        .unwrap_or("Auteur historique")
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect()
}
fn results(value: &Value) -> Result<&Vec<Value>, Error> {
    value["results"]
        .as_array()
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_import_source_response"))
}
fn count(value: &Value) -> Result<i64, Error> {
    value["pageInfo"]["results"]
        .as_i64()
        .filter(|n| (0..=MAX_RECORDS).contains(n))
        .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_import_source_count"))
}
async fn page(connection: &Connection, kind: &str, take: i64, offset: i64) -> Result<Value, Error> {
    let path = match kind {
        "requests" => "api/v1/request",
        "reports" => "api/v1/issue",
        "users" => "api/v1/user",
        _ => return Err(err(StatusCode::BAD_REQUEST, "invalid_import_phase")),
    };
    transport::request(
        connection,
        "GET",
        path,
        &[
            ("take", take.to_string()),
            ("skip", offset.to_string()),
            ("sort", "added".into()),
        ],
        None,
    )
    .await
}

async fn preview(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<PreviewInput>,
) -> Result<(StatusCode, Json<Value>), Error> {
    administrator(&auth, &headers, true).await?;
    let connection = transport::configure(&input.base_url, &input.api_key).await?;
    let requests = page(&connection, "requests", 1, 0).await?;
    let reports = page(&connection, "reports", 1, 0).await?;
    results(&requests)?;
    results(&reports)?;
    let mut users = Vec::new();
    let mut warnings=vec!["Suspendre les nouvelles demandes dans la source avant l’import final. Les dossiers importés restent suspendus ; aucune acquisition ni notification n’est lancée.".to_owned()];
    for offset in (0..1000).step_by(100) {
        let response = page(&connection, "users", 100, offset).await?;
        let entries = results(&response)?;
        for user in entries {
            let id = source_identifier(&user["id"])
                .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_import_source_user"))?;
            users.push(json!({"id":id,"name":label(user)}));
        }
        if entries.len() < 100 {
            break;
        }
        if offset == 900 {
            warnings.push("Aperçu limité aux 1 000 premiers comptes ; les autres auteurs seront conservés comme auteurs historiques.".into());
        }
    }
    let mut servers = Vec::new();
    for kind in ["radarr", "sonarr"] {
        let response = transport::request(
            &connection,
            "GET",
            &format!("api/v1/settings/{kind}"),
            &[],
            None,
        )
        .await?;
        let entries = response
            .as_array()
            .filter(|s| s.len() <= 1000)
            .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_import_source_servers"))?;
        for server in entries {
            let id = source_identifier(&server["id"])
                .ok_or_else(|| err(StatusCode::BAD_GATEWAY, "invalid_import_source_server"))?;
            // Never persist the source configuration: it contains acquisition API keys.
            servers.push(json!({"id":format!("{kind}:{id}"),"name":label(server),"kind":kind,
                "isDefault":server["isDefault"].as_bool().unwrap_or(false),"is4k":server["is4k"].as_bool().unwrap_or(false)}));
        }
    }
    let id = Uuid::new_v4().to_string();
    let public = json!({"id":id,"sourceUrl":connection.base_url,"users":users,"servers":servers,
        "counts":{"requests":count(&requests)?,"reports":count(&reports)?},"warnings":warnings});
    let serialized = serde_json::to_string(&connection)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    query("INSERT INTO application_imports(id,source_url,connection_json,status,phase,preview_json,updated_at) VALUES(?,?,?,'preview','requests',?,?)")
        .bind(&id).bind(&connection.base_url).bind(serialized).bind(public.to_string()).bind(now()).execute(auth.store.pool()).await.map_err(db)?;
    Ok((StatusCode::CREATED, Json(public)))
}

#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mappings {
    #[serde(default)]
    user_mappings: BTreeMap<String, String>,
    #[serde(default)]
    server_mappings: BTreeMap<String, String>,
}

async fn validate_mappings(
    store: &IdentityStore,
    preview: &Value,
    mappings: &Mappings,
) -> Result<(), Error> {
    if mappings.user_mappings.len() > 1000 || mappings.server_mappings.len() > 2000 {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "import_mapping_limit",
        ));
    }
    let users = preview["users"]
        .as_array()
        .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    let servers = preview["servers"]
        .as_array()
        .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    for (source, target) in &mappings.user_mappings {
        if !users.iter().any(|v| v["id"].as_str() == Some(source)) {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "unknown_source_user"));
        }
        let count: i64 =
            query_scalar("SELECT COUNT(*) FROM users WHERE id=? AND disabled_at IS NULL")
                .bind(target)
                .fetch_one(store.pool())
                .await
                .map_err(db)?;
        if count != 1 {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_target_user"));
        }
    }
    for (source, target) in &mappings.server_mappings {
        let server = servers
            .iter()
            .find(|v| v["id"].as_str() == Some(source))
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "unknown_source_server"))?;
        let kind = server["kind"]
            .as_str()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM acquisition_servers WHERE id=? AND kind=? AND enabled=1",
        )
        .bind(target)
        .bind(kind)
        .fetch_one(store.pool())
        .await
        .map_err(db)?;
        if count != 1 {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_target_server",
            ));
        }
    }
    Ok(())
}

async fn start(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mappings): Json<Mappings>,
) -> Result<Json<Value>, Error> {
    administrator(&auth, &headers, true).await?;
    let existing = record(&auth.store, &id).await?;
    let preview: Value = serde_json::from_str(&existing.preview_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    validate_mappings(&auth.store, &preview, &mappings).await?;
    for cap in ["requests", "reports"] {
        if preview["counts"][cap].as_i64().unwrap_or(0) > 0
            && !applications::enabled(&auth.store, cap).await?
        {
            return Err(err(
                StatusCode::SERVICE_UNAVAILABLE,
                "import_extension_required",
            ));
        }
    }
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_scoped(&mut conn, "application-import", &id)
        .await
        .map_err(db)?;
    let current: Import = query_as("SELECT * FROM application_imports WHERE id=?")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if current.status == "running" || current.lease_until > now() {
        return Err(err(StatusCode::CONFLICT, "import_busy"));
    }
    let reset = matches!(
        current.status.as_str(),
        "completed" | "completed_with_errors" | "failed"
    );
    let (phase, offset, processed, errors) = if reset {
        ("requests", 0, 0, 0)
    } else {
        (
            current.phase.as_str(),
            current.page_offset,
            current.processed,
            current.errors,
        )
    };
    query("UPDATE application_imports SET status='running',phase=?,page_offset=?,processed=?,errors=?,mappings_json=?,lease_token=NULL,lease_until=0,last_error=NULL,updated_at=? WHERE id=?")
        .bind(phase).bind(offset).bind(processed).bind(errors).bind(json!(mappings).to_string()).bind(now()).bind(&id).execute(&mut *tx).await.map_err(db)?;
    crate::database::commit(tx).await.map_err(db)?;
    Ok(Json(record(&auth.store, &id).await?.public()))
}

async fn pause(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    administrator(&auth, &headers, true).await?;
    record(&auth.store, &id).await?;
    // The lease is retained until its current bounded page stops; /start cannot overlap it.
    query("UPDATE application_imports SET status='paused',updated_at=? WHERE id=? AND status='running'")
        .bind(now()).bind(&id).execute(auth.store.pool()).await.map_err(db)?;
    Ok(Json(record(&auth.store, &id).await?.public()))
}
async fn run(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    administrator(&auth, &headers, true).await?;
    let state = record(&auth.store, &id).await?;
    if state.lease_until > now() {
        return Err(err(StatusCode::CONFLICT, "import_busy"));
    }
    if state.status == "paused" {
        query("UPDATE application_imports SET status='running',last_error=NULL,updated_at=? WHERE id=? AND status='paused' AND lease_until<=?")
            .bind(now()).bind(&id).bind(now()).execute(auth.store.pool()).await.map_err(db)?;
    } else if state.status == "failed" {
        query("UPDATE application_imports SET status='running',phase='requests',page_offset=0,processed=0,errors=0,last_error=NULL,updated_at=? WHERE id=? AND status='failed' AND lease_until<=?")
            .bind(now()).bind(&id).bind(now()).execute(auth.store.pool()).await.map_err(db)?;
    }
    process(&auth.store, &id).await?;
    Ok(Json(record(&auth.store, &id).await?.public()))
}

fn server_key(preview: &Value, value: &Value) -> Option<String> {
    let kind = match value["media"]["mediaType"]
        .as_str()
        .or_else(|| value["type"].as_str())
    {
        Some("movie") => "radarr",
        Some("tv") => "sonarr",
        _ => return None,
    };
    let is4k = value["is4k"].as_bool().unwrap_or(false);
    let direct = source_identifier(&value["serverId"]).or_else(|| {
        source_identifier(&value["media"][if is4k { "serviceId4k" } else { "serviceId" }])
    });
    if let Some(id) = direct {
        return Some(format!("{kind}:{id}"));
    }
    let defaults: Vec<_> = preview["servers"]
        .as_array()?
        .iter()
        .filter(|s| s["kind"] == kind && s["isDefault"] == true && s["is4k"] == is4k)
        .collect();
    if defaults.len() == 1 {
        defaults[0]["id"].as_str().map(str::to_owned)
    } else {
        None
    }
}
async fn active_user(
    store: &IdentityStore,
    mapped: Option<&String>,
) -> Result<Option<String>, Error> {
    let Some(user) = mapped else {
        return Ok(None);
    };
    query_scalar("SELECT id FROM users WHERE id=? AND disabled_at IS NULL")
        .bind(user)
        .fetch_optional(store.pool())
        .await
        .map_err(db)
}

async fn process(store: &IdentityStore, id: &str) -> Result<(), Error> {
    let token = Uuid::new_v4().to_string();
    let timestamp = now();
    let mut conn = store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_scoped(&mut conn, "application-import", id)
        .await
        .map_err(db)?;
    let claimed=query("UPDATE application_imports SET lease_token=?,lease_until=?,updated_at=? WHERE id=? AND status='running' AND lease_until<=?")
        .bind(&token).bind(timestamp+90).bind(timestamp).bind(id).bind(timestamp).execute(&mut *tx).await.map_err(db)?.rows_affected();
    crate::database::commit(tx).await.map_err(db)?;
    if claimed == 0 {
        return Ok(());
    }
    let state = record(store, id).await?;
    let result = process_claimed(store, &state, &token).await;
    if let Err((_, body)) = &result {
        let code = body.0["error"].as_str().unwrap_or("import_failed");
        let status = if code == "extension_disabled" {
            "paused"
        } else {
            "failed"
        };
        query("UPDATE application_imports SET status=?,lease_token=NULL,lease_until=0,last_error=?,updated_at=? WHERE id=? AND lease_token=?")
            .bind(status).bind(code).bind(now()).bind(id).bind(&token).execute(store.pool()).await.map_err(db)?;
    }
    result
}

async fn process_claimed(store: &IdentityStore, state: &Import, token: &str) -> Result<(), Error> {
    if !matches!(state.phase.as_str(), "requests" | "reports") {
        return Err(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_import_phase",
        ));
    }
    if state.page_offset > MAX_RECORDS {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "import_record_limit"));
    }
    let connection: Connection = serde_json::from_str(&state.connection_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    let preview: Value = serde_json::from_str(&state.preview_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    if !applications::enabled(store, &state.phase).await? {
        if preview["counts"][&state.phase].as_i64() == Some(0) {
            advance(store, state, token).await?;
            return Ok(());
        }
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "extension_disabled"));
    }
    let mappings: Mappings = serde_json::from_str(&state.mappings_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
    let response = page(&connection, &state.phase, PAGE_SIZE, state.page_offset).await?;
    let entries = results(&response)?;
    if entries.len() > PAGE_SIZE as usize {
        return Err(err(StatusCode::BAD_GATEWAY, "invalid_import_source_page"));
    }
    let source = source_id(&connection);
    for value in entries {
        let current = record(store, &state.id).await?;
        if current.status != "running"
            || current.lease_token.as_deref() != Some(token)
            || current.lease_until <= now()
        {
            break;
        }
        let owner = if state.phase == "requests" {
            &value["requestedBy"]["id"]
        } else {
            &value["createdBy"]["id"]
        };
        let owner = source_identifier(owner);
        let user = active_user(
            store,
            owner.as_ref().and_then(|id| mappings.user_mappings.get(id)),
        )
        .await?;
        let imported = if state.phase == "requests" {
            let server =
                server_key(&preview, value).and_then(|key| mappings.server_mappings.get(&key));
            crate::acquisition::import_legacy(
                store,
                &source,
                value,
                user.as_deref(),
                server.map(String::as_str),
            )
            .await
        } else {
            crate::reports::import_legacy(
                store,
                &source,
                value,
                user.as_deref(),
                &mappings.user_mappings,
            )
            .await
        };
        let failed = imported.is_err();
        let code = imported
            .err()
            .and_then(|(_, body)| body.0["error"].as_str().map(str::to_owned));
        let mut public_preview: Value = serde_json::from_str(&current.preview_json)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_import_state"))?;
        if let Some(code) = &code {
            if !public_preview["failures"].is_array() {
                public_preview["failures"] = json!([]);
            }
            if let Some(failures) = public_preview["failures"].as_array_mut()
                && failures.len() < 100
            {
                failures.push(json!({"phase":state.phase,"sourceId":source_identifier(&value["id"]),"error":code}));
            }
        }
        query("UPDATE application_imports SET page_offset=page_offset+1,processed=processed+1,errors=errors+?,last_error=COALESCE(?,last_error),preview_json=?,updated_at=? WHERE id=? AND lease_token=? AND status='running'")
            .bind(i64::from(failed)).bind(code).bind(public_preview.to_string()).bind(now()).bind(&state.id).bind(token).execute(store.pool()).await.map_err(db)?;
    }
    let current = record(store, &state.id).await?;
    if current.status == "running"
        && current.lease_token.as_deref() == Some(token)
        && current.page_offset == state.page_offset + entries.len() as i64
        && entries.len() < PAGE_SIZE as usize
    {
        advance(store, state, token).await?;
    }
    query("UPDATE application_imports SET lease_token=NULL,lease_until=0,updated_at=? WHERE id=? AND lease_token=?")
        .bind(now()).bind(&state.id).bind(token).execute(store.pool()).await.map_err(db)?;
    Ok(())
}

async fn advance(store: &IdentityStore, state: &Import, token: &str) -> Result<(), Error> {
    if state.phase == "requests" {
        query("UPDATE application_imports SET phase='reports',page_offset=0,lease_token=NULL,lease_until=0,updated_at=? WHERE id=? AND lease_token=? AND status='running'")
            .bind(now()).bind(&state.id).bind(token).execute(store.pool()).await.map_err(db)?;
    } else {
        query("UPDATE application_imports SET status=CASE WHEN errors=0 THEN 'completed' ELSE 'completed_with_errors' END,lease_token=NULL,lease_until=0,updated_at=? WHERE id=? AND lease_token=? AND status='running'")
            .bind(now()).bind(&state.id).bind(token).execute(store.pool()).await.map_err(db)?;
    }
    Ok(())
}

/// One bounded page per tick; durable offsets and record keys make interruption replay safe.
pub async fn tick(store: &IdentityStore) -> Result<(), Error> {
    let id:Option<String>=query_scalar("SELECT id FROM application_imports WHERE status='running' AND lease_until<=? ORDER BY updated_at,id LIMIT 1")
        .bind(now()).fetch_optional(store.pool()).await.map_err(db)?;
    if let Some(id) = id {
        process(store, &id).await?;
    }
    Ok(())
}
