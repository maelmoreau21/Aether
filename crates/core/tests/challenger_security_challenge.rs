//! Adversarial challenge test suite for Milestone 1
//! Empirically challenges:
//! 1. /api/v1/health probe behavior under varied/malformed/missing Host headers and methods.
//! 2. 100% rejection (403 Forbidden) of all protected routes under mismatched Host, Sec-Fetch-Site, and Origin spoofing.
//! 3. CSRF token and origin protections across mutating endpoints (login, logout, scans, admin libraries).

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory, secure_app};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;

async fn setup_test_app(
    origin: &str,
) -> (
    axum::Router,
    IdentityStore,
    tempfile::TempDir,
    String, // cookie
    String, // csrf_token
    String, // created library id
) {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("challenger_test.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("secure-admin-pass-123".into()))
        .await
        .unwrap();

    let auth = AuthState::new(store.clone(), origin.to_string())
        .await
        .unwrap();

    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::local(media_dir.clone()).await.unwrap();

    let router = secure_app(inventory, 3000, auth).await.unwrap();

    // Authenticate admin to obtain cookie and CSRF token
    let authority = origin
        .parse::<axum::http::Uri>()
        .unwrap()
        .authority()
        .unwrap()
        .as_str()
        .to_string();

    let login_req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::HOST, &authority)
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"login":"admin","password":"secure-admin-pass-123"}"#,
        ))
        .unwrap();

    let res = router.clone().oneshot(login_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let cookie_header = res
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    let cookie = cookie_header.split(';').next().unwrap().to_string();

    let body_bytes = res.into_body().collect().await.unwrap().to_bytes();
    let login_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    let csrf_token = login_json["csrfToken"].as_str().unwrap().to_string();

    // Create a test library for scan / admin mutations
    let lib_req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/libraries")
        .header(header::HOST, &authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &csrf_token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "name": "ChallengeLib",
                "kind": "films",
                "nodeId": "local",
                "root": media_dir.to_string_lossy(),
                "enabled": true,
                "userIds": []
            })
            .to_string(),
        ))
        .unwrap();
    let res = router.clone().oneshot(lib_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let lib_bytes = res.into_body().collect().await.unwrap().to_bytes();
    let lib_json: Value = serde_json::from_slice(&lib_bytes).unwrap();
    let library_id = lib_json["id"].as_str().unwrap().to_string();

    (router, store, temp, cookie, csrf_token, library_id)
}

