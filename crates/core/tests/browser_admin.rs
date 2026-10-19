//! Real Core/SQLite UI evidence; no personal media and no API interception.
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory, scans::ScanJobs};
use std::sync::Arc;
use zeroize::Zeroizing;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires built Web assets and installed Chrome/Playwright"]
async fn browser_administration_real_api() {
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
        .arg("scripts/test-admin-live.mjs")
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
