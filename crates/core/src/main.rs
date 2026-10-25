use media_poc::{auth::AuthState, node::Inventory, scans::ScanJobs, secure_app_with_jobs};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
};

fn parse_bind_addr_from(
    var: Result<String, std::env::VarError>,
) -> Result<Ipv4Addr, std::io::Error> {
    match var {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Ok(Ipv4Addr::LOCALHOST)
            } else {
                trimmed.parse::<Ipv4Addr>().map_err(|err| {
                    std::io::Error::other(format!("invalid MEDIA_BIND_ADDR '{raw}': {err}"))
                })
            }
        }
        Err(std::env::VarError::NotPresent) => Ok(Ipv4Addr::LOCALHOST),
        Err(std::env::VarError::NotUnicode(_)) => Err(std::io::Error::other(
            "MEDIA_BIND_ADDR is not valid unicode",
        )),
    }
}

fn parse_bind_addr() -> Result<Ipv4Addr, std::io::Error> {
    parse_bind_addr_from(std::env::var("MEDIA_BIND_ADDR"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args
        .iter()
        .any(|a| a == "--version" || a == "-V" || a == "-v")
    {
        println!("aether {}", media_contracts::version::current_version());
        return Ok(());
    }

    let library_root = std::env::var("MEDIA_LIBRARY_ROOT").ok();
    let root = PathBuf::from(library_root.clone().unwrap_or_else(|| {
        std::env::var("MEDIA_FIXTURE_DIR").unwrap_or_else(|_| "tests/fixtures/generated".into())
    }));
    let port: u16 = std::env::var("MEDIA_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()?;
    let bind_addr = parse_bind_addr()?;
    let database = media_poc::identity::IdentityStore::open_configured(&root).await?;
    if let Some(directory) = std::env::var_os("MEDIA_EXTENSION_IMPORT_DIR") {
        let imported =
            media_poc::extension_files::import(&database, std::path::Path::new(&directory)).await?;
        eprintln!("{{\"event\":\"local_extensions_imported\",\"count\":{imported}}}");
    }
    if let Err(error) = media_poc::extension_storage::sync_installed(&database).await {
        eprintln!(
            "plugin storage unavailable ({:?}): {}; affected plugins cannot execute",
            error.kind(),
            error
        );
    }
    let inventory = if library_root.is_some() {
        Inventory::local(root).await?
    } else {
        Inventory::load(root)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?
    };
    let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(bind_addr, port)).await?;
    let port = listener.local_addr()?.port();
    let public_origin =
        std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let auth = AuthState::new(database.clone(), public_origin.clone()).await?;
    println!("Authenticated local Core: {public_origin}/app/");
    let web_dist = media_poc::configured_web_dist();
    if !web_dist.exists() {
        eprintln!(
            "Warning: Static web assets not found at '{}'. /app routes will return 404.",
            web_dist.display()
        );
    }
    let jobs = std::sync::Arc::new(ScanJobs::default());
    let shutdown_jobs = jobs.clone();
    let scheduler_root = inventory.root().to_owned();
    let app = secure_app_with_jobs(inventory, port, auth.clone(), jobs.clone()).await?;
    let scheduler = media_poc::scheduler::Scheduler::start(auth, scheduler_root, jobs);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            drop(scheduler);
            shutdown_jobs.shutdown().await;
        })
        .await?;
    database.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bind_addr_default_when_unset_or_empty() {
        assert_eq!(
            parse_bind_addr_from(Err(std::env::VarError::NotPresent)).unwrap(),
            Ipv4Addr::LOCALHOST
        );
        assert_eq!(
            parse_bind_addr_from(Ok(String::new())).unwrap(),
            Ipv4Addr::LOCALHOST
        );
        assert_eq!(
            parse_bind_addr_from(Ok("   \t  ".into())).unwrap(),
            Ipv4Addr::LOCALHOST
        );
    }

    #[test]
    fn parse_bind_addr_valid_ipv4() {
        assert_eq!(
            parse_bind_addr_from(Ok("0.0.0.0".into())).unwrap(),
            Ipv4Addr::UNSPECIFIED
        );
        assert_eq!(
            parse_bind_addr_from(Ok("  0.0.0.0  ".into())).unwrap(),
            Ipv4Addr::UNSPECIFIED
        );
        assert_eq!(
            parse_bind_addr_from(Ok("127.0.0.1".into())).unwrap(),
            Ipv4Addr::LOCALHOST
        );
        assert_eq!(
            parse_bind_addr_from(Ok("192.168.1.50".into())).unwrap(),
            Ipv4Addr::new(192, 168, 1, 50)
        );
    }

    #[test]
    fn parse_bind_addr_invalid_ipv4_returns_error() {
        let err = parse_bind_addr_from(Ok("invalid-ip".into())).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid MEDIA_BIND_ADDR 'invalid-ip'")
        );
        let err = parse_bind_addr_from(Ok("256.0.0.1".into())).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid MEDIA_BIND_ADDR '256.0.0.1'")
        );
        let err = parse_bind_addr_from(Ok("::1".into())).unwrap_err();
        assert!(err.to_string().contains("invalid MEDIA_BIND_ADDR '::1'"));
        let err = parse_bind_addr_from(Ok("0.0.0.0:3000".into())).unwrap_err();
        assert!(
            err.to_string()
                .contains("invalid MEDIA_BIND_ADDR '0.0.0.0:3000'")
        );
    }
}
