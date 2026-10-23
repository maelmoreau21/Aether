//! Extension v1: signed local packages, explicit grants and disposable Wasm workers.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    routing::{get, put},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{
    API_VERSION, ExtensionRequest, ExtensionResponse, MAX_MODULE, Manifest, MetadataProposal,
    Package,
};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    process::Stdio,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, Semaphore},
};

static MUTATIONS: Mutex<()> = Mutex::const_new(());
static EXECUTIONS: Semaphore = Semaphore::const_new(1);

#[path = "extension_adapters.rs"]
mod adapters;
#[path = "extension_avatar_collections.rs"]
mod collections;
#[path = "extension_repositories.rs"]
mod repositories;

fn metadata_input_with_settings(input: Value, settings: Value) -> Value {
    // Configuration belongs only to this installed extension. It travels over the
    // private worker pipe and is never included in package files or run history.
    let mut value = input;
    if settings
        .as_object()
        .is_some_and(|fields| !fields.is_empty())
    {
        value["config"] = settings;
    }
    value
}

type Error = (StatusCode, Json<Value>);
fn err(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error": code})))
}
fn db(_: sqlx::Error) -> Error {
    err(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.ends_with('.')
        && value.len() <= 80
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-_".contains(&c))
}

fn http_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.parse::<std::net::IpAddr>().is_err()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

pub fn validate_package(package: &Package, public_key: &[u8]) -> Result<(), &'static str> {
    let m = &package.manifest;
    if m.api_version != API_VERSION {
        return Err("incompatible_api");
    }
    if !identifier(&m.id)
        || !identifier(&package.publisher_id)
        || m.name.is_empty()
        || m.name.len() > 120
        || m.description.len() > 2048
        || m.author.is_empty()
        || m.author.len() > 120
        || m.license.is_empty()
        || m.license.len() > 80
        || m.repository.len() > 512
        || !m.repository.starts_with("https://")
        || m.version.split('.').count() != 3
        || m.version
            .split('.')
            .any(|s| s.is_empty() || s.len() > 8 || !s.bytes().all(|b| b.is_ascii_digit()))
        || m.permissions.is_empty()
        || m.http_hosts.len() > 16
        || m.http_hosts.iter().any(|host| !http_host(host))
        || m.permissions.iter().any(|p| {
            ![
                "metadata.read",
                "metadata.write",
                "avatars",
                "images",
                "providers",
            ]
            .contains(&p.as_str())
        })
    {
        return Err("invalid_manifest");
    }
    if package.wasm.len() > MAX_MODULE * 4 / 3 + 4 {
        return Err("module_too_large");
    }
    let wasm = STANDARD
        .decode(&package.wasm)
        .map_err(|_| "invalid_module")?;
    if wasm.len() > MAX_MODULE || !wasm.starts_with(b"\0asm") {
        return Err("invalid_module");
    }
    if package.sha256 != format!("{:x}", Sha256::digest(&wasm)) {
        return Err("invalid_digest");
    }
    let signature = STANDARD
        .decode(&package.signature)
        .map_err(|_| "invalid_signature")?;
    let signed = package.signing_bytes().map_err(|_| "invalid_manifest")?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&signed, &signature)
        .map_err(|_| "invalid_signature")
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .merge(repositories::router())
        .merge(collections::router())
        .route("/api/v1/admin/extensions", get(list).post(install))
        .route(
            "/api/v1/admin/extensions/publishers",
            get(publishers).post(trust),
        )
        .route(
            "/api/v1/admin/extensions/publishers/{id}",
            axum::routing::delete(revoke_publisher),
        )
        .route("/api/v1/admin/extensions/runs", get(runs))
        .route("/api/v1/admin/extensions/audit", get(audit_log))
        .route(
            "/api/v1/admin/extensions/{id}",
            put(configure).delete(uninstall),
        )
        .route(
            "/api/v1/admin/extensions/{id}/settings",
            get(get_settings).put(put_settings),
        )
        .route(
            "/api/v1/admin/extensions/{id}/run",
            axum::routing::post(run),
        )
        .route(
            "/api/v1/admin/extensions/{id}/apply",
            axum::routing::post(apply_metadata),
        )
        .route("/api/v1/admin/extensions/{id}/works", get(works))
        .route(
            "/api/v1/admin/extensions/{id}/image",
            axum::routing::post(adapters::import_image),
        )
        .route(
            "/api/v1/account/avatar/extensions",
            get(adapters::avatar_extensions),
        )
        .route(
            "/api/v1/account/avatar/extensions/{id}",
            axum::routing::post(adapters::avatar_operation),
        )
        .layer(DefaultBodyLimit::max(400_000))
        .layer(
            tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            ),
        )
        .with_state(auth)
}

