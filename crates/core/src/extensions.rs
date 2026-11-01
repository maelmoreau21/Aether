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

#[path = "extension_adapters.rs"]
mod adapters;
#[path = "extension_artwork_rotation.rs"]
mod artwork_rotation;
#[path = "extension_avatar_collections.rs"]
pub(crate) mod collections;
#[path = "extension_identify.rs"]
mod identify;
#[path = "extension_image_collection.rs"]
mod image_collection;
#[path = "extension_repositories.rs"]
pub(crate) mod repositories;

const DEFAULT_COMMUNITY_TMDB_API_KEY: &str = "4219e299c89411838049ab0dab19ebd5";

fn resolve_effective_extension_settings(extension_id: &str, mut settings: Value) -> Value {
    if extension_id == "tmdb"
        && settings
            .get("api_key")
            .and_then(Value::as_str)
            .is_none_or(|k| k.trim().is_empty())
    {
        let env_key = std::env::var("TMDB_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());
        let default_key = env_key.unwrap_or_else(|| DEFAULT_COMMUNITY_TMDB_API_KEY.to_string());
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("api_key".into(), json!(default_key));
        }
    }
    settings
}

fn metadata_input_with_settings(input: Value, mut settings: Value) -> Value {
    // Historical forms saved an empty endpoint. Omit it so the provider can use
    // its official default rather than attempting a request against an empty URL.
    if let Some(fields) = settings.as_object_mut() {
        for key in ["base_url", "baseUrl"] {
            if fields
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim().is_empty())
            {
                fields.remove(key);
            }
        }
    }
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

fn music_album_search_input(track: &str, artist: &str, album: &str) -> Option<Value> {
    if artist.trim().is_empty()
        || artist.eq_ignore_ascii_case("unknown artist")
        || album.trim().is_empty()
        || album.eq_ignore_ascii_case("unknown album")
    {
        return None;
    }
    Some(
        json!({"title":album, "artist":artist, "album":album, "trackTitle":track,
        "mediaType":"music", "externalIds":{"artist":artist}}),
    )
}

fn lyrics_request_input(title: &str, artist: &str, album: &str, duration: Option<f64>) -> Value {
    json!({
        "track": title,
        "artist": artist,
        "album": album,
        "durationSecs": duration.filter(|value| value.is_finite() && *value >= 0.0 && *value <= u32::MAX as f64).map(|value| value.round() as u32),
    })
}

// A process-local key prevents offline guessing of low-entropy provider secrets
// from cache keys. Old cache entries naturally miss after restart.
fn provider_cache_context(
    payload: &Value,
    config: &Value,
    generation: &str,
    package_sha: &str,
    music_search: bool,
) -> Result<Value, serde_json::Error> {
    static KEY: std::sync::OnceLock<ring::hmac::Key> = std::sync::OnceLock::new();
    let key = KEY.get_or_init(|| {
        ring::hmac::Key::new(ring::hmac::HMAC_SHA256, uuid::Uuid::new_v4().as_bytes())
    });
    let tag = ring::hmac::sign(key, &serde_json::to_vec(config)?);
    Ok(
        json!({"request": payload, "generation": generation, "packageSha256": package_sha,
        "settingsTag": STANDARD.encode(tag.as_ref()), "musicMatchingVersion": if music_search { 2 } else { 0 }}),
    )
}

