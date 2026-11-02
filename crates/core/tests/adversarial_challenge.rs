//! Empirical stress tests for binding and static web serving.
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::AuthState, identity::IdentityStore, node::Inventory, secure_app_with_web_dist,
};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    process::Command,
};
use tower::ServiceExt;
use zeroize::Zeroizing;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ---------------------------------------------------------------------------
// 1. MEDIA_BIND_ADDR Empirical Verification
// ---------------------------------------------------------------------------

#[test]
fn adversarial_bind_addr_invalid_formats_reject_immediately() {
    let invalid_inputs = [
        "999.999.999.999",
        "localhost",
        "::1",
        "127.0.0.1:3000",
        "256.0.0.1",
        "127.0.0.1/24",
        "-1.0.0.1",
        "invalid-ip-string",
    ];

    let binary = env!("CARGO_BIN_EXE_media-poc");

    for input in invalid_inputs {
        let output = Command::new(binary)
            .env("MEDIA_BIND_ADDR", input)
            .output()
            .unwrap_or_else(|e| panic!("failed to execute binary for input '{input}': {e}"));

        assert!(
            !output.status.success(),
            "Expected binary to fail for invalid MEDIA_BIND_ADDR='{input}', but exit code was 0"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("invalid MEDIA_BIND_ADDR '{input}'")),
            "Expected stderr to contain descriptive error for '{input}', got: {stderr}"
        );
    }
}

#[tokio::test]
async fn adversarial_bind_addr_permutations_socket_binding() {
    // Test socket binding across valid permutations:
    // 127.0.0.1, 0.0.0.0, 127.0.0.2
    let addrs = [
        Ipv4Addr::new(127, 0, 0, 1),
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(127, 0, 0, 2),
    ];

    for ip in addrs {
        // Port 0 picks an ephemeral available port
        let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(ip, 0)).await;
        assert!(
            listener.is_ok(),
            "Expected successful TcpListener bind on {ip}:0, error: {:?}",
            listener.err()
        );
        let listener = listener.unwrap();
        let local_addr = listener.local_addr().unwrap();
        assert_eq!(local_addr.ip(), ip);
    }
}

// ---------------------------------------------------------------------------
// 2. Static web serving under /app
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 3. Static Web Serving Behavior under /app
// ---------------------------------------------------------------------------

#[tokio::test]
async fn adversarial_web_dist_serving_and_security_under_app() {
    let _lock = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let web_dist = temp.path().join("test_dist_with spaces");
    std::fs::create_dir_all(web_dist.join("nested")).unwrap();

    // Sensitive file outside web_dist to verify path traversal resistance
    let outside_secret = temp.path().join("secret.txt");
    std::fs::write(&outside_secret, "SUPER_SECRET_TOKEN").unwrap();

    // Standard index.html
    std::fs::write(
        web_dist.join("index.html"),
        "<!doctype html><html><body>Root Index</body></html>",
    )
    .unwrap();

    // Nested index.html
    std::fs::write(
        web_dist.join("nested").join("index.html"),
        "<!doctype html><html><body>Nested Page</body></html>",
    )
    .unwrap();

    // Asset with spaces in filename
    std::fs::write(
        web_dist.join("file with space.css"),
        "body { background: #000; }",
    )
    .unwrap();

    let db_path = temp.path().join("test_app.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    store
        .bootstrap(
            "admin",
            Zeroizing::new("valid-test-password-16chars".into()),
        )
        .await
        .unwrap();

    let origin = "http://127.0.0.1:3000".to_string();
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::local(media_dir).await.unwrap();

    let router = secure_app_with_web_dist(inventory, 3000, auth, web_dist)
        .await
        .unwrap();

    // A. Root index at /app/ -> 200 OK
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("Root Index"));

    // B. Nested index at /app/nested/ -> 200 OK
    let req = Request::builder()
        .uri("/app/nested/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("Nested Page"));

    // C. File with space in filename -> 200 OK
    let req = Request::builder()
        .uri("/app/file%20with%20space.css")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("body { background: #000; }"));

    // D. Non-existent file -> 404 Not Found
    let req = Request::builder()
        .uri("/app/nonexistent_file.js")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // E. Directory traversal attack attempt -> ServeDir rejects escaping root
    let req = Request::builder()
        .uri("/app/../secret.txt")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_ne!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains("SUPER_SECRET_TOKEN"));

    // F. Encoded directory traversal attempt -> rejected
    let req = Request::builder()
        .uri("/app/%2e%2e/secret.txt")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_ne!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert!(!String::from_utf8_lossy(&body).contains("SUPER_SECRET_TOKEN"));

    // G. Security headers enforced on /app/
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
    assert!(
        res.headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("default-src 'none'")
    );

    // H. Host header validation: mismatched host returns 403 Forbidden
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "evil-domain.com")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // I. Cross-site Origin returns 403 Forbidden
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "127.0.0.1:3000")
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    let res = router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    store.close().await;
}

#[tokio::test]
async fn adversarial_nonexistent_web_dist_serves_404_without_crashing() {
    let _lock = ENV_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let non_existent_dist = temp.path().join("completely_missing_dir");

    let db_path = temp.path().join("test_app_missing.sqlite3");
    let store = IdentityStore::open(&db_path).await.unwrap();
    store
        .bootstrap(
            "admin",
            Zeroizing::new("valid-test-password-16chars".into()),
        )
        .await
        .unwrap();

    let origin = "http://127.0.0.1:3000".to_string();
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let media_dir = temp.path().join("media");
    std::fs::create_dir_all(&media_dir).unwrap();
    let inventory = Inventory::local(media_dir).await.unwrap();

    let router = secure_app_with_web_dist(inventory, 3000, auth, non_existent_dist)
        .await
        .unwrap();

    // When web_dist does not exist, requests to /app/ return 404 cleanly
    let req = Request::builder()
        .uri("/app/")
        .header(header::HOST, "127.0.0.1:3000")
        .body(Body::empty())
        .unwrap();
    let res = router.oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    eprintln!("STATUS: {status}");
    eprintln!("HEADERS: {headers:?}");
    eprintln!("BODY: {}", String::from_utf8_lossy(&body));
    assert_eq!(status, StatusCode::NOT_FOUND);

    store.close().await;
}
