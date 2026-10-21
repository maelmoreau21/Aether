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

// Generated, public-domain 1x1 PNG. These bytes and all videos are copied into disposable roots.
fn png() -> Vec<u8> {
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 3, 8, 2,
        0, 0, 0, 54, 136, 73, 214, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 136, 48, 234, 7, 34,
        6, 20, 10, 0, 67, 242, 6, 151, 143, 92, 131, 231, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]
}

#[tokio::test]
async fn local_posters_refresh_preserve_media_and_enforce_library_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let film = root.join("first/Poster Film (2026)");
    let second = root.join("second/Other Film (2025)");
    tokio::fs::create_dir_all(&film).await.unwrap();
    tokio::fs::create_dir_all(&second).await.unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/p1/clip-mp4-fr.mp4");
    let video = film.join("Film.mp4");
    tokio::fs::copy(&fixture, &video).await.unwrap();
    tokio::fs::copy(&fixture, second.join("Other.mp4"))
        .await
        .unwrap();
    let poster = film.join("poster.png");
    tokio::fs::write(&poster, png()).await.unwrap();
    tokio::fs::write(second.join("folder.png"), png())
        .await
        .unwrap();
    let nfo = film.join("movie.nfo");
    tokio::fs::write(
        &nfo,
        "<movie><title>Editorial preserved</title><genre>Drame</genre></movie>",
    )
    .await
    .unwrap();
    let original = (hash(&video).await, hash(&nfo).await, hash(&poster).await);
    let db = temp.path().join("posters.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();
    let user = store
        .bootstrap(
            "poster-admin",
            Zeroizing::new("synthetic poster test password".into()),
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
        json!({"login":"poster-admin","password":"synthetic poster test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let library = create_library(
        &app,
        film.parent().unwrap(),
        &cookie,
        csrf,
        "Poster library",
    )
    .await;
    let other_library = create_library(
        &app,
        second.parent().unwrap(),
        &cookie,
        csrf,
        "Other library",
    )
    .await;
    assert_eq!(scan(&app, &library, &cookie, csrf).await["probed"], 1);
    scan(&app, &other_library, &cookie, csrf).await;
    let (_, catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    let items = catalog["items"].as_array().unwrap();
    let work = items
        .iter()
        .find(|w| w["title"] == "Editorial preserved")
        .unwrap();
    let detail_url = format!("/api/v1/catalog/{}", work["id"].as_str().unwrap());
    let image_url = work["posterUrl"]
        .as_str()
        .expect("poster is indexed")
        .to_owned();
    assert!(image_url.starts_with("/api/v1/assets/"));
    assert!(
        !catalog
            .to_string()
            .contains(&root.to_string_lossy().to_string())
    );
    let other_url = items
        .iter()
        .find(|w| w["title"] != "Editorial preserved")
        .unwrap()["posterUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let (_, initial, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(&app, method, &image_url, "", None, Value::Null, None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        let (status, _, bytes, _) =
            request(&app, method, &image_url, &cookie, None, Value::Null, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, if method == "GET" { png() } else { vec![] });
    }
    let preview_url = format!("{image_url}&width=320");
    for width in [320, 640, 1280] {
        let url = format!("{image_url}&width={width}");
        for method in ["GET", "HEAD"] {
            assert_eq!(
                request(&app, method, &url, "", None, Value::Null, None)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
            let result = request(&app, method, &url, &cookie, None, Value::Null, None).await;
            assert_eq!(result.0, StatusCode::OK);
            if method == "GET" {
                assert_eq!(image::load_from_memory(&result.2).unwrap().width(), 2);
            } else {
                assert!(result.2.is_empty());
            }
        }
    }
    assert_eq!(
        request(
            &app,
            "GET",
            &preview_url,
            &cookie,
            None,
            Value::Null,
            Some("bytes=0-3")
        )
        .await
        .0,
        StatusCode::PARTIAL_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &preview_url,
            &cookie,
            None,
            Value::Null,
            Some("bytes=99999-")
        )
        .await
        .0,
        StatusCode::RANGE_NOT_SATISFIABLE
    );
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{image_url}&width=99999"),
            &cookie,
            None,
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(scan(&app, &library, &cookie, csrf).await["probed"], 0);
    assert_eq!(
        (hash(&video).await, hash(&nfo).await, hash(&poster).await),
        original
    );
    image::RgbImage::from_pixel(3, 4, image::Rgb([18, 150, 90]))
        .save(&poster)
        .unwrap();
    let changed = tokio::fs::read(&poster).await.unwrap();
    assert_eq!(scan(&app, &library, &cookie, csrf).await["probed"], 0);
    let (_, updated, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    let updated_url = updated["posterUrl"].as_str().unwrap();
    assert_ne!(updated_url, image_url);
    assert_eq!(updated_url.split('?').next(), image_url.split('?').next());
    assert_eq!(
        request(&app, "GET", updated_url, &cookie, None, Value::Null, None)
            .await
            .2,
        changed
    );
    // An invalid replacement must never be served, even before the next scan.
    tokio::fs::write(&poster, b"not a PNG").await.unwrap();
    assert_ne!(
        request(&app, "GET", &preview_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    assert_ne!(
        request(&app, "GET", &image_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(scan(&app, &library, &cookie, csrf).await["probed"], 0);
    tokio::fs::write(&poster, png()).await.unwrap();
    assert_eq!(scan(&app, &library, &cookie, csrf).await["probed"], 0);
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    sqlx::query("UPDATE users SET role='user' WHERE id=?")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
        .bind(&library)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(&app, method, &image_url, &cookie, None, Value::Null, None)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, method, &other_url, &cookie, None, Value::Null, None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE libraries SET enabled=0 WHERE id=?")
        .bind(&library)
        .execute(&pool)
        .await
        .unwrap();
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(&app, method, &image_url, &cookie, None, Value::Null, None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE libraries SET enabled=1 WHERE id=?")
        .bind(&library)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM library_permissions WHERE library_id=?")
        .bind(&library)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", &preview_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(&app, method, &image_url, &cookie, None, Value::Null, None)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE users SET role='administrator' WHERE id=?")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    tokio::fs::remove_file(&poster).await.unwrap();
    assert_eq!(scan(&app, &library, &cookie, csrf).await["probed"], 0);
    let (_, after, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    assert_eq!(after["id"], initial["id"]);
    assert_eq!(after["editions"], initial["editions"]);
    assert_eq!(after["title"], "Editorial preserved");
    assert!(after["posterUrl"].is_null());
    assert_ne!(
        request(&app, "GET", &image_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        (hash(&video).await, hash(&nfo).await),
        (original.0, original.1)
    );
    sqlx::query("DELETE FROM sessions WHERE user_id=?")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", &preview_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(&app, method, &image_url, &cookie, None, Value::Null, None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    drop(app);
    pool.close().await;
    store.close().await;
}
