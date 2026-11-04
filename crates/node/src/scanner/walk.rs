use super::*;
use crate::{
    files::{OpenedFile, is_linked},
    probe,
};
use media_contracts::inventory::{CachedFile, InventoryBatch, ScanIssue, ScanReport, ScannedFile};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) const MAX_FOLDER_CACHE: usize = 128;

pub(super) const MAX_SERIES_CACHE: usize = 128;

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
    image_cache: &HashMap<String, crate::images::LocalImage>,
    cancel: &AtomicBool,
) -> io::Result<ScanReport> {
    let mut files = Vec::new();
    let mut report = scan_streaming(
        root,
        "films",
        cache,
        image_cache,
        cancel,
        100,
        &[],
        |batch| {
            files.extend(batch.files);
            async { Ok(()) }
        },
    )
    .await?;
    report.files = files;
    crate::trim_memory();

    Ok(report)
}

/// Maximum diagnostic issues retained in memory in `ScanReport`.
/// All issues are streamed in batches and persisted into SQLite `scan_staging_issues`.
pub const MAX_REPORT_ISSUES: usize = 100;

#[inline]
pub(super) fn record_error(report: &mut ScanReport, batch: &mut Vec<ScanIssue>, issue: ScanIssue) {
    report.errors_count += 1;
    if report.errors.len() < MAX_REPORT_ISSUES {
        report.errors.push(issue.clone());
    }
    batch.push(issue);
}

#[inline]
pub(super) fn record_warning(
    report: &mut ScanReport,
    batch: &mut Vec<ScanIssue>,
    issue: ScanIssue,
) {
    report.warnings_count += 1;
    if report.warnings.len() < MAX_REPORT_ISSUES {
        report.warnings.push(issue.clone());
    }
    batch.push(issue);
}

