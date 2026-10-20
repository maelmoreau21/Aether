//! Persisted server presentation and effective network configuration. Deployment transport remains explicit.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub server_name: String,
    pub welcome_message: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            server_name: media_contracts::brand::DEFAULT_NAME.into(),
            welcome_message: String::new(),
        }
    }
}
pub async fn load(store: &IdentityStore) -> Result<Settings, StatusCode> {
    let saved: Option<String> =
        query_scalar("SELECT settings_json FROM server_settings WHERE singleton=1")
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    saved
        .map(|s| serde_json::from_str(&s).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR))
        .unwrap_or(Ok(Settings::default()))
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/settings", get(read).put(write))
        .route("/api/v1/admin/network", get(network))
        .route("/api/v1/server", get(presentation))
        .layer(DefaultBodyLimit::max(4096))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let identity = auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &identity)?;
    }
    Ok(())
}
async fn read(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Settings>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    Ok(Json(load(&auth.store).await?))
}
async fn write(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(mut input): Json<Settings>,
) -> Result<StatusCode, StatusCode> {
    administrator(&auth, &headers, true).await?;
    input.server_name = input.server_name.trim().to_owned();
    input.welcome_message = input.welcome_message.trim().to_owned();
    if input.server_name.is_empty()
        || input.server_name.chars().count() > 80
        || input.server_name.chars().any(char::is_control)
        || input.welcome_message.chars().count() > 500
        || input
            .welcome_message
            .chars()
            .any(|c| c.is_control() && c != '\n')
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let json = serde_json::to_string(&input).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    query("INSERT INTO server_settings(singleton,settings_json) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET settings_json=excluded.settings_json").bind(json).execute(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn presentation(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Settings>, StatusCode> {
    auth.authenticate(&headers).await?;
    Ok(Json(load(&auth.store).await?))
}
async fn network(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    Ok(Json(
        serde_json::json!({"publicUrl":auth.origin(),"secureCookies":auth.secure,"webPath":"/app/","apiPath":"/api/v1/","transportManagedByDeployment":true}),
    ))
}
