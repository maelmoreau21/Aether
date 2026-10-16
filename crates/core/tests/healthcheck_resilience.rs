//! Integration tests for healthcheck probe resilience when MEDIA_PUBLIC_URL is configured
//! to an external origin (e.g. reverse proxy / Synology NAS domain).
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory, secure_app};
use tower::ServiceExt;
use zeroize::Zeroizing;

#[tokio::test]
async fn healthcheck_resilient_under_external_public_url_while_protected_routes_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("health_resilience.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    store
        .bootstrap("admin", Zeroizing::new("secure-admin-pass-123".into()))
        .await
        .unwrap();

    let external_origin = "https://nas.example.invalid".to_string();
    let auth = AuthState::new(store.clone(), external_origin.clone())
        .await
        .unwrap();

    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::local(media_dir).await.unwrap();

    let router = secure_app(inventory, 3000, auth).await.unwrap();

    // 1. Internal Docker container healthcheck probe:
    // GET /api/v1/health with loopback Host: 127.0.0.1:3000 -> 200 OK
    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(),
        "nosniff"
    );
    assert_eq!(
        res.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["mode"], "standalone-local");

    // 2. Healthcheck probe with localhost:3000 -> 200 OK
    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "localhost:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 3. Healthcheck probe with external domain -> 200 OK
    let req = Request::builder()
        .uri("/api/v1/health")
        .header(header::HOST, "nas.example.invalid")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 4. Protected API: GET /api/v1/catalog with loopback Host -> 403 Forbidden
    let req = Request::builder()
        .uri("/api/v1/catalog")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 5. Protected Auth: POST /api/v1/auth/login with loopback Host -> 403 Forbidden
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::HOST, "127.0.0.1:3000")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"login":"admin","password":"secure-admin-pass-123"}"#,
        ))
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 6. Protected Config: GET /api/v1/config with loopback Host -> 403 Forbidden
    let req = Request::builder()
        .uri("/api/v1/config")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 7. Protected Static Frontend: GET /app/ with loopback Host -> 403 Forbidden
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 8. Protected Assets: GET /api/v1/assets with loopback Host -> 403 Forbidden
    let req = Request::builder()
        .uri("/api/v1/assets")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 9. Protected Admin: GET /api/v1/admin/libraries with loopback Host -> 403 Forbidden
    let req = Request::builder()
        .uri("/api/v1/admin/libraries")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 10. Legitimate traffic matching external domain: GET /api/v1/config -> 200 OK
    let req = Request::builder()
        .uri("/api/v1/config")
        .header(header::HOST, "nas.example.invalid")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    store.close().await;
}
