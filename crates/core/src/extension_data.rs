//! Core-owned private state. No database credentials or namespace IDs cross the ABI.
use crate::{
    database::{begin_write_scoped, query, query_as, query_scalar},
    identity::IdentityStore,
};
use media_extension_sdk::storage::{
    MAX_STORAGE_VALUE, STORAGE_API_VERSION, StorageEntry, StorageOperation, StorageRequest,
    StorageResponse, StorageScope,
};
use sha2::{Digest, Sha256};
use sqlx::AnyConnection;

pub const MAX_ENTRIES: i64 = 100_000;
pub const MAX_BYTES: i64 = 64 * 1024 * 1024;

// The same predicate guards transaction mutations, enumeration and atomic Get.
// Aliases e/l/n/u always denote extension/library/node/authenticated user.
const LIBRARY_ACCESS: &str = "l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0)) AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=l.id) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=l.id AND f.active=1)) AND (u.role='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions lp WHERE lp.user_id=u.id AND lp.library_id=l.id))";

fn private_permission(manifest: Option<&str>) -> Result<(), Error> {
    let manifest: media_extension_sdk::Manifest =
        serde_json::from_str(manifest.ok_or(Error::Forbidden)?).map_err(|_| Error::Corrupt)?;
    if manifest.permissions.iter().any(|p| p == "storage.private") {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

/// Constructed by the authenticated Core caller, never deserialized from guest input.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub extension_id: String,
    pub generation: String,
    pub library_id: String,
    pub work_id: String,
    pub season_id: String,
    pub episode_id: String,
    pub user_id: String,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage authorization revoked")]
    Forbidden,
    #[error("invalid storage request")]
    Invalid,
    #[error("storage quota exceeded")]
    Quota,
    #[error("storage version conflict")]
    Conflict,
    #[error("storage integrity failure")]
    Corrupt,
    #[error("storage operation timed out; verify state before retrying")]
    Timeout,
    #[error("storage database operation failed")]
    Database(#[from] sqlx::Error),
}
impl From<Error> for media_extension_sdk::storage::StorageError {
    fn from(error: Error) -> Self {
        Self {
            code: match error {
                Error::Forbidden => "forbidden",
                Error::Invalid => "invalid_request",
                Error::Quota => "quota_exceeded",
                Error::Conflict => "version_conflict",
                Error::Corrupt => "corrupt_data",
                Error::Timeout => "timeout",
                Error::Database(_) => "database_error",
            }
            .into(),
            message: error.to_string(),
        }
    }
}

/// Re-check authorization in the transaction that publishes each operation.
pub async fn authorize(conn: &mut AnyConnection, c: &Context) -> Result<(), Error> {
    let package:Option<String>=query_scalar("SELECT json_extract(e.package_json,'$.manifest') FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id JOIN users u ON u.id=? WHERE e.id=? AND e.generation=? AND e.enabled=1 AND p.active=1 AND u.disabled_at IS NULL")
 .bind(&c.user_id).bind(&c.extension_id).bind(&c.generation).fetch_optional(&mut *conn).await?;
    private_permission(package.as_deref())?;
    if !c.library_id.is_empty() {
        let allowed:i64=query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM extensions e JOIN libraries l ON l.id=? JOIN media_nodes n ON n.id=l.node_id JOIN users u ON u.id=? WHERE e.id=? AND {LIBRARY_ACCESS}")))
 .bind(&c.library_id).bind(&c.user_id).bind(&c.extension_id).fetch_one(&mut *conn).await?;
        if allowed != 1 {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}
pub fn scope_identity(c: &Context, s: &StorageScope) -> Result<(&'static str, String), Error> {
    let (kind, id) = match s.kind.as_str() {
        "global" => ("global", ""),
        "library" => ("library", c.library_id.as_str()),
        "work" => ("work", c.work_id.as_str()),
        "season" => ("season", c.season_id.as_str()),
        "episode" => ("episode", c.episode_id.as_str()),
        "user" => ("user", c.user_id.as_str()),
        _ => return Err(Error::Invalid),
    };
    if (!s.id.is_empty() && s.id != id) || (kind != "global" && id.is_empty()) {
        return Err(Error::Forbidden);
    }
    Ok((kind, id.into()))
}
pub async fn authorize_scope(
    conn: &mut AnyConnection,
    c: &Context,
    scope: &str,
    id: &str,
) -> Result<(), Error> {
    let mut statement = query_scalar::<sqlx::Any, i64>(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM users u WHERE u.id=? AND u.disabled_at IS NULL AND {}",
        scope_access(scope)?
    )))
    .bind(&c.user_id);
    if matches!(scope, "work" | "season" | "episode") {
        statement = statement.bind(id).bind(&c.library_id);
    }
    let valid = statement.fetch_one(&mut *conn).await?;
    if valid != 1 {
        return Err(Error::Forbidden);
    }
    Ok(())
}
fn scope_access(scope: &str) -> Result<&'static str, Error> {
    match scope {
        "global" => Ok("u.role='administrator'"),
        "work" => Ok("EXISTS(SELECT 1 FROM inventory_works w WHERE w.id=? AND w.library_id=?)"),
        "season" => Ok(
            "EXISTS(SELECT 1 FROM inventory_seasons s JOIN inventory_works w ON w.id=s.series_id WHERE s.id=? AND w.library_id=?)",
        ),
        "episode" => Ok(
            "EXISTS(SELECT 1 FROM inventory_episodes episode JOIN inventory_works w ON w.id=episode.work_id WHERE episode.id=? AND w.library_id=?)",
        ),
        "library" | "user" => Ok("1=1"),
        _ => Err(Error::Invalid),
    }
}
#[derive(sqlx::FromRow)]
struct Row {
    key: String,
    value: String,
    version: String,
    expires_at: Option<i64>,
    durability: String,
    value_bytes: i64,
    sha256: String,
}
impl Row {
    fn entry(self) -> Result<StorageEntry, Error> {
        if !valid_key(&self.key)
            || uuid::Uuid::parse_str(&self.version).is_err()
            || self.sha256.len() != 64
            || self.value.len() != self.value_bytes as usize
            || self.value.len() > MAX_STORAGE_VALUE
            || serde_json::to_vec(&self.value)
                .map_err(|_| Error::Corrupt)?
                .len()
                > 32 * 1024
            || format!("{:x}", Sha256::digest(self.value.as_bytes())) != self.sha256
            || !matches!(self.durability.as_str(), "cache" | "durable")
        {
            return Err(Error::Corrupt);
        }
        Ok(StorageEntry {
            key: self.key,
            value: self.value,
            version: self.version,
            expires_at: self.expires_at,
            durability: self.durability,
        })
    }
}
fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 256 && !key.chars().any(char::is_control)
}

