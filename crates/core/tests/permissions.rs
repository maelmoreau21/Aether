use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use media_poc::{
    auth::AuthState,
    identity::{IdentityStore, Role},
    node::Inventory,
    secure_app,
};
use std::path::PathBuf;
use tower::ServiceExt;
use zeroize::Zeroizing;

const PROTECTED: &[&str] = &[
    "/",
    "/mse",
    "/shaka",
    "/api/v1/catalog",
    "/api/v1/catalog/unknown",
    "/api/v1/demo/media",
    "/api/v1/assets",
    "/api/v1/assets/fixture-mp4/content",
    "/api/v1/assets/unknown/content",
    "/api/v1/demo/subtitles/unknown.vtt",
    "/api/v1/experiment/mse/unknown",
    "/api/v1/experiment/shaka/unknown",
];

async fn assert_denied(router: &Router, cookie: Option<&str>, expected: StatusCode) {
    for path in PROTECTED {
        for method in ["GET", "HEAD"] {
            for range in [None, Some("bytes=0-31"), Some("bytes=999999999999-")] {
                let mut request = Request::builder()
                    .method(method)
                    .uri(*path)
                    .header("host", "127.0.0.1:3000");
                if let Some(cookie) = cookie {
                    request = request.header(header::COOKIE, cookie);
                }
                if let Some(range) = range {
                    request = request.header(header::RANGE, range);
                }
                let response = router
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    expected,
                    "{method} {path} Range={range:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn auth_precedes_all_fixture_metadata_streams_and_derived_resources() {
    assert!(media_poc::permissions::can_read_fixture_library(
        Role::Administrator
    ));
    assert!(!media_poc::permissions::can_read_fixture_library(
        Role::User
    ));
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("core.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    let id = store
        .bootstrap(
            "local-admin",
            Zeroizing::new("synthetic permissions password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let router = secure_app(Inventory::load(root).await.unwrap(), 3000, auth)
        .await
        .unwrap();
    assert_denied(&router, None, StatusCode::UNAUTHORIZED).await;
    assert_denied(
        &router,
        Some("media_session=invalid"),
        StatusCode::UNAUTHORIZED,
    )
    .await;

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("host", "127.0.0.1:3000")
                .header("origin", "http://127.0.0.1:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"login":"local-admin","password":"synthetic permissions password"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    for (method, range, expected) in [
        ("GET", None, StatusCode::OK),
        ("HEAD", None, StatusCode::OK),
        ("GET", Some("bytes=0-31"), StatusCode::PARTIAL_CONTENT),
        ("HEAD", Some("bytes=0-31"), StatusCode::PARTIAL_CONTENT),
        (
            "GET",
            Some("bytes=999999999999-"),
            StatusCode::RANGE_NOT_SATISFIABLE,
        ),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri("/api/v1/assets/fixture-mp4/content")
            .header("host", "127.0.0.1:3000")
            .header(header::COOKIE, &cookie);
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            expected,
            "administrator {method} {range:?}"
        );
    }
    // Role decisions use live backend identity even for an existing session.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    sqlx::query("UPDATE users SET role='user' WHERE id=?")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    assert_denied(&router, Some(&cookie), StatusCode::FORBIDDEN).await;
    sqlx::query("INSERT INTO library_permissions(library_id,user_id) VALUES('fixture-series',?)")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    for (path, expected) in [
        ("/api/v1/catalog/fixture-film", StatusCode::FORBIDDEN),
        ("/api/v1/catalog/fixture-series", StatusCode::OK),
        (
            "/api/v1/assets/demo-mp4-fr/content",
            StatusCode::PARTIAL_CONTENT,
        ),
        (
            "/api/v1/demo/subtitles/fr-forced.vtt",
            StatusCode::PARTIAL_CONTENT,
        ),
        ("/api/v1/assets/fixture-mp4/content", StatusCode::FORBIDDEN),
        ("/api/v1/admin/libraries", StatusCode::FORBIDDEN),
    ] {
        for method in ["GET", "HEAD"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header("host", "127.0.0.1:3000")
                        .header(header::COOKIE, &cookie)
                        .header(header::RANGE, "bytes=0-15")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{method} {path}");
        }
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/catalog?q=mire")
                .header("host", "127.0.0.1:3000")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    use http_body_util::BodyExt;
    let body: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["libraryId"], "fixture-series");
    sqlx::query("UPDATE libraries SET enabled=0 WHERE id='fixture-series'")
        .execute(&pool)
        .await
        .unwrap();
    assert_denied(&router, Some(&cookie), StatusCode::FORBIDDEN).await;
    sqlx::query("UPDATE libraries SET enabled=1 WHERE id='fixture-series'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM library_permissions WHERE user_id=?")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    assert_denied(&router, Some(&cookie), StatusCode::FORBIDDEN).await;

    sqlx::query("UPDATE users SET disabled_at=unixepoch() WHERE id=?")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    assert_denied(&router, Some(&cookie), StatusCode::UNAUTHORIZED).await;
    pool.close().await;
    store.close().await;
}

#[tokio::test]
async fn configured_https_origin_is_enforced_and_sets_secure_cookie() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap(
            "local-admin",
            Zeroizing::new("synthetic permissions password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "https://media.example.invalid".into())
        .await
        .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let router = secure_app(Inventory::load(root).await.unwrap(), 3000, auth)
        .await
        .unwrap();
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
    };
    let body =
        || Body::from(r#"{"login":"local-admin","password":"synthetic permissions password"}"#);
    for (host, origin) in [
        ("127.0.0.1:3000", "https://media.example.invalid"),
        ("media.example.invalid", "http://media.example.invalid"),
    ] {
        let response = router
            .clone()
            .oneshot(
                request()
                    .header("host", host)
                    .header("origin", origin)
                    .body(body())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let response = router
        .clone()
        .oneshot(
            request()
                .header("host", "media.example.invalid")
                .header("origin", "https://media.example.invalid")
                .body(body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("; Secure"));
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/assets")
                .header("host", "media.example.invalid")
                .header(header::COOKIE, cookie.split(';').next().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    store.close().await;
}