type Error = (StatusCode, Json<Value>);
pub(super) fn storage_error(error: std::io::Error) -> Error {
    eprintln!(
        "Plugin storage operation failed ({:?}): {}",
        error.kind(),
        error
    );
    let (status, code) = match error.kind() {
        std::io::ErrorKind::PermissionDenied => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "plugin_storage_permission_denied",
        ),
        std::io::ErrorKind::AlreadyExists => (StatusCode::CONFLICT, "plugin_storage_conflict"),
        std::io::ErrorKind::InvalidData => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "plugin_storage_integrity_failed",
        ),
        std::io::ErrorKind::NotFound => {
            (StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_missing")
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "plugin_storage_failed"),
    };
    err(status, code)
}
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
                "storage.private",
                "playback.markers",
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
            "/api/v1/admin/extensions/storage/health",
            get(storage_health),
        )
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
            "/api/v1/admin/extensions/images/collect",
            axum::routing::post(image_collection::collect),
        )
        .route(
            "/api/v1/admin/extensions/images/candidates",
            axum::routing::post(image_collection::search_candidates),
        )
        .route(
            "/api/v1/admin/extensions/identify/search",
            axum::routing::post(identify::search),
        )
        .route(
            "/api/v1/admin/extensions/identify/apply",
            axum::routing::post(identify::apply),
        )
        .route(
            "/api/v1/admin/artwork/rotate",
            axum::routing::post(artwork_rotation::rotate),
        )
        .route(
            "/api/v1/admin/artwork/collect-batch",
            axum::routing::post(image_collection::collect_batch),
        )
        .route(
            "/api/v1/admin/artwork/works",
            get(image_collection::list_works),
        )
        .route(
            "/api/v1/admin/extensions/{id}",
            put(configure).delete(uninstall),
        )
        .route(
            "/api/v1/admin/extensions/{id}/upgrade",
            axum::routing::post(upgrade),
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

/// Recover stale invocation history without interrupting another active Core.
pub async fn initialize(auth: &AuthState) -> Result<(), sqlx::Error> {
    let mut conn = auth.store.pool().acquire().await?;
    if conn.backend_name() == "SQLite" {
        query("UPDATE extension_runs SET status='interrupted' WHERE status='running'")
            .execute(&mut *conn)
            .await?;
    } else {
        // The current history table has no Core owner or heartbeat. A second Core
        // must not declare a live invocation interrupted on startup. The worker
        // deadline is bounded; old rows can be recovered after a conservative hour.
        query("UPDATE extension_runs SET status='interrupted' WHERE status='running' AND started_at<?")
            .bind(media_contracts::protocol::unix_now().saturating_sub(3600))
            .execute(&mut *conn)
            .await?;
    }
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
    let storage = crate::extension_storage::check_health(&auth.store).await;
    Ok(Json(
        json!({"apiVersion":1,"extensions":installed,"storage":storage}),
    ))
}
async fn storage_health(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<crate::extension_storage::PluginStorageHealth>, Error> {
    admin(&auth, &headers, false).await?;
    Ok(Json(
        crate::extension_storage::check_health(&auth.store).await,
    ))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallPackage {
    #[serde(flatten)]
    package: Package,
    public_key: Option<String>,
    #[serde(default)]
    upgrade: bool,
}
async fn install(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<InstallPackage>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let key = if let Some(key) = input.public_key {
        key
    } else if input.upgrade {
        query_scalar("SELECT p.public_key FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? AND p.active=1")
            .bind(&input.package.manifest.id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "package_public_key_required"))?
    } else {
        // Compatibility for old local imports; new imports supply their public key directly.
        query_scalar("SELECT public_key FROM extension_publishers WHERE id=? AND active=1")
            .bind(&input.package.publisher_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "package_public_key_required"))?
    };
    if input.upgrade {
        let ext_id = input.package.manifest.id.clone();
        upgrade_with_key(auth, headers, ext_id, input.package, key).await
    } else {
        install_with_key(auth, headers, input.package, key).await
    }
}

async fn upgrade(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<InstallPackage>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let key = if let Some(key) = input.public_key {
        key
    } else {
        query_scalar("SELECT p.public_key FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? AND p.active=1")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "package_public_key_required"))?
    };
    upgrade_with_key(auth, headers, id, input.package, key).await
}

