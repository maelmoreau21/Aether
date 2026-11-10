//! Local authentication REST router and handlers.
use super::{
    config::{is_http_bootstrap_disabled, is_local_login_enabled},
    state::{AuthState, AuthUser, Authenticated},
    util::{
        ABSOLUTE_SECONDS, COOKIE, IDLE_SECONDS, TokenType, constant_equal, device_label,
        extract_client_key, hash, random_token,
    },
};
use crate::identity::IdentityError;
use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct Login {
    login: String,
    password: String,
    #[serde(default, rename = "tokenType")]
    token_type: TokenType,
}
/// How long a refreshed token stays usable, so requests already in flight on the
/// device (segment fetches, a download resuming) do not fail mid-rotation.
const REFRESH_GRACE_SECONDS: i64 = 60;
/// Body handed to a native application in place of a cookie; never cached.
pub(super) fn bearer_grant(user: AuthUser, raw: &str, status: Option<&str>) -> Response {
    let mut body = serde_json::json!({
        "user": user,
        "tokenType": "bearer",
        "accessToken": raw,
        "expiresIn": ABSOLUTE_SECONDS,
    });
    if let Some(status) = status {
        body["status"] = status.into();
    }
    let mut response = Json(body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
#[derive(Deserialize)]
struct BootstrapInput {
    login: String,
    password: String,
    #[serde(default)]
    bootstrap_token: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityResponse {
    user: AuthUser,
    csrf_token: String,
}

pub fn router(state: AuthState) -> Router {
    Router::new()
        .merge(super::quick_connect::router())
        .merge(super::mfa::router())
        .merge(super::passkey::router())
        .route("/api/v1/auth/bootstrap/status", get(bootstrap_status))
        .route("/api/v1/auth/bootstrap", post(bootstrap))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/token/refresh", post(refresh))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(state)
}

async fn bootstrap_status(State(state): State<AuthState>) -> Response {
    let closed: i64 = crate::database::query_scalar(
        "SELECT CAST((EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM users)) AS INTEGER)",
    )
    .fetch_one(&state.store.pool)
    .await
    .unwrap_or(1);
    let oidc_admin_configured = std::env::var("MEDIA_OIDC_ADMIN_GROUPS")
        .or_else(|_| std::env::var("MEDIA_OIDC_ADMIN_GROUP"))
        .or_else(|_| std::env::var("OIDC_ADMIN_GROUP"))
        .or_else(|_| std::env::var("OIDC_ADMIN_GROUPS"))
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let local_enabled = is_local_login_enabled();
    let http_bootstrap_enabled = !is_http_bootstrap_disabled();
    Json(serde_json::json!({
        "needsBootstrap": closed == 0 && !oidc_admin_configured,
        "localLoginEnabled": local_enabled,
        "oidcAdminConfigured": oidc_admin_configured,
        "httpBootstrapEnabled": http_bootstrap_enabled
    }))
    .into_response()
}

async fn bootstrap(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<BootstrapInput>,
) -> Response {
    if !is_local_login_enabled() {
        return failure(StatusCode::FORBIDDEN, "local_login_disabled");
    }
    if is_http_bootstrap_disabled() {
        return failure(StatusCode::FORBIDDEN, "http_bootstrap_disabled");
    }
    if let Err(status) = state.check_origin(&headers) {
        return failure(status, "forbidden");
    }
    let Ok(_permit) = state.workers.clone().try_acquire_owned() else {
        return failure(StatusCode::TOO_MANY_REQUESTS, "try_later");
    };

    let expected = state.get_bootstrap_token();
    let Some(expected_token) = expected else {
        return failure(StatusCode::FORBIDDEN, "bootstrap_closed");
    };

    let provided = input.bootstrap_token.as_deref().unwrap_or("").trim();
    if provided.is_empty()
        || provided.len() != expected_token.len()
        || !constant_equal(hash(provided).as_bytes(), hash(&expected_token).as_bytes())
    {
        return failure(StatusCode::UNAUTHORIZED, "invalid_bootstrap_token");
    }

    let login_str = input.login.trim();
    if login_str.is_empty()
        || login_str.len() > 64
        || !login_str
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    {
        return failure(StatusCode::BAD_REQUEST, "invalid_login");
    }
    if input.password.chars().count() < 15
        || input.password.len() > 512
        || input.password.chars().any(char::is_control)
    {
        return failure(StatusCode::BAD_REQUEST, "invalid_password");
    }
    let id = match state
        .store
        .bootstrap(login_str, Zeroizing::new(input.password))
        .await
    {
        Ok(id) => id,
        Err(IdentityError::AlreadyInitialized) => {
            return failure(StatusCode::FORBIDDEN, "bootstrap_closed");
        }
        Err(IdentityError::InvalidLogin) => {
            return failure(StatusCode::BAD_REQUEST, "invalid_login");
        }
        Err(IdentityError::InvalidPassword) => {
            return failure(StatusCode::BAD_REQUEST, "invalid_password");
        }
        Err(_) => {
            return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
        }
    };

    state.set_bootstrap_token(None);
    if let Some(path) = state
        .bootstrap_file_path
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        let _ = std::fs::remove_file(path);
    }

    let raw = random_token();
    if crate::database::query(
        "INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+? FROM users WHERE id=? AND disabled_at IS NULL",
    )
    .bind(hash(&raw))
    .bind(ABSOLUTE_SECONDS)
    .bind(&id)
    .execute(&state.store.pool)
    .await
    .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    let _ = crate::database::query("UPDATE sessions SET device_label=? WHERE token_hash=?")
        .bind(device_label(&headers))
        .bind(hash(&raw))
        .execute(&state.store.pool)
        .await;
    let mut auth_headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("{COOKIE}={raw}")) {
        auth_headers.insert(header::COOKIE, value);
    }
    let identity = match state.authenticate(&auth_headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "internal_error"),
    };
    let mut response = Json(IdentityResponse {
        user: identity.user,
        csrf_token: identity.csrf_token,
    })
    .into_response();
    if let Ok(value) = HeaderValue::from_str(&state.cookie(&raw, ABSOLUTE_SECONDS)) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
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
    if !is_local_login_enabled() {
        return failure(StatusCode::FORBIDDEN, "local_login_disabled");
    }
    let token_type = input.token_type;
    let origin_check = match token_type {
        TokenType::Cookie => state.check_origin(&headers),
        TokenType::Bearer => state.check_origin_or_native(&headers),
    };
    if let Err(status) = origin_check {
        return failure(status, "forbidden");
    }
    let Ok(_permit) = state.workers.clone().try_acquire_owned() else {
        return failure(StatusCode::TOO_MANY_REQUESTS, "try_later");
    };
    // Scoped throttle: budget per client IP, durable across restarts.
    let client_key = extract_client_key(&headers);
    let allowed = crate::database::query_scalar::<_, i64>(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES(?,unixepoch(),1) ON CONFLICT(throttle_key) DO UPDATE SET window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN unixepoch() ELSE login_throttle.window_started END, attempts=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 ELSE login_throttle.attempts+1 END WHERE login_throttle.window_started<=unixepoch()-60 OR login_throttle.attempts<10 RETURNING login_throttle.attempts",
    )
    .bind(&client_key)
    .fetch_optional(&state.store.pool)
    .await;
    match allowed {
        Ok(Some(_)) => {}
        Ok(None) => return failure(StatusCode::TOO_MANY_REQUESTS, "try_later"),
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    }
    let _ = crate::database::query(
        "DELETE FROM login_throttle WHERE window_started <= unixepoch() - 300",
    )
    .execute(&state.store.pool)
    .await;
    if input.login.len() > 64 || input.password.len() > 512 {
        return failure(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    let row=crate::database::query_as::<_,(String,String)>("SELECT c.user_id,c.password_hash FROM local_credentials c JOIN users u ON u.id=c.user_id WHERE c.login=? AND u.disabled_at IS NULL").bind(&input.login).fetch_optional(&state.store.pool).await;
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
        let ok = encoded.starts_with("$argon2id$")
            && Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok();
        crate::system_memory::trim_memory();
        ok
    })
    .await
    .unwrap_or(false);
    let Some((id, _)) = row.filter(|_| verified) else {
        return failure(StatusCode::UNAUTHORIZED, "invalid_credentials");
    };
    let bearer = token_type == TokenType::Bearer;
    // An account with TOTP or a passkey gets a single-use mfaToken instead of a session.
    match super::mfa::second_factor_challenge(&state, &id, bearer).await {
        Ok(None) => issue_session(&state, &headers, &id, bearer).await,
        Ok(Some(challenge)) => challenge,
        Err(error) => error.into_response(),
    }
}
/// Opens a fresh session for an already-verified user: a cookie for the browser,
/// or a bearer grant for a native application. Shared by login and profile switching.
pub(crate) async fn issue_session(
    state: &AuthState,
    headers: &HeaderMap,
    id: &str,
    bearer: bool,
) -> Response {
    let raw = random_token();
    // Opportunistic bounded-state cleanup; no secret material is persisted.
    if crate::database::query(
        "DELETE FROM sessions WHERE expires_at<=unixepoch() OR last_seen_at<=unixepoch()-?",
    )
    .bind(IDLE_SECONDS)
    .execute(&state.store.pool)
    .await
    .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    if crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+? FROM users WHERE id=? AND disabled_at IS NULL").bind(hash(&raw)).bind(ABSOLUTE_SECONDS).bind(id).execute(&state.store.pool).await.is_err(){return failure(StatusCode::INTERNAL_SERVER_ERROR,"internal_error")}
    let _ = crate::database::query("UPDATE sessions SET device_label=? WHERE token_hash=?")
        .bind(device_label(headers))
        .bind(hash(&raw))
        .execute(&state.store.pool)
        .await;
    if bearer {
        return match state.session_from_hash(&hash(&raw)).await {
            Ok(user) => bearer_grant(user, &raw, None),
            Err(s) => failure(s, "invalid_credentials"),
        };
    }
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
    state.toucher.shared.cancel(&identity.token_hash);
    if crate::database::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(&identity.token_hash)
        .execute(&state.store.pool)
        .await
        .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    state.toucher.shared.cancel(&identity.token_hash);
    let mut response = StatusCode::NO_CONTENT.into_response();
    if !identity.bearer
        && let Ok(cookie) = HeaderValue::from_str(&state.cookie("", 0))
    {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}
/// Rotates a native application's token before its absolute expiry. The old token
/// keeps working for `REFRESH_GRACE_SECONDS` and can no longer be refreshed itself,
/// so a copied token cannot fork the device session.
async fn refresh(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let identity = match state.authenticate(&headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "authentication_required"),
    };
    if !identity.bearer {
        return failure(StatusCode::BAD_REQUEST, "bearer_required");
    }
    let raw = random_token();
    let rotated: Result<bool, sqlx::Error> = async {
        let mut conn = state.store.pool.acquire().await?;
        let mut tx = crate::database::begin_write(&mut conn).await?;
        let created = crate::database::query("INSERT INTO sessions(token_hash,user_id,device_label,created_at,last_seen_at,expires_at) SELECT ?,s.user_id,s.device_label,unixepoch(),unixepoch(),unixepoch()+? FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.expires_at>unixepoch()+? AND s.last_seen_at>unixepoch()-? AND u.disabled_at IS NULL")
            .bind(hash(&raw)).bind(ABSOLUTE_SECONDS).bind(&identity.token_hash).bind(REFRESH_GRACE_SECONDS).bind(IDLE_SECONDS)
            .execute(&mut *tx).await?;
        if created.rows_affected() != 1 {
            return Ok(false);
        }
        crate::database::query("UPDATE sessions SET expires_at=unixepoch()+? WHERE token_hash=?")
            .bind(REFRESH_GRACE_SECONDS)
            .bind(&identity.token_hash)
            .execute(&mut *tx)
            .await?;
        // A cast in progress keeps playing across the rotation.
        crate::database::query("UPDATE playback_grants SET session_hash=? WHERE session_hash=?")
            .bind(hash(&raw))
            .bind(&identity.token_hash)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }
    .await;
    match rotated {
        Ok(true) => bearer_grant(identity.user, &raw, None),
        Ok(false) => failure(StatusCode::UNAUTHORIZED, "authentication_required"),
        Err(_) => failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    }
}
fn failure(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({"error":code}))).into_response()
}