/// Bounded batch-streaming discovery traversal yielding `InventoryBatch` chunks with backpressure.
/// Does not retain discovered files in RAM across batches, ensuring strictly bounded memory footprint.
#[allow(clippy::too_many_arguments)]
pub async fn scan_streaming<F, Fut>(
    root: &Path,
    kind: &str,
    cache: &HashMap<String, CachedFile>,
    image_cache: &HashMap<String, crate::images::LocalImage>,
    cancel: &AtomicBool,
    batch_size: usize,
    excluded_patterns: &[String],
    mut on_batch: F,
) -> io::Result<ScanReport>
where
    F: FnMut(InventoryBatch) -> Fut,
    Fut: std::future::Future<Output = Result<(), io::Error>>,
{
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
    let read_root = tokio::fs::read_dir(&root).await.map_err(|err| {
        if err.kind() == io::ErrorKind::PermissionDenied {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{err} : racine de bibliothèque inaccessible (vérifiez les droits de lecture et d'exécution +x hôte pour l'utilisateur UID 10001)"),
            )
        } else {
            err
        }
    })?;
    let mut stack = vec![read_root];
    let mut artwork_memo = ArtworkCache::new();
    let mut folder_cache: HashMap<PathBuf, FolderVideos> = HashMap::new();
    let mut series_cache: HashMap<
        PathBuf,
        std::sync::Arc<(crate::nfo::NfoOutcome, Option<crate::images::LocalImage>)>,
    > = HashMap::new();
    let mut report = ScanReport {
        complete: true,
        ..Default::default()
    };
    let mut current_batch_files = Vec::with_capacity(batch_size.max(1));
    let mut current_batch_errors = Vec::new();
    let mut current_batch_warnings = Vec::new();
    let mut sequence = 0u64;

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
                    let issue = ScanIssue {
                        relative_path: String::new(),
                        message: "directory traversal failed".into(),
                    };
                    record_error(&mut report, &mut current_batch_errors, issue);
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
        let name_str = name.to_string_lossy();
        if is_excluded(&relative_path, &name_str, excluded_patterns) {
            continue;
        }
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(_) => {
                report.complete = false;
                let issue = ScanIssue {
                    relative_path,
                    message: "file metadata unavailable".into(),
                };
                record_error(&mut report, &mut current_batch_errors, issue);
                continue;
            }
        };
        if is_linked(&metadata) {
            let issue = ScanIssue {
                relative_path,
                message: "link or reparse point ignored".into(),
            };
            record_error(&mut report, &mut current_batch_errors, issue);
            continue;
        }
        if metadata.is_dir() {
            if directory_is_ignored(&path) {
                continue;
            }
            match tokio::fs::read_dir(&path).await {
                Ok(dir) => stack.push(dir),
                Err(err) => {
                    report.complete = false;
                    let message = if err.kind() == io::ErrorKind::PermissionDenied {
                        "directory unreadable: permission denied (check Docker UID 10001 / file permissions)".into()
                    } else {
                        format!("directory unreadable: {err}")
                    };
                    let issue = ScanIssue {
                        relative_path,
                        message,
                    };
                    record_error(&mut report, &mut current_batch_errors, issue);
                }
            }
            continue;
        }
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !metadata.is_file()
            || !matches!(
                extension.as_str(),
                "mkv" | "mp4" | "webm" | "m4v" | "mov" | "obu"
            )
        {
            continue;
        }
        report.discovered += 1;
        let opened = match OpenedFile::open_in_root(&root, relative).await {
            Ok(f) => f,
            Err(_) => {
                let issue = ScanIssue {
                    relative_path,
                    message: "file failed confinement validation".into(),
                };
                record_error(&mut report, &mut current_batch_errors, issue);
                continue;
            }
        };
        let modified_ns = opened.modified_ns().to_owned();
        let file_size = opened.size();
        let (probe, reused) = if let Some(cached) = cache
            .get(&relative_path)
            .filter(|c| c.size == file_size && c.modified_ns == modified_ns)
        {
            report.reused += 1;
            (
                if cached.probe.streams.is_empty() {
                    media_contracts::media::Probe::default()
                } else {
                    cached.probe.clone()
                },
                true,
            )
        } else {
            report.probed += 1;
            let probing = probe::probe_opened(&opened);
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
                Some(Ok(probe)) => (probe, false),
                // A failed adapter/process is not evidence that all media are corrupt.
                // Abort publication so an unavailable ffprobe cannot invalidate an inventory.
                Some(Err(probe::ProbeError::Io(error))) => return Err(error),
                Some(Err(error)) => {
                    let issue = ScanIssue {
                        relative_path,
                        message: error.to_string(),
                    };
                    record_error(&mut report, &mut current_batch_errors, issue);
                    continue;
                }
            }
        };
        // Do not persist metadata paired with a file changed during its probe.
        if opened.verify_unchanged_sync().is_err()
            || file_size != metadata.len()
            || opened.modified() != metadata.modified().ok()
        {
            let issue = ScanIssue {
                relative_path,
                message: "file changed during analysis; retry scan".into(),
            };
            record_error(&mut report, &mut current_batch_errors, issue);
            continue;
        }
        let parent = relative.parent().unwrap_or(Path::new("")).to_path_buf();
        if !folder_cache.contains_key(&parent)
            && let Ok(videos) = FolderVideos::load(&root, &parent).await
        {
            if folder_cache.len() >= MAX_FOLDER_CACHE
                && let Some(first_key) = folder_cache.keys().next().cloned()
            {
                folder_cache.remove(&first_key);
            }
            folder_cache.insert(parent.clone(), videos);
        }
        let folder_videos = folder_cache.get(&parent);

        // Sidecars must be refreshed even when the video probe is reused.
        let (subtitles, subtitle_issues) =
            crate::subtitles::discover_with_folder(&root, relative, folder_videos).await;
        for issue in subtitle_issues {
            record_warning(&mut report, &mut current_batch_warnings, issue);
        }
        let (audios, audio_issues) =
            crate::external_audio::discover(&root, relative, folder_videos).await;
        for issue in audio_issues {
            record_warning(&mut report, &mut current_batch_warnings, issue);
        }

        let mut artwork_directory = relative.parent().unwrap_or(Path::new("")).to_path_buf();
        let scanned_file = if kind == "series" {
            let Some(parsed) = parse_series_episode(relative) else {
                let issue = ScanIssue {
                    relative_path: relative_path.clone(),
                    message: "unrecognized series file; expected S01E01, 1x01, or Season folder"
                        .into(),
                };
                record_warning(&mut report, &mut current_batch_warnings, issue);
                continue;
            };

            artwork_directory = parsed.series_dir.clone();
            if !series_cache.contains_key(&parsed.series_dir) {
                if series_cache.len() >= MAX_SERIES_CACHE
                    && let Some(first_key) = series_cache.keys().next().cloned()
                {
                    series_cache.remove(&first_key);
                }
                let series_nfo = crate::nfo::discover_series_nfo(&root, &parsed.series_dir).await;
                if let crate::nfo::NfoOutcome::Invalid { path, message } = &series_nfo {
                    let issue = ScanIssue {
                        relative_path: path.clone(),
                        message: message.clone(),
                    };
                    record_warning(&mut report, &mut current_batch_warnings, issue);
                }
                let (series_poster, poster_warning) =
                    crate::images::discover_series_poster(&root, &parsed.series_dir, image_cache)
                        .await;
                if let Some(message) = poster_warning {
                    let issue = ScanIssue {
                        relative_path: parsed.series_dir.to_string_lossy().replace('\\', "/"),
                        message,
                    };
                    record_warning(&mut report, &mut current_batch_warnings, issue);
                }
                series_cache.insert(
                    parsed.series_dir.clone(),
                    std::sync::Arc::new((series_nfo, series_poster)),
                );
            }

            let entry = series_cache.get(&parsed.series_dir).cloned().unwrap();
            let (series_title, series_year, series_external_ids) = match &entry.0 {
                crate::nfo::NfoOutcome::Valid { metadata, .. } => (
                    metadata.title.clone().unwrap_or(parsed.series_raw_title),
                    metadata.year.or(parsed.series_year),
                    if !metadata.external_ids.is_empty() {
                        metadata.external_ids.clone()
                    } else {
                        parsed.series_external_ids
                    },
                ),
                _ => (
                    parsed.series_raw_title,
                    parsed.series_year,
                    parsed.series_external_ids,
                ),
            };

            let episode_editorial = crate::nfo::discover_episode_nfo(&root, relative).await;
            if let crate::nfo::NfoOutcome::Invalid { path, message } = &episode_editorial {
                let issue = ScanIssue {
                    relative_path: path.clone(),
                    message: message.clone(),
                };
                record_warning(&mut report, &mut current_batch_warnings, issue);
            }

            let (episode_title, episode_year) = match &episode_editorial {
                crate::nfo::NfoOutcome::Valid { metadata, .. } => (
                    metadata.title.clone().unwrap_or(parsed.episode_title),
                    metadata.year.or(series_year),
                ),
                _ => (parsed.episode_title, series_year),
            };

            let episode_identity_key = format!(
                "episode:{}:s{:02}e{:02}",
                parsed.series_identity_key, parsed.season_number, parsed.episode_number
            );

            ScannedFile {
                audios,
                subtitles,
                poster: None,
                editorial: episode_editorial,
                relative_path,
                size: file_size,
                modified_ns,
                title: episode_title,
                year: episode_year,
                identity_key: episode_identity_key,
                external_ids: BTreeMap::new(),
                probe,
                mime: match extension.as_str() {
                    "mp4" | "m4v" | "mov" => "video/mp4",
                    "webm" => "video/webm",
                    "obu" => "video/mp4",
                    _ => "video/x-matroska",
                }
                .into(),
                reused,
                series_identity_key: Some(parsed.series_identity_key),
                series_title: Some(series_title),
                series_year,
                series_external_ids: Some(series_external_ids),
                series_editorial: Some(entry.0.clone()),
                artwork: Default::default(),
                series_poster: entry.1.clone(),
                season_number: Some(parsed.season_number),
                episode_number: Some(parsed.episode_number),
            }
        } else {
            let (title, year, external_ids, identity_key) = identity(relative);
            let editorial = crate::nfo::discover_with_folder(&root, relative, folder_videos).await;
            if let crate::nfo::NfoOutcome::Invalid { path, message } = &editorial {
                let issue = ScanIssue {
                    relative_path: path.clone(),
                    message: message.clone(),
                };
                record_warning(&mut report, &mut current_batch_warnings, issue);
            }
            let (poster, poster_warning) = crate::images::discover_cached_with_folder(
                &root,
                relative,
                image_cache,
                folder_videos,
            )
            .await;
            if let Some(message) = poster_warning {
                let issue = ScanIssue {
                    relative_path: relative_path.clone(),
                    message,
                };
                record_warning(&mut report, &mut current_batch_warnings, issue);
            }
            ScannedFile {
                audios,
                subtitles,
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
                mime: match extension.as_str() {
                    "mp4" | "m4v" | "mov" => "video/mp4",
                    "webm" => "video/webm",
                    "obu" => "video/mp4",
                    _ => "video/x-matroska",
                }
                .into(),
                reused,
                series_identity_key: None,
                series_title: None,
                series_year: None,
                series_external_ids: None,
                series_editorial: None,
                artwork: Default::default(),
                series_poster: None,
                season_number: None,
                episode_number: None,
            }
        };
        let mut scanned_file = scanned_file;
        let parent = relative.parent().unwrap_or(Path::new(""));
        let directory = artwork_directory.as_path();
        let is_series = scanned_file.series_identity_key.is_some();
        let shared_artwork = is_series
            || folder_videos
                .is_some_and(|folder| crate::images::shared_allowed_from_folder(relative, folder));
        let exact_artwork = is_series
            || folder_videos.is_some_and(|folder| {
                folder.stem_unambiguous(relative.file_stem(), &scanned_file.identity_key)
            });
        let artwork_stem = relative.file_stem().unwrap_or_default().to_string_lossy();
        for (role, names) in [
            ("backdrop", vec!["backdrop", "fanart", "background", "art"]),
            ("logo", vec!["logo", "clearlogo"]),
            ("landscape", vec!["landscape"]),
            ("banner", vec!["banner"]),
            ("disc", vec!["disc", "discart"]),
            ("thumb", vec!["thumb"]),
            ("illustration", vec!["clearart"]),
        ] {
            if !shared_artwork && !exact_artwork {
                continue;
            }
            let exact_names: Vec<String> = names
                .iter()
                .map(|name| format!("{artwork_stem}-{name}"))
                .collect();
            let candidates: Vec<&str> = names
                .iter()
                .copied()
                .filter(|_| shared_artwork)
                .chain(
                    exact_names
                        .iter()
                        .map(String::as_str)
                        .filter(|_| exact_artwork),
                )
                .collect();
            let memo_key = if shared_artwork {
                role.to_owned()
            } else {
                format!("{}:{role}", scanned_file.identity_key)
            };
            let (image, warning) = artwork_cached(
                &root,
                directory,
                image_cache,
                &mut artwork_memo,
                &memo_key,
                &candidates,
            )
            .await;
            if let Some(image) = image {
                let key = if scanned_file.series_identity_key.is_some() {
                    format!("series/{role}")
                } else {
                    role.into()
                };
                scanned_file.artwork.insert(key, image);
            }
            if let Some(message) = warning {
                record_warning(
                    &mut report,
                    &mut current_batch_warnings,
                    ScanIssue {
                        relative_path: relative.to_string_lossy().into_owned(),
                        message,
                    },
                );
            }
        }
        let stem = relative.file_stem().unwrap_or_default().to_string_lossy();
        let thumb = format!("{stem}-thumb");
        if exact_artwork
            && let (Some(image), _) =
                crate::images::discover_named(&root, parent, image_cache, &[&thumb]).await
        {
            scanned_file.artwork.insert("thumb".into(), image);
        }
        if let Some(number) = scanned_file.season_number {
            let name = if number == 0 {
                "season-specials-poster".into()
            } else {
                format!("season{number:02}-poster")
            };
            if let (Some(image), _) = artwork_cached(
                &root,
                directory,
                image_cache,
                &mut artwork_memo,
                &name,
                &[&name],
            )
            .await
            {
                scanned_file
                    .artwork
                    .insert(format!("series/season{number}"), image);
            }
        }
        current_batch_files.push(scanned_file);

        if current_batch_files.len() >= batch_size
            || current_batch_files
                .iter()
                .flat_map(|f| &f.subtitles)
                .map(|s| s.webvtt.len())
                .sum::<usize>()
                >= 512 * 1024
        {
            sequence += 1;
            let batch = InventoryBatch {
                scan_id: media_contracts::ids::ScanId::from(format!("seq-{sequence}")),
                sequence,
                files: std::mem::take(&mut current_batch_files),
                errors: std::mem::take(&mut current_batch_errors),
                warnings: std::mem::take(&mut current_batch_warnings),
            };
            on_batch(batch).await?;
        }
    }

    if !current_batch_files.is_empty()
        || !current_batch_errors.is_empty()
        || !current_batch_warnings.is_empty()
    {
        sequence += 1;
        let batch = InventoryBatch {
            scan_id: media_contracts::ids::ScanId::from(format!("seq-{sequence}")),
            sequence,
            files: current_batch_files,
            errors: current_batch_errors,
            warnings: current_batch_warnings,
        };
        on_batch(batch).await?;
    }

    drop(folder_cache);
    drop(series_cache);
    drop(artwork_memo);
    crate::trim_memory();

    Ok(report)
}
