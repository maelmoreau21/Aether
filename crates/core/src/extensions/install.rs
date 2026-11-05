use super::*;
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use base64::engine::general_purpose::STANDARD;
use media_extension_sdk::Package;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstallPackage {
    #[serde(flatten)]
    pub(super) package: Package,
    pub(super) public_key: Option<String>,
    #[serde(default)]
    pub(super) upgrade: bool,
}

pub(super) async fn install(
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

pub(super) async fn upgrade(
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

pub(super) async fn upgrade_with_key(
    auth: AuthState,
    headers: HeaderMap,
    id: String,
    package: Package,
    public_key: String,
) -> Result<(StatusCode, Json<Value>), Error> {
    change_version_with_key(auth, headers, id, package, public_key, None).await
}

pub(super) struct DowngradeGuard {
    pub expected_version: String,
    pub expected_sha256: String,
}

pub(super) async fn change_version_with_key(
    auth: AuthState,
    headers: HeaderMap,
    id: String,
    package: Package,
    public_key: String,
    downgrade: Option<DowngradeGuard>,
) -> Result<(StatusCode, Json<Value>), Error> {
    let actor = admin(&auth, &headers, true).await?;
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
    if let Some(ref guard) = downgrade {
        if installed.manifest.version != guard.expected_version
            || installed.sha256 != guard.expected_sha256
        {
            return Err(err(StatusCode::CONFLICT, "extension_changed"));
        }
        if target_version >= installed_version {
            return Err(err(StatusCode::CONFLICT, "extension_version_not_older"));
        }
        if installed
            .manifest
            .permissions
            .iter()
            .any(|p| p == "storage.private")
            || package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "storage.private")
            || package.manifest.storage_migration.is_some()
            || serde_json::to_value(&installed.manifest.config_schema).ok()
                != serde_json::to_value(&package.manifest.config_schema).ok()
        {
            return Err(err(
                StatusCode::CONFLICT,
                "extension_downgrade_incompatible",
            ));
        }
    } else if target_version <= installed_version {
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
    let private_rows: i64 = query_scalar("SELECT (SELECT COUNT(*) FROM extension_data WHERE extension_id=?) + (SELECT COUNT(*) FROM extension_objects WHERE extension_id=?)")
        .bind(&id).bind(&id).fetch_one(auth.store.pool()).await.map_err(db)?;
    if downgrade.is_some() && private_rows != 0 {
        return Err(err(
            StatusCode::CONFLICT,
            "extension_downgrade_incompatible",
        ));
    }
    let needs_migration = private_rows != 0
        || installed
            .manifest
            .permissions
            .iter()
            .any(|p| p == "storage.private");
    if needs_migration
        && package
            .manifest
            .storage_migration
            .as_ref()
            .is_none_or(|m| m.from_version != installed.manifest.version)
    {
        let code = if package.manifest.config_schema != installed.manifest.config_schema {
            "extension_settings_compatibility_unverified"
        } else {
            "extension_update_compatibility_unverified"
        };
        return Err(err(StatusCode::CONFLICT, code));
    }
    if package
        .manifest
        .storage_migration
        .as_ref()
        .is_some_and(|m| m.from_version != installed.manifest.version)
    {
        return Err(err(
            StatusCode::CONFLICT,
            "extension_migration_source_mismatch",
        ));
    }
    // Optional additive fields and presentation changes preserve saved values.
    // Incompatible value types or newly required values still require migration.
    let old = installed
        .manifest
        .config_schema
        .as_deref()
        .unwrap_or_default();
    let new = package
        .manifest
        .config_schema
        .as_deref()
        .unwrap_or_default();
    let field_kind = |kind: &str| match kind {
        "boolean" => 0,
        "number" => 1,
        _ => 2,
    };
    if new
        .iter()
        .any(|f| match old.iter().find(|o| o.key == f.key) {
            Some(o) => {
                field_kind(&o.field_type) != field_kind(&f.field_type)
                    || (f.required && !o.required && f.default_value.is_none())
            }
            None => f.required && f.default_value.is_none(),
        })
    {
        return Err(err(
            StatusCode::CONFLICT,
            "extension_settings_compatibility_unverified",
        ));
    }
    if package.manifest.storage_migration.is_some() {
        updates::verify_candidate(&package).await?;
    }
    crate::extensions::storage::backup_package(&auth.store, &installed)
        .await
        .map_err(storage_error)?;
    let bytes = serde_json::to_string(&package)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_package"))?;
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    // Repeat the private-data check under the write lock: a worker may have written
    // after the earlier read. Downgrades never reinterpret private data.
    if downgrade.is_some() {
        let snapshots: i64 =
            query_scalar("SELECT COUNT(*) FROM extension_update_revisions WHERE extension_id=?")
                .bind(&id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if snapshots != 0 {
            return Err(err(
                StatusCode::CONFLICT,
                "extension_downgrade_snapshot_exists",
            ));
        }
        let rows: i64 = query_scalar("SELECT (SELECT COUNT(*) FROM extension_data WHERE extension_id=?) + (SELECT COUNT(*) FROM extension_objects WHERE extension_id=?)")
            .bind(&id).bind(&id).fetch_one(&mut *tx).await.map_err(db)?;
        if rows != 0 {
            return Err(err(
                StatusCode::CONFLICT,
                "extension_downgrade_incompatible",
            ));
        }
    }
    // The database write lock fences storage operations from old generations.
    // Snapshot and all declarative migration operations share the package commit.
    updates::snapshot(&mut tx, &id, &existing_package, &package.sha256).await?;
    updates::migrate(&mut tx, &id, package.manifest.storage_migration.as_ref()).await?;
    let changed =
        query("UPDATE extensions SET package_json=?,generation=?,enabled=CASE WHEN ?=1 THEN 0 ELSE enabled END WHERE id=? AND package_json=?")
            .bind(bytes)
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(i64::from(downgrade.is_some()))
            .bind(&id)
            .bind(existing_package)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    if changed.rows_affected() != 1 {
        return Err(err(StatusCode::CONFLICT, "extension_changed"));
    }
    audit(
        &mut tx,
        &actor.user.id,
        &id,
        if downgrade.is_some() {
            "package.downgraded"
        } else {
            "package.upgraded"
        },
    )
    .await?;
    tx.commit().await.map_err(db)?;
    if let Err(error) = crate::extensions::storage::persist_upgrade(&auth.store, &package).await {
        eprintln!("Plugin '{id}' cache synchronization pending: {error}");
    }
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}

pub(super) async fn install_with_key(
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
    let _ = crate::automation::emit_extension(
        &mut tx,
        &package.manifest.id,
        media_contracts::protocol::unix_now(),
    )
    .await;
    audit(
        &mut tx,
        &actor.user.id,
        &package.manifest.id,
        &format!("installed:{}:{}", package.manifest.version, package.sha256),
    )
    .await?;
    crate::extensions::storage::persist(&auth.store, &package)
        .await
        .map_err(storage_error)?;
    tx.commit().await.map_err(db)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": package.manifest.id})),
    ))
}

pub async fn install_internal_with_key(
    store: &crate::identity::IdentityStore,
    package: Package,
    public_key: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let key = STANDARD.decode(public_key)?;
    validate_package(&package, &key).map_err(|e| format!("Validation error: {e}"))?;
    let key_id = format!("key.{:x}", Sha256::digest(&key));
    let bytes = serde_json::to_string(&package)?;
    let mut tx = store.pool().begin().await?;
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES(?,?,1) ON CONFLICT(id) DO UPDATE SET active=1")
        .bind(&key_id).bind(public_key).execute(&mut *tx).await?;
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation,all_libraries) VALUES(?,?,?,1,?,1) ON CONFLICT(id) DO NOTHING")
        .bind(&package.manifest.id)
        .bind(&key_id)
        .bind(bytes)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&mut *tx)
        .await?;
    let _ = crate::automation::emit_extension(
        &mut tx,
        &package.manifest.id,
        media_contracts::protocol::unix_now(),
    )
    .await;
    crate::extensions::storage::persist(store, &package).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn bootstrap_essential_plugins(store: &crate::identity::IdentityStore) {
    // The seeded official catalogue follows the Core's channel once. An explicit
    // custom repository or a later administrator override is preserved.
    if media_contracts::version::current_version_info().channel != "stable" {
        let _ = query("UPDATE extension_repositories SET url=?,name='Aether Plugins (develop)' WHERE id='community-default' AND name='Aether Plugins' AND url=?")
            .bind("https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/develop/repository/manifest.json")
            .bind("https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json")
            .execute(store.pool()).await;
    }

    const PUB_KEY: &str = include_str!("../../embedded_plugins/publisher-public.txt");
    for (id, bytes) in [
        (
            "tmdb",
            include_str!("../../embedded_plugins/tmdb-0.1.1.package.json"),
        ),
        (
            "imdb",
            include_str!("../../embedded_plugins/imdb-0.1.1.package.json"),
        ),
    ] {
        let exists: bool =
            match query_scalar::<sqlx::Any, i64>("SELECT COUNT(*) FROM extensions WHERE id=?")
                .bind(id)
                .fetch_one(store.pool())
                .await
            {
                Ok(count) => count > 0_i64,
                Err(error) => {
                    eprintln!("Could not check bundled extension {id}: {error}");
                    continue;
                }
            };
        // Bootstrap defaults only. A restart must preserve the administrator's
        // activation and library grants, including an intentionally empty scope.
        if exists {
            continue;
        }
        let package = match serde_json::from_str::<Package>(bytes) {
            Ok(package) => package,
            Err(error) => {
                eprintln!("Invalid bundled extension package {id}: {error}");
                continue;
            }
        };
        if let Err(error) = install_internal_with_key(store, package, PUB_KEY.trim()).await {
            eprintln!("Could not install bundled extension {id}: {error}");
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Removal {
    pub(super) delete_data: bool,
}

pub(super) async fn uninstall(
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
    updates::clear_snapshot(&mut tx, &id).await.map_err(db)?;
    crate::extensions::data_lifecycle::uninstall(&mut tx, &id, input.delete_data)
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
    crate::extensions::storage::remove(&auth.store, &id)
        .await
        .map_err(storage_error)?;
    Ok(StatusCode::NO_CONTENT)
}
