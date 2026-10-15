use media_poc::{app, node::Inventory};
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
    let inventory = Inventory::load(root).await?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
    let port = listener.local_addr()?.port();
    println!("Local P0 bench: http://127.0.0.1:{port} (synthetic fixtures only)");
    axum::serve(listener, app(inventory, port))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
