use super::*;

/// Cache provider responses for external APIs with TTL.
pub async fn get_cached_provider_response(
    tx: &mut Transaction<'_, Any>,
    provider: &str,
    operation: &str,
    query_val: &Value,
    now: i64,
) -> Result<Option<Value>, sqlx::Error> {
    let query_hash = hash(query_val)?;
    let cache_key = format!("{}:{}:{}", provider, operation, query_hash);
    let row: Option<(String, i64)> = query_as(
        "SELECT response_json, expires_at FROM automation_provider_cache WHERE cache_key=?",
    )
    .bind(&cache_key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((response_json, expires_at)) = row {
        if expires_at > now {
            if let Ok(val) = serde_json::from_str::<Value>(&response_json) {
                return Ok(Some(val));
            }
        } else {
            query("DELETE FROM automation_provider_cache WHERE cache_key=?")
                .bind(&cache_key)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(None)
}

pub async fn put_cached_provider_response(
    tx: &mut Transaction<'_, Any>,
    provider: &str,
    operation: &str,
    query_val: &Value,
    response: &Value,
    ttl_seconds: i64,
    now: i64,
) -> Result<(), sqlx::Error> {
    let query_hash = hash(query_val)?;
    let cache_key = format!("{}:{}:{}", provider, operation, query_hash);
    let expires_at = now.saturating_add(ttl_seconds.max(60));
    let response_json =
        serde_json::to_string(response).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    query("INSERT INTO automation_provider_cache(cache_key,provider,operation,query_hash,response_json,expires_at,created_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(cache_key) DO UPDATE SET response_json=excluded.response_json,expires_at=excluded.expires_at,created_at=excluded.created_at")
        .bind(cache_key)
        .bind(provider)
        .bind(operation)
        .bind(query_hash)
        .bind(response_json)
        .bind(expires_at)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn purge_expired_cache(
    tx: &mut Transaction<'_, Any>,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let affected = query("DELETE FROM automation_provider_cache WHERE expires_at<=?")
        .bind(now)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(affected)
}

/// Examine at most 64 primary-key-ordered entries in a background pass.
/// The caller holds the cursor; wrapping eventually revisits unexpired entries.
pub async fn purge_provider_cache_batch(
    store: &IdentityStore,
    cursor: &mut String,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let keys: Vec<String> = query_scalar("SELECT cache_key FROM automation_provider_cache WHERE cache_key>? ORDER BY cache_key LIMIT 64")
        .bind(&*cursor).fetch_all(store.pool()).await?;
    let Some(last) = keys.last() else {
        cursor.clear();
        return Ok(0);
    };
    let affected = query("DELETE FROM automation_provider_cache WHERE cache_key IN (SELECT cache_key FROM automation_provider_cache WHERE cache_key>? AND cache_key<=? AND expires_at<=? ORDER BY cache_key LIMIT 64)")
        .bind(&*cursor).bind(last).bind(now).execute(store.pool()).await?.rows_affected();
    *cursor = last.clone();
    Ok(affected)
}
