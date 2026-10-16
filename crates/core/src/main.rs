use media_poc::{auth::AuthState, node::Inventory, scans::ScanJobs, secure_app_with_jobs};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let library_root = std::env::var("MEDIA_LIBRARY_ROOT").ok();
    let root = PathBuf::from(library_root.clone().unwrap_or_else(|| {
        std::env::var("MEDIA_FIXTURE_DIR").unwrap_or_else(|_| "tests/fixtures/generated".into())
    }));
    let port: u16 = std::env::var("MEDIA_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()?;
    let db_path = media_poc::identity::configured_path(&root)?;
    let database = media_poc::identity::IdentityStore::open(&db_path).await?;
    let inventory = if library_root.is_some() {
        Inventory::local(root).await?
    } else {
        Inventory::load(root)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
    };
    let listener =
        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
    let port = listener.local_addr()?.port();
    let public_origin =
        std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let auth = AuthState::new(database.clone(), public_origin).await?;
    println!("Authenticated local Core: http://127.0.0.1:{port}/app/");
    let jobs = std::sync::Arc::new(ScanJobs::default());
    let shutdown_jobs = jobs.clone();
    axum::serve(
        listener,
        secure_app_with_jobs(inventory, port, auth, jobs).await?,
    )
    .with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        shutdown_jobs.shutdown().await;
    })
    .await?;
    database.close().await;
    Ok(())
}
