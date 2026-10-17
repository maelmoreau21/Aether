//! SQLite backup and restore operations for Aether standalone.
//!
//! Provides WAL-consistent snapshots via SQLite `VACUUM INTO` and safe,
//! atomic restoration into fresh or existing Docker volumes.
use crate::identity::IdentityStore;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::{
    path::{Component, Path, PathBuf},
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
    #[error("target already exists: {0}")]
    TargetExists(String),
    #[error("residual SQLite journal files found: {0}")]
    ResidualJournals(String),
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

/// Options for SQLite database backup.
#[derive(Debug, Clone, Copy, Default)]
pub struct BackupOptions {
    /// Whether to replace an existing target backup file.
    ///
    /// By default false: refusing to overwrite an existing backup ensures that
    /// no previous valid backup can ever be lost due to an error.
    pub allow_replace: bool,
}

/// RAII guard to clean up a temporary file unless disarmed after successful publication.
struct TempFileGuard {
    path: PathBuf,
    active: bool,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, active: true }
    }

    fn disarm(mut self) {
        self.active = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Check if metadata represents a symlink or Windows reparse point (junction).
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Verify that a path does not contain parent directory traversal ('..') and
/// neither the path itself nor its existing parent is a symlink or reparse point.
fn check_path_safety(path: &Path, label: &str) -> Result<(), BackupError> {
    if path.as_os_str().is_empty() {
        return Err(BackupError::InvalidPath(format!(
            "{label} path cannot be empty"
        )));
    }

    if path.components().any(|c| c == Component::ParentDir) {
        return Err(BackupError::InvalidPath(format!(
            "{label} path '{}' must not contain '..' components",
            path.display()
        )));
    }

    if let Ok(meta) = std::fs::symlink_metadata(path)
        && is_symlink_or_reparse(&meta)
    {
        return Err(BackupError::InvalidPath(format!(
            "{label} path '{}' is a symlink or reparse point, which is forbidden",
            path.display()
        )));
    }

    if let Some(parent) = path.parent()
        && parent.exists()
        && let Ok(meta) = std::fs::symlink_metadata(parent)
        && is_symlink_or_reparse(&meta)
    {
        return Err(BackupError::InvalidPath(format!(
            "parent directory '{}' of {label} is a symlink or reparse point, which is forbidden",
            parent.display()
        )));
    }

    Ok(())
}

/// Ensure a path is not placed inside the media directory (which must stay read-only).
fn check_not_in_media_root(
    path: &Path,
    media_root: Option<&Path>,
    label: &str,
) -> Result<(), BackupError> {
    if let Some(root) = media_root
        && let Ok(root_canon) = root.canonicalize()
    {
        if path.starts_with(&root_canon) {
            return Err(BackupError::InvalidPath(format!(
                "{label} must not be placed inside the media directory",
            )));
        }
        if let Some(parent) = path.parent()
            && let Ok(parent_canon) = parent.canonicalize()
            && parent_canon.starts_with(&root_canon)
        {
            return Err(BackupError::InvalidPath(format!(
                "{label} must not be placed inside the media directory",
            )));
        }
    }
    Ok(())
}

/// Synchronize file bytes to disk for durability across platforms.
///
/// On Windows, FlushFileBuffers requires GENERIC_WRITE permissions.
fn sync_file(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let f = std::fs::File::open(path)?;
        f.sync_all()?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let f = std::fs::OpenOptions::new().write(true).open(path)?;
        f.sync_all()?;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let f = std::fs::OpenOptions::new().write(true).open(path)?;
        f.sync_all()?;
        Ok(())
    }
}

/// Safely publish a validated temporary file to its target destination path.
///
/// On Unix, `rename` is atomic and replaces the destination if it exists.
/// On Windows, `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` is used,
/// after ensuring any existing destination is not marked read-only.
fn publish_file(temp_path: &Path, target_path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::rename(temp_path, target_path)?;
        if let Some(parent) = target_path.parent()
            && let Ok(dir) = std::fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        if target_path.exists()
            && let Ok(meta) = std::fs::metadata(target_path)
        {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            if perms.readonly() {
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(target_path, perms);
            }
        }
        std::fs::rename(temp_path, target_path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::rename(temp_path, target_path)
    }
}

