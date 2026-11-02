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
    #[serde(default = "default_min_resume_pct")]
    pub min_resume_pct: u8,
    #[serde(default = "default_max_resume_pct")]
    pub max_resume_pct: u8,
    #[serde(default = "default_min_resume_duration")]
    pub min_resume_duration_seconds: u32,
    #[serde(default = "default_metadata_language")]
    pub preferred_metadata_language: String,
    #[serde(default = "default_metadata_country")]
    pub metadata_country_code: String,
    #[serde(default = "default_activity_retention")]
    pub activity_log_retention_days: u32,
    #[serde(default)]
    pub cors_hosts: Vec<String>,
}
fn default_activity_retention() -> u32 {
    30
}
fn default_min_resume_pct() -> u8 {
    0
}
fn default_max_resume_pct() -> u8 {
    90
}
fn default_min_resume_duration() -> u32 {
    0
}
fn default_metadata_language() -> String {
    "fr".into()
}
fn default_metadata_country() -> String {
    "FR".into()
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
    min_resume_pct: Option<u8>,
    max_resume_pct: Option<u8>,
    min_resume_duration_seconds: Option<u32>,
    preferred_metadata_language: Option<String>,
    metadata_country_code: Option<String>,
    activity_log_retention_days: Option<u32>,
    cors_hosts: Option<Vec<String>>,
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
            min_resume_pct: 0,
            max_resume_pct: 90,
            min_resume_duration_seconds: 0,
            preferred_metadata_language: default_metadata_language(),
            metadata_country_code: default_metadata_country(),
            activity_log_retention_days: default_activity_retention(),
            cors_hosts: Vec::new(),
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
        min_resume_pct: input.min_resume_pct.unwrap_or(previous.min_resume_pct),
        max_resume_pct: input.max_resume_pct.unwrap_or(previous.max_resume_pct),
        min_resume_duration_seconds: input
            .min_resume_duration_seconds
            .unwrap_or(previous.min_resume_duration_seconds),
        preferred_metadata_language: input
            .preferred_metadata_language
            .map(|v| v.trim().to_owned())
            .unwrap_or(previous.preferred_metadata_language),
        metadata_country_code: input
            .metadata_country_code
            .map(|v| v.trim().to_ascii_uppercase())
            .unwrap_or(previous.metadata_country_code),
        activity_log_retention_days: input
            .activity_log_retention_days
            .unwrap_or(previous.activity_log_retention_days),
        cors_hosts: input
            .cors_hosts
            .map(|list| {
                list.into_iter()
                    .map(|h| h.trim().to_owned())
                    .filter(|h| !h.is_empty())
                    .collect()
            })
            .unwrap_or(previous.cors_hosts),
    };
    if !valid_resume(&input)
        || !valid_language(&input.preferred_metadata_language)
        || !valid_country(&input.metadata_country_code)
        || !valid_activity_retention(input.activity_log_retention_days)
        || !valid_cors_hosts(&input.cors_hosts)
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
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
    if let Ok(mut guard) = auth.store.cors_origins.write() {
        *guard = input.cors_hosts.clone();
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn ensure_cors_origins_warm(store: &IdentityStore) {
    let empty = store
        .cors_origins
        .read()
        .map(|g| g.is_empty())
        .unwrap_or(true);
    if empty
        && let Ok(settings) = load(store).await
        && let Ok(mut guard) = store.cors_origins.write()
    {
        *guard = settings.cors_hosts;
    }
}
fn valid_resume(s: &Settings) -> bool {
    (0..=49).contains(&s.min_resume_pct)
        && (50..=100).contains(&s.max_resume_pct)
        && s.min_resume_duration_seconds <= 86_400
}
/// BCP 47-style primary tag with optional region, e.g. r, pt-BR.
fn valid_language(v: &str) -> bool {
    let mut parts = v.split('-');
    let ok_primary = parts
        .next()
        .is_some_and(|p| (2..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_lowercase()));
    let ok_region = parts
        .next()
        .is_none_or(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_uppercase()));
    ok_primary && ok_region && parts.next().is_none()
}
fn valid_country(v: &str) -> bool {
    v.len() == 2 && v.chars().all(|c| c.is_ascii_uppercase())
}
fn valid_activity_retention(days: u32) -> bool {
    (1..=3650).contains(&days)
}
fn valid_cors_hosts(hosts: &[String]) -> bool {
    if hosts.len() > 50 {
        return false;
    }
    for host in hosts {
        if host.is_empty() || host.len() > 255 || host.chars().any(char::is_control) {
            return false;
        }
        if host == "*" {
            continue;
        }
        if !(host.starts_with("http://") || host.starts_with("https://")) {
            return false;
        }
        if let Ok(parsed) = url::Url::parse(host) {
            if parsed.path() != "" && parsed.path() != "/" {
                return false;
            }
            if parsed.query().is_some() || parsed.fragment().is_some() {
                return false;
            }
        } else {
            return false;
        }
    }
    true
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
    fn resume_and_metadata_defaults_and_validation() {
        let s = Settings::default();
        assert!(valid_resume(&s));
        assert_eq!(
            (
                s.min_resume_pct,
                s.max_resume_pct,
                s.min_resume_duration_seconds
            ),
            (0, 90, 0)
        );
        let legacy: Settings =
            serde_json::from_str(r#"{"serverName":"a","welcomeMessage":""}"#).unwrap();
        assert_eq!(legacy.preferred_metadata_language, "fr");
        assert!(
            valid_language("fr")
                && valid_language("pt-BR")
                && !valid_language("FR")
                && !valid_language("fr-br")
                && !valid_language("")
        );
        assert!(valid_country("US") && !valid_country("usa"));
        let bad = Settings {
            max_resume_pct: 30,
            ..Settings::default()
        };
        assert!(!valid_resume(&bad));
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

    #[test]
    fn activity_retention_and_cors_hosts_validation() {
        let s = Settings::default();
        assert_eq!(s.activity_log_retention_days, 30);
        assert!(s.cors_hosts.is_empty());
        assert!(valid_activity_retention(1));
        assert!(valid_activity_retention(30));
        assert!(valid_activity_retention(3650));
        assert!(!valid_activity_retention(0));
        assert!(!valid_activity_retention(3651));

        let valid_hosts = vec![
            "http://localhost:3000".to_string(),
            "https://app.example.com".to_string(),
            "*".to_string(),
        ];
        assert!(valid_cors_hosts(&valid_hosts));

        let invalid_proto = vec!["ftp://example.com".to_string()];
        assert!(!valid_cors_hosts(&invalid_proto));

        let invalid_path = vec!["http://example.com/foo".to_string()];
        assert!(!valid_cors_hosts(&invalid_path));

        let invalid_empty = vec!["".to_string()];
        assert!(!valid_cors_hosts(&invalid_empty));
    }
}
