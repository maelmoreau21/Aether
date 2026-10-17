use std::path::PathBuf;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Restore failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let backup_arg = args
        .next()
        .ok_or("usage: media-restore <BACKUP_PATH> [TARGET_PATH]")?;

    let backup_path = PathBuf::from(backup_arg);

    let media_root = std::env::var_os("MEDIA_LIBRARY_ROOT")
        .or_else(|| std::env::var_os("MEDIA_FIXTURE_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| "tests/fixtures/generated".into());

    let target_path = if let Some(target_arg) = args.next() {
        PathBuf::from(target_arg)
    } else {
        media_poc::identity::configured_path(&media_root)?
    };

    let summary =
        media_poc::backup::restore_database(&backup_path, &target_path, Some(&media_root)).await?;

    println!("Aether SQLite database restored successfully.");
    println!("  Backup:    {}", summary.backup_path);
    println!("  Target:    {}", summary.target_path);
    println!("  Size:      {} bytes", summary.file_size);
    println!("  Users:     {}", summary.user_count);
    println!("  Libraries: {}", summary.library_count);
    println!("  Works:     {}", summary.work_count);
    println!("The restored database is ready. You can safely start the Aether service.");

    Ok(())
}
