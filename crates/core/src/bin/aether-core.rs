//! Single Core entry point, shared by the Docker images (`aether-core`, with the
//! `aether` and `media-poc` aliases) and local development (`cargo run -p media-poc`).
//!
//! Library selection:
//! - `MEDIA_LIBRARY_ROOT` set: local library scanned from that folder;
//! - otherwise `MEDIA_FIXTURE_DIR` set: synthetic fixture inventory (tests, benchmarks);
//! - otherwise: empty local library in a temporary folder (production default).
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

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

enum LibrarySource {
    Local(PathBuf),
    Fixture(PathBuf),
}

async fn library_source() -> Result<LibrarySource, std::io::Error> {
    if let Some(path) = non_empty_env("MEDIA_LIBRARY_ROOT") {
        return Ok(LibrarySource::Local(PathBuf::from(path)));
    }
    if let Some(path) = non_empty_env("MEDIA_FIXTURE_DIR") {
        return Ok(LibrarySource::Fixture(PathBuf::from(path)));
    }
    let empty_local = std::env::temp_dir().join("aether-core-empty-local-root");
    tokio::fs::create_dir_all(&empty_local).await?;
    Ok(LibrarySource::Local(empty_local))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    if args
        .iter()
        .any(|a| a == "--version" || a == "-V" || a == "-v")
    {
        println!(
            "aether-core {}",
            media_contracts::version::current_version()
        );
        return Ok(());
    }

    let source = library_source().await?;
    let root = match &source {
        LibrarySource::Local(path) | LibrarySource::Fixture(path) => path.clone(),
    };
    let port: u16 = std::env::var("MEDIA_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse()?;
    let bind_addr = parse_bind_addr()?;
    let database = media_poc::identity::IdentityStore::open_configured(&root).await?;
    if let Some(directory) = std::env::var_os("MEDIA_EXTENSION_IMPORT_DIR") {
        let imported =
            media_poc::extensions::files::import(&database, std::path::Path::new(&directory))
                .await?;
        eprintln!("{{\"event\":\"local_extensions_imported\",\"count\":{imported}}}");
    }
    media_poc::extensions::bootstrap_essential_plugins(&database).await;
    match media_poc::extensions::storage::sync_installed(&database).await {
        Ok(count) => {
            eprintln!(
                "Plugin storage initialized at '{}' ({} plugin(s) verified/synced)",
                database.plugins_path().display(),
                count
            );
        }
        Err(error) => {
            eprintln!(
                "Plugin storage error at '{}' ({:?}): {}; affected plugins cannot execute",
                database.plugins_path().display(),
                error.kind(),
                error
            );
        }
    }
    let inventory = match source {
        LibrarySource::Local(path) => Inventory::local(path).await?,
        LibrarySource::Fixture(path) => Inventory::load(path)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    };

    let listener = tokio::net::TcpListener::bind(SocketAddrV4::new(bind_addr, port)).await?;
    let port = listener.local_addr()?.port();
    let public_origin =
        std::env::var("MEDIA_PUBLIC_URL").unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
    let auth = AuthState::new(database.clone(), public_origin.clone()).await?;
    println!("aether-core listening on {public_origin}/app/");
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
