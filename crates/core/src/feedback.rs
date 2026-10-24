//! Private account-owned ratings and favorites; never included in public catalogue responses.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/feedback/{work}", get(read).put(write))
        .layer(DefaultBodyLimit::max(2048))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
pub(crate) async fn authorized(
    auth: &AuthState,
    headers: &HeaderMap,
    work: &str,
    mutation: bool,
) -> Result<String, StatusCode> {
    let identity = auth.authenticate(headers).await?;
    if mutation {
        auth.require_csrf(headers, &identity)?;
    }
    let found: Option<String> = query_scalar(
        "SELECT COALESCE( \
           (SELECT w.id FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id \
            WHERE w.id=? AND w.kind IN ('film','series','episode') AND l.enabled=1 AND n.status != 'revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?))), \
           (SELECT gw.id FROM media_group_members gm JOIN inventory_works gw ON gw.id=gm.work_id JOIN libraries gl ON gl.id=gw.library_id JOIN media_nodes gn ON gn.id=gl.node_id \
            WHERE (gm.group_id=? OR gm.group_id=(SELECT m2.group_id FROM media_group_members m2 WHERE m2.work_id=?)) \
              AND gw.kind IN ('film','series','episode') AND gl.enabled=1 AND gn.status != 'revoked' \
              AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=gl.id AND p.user_id=?)) \
            ORDER BY gw.id LIMIT 1) \
         )",
    )
    .bind(work)
    .bind(identity.user.role.as_str())
    .bind(&identity.user.id)
    .bind(work)
    .bind(work)
    .bind(identity.user.role.as_str())
    .bind(&identity.user.id)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .flatten();
    if found.is_none() {
        return Err(StatusCode::FORBIDDEN);
    }
    if !crate::product::target_allowed(&auth.store, &identity.user.id, "work", work).await? {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(identity.user.id)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Feedback {
    favorite: bool,
    rating: Option<f64>,
}
async fn read(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Feedback>, StatusCode> {
    let user = authorized(&auth, &headers, &work, false).await?;
    let work = crate::product::canonical_work(&auth.store, &work).await?;
    let row = query("SELECT favorite,rating FROM work_feedback WHERE user_id=? AND work_id=?")
        .bind(user)
        .bind(work)
        .fetch_optional(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        row.map(|r| Feedback {
            favorite: r.get::<i64, _>("favorite") == 1,
            rating: r.get("rating"),
        })
        .unwrap_or(Feedback {
            favorite: false,
            rating: None,
        }),
    ))
}
async fn write(
    State(auth): State<AuthState>,
    Path(work): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Feedback>,
) -> Result<Json<Feedback>, StatusCode> {
    let user = authorized(&auth, &headers, &work, true).await?;
    let work = crate::product::canonical_work(&auth.store, &work).await?;
    if input
        .rating
        .is_some_and(|r| !r.is_finite() || !(0.5..=5.0).contains(&r) || (r * 2.0).fract() != 0.0)
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    query("INSERT INTO work_feedback(user_id,work_id,favorite,rating) VALUES(?,?,?,?) ON CONFLICT(user_id,work_id) DO UPDATE SET favorite=excluded.favorite,rating=excluded.rating")
    .bind(user).bind(work).bind(if input.favorite {1i64} else {0}).bind(input.rating).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(input))
}
