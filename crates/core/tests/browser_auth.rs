//! Explicit opt-in browser evidence against an isolated, disposable SQLite database.
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory, secure_app};
use zeroize::Zeroizing;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires built Web assets, generated fixtures and installed Chrome/Playwright"]
async fn browser_login_catalog_player_logout() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("browser.sqlite3"))
        .await
        .unwrap();
    // Synthetic disposable credential only; never an application default account.
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
    let inventory = Inventory::load(std::path::PathBuf::from("../../tests/fixtures/generated"))
        .await
        .unwrap();
    // Static file service resolves from repository root.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    // Run this test alone: ServeDir uses the process working directory.
    std::env::set_current_dir(&root).unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, secure_app(inventory, port, auth))
            .await
            .unwrap()
    });
    let result = tokio::process::Command::new("node")
        .arg("scripts/browser-auth.cjs")
        .env("MEDIA_TEST_ORIGIN", &origin)
        .current_dir(&root)
        .status()
        .await
        .unwrap();
    server.abort();
    store.close().await;
    assert!(result.success(), "browser auth scenario failed");
}
