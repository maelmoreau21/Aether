use std::path::PathBuf;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Backup failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut allow_replace = false;
    let mut target_arg = None;

    for arg in std::env::args_os().skip(1) {
        if arg == "--replace" {
            allow_replace = true;
        } else if target_arg.is_none() {
            target_arg = Some(PathBuf::from(arg));
        } else {
            return Err("usage: media-backup [--replace] [TARGET_PATH]".into());
        }
    }

    let media_root = std::env::var_os("MEDIA_LIBRARY_ROOT")
        .or_else(|| std::env::var_os("MEDIA_FIXTURE_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| "tests/fixtures/generated".into());

    match std::env::var("MEDIA_DB_ENGINE").as_deref() {
        Ok("postgres") => {
            if allow_replace {
                return Err(
                    "PostgreSQL backups reject --replace; choose a new archive path".into(),
                );
            }
            let target = target_arg.ok_or("PostgreSQL backup requires an explicit TARGET_PATH")?;
            let url = std::env::var("MEDIA_DB_URL").map_err(|_| "MEDIA_DB_URL is required")?;
            let size = media_poc::postgres_backup::backup(&url, &target, Some(&media_root)).await?;
            println!("PostgreSQL custom archive completed successfully ({size} bytes).");
            return Ok(());
        }
        Ok("sqlite") | Err(std::env::VarError::NotPresent) => {}
        _ => return Err("MEDIA_DB_ENGINE must be sqlite or postgres".into()),
    }

    let db_path = media_poc::identity::configured_path(&media_root)?;

    let target_path = if let Some(path) = target_arg {
        path
    } else {
        let timestamp = chrono_lite_timestamp();
        let backup_dir = db_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("backups");
        backup_dir.join(format!("core_backup_{timestamp}.sqlite3"))
    };

    let options = media_poc::backup::BackupOptions { allow_replace };
    let summary = media_poc::backup::backup_database_with_options(
        &db_path,
        &target_path,
        Some(&media_root),
        options,
    )
    .await?;

    println!("Aether SQLite WAL-consistent backup completed successfully.");
    println!("  Source:    {}", summary.source_path);
    println!("  Backup:    {}", summary.backup_path);
    println!("  Size:      {} bytes", summary.file_size);
    println!("  Users:     {}", summary.user_count);
    println!("  Libraries: {}", summary.library_count);
    println!("  Works:     {}", summary.work_count);

    Ok(())
}

fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}