/// Create a WAL-consistent point-in-time backup using SQLite's `VACUUM INTO`.
///
/// This does not disrupt concurrent readers or writers and folds all WAL changes
/// into a single, clean, defragmented standalone SQLite database file.
///
/// Refuses to overwrite an existing backup by default.
pub async fn backup_database(
    source_db_path: &Path,
    target_backup_path: &Path,
    media_root: Option<&Path>,
) -> Result<BackupSummary, BackupError> {
    backup_database_with_options(
        source_db_path,
        target_backup_path,
        media_root,
        BackupOptions::default(),
    )
    .await
}

/// Create a WAL-consistent point-in-time backup with configurable options.
///
/// Never destroys an existing backup file prior to producing and validating
/// the replacement. Employs a unique temporary file in the same directory,
/// runs VACUUM INTO, performs an integrity and schema check, sets 0600 permissions,
/// syncs to disk, and atomically publishes the new file.
pub async fn backup_database_with_options(
    source_db_path: &Path,
    target_backup_path: &Path,
    media_root: Option<&Path>,
    options: BackupOptions,
) -> Result<BackupSummary, BackupError> {
    if !source_db_path.exists() {
        return Err(BackupError::InvalidPath(format!(
            "source database '{}' does not exist",
            source_db_path.display()
        )));
    }

    check_path_safety(source_db_path, "source database")?;
    check_path_safety(target_backup_path, "target backup")?;
    check_not_in_media_root(target_backup_path, media_root, "target backup path")?;

    let source_canonical = source_db_path
        .canonicalize()
        .map_err(|e| BackupError::InvalidPath(format!("cannot resolve source path: {e}")))?;

    if let Ok(target_canon) = target_backup_path.canonicalize()
        && target_canon == source_canonical
    {
        return Err(BackupError::InvalidPath(
            "target backup path cannot be identical to source database".into(),
        ));
    }

    // Check if target already exists: by default, refuse to overwrite
    let target_exists = std::fs::symlink_metadata(target_backup_path).is_ok();
    if target_exists && !options.allow_replace {
        return Err(BackupError::TargetExists(format!(
            "target backup file '{}' already exists; refusing to overwrite to prevent data loss. Use --replace to safely replace an existing backup, or specify a new filename",
            target_backup_path.display()
        )));
    }

    // Ensure target parent directory exists with secure permissions (0750)
    let parent = target_backup_path.parent().unwrap_or(Path::new("."));
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o750));
        }
    }

    // Generate unique temporary destination in the same directory (same filesystem)
    let temp_filename = format!(
        ".backup-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    );
    let temp_backup_path = parent.join(temp_filename);
    let guard = TempFileGuard::new(temp_backup_path.clone());

    let temp_str = temp_backup_path.to_str().ok_or_else(|| {
        BackupError::InvalidPath("temporary backup path is not valid unicode".into())
    })?;

    // Connect to source database with WAL mode and 15s timeout
    let connect_options = SqliteConnectOptions::new()
        .filename(&source_canonical)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(15));

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options)
        .await?;

    // Execute VACUUM INTO on the temporary path
    let escaped_path = temp_str.replace('\'', "''");
    let mut builder = sqlx::QueryBuilder::<sqlx::Sqlite>::new("VACUUM INTO ");
    builder.push(format!("'{}'", escaped_path));
    builder.build().execute(&pool).await?;
    pool.close().await;

    // Apply strict permissions (0600) on the temporary backup file
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temp_backup_path, std::fs::Permissions::from_mode(0o600));
    }

    // Verify backup file integrity and schema before publishing
    let verify_options = SqliteConnectOptions::new()
        .filename(&temp_backup_path)
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

    // Verify required schema tables exist
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

    // Sync content to disk for durability before publishing
    sync_file(&temp_backup_path)?;

    publish_file(&temp_backup_path, target_backup_path)?;
    guard.disarm();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            std::fs::set_permissions(target_backup_path, std::fs::Permissions::from_mode(0o600));
    }

    let meta = std::fs::metadata(target_backup_path)?;

    Ok(BackupSummary {
        source_path: source_canonical.to_string_lossy().to_string(),
        backup_path: target_backup_path.to_string_lossy().to_string(),
        file_size: meta.len(),
        user_count,
        library_count,
        work_count,
    })
}

