//! Several accounts signed in on one browser, one of them active.
//!
//! The active account is the usual session cookie. The others are ordinary
//! sessions too, parked in a second `HttpOnly` cookie that only these routes
//! read: switching swaps two cookies, nobody signs in again, and each account
//! keeps its own session, with its own expiry and revocation.
use super::{
    state::{AuthState, AuthUser},
    util::{ABSOLUTE_SECONDS, COOKIE, hash, session_token},
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

const PARKED: &str = "aether_accounts";
/// Accounts kept beside the active one.
const MAX_PARKED: usize = 5;

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route("/api/v1/auth/accounts", get(list))
        .route("/api/v1/auth/accounts/park", post(park))
        .route("/api/v1/auth/accounts/switch", post(switch))
        .route("/api/v1/auth/accounts/forget", post(forget))
}

fn failure(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error":code}))).into_response()
}

/// Session tokens of the parked accounts, as the browser sent them.
pub(super) fn parked(headers: &HeaderMap) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for value in headers.get_all(header::COOKIE) {
        let Ok(value) = value.to_str() else { continue };
        for pair in value.split(';') {
            let Some((name, list)) = pair.trim().split_once('=') else {
                continue;
            };
            if name != PARKED {
                continue;
            }
            for token in list.split('.') {
                let valid = token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit());
                if valid && tokens.len() < MAX_PARKED && !tokens.iter().any(|t| t == token) {
                    tokens.push(token.to_owned());
                }
            }
        }
    }
    tokens
}

