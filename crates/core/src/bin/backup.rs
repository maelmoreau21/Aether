use std::path::PathBuf;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Backup failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let media_root = std::env::var_os("MEDIA_LIBRARY_ROOT")
        .or_else(|| std::env::var_os("MEDIA_FIXTURE_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| "tests/fixtures/generated".into());

    let db_path = media_poc::identity::configured_path(&media_root)?;

    let target_path = if let Some(arg) = std::env::args_os().nth(1) {
        PathBuf::from(arg)
    } else {
        let timestamp = chrono_lite_timestamp();
        let backup_dir = db_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("backups");
        backup_dir.join(format!("core_backup_{timestamp}.sqlite3"))
    };

    let summary =
        media_poc::backup::backup_database(&db_path, &target_path, Some(&media_root)).await?;

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