/// Restore a database from a validated backup file into the target database path.
///
/// Ensures point-in-time integrity, validates required tables, creates atomic staging,
/// synchronizes disk bytes, and applies idempotent migrations.
///
/// Security constraints:
/// - Explicitly refuses to overwrite any existing database.
/// - Explicitly refuses if residual WAL or SHM journals exist (never deletes them automatically).
/// - Refuses symlinks and reparse points.
/// - Cleans up temporary staging files on any failure.
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

    check_path_safety(backup_path, "backup")?;
    check_path_safety(target_db_path, "target database")?;
    check_not_in_media_root(target_db_path, media_root, "target database path")?;

    // Refuse restoration if target database already exists
    if std::fs::symlink_metadata(target_db_path).is_ok() {
        return Err(BackupError::TargetExists(format!(
            "target database '{}' already exists; restoration into an existing database is refused to prevent data loss. Restore into a new/empty volume or directory instead",
            target_db_path.display()
        )));
    }

    // Refuse restoration if residual WAL or SHM journals exist (do NOT delete them automatically)
    let wal_path = PathBuf::from(format!("{}-wal", target_db_path.display()));
    let shm_path = PathBuf::from(format!("{}-shm", target_db_path.display()));

    if std::fs::symlink_metadata(&wal_path).is_ok() {
        return Err(BackupError::ResidualJournals(format!(
            "residual WAL journal '{}' exists; restoration refused to prevent database corruption. Destination directory must be clean",
            wal_path.display()
        )));
    }

    if std::fs::symlink_metadata(&shm_path).is_ok() {
        return Err(BackupError::ResidualJournals(format!(
            "residual SHM index '{}' exists; restoration refused to prevent database corruption. Destination directory must be clean",
            shm_path.display()
        )));
    }

    // Validate backup file integrity and structure before any disk alteration
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

    // Ensure target parent directory exists with 0750 permissions
    let parent = target_db_path.parent().unwrap_or(Path::new("."));
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o750));
        }
    }

    // Use a unique staging file in the target directory for atomic publication
    let staging_filename = format!(
        ".restore-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    );
    let staging_path = parent.join(staging_filename);
    let guard = TempFileGuard::new(staging_path.clone());

    std::fs::copy(backup_path, &staging_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600));
    }

    // Sync copied bytes to disk for durability
    sync_file(&staging_path)?;

    // Re-verify that target database and journals STILL do not exist before publishing
    if std::fs::symlink_metadata(target_db_path).is_ok() {
        return Err(BackupError::TargetExists(format!(
            "target database '{}' was created during restore; aborting to prevent overwrite",
            target_db_path.display()
        )));
    }
    if std::fs::symlink_metadata(&wal_path).is_ok() {
        return Err(BackupError::ResidualJournals(format!(
            "residual WAL journal '{}' was created during restore; aborting",
            wal_path.display()
        )));
    }
    if std::fs::symlink_metadata(&shm_path).is_ok() {
        return Err(BackupError::ResidualJournals(format!(
            "residual SHM index '{}' was created during restore; aborting",
            shm_path.display()
        )));
    }

    // Atomic move of staging file to target database path
    publish_file(&staging_path, target_db_path)?;
    guard.disarm();

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