#[tokio::test]
async fn challenge_healthcheck_host_permutations_and_info_leak_audit() {
    let origin = "http://127.0.0.1:3000";
    let (router, store, _temp, _cookie, _csrf, _lib_id) = setup_test_app(origin).await;

    // Test matrix of Host headers against /api/v1/health
    let test_hosts = vec![
        Some("127.0.0.1:3000"),
        Some("localhost:3000"),
        Some("127.0.0.1"),
        Some("localhost"),
        Some("nas.example.invalid"),
        Some("nas.example.invalid:443"),
        Some("evil.attacker.com"),
        Some("evil.attacker.com:8080"),
        Some("192.168.1.100:9999"),
        Some("[::1]:3000"),
        Some(""), // empty Host header
        None,     // missing Host header entirely
    ];

    for host_opt in test_hosts {
        let mut req_builder = Request::builder().uri("/api/v1/health");
        if let Some(h) = host_opt {
            req_builder = req_builder.header(header::HOST, h);
        }
        let req = req_builder.body(Body::empty()).unwrap();
        let res = router.clone().oneshot(req).await.unwrap();

        assert_eq!(
            res.status(),
            StatusCode::OK,
            "Healthcheck failed for Host header: {:?}",
            host_opt
        );

        // Security headers verification
        assert_eq!(
            res.headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff"),
            "nosniff missing for Host: {:?}",
            host_opt
        );
        assert_eq!(
            res.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store"),
            "no-store missing for Host: {:?}",
            host_opt
        );

        // Sensitive information leakage verification
        let body_bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body_str = std::str::from_utf8(&body_bytes).unwrap();
        let json: Value = serde_json::from_str(body_str).unwrap();

        assert_eq!(json["status"], "ok");
        assert_eq!(json["mode"], "standalone-local");

        // Strict verification: only status and mode are present in JSON response
        let obj = json.as_object().unwrap();
        assert_eq!(
            obj.len(),
            2,
            "Unexpected extra fields in healthcheck payload: {:?}",
            obj
        );

        // Check for sensitive substrings
        let forbidden_substrings = [
            "password",
            "secret",
            "token",
            "admin",
            "sqlite",
            "media",
            "hash",
            "argon2",
            "path",
            "user",
            "error",
            "trace",
            "127.0.0.1",
            "localhost",
        ];
        for word in forbidden_substrings {
            assert!(
                !body_str.to_lowercase().contains(word),
                "Healthcheck leaks sensitive keyword '{}' in body: {}",
                word,
                body_str
            );
        }
    }

    // Stress test: Extremely large Host header (16KB)
    let giant_host = format!("{}.example.invalid", "a".repeat(16384));
    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, giant_host)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Stress test: Query params should not alter healthcheck or leak
    let req = Request::builder()
        .uri("/api/v1/health?debug=true&token=supersecret123")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body_bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body_bytes).unwrap();
    assert!(!body_str.contains("supersecret123"));

    // Verify non-GET methods on /api/v1/health fail (405 Method Not Allowed)
    let methods = vec![Method::POST, Method::PUT, Method::DELETE, Method::PATCH];
    for method in methods {
        let req = Request::builder()
            .method(method.clone())
            .uri("/api/v1/health")
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "Method {} on /api/v1/health should return 405",
            method
        );
    }

    store.close().await;
}

