//! Private, durable binary objects stored separately from small values.
//! Chunks keep guest buffers and Core allocations bounded; all state and payloads
//! share the existing SQL transaction, backup and PostgreSQL visibility boundary.
use crate::{
    database::{begin_write_scoped, query, query_as, query_scalar},
    extension_data::{self, Context},
    identity::IdentityStore,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::objects::{
    MAX_OBJECT_BYTES, OBJECT_API_VERSION, OBJECT_CHUNK_BYTES, ObjectError, ObjectMetadata,
    ObjectOperation, ObjectRequest, ObjectResponse,
};
use sha2::{Digest, Sha256};
use sqlx::AnyConnection;
use std::time::Duration;

pub const MAX_OBJECT_COUNT: i64 = 64;
pub const MAX_OBJECT_TOTAL_BYTES: i64 = 128 * 1024 * 1024;

fn error(code: &str) -> ObjectError {
    ObjectError {
        code: code.into(),
        message: code.into(),
    }
}
fn database(_: sqlx::Error) -> ObjectError {
    error("storage_failed")
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    size: i64,
    received: i64,
    sha256: String,
    durability: String,
    expires_at: Option<i64>,
    complete: i64,
}
impl Row {
    fn metadata(self) -> ObjectMetadata {
        ObjectMetadata {
            id: self.id,
            size: self.size,
            sha256: self.sha256,
            durability: self.durability,
            expires_at: self.expires_at,
            complete: self.complete == 1,
        }
    }
}

async fn lookup(
    conn: &mut AnyConnection,
    context: &Context,
    kind: &str,
    scope_id: &str,
    id: &str,
    now: i64,
) -> Result<Row, ObjectError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error("object_not_found"));
    }
    let row: Option<Row> = query_as("SELECT id,size,received,substr(sha256,1,65) AS sha256,substr(durability,1,16) AS durability,expires_at,complete FROM extension_objects WHERE id=? AND extension_id=? AND scope_kind=? AND scope_id=? AND (expires_at IS NULL OR expires_at>?) AND (complete=1 OR upload_expires_at>?)")
        .bind(id).bind(&context.extension_id).bind(kind).bind(scope_id).bind(now).bind(now).fetch_optional(&mut *conn).await.map_err(database)?;
    let row = row.ok_or_else(|| error("object_not_found"))?;
    if !(0..=MAX_OBJECT_BYTES).contains(&row.size)
        || !(0..=row.size).contains(&row.received)
        || ![0, 1].contains(&row.complete)
        || !["cache", "durable"].contains(&row.durability.as_str())
        || row.sha256.len() != 64
    {
        return Err(error("object_corrupt"));
    }
    Ok(row)
}

async fn chunk(conn: &mut AnyConnection, row: &Row, offset: i64) -> Result<Vec<u8>, ObjectError> {
    let found: Option<(Vec<u8>, String)> = query_as(
        "SELECT substr(payload,1,16385),substr(sha256,1,65) FROM extension_object_chunks WHERE object_id=? AND offset_bytes=?",
    )
    .bind(&row.id)
    .bind(offset)
    .fetch_optional(&mut *conn)
    .await
    .map_err(database)?;
    let (bytes, hash) = found.ok_or_else(|| error("object_corrupt"))?;
    let expected = (row.size - offset).min(OBJECT_CHUNK_BYTES as i64);
    if bytes.len() as i64 != expected || digest(&bytes) != hash {
        return Err(error("object_corrupt"));
    }
    Ok(bytes)
}

/// Host supplies namespace/context and time; no identity is accepted from Wasm.
pub async fn execute(
    store: &IdentityStore,
    context: &Context,
    request: ObjectRequest,
    now: i64,
) -> Result<ObjectResponse, ObjectError> {
    tokio::time::timeout(
        Duration::from_secs(5),
        execute_inner(store, context, request, now),
    )
    .await
    .map_err(|_| error("storage_timeout"))?
}

