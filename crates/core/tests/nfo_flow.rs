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
async fn hash(path: &Path) -> Vec<u8> {
    Sha256::digest(tokio::fs::read(path).await.unwrap()).to_vec()
}

#[tokio::test]
async fn nfo_refresh_preserves_media_graph_probe_and_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let film = root.join("Folder Film (2026) [tmdb-123]");
    tokio::fs::create_dir_all(&film).await.unwrap();
    let generated = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/p1");
    let mp4 = film.join("version-mp4.mp4");
    let mkv = film.join("version-mkv.mkv");
    tokio::fs::copy(generated.join("clip-mp4-fr.mp4"), &mp4)
        .await
        .unwrap();
    tokio::fs::copy(generated.join("clip-mkv-fr.mkv"), &mkv)
        .await
        .unwrap();
    let nfo = film.join("movie.nfo");
    let initial_xml = r#"<?xml version="1.0" encoding="UTF-8"?><movie><title>Été &amp; Neige</title><originaltitle>Original title</originaltitle><plot>Résumé &lt;img src=x onerror=alert(1)&gt;</plot><outline>Court résumé</outline><year>2020</year><premiered>2020-02-29</premiered><genre>Drame</genre><genre>Science-fiction</genre><rating>8.5</rating><mpaa>FR:Tous publics</mpaa><uniqueid type="tmdb">123</uniqueid><imdbid>tt1234567</imdbid><codec>fake-codec</codec><duration>999999</duration></movie>"#;
    tokio::fs::write(&nfo, initial_xml).await.unwrap();
    let before = (hash(&mp4).await, hash(&mkv).await, hash(&nfo).await);
    let db = temp.path().join("nfo.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();
    let user = store
        .bootstrap(
            "nfo-admin",
            Zeroizing::new("synthetic nfo test password".into()),
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
        json!({"login":"nfo-admin","password":"synthetic nfo test password"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let library = create_library(&app, &root, &cookie, csrf, "NFO Films").await;
    let first = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(first["probed"], 2);
    let (_, catalog, _, _) = request(
        &app,
        "GET",
        "/api/v1/catalog?q=%C3%89t%C3%A9",
        &cookie,
        None,
        Value::Null,
        None,
    )
    .await;
    assert_eq!(catalog["total"], 1, "{catalog}");
    let work = catalog["items"][0]["id"].as_str().unwrap().to_owned();
    let detail_url = format!("/api/v1/catalog/{work}");
    let (_, initial, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    assert_eq!(initial["title"], "Été & Neige");
    assert_eq!(initial["year"], 2020);
    assert_eq!(initial["originalTitle"], "Original title");
    assert_eq!(initial["genres"], json!(["Drame", "Science-fiction"]));
    assert_eq!(initial["premiered"], "2020-02-29");
    assert_eq!(initial["rating"], 8.5);
    assert_eq!(initial["mpaa"], "FR:Tous publics");
    assert!(
        initial["description"]
            .as_str()
            .unwrap()
            .contains("<img src=x onerror=alert(1)>")
    );
    assert_eq!(initial["externalIds"]["tmdb"], "123");
    assert_eq!(initial["externalIds"]["imdb"], "tt1234567");
    let graph = initial["editions"].clone();
    assert_eq!(graph[0]["versions"].as_array().unwrap().len(), 2);
    let asset = graph[0]["versions"][0]["sources"][0]["assetIds"][0]
        .as_str()
        .unwrap();
    let media_url = format!("/api/v1/media/{asset}");
    let (_, media_before, _, _) =
        request(&app, "GET", &media_url, &cookie, None, Value::Null, None).await;
    assert_eq!(media_before["sources"][0]["tracks"][0]["codec_name"], "av1");
    let repeat = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(repeat["probed"], 0);
    assert_eq!(repeat["reused"], 2);
    assert_eq!(
        (hash(&mp4).await, hash(&mkv).await, hash(&nfo).await),
        before
    );
    // Only disposable sidecars are intentionally edited to exercise refresh; the scanner never writes them.
    let update_xml = "<movie><title>Nouveau titre</title><plot>Synopsis actualisé</plot><year>2021</year><genre>Aventure</genre><tmdbid>123</tmdbid></movie>";
    tokio::fs::write(&nfo, update_xml).await.unwrap();
    let updated_hash = hash(&nfo).await;
    let update = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(update["probed"], 0);
    let (_, updated, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    assert_eq!(updated["title"], "Nouveau titre");
    assert_eq!(updated["year"], 2021);
    assert_eq!(updated["editions"], graph);
    assert_eq!(hash(&nfo).await, updated_hash);
    tokio::fs::write(&nfo, "<movie><title>broken")
        .await
        .unwrap();
    let bad_hash = hash(&nfo).await;
    let malformed = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(malformed["probed"], 0);
    assert!(!malformed["errors"].as_array().unwrap().is_empty());
    let (_, preserved, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    assert_eq!(preserved["title"], "Nouveau titre");
    assert_eq!(preserved["editions"], graph);
    assert_eq!(hash(&nfo).await, bad_hash);
    tokio::fs::remove_file(&nfo).await.unwrap();
    let removed = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(removed["probed"], 0);
    let (_, fallback, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    assert_eq!(fallback["title"], "Folder Film");
    assert_eq!(fallback["year"], 2026);
    assert_eq!(fallback["editions"], graph);
    assert_eq!(fallback["genres"], json!([]));
    let (_, media_after, _, _) =
        request(&app, "GET", &media_url, &cookie, None, Value::Null, None).await;
    assert_eq!(
        media_after["sources"], media_before["sources"],
        "NFO cannot overwrite technical tracks"
    );
    assert_eq!((hash(&mp4).await, hash(&mkv).await), (before.0, before.1));
    for hostile in [
        "<movie><title>broken".to_owned(),
        "<!DOCTYPE movie [<!ENTITY x SYSTEM 'file:///private'>]><movie><title>&x;</title></movie>"
            .into(),
        "<!DOCTYPE movie [<!ENTITY a 'xx'><!ENTITY b '&a;&a;'>]><movie><title>&b;</title></movie>"
            .into(),
        "x".repeat(1024 * 1024 + 1),
    ] {
        tokio::fs::write(&nfo, &hostile).await.unwrap();
        let expected_hash = hash(&nfo).await;
        let rejected = scan(&app, &library, &cookie, csrf).await;
        assert_eq!(rejected["probed"], 0);
        assert!(!rejected["errors"].as_array().unwrap().is_empty());
        let (_, still_available, _, _) =
            request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
        assert_eq!(still_available["title"], "Folder Film");
        assert_eq!(still_available["editions"], graph);
        assert_eq!(hash(&nfo).await, expected_hash);
    }
    tokio::fs::write(&nfo, initial_xml).await.unwrap();
    let exact_nfo = film.join("version-mkv.nfo");
    tokio::fs::write(&exact_nfo,"<movie><title>Exact NFO title</title><uniqueid type=\"tmdb\">456</uniqueid><uniqueid type=\"imdb\">tt7654321</uniqueid></movie>").await.unwrap();
    let conflict = scan(&app, &library, &cookie, csrf).await;
    assert_eq!(conflict["probed"], 0);
    assert!(!conflict["errors"].as_array().unwrap().is_empty());
    let (_, conflicted, _, _) =
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None).await;
    assert_eq!(conflicted["title"], "Exact NFO title");
    assert_eq!(conflicted["editions"], graph);
    assert!(conflicted["externalIds"].get("tmdb").is_none());
    assert!(conflicted["externalIds"].get("imdb").is_none());
    assert_eq!(conflicted["metadataWarnings"].as_array().unwrap().len(), 2);
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", db.display()))
        .await
        .unwrap();
    sqlx::query("UPDATE users SET role='user' WHERE id=?")
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
        .bind(&library)
        .bind(&user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", &detail_url, &cookie, None, Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
    sqlx::query("DELETE FROM library_permissions WHERE library_id=?")
        .bind(&library)
        .execute(&pool)
        .await
        .unwrap();
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(
                &app,
                method,
                &format!("/api/v1/assets/{asset}/content"),
                &cookie,
                None,
                Value::Null,
                Some("bytes=0-31")
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    drop(app);
    pool.close().await;
    store.close().await;
}

#[tokio::test]
async fn refresh_modes_preserve_known_fill_missing_and_force_all_without_writing_media() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    let generated = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/p1/clip-mp4-fr.mp4");
    for folder in ["First (2026) [tmdb-111]", "Second (2026) [tmdb-222]"] {
        tokio::fs::create_dir_all(root.join(folder)).await.unwrap();
        tokio::fs::copy(&generated, root.join(folder).join("movie.mp4"))
            .await
            .unwrap();
    }
    let first = root.join("First (2026) [tmdb-111]/movie.nfo");
    let second = root.join("Second (2026) [tmdb-222]/movie.nfo");
    tokio::fs::write(&first, "<movie><title>First original</title></movie>")
        .await
        .unwrap();
    let before = hash(&root.join("First (2026) [tmdb-111]/movie.mp4")).await;
    let store = IdentityStore::open(&temp.path().join("modes.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "modes-admin",
            Zeroizing::new("temporary modes test password".into()),
        )
        .await
        .unwrap();
    let app = router(&root, &store).await;
    let (_, login, _, cookie) = request(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        None,
        json!({"login":"modes-admin","password":"temporary modes test password"}),
        None,
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let library = create_library(&app, &root, &cookie, csrf, "Refresh modes").await;
    scan(&app, &library, &cookie, csrf).await;
    let initial: Vec<String> =
        media_poc::database::query_scalar("SELECT id FROM inventory_assets ORDER BY id")
            .fetch_all(store.pool())
            .await
            .unwrap();
    tokio::fs::write(&first, "<movie><title>First replacement</title></movie>")
        .await
        .unwrap();
    tokio::fs::write(&second, "<movie><title>Second completed</title></movie>")
        .await
        .unwrap();
    let missing = scan_mode(
        &app,
        &library,
        &cookie,
        csrf,
        json!({"mode":"missing_metadata"}),
    )
    .await;
    assert_eq!(missing["probed"], 0);
    let titles: Vec<String> =
        media_poc::database::query_scalar("SELECT title FROM inventory_works ORDER BY title")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert!(titles.contains(&"First original".into()));
    assert!(titles.contains(&"Second completed".into()));
    let third = root.join("Third (2026) [tmdb-333]");
    tokio::fs::create_dir_all(&third).await.unwrap();
    tokio::fs::copy(&generated, third.join("movie.mp4"))
        .await
        .unwrap();
    tokio::fs::write(
        third.join("movie.nfo"),
        "<movie><title>Third new</title></movie>",
    )
    .await
    .unwrap();
    tokio::fs::write(&second, "<movie><title>Second replacement</title></movie>")
        .await
        .unwrap();
    scan_mode(&app, &library, &cookie, csrf, json!({"mode":"new_only"})).await;
    let titles: Vec<String> =
        media_poc::database::query_scalar("SELECT title FROM inventory_works ORDER BY title")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert!(titles.contains(&"First original".into()));
    assert!(titles.contains(&"Second completed".into()));
    assert!(titles.contains(&"Third new".into()));
    let full = scan_mode(&app, &library, &cookie, csrf, json!({"mode":"full"})).await;
    assert_eq!(full["probed"], 3);
    assert_eq!(full["reused"], 0);
    let titles: Vec<String> =
        media_poc::database::query_scalar("SELECT title FROM inventory_works ORDER BY title")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert!(titles.contains(&"First replacement".into()));
    assert!(titles.contains(&"Second replacement".into()));
    let after: Vec<String> =
        media_poc::database::query_scalar("SELECT id FROM inventory_assets ORDER BY id")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert!(initial.iter().all(|id| after.contains(id)));
    assert_eq!(
        hash(&root.join("First (2026) [tmdb-111]/movie.mp4")).await,
        before
    );
    for options in [
        json!({"mode":"arbitrary"}),
        json!({"mode":"full","path":"/outside"}),
    ] {
        assert_eq!(
            request(
                &app,
                "POST",
                &format!("/api/v1/admin/libraries/{library}/scan"),
                &cookie,
                Some(csrf),
                options,
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }

    let series_root = root.join("Series");
    let show = series_root.join("Show (2026)");
    let season = show.join("Season 01");
    tokio::fs::create_dir_all(&season).await.unwrap();
    tokio::fs::copy(&generated, season.join("Show.S01E01.mp4"))
        .await
        .unwrap();
    let tv = show.join("tvshow.nfo");
    let episode = season.join("Show.S01E01.nfo");
    tokio::fs::write(&tv, "<tvshow><title>Show original</title></tvshow>")
        .await
        .unwrap();
    tokio::fs::write(
        &episode,
        "<episodedetails><title>Episode original</title></episodedetails>",
    )
    .await
    .unwrap();
    let (status,body,_,_)=request(&app,"POST","/api/v1/admin/libraries",&cookie,Some(csrf),json!({"name":"Series modes","kind":"series","nodeId":"local","root":series_root.to_string_lossy(),"enabled":true,"userIds":[]}),None).await;
    assert_eq!(status, StatusCode::CREATED);
    let series = body["id"].as_str().unwrap();
    scan(&app, series, &cookie, csrf).await;
    tokio::fs::write(&tv, "<tvshow><title>Show replaced</title></tvshow>")
        .await
        .unwrap();
    tokio::fs::write(
        &episode,
        "<episodedetails><title>Episode replaced</title></episodedetails>",
    )
    .await
    .unwrap();
    for mode in ["new_only", "missing_metadata"] {
        scan_mode(&app, series, &cookie, csrf, json!({"mode":mode})).await;
        let titles: Vec<String> = media_poc::database::query_scalar(
            "SELECT title FROM inventory_works WHERE library_id=? ORDER BY title",
        )
        .bind(series)
        .fetch_all(store.pool())
        .await
        .unwrap();
        assert!(
            titles.contains(&"Show original".into()),
            "{mode}: {titles:?}"
        );
        assert!(
            titles.contains(&"Episode original".into()),
            "{mode}: {titles:?}"
        );
    }
    let full = scan_mode(&app, series, &cookie, csrf, json!({"mode":"full"})).await;
    assert_eq!(full["probed"], 1);
    let titles: Vec<String> = media_poc::database::query_scalar(
        "SELECT title FROM inventory_works WHERE library_id=? ORDER BY title",
    )
    .bind(series)
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert!(titles.contains(&"Show replaced".into()));
    assert!(titles.contains(&"Episode replaced".into()));
    store.close().await;
}
