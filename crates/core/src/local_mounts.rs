//! Discover directory mounts without exposing the container filesystem root.
use std::path::{Path, PathBuf};

fn media_mounts(input: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for line in input.lines() {
        let Some(raw) = line.split_whitespace().nth(4) else {
            continue;
        };
        let decoded = raw
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\");
        let path = PathBuf::from(decoded);
        if !raw.starts_with('/')
            || path == Path::new("/")
            || [
                "/proc", "/sys", "/dev", "/etc", "/app", "/usr", "/bin", "/sbin", "/lib", "/lib64",
                "/run", "/tmp", "/var", "/data", "/backups",
            ]
            .iter()
            .any(|excluded| path.starts_with(excluded))
        {
            continue;
        }
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    roots
}

pub(crate) async fn allowed_roots(fallback: &Path) -> Vec<PathBuf> {
    // An explicit operator root keeps its original confinement contract.
    if std::env::var_os("MEDIA_LIBRARY_ROOT").is_some_and(|v| !v.is_empty()) {
        return vec![fallback.to_owned()];
    }
    let mut roots = vec![fallback.to_owned()];
    if cfg!(target_os = "linux")
        && let Ok(input) = tokio::fs::read_to_string("/proc/self/mountinfo").await
    {
        for path in media_mounts(&input) {
            if tokio::fs::metadata(&path).await.is_ok_and(|m| m.is_dir()) {
                roots.push(path);
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_custom_mounts_and_excludes_system_and_state() {
        let input = [
            "/",
            "/data",
            "/backups",
            "/etc/hosts",
            "/proc",
            "/dev/shm",
            "/app/config",
            "/Films_UHD",
            "/Séries_UHD",
            "/Films\\040HD",
            "/Films_UHD",
        ]
        .map(|p| format!("1 0 0:1 / {p} ro - ext4 /dev/a ro"))
        .join("\n");
        assert_eq!(
            media_mounts(&input),
            vec![
                PathBuf::from("/Films_UHD"),
                PathBuf::from("/Séries_UHD"),
                PathBuf::from("/Films HD")
            ]
        );
    }
}

pub(crate) async fn validate(root: &Path, fallback: &Path) -> std::io::Result<PathBuf> {
    for allowed in allowed_roots(fallback).await {
        if let Ok(path) = crate::node::validate_library_root(root, &allowed).await {
            return Ok(path);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "directory outside allowed media mounts",
    ))
}
