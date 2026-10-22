//! Local Media Node discovery. No writes, database access, or HTTP concerns.
pub use crate::files::confined_file;
use crate::{
    files::{OpenedFile, is_linked},
    probe,
};
pub use media_contracts::inventory::{
    CachedFile, InventoryBatch, ScanIssue, ScanReport, ScannedFile,
};
use std::{
    collections::{BTreeMap, HashMap},
    ffi::{OsStr, OsString},
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

type ArtworkCache = HashMap<(PathBuf, String), (Option<super::images::LocalImage>, Option<String>)>;
async fn artwork_cached(
    root: &Path,
    dir: &Path,
    cache: &HashMap<String, super::images::LocalImage>,
    memo: &mut ArtworkCache,
    role: &str,
    names: &[&str],
) -> (Option<super::images::LocalImage>, Option<String>) {
    let key = (dir.to_path_buf(), role.to_owned());
    if let Some(found) = memo.get(&key) {
        return found.clone();
    }
    let found = super::images::discover_named(root, dir, cache, names).await;
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
                    "mkv", "mp4", "webm", "opus", "ogg", "srt", "vtt", "ass", "ssa",
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

pub(crate) fn ignored(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with('@')
        || name.starts_with("#recycle")
        || name.starts_with("#snapshot")
        || name.ends_with(".trickplay")
        || name.ends_with(".metathumb")
        || matches!(
            name.as_str(),
            "extrafanart"
                | "$recycle.bin"
                | "system volume information"
                | "lost+found"
                | "#recycle"
                | "#snapshot"
                | ".@__thumb"
                | "thumbs.db"
                | "desktop.ini"
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

pub(crate) fn is_excluded(relative_path: &str, name: &str, excluded_patterns: &[String]) -> bool {
    if ignored(name) {
        return true;
    }
    let lower_name = name.to_ascii_lowercase();
    let lower_rel = relative_path.to_ascii_lowercase();
    for pat in excluded_patterns {
        let pat = pat.trim().to_ascii_lowercase();
        if pat.is_empty() {
            continue;
        }
        if lower_name == pat || lower_rel.split('/').any(|seg| seg == pat) {
            return true;
        }
    }
    false
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSeriesEpisode {
    pub series_dir: PathBuf,
    pub series_raw_title: String,
    pub series_year: Option<u16>,
    pub series_external_ids: BTreeMap<String, String>,
    pub series_identity_key: String,
    pub season_number: u32,
    pub episode_number: u32,
    pub episode_title: String,
}

fn extract_s_e_pattern(stem: &str) -> Option<(u32, u32, Option<String>)> {
    let bytes = stem.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if (bytes[i] == b's' || bytes[i] == b'S') && i + 1 < len && bytes[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(s_val) = stem[i + 1..j].parse::<u32>() {
                let mut k = j;
                while k < len && matches!(bytes[k], b'.' | b' ' | b'_' | b'-') {
                    k += 1;
                }
                if k < len
                    && (bytes[k] == b'e' || bytes[k] == b'E')
                    && k + 1 < len
                    && bytes[k + 1].is_ascii_digit()
                {
                    let mut m = k + 1;
                    while m < len && bytes[m].is_ascii_digit() {
                        m += 1;
                    }
                    if let Ok(e_val) = stem[k + 1..m].parse::<u32>() {
                        // Check if multi-episode suffix like -E02 or E02
                        let mut after = m;
                        if after < len
                            && (bytes[after] == b'-'
                                || bytes[after] == b'e'
                                || bytes[after] == b'E')
                        {
                            if bytes[after] == b'-'
                                && after + 1 < len
                                && (bytes[after + 1] == b'e' || bytes[after + 1] == b'E')
                            {
                                after += 2;
                                while after < len && bytes[after].is_ascii_digit() {
                                    after += 1;
                                }
                            } else if bytes[after] == b'e' || bytes[after] == b'E' {
                                after += 1;
                                while after < len && bytes[after].is_ascii_digit() {
                                    after += 1;
                                }
                            }
                        }
                        let rem = stem[after..].trim();
                        let title = if let Some(stripped) = rem.strip_prefix('-') {
                            let t = stripped.trim().trim_matches(['.', '_', '-']).trim();
                            if !t.is_empty() {
                                Some(t.to_string())
                            } else {
                                None
                            }
                        } else if !rem.is_empty() {
                            let t = rem.trim_matches(['.', '_', '-']).trim();
                            if !t.is_empty() {
                                Some(t.to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        return Some((s_val, e_val, title));
                    }
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

fn extract_x_pattern(stem: &str) -> Option<(u32, u32, Option<String>)> {
    let bytes = stem.as_bytes();
    let len = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        if (b == b'x' || b == b'X') && i > 0 && i + 1 < len {
            let mut start = i;
            while start > 0 && bytes[start - 1].is_ascii_digit() {
                start -= 1;
            }
            if start == i {
                continue;
            }
            if start > 0 && !matches!(bytes[start - 1], b'.' | b' ' | b'_' | b'-' | b'[' | b'(') {
                continue;
            }
            let Ok(s_val) = stem[start..i].parse::<u32>() else {
                continue;
            };

            let mut end = i + 1;
            while end < len && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end == i + 1 {
                continue;
            }
            if end < len && !matches!(bytes[end], b'.' | b' ' | b'_' | b'-' | b']' | b')') {
                continue;
            }
            let Ok(e_val) = stem[i + 1..end].parse::<u32>() else {
                continue;
            };

            let rem = stem[end..].trim();
            let title = if let Some(stripped) = rem.strip_prefix('-') {
                let t = stripped.trim().trim_matches(['.', '_', '-']).trim();
                if !t.is_empty() {
                    Some(t.to_string())
                } else {
                    None
                }
            } else if !rem.is_empty() {
                let t = rem.trim_matches(['.', '_', '-']).trim();
                if !t.is_empty() {
                    Some(t.to_string())
                } else {
                    None
                }
            } else {
                None
            };
            return Some((s_val, e_val, title));
        }
    }
    None
}

fn extract_season_from_folder(parent: &str) -> Option<u32> {
    let lower = parent.to_ascii_lowercase();
    let lower = lower.trim();
    if lower == "specials"
        || lower == "special"
        || lower == "season 0"
        || lower == "saison 0"
        || lower == "s0"
        || lower == "s00"
        || lower == "hors-série"
        || lower == "hors série"
    {
        return Some(0);
    }
    for prefix in ["season", "saison", "staffel"] {
        if let Some(rem) = lower.strip_prefix(prefix) {
            let digits: String = rem.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(num) = digits.parse::<u32>() {
                return Some(num);
            }
        }
    }
    if let Some(rem) = lower.strip_prefix('s') {
        let digits: String = rem.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = digits.parse::<u32>() {
            return Some(num);
        }
    }
    None
}

fn extract_episode_from_filename(stem: &str) -> Option<(u32, Option<String>)> {
    let lower = stem.to_ascii_lowercase();
    for prefix in ["episode", "ep", "e"] {
        if let Some(idx) = lower.find(prefix) {
            let after = &lower[idx + prefix.len()..];
            let after_trimmed = after.trim_start_matches(['.', ' ', '_', '-']);
            let digits: String = after_trimmed
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(num) = digits.parse::<u32>() {
                let rem_idx =
                    idx + prefix.len() + (after.len() - after_trimmed.len()) + digits.len();
                let rem = stem[rem_idx..].trim();
                let title = if let Some(stripped) = rem.strip_prefix('-') {
                    let t = stripped.trim().trim_matches(['.', '_', '-']).trim();
                    if !t.is_empty() {
                        Some(t.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                };
                return Some((num, title));
            }
        }
    }
    let trimmed = stem.trim();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty()
        && digits.len() <= 4
        && let Ok(num) = digits.parse::<u32>()
    {
        let rem = trimmed[digits.len()..].trim();
        let title = if let Some(stripped) = rem.strip_prefix('-') {
            let t = stripped.trim().trim_matches(['.', '_', '-']).trim();
            if !t.is_empty() {
                Some(t.to_string())
            } else {
                None
            }
        } else {
            None
        };
        return Some((num, title));
    }
    None
}

pub fn parse_series_episode(relative: &Path) -> Option<ParsedSeriesEpisode> {
    let mut components = Vec::new();
    for c in relative.components() {
        if let Component::Normal(s) = c
            && let Some(str_val) = s.to_str()
        {
            components.push(str_val);
        }
    }
    if components.is_empty() {
        return None;
    }

    let (series_dir, series_folder_name) = if components.len() >= 2 {
        (PathBuf::from(components[0]), components[0])
    } else {
        (PathBuf::new(), components[0])
    };

    let (series_raw_title, series_year, series_external_ids) = metadata_name(series_folder_name);
    let series_identity_key = ["tvdb", "tmdb", "imdb"]
        .iter()
        .find_map(|p| series_external_ids.get(*p).map(|v| format!("{p}:{v}")))
        .unwrap_or_else(|| {
            if series_dir.as_os_str().is_empty() {
                format!("series:{}", series_raw_title)
            } else {
                format!("folder:{}", series_dir.to_string_lossy().replace('\\', "/"))
            }
        });

    let stem = relative.file_stem().and_then(|s| s.to_str())?;
    let parent_name = relative
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // 1. Pattern S(\d+)E(\d+)
    if let Some((s, e, remaining_title)) = extract_s_e_pattern(stem) {
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    // 2. Pattern (\d+)x(\d+)
    if let Some((s, e, remaining_title)) = extract_x_pattern(stem) {
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    // 3. Parent folder provides season, filename provides episode
    let season_from_parent = extract_season_from_folder(parent_name);
    if let Some(s) = season_from_parent
        && let Some((e, remaining_title)) = extract_episode_from_filename(stem)
    {
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    None
}

const MAX_FOLDER_CACHE: usize = 128;
const MAX_SERIES_CACHE: usize = 128;

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
    Ok(report)
}

/// Maximum diagnostic issues retained in memory in `ScanReport`.
/// All issues are streamed in batches and persisted into SQLite `scan_staging_issues`.
pub const MAX_REPORT_ISSUES: usize = 100;

#[inline]
fn record_error(report: &mut ScanReport, batch: &mut Vec<ScanIssue>, issue: ScanIssue) {
    report.errors_count += 1;
    if report.errors.len() < MAX_REPORT_ISSUES {
        report.errors.push(issue.clone());
    }
    batch.push(issue);
}

#[inline]
fn record_warning(report: &mut ScanReport, batch: &mut Vec<ScanIssue>, issue: ScanIssue) {
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
    image_cache: &HashMap<String, super::images::LocalImage>,
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
        std::sync::Arc<(super::nfo::NfoOutcome, Option<super::images::LocalImage>)>,
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
            if path.join(".nomedia").exists()
                || path.join(".ignore").exists()
                || path.join(".aetherignore").exists()
            {
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
        if metadata.is_file() && extension == "obu" {
            report.discovered += 1;
            record_error(&mut report, &mut current_batch_errors, ScanIssue {
                relative_path,
                message: "raw AV1 OBU playback unavailable: a validated temporal index, timestamps and Web packaging are required; no frame rate is inferred".into(),
            });
            continue;
        }
        if !metadata.is_file() || !matches!(extension.as_str(), "mkv" | "mp4" | "webm") {
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
            super::subtitles::discover_with_folder(&root, relative, folder_videos).await;
        for issue in subtitle_issues {
            record_warning(&mut report, &mut current_batch_warnings, issue);
        }
        let (audios, audio_issues) =
            super::external_audio::discover(&root, relative, folder_videos).await;
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
                let series_nfo = super::nfo::discover_series_nfo(&root, &parsed.series_dir).await;
                if let super::nfo::NfoOutcome::Invalid { path, message } = &series_nfo {
                    let issue = ScanIssue {
                        relative_path: path.clone(),
                        message: message.clone(),
                    };
                    record_warning(&mut report, &mut current_batch_warnings, issue);
                }
                let (series_poster, poster_warning) =
                    super::images::discover_series_poster(&root, &parsed.series_dir, image_cache)
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
                super::nfo::NfoOutcome::Valid { metadata, .. } => (
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

            let episode_editorial = super::nfo::discover_episode_nfo(&root, relative).await;
            if let super::nfo::NfoOutcome::Invalid { path, message } = &episode_editorial {
                let issue = ScanIssue {
                    relative_path: path.clone(),
                    message: message.clone(),
                };
                record_warning(&mut report, &mut current_batch_warnings, issue);
            }

            let (episode_title, episode_year) = match &episode_editorial {
                super::nfo::NfoOutcome::Valid { metadata, .. } => (
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
                    "mp4" => "video/mp4",
                    "webm" => "video/webm",
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
            let editorial = super::nfo::discover_with_folder(&root, relative, folder_videos).await;
            if let super::nfo::NfoOutcome::Invalid { path, message } = &editorial {
                let issue = ScanIssue {
                    relative_path: path.clone(),
                    message: message.clone(),
                };
                record_warning(&mut report, &mut current_batch_warnings, issue);
            }
            let (poster, poster_warning) = super::images::discover_cached_with_folder(
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
                    "mp4" => "video/mp4",
                    "webm" => "video/webm",
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
                .is_some_and(|folder| super::images::shared_allowed_from_folder(relative, folder));
        let exact_artwork = is_series
            || folder_videos.is_some_and(|folder| {
                folder.stem_unambiguous(relative.file_stem(), &scanned_file.identity_key)
            });
        let artwork_stem = relative.file_stem().unwrap_or_default().to_string_lossy();
        for (role, names) in [
            ("backdrop", vec!["backdrop", "fanart"]),
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
            let candidates: Vec<&str> = if shared_artwork {
                names
            } else {
                exact_names.iter().map(String::as_str).collect()
            };
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
                super::images::discover_named(&root, parent, image_cache, &[&thumb]).await
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

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Probe;
    use std::time::UNIX_EPOCH;

    #[tokio::test]
    async fn artwork_is_never_shared_between_ambiguous_films() {
        let root = tempfile::tempdir().unwrap();
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated/p1");
        for name in ["a.mp4", "b.mp4"] {
            tokio::fs::copy(fixtures.join("clip-mp4-fr.mp4"), root.path().join(name))
                .await
                .unwrap();
        }
        let bytes = {
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(2, 3)
                .write_to(&mut out, image::ImageFormat::Png)
                .unwrap();
            out.into_inner()
        };
        for name in ["backdrop.png", "a-backdrop.png"] {
            tokio::fs::write(root.path().join(name), &bytes)
                .await
                .unwrap();
        }
        let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(false))
            .await
            .unwrap();
        let a = report
            .files
            .iter()
            .find(|f| f.relative_path == "a.mp4")
            .unwrap();
        let b = report
            .files
            .iter()
            .find(|f| f.relative_path == "b.mp4")
            .unwrap();
        assert_eq!(a.artwork["backdrop"].path, "a-backdrop.png");
        assert!(b.artwork.is_empty());
        tokio::fs::copy(fixtures.join("clip-mkv-fr.mkv"), root.path().join("a.mkv"))
            .await
            .unwrap();
        let report = scan(root.path(), &HashMap::new(), &AtomicBool::new(false))
            .await
            .unwrap();
        assert!(
            report
                .files
                .iter()
                .filter(|f| f.relative_path.starts_with("a."))
                .all(|f| f.artwork.is_empty())
        );
    }

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
            chapters: Vec::new(),
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
                        chapters: Vec::new(),
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

    #[tokio::test]
    async fn scanner_ignores_nas_system_folders_nomedia_and_custom_exclusions() {
        let temp = tempfile::tempdir().unwrap();
        // 1. Valid video
        let valid_dir = temp.path().join("Valid (2024)");
        tokio::fs::create_dir(&valid_dir).await.unwrap();
        let path = valid_dir.join("valid.mp4");
        tokio::fs::write(&path, b"synthetic media").await.unwrap();
        let m = tokio::fs::metadata(&path).await.unwrap();
        let cache = HashMap::from([(
            "Valid (2024)/valid.mp4".into(),
            CachedFile {
                size: m.len(),
                modified_ns: m
                    .modified()
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string(),
                probe: Probe::default(),
            },
        )]);

        // 2. Synology #recycle folder
        let recycle = temp.path().join("#recycle");
        tokio::fs::create_dir(&recycle).await.unwrap();
        tokio::fs::write(recycle.join("deleted.mp4"), b"deleted media")
            .await
            .unwrap();

        // 3. Synology @eaDir folder
        let eadir = valid_dir.join("@eaDir");
        tokio::fs::create_dir(&eadir).await.unwrap();
        tokio::fs::write(eadir.join("thumb.mp4"), b"thumb media")
            .await
            .unwrap();

        // 4. Directory with .nomedia file
        let nomedia_dir = temp.path().join("IgnoredFolder");
        tokio::fs::create_dir(&nomedia_dir).await.unwrap();
        tokio::fs::write(nomedia_dir.join(".nomedia"), b"")
            .await
            .unwrap();
        tokio::fs::write(nomedia_dir.join("unwanted.mp4"), b"unwanted")
            .await
            .unwrap();

        // 5. Custom excluded folder
        let bonus_dir = temp.path().join("Bonus");
        tokio::fs::create_dir(&bonus_dir).await.unwrap();
        tokio::fs::write(bonus_dir.join("featurette.mp4"), b"bonus")
            .await
            .unwrap();

        let excluded = vec!["bonus".to_string()];
        let mut files = Vec::new();
        let report = scan_streaming(
            temp.path(),
            "films",
            &cache,
            &HashMap::new(),
            &AtomicBool::new(false),
            100,
            &excluded,
            |batch| {
                files.extend(batch.files);
                async { Ok(()) }
            },
        )
        .await
        .unwrap();

        assert_eq!(report.discovered, 1);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].title, "Valid");
    }
}
