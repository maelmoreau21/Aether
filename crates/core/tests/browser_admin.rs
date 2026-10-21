//! Real Core/SQLite UI evidence; no personal media and no API interception.
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory, scans::ScanJobs};
use std::sync::Arc;
use zeroize::Zeroizing;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires built Web assets and installed Chrome/Playwright"]
async fn browser_administration_real_api() {
    run_browser("scripts/test-admin-live.mjs", false).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires built Web assets and installed Chrome/Playwright"]
async fn browser_library_options_and_media_policy() {
    run_browser("scripts/test-library-options-live.mjs", true).await;
}

async fn run_browser(script: &str, seed: bool) {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap(
            "browser-admin",
            Zeroizing::new("disposable browser fixture password".into()),
        )
        .await
        .unwrap();
    if seed {
        use media_poc::database::query;
        let root_string = media.to_string_lossy().to_string();
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('collage','Mosaïque de test','films','local',?,1)").bind(&root_string).execute(store.pool()).await.unwrap();
        for index in 0..4 {
            let id = format!("cover-{index}");
            query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json) VALUES(?,'collage',?,?,?,'film','{}')").bind(&id).bind(&id).bind(&id).bind(&id).execute(store.pool()).await.unwrap();
            query("INSERT INTO inventory_editions(id,work_id) VALUES(?,?)")
                .bind(&id)
                .bind(&id)
                .execute(store.pool())
                .await
                .unwrap();
            query("INSERT INTO inventory_versions(id,edition_id) VALUES(?,?)")
                .bind(&id)
                .bind(&id)
                .execute(store.pool())
                .await
                .unwrap();
            query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,'collage',?)").bind(&id).bind(&id).bind(&root_string).execute(store.pool()).await.unwrap();
            query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,?,'collage',?,1,'0','video/mp4','{}',1)").bind(&id).bind(&id).bind(format!("{id}.mp4")).execute(store.pool()).await.unwrap();
            let mut bytes = std::io::Cursor::new(vec![]);
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                100,
                150,
                image::Rgb([40 + index * 40, 100, 160]),
            ))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
            query("INSERT INTO work_posters(work_id,image_data) VALUES(?,?)")
                .bind(&id)
                .bind(bytes.into_inner())
                .execute(store.pool())
                .await
                .unwrap();
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://127.0.0.1:{port}");
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    std::env::set_current_dir(&root).unwrap();
    let jobs = Arc::new(ScanJobs::default());
    let app = media_poc::secure_app_with_jobs(
        Inventory::local(media.clone()).await.unwrap(),
        port,
        auth,
        jobs.clone(),
    )
    .await
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let status = tokio::process::Command::new("node")
        .arg(script)
        .env("MEDIA_TEST_ORIGIN", origin)
        .env("MEDIA_TEST_ROOT", media)
        .current_dir(root.join("apps/web"))
        .status()
        .await
        .unwrap();
    jobs.shutdown().await;
    server.abort();
    store.close().await;
    assert!(
        status.success(),
        "real administration browser scenario failed"
    );
}