async fn upgrade_with_key(
    auth: AuthState,
    headers: HeaderMap,
    id: String,
    package: Package,
    public_key: String,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    if package.manifest.id != id {
        return Err(err(StatusCode::BAD_REQUEST, "extension_id_mismatch"));
    }
    let (existing_package, trusted_key): (String, String) =
        query_as("SELECT e.package_json,p.public_key FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id AND p.active=1 WHERE e.id = ?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "extension_not_found"))?;

    let key = STANDARD
        .decode(&public_key)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_public_key"))?;
    validate_package(&package, &key).map_err(|code| err(StatusCode::BAD_REQUEST, code))?;

    let installed: Package = serde_json::from_str(&existing_package)
        .map_err(|_| err(StatusCode::CONFLICT, "installed_package_invalid"))?;
    if installed.publisher_id != package.publisher_id {
        return Err(err(StatusCode::FORBIDDEN, "publisher_mismatch"));
    }
    if STANDARD.decode(&trusted_key).ok().as_deref() != Some(key.as_slice()) {
        return Err(err(StatusCode::FORBIDDEN, "publisher_key_mismatch"));
    }
    let version = |value: &str| -> Option<Vec<u32>> {
        value.split('.').map(|part| part.parse().ok()).collect()
    };
    let installed_version = version(&installed.manifest.version)
        .filter(|v| v.len() == 3)
        .ok_or_else(|| err(StatusCode::CONFLICT, "installed_package_invalid"))?;
    let target_version = version(&package.manifest.version)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_manifest"))?;
    if target_version <= installed_version {
        return Err(err(StatusCode::CONFLICT, "extension_version_not_newer"));
    }
    if !repositories::compatible(&package.manifest) {
        return Err(err(StatusCode::BAD_REQUEST, "incompatible_server_version"));
    }
    validate_update_module(&package).map_err(|code| err(StatusCode::BAD_REQUEST, code))?;
    if package
        .manifest
        .permissions
        .iter()
        .any(|permission| !installed.manifest.permissions.contains(permission))
        || package
            .manifest
            .http_hosts
            .iter()
            .any(|host| !installed.manifest.http_hosts.contains(host))
    {
        return Err(err(
            StatusCode::CONFLICT,
            "extension_update_requires_authorization",
        ));
    }
    if package.manifest.config_schema != installed.manifest.config_schema {
        return Err(err(
            StatusCode::CONFLICT,
            "extension_settings_compatibility_unverified",
        ));
    }

    // Existing packages do not declare a private-data format, and in-flight
    // writes are not yet fenced atomically with the version switch. Do not
    // mutate the installed package until both guarantees can be established.
    Err(err(
        StatusCode::CONFLICT,
        "extension_update_compatibility_unverified",
    ))
}

// Validate the same bounded module envelope used by the worker before any
// prospective version switch; signature and digest have already been checked.
fn validate_update_module(package: &Package) -> Result<(), &'static str> {
    use wasmi::{Config, EnforcedLimits, Engine, FuncType, Module, ValType};
    let bytes = STANDARD
        .decode(&package.wasm)
        .map_err(|_| "invalid_module")?;
    let mut config = Config::default();
    config.consume_fuel(true).allow_start_fn(false);
    config.enforced_limits(EnforcedLimits::strict());
    config.set_max_stack_height(64 * 1024);
    config.set_max_recursion_depth(128);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &bytes).map_err(|_| "invalid_module")?;
    let host_signature = FuncType::new([ValType::I32, ValType::I32], [ValType::I64]);
    for import in module.imports() {
        let allowed = import.module() == "aether" && import.name() == "aether_host_http"
            || import.module() == "media"
                && matches!(
                    import.name(),
                    "media_host_storage_v1" | "media_host_objects_v1"
                );
        if !allowed || import.ty().func() != Some(&host_signature) {
            return Err("imports_forbidden");
        }
    }
    let alloc = FuncType::new([ValType::I32], [ValType::I32]);
    let run = FuncType::new([ValType::I32, ValType::I32], [ValType::I64]);
    if module
        .get_export("media_alloc")
        .and_then(|e| e.func().cloned())
        != Some(alloc)
        || module
            .get_export("media_run")
            .and_then(|e| e.func().cloned())
            != Some(run)
        || !module
            .get_export("memory")
            .and_then(|e| e.memory().copied())
            .is_some_and(|m| !m.is_64() && m.minimum() <= 256)
    {
        return Err("incompatible_abi");
    }
    Ok(())
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
        .map_err(storage_error)?;
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
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
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
    if input.enabled {
        crate::automation::emit_extension(&mut tx, &id, now())
            .await
            .map_err(db)?;
    }
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
    if !settings.is_object() {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_settings"));
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
    crate::automation::emit_extension(&mut tx, &id, now())
        .await
        .map_err(db)?;
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
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    if id == "avatars" {
        let packs: i64 =
            query_scalar("SELECT COUNT(*) FROM extension_avatar_packs WHERE extension_id=?")
                .bind(&id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if packs != 0 {
            return Err(err(StatusCode::CONFLICT, "avatar_migration_required"));
        }
    }
    crate::extension_data_lifecycle::uninstall(&mut tx, &id, input.delete_data)
        .await
        .map_err(db)?;
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
        .map_err(storage_error)?;
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
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) JOIN media_nodes n ON n.id=l.node_id WHERE e.id=? AND l.id=? AND e.generation=? AND e.enabled=1 AND p.active=1 AND l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))")
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
    let _slot = crate::extension_runtime::try_acquire_execution(
        &id,
        crate::extension_runtime::ExecutionClass::Interactive,
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

    if !settings.is_object() {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_settings"));
    }
    let mut settings = resolve_effective_extension_settings(&id, settings);
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
    if !simple_legacy {
        settings["language"] = json!(options.preferred_metadata_language);
    }
    if input.operation == "metadata.images" {
        image_collection::apply_language_settings(&input.input, &mut settings)?;
    }
    adapters::authorize_operation(&package.manifest, &input.operation)?;
    let mut payload =
        adapters::metadata_input(&context, &input.work_id, &input.operation, &input.input)?;
    if input.operation != "metadata.propose" {
        payload["language"] = settings["language"].clone();
    }
    payload = metadata_input_with_settings(payload, settings);
    let operation = input.operation.clone();
    let invocation = crate::extension_runtime::Invocation {
        wasm: crate::extension_storage::wasm(&auth.store, &package)
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
    invoke_worker_context(auth, headers, id, library, generation, invocation, None).await
}

#[allow(clippy::too_many_arguments)]
async fn invoke_worker_context(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
    invocation: crate::extension_runtime::Invocation,
    background: Option<&crate::task_engine::Job>,
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
    let user_id = if let Some(job) = background {
        job.owner.clone()
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
    let context = crate::extension_data::Context {
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
            if kind == "provider_retry" {
                let delay = frame["retryAfterSeconds"]
                    .as_i64()
                    .unwrap_or(60)
                    .clamp(1, 86400);
                if let Some(job) = background {
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
            let result = match kind {
                "storage" => {
                    let request = serde_json::from_value(frame["request"].clone())
                        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_storage_request"))?;
                    serde_json::to_value(
                        crate::extension_data::execute(&auth.store, &context, request, now())
                            .await
                            .map_err(media_extension_sdk::storage::StorageError::from),
                    )
                    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "storage_failed"))?
                }
                "objects" => {
                    let request = serde_json::from_value(frame["request"].clone())
                        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_object_request"))?;
                    serde_json::to_value(
                        crate::extension_objects::execute(&auth.store, &context, request, now())
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
        crate::extension_runtime::INVOCATION_TIMEOUT_SECS,
    ));
    tokio::pin!(deadline);
    let mut check = tokio::time::interval(Duration::from_millis(30));
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

// Background authority is the persisted initiating administrator, rechecked together
// with grants and the current fenced lease; it never fabricates a browser session.
async fn worker_permitted(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    library: &str,
    generation: &str,
    background: Option<&crate::task_engine::Job>,
) -> Result<bool, Error> {
    let Some(job) = background else {
        return permitted(auth, headers, id, library, generation).await;
    };
    let valid: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks j JOIN users u ON u.id=j.owner JOIN extensions e ON e.id=? JOIN extension_publishers p ON p.id=e.publisher_id JOIN libraries l ON l.id=j.library_id JOIN media_nodes n ON n.id=j.node_id WHERE j.id=? AND j.attempt=? AND j.status='running' AND j.lease_until>? AND u.role='administrator' AND u.disabled_at IS NULL AND NOT EXISTS(SELECT 1 FROM automation_policies policy WHERE policy.extension_id=e.id AND policy.library_id=l.id AND policy.mode='disabled') AND e.generation=? AND e.enabled=1 AND p.active=1 AND l.id=? AND l.node_id=j.node_id AND l.enabled=1 AND n.status!='revoked' AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))")
        .bind(id).bind(&job.id).bind(job.attempt).bind(now()).bind(generation).bind(library)
        .fetch_one(auth.store.pool()).await.map_err(db)?;
    Ok(valid == 1)
}

/// Durable invocation uses the same validation, language policy and isolated worker
/// as interactive calls. No disk path or SQL capability is sent to the guest.
type TargetResolution = (
    Option<String>,
    Option<adapters::WorkContext>,
    Option<(String, String, String, Option<f64>)>,
);

pub(crate) struct AutomatedResult {
    pub value: Value,
    pub marker_sources: Vec<(String, String)>,
}

pub(crate) async fn run_automated(
    auth: &AuthState,
    job: &crate::task_engine::Job,
    extension_id: &str,
    generation: &str,
    operation: &str,
) -> Result<AutomatedResult, Error> {
    let headers = HeaderMap::new();
    if !worker_permitted(
        auth,
        &headers,
        extension_id,
        &job.library_id,
        generation,
        Some(job),
    )
    .await?
    {
        return Err(err(StatusCode::FORBIDDEN, "extension_revoked"));
    }
    let (work_id, context, music_track): TargetResolution = {
        let work_id: Option<String> = query_scalar("SELECT e.work_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN libraries l ON l.id=a.library_id WHERE a.id=? AND a.library_id=? AND a.available=1 AND s.root_snapshot=l.root")
            .bind(&job.target_id).bind(&job.library_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
        if let Some(work_id) = work_id {
            let context = query_as("SELECT library_id,title,year,kind,external_ids_json,editorial_json,manual_json FROM inventory_works WHERE id=?")
                .bind(&work_id).fetch_one(auth.store.pool()).await.map_err(db)?;
            (Some(work_id), Some(context), None)
        } else {
            let track: Option<(String, String, String, Option<f64>)> = query_as("SELECT title, artist, album, duration FROM music_tracks WHERE id=? AND library_id=? AND available=1")
                .bind(&job.target_id).bind(&job.library_id).fetch_optional(auth.store.pool()).await.map_err(db)?;
            let track =
                track.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_asset_or_track"))?;
            (None, None, Some(track))
        }
    };
    let raw: String =
        query_scalar("SELECT package_json FROM extensions WHERE id=? AND generation=?")
            .bind(extension_id)
            .bind(generation)
            .fetch_one(auth.store.pool())
            .await
            .map_err(db)?;
    let package: Package =
        serde_json::from_str(&raw).map_err(|_| err(StatusCode::CONFLICT, "invalid_package"))?;
    adapters::authorize_operation(&package.manifest, operation)?;
    let raw: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(extension_id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = raw
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_settings"))?
        .unwrap_or_else(|| json!({}));
    if !settings.is_object() {
        return Err(err(StatusCode::CONFLICT, "invalid_settings"));
    }
    let mut settings = resolve_effective_extension_settings(extension_id, settings);
    let options: String = query_scalar("SELECT options_json FROM libraries WHERE id=?")
        .bind(&job.library_id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    let options: crate::library_options::LibraryOptions = serde_json::from_str(&options)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_library_options"))?;
    let legacy = operation == "metadata.propose"
        && package
            .manifest
            .config_schema
            .as_ref()
            .is_none_or(Vec::is_empty)
        && settings.as_object().is_some_and(|v| v.is_empty());
    if !legacy {
        settings["language"] = json!(options.preferred_metadata_language);
    }
    let mut marker_sources = Vec::new();
    let payload = if let Some((title, artist, album, duration)) = &music_track {
        if operation == "lyrics.get" {
            lyrics_request_input(title, artist, album, *duration)
        } else if operation == "metadata.search" {
            music_album_search_input(title, artist, album).ok_or_else(|| {
                err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "music_identity_incomplete",
                )
            })?
        } else {
            json!({
                "title": title,
                "artist": artist,
                "album": album,
                "duration": duration,
                "mediaType": "music",
            })
        }
    } else if operation == "intro.detect" {
        let context = context
            .as_ref()
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"))?;
        let work_id = work_id
            .as_ref()
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "work_required"))?;
        if context.kind != "episode" {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"));
        }
        let scope: (String, String, i64) = query_as("SELECT ep.season_id,se.series_id,se.season_number FROM inventory_episodes ep JOIN inventory_seasons se ON se.id=ep.season_id WHERE ep.work_id=?")
            .bind(work_id).fetch_one(auth.store.pool()).await.map_err(db)?;
        let candidates: Vec<(String,String,String,String,String,String,String)> = query_as("SELECT a.id,t.result_json,t.idempotency_key,obs.fingerprint,a.probe_json,ep.work_id,CAST(ep.episode_number AS TEXT) FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=a.library_id JOIN automation_observations obs ON obs.asset_id=a.id JOIN durable_tasks t ON t.target_id=a.id AND t.category='analyze' AND t.component='fingerprint' AND t.status='completed' WHERE ep.season_id=? AND a.library_id=? AND a.available=1 AND s.root_snapshot=l.root ORDER BY CASE WHEN a.id=? THEN 0 ELSE 1 END,t.finished_at DESC,a.id LIMIT 12")
            .bind(&scope.0).bind(&job.library_id).bind(&job.target_id).fetch_all(auth.store.pool()).await.map_err(db)?;
        let mut episodes = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (asset, raw, key, source, probe, episode_work, number) in candidates {
            if episodes.len() >= 8 {
                break;
            }
            if seen.contains(&episode_work) {
                continue;
            }
            let expected = format!(
                "fp:{}",
                crate::automation::hash(&(&asset, &source, "chromaprint-v1")).map_err(db)?
            );
            if key != expected {
                continue;
            }
            let Ok(result) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(words) = result["fingerprints"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 6000)
            else {
                continue;
            };
            let mut bytes = Vec::with_capacity(words.len() * 4);
            let mut valid = true;
            for word in words {
                if let Some(n) = word.as_u64().filter(|n| *n <= u32::MAX as u64) {
                    bytes.extend_from_slice(&(n as u32).to_le_bytes());
                } else {
                    valid = false;
                    break;
                }
            }
            if !valid {
                continue;
            }

            let mut tail_compact = None;
            if let Some(tail_words) = result["tailFingerprints"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 4000)
            {
                let mut tail_bytes = Vec::with_capacity(tail_words.len() * 4);
                let mut tail_valid = true;
                for word in tail_words {
                    if let Some(n) = word.as_u64().filter(|n| *n <= u32::MAX as u64) {
                        tail_bytes.extend_from_slice(&(n as u32).to_le_bytes());
                    } else {
                        tail_valid = false;
                        break;
                    }
                }
                if tail_valid && !tail_bytes.is_empty() {
                    tail_compact = Some(STANDARD.encode(tail_bytes));
                }
            }

            let Ok(probe) = serde_json::from_str::<Value>(&probe) else {
                continue;
            };
            let Some(duration) = probe["format"]["duration"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n > 0.0)
            else {
                continue;
            };
            seen.insert(episode_work);
            marker_sources.push((asset.clone(), source));
            let mut ep_val = json!({
                "id": asset,
                "seriesId": scope.1,
                "seasonNumber": scope.2,
                "episodeNumber": number.parse::<u32>().ok(),
                "duration": duration,
                "fingerprintsCompact": STANDARD.encode(bytes),
                "fingerprintRate": result["fingerprintRate"]
            });
            if let Some(compact) = tail_compact {
                ep_val["tailFingerprintsCompact"] = json!(compact);
            }
            if let Some(ts) = result["tailStart"].as_f64() {
                ep_val["tailStart"] = json!(ts);
            }
            episodes.push(ep_val);
        }
        if episodes.len() < 2 || episodes[0]["id"] != job.target_id {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "compatible_fingerprint_peer_required",
            ));
        }
        json!({"episodes":episodes})
    } else if operation == "intro.parse_chapters" {
        let context = context
            .as_ref()
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"))?;
        if context.kind != "episode" {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "episode_required"));
        }
        let raw: String = query_scalar(
            "SELECT probe_json FROM inventory_assets WHERE id=? AND library_id=? AND available=1",
        )
        .bind(&job.target_id)
        .bind(&job.library_id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
        let probe: Value = serde_json::from_str(&raw)
            .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_probe"))?;
        let duration = probe["format"]["duration"]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "duration_required"))?;
        let chapters = probe["chapters"].as_array().cloned().unwrap_or_default();
        if chapters.len() > 1000 {
            return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "too_many_chapters"));
        }
        let chapters: Vec<Value> = chapters
            .iter()
            .filter_map(|chapter| {
                let start = chapter["start_time"].as_str()?.parse::<f64>().ok()?;
                let title = chapter["tags"]["title"].as_str()?;
                (start.is_finite() && start >= 0.0 && title.len() <= 128)
                    .then(|| json!({"start":start,"title":title}))
            })
            .collect();
        json!({"episodeId":work_id,"duration":duration,"chapters":chapters})
    } else {
        let context = context
            .as_ref()
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "work_context_required"))?;
        let work_id = work_id
            .as_ref()
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "work_id_required"))?;
        let mut value = adapters::metadata_input(context, work_id, operation, &json!({}))?;
        if operation != "metadata.propose" {
            value["language"] = settings["language"].clone();
        }
        value
    };
    let invocation = crate::extension_runtime::Invocation {
        wasm: crate::extension_storage::wasm(&auth.store, &package)
            .await
            .map_err(storage_error)?,
        input: ExtensionRequest {
            api_version: 1,
            operation: operation.into(),
            input: metadata_input_with_settings(payload.clone(), settings),
        },
        allowed_hosts: package.manifest.http_hosts.clone(),
        auth_headers: Default::default(),
    };
    let cache_payload = provider_cache_context(
        &payload,
        &invocation.input.input["config"],
        generation,
        &package.sha256,
        music_track.is_some() && operation == "metadata.search",
    )
    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "cache_context_invalid"))?;
    let now = media_contracts::protocol::unix_now();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    if let Some(cached) = crate::automation::get_cached_provider_response(
        &mut tx,
        extension_id,
        operation,
        &cache_payload,
        now,
    )
    .await
    .map_err(db)?
    {
        if operation == "lyrics.get" && !lyrics_result_matches_input(&payload, &cached) {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_lyrics_origin",
            ));
        }
        if operation == "intro.detect" && !adapters::marker_result_matches_input(&payload, &cached)
        {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_marker_origin",
            ));
        }
        return Ok(AutomatedResult {
            value: cached,
            marker_sources,
        });
    }
    let result = invoke_worker_context(
        auth,
        &headers,
        extension_id,
        &job.library_id,
        generation,
        invocation.clone(),
        Some(job),
    )
    .await?;
    if operation == "lyrics.get" && !lyrics_result_matches_input(&payload, &result) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_lyrics_origin",
        ));
    }
    if operation == "intro.detect" && !adapters::marker_result_matches_input(&payload, &result) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_marker_origin",
        ));
    }
    if music_track.is_some()
        && operation == "metadata.search"
        && let Some(items) = result.as_array()
        && let Some(candidate) = trusted_music_candidate(items, &invocation.input.input)
    {
        let mut details = invocation.clone();
        details.input.operation = "metadata.details".into();
        details.input.input["id"] = candidate["id"].clone();
        let enriched = invoke_worker_context(
            auth,
            &headers,
            extension_id,
            &job.library_id,
            generation,
            details,
            Some(job),
        )
        .await?;
        if music_details_match(&enriched, candidate) {
            let _ = crate::automation::put_cached_provider_response(
                &mut tx,
                extension_id,
                operation,
                &cache_payload,
                &enriched,
                86400,
                now,
            )
            .await;
            let _ = crate::database::commit(tx).await;
            return Ok(AutomatedResult {
                value: enriched,
                marker_sources,
            });
        }
    }
    // Only an unambiguous title/type/year match may reach automatic details.
    if music_track.is_none()
        && operation == "metadata.search"
        && let Some(items) = result.as_array()
        && let Some(candidate) = trusted_metadata_candidate(items, &invocation.input.input)
    {
        let mut details = invocation;
        details.input.operation = "metadata.details".into();
        details.input.input["id"] = candidate["id"].clone();
        let enriched = invoke_worker_context(
            auth,
            &headers,
            extension_id,
            &job.library_id,
            generation,
            details,
            Some(job),
        )
        .await?;
        let id_matches = enriched["providerId"]
            .as_str()
            .is_some_and(|id| id == candidate["id"].as_str().unwrap_or(""));
        let title_matches = enriched["title"].as_str().is_none_or(|title| {
            normalize_title(title) == normalize_title(candidate["title"].as_str().unwrap_or(""))
        });
        let year_matches = invocation_year_matches(&enriched, candidate);
        if id_matches && title_matches && year_matches {
            let _ = crate::automation::put_cached_provider_response(
                &mut tx,
                extension_id,
                operation,
                &cache_payload,
                &enriched,
                86400,
                now,
            )
            .await;
            let _ = crate::database::commit(tx).await;
            return Ok(AutomatedResult {
                value: enriched,
                marker_sources,
            });
        }
    }
    let _ = crate::automation::put_cached_provider_response(
        &mut tx,
        extension_id,
        operation,
        &cache_payload,
        &result,
        86400,
        now,
    )
    .await;
    let _ = crate::database::commit(tx).await;
    Ok(AutomatedResult {
        value: result,
        marker_sources,
    })
}

