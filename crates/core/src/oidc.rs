//! Confidential OIDC code flow. Provider tokens never leave this module.
use crate::{
    auth::{self, AuthState},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use url::Url;
use zeroize::Zeroizing;

type Error = (StatusCode, Json<serde_json::Value>);
fn deny(status: StatusCode, code: &str) -> Error {
    (status, Json(serde_json::json!({"error":code})))
}
fn invalid() -> Error {
    deny(StatusCode::UNAUTHORIZED, "oidc_invalid_response")
}
fn unavailable() -> Error {
    deny(StatusCode::BAD_GATEWAY, "oidc_provider_unavailable")
}
fn db(_: sqlx::Error) -> Error {
    deny(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

#[derive(Clone)]
struct Config {
    issuer: String,
    client_id: String,
    secret: Arc<Zeroizing<String>>,
    redirect: String,
    scopes: String,
    groups: Vec<String>,
    admin_groups: Vec<String>,
    groups_claim: String,
    require_group: bool,
}
impl Config {
    fn from_env(auth: &AuthState) -> Result<Option<Self>, &'static str> {
        let enabled = std::env::var("MEDIA_OIDC_ENABLED").unwrap_or_else(|_| "false".into());
        if enabled == "false" {
            return Ok(None);
        }
        if enabled != "true" {
            return Err("MEDIA_OIDC_ENABLED must be true or false");
        }
        let required = |name| {
            std::env::var(name)
                .ok()
                .filter(|s| !s.trim().is_empty())
                .ok_or("missing required MEDIA_OIDC configuration")
        };
        let issuer = required("MEDIA_OIDC_ISSUER")?;
        let url = endpoint(&issuer).map_err(|_| "OIDC issuer must be HTTPS (HTTP loopback only), without credentials, query or fragment")?;
        if url.query().is_some() {
            return Err("OIDC issuer cannot have a query");
        }

        let scopes = std::env::var("MEDIA_OIDC_SCOPES")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "openid profile email".to_string());

        let groups_claim = std::env::var("MEDIA_OIDC_GROUPS_CLAIM")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "groups".to_string());

        let require_group = std::env::var("MEDIA_OIDC_REQUIRE_GROUP")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(true);

        let groups: Vec<String> = std::env::var("MEDIA_OIDC_USER_GROUPS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();

        let admin_groups: Vec<String> = std::env::var("MEDIA_OIDC_ADMIN_GROUPS")
            .or_else(|_| std::env::var("MEDIA_OIDC_ADMIN_GROUP"))
            .or_else(|_| std::env::var("OIDC_ADMIN_GROUP"))
            .or_else(|_| std::env::var("OIDC_ADMIN_GROUPS"))
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();

        if require_group && groups.is_empty() && admin_groups.is_empty() {
            return Err(
                "MEDIA_OIDC_USER_GROUPS or MEDIA_OIDC_ADMIN_GROUPS must explicitly allow at least one group when OIDC is enabled. If you intentionally want to allow all users from this IdP without group filtering, explicitly set MEDIA_OIDC_REQUIRE_GROUP=false",
            );
        }

        Ok(Some(Self {
            issuer,
            client_id: required("MEDIA_OIDC_CLIENT_ID")?,
            secret: Arc::new(Zeroizing::new(required("MEDIA_OIDC_CLIENT_SECRET")?)),
            redirect: format!("{}/api/v1/auth/oidc/callback", auth.origin()),
            scopes,
            groups,
            admin_groups,
            groups_claim,
            require_group,
        }))
    }
}

fn endpoint(raw: &str) -> Result<Url, Error> {
    let u = Url::parse(raw).map_err(|_| invalid())?;
    if !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || !(u.scheme() == "https"
            || (u.scheme() == "http"
                && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        return Err(invalid());
    }
    Ok(u)
}

fn issuers_match(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

#[derive(Clone)]
struct Oidc {
    auth: AuthState,
    config: Option<Config>,
    http: reqwest::Client,
    workers: Arc<tokio::sync::Semaphore>,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    end_session_endpoint: Option<String>,
    id_token_signing_alg_values_supported: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Vec<String>,
}

#[derive(Deserialize)]
struct Tokens {
    id_token: String,
}

#[derive(Clone, Deserialize)]
struct Claims {
    sub: String,
    nonce: String,
    iat: u64,
    exp: u64,
    aud: serde_json::Value,
    #[serde(default)]
    azp: Option<String>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl Claims {
    fn extract_groups(&self, claim_name: &str) -> Vec<String> {
        let Some(val) = self.extra.get(claim_name) else {
            return Vec::new();
        };
        match val {
            serde_json::Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect(),
            serde_json::Value::String(s) => s
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn username(&self) -> Option<String> {
        for key in [
            "preferred_username",
            "username",
            "nickname",
            "upn",
            "unique_name",
        ] {
            if let Some(val) = self.extra.get(key).and_then(|v| v.as_str()) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && trimmed.chars().count() <= 128 {
                    return Some(trimmed.to_string());
                }
            }
        }
        if let Some(email) = self.extra.get("email").and_then(|v| v.as_str())
            && let Some((local, _)) = email.split_once('@')
        {
            let trimmed = local.trim();
            if !trimmed.is_empty() && trimmed.chars().count() <= 128 {
                return Some(trimmed.to_string());
            }
        }
        None
    }

    fn display_name(&self) -> Option<String> {
        for key in ["name", "display_name"] {
            if let Some(val) = self.extra.get(key).and_then(|v| v.as_str()) {
                let trimmed = val.trim();
                if !trimmed.is_empty() && trimmed.chars().count() <= 128 {
                    return Some(trimmed.to_string());
                }
            }
        }
        if let (Some(g), Some(f)) = (
            self.extra.get("given_name").and_then(|v| v.as_str()),
            self.extra.get("family_name").and_then(|v| v.as_str()),
        ) {
            let full = format!("{} {}", g.trim(), f.trim()).trim().to_string();
            if !full.is_empty() && full.chars().count() <= 128 {
                return Some(full);
            }
        }
        self.username()
    }
}

#[derive(Deserialize)]
struct Callback {
    state: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct Link {
    password: String,
}

#[derive(sqlx::FromRow)]
struct Flow {
    browser_hash: String,
    nonce: String,
    verifier: String,
    link_session_hash: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    enabled: bool,
    issuer: Option<String>,
    client_id: Option<String>,
    redirect_uri: Option<String>,
    scopes: Option<String>,
    groups_claim: Option<String>,
    require_group: bool,
    user_groups: Vec<String>,
    admin_groups: Vec<String>,
    end_session_endpoint: Option<String>,
    local_login_enabled: bool,
}

pub fn router(auth: AuthState) -> Result<Router, Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::from_env(&auth).map_err(std::io::Error::other)?;
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(4))
        .build()?;
    Ok(routes(Oidc {
        auth,
        config,
        http,
        workers: Arc::new(tokio::sync::Semaphore::new(8)),
    }))
}

fn routes(state: Oidc) -> Router {
    Router::new()
        .route("/api/v1/auth/oidc/status", get(public_status))
        .route(
            "/api/v1/auth/oidc/complete.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    "window.location.replace('/app/');",
                )
            }),
        )
        .route("/api/v1/admin/oidc", get(admin_status))
        .route("/api/v1/auth/oidc/start", post(start))
        .route("/api/v1/auth/oidc/link", post(link))
        .route("/api/v1/auth/oidc/callback", get(callback))
        .route("/api/v1/auth/oidc/logout", get(logout))
        .layer(axum::extract::DefaultBodyLimit::max(4096))
        .with_state(state)
}

async fn public_status(State(s): State<Oidc>) -> Result<Json<serde_json::Value>, Error> {
    let button_label = crate::server_settings::load(&s.auth.store)
        .await
        .map_err(|status| deny(status, "internal_error"))?
        .sso_button_label;
    let local_login_enabled = crate::auth::is_local_login_enabled();
    if let Some(c) = &s.config {
        let issuer_lower = c.issuer.to_lowercase();
        let provider = if issuer_lower.contains("authentik") {
            "Authentik"
        } else if issuer_lower.contains("keycloak") {
            "Keycloak"
        } else if issuer_lower.contains("dex") {
            "Dex"
        } else if issuer_lower.contains("pocket-id") || issuer_lower.contains("pocketid") {
            "Pocket ID"
        } else {
            "SSO"
        };
        Ok(Json(serde_json::json!({
            "enabled": true,
            "provider": provider,
            "localLoginEnabled": local_login_enabled,
            "buttonLabel": button_label
        })))
    } else {
        Ok(Json(serde_json::json!({
            "enabled": false,
            "localLoginEnabled": local_login_enabled,
            "buttonLabel": button_label
        })))
    }
}

async fn admin_status(State(s): State<Oidc>, headers: HeaderMap) -> Result<Json<Status>, Error> {
    let identity = s
        .auth
        .authenticate(&headers)
        .await
        .map_err(|st| deny(st, "authentication_required"))?;
    if identity.user.role != Role::Administrator {
        return Err(deny(StatusCode::FORBIDDEN, "forbidden"));
    }

    let end_session_endpoint = if s.config.is_some() {
        s.discover().await.ok().and_then(|d| d.end_session_endpoint)
    } else {
        None
    };

    Ok(Json(Status {
        enabled: s.config.is_some(),
        issuer: s.config.as_ref().map(|c| c.issuer.clone()),
        client_id: s.config.as_ref().map(|c| c.client_id.clone()),
        redirect_uri: s.config.as_ref().map(|c| c.redirect.clone()),
        scopes: s.config.as_ref().map(|c| c.scopes.clone()),
        groups_claim: s.config.as_ref().map(|c| c.groups_claim.clone()),
        require_group: s.config.as_ref().is_some_and(|c| c.require_group),
        user_groups: s
            .config
            .as_ref()
            .map(|c| c.groups.clone())
            .unwrap_or_default(),
        admin_groups: s
            .config
            .as_ref()
            .map(|c| c.admin_groups.clone())
            .unwrap_or_default(),
        end_session_endpoint,
        local_login_enabled: crate::auth::is_local_login_enabled(),
    }))
}

const SUPPORTED_ALGS: &[Algorithm] = &[
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::ES256,
    Algorithm::ES384,
    Algorithm::EdDSA,
];

impl Oidc {
    fn config(&self) -> Result<&Config, Error> {
        self.config
            .as_ref()
            .ok_or_else(|| deny(StatusCode::NOT_FOUND, "oidc_disabled"))
    }

    async fn json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let mut response = request
            .send()
            .await
            .map_err(|_| unavailable())?
            .error_for_status()
            .map_err(|_| unavailable())?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if body.len() + chunk.len() > 262144 {
                return Err(unavailable());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| unavailable())
    }

    async fn discover(&self) -> Result<Discovery, Error> {
        let c = self.config()?;
        let d: Discovery = self
            .json(self.http.get(format!(
                "{}/.well-known/openid-configuration",
                c.issuer.trim_end_matches('/')
            )))
            .await?;

        if !issuers_match(&d.issuer, &c.issuer) {
            return Err(invalid());
        }

        let has_supported_alg = d
            .id_token_signing_alg_values_supported
            .iter()
            .any(|a| SUPPORTED_ALGS.iter().any(|sa| format!("{sa:?}") == *a));
        if !has_supported_alg {
            return Err(invalid());
        }

        let issuer_url = endpoint(&c.issuer)?;
        let mut check_endpoints = vec![&d.authorization_endpoint, &d.token_endpoint, &d.jwks_uri];
        if let Some(ref ese) = d.end_session_endpoint {
            check_endpoints.push(ese);
        }

        for raw in check_endpoints {
            let ep = endpoint(raw)?;
            // Endpoints must share origin or be a valid secure endpoint
            if ep.origin() != issuer_url.origin() {
                return Err(invalid());
            }
        }
        Ok(d)
    }

    async fn begin(
        &self,
        headers: &HeaderMap,
        link_session: Option<String>,
    ) -> Result<Response, Error> {
        self.auth
            .check_origin(headers)
            .map_err(|s| deny(s, "forbidden"))?;
        let _permit = self
            .workers
            .try_acquire()
            .map_err(|_| deny(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
        let c = self.config()?;
        let rescue: i64 = crate::database::query_scalar("SELECT CAST(EXISTS(SELECT 1 FROM users u JOIN local_credentials c ON c.user_id=u.id WHERE u.role='administrator' AND u.disabled_at IS NULL) AS INTEGER)")
            .fetch_one(&self.auth.store.pool).await.map_err(db)?;
        let has_any_admin: i64 = crate::database::query_scalar("SELECT CAST(EXISTS(SELECT 1 FROM users WHERE role='administrator' AND disabled_at IS NULL) AS INTEGER)")
            .fetch_one(&self.auth.store.pool).await.map_err(db)?;
        if rescue == 0 && has_any_admin == 0 && c.admin_groups.is_empty() {
            return Err(deny(StatusCode::FORBIDDEN, "local_administrator_required"));
        }
        let d = self.discover().await?;
        let state = auth::random_token();
        let browser = auth::random_token();
        let nonce = auth::random_token();
        let verifier = auth::random_token();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        crate::database::query("DELETE FROM oidc_flows WHERE expires_at<=unixepoch()")
            .execute(&self.auth.store.pool)
            .await
            .map_err(db)?;
        let result = crate::database::query("INSERT INTO oidc_flows(state_hash,browser_hash,nonce,verifier,link_session_hash,expires_at) SELECT ?,?,?,?,?,unixepoch()+300 WHERE (SELECT count(*) FROM oidc_flows)<256")
            .bind(auth::hash(&state)).bind(auth::hash(&browser)).bind(&nonce).bind(&verifier).bind(link_session).execute(&self.auth.store.pool).await.map_err(db)?;
        if result.rows_affected() != 1 {
            return Err(deny(StatusCode::TOO_MANY_REQUESTS, "try_later"));
        }
        let mut url = endpoint(&d.authorization_endpoint)?;
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("scope", c.scopes.as_str()),
            ("client_id", &c.client_id),
            ("redirect_uri", &c.redirect),
            ("state", &state),
            ("nonce", &nonce),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
        ]);
        let mut response = Json(serde_json::json!({"url":url.as_str()})).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&self.flow_cookie(&browser, 300)).map_err(|_| invalid())?,
        );
        Ok(response)
    }

    fn flow_cookie(&self, raw: &str, age: u32) -> String {
        format!(
            "media_oidc_flow={raw}; Path=/api/v1/auth/oidc; HttpOnly; SameSite=Lax; Max-Age={age}{}",
            if self.auth.secure { "; Secure" } else { "" }
        )
    }

    async fn claims(&self, d: &Discovery, token: &str, nonce: &str) -> Result<Claims, Error> {
        let c = self.config()?;
        let h = jsonwebtoken::decode_header(token).map_err(|_| invalid())?;

        // Strict rejection of alg "none" or unsupported algorithms
        if !SUPPORTED_ALGS.contains(&h.alg) {
            return Err(invalid());
        }

        let kid = h.kid.ok_or_else(invalid)?;
        // Fresh JWKS on each callback: rotation is immediate, no stale-key acceptance.
        let keys: JwkSet = self.json(self.http.get(&d.jwks_uri)).await?;
        let jwk = keys.find(&kid).ok_or_else(invalid)?;
        let key = DecodingKey::try_from(jwk).map_err(|_| invalid())?;
        let mut validation = Validation::new(h.alg);
        validation.set_issuer(&[&c.issuer, c.issuer.trim_end_matches('/')]);
        validation.set_audience(&[&c.client_id]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub", "iat"]);
        validation.leeway = 0;
        validation.validate_nbf = true;
        let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map_err(|_| invalid())?
            .claims;
        let now = jsonwebtoken::get_current_timestamp();
        if claims.nonce != nonce
            || claims.sub.is_empty()
            || claims.sub.len() > 255
            || claims.iat > now + 30
            || claims.exp <= now
            || claims.azp.as_ref().is_some_and(|v| v != &c.client_id)
            || (claims.aud.as_array().is_some_and(|a| a.len() > 1)
                && claims.azp.as_deref() != Some(&c.client_id))
        {
            return Err(invalid());
        }
        Ok(claims)
    }

    async fn finish(&self, headers: &HeaderMap, q: Callback) -> Result<Response, Error> {
        let _permit = self
            .workers
            .try_acquire()
            .map_err(|_| deny(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
        let c = self.config()?;
        if q.state.len() != 64 {
            return Err(invalid());
        }
        let browser = flow_token(headers).ok_or_else(invalid)?;
        // Atomic consume, bound to browser and deadline, even for provider errors.
        let flow: Flow = crate::database::query_as("DELETE FROM oidc_flows WHERE state_hash=? AND browser_hash=? AND expires_at>unixepoch() RETURNING browser_hash,nonce,verifier,link_session_hash")
            .bind(auth::hash(&q.state)).bind(auth::hash(browser)).fetch_optional(&self.auth.store.pool).await.map_err(db)?.ok_or_else(invalid)?;
        if flow.browser_hash != auth::hash(browser) || q.error.is_some() {
            return Err(invalid());
        }
        let code = q
            .code
            .filter(|s| !s.is_empty() && s.len() < 4096)
            .ok_or_else(invalid)?;
        let d = self.discover().await?;

        // Client authentication negotiation at token endpoint:
        // If provider advertises token_endpoint_auth_methods_supported containing client_secret_post
        // and NOT client_secret_basic, use POST body authentication.
        // Otherwise, default to standard client_secret_basic (RFC 6749 2.3.1).
        let prefers_post = d
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "client_secret_post")
            && !d
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|m| m == "client_secret_basic");

        let mut req = self.http.post(&d.token_endpoint);
        let mut form_params = vec![
            ("grant_type", "authorization_code".to_string()),
            ("code", code),
            ("redirect_uri", c.redirect.clone()),
            ("code_verifier", flow.verifier.clone()),
        ];

        if prefers_post {
            form_params.push(("client_id", c.client_id.clone()));
            form_params.push(("client_secret", c.secret.as_str().to_string()));
        } else {
            req = req.basic_auth(&c.client_id, Some(c.secret.as_str()));
        }

        let tokens: Tokens = self.json(req.form(&form_params)).await?;
        let claims = self.claims(&d, &tokens.id_token, &flow.nonce).await?;

        let extracted_groups = claims.extract_groups(&c.groups_claim);
        if c.require_group {
            let in_user_groups = extracted_groups
                .iter()
                .any(|g| c.groups.iter().any(|ug| ug.eq_ignore_ascii_case(g)));
            let in_admin_groups = !c.admin_groups.is_empty()
                && extracted_groups
                    .iter()
                    .any(|g| c.admin_groups.iter().any(|ag| ag.eq_ignore_ascii_case(g)));
            if !in_user_groups && !in_admin_groups {
                if let Ok(Some(existing_id)) = crate::database::query_scalar::<_, String>(
                    "SELECT user_id FROM oidc_identities WHERE issuer=? AND subject=?",
                )
                .bind(&c.issuer)
                .bind(&claims.sub)
                .fetch_optional(&self.auth.store.pool)
                .await
                {
                    let _ = crate::database::query("DELETE FROM sessions WHERE user_id=?")
                        .bind(&existing_id)
                        .execute(&self.auth.store.pool)
                        .await;
                }
                return Err(deny(StatusCode::FORBIDDEN, "oidc_group_denied"));
            }
        }
        let is_admin = !c.admin_groups.is_empty()
            && extracted_groups
                .iter()
                .any(|g| c.admin_groups.iter().any(|ag| ag.eq_ignore_ascii_case(g)));
        let role = if is_admin { "administrator" } else { "user" };
        let display_name = claims.display_name();
        let sso_username = claims.username();

        let mut conn = self.auth.store.pool.acquire().await.map_err(db)?;
        let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
        let linked_user: Option<String> = if let Some(ref session_hash) = flow.link_session_hash {
            Some(crate::database::query_scalar("SELECT s.user_id FROM sessions s JOIN users u ON u.id=s.user_id JOIN local_credentials c ON c.user_id=u.id WHERE s.token_hash=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-604800 AND u.disabled_at IS NULL")
                .bind(session_hash).fetch_optional(&mut *tx).await.map_err(db)?
                .ok_or_else(|| deny(StatusCode::UNAUTHORIZED,"link_session_expired"))?)
        } else {
            None
        };
        let existing: Option<String> = crate::database::query_scalar(
            "SELECT user_id FROM oidc_identities WHERE issuer=? AND subject=?",
        )
        .bind(&c.issuer)
        .bind(&claims.sub)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let user = match (existing, linked_user) {
            (Some(id), Some(link)) if id != link => {
                return Err(deny(StatusCode::CONFLICT, "oidc_identity_already_linked"));
            }
            (Some(id), _) => {
                let has_local_cred: bool = crate::database::query_scalar::<_, i64>(
                    "SELECT CAST(EXISTS(SELECT 1 FROM local_credentials WHERE user_id=?) AS INTEGER)",
                )
                .bind(&id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)? != 0;

                let current_role: String =
                    crate::database::query_scalar("SELECT role FROM users WHERE id=?")
                        .bind(&id)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(db)?;

                if is_admin && !has_local_cred {
                    crate::database::query("UPDATE users SET role='administrator', display_name=COALESCE(?, display_name), sso_username=COALESCE(?, sso_username) WHERE id=?")
                        .bind(&display_name)
                        .bind(&sso_username)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                } else if !has_local_cred
                    && !c.admin_groups.is_empty()
                    && current_role == "administrator"
                {
                    // Downgrade: SSO-managed user lost their admin group membership in IdP.
                    crate::database::query(
                        "UPDATE users SET role='user', display_name=COALESCE(?, display_name), sso_username=COALESCE(?, sso_username) WHERE id=?",
                    )
                    .bind(&display_name)
                    .bind(&sso_username)
                    .bind(&id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;

                    // Immediately revoke all existing sessions to invalidate stale administrator privileges.
                    crate::database::query("DELETE FROM sessions WHERE user_id=?")
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .map_err(db)?;
                } else {
                    crate::database::query(
                        "UPDATE users SET display_name=COALESCE(?, display_name), sso_username=COALESCE(?, sso_username) WHERE id=?",
                    )
                    .bind(&display_name)
                    .bind(&sso_username)
                    .bind(&id)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                }
                id
            }
            (None, target) => {
                let id = target.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                crate::database::query(
                    "INSERT INTO users(id,role,display_name,sso_username) VALUES (?,?,?,?) ON CONFLICT(id) DO UPDATE SET display_name=COALESCE(excluded.display_name, users.display_name), sso_username=COALESCE(excluded.sso_username, users.sso_username)",
                )
                .bind(&id)
                .bind(role)
                .bind(&display_name)
                .bind(&sso_username)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
                let result = crate::database::query(
                    "INSERT INTO oidc_identities(issuer,subject,user_id) VALUES (?,?,?)",
                )
                .bind(&c.issuer)
                .bind(&claims.sub)
                .bind(&id)
                .execute(&mut *tx)
                .await;
                if result.is_err() {
                    return Err(deny(StatusCode::CONFLICT, "oidc_identity_already_linked"));
                }
                id
            }
        };
        let raw = auth::random_token();
        let created=crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+2592000 FROM users WHERE id=? AND disabled_at IS NULL").bind(auth::hash(&raw)).bind(&user).execute(&mut *tx).await.map_err(db)?;
        crate::database::query("UPDATE sessions SET device_label=? WHERE token_hash=?")
            .bind(auth::device_label(headers))
            .bind(auth::hash(&raw))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        if created.rows_affected() != 1 {
            return Err(deny(StatusCode::FORBIDDEN, "oidc_account_disabled"));
        }
        // Rotate the original local session after linking; local credential is preserved.
        if let Some(old) = flow.link_session_hash {
            crate::database::query("DELETE FROM sessions WHERE token_hash=?")
                .bind(old)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        tx.commit().await.map_err(db)?;
        if crate::extensions::collections::assign_new_user(&self.auth, &user)
            .await
            .is_err()
        {
            eprintln!("Automatic avatar assignment failed");
        }
        // End the cross-site redirect chain on a Core document. A same-origin
        // external script starts a new navigation so Strict session cookies and
        // the existing cross-site guards both remain effective on /app/.
        let mut response = Html(include_str!("oidc_complete.html")).into_response();
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&self.auth.cookie(&raw, auth::ABSOLUTE_SECONDS))
                .map_err(|_| invalid())?,
        );
        Ok(response)
    }

    async fn logout_url(&self) -> Result<Option<String>, Error> {
        let d = self.discover().await?;
        let Some(ese) = d.end_session_endpoint else {
            return Ok(None);
        };
        let mut url = endpoint(&ese)?;
        url.query_pairs_mut()
            .append_pair("post_logout_redirect_uri", self.auth.origin());
        Ok(Some(url.to_string()))
    }
}

