//! Explicit original-download permission. Playback bytes cannot be made copy-proof.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    allow_media_download: bool,
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/me/media-policy", get(own))
        .route("/api/v1/admin/users/{id}/media-policy", get(read).put(save))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}
pub async fn can_download(store: &IdentityStore, user: &str) -> Result<bool, StatusCode> {
    let flag: Option<i64> =
        query_scalar("SELECT allow_media_download FROM user_media_policies WHERE user_id=?")
            .bind(user)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(flag.unwrap_or(1) == 1)
}
async fn own(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Policy>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    Ok(Json(Policy {
        allow_media_download: can_download(&auth.store, &actor.user.id).await?,
    }))
}
async fn admin(auth: &AuthState, headers: &HeaderMap, mutation: bool) -> Result<(), StatusCode> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(())
}
async fn exists(store: &IdentityStore, id: &str) -> Result<(), StatusCode> {
    let count: i64 = query_scalar("SELECT COUNT(*) FROM users WHERE id=?")
        .bind(id)
        .fetch_one(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(())
    }
}
async fn read(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Policy>, StatusCode> {
    admin(&auth, &headers, false).await?;
    exists(&auth.store, &id).await?;
    Ok(Json(Policy {
        allow_media_download: can_download(&auth.store, &id).await?,
    }))
}
async fn save(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Policy>,
) -> Result<StatusCode, StatusCode> {
    admin(&auth, &headers, true).await?;
    exists(&auth.store, &id).await?;
    query("INSERT INTO user_media_policies(user_id,allow_media_download) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET allow_media_download=excluded.allow_media_download")
        .bind(id).bind(i64::from(input.allow_media_download)).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
