//! SQLite backup and restore operations for Aether standalone.
//!
//! Provides WAL-consistent snapshots via SQLite `VACUUM INTO` and safe,
//! atomic restoration into fresh or existing Docker volumes.
use crate::identity::IdentityStore;
use sqlx::{
    Connection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("backup integrity check failed: {0}")]
    IntegrityCheck(String),
    #[error("database is locked: {0}")]
    Locked(String),
    #[error("corrupt backup: missing table '{0}'")]
    MissingTable(String),
    #[error("identity store error: {0}")]
    Identity(#[from] crate::identity::IdentityError),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackupSummary {
    pub source_path: String,
    pub backup_path: String,
    pub file_size: u64,
    pub user_count: i64,
    pub library_count: i64,
    pub work_count: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RestoreSummary {
    pub backup_path: String,
    pub target_path: String,
    pub file_size: u64,
    pub user_count: i64,
    pub library_count: i64,
    pub work_count: i64,
}

/// Create a WAL-consistent point-in-time backup using SQLite's `VACUUM INTO`.
///
/// This does not disrupt concurrent readers or writers and folds all WAL changes
/// into a single, clean, defragmented standalone SQLite database file.
pub async fn backup_database(
    source_db_path: &Path,
    target_backup_path: &Path,
    media_root: Option<&Path>,
) -> Result<BackupSummary, BackupError> {
    if !source_db_path.exists() {
        return Err(BackupError::InvalidPath(format!(
            "source database '{}' does not exist",
            source_db_path.display()
        )));
    }

    let source_canonical = source_db_path
        .canonicalize()
        .map_err(|e| BackupError::InvalidPath(format!("cannot resolve source path: {e}")))?;

    // Ensure target backup path is not inside media root
    if let Some(root) = media_root
        && let Ok(root_canon) = root.canonicalize()
        && target_backup_path.starts_with(&root_canon)
    {
        return Err(BackupError::InvalidPath(
            "backup file must not be placed inside the media directory".into(),
        ));
    }

    if let Ok(target_canon) = target_backup_path.canonicalize()
        && target_canon == source_canonical
    {
        return Err(BackupError::InvalidPath(
            "target backup path cannot be identical to source database".into(),
        ));
    }

    // Ensure target parent directory exists with secure permissions (0750)
    if let Some(parent) = target_backup_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o750));
        }
    }

    // SQLite `VACUUM INTO` requires that the destination file does not already exist.
    // If it exists, remove it first so the backup can overwrite cleanly.
    if target_backup_path.exists() {
        std::fs::remove_file(target_backup_path)?;
    }

    let target_str = target_backup_path.to_str().ok_or_else(|| {
        BackupError::InvalidPath("target backup path is not valid unicode".into())
    })?;

    // Connect to source database with WAL mode and 15s timeout
    let options = SqliteConnectOptions::new()
        .filename(&source_canonical)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(15));

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    // Escape single quotes for the SQLite string literal
    let escaped_path = target_str.replace('\'', "''");
    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new("VACUUM INTO ");
    builder.push(format!("'{}'", escaped_path));
    builder.build().execute(&pool).await?;
    pool.close().await;

    // Enforce strict file permissions (0600) on the created backup file
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            std::fs::set_permissions(target_backup_path, std::fs::Permissions::from_mode(0o600));
    }

    // Verify backup file integrity
    let verify_options = SqliteConnectOptions::new()
        .filename(target_backup_path)
        .read_only(true);

    let verify_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(verify_options)
        .await?;

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&verify_pool)
        .await?;

    if integrity != "ok" {
        return Err(BackupError::IntegrityCheck(integrity));
    }

    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&verify_pool)
        .await
        .unwrap_or(0);

    let library_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM libraries")
        .fetch_one(&verify_pool)
        .await
        .unwrap_or(0);

    let work_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inventory_works")
        .fetch_one(&verify_pool)
        .await
        .unwrap_or(0);

    verify_pool.close().await;

    let meta = std::fs::metadata(target_backup_path)?;

    Ok(BackupSummary {
        source_path: source_canonical.to_string_lossy().to_string(),
        backup_path: target_str.to_string(),
        file_size: meta.len(),
        user_count,
        library_count,
        work_count,
    })
}