async fn execute_inner(
    store: &IdentityStore,
    context: &Context,
    request: ObjectRequest,
    now: i64,
) -> Result<ObjectResponse, ObjectError> {
    if request.api_version != OBJECT_API_VERSION {
        return Err(error("unsupported_api_version"));
    }
    let (kind, scope_id) = extension_data::scope_identity(context, &request.scope)
        .map_err(|_| error("permission_denied"))?;
    let mut conn = store.pool().acquire().await.map_err(database)?;
    let mut tx = begin_write_scoped(&mut conn, "extension_objects", &context.extension_id)
        .await
        .map_err(database)?;
    extension_data::authorize(&mut tx, context)
        .await
        .map_err(|_| error("permission_denied"))?;
    extension_data::authorize_scope(&mut tx, context, kind, &scope_id)
        .await
        .map_err(|_| error("permission_denied"))?;
    query("INSERT INTO extension_data_namespaces(extension_id,publisher_id,publisher_key) SELECT e.id,e.publisher_id,p.public_key FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? ON CONFLICT(extension_id) DO NOTHING")
        .bind(&context.extension_id).execute(&mut *tx).await.map_err(database)?;
    let mismatch:i64=query_scalar("SELECT COUNT(*) FROM extension_data_namespaces n JOIN extensions e ON e.id=n.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE n.extension_id=? AND (n.publisher_id!=e.publisher_id OR n.publisher_key!=p.public_key)")
        .bind(&context.extension_id).fetch_one(&mut *tx).await.map_err(database)?;
    if mismatch != 0 {
        return Err(error("permission_denied"));
    }
    let mut response = ObjectResponse::default();
    match request.operation {
        ObjectOperation::Begin {
            size,
            sha256,
            durability,
            expires_at,
        } => {
            if !(0..=MAX_OBJECT_BYTES).contains(&size)
                || sha256.len() != 64
                || !sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !["cache", "durable"].contains(&durability.as_str())
                || expires_at.is_some_and(|time| time <= now)
                || (durability == "cache" && expires_at.is_none())
            {
                return Err(error("invalid_object"));
            }
            // Expired uploads and cache are reclaimed transactionally. Durable
            // results without an expiry are never automatically removed.
            query("DELETE FROM extension_objects WHERE extension_id=? AND ((expires_at IS NOT NULL AND expires_at<=?) OR (complete=0 AND upload_expires_at<=?))")
                .bind(&context.extension_id).bind(now).bind(now).execute(&mut *tx).await.map_err(database)?;
            let (count,used):(i64,i64)=query_as("SELECT COUNT(*),CAST(COALESCE(SUM(size),0) AS BIGINT) FROM extension_objects WHERE extension_id=?")
                .bind(&context.extension_id).fetch_one(&mut *tx).await.map_err(database)?;
            if count >= MAX_OBJECT_COUNT || used > MAX_OBJECT_TOTAL_BYTES - size {
                return Err(error("object_quota_exceeded"));
            }
            let id = uuid::Uuid::new_v4().to_string();
            query("INSERT INTO extension_objects(id,extension_id,scope_kind,scope_id,size,sha256,durability,expires_at,upload_expires_at) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(&id).bind(&context.extension_id).bind(kind).bind(&scope_id).bind(size).bind(&sha256).bind(&durability).bind(expires_at).bind(now.saturating_add(3600)).execute(&mut *tx).await.map_err(database)?;
            response.metadata = Some(ObjectMetadata {
                id,
                size,
                sha256,
                durability,
                expires_at,
                complete: false,
            });
            response.next_offset = Some(0);
        }
        ObjectOperation::Append {
            id,
            offset,
            data_base64,
        } => {
            if data_base64.len() > OBJECT_CHUNK_BYTES.div_ceil(3) * 4 {
                return Err(error("chunk_too_large"));
            }
            let row = lookup(&mut tx, context, kind, &scope_id, &id, now).await?;
            if row.complete != 0 || offset != row.received || offset >= row.size {
                return Err(error("object_conflict"));
            }
            let bytes = STANDARD
                .decode(data_base64)
                .map_err(|_| error("invalid_chunk"))?;
            if bytes.len() as i64 != (row.size - offset).min(OBJECT_CHUNK_BYTES as i64) {
                return Err(error("invalid_chunk"));
            }
            query("INSERT INTO extension_object_chunks(object_id,offset_bytes,payload,sha256) VALUES(?,?,?,?)")
                .bind(&id).bind(offset).bind(&bytes).bind(digest(&bytes)).execute(&mut *tx).await.map_err(database)?;
            let next = offset + bytes.len() as i64;
            query(
                "UPDATE extension_objects SET received=? WHERE id=? AND received=? AND complete=0",
            )
            .bind(next)
            .bind(&id)
            .bind(offset)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
            response.metadata = Some(row.metadata());
            response.next_offset = Some(next);
        }
        ObjectOperation::Finish { id } => {
            let mut row = lookup(&mut tx, context, kind, &scope_id, &id, now).await?;
            if row.received != row.size {
                return Err(error("object_incomplete"));
            }
            // Whole-object validation streams one bounded chunk at a time.
            let mut hash = Sha256::new();
            let mut offset = 0;
            while offset < row.size {
                let bytes = chunk(&mut tx, &row, offset).await?;
                offset += bytes.len() as i64;
                hash.update(bytes);
            }
            if format!("{:x}", hash.finalize()) != row.sha256 {
                return Err(error("object_corrupt"));
            }
            query("UPDATE extension_objects SET complete=1 WHERE id=?")
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(database)?;
            row.complete = 1;
            response.metadata = Some(row.metadata());
        }
        ObjectOperation::Read { id, offset } => {
            let row = lookup(&mut tx, context, kind, &scope_id, &id, now).await?;
            if row.complete != 1 {
                return Err(error("object_incomplete"));
            }
            if offset < 0
                || offset > row.size
                || (offset != row.size && offset % OBJECT_CHUNK_BYTES as i64 != 0)
            {
                return Err(error("invalid_offset"));
            }
            let bytes = if offset == row.size {
                Vec::new()
            } else {
                chunk(&mut tx, &row, offset).await?
            };
            let next = offset + bytes.len() as i64;
            response.next_offset = (next < row.size).then_some(next);
            response.data_base64 = Some(STANDARD.encode(bytes));
            response.metadata = Some(row.metadata());
        }
        ObjectOperation::Delete { id } => {
            lookup(&mut tx, context, kind, &scope_id, &id, now).await?;
            query("DELETE FROM extension_objects WHERE id=? AND extension_id=?")
                .bind(&id)
                .bind(&context.extension_id)
                .execute(&mut *tx)
                .await
                .map_err(database)?;
        }
    }
    tx.commit().await.map_err(database)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_extension_sdk::storage::StorageScope;

    async fn setup(store: &IdentityStore) -> Context {
        query("INSERT INTO users(id,role) VALUES('object-admin','administrator') ON CONFLICT DO NOTHING").execute(store.pool()).await.unwrap();
        query("INSERT INTO extension_publishers(id,public_key,active) VALUES('object-publisher','fixture',1) ON CONFLICT DO NOTHING").execute(store.pool()).await.unwrap();
        for id in ["object-test", "object-other"] {
            let package = media_extension_sdk::Package {
                manifest: media_extension_sdk::Manifest {
                    id: id.into(),
                    api_version: 1,
                    permissions: vec!["storage.private".into()],
                    ..Default::default()
                },
                publisher_id: "object-publisher".into(),
                wasm: String::new(),
                sha256: String::new(),
                signature: String::new(),
            };
            query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES(?,'object-publisher',?,1,'gen') ON CONFLICT DO NOTHING").bind(id).bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
        }
        Context {
            extension_id: "object-test".into(),
            generation: "gen".into(),
            user_id: "object-admin".into(),
            ..Default::default()
        }
    }
    fn request(operation: ObjectOperation) -> ObjectRequest {
        ObjectRequest {
            api_version: 1,
            scope: StorageScope {
                kind: "global".into(),
                id: String::new(),
            },
            operation,
        }
    }
    async fn begin(
        store: &IdentityStore,
        c: &Context,
        bytes: &[u8],
        durability: &str,
        expires_at: Option<i64>,
    ) -> String {
        execute(
            store,
            c,
            request(ObjectOperation::Begin {
                size: bytes.len() as i64,
                sha256: digest(bytes),
                durability: durability.into(),
                expires_at,
            }),
            100,
        )
        .await
        .unwrap()
        .metadata
        .unwrap()
        .id
    }
    async fn upload(store: &IdentityStore, c: &Context, bytes: &[u8]) -> String {
        let id = begin(store, c, bytes, "durable", None).await;
        for (index, chunk) in bytes.chunks(OBJECT_CHUNK_BYTES).enumerate() {
            execute(
                store,
                c,
                request(ObjectOperation::Append {
                    id: id.clone(),
                    offset: (index * OBJECT_CHUNK_BYTES) as i64,
                    data_base64: STANDARD.encode(chunk),
                }),
                100,
            )
            .await
            .unwrap();
        }
        execute(
            store,
            c,
            request(ObjectOperation::Finish { id: id.clone() }),
            100,
        )
        .await
        .unwrap();
        id
    }
    async fn concurrent_append_and_byte_quota(store: &IdentityStore) {
        let c = setup(store).await;
        let id = begin(store, &c, b"race", "durable", None).await;
        let a = request(ObjectOperation::Append {
            id: id.clone(),
            offset: 0,
            data_base64: STANDARD.encode(b"race"),
        });
        let (first, second) = tokio::join!(
            execute(store, &c, a.clone(), 100),
            execute(store, &c, a, 100)
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        assert_eq!(
            first.err().or_else(|| second.err()).unwrap().code,
            "object_conflict"
        );
        execute(
            store,
            &c,
            request(ObjectOperation::Finish { id: id.clone() }),
            100,
        )
        .await
        .unwrap();
        let output = execute(
            store,
            &c,
            request(ObjectOperation::Read {
                id: id.clone(),
                offset: 0,
            }),
            100,
        )
        .await
        .unwrap();
        assert_eq!(
            STANDARD.decode(output.data_base64.unwrap()).unwrap(),
            b"race"
        );
        execute(store, &c, request(ObjectOperation::Delete { id }), 100)
            .await
            .unwrap();
        for _ in 0..8 {
            execute(
                store,
                &c,
                request(ObjectOperation::Begin {
                    size: MAX_OBJECT_BYTES,
                    sha256: digest(b"reserved"),
                    durability: "durable".into(),
                    expires_at: None,
                }),
                100,
            )
            .await
            .unwrap();
        }
        assert_eq!(
            execute(
                store,
                &c,
                request(ObjectOperation::Begin {
                    size: 1,
                    sha256: digest(b"x"),
                    durability: "durable".into(),
                    expires_at: None
                }),
                100
            )
            .await
            .unwrap_err()
            .code,
            "object_quota_exceeded"
        );
        // Upload reservations expire without removing completed durable results.
        let next = execute(
            store,
            &c,
            request(ObjectOperation::Begin {
                size: 1,
                sha256: digest(b"x"),
                durability: "durable".into(),
                expires_at: None,
            }),
            3700,
        )
        .await
        .unwrap();
        assert_eq!(next.next_offset, Some(0));
    }

    #[tokio::test]
    async fn concurrent_append_and_reserved_byte_quota_sqlite() {
        let directory = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&directory.path().join("objects.sqlite3"))
            .await
            .unwrap();
        concurrent_append_and_byte_quota(&store).await;
    }

    #[tokio::test]
    #[ignore = "requires dedicated temporary MEDIA_TEST_POSTGRES_URL, never production"]
    async fn concurrent_append_and_reserved_byte_quota_postgres() {
        let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("dedicated PostgreSQL URL");
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
        let schema = format!("extension_objects_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin)
            .await
            .unwrap();
        let mut scoped = url::Url::parse(&url).unwrap();
        scoped
            .query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let store = IdentityStore::open_postgres(scoped.as_str()).await.unwrap();
        concurrent_append_and_byte_quota(&store).await;
        store.pool().close().await;
        // Only the fresh UUID schema created above is removed.
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }
    #[tokio::test]
    async fn chunked_objects_restart_integrity_isolation_and_revocation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("objects.sqlite3");
        let store = IdentityStore::open(&path).await.unwrap();
        let context = setup(&store).await;
        let bytes = vec![42u8; OBJECT_CHUNK_BYTES + 7];
        let id = upload(&store, &context, &bytes).await;
        let unfinished = begin(&store, &context, b"unfinished", "durable", None).await;
        store.pool().close().await;
        drop(store);
        let store = IdentityStore::open(&path).await.unwrap();
        let first = execute(
            &store,
            &context,
            request(ObjectOperation::Read {
                id: id.clone(),
                offset: 0,
            }),
            101,
        )
        .await
        .unwrap();
        assert_eq!(
            STANDARD.decode(first.data_base64.unwrap()).unwrap(),
            bytes[..OBJECT_CHUNK_BYTES]
        );
        assert_eq!(first.next_offset, Some(OBJECT_CHUNK_BYTES as i64));
        let last = execute(
            &store,
            &context,
            request(ObjectOperation::Read {
                id: id.clone(),
                offset: OBJECT_CHUNK_BYTES as i64,
            }),
            101,
        )
        .await
        .unwrap();
        assert_eq!(
            STANDARD.decode(last.data_base64.unwrap()).unwrap(),
            bytes[OBJECT_CHUNK_BYTES..]
        );
        assert!(last.next_offset.is_none());
        assert_eq!(
            execute(
                &store,
                &context,
                request(ObjectOperation::Read {
                    id: unfinished,
                    offset: 0
                }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "object_incomplete"
        );
        let other = Context {
            extension_id: "object-other".into(),
            ..context.clone()
        };
        assert_eq!(
            execute(
                &store,
                &other,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 0
                }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "object_not_found"
        );
        for bad in ["../private.sqlite3", "C:\\secret", "not-an-id"] {
            assert_eq!(
                execute(
                    &store,
                    &context,
                    request(ObjectOperation::Read {
                        id: bad.into(),
                        offset: 0
                    }),
                    101
                )
                .await
                .unwrap_err()
                .code,
                "object_not_found"
            );
        }
        query("UPDATE extension_object_chunks SET sha256='corrupt' WHERE object_id=? AND offset_bytes=0").bind(&id).execute(store.pool()).await.unwrap();
        assert_eq!(
            execute(
                &store,
                &context,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 0
                }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "object_corrupt"
        );
        // Simulate an externally corrupted database bypassing normal CHECKs.
        // The SQL projection caps the allocation before integrity validation.
        let mut corrupt_conn = store.pool().acquire().await.unwrap();
        query("PRAGMA ignore_check_constraints=ON")
            .execute(&mut *corrupt_conn)
            .await
            .unwrap();
        query("UPDATE extension_object_chunks SET payload=?,sha256=? WHERE object_id=? AND offset_bytes=0")
            .bind(vec![42u8;OBJECT_CHUNK_BYTES*2]).bind(digest(&bytes[..OBJECT_CHUNK_BYTES])).bind(&id).execute(&mut *corrupt_conn).await.unwrap();
        query("PRAGMA ignore_check_constraints=OFF")
            .execute(&mut *corrupt_conn)
            .await
            .unwrap();
        drop(corrupt_conn);
        assert_eq!(
            execute(
                &store,
                &context,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 0
                }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "object_corrupt"
        );
        query("UPDATE extension_objects SET sha256=? WHERE id=?")
            .bind("a".repeat(32 * 1024))
            .bind(&id)
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            execute(
                &store,
                &context,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 0
                }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "object_corrupt"
        );
        query("UPDATE extensions SET enabled=0 WHERE id=?")
            .bind(&context.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            execute(
                &store,
                &context,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 0
                }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "permission_denied"
        );
        let retained: i64 = query_scalar("SELECT COUNT(*) FROM extension_objects WHERE id=?")
            .bind(&id)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(retained, 1);
    }

    #[tokio::test]
    async fn chunk_conditions_expiry_quota_and_publisher_retention() {
        let directory = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&directory.path().join("objects.sqlite3"))
            .await
            .unwrap();
        let c = setup(&store).await;
        let id = begin(&store, &c, b"first", "cache", Some(200)).await;
        let append = request(ObjectOperation::Append {
            id: id.clone(),
            offset: 0,
            data_base64: STANDARD.encode(b"first"),
        });
        execute(&store, &c, append.clone(), 100).await.unwrap();
        assert_eq!(
            execute(&store, &c, append, 100).await.unwrap_err().code,
            "object_conflict"
        );
        execute(
            &store,
            &c,
            request(ObjectOperation::Finish { id: id.clone() }),
            100,
        )
        .await
        .unwrap();
        assert_eq!(
            execute(
                &store,
                &c,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 1
                }),
                100
            )
            .await
            .unwrap_err()
            .code,
            "invalid_offset"
        );
        assert_eq!(
            execute(
                &store,
                &c,
                request(ObjectOperation::Read {
                    id: id.clone(),
                    offset: 0
                }),
                200
            )
            .await
            .unwrap_err()
            .code,
            "object_not_found"
        );
        let wrong = begin(&store, &c, b"right", "durable", None).await;
        execute(
            &store,
            &c,
            request(ObjectOperation::Append {
                id: wrong.clone(),
                offset: 0,
                data_base64: STANDARD.encode(b"wrong"),
            }),
            100,
        )
        .await
        .unwrap();
        assert_eq!(
            execute(
                &store,
                &c,
                request(ObjectOperation::Finish { id: wrong }),
                100
            )
            .await
            .unwrap_err()
            .code,
            "object_corrupt"
        );
        let invalid = request(ObjectOperation::Begin {
            size: MAX_OBJECT_BYTES + 1,
            sha256: digest(b""),
            durability: "durable".into(),
            expires_at: None,
        });
        assert_eq!(
            execute(&store, &c, invalid, 100).await.unwrap_err().code,
            "invalid_object"
        );
        for _ in 2..MAX_OBJECT_COUNT {
            begin(&store, &c, b"", "durable", None).await;
        }
        assert_eq!(
            execute(
                &store,
                &c,
                request(ObjectOperation::Begin {
                    size: 0,
                    sha256: digest(b""),
                    durability: "durable".into(),
                    expires_at: None
                }),
                100
            )
            .await
            .unwrap_err()
            .code,
            "object_quota_exceeded"
        );
        query("INSERT INTO extension_publishers(id,public_key,active) VALUES('replacement','fixture',1)").execute(store.pool()).await.unwrap();
        query("UPDATE extensions SET publisher_id='replacement' WHERE id=?")
            .bind(&c.extension_id)
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            execute(
                &store,
                &c,
                request(ObjectOperation::Read { id, offset: 0 }),
                101
            )
            .await
            .unwrap_err()
            .code,
            "permission_denied"
        );
    }
}
