//! Focused real-TCP chapter preview test: signed direct Node, then outbound relay.
use axum::http::{Method, StatusCode, header};
use media_node::server::{NodeServerConfig, NodeServerState};
use media_poc::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::IdentityStore,
    node::Inventory,
    scans::ScanJobs,
    secure_app_with_jobs,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::net::TcpListener;

async fn call(
    client: &reqwest::Client,
    origin: &str,
    method: Method,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Option<Value>,
) -> reqwest::Response {
    let mut request = client
        .request(method, format!("{origin}{path}"))
        .header(header::ORIGIN, origin);
    if !cookie.is_empty() {
        request = request.header(header::COOKIE, cookie);
    }
    if !csrf.is_empty() {
        request = request
            .header(header::ORIGIN, origin)
            .header("x-csrf-token", csrf);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    request.send().await.expect("local TCP request")
}

#[tokio::test]
async fn chapter_preview_direct_and_outbound_preserve_head_range_and_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let base = tokio::fs::canonicalize(temp.path()).await.unwrap();
    let root = base.join("node-media");
    let empty = base.join("core-empty");
    tokio::fs::create_dir_all(&root).await.unwrap();
    tokio::fs::create_dir_all(&empty).await.unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/av1-opus.mp4");
    assert!(fixture.exists(), "generate AV1 fixture first");
    let metadata = base.join("chapters.ffmetadata");
    tokio::fs::write(&metadata, ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=2000\ntitle=Opening\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=2000\nEND=4000\ntitle=Second\n").await.unwrap();
    let media = root.join("Chapter Test (2026).mkv");
    let status = tokio::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(&fixture)
        .args(["-i"])
        .arg(&metadata)
        .args([
            "-map",
            "0",
            "-map_metadata",
            "1",
            "-map_chapters",
            "1",
            "-c",
            "copy",
        ])
        .arg(&media)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let original = tokio::fs::read(&media).await.unwrap();
    let original_permissions = tokio::fs::metadata(&media).await.unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    tokio::fs::set_permissions(&media, permissions)
        .await
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://127.0.0.1:{port}");
    let store = IdentityStore::open(&base.join("core.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "chapter-admin",
            zeroize::Zeroizing::new("synthetic-chapter-password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let app = secure_app_with_jobs(
        Inventory::local(empty).await.unwrap(),
        port,
        auth,
        Arc::new(ScanJobs::default()),
    )
    .await
    .unwrap();
    let core_task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let login = call(
        &client,
        &origin,
        Method::POST,
        "/api/v1/auth/login",
        "",
        "",
        Some(json!({"login":"chapter-admin","password":"synthetic-chapter-password"})),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let login: Value = login.json().await.unwrap();
    let csrf = login["csrfToken"].as_str().unwrap();

    let node_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let node_addr = node_listener.local_addr().unwrap();
    let node_url = format!("http://{node_addr}");
    let mut node = NodeServerState::initialize(NodeServerConfig {
        bind: node_addr,
        data_dir: base.join("node-data"),
        allowed_roots: vec![root.clone()],
        bootstrap_token: Some("chapter-pairing-fixture".into()),
        configured_node_id: None,
        configured_name: Some("Chapter Node".into()),
        public_url: Some(node_url.clone()),
        batch_size: 16,
    })
    .await
    .unwrap();
    node.trickplay = Arc::new(media_node::trickplay::TrickplayManager::new(
        base.join("node-cache"),
        16 * 1024 * 1024,
        "ffmpeg".into(),
        true,
        None,
    ));
    let node_app = media_node::server::app_with_state(node.clone());
    let node_task = tokio::spawn(async move {
        axum::serve(node_listener, node_app).await.unwrap();
    });
    let paired = call(&client, &origin, Method::POST, "/api/v1/admin/nodes/pair", &cookie, csrf,
        Some(json!({"name":"Chapter Node","baseUrl":node_url,"publicUrl":node_url,"pairingToken":"chapter-pairing-fixture"}))).await;
    assert_eq!(paired.status(), StatusCode::CREATED);
    let paired: Value = paired.json().await.unwrap();
    let node_id = paired["id"].as_str().unwrap();
    let library = call(&client, &origin, Method::POST, "/api/v1/admin/libraries", &cookie, csrf,
        Some(json!({"name":"Chapters","kind":"films","nodeId":node_id,"root":root,"enabled":true,"userIds":[]}))).await;
    assert_eq!(library.status(), StatusCode::CREATED);
    let library: Value = library.json().await.unwrap();
    let library_id = library["id"].as_str().unwrap();
    query("UPDATE libraries SET options_json=? WHERE id=?")
        .bind(
            json!({"chapterImagesEnabled":true,"trickplayEnabled":true,"trickplayDuringScan":false,"trickplayIntervalSeconds":2,"trickplayWidth":410,"trickplayQuality":12})
                .to_string(),
        )
        .bind(library_id)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(
            &client,
            &origin,
            Method::POST,
            &format!("/api/v1/admin/libraries/{library_id}/scan"),
            &cookie,
            csrf,
            None
        )
        .await
        .status(),
        StatusCode::ACCEPTED
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let response = call(
            &client,
            &origin,
            Method::GET,
            &format!("/api/v1/admin/libraries/{library_id}/scan"),
            &cookie,
            "",
            None,
        )
        .await;
        let status: Value = response.json().await.unwrap();
        if status["status"] == "completed" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Node scan did not finish: {status}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let asset: String =
        query_scalar("SELECT id FROM inventory_assets WHERE library_id=? AND available=1")
            .bind(library_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    let path = format!("/api/v1/assets/{asset}/chapter-images");
    let manifest = call(&client, &origin, Method::GET, &path, &cookie, "", None).await;
    assert_eq!(manifest.status(), StatusCode::OK);
    let manifest: Value = manifest.json().await.unwrap();
    assert_eq!(manifest["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["chapters"][1]["start"], 2.0);
    let image_path = format!("{path}/1");
    let unsigned = client.get(format!("{node_url}/internal/v1/node/assets/{asset}/chapter-images/1?time=2&duration=4&width=320&quality=7")).send().await.unwrap();
    assert_eq!(unsigned.status(), StatusCode::UNAUTHORIZED);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let jpeg = loop {
        let response = call(
            &client,
            &origin,
            Method::GET,
            &image_path,
            &cookie,
            "",
            None,
        )
        .await;
        if response.status() == StatusCode::OK {
            break response.bytes().await.unwrap();
        }
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert!(
            tokio::time::Instant::now() < deadline,
            "chapter generation timed out"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(image::load_from_memory(&jpeg).unwrap().width(), 410);
    let preview_path = format!("/api/v1/assets/{asset}/trickplay");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let response = call(
            &client,
            &origin,
            Method::GET,
            &preview_path,
            &cookie,
            "",
            None,
        )
        .await;
        assert!(matches!(
            response.status(),
            StatusCode::OK | StatusCode::ACCEPTED
        ));
        let manifest: Value = response.json().await.unwrap();
        if manifest["status"] == "ready" {
            assert_eq!(manifest["width"], 410);
            assert_eq!(manifest["intervalMs"], 2000);
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "custom preview generation timed out: {manifest}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let preview_tile = format!("{preview_path}/0");
    let direct = call(
        &client,
        &origin,
        Method::GET,
        &preview_tile,
        &cookie,
        "",
        None,
    )
    .await;
    assert_eq!(direct.status(), StatusCode::OK);
    let preview_bytes = direct.bytes().await.unwrap();
    assert!(image::load_from_memory(&preview_bytes).unwrap().width() >= 410);

    for outbound in [false, true] {
        let channel = if outbound {
            query("UPDATE media_nodes SET base_url=NULL WHERE id=?")
                .bind(node_id)
                .execute(store.pool())
                .await
                .unwrap();
            let refreshed = media_poc::nodes::refresh_node_in_registry(&store, node_id)
                .await
                .unwrap()
                .unwrap();
            assert!(refreshed.base_url.is_none(), "disable any direct fallback");
            Some(media_node::server::start_channel_worker(node.clone()))
        } else {
            None
        };
        if outbound {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            while !media_poc::nodes::ACTIVE_CHANNELS
                .read()
                .await
                .contains_key(node_id)
            {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "outbound channel not connected"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        let response = call(
            &client,
            &origin,
            Method::GET,
            &preview_path,
            &cookie,
            "",
            None,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "preview manifest outbound={outbound}"
        );
        let preview: Value = response.json().await.unwrap();
        assert_eq!(preview["status"], "ready");
        assert_eq!(preview["width"], 410);
        assert_eq!(preview["intervalMs"], 2000);
        let response = call(
            &client,
            &origin,
            Method::GET,
            &preview_tile,
            &cookie,
            "",
            None,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "preview tile outbound={outbound}"
        );
        assert_eq!(response.bytes().await.unwrap(), preview_bytes);
        let response = call(
            &client,
            &origin,
            Method::GET,
            &image_path,
            &cookie,
            "",
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "GET outbound={outbound}");
        assert_eq!(response.bytes().await.unwrap(), jpeg);
        let response = call(
            &client,
            &origin,
            Method::HEAD,
            &image_path,
            &cookie,
            "",
            None,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "HEAD outbound={outbound}"
        );
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH].to_str().unwrap(),
            jpeg.len().to_string()
        );
        assert!(response.bytes().await.unwrap().is_empty());
        let response = client
            .get(format!("{origin}{image_path}"))
            .header(header::COOKIE, &cookie)
            .header(header::RANGE, "bytes=0-15")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(&response.bytes().await.unwrap()[..], &jpeg[..16]);
        let response = client
            .head(format!("{origin}{image_path}"))
            .header(header::COOKIE, &cookie)
            .header(header::RANGE, "bytes=0-15")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "16");
        assert!(response.bytes().await.unwrap().is_empty());
        if let Some(channel) = channel {
            channel.abort();
        }
    }
    assert_ne!(
        call(&client, &origin, Method::GET, &image_path, "", "", None)
            .await
            .status(),
        StatusCode::OK
    );
    query("UPDATE libraries SET enabled=0 WHERE id=?")
        .bind(library_id)
        .execute(store.pool())
        .await
        .unwrap();
    assert_ne!(
        call(
            &client,
            &origin,
            Method::GET,
            &image_path,
            &cookie,
            "",
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(tokio::fs::read(&media).await.unwrap(), original);
    let mut entries = tokio::fs::read_dir(&root).await.unwrap();
    assert_eq!(entries.next_entry().await.unwrap().unwrap().path(), media);
    assert!(
        entries.next_entry().await.unwrap().is_none(),
        "no cache beside media"
    );
    assert!(base.join("node-cache").exists());
    node_task.abort();
    core_task.abort();
    tokio::fs::set_permissions(&media, original_permissions)
        .await
        .unwrap();
}
