//! `GET /api/v1/assets/{id}/details`: Media Node, library and technical sheet of an
//! indexed asset. Node-side paths are administrator-only; library rights stay live.
use aether_core::{auth::AuthState, identity::IdentityStore, node::Inventory, secure_app};
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

const PASSWORD: &str = "synthetic details password";

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    cookie: &str,
    csrf: Option<&str>,
    body: Value,
) -> (StatusCode, Value, String, String) {
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
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_owned();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        text,
        set_cookie,
    )
}

async fn login(app: &Router, login: &str) -> (String, String) {
    let (status, body, _, cookie) = request(
        app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login": login, "password": PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (cookie, body["csrfToken"].as_str().unwrap().to_owned())
}

async fn create_library(
    app: &Router,
    root: &Path,
    cookie: &str,
    csrf: &str,
    name: &str,
    users: &[&str],
) -> String {
    let (status, body, _, _) = request(
        app,
        "POST",
        "/api/v1/admin/libraries",
        cookie,
        Some(csrf),
        json!({"name": name, "kind": "films", "nodeId": "local", "root": root.to_string_lossy(), "enabled": true, "userIds": users}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_owned()
}

async fn scan(app: &Router, library: &str, cookie: &str, csrf: &str) {
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    let (status, _, _, _) = request(app, "POST", &uri, cookie, Some(csrf), Value::Null).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let (_, body, _, _) = request(app, "GET", &uri, cookie, None, Value::Null).await;
            if body["status"] == "completed" {
                return;
            }
            assert_ne!(body["status"], "error", "{body}");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn asset_details_expose_node_and_paths_to_administrators_only() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    let film = films.join("Details Film (2026)");
    let private = root.join("Private");
    tokio::fs::create_dir_all(&film).await.unwrap();
    tokio::fs::create_dir_all(&private).await.unwrap();
    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    tokio::fs::copy(
        generated.join("representative/representative.mkv"),
        film.join("details-version.mkv"),
    )
    .await
    .unwrap();
    tokio::fs::copy(
        generated.join("p1/clip-mp4-en.mp4"),
        private.join("Hidden Film (2025).mp4"),
    )
    .await
    .unwrap();

    let store = IdentityStore::open(&temp.path().join("index.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap("details-admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let app = secure_app(
        Inventory::local(root.clone()).await.unwrap(),
        3000,
        AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let (admin, csrf) = login(&app, "details-admin").await;
    let (status, created, _, _) = request(
        &app,
        "POST",
        "/api/v1/admin/users",
        &admin,
        Some(&csrf),
        json!({"login": "details-reader", "password": PASSWORD, "role": "user"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let reader_id = created["id"].as_str().unwrap().to_owned();
    let library = create_library(&app, &films, &admin, &csrf, "Films", &[&reader_id]).await;
    let hidden = create_library(&app, &private, &admin, &csrf, "Privée", &[]).await;
    scan(&app, &library, &admin, &csrf).await;
    scan(&app, &hidden, &admin, &csrf).await;

    let asset_for = |relative: &'static str| {
        let store = store.clone();
        async move {
            aether_core::database::query_scalar::<_, String>(
                "SELECT id FROM inventory_assets WHERE relative_path LIKE ?",
            )
            .bind(format!("%{relative}"))
            .fetch_one(store.pool())
            .await
            .unwrap()
        }
    };
    let asset = asset_for("details-version.mkv").await;
    let hidden_asset = asset_for("Hidden Film (2025).mp4").await;
    let uri = format!("/api/v1/assets/{asset}/details");

    // Administrator: Node identity, logical library and both path forms.
    let (status, details, text, _) = request(&app, "GET", &uri, &admin, None, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(details["assetId"], asset.as_str());
    assert_eq!(details["pathsVisible"], true);
    assert_eq!(details["node"]["id"], "local");
    assert!(!details["node"]["name"].as_str().unwrap().is_empty());
    assert_eq!(details["node"]["status"], "connected");
    assert_eq!(details["library"]["id"], library.as_str());
    assert_eq!(details["library"]["name"], "Films");
    let relative = details["file"]["relativePath"].as_str().unwrap();
    assert!(relative.ends_with("details-version.mkv"), "{relative}");
    assert_eq!(details["file"]["name"], "details-version.mkv");
    let absolute = details["file"]["absolutePath"].as_str().unwrap();
    // The absolute path is the one known by the Node: it designates the real file.
    assert_eq!(
        std::fs::canonicalize(absolute).unwrap(),
        std::fs::canonicalize(film.join("details-version.mkv")).unwrap(),
        "{absolute}"
    );
    assert!(details["file"]["size"].as_i64().unwrap() > 0);
    assert_eq!(details["file"]["mime"], "video/x-matroska");
    assert!(details["container"]["formatName"].is_string());
    let tracks = details["tracks"].as_array().unwrap();
    let video = tracks.iter().find(|t| t["codecType"] == "video").unwrap();
    assert_eq!(video["codecName"], "av1");
    assert!(video["width"].as_u64().unwrap() > 0);
    let audio: Vec<_> = tracks
        .iter()
        .filter(|t| t["codecType"] == "audio")
        .collect();
    assert_eq!(audio.len(), 2);
    assert_eq!(audio[0]["language"], "fra");
    // ffprobe's `format.filename` and scanner-private markers never leave the Core.
    assert!(!text.contains("\"filename\""));
    assert!(!text.contains("native_opus_tracks_v1"));

    // Regular user with live access: same Node/library/tracks, no path at all.
    let (reader, _) = login(&app, "details-reader").await;
    let (status, details, text, _) = request(&app, "GET", &uri, &reader, None, Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(details["pathsVisible"], false);
    assert_eq!(details["node"]["id"], "local");
    assert!(details["node"]["name"].is_string());
    assert_eq!(details["library"]["name"], "Films");
    assert_eq!(details["tracks"].as_array().unwrap().len(), tracks.len());
    for key in ["relativePath", "absolutePath", "libraryRoot", "name"] {
        assert!(details["file"].get(key).is_none(), "{key} leaked: {text}");
    }
    assert!(!text.contains("details-version"), "{text}");
    assert!(!text.contains(&*root.to_string_lossy()), "{text}");
    assert!(!text.contains(&*root.to_string_lossy().replace('\\', "\\\\")));

    // Live library rights, unknown identifiers and anonymous calls fail closed.
    for (cookie, target, expected) in [
        (
            reader.as_str(),
            hidden_asset.as_str(),
            StatusCode::FORBIDDEN,
        ),
        (reader.as_str(), "unknown-asset", StatusCode::FORBIDDEN),
        ("", asset.as_str(), StatusCode::UNAUTHORIZED),
    ] {
        let (status, _, text, _) = request(
            &app,
            "GET",
            &format!("/api/v1/assets/{target}/details"),
            cookie,
            None,
            Value::Null,
        )
        .await;
        assert_eq!(status, expected, "{target}: {text}");
        assert!(!text.contains("Hidden Film"));
    }
    let (status, _, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/assets/{hidden_asset}/details"),
        &admin,
        None,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
