//! Browser pairing: a public approval code never grants the requesting session.
use super::{
    AuthState,
    util::{
        ABSOLUTE_SECONDS, COOKIE, IDLE_SECONDS, TokenType, device_label, extract_client_key, hash,
        random_token,
    },
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::json;

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route("/api/v1/auth/device/start", post(start))
        .route("/api/v1/auth/device/poll", post(poll))
        .route("/api/v1/auth/device/inspect", post(inspect))
        .route("/api/v1/auth/device/approve", post(approve))
}
type Error = (StatusCode, Json<serde_json::Value>);
fn fail(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error":code})))
}
fn db(_: sqlx::Error) -> Error {
    fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}
async fn throttle(state: &AuthState, key: String) -> Result<(), Error> {
    let allowed: Option<i64> = crate::database::query_scalar(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES(?,unixepoch(),1) \
            ON CONFLICT(throttle_key) DO UPDATE \
            SET window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 \
            THEN unixepoch() ELSE login_throttle.window_started END, attempts=CASE \
            WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 \
            ELSE login_throttle.attempts+1 END \
            WHERE login_throttle.window_started<=unixepoch()-60 OR login_throttle.attempts<10 \
            RETURNING attempts",
    )
    .bind(key)
    .fetch_optional(&state.store.pool)
    .await
    .map_err(db)?;
    allowed
        .map(|_| ())
        .ok_or_else(|| fail(StatusCode::TOO_MANY_REQUESTS, "try_later"))
}
async fn start(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, Error> {
    // Starting grants nothing; native applications (TV, phone) start without Origin.
    state
        .check_origin_or_native(&headers)
        .map_err(|s| fail(s, "forbidden"))?;
    throttle(
        &state,
        format!("device:start:{}", extract_client_key(&headers)),
    )
    .await?;
    crate::database::query("DELETE FROM device_connections WHERE expires_at<=unixepoch()")
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    let secret = random_token();
    let code = random_token()[..8].to_ascii_uppercase();
    let created = crate::database::query("INSERT INTO device_connections(code_hash,requester_hash,device_label,expires_at) SELECT ?,?,?,unixepoch()+300 WHERE (SELECT COUNT(*) FROM device_connections)<256")
        .bind(hash(&code)).bind(hash(&secret)).bind(device_label(&headers)).execute(&state.store.pool).await.map_err(db)?;
    if created.rows_affected() != 1 {
        return Err(fail(StatusCode::TOO_MANY_REQUESTS, "try_later"));
    }
    // Fragment is not sent in HTTP request URLs, access logs or Referer headers.
    let url = format!("{}/app/connect/#{}", state.origin(), code);
    let qr = qrcode::QrCode::new(url.as_bytes())
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?;
    let width = qr.width();
    let modules: Vec<bool> = qr
        .into_colors()
        .into_iter()
        .map(|color| color == qrcode::Color::Dark)
        .collect();
    Ok(Json(
        json!({"secret":secret,"code":code,"expiresIn":300,"url":url,"qrWidth":width,"qrModules":modules}),
    ))
}
#[derive(Deserialize)]
struct Code {
    code: String,
}
fn normalize_code(input: &str) -> Result<String, Error> {
    let code = input.trim().replace([' ', '-'], "").to_ascii_uppercase();
    if code.len() != 8 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail(StatusCode::BAD_REQUEST, "invalid_code"));
    }
    Ok(code)
}
async fn actor(state: &AuthState, headers: &HeaderMap) -> Result<super::Authenticated, Error> {
    let actor = state
        .authenticate(headers)
        .await
        .map_err(|s| fail(s, "authentication_required"))?;
    state
        .require_csrf(headers, &actor)
        .map_err(|s| fail(s, "forbidden"))?;
    throttle(state, format!("device:approve:{}", actor.token_hash)).await?;
    Ok(actor)
}
async fn inspect(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Code>,
) -> Result<Json<serde_json::Value>, Error> {
    actor(&state, &headers).await?;
    let code = normalize_code(&input.code)?;
    let device: Option<String> = crate::database::query_scalar("SELECT device_label FROM device_connections WHERE code_hash=? AND expires_at>unixepoch() AND approver_session_hash IS NULL")
        .bind(hash(&code)).fetch_optional(&state.store.pool).await.map_err(db)?;
    let device = device.ok_or_else(|| fail(StatusCode::NOT_FOUND, "invalid_code"))?;
    Ok(Json(json!({"device":device})))
}
async fn approve(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Code>,
) -> Result<StatusCode, Error> {
    let actor = actor(&state, &headers).await?;
    let code = normalize_code(&input.code)?;
    let updated=crate::database::query("UPDATE device_connections \
            SET approver_session_hash=? \
            WHERE code_hash=? AND expires_at>unixepoch() AND approver_session_hash IS NULL \
            AND EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? \
            AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-? \
            AND u.disabled_at IS NULL)")
        .bind(&actor.token_hash).bind(hash(&code)).bind(&actor.token_hash).bind(IDLE_SECONDS).execute(&state.store.pool).await.map_err(db)?;
    if updated.rows_affected() != 1 {
        return Err(fail(StatusCode::NOT_FOUND, "invalid_code"));
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct Poll {
    secret: String,
    #[serde(default, rename = "tokenType")]
    token_type: TokenType,
}
async fn poll(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Poll>,
) -> Result<Response, Error> {
    // A cookie is only ever set for the primary origin (no login CSRF); a bearer
    // token is returned in the body to the holder of the requester secret.
    match input.token_type {
        TokenType::Cookie => state.check_origin(&headers),
        TokenType::Bearer => state.check_origin_or_native(&headers),
    }
    .map_err(|s| fail(s, "forbidden"))?;
    if input.secret.len() != 64 || !input.secret.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(fail(StatusCode::BAD_REQUEST, "invalid_request"));
    }
    let requester = hash(&input.secret);
    let pending: Option<(Option<String>,)> = crate::database::query_as("SELECT approver_session_hash FROM device_connections WHERE requester_hash=? AND expires_at>unixepoch()")
        .bind(&requester).fetch_optional(&state.store.pool).await.map_err(db)?;
    let Some((approver,)) = pending else {
        return Err(fail(StatusCode::GONE, "device_request_expired"));
    };
    let Some(approver) = approver else {
        return Ok(Json(json!({"status":"pending"})).into_response());
    };
    // Revalidate the live originating session, including account restrictions, for SSO and local accounts alike.
    state
        .session_from_hash(&approver)
        .await
        .map_err(|s| fail(s, "authentication_required"))?;
    let raw = random_token();
    let mut conn = state.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let device: Option<String>=crate::database::query_scalar("DELETE FROM device_connections WHERE requester_hash=? AND approver_session_hash=? AND expires_at>unixepoch() RETURNING device_label")
        .bind(&requester).bind(&approver).fetch_optional(&mut *tx).await.map_err(db)?;
    let device = device.ok_or_else(|| fail(StatusCode::GONE, "device_request_expired"))?;
    let created = crate::database::query(
        "INSERT INTO sessions(token_hash,user_id,device_label,created_at,last_seen_at,expires_at) \
            SELECT ?,s.user_id,?,unixepoch(),unixepoch(),unixepoch()+? \
            FROM sessions s \
            JOIN users u ON u.id=s.user_id \
            WHERE s.token_hash=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-? \
            AND u.disabled_at IS NULL",
    )
    .bind(hash(&raw))
    .bind(device)
    .bind(ABSOLUTE_SECONDS)
    .bind(&approver)
    .bind(IDLE_SECONDS)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if created.rows_affected() != 1 {
        return Err(fail(StatusCode::UNAUTHORIZED, "authentication_required"));
    }
    tx.commit().await.map_err(db)?;
    if input.token_type == TokenType::Bearer {
        let user = state
            .session_from_hash(&hash(&raw))
            .await
            .map_err(|s| fail(s, "authentication_required"))?;
        return Ok(super::http::bearer_grant(user, &raw, Some("authenticated")));
    }
    let mut auth_headers = HeaderMap::new();
    auth_headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{COOKIE}={raw}"))
            .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?,
    );
    let identity = state
        .authenticate(&auth_headers)
        .await
        .map_err(|s| fail(s, "authentication_required"))?;
    let mut response = Json(
        json!({"status":"authenticated","user":identity.user,"csrfToken":identity.csrf_token}),
    )
    .into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&state.cookie(&raw, ABSOLUTE_SECONDS))
            .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?,
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    const ORIGIN: &str = "https://media.example";
    async fn setup() -> (tempfile::TempDir, AuthState, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&temp.path().join("device.db"))
            .await
            .unwrap();
        // SSO-only user: there is deliberately no local credential.
        crate::database::query("INSERT INTO users(id,role,display_name,sso_username) VALUES('sso-user','user','SSO member','sso-member')").execute(&store.pool).await.unwrap();
        let token = random_token();
        crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'sso-user',unixepoch(),unixepoch(),unixepoch()+3600)").bind(hash(&token)).execute(&store.pool).await.unwrap();
        let state = AuthState::new(store, ORIGIN.into()).await.unwrap();
        (
            temp,
            state,
            format!("{COOKIE}={token}"),
            super::super::util::csrf(&token),
        )
    }
    async fn call(
        state: &AuthState,
        action: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
        csrf: Option<&str>,
        origin: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/auth/device/{action}"))
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(value) = cookie {
            request = request.header(header::COOKIE, value);
        }
        if let Some(value) = csrf {
            request = request.header("x-csrf-token", value);
        }
        if let Some(value) = origin {
            request = request.header(header::ORIGIN, value);
        }
        super::super::http::router(state.clone())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
    async fn body(response: Response) -> serde_json::Value {
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }
    async fn start(state: &AuthState) -> serde_json::Value {
        let response = call(state, "start", json!({}), None, None, Some(ORIGIN)).await;
        assert_eq!(response.status(), StatusCode::OK);
        body(response).await
    }
    #[tokio::test]
    async fn sso_only_account_approves_once_and_requester_secret_is_required() {
        let (_temp, state, cookie, csrf) = setup().await;
        let challenge = start(&state).await;
        assert!(
            challenge["url"]
                .as_str()
                .unwrap()
                .starts_with("https://media.example/app/connect/#")
        );
        assert!(
            !challenge["url"]
                .as_str()
                .unwrap()
                .contains(challenge["secret"].as_str().unwrap())
        );
        let width = challenge["qrWidth"].as_u64().unwrap();
        assert_eq!(
            challenge["qrModules"].as_array().unwrap().len(),
            (width * width) as usize
        );
        let row: (String, String) =
            crate::database::query_as("SELECT code_hash,requester_hash FROM device_connections")
                .fetch_one(&state.store.pool)
                .await
                .unwrap();
        assert_ne!(row.0, challenge["code"].as_str().unwrap());
        assert_ne!(row.1, challenge["secret"].as_str().unwrap());
        let code = json!({"code":challenge["code"]});
        let poll = json!({"secret":challenge["secret"]});
        assert_eq!(
            body(call(&state, "poll", poll.clone(), None, None, Some(ORIGIN)).await).await["status"],
            "pending"
        );
        assert_eq!(
            call(&state, "approve", code.clone(), None, None, Some(ORIGIN))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(
                &state,
                "approve",
                code.clone(),
                Some(&cookie),
                None,
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &state,
                "inspect",
                code.clone(),
                Some(&cookie),
                Some(&csrf),
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            call(
                &state,
                "approve",
                code.clone(),
                Some(&cookie),
                Some(&csrf),
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(
                &state,
                "approve",
                code,
                Some(&cookie),
                Some(&csrf),
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &state,
                "poll",
                json!({"secret":random_token()}),
                None,
                None,
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::GONE
        );
        let (first, second) = tokio::join!(
            call(&state, "poll", poll.clone(), None, None, Some(ORIGIN)),
            call(&state, "poll", poll, None, None, Some(ORIGIN))
        );
        let successful = if first.status() == StatusCode::OK {
            assert_eq!(second.status(), StatusCode::GONE);
            first
        } else {
            assert_eq!(first.status(), StatusCode::GONE);
            assert_eq!(second.status(), StatusCode::OK);
            second
        };
        assert!(
            successful.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("HttpOnly; SameSite=Strict")
        );
        assert!(
            successful.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Secure")
        );
        let payload = body(successful).await;
        assert_eq!(payload["user"]["id"], "sso-user");
        assert_eq!(payload["user"]["sso"], true);
        assert_eq!(payload["user"]["role"], "user");
        let count: i64 =
            crate::database::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id='sso-user'")
                .fetch_one(&state.store.pool)
                .await
                .unwrap();
        assert_eq!(count, 2);
    }
    #[tokio::test]
    async fn expired_disabled_and_revoked_requests_cannot_create_sessions() {
        let (_temp, state, cookie, csrf) = setup().await;
        let challenge = start(&state).await;
        crate::database::query("UPDATE device_connections SET expires_at=unixepoch()-1")
            .execute(&state.store.pool)
            .await
            .unwrap();
        assert_eq!(
            call(
                &state,
                "approve",
                json!({"code":challenge["code"]}),
                Some(&cookie),
                Some(&csrf),
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &state,
                "poll",
                json!({"secret":challenge["secret"]}),
                None,
                None,
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::GONE
        );
        for revoked in [false, true] {
            let challenge = start(&state).await;
            assert_eq!(
                call(
                    &state,
                    "approve",
                    json!({"code":challenge["code"]}),
                    Some(&cookie),
                    Some(&csrf),
                    Some(ORIGIN)
                )
                .await
                .status(),
                StatusCode::NO_CONTENT
            );
            if revoked {
                crate::database::query("DELETE FROM sessions")
                    .execute(&state.store.pool)
                    .await
                    .unwrap();
            } else {
                crate::database::query("UPDATE users SET disabled_at=unixepoch()")
                    .execute(&state.store.pool)
                    .await
                    .unwrap();
            }
            let response = call(
                &state,
                "poll",
                json!({"secret":challenge["secret"]}),
                None,
                None,
                Some(ORIGIN),
            )
            .await;
            assert!(matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::GONE
            ));
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            if !revoked {
                crate::database::query("UPDATE users SET disabled_at=NULL")
                    .execute(&state.store.pool)
                    .await
                    .unwrap();
            }
        }
    }
    async fn with_bearer(
        state: &AuthState,
        action: &str,
        token: &str,
        body: serde_json::Value,
    ) -> Response {
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/auth/device/{action}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(body.to_string()))
            .unwrap();
        super::super::http::router(state.clone())
            .oneshot(request)
            .await
            .unwrap()
    }
    #[tokio::test]
    async fn native_device_flow_returns_a_bearer_token_and_never_a_cookie() {
        let (_temp, state, cookie, csrf) = setup().await;
        let challenge = body(call(&state, "start", json!({}), None, None, None).await).await;
        let code = json!({"code":challenge["code"]});
        let bearer_poll = json!({"secret":challenge["secret"],"tokenType":"bearer"});
        assert_eq!(
            call(
                &state,
                "approve",
                code.clone(),
                Some(&cookie),
                Some(&csrf),
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        // A cookie still needs the primary origin; a bearer token is refused to a foreign page.
        assert_eq!(
            call(
                &state,
                "poll",
                json!({"secret":challenge["secret"]}),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &state,
                "poll",
                bearer_poll.clone(),
                None,
                None,
                Some("https://attacker.example")
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let granted = call(&state, "poll", bearer_poll.clone(), None, None, None).await;
        assert_eq!(granted.status(), StatusCode::OK);
        assert!(!granted.headers().contains_key(header::SET_COOKIE));
        assert_eq!(granted.headers()[header::CACHE_CONTROL], "no-store");
        let granted = body(granted).await;
        assert_eq!(granted["status"], "authenticated");
        assert_eq!(granted["tokenType"], "bearer");
        assert_eq!(granted["user"]["id"], "sso-user");
        assert!(granted.get("csrfToken").is_none());
        let token = granted["accessToken"].as_str().unwrap().to_owned();
        assert_eq!(token.len(), 64);
        assert_eq!(
            call(&state, "poll", bearer_poll, None, None, None)
                .await
                .status(),
            StatusCode::GONE
        );
        // The phone, signed in by bearer, approves a TV without cookie, CSRF or Origin.
        let tv = start(&state).await;
        let tv_code = json!({"code":tv["code"]});
        assert_eq!(
            with_bearer(&state, "inspect", &token, tv_code.clone())
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            with_bearer(&state, "approve", &token, tv_code)
                .await
                .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            with_bearer(
                &state,
                "approve",
                &random_token(),
                json!({"code":"1234ABCD"})
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn origins_and_persistent_attempt_limits_are_enforced() {
        let (temp, state, cookie, csrf) = setup().await;
        // Without Origin, start is the native path (covered below) and is allowed.
        assert_eq!(
            call(
                &state,
                "start",
                json!({}),
                None,
                None,
                Some("https://attacker.example")
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        for origin in [None, Some("https://attacker.example")] {
            assert_eq!(
                call(
                    &state,
                    "poll",
                    json!({"secret":random_token()}),
                    None,
                    None,
                    origin
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                call(
                    &state,
                    "approve",
                    json!({"code":"1234ABCD"}),
                    Some(&cookie),
                    Some(&csrf),
                    origin
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        for _ in 0..10 {
            assert_eq!(
                call(
                    &state,
                    "inspect",
                    json!({"code":"1234ABCD"}),
                    Some(&cookie),
                    Some(&csrf),
                    Some(ORIGIN)
                )
                .await
                .status(),
                StatusCode::NOT_FOUND
            );
        }
        let reopened = crate::identity::IdentityStore::open(&temp.path().join("device.db"))
            .await
            .unwrap();
        let recreated = AuthState::new(reopened, ORIGIN.into()).await.unwrap();
        assert_eq!(
            call(
                &recreated,
                "approve",
                json!({"code":"1234ABCD"}),
                Some(&cookie),
                Some(&csrf),
                Some(ORIGIN)
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        for _ in 0..10 {
            start(&state).await;
        }
        assert_eq!(
            call(&recreated, "start", json!({}), None, None, Some(ORIGIN))
                .await
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}