async fn logout(State(s): State<Oidc>) -> Result<Json<serde_json::Value>, Error> {
    let url = s.logout_url().await?;
    Ok(Json(serde_json::json!({ "endSessionUrl": url })))
}
fn flow_token(headers: &HeaderMap) -> Option<&str> {
    let mut found = None;
    for h in headers.get_all(header::COOKIE) {
        for pair in h.to_str().ok()?.split(';') {
            if let Some(("media_oidc_flow", v)) = pair.trim().split_once('=') {
                if found.is_some() || v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                found = Some(v);
            }
        }
    }
    found
}
async fn start(State(s): State<Oidc>, headers: HeaderMap) -> Result<Response, Error> {
    s.begin(&headers, None).await
}
async fn link(
    State(s): State<Oidc>,
    headers: HeaderMap,
    Json(input): Json<Link>,
) -> Result<Response, Error> {
    use argon2::{
        Argon2,
        password_hash::{PasswordVerifier, phc::PasswordHash},
    };
    let identity = s
        .auth
        .authenticate(&headers)
        .await
        .map_err(|st| deny(st, "authentication_required"))?;
    s.auth
        .require_csrf(&headers, &identity)
        .map_err(|st| deny(st, "forbidden"))?;
    if input.password.len() > 512 {
        return Err(invalid());
    }
    // Reuse local login's persistent client rate limit before expensive Argon2 work.
    let client_key = auth::extract_client_key(&headers);
    let allowed = crate::database::query_scalar::<_, i64>(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES(?,unixepoch(),1) ON CONFLICT(throttle_key) DO UPDATE SET attempts=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 ELSE login_throttle.attempts+1 END, window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN unixepoch() ELSE login_throttle.window_started END RETURNING login_throttle.attempts",
    )
    .bind(&client_key)
    .fetch_one(&s.auth.store.pool)
    .await
    .map_err(db)?;
    if allowed > 10 {
        return Err(deny(StatusCode::TOO_MANY_REQUESTS, "try_later"));
    }
    let permit = s
        .auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| deny(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
    let encoded: String = crate::database::query_scalar(
        "SELECT password_hash FROM local_credentials WHERE user_id=?",
    )
    .bind(&identity.user.id)
    .fetch_optional(&s.auth.store.pool)
    .await
    .map_err(db)?
    .ok_or_else(invalid)?;
    let password = Zeroizing::new(input.password);
    let ok = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&encoded).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !ok {
        return Err(invalid());
    }
    s.begin(&headers, Some(identity.token_hash)).await
}
async fn callback(
    State(s): State<Oidc>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Response {
    let mut response = match s.finish(&headers, q).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    if let Ok(v) = HeaderValue::from_str(&s.flow_cookie("", 0)) {
        response.headers_mut().append(header::SET_COOKIE, v);
    }
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::Form, http::Request};
    use http_body_util::BodyExt;
    use std::{collections::HashMap, sync::Mutex};
    use tower::ServiceExt;
    const ORIGIN: &str = "http://127.0.0.1:3000";
    const PASSWORD: &str = "synthetic local rescue password";
    #[derive(Clone)]
    struct Provider {
        issuer: String,
        claims: Arc<Mutex<serde_json::Value>>,
        key: Arc<std::sync::atomic::AtomicUsize>,
        exchanges: Arc<std::sync::atomic::AtomicUsize>,
    }
    struct Fixture {
        _temp: tempfile::TempDir,
        s: Oidc,
        provider: Provider,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    async fn fixture() -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!(
            "http://{}/application/o/media/",
            listener.local_addr().unwrap()
        );
        let provider = Provider {
            issuer: issuer.clone(),
            claims: Arc::new(Mutex::new(serde_json::json!({}))),
            key: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
            exchanges: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        let app=Router::new().route("/application/o/media/.well-known/openid-configuration",get(|State(p): State<Provider>| async move { let base=Url::parse(&p.issuer).unwrap().origin().ascii_serialization(); Json(serde_json::json!({"issuer":p.issuer,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"jwks_uri":format!("{base}/jwks"),"id_token_signing_alg_values_supported":["RS256"]})) }))
            .route("/jwks",get(|State(p): State<Provider>| async move { Json(serde_json::from_str::<serde_json::Value>(if p.key.load(std::sync::atomic::Ordering::SeqCst)==1 { include_str!("../tests/fixtures/oidc/jwks1.json") } else { include_str!("../tests/fixtures/oidc/jwks2.json") }).unwrap()) }))
            .route("/token",post(|State(p): State<Provider>, headers: HeaderMap, Form(form): Form<HashMap<String,String>>| async move {
                assert_eq!(headers[header::AUTHORIZATION],format!("Basic {}",base64::engine::general_purpose::STANDARD.encode("client:synthetic-secret")));
                assert_eq!(form["grant_type"],"authorization_code"); assert_eq!(form["redirect_uri"],format!("{ORIGIN}/api/v1/auth/oidc/callback")); assert_eq!(form["code_verifier"].len(),64);
                p.exchanges.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
                let i=p.key.load(std::sync::atomic::Ordering::SeqCst);
                let key=jsonwebtoken::EncodingKey::from_rsa_pem(if i==1 { include_bytes!("../tests/fixtures/oidc/key1.pem") } else { include_bytes!("../tests/fixtures/oidc/key2.pem") }).unwrap();
                let mut h=jsonwebtoken::Header::new(Algorithm::RS256);h.kid=Some(format!("test-{i}"));
                let token=jsonwebtoken::encode(&h,&*p.claims.lock().unwrap(),&key).unwrap();
                Json(serde_json::json!({"id_token":token,"access_token":"never-exposed","refresh_token":"never-stored"}))
            })).with_state(provider.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let temp = tempfile::tempdir().unwrap();
        // Test-only opt-in: the same provider/security cases run against PostgreSQL.
        // Each fixture receives an isolated schema in the disposable test database.
        let store = if let Ok(raw_url) = std::env::var("MEDIA_TEST_POSTGRES_URL") {
            sqlx::any::install_default_drivers();
            let admin = sqlx::AnyPool::connect(&raw_url).await.unwrap();
            let schema = format!("oidc_{}", uuid::Uuid::new_v4().simple());
            crate::database::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
                .execute(&admin)
                .await
                .unwrap();
            admin.close().await;
            let mut url = url::Url::parse(&raw_url).unwrap();
            url.query_pairs_mut()
                .append_pair("options", &format!("-csearch_path={schema}"));
            crate::identity::IdentityStore::open_postgres(url.as_str())
                .await
                .unwrap()
        } else {
            crate::identity::IdentityStore::open(&temp.path().join("core.sqlite"))
                .await
                .unwrap()
        };
        store
            .bootstrap("rescue", Zeroizing::new(PASSWORD.into()))
            .await
            .unwrap();
        let auth = AuthState::new(store, ORIGIN.into()).await.unwrap();
        let c = Config {
            issuer,
            client_id: "client".into(),
            secret: Arc::new(Zeroizing::new("synthetic-secret".into())),
            redirect: format!("{ORIGIN}/api/v1/auth/oidc/callback"),
            scopes: "openid profile email".into(),
            groups: vec!["media-users".into()],
            admin_groups: vec![],
            groups_claim: "groups".into(),
            require_group: true,
        };
        let s = Oidc {
            auth,
            config: Some(c),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            workers: Arc::new(tokio::sync::Semaphore::new(8)),
        };
        Fixture {
            _temp: temp,
            s,
            provider,
            task,
        }
    }
    #[tokio::test]
    async fn public_status_reads_persisted_button_label_without_exposing_settings() {
        let f = fixture().await;
        crate::database::query("INSERT INTO server_settings(singleton,settings_json) VALUES(1,?)")
            .bind(serde_json::json!({"serverName":"Private server name","welcomeMessage":"Private welcome","ssoButtonLabel":"Connexion avec mon compte"}).to_string())
            .execute(&f.s.auth.store.pool).await.unwrap();
        let value = body(
            req(
                &f.s,
                "/api/v1/auth/oidc/status",
                "GET",
                "",
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(value["buttonLabel"], "Connexion avec mon compte");
        assert!(value.get("serverName").is_none());
        assert!(value.get("welcomeMessage").is_none());
    }

    async fn body(r: Response) -> serde_json::Value {
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }
    async fn req(
        s: &Oidc,
        path: &str,
        method: &str,
        cookie: &str,
        csrf: &str,
        data: serde_json::Value,
    ) -> Response {
        routes(s.clone())
            .merge(auth::router(s.auth.clone()))
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method(method)
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::COOKIE, cookie)
                    .header("x-csrf-token", csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(data.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn begin(f: &Fixture) -> (String, String) {
        let r = req(
            &f.s,
            "/api/v1/auth/oidc/start",
            "POST",
            "",
            "",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let cookie = r.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let data = body(r).await;
        let url = Url::parse(data["url"].as_str().unwrap()).unwrap();
        let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(pairs["response_type"], "code");
        let nonce = pairs["nonce"].clone();
        let now = jsonwebtoken::get_current_timestamp();
        *f.provider.claims.lock().unwrap() = serde_json::json!({"iss":f.provider.issuer,"sub":"subject-1","aud":"client","iat":now,"exp":now+300,"nonce":nonce,"groups":["media-users"],"email":"rescue@example.test","role":"administrator"});
        let verifier: String =
            crate::database::query_scalar("SELECT verifier FROM oidc_flows WHERE state_hash=?")
                .bind(auth::hash(&pairs["state"]))
                .fetch_one(&f.s.auth.store.pool)
                .await
                .unwrap();
        assert_eq!(
            pairs["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
        );
        (cookie, pairs["state"].clone())
    }
    async fn finish(f: &Fixture, cookie: &str, state: &str) -> Response {
        req(
            &f.s,
            &format!("/api/v1/auth/oidc/callback?state={state}&code=synthetic-code"),
            "GET",
            cookie,
            "",
            serde_json::Value::Null,
        )
        .await
    }
    fn session_cookie(r: &Response) -> String {
        r.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|h| h.to_str().ok())
            .find(|v| v.starts_with("media_session="))
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned()
    }
    #[tokio::test]
    async fn oidc_sessions_rotation_roles_disabled_email_collision_and_local_regression() {
        let f = fixture().await;
        let (cookie, state) = begin(&f).await;
        let r = finish(&f, &cookie, &state).await;
        assert_eq!(r.status(), StatusCode::OK);
        let session = session_cookie(&r);
        assert!(
            r.headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|v| v.to_str().unwrap().contains("SameSite=Strict"))
        );
        let me = body(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &session,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(me["user"]["role"], "user");
        let id = me["user"]["id"].as_str().unwrap();
        let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            StatusCode::UNAUTHORIZED
        );
        f.provider.key.store(2, std::sync::atomic::Ordering::SeqCst);
        let (cookie, state) = begin(&f).await;
        let r = finish(&f, &cookie, &state).await;
        assert_eq!(r.status(), StatusCode::OK);
        let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
        crate::database::query("UPDATE users SET role='administrator' WHERE id=?")
            .bind(id)
            .execute(&f.s.auth.store.pool)
            .await
            .unwrap();
        let now = body(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &session,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(now["user"]["role"], "administrator");
        crate::database::query("UPDATE users SET role='user',disabled_at=unixepoch() WHERE id=?")
            .bind(id)
            .execute(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &session,
                "",
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let (cookie, state) = begin(&f).await;
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            StatusCode::FORBIDDEN
        );
        let r = req(
            &f.s,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            serde_json::json!({"login":"rescue","password":PASSWORD}),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let local = session_cookie(&r);
        let data = body(r).await;
        let csrf = data["csrfToken"].as_str().unwrap();
        let status = body(
            req(
                &f.s,
                "/api/v1/admin/oidc",
                "GET",
                &local,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(status["enabled"], true);
        assert!(!status.to_string().contains("synthetic-secret"));
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/logout",
                "POST",
                &local,
                "bad",
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/logout",
                "POST",
                &local,
                csrf,
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &local,
                "",
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn oidc_rejects_invalid_state_browser_nonce_signature_claims_and_groups() {
        let f = fixture().await;
        let (cookie, state) = begin(&f).await;
        assert_eq!(
            finish(&f, "", &state).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            finish(&f, &cookie, &"0".repeat(64)).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            f.provider
                .exchanges
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        for (field, value) in [
            ("nonce", serde_json::json!("bad")),
            ("iss", serde_json::json!("https://evil.test/")),
            ("aud", serde_json::json!("other")),
            ("exp", serde_json::json!(1)),
            ("iat", serde_json::json!(99999999999u64)),
            ("azp", serde_json::json!("other")),
            ("groups", serde_json::json!(["administrators"])),
        ] {
            let (cookie, state) = begin(&f).await;
            f.provider.claims.lock().unwrap()[field] = value;
            let expected = if field == "groups" {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            };
            assert_eq!(
                finish(&f, &cookie, &state).await.status(),
                expected,
                "{field}"
            );
        }
        let (cookie, state) = begin(&f).await;
        let exchanges = f
            .provider
            .exchanges
            .load(std::sync::atomic::Ordering::SeqCst);
        let denied = req(
            &f.s,
            &format!("/api/v1/auth/oidc/callback?state={state}&error=access_denied"),
            "GET",
            &cookie,
            "",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            f.provider
                .exchanges
                .load(std::sync::atomic::Ordering::SeqCst),
            exchanges
        );
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let (cookie, state) = begin(&f).await;
        crate::database::query("UPDATE oidc_flows SET expires_at=0")
            .execute(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let (cookie, state) = begin(&f).await;
        f.task.abort();
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            StatusCode::BAD_GATEWAY
        );
    }
    #[tokio::test]
    async fn oidc_admin_group_does_not_promote_a_linked_local_user() {
        let mut f = fixture().await;
        f.s.config.as_mut().unwrap().admin_groups = vec!["media-admins".into()];
        crate::database::query("INSERT INTO users(id,role) VALUES('local-user','user')")
            .execute(&f.s.auth.store.pool)
            .await
            .unwrap();
        crate::database::query("INSERT INTO local_credentials(user_id,login,password_hash) SELECT 'local-user','local-user',password_hash FROM local_credentials WHERE login='rescue'")
            .execute(&f.s.auth.store.pool).await.unwrap();
        let login = req(
            &f.s,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            serde_json::json!({"login":"local-user","password":PASSWORD}),
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = session_cookie(&login);
        let csrf = body(login).await["csrfToken"].as_str().unwrap().to_owned();
        let link = req(
            &f.s,
            "/api/v1/auth/oidc/link",
            "POST",
            &cookie,
            &csrf,
            serde_json::json!({"password":PASSWORD}),
        )
        .await;
        assert_eq!(link.status(), StatusCode::OK);
        let browser = link.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let url = Url::parse(body(link).await["url"].as_str().unwrap()).unwrap();
        let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let now = jsonwebtoken::get_current_timestamp();
        *f.provider.claims.lock().unwrap() = serde_json::json!({"iss":f.provider.issuer,"sub":"local-subject","aud":"client","iat":now,"exp":now+300,"nonce":pairs["nonce"],"groups":["media-admins"]});
        let linked = finish(&f, &browser, &pairs["state"]).await;
        assert_eq!(linked.status(), StatusCode::OK);
        let role: String =
            crate::database::query_scalar("SELECT role FROM users WHERE id='local-user'")
                .fetch_one(&f.s.auth.store.pool)
                .await
                .unwrap();
        assert_eq!(role, "user");

        let (browser, state) = begin(&f).await;
        f.provider.claims.lock().unwrap()["sub"] = serde_json::json!("local-subject");
        f.provider.claims.lock().unwrap()["groups"] = serde_json::json!(["media-admins"]);
        assert_eq!(finish(&f, &browser, &state).await.status(), StatusCode::OK);
        let role: String =
            crate::database::query_scalar("SELECT role FROM users WHERE id='local-user'")
                .fetch_one(&f.s.auth.store.pool)
                .await
                .unwrap();
        assert_eq!(role, "user");
    }

    #[tokio::test]
    async fn oidc_link_requires_local_proof_and_retains_rescue_account() {
        let f = fixture().await;
        let r = req(
            &f.s,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            serde_json::json!({"login":"rescue","password":PASSWORD}),
        )
        .await;
        let local = session_cookie(&r);
        let data = body(r).await;
        let csrf = data["csrfToken"].as_str().unwrap();
        let id = data["user"]["id"].as_str().unwrap();
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/oidc/link",
                "POST",
                &local,
                csrf,
                serde_json::json!({"password":"wrong"})
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/oidc/link",
                "POST",
                &local,
                "bad",
                serde_json::json!({"password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let (cookie, state) = begin(&f).await;
        // Real link endpoint supplies an original session hash after password reauthentication.
        let r = req(
            &f.s,
            "/api/v1/auth/oidc/link",
            "POST",
            &local,
            csrf,
            serde_json::json!({"password":PASSWORD}),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let link_cookie = r.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let data = body(r).await;
        let url = Url::parse(data["url"].as_str().unwrap()).unwrap();
        let pairs: HashMap<_, _> = url.query_pairs().into_owned().collect();
        f.provider.claims.lock().unwrap()["nonce"] = serde_json::json!(pairs["nonce"]);
        let r = finish(&f, &link_cookie, &pairs["state"]).await;
        assert_eq!(r.status(), StatusCode::OK);
        let linked = session_cookie(&r);
        let me = body(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &linked,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(me["user"]["id"], id);
        assert_eq!(me["user"]["role"], "administrator");
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &local,
                "",
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            finish(&f, &cookie, &state).await.status(),
            StatusCode::UNAUTHORIZED
        ); // nonce differs
        let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        crate::database::query("UPDATE sessions SET last_seen_at=0")
            .execute(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &linked,
                "",
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/login",
                "POST",
                "",
                "",
                serde_json::json!({"login":"rescue","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    #[tokio::test]
    async fn oidc_signature_algorithm_audience_and_identity_collisions() {
        let mut f = fixture().await;
        let (_, _) = begin(&f).await;
        let d = f.s.discover().await.unwrap();
        let claims = f.provider.claims.lock().unwrap().clone();
        let nonce = claims["nonce"].as_str().unwrap();
        let mut h = jsonwebtoken::Header::new(Algorithm::RS256);
        h.kid = Some("test-1".into());
        let wrong = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
            "../tests/fixtures/oidc/key2.pem"
        ))
        .unwrap();
        let token = jsonwebtoken::encode(&h, &claims, &wrong).unwrap();
        assert!(f.s.claims(&d, &token, nonce).await.is_err());
        let hmac = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(b"not-an-RSA-key"),
        )
        .unwrap();
        assert!(f.s.claims(&d, &hmac, nonce).await.is_err());
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
            "../tests/fixtures/oidc/key1.pem"
        ))
        .unwrap();
        let mut multiple = claims.clone();
        multiple["aud"] = serde_json::json!(["client", "other"]);
        let token = jsonwebtoken::encode(&h, &multiple, &key).unwrap();
        assert!(f.s.claims(&d, &token, nonce).await.is_err());
        multiple["azp"] = serde_json::json!("client");
        let token = jsonwebtoken::encode(&h, &multiple, &key).unwrap();
        assert!(f.s.claims(&d, &token, nonce).await.is_ok());
        let (cookie, state) = begin(&f).await;
        assert_eq!(finish(&f, &cookie, &state).await.status(), StatusCode::OK);
        let (cookie, state) = begin(&f).await;
        f.provider.claims.lock().unwrap()["sub"] =
            serde_json::json!("different-subject-same-email");
        assert_eq!(finish(&f, &cookie, &state).await.status(), StatusCode::OK);
        let count: i64 = crate::database::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&f.s.auth.store.pool)
            .await
            .unwrap();
        assert_eq!(count, 3);
        f.s.auth.secure = true;
        assert!(f.s.flow_cookie("test", 300).contains("; Secure"));
        assert!(f.s.auth.cookie("test", 300).contains("; Secure"));
        f.s.config = None;
        assert_eq!(
            body(
                req(
                    &f.s,
                    "/api/v1/auth/oidc/status",
                    "GET",
                    "",
                    "",
                    serde_json::Value::Null
                )
                .await
            )
            .await["enabled"],
            false
        );
        assert_eq!(
            req(
                &f.s,
                "/api/v1/auth/oidc/start",
                "POST",
                "",
                "",
                serde_json::Value::Null
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn oidc_generic_providers_keycloak_and_pocket_id_styles() {
        // Test 1: Keycloak / Dex style provider:
        // - Uses client_secret_post auth method
        // - Uses custom groups_claim ("roles")
        // - Has end_session_endpoint advertised
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}/realms/media", listener.local_addr().unwrap());
        let claims = Arc::new(Mutex::new(serde_json::json!({})));
        let claims_for_app = claims.clone();
        let issuer_for_app = issuer.clone();

        let app = Router::new()
            .route(
                "/realms/media/.well-known/openid-configuration",
                get({
                    let iss = issuer_for_app.clone();
                    move || async move {
                        let base = Url::parse(&iss).unwrap().origin().ascii_serialization();
                        Json(serde_json::json!({
                            "issuer": iss,
                            "authorization_endpoint": format!("{base}/auth"),
                            "token_endpoint": format!("{base}/token"),
                            "jwks_uri": format!("{base}/jwks"),
                            "end_session_endpoint": format!("{base}/logout"),
                            "id_token_signing_alg_values_supported": ["RS256", "ES256"],
                            "token_endpoint_auth_methods_supported": ["client_secret_post"]
                        }))
                    }
                }),
            )
            .route(
                "/jwks",
                get(|| async {
                    Json(
                        serde_json::from_str::<serde_json::Value>(include_str!(
                            "../tests/fixtures/oidc/jwks1.json"
                        ))
                        .unwrap(),
                    )
                }),
            )
            .route(
                "/token",
                post({
                    let claims_post = claims_for_app.clone();
                    move |Form(form): Form<HashMap<String, String>>| async move {
                        assert_eq!(form["grant_type"], "authorization_code");
                        assert_eq!(form["client_id"], "keycloak-client");
                        assert_eq!(form["client_secret"], "keycloak-secret");
                        assert_eq!(
                            form["redirect_uri"],
                            format!("{ORIGIN}/api/v1/auth/oidc/callback")
                        );
                        let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
                            "../tests/fixtures/oidc/key1.pem"
                        ))
                        .unwrap();
                        let mut h = jsonwebtoken::Header::new(Algorithm::RS256);
                        h.kid = Some("test-1".into());
                        let token =
                            jsonwebtoken::encode(&h, &*claims_post.lock().unwrap(), &key).unwrap();
                        Json(serde_json::json!({"id_token": token}))
                    }
                }),
            );

        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let temp = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap("rescue", Zeroizing::new(PASSWORD.into()))
            .await
            .unwrap();
        let auth = AuthState::new(store, ORIGIN.into()).await.unwrap();

        let config = Config {
            issuer: issuer.clone(),
            client_id: "keycloak-client".into(),
            secret: Arc::new(Zeroizing::new("keycloak-secret".into())),
            redirect: format!("{ORIGIN}/api/v1/auth/oidc/callback"),
            scopes: "openid profile email".into(),
            groups: vec!["media-access".into()],
            admin_groups: vec![],
            groups_claim: "roles".into(),
            require_group: true,
        };

        let s = Oidc {
            auth,
            config: Some(config),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            workers: Arc::new(tokio::sync::Semaphore::new(8)),
        };

        let f = Fixture {
            _temp: temp,
            s,
            provider: Provider {
                issuer,
                claims,
                key: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
                exchanges: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            },
            task,
        };

        // 1. Check logout URL discovery
        let logout_resp = req(
            &f.s,
            "/api/v1/auth/oidc/logout",
            "GET",
            "",
            "",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(logout_resp.status(), StatusCode::OK);
        let logout_data = body(logout_resp).await;
        let end_session = logout_data["endSessionUrl"].as_str().unwrap();
        assert!(end_session.contains("/logout"));
        assert!(end_session.contains("post_logout_redirect_uri="));

        // 2. Login with custom roles claim
        let (cookie, state) = begin(&f).await;
        let now = jsonwebtoken::get_current_timestamp();
        let nonce = f.provider.claims.lock().unwrap()["nonce"]
            .as_str()
            .unwrap()
            .to_string();
        *f.provider.claims.lock().unwrap() = serde_json::json!({
            "iss": f.provider.issuer,
            "sub": "keycloak-user-42",
            "aud": "keycloak-client",
            "iat": now,
            "exp": now + 300,
            "nonce": nonce,
            "roles": ["media-access"]
        });

        let r = finish(&f, &cookie, &state).await;
        assert_eq!(r.status(), StatusCode::OK);
        let session = session_cookie(&r);

        let me = body(
            req(
                &f.s,
                "/api/v1/auth/me",
                "GET",
                &session,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(me["user"]["role"], "user");

        // Test 2: Pocket ID style provider:
        // - require_group = false
        // - claims have no groups / roles claim at all
        let mut cfg_pocket = f.s.config.as_ref().unwrap().clone();
        cfg_pocket.require_group = false;
        cfg_pocket.groups.clear();
        let mut s_pocket = f.s.clone();
        s_pocket.config = Some(cfg_pocket);
        let f_pocket = Fixture {
            _temp: tempfile::tempdir().unwrap(),
            s: s_pocket,
            provider: f.provider.clone(),
            task: tokio::spawn(async {}),
        };

        let (cookie2, state2) = begin(&f_pocket).await;
        let nonce2 = f_pocket.provider.claims.lock().unwrap()["nonce"]
            .as_str()
            .unwrap()
            .to_string();
        *f_pocket.provider.claims.lock().unwrap() = serde_json::json!({
            "iss": f_pocket.provider.issuer,
            "sub": "pocket-id-user-99",
            "aud": "keycloak-client",
            "iat": now,
            "exp": now + 300,
            "nonce": nonce2
            // No groups or roles claim
        });

        let r2 = finish(&f_pocket, &cookie2, &state2).await;
        assert_eq!(r2.status(), StatusCode::OK);
        let session2 = session_cookie(&r2);

        let me2 = body(
            req(
                &f_pocket.s,
                "/api/v1/auth/me",
                "GET",
                &session2,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(me2["user"]["role"], "user");
    }

    #[tokio::test]
    async fn oidc_admin_group_and_unicode_display_name() {
        let f = fixture().await;
        let mut cfg = f.s.config.as_ref().unwrap().clone();
        cfg.admin_groups = vec!["admingroup".into(), "Admins".into()];
        cfg.groups = vec!["media-users".into()];
        let mut s_custom = f.s.clone();
        s_custom.config = Some(cfg);
        let f_custom = Fixture {
            _temp: tempfile::tempdir().unwrap(),
            s: s_custom,
            provider: f.provider.clone(),
            task: tokio::spawn(async {}),
        };

        let (cookie, state) = begin(&f_custom).await;
        let nonce = f_custom.provider.claims.lock().unwrap()["nonce"]
            .as_str()
            .unwrap()
            .to_string();
        let now = jsonwebtoken::get_current_timestamp();
        *f_custom.provider.claims.lock().unwrap() = serde_json::json!({
            "iss": f_custom.provider.issuer,
            "sub": "sso-mael-id",
            "aud": "client",
            "iat": now,
            "exp": now + 300,
            "nonce": nonce,
            "groups": ["Admins"],
            "preferred_username": "mael",
            "name": "Maël Moreau"
        });

        let r = finish(&f_custom, &cookie, &state).await;
        assert_eq!(r.status(), StatusCode::OK);
        let session = session_cookie(&r);

        let me = body(
            req(
                &f_custom.s,
                "/api/v1/auth/me",
                "GET",
                &session,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;

        assert_eq!(me["user"]["role"], "administrator");
        assert_eq!(me["user"]["login"], "mael");
        assert_eq!(me["user"]["displayName"], "Maël Moreau");

        // 2. User loses admin group in IdP (only has "media-users" now)
        let (cookie2, state2) = begin(&f_custom).await;
        let nonce2 = f_custom.provider.claims.lock().unwrap()["nonce"]
            .as_str()
            .unwrap()
            .to_string();
        *f_custom.provider.claims.lock().unwrap() = serde_json::json!({
            "iss": f_custom.provider.issuer,
            "sub": "sso-mael-id",
            "aud": "client",
            "iat": now + 10,
            "exp": now + 310,
            "nonce": nonce2,
            "groups": ["media-users"],
            "preferred_username": "mael",
            "name": "Maël Moreau"
        });

        let r2 = finish(&f_custom, &cookie2, &state2).await;
        assert_eq!(r2.status(), StatusCode::OK);
        let session2 = session_cookie(&r2);

        // 3. New session has downgraded role "user"
        let me2 = body(
            req(
                &f_custom.s,
                "/api/v1/auth/me",
                "GET",
                &session2,
                "",
                serde_json::Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(me2["user"]["role"], "user");

        // 4. Stale admin session was immediately revoked upon downgrade
        let old_session_res = req(
            &f_custom.s,
            "/api/v1/auth/me",
            "GET",
            &session,
            "",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(old_session_res.status(), StatusCode::UNAUTHORIZED);

        // 5. User loses all groups in IdP: login is denied and active sessions are revoked
        let (cookie3, state3) = begin(&f_custom).await;
        let nonce3 = f_custom.provider.claims.lock().unwrap()["nonce"]
            .as_str()
            .unwrap()
            .to_string();
        *f_custom.provider.claims.lock().unwrap() = serde_json::json!({
            "iss": f_custom.provider.issuer,
            "sub": "sso-mael-id",
            "aud": "client",
            "iat": now + 20,
            "exp": now + 320,
            "nonce": nonce3,
            "groups": ["unrelated-group"],
            "preferred_username": "mael"
        });

        let r3 = finish(&f_custom, &cookie3, &state3).await;
        assert_eq!(r3.status(), StatusCode::FORBIDDEN);

        // Session2 is now revoked
        let session2_after_removal = req(
            &f_custom.s,
            "/api/v1/auth/me",
            "GET",
            &session2,
            "",
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(session2_after_removal.status(), StatusCode::UNAUTHORIZED);
    }
}
