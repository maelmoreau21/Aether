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
    #[serde(default = "default_sso_button_label")]
    pub sso_button_label: String,
    #[serde(default = "default_block_sso_local")]
    pub block_sso_local_accounts: bool,
    #[serde(default = "default_true")]
    pub trickplay_enabled: bool,
    #[serde(default = "default_trickplay_quality")]
    pub trickplay_quality: u8,
    #[serde(default = "default_trickplay_width")]
    pub trickplay_width: u16,
    #[serde(default = "default_trickplay_interval")]
    pub trickplay_interval_seconds: u16,
    #[serde(default = "default_false")]
    pub trickplay_auto_generate: bool,
    #[serde(default = "default_trickplay_cache_mb")]
    pub trickplay_max_cache_mb: u32,
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_trickplay_quality() -> u8 {
    7
}
fn default_trickplay_width() -> u16 {
    320
}
fn default_trickplay_interval() -> u16 {
    10
}
fn default_trickplay_cache_mb() -> u32 {
    5120
}
fn default_block_sso_local() -> bool {
    true
}
fn default_sso_button_label() -> String {
    "Se connecter avec SSO".into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    server_name: String,
    welcome_message: String,
    sso_button_label: Option<String>,
    block_sso_local_accounts: Option<bool>,
    trickplay_enabled: Option<bool>,
    trickplay_quality: Option<u8>,
    trickplay_width: Option<u16>,
    trickplay_interval_seconds: Option<u16>,
    trickplay_auto_generate: Option<bool>,
    trickplay_max_cache_mb: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_name: media_contracts::brand::DEFAULT_NAME.into(),
            welcome_message: String::new(),
            sso_button_label: default_sso_button_label(),
            block_sso_local_accounts: true,
            trickplay_enabled: true,
            trickplay_quality: 7,
            trickplay_width: 320,
            trickplay_interval_seconds: 10,
            trickplay_auto_generate: false,
            trickplay_max_cache_mb: 5120,
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
    Json(input): Json<SettingsInput>,
) -> Result<StatusCode, StatusCode> {
    administrator(&auth, &headers, true).await?;
    let previous = load(&auth.store).await?;
    let input = Settings {
        block_sso_local_accounts: input
            .block_sso_local_accounts
            .unwrap_or(previous.block_sso_local_accounts),
        server_name: input.server_name.trim().to_owned(),
        welcome_message: input.welcome_message.trim().to_owned(),
        sso_button_label: input
            .sso_button_label
            .unwrap_or(previous.sso_button_label)
            .trim()
            .to_owned(),
        trickplay_enabled: input
            .trickplay_enabled
            .unwrap_or(previous.trickplay_enabled),
        trickplay_quality: input
            .trickplay_quality
            .filter(|&q| (1..=31).contains(&q))
            .unwrap_or(previous.trickplay_quality),
        trickplay_width: input
            .trickplay_width
            .filter(|&w| (160..=1920).contains(&w))
            .unwrap_or(previous.trickplay_width),
        trickplay_interval_seconds: input
            .trickplay_interval_seconds
            .filter(|&i| (1..=60).contains(&i))
            .unwrap_or(previous.trickplay_interval_seconds),
        trickplay_auto_generate: input
            .trickplay_auto_generate
            .unwrap_or(previous.trickplay_auto_generate),
        trickplay_max_cache_mb: input
            .trickplay_max_cache_mb
            .filter(|&m| (128..=1048576).contains(&m))
            .unwrap_or(previous.trickplay_max_cache_mb),
    };
    if input.sso_button_label.is_empty()
        || input.sso_button_label.chars().count() > 80
        || input.sso_button_label.chars().any(char::is_control)
        || input.server_name.is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_deserialize_with_trickplay_defaults() {
        let legacy = r#"{
            "serverName": "Mon Aether",
            "welcomeMessage": "Bienvenue"
        }"#;

        let parsed: Settings = serde_json::from_str(legacy).expect("deserialize legacy settings");
        assert_eq!(parsed.server_name, "Mon Aether");
        assert_eq!(parsed.welcome_message, "Bienvenue");
        assert_eq!(parsed.sso_button_label, "Se connecter avec SSO");
        assert!(parsed.block_sso_local_accounts);
        assert!(parsed.trickplay_enabled);
        assert_eq!(parsed.trickplay_quality, 7);
        assert_eq!(parsed.trickplay_width, 320);
        assert_eq!(parsed.trickplay_interval_seconds, 10);
        assert!(!parsed.trickplay_auto_generate);
        assert_eq!(parsed.trickplay_max_cache_mb, 5120);
    }

    #[test]
    fn custom_trickplay_settings_roundtrip() {
        let custom = r#"{
            "serverName": "Aether Custom",
            "welcomeMessage": "Hello",
            "ssoButtonLabel": "SSO",
            "blockSsoLocalAccounts": false,
            "trickplayEnabled": false,
            "trickplayQuality": 12,
            "trickplayWidth": 240,
            "trickplayIntervalSeconds": 5,
            "trickplayAutoGenerate": true,
            "trickplayMaxCacheMb": 10240
        }"#;

        let parsed: Settings = serde_json::from_str(custom).expect("deserialize custom settings");
        assert!(!parsed.trickplay_enabled);
        assert_eq!(parsed.trickplay_quality, 12);
        assert_eq!(parsed.trickplay_width, 240);
        assert_eq!(parsed.trickplay_interval_seconds, 5);
        assert!(parsed.trickplay_auto_generate);
        assert_eq!(parsed.trickplay_max_cache_mb, 10240);

        let serialized = serde_json::to_string(&parsed).expect("serialize settings");
        let re_parsed: Settings = serde_json::from_str(&serialized).expect("roundtrip settings");
        assert_eq!(re_parsed.trickplay_quality, 12);
        assert_eq!(re_parsed.trickplay_width, 240);
        assert_eq!(re_parsed.trickplay_interval_seconds, 5);
        assert!(re_parsed.trickplay_auto_generate);
        assert_eq!(re_parsed.trickplay_max_cache_mb, 10240);
    }
}
