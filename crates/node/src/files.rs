//! Confined, read-only filesystem handle opening for the Media Node.
//!
//! Replaces vulnerable `canonicalize()` + later re-open patterns with handle-confined
//! read-only opening (`OpenedFile`). Callers inspect metadata and stream bytes directly
//! from the already opened and verified file descriptor/handle without re-opening by path.
//!
//! ## Platform Security Guarantees
//! - **Linux**: Uses `openat2(2)` with `ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV`
//!   when supported by the kernel (Linux 5.6+), providing atomic kernel-enforced confinement
//!   against path traversal, symlink substitution, and mount-point crossing during resolution.
//!   If `openat2` is unavailable (`ENOSYS`), falls back to step-by-step directory descriptor
//!   walking via `openat` with `OFlags::NOFOLLOW | OFlags::DIRECTORY | OFlags::RDONLY` and
//!   final file open with `OFlags::NOFOLLOW | OFlags::RDONLY` + `fstat` verification.
//! - **Other Unix**: Step-by-step directory descriptor walking via `openat` + `OFlags::NOFOLLOW`.
//! - **Windows**: Rejects parent components, symlinks, and all reparse points (including NTFS
//!   directory junctions, `FILE_ATTRIBUTE_REPARSE_POINT = 0x400`) at every path component,
//!   opens the file in strict read-only mode (`FILE_FLAG_OPEN_REPARSE_POINT` on reparse verification
//!   and handle `metadata()` verification + canonical path prefix check on the opened handle),
//!   and reuses the opened `std::fs::File` handle directly for validation and streaming so no
//!   second path-based open (`ServeFile`) occurs. Note: Windows Win32 paths do not expose an
//!   unprivileged `openat2(RESOLVE_BENEATH)` equivalent, so intermediate component checks and final
//!   handle verification still require operator-controlled directory ACLs against a concurrent
//!   local administrator/writer race during traversal before handle acquisition; once opened,
//!   the handle itself is never reopened by path.

use std::{
    fs::{File, Metadata},
    io,
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Check whether metadata represents a symlink or Windows reparse point (including junctions).
pub fn is_linked(metadata: &Metadata) -> bool {
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

/// Validate that a relative inventory path is non-empty and consists strictly of normal components.
pub fn validate_relative_components(relative: &Path) -> io::Result<()> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid inventory-relative path",
        ));
    }
    Ok(())
}

/// Validate administrative configuration against the local Node's operator-selected
/// root. Performs no discovery, probe, or write. The canonical result is private
/// configuration, never a public media identifier.
pub async fn validate_library_root(root: &Path, allowed_root: &Path) -> io::Result<PathBuf> {
    if !root.is_absolute()
        || !allowed_root.is_absolute()
        || root.components().any(|part| part == Component::ParentDir)
        || allowed_root
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "library roots must be absolute without parent components",
        ));
    }
    if is_linked(&tokio::fs::symlink_metadata(allowed_root).await?)
        || is_linked(&tokio::fs::symlink_metadata(root).await?)
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "library root outside local Node inventory",
        ));
    }
    let allowed = tokio::fs::canonicalize(allowed_root).await?;
    let canonical = tokio::fs::canonicalize(root).await?;
    if !canonical.starts_with(&allowed) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "library root outside local Node inventory",
        ));
    }
    if !tokio::fs::metadata(&allowed).await?.is_dir()
        || !tokio::fs::metadata(&canonical).await?.is_dir()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "library root must be a directory",
        ));
    }
    Ok(canonical)
}

/// An already-opened, read-only, confined regular file handle with its verified metadata.
#[derive(Debug)]
pub struct OpenedFile {
    file: File,
    metadata: Metadata,
    modified_ns: String,
    canonical_path: PathBuf,
}

impl OpenedFile {
    /// Open a file relative to `root` in strict read-only mode, rejecting symlinks,
    /// junctions/reparse points, parent directory traversal, and non-regular files.
    pub async fn open_in_root(root: &Path, relative: &Path) -> io::Result<Self> {
        let root = root.to_owned();
        let relative = relative.to_owned();
        tokio::task::spawn_blocking(move || Self::open_in_root_sync(&root, &relative))
            .await
            .map_err(|_| io::Error::other("confined file worker failed"))?
    }

