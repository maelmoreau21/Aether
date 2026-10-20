//! Revocable, optionally expiring, catalogue-only credentials for explicit integrations.
use crate::{
    auth::{AuthState, hash, random_token},
    database::{query, query_as, query_scalar},
    groups::{administrator, recheck_administrator},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/api-keys", get(list).post(create))
        .route("/api/v1/admin/api-keys/{id}", axum::routing::delete(revoke))
        .route("/api/v1/integrations/catalog", get(catalog))
        .layer(DefaultBodyLimit::max(16384))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
fn database(_: sqlx::Error) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    name: String,
    user_id: String,
    library_ids: Vec<String>,
    expires_in_days: Option<i64>,
    #[serde(default)]
    never_expires: bool,
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    let mut tx = auth.store.pool.begin().await.map_err(database)?;
    let rows:Vec<(String,String,String,i64,i64,Option<i64>)>=query_as("SELECT id,name,user_id,created_at,expires_at,revoked_at FROM api_keys ORDER BY created_at DESC,id").fetch_all(&mut *tx).await.map_err(database)?;
    let libs: Vec<(String, String)> =
        query_as("SELECT key_id,library_id FROM api_key_libraries ORDER BY library_id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let keys:Vec<_>=rows.into_iter().map(|(id,name,user,created,expires,revoked)|json!({"id":id,"name":name,"userId":user,"createdAt":created,"expiresAt":if expires == 0 { None } else { Some(expires) },"revokedAt":revoked,"scope":"catalog:read","libraryIds":libs.iter().filter(|v|v.0==id).map(|v|&v.1).collect::<Vec<_>>()})).collect();
    tx.commit().await.map_err(database)?;
    Ok(Json(json!({"keys":keys})))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    let name = input.name.trim();
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || !matches!(
            (input.never_expires, input.expires_in_days),
            (true, None) | (false, Some(1..=365))
        )
        || input.library_ids.is_empty()
        || input.library_ids.len() > 100
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, &actor).await?;
    let role: Option<String> =
        query_scalar("SELECT role FROM users WHERE id=? AND disabled_at IS NULL")
            .bind(&input.user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database)?;
    let role = role.ok_or(StatusCode::BAD_REQUEST)?;
    for lib in &input.library_ids {
        let allowed:i64=query_scalar("SELECT count(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND n.status!='revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?))").bind(lib).bind(&role).bind(&input.user_id).fetch_one(&mut *tx).await.map_err(database)?;
        if allowed != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let count: i64 = query_scalar("SELECT count(*) FROM api_keys")
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
    if count >= 1000 {
        return Err(StatusCode::CONFLICT);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let token = zeroize::Zeroizing::new(format!("media_{}", random_token()));
    let now: i64 = query_scalar("SELECT unixepoch()")
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
    // Zero is the persisted no-expiration sentinel; never infer it from an omitted duration.
    let expires = input.expires_in_days.map_or(0, |days| now + days * 86400);
    query("INSERT INTO api_keys(id,name,user_id,token_hash,created_at,expires_at,scope) VALUES(?,?,?,?,?,?,'catalog:read')").bind(&id).bind(name).bind(&input.user_id).bind(hash(&token)).bind(now).bind(expires).execute(&mut *tx).await.map_err(database)?;
    for lib in input.library_ids {
        query(
            "INSERT INTO api_key_libraries(key_id,library_id) VALUES(?,?) ON CONFLICT DO NOTHING",
        )
        .bind(&id)
        .bind(lib)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"id":id,"token":token.as_str(),"expiresAt":if expires == 0 { None } else { Some(expires) },"scope":"catalog:read"}),
        ),
    ))
}
async fn revoke(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, &actor).await?;
    let affected =
        query("UPDATE api_keys SET revoked_at=COALESCE(revoked_at,unixepoch()) WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?
            .rows_affected();
    if affected == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    tx.commit().await.map_err(database)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Page {
    offset: Option<i64>,
    limit: Option<i64>,
}
async fn catalog(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>, StatusCode> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| {
            v.len() == 70
                && v.starts_with("media_")
                && v[6..].bytes().all(|b| b.is_ascii_hexdigit())
        })
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let limit = page.limit.unwrap_or(24);
    let offset = page.offset.unwrap_or(0);
    if !(1..=100).contains(&limit) || !(0..=1_000_000).contains(&offset) {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Credential, owner and permissions are read from one live snapshot, every request.
    let mut tx = auth.store.pool.begin().await.map_err(database)?;
    let credential:Option<(String,String,String)>=query_as("SELECT k.id,k.user_id,u.role FROM api_keys k JOIN users u ON u.id=k.user_id WHERE k.token_hash=? AND k.scope='catalog:read' AND k.revoked_at IS NULL AND (k.expires_at=0 OR k.expires_at>unixepoch()) AND u.disabled_at IS NULL").bind(hash(token)).fetch_optional(&mut *tx).await.map_err(database)?;
    let (key, user, role) = credential.ok_or(StatusCode::UNAUTHORIZED)?;
    let sql = format!(
        "SELECT w.id,w.library_id,w.title,w.kind,w.year FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id JOIN api_key_libraries kl ON kl.library_id=l.id AND kl.key_id=? WHERE l.enabled=1 AND n.status!='revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND {} ORDER BY w.title_sort,w.id LIMIT ? OFFSET ?",
        crate::product::policy_sql()
    );
    let sql = crate::database::normalize_sql(&sql);
    let rows: Vec<(String, String, String, String, Option<i64>)> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(key)
            .bind(role)
            .bind(&user)
            .bind(&user)
            .bind(limit)
            .bind(offset)
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    tx.commit().await.map_err(database)?;
    let items:Vec<_>=rows.into_iter().map(|(id,library,title,kind,year)|json!({"id":id,"libraryId":library,"title":title,"kind":kind,"year":year})).collect();
    Ok(Json(json!({"items":items,"offset":offset,"limit":limit})))
}
