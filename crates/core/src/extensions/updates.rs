//! One bounded, durable rollback snapshot per installed extension. Snapshots
//! never leave the database and contain secrets; they are not API responses.
use super::*;
// Dynamic identifiers below come exclusively from fixed local arrays.
use sqlx::AnyConnection;

pub(super) async fn clear_snapshot(conn: &mut AnyConnection, id: &str) -> Result<(), sqlx::Error> {
    query("DELETE FROM extension_update_chunks WHERE object_id IN (SELECT id FROM extension_update_objects WHERE extension_id=?)")
        .bind(id).execute(&mut *conn).await?;
    for table in ["data", "objects", "namespaces", "grants"] {
        query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM extension_update_{table} WHERE extension_id=?"
        )))
        .bind(id)
        .execute(&mut *conn)
        .await?;
    }
    query("DELETE FROM extension_update_revisions WHERE extension_id=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub(super) async fn snapshot(
    conn: &mut AnyConnection,
    id: &str,
    previous: &str,
    target: &str,
) -> Result<(), Error> {
    // Retained data can belong to an older publisher after uninstall/reinstall.
    // A code update must not mutate or snapshot that inaccessible namespace.
    let mismatched: i64 = query_scalar("SELECT COUNT(*) FROM extension_data_namespaces n JOIN extensions e ON e.id=n.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE n.extension_id=? AND (n.publisher_id!=e.publisher_id OR n.publisher_key!=p.public_key)")
        .bind(id).fetch_one(&mut *conn).await.map_err(db)?;
    if mismatched != 0 {
        return Err(err(
            StatusCode::CONFLICT,
            "extension_namespace_identity_mismatch",
        ));
    }
    clear_snapshot(conn, id).await.map_err(db)?;
    query("INSERT INTO extension_update_revisions(extension_id,package_json,settings_json,enabled,all_libraries,target_sha256) SELECT e.id,?,COALESCE(s.settings_json,'{}'),e.enabled,e.all_libraries,? \
            FROM extensions e \
            LEFT \
            JOIN extension_settings s ON s.extension_id=e.id \
            WHERE e.id=? AND e.package_json=?")
        .bind(previous).bind(target).bind(id).bind(previous).execute(&mut *conn).await.map_err(db)?;
    for (snapshot, source) in [
        ("data", "extension_data"),
        ("objects", "extension_objects"),
        ("namespaces", "extension_data_namespaces"),
        ("grants", "extension_grants"),
    ] {
        query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO extension_update_{snapshot} SELECT * FROM {source} WHERE extension_id=?"
        )))
        .bind(id)
        .execute(&mut *conn)
        .await
        .map_err(db)?;
    }
    query("INSERT INTO extension_update_chunks SELECT c.* FROM extension_object_chunks c JOIN extension_objects o ON o.id=c.object_id WHERE o.extension_id=?")
        .bind(id).execute(&mut *conn).await.map_err(db)?;
    Ok(())
}