    /// Synchronous confined open for blocking workers.
    pub fn open_in_root_sync(root: &Path, relative: &Path) -> io::Result<Self> {
        validate_relative_components(relative)?;
        let root_meta = std::fs::symlink_metadata(root)?;
        if is_linked(&root_meta) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "library root link or reparse point forbidden",
            ));
        }
        if !root_meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "library root must be a directory",
            ));
        }

        #[cfg(unix)]
        {
            Self::open_unix(root, relative)
        }
        #[cfg(windows)]
        {
            Self::open_windows(root, relative)
        }
    }

    #[cfg(unix)]
    fn open_unix(root: &Path, relative: &Path) -> io::Result<Self> {
        Self::open_unix_with_mode(root, relative, false)
    }

    #[cfg(unix)]
    fn open_unix_with_mode(root: &Path, relative: &Path, force_fallback: bool) -> io::Result<Self> {
        use rustix::fs::{Mode, OFlags, openat};
        let canonical_root = std::fs::canonicalize(root)?;
        let root_dir = openat(
            rustix::fs::CWD,
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;

        #[cfg(target_os = "linux")]
        let opened_fd = if force_fallback {
            None
        } else {
            use rustix::fs::{ResolveFlags, openat2};
            match openat2(
                &root_dir,
                relative,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
                ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_XDEV,
            ) {
                Ok(fd) => Some(fd),
                // Fall back to step-by-step openat(O_NOFOLLOW) on kernels < 5.6 (ENOSYS)
                // or container seccomp profiles that block SYS_openat2 with EPERM.
                Err(err) if err == rustix::io::Errno::NOSYS || err == rustix::io::Errno::PERM => {
                    None
                }
                Err(err) => {
                    let io_err = io::Error::from(err);
                    return Err(match io_err.kind() {
                        io::ErrorKind::NotFound => io_err,
                        _ => io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "links and reparse points are forbidden",
                        ),
                    });
                }
            }
        };

        #[cfg(not(target_os = "linux"))]
        let opened_fd: Option<std::os::fd::OwnedFd> = {
            let _ = force_fallback;
            None
        };

        let file_fd = match opened_fd {
            Some(fd) => fd,
            None => Self::open_unix_fallback(root_dir, relative)?,
        };

        let file = File::from(file_fd);
        let metadata = file.metadata()?;
        if !metadata.is_file() || is_linked(&metadata) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "asset outside root or not a regular file",
            ));
        }
        let modified_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid mtime"))?
            .as_nanos()
            .to_string();
        let canonical_path = canonical_root.join(relative);
        Ok(Self {
            file,
            metadata,
            modified_ns,
            canonical_path,
        })
    }

    #[cfg(unix)]
    fn open_unix_fallback(
        root_dir: std::os::fd::OwnedFd,
        relative: &Path,
    ) -> io::Result<std::os::fd::OwnedFd> {
        use rustix::fs::{Mode, OFlags, openat};
        let components: Vec<_> = relative.components().collect();
        let mut current_dir = root_dir;
        for component in &components[..components.len().saturating_sub(1)] {
            let name = component.as_os_str();
            current_dir = openat(
                &current_dir,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|err| {
                let io_err = io::Error::from(err);
                if io_err.kind() == io::ErrorKind::NotFound {
                    io_err
                } else {
                    io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "links and reparse points are forbidden",
                    )
                }
            })?;
        }
        let last = components
            .last()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty path"))?
            .as_os_str();
        openat(
            &current_dir,
            last,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|err| {
            let io_err = io::Error::from(err);
            if io_err.kind() == io::ErrorKind::NotFound {
                io_err
            } else {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "links and reparse points are forbidden",
                )
            }
        })
    }

    #[cfg(windows)]
    fn open_windows(root: &Path, relative: &Path) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;

        let canonical_root = std::fs::canonicalize(root)?;
        let components: Vec<_> = relative.components().collect();
        // Pin root and all intermediate directories without FILE_SHARE_DELETE so Windows NTFS
        // forbids renaming, deleting, or replacing any ancestor directory during traversal.
        let mut pinned_dirs = Vec::with_capacity(components.len());
        let root_handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(&canonical_root)?;
        let root_meta = root_handle.metadata()?;
        if is_linked(&root_meta) || !root_meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "links and reparse points are forbidden",
            ));
        }
        pinned_dirs.push(root_handle);

        let mut current = canonical_root.clone();
        for (idx, component) in components.iter().enumerate() {
            current.push(component);
            let is_last = idx + 1 == components.len();
            if !is_last {
                // Open directory handle with FILE_FLAG_OPEN_REPARSE_POINT and no FILE_SHARE_DELETE.
                let dir_handle = std::fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                    .open(&current)?;
                let dir_meta = dir_handle.metadata()?;
                if is_linked(&dir_meta) || !dir_meta.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "links and reparse points are forbidden",
                    ));
                }
                pinned_dirs.push(dir_handle);
            }
        }

        // Open final target with FILE_FLAG_OPEN_REPARSE_POINT so Windows does NOT follow
        // a symlink or reparse point on the leaf component, allowing us to inspect the
        // opened handle's attributes directly and reject any link atomically on the leaf.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)?;
        let metadata = file.metadata()?;
        if is_linked(&metadata) || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "links and reparse points are forbidden",
            ));
        }

        // Verify that the canonical path of the opened target remains strictly inside canonical_root
        // while all ancestor directory handles are still pinned without FILE_SHARE_DELETE.
        let canonical_target = std::fs::canonicalize(&current)?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "asset outside root or not a regular file",
            ));
        }

        let after_meta = file.metadata()?;
        if is_linked(&after_meta)
            || !after_meta.is_file()
            || after_meta.len() != metadata.len()
            || after_meta.modified()? != metadata.modified()?
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file changed during confinement verification",
            ));
        }
        drop(pinned_dirs);

        let modified_ns = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid mtime"))?
            .as_nanos()
            .to_string();

        Ok(Self {
            file,
            metadata,
            modified_ns,
            canonical_path: canonical_target,
        })
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn size(&self) -> u64 {
        self.metadata.len()
    }

    pub fn modified_ns(&self) -> &str {
        &self.modified_ns
    }

    pub fn modified(&self) -> Option<std::time::SystemTime> {
        self.metadata.modified().ok()
    }

    pub fn std_file(&self) -> &File {
        &self.file
    }

    pub fn std_file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
            metadata: self.metadata.clone(),
            modified_ns: self.modified_ns.clone(),
            canonical_path: self.canonical_path.clone(),
        })
    }

    /// Re-query the open file descriptor/handle (`fstat` / `GetFileInformationByHandle`)
    /// to verify that the underlying file's size and modification timestamp have not changed.
    pub fn verify_unchanged(&self) -> io::Result<()> {
        let current = self.file.metadata()?;
        if is_linked(&current) || !current.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file handle is no longer a regular file",
            ));
        }
        let current_ns = current
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid mtime"))?
            .as_nanos()
            .to_string();
        if current.len() != self.metadata.len() || current_ns != self.modified_ns {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file changed during operation",
            ));
        }
        Ok(())
    }

    /// Verify both handle metadata and path metadata remain unchanged and unlinked.
    pub fn verify_unchanged_sync(&self) -> io::Result<()> {
        self.verify_unchanged()?;
        let path_meta = std::fs::symlink_metadata(&self.canonical_path)?;
        if is_linked(&path_meta)
            || !path_meta.is_file()
            || path_meta.len() != self.metadata.len()
            || path_meta.modified().ok() != self.metadata.modified().ok()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file path changed during operation",
            ));
        }
        Ok(())
    }

    pub fn try_clone_std(&self) -> io::Result<File> {
        self.file.try_clone()
    }

    pub fn into_std(self) -> File {
        self.file
    }

    pub fn into_tokio(self) -> tokio::fs::File {
        tokio::fs::File::from_std(self.file)
    }

    pub fn into_tokio_file(self) -> tokio::fs::File {
        tokio::fs::File::from_std(self.file)
    }

    /// Private canonical path for trusted local subprocess adapters (`ffprobe`) only.
    pub(crate) fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

