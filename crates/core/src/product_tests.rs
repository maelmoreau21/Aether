use crate::{
    auth::{self, AuthState},
    database::{query, query_scalar},
    identity::IdentityStore,
    product,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;
const ORIGIN: &str = "http://127.0.0.1:3000";
#[test]
fn access_schedule_utc_boundaries_and_week_wrap() {
    let windows = serde_json::from_value::<Vec<product::AccessWindow>>(json!([
        {"day":0,"startMinute":480,"endMinute":1200},
        {"day":6,"startMinute":1380,"endMinute":1440}
    ]))
    .unwrap();
    let monday = 4 * 86400;
    assert!(!product::schedule_allows(&windows, monday + 479 * 60 + 59));
    assert!(product::schedule_allows(&windows, monday + 480 * 60));
    assert!(!product::schedule_allows(&windows, monday + 1200 * 60));
    assert!(product::schedule_allows(
        &windows,
        monday + 6 * 86400 + 86399
    ));
    assert!(!product::schedule_allows(&windows, monday + 7 * 86400));
    assert!(product::schedule_allows(&[], monday));
}
#[tokio::test]
async fn upgrade_keeps_existing_libraries_rights_inventory_and_progress() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("upgrade.db");
    sqlx::any::install_default_drivers();
    let encoded: String = path
        .to_str()
        .unwrap()
        .as_bytes()
        .iter()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite:{encoded}?mode=rwc"))
        .await
        .unwrap();
    query("PRAGMA foreign_keys=OFF")
        .execute(&pool)
        .await
        .unwrap();
    let mut previous = sqlx::migrate!("./migrations");
    previous.migrations = std::borrow::Cow::Owned(
        previous
            .iter()
            .filter(|migration| migration.version <= 31)
            .cloned()
            .collect(),
    );
    previous.run(&pool).await.unwrap();
    query("INSERT INTO users(id,role) VALUES('kept-user','administrator')")
        .execute(&pool)
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('kept-library','Kept library','films','local','/test-media',1)").execute(&pool).await.unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('kept-library','kept-user')")
        .execute(&pool)
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,external_ids_json,kind) VALUES('kept-work','kept-library','kept','Kept film','kept film','{}','film')").execute(&pool).await.unwrap();
    query("INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) VALUES('kept-user','kept-work',42.0,100.0,0,3,unixepoch())").execute(&pool).await.unwrap();
    pool.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        query_scalar::<_, i64>(
            "SELECT count(*) FROM libraries WHERE id='kept-library' AND kind='films'"
        )
        .fetch_one(&store.pool)
        .await
        .unwrap(),
        1
    );
    assert_eq!(query_scalar::<_,i64>("SELECT count(*) FROM effective_library_permissions WHERE library_id='kept-library' AND user_id='kept-user'").fetch_one(&store.pool).await.unwrap(),1);
    assert_eq!(
        query_scalar::<_, f64>(
            "SELECT position_seconds FROM playback_progress WHERE work_id='kept-work'"
        )
        .fetch_one(&store.pool)
        .await
        .unwrap(),
        42.0
    );
    assert_eq!(
        query_scalar::<_, i64>("SELECT count(*) FROM pragma_foreign_key_check")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        0
    );
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('new-music','Music','music','local','/test-music',1)").execute(&store.pool).await.unwrap();
}
async fn request(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .header("origin", ORIGIN)
                .header("host", "127.0.0.1:3000")
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn value(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
#[tokio::test]
async fn parental_filters_overrides_lists_and_explicit_grouping() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("media");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap().to_string_lossy().into_owned();
    let store = IdentityStore::open(&temporary.path().join("product.db"))
        .await
        .unwrap();
    let admin = store
        .bootstrap(
            "admin",
            Zeroizing::new("product integration strong secret".into()),
        )
        .await
        .unwrap();
    query("INSERT INTO users(id,role,created_at) VALUES('reader','user',unixepoch()),('other','user',unixepoch())").execute(&store.pool).await.unwrap();
    for (token, user) in [
        ("a".repeat(64), admin.clone()),
        ("b".repeat(64), "reader".into()),
        ("c".repeat(64), "other".into()),
    ] {
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)").bind(auth::hash(&token)).bind(user).execute(&store.pool).await.unwrap();
    }
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)").bind(&root).execute(&store.pool).await.unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader'),('films','other')").execute(&store.pool).await.unwrap();
    for (work, age) in [
        ("young", Some(7i64)),
        ("adult", Some(18i64)),
        ("unknown", None),
    ] {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,year,external_ids_json,editorial_json,minimum_age) VALUES(?,'films',?,?,?,'film',2026,'{}','{}',?)").bind(work).bind(work).bind(work).bind(work).bind(age).execute(&store.pool).await.unwrap();
        query("INSERT INTO inventory_editions(id,work_id) VALUES(?,?)")
            .bind(work)
            .bind(work)
            .execute(&store.pool)
            .await
            .unwrap();
        query("INSERT INTO inventory_versions(id,edition_id) VALUES(?,?)")
            .bind(work)
            .bind(work)
            .execute(&store.pool)
            .await
            .unwrap();
        query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,'films',?)").bind(work).bind(work).bind(&root).execute(&store.pool).await.unwrap();
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,?,'films',?,1,'0','video/mp4','{\"streams\":[],\"format\":{}}',1)").bind(work).bind(work).bind(work).execute(&store.pool).await.unwrap();
    }
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let a = format!("media_session={}", "a".repeat(64));
    let b = format!("media_session={}", "b".repeat(64));
    let c = format!("media_session={}", "c".repeat(64));
    let app = crate::secure_app(
        crate::node::Inventory::local(root.clone().into())
            .await
            .unwrap(),
        3000,
        auth.clone(),
    )
    .await
    .unwrap();
    let ac=value(request(&app,"/api/v1/auth/me","GET",&a,"",Value::Null).await).await["csrfToken"].as_str().unwrap().to_owned();
    let bc=value(request(&app,"/api/v1/auth/me","GET",&b,"",Value::Null).await).await["csrfToken"].as_str().unwrap().to_owned();
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users/reader/parental",
            "PUT",
            &b,
            &bc,
            json!({"maximumAge":12,"allowUnrated":false})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users/reader/parental",
            "PUT",
            &a,
            &ac,
            json!({"maximumAge":12,"allowUnrated":false})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let catalog = value(
        request(
            &app,
            "/api/v1/catalog?kind=film",
            "GET",
            &b,
            "",
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(catalog["total"], 1);
    assert_eq!(catalog["items"][0]["id"], "young");
    for path in [
        "/api/v1/catalog/adult",
        "/api/v1/media/adult",
        "/api/v1/assets/adult/content",
        "/api/v1/assets/adult/prepared/video",
    ] {
        for method in ["GET", "HEAD"] {
            assert_eq!(
                request(&app, path, method, &b, "", Value::Null)
                    .await
                    .status(),
                StatusCode::FORBIDDEN,
                "{method} {path}"
            );
        }
    }
    assert!(
        !product::target_allowed(&store, "reader", "asset", "adult")
            .await
            .unwrap()
    );
    assert_eq!(request(&app,"/api/v1/admin/works/young/metadata","PUT",&a,&ac,json!({"title":"Titre corrigé","year":2025,"plot":"Description manuelle","genres":["Animation"],"externalIds":{"imdb":"tt0000000"},"minimumAge":7})).await.status(),StatusCode::NO_CONTENT);
    let detail =
        value(request(&app, "/api/v1/catalog/young", "GET", &b, "", Value::Null).await).await;
    assert_eq!(detail["title"], "Titre corrigé");
    let catalog = value(
        request(
            &app,
            "/api/v1/catalog?kind=film&genre=Animation&year=2025",
            "GET",
            &b,
            "",
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(catalog["total"], 1);
    let list = value(
        request(
            &app,
            "/api/v1/lists",
            "POST",
            &b,
            &bc,
            json!({"name":"À regarder"}),
        )
        .await,
    )
    .await;
    let id = list["id"].as_str().unwrap();
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/lists/{id}/items/young"),
            "PUT",
            &b,
            &bc,
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/lists/{id}"),
            "GET",
            &c,
            "",
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        value(
            request(
                &app,
                &format!("/api/v1/lists/{id}"),
                "GET",
                &b,
                "",
                Value::Null
            )
            .await
        )
        .await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/media-groups",
            "POST",
            &a,
            &ac,
            json!({"label":"Même film","workIds":["young","unknown"]})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let catalog = value(
        request(
            &app,
            "/api/v1/catalog?kind=film",
            "GET",
            &a,
            "",
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(catalog["total"], 2);
    let detail =
        value(request(&app, "/api/v1/catalog/young", "GET", &a, "", Value::Null).await).await;
    assert_eq!(detail["editions"].as_array().unwrap().len(), 2);
    // A second device reads the first device's track choices, without leaking them to another account.
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'reader',unixepoch(),unixepoch(),unixepoch()+3600)").bind(auth::hash(&"e".repeat(64))).execute(&store.pool).await.unwrap();
    assert_eq!(request(&app,"/api/v1/preferences","PUT",&b,&bc,json!({"audioLanguage":"fr","subtitlePreference":{"enabled":true,"language":"fr","forced":true},"offsets":{"audio":1.25,"subtitle":-0.5}})).await.status(),StatusCode::NO_CONTENT);
    let second = format!("media_session={}", "e".repeat(64));
    let synced =
        value(request(&app, "/api/v1/preferences", "GET", &second, "", Value::Null).await).await;
    assert_eq!(synced["audioLanguage"], "fr");
    assert_eq!(synced["offsets"]["audio"], 1.25);
    assert_eq!(
        value(request(&app, "/api/v1/preferences", "GET", &c, "", Value::Null).await).await,
        json!({})
    );
    // Progress and feedback refer to a stable shared identity while requests keep their visible copy ID.
    assert_eq!(
        request(
            &app,
            "/api/v1/progress/young",
            "PUT",
            &a,
            &ac,
            json!({"position":3.0,"duration":10.0,"watched":false,"expectedRevision":0})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let shared =
        value(request(&app, "/api/v1/progress/unknown", "GET", &a, "", Value::Null).await).await;
    assert_eq!(shared["position"], 3.0);
    assert_eq!(shared["workId"], "unknown");
    assert_eq!(
        request(
            &app,
            "/api/v1/feedback/young",
            "PUT",
            &a,
            &ac,
            json!({"favorite":true,"rating":4.5})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        value(request(&app, "/api/v1/feedback/unknown", "GET", &a, "", Value::Null).await).await["favorite"],
        true
    );
    query("INSERT INTO inventory_tracks(id,asset_id,stream_index,metadata_json) VALUES('track-video','young',0,'{\"codec_type\":\"video\",\"codec_name\":\"av1\",\"height\":1080}'),('track-audio','young',1,'{\"codec_type\":\"audio\",\"codec_name\":\"opus\",\"tags\":{\"language\":\"fra\"}}')").execute(&store.pool).await.unwrap();
    let filtered = value(
        request(
            &app,
            "/api/v1/catalog?kind=film&quality=1080&codec=opus&language=fra",
            "GET",
            &b,
            "",
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(filtered["total"], 1);
    let token = format!("media_{}", "f".repeat(64));
    query("INSERT INTO api_keys(id,name,user_id,token_hash,created_at,expires_at,scope) VALUES('parent-key','Restricted key','reader',?,unixepoch(),unixepoch()+3600,'catalog:read')").bind(auth::hash(&token)).execute(&store.pool).await.unwrap();
    query("INSERT INTO api_key_libraries(key_id,library_id) VALUES('parent-key','films')")
        .execute(&store.pool)
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/integrations/catalog")
                .header("host", "127.0.0.1:3000")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(value(response).await["items"].as_array().unwrap().len(), 1);
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(64, 96)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/works/young/poster")
                .method("PUT")
                .header("host", "127.0.0.1:3000")
                .header("origin", ORIGIN)
                .header("cookie", &a)
                .header("x-csrf-token", &ac)
                .header("content-type", "image/png")
                .body(Body::from(bytes.into_inner()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = request(
        &app,
        "/api/v1/works/young/poster",
        "GET",
        &b,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!(image.width() * 3, image.height() * 2);
    query("DELETE FROM library_permissions WHERE user_id='reader'")
        .execute(&store.pool)
        .await
        .unwrap();
    assert!(
        value(
            request(
                &app,
                &format!("/api/v1/lists/{id}"),
                "GET",
                &b,
                "",
                Value::Null
            )
            .await
        )
        .await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let violations: i64 = query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(violations, 0);
}

#[tokio::test]
async fn music_scan_metadata_stable_identity_and_range() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=1",
            "-c:a",
            "libopus",
            "-metadata",
            "title=Une piste",
            "-metadata",
            "artist=Un artiste",
            "-metadata",
            "album=Un album",
            "-y",
        ])
        .arg(root.join("track.opus"))
        .status()
        .unwrap();
    assert!(generated.success());
    let store = IdentityStore::open(&temp.path().join("music.db"))
        .await
        .unwrap();
    let user = store
        .bootstrap(
            "admin",
            Zeroizing::new("music integration strong secret".into()),
        )
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('music','Musique','music','local',?,1)").bind(root.to_string_lossy().as_ref()).execute(&store.pool).await.unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(
        crate::music::scan(&store, "music", &root, &cancel)
            .await
            .unwrap(),
        1
    );
    let id: String = query_scalar("SELECT id FROM music_tracks")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    crate::music::scan(&store, "music", &root, &cancel)
        .await
        .unwrap();
    assert_eq!(
        query_scalar::<_, String>("SELECT id FROM music_tracks")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        id
    );
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)").bind(auth::hash(&"d".repeat(64))).bind(user).execute(&store.pool).await.unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = crate::music::router(auth);
    let cookie = format!("media_session={}", "d".repeat(64));
    let listed =
        value(request(&app, "/api/v1/music?q=Une", "GET", &cookie, "", Value::Null).await).await;
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["artist"], "Un artiste");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/music/tracks/{id}/content"))
                .header("cookie", &cookie)
                .header("range", "bytes=0-15")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len(),
        16
    );
    std::fs::remove_file(root.join("track.opus")).unwrap();
    crate::music::scan(&store, "music", &root, &cancel)
        .await
        .unwrap();
    assert_eq!(
        value(request(&app, "/api/v1/music", "GET", &cookie, "", Value::Null).await).await["total"],
        0
    );
}

#[tokio::test]
async fn granular_parental_tags_schedules_downloads_and_library_images() {
    use sha2::{Digest, Sha256};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("media");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("clip.mp4"), b"synthetic range fixture").unwrap();
    let opened = media_node::OpenedFile::open_in_root(&root, std::path::Path::new("clip.mp4"))
        .await
        .unwrap();
    let store = IdentityStore::open(&temp.path().join("options.sqlite"))
        .await
        .unwrap();
    for (id, role, token) in [
        ("admin", "administrator", "a".repeat(64)),
        ("reader", "user", "b".repeat(64)),
    ] {
        query("INSERT INTO users(id,role) VALUES(?,?)")
            .bind(id)
            .bind(role)
            .execute(&store.pool)
            .await
            .unwrap();
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)")
            .bind(format!("{:x}",Sha256::digest(token.as_bytes()))).bind(id).execute(&store.pool).await.unwrap();
    }
    let root_string = root.to_string_lossy().to_string();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local',?,1)").bind(&root_string).execute(&store.pool).await.unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('films','reader')")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,editorial_json) VALUES('film','films','film','Film','film','film','{}','{\"mpaa\":\"-12\",\"tags\":[\"horreur\"]}')").execute(&store.pool).await.unwrap();
    query("INSERT INTO inventory_editions(id,work_id) VALUES('edition','film')")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO inventory_versions(id,edition_id) VALUES('version','edition')")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('source','version','films',?)").bind(&root_string).execute(&store.pool).await.unwrap();
    query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('asset','source','films','clip.mp4',?,?,'video/mp4','{\"streams\":[],\"format\":{}}',1)").bind(opened.size() as i64).bind(opened.modified_ns()).execute(&store.pool).await.unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let a = format!("media_session={}", "a".repeat(64));
    let b = format!("media_session={}", "b".repeat(64));
    let csrf = format!(
        "{:x}",
        Sha256::digest(format!("csrf:{}", "a".repeat(64)).as_bytes())
    );
    let app = crate::secure_app(
        crate::node::Inventory::local(root.clone()).await.unwrap(),
        3000,
        auth.clone(),
    )
    .await
    .unwrap();
    let permission = "/api/v1/admin/users/reader/media-policy";
    for (cookie, token) in [(&b, csrf.as_str()), (&a, "")] {
        assert_eq!(
            request(
                &app,
                permission,
                "PUT",
                cookie,
                token,
                json!({"allowMediaDownload":false})
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &app,
            "/api/v1/assets/asset/download",
            "GET",
            &b,
            "",
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            permission,
            "PUT",
            &a,
            &csrf,
            json!({"allowMediaDownload":false})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        value(request(&app, "/api/v1/me/media-policy", "GET", &b, "", Value::Null).await).await["allowMediaDownload"],
        false
    );
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(
                &app,
                "/api/v1/assets/asset/download",
                method,
                &b,
                "",
                Value::Null
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &app,
                "/api/v1/assets/asset/content",
                method,
                &b,
                "",
                Value::Null
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    let range = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/assets/asset/content")
                .header("host", "127.0.0.1:3000")
                .header("cookie", &b)
                .header("range", "bytes=0-3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        &range.into_body().collect().await.unwrap().to_bytes()[..],
        b"synt"
    );
    let parental = "/api/v1/admin/users/reader/parental";
    assert_eq!(
        request(
            &app,
            parental,
            "PUT",
            &a,
            &csrf,
            json!({"maximumAge":12,"allowUnrated":false})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        product::parental_allowed(&store, "reader", "film")
            .await
            .unwrap()
    );
    assert_eq!(
        request(
            &app,
            parental,
            "PUT",
            &a,
            &csrf,
            json!({"maximumAge":12,"allowUnrated":false,"blockedTags":["HORREUR"]})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let catalog = value(
        request(
            &app,
            "/api/v1/catalog?kind=film",
            "GET",
            &b,
            "",
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(catalog["total"], 0);
    for path in [
        "/api/v1/catalog/film",
        "/api/v1/media/asset",
        "/api/v1/assets/asset/content",
        "/api/v1/assets/asset/download",
        "/api/v1/assets/asset/prepared/video",
    ] {
        for method in ["GET", "HEAD"] {
            assert_eq!(
                request(&app, path, method, &b, "", Value::Null)
                    .await
                    .status(),
                StatusCode::FORBIDDEN,
                "{method} {path}"
            );
        }
    }
    // Exact matching: a substring must not block a tag.
    assert_eq!(
        request(
            &app,
            parental,
            "PUT",
            &a,
            &csrf,
            json!({"maximumAge":12,"allowUnrated":false,"blockedTags":["hor"]})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        product::parental_allowed(&store, "reader", "film")
            .await
            .unwrap()
    );
    let epoch: i64 = query_scalar("SELECT unixepoch()")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    let tomorrow = (epoch.div_euclid(86400) + 4).rem_euclid(7);
    assert_eq!(request(&app,parental,"PUT",&a,&csrf,json!({"maximumAge":12,"allowUnrated":false,"accessSchedule":[{"day":tomorrow,"startMinute":0,"endMinute":1440}]})).await.status(),StatusCode::NO_CONTENT);
    assert_eq!(
        request(&app, "/api/v1/auth/me", "GET", &b, "", Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "/api/v1/auth/me", "GET", &a, "", Value::Null)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(request(&app,parental,"PUT",&a,&csrf,json!({"maximumAge":12,"allowUnrated":false,"accessSchedule":[{"day":7,"startMinute":0,"endMinute":1440}]})).await.status(),StatusCode::BAD_REQUEST);
    assert_eq!(
        request(&app, parental, "DELETE", &a, &csrf, Value::Null)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "/api/v1/auth/me", "GET", &b, "", Value::Null)
            .await
            .status(),
        StatusCode::OK
    );
    let mut output = std::io::Cursor::new(vec![]);
    image::DynamicImage::new_rgb8(20, 30)
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    let image_path = "/api/v1/admin/libraries/films/images/poster";
    for (cookie, token, status) in [
        (&b, csrf.as_str(), StatusCode::FORBIDDEN),
        (&a, "", StatusCode::FORBIDDEN),
        (&a, csrf.as_str(), StatusCode::NO_CONTENT),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(image_path)
                    .header("host", "127.0.0.1:3000")
                    .header("origin", ORIGIN)
                    .header("cookie", cookie)
                    .header("x-csrf-token", token)
                    .header("content-type", "image/png")
                    .body(Body::from(output.get_ref().clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
    let response = request(&app, image_path, "GET", &a, "", Value::Null).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(image::load_from_memory(&bytes).is_ok());
    assert_eq!(
        request(&app, image_path, "GET", &b, "", Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, image_path, "DELETE", &a, &csrf, Value::Null)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, image_path, "GET", &a, "", Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // Lightweight library list is scoped and does not expose disk paths or grants.
    let libraries =
        value(request(&app, "/api/v1/libraries", "GET", &b, "", Value::Null).await).await;
    assert_eq!(libraries["libraries"][0]["name"], "Films");
    assert!(libraries["libraries"][0].get("root").is_none());
    assert_eq!(
        request(
            &app,
            "/api/v1/catalog?library=not-granted",
            "GET",
            &b,
            "",
            Value::Null
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        value(
            request(
                &app,
                "/api/v1/catalog?library=films",
                "GET",
                &b,
                "",
                Value::Null
            )
            .await
        )
        .await["total"],
        1
    );
    let mut saved = vec![];
    for kind in ["poster", "logo", "backdrop", "backdrop"] {
        let url = format!("/api/v1/works/film/images/{kind}");
        for (cookie, token, status) in [
            (&b, csrf.as_str(), StatusCode::FORBIDDEN),
            (&a, "", StatusCode::FORBIDDEN),
            (&a, csrf.as_str(), StatusCode::CREATED),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(&url)
                        .header("host", "127.0.0.1:3000")
                        .header("origin", ORIGIN)
                        .header("cookie", cookie)
                        .header("x-csrf-token", token)
                        .header("content-type", "image/png")
                        .body(Body::from(output.get_ref().clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            if status == StatusCode::CREATED {
                saved.push(value(response).await["id"].as_str().unwrap().to_owned());
            }
        }
    }
    let entries = value(
        request(
            &app,
            "/api/v1/works/film/images",
            "GET",
            &b,
            "",
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(entries["images"].as_array().unwrap().len(), 4);
    assert!(
        entries["images"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["width"] == 20 && e["height"] == 30)
    );
    let url = format!("/api/v1/works/film/images/{}?width=320", saved[0]);
    let image = request(&app, &url, "GET", &b, "", Value::Null).await;
    assert_eq!(image.status(), StatusCode::OK);
    let etag = image.headers()["etag"].to_str().unwrap().to_owned();
    let bytes = image.into_body().collect().await.unwrap().to_bytes();
    assert!(image::load_from_memory(&bytes).is_ok());
    let conditional = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&url)
                .header("host", "127.0.0.1:3000")
                .header("cookie", &b)
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
    let reverse = json!({"ids":[saved[3],saved[2]]});
    assert_eq!(
        request(
            &app,
            "/api/v1/works/film/images/order",
            "PUT",
            &a,
            &csrf,
            reverse
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let work = value(request(&app, "/api/v1/catalog/film", "GET", &b, "", Value::Null).await).await;
    assert!(
        work["artwork"]["backdrop"]
            .as_str()
            .unwrap()
            .contains(&saved[3])
    );
    assert!(work["posterUrl"].as_str().unwrap().contains(&saved[0]));
    let original = std::fs::read(root.join("clip.mp4")).unwrap();
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/works/film/images/{}", saved[0]),
            "DELETE",
            &a,
            &csrf,
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, &url, "HEAD", &b, "", Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    query("UPDATE libraries SET enabled=0 WHERE id='films'")
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/works/film/images/{}", saved[1]),
            "HEAD",
            &b,
            "",
            Value::Null
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(std::fs::read(root.join("clip.mp4")).unwrap(), original);
}
