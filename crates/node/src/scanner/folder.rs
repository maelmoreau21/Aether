use super::*;
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

pub(super) type ArtworkCache =
    HashMap<(PathBuf, String), (Option<crate::images::LocalImage>, Option<String>)>;

pub(super) async fn artwork_cached(
    root: &Path,
    dir: &Path,
    cache: &HashMap<String, crate::images::LocalImage>,
    memo: &mut ArtworkCache,
    role: &str,
    names: &[&str],
) -> (Option<crate::images::LocalImage>, Option<String>) {
    let key = (dir.to_path_buf(), role.to_owned());
    if let Some(found) = memo.get(&key) {
        return found.clone();
    }
    let found = crate::images::discover_named(root, dir, cache, names).await;
    if memo.len() >= 1024 {
        memo.clear();
    }
    memo.insert(key, found.clone());
    found
}

/// Precomputed video stems and identity keys for a single directory,
/// avoiding `O(N^2)` directory re-reads when resolving sidecar NFOs and posters.
#[derive(Debug, Clone)]
pub struct FolderVideos {
    entries: Vec<(Option<OsString>, String)>,
    pub(crate) sidecar_paths: Vec<PathBuf>,
}

impl FolderVideos {
    pub async fn load(root: &Path, parent: &Path) -> io::Result<Self> {
        let dir_path = if parent.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(parent)
        };
        let mut read_dir = tokio::fs::read_dir(dir_path).await?;
        let mut entries = Vec::new();
        let mut sidecar_paths = Vec::new();
        while let Some(entry) = read_dir.next_entry().await? {
            let p = entry.path();
            let file_name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if ignored(file_name) {
                continue;
            }
            if p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                [
                    "mkv", "mp4", "webm", "m4v", "mov", "obu", "opus", "ogg", "srt", "vtt", "ass",
                    "ssa",
                ]
                .iter()
                .any(|e| s.eq_ignore_ascii_case(e))
            }) {
                if sidecar_paths.len() >= 10000 {
                    return Err(io::Error::other("directory sidecar limit exceeded"));
                }
                sidecar_paths.push(p.clone());
            }
            if p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
                s.eq_ignore_ascii_case("mkv")
                    || s.eq_ignore_ascii_case("mp4")
                    || s.eq_ignore_ascii_case("webm")
                    || s.eq_ignore_ascii_case("m4v")
                    || s.eq_ignore_ascii_case("mov")
                    || s.eq_ignore_ascii_case("obu")
            }) {
                let relative = p
                    .strip_prefix(root)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "outside root"))?;
                let stem = p.file_stem().map(OsStr::to_os_string);
                let key = identity(relative).3;
                entries.push((stem, key));
            }
        }
        Ok(Self {
            entries,
            sidecar_paths,
        })
    }

    pub fn all_match_identity(&self, key: &str) -> bool {
        self.entries.iter().all(|(_, k)| k == key)
    }

    pub fn stem_unambiguous(&self, stem: Option<&OsStr>, key: &str) -> bool {
        self.entries
            .iter()
            .all(|(s, k)| s.as_deref() != stem || k == key)
    }
}