#[tokio::test]
async fn challenge_all_protected_routes_fail_closed_under_host_origin_and_fetch_site_attacks() {
    let origin = "http://127.0.0.1:3000";
    let (router, store, _temp, _cookie, _csrf, _lib_id) = setup_test_app(origin).await;

    // List of protected routes from requirement:
    // /app/, /api/v1/catalog, /api/v1/config, /api/v1/auth/login, /api/v1/admin/libraries
    // plus additional protected endpoints: /api/v1/assets, /api/v1/scans
    let protected_endpoints = vec![
        ("GET", "/app/"),
        ("GET", "/app/index.html"),
        ("GET", "/api/v1/catalog"),
        ("GET", "/api/v1/config"),
        ("POST", "/api/v1/auth/login"),
        ("GET", "/api/v1/admin/libraries"),
        ("GET", "/api/v1/assets"),
        ("POST", "/api/v1/admin/libraries/test/scan"),
    ];

    // Attack Vectors
    enum AttackType {
        MismatchedHost(&'static str),
        MissingHost,
        SpoofedForwardedHost,
        CrossSiteSecFetchSite,
        InvalidOrigin(&'static str),
    }

    let attack_vectors = vec![
        AttackType::MismatchedHost("evil.attacker.com"),
        AttackType::MismatchedHost("evil.attacker.com:3000"),
        AttackType::MismatchedHost("127.0.0.1:3000.attacker.com"),
        AttackType::MismatchedHost("127.0.0.1:3000@attacker.com"),
        AttackType::MismatchedHost("localhost:3000"),
        AttackType::MismatchedHost("127.0.0.1:9999"),
        AttackType::MismatchedHost("127.0.0.1"), // missing port
        AttackType::MismatchedHost("nas.example.invalid"),
        AttackType::MissingHost,
        AttackType::SpoofedForwardedHost,
        AttackType::CrossSiteSecFetchSite,
        AttackType::InvalidOrigin("http://evil.attacker.com"),
        AttackType::InvalidOrigin("https://evil.attacker.com"),
        AttackType::InvalidOrigin("http://127.0.0.1:3001"),
        AttackType::InvalidOrigin("https://127.0.0.1:3000"),
        AttackType::InvalidOrigin("http://127.0.0.1:3000/"), // invalid trailing slash
    ];

    for (method, path) in &protected_endpoints {
        for attack in &attack_vectors {
            let mut req_builder = Request::builder().method(*method).uri(*path);

            match attack {
                AttackType::MismatchedHost(bad_host) => {
                    req_builder = req_builder
                        .header(header::HOST, *bad_host)
                        .header(header::ORIGIN, origin);
                }
                AttackType::MissingHost => {
                    req_builder = req_builder.header(header::ORIGIN, origin);
                }
                AttackType::SpoofedForwardedHost => {
                    req_builder = req_builder
                        .header(header::HOST, "evil.attacker.com")
                        .header("x-forwarded-host", "127.0.0.1:3000")
                        .header("x-forwarded-for", "127.0.0.1")
                        .header(header::ORIGIN, origin);
                }
                AttackType::CrossSiteSecFetchSite => {
                    req_builder = req_builder
                        .header(header::HOST, "127.0.0.1:3000")
                        .header(header::ORIGIN, origin)
                        .header("sec-fetch-site", "cross-site");
                }
                AttackType::InvalidOrigin(bad_origin) => {
                    req_builder = req_builder
                        .header(header::HOST, "127.0.0.1:3000")
                        .header(header::ORIGIN, *bad_origin);
                }
            }

            let body = if *method == "POST" {
                req_builder = req_builder.header(header::CONTENT_TYPE, "application/json");
                Body::from(r#"{"login":"admin","password":"password"}"#)
            } else {
                Body::empty()
            };

            let req = req_builder.body(body).unwrap();
            let res = router.clone().oneshot(req).await.unwrap();

            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "Protected route {} {} failed to reject with 403 Forbidden under attack vector! Actual status: {}",
                method,
                path,
                res.status()
            );
        }
    }

    // Additional Adversarial Test: Streaming / Range request with mismatched Host
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/assets/nonexistent-asset/content")
        .header(header::HOST, "evil.attacker.com")
        .header(header::ORIGIN, origin)
        .header(header::RANGE, "bytes=0-1024")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Streaming Range request with mismatched Host must be rejected with 403 Forbidden"
    );

    // Additional Adversarial Test: CORS Preflight (OPTIONS) with evil Origin
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/v1/catalog")
        .header(header::HOST, "127.0.0.1:3000")
        .header(header::ORIGIN, "http://evil.attacker.com")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "CORS preflight with evil Origin must be rejected with 403 Forbidden"
    );

    store.close().await;
}

#[tokio::test]
async fn challenge_reverse_proxy_configuration_boundaries() {
    // When MEDIA_PUBLIC_URL is set to an external reverse-proxy HTTPS domain:
    let external_origin = "https://nas.example.invalid";
    let (router, store, _temp, _cookie, _csrf, _lib_id) = setup_test_app(external_origin).await;

    // 1. Internal healthcheck probe (127.0.0.1:3000) must SUCCEED with 200 OK
    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 2. Loopback requests to protected routes with 127.0.0.1:3000 must FAIL CLOSED with 403 Forbidden
    let routes = [
        "/app/",
        "/api/v1/catalog",
        "/api/v1/config",
        "/api/v1/admin/libraries",
    ];
    for route in routes {
        let req = Request::builder()
            .uri(route)
            .header(header::HOST, "127.0.0.1:3000")
            .body(Body::empty())
            .unwrap();
        let res = router.clone().oneshot(req).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::FORBIDDEN,
            "Route {} allowed loopback host when origin is external reverse proxy!",
            route
        );
    }

    // 3. Legitimate reverse-proxy requests matching external host: nas.example.invalid
    let req = Request::builder()
        .uri("/api/v1/config")
        .header(header::HOST, "nas.example.invalid")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    store.close().await;
}

