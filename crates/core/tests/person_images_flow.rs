use aether_core::{auth::AuthState, identity::IdentityStore, node::Inventory, secure_app};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tower::ServiceExt;
use zeroize::Zeroizing;

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
        .and_then(|v| v.to_str().ok())
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
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, bytes, cookie)
}
async fn create_library(app: &Router, root: &Path, cookie: &str, csrf: &str, name: &str) -> String {
    let (status, body, _, _) = request(app,"POST","/api/v1/admin/libraries",cookie,Some(csrf),json!({"name":name,"kind":"films","nodeId":"local","root":root.to_string_lossy(),"enabled":true,"userIds":[]}),None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().into()
}
async fn scan(app: &Router, library: &str, cookie: &str, csrf: &str) -> Value {
    scan_mode(app, library, cookie, csrf, Value::Null).await
}
async fn scan_mode(app: &Router, library: &str, cookie: &str, csrf: &str, mode: Value) -> Value {
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    assert_eq!(
        request(app, "POST", &uri, cookie, Some(csrf), mode, None)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (status, body, _, _) =
                request(app, "GET", &uri, cookie, None, Value::Null, None).await;
            assert_eq!(status, StatusCode::OK);
            if body["status"] == "completed" {
                return body;
            }
            assert_ne!(body["status"], "error", "{body}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}
async fn router(root: &Path, store: &IdentityStore) -> Router {
    secure_app(
        Inventory::local(root.to_path_buf()).await.unwrap(),
        3000,
        AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn nfo_portraits_are_served_only_through_core_for_credited_people() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let film = root.join("Portrait Film (2026)");
    tokio::fs::create_dir_all(&film).await.unwrap();
    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/p1");
    tokio::fs::copy(generated.join("clip-mp4-fr.mp4"), film.join("movie.mp4"))
        .await
        .unwrap();
    let portrait = "https://image.example.org/t/p/w185/portrait.jpg";
    tokio::fs::write(
        film.join("movie.nfo"),
        format!("<movie><title>Portrait Film</title><actor><name>With Photo</name><role>Lead</role><thumb>{portrait}</thumb></actor><actor><name>Local Path</name><thumb>/config/metadata/People/L/folder.jpg</thumb></actor><director>With Photo</director></movie>"),
    )
    .await
    .unwrap();
    let store = IdentityStore::open(&temp.path().join("people.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "people-admin",
            Zeroizing::new("synthetic people test password".into()),
        )
        .await
        .unwrap();
    let app = router(&root, &store).await;
    let (status, login, _, cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login":"people-admin","password":"synthetic people test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let library = create_library(&app, &root, &cookie, csrf, "People").await;
    scan(&app, &library, &cookie, csrf).await;
    let (_, catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?q=Portrait",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let work = catalog["items"][0]["id"].as_str().unwrap().to_owned();
    let (_, detail, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{work}"),
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let actors = detail["actors"].as_array().unwrap();
    let image_url = actors[0]["imageUrl"].as_str().unwrap().to_owned();
    let source_hash = format!("{:x}", Sha256::digest(portrait.as_bytes()));
    assert_eq!(
        image_url,
        format!("/api/v1/works/{work}/people/{}/image", &source_hash[..32])
    );
    assert!(!image_url.contains("example.org"));
    assert!(actors[1].get("imageUrl").is_none(), "{detail}");

    // Anonymous readers never reach the cache or the provider.
    let (status, _, _, _) = request(&app, "GET", &image_url, "", None, Value::Null, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A key that is not a credit of this work is refused, even if well formed.
    let other = format!("/api/v1/works/{work}/people/{}/image", "a".repeat(32));
    let (status, body, _, _) = request(&app, "GET", &other, &cookie, None, Value::Null, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_person_image");
    // No installed extension declares the host: no download is attempted and nothing is cached.
    let (status, body, _, _) =
        request(&app, "GET", &image_url, &cookie, None, Value::Null, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "person_image_host_denied");
    let cached: i64 = aether_core::database::query_scalar("SELECT COUNT(*) FROM person_images")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(cached, 0);
    // A recent failure is remembered and answered without a new download.
    aether_core::database::query(
        "INSERT INTO person_images(source_hash,sha256,error_code,fetched_at) VALUES(?,NULL,'download_failed',?)",
    )
    .bind(&source_hash)
    .bind(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    )
    .execute(store.pool())
    .await
    .unwrap();
    let (status, body, _, _) =
        request(&app, "GET", &image_url, &cookie, None, Value::Null, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "person_image_unavailable");
    drop(app);
    store.close().await;
}
