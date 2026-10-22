use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory, secure_app};
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
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    assert_eq!(
        request(app, "POST", &uri, cookie, Some(csrf), Value::Null, None)
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
async fn hash(path: &Path) -> Vec<u8> {
    Sha256::digest(tokio::fs::read(path).await.unwrap()).to_vec()
}

#[tokio::test]
async fn real_films_scan_persists_reuses_probes_and_enforces_live_library_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let films = root.join("Films");
    let film = films.join("Synthetic Film (2026) [tmdb-123]");
    let other = root.join("Restricted");
    tokio::fs::create_dir_all(&film).await.unwrap();
    tokio::fs::create_dir_all(&other).await.unwrap();
    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    let mkv = film.join("version-mkv.mkv");
    let mp4 = film.join("version-mp4.mp4");
    tokio::fs::copy(generated.join("representative/representative.mkv"), &mkv)
        .await
        .unwrap();
    tokio::fs::copy(generated.join("p1/clip-mp4-en.mp4"), &mp4)
        .await
        .unwrap();
    let sidecar = film.join("version-mp4.fr.forced.srt");
    tokio::fs::write(
        &sidecar,
        "1\n00:00:00,000 --> 00:00:10,000\nBonjour externe",
    )
    .await
    .unwrap();
    tokio::fs::copy(&mp4, other.join("Private Film (2025).mp4"))
        .await
        .unwrap();
    tokio::fs::write(film.join("corrupt.mkv"), b"synthetic corrupt file")
        .await
        .unwrap();
    tokio::fs::write(film.join("temporary.mkv.part"), b"temporary download")
        .await
        .unwrap();
    let before = (hash(&mkv).await, hash(&mp4).await);
    let db = temp.path().join("index.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    let user = store
        .bootstrap(
            "scan-admin",
            Zeroizing::new("synthetic scanner password".into()),
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
        json!({"login":"scan-admin","password":"synthetic scanner password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let library = create_library(&app, &films, &cookie, csrf, "Films").await;
    let private = create_library(&app, &other, &cookie, csrf, "Restricted").await;
    let uri = format!("/api/v1/admin/libraries/{library}/scan");
    assert_eq!(
        request(&app, "POST", &uri, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let first = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(first["discovered"], 3);
    assert_eq!(first["probed"], 3);
    assert_eq!(first["reused"], 0);
    assert_eq!(first["errors"].as_array().unwrap().len(), 1);
    scan(&app, &private, &cookie, csrf).await;
    let (_, catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?q=Synthetic",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(catalog["total"], 1, "{catalog}");
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
    assert_eq!(
        detail["editions"][0]["versions"].as_array().unwrap().len(),
        2
    );
    let assets: Vec<String> = detail["editions"][0]["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["sources"][0]["assetIds"][0].as_str().unwrap().to_owned())
        .collect();
    for asset in &assets {
        let (status, media, _, _) = request(
            &app,
            "GET",
            &format!("/api/v1/media/{asset}"),
            &cookie,
            None,
            Value::Null,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(media["synthetic"], false);
        for component in ["video", "audio-1", "audio-2", "subtitle-4"] {
            let tracks = media["sources"][0]["tracks"].as_array().unwrap();
            if component != "video" {
                let index: u64 = component.split('-').nth(1).unwrap().parse().unwrap();
                if !tracks.iter().any(|t| t["index"] == index) {
                    continue;
                }
            }
            let url = format!("/api/v1/assets/{asset}/prepared/{component}");
            for (method, range, expected) in [
                ("HEAD", None, StatusCode::OK),
                ("GET", Some("bytes=0-5"), StatusCode::PARTIAL_CONTENT),
                (
                    "GET",
                    Some("bytes=999999999999-"),
                    StatusCode::RANGE_NOT_SATISFIABLE,
                ),
            ] {
                let (status, _, bytes, _) =
                    request(&app, method, &url, &cookie, None, Value::Null, range).await;
                assert_eq!(status, expected, "{component} {method}");
                if method == "HEAD" {
                    assert!(bytes.is_empty());
                }
                if range == Some("bytes=0-5") {
                    assert_eq!(bytes.len(), 6);
                }
            }
            assert_eq!(
                request(&app, "GET", &url, "", None, Value::Null, None)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("/api/v1/assets/{asset}/prepared/audio-999"),
                &cookie,
                None,
                Value::Null,
                None
            )
            .await
            .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let source = &media["sources"][0];
        let tracks = source["tracks"].as_array().unwrap();
        let video = tracks.iter().find(|t| t["codec_type"] == "video").unwrap();
        assert_eq!(video["codec_name"], "av1");
        assert_eq!(video["pix_fmt"], "yuv420p10le");
        if source["mime"] == "video/x-matroska" {
            let audio: Vec<_> = tracks
                .iter()
                .filter(|t| t["codec_type"] == "audio")
                .collect();
            assert_eq!(audio.len(), 2);
            assert_eq!(audio[0]["codec_name"], "opus");
            assert_eq!(audio[0]["tags"]["language"], "fra");
            assert_eq!(audio[1]["tags"]["language"], "eng");
            assert_ne!(audio[0]["index"], audio[1]["index"]);
            assert_eq!(audio[0]["channels"], 6);
            // The original container cannot guarantee the browser's default audio.
            assert_eq!(source["audioTrackId"], "internal-default");
            assert_eq!(source["language"], "und");
            for (track, language) in audio.iter().zip(["fr", "en"]) {
                let track_id = format!("internal-{}", track["index"].as_u64().unwrap());
                let explicit = media["sources"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|candidate| candidate["audioTrackId"] == track_id)
                    .expect("each internal audio track must have an explicit source");
                assert_eq!(explicit["language"], language);
                assert_eq!(explicit["tracks"][0]["index"], track["index"]);
                assert_eq!(explicit["tracks"][0]["channels"], track["channels"]);
            }
            assert_eq!(
                tracks
                    .iter()
                    .filter(|t| t["codec_type"] == "subtitle")
                    .count(),
                3
            );
        } else {
            assert_eq!(source["language"], "en");
            assert_eq!(media["subtitles"][0]["language"], "fr");
            assert_eq!(media["subtitles"][0]["forced"], true);
            let url = media["subtitles"][0]["url"].as_str().unwrap();
            assert!(!media.to_string().contains("relative_path"));
            for (method, range, expected) in [
                ("GET", None, StatusCode::OK),
                ("HEAD", None, StatusCode::OK),
                ("GET", Some("bytes=0-5"), StatusCode::PARTIAL_CONTENT),
                (
                    "GET",
                    Some("bytes=999999-"),
                    StatusCode::RANGE_NOT_SATISFIABLE,
                ),
            ] {
                let (status, _, bytes, _) =
                    request(&app, method, url, &cookie, None, Value::Null, range).await;
                assert_eq!(status, expected);
                if method == "HEAD" {
                    assert!(bytes.is_empty());
                }
                if range == Some("bytes=0-5") {
                    assert_eq!(bytes, b"WEBVTT");
                }
            }
            assert_eq!(
                request(&app, "GET", url, "", None, Value::Null, None)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        }
    }
    tokio::fs::write(
        &sidecar,
        "1\n00:00:00,000 --> 00:00:10,000\nTexte actualisé",
    )
    .await
    .unwrap();
    let second = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(second["reused"], 2);
    assert_eq!(second["probed"], 1); // invalid input is retried, successful unchanged media is not
    let subtitle_asset: String = media_poc::database::query_scalar("SELECT a.id FROM inventory_assets a JOIN inventory_subtitles t ON t.asset_id=a.id WHERE a.relative_path LIKE '%version-mp4.mp4'")
        .fetch_one(store.pool()).await.unwrap();
    let subtitle_url = format!("/api/v1/assets/{subtitle_asset}/subtitles/0");
    assert!(
        String::from_utf8(
            request(&app, "GET", &subtitle_url, &cookie, None, Value::Null, None)
                .await
                .2
        )
        .unwrap()
        .contains("Texte actualisé")
    );
    let (_, again, _, _) = request(
        &app,
        "GET",
        &format!("/api/v1/catalog/{work}"),
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(again, detail, "stable IDs and no duplicate versions");
    tokio::fs::remove_file(&sidecar).await.unwrap();
    assert_eq!(
        request(&app, "GET", &subtitle_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::OK,
        "published snapshot survives until rescan"
    );
    scan(&app, &library, &cookie, csrf).await;
    assert_eq!(
        request(&app, "GET", &subtitle_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::NOT_FOUND,
        "rescan removes deleted sidecar even with reused video probe"
    );
    drop(app);
    store.close().await;
    let store = IdentityStore::open(&db).await.unwrap();
    let app = router(&root, &store).await;
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/api/v1/catalog/{work}"),
            &cookie,
            None,
            Value::Null,
            None
        )
        .await
        .1,
        detail,
        "persisted catalogue and session after reopened Core"
    );
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    for asset in &assets {
        let uri = format!("/api/v1/assets/{asset}/content");
        for (method, range, expected) in [
            ("GET", None, StatusCode::OK),
            ("HEAD", None, StatusCode::OK),
            ("GET", Some("bytes=0-31"), StatusCode::PARTIAL_CONTENT),
            ("HEAD", Some("bytes=0-31"), StatusCode::PARTIAL_CONTENT),
            (
                "GET",
                Some("bytes=999999999-"),
                StatusCode::RANGE_NOT_SATISFIABLE,
            ),
        ] {
            assert_eq!(
                request(&app, method, &uri, &cookie, None, Value::Null, range)
                    .await
                    .0,
                expected
            );
        }
    }
    sqlx::query("UPDATE users SET role='user' WHERE id=?")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "HEAD",
            &format!("/api/v1/assets/{subtitle_asset}/prepared/video"),
            &cookie,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", &subtitle_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
        .bind(&private)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "POST", &uri, &cookie, Some(csrf), Value::Null, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for asset in &assets {
        for method in ["GET", "HEAD"] {
            for range in [None, Some("bytes=0-31")] {
                assert_eq!(
                    request(
                        &app,
                        method,
                        &format!("/api/v1/assets/{asset}/content"),
                        &cookie,
                        None,
                        Value::Null,
                        range
                    )
                    .await
                    .0,
                    StatusCode::FORBIDDEN
                );
            }
        }
    }
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/catalog",
            &cookie,
            None,
            Value::Null,
            None
        )
        .await
        .1["total"],
        1
    );
    sqlx::query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
        .bind(&library)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    let content = format!("/api/v1/assets/{}/content", assets[0]);
    assert_eq!(
        request(
            &app,
            "GET",
            &content,
            &cookie,
            None,
            Value::Null,
            Some("bytes=0-31")
        )
        .await
        .0,
        StatusCode::PARTIAL_CONTENT
    );
    for statement in [
        "UPDATE libraries SET enabled=0 WHERE id=?",
        "UPDATE libraries SET enabled=1 WHERE id=?",
        "DELETE FROM library_permissions WHERE library_id=?",
    ] {
        sqlx::query(statement)
            .bind(&library)
            .execute(&pool)
            .await
            .unwrap();
        let expected = if statement.contains("enabled=1") {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::FORBIDDEN
        };
        for method in ["GET", "HEAD"] {
            assert_eq!(
                request(
                    &app,
                    method,
                    &content,
                    &cookie,
                    None,
                    Value::Null,
                    Some("bytes=0-31")
                )
                .await
                .0,
                expected
            );
        }
    }
    assert_eq!(
        (hash(&mkv).await, hash(&mp4).await),
        before,
        "source bytes remain unchanged"
    );
    drop(app);
    pool.close().await;
    store.close().await;
}
