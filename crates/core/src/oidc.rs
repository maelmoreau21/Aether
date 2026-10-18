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
    groups: Vec<String>,
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
        let groups: Vec<String> = required("MEDIA_OIDC_USER_GROUPS")?
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if groups.is_empty() {
            return Err("MEDIA_OIDC_USER_GROUPS must explicitly allow at least one group");
        }
        Ok(Some(Self {
            issuer,
            client_id: required("MEDIA_OIDC_CLIENT_ID")?,
            secret: Arc::new(Zeroizing::new(required("MEDIA_OIDC_CLIENT_SECRET")?)),
            redirect: format!("{}/api/v1/auth/oidc/callback", auth.origin()),
            groups,
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
    id_token_signing_alg_values_supported: Vec<String>,
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
    #[serde(default)]
    groups: Vec<String>,
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
    user_groups: Vec<String>,
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
        .layer(axum::extract::DefaultBodyLimit::max(4096))
        .with_state(state)
}
async fn public_status(State(s): State<Oidc>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"enabled": s.config.is_some()}))
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
    Ok(Json(Status {
        enabled: s.config.is_some(),
        issuer: s.config.as_ref().map(|c| c.issuer.clone()),
        client_id: s.config.as_ref().map(|c| c.client_id.clone()),
        redirect_uri: s.config.as_ref().map(|c| c.redirect.clone()),
        user_groups: s
            .config
            .as_ref()
            .map(|c| c.groups.clone())
            .unwrap_or_default(),
        local_login_enabled: true,
    }))
}
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
        if d.issuer != c.issuer
            || !d
                .id_token_signing_alg_values_supported
                .iter()
                .any(|a| a == "RS256")
        {
            return Err(invalid());
        }
        let issuer = endpoint(&c.issuer)?;
        for raw in [&d.authorization_endpoint, &d.token_endpoint, &d.jwks_uri] {
            // Authentik endpoints share the issuer origin. No arbitrary discovery redirects/SSRF.
            if endpoint(raw)?.origin() != issuer.origin() {
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
        if rescue == 0 {
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
            ("scope", "openid profile email"),
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
        if h.alg != Algorithm::RS256 {
            return Err(invalid());
        }
        let kid = h.kid.ok_or_else(invalid)?;
        // Fresh JWKS on each callback: rotation is immediate, no stale-key acceptance.
        let keys: JwkSet = self.json(self.http.get(&d.jwks_uri)).await?;
        let jwk = keys.find(&kid).ok_or_else(invalid)?;
        let key = DecodingKey::try_from(jwk).map_err(|_| invalid())?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&c.issuer]);
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
        let tokens: Tokens = self
            .json(
                self.http
                    .post(&d.token_endpoint)
                    .basic_auth(&c.client_id, Some(c.secret.as_str()))
                    .form(&[
                        ("grant_type", "authorization_code"),
                        ("code", &code),
                        ("redirect_uri", &c.redirect),
                        ("code_verifier", &flow.verifier),
                    ]),
            )
            .await?;
        let claims = self.claims(&d, &tokens.id_token, &flow.nonce).await?;
        if !claims.groups.iter().any(|g| c.groups.contains(g)) {
            return Err(deny(StatusCode::FORBIDDEN, "oidc_group_denied"));
        }
        let mut conn = self.auth.store.pool.acquire().await.map_err(db)?;
        let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
        let linked_user: Option<String> = if let Some(ref session_hash) = flow.link_session_hash {
            Some(crate::database::query_scalar("SELECT s.user_id FROM sessions s JOIN users u ON u.id=s.user_id JOIN local_credentials c ON c.user_id=u.id WHERE s.token_hash=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-1800 AND u.disabled_at IS NULL")
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
            (Some(id), _) => id,
            (None, target) => {
                let id = target.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                crate::database::query(
                    "INSERT INTO users(id,role) VALUES (?,'user') ON CONFLICT(id) DO NOTHING",
                )
                .bind(&id)
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
        let created=crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+43200 FROM users WHERE id=? AND disabled_at IS NULL").bind(auth::hash(&raw)).bind(&user).execute(&mut *tx).await.map_err(db)?;
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
        // End the cross-site redirect chain on a Core document. A same-origin
        // external script starts a new navigation so Strict session cookies and
        // the existing cross-site guards both remain effective on /app/.
        let mut response = Html("<!doctype html><html lang=\"fr\"><meta charset=\"utf-8\"><title>Connexion</title><script src=\"/api/v1/auth/oidc/complete.js\" defer></script><p>Connexion en cours...</p><noscript><a href=\"/app/\">Continuer</a></noscript></html>").into_response();
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&self.auth.cookie(&raw, 43200)).map_err(|_| invalid())?,
        );
        Ok(response)
    }
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
    // Reuse local login's persistent global rate limit before expensive Argon2 work.
    let allowed=crate::database::query_scalar::<_,i64>("INSERT INTO login_throttle(singleton,window_started,attempts) VALUES(1,unixepoch(),1) ON CONFLICT(singleton) DO UPDATE SET attempts=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 ELSE login_throttle.attempts+1 END, window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN unixepoch() ELSE login_throttle.window_started END RETURNING login_throttle.attempts").fetch_one(&s.auth.store.pool).await.map_err(db)?;
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
            groups: vec!["media-users".into()],
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
}
