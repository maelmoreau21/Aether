//! Local authentication. Session secrets never enter SQLite or application logs.
use crate::identity::{IdentityError, IdentityStore, Role};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

const ABSOLUTE_SECONDS: i64 = 12 * 60 * 60;
const IDLE_SECONDS: i64 = 30 * 60;
const COOKIE: &str = "media_session";

#[derive(Clone)]
pub struct AuthState {
    store: IdentityStore,
    origin: Arc<str>,
    secure: bool,
    dummy_hash: Arc<str>,
    workers: Arc<Semaphore>,
}
#[derive(Debug, thiserror::Error)]
pub enum AuthConfigError {
    #[error("public origin must be HTTPS or HTTP on loopback, without path or credentials")]
    Origin,
    #[error(transparent)]
    Identity(#[from] IdentityError),
}
#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct AuthUser {
    pub id: String,
    pub login: String,
    #[serde(serialize_with = "serialize_role")]
    pub role: Role,
}
fn serialize_role<S: serde::Serializer>(role: &Role, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(role.as_str())
}
pub struct Authenticated {
    pub user: AuthUser,
    pub csrf_token: String,
    token_hash: String,
}
#[derive(Deserialize)]
struct Login {
    login: String,
    password: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityResponse {
    user: AuthUser,
    csrf_token: String,
}

impl AuthState {
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub async fn new(store: IdentityStore, origin: String) -> Result<Self, AuthConfigError> {
        let uri: axum::http::Uri = origin.parse().map_err(|_| AuthConfigError::Origin)?;
        let scheme = uri.scheme_str().ok_or(AuthConfigError::Origin)?;
        let host = uri.host().ok_or(AuthConfigError::Origin)?;
        if uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
            || !(scheme == "https"
                || (scheme == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")))
        {
            return Err(AuthConfigError::Origin);
        }
        let secure = scheme == "https";
        let origin = origin.trim_end_matches('/').to_owned();
        let dummy_hash = tokio::task::spawn_blocking(|| {
            let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| IdentityError::Hash)?;
            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password(random_token().as_bytes())
                .map(|h| h.to_string())
                .map_err(|_| IdentityError::Hash)
        })
        .await
        .map_err(|_| IdentityError::Worker)??;
        Ok(Self {
            store,
            origin: origin.into(),
            secure,
            dummy_hash: dummy_hash.into(),
            workers: Arc::new(Semaphore::new(2)),
        })
    }
    pub async fn authenticate(&self, headers: &HeaderMap) -> Result<Authenticated, StatusCode> {
        let raw = session_token(headers).ok_or(StatusCode::UNAUTHORIZED)?;
        let token_hash = hash(raw);
        // One atomic update enforces expiry and prevents revival of expired sessions.
        let user_id: Option<String> = sqlx::query_scalar("UPDATE sessions SET last_seen_at=unixepoch() WHERE token_hash=? AND expires_at>unixepoch() AND last_seen_at>unixepoch()-? AND EXISTS(SELECT 1 FROM users WHERE users.id=sessions.user_id AND disabled_at IS NULL) RETURNING user_id")
            .bind(&token_hash).bind(IDLE_SECONDS).fetch_optional(&self.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
        let id = user_id.ok_or(StatusCode::UNAUTHORIZED)?;
        let user=sqlx::query_as("SELECT u.id,c.login,u.role FROM users u JOIN local_credentials c ON c.user_id=u.id WHERE u.id=? AND u.disabled_at IS NULL")
            .bind(id).fetch_optional(&self.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::UNAUTHORIZED)?;
        Ok(Authenticated {
            user,
            csrf_token: csrf(raw),
            token_hash,
        })
    }
    pub fn require_csrf(
        &self,
        headers: &HeaderMap,
        identity: &Authenticated,
    ) -> Result<(), StatusCode> {
        self.check_origin(headers)?;
        let provided = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .ok_or(StatusCode::FORBIDDEN)?;
        // Hash both equal-length values to avoid comparing unbounded attacker input.
        if provided.len() != 64
            || !constant_equal(
                hash(provided).as_bytes(),
                hash(&identity.csrf_token).as_bytes(),
            )
        {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(())
    }
    fn check_origin(&self, headers: &HeaderMap) -> Result<(), StatusCode> {
        if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&self.origin)
            || headers
                .get("sec-fetch-site")
                .is_some_and(|v| v == "cross-site")
        {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(())
    }
    fn cookie(&self, token: &str, max_age: i64) -> String {
        format!(
            "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
            if self.secure { "; Secure" } else { "" }
        )
    }
}
pub fn router(state: AuthState) -> Router {
    Router::new()
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(state)
}
pub async fn authenticate(
    state: &AuthState,
    headers: &HeaderMap,
) -> Result<Authenticated, StatusCode> {
    state.authenticate(headers).await
}
async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Response {
    if let Err(status) = state.check_origin(&headers) {
        return failure(status, "forbidden");
    }
    let Ok(_permit) = state.workers.clone().try_acquire_owned() else {
        return failure(StatusCode::TOO_MANY_REQUESTS, "try_later");
    };
    // Single bounded row: shared budget across all account names, durable across restarts.
    let allowed=sqlx::query_scalar::<_,i64>("INSERT INTO login_throttle(singleton,window_started,attempts) VALUES(1,unixepoch(),1) ON CONFLICT(singleton) DO UPDATE SET window_started=CASE WHEN window_started<=unixepoch()-60 THEN unixepoch() ELSE window_started END, attempts=CASE WHEN window_started<=unixepoch()-60 THEN 1 ELSE attempts+1 END WHERE window_started<=unixepoch()-60 OR attempts<10 RETURNING attempts").fetch_optional(&state.store.pool).await;
    match allowed {
        Ok(Some(_)) => {}
        Ok(None) => return failure(StatusCode::TOO_MANY_REQUESTS, "try_later"),
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    }
    if input.login.len() > 64 || input.password.len() > 512 {
        return failure(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    let row=sqlx::query_as::<_,(String,String)>("SELECT c.user_id,c.password_hash FROM local_credentials c JOIN users u ON u.id=c.user_id WHERE c.login=? AND u.disabled_at IS NULL").bind(&input.login).fetch_optional(&state.store.pool).await;
    let row = match row {
        Ok(r) => r,
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let encoded = row
        .as_ref()
        .map(|r| r.1.clone())
        .unwrap_or_else(|| state.dummy_hash.to_string());
    let password = Zeroizing::new(input.password);
    let verified = tokio::task::spawn_blocking(move || {
        // Keep the permit inside the worker even if its HTTP caller disconnects.
        let _permit = _permit;
        let Ok(parsed) = PasswordHash::new(&encoded) else {
            return false;
        };
        encoded.starts_with("$argon2id$")
            && Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
    })
    .await
    .unwrap_or(false);
    let Some((id, _)) = row.filter(|_| verified) else {
        return failure(StatusCode::UNAUTHORIZED, "invalid_credentials");
    };
    let raw = random_token();
    // Opportunistic bounded-state cleanup; no secret material is persisted.
    if sqlx::query(
        "DELETE FROM sessions WHERE expires_at<=unixepoch() OR last_seen_at<=unixepoch()-?",
    )
    .bind(IDLE_SECONDS)
    .execute(&state.store.pool)
    .await
    .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    if sqlx::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+? FROM users WHERE id=? AND disabled_at IS NULL").bind(hash(&raw)).bind(ABSOLUTE_SECONDS).bind(&id).execute(&state.store.pool).await.is_err(){return failure(StatusCode::INTERNAL_SERVER_ERROR,"internal_error")}
    let mut auth_headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("{COOKIE}={raw}")) {
        auth_headers.insert(header::COOKIE, value);
    }
    let identity = match state.authenticate(&auth_headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "invalid_credentials"),
    };
    let mut response = Json(IdentityResponse {
        user: identity.user,
        csrf_token: identity.csrf_token,
    })
    .into_response();
    if let Ok(cookie) = HeaderValue::from_str(&state.cookie(&raw, ABSOLUTE_SECONDS)) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}
async fn me(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    match state.authenticate(&headers).await {
        Ok(i) => Json(IdentityResponse {
            user: i.user,
            csrf_token: i.csrf_token,
        })
        .into_response(),
        Err(s) => failure(s, "authentication_required"),
    }
}
async fn logout(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let identity = match state.authenticate(&headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "authentication_required"),
    };
    if let Err(s) = state.require_csrf(&headers, &identity) {
        return failure(s, "forbidden");
    }
    if sqlx::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(identity.token_hash)
        .execute(&state.store.pool)
        .await
        .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(cookie) = HeaderValue::from_str(&state.cookie("", 0)) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}
fn failure(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({"error":code}))).into_response()
}
fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}
fn csrf(raw: &str) -> String {
    hash(&format!("csrf:{raw}"))
}
fn constant_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}
fn session_token(headers: &HeaderMap) -> Option<&str> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for pair in value.to_str().ok()?.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if name == COOKIE {
                if found.is_some()
                    || value.len() != 64
                    || !value.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    const ORIGIN: &str = "http://127.0.0.1:3000";
    const PASSWORD: &str = "synthetic auth test password";
    async fn setup() -> (tempfile::TempDir, AuthState) {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("auth.sqlite3"))
            .await
            .unwrap();
        store
            .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
            .await
            .unwrap();
        let state = AuthState::new(store, ORIGIN.into()).await.unwrap();
        (temp, state)
    }
    async fn request(
        state: &AuthState,
        path: &str,
        method: &str,
        cookie: Option<&str>,
        origin: Option<&str>,
        csrf: Option<&str>,
        body: serde_json::Value,
    ) -> Response {
        let mut req = Request::builder()
            .uri(path)
            .method(method)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(v) = cookie {
            req = req.header(header::COOKIE, v)
        }
        if let Some(v) = origin {
            req = req.header(header::ORIGIN, v)
        }
        if let Some(v) = csrf {
            req = req.header("x-csrf-token", v)
        }
        router(state.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
    async fn sign_in(state: &AuthState) -> (String, String) {
        let response = request(
            state,
            "/api/v1/auth/login",
            "POST",
            None,
            Some(ORIGIN),
            None,
            serde_json::json!({"login":"admin","password":PASSWORD}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly; SameSite=Strict"));
        assert!(!cookie.contains("Secure"));
        let body: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["user"]["role"], "administrator");
        (
            cookie.split(';').next().unwrap().into(),
            body["csrfToken"].as_str().unwrap().into(),
        )
    }
    #[tokio::test]
    async fn login_persistence_expiry_csrf_and_revocation() {
        let (temp, state) = setup().await;
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/me",
                "GET",
                None,
                None,
                None,
                serde_json::json!(null)
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let (cookie, csrf) = sign_in(&state).await;
        let (second_cookie, _) = sign_in(&state).await;
        assert_ne!(cookie, second_cookie);
        let raw = cookie.split('=').nth(1).unwrap();
        let hashes: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&state.store.pool)
            .await
            .unwrap();
        assert!(hashes.contains(&hash(raw)));
        assert_eq!(hashes.len(), 2);
        assert_ne!(hashes[0], raw);
        state.store.close().await;
        let reopened = AuthState::new(
            IdentityStore::open(&temp.path().join("auth.sqlite3"))
                .await
                .unwrap(),
            ORIGIN.into(),
        )
        .await
        .unwrap();
        let headers =
            HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
        assert!(reopened.authenticate(&headers).await.is_ok());
        for (origin, token) in [
            (None, Some(csrf.as_str())),
            (Some("https://attacker.invalid"), Some(csrf.as_str())),
            (Some(ORIGIN), None),
            (Some(ORIGIN), Some("wrong")),
        ] {
            assert_eq!(
                request(
                    &reopened,
                    "/api/v1/auth/logout",
                    "POST",
                    Some(&cookie),
                    origin,
                    token,
                    serde_json::json!(null)
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert!(reopened.authenticate(&headers).await.is_ok());
        assert_eq!(
            request(
                &reopened,
                "/api/v1/auth/logout",
                "POST",
                Some(&cookie),
                Some(ORIGIN),
                Some(&csrf),
                serde_json::json!(null)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert!(matches!(
            reopened.authenticate(&headers).await,
            Err(StatusCode::UNAUTHORIZED)
        ));
        for query in [
            "UPDATE sessions SET last_seen_at=0",
            "UPDATE sessions SET expires_at=0",
        ] {
            let (cookie, _) = sign_in(&reopened).await;
            sqlx::query(query)
                .execute(&reopened.store.pool)
                .await
                .unwrap();
            let headers =
                HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
            assert!(matches!(
                reopened.authenticate(&headers).await,
                Err(StatusCode::UNAUTHORIZED)
            ));
        }
        let (cookie, _) = sign_in(&reopened).await;
        sqlx::query("UPDATE users SET disabled_at=unixepoch()")
            .execute(&reopened.store.pool)
            .await
            .unwrap();
        let headers =
            HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
        assert!(matches!(
            reopened.authenticate(&headers).await,
            Err(StatusCode::UNAUTHORIZED)
        ));
    }
    #[tokio::test]
    async fn generic_credentials_durable_throttle_and_origin() {
        let (temp, state) = setup().await;
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                None,
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let mut bodies = Vec::new();
        for login in ["admin", "missing"] {
            let response = request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":login,"password":"wrong"}),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            bodies.push(response.into_body().collect().await.unwrap().to_bytes());
        }
        assert_eq!(bodies[0], bodies[1]);
        sqlx::query("UPDATE login_throttle SET attempts=8")
            .execute(&state.store.pool)
            .await
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                request(
                    &state,
                    "/api/v1/auth/login",
                    "POST",
                    None,
                    Some(ORIGIN),
                    None,
                    serde_json::json!({"login":"missing","password":"wrong"})
                )
                .await
                .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        state.store.close().await;
        let restarted = AuthState::new(
            IdentityStore::open(&temp.path().join("auth.sqlite3"))
                .await
                .unwrap(),
            ORIGIN.into(),
        )
        .await
        .unwrap();
        assert_eq!(
            request(
                &restarted,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        sqlx::query("UPDATE login_throttle SET window_started=0")
            .execute(&restarted.store.pool)
            .await
            .unwrap();
        sign_in(&restarted).await;
        assert!(
            AuthState::new(restarted.store.clone(), "http://192.168.1.2:3000".into())
                .await
                .is_err()
        );
        let secure = AuthState::new(restarted.store.clone(), "https://media.example".into())
            .await
            .unwrap();
        assert!(secure.cookie("test", 100).ends_with("; Secure"));
        let permit1 = restarted.workers.clone().acquire_owned().await.unwrap();
        let permit2 = restarted.workers.clone().acquire_owned().await.unwrap();
        assert_eq!(
            request(
                &restarted,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        drop((permit1, permit2));
    }
}