fn set_cookie(response: &mut Response, cookie: String) {
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn set_parked(state: &AuthState, response: &mut Response, tokens: &[String]) {
    let age = if tokens.is_empty() {
        0
    } else {
        ABSOLUTE_SECONDS
    };
    set_cookie(
        response,
        format!(
            "{PARKED}={}; Path=/api/v1/auth; HttpOnly; SameSite=Strict; Max-Age={age}{}",
            tokens.join("."),
            if state.secure { "; Secure" } else { "" }
        ),
    );
}

/// The parked sessions that still stand, with their account. A second session
/// of the active account, or of an account already listed, is closed.
async fn standing(
    state: &AuthState,
    tokens: Vec<String>,
    active: Option<&str>,
) -> Vec<(String, AuthUser)> {
    let mut accounts: Vec<(String, AuthUser)> = Vec::new();
    for token in tokens {
        let Ok(user) = state.session_from_hash(&hash(&token)).await else {
            continue;
        };
        if active == Some(user.id.as_str()) || accounts.iter().any(|(_, u)| u.id == user.id) {
            close(state, &token).await;
            continue;
        }
        accounts.push((token, user));
    }
    accounts
}

async fn close(state: &AuthState, token: &str) {
    let _ = crate::database::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(hash(token))
        .execute(&state.store.pool)
        .await;
}

/// The active account, if any, checked as a browser request that changes state.
async fn active(
    state: &AuthState,
    headers: &HeaderMap,
) -> Result<Option<super::Authenticated>, (StatusCode, &'static str)> {
    state.check_origin(headers).map_err(|s| (s, "forbidden"))?;
    let Ok(identity) = state.authenticate(headers).await else {
        return Ok(None);
    };
    if identity.bearer {
        return Err((StatusCode::BAD_REQUEST, "cookie_required"));
    }
    state
        .require_csrf(headers, &identity)
        .map_err(|s| (s, "forbidden"))?;
    Ok(Some(identity))
}

/// The accounts of this browser, the active one first. Also read by the
/// sign-in page, signed out, to offer the parked accounts again.
async fn list(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let current = state.authenticate(&headers).await.ok();
    let mut accounts = Vec::new();
    if let Some(identity) = &current {
        let has_pin = crate::profiles::has_pin(&state, &identity.user.id).await;
        accounts.push(json!({"user": identity.user, "current": true, "hasPin": has_pin}));
        if identity.bearer {
            return Json(json!({"accounts": accounts, "max": 1})).into_response();
        }
    }
    let others = standing(
        &state,
        parked(&headers),
        current.as_ref().map(|i| i.user.id.as_str()),
    )
    .await;
    let mut tokens = Vec::new();
    for (token, user) in others {
        // Listed on a device in use: the idle limit does not end a parked account.
        let _ = crate::database::query(
            "UPDATE sessions SET last_seen_at=unixepoch() WHERE token_hash=?",
        )
        .bind(hash(&token))
        .execute(&state.store.pool)
        .await;
        let has_pin = crate::profiles::has_pin(&state, &user.id).await;
        accounts.push(json!({"user": user, "current": false, "hasPin": has_pin}));
        tokens.push(token);
    }
    let mut response = Json(json!({"accounts": accounts, "max": MAX_PARKED + 1})).into_response();
    set_parked(&state, &mut response, &tokens);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Sets the active account aside to sign in to another one: the browser is
/// signed out, the session stays open in the parked list.
async fn park(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let identity = match active(&state, &headers).await {
        Ok(Some(identity)) => identity,
        Ok(None) => return failure(StatusCode::UNAUTHORIZED, "authentication_required"),
        Err((status, code)) => return failure(status, code),
    };
    let Some(raw) = session_token(&headers) else {
        return failure(StatusCode::UNAUTHORIZED, "authentication_required");
    };
    let mut tokens: Vec<String> =
        standing(&state, parked(&headers), Some(identity.user.id.as_str()))
            .await
            .into_iter()
            .map(|(token, _)| token)
            .collect();
    if tokens.len() >= MAX_PARKED {
        return failure(StatusCode::CONFLICT, "account_limit");
    }
    tokens.insert(0, raw.to_owned());
    let mut response = StatusCode::NO_CONTENT.into_response();
    set_cookie(&mut response, state.cookie("", 0));
    set_parked(&state, &mut response, &tokens);
    response
}

#[derive(Deserialize)]
struct Target {
    #[serde(rename = "userId")]
    user_id: String,
    /// Short PIN of the chosen account, when it has one (`switch` only).
    #[serde(default)]
    pin: Option<String>,
}

/// Makes a parked account the active one; the account that was active, if
/// any, takes its place in the list.
async fn switch(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(target): Json<Target>,
) -> Response {
    let current = match active(&state, &headers).await {
        Ok(current) => current,
        Err((status, code)) => return failure(status, code),
    };
    let others = standing(
        &state,
        parked(&headers),
        current.as_ref().map(|i| i.user.id.as_str()),
    )
    .await;
    let Some(position) = others.iter().position(|(_, u)| u.id == target.user_id) else {
        return failure(StatusCode::NOT_FOUND, "account_not_found");
    };
    // The chosen session already stands; on a shared device its PIN stands in for the password.
    if let Err(refused) =
        crate::profiles::check_switch_pin(&state, &target.user_id, target.pin.clone()).await
    {
        return refused.into_response();
    }
    let mut tokens: Vec<String> = others.into_iter().map(|(token, _)| token).collect();
    let chosen = tokens.remove(position);
    if current.is_some()
        && let Some(raw) = session_token(&headers)
    {
        tokens.insert(0, raw.to_owned());
    }
    let mut as_chosen = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("{COOKIE}={chosen}")) {
        as_chosen.insert(header::COOKIE, value);
    }
    let identity = match state.authenticate(&as_chosen).await {
        Ok(identity) => identity,
        Err(status) => return failure(status, "account_not_found"),
    };
    let mut response =
        Json(json!({"user": identity.user, "csrfToken": identity.csrf_token})).into_response();
    set_cookie(&mut response, state.cookie(&chosen, ABSOLUTE_SECONDS));
    set_parked(&state, &mut response, &tokens);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Signs a parked account out of this browser.
async fn forget(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(target): Json<Target>,
) -> Response {
    let current = match active(&state, &headers).await {
        Ok(current) => current,
        Err((status, code)) => return failure(status, code),
    };
    let others = standing(
        &state,
        parked(&headers),
        current.as_ref().map(|i| i.user.id.as_str()),
    )
    .await;
    let mut tokens = Vec::new();
    for (token, user) in others {
        if user.id == target.user_id {
            close(&state, &token).await;
        } else {
            tokens.push(token);
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    set_parked(&state, &mut response, &tokens);
    response
}