/// Restore a database from a validated backup file into the target database path.
///
/// Ensures point-in-time integrity, removes any stale `-wal` and `-shm` journals,
/// applies strict 0600 permissions, and runs idempotent migrations.
pub async fn restore_database(
    backup_path: &Path,
    target_db_path: &Path,
    media_root: Option<&Path>,
) -> Result<RestoreSummary, BackupError> {
    if !backup_path.exists() {
        return Err(BackupError::InvalidPath(format!(
            "backup file '{}' does not exist",
            backup_path.display()
        )));
    }

    if !backup_path.is_file() {
        return Err(BackupError::InvalidPath(format!(
            "backup path '{}' is not a regular file",
            backup_path.display()
        )));
    }

    // Validate backup file integrity and structure before any restoration attempt
    let verify_options = SqliteConnectOptions::new()
        .filename(backup_path)
        .read_only(true);

    let verify_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(verify_options)
        .await?;

    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&verify_pool)
        .await?;

    if integrity != "ok" {
        return Err(BackupError::IntegrityCheck(format!(
            "backup integrity check failed: {integrity}"
        )));
    }

    // Verify required schema tables exist in the backup
    for table in &["users", "local_credentials", "libraries", "inventory_works"] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        )
        .bind(table)
        .fetch_one(&verify_pool)
        .await?;

        if !exists {
            return Err(BackupError::MissingTable((*table).into()));
        }
    }

    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&verify_pool)
        .await
        .unwrap_or(0);

    let library_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM libraries")
        .fetch_one(&verify_pool)
        .await
        .unwrap_or(0);

    let work_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inventory_works")
        .fetch_one(&verify_pool)
        .await
        .unwrap_or(0);

    verify_pool.close().await;

    // Check media root constraint on target
    if let Some(root) = media_root
        && let Ok(root_canon) = root.canonicalize()
        && target_db_path.starts_with(&root_canon)
    {
        return Err(BackupError::InvalidPath(
            "target database must not be placed inside the media directory".into(),
        ));
    }

    // Check if target database currently exists and is locked by a running process
    if target_db_path.exists() {
        let test_options = SqliteConnectOptions::new()
            .filename(target_db_path)
            .busy_timeout(Duration::from_millis(500));

        if let Ok(pool) = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(test_options)
            .await
        {
            let test_lock = async {
                let mut conn = pool.acquire().await?;
                let tx = conn.begin_with("BEGIN IMMEDIATE").await?;
                tx.rollback().await?;
                Ok::<(), sqlx::Error>(())
            }
            .await;

            pool.close().await;

            if let Err(err) = test_lock {
                return Err(BackupError::Locked(format!(
                    "target database is currently in use or locked ({err}); stop the Aether service before restoring"
                )));
            }
        }
    }

    // Ensure target parent directory exists with 0750 permissions
    if let Some(parent) = target_db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o750));
        }
    }

    // Staging file in target directory for atomic replacement
    let staging_path = target_db_path.with_extension("sqlite3.restoring");
    if staging_path.exists() {
        std::fs::remove_file(&staging_path)?;
    }

    std::fs::copy(backup_path, &staging_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600));
    }

    // Clean up any stale WAL or SHM journals that belonged to the previous database
    let wal_path = PathBuf::from(format!("{}-wal", target_db_path.display()));
    let shm_path = PathBuf::from(format!("{}-shm", target_db_path.display()));
    if wal_path.exists() {
        let _ = std::fs::remove_file(&wal_path);
    }
    if shm_path.exists() {
        let _ = std::fs::remove_file(&shm_path);
    }

    // Atomic rename of staging file to target database path
    std::fs::rename(&staging_path, target_db_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target_db_path, std::fs::Permissions::from_mode(0o600));
    }

    // Open target with IdentityStore to apply any missing migrations idempotently
    let store = IdentityStore::open(target_db_path).await?;
    store.close().await;

    let meta = std::fs::metadata(target_db_path)?;

    Ok(RestoreSummary {
        backup_path: backup_path.to_string_lossy().to_string(),
        target_path: target_db_path.to_string_lossy().to_string(),
        file_size: meta.len(),
        user_count,
        library_count,
        work_count,
    })
}