/// Called only when constructing the single production Core, before serving requests.
pub async fn initialize(auth: &AuthState) -> Result<(), sqlx::Error> {
    query("UPDATE extension_runs SET status='interrupted' WHERE status='running'")
        .execute(auth.store.pool())
        .await?;
    Ok(())
}

struct RunGuard {
    auth: AuthState,
    id: String,
    finished: bool,
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let auth = self.auth.clone();
        let id = self.id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = query("UPDATE extension_runs SET status='interrupted' WHERE id=? AND status='running'")
                    .bind(id).execute(auth.store.pool()).await;
            });
        }
    }
}

async fn admin(
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
    Ok(actor)
}
async fn audit(
    conn: &mut sqlx::AnyConnection,
    actor: &str,
    id: &str,
    action: &str,
) -> Result<(), Error> {
    let authorized: i64 = query_scalar(
        "SELECT COUNT(*) FROM users WHERE id=? AND role='administrator' AND disabled_at IS NULL",
    )
    .bind(actor)
    .fetch_one(&mut *conn)
    .await
    .map_err(db)?;
    if authorized != 1 {
        return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
    }
    query(
        "INSERT INTO extension_audit(id,actor_id,extension_id,action,created_at) VALUES(?,?,?,?,?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(actor)
    .bind(id)
    .bind(action)
    .bind(now())
    .execute(&mut *conn)
    .await
    .map_err(db)?;
    Ok(())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Publisher {
    id: String,
    public_key: String,
}
async fn trust(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Publisher>,
) -> Result<(StatusCode, Json<Value>), Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    if !identifier(&input.id)
        || STANDARD
            .decode(&input.public_key)
            .map_or(true, |k| k.len() != 32)
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_publisher"));
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_publishers")
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    if count >= 32 {
        return Err(err(StatusCode::CONFLICT, "publisher_limit"));
    }
    // Rotation requires a distinct publisher ID; never silently replace a trusted key.
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES(?,?,1)")
        .bind(&input.id)
        .bind(input.public_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| err(StatusCode::CONFLICT, "publisher_exists"))?;
    audit(&mut tx, &actor.user.id, &input.id, "publisher.trusted").await?;
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::CREATED, Json(json!({"id": input.id}))))
}
async fn publishers(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, i64)> =
        query_as("SELECT id,public_key,active FROM extension_publishers ORDER BY id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    Ok(Json(
        json!({"publishers": rows.into_iter().map(|(id,key,active)| json!({"id":id,"publicKey":key,"active":active==1})).collect::<Vec<_>>()}),
    ))
}
async fn revoke_publisher(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("UPDATE extension_publishers SET active=0 WHERE id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    audit(&mut tx, &actor.user.id, &id, "publisher.revoked").await?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Installed {
    manifest: Manifest,
    publisher_id: String,
    sha256: String,
    enabled: bool,
    library_ids: Vec<String>,
    all_libraries: bool,
    settings: Value,
}
async fn list(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, i64, i64)> =
        query_as("SELECT id,package_json,enabled,all_libraries FROM extensions ORDER BY id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let mut installed = Vec::new();
    for (id, bytes, enabled, all_libraries) in rows {
        let package: Package = serde_json::from_str(&bytes)
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_stored_package"))?;
        let grants: Vec<String> = query_scalar(
            "SELECT library_id FROM extension_grants WHERE extension_id=? ORDER BY library_id",
        )
        .bind(&id)
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
        let settings_str: Option<String> =
            query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
                .bind(&id)
                .fetch_optional(auth.store.pool())
                .await
                .map_err(db)?;
        let settings: Value = settings_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({}));
        installed.push(Installed {
            manifest: package.manifest,
            publisher_id: package.publisher_id,
            sha256: package.sha256,
            enabled: enabled == 1,
            library_ids: grants,
            all_libraries: all_libraries == 1,
            settings,
        });
    }
    Ok(Json(json!({"apiVersion":1,"extensions":installed})))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallPackage {
    #[serde(flatten)]
    package: Package,
    public_key: Option<String>,
}
async fn install(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<InstallPackage>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let key = if let Some(key) = input.public_key {
        key
    } else {
        // Compatibility for old local imports; new imports supply their public key directly.
        query_scalar("SELECT public_key FROM extension_publishers WHERE id=? AND active=1")
            .bind(&input.package.publisher_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "package_public_key_required"))?
    };
    install_with_key(auth, headers, input.package, key).await
}
async fn install_with_key(
    auth: AuthState,
    headers: HeaderMap,
    package: Package,
    public_key: String,
) -> Result<(StatusCode, Json<Value>), Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    let key = STANDARD
        .decode(&public_key)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_public_key"))?;
    validate_package(&package, &key).map_err(|code| err(StatusCode::BAD_REQUEST, code))?;
    // Registry identity is the key digest, not a publisher name supplied by a repository.
    // Different authors may use the same display name without replacing another key.
    let key_id: Option<String> =
        query_scalar("SELECT id FROM extension_publishers WHERE id=? AND public_key=?")
            .bind(&package.publisher_id)
            .bind(&public_key)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let key_id = key_id.unwrap_or_else(|| format!("key.{:x}", Sha256::digest(&key)));
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions")
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    if count >= 32 {
        return Err(err(StatusCode::CONFLICT, "extension_limit"));
    }
    let bytes = serde_json::to_string(&package)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_package"))?;
    // Duplicate IDs are refused: updates must not inherit previous consents.
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES(?,?,1) ON CONFLICT(id) DO UPDATE SET active=1")
        .bind(&key_id).bind(&public_key).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO extensions(id,publisher_id,package_json,generation,all_libraries) VALUES(?,?,?,?,?)")
        .bind(&package.manifest.id)
        .bind(&key_id)
        .bind(bytes)
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(i64::from(package.manifest.permissions.iter().any(|permission|permission=="metadata.read")))
        .execute(&mut *tx)
        .await
        .map_err(|_| err(StatusCode::CONFLICT, "extension_exists"))?;
    audit(
        &mut tx,
        &actor.user.id,
        &package.manifest.id,
        &format!("installed:{}:{}", package.manifest.version, package.sha256),
    )
    .await?;
    crate::extension_storage::persist(&auth.store, &package)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_failed"))?;
    tx.commit().await.map_err(db)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": package.manifest.id})),
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    enabled: bool,
    #[serde(default)]
    library_ids: Vec<String>,
    #[serde(default)]
    all_libraries: bool,
}
async fn configure(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Configuration>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    if input.library_ids.len() > 100 {
        return Err(err(StatusCode::BAD_REQUEST, "grant_limit"));
    }
    for library in &input.library_ids {
        let count: i64 = query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND n.status!='revoked'")
            .bind(library).fetch_one(auth.store.pool()).await.map_err(db)?;
        if count != 1 {
            return Err(err(StatusCode::BAD_REQUEST, "invalid_library"));
        }
    }
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    let changed = query("UPDATE extensions SET enabled=?,generation=?,all_libraries=? WHERE id=? AND (?=0 OR EXISTS(SELECT 1 FROM extension_publishers p WHERE p.id=extensions.publisher_id AND p.active=1))")
        .bind(i64::from(input.enabled)).bind(uuid::Uuid::new_v4().to_string()).bind(i64::from(input.all_libraries)).bind(&id).bind(i64::from(input.enabled)).execute(&mut *tx).await.map_err(db)?;
    if changed.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "extension_unavailable"));
    }
    query("DELETE FROM extension_grants WHERE extension_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    for library in &input.library_ids {
        query("INSERT INTO extension_grants(extension_id,library_id) VALUES(?,?) ON CONFLICT DO NOTHING").bind(&id).bind(library).execute(&mut *tx).await.map_err(db)?;
    }
    let action = json!({"action": if input.enabled { "permissions.enabled" } else { "permissions.disabled" }, "libraryIds": input.library_ids, "allLibraries": input.all_libraries}).to_string();
    audit(&mut tx, &actor.user.id, &id, &action).await?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn get_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let exists: Option<i64> = query_scalar("SELECT 1 FROM extensions WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    if exists.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "extension_not_found"));
    }
    let settings_str: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = settings_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    Ok(Json(json!({
        "apiVersion": 1,
        "extensionId": id,
        "settings": settings,
    })))
}
async fn put_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(settings): Json<Value>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    let exists: Option<i64> = query_scalar("SELECT 1 FROM extensions WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    if exists.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "extension_not_found"));
    }
    let settings_str = serde_json::to_string(&settings)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_settings"))?;
    if settings_str.len() > 65_536 {
        return Err(err(StatusCode::BAD_REQUEST, "settings_too_large"));
    }
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("INSERT INTO extension_settings(extension_id, settings_json) VALUES(?, ?) ON CONFLICT(extension_id) DO UPDATE SET settings_json=excluded.settings_json")
        .bind(&id)
        .bind(&settings_str)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    audit(&mut tx, &actor.user.id, &id, "settings.updated").await?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Removal {
    delete_data: bool,
}
async fn uninstall(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Removal>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("DELETE FROM extensions WHERE id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if input.delete_data {
        query("DELETE FROM extension_runs WHERE extension_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    audit(
        &mut tx,
        &actor.user.id,
        &id,
        if input.delete_data {
            "uninstalled.purged"
        } else {
            "uninstalled.retained"
        },
    )
    .await?;
    tx.commit().await.map_err(db)?;
    crate::extension_storage::remove(&auth.store, &id)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyMetadata {
    work_id: String,
    metadata: Value,
}

async fn apply_metadata(
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
struct Run {
    work_id: String,
    #[serde(default = "default_operation")]
    operation: String,
    #[serde(default)]
    input: Value,
}
fn default_operation() -> String {
    "metadata.propose".into()
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: u32,
}
async fn works(
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

async fn permitted(
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
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id)) JOIN media_nodes n ON n.id=l.node_id WHERE e.id=? AND l.id=? AND e.generation=? AND e.enabled=1 AND p.active=1 AND l.enabled=1 AND n.status!='revoked'")
        .bind(id).bind(library).bind(generation).fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(count == 1)
}

async fn run(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Run>,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _slot = EXECUTIONS
        .try_acquire()
        .map_err(|_| err(StatusCode::CONFLICT, "extension_busy"))?;
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
    let settings_str: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = settings_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));

    adapters::authorize_operation(&package.manifest, &input.operation)?;
    let mut payload =
        adapters::metadata_input(&context, &input.work_id, &input.operation, &input.input)?;
    payload = metadata_input_with_settings(payload, settings);
    let operation = input.operation.clone();
    let invocation = crate::extension_runtime::Invocation {
        wasm: crate::extension_storage::wasm(&auth.store, &package)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_failed"))?,
        input: ExtensionRequest {
            api_version: 1,
            operation: operation.clone(),
            input: payload,
        },
        allowed_hosts: package.manifest.http_hosts.clone(),
        auth_headers: std::collections::BTreeMap::new(),
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO extension_runs(id,extension_id,library_id,work_id,actor_id,started_at,status) VALUES(?,?,?,?,?,?,'running')")
        .bind(&run_id).bind(&id).bind(&library).bind(&input.work_id).bind(&actor.user.id).bind(now()).execute(auth.store.pool()).await.map_err(db)?;
    let mut guard = RunGuard {
        auth: auth.clone(),
        id: run_id.clone(),
        finished: false,
    };
    let started = Instant::now();
    let result = invoke_worker(&auth, &headers, &id, &library, &generation, invocation).await;
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
    query("UPDATE extension_runs SET status=?,result_json=?,duration_ms=? WHERE id=?")
        .bind(status)
        .bind(details.to_string())
        .bind(duration)
        .bind(&run_id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    guard.finished = true;
    query("DELETE FROM extension_runs WHERE status!='running' AND id NOT IN (SELECT id FROM extension_runs ORDER BY started_at DESC,id DESC LIMIT 1000)")
        .execute(auth.store.pool()).await.map_err(db)?;
    let proposal = result?;
    Ok(Json(
        json!({"runId":run_id,"extensionId":id,"version":package.manifest.version,"sha256":package.sha256,"durationMs":duration,"operation":operation,"proposal":proposal}),
    ))
}

async fn invoke_worker(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
    invocation: crate::extension_runtime::Invocation,
) -> Result<Value, Error> {
    let requested_operation = invocation.input.operation.clone();
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
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        stdin
            .write_all(&payload)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        drop(stdin);
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?
            .take((media_extension_sdk::MAX_JSON + 1) as u64);
        let mut bytes = Vec::new();
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        let status = child
            .wait()
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        if !status.success() || bytes.len() > media_extension_sdk::MAX_JSON {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "extension_failed"));
        }
        let response: ExtensionResponse = serde_json::from_slice(&bytes)
            .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"))?;
        if response.api_version != 1 {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_output"));
        }
        adapters::validate_result(&requested_operation, response.result)
    };
    tokio::pin!(operation);
    let deadline = tokio::time::sleep(Duration::from_secs(
        crate::extension_runtime::INVOCATION_TIMEOUT_SECS,
    ));
    tokio::pin!(deadline);
    let mut check = tokio::time::interval(Duration::from_millis(30));
    loop {
        tokio::select! {
            result = &mut operation => {
                if !permitted(auth, headers, id, library, generation).await? { return Err(err(StatusCode::FORBIDDEN, "extension_revoked")); }
                return result;
            }
            _ = &mut deadline => return Err(err(StatusCode::REQUEST_TIMEOUT, "extension_timeout")),
            _ = check.tick() => {
                if !permitted(auth, headers, id, library, generation).await? { return Err(err(StatusCode::FORBIDDEN, "extension_revoked")); }
            }
        }
    }
}
#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    extension_id: String,
    library_id: String,
    work_id: String,
    started_at: i64,
    duration_ms: i64,
    status: String,
    result_json: String,
}
async fn runs(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<RunRow> = query_as("SELECT id,extension_id,library_id,work_id,started_at,duration_ms,status,result_json FROM extension_runs ORDER BY started_at DESC,id DESC LIMIT 100").fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"runs":rows.into_iter().map(|r| json!({"id":r.id,"extensionId":r.extension_id,"libraryId":r.library_id,"workId":r.work_id,"startedAt":r.started_at,"durationMs":r.duration_ms,"status":r.status,"result":serde_json::from_str::<Value>(&r.result_json).unwrap_or(Value::Null)})).collect::<Vec<_>>()}),
    ))
}
async fn audit_log(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String,String,String,i64)> = query_as("SELECT actor_id,extension_id,action,created_at FROM extension_audit ORDER BY created_at DESC,id DESC LIMIT 100").fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"audit":rows.into_iter().map(|(actor,id,action,time)| json!({"actorId":actor,"extensionId":id,"action":action,"createdAt":time})).collect::<Vec<_>>()}),
    ))
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use media_extension_sdk::MetadataInput;

    #[test]
    fn worker_receives_only_the_selected_extensions_configuration() {
        let value = metadata_input_with_settings(
            json!(MetadataInput {
                api_version: 1,
                work_id: "work".into(),
                library_id: "library".into(),
                title: "Film".into(),
            }),
            json!({"api_key": "synthetic-key", "language": "fr-FR"}),
        );
        assert_eq!(value["config"]["api_key"], "synthetic-key");
        assert_eq!(value["title"], "Film");
        assert!(value.get("settings").is_none());
        let legacy = metadata_input_with_settings(
            json!(MetadataInput {
                api_version: 1,
                work_id: "work".into(),
                library_id: "library".into(),
                title: "Film".into(),
            }),
            json!({}),
        );
        assert!(serde_json::from_value::<MetadataInput>(legacy).is_ok());
    }
}
