//! Crash-recoverable preparation directories. Only versioned, leased cache entries are reclaimed.
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

#[derive(Debug)]
pub struct CacheDirectory {
    directory: Option<tempfile::TempDir>,
    lease: Option<File>,
}
impl CacheDirectory {
    pub fn path(&self) -> &Path {
        self.directory
            .as_ref()
            .map(|d| d.path())
            .unwrap_or(Path::new(""))
    }
    pub fn new() -> io::Result<Self> {
        let root = match std::env::var_os("MEDIA_PREPARATION_CACHE_DIR") {
            Some(path) => {
                let path = std::path::PathBuf::from(path);
                if !path.is_absolute() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "MEDIA_PREPARATION_CACHE_DIR must be absolute",
                    ));
                }
                path
            }
            None => std::env::temp_dir().join("media-preparation-v2"),
        };
        Self::in_root(&root)
    }
    fn in_root(root: &Path) -> io::Result<Self> {
        match fs::create_dir(root) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        safe_directory(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(root)?;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(io::Error::other("cache root is not private"));
            }
        }
        let lock_path = root.join("allocation.lock");
        if lock_path.exists() && fs::symlink_metadata(&lock_path)?.file_type().is_symlink() {
            return Err(io::Error::other("invalid cache lock"));
        }
        let global = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(5);
        loop {
            match global.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        return Err(io::Error::other("cache allocation busy"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::Error(err)) => {
                    return Err(io::Error::other(format!("cache lock failed: {err}")));
                }
            }
        }
        for entry in fs::read_dir(root)?.take(4096) {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("entry-") || safe_directory(&entry.path()).is_err() {
                continue;
            }
            let marker = entry.path().join("lease");
            if fs::symlink_metadata(&marker).is_err()
                || fs::symlink_metadata(&marker)?.file_type().is_symlink()
            {
                continue;
            }
            let Ok(lease) = OpenOptions::new().read(true).write(true).open(marker) else {
                continue;
            };
            if lease.try_lock().is_ok() {
                drop(lease);
                // Root is private, allocation serialized, and active entries hold their lease.
                fs::remove_dir_all(entry.path())?;
            }
        }
        let directory = tempfile::Builder::new().prefix("entry-").tempdir_in(root)?;
        let lease = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join("lease"))?;
        lease
            .try_lock()
            .map_err(|_| io::Error::other("cache lease unavailable"))?;
        Ok(Self {
            directory: Some(directory),
            lease: Some(lease),
        })
    }
}
fn safe_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("invalid cache directory"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("cache reparse point refused"));
        }
    }
    Ok(())
}
impl Drop for CacheDirectory {
    fn drop(&mut self) {
        drop(self.lease.take());
        drop(self.directory.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reclaims_orphan_but_preserves_active_and_unowned() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().join("cache");
        let active = CacheDirectory::in_root(&root).unwrap();
        let active_path = active.path().to_owned();
        let mut orphan = CacheDirectory::in_root(&root).unwrap();
        let orphan_path: std::path::PathBuf = orphan.directory.take().unwrap().keep();
        drop(orphan);
        fs::write(orphan_path.join("content"), b"partial output after crash").unwrap();
        fs::create_dir(root.join("unowned")).unwrap();
        let _next = CacheDirectory::in_root(&root).unwrap();
        assert!(active_path.exists());
        assert!(!orphan_path.exists());
        assert!(root.join("unowned").exists());
        drop(active);
        assert!(!active_path.exists());
    }
    #[test]
    #[ignore = "helper launched by abrupt_exit_releases_lease"]
    fn crash_child() {
        let root =
            std::path::PathBuf::from(std::env::var_os("MEDIA_TEST_CACHE_CRASH_ROOT").unwrap());
        let directory = CacheDirectory::in_root(&root).unwrap();
        fs::write(directory.path().join("partial"), b"incomplete packet copy").unwrap();
        std::process::exit(17);
    }
    #[test]
    fn abrupt_exit_releases_lease() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("cache");
        let active = CacheDirectory::in_root(&root).unwrap();
        let active_path = active.path().to_owned();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cache_dirs::tests::crash_child", "--ignored"])
            .env("MEDIA_TEST_CACHE_CRASH_ROOT", &root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(17));
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("entry-"))
                .count(),
            2
        );
        let _next = CacheDirectory::in_root(&root).unwrap();
        assert!(active_path.exists());
        assert_eq!(
            fs::read_dir(&root)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("entry-"))
                .count(),
            2
        );
    }
}
