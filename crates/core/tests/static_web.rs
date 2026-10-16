//! Integration tests for static asset serving under /app and MEDIA_WEB_DIST support.
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::AuthState, configured_web_dist, identity::IdentityStore, node::Inventory, secure_app,
};
use tower::ServiceExt;
use zeroize::Zeroizing;

#[tokio::test]
async fn static_web_serves_custom_dist_and_applies_security_headers() {
    let temp = tempfile::tempdir().unwrap();
    let web_dist = temp.path().join("custom_dist");
    std::fs::create_dir_all(web_dist.join("login")).unwrap();
    std::fs::write(
        web_dist.join("index.html"),
        "<!doctype html><html><body>Aether Test Index</body></html>",
    )
    .unwrap();
    std::fs::write(
        web_dist.join("login").join("index.html"),
        "<!doctype html><html><body>Aether Test Login</body></html>",
    )
    .unwrap();

    // Verify configured_web_dist with explicit MEDIA_WEB_DIST
    unsafe {
        std::env::set_var("MEDIA_WEB_DIST", &web_dist);
    }
    assert_eq!(configured_web_dist(), web_dist.canonicalize().unwrap());

    let db_path = temp.path().join("test_static.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    store
        .bootstrap("test-admin", Zeroizing::new("disposable test pass".into()))
        .await
        .unwrap();

    let origin = "http://127.0.0.1:3000".to_string();
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::local(media_dir).await.unwrap();

    let router = secure_app(inventory, 3000, auth).await.unwrap();

    // 1. Request to /app/ -> returns 200 OK with custom index.html content
    let req = Request::builder()
        .uri("/app/")
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
    let csp = res
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(csp.contains("default-src 'none'"));
    assert!(csp.contains("script-src 'self'"));
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("Aether Test Index"));

    // 2. Request to /app/login/ -> returns 200 OK with custom login content
    let req = Request::builder()
        .uri("/app/login/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("Aether Test Login"));

    // 3. Request to missing file -> returns 404 Not Found
    let req = Request::builder()
        .uri("/app/nonexistent.js")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // 4. Request to /app/ with mismatched host -> returns 403 Forbidden (guarantees local_security encloses /app)
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "malicious.attacker.com")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    unsafe {
        std::env::remove_var("MEDIA_WEB_DIST");
    }
    store.close().await;
}
