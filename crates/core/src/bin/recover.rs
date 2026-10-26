//! Offline recovery staging. Never connects to a database or applies configuration.
use std::{ffi::OsString, path::PathBuf};

const USAGE: &str = "usage: media-recover ARCHIVE NEW_OUTPUT_DIR\nSet MEDIA_BACKUP_KEY (64 hexadecimal characters) or MEDIA_BACKUP_KEY_FILE; there is no default key. Archives made by old betas with the public all-zero key can only be opened with the explicit --legacy-public-key flag as a third argument (they never had confidentiality). The output directory must not exist; its parent must exist. This extracts sensitive recovery files only; stop Core and use media-restore separately.";

#[cfg(test)]
fn paths(mut args: impl Iterator<Item = OsString>) -> Result<(PathBuf, PathBuf), &'static str> {
    let (source, target, _) = paths_with_flag(&mut args)?;
    Ok((source, target))
}

fn paths_with_flag(
    args: &mut impl Iterator<Item = OsString>,
) -> Result<(PathBuf, PathBuf, bool), &'static str> {
    let source = args.next().ok_or(USAGE)?;
    let target = args.next().ok_or(USAGE)?;
    let legacy = match args.next() {
        None => false,
        Some(flag) if flag == "--legacy-public-key" => true,
        Some(_) => return Err(USAGE),
    };
    if args.next().is_some() || source.is_empty() || target.is_empty() {
        return Err(USAGE);
    }
    Ok((source.into(), target.into(), legacy))
}

fn validate_key(key: &str) -> Result<(), &'static str> {
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("MEDIA_BACKUP_KEY must contain exactly 64 hexadecimal characters");
    }
    Ok(())
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
    let (source, target, legacy) = paths_with_flag(&mut std::env::args_os().skip(1))?;
    let summary = if legacy {
        eprintln!(
            "WARNING: opening an archive protected by the publicly known legacy key. Its content was never confidential; rotate every secret it contains."
        );
        media_poc::backup_bundle::extract_legacy_public_key(&source, &target).await?
    } else {
        let key = media_poc::backup_bundle::resolve_key(
            std::env::var("MEDIA_BACKUP_KEY").ok().as_deref(),
            std::env::var_os("MEDIA_BACKUP_KEY_FILE")
                .map(PathBuf::from)
                .as_deref(),
            std::env::var_os("MEDIA_DATA_DIR")
                .map(PathBuf::from)
                .as_deref(),
            false,
        )?;
        validate_key(&key)?;
        media_poc::backup_bundle::extract(&source, &target, &key).await?
    };
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
        let (_, _, legacy) = paths_with_flag(
            &mut [
                OsString::from("a"),
                OsString::from("b"),
                OsString::from("--legacy-public-key"),
            ]
            .into_iter(),
        )
        .unwrap();
        assert!(legacy);
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
    fn key_validation_never_embeds_secret_in_error() {
        assert!(validate_key(&"aB19".repeat(16)).is_ok());
        assert!(validate_key(&"z".repeat(64)).is_err());
        assert!(validate_key(&"a".repeat(63)).is_err());
        let secret = "synthetic secret which must not appear";
        assert!(!validate_key(secret).unwrap_err().contains(secret));
    }

    #[test]
    fn recovery_instructions_match_engine_without_applying_configuration() {
        assert!(next_steps("sqlite").unwrap().contains("database.sqlite3"));
        assert!(next_steps("postgres").unwrap().contains("database.dump"));
        assert!(next_steps("postgres").unwrap().contains("empty target"));
        assert!(next_steps("unknown").is_err());
    }
}
