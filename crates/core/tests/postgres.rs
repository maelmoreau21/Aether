//! The same HTTP/storage contract is exercised against SQLite and an isolated PostgreSQL 18 schema.
//! PostgreSQL: MEDIA_TEST_POSTGRES_URL=... cargo test -p aether-core --test postgres -- --ignored
use aether_core::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::{IdentityError, IdentityStore},
    node::Inventory,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tower::ServiceExt;
use zeroize::Zeroizing;

const PASSWORD: &str = "synthetic postgres contract password";

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: &str,
    csrf: Option<&str>,
    body: Value,
    range: Option<&str>,
) -> (StatusCode, Value, Vec<u8>, String) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:3000")
        .header("origin", "http://127.0.0.1:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if !cookie.is_empty() {
        req = req.header(header::COOKIE, cookie);
    }
    if let Some(csrf) = csrf {
        req = req.header("x-csrf-token", csrf);
    }
    if let Some(range) = range {
        req = req.header(header::RANGE, range);
    }
    let response = app
        .clone()
        .oneshot(
            req.body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_owned();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        bytes,
        cookie,
    )
}
/// A native application: no Origin, no cookie, optional `Authorization: Bearer`.
async fn native(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:3000")
        .header(header::CONTENT_TYPE, "application/json");
    if !token.is_empty() {
        req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(
            req.body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !response.headers().contains_key(header::SET_COOKIE),
        "{method} {uri} set {:?}",
        response.headers().get(header::SET_COOKIE)
    );
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
/// A cast receiver: no cookie, no Authorization, maybe a foreign Origin.
async fn cast(
    app: &Router,
    path: &str,
    origin: Option<&str>,
) -> (StatusCode, String, Option<String>) {
    let mut req = Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:3000");
    if let Some(origin) = origin {
        req = req
            .header("origin", origin)
            .header("sec-fetch-site", "cross-site");
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let allow = response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned(), allow)
}
async fn native_bearer_contract(app: &Router, asset: &str) {
    let (status, granted) = native(
        app,
        "POST",
        "/api/v1/auth/login",
        "",
        json!({"login":"contract-admin","password":PASSWORD,"tokenType":"bearer"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{granted}");
    let first = granted["accessToken"].as_str().unwrap().to_owned();
    for uri in ["/api/v1/auth/me", "/api/v1/catalog"] {
        assert_eq!(
            native(app, "GET", uri, &first, Value::Null).await.0,
            StatusCode::OK,
            "{uri}"
        );
    }

    // HLS capability URL: issued to the device, then opened with no credential.
    let (status, issued) = native(
        app,
        "POST",
        &format!("/api/v1/assets/{asset}/hls"),
        &first,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued}");
    let url = issued["url"].as_str().unwrap();
    let master = url
        .strip_prefix("http://127.0.0.1:3000")
        .unwrap()
        .to_owned();
    assert!(master.ends_with("/master.m3u8"), "{url}");
    let (status, text, allow) = cast(app, &master, Some("https://www.gstatic.com")).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(allow.as_deref(), Some("*"));
    assert!(
        text.starts_with("#EXTM3U") && text.contains("#EXT-X-STREAM-INF:"),
        "{text}"
    );
    let variant = text
        .lines()
        .find(|l| l.ends_with(".m3u8") && !l.starts_with('#'))
        .unwrap();
    let base = master.trim_end_matches("master.m3u8");
    let (status, media, _) = cast(app, &format!("{base}{variant}"), None).await;
    assert_eq!(status, StatusCode::OK, "{media}");
    assert!(
        media.contains("#EXT-X-MAP:URI=") && media.contains("#EXT-X-ENDLIST"),
        "{media}"
    );
    let grant = base.trim_end_matches('/').rsplit('/').next().unwrap();
    for refused in [
        format!("/api/v1/assets/another-asset/hls/{grant}/master.m3u8"),
        format!("{base}../{}/master.m3u8", "0".repeat(64)),
        format!("{base}not-a-component"),
    ] {
        assert_ne!(
            cast(app, &refused, None).await.0,
            StatusCode::OK,
            "{refused}"
        );
    }
    let (status, refreshed) = native(
        app,
        "POST",
        "/api/v1/auth/token/refresh",
        &first,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");
    let second = refreshed["accessToken"].as_str().unwrap().to_owned();
    assert_ne!(first, second);
    assert_eq!(
        native(app, "GET", "/api/v1/auth/me", &first, Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        native(
            app,
            "POST",
            "/api/v1/auth/token/refresh",
            &first,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    // The grant followed the rotation; it ends with the device's logout.
    assert_eq!(cast(app, &master, None).await.0, StatusCode::OK);
    assert_eq!(
        native(app, "POST", "/api/v1/auth/logout", &second, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(cast(app, &master, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        native(app, "GET", "/api/v1/auth/me", &second, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
async fn app(root: &Path, store: &IdentityStore) -> Router {
    aether_core::secure_app(
        Inventory::local(root.to_owned()).await.unwrap(),
        3000,
        AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
    )
    .await
    .unwrap()
}
async fn login(app: &Router, name: &str) -> (String, String) {
    let (status, body, _, cookie) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login":name,"password":PASSWORD}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (cookie, body["csrfToken"].as_str().unwrap().to_owned())
}
async fn exercise(store: IdentityStore, root: &Path) -> (String, String, String) {
    let admin = store
        .bootstrap("contract-admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    assert!(matches!(
        store
            .bootstrap("second-admin", Zeroizing::new(PASSWORD.into()))
            .await,
        Err(IdentityError::AlreadyInitialized)
    ));
    let user = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO users(id,role) VALUES(?,'user')")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO local_credentials(user_id,login,password_hash) SELECT ?,'contract-user',password_hash FROM local_credentials WHERE user_id=?")
        .bind(&user).bind(&admin).execute(store.pool()).await.unwrap();
    assert!(
        query("INSERT INTO users(id,role) VALUES('invalid-role','guest')")
            .execute(store.pool())
            .await
            .is_err()
    );
    assert!(
        query("INSERT INTO library_permissions(user_id,library_id) VALUES('absent','absent')")
            .execute(store.pool())
            .await
            .is_err()
    );
    let app = app(root, &store).await;
    assert_eq!(
        request(&app, "GET", "/api/v1/catalog", "", None, Value::Null, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (cookie, csrf) = login(&app, "contract-admin").await;
    let (user_cookie, user_csrf) = login(&app, "contract-user").await;
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/admin/libraries",
            &user_cookie,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut film_library = String::new();
    for (kind, folder) in [("films", "Films"), ("series", "Series")] {
        let (status,body,_,_) = request(&app,"POST","/api/v1/admin/libraries",&cookie,Some(&csrf),json!({"name":folder,"kind":kind,"nodeId":"local","root":root.join(folder).to_string_lossy(),"enabled":true,"userIds":[]}),None).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let library = body["id"].as_str().unwrap();
        if kind == "films" {
            film_library = library.to_owned();
        }
        let uri = format!("/api/v1/admin/libraries/{library}/scan");
        let (status, body, _, _) =
            request(&app, "POST", &uri, &cookie, Some(&csrf), Value::Null, None).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        tokio::time::timeout(Duration::from_secs(45), async {
            loop {
                let (status, body, _, _) =
                    request(&app, "GET", &uri, &cookie, None, Value::Null, None).await;
                assert_eq!(status, StatusCode::OK, "{body}");
                if body["status"] == "completed" {
                    break;
                }
                assert_ne!(body["status"], "error", "{body}");
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
    }
    for kind in ["film", "series"] {
        let (status, body, _, _) = request(
            &app,
            "GET",
            &format!("/api/v1/catalog?kind={kind}"),
            &cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["total"], 1, "{body}");
    }
    let asset: String = query_scalar("SELECT id FROM inventory_assets WHERE library_id=?")
        .bind(&film_library)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let uri = format!("/api/v1/assets/{asset}/content");
    let (_, descriptor, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/media/{asset}"),
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let work = descriptor["workId"].as_str().unwrap();
    let progress_uri = format!("/api/v1/progress/{work}");
    assert_eq!(
        request(
            &app,
            "GET",
            &progress_uri,
            &user_cookie,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let subtitle_uri = descriptor["subtitles"][0]["url"].as_str().unwrap();
    assert_eq!(
        request(&app, "GET", subtitle_uri, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &uri,
            &user_cookie,
            None,
            Value::Null,
            Some("bytes=0-31")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    query("INSERT INTO library_permissions(user_id,library_id) VALUES(?,?)")
        .bind(&user)
        .bind(&film_library)
        .execute(store.pool())
        .await
        .unwrap();
    let update = json!({"position":4.0,"duration":12.0,"watched":false,"expectedRevision":0});
    assert_eq!(
        request(
            &app,
            "PUT",
            &progress_uri,
            &user_cookie,
            None,
            update.clone(),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, progress, _, _) = request(
        &app,
        "PUT",
        &progress_uri,
        &user_cookie,
        Some(&user_csrf),
        update.clone(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(progress["revision"], 1);
    assert_eq!(
        request(
            &app,
            "PUT",
            &progress_uri,
            &user_cookie,
            Some(&user_csrf),
            update,
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, saved, _, _) = request(
        &app,
        "GET",
        &progress_uri,
        &user_cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(saved["position"], 4.0);
    let (other_session, _) = login(&app, "contract-user").await;
    assert_eq!(
        request(
            &app,
            "GET",
            &progress_uri,
            &other_session,
            None,
            Value::Null,
            None
        )
        .await
        .1["position"],
        4.0,
        "progress is shared across sessions for the same account"
    );
    assert_eq!(
        request(&app, "GET", &progress_uri, &cookie, None, Value::Null, None)
            .await
            .1["revision"],
        0,
        "per-user isolation"
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &progress_uri,
            &user_cookie,
            Some(&user_csrf),
            json!({"position":99,"duration":12,"watched":false,"expectedRevision":1}),
            None
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    for (method, range, expected) in [
        ("GET", None, StatusCode::OK),
        ("HEAD", None, StatusCode::OK),
        ("GET", Some("bytes=0-31"), StatusCode::PARTIAL_CONTENT),
        ("GET", Some("bytes=100-131"), StatusCode::PARTIAL_CONTENT),
        (
            "GET",
            Some("bytes=999999999999-"),
            StatusCode::RANGE_NOT_SATISFIABLE,
        ),
    ] {
        let (status, _, bytes, _) =
            request(&app, method, &uri, &user_cookie, None, Value::Null, range).await;
        assert_eq!(status, expected);
        if expected == StatusCode::PARTIAL_CONTENT {
            let source = tokio::fs::read(root.join("Films/Contract Film (2026)/film.mp4"))
                .await
                .unwrap();
            let start = if range == Some("bytes=0-31") { 0 } else { 100 };
            assert_eq!(bytes, source[start..start + 32], "exact seek bytes");
        }
        if method == "HEAD" {
            assert!(bytes.is_empty());
        }
    }
    query("DELETE FROM library_permissions WHERE user_id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            &progress_uri,
            &user_cookie,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(
        request(
            &app,
            "GET",
            "/api/v1/progress",
            &user_cookie,
            None,
            Value::Null,
            None
        )
        .await
        .1["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        request(
            &app,
            "GET",
            subtitle_uri,
            &user_cookie,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", &uri, &user_cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for expiration in ["last_seen_at", "expires_at"] {
        query(sqlx::AssertSqlSafe(format!(
            "UPDATE sessions SET {expiration}=0 WHERE user_id=?"
        )))
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
        let current = if expiration == "last_seen_at" {
            user_cookie.clone()
        } else {
            login(&app, "contract-user").await.0
        };
        // Expire the newly issued session as well for the absolute-expiration case.
        query(sqlx::AssertSqlSafe(format!(
            "UPDATE sessions SET {expiration}=0 WHERE user_id=?"
        )))
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
        assert_eq!(
            request(
                &app,
                "GET",
                "/api/v1/auth/me",
                &current,
                None,
                Value::Null,
                None
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (current, user_csrf) = login(&app, "contract-user").await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/auth/logout",
            &current,
            Some(&user_csrf),
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/auth/me",
            &current,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (current, _) = login(&app, "contract-user").await;
    query("UPDATE users SET disabled_at=unixepoch() WHERE id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/auth/me",
            &current,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    native_bearer_contract(&app, &asset).await;
    drop(app);
    store.close().await;
    (admin, cookie, asset)
}
async fn fixtures(root: &Path) {
    let generated = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/p1/clip-mp4-en.mp4");
    for target in [
        root.join("Films/Contract Film (2026)/film.mp4"),
        root.join("Series/Contract Series (2026)/Season 01/Contract Series S01E01.mp4"),
    ] {
        tokio::fs::create_dir_all(target.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::copy(&generated, target).await.unwrap();
    }
    tokio::fs::write(
        root.join("Films/Contract Film (2026)/film.fr.forced.srt"),
        "1\n00:00:00,000 --> 00:00:10,000\nBonjour PostgreSQL et SQLite",
    )
    .await
    .unwrap();
}
async fn persistence(store: IdentityStore, root: &Path, identity: (String, String, String)) {
    assert!(store.user(&identity.0).await.unwrap().is_some());
    assert!(matches!(
        store
            .bootstrap("another-admin", Zeroizing::new(PASSWORD.into()))
            .await,
        Err(IdentityError::AlreadyInitialized)
    ));
    let app = app(root, &store).await;
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/auth/me",
            &identity.1,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/v1/assets/{}/content", identity.2),
            &identity.1,
            None,
            Value::Null,
            Some("bytes=100-131")
        )
        .await
        .0,
        StatusCode::PARTIAL_CONTENT
    );
    drop(app);
    store.close().await;
}
#[tokio::test]
async fn sqlite_http_contract_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    fixtures(temp.path()).await;
    let db = temp.path().join("contract.sqlite");
    let identity = exercise(IdentityStore::open(&db).await.unwrap(), temp.path()).await;
    persistence(
        IdentityStore::open(&db).await.unwrap(),
        temp.path(),
        identity,
    )
    .await;
}
#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres18_http_contract_and_reopen() {
    let connection = Zeroizing::new(
        std::env::var("MEDIA_TEST_POSTGRES_URL")
            .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server"),
    );
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        (180000..190000).contains(&version.parse::<i32>().unwrap()),
        "PostgreSQL 18 required"
    );
    let schema = format!("contract_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let url = Zeroizing::new(url.to_string());
    let temp = tempfile::tempdir().unwrap();
    fixtures(temp.path()).await;
    let identity = exercise(
        IdentityStore::open_postgres(&url).await.unwrap(),
        temp.path(),
    )
    .await;
    persistence(
        IdentityStore::open_postgres(&url).await.unwrap(),
        temp.path(),
        identity,
    )
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres18_v1_to_v2_migration_preserves_data_and_is_idempotent() {
    use sqlx::Connection;

    let connection = Zeroizing::new(
        std::env::var("MEDIA_TEST_POSTGRES_URL")
            .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server"),
    );
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let schema = format!("mig_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));

    sqlx::any::install_default_drivers();
    let mut conn = sqlx::AnyConnection::connect(url.as_str()).await.unwrap();
    aether_core::database::initialize_fresh_schema(&mut conn)
        .await
        .unwrap();
    query("INSERT INTO users(id,role,display_name) VALUES('u1','administrator','Admin')")
        .execute(&mut conn)
        .await
        .unwrap();

    let next_version = aether_core::database::CURRENT_SCHEMA_VERSION + 1;
    let steps = [aether_core::database::MigrationStep {
        version: next_version,
        name: "0003_test_migration",
        sqlite_sql: "ALTER TABLE users ADD COLUMN note TEXT DEFAULT '';",
        postgres_sql: "ALTER TABLE users ADD COLUMN note TEXT DEFAULT '';",
    }];
    let version = aether_core::database::upgrade_schema_with_steps(&mut conn, next_version, &steps)
        .await
        .unwrap();
    assert_eq!(version, next_version);

    // Re-running is idempotent and existing data is intact
    let version_again =
        aether_core::database::upgrade_schema_with_steps(&mut conn, next_version, &steps)
            .await
            .unwrap();
    assert_eq!(version_again, next_version);
    let kept: String = query_scalar("SELECT display_name FROM users WHERE id='u1'")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(kept, "Admin");

    drop(conn);
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}
