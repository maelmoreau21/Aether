//! Second restore step: put the verified assets of an extracted backup into the configured store
//! and audit it against the restored database. Run with Core stopped, after `media-restore`.
//! Uses the normal configuration (`MEDIA_DB_*`, `MEDIA_ASSETS_PATH`, `MEDIA_LIBRARY_ROOT`); it never writes
//! to the media root and never deletes anything.
use std::path::PathBuf;

const USAGE: &str = "usage: media-restore-assets EXTRACTED_DIR\nEXTRACTED_DIR is the output of media-recover (it must contain assets.json and assets/). Configure the target database and MEDIA_ASSETS_PATH as for Core. Exit status 0 only if every asset row of the database has a verified object in the store.";

#[tokio::main]
async fn main() {
    if std::env::args_os()
        .nth(1)
        .is_none_or(|arg| arg == "--help" || arg == "-h")
    {
        println!("{USAGE}");
        return;
    }
    if let Err(error) = run().await {
        eprintln!("Asset restore failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let dir = PathBuf::from(args.next().ok_or(USAGE)?);
    if args.next().is_some() {
        return Err(USAGE.into());
    }
    let root = std::env::var_os("MEDIA_LIBRARY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("media"));
    let store = media_poc::identity::IdentityStore::open_configured(&root).await?;
    let report = store.assets().restore_extracted(&dir).await?;
    println!(
        "imported {}, already present {}, audited {} row(s): {} missing, {} corrupt",
        report.imported,
        report.already_present,
        report.audit.checked,
        report.audit.missing.len(),
        report.audit.corrupt.len()
    );
    if !report.is_complete() {
        return Err(
            "restore is INCOMPLETE: do not reopen access (see missing/corrupt counts)".into(),
        );
    }
    match media_poc::extension_storage::sync_installed(&store).await {
        Ok(count) => println!("Extensions synchronized to disk: {count}"),
        Err(error) => eprintln!("Warning: extensions could not be synchronized to disk: {error}"),
    }
    println!("Assets and extensions restored and verified.");
    Ok(())
}
