use media_poc::{auth::AuthState, node::Inventory, scans::ScanJobs, secure_app_with_jobs};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
};

fn parse_bind_addr() -> Result<Ipv4Addr, std::io::Error> {
    match std::env::var("MEDIA_BIND_ADDR") {
        Ok(raw) if !raw.trim().is_empty() => raw
            .trim()
            .parse::<Ipv4Addr>()
            .map_err(|e| std::io::Error::other(format!("invalid MEDIA_BIND_ADDR '{raw}': {e}"))),
        _ => Ok(Ipv4Addr::LOCALHOST),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let library_root = std::env::var("MEDIA_LIBRARY_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let root = match library_root {
        Some(path) => PathBuf::from(path),
        None => {
            let empty_local = std::env::temp_dir().join("aether-core-empty-local-root");
            tokio::fs::create_dir_all(&empty_local).await?;
            empty_local
        }
    };

    let port: u16 = std::env::var("MEDIA_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()?;
    let bind_addr = parse_bind_addr()?;
    let db_path = media_poc::identity::configured_path(&root)?;
    let database = media_poc::identity::IdentityStore::open(&db_path).await?;
    let inventory = Inventory::local(root).await?;

    let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(bind_addr, port)).await?;
    let port = listener.local_addr()?.port();
    let public_origin =
        std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let auth = AuthState::new(database.clone(), public_origin.clone()).await?;
    println!("aether-core listening on {public_origin}/app/");

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
