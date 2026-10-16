//! Local Media Node discovery. No writes, database access, or HTTP concerns.
pub use crate::files::confined_file;
use crate::{
    files::{OpenedFile, is_linked},
    probe,
};
pub use media_contracts::inventory::{CachedFile, ScanIssue, ScanReport, ScannedFile};
use std::{
    collections::{BTreeMap, HashMap},
    ffi::{OsStr, OsString},
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Precomputed video stems and identity keys for a single directory,
/// avoiding `O(N^2)` directory re-reads when resolving sidecar NFOs and posters.
#[derive(Debug, Clone)]
pub struct FolderVideos {
    entries: Vec<(Option<OsString>, String)>,
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
        while let Some(entry) = read_dir.next_entry().await? {
            let p = entry.path();
            if p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("mkv") || s.eq_ignore_ascii_case("mp4"))
            {
                let relative = p
                    .strip_prefix(root)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "outside root"))?;
                let stem = p.file_stem().map(OsStr::to_os_string);
                let key = identity(relative).3;
                entries.push((stem, key));
            }
        }
        Ok(Self { entries })
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

fn ignored(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.ends_with(".trickplay")
        || name.ends_with(".metathumb")
        || matches!(
            name.as_str(),
            "extrafanart" | "$recycle.bin" | "system volume information"
        )
        || [
            ".part",
            ".partial",
            ".tmp",
            ".temp",
            ".crdownload",
            ".download",
            ".!qb",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

fn metadata_name(name: &str) -> (String, Option<u16>, BTreeMap<String, String>) {
    let mut ids = BTreeMap::new();
    for token in name
        .split('[')
        .skip(1)
        .filter_map(|s| s.split_once(']').map(|p| p.0))
    {
        if let Some((provider, id)) = token.split_once('-') {
            let provider = provider.to_ascii_lowercase();
            let valid = match provider.as_str() {
                "tmdb" | "tvdb" => !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
                "imdb" => id
                    .strip_prefix("tt")
                    .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())),
                _ => false,
            };
            if valid {
                ids.insert(provider, id.to_owned());
            }
        }
    }
    let mut year = None;
    let mut end = name.find('[').unwrap_or(name.len());
    for (start, _) in name.match_indices('(') {
        if let Some(value) = name.get(start + 1..start + 5)
            && name.as_bytes().get(start + 5) == Some(&b')')
            && let Ok(value) = value.parse::<u16>()
            && (1888..=2100).contains(&value)
        {
            year = Some(value);
            end = end.min(start);
            break;
        }
    }
    let title = name[..end]
        .trim()
        .trim_end_matches(['-', '.', '_', ' '])
        .to_owned();
    (
        if title.is_empty() {
            name.to_owned()
        } else {
            title
        },
        year,
        ids,
    )
}

pub(crate) fn identity(relative: &Path) -> (String, Option<u16>, BTreeMap<String, String>, String) {
    let stem = relative
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let parent = relative.parent().filter(|p| !p.as_os_str().is_empty());
    let folder = parent.and_then(|p| p.file_name()).and_then(|s| s.to_str());
    let parsed_folder = folder.map(metadata_name);
    let parsed_file = metadata_name(stem);
    let conflict = parsed_folder.as_ref().is_some_and(|(title, year, ids)| {
        parsed_file
            .2
            .iter()
            .any(|(provider, id)| ids.get(provider).is_some_and(|folder_id| folder_id != id))
            || parsed_file
                .1
                .is_some_and(|file_year| year.is_some_and(|folder_year| folder_year != file_year))
            || (parsed_file.1.is_some() && !parsed_file.0.eq_ignore_ascii_case(title))
    });
    let (title, year, ids, group) = match parsed_folder {
        Some((title, year, ids)) if !conflict && (year.is_some() || !ids.is_empty()) => (
            title,
            year,
            ids,
            format!(
                "folder:{}",
                parent.unwrap_or(Path::new("")).to_string_lossy()
            ),
        ),
        _ => {
            let (t, y, ids) = parsed_file;
            (t, y, ids, format!("file:{}", relative.to_string_lossy()))
        }
    };
    let key = ["tmdb", "imdb", "tvdb"]
        .iter()
        .find_map(|p| ids.get(*p).map(|v| format!("{p}:{v}")))
        .unwrap_or(group);
    (title, year, ids, key)
}

