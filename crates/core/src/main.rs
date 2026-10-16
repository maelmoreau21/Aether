use media_poc::{auth::AuthState, node::Inventory, secure_app};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::var("MEDIA_FIXTURE_DIR").unwrap_or_else(|_| "tests/fixtures/generated".into()),
    );
    let port: u16 = std::env::var("MEDIA_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()?;
    let db_path = media_poc::identity::configured_path(&root)?;
    let database = media_poc::identity::IdentityStore::open(&db_path).await?;
    let inventory = Inventory::load(root).await?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
    let port = listener.local_addr()?.port();
    let public_origin =
        std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let auth = AuthState::new(database.clone(), public_origin).await?;
    println!("Authenticated local fixtures: http://127.0.0.1:{port}/app/ (P1) ; / (P0 bench)");
    axum::serve(listener, secure_app(inventory, port, auth))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    database.close().await;
    Ok(())
}