/// Legacy compatibility helper returning the verified path after opening and validating the handle.
pub async fn confined_file(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let opened = OpenedFile::open_in_root(root, relative).await?;
    Ok(opened.canonical_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[tokio::test]
    async fn library_root_requires_existing_confined_absolute_directory() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed");
        let child = allowed.join("films");
        let sibling = temp.path().join("allowed-other");
        tokio::fs::create_dir_all(&child).await.unwrap();
        tokio::fs::create_dir(&sibling).await.unwrap();
        let canonical = tokio::fs::canonicalize(&child).await.unwrap();
        assert_eq!(
            validate_library_root(&child, &allowed).await.unwrap(),
            canonical
        );
        assert_eq!(
            validate_library_root(&allowed, &allowed).await.unwrap(),
            tokio::fs::canonicalize(&allowed).await.unwrap()
        );
        assert_eq!(
            validate_library_root(&sibling, &allowed)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        for invalid in [
            Path::new("films").to_path_buf(),
            child.join(".."),
            child.join("missing"),
        ] {
            assert!(validate_library_root(&invalid, &allowed).await.is_err());
        }
        let file = allowed.join("file.mkv");
        tokio::fs::write(&file, b"fixture").await.unwrap();
        assert!(validate_library_root(&file, &allowed).await.is_err());
        assert!(validate_library_root(&child, &file).await.is_err());
        assert!(
            validate_library_root(&child, Path::new("allowed"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn library_root_rejects_symlink_escape() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = temp.path().join("allowed");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir(&allowed).await.unwrap();
        tokio::fs::create_dir(&outside).await.unwrap();
        let link = allowed.join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(output.status.success(), "junction fixture creation failed");
        }
        assert_eq!(
            validate_library_root(&link, &allowed)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[tokio::test]
    async fn opened_file_reads_original_handle_even_if_path_is_substituted_after_open() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();

        let target = root.join("video.mp4");
        let secret = outside.join("secret.mp4");
        tokio::fs::write(&target, b"legitimate-video-bytes")
            .await
            .unwrap();
        tokio::fs::write(&secret, b"hostile-secret-bytes")
            .await
            .unwrap();

        // 1. Acquire confined handle
        let opened = OpenedFile::open_in_root(&root, Path::new("video.mp4"))
            .await
            .unwrap();
        assert_eq!(opened.size(), 22);
        opened.verify_unchanged().unwrap();

        // 2. Substitute the path on disk while the handle is already open
        let moved = root.join("video_moved.mp4");
        std::fs::rename(&target, &moved).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, &target).unwrap();
        #[cfg(windows)]
        {
            // On Windows, replace the path with a junction or different content
            std::fs::write(&target, b"replaced-content-after-open").unwrap();
        }

        // 3. Reading from the already-opened handle MUST return the original legitimate bytes
        let mut std_file = opened.into_std();
        let mut buf = Vec::new();
        std_file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"legitimate-video-bytes");
    }

    #[tokio::test]
    async fn opened_file_detects_in_place_mutation_via_verify_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let target = root.join("mutable.mp4");
        tokio::fs::write(&target, b"initial-bytes").await.unwrap();

        let opened = OpenedFile::open_in_root(&root, Path::new("mutable.mp4"))
            .await
            .unwrap();
        assert!(opened.verify_unchanged().is_ok());

        // Append to the same underlying inode/file
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .unwrap();
        std::io::Write::write_all(&mut writer, b"-mutated").unwrap();
        writer.sync_all().unwrap();
        drop(writer);

        assert!(
            opened.verify_unchanged().is_err(),
            "fstat / handle metadata must detect in-place size mutation"
        );
    }

    #[tokio::test]
    async fn opened_file_rejects_traversal_and_directory_links() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("secret.mp4"), b"secret")
            .await
            .unwrap();

        let link = root.join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(output.status.success());
        }

        for invalid in [
            Path::new(""),
            Path::new("../outside/secret.mp4"),
            Path::new("escape/secret.mp4"),
            Path::new("/etc/passwd"),
        ] {
            assert!(
                OpenedFile::open_in_root(&root, invalid).await.is_err(),
                "Expected rejection for {:?}",
                invalid
            );
        }
    }

    #[tokio::test]
    async fn nested_subdirectories_and_fallback_traversal_confinement() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let nested = root.join("alpha").join("beta");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir_all(&nested).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(nested.join("film.mkv"), b"nested-mkv-bytes")
            .await
            .unwrap();
        tokio::fs::write(outside.join("secret.mkv"), b"secret-mkv-bytes")
            .await
            .unwrap();

        let opened = OpenedFile::open_in_root(&root, Path::new("alpha/beta/film.mkv"))
            .await
            .unwrap();
        assert_eq!(opened.size(), 16);
        assert!(opened.verify_unchanged_sync().is_ok());

        assert_eq!(
            OpenedFile::open_in_root(&root, Path::new("alpha/beta/missing.mkv"))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );

        // A regular file used as an intermediate directory component must be rejected
        assert!(
            OpenedFile::open_in_root(&root, Path::new("alpha/beta/film.mkv/child.mkv"))
                .await
                .is_err()
        );

        #[cfg(unix)]
        {
            let link_dir = root.join("alpha").join("escape_dir");
            let link_file = nested.join("escape_file.mkv");
            std::os::unix::fs::symlink(&outside, &link_dir).unwrap();
            std::os::unix::fs::symlink(outside.join("secret.mkv"), &link_file).unwrap();

            // Force the component-by-component openat(O_NOFOLLOW) fallback even on Linux 5.6+
            let fallback_ok =
                OpenedFile::open_unix_with_mode(&root, Path::new("alpha/beta/film.mkv"), true)
                    .unwrap();
            assert_eq!(fallback_ok.size(), 16);
            assert_eq!(
                OpenedFile::open_unix_with_mode(
                    &root,
                    Path::new("alpha/escape_dir/secret.mkv"),
                    true
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::PermissionDenied
            );
            assert_eq!(
                OpenedFile::open_unix_with_mode(
                    &root,
                    Path::new("alpha/beta/escape_file.mkv"),
                    true
                )
                .unwrap_err()
                .kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }
}
