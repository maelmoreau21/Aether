//! Fast, *partial* content fingerprint used only to recognise a file that was renamed or moved.
//!
//! The fingerprint hashes the file size plus a few fixed-size samples (head, middle, tail, and two
//! more on very large files) with BLAKE3. It reads at most `5 * SAMPLE_BYTES` per file, whatever the
//! file size, which keeps the cost bounded on HDD and network shares (a handful of seeks).
//!
//! Hard limits, enforced by the format and by the Core:
//! - It is **not** a cryptographic identity of the whole file. Two different files can share the
//!   sampled bytes and the size (for example two encodes with identical container headers and
//!   trailers), and a file modified between the samples keeps its fingerprint. Callers must treat
//!   a match as a *candidate* and corroborate it (unique on both sides, same library, consistent
//!   external identifiers, same modification time or file name) before acting on it.
//! - Small files (at most `3 * SAMPLE_BYTES`) are hashed whole and carry the `b3f1` tag; larger files
//!   carry `b3s1`. The tag is part of the value, so the two never compare equal.
//! - Reading goes through the confined [`OpenedFile`] handle in read-only mode and the result is
//!   discarded if the file changed while it was being read.
use crate::files::OpenedFile;
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

/// Bytes read per sample.
pub const SAMPLE_BYTES: u64 = 64 * 1024;
/// Files at least this large get two additional samples (quarter and three quarters).
const LARGE_FILE_BYTES: u64 = 1 << 30;
const DOMAIN: &[u8] = b"aether-content-fingerprint-v1";
/// Hex characters kept from the 256-bit BLAKE3 output (128 bits).
const HEX_LEN: usize = 32;

/// Offsets (all of length [`SAMPLE_BYTES`]) sampled for a file of `size` bytes, ascending and distinct.
/// Empty when the whole file is hashed instead.
fn sample_offsets(size: u64) -> Vec<u64> {
    if size <= 3 * SAMPLE_BYTES {
        return Vec::new();
    }
    let last = size - SAMPLE_BYTES;
    let mut offsets = vec![0, last / 2, last];
    if size >= LARGE_FILE_BYTES {
        offsets.extend([last / 4, last / 4 * 3]);
    }
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

/// Fingerprint of `reader`, a seekable stream of exactly `size` bytes.
pub fn fingerprint_reader<R: Read + Seek>(reader: &mut R, size: u64) -> io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN);
    hasher.update(&size.to_le_bytes());
    let offsets = sample_offsets(size);
    let tag = if offsets.is_empty() {
        reader.seek(SeekFrom::Start(0))?;
        let mut remaining = size;
        let mut buffer = vec![0u8; SAMPLE_BYTES as usize];
        while remaining > 0 {
            let take = remaining.min(SAMPLE_BYTES) as usize;
            reader.read_exact(&mut buffer[..take])?;
            hasher.update(&buffer[..take]);
            remaining -= take as u64;
        }
        "b3f1"
    } else {
        let mut buffer = vec![0u8; SAMPLE_BYTES as usize];
        for offset in offsets {
            reader.seek(SeekFrom::Start(offset))?;
            reader.read_exact(&mut buffer)?;
            hasher.update(&offset.to_le_bytes());
            hasher.update(&buffer);
        }
        "b3s1"
    };
    let hex = hasher.finalize().to_hex();
    Ok(format!("{tag}:{size}:{}", &hex.as_str()[..HEX_LEN]))
}

fn fingerprint_file(mut file: File, size: u64) -> io::Result<String> {
    let value = fingerprint_reader(&mut file, size)?;
    // The handle is a duplicate sharing its cursor with the scan's handle; leave it where
    // every other consumer expects it.
    file.seek(SeekFrom::Start(0))?;
    Ok(value)
}

/// Fingerprint of an opened library file. `None` when the file cannot be read or changed
/// while it was sampled: a missing hint is always safe, a wrong one is not.
pub async fn quick_fingerprint(opened: &OpenedFile) -> Option<String> {
    let size = opened.size();
    let file = opened.try_clone_std().ok()?;
    let value = tokio::task::spawn_blocking(move || fingerprint_file(file, size))
        .await
        .ok()?
        .ok()?;
    opened.verify_unchanged_sync().ok()?;
    Some(value)
}