fn lyrics_result_matches_input(input: &Value, result: &Value) -> bool {
    let Some(track) = input["track"]
        .as_str()
        .map(normalize_title)
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    let Some(artist) = input["artist"]
        .as_str()
        .map(normalize_title)
        .filter(|s| !s.is_empty() && s != "unknown artist")
    else {
        return false;
    };
    if result["trackName"]
        .as_str()
        .is_none_or(|s| normalize_title(s) != track)
        || result["artistName"]
            .as_str()
            .is_none_or(|s| normalize_title(s) != artist)
    {
        return false;
    }
    match (
        input["durationSecs"].as_f64(),
        result["durationSecs"].as_f64(),
    ) {
        (Some(a), Some(b)) => {
            a > 0.0 && b > 0.0 && ((a - b).abs() <= 5.0 || (a - b).abs() / a <= 0.05)
        }
        _ => true,
    }
}

fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn invocation_year_matches(details: &Value, candidate: &Value) -> bool {
    details["year"].as_i64().is_none_or(|year| {
        candidate["year"]
            .as_i64()
            .is_none_or(|expected| year == expected)
    })
}
fn trusted_metadata_candidate<'a>(items: &'a [Value], input: &Value) -> Option<&'a Value> {
    let title = normalize_title(input["title"].as_str()?);
    let media_type = input["mediaType"].as_str()?;
    let year = input["year"].as_i64();
    let mut matches = items.iter().filter(|item| {
        item["title"]
            .as_str()
            .is_some_and(|name| normalize_title(name) == title)
            && item["mediaType"].as_str() == Some(media_type)
            && year.is_none_or(|expected| item["year"].as_i64() == Some(expected))
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

pub fn trusted_music_candidate<'a>(items: &'a [Value], input: &Value) -> Option<&'a Value> {
    let title = normalize_title(input["title"].as_str()?);
    let artist = normalize_title(input["artist"].as_str()?);
    if title.is_empty() || artist.is_empty() || artist == "unknown artist" {
        return None;
    }
    let duration = input["duration"].as_f64();
    let mut matches = items.iter().filter(|item| {
        let id_ok = item["id"]
            .as_str()
            .is_some_and(|id| !id.trim().is_empty() && id.len() <= 256);
        let title_ok = item["title"]
            .as_str()
            .is_some_and(|name| normalize_title(name) == title);
        let artist_ok = item["artist"]
            .as_str()
            .is_some_and(|name| normalize_title(name) == artist);
        let duration_ok = match (
            duration,
            item["duration"]
                .as_f64()
                .or_else(|| item["length"].as_f64().map(|ms| ms / 1000.0)),
        ) {
            (Some(a), Some(b)) => {
                a.is_finite()
                    && b.is_finite()
                    && a > 0.0
                    && b > 0.0
                    && ((a - b).abs() <= 5.0 || (a - b).abs() / a <= 0.05)
            }
            _ => true,
        };
        id_ok && item["mediaType"].as_str() == Some("music") && title_ok && artist_ok && duration_ok
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn music_details_match(details: &Value, candidate: &Value) -> bool {
    let id_matches = candidate["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .is_some_and(|id| details["providerId"].as_str() == Some(id));
    let title_matches = details["title"].as_str().is_some_and(|title| {
        candidate["title"]
            .as_str()
            .is_some_and(|expected| normalize_title(title) == normalize_title(expected))
    });
    let artists = details["actors"].as_array().map(|credits| {
        credits
            .iter()
            .filter(|credit| credit["role"].as_str() == Some("artist"))
            .filter_map(|credit| credit["name"].as_str())
            .collect::<Vec<_>>()
            .join(", ")
    });
    let artist_matches = candidate["artist"]
        .as_str()
        .zip(artists.as_deref())
        .is_some_and(|(expected, actual)| {
            !actual.is_empty() && normalize_title(expected) == normalize_title(actual)
        });
    id_matches && title_matches && artist_matches
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use media_extension_sdk::MetadataInput;
    #[test]
    fn music_album_search_uses_album_and_explicit_artist() {
        let input = music_album_search_input("Track", "Artist", "Album").unwrap();
        assert_eq!(input["title"], "Album");
        assert_eq!(input["trackTitle"], "Track");
        assert_eq!(input["externalIds"]["artist"], "Artist");
        assert!(music_album_search_input("Track", "Artist", "Unknown Album").is_none());
        assert!(music_album_search_input("Track", "", "Album").is_none());
    }

    #[test]
    fn provider_cache_depends_on_settings_generation_and_package_without_disclosing_secrets() {
        let payload = json!({"title":"Song"});
        let settings = json!({"api_key":"example-secret", "language":"fr"});
        let original =
            provider_cache_context(&payload, &settings, "gen1", "package1", true).unwrap();
        assert_eq!(
            original,
            provider_cache_context(&payload, &settings, "gen1", "package1", true).unwrap()
        );
        assert!(!original.to_string().contains("example-secret"));
        assert_ne!(
            original,
            provider_cache_context(
                &payload,
                &json!({"api_key":"different"}),
                "gen1",
                "package1",
                true
            )
            .unwrap()
        );
        assert_ne!(
            original,
            provider_cache_context(&payload, &settings, "gen2", "package1", true).unwrap()
        );
        assert_ne!(
            original,
            provider_cache_context(&payload, &settings, "gen1", "package2", true).unwrap()
        );
        assert_ne!(
            original,
            provider_cache_context(&payload, &settings, "gen1", "package1", false).unwrap()
        );
    }

    #[test]
    fn music_candidates_and_details_require_consistent_identity() {
        let input = json!({"title":"Song", "artist":"Muse", "duration":180});
        let candidate = json!({"id":"42", "title":"Song", "artist":"Muse", "duration":182, "mediaType":"music"});
        assert!(trusted_music_candidate(std::slice::from_ref(&candidate), &input).is_some());
        for invalid in [
            json!({"id":"42", "title":"Song", "artist":"Museum", "duration":180, "mediaType":"music"}),
            json!({"id":"42", "title":"Song", "artist":"", "duration":180, "mediaType":"music"}),
            json!({"id":"", "title":"Song", "artist":"Muse", "duration":180, "mediaType":"music"}),
            json!({"id":"42", "title":"Song", "artist":"Muse", "duration":240, "mediaType":"music"}),
            json!({"id":"42", "title":"Song", "artist":"Muse", "duration":180, "mediaType":"movie"}),
        ] {
            assert!(trusted_music_candidate(&[invalid], &input).is_none());
        }
        assert!(trusted_music_candidate(&[candidate.clone(), candidate.clone()], &input).is_none());
        assert!(
            trusted_music_candidate(
                std::slice::from_ref(&candidate),
                &json!({"title":"Song", "artist":""})
            )
            .is_none()
        );
        assert!(music_details_match(
            &json!({"providerId":"42", "title":"Song", "actors":[{"name":"Muse", "role":"artist"}]}),
            &candidate
        ));
        assert!(!music_details_match(
            &json!({"providerId":"43", "title":"Song", "actors":[{"name":"Muse", "role":"artist"}]}),
            &candidate
        ));
        assert!(!music_details_match(
            &json!({"providerId":"42", "title":"Other", "actors":[{"name":"Muse", "role":"artist"}]}),
            &candidate
        ));
        assert!(!music_details_match(
            &json!({"providerId":"42", "title":"Song", "actors":[{"name":"Other", "role":"artist"}]}),
            &candidate
        ));
        assert!(!music_details_match(
            &json!({"providerId":"42", "title":"Song"}),
            &candidate
        ));
    }

    #[test]
    fn lyrics_match_track_artist_and_duration() {
        let input = lyrics_request_input("Song", "Artist", "Album", Some(180.0));
        let mut result = json!({"id":42,"trackName":"Song","artistName":"Artist","durationSecs":181,"instrumental":false});
        assert!(lyrics_result_matches_input(&input, &result));
        result["artistName"] = json!("Different");
        assert!(!lyrics_result_matches_input(&input, &result));
        result["artistName"] = json!("Artist");
        result["durationSecs"] = json!(300);
        assert!(!lyrics_result_matches_input(&input, &result));
    }

    #[test]
    fn lyrics_payload_matches_sdk_request() {
        let input = lyrics_request_input("Track", "Artist", "Album", Some(123.6));
        let decoded: media_extension_sdk::LyricsRequest = serde_json::from_value(input).unwrap();
        assert_eq!(decoded.track, "Track");
        assert_eq!(decoded.artist, "Artist");
        assert_eq!(decoded.album.as_deref(), Some("Album"));
        assert_eq!(decoded.duration_secs, Some(124));
    }

    #[test]
    fn automatic_candidate_requires_unique_title_type_and_year() {
        let input = json!({"title":"  Film  Test ","mediaType":"movie","year":2024});
        let candidate = json!({"id":"42","title":"Film Test","mediaType":"movie","year":2024});
        assert_eq!(
            trusted_metadata_candidate(std::slice::from_ref(&candidate), &input).unwrap()["id"],
            "42"
        );
        assert!(
            trusted_metadata_candidate(&[candidate.clone(), candidate.clone()], &input).is_none()
        );
        for other in [
            json!({"id":"42","title":"Film Test","mediaType":"tv_show","year":2024}),
            json!({"id":"42","title":"Film Test","mediaType":"movie","year":2023}),
            json!({"id":"42","title":"Different","mediaType":"movie","year":2024}),
        ] {
            assert!(trusted_metadata_candidate(&[other], &input).is_none());
        }
    }
    #[test]
    fn historical_empty_endpoint_keeps_provider_default_and_credentials() {
        let value = metadata_input_with_settings(
            json!({"title":"Fixture"}),
            json!({"api_key":"synthetic","base_url":"  "}),
        );
        assert!(value["config"].get("base_url").is_none());
        assert_eq!(value["config"]["api_key"], "synthetic");
        let value = metadata_input_with_settings(
            json!({}),
            json!({"base_url":"https://custom.example/api"}),
        );
        assert_eq!(value["config"]["base_url"], "https://custom.example/api");
    }

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