/// Bounded physical expiry reclamation, only under write pressure. Expired rows
/// remain charged until reclaimed; no unbounded scan occurs on the read path.
async fn reclaim_expired(conn: &mut AnyConnection, c: &Context, now: i64) -> Result<(), Error> {
    let rows:Vec<(String,String,String,i64)>=query_as("SELECT scope,scope_id,key,value_bytes FROM extension_data WHERE extension_id=? AND expires_at IS NOT NULL AND expires_at<=? ORDER BY expires_at LIMIT 128").bind(&c.extension_id).bind(now).fetch_all(&mut *conn).await?;
    let count = rows.len() as i64;
    let mut bytes = 0i64;
    for (scope, id, key, size) in rows {
        bytes = bytes.checked_add(size).ok_or(Error::Corrupt)?;
        query(
            "DELETE FROM extension_data WHERE extension_id=? AND scope=? AND scope_id=? AND key=?",
        )
        .bind(&c.extension_id)
        .bind(scope)
        .bind(id)
        .bind(key)
        .execute(&mut *conn)
        .await?;
    }
    query(
        "UPDATE extension_data_namespaces SET entries=entries-?,bytes=bytes-? WHERE extension_id=?",
    )
    .bind(count)
    .bind(bytes)
    .bind(&c.extension_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct GetRow {
    manifest: Option<String>,
    key: Option<String>,
    value: Option<String>,
    version: Option<String>,
    expires_at: Option<i64>,
    durability: Option<String>,
    value_bytes: Option<i64>,
    sha256: Option<String>,
}

fn get_sql(scope: &str) -> Result<String, Error> {
    Ok(format!(
        "SELECT json_extract(e.package_json,'$.manifest') AS manifest,substr(d.key,1,257) AS key,substr(d.value,1,16385) AS value,substr(d.version,1,81) AS version,d.expires_at,substr(d.durability,1,16) AS durability,d.value_bytes,substr(d.sha256,1,65) AS sha256 FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id JOIN users u ON u.id=? LEFT JOIN extension_data_namespaces ns ON ns.extension_id=e.id LEFT JOIN extension_data d ON d.extension_id=e.id AND d.scope=? AND d.scope_id=? AND d.key=? AND (d.expires_at IS NULL OR d.expires_at>?) WHERE e.id=? AND e.generation=? AND e.enabled=1 AND p.active=1 AND u.disabled_at IS NULL AND (ns.extension_id IS NULL OR (ns.publisher_id=e.publisher_id AND ns.publisher_key=p.public_key)) AND (?='' OR EXISTS(SELECT 1 FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND {LIBRARY_ACCESS})) AND {}",
        scope_access(scope)?
    ))
}

macro_rules! bind_get {
    ($statement:expr,$c:expr,$scope:expr,$id:expr,$key:expr,$now:expr) => {{
        let mut statement = $statement
            .bind(&$c.user_id)
            .bind($scope)
            .bind($id)
            .bind($key)
            .bind($now)
            .bind(&$c.extension_id)
            .bind(&$c.generation)
            .bind(&$c.library_id)
            .bind(&$c.library_id);
        if matches!($scope, "work" | "season" | "episode") {
            statement = statement.bind($id).bind(&$c.library_id);
        }
        statement
    }};
}

/// Authorization, namespace ownership, expiry and value lookup share one SQL
/// statement snapshot. There is no permission cache and no read transaction
/// round trip; the same policy fragments guard the transactional operations.
async fn get(
    store: &IdentityStore,
    c: &Context,
    scope: &str,
    id: &str,
    key: &str,
    now: i64,
) -> Result<StorageResponse, Error> {
    if !valid_key(key) {
        return Err(Error::Invalid);
    }
    let sql = sqlx::AssertSqlSafe(get_sql(scope)?);
    let statement = bind_get!(query_as::<sqlx::Any, GetRow>(sql), c, scope, id, key, now);
    let found = statement
        .fetch_optional(store.pool())
        .await?
        .ok_or(Error::Forbidden)?;
    private_permission(found.manifest.as_deref())?;
    let mut response = StorageResponse::default();
    if let Some(key) = found.key {
        response.entries.push(
            Row {
                key,
                value: found.value.ok_or(Error::Corrupt)?,
                version: found.version.ok_or(Error::Corrupt)?,
                expires_at: found.expires_at,
                durability: found.durability.ok_or(Error::Corrupt)?,
                value_bytes: found.value_bytes.ok_or(Error::Corrupt)?,
                sha256: found.sha256.ok_or(Error::Corrupt)?,
            }
            .entry()?,
        );
    }
    Ok(response)
}

/// Test instrumentation only: PostgreSQL server execution time excludes the
/// client/network and includes EXPLAIN ANALYZE instrumentation. The normal Get
/// is authorized first; EXPLAIN executes the identical statement and parameters.
#[cfg(feature = "query-metrics")]
pub async fn postgres_get_plan(
    store: &IdentityStore,
    c: &Context,
    request: StorageRequest,
    now: i64,
) -> Result<Vec<String>, Error> {
    let StorageOperation::Get { key } = &request.operation else {
        return Err(Error::Invalid);
    };
    execute(store, c, request.clone(), now).await?;
    let (scope, id) = scope_identity(c, &request.scope)?;
    let mut conn = store.pool().acquire().await?;
    if conn.backend_name() != "PostgreSQL" {
        return Err(Error::Invalid);
    }
    let sql = sqlx::AssertSqlSafe(format!(
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) {}",
        get_sql(scope)?
    ));
    let statement = bind_get!(
        query_scalar::<sqlx::Any, String>(sql),
        c,
        scope,
        id.as_str(),
        key,
        now
    );
    Ok(statement.fetch_all(&mut *conn).await?)
}

pub async fn execute(
    store: &IdentityStore,
    c: &Context,
    request: StorageRequest,
    now: i64,
) -> Result<StorageResponse, Error> {
    let timeout_duration = if cfg!(test) {
        std::time::Duration::from_secs(10)
    } else {
        std::time::Duration::from_secs(2)
    };
    tokio::time::timeout(timeout_duration, execute_inner(store, c, request, now))
        .await
        .map_err(|_| Error::Timeout)?
}
async fn execute_inner(
    store: &IdentityStore,
    c: &Context,
    r: StorageRequest,
    now: i64,
) -> Result<StorageResponse, Error> {
    if r.api_version != STORAGE_API_VERSION {
        return Err(Error::Invalid);
    }
    let (scope, id) = scope_identity(c, &r.scope)?;
    if let StorageOperation::Get { key } = &r.operation {
        return get(store, c, scope, &id, key, now).await;
    }
    let mut conn = store.pool().acquire().await?;
    // Reads use a read transaction; writes share the existing cross-Core transaction lock.
    let writing = matches!(
        r.operation,
        StorageOperation::Put { .. } | StorageOperation::Delete { .. }
    );
    let mut tx = if writing {
        begin_write_scoped(&mut conn, "extension_data", &c.extension_id).await?
    } else {
        sqlx::Connection::begin(&mut *conn).await?
    };
    authorize(&mut tx, c).await?;
    authorize_scope(&mut tx, c, scope, &id).await?;
    let mut response = StorageResponse::default();
    if writing {
        query("INSERT INTO extension_data_namespaces(extension_id,publisher_id,publisher_key) SELECT e.id,e.publisher_id,p.public_key FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? ON CONFLICT(extension_id) DO NOTHING").bind(&c.extension_id).execute(&mut *tx).await?;
    }
    // A different publisher cannot inherit retained data by reusing an extension ID.
    let mismatch:i64=query_scalar("SELECT COUNT(*) FROM extension_data_namespaces n JOIN extensions e ON e.id=n.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE n.extension_id=? AND (n.publisher_id!=e.publisher_id OR n.publisher_key!=p.public_key)").bind(&c.extension_id).fetch_one(&mut *tx).await?;
    if mismatch != 0 {
        return Err(Error::Forbidden);
    }
    match r.operation {
        StorageOperation::Get { .. } => return Err(Error::Invalid),
        StorageOperation::List { after, limit } => {
            if after.len() > 256 || !(1..=100).contains(&limit) {
                return Err(Error::Invalid);
            }
            let rows:Vec<Row>=query_as("SELECT substr(key,1,257) AS key,substr(value,1,16385) AS value,substr(version,1,81) AS version,expires_at,substr(durability,1,16) AS durability,value_bytes,substr(sha256,1,65) AS sha256 FROM extension_data WHERE extension_id=? AND scope=? AND scope_id=? AND key>? AND (expires_at IS NULL OR expires_at>?) ORDER BY extension_data.key LIMIT ?").bind(&c.extension_id).bind(scope).bind(&id).bind(&after).bind(now).bind(i64::from(limit)+1).fetch_all(&mut *tx).await?;
            let mut bytes = 0;
            let mut more = false;
            for row in rows {
                // JSON strings can expand by 6x; keep the serialized ABI response below MAX_JSON.
                let entry = row.entry()?;
                let encoded = serde_json::to_vec(&entry)
                    .map_err(|_| Error::Corrupt)?
                    .len();
                if response.entries.len() >= limit as usize || bytes + encoded > 48 * 1024 {
                    more = true;
                    break;
                }
                bytes += encoded;
                response.entries.push(entry);
            }
            if more {
                response.next_cursor = response.entries.last().map(|e| e.key.clone());
            }
        }
        StorageOperation::Put {
            key,
            value,
            expected_version,
            expires_at,
            durability,
        } => {
            if !valid_key(&key)
                || value.len() > MAX_STORAGE_VALUE
                || serde_json::to_vec(&value)
                    .map_err(|_| Error::Invalid)?
                    .len()
                    > 32 * 1024
                || !matches!(durability.as_str(), "cache" | "durable")
                || expires_at.is_some_and(|v| v <= now)
            {
                return Err(Error::Invalid);
            }
            let old:Option<(String,i64,Option<i64>)>=query_as("SELECT substr(version,1,81),value_bytes,expires_at FROM extension_data WHERE extension_id=? AND scope=? AND scope_id=? AND key=?").bind(&c.extension_id).bind(scope).bind(&id).bind(&key).fetch_optional(&mut *tx).await?;
            if old.as_ref().is_some_and(|(version, bytes, _)| {
                uuid::Uuid::parse_str(version).is_err()
                    || !(0..=MAX_STORAGE_VALUE as i64).contains(bytes)
            }) {
                return Err(Error::Corrupt);
            }
            let live = old.as_ref().filter(|(_, _, e)| e.is_none_or(|v| v > now));
            if live.map(|v| v.0.as_str()) != expected_version.as_deref() {
                return Err(Error::Conflict);
            }
            let delta_entries = i64::from(old.is_none());
            let old_bytes = old.as_ref().map_or(0, |v| v.1);
            let bytes = value.len() as i64;
            let mut changed=query("UPDATE extension_data_namespaces SET entries=entries+?,bytes=bytes+? WHERE extension_id=? AND entries+?<=? AND bytes+?<=?").bind(delta_entries).bind(bytes-old_bytes).bind(&c.extension_id).bind(delta_entries).bind(MAX_ENTRIES).bind(bytes-old_bytes).bind(MAX_BYTES).execute(&mut *tx).await?;
            if changed.rows_affected() != 1 && old.is_none() {
                reclaim_expired(&mut tx, c, now).await?;
                changed=query("UPDATE extension_data_namespaces SET entries=entries+1,bytes=bytes+? WHERE extension_id=? AND entries+1<=? AND bytes+?<=?").bind(bytes).bind(&c.extension_id).bind(MAX_ENTRIES).bind(bytes).bind(MAX_BYTES).execute(&mut *tx).await?;
                if changed.rows_affected() != 1 {
                    // Preserve bounded cleanup progress even if this allocation
                    // needs multiple calls to free enough expired payloads.
                    tx.commit().await?;
                    return Err(Error::Quota);
                }
            }
            if changed.rows_affected() != 1 {
                return Err(Error::Quota);
            }
            let version = uuid::Uuid::new_v4().to_string();
            let hash = format!("{:x}", Sha256::digest(value.as_bytes()));
            query("INSERT INTO extension_data(extension_id,scope,scope_id,key,value,version,expires_at,durability,value_bytes,sha256) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(extension_id,scope,scope_id,key) DO UPDATE SET value=excluded.value,version=excluded.version,expires_at=excluded.expires_at,durability=excluded.durability,value_bytes=excluded.value_bytes,sha256=excluded.sha256")
 .bind(&c.extension_id).bind(scope).bind(&id).bind(&key).bind(&value).bind(&version).bind(expires_at).bind(&durability).bind(bytes).bind(hash).execute(&mut *tx).await?;
            response.entries.push(StorageEntry {
                key,
                value,
                version,
                expires_at,
                durability,
            });
        }
        StorageOperation::Delete {
            key,
            expected_version,
        } => {
            if !valid_key(&key) {
                return Err(Error::Invalid);
            }
            let row:Option<(String,i64)>=query_as("SELECT substr(version,1,81),value_bytes FROM extension_data WHERE extension_id=? AND scope=? AND scope_id=? AND key=?").bind(&c.extension_id).bind(scope).bind(&id).bind(&key).fetch_optional(&mut *tx).await?;
            if row.as_ref().is_some_and(|(version, bytes)| {
                uuid::Uuid::parse_str(version).is_err()
                    || !(0..=MAX_STORAGE_VALUE as i64).contains(bytes)
            }) {
                return Err(Error::Corrupt);
            }
            if expected_version
                .as_deref()
                .is_some_and(|expected| row.as_ref().is_none_or(|r| r.0 != expected))
            {
                return Err(Error::Conflict);
            }
            if let Some((_, bytes)) = row {
                query("DELETE FROM extension_data WHERE extension_id=? AND scope=? AND scope_id=? AND key=?").bind(&c.extension_id).bind(scope).bind(&id).bind(key).execute(&mut *tx).await?;
                query("UPDATE extension_data_namespaces SET entries=entries-1,bytes=bytes-? WHERE extension_id=?").bind(bytes).bind(&c.extension_id).execute(&mut *tx).await?;
            }
        }
    }
    tx.commit().await?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(operation: StorageOperation) -> StorageRequest {
        StorageRequest {
            api_version: 1,
            scope: StorageScope {
                kind: "global".into(),
                id: String::new(),
            },
            operation,
        }
    }
    fn put(key: &str, value: &str, version: Option<String>) -> StorageRequest {
        request(StorageOperation::Put {
            key: key.into(),
            value: value.into(),
            expected_version: version,
            expires_at: None,
            durability: "durable".into(),
        })
    }
    async fn fixture(store: &IdentityStore, id: &str) -> Context {
        query("INSERT INTO users(id,role) VALUES('storage-admin','administrator') ON CONFLICT(id) DO NOTHING").execute(store.pool()).await.unwrap();
        query("INSERT INTO extension_publishers(id,public_key) VALUES('storage-publisher','test-key') ON CONFLICT(id) DO NOTHING").execute(store.pool()).await.unwrap();
        let package = media_extension_sdk::Package {
            manifest: media_extension_sdk::Manifest {
                id: id.into(),
                permissions: vec!["storage.private".into()],
                ..Default::default()
            },
            publisher_id: "storage-publisher".into(),
            wasm: String::new(),
            sha256: String::new(),
            signature: String::new(),
        };
        query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES(?,'storage-publisher',?,1,'one')").bind(id).bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
        Context {
            extension_id: id.into(),
            generation: "one".into(),
            user_id: "storage-admin".into(),
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn persistence_isolation_cas_expiry_corruption_and_revocation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("core.sqlite3");
        let store = IdentityStore::open(&path).await.unwrap();
        let a = fixture(&store, "storage.a").await;
        let b = fixture(&store, "storage.b").await;
        let first = execute(&store, &a, put("key", "first", None), 10)
            .await
            .unwrap()
            .entries
            .remove(0);
        assert!(
            execute(
                &store,
                &b,
                request(StorageOperation::Get { key: "key".into() }),
                10
            )
            .await
            .unwrap()
            .entries
            .is_empty()
        );
        assert!(matches!(
            execute(&store, &a, put("key", "lost", None), 10).await,
            Err(Error::Conflict)
        ));
        let (left, right) = tokio::join!(
            execute(
                &store,
                &a,
                put("key", "left", Some(first.version.clone())),
                10
            ),
            execute(&store, &a, put("key", "right", Some(first.version)), 10)
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let exp = request(StorageOperation::Put {
            key: "expires".into(),
            value: "cache".into(),
            expected_version: None,
            expires_at: Some(20),
            durability: "cache".into(),
        });
        execute(&store, &a, exp, 10).await.unwrap();
        assert!(
            execute(
                &store,
                &a,
                request(StorageOperation::Get {
                    key: "expires".into()
                }),
                20
            )
            .await
            .unwrap()
            .entries
            .is_empty()
        );
        store.close().await;
        let store = IdentityStore::open(&path).await.unwrap();
        assert_eq!(
            execute(
                &store,
                &a,
                request(StorageOperation::Get { key: "key".into() }),
                21
            )
            .await
            .unwrap()
            .entries
            .len(),
            1
        );
        query("UPDATE extension_data SET value='corrupted' WHERE extension_id=? AND key='key'")
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(
                &store,
                &a,
                request(StorageOperation::Get { key: "key".into() }),
                21
            )
            .await,
            Err(Error::Corrupt)
        ));
        query("UPDATE extensions SET generation='revoked' WHERE id=?")
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(
                &store,
                &a,
                request(StorageOperation::Get { key: "key".into() }),
                21
            )
            .await,
            Err(Error::Forbidden)
        ));
    }
    #[tokio::test]
    async fn quotas_pagination_aba_and_publisher_pin() {
        let directory = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&directory.path().join("core.sqlite3"))
            .await
            .unwrap();
        let c = fixture(&store, "storage.c").await;
        let old = execute(&store, &c, put("a", "hello", None), 1)
            .await
            .unwrap()
            .entries
            .remove(0);
        execute(
            &store,
            &c,
            request(StorageOperation::Delete {
                key: "a".into(),
                expected_version: Some(old.version.clone()),
            }),
            1,
        )
        .await
        .unwrap();
        let new = execute(&store, &c, put("a", "hello", None), 1)
            .await
            .unwrap()
            .entries
            .remove(0);
        assert_ne!(old.version, new.version);
        assert!(matches!(
            execute(&store, &c, put("a", "stale", Some(old.version)), 1).await,
            Err(Error::Conflict)
        ));
        execute(&store, &c, put("b", "hello", None), 1)
            .await
            .unwrap();
        let page = execute(
            &store,
            &c,
            request(StorageOperation::List {
                after: String::new(),
                limit: 1,
            }),
            1,
        )
        .await
        .unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("a"));
        query("UPDATE extension_data_namespaces SET entries=? WHERE extension_id=?")
            .bind(MAX_ENTRIES)
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, put("over", "quota", None), 1).await,
            Err(Error::Quota)
        ));
        query("UPDATE extension_data_namespaces SET entries=2,bytes=? WHERE extension_id=?")
            .bind(MAX_BYTES)
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, put("bytes", "quota", None), 1).await,
            Err(Error::Quota)
        ));
        let mut hostile = request(StorageOperation::Get { key: "a".into() });
        hostile.scope = StorageScope {
            kind: "user".into(),
            id: "another-user".into(),
        };
        assert!(matches!(
            execute(&store, &c, hostile, 1).await,
            Err(Error::Forbidden)
        ));
        query(
            "UPDATE extension_publishers SET public_key='other-key' WHERE id='storage-publisher'",
        )
        .execute(store.pool())
        .await
        .unwrap();
        assert!(matches!(
            execute(
                &store,
                &c,
                request(StorageOperation::Get { key: "a".into() }),
                1
            )
            .await,
            Err(Error::Forbidden)
        ));
    }

    async fn common_contract(store: &IdentityStore) {
        let a = fixture(store, "contract.a").await;
        let b = fixture(store, "contract.b").await;
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('storage-library','Storage','series','local','synthetic',1)").execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('storage-work','storage-library','synthetic','Synthetic','{}')").execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_seasons(id,series_id,season_number) VALUES('storage-season','storage-work',1)").execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) VALUES('storage-episode','storage-season',1,'storage-work')").execute(store.pool()).await.unwrap();
        query("UPDATE extensions SET all_libraries=1 WHERE id=?")
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        let scoped = Context {
            library_id: "storage-library".into(),
            work_id: "storage-work".into(),
            season_id: "storage-season".into(),
            episode_id: "storage-episode".into(),
            ..a.clone()
        };
        for (kind, id) in [
            ("library", "storage-library"),
            ("work", "storage-work"),
            ("season", "storage-season"),
            ("episode", "storage-episode"),
            ("user", "storage-admin"),
        ] {
            let mut op = put("scoped", "value", None);
            op.scope = StorageScope {
                kind: kind.into(),
                id: id.into(),
            };
            execute(store, &scoped, op, 1).await.unwrap();
            let read = StorageRequest {
                api_version: 1,
                scope: StorageScope {
                    kind: kind.into(),
                    id: id.into(),
                },
                operation: StorageOperation::Get {
                    key: "scoped".into(),
                },
            };
            assert_eq!(
                execute(store, &scoped, read, 1).await.unwrap().entries[0].value,
                "value"
            );
        }
        query("UPDATE extensions SET all_libraries=0 WHERE id=?")
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        let mut revoked_scope = request(StorageOperation::Get {
            key: "scoped".into(),
        });
        revoked_scope.scope = StorageScope {
            kind: "library".into(),
            id: "storage-library".into(),
        };
        assert!(matches!(
            execute(store, &scoped, revoked_scope, 1).await,
            Err(Error::Forbidden)
        ));
        let first = execute(store, &a, put("key", "hello", None), 1)
            .await
            .unwrap()
            .entries
            .remove(0);
        assert!(
            execute(
                store,
                &b,
                request(StorageOperation::Get { key: "key".into() }),
                1
            )
            .await
            .unwrap()
            .entries
            .is_empty()
        );
        let (left, right) = tokio::join!(
            execute(
                store,
                &a,
                put("key", "left", Some(first.version.clone())),
                1
            ),
            execute(store, &a, put("key", "right", Some(first.version)), 1)
        );
        assert_ne!(left.is_ok(), right.is_ok());
        execute(
            store,
            &a,
            request(StorageOperation::Put {
                key: "expiry".into(),
                value: "cache".into(),
                expected_version: None,
                expires_at: Some(10),
                durability: "cache".into(),
            }),
            1,
        )
        .await
        .unwrap();
        assert!(
            execute(
                store,
                &a,
                request(StorageOperation::Get {
                    key: "expiry".into()
                }),
                10
            )
            .await
            .unwrap()
            .entries
            .is_empty()
        );
        query("UPDATE extension_data_namespaces SET entries=? WHERE extension_id=?")
            .bind(MAX_ENTRIES)
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(store, &a, put("over", "quota", None), 1).await,
            Err(Error::Quota)
        ));
        query("UPDATE extension_data SET sha256='bad' WHERE extension_id=? AND key='key'")
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(
                store,
                &a,
                request(StorageOperation::Get { key: "key".into() }),
                1
            )
            .await,
            Err(Error::Corrupt)
        ));
        query("UPDATE extensions SET enabled=0 WHERE id=?")
            .bind(&a.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(
                store,
                &a,
                request(StorageOperation::Get { key: "key".into() }),
                1
            )
            .await,
            Err(Error::Forbidden)
        ));
    }

    #[tokio::test]
    async fn sqlite_common_contract() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        common_contract(&store).await;
    }

    #[tokio::test]
    async fn bounded_expiry_cleanup_commits_progress_and_empty_scopes_are_bound() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        let c = fixture(&store, "expiry.progress").await;
        assert_eq!(
            scope_identity(
                &c,
                &StorageScope {
                    kind: "user".into(),
                    id: String::new()
                }
            )
            .unwrap()
            .1,
            c.user_id
        );
        assert!(matches!(
            scope_identity(
                &c,
                &StorageScope {
                    kind: "work".into(),
                    id: String::new()
                }
            ),
            Err(Error::Forbidden)
        ));
        assert!(matches!(
            scope_identity(
                &c,
                &StorageScope {
                    kind: "user".into(),
                    id: "other-user".into()
                }
            ),
            Err(Error::Forbidden)
        ));
        for i in 0..256 {
            execute(
                &store,
                &c,
                request(StorageOperation::Put {
                    key: format!("expires-{i}"),
                    value: "x".into(),
                    expected_version: None,
                    expires_at: Some(2),
                    durability: "cache".into(),
                }),
                1,
            )
            .await
            .unwrap();
        }
        // Inject quota pressure without writing another 64 MiB in a unit test.
        // The charged live remainder is irrelevant to the bounded GC sequence.
        query("UPDATE extension_data_namespaces SET bytes=? WHERE extension_id=?")
            .bind(MAX_BYTES)
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, put("new", &"x".repeat(129), None), 3).await,
            Err(Error::Quota)
        ));
        let remaining: i64 =
            query_scalar("SELECT COUNT(*) FROM extension_data WHERE extension_id=?")
                .bind(&c.extension_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(remaining, 128);
        execute(&store, &c, put("new", &"x".repeat(129), None), 3)
            .await
            .unwrap();
        let count: i64 =
            query_scalar("SELECT entries FROM extension_data_namespaces WHERE extension_id=?")
                .bind(&c.extension_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn oversized_persisted_fields_are_bounded_and_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        let c = fixture(&store, "corrupt.bounds").await;
        execute(&store, &c, put("key", "original", None), 1)
            .await
            .unwrap();
        let huge = "x".repeat(100_000);
        query("UPDATE extension_data SET value=?,value_bytes=? WHERE extension_id=?")
            .bind(&huge)
            .bind(huge.len() as i64)
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        for operation in [
            StorageOperation::Get { key: "key".into() },
            StorageOperation::List {
                after: String::new(),
                limit: 100,
            },
        ] {
            assert!(matches!(
                execute(&store, &c, request(operation), 1).await,
                Err(Error::Corrupt)
            ));
        }
        query(
            "UPDATE extension_data SET value='original',value_bytes=8,key=? WHERE extension_id=?",
        )
        .bind(&huge)
        .bind(&c.extension_id)
        .execute(store.pool())
        .await
        .unwrap();
        assert!(matches!(
            execute(
                &store,
                &c,
                request(StorageOperation::List {
                    after: String::new(),
                    limit: 100
                }),
                1
            )
            .await,
            Err(Error::Corrupt)
        ));
    }

    #[tokio::test]
    async fn atomic_get_rechecks_actor_publisher_and_signed_capability() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        let c = fixture(&store, "atomic.authorization").await;
        execute(&store, &c, put("private", "value", None), 1)
            .await
            .unwrap();
        let read = || {
            request(StorageOperation::Get {
                key: "private".into(),
            })
        };
        query("UPDATE users SET role='user' WHERE id=?")
            .bind(&c.user_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, read(), 1).await,
            Err(Error::Forbidden)
        ));
        query("UPDATE users SET role='administrator',disabled_at=1 WHERE id=?")
            .bind(&c.user_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, read(), 1).await,
            Err(Error::Forbidden)
        ));
        query("UPDATE users SET disabled_at=NULL WHERE id=?")
            .bind(&c.user_id)
            .execute(store.pool())
            .await
            .unwrap();
        query("UPDATE extension_publishers SET active=0 WHERE id='storage-publisher'")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, read(), 1).await,
            Err(Error::Forbidden)
        ));
        query("UPDATE extension_publishers SET active=1 WHERE id='storage-publisher'")
            .execute(store.pool())
            .await
            .unwrap();
        let json: String = query_scalar("SELECT package_json FROM extensions WHERE id=?")
            .bind(&c.extension_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
        let mut package: media_extension_sdk::Package = serde_json::from_str(&json).unwrap();
        package.manifest.permissions.clear();
        query("UPDATE extensions SET package_json=? WHERE id=?")
            .bind(serde_json::to_string(&package).unwrap())
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, read(), 1).await,
            Err(Error::Forbidden)
        ));
    }

    #[tokio::test]
    async fn grouped_parent_grants_do_not_bypass_disabled_parent_or_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        let mut c = fixture(&store, "grouped.permissions").await;
        for id in ["storage-parent", "storage-child"] {
            query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','synthetic',1)").bind(id).bind(id).execute(store.pool()).await.unwrap();
        }
        query("INSERT INTO library_folders(library_id,parent_id) VALUES('storage-child','storage-parent')").execute(store.pool()).await.unwrap();
        query("INSERT INTO extension_grants(extension_id,library_id) VALUES(?,'storage-parent')")
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        c.library_id = "storage-child".into();
        let scoped = || StorageRequest {
            api_version: 1,
            scope: StorageScope {
                kind: "library".into(),
                id: String::new(),
            },
            operation: StorageOperation::Get { key: "key".into() },
        };
        execute(&store, &c, scoped(), 1).await.unwrap();
        query("UPDATE libraries SET enabled=0 WHERE id='storage-parent'")
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, scoped(), 1).await,
            Err(Error::Forbidden)
        ));
        query("UPDATE libraries SET enabled=1 WHERE id='storage-parent'")
            .execute(store.pool())
            .await
            .unwrap();
        query("UPDATE library_folders SET active=0 WHERE library_id='storage-child'")
            .execute(store.pool())
            .await
            .unwrap();
        query("UPDATE extensions SET all_libraries=1 WHERE id=?")
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert!(matches!(
            execute(&store, &c, scoped(), 1).await,
            Err(Error::Forbidden)
        ));
    }

    #[tokio::test]
    #[ignore = "requires disposable MEDIA_TEST_POSTGRES_URL"]
    async fn postgres_common_contract() {
        let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL URL");
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
        let schema = format!("extension_storage_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        let mut scoped = url::Url::parse(&url).unwrap();
        scoped
            .query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let store = IdentityStore::open_postgres(scoped.as_str()).await.unwrap();
        common_contract(&store).await;
        store.close().await;
        // Only this test-created schema is removed, never an existing operator schema.
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
    }
}
