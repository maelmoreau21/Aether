//! Per-file media tools over HTTP: generated-data status, preview and chapter rebuilds, and the
//! guarantee that a manual segment is locked (never touched by an analysis, released only by an
//! explicit reset). A remote Node stands in for the file's holder: nothing here runs FFmpeg.
use aether_core::{
    auth::{self, AuthState},
    database::{query, query_scalar},
    identity::IdentityStore,
};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn fixture(dir: &std::path::Path) -> IdentityStore {
    let store = IdentityStore::open(&dir.join("media-tools.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "tools-admin",
            zeroize::Zeroizing::new("synthetic tools password".into()),
        )
        .await
        .unwrap();
    for statement in [
        "INSERT INTO media_nodes(id,name,kind,status) VALUES('node-1','node-1','remote','connected')",
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('library-1','Films','films','node-1','/read-only',1)",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind) VALUES('film-1','library-1','film-1','Film','{}','film')",
        "INSERT INTO inventory_editions(id,work_id) VALUES('edition-1','film-1')",
        "INSERT INTO inventory_versions(id,edition_id) VALUES('version-1','edition-1')",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('source-1','version-1','library-1','/read-only')",
    ] {
        query(statement).execute(store.pool()).await.unwrap();
    }
    query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('asset-1','source-1','library-1','film.mkv',100,'1','video/x-matroska',?,1)")
        .bind(json!({
            "streams": [{"index":0,"codec_type":"video","codec_name":"av1"}],
            "format": {"duration": "1500"},
            "chapters": [
                {"start_time":"0","tags":{"title":"Intro"}},
                {"start_time":"90","tags":{"title":"Film"}},
                {"start_time":"1400","tags":{"title":"Credits"}}
            ]
        }).to_string())
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO automation_observations(asset_id,library_id,fingerprint,revision) VALUES('asset-1','library-1','fp-1',1)")
        .execute(store.pool()).await.unwrap();
    store
}

#[tokio::test]
async fn status_actions_and_locked_segments_follow_the_administrator() {
    let temp = tempfile::tempdir().unwrap();
    let store = fixture(temp.path()).await;
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = aether_core::timeline_markers::router(auth.clone())
        .merge(aether_core::media_tools::router(auth.clone()))
        .merge(auth::router(auth));

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"login":"tools-admin","password":"synthetic tools password"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let login: Value =
        serde_json::from_slice(&login.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let csrf = login["csrfToken"].as_str().unwrap().to_owned();

    let call = |method: &'static str, uri: &str, token: Option<&str>, with_cookie: bool| {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::ORIGIN, "http://127.0.0.1:3000");
        if with_cookie {
            builder = builder.header(header::COOKIE, &cookie);
        }
        if let Some(token) = token {
            builder = builder.header("x-csrf-token", token);
        }
        let app = app.clone();
        let request = builder.body(Body::empty()).unwrap();
        async move {
            let response = app.oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };

    // Anonymous callers and a mutation without the CSRF token are refused.
    let (status, _) = call(
        "GET",
        "/api/v1/admin/assets/asset-1/media-status",
        None,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        "POST",
        "/api/v1/admin/assets/asset-1/regenerate-trickplay",
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Nothing generated yet: three chapters, one detection-free file.
    let (status, body) = call(
        "GET",
        "/api/v1/admin/assets/asset-1/media-status",
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["trickplay"]["status"], "missing");
    assert_eq!(body["trickplay"]["canPurge"], false);
    assert_eq!(body["chapters"]["count"], 3);
    assert_eq!(body["segments"]["manual"], 0);

    // Regenerating queues one durable preview task; the status then reports it as running.
    let (status, body) = call(
        "POST",
        "/api/v1/admin/assets/asset-1/regenerate-trickplay",
        Some(&csrf),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["purged"], false,
        "a remote Node's cache is not deleted from the Core"
    );
    assert_eq!(body["tasksQueued"], 1);
    let queued: i64 = query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE target_id='asset-1' AND category='trickplay' AND component='' AND status='queued'")
        .fetch_one(store.pool()).await.unwrap();
    assert_eq!(queued, 1);
    let (_, body) = call(
        "GET",
        "/api/v1/admin/assets/asset-1/media-status",
        None,
        true,
    )
    .await;
    assert_eq!(body["trickplay"]["status"], "running");

    // Refreshing the chapters also queues their images, and reports the chapters re-read.
    let (status, body) = call(
        "POST",
        "/api/v1/admin/assets/asset-1/refresh-chapters",
        Some(&csrf),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["chaptersReread"], true);
    assert_eq!(body["tasksQueued"], 1);

    // A correction made on the chapters' own bounds is locked and reads back as a chapter segment.
    query("INSERT INTO manual_timeline_markers(asset_id,marker_type,start_seconds,end_seconds,enabled,updated_by,updated_at) VALUES('asset-1','intro',0,90,1,(SELECT id FROM users LIMIT 1),1)")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO manual_timeline_markers(asset_id,marker_type,start_seconds,end_seconds,enabled,updated_by,updated_at) VALUES('asset-1','recap',10,40.5,1,(SELECT id FROM users LIMIT 1),1)")
        .execute(store.pool()).await.unwrap();
    let (status, body) = call("GET", "/api/v1/admin/assets/asset-1/segments", None, true).await;
    assert_eq!(status, StatusCode::OK);
    let manual = body["asset"]["manual"].as_array().unwrap();
    let source_of =
        |kind: &str| manual.iter().find(|m| m["type"] == kind).unwrap()["source"].clone();
    assert_eq!(source_of("intro"), "chapter");
    assert_eq!(source_of("recap"), "manual");
    assert!(manual.iter().all(|m| m["locked"] == true));
    let (_, body) = call(
        "GET",
        "/api/v1/admin/assets/asset-1/media-status",
        None,
        true,
    )
    .await;
    assert_eq!(body["segments"]["locked"], 2);

    // Re-reading the chapters (an analysis) leaves the corrections as they are.
    let (status, _) = call(
        "POST",
        "/api/v1/admin/assets/asset-1/refresh-chapters",
        Some(&csrf),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let kept: i64 =
        query_scalar("SELECT COUNT(*) FROM manual_timeline_markers WHERE asset_id='asset-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(kept, 2);

    // Only the explicit reset releases them, and it needs the CSRF token.
    let (status, _) = call("DELETE", "/api/v1/admin/assets/asset-1/markers", None, true).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        "DELETE",
        "/api/v1/admin/assets/asset-1/markers",
        Some(&csrf),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let kept: i64 =
        query_scalar("SELECT COUNT(*) FROM manual_timeline_markers WHERE asset_id='asset-1'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(kept, 0);
}