/// Depth-first streaming traversal (one directory iterator per depth), one probe at a time.
/// Probe cancellation drops its kill-on-drop child. Errors never contain absolute paths.
pub async fn scan(
    root: &Path,
    cache: &HashMap<String, CachedFile>,
    cancel: &AtomicBool,
) -> io::Result<ScanReport> {
    scan_with_images(root, cache, &HashMap::new(), cancel).await
}

pub async fn scan_with_images(
    root: &Path,
    cache: &HashMap<String, CachedFile>,
    image_cache: &HashMap<String, super::images::LocalImage>,
    cancel: &AtomicBool,
) -> io::Result<ScanReport> {
    if !root.is_absolute()
        || root.components().any(|c| c == Component::ParentDir)
        || is_linked(&tokio::fs::symlink_metadata(root).await?)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid library root",
        ));
    }
    let root = tokio::fs::canonicalize(root).await?;
    let mut stack = vec![tokio::fs::read_dir(&root).await?];
    let mut folder_cache: HashMap<PathBuf, FolderVideos> = HashMap::new();
    let mut report = ScanReport {
        complete: true,
        ..Default::default()
    };
    while !stack.is_empty() {
        if cancel.load(Ordering::Relaxed) {
            report.complete = false;
            break;
        }
        let entry = match stack.last_mut() {
            Some(dir) => match dir.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => {
                    stack.pop();
                    continue;
                }
                Err(_) => {
                    report.complete = false;
                    report.errors.push(ScanIssue {
                        relative_path: String::new(),
                        message: "directory traversal failed".into(),
                    });
                    stack.pop();
                    continue;
                }
            },
            None => break,
        };
        let path = entry.path();
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| io::Error::other("unconfined traversal"))?;
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let name = entry.file_name();
        if ignored(&name.to_string_lossy()) {
            continue;
        }
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(_) => {
                report.complete = false;
                report.errors.push(ScanIssue {
                    relative_path,
                    message: "file metadata unavailable".into(),
                });
                continue;
            }
        };
        if is_linked(&metadata) {
            report.errors.push(ScanIssue {
                relative_path,
                message: "link or reparse point ignored".into(),
            });
            continue;
        }
        if metadata.is_dir() {
            match tokio::fs::read_dir(&path).await {
                Ok(dir) => stack.push(dir),
                Err(_) => {
                    report.complete = false;
                    report.errors.push(ScanIssue {
                        relative_path,
                        message: "directory unreadable".into(),
                    });
                }
            }
            continue;
        }
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !metadata.is_file() || !matches!(extension.as_str(), "mkv" | "mp4") {
            continue;
        }
        report.discovered += 1;
        let opened = match OpenedFile::open_in_root(&root, relative).await {
            Ok(f) => f,
            Err(_) => {
                report.errors.push(ScanIssue {
                    relative_path,
                    message: "file failed confinement validation".into(),
                });
                continue;
            }
        };
        let modified_ns = opened.modified_ns().to_owned();
        let file_size = opened.size();
        let probe = if let Some(cached) = cache
            .get(&relative_path)
            .filter(|c| c.size == file_size && c.modified_ns == modified_ns)
        {
            report.reused += 1;
            cached.probe.clone()
        } else {
            report.probed += 1;
            let probing = probe::probe(opened.canonical_path());
            tokio::pin!(probing);
            let result = loop {
                tokio::select! {
                    result = &mut probing => break Some(result),
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        if cancel.load(Ordering::Relaxed) { break None; }
                    }
                }
            };
            match result {
                None => {
                    report.complete = false;
                    break;
                }
                Some(Ok(probe)) => probe,
                // A failed adapter/process is not evidence that all media are corrupt.
                // Abort publication so an unavailable ffprobe cannot invalidate an inventory.
                Some(Err(probe::ProbeError::Io(error))) => return Err(error),
                Some(Err(error)) => {
                    report.errors.push(ScanIssue {
                        relative_path,
                        message: error.to_string(),
                    });
                    continue;
                }
            }
        };
        // Do not persist metadata paired with a file changed during its probe.
        if opened.verify_unchanged_sync().is_err()
            || file_size != metadata.len()
            || opened.modified() != metadata.modified().ok()
        {
            report.errors.push(ScanIssue {
                relative_path,
                message: "file changed during analysis; retry scan".into(),
            });
            continue;
        }
        let parent = relative.parent().unwrap_or(Path::new("")).to_path_buf();
        if !folder_cache.contains_key(&parent)
            && let Ok(videos) = FolderVideos::load(&root, &parent).await
        {
            folder_cache.insert(parent.clone(), videos);
        }
        let folder_videos = folder_cache.get(&parent);

        let (title, year, external_ids, identity_key) = identity(relative);
        let editorial = super::nfo::discover_with_folder(&root, relative, folder_videos).await;
        if let super::nfo::NfoOutcome::Invalid { path, message } = &editorial {
            report.warnings.push(ScanIssue {
                relative_path: path.clone(),
                message: message.clone(),
            });
        }
        let (poster, poster_warning) =
            super::images::discover_cached_with_folder(&root, relative, image_cache, folder_videos)
                .await;
        if let Some(message) = poster_warning {
            report.warnings.push(ScanIssue {
                relative_path: relative_path.clone(),
                message,
            });
        }
        report.files.push(ScannedFile {
            poster,
            editorial,
            relative_path,
            size: file_size,
            modified_ns,
            title,
            year,
            identity_key,
            external_ids,
            probe,
            mime: if extension == "mp4" {
                "video/mp4"
            } else {
                "video/x-matroska"
            }
            .into(),
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Probe;
    use std::time::UNIX_EPOCH;

    #[tokio::test]
    async fn traversal_refuses_directory_links_and_windows_junctions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir(&root).await.unwrap();
        tokio::fs::create_dir(&outside).await.unwrap();
        tokio::fs::write(outside.join("secret.mp4"), b"not media")
            .await
            .unwrap();
        let link = root.join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&outside)
                .output()
                .unwrap()
                .status
                .success()
        );
        let report = scan(&root, &HashMap::new(), &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(report.discovered, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(
            confined_file(&root, Path::new("escape/secret.mp4"))
                .await
                .is_err()
        );
        assert!(confined_file(&link, Path::new("secret.mp4")).await.is_err());
        assert!(
            scan(&link, &HashMap::new(), &AtomicBool::new(false))
                .await
                .is_err()
        );
    }
    #[test]
    fn qualified_identity_and_exact_folders_only() {
        let (title, year, ids, key) = identity(Path::new("Film (2024) [tmdb-123]/film-4k.mp4"));
        assert_eq!(title, "Film");
        assert_eq!(year, Some(2024));
        assert_eq!(ids["tmdb"], "123");
        assert_eq!(key, "tmdb:123");
        assert_ne!(
            identity(Path::new("A/Foo.mp4")).3,
            identity(Path::new("B/Foo.mp4")).3
        );
        assert!(metadata_name("Film [12345]").2.is_empty());
        assert_eq!(
            identity(Path::new("Film (2026)/Other Film (2025).mp4")).0,
            "Other Film"
        );
        assert_eq!(
            identity(Path::new("Film (2026) [tmdb-1]/Film [tmdb-2].mp4")).3,
            "tmdb:2"
        );
        assert_ne!(
            identity(Path::new("Film (2026)/Film (2025).mp4")).3,
            "folder:Film (2026)"
        );
        assert!(ignored("Film.trickplay"));
        assert!(ignored("movie.mkv.part"));
    }
    #[tokio::test]
    async fn cached_scan_skips_probe_and_temporary_files() {
        let temp = tempfile::tempdir().unwrap();
        let film = temp.path().join("Film (2024)");
        tokio::fs::create_dir(&film).await.unwrap();
        let path = film.join("film.mp4");
        tokio::fs::write(&path, b"cached synthetic media")
            .await
            .unwrap();
        tokio::fs::write(film.join("incomplete.mkv.part"), b"temporary")
            .await
            .unwrap();
        let m = tokio::fs::metadata(path).await.unwrap();
        let probe = Probe {
            streams: vec![],
            format: BTreeMap::new(),
        };
        let cache = HashMap::from([(
            "Film (2024)/film.mp4".into(),
            CachedFile {
                size: m.len(),
                modified_ns: m
                    .modified()
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string(),
                probe,
            },
        )]);
        tokio::fs::write(
            film.join("movie.nfo"),
            b"<movie><title>Editorial title</title></movie>",
        )
        .await
        .unwrap();
        let result = scan(temp.path(), &cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert!(result.complete);
        assert_eq!(result.discovered, 1);
        assert_eq!(result.reused, 1);
        assert_eq!(result.probed, 0);
        assert_eq!(result.files[0].title, "Film");
        assert!(
            matches!(&result.files[0].editorial, super::super::nfo::NfoOutcome::Valid { metadata, .. } if metadata.title.as_deref() == Some("Editorial title"))
        );
        tokio::fs::write(
            film.join("movie.nfo"),
            b"<movie><title>Changed</title></movie>",
        )
        .await
        .unwrap();
        let updated = scan(temp.path(), &cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(updated.probed, 0);
        assert!(
            matches!(&updated.files[0].editorial, super::super::nfo::NfoOutcome::Valid { metadata, .. } if metadata.title.as_deref() == Some("Changed"))
        );
        tokio::fs::remove_file(film.join("movie.nfo"))
            .await
            .unwrap();
        let removed = scan(temp.path(), &cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(removed.probed, 0);
        assert!(matches!(
            &removed.files[0].editorial,
            super::super::nfo::NfoOutcome::Missing
        ));
        assert_eq!(removed.files[0].identity_key, result.files[0].identity_key);
        let poster = film.join("poster.png");
        image::DynamicImage::new_rgb8(2, 3).save(&poster).unwrap();
        let with_image = scan(temp.path(), &cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(with_image.probed, 0);
        let image = with_image.files[0].poster.clone().unwrap();
        let image_cache = HashMap::from([(image.path.clone(), image.clone())]);
        let reused = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(
            reused.files[0].poster.as_ref().unwrap().modified_ns,
            image.modified_ns
        );
        image::DynamicImage::new_rgb8(5, 7).save(&poster).unwrap();
        let updated = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(updated.probed, 0);
        assert!(updated.files[0].poster.is_some());
        assert_ne!(
            updated.files[0].poster.as_ref().unwrap().size_bytes,
            image.size_bytes
        );
        tokio::fs::remove_file(&poster).await.unwrap();
        let deleted = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert_eq!(deleted.probed, 0);
        assert!(deleted.files[0].poster.is_none());
        assert_eq!(deleted.files[0].identity_key, result.files[0].identity_key);
        assert!(
            confined_file(temp.path(), Path::new("../outside.mp4"))
                .await
                .is_err()
        );
        let cancelled = scan(temp.path(), &cache, &AtomicBool::new(true))
            .await
            .unwrap();
        assert!(!cancelled.complete);
    }

    #[tokio::test]
    async fn dense_directory_scan_caches_folder_videos_and_reuses_sidecars() {
        let temp = tempfile::tempdir().unwrap();
        let dense = temp.path().join("Dense [tmdb-777]");
        tokio::fs::create_dir(&dense).await.unwrap();
        tokio::fs::write(
            dense.join("movie.nfo"),
            b"<movie><title>Dense Collection</title><year>2025</year></movie>",
        )
        .await
        .unwrap();
        let poster_path = dense.join("poster.png");
        image::DynamicImage::new_rgb8(2, 2)
            .save(&poster_path)
            .unwrap();

        let mut cache = HashMap::new();
        for i in 0..20 {
            let filename = format!("version-{i}.mp4");
            let file_path = dense.join(&filename);
            tokio::fs::write(&file_path, b"synthetic-video-payload")
                .await
                .unwrap();
            let meta = tokio::fs::metadata(&file_path).await.unwrap();
            let modified_ns = meta
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_string();
            cache.insert(
                format!("Dense [tmdb-777]/{filename}"),
                CachedFile {
                    size: meta.len(),
                    modified_ns,
                    probe: Probe {
                        streams: vec![],
                        format: BTreeMap::new(),
                    },
                },
            );
        }

        let first = scan(temp.path(), &cache, &AtomicBool::new(false))
            .await
            .unwrap();
        assert!(first.complete);
        assert_eq!(first.discovered, 20);
        assert_eq!(first.reused, 20);
        assert_eq!(first.probed, 0);
        assert!(first.errors.is_empty());
        assert!(first.warnings.is_empty());
        let poster = first.files[0].poster.clone().unwrap();
        let image_cache = HashMap::from([(poster.path.clone(), poster)]);

        let start = std::time::Instant::now();
        let second = scan_with_images(temp.path(), &cache, &image_cache, &AtomicBool::new(false))
            .await
            .unwrap();
        let elapsed = start.elapsed();
        assert!(second.complete);
        assert_eq!(second.discovered, 20);
        assert_eq!(second.reused, 20);
        assert_eq!(second.probed, 0);
        assert!(elapsed < Duration::from_secs(2));
    }
}