/// Whether `value` has the shape produced by this module (cache and wire sanity check).
pub fn is_well_formed(value: &str) -> bool {
    let mut parts = value.split(':');
    matches!(parts.next(), Some("b3f1" | "b3s1"))
        && parts
            .next()
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        && parts
            .next()
            .is_some_and(|h| h.len() == HEX_LEN && h.bytes().all(|b| b.is_ascii_hexdigit()))
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn pattern(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }
    fn fp(bytes: &[u8]) -> String {
        fingerprint_reader(&mut Cursor::new(bytes), bytes.len() as u64).unwrap()
    }

    #[test]
    fn deterministic_well_formed_and_tagged_by_scheme() {
        let small = pattern(1000, 1);
        let large = pattern(4 * SAMPLE_BYTES as usize, 1);
        assert_eq!(fp(&small), fp(&small));
        assert!(fp(&small).starts_with("b3f1:1000:"));
        assert!(fp(&large).starts_with("b3s1:262144:"));
        assert!(is_well_formed(&fp(&small)) && is_well_formed(&fp(&large)));
        assert!(!is_well_formed("b3s1:12:zz") && !is_well_formed("sha256:1:ab"));
        assert!(is_well_formed(&fp(&[])));
    }

    #[test]
    fn size_and_sampled_bytes_distinguish_files() {
        let a = pattern(4 * SAMPLE_BYTES as usize, 1);
        let mut head = a.clone();
        head[10] ^= 1;
        let mut tail = a.clone();
        *tail.last_mut().unwrap() ^= 1;
        let mut longer = a.clone();
        longer.push(0);
        assert_ne!(fp(&a), fp(&head));
        assert_ne!(fp(&a), fp(&tail));
        assert_ne!(fp(&a), fp(&longer));
    }

    #[test]
    fn partial_scheme_is_blind_between_samples_which_is_why_it_is_only_a_hint() {
        // Documents the limitation instead of hiding it: bytes no sample covers do not
        // change the value, so a match can never prove two files are identical.
        let a = pattern(10 * SAMPLE_BYTES as usize, 7);
        let mut b = a.clone();
        let unsampled = SAMPLE_BYTES as usize + 5;
        assert!(
            !sample_offsets(a.len() as u64)
                .iter()
                .any(|&o| { (o..o + SAMPLE_BYTES).contains(&(unsampled as u64)) })
        );
        b[unsampled] ^= 0xff;
        assert_ne!(a, b);
        assert_eq!(fp(&a), fp(&b));
        // A fully hashed small file has no such blind spot.
        let mut small = pattern(1000, 7);
        let before = fp(&small);
        small[500] ^= 1;
        assert_ne!(before, fp(&small));
    }

    #[test]
    fn read_budget_is_bounded_whatever_the_file_size() {
        for (size, expected) in [
            (3 * SAMPLE_BYTES + 1, 3),
            (900 << 20, 3),
            (LARGE_FILE_BYTES, 5),
            (50 * (1 << 30), 5),
        ] {
            let offsets = sample_offsets(size);
            assert_eq!(offsets.len(), expected, "{size}");
            assert!(offsets.windows(2).all(|w| w[0] + SAMPLE_BYTES <= w[1]));
            assert!(offsets.last().unwrap() + SAMPLE_BYTES <= size);
        }
        assert!(sample_offsets(3 * SAMPLE_BYTES).is_empty());
    }

    #[tokio::test]
    async fn opened_file_fingerprint_matches_reader_and_survives_rename() {
        let root = tempfile::tempdir().unwrap();
        let bytes = pattern(5 * SAMPLE_BYTES as usize + 17, 3);
        std::fs::create_dir(root.path().join("Old (2020)")).unwrap();
        std::fs::write(root.path().join("Old (2020)/Film.mkv"), &bytes).unwrap();
        let before = quick_fingerprint(
            &OpenedFile::open_in_root(root.path(), std::path::Path::new("Old (2020)/Film.mkv"))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        std::fs::rename(
            root.path().join("Old (2020)"),
            root.path().join("New (2020)"),
        )
        .unwrap();
        std::fs::rename(
            root.path().join("New (2020)/Film.mkv"),
            root.path().join("New (2020)/Renamed.mkv"),
        )
        .unwrap();
        let after = quick_fingerprint(
            &OpenedFile::open_in_root(root.path(), std::path::Path::new("New (2020)/Renamed.mkv"))
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(before, after);
        assert_eq!(before, fp(&bytes));
    }

    /// Cost probe for the media you actually run on. Not part of the normal suite:
    /// `MEDIA_FINGERPRINT_BENCH_DIR=/path/on/the/target/disk cargo test -p media-node \
    ///  fingerprint::tests::bench -- --ignored --nocapture`
    /// Writes nothing: it fingerprints existing files of at least 1 MiB found directly in the
    /// directory (up to 200) and prints cold-per-file latency percentiles and the full-read
    /// throughput for comparison. Run it once per storage class (SSD, HDD, NAS mount); the page
    /// cache is warm after the first pass, so evict it first for honest HDD/NAS numbers.
    #[test]
    #[ignore = "benchmark: needs MEDIA_FINGERPRINT_BENCH_DIR"]
    fn bench() {
        let dir =
            std::env::var_os("MEDIA_FINGERPRINT_BENCH_DIR").expect("MEDIA_FINGERPRINT_BENCH_DIR");
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.metadata()
                    .is_ok_and(|m| m.is_file() && m.len() >= 1 << 20)
            })
            .map(|e| e.path())
            .collect();
        files.sort();
        files.truncate(200);
        assert!(
            !files.is_empty(),
            "no file of at least 1 MiB in the directory"
        );
        let mut micros = Vec::new();
        let mut total_bytes = 0u64;
        for path in &files {
            let file = File::open(path).unwrap();
            let size = file.metadata().unwrap().len();
            total_bytes += size;
            let start = std::time::Instant::now();
            fingerprint_file(file, size).unwrap();
            micros.push(start.elapsed().as_micros());
        }
        micros.sort_unstable();
        let pick = |q: f64| micros[((micros.len() - 1) as f64 * q) as usize];
        println!(
            "files={} total={:.1} GiB  fingerprint p50={}us p90={}us max={}us  mean={}us",
            files.len(),
            total_bytes as f64 / f64::from(1u32 << 30),
            pick(0.5),
            pick(0.9),
            micros.last().unwrap(),
            micros.iter().sum::<u128>() / micros.len() as u128,
        );
    }
}
