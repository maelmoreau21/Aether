use super::{
    http::router,
    state::{AuthConfigError, AuthState},
    util::{COOKIE, hash},
};
use crate::identity::{IdentityStore, Role};
use axum::{body::Body, http::Request};
use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use std::time::Duration;
use tower::ServiceExt;
use zeroize::Zeroizing;
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
#[allow(clippy::too_many_arguments)]
async fn request_with_ip(
    state: &AuthState,
    path: &str,
    method: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    csrf: Option<&str>,
    ip: Option<&str>,
    body: serde_json::Value,
) -> Response {
    let mut req = Request::builder()
        .uri(path)
        .method(method)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(v) = cookie {
        req = req.header(header::COOKIE, v);
    }
    if let Some(v) = origin {
        req = req.header(header::ORIGIN, v);
    }
    if let Some(v) = csrf {
        req = req.header("x-csrf-token", v);
    }
    if let Some(v) = ip {
        req = req.header("x-forwarded-for", v);
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
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["user"]["role"], "administrator");
    (
        cookie.split(';').next().unwrap().into(),
        body["csrfToken"].as_str().unwrap().into(),
    )
}
/// `name=value` of a cookie the response sets.
fn cookie_set(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with(&format!("{name}=")))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
#[tokio::test]
async fn several_accounts_share_a_browser_and_switch_without_signing_in_again() {
    let (_temp, state) = setup().await;
    let (first, first_csrf) = sign_in(&state).await;
    crate::database::query(
        "INSERT INTO users(id,role,display_name) VALUES('second','user','Second')",
    )
    .execute(&state.store.pool)
    .await
    .unwrap();
    let second_raw = super::util::random_token();
    crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'second',unixepoch(),unixepoch(),unixepoch()+3600)")
        .bind(hash(&second_raw))
        .execute(&state.store.pool)
        .await
        .unwrap();
    let call =
        |path: &'static str, cookies: String, csrf: Option<String>, body: serde_json::Value| {
            let state = state.clone();
            async move {
                request(
                    &state,
                    path,
                    if path.ends_with("/accounts") {
                        "GET"
                    } else {
                        "POST"
                    },
                    Some(&cookies),
                    Some(ORIGIN),
                    csrf.as_deref(),
                    body,
                )
                .await
            }
        };
    let null = serde_json::Value::Null;

    // Setting the first account aside signs the browser out and keeps its session.
    let denied = call(
        "/api/v1/auth/accounts/park",
        first.clone(),
        None,
        null.clone(),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let parked = call(
        "/api/v1/auth/accounts/park",
        first.clone(),
        Some(first_csrf),
        null.clone(),
    )
    .await;
    assert_eq!(parked.status(), StatusCode::NO_CONTENT);
    assert_eq!(cookie_set(&parked, "aether_session"), "aether_session=");
    let aside = cookie_set(&parked, "aether_accounts");
    assert_eq!(
        aside,
        format!("aether_accounts={}", first.split('=').nth(1).unwrap())
    );

    // Signed in as the second account: both are listed, the active one first.
    let second = format!("aether_session={second_raw}");
    let both = format!("{second}; {aside}");
    let listed = call("/api/v1/auth/accounts", both.clone(), None, null.clone()).await;
    let listed: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(listed.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let accounts = listed["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["user"]["id"], "second");
    assert_eq!(accounts[0]["current"], true);
    assert_eq!(accounts[1]["current"], false);
    let first_id = accounts[1]["user"]["id"].as_str().unwrap().to_owned();

    // Switching swaps the two cookies and answers with the new identity.
    let second_csrf = super::util::csrf(&second_raw);
    let target = serde_json::json!({"userId": first_id});
    let denied = call(
        "/api/v1/auth/accounts/switch",
        both.clone(),
        None,
        target.clone(),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let switched = call(
        "/api/v1/auth/accounts/switch",
        both.clone(),
        Some(second_csrf.clone()),
        target.clone(),
    )
    .await;
    assert_eq!(switched.status(), StatusCode::OK);
    assert_eq!(cookie_set(&switched, "aether_session"), first);
    assert_eq!(
        cookie_set(&switched, "aether_accounts"),
        format!("aether_accounts={second_raw}")
    );
    let unknown = call(
        "/api/v1/auth/accounts/switch",
        both.clone(),
        Some(second_csrf.clone()),
        serde_json::json!({"userId":"nobody"}),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    // Signed out, the sign-in page still offers the parked account, and can enter it.
    let offered = call(
        "/api/v1/auth/accounts/switch",
        aside.clone(),
        None,
        target.clone(),
    )
    .await;
    assert_eq!(offered.status(), StatusCode::OK);
    assert_eq!(cookie_set(&offered, "aether_session"), first);

    // With a short PIN, the parked account opens only with it, and the list says so.
    let encoded = crate::profiles::hash_pin(&state, "2468".into())
        .await
        .unwrap();
    crate::database::query(
        "INSERT INTO account_profiles(user_id,owner_id,pin_hash,created_at) VALUES(?,?,?,unixepoch())",
    )
    .bind(&first_id)
    .bind(&first_id)
    .bind(encoded)
    .execute(&state.store.pool)
    .await
    .unwrap();
    let listed = call("/api/v1/auth/accounts", aside.clone(), None, null.clone()).await;
    let listed: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(listed.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(listed["accounts"][0]["hasPin"], true);
    for (body, status, code) in [
        (target.clone(), StatusCode::FORBIDDEN, Some("pin_required")),
        (
            serde_json::json!({"userId": first_id, "pin": "1357"}),
            StatusCode::FORBIDDEN,
            Some("invalid_pin"),
        ),
        (
            serde_json::json!({"userId": first_id, "pin": "2468"}),
            StatusCode::OK,
            None,
        ),
    ] {
        let answered = call("/api/v1/auth/accounts/switch", aside.clone(), None, body).await;
        assert_eq!(answered.status(), status);
        if let Some(code) = code {
            let error: serde_json::Value = serde_json::from_slice(
                &axum::body::to_bytes(answered.into_body(), usize::MAX)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(error["error"], code);
        } else {
            assert_eq!(cookie_set(&answered, "aether_session"), first);
        }
    }

    // Forgetting a parked account closes its session.
    let forgotten = call(
        "/api/v1/auth/accounts/forget",
        both,
        Some(second_csrf),
        target,
    )
    .await;
    assert_eq!(forgotten.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        cookie_set(&forgotten, "aether_accounts"),
        "aether_accounts="
    );
    let left: Vec<String> = crate::database::query_scalar("SELECT user_id FROM sessions")
        .fetch_all(&state.store.pool)
        .await
        .unwrap();
    assert_eq!(left, ["second"]);
}
#[tokio::test]
async fn a_cookie_session_is_extended_only_with_its_csrf_token() {
    let (_temp, state) = setup().await;
    let (cookie, csrf) = sign_in(&state).await;
    crate::database::query("UPDATE sessions SET expires_at=unixepoch()+60")
        .execute(&state.store.pool)
        .await
        .unwrap();
    let extend = |csrf| {
        request(
            &state,
            "/api/v1/auth/session/extend",
            "POST",
            Some(&cookie),
            Some(ORIGIN),
            csrf,
            serde_json::json!(null),
        )
    };
    assert_eq!(extend(None).await.status(), StatusCode::FORBIDDEN);
    let extended = extend(Some(&csrf)).await;
    assert_eq!(extended.status(), StatusCode::NO_CONTENT);
    let renewed = extended.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(renewed.starts_with(&cookie) && renewed.contains("Max-Age=2592000"));
    let left: i64 = crate::database::query_scalar("SELECT expires_at-unixepoch() FROM sessions")
        .fetch_one(&state.store.pool)
        .await
        .unwrap();
    assert!(left > 2_591_000);
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
    let hashes: Vec<String> = crate::database::query_scalar("SELECT token_hash FROM sessions")
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
    let headers = HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
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
        crate::database::query(query)
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
    crate::database::query("UPDATE users SET disabled_at=unixepoch()")
        .execute(&reopened.store.pool)
        .await
        .unwrap();
    let headers = HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
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
    crate::database::query("UPDATE login_throttle SET attempts=8")
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
    let reopened_store = IdentityStore::open(&temp.path().join("auth.sqlite3"))
        .await
        .unwrap();
    // Keep throttle window fresh across test restarts under heavy CI load
    crate::database::query("UPDATE login_throttle SET window_started = unixepoch()")
        .execute(&reopened_store.pool)
        .await
        .unwrap();
    let restarted = AuthState::new(reopened_store, ORIGIN.into()).await.unwrap();
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

    // Different client IP is NOT affected by the throttle on 127.0.0.1 (Anti-DoS)
    let other_ip_res = request_with_ip(
        &restarted,
        "/api/v1/auth/login",
        "POST",
        None,
        Some(ORIGIN),
        None,
        Some("198.51.100.42"),
        serde_json::json!({"login":"admin","password":PASSWORD}),
    )
    .await;
    assert_eq!(other_ip_res.status(), StatusCode::OK);

    crate::database::query("UPDATE login_throttle SET window_started=0")
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

#[tokio::test]
async fn edge_cases_coalescing_expiry_saturation_logout_and_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("auth_edges.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    let admin_id = store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    // Configure max_pending_touches = 4 to test saturation and backpressure deterministically.
    let state = AuthState::new_with_touch_config(store.clone(), ORIGIN.into(), 60, 4)
        .await
        .unwrap();
    let (cookie, csrf) = sign_in(&state).await;
    let raw = cookie.split('=').nth(1).unwrap();
    let token_hash = hash(raw);
    let headers = HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);

    // 1. Fresh session (< 60s old): 20 concurrent requests cause 0 SQLite writes.
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let s = state.clone();
        let h = headers.clone();
        tasks.push(tokio::spawn(
            async move { s.authenticate(&h).await.unwrap() },
        ));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap().user.role, Role::Administrator);
    }
    assert_eq!(state.touch_stats().write_batches_succeeded, 0);
    assert_eq!(state.touch_stats().pending_count, 0);

    // 2. Activity near idle expiration boundary (last_seen_at = now - 604795s, 5s before 604800s limit):
    // 30 concurrent requests from the same user coalesce into 1 single SQLite write batch.
    crate::database::query(
        "UPDATE sessions SET last_seen_at = unixepoch() - 604795 WHERE token_hash = ?",
    )
    .bind(&token_hash)
    .execute(&store.pool)
    .await
    .unwrap();
    let before_seen: i64 =
        crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
            .bind(&token_hash)
            .fetch_one(&store.pool)
            .await
            .unwrap();

    let mut concurrent = Vec::new();
    for _ in 0..30 {
        let s = state.clone();
        let h = headers.clone();
        concurrent.push(tokio::spawn(
            async move { s.authenticate(&h).await.unwrap() },
        ));
    }
    for t in concurrent {
        t.await.unwrap();
    }
    state.flush_session_touches().await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let after_seen: i64 =
        crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
            .bind(&token_hash)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert!(
        after_seen > before_seen + 1700,
        "Session near expiration must be refreshed to current unixepoch"
    );
    assert_eq!(
        state.touch_stats().write_batches_succeeded,
        1,
        "30 concurrent requests must coalesce into exactly 1 SQLite write batch"
    );
    assert_eq!(state.touch_stats().rows_updated, 1);

    // 3. Delayed touch never revives a session that expired while SQLite was write-locked:
    let expired_raw = "e".repeat(64);
    let expired_hash = hash(&expired_raw);
    crate::database::query(
        "INSERT INTO sessions(token_hash, user_id, created_at, last_seen_at, expires_at) \
         VALUES(?, ?, unixepoch() - 2000, unixepoch() - 604790, unixepoch() + 3600)",
    )
    .bind(&expired_hash)
    .bind(&admin_id)
    .execute(&store.pool)
    .await
    .unwrap();

    let mut lock_conn = store.pool.acquire().await.unwrap();
    let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();

    let exp_headers = HeaderMap::from_iter([(
        header::COOKIE,
        HeaderValue::from_str(&format!("{COOKIE}={expired_raw}")).unwrap(),
    )]);
    // Still valid during the lock (604790 < 604800) -> queues touch in memory
    assert!(state.authenticate(&exp_headers).await.is_ok());
    assert_eq!(state.touch_stats().pending_count, 1);
    lock_tx.rollback().await.unwrap();
    drop(lock_conn);

    // Simulate session crossing the 604800s boundary before worker flushes
    crate::database::query(
        "UPDATE sessions SET last_seen_at = unixepoch() - 604805 WHERE token_hash = ?",
    )
    .bind(&expired_hash)
    .execute(&store.pool)
    .await
    .unwrap();
    state.flush_session_touches().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        matches!(
            state.authenticate(&exp_headers).await,
            Err(StatusCode::UNAUTHORIZED)
        ),
        "Deferred touch must NEVER revive an already-expired session"
    );

    // 4. Queue saturation (max_pending = 4) under write lock:
    let mut lock_conn = store.pool.acquire().await.unwrap();
    let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();

    for i in 0..8u8 {
        let tok = format!("{:02x}", i).repeat(32);
        state.toucher.shared.record(&hash(&tok), 2_000_000_000);
    }
    let stats_under_lock = state.touch_stats();
    assert_eq!(
        stats_under_lock.pending_count, 4,
        "Queue must be strictly bounded to 4"
    );
    assert_eq!(
        stats_under_lock.dropped_overflow, 4,
        "Overflow touches beyond capacity must be dropped with backpressure counter"
    );
    // Reads still succeed immediately while queue is saturated and DB is write-locked!
    assert!(state.authenticate(&headers).await.is_ok());

    lock_tx.rollback().await.unwrap();
    drop(lock_conn);
    state.flush_session_touches().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(state.touch_stats().pending_count, 0);

    // 5. Logout while an activity update is pending:
    crate::database::query(
        "UPDATE sessions SET last_seen_at = unixepoch() - 300 WHERE token_hash = ?",
    )
    .bind(&token_hash)
    .execute(&store.pool)
    .await
    .unwrap();
    let mut lock_conn = store.pool.acquire().await.unwrap();
    let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();
    assert!(state.authenticate(&headers).await.is_ok());
    assert_eq!(state.touch_stats().pending_count, 1);
    lock_tx.rollback().await.unwrap();
    drop(lock_conn);

    assert_eq!(
        request(
            &state,
            "/api/v1/auth/logout",
            "POST",
            Some(&cookie),
            Some(ORIGIN),
            Some(&csrf),
            serde_json::json!(null),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        state.touch_stats().pending_count,
        0,
        "Logout must immediately cancel pending touch"
    );
    state.flush_session_touches().await;
    assert!(
        matches!(
            state.authenticate(&headers).await,
            Err(StatusCode::UNAUTHORIZED)
        ),
        "Logged-out session must remain revoked after flush"
    );

    // 6. Server restart before activity persistence (unflushed touch in memory):
    let (restart_cookie, _) = sign_in(&state).await;
    let restart_raw = restart_cookie.split('=').nth(1).unwrap();
    let restart_hash = hash(restart_raw);
    let restart_headers = HeaderMap::from_iter([(
        header::COOKIE,
        HeaderValue::from_str(&restart_cookie).unwrap(),
    )]);
    crate::database::query(
        "UPDATE sessions SET last_seen_at = unixepoch() - 600000 WHERE token_hash = ?",
    )
    .bind(&restart_hash)
    .execute(&store.pool)
    .await
    .unwrap();
    let persisted_before_restart: i64 =
        crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
            .bind(&restart_hash)
            .fetch_one(&store.pool)
            .await
            .unwrap();

    // Hold write lock so touch stays in memory, then simulate abrupt restart before flush
    let mut lock_conn = store.pool.acquire().await.unwrap();
    let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();
    assert!(state.authenticate(&restart_headers).await.is_ok());
    assert_eq!(state.touch_stats().pending_count, 1);
    drop(state);
    lock_tx.rollback().await.unwrap();
    drop(lock_conn);
    store.close().await;

    let reopened_store = IdentityStore::open(&db_path).await.unwrap();
    let reopened_state = AuthState::new(reopened_store.clone(), ORIGIN.into())
        .await
        .unwrap();
    let persisted_after_restart: i64 =
        crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
            .bind(&restart_hash)
            .fetch_one(&reopened_store.pool)
            .await
            .unwrap();
    assert_eq!(
        persisted_before_restart, persisted_after_restart,
        "Unpersisted in-memory touch is discarded on abrupt restart"
    );
    // Because persisted last_seen_at is still < 604800s old, session remains valid after restart
    assert!(reopened_state.authenticate(&restart_headers).await.is_ok());
}

#[tokio::test]
async fn origin_configuration_http_loopback_http_lan_and_https_matrix() {
    let (_temp, base_state) = setup().await;
    let store = base_state.store.clone();

    // 1. HTTP loopback origins (127.0.0.1, localhost, [::1], with or without trailing slash) are accepted
    for (input, canonical) in [
        ("http://127.0.0.1:3000", "http://127.0.0.1:3000"),
        ("http://127.0.0.1:3000/", "http://127.0.0.1:3000"),
        ("http://localhost:3000", "http://localhost:3000"),
        ("http://[::1]:3000", "http://[::1]:3000"),
    ] {
        let state = AuthState::new(store.clone(), input.into()).await.unwrap();
        assert_eq!(state.origin(), canonical);
        assert!(!state.cookie("tok", 300).contains("; Secure"));
    }

    // 2. HTTP LAN and non-loopback origins are explicitly rejected with AuthConfigError::Origin
    for rejected in [
        "http://192.168.1.100:3000",
        "http://10.0.0.10:3000",
        "http://172.16.0.5:3000",
        "http://0.0.0.0:3000",
        "http://nas.local:3000",
        "http://aether.mondomaine.fr",
        "https://admin@aether.mondomaine.fr",
        "https://aether.mondomaine.fr/app",
        "https://aether.mondomaine.fr?foo=bar",
        "ws://127.0.0.1:3000",
    ] {
        assert!(
            matches!(
                AuthState::new(store.clone(), rejected.into()).await,
                Err(AuthConfigError::Origin)
            ),
            "Expected AuthConfigError::Origin for {rejected}"
        );
    }

    // 3. HTTPS LAN / Internet origins (standard port 443 and custom port e.g. 8443 on Synology DSM) are accepted
    for (input, canonical) in [
        (
            "https://aether.mondomaine.fr",
            "https://aether.mondomaine.fr",
        ),
        (
            "https://aether.mondomaine.fr/",
            "https://aether.mondomaine.fr",
        ),
        ("https://aether.nas.lan:8443", "https://aether.nas.lan:8443"),
        ("https://192.168.1.100:8443", "https://192.168.1.100:8443"),
        ("https://[::1]:8443", "https://[::1]:8443"),
    ] {
        let state = AuthState::new(store.clone(), input.into()).await.unwrap();
        assert_eq!(state.origin(), canonical);
        let set_cookie = state.cookie("tok", 300);
        assert!(set_cookie.contains("HttpOnly; SameSite=Strict"));
        assert!(set_cookie.ends_with("; Secure"));

        // Reset login throttle before testing login on this HTTPS origin
        crate::database::query("DELETE FROM login_throttle")
            .execute(&store.pool)
            .await
            .unwrap();

        // Login with exact canonical HTTPS Origin succeeds and emits a Secure cookie
        let login_res = request(
            &state,
            "/api/v1/auth/login",
            "POST",
            None,
            Some(canonical),
            None,
            serde_json::json!({"login":"admin","password":PASSWORD}),
        )
        .await;
        assert_eq!(login_res.status(), StatusCode::OK);
        let raw_set_cookie = login_res.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(raw_set_cookie.ends_with("; Secure"));
        let cookie_pair = raw_set_cookie.split(';').next().unwrap().to_owned();
        let body: serde_json::Value =
            serde_json::from_slice(&login_res.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let csrf_tok = body["csrfToken"].as_str().unwrap().to_owned();

        // Mismatched scheme (http:// instead of https://) or mismatched port is rejected with 403
        let wrong_scheme = canonical.replacen("https://", "http://", 1);
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/logout",
                "POST",
                Some(&cookie_pair),
                Some(&wrong_scheme),
                Some(&csrf_tok),
                serde_json::json!(null),
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );

        // Exact canonical HTTPS Origin + valid CSRF token succeeds
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/logout",
                "POST",
                Some(&cookie_pair),
                Some(canonical),
                Some(&csrf_tok),
                serde_json::json!(null),
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
    }
}

#[tokio::test]
async fn bootstrap_status_and_bootstrap_web_flow() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("bootstrap_test.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    let state = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();

    // 1. Initial state: needsBootstrap is true, httpBootstrapEnabled is true
    let status_res = request(
        &state,
        "/api/v1/auth/bootstrap/status",
        "GET",
        None,
        None,
        None,
        serde_json::json!(null),
    )
    .await;
    assert_eq!(status_res.status(), StatusCode::OK);
    let status_bytes = status_res.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&status_bytes).unwrap();
    assert_eq!(body["needsBootstrap"], true);
    assert_eq!(body["httpBootstrapEnabled"], true);

    let token = state
        .get_bootstrap_token()
        .expect("bootstrap token must be generated");

    // 2. Missing or wrong token rejected with 401 Unauthorized
    let wrong_token_res = request(
        &state,
        "/api/v1/auth/bootstrap",
        "POST",
        None,
        Some("http://127.0.0.1:3000"),
        None,
        serde_json::json!({
            "login": "admin",
            "password": "my-secure-admin-password-1234",
            "bootstrap_token": "wrong-token-value"
        }),
    )
    .await;
    assert_eq!(wrong_token_res.status(), StatusCode::UNAUTHORIZED);

    // 3. Invalid password (< 15 chars) with valid token rejected
    let invalid_res = request(
        &state,
        "/api/v1/auth/bootstrap",
        "POST",
        None,
        Some("http://127.0.0.1:3000"),
        None,
        serde_json::json!({
            "login": "admin",
            "password": "short",
            "bootstrap_token": token
        }),
    )
    .await;
    assert_eq!(invalid_res.status(), StatusCode::BAD_REQUEST);

    // 4. Valid bootstrap with valid token succeeds
    let valid_res = request(
        &state,
        "/api/v1/auth/bootstrap",
        "POST",
        None,
        Some("http://127.0.0.1:3000"),
        None,
        serde_json::json!({
            "login": "admin",
            "password": "my-secure-admin-password-1234",
            "bootstrap_token": token
        }),
    )
    .await;
    assert_eq!(valid_res.status(), StatusCode::OK);
    let cookie_header = valid_res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cookie_header.contains("aether_session="));
    let valid_bytes = valid_res.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&valid_bytes).unwrap();
    assert_eq!(body["user"]["role"], "administrator");
    assert_eq!(body["user"]["login"], "admin");
    assert!(state.get_bootstrap_token().is_none());

    // 5. After bootstrap, needsBootstrap is false
    let status_res2 = request(
        &state,
        "/api/v1/auth/bootstrap/status",
        "GET",
        None,
        None,
        None,
        serde_json::json!(null),
    )
    .await;
    let status_bytes2 = status_res2.into_body().collect().await.unwrap().to_bytes();
    let body2: serde_json::Value = serde_json::from_slice(&status_bytes2).unwrap();
    assert_eq!(body2["needsBootstrap"], false);

    // 6. Repeated bootstrap attempt is rejected with 403 Forbidden
    let repeated_res = request(
        &state,
        "/api/v1/auth/bootstrap",
        "POST",
        None,
        Some("http://127.0.0.1:3000"),
        None,
        serde_json::json!({
            "login": "hacker",
            "password": "another-password-12345",
            "bootstrap_token": token
        }),
    )
    .await;
    assert_eq!(repeated_res.status(), StatusCode::FORBIDDEN);
}
async fn with_authorization(
    state: &AuthState,
    path: &str,
    method: &str,
    authorization: &str,
    cookie: Option<&str>,
) -> Response {
    let mut req = Request::builder()
        .uri(path)
        .method(method)
        .header(header::AUTHORIZATION, authorization);
    if let Some(v) = cookie {
        req = req.header(header::COOKIE, v);
    }
    router(state.clone())
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn json_body(response: Response) -> serde_json::Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
#[tokio::test]
async fn native_bearer_login_refresh_and_logout() {
    let (_temp, state) = setup().await;
    let login = serde_json::json!({"login":"admin","password":PASSWORD,"tokenType":"bearer"});
    let foreign = request(
        &state,
        "/api/v1/auth/login",
        "POST",
        None,
        Some("https://attacker.example"),
        None,
        login.clone(),
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    let granted = request(
        &state,
        "/api/v1/auth/login",
        "POST",
        None,
        None,
        None,
        login,
    )
    .await;
    assert_eq!(granted.status(), StatusCode::OK);
    assert!(!granted.headers().contains_key(header::SET_COOKIE));
    assert_eq!(granted.headers()[header::CACHE_CONTROL], "no-store");
    let granted = json_body(granted).await;
    assert_eq!(granted["user"]["role"], "administrator");
    assert!(granted.get("csrfToken").is_none());
    let first = granted["accessToken"].as_str().unwrap().to_owned();
    let bearer = |token: &str| format!("Bearer {token}");
    let me = |token: String| {
        let state = state.clone();
        async move {
            with_authorization(&state, "/api/v1/auth/me", "GET", &bearer(&token), None)
                .await
                .status()
        }
    };
    assert_eq!(me(first.clone()).await, StatusCode::OK);
    assert_eq!(
        me(super::util::random_token()).await,
        StatusCode::UNAUTHORIZED
    );

    // Rotation: the old token survives the grace period but cannot be refreshed again.
    let refreshed = with_authorization(
        &state,
        "/api/v1/auth/token/refresh",
        "POST",
        &bearer(&first),
        None,
    )
    .await;
    assert_eq!(refreshed.status(), StatusCode::OK);
    let second = json_body(refreshed).await["accessToken"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(first, second);
    assert_eq!(me(first.clone()).await, StatusCode::OK);
    assert_eq!(
        with_authorization(
            &state,
            "/api/v1/auth/token/refresh",
            "POST",
            &bearer(&first),
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let label: String =
        crate::database::query_scalar("SELECT device_label FROM sessions WHERE token_hash=?")
            .bind(hash(&second))
            .fetch_one(&state.store.pool)
            .await
            .unwrap();
    assert_eq!(label, "Navigateur");

    // A browser cookie keeps CSRF even when a Bearer header rides along, and a
    // proxy's non-session Authorization never breaks cookie authentication.
    let (cookie, _csrf) = sign_in(&state).await;
    assert_eq!(
        with_authorization(
            &state,
            "/api/v1/auth/logout",
            "POST",
            &bearer(&second),
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        with_authorization(
            &state,
            "/api/v1/auth/me",
            "GET",
            "Bearer eyJhbGciOi.jwt.sig",
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        with_authorization(
            &state,
            "/api/v1/auth/token/refresh",
            "POST",
            "Basic abc",
            Some(&cookie)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // Bearer logout needs neither CSRF nor Origin and ends that device only.
    assert_eq!(
        with_authorization(
            &state,
            "/api/v1/auth/logout",
            "POST",
            &bearer(&second),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(me(second).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        request(
            &state,
            "/api/v1/auth/me",
            "GET",
            Some(&cookie),
            None,
            None,
            serde_json::json!({})
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn rotating_addresses_cannot_bypass_the_account_budget() {
    let (_temp, state) = setup().await;
    let attempt = |ip: String, login: &'static str, password: &'static str| {
        let state = state.clone();
        async move {
            request_with_ip(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                Some(&ip),
                serde_json::json!({"login":login,"password":password}),
            )
            .await
            .status()
        }
    };
    // Attempts from different addresses all count against the one login.
    for n in 0..3 {
        let ip = format!("203.0.113.{}", n + 1);
        assert_eq!(
            attempt(ip, "admin", "wrong").await,
            StatusCode::UNAUTHORIZED
        );
    }
    let counted: i64 =
        crate::database::query_scalar("SELECT attempts FROM login_throttle WHERE throttle_key=?")
            .bind("login:admin")
            .fetch_one(&state.store.pool)
            .await
            .unwrap();
    assert_eq!(counted, 3);
    // The rest of the budget is spent directly: thirty Argon2 checks can outlast the one-minute
    // window on a slow runner, which would reopen it before the last attempt.
    crate::database::query(
        "UPDATE login_throttle SET attempts=?, window_started=unixepoch() WHERE throttle_key=?",
    )
    .bind(super::http::LOGIN_ATTEMPTS_PER_ACCOUNT)
    .bind("login:admin")
    .execute(&state.store.pool)
    .await
    .unwrap();
    // A fresh address no longer helps for this login, even with the right password...
    assert_eq!(
        attempt("198.51.100.1".into(), "admin", PASSWORD).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // ...while other logins keep their own budget.
    assert_eq!(
        attempt("198.51.100.2".into(), "someone", "wrong").await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn bootstrap_is_rate_limited() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("bootstrap-limit.db");
    let store = IdentityStore::open(&db).await.unwrap();
    let state = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let token = state.get_bootstrap_token().expect("bootstrap token");

    for _ in 0..10 {
        let res = request(
            &state,
            "/api/v1/auth/bootstrap",
            "POST",
            None,
            Some("http://127.0.0.1:3000"),
            None,
            serde_json::json!({
                "login": "admin",
                "password": "my-secure-admin-password-1234",
                "bootstrap_token": "wrong-token"
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    let throttled = request(
        &state,
        "/api/v1/auth/bootstrap",
        "POST",
        None,
        Some("http://127.0.0.1:3000"),
        None,
        serde_json::json!({
            "login": "admin",
            "password": "my-secure-admin-password-1234",
            "bootstrap_token": token
        }),
    )
    .await;
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
}
