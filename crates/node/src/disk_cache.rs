//! Size quota and crash-leftover cleanup for the Node's small flat on-disk caches (image previews,
//! audio fingerprints). Every entry is rebuildable from the library, so evicting is always safe;
//! only files directly inside the cache directory that this module's naming recognises are ever
//! touched, symbolic links are never followed, and nothing outside the cache directory is read
//! or written, in particular nothing next to the media.
use std::{
    fs, io,
    path::Path,
    time::{Duration, SystemTime},
};

/// Eviction stops once the cache is back under this share of its quota, so a full cache is not
/// swept again by the very next write.
const LOW_WATERMARK_PERCENT: u64 = 80;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sweep {
    pub removed_files: u64,
    pub removed_bytes: u64,
    pub kept_bytes: u64,
}

/// A byte budget from `variable` (decimal bytes), else `default`.
pub(crate) fn budget_from_env(variable: &str, default: u64) -> u64 {
    std::env::var(variable)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

/// Remove abandoned `*.tmp*` files older than `stale_after` (a writer crashed between write and
/// rename), then evict the oldest `*.{extension}` entries until the cache fits its quota.
pub(crate) fn enforce_quota(
    dir: &Path,
    extension: &str,
    max_bytes: u64,
    stale_after: Duration,
) -> io::Result<Sweep> {
    let mut sweep = Sweep::default();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(sweep),
        Err(error) => return Err(error),
    };
    let now = SystemTime::now();
    let mut kept: Vec<(SystemTime, u64, std::path::PathBuf)> = Vec::new();
    for entry in entries.filter_map(Result::ok).take(1_000_000) {
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if name.contains(".tmp") {
            let age = now.duration_since(modified).unwrap_or_default();
            if age >= stale_after && fs::remove_file(entry.path()).is_ok() {
                sweep.removed_files += 1;
                sweep.removed_bytes += metadata.len();
            }
        } else if Path::new(name.as_ref())
            .extension()
            .is_some_and(|found| found == extension)
        {
            kept.push((modified, metadata.len(), entry.path()));
        }
    }
    let mut total: u64 = kept.iter().map(|(_, len, _)| *len).sum();
    if total > max_bytes {
        kept.sort_by_key(|(modified, _, path)| (*modified, path.clone()));
        let target = (u128::from(max_bytes) * u128::from(LOW_WATERMARK_PERCENT) / 100) as u64;
        for (_, len, path) in &kept {
            if total <= target {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total -= len;
                sweep.removed_files += 1;
                sweep.removed_bytes += len;
            }
        }
    }
    sweep.kept_bytes = total;
    Ok(sweep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, len: usize, age_secs: u64) {
        let path = dir.join(name);
        fs::write(&path, vec![0u8; len]).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_secs);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn evicts_oldest_first_down_to_the_low_watermark_and_ignores_unowned_files() {
        let dir = tempfile::tempdir().unwrap();
        for (name, age) in [
            ("a.img", 400),
            ("b.img", 300),
            ("c.img", 200),
            ("d.img", 100),
        ] {
            write(dir.path(), name, 100, age);
        }
        write(dir.path(), "notes.txt", 5000, 9999);
        fs::create_dir(dir.path().join("sub.img")).unwrap();
        let sweep = enforce_quota(dir.path(), "img", 250, Duration::from_secs(60)).unwrap();
        // 400 bytes > 250: evict oldest until <= 200 (80% of 250).
        assert_eq!(sweep.removed_files, 2);
        assert_eq!(sweep.kept_bytes, 200);
        assert!(!dir.path().join("a.img").exists() && !dir.path().join("b.img").exists());
        assert!(dir.path().join("c.img").exists() && dir.path().join("d.img").exists());
        assert!(dir.path().join("notes.txt").exists() && dir.path().join("sub.img").is_dir());
        // Under quota: nothing more happens.
        let again = enforce_quota(dir.path(), "img", 250, Duration::from_secs(60)).unwrap();
        assert_eq!(again.removed_files, 0);
    }

    #[test]
    fn removes_only_stale_temporaries_and_tolerates_a_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "x.img.tmp.1", 10, 7200);
        write(dir.path(), "y.tmp", 10, 7200);
        write(dir.path(), "z.tmp", 10, 1);
        write(dir.path(), "keep.img", 10, 7200);
        let sweep = enforce_quota(dir.path(), "img", 1 << 20, Duration::from_secs(3600)).unwrap();
        assert_eq!(sweep.removed_files, 2);
        assert!(
            dir.path().join("z.tmp").exists(),
            "a fresh temporary may be in use"
        );
        assert!(dir.path().join("keep.img").exists());
        assert_eq!(
            enforce_quota(&dir.path().join("missing"), "img", 1, Duration::ZERO).unwrap(),
            Sweep::default()
        );
    }

    #[test]
    fn budget_falls_back_on_missing_or_invalid_values() {
        assert_eq!(budget_from_env("MEDIA_TEST_UNSET_BUDGET_VARIABLE", 7), 7);
    }
}
