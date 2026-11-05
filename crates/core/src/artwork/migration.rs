//! One resumable application-only migration unit. Never opens a media-library path.
use super::*;
static MIGRATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[cfg(test)]
mod tests;

pub(crate) async fn migrate_one(store: &IdentityStore) -> Result<bool, StatusCode> {
    let _guard = MIGRATION.lock().await;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // Rebuild all representations from the best stored source, never from an
    // already resized derivative. Existing image identities and aliases survive.
    let entry: Option<(String,String,String,String)> = query_as("SELECT e.work_id,e.id,e.kind,d.sha256 FROM work_image_entries e JOIN work_image_data d ON d.image_id=e.id AND d.width=0 LEFT JOIN artwork_annotations a ON a.work_id=e.work_id AND a.image_id=e.id LEFT JOIN image_migration_attempts m ON m.sha256='entry:'||e.id WHERE (a.encoding IS NULL OR a.encoding!=?) AND (m.sha256 IS NULL OR m.retry_at<=?) ORDER BY e.id LIMIT 1")
        .bind(crate::image_import::ENCODING).bind(timestamp).fetch_optional(store.pool()).await.map_err(db)?;
    if let Some((work, id, kind, sha)) = entry {
        let key = format!("entry:{id}");
        let result = migrate_entry(store, &work, &id, &kind, &sha).await;
        record(store, &key, result.is_ok(), timestamp).await?;
        result?;
        return Ok(true);
    }
    // Other owner tables keep their established APIs. Only referenced raster
    // objects are selected. The registry also drives GC and is schema-tested.
    let referenced = crate::assets::REFERENCING_COLUMNS
        .iter()
        .map(|(table, column)| format!("EXISTS(SELECT 1 FROM {table} WHERE {column}=a.sha256)"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "SELECT a.sha256 FROM assets a LEFT JOIN image_migration_attempts m ON m.sha256=a.sha256 WHERE (a.mime IN ('image/jpeg','image/png') OR (a.mime='image/webp' AND EXISTS(SELECT 1 FROM work_posters p WHERE p.sha256=a.sha256))) AND (m.sha256 IS NULL OR (m.outcome!='completed' AND m.retry_at<=?)) AND ({referenced}) ORDER BY a.sha256 LIMIT 1"
    );
    let sha: Option<String> = query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(timestamp)
        .fetch_optional(store.pool())
        .await
        .map_err(db)?;
    let Some(sha) = sha else {
        return Ok(false);
    };
    let result = migrate_object(store, &sha).await;
    record(store, &sha, result.is_ok(), timestamp).await?;
    result?;
    Ok(true)
}

async fn record(
    store: &IdentityStore,
    key: &str,
    success: bool,
    timestamp: i64,
) -> Result<(), StatusCode> {
    query("INSERT INTO image_migration_attempts(sha256,attempts,retry_at,outcome) VALUES(?,1,?,?) ON CONFLICT(sha256) DO UPDATE SET attempts=image_migration_attempts.attempts+1,retry_at=excluded.retry_at,outcome=excluded.outcome")
        .bind(key).bind(timestamp+3600).bind(if success {"completed"} else {"retry"}).execute(store.pool()).await.map_err(db)?;
    Ok(())
}

async fn prepared_source(
    store: &IdentityStore,
    sha: &str,
    kind: &str,
) -> Result<Encoded, StatusCode> {
    let (_, bytes) = store
        .assets
        .read_all(sha)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Repair historical PNG bytes incorrectly declared image/webp as well.
    let mime = image::guess_format(&bytes)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?
        .to_mime_type();
    prepare(bytes.into(), mime.into(), kind.into()).await
}

async fn migrate_entry(
    store: &IdentityStore,
    work: &str,
    id: &str,
    kind: &str,
    sha: &str,
) -> Result<(), StatusCode> {
    let _source_lease = store
        .assets
        .lease(sha)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let encoded = prepared_source(store, sha, kind).await?;
    let mut variants = Vec::new();
    let mut leases = Vec::new();
    for (width, mime, bytes) in encoded.variants {
        let meta = store
            .assets
            .put_bytes(&bytes, mime)
            .await
            .map_err(|_| StatusCode::INSUFFICIENT_STORAGE)?;
        leases.push(
            store
                .assets
                .lease(&meta.sha256)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        store
            .assets
            .verify(&meta.sha256)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        variants.push((width, meta.sha256));
    }
    let mut connection = store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut connection)
        .await
        .map_err(db)?;
    let current: Option<String> =
        query_scalar("SELECT sha256 FROM work_image_data WHERE image_id=? AND width=0")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    if current.as_deref() != Some(sha) {
        return Ok(());
    }
    for (width, sha) in variants {
        query("INSERT INTO work_image_data(image_id,width,mime,sha256) VALUES(?,?,'image/webp',?) ON CONFLICT(image_id,width) DO UPDATE SET mime=excluded.mime,sha256=excluded.sha256")
            .bind(id).bind(width).bind(sha).execute(&mut *tx).await.map_err(db)?;
    }
    query("UPDATE work_image_entries SET width=?,height=?,fingerprint=? WHERE id=? AND work_id=?")
        .bind(encoded.width)
        .bind(encoded.height)
        .bind(encoded.fingerprint.0)
        .bind(id)
        .bind(work)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    query("INSERT INTO artwork_annotations(work_id,image_id,kind,source_sha256,encoding) VALUES(?,?,?,?,?) ON CONFLICT(work_id,image_id) DO UPDATE SET encoding=excluded.encoding")
        .bind(work).bind(id).bind(kind).bind(sha).bind(crate::image_import::ENCODING).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(())
}

async fn migrate_object(store: &IdentityStore, sha: &str) -> Result<(), StatusCode> {
    let _source_lease = store
        .assets
        .lease(sha)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Generic raster owners include avatars and library artwork, for which
    // preserving pixels is preferable to guessing a photographic role.
    let encoded = prepared_source(store, sha, "poster").await?;
    let mut variants = Vec::new();
    let mut leases = Vec::new();
    for (width, mime, bytes) in encoded.variants {
        let meta = store
            .assets
            .put_bytes(&bytes, mime)
            .await
            .map_err(|_| StatusCode::INSUFFICIENT_STORAGE)?;
        leases.push(
            store
                .assets
                .lease(&meta.sha256)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        store
            .assets
            .verify(&meta.sha256)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        variants.push((width, meta.sha256));
    }
    let target = &variants[0].1;
    let mut connection = store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut connection)
        .await
        .map_err(db)?;
    // Legacy poster derivatives are generated once before references change.
    let posters: Vec<(String, String)> =
        query_as("SELECT work_id,revision FROM work_posters WHERE sha256=?")
            .bind(sha)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
    for (work, revision) in posters {
        for (width, digest) in variants.iter().filter(|v| v.0 != 0) {
            query("INSERT INTO poster_previews(work_id,width,revision,mime,sha256) VALUES(?,?,?,'image/webp',?) ON CONFLICT(work_id,width) DO UPDATE SET revision=excluded.revision,mime=excluded.mime,sha256=excluded.sha256")
                .bind(&work).bind(width).bind(&revision).bind(digest).execute(&mut *tx).await.map_err(db)?;
        }
    }
    let libraries: Vec<(String, String)> =
        query_as("SELECT library_id,kind FROM library_artwork WHERE sha256=?")
            .bind(sha)
            .fetch_all(&mut *tx)
            .await
            .map_err(db)?;
    for (library, kind) in libraries {
        for (width, digest) in variants.iter().filter(|v| v.0 != 0) {
            query("INSERT INTO library_artwork_previews(library_id,kind,width,sha256) VALUES(?,?,?,?) ON CONFLICT(library_id,kind,width) DO UPDATE SET sha256=excluded.sha256")
                .bind(&library).bind(&kind).bind(width).bind(digest).execute(&mut *tx).await.map_err(db)?;
        }
    }
    for (table, column) in crate::assets::REFERENCING_COLUMNS {
        query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET {column}=? WHERE {column}=?"
        )))
        .bind(target)
        .bind(sha)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    query("UPDATE work_image_data SET mime='image/webp' WHERE sha256=?")
        .bind(target)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    query("UPDATE poster_previews SET mime='image/webp' WHERE sha256=?")
        .bind(target)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    crate::inventory::invalidate_catalogue_cache();
    // No deletion: AssetStore's two-phase GC retains the former objects for its grace period.
    Ok(())
}
