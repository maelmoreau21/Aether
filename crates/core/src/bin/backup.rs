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
    let mut full = false;
    let mut target_arg = None;

    for arg in std::env::args_os().skip(1) {
        if arg == "--full" {
            full = true;
        } else if arg == "--replace" {
            allow_replace = true;
        } else if target_arg.is_none() {
            target_arg = Some(PathBuf::from(arg));
        } else {
            return Err("usage: media-backup [--full | --replace] [TARGET_PATH]".into());
        }
    }

    let media_root = std::env::var_os("MEDIA_LIBRARY_ROOT")
        .or_else(|| std::env::var_os("MEDIA_FIXTURE_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| "tests/fixtures/generated".into());

    if full {
        if allow_replace {
            return Err("full backups never overwrite an archive".into());
        }
        let target = target_arg.ok_or("full backup requires a new archive path")?;
        full_backup(&target, &media_root).await?;
        println!("Server backup completed: database and runtime configuration.");
        return Ok(());
    }

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

/// Explicit full-state CLI; old database-only commands remain available for operators.
async fn full_backup(
    target: &std::path::Path,
    media_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // No fixed fallback key: explicit key, key file, or the installation key created by Core.
    let data_dir = match media_poc::identity::configured_path(media_root) {
        Ok(db) => db.parent().map(|p| p.to_owned()),
        Err(_) => None,
    }
    .or_else(|| std::env::var_os("MEDIA_DATA_DIR").map(PathBuf::from));
    let key = media_poc::backup_bundle::resolve_key(
        std::env::var("MEDIA_BACKUP_KEY").ok().as_deref(),
        std::env::var_os("MEDIA_BACKUP_KEY_FILE")
            .map(PathBuf::from)
            .as_deref(),
        data_dir.as_deref(),
        false,
    )?;
    media_poc::backup::check_path_safety(target, "full backup")?;
    media_poc::backup::check_not_in_media_root(target, Some(media_root), "full backup")?;
    if target.exists() {
        return Err("backup target already exists".into());
    }
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let stage = parent.join(format!(".media-full-{}", uuid::Uuid::new_v4()));
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let mut builder = builder;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&stage)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("database.snapshot"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let cleanup = Cleanup(stage);
    let snapshot = cleanup.0.join("database.snapshot");
    let mut config: std::collections::BTreeMap<String, String> = std::env::vars()
        .filter(|(name, _)| {
            (name.starts_with("MEDIA_") || name.starts_with("OIDC_")) && name != "MEDIA_BACKUP_KEY"
        })
        .collect();
    let engine = std::env::var("MEDIA_DB_ENGINE").unwrap_or_else(|_| "sqlite".into());
    match engine.as_str() {
        "sqlite" => {
            let source = media_poc::identity::configured_path(media_root)?;
            config.insert("MEDIA_DB_PATH".into(), source.to_string_lossy().into());
            media_poc::backup::backup_database(&source, &snapshot, Some(media_root)).await?;
        }
        "postgres" => {
            let url = zeroize::Zeroizing::new(
                std::env::var("MEDIA_DB_URL").map_err(|_| "MEDIA_DB_URL is required")?,
            );
            media_poc::postgres_backup::backup(&url, &snapshot, Some(media_root)).await?;
        }
        _ => return Err("MEDIA_DB_ENGINE must be sqlite or postgres".into()),
    }
    config.insert("MEDIA_DB_ENGINE".into(), engine.clone());
    media_poc::backup_bundle::create(&snapshot, config, target, &key, &engine).await?;
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
