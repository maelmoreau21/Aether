use super::*;
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use base64::engine::general_purpose::STANDARD;
use media_extension_sdk::{Manifest, Package};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(super) async fn admin(
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

pub(super) async fn audit(
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
pub(super) struct Publisher {
    pub(super) id: String,
    pub(super) public_key: String,
}

pub(super) async fn trust(
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

pub(super) async fn publishers(
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

pub(super) async fn revoke_publisher(
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
pub(super) struct Installed {
    pub(super) manifest: Manifest,
    pub(super) publisher_id: String,
    pub(super) sha256: String,
    pub(super) enabled: bool,
    pub(super) library_ids: Vec<String>,
    pub(super) all_libraries: bool,
    pub(super) settings: Value,
    pub(super) rollback_version: Option<String>,
}

pub(super) async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
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
        let rollback_package: Option<String> = query_scalar(
            "SELECT package_json FROM extension_update_revisions WHERE extension_id=?",
        )
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
        let rollback_version = rollback_package
            .and_then(|bytes| serde_json::from_str::<Package>(&bytes).ok())
            .map(|previous| previous.manifest.version);
        installed.push(Installed {
            manifest: package.manifest,
            publisher_id: package.publisher_id,
            sha256: package.sha256,
            enabled: enabled == 1,
            library_ids: grants,
            all_libraries: all_libraries == 1,
            settings,
            rollback_version,
        });
    }
    let storage = crate::extensions::storage::check_health(&auth.store).await;
    Ok(Json(
        json!({"apiVersion":1,"extensions":installed,"storage":storage}),
    ))
}

pub(super) async fn storage_health(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<crate::extensions::storage::PluginStorageHealth>, Error> {
    admin(&auth, &headers, false).await?;
    Ok(Json(
        crate::extensions::storage::check_health(&auth.store).await,
    ))
}