#[tokio::test]
async fn challenge_csrf_and_mutating_endpoint_protections() {
    let origin = "http://127.0.0.1:3000";
    let (router, store, _temp, cookie, csrf_token, lib_id) = setup_test_app(origin).await;
    let authority = "127.0.0.1:3000";

    // --- Scenario A: POST /api/v1/auth/login ---
    // 1. Missing Origin on login -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::HOST, authority)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"login":"admin","password":"secure-admin-pass-123"}"#,
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Login without Origin header must be rejected with 403"
    );

    // 2. Cross-site Sec-Fetch-Site on login -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header("sec-fetch-site", "cross-site")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"login":"admin","password":"secure-admin-pass-123"}"#,
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Login with cross-site Sec-Fetch-Site must be rejected with 403"
    );

    // 3. Evil Origin on login -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::HOST, authority)
        .header(header::ORIGIN, "http://evil.attacker.com")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"login":"admin","password":"secure-admin-pass-123"}"#,
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Login with evil Origin must be rejected with 403"
    );

    // --- Scenario B: POST /api/v1/auth/logout ---
    // 1. Missing CSRF token on logout -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Logout without x-csrf-token must return 403"
    );

    // 2. Wrong CSRF token (equal length 64 hex chars, bad hash) -> 403 Forbidden
    let bad_csrf_64 = "f".repeat(64);
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &bad_csrf_64)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Logout with incorrect 64-char csrf token must return 403"
    );

    // 3. Short CSRF token (malformed length) -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", "malformed_short_token")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Logout with malformed length csrf token must return 403"
    );

    // 4. Valid CSRF token, but cross-site Sec-Fetch-Site -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &csrf_token)
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Logout with cross-site Sec-Fetch-Site must return 403"
    );

    // 5. Valid CSRF token, but evil Origin -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::HOST, authority)
        .header(header::ORIGIN, "http://evil.com")
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &csrf_token)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Logout with evil Origin must return 403"
    );

    // --- Scenario C: POST /api/v1/admin/libraries ---
    let valid_lib_payload = json!({
        "name": "NewLib",
        "kind": "films",
        "nodeId": "local",
        "root": "/tmp/dummy",
        "enabled": true,
        "userIds": []
    })
    .to_string();

    // 1. Missing CSRF token -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/libraries")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_lib_payload.clone()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Admin create library without CSRF token must return 403"
    );

    // 2. Bad CSRF token -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/libraries")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &bad_csrf_64)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_lib_payload.clone()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "Admin create library with bad CSRF token must return 403"
    );

    // --- Scenario D: PUT /api/v1/admin/libraries/{id} ---
    let put_uri = format!("/api/v1/admin/libraries/{}", lib_id);
    let req = Request::builder()
        .method("PUT")
        .uri(&put_uri)
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(valid_lib_payload.clone()))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "PUT library without CSRF token must return 403"
    );

    // --- Scenario F: POST /api/v1/admin/libraries/{id}/scan ---
    let scan_uri = format!("/api/v1/admin/libraries/{}/scan", lib_id);
    let req = Request::builder()
        .method("POST")
        .uri(&scan_uri)
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "POST scans without CSRF token must return 403"
    );

    let req = Request::builder()
        .method("POST")
        .uri(&scan_uri)
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &bad_csrf_64)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::FORBIDDEN,
        "POST scans with bad CSRF token must return 403"
    );

    // --- Scenario G: Valid mutating operations succeed with valid credentials, CSRF, and Origin ---
    // 1. Valid library update with valid CSRF
    let update_payload = json!({
        "name": "LegitUpdate",
        "kind": "films",
        "nodeId": "local",
        "root": _temp.path().join("media").to_string_lossy(),
        "enabled": true,
        "userIds": []
    })
    .to_string();

    let req = Request::builder()
        .method("PUT")
        .uri(&put_uri)
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &csrf_token)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(update_payload))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "PUT library with valid CSRF must succeed"
    );

    // 2. Valid logout with valid CSRF
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/logout")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .header("x-csrf-token", &csrf_token)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::NO_CONTENT,
        "POST logout with valid CSRF must succeed with 204 No Content"
    );

    // 3. Subsequent request with invalidated session cookie must return 401 Unauthorized
    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/admin/libraries")
        .header(header::HOST, authority)
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        res.status(),
        StatusCode::UNAUTHORIZED,
        "Revoked session cookie must return 401"
    );

    store.close().await;
}
