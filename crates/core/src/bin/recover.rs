//! Offline recovery staging. Never connects to a database or applies configuration.
use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "usage: media-recover ARCHIVE NEW_OUTPUT_DIR\nOnly version 2 archives are supported. The output directory must not exist; its parent must exist. Archives are unencrypted and contain sensitive data. Stop Core and use media-restore separately.";

fn paths(mut args: impl Iterator<Item = OsString>) -> Result<(PathBuf, PathBuf), &'static str> {
    let source = args.next().ok_or(USAGE)?;
    let target = args.next().ok_or(USAGE)?;
    if args.next().is_some() || source.is_empty() || target.is_empty() {
        return Err(USAGE);
    }
    Ok((source.into(), target.into()))
}

fn next_steps(engine: &str) -> Result<&'static str, &'static str> {
    match engine {
        "sqlite" => Ok(
            "Engine: SQLite. Stop Core. Review config.json privately; adapt paths and service settings manually. Run media-restore with the extracted database.sqlite3 and a new target database path. Configure the service and verify it before reopening access.",
        ),
        "postgres" => Ok(
            "Engine: PostgreSQL. Stop Core. Review config.json privately; adapt connection and service settings manually. Set MEDIA_DB_ENGINE=postgres and MEDIA_DB_URL securely to an empty target database, then run media-restore with the extracted database.dump. Verify the service before reopening access.",
        ),
        _ => Err("Unsupported database engine in recovery manifest"),
    }
}

#[tokio::main]
async fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!("{USAGE}");
        return;
    }
    if let Err(error) = run().await {
        eprintln!("Recovery staging failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (source, target) = paths(std::env::args_os().skip(1))?;
    let summary = media_poc::backup_bundle::extract(&source, &target).await?;
    println!(
        "Recovery files extracted. Keep the output directory private: config.json contains sensitive settings."
    );
    println!("{}", next_steps(&summary.engine)?);
    if summary.assets > 0 {
        println!(
            "{} durable assets were verified (size and SHA-256) and extracted to assets/<sha256> with assets.json. They are not yet in the live store: after restoring the database, import them with AssetStore::restore_object and require AssetStore::verify_all to report no missing or corrupt object before declaring the restore successful.",
            summary.assets
        );
    } else {
        println!(
            "This archive contains no durable assets (database-only or pre-asset format). Avatars, posters and other stored images referenced by the database will be missing."
        );
    }
    println!(
        "If internal-ca.pem is present, remount it at the original path or set MEDIA_INTERNAL_CA_FILE to its restored path before starting Core. If postgres-ca.pem, postgres-cert.pem or postgres-key.pem are present, restore their original paths or update the TLS file path parameters in MEDIA_DB_URL. Other external file secrets must be restored separately."
    );
    println!("No live database or service environment was modified.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_exactly_two_nonempty_paths() {
        let (source, target) =
            paths([OsString::from("archive"), OsString::from("new directory")].into_iter())
                .unwrap();
        assert_eq!(source, PathBuf::from("archive"));
        assert_eq!(target, PathBuf::from("new directory"));
        assert!(paths(std::iter::empty()).is_err());
        assert!(paths([OsString::from("archive")].into_iter()).is_err());
        assert!(paths([OsString::from("archive"), OsString::new()].into_iter()).is_err());
        assert!(
            paths(
                [
                    OsString::from("a"),
                    OsString::from("b"),
                    OsString::from("--replace"),
                    OsString::from("x")
                ]
                .into_iter()
            )
            .is_err()
        );
    }

    #[test]
    fn recovery_instructions_match_engine_without_applying_configuration() {
        assert!(next_steps("sqlite").unwrap().contains("database.sqlite3"));
        assert!(next_steps("postgres").unwrap().contains("database.dump"));
        assert!(next_steps("postgres").unwrap().contains("empty target"));
        assert!(next_steps("unknown").is_err());
    }
}