pub(super) async fn migrate(
    conn: &mut AnyConnection,
    id: &str,
    migration: Option<&media_extension_sdk::StorageMigration>,
) -> Result<(), Error> {
    if let Some(migration) = migration {
        for rename in &migration.key_renames {
            let conflicts: i64 = query_scalar("SELECT COUNT(*) FROM extension_data a JOIN extension_data b ON a.extension_id=b.extension_id AND a.scope=b.scope AND a.scope_id=b.scope_id WHERE a.extension_id=? AND a.key=? AND b.key=?")
                .bind(id).bind(&rename.from).bind(&rename.to).fetch_one(&mut *conn).await.map_err(db)?;
            if conflicts != 0 {
                return Err(err(
                    StatusCode::CONFLICT,
                    "extension_migration_key_conflict",
                ));
            }
            query("UPDATE extension_data SET key=?,version=? WHERE extension_id=? AND key=?")
                .bind(&rename.to)
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(id)
                .bind(&rename.from)
                .execute(&mut *conn)
                .await
                .map_err(db)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Rollback {
    expected_sha256: String,
    expected_version: String,
    confirm_data_restore: bool,
}

pub(super) async fn rollback(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Rollback>,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;
    if !input.confirm_data_restore {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "extension_rollback_confirmation_required",
        ));
    }
    let _lock = MUTATIONS.lock().await;
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let saved: Option<(String, String, String, String, String)> = query_as(
        "SELECT r.package_json,r.settings_json,r.target_sha256,e.package_json,p.public_key \
            FROM extension_update_revisions r \
            JOIN extensions e ON e.id=r.extension_id \
            JOIN extension_publishers p ON p.id=e.publisher_id AND p.active=1 \
            WHERE r.extension_id=?",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    let (previous, settings, target, current, key) =
        saved.ok_or_else(|| err(StatusCode::CONFLICT, "extension_rollback_unavailable"))?;
    let package: Package = serde_json::from_str(&previous)
        .map_err(|_| err(StatusCode::CONFLICT, "installed_package_invalid"))?;
    let active: Package = serde_json::from_str(&current)
        .map_err(|_| err(StatusCode::CONFLICT, "installed_package_invalid"))?;
    if active.sha256 != input.expected_sha256
        || active.sha256 != target
        || active.manifest.version != input.expected_version
    {
        return Err(err(StatusCode::CONFLICT, "extension_changed"));
    }
    let key = STANDARD
        .decode(key)
        .map_err(|_| err(StatusCode::CONFLICT, "invalid_public_key"))?;
    validate_package(&package, &key).map_err(|code| err(StatusCode::CONFLICT, code))?;
    if package.manifest.id != id || package.publisher_id != active.publisher_id {
        return Err(err(StatusCode::CONFLICT, "publisher_mismatch"));
    }
    // Disable after restoration. New invocations require administrator activation;
    // old workers immediately lose their generation and storage authorization.
    query("UPDATE extensions SET package_json=?,generation=?,enabled=0,all_libraries=(SELECT all_libraries FROM extension_update_revisions WHERE extension_id=?) WHERE id=?")
        .bind(&previous).bind(uuid::Uuid::new_v4().to_string()).bind(&id).bind(&id).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO extension_settings(extension_id,settings_json) VALUES(?,?) ON CONFLICT(extension_id) DO UPDATE SET settings_json=excluded.settings_json")
        .bind(&id).bind(settings).execute(&mut *tx).await.map_err(db)?;
    for table in [
        "extension_data",
        "extension_objects",
        "extension_data_namespaces",
        "extension_grants",
    ] {
        query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE extension_id=?"
        )))
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    for (destination, snapshot) in [
        ("extension_data_namespaces", "namespaces"),
        ("extension_data", "data"),
        ("extension_objects", "objects"),
        ("extension_grants", "grants"),
    ] {
        query(sqlx::AssertSqlSafe(format!("INSERT INTO {destination} SELECT * FROM extension_update_{snapshot} WHERE extension_id=?")))
            .bind(&id).execute(&mut *tx).await.map_err(db)?;
    }
    query("INSERT INTO extension_object_chunks SELECT c.* FROM extension_update_chunks c JOIN extension_update_objects o ON o.id=c.object_id WHERE o.extension_id=?")
        .bind(&id).execute(&mut *tx).await.map_err(db)?;
    clear_snapshot(&mut tx, &id).await.map_err(db)?;
    audit(&mut tx, &actor.user.id, &id, "package.rolled_back").await?;
    tx.commit().await.map_err(db)?;
    if let Err(error) = crate::extensions::storage::persist_upgrade(&auth.store, &package).await {
        eprintln!("Plugin '{id}' cache synchronization pending: {error}");
    }
    Ok(Json(
        json!({"id":id,"version":package.manifest.version,"enabled":false}),
    ))
}

/// Run the candidate before any mutation, with no HTTP hosts and no storage
/// broker. A health check must be self-contained and cannot alter live data.
pub(super) async fn verify_candidate(package: &Package) -> Result<(), Error> {
    let mut child = worker::spawn()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "worker_unavailable"))?;
    let invocation = crate::extensions::runtime::Invocation::new(
        package.wasm.clone(),
        ExtensionRequest {
            api_version: 1,
            operation: "lifecycle.verify".into(),
            input: json!({"version":package.manifest.version}),
        },
    );
    let operation = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        let bytes = serde_json::to_vec(&invocation)
            .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_input"))?;
        worker::write_frame(&mut stdin, &bytes)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        drop(stdin);
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?;
        // Oversized and truncated frames are the same failed health check.
        let bytes = worker::read_frame(&mut stdout).await.map_err(|_| {
            err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "extension_health_check_failed",
            )
        })?;
        let result: Value = serde_json::from_slice(&bytes).map_err(|_| {
            err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "extension_health_check_failed",
            )
        })?;
        if result["kind"] != "finished"
            || result["response"]["apiVersion"] != 1
            || result["response"]["result"]["healthy"] != true
        {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "extension_health_check_failed",
            ));
        }
        if !child
            .wait()
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "worker_io"))?
            .success()
        {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "extension_health_check_failed",
            ));
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(10), operation)
        .await
        .map_err(|_| {
            err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "extension_health_check_timeout",
            )
        })?
}
