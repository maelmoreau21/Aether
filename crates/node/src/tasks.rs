//! Reuse confined Node operations for background work; no caller-provided disk path.
use crate::{
    LocalNode,
    packaging::{Component, Preparations},
};
use media_contracts::inventory::{ReadGrant, ReadTarget};
use serde_json::{Value, json};
use std::process::Stdio;
use std::{io, path::Path};
use tokio::io::AsyncReadExt;

/// Chromaprint items per second: 11025 Hz audio, one item every 1365 samples (about 123.8 ms).
/// Until November 2026 this was announced as 11025/1280 = 8.6133, which stretched every marker
/// time by about 6 %: measured on `ffmpeg -f chromaprint` output of a repeated 60 s theme.
pub const FINGERPRINT_RATE: f64 = 11025.0 / 1365.0;
/// Longest audio decoded from the start of the file. Like intro-skipper, a quarter of the
/// file is read, between [`HEAD_MIN_SECONDS`] and this (10 min): an opening after a long cold
/// open is still inside, an opening is never looked for in the middle of an episode.
pub const HEAD_SECONDS: f64 = 600.0;
/// Shortest head window (the window of every extraction before November 2026).
pub const HEAD_MIN_SECONDS: f64 = 300.0;
/// The head and the tail each keep at most this many items.
const HEAD_ITEMS: usize = 4850;
const TAIL_ITEMS: usize = 3635;
/// The tail window ends with the file and is exactly as long as what is kept (3635 items =
/// 450 s, intro-skipper's longest credits).
const TAIL_SECONDS: f64 = 450.0;
/// Layout of the windows above, stored with each result: 2 since November 2026 (head up to
/// 10 min, tail 7 min 30). An entry without it was extracted with 300 s / 186 s.
pub const WINDOW_VERSION: u64 = 2;

/// Start window of a fingerprint when the Core sends none: a quarter of the file, at most 10 min.
pub const DEFAULT_FINGERPRINT_OPTIONS: media_contracts::protocol::FingerprintTaskOptions =
    media_contracts::protocol::FingerprintTaskOptions {
        head_percent: 25,
        head_max_seconds: 600,
    };

/// Head window for a file of this duration: `head_percent` of it, at least 5 minutes, at most
/// `head_max_seconds` (which wins when shorter), 10 minutes at most in any case. Unknown
/// duration: the maximum.
pub fn head_seconds(
    duration: Option<f64>,
    options: &media_contracts::protocol::FingerprintTaskOptions,
) -> f64 {
    let max = f64::from(options.head_max_seconds.clamp(60, 600)).min(HEAD_SECONDS);
    duration.map_or(max, |d| {
        (d * f64::from(options.head_percent.clamp(1, 50)) / 100.0)
            .max(HEAD_MIN_SECONDS)
            .min(max)
    })
}

/// `MEDIA_FINGERPRINT_CACHE_DIR`, else `<data dir>/cache/fingerprints` (never the working directory).
fn fingerprint_cache_dir() -> std::path::PathBuf {
    crate::data_subdir("MEDIA_FINGERPRINT_CACHE_DIR", "cache/fingerprints")
}

fn fingerprint_cache_path(asset_id: &str, size: u64, modified_ns: &str) -> std::path::PathBuf {
    let cache_dir = fingerprint_cache_dir();
    let sanitized_id = asset_id.replace(['/', '\\', ':', '.', ' '], "_");
    cache_dir.join(format!("{sanitized_id}_{size}_{modified_ns}.json"))
}

/// A cached extraction is reused only if it was made with the current rate (and so the current tail
/// window). The check is on the content, not the file name, so only entries that are truly stale
/// are recomputed.
fn is_current_cache_entry(
    value: &Value,
    options: &media_contracts::protocol::FingerprintTaskOptions,
) -> bool {
    value["fingerprintRate"]
        .as_f64()
        .is_some_and(|rate| (rate - FINGERPRINT_RATE).abs() < 1e-9)
        && value["window"].as_u64() == Some(WINDOW_VERSION)
        && value["headPercent"].as_u64() == Some(u64::from(options.head_percent))
        && value["headMaxSeconds"].as_u64() == Some(u64::from(options.head_max_seconds))
}

pub async fn clear_fingerprint_cache() -> io::Result<u64> {
    let cache_dir = fingerprint_cache_dir();
    let mut cleared = 0u64;
    if let Ok(mut entries) = tokio::fs::read_dir(&cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().extension().is_some_and(|ext| ext == "json") {
                if let Ok(meta) = entry.metadata().await {
                    cleared += meta.len();
                }
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }
    Ok(cleared)
}

/// Fingerprint one window of the file. The file is given to ffmpeg as its standard input; with
/// `seekable`, ffmpeg reads it through the `fd:` protocol, which can seek: an MP4 whose index
/// is at the end (no "faststart") can be read at all, and the tail window is reached by a seek
/// instead of decoding the whole file through a pipe. ffmpeg before 6.1 has no `fd:` protocol:
/// the caller then retries through the pipe.
async fn run_single_chromaprint(
    opened: &crate::files::OpenedFile,
    seek_seconds: Option<f64>,
    max_duration_seconds: f64,
    seekable: bool,
) -> io::Result<Vec<u32>> {
    let mut file = opened.try_clone_std()?;
    std::io::Seek::rewind(&mut file)?;
    let mut cmd = tokio::process::Command::new("ffmpeg");
    cmd.args([
        "-nostdin",
        "-v",
        "error",
        "-protocol_whitelist",
        "fd,pipe",
        "-probesize",
        "5000000",
        "-analyzeduration",
        "5000000",
    ]);
    if let Some(seek) = seek_seconds.filter(|s| *s > 0.0) {
        cmd.args(["-ss", &format!("{:.3}", seek)]);
    }
    if seekable {
        cmd.args(["-fd", "0", "-i", "fd:"]);
    } else {
        cmd.args(["-i", "-"]);
    }
    cmd.args([
        "-map",
        "0:a:0",
        "-vn",
        "-ac",
        "1",
        "-ar",
        "11025",
        "-t",
        &format!("{:.3}", max_duration_seconds),
        "-f",
        "chromaprint",
        "-fp_format",
        "raw",
        "-",
    ])
    .stdin(Stdio::from(file))
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);

    let mut child = cmd.spawn()?;
    let mut output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("fingerprint_output_unavailable"))?
        .take(16_385);
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        output.read_to_end(&mut bytes).await?;
        if bytes.len() > 16_384 || bytes.len() % 4 != 0 {
            return Err(io::Error::other("fingerprint_result_invalid"));
        }
        if !child.wait().await?.success() {
            return Err(io::Error::other("fingerprint_decoder_failed"));
        }
        Ok(())
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "fingerprint_timeout"))?;
    result?;
    opened.verify_unchanged()?;
    let fps: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_le_bytes(*chunk))
        .collect();
    Ok(fps)
}

async fn audio_fingerprint(
    opened: &crate::files::OpenedFile,
    asset_id: &str,
    options: &media_contracts::protocol::FingerprintTaskOptions,
) -> io::Result<Value> {
    let cache_path = fingerprint_cache_path(asset_id, opened.size(), opened.modified_ns());
    // An entry written with the previous rate also has the previous tail window: it is stale, not
    // merely mislabelled, and is recomputed in place.
    if let Ok(content) = tokio::fs::read_to_string(&cache_path).await
        && let Ok(val) = serde_json::from_str::<Value>(&content)
        && val.get("fingerprints").is_some()
        && val.get("algorithm").is_some()
        && is_current_cache_entry(&val, options)
    {
        return Ok(val);
    }

    let probe = crate::probe::probe_opened(opened).await.ok();
    let duration = probe
        .as_ref()
        .and_then(|p| {
            p.format.get("duration").and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| v.as_f64())
            })
        })
        .filter(|d| d.is_finite() && *d > 0.0);

    // Seekable input first; a Node whose ffmpeg predates the `fd:` protocol uses the pipe.
    let head = head_seconds(duration, options);
    let (mut intro_fps, seekable) = match run_single_chromaprint(opened, None, head, true).await {
        Ok(fps) if !fps.is_empty() => (fps, true),
        Ok(_) | Err(_) => (
            run_single_chromaprint(opened, None, head, false).await?,
            false,
        ),
    };
    if intro_fps.is_empty() {
        return Err(io::Error::other("fingerprint_empty"));
    }
    intro_fps.truncate(HEAD_ITEMS);
    let analyzed_seconds = intro_fps.len() as f64 / FINGERPRINT_RATE;

    let (tail_fps, tail_start) = if let Some(dur) = duration {
        if dur <= head {
            let t_start = (dur - TAIL_SECONDS).max(0.0);
            let start_idx = ((t_start * FINGERPRINT_RATE).round() as usize).min(intro_fps.len());
            (Some(intro_fps[start_idx..].to_vec()), Some(t_start))
        } else {
            let t_start = (dur - TAIL_SECONDS).max(0.0);
            if let Ok(mut tail) =
                run_single_chromaprint(opened, Some(t_start), TAIL_SECONDS, seekable).await
            {
                tail.truncate(TAIL_ITEMS);
                (Some(tail), Some(t_start))
            } else {
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    let mut result = json!({
        "fingerprints": intro_fps,
        "fingerprintRate": FINGERPRINT_RATE,
        "analyzedSeconds": analyzed_seconds,
        "algorithm": "chromaprint-v1",
        "window": WINDOW_VERSION,
        "headPercent": options.head_percent,
        "headMaxSeconds": options.head_max_seconds
    });

    if let (Some(tf), Some(ts)) = (tail_fps, tail_start)
        && !tf.is_empty()
    {
        result["tailFingerprints"] = json!(tf);
        result["tailStart"] = json!(ts);
        result["tailAnalyzedSeconds"] = json!(tf.len() as f64 / FINGERPRINT_RATE);
    }

    let json_str = result.to_string();
    if json_str.len() > 64 * 1024 {
        return Err(io::Error::other("fingerprint_result_too_large"));
    }

    if let Some(parent) = cache_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let tmp_path = format!("{}.tmp.{}", cache_path.display(), uuid::Uuid::new_v4());
    if tokio::fs::write(&tmp_path, json_str.as_bytes())
        .await
        .is_ok()
    {
        let _ = tokio::fs::rename(&tmp_path, &cache_path).await;
    }
    // Bounded size and crash leftovers; every entry can be recomputed from the media.
    let dir = fingerprint_cache_dir();
    let _ = tokio::task::spawn_blocking(move || {
        crate::disk_cache::enforce_quota(
            &dir,
            "json",
            crate::disk_cache::budget_from_env(
                "MEDIA_FINGERPRINT_CACHE_MAX_BYTES",
                64 * 1024 * 1024,
            ),
            std::time::Duration::from_secs(3600),
        )
    })
    .await;

    Ok(result)
}

pub async fn run_task_on_grant(
    category: &str,
    grant: &ReadGrant,
    component: Option<&str>,
    width: Option<u32>,
    preparations: &Preparations,
    expected_size: u64,
    expected_modified_ns: &str,
) -> io::Result<Value> {
    run_task_with_trickplay_options(
        category,
        grant,
        component,
        width,
        preparations,
        expected_size,
        expected_modified_ns,
        None,
        None,
        None,
    )
    .await
}

/// Chapter preparation is bounded like the visible chapter list in the player.
const MAX_PREPARED_CHAPTERS: usize = 100;

#[allow(clippy::too_many_arguments)]
pub async fn run_task_with_trickplay_options(
    category: &str,
    grant: &ReadGrant,
    component: Option<&str>,
    width: Option<u32>,
    preparations: &Preparations,
    expected_size: u64,
    expected_modified_ns: &str,
    trickplay: Option<&media_contracts::protocol::TrickplayTaskOptions>,
    fingerprint: Option<&media_contracts::protocol::FingerprintTaskOptions>,
    manager: Option<&crate::trickplay::TrickplayManager>,
) -> io::Result<Value> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid task target or options",
        )
    };
    if trickplay.is_some_and(|options| {
        category != "trickplay"
            || !(1000..=60000).contains(&options.interval_ms)
            || !(1..=31).contains(&options.quality)
            || !(1..=20).contains(&options.columns)
            || !(1..=20).contains(&options.rows)
    }) {
        return Err(invalid());
    }
    if fingerprint.is_some_and(|options| {
        component != Some("fingerprint")
            || !(1..=50).contains(&options.head_percent)
            || !(60..=600).contains(&options.head_max_seconds)
    }) {
        return Err(invalid());
    }
    let root = Path::new(&grant.root_snapshot);
    let relative = Path::new(&grant.relative_path);
    let opened = LocalNode.open_media(root, relative).await?;
    if opened.size() != expected_size || opened.modified_ns() != expected_modified_ns {
        return Err(io::Error::other("source_changed_rescan_required"));
    }
    match (category, &grant.target) {
        ("analyze", ReadTarget::Asset(asset_id)) => {
            if component == Some("fingerprint") {
                return audio_fingerprint(
                    &opened,
                    asset_id.as_str(),
                    fingerprint.unwrap_or(&DEFAULT_FINGERPRINT_OPTIONS),
                )
                .await;
            }
            if component == Some(crate::end_credits::COMPONENT) {
                return crate::end_credits::scan(&opened).await;
            }
            let probe = crate::probe::probe_opened(&opened)
                .await
                .map_err(io::Error::other)?;
            let encoded = serde_json::to_vec(&probe).map_err(io::Error::other)?;
            if encoded.len() > 64 * 1024 {
                return Err(io::Error::other("analysis_result_limit_exceeded"));
            }
            serde_json::from_slice(&encoded).map_err(io::Error::other)
        }
        ("prepare", ReadTarget::Asset(_)) => {
            let raw = component.ok_or_else(invalid)?;
            let selected = if raw == "video" {
                Component::Video
            } else if let Some(index) = raw
                .strip_prefix("audio-")
                .and_then(|s| s.parse::<u32>().ok())
            {
                Component::Audio(index)
            } else if let Some(index) = raw
                .strip_prefix("subtitle-")
                .and_then(|s| s.parse::<u32>().ok())
            {
                Component::Subtitle(index)
            } else {
                return Err(invalid());
            };
            let prepared = preparations
                .prepare(
                    root,
                    relative,
                    opened.size(),
                    opened.modified_ns(),
                    selected,
                )
                .await?;
            Ok(
                json!({"mime":selected.mime(),"size":prepared.size(),"component":raw,"cacheWarm":true}),
            )
        }
        (
            "image",
            ReadTarget::Image {
                expected_size,
                expected_modified_ns,
                ..
            },
        ) => {
            if opened.size() != *expected_size || opened.modified_ns() != expected_modified_ns {
                return Err(io::Error::other("source_changed_rescan_required"));
            }
            let width = width.unwrap_or(640);
            let image = crate::image_preview::generate(&opened, width).await?;
            if image.source_mime != grant.mime {
                return Err(io::Error::other("source_changed_rescan_required"));
            }
            Ok(json!({"width":width,"size":image.bytes.len(),"mime":image.mime,"cacheWarm":true}))
        }
        ("trickplay", ReadTarget::Asset(asset_id)) => {
            let probe = crate::probe::probe_opened(&opened)
                .await
                .map_err(io::Error::other)?;
            let duration = probe
                .format
                .get("duration")
                .and_then(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .or_else(|| v.as_f64())
                })
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| io::Error::other("invalid_duration"))?;
            let fallback;
            let mgr = if let Some(manager) = manager {
                manager
            } else {
                fallback = crate::trickplay::global_manager();
                &fallback
            };
            if let Some(mb) = trickplay.and_then(|options| options.max_cache_mb) {
                mgr.set_max_cache_mb(mb);
            }
            if component == Some("chapters") {
                // Same profile and boundaries as the on-demand chapter route,
                // so the player is served from this cache afterwards.
                let mut times: Vec<f64> = probe
                    .chapters
                    .iter()
                    .take(1000)
                    .filter_map(|chapter| chapter["start_time"].as_str()?.parse::<f64>().ok())
                    .filter(|start| start.is_finite() && *start >= 0.0 && *start < duration)
                    .collect();
                times.sort_by(f64::total_cmp);
                times.dedup();
                let mut prepared = 0_u64;
                for time in times.into_iter().take(MAX_PREPARED_CHAPTERS) {
                    let req = crate::trickplay::TrickplayRequest {
                        asset_id: asset_id.as_str().to_owned(),
                        media_root: root.to_path_buf(),
                        relative_path: relative.to_path_buf(),
                        duration,
                        size: opened.size(),
                        modified_ns: opened.modified_ns().to_owned(),
                        interval_ms: None,
                        width,
                        quality: trickplay.map(|options| options.quality),
                        columns: None,
                        rows: None,
                        chapter_time: Some(time),
                    };
                    if mgr.check_native_cache(&req).await.is_none() {
                        let key = mgr.fingerprint(&req);
                        mgr.generate_background(&req, &key)
                            .await
                            .map_err(io::Error::other)?;
                    }
                    prepared += 1;
                }
                return Ok(json!({"assetId": asset_id.as_str(), "chapterImages": prepared}));
            }
            let req = crate::trickplay::TrickplayRequest {
                asset_id: asset_id.as_str().to_owned(),
                media_root: root.to_path_buf(),
                relative_path: relative.to_path_buf(),
                duration,
                size: opened.size(),
                modified_ns: opened.modified_ns().to_owned(),
                interval_ms: trickplay.map(|options| options.interval_ms),
                width,
                quality: trickplay.map(|options| options.quality),
                columns: trickplay.map(|options| options.columns),
                rows: trickplay.map(|options| options.rows),
                chapter_time: None,
            };
            let key = mgr.fingerprint(&req);
            mgr.generate_background(&req, &key)
                .await
                .map_err(io::Error::other)?;
            let manifest = mgr
                .check_native_cache(&req)
                .await
                .ok_or_else(|| io::Error::other("generation_not_found"))?;
            Ok(json!({
                "assetId": asset_id.as_str(),
                "sheetCount": manifest.sheet_count,
                "thumbnailCount": manifest.thumbnail_count,
                "intervalMs": manifest.interval_ms,
                "width": manifest.width,
                "height": manifest.height,
            }))
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_contracts::ids::{AssetId, ImageId, LibraryId, NodeId};

    #[test]
    fn only_extractions_made_with_the_current_rate_are_reused() {
        // Chromaprint: 11025 Hz, one item per 1365 samples.
        assert!((FINGERPRINT_RATE - 8.076923).abs() < 1e-6);
        let options = DEFAULT_FINGERPRINT_OPTIONS;
        let current = json!({"fingerprintRate": FINGERPRINT_RATE, "window": WINDOW_VERSION,
            "headPercent": 25, "headMaxSeconds": 600});
        assert!(is_current_cache_entry(&current, &options));
        // The administrator changed the start window: made again with the new one.
        let narrower = media_contracts::protocol::FingerprintTaskOptions {
            head_percent: 10,
            head_max_seconds: 600,
        };
        assert!(!is_current_cache_entry(&current, &narrower));
        // Same rate, but extracted with the 300 s / 186 s windows: made again.
        assert!(!is_current_cache_entry(
            &json!({"fingerprintRate": FINGERPRINT_RATE}),
            &options
        ));
        // Written before November 2026: mislabelled rate and a tail that stopped 14 s early.
        assert!(!is_current_cache_entry(
            &json!({"fingerprintRate": 8.61328125}),
            &options
        ));
        assert!(!is_current_cache_entry(&json!({}), &options));
        // The window the Node asks for is what it keeps, so the tail reaches the end of the file.
        assert!((TAIL_ITEMS as f64 / FINGERPRINT_RATE - TAIL_SECONDS).abs() < 1.0);
        assert!(HEAD_ITEMS as f64 / FINGERPRINT_RATE > HEAD_SECONDS);
        // A quarter of the file, within 5 and 10 minutes, 10 when the duration is unknown.
        assert_eq!(head_seconds(Some(18.0 * 60.0), &options), HEAD_MIN_SECONDS);
        assert_eq!(head_seconds(Some(44.0 * 60.0), &options), HEAD_SECONDS);
        assert_eq!(head_seconds(Some(32.0 * 60.0), &options), 480.0);
        assert_eq!(head_seconds(None, &options), HEAD_SECONDS);
        // 40 % within 8 minutes; a maximum under 5 minutes wins over the 5-minute floor.
        let custom =
            |head_percent, minutes: u32| media_contracts::protocol::FingerprintTaskOptions {
                head_percent,
                head_max_seconds: minutes * 60,
            };
        assert_eq!(head_seconds(Some(30.0 * 60.0), &custom(40, 8)), 480.0);
        assert_eq!(head_seconds(Some(10.0 * 60.0), &custom(40, 8)), 300.0);
        assert_eq!(head_seconds(Some(60.0 * 60.0), &custom(25, 3)), 180.0);
    }

    #[tokio::test]
    #[ignore = "requires FFmpeg and generated legal representative fixture"]
    async fn background_trickplay_uses_the_library_profile() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative/representative.mp4");
        let data = tempfile::tempdir().unwrap();
        let media = data.path().join("media");
        tokio::fs::create_dir(&media).await.unwrap();
        tokio::fs::copy(fixture, media.join("video.mp4"))
            .await
            .unwrap();
        let root = tokio::fs::canonicalize(&media).await.unwrap();
        let original = tokio::fs::read(root.join("video.mp4")).await.unwrap();
        let opened = LocalNode
            .open_media(&root, Path::new("video.mp4"))
            .await
            .unwrap();
        let grant = ReadGrant {
            target: ReadTarget::Asset(AssetId::new("library-profile")),
            library_id: LibraryId::new("profile"),
            node_id: NodeId::new("local"),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "video.mp4".into(),
            mime: "video/mp4".into(),
        };
        let manager = crate::trickplay::TrickplayManager::new(
            data.path().join("cache"),
            16 * 1024 * 1024,
            "ffmpeg".into(),
            true,
            None,
        );
        let profile = media_contracts::protocol::TrickplayTaskOptions {
            interval_ms: 2000,
            quality: 12,
            columns: 10,
            rows: 10,
            max_cache_mb: None,
        };
        let result = run_task_with_trickplay_options(
            "trickplay",
            &grant,
            None,
            Some(240),
            &Preparations::default(),
            opened.size(),
            opened.modified_ns(),
            Some(&profile),
            None,
            Some(&manager),
        )
        .await
        .unwrap();
        assert_eq!(result["width"], 240);
        assert_eq!(result["intervalMs"], 2000);
        assert!(result["thumbnailCount"].as_u64().unwrap() > 1);
        let request = crate::trickplay::TrickplayRequest {
            asset_id: "library-profile".into(),
            media_root: root.clone(),
            relative_path: "video.mp4".into(),
            duration: 12.0,
            size: opened.size(),
            modified_ns: opened.modified_ns().into(),
            interval_ms: Some(2000),
            width: Some(240),
            quality: Some(12),
            columns: Some(10),
            rows: Some(10),
            chapter_time: None,
        };
        let mgr = &manager;
        assert!(mgr.check_native_cache(&request).await.is_some());
        let other = crate::trickplay::TrickplayRequest {
            quality: Some(7),
            ..request
        };
        assert!(mgr.check_native_cache(&other).await.is_none());
        assert_eq!(
            tokio::fs::read(root.join("video.mp4")).await.unwrap(),
            original
        );
    }

    #[tokio::test]
    async fn confined_av1_opus_asset_produces_bounded_chromaprint() {
        let has_chromaprint = std::process::Command::new("ffmpeg")
            .arg("-muxers")
            .output()
            .map(|out| {
                out.status.success() && String::from_utf8_lossy(&out.stdout).contains("chromaprint")
            })
            .unwrap_or(false);
        if !has_chromaprint {
            eprintln!("ffmpeg Chromaprint muxer unavailable; Chromaprint adapter not exercised");
            return;
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative");
        let root = tokio::fs::canonicalize(root).await.unwrap();
        let opened = LocalNode
            .open_media(&root, Path::new("representative.mp4"))
            .await
            .unwrap();
        let grant = ReadGrant {
            target: ReadTarget::Asset(AssetId::new("asset")),
            library_id: LibraryId::new("library"),
            node_id: NodeId::new("node"),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "representative.mp4".into(),
            mime: "video/mp4".into(),
        };
        let result = run_task_on_grant(
            "analyze",
            &grant,
            Some("fingerprint"),
            None,
            &Preparations::default(),
            opened.size(),
            opened.modified_ns(),
        )
        .await
        .unwrap();
        assert_eq!(result["algorithm"], "chromaprint-v1");
        assert!(
            result["fingerprints"]
                .as_array()
                .is_some_and(|values| values.len() > 20 && values.len() < 256)
        );
        assert!(result.to_string().len() < 64 * 1024);
    }

    /// End to end through the real ffmpeg: the tail window must end with the file and the cache must
    /// carry the Chromaprint rate. With the previous 200 s window kept to 1500 items the tail
    /// stopped 14 s before the end.
    #[tokio::test]
    async fn tail_reaches_the_end_of_the_file_and_carries_the_chromaprint_rate() {
        let has_chromaprint = std::process::Command::new("ffmpeg")
            .arg("-muxers")
            .output()
            .map(|out| {
                out.status.success() && String::from_utf8_lossy(&out.stdout).contains("chromaprint")
            })
            .unwrap_or(false);
        if !has_chromaprint {
            eprintln!("ffmpeg Chromaprint muxer unavailable; tail window not exercised");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("aether-fingerprint-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let made = std::process::Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=330:sample_rate=44100:duration=400",
                "-c:a",
                "aac",
            ])
            .arg(root.join("long.mkv"))
            .status()
            .unwrap();
        assert!(made.success());
        let root = tokio::fs::canonicalize(&root).await.unwrap();
        let opened = LocalNode
            .open_media(&root, Path::new("long.mkv"))
            .await
            .unwrap();
        let grant = ReadGrant {
            target: ReadTarget::Asset(AssetId::new("asset-tail")),
            library_id: LibraryId::new("library"),
            node_id: NodeId::new("node"),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "long.mkv".into(),
            mime: "video/x-matroska".into(),
        };
        let result = run_task_on_grant(
            "analyze",
            &grant,
            Some("fingerprint"),
            None,
            &Preparations::default(),
            opened.size(),
            opened.modified_ns(),
        )
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!((result["fingerprintRate"].as_f64().unwrap() - FINGERPRINT_RATE).abs() < 1e-9);
        let head = result["fingerprints"].as_array().unwrap().len() as f64;
        assert!(
            (head / FINGERPRINT_RATE - head_seconds(Some(400.0), &DEFAULT_FINGERPRINT_OPTIONS))
                .abs()
                < 3.0,
            "head {head}"
        );
        let tail = result["tailFingerprints"].as_array().unwrap().len() as f64;
        let start = result["tailStart"].as_f64().unwrap();
        let end = start + tail / FINGERPRINT_RATE;
        // Chromaprint itself ends about 3 s before the audio does (the old window ended 14 s early).
        assert!(
            (start - (400.0 - TAIL_SECONDS).max(0.0)).abs() < 1.0,
            "start {start}"
        );
        assert!(
            end > 400.0 - 4.5,
            "the tail ends {:.1} s before the end of the file",
            400.0 - end
        );
    }

    #[tokio::test]
    async fn analysis_uses_real_confined_asset_and_rejects_changed_fingerprint() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative");
        let root = tokio::fs::canonicalize(root).await.unwrap();
        let opened = LocalNode
            .open_media(&root, Path::new("representative.mp4"))
            .await
            .unwrap();
        let grant = ReadGrant {
            target: ReadTarget::Asset(AssetId::new("asset")),
            library_id: LibraryId::new("library"),
            node_id: NodeId::new("node"),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "representative.mp4".into(),
            mime: "video/mp4".into(),
        };
        let preparations = Preparations::default();
        assert!(
            run_task_on_grant(
                "analyze",
                &grant,
                None,
                None,
                &preparations,
                opened.size() + 1,
                opened.modified_ns()
            )
            .await
            .is_err()
        );
        let result = run_task_on_grant(
            "analyze",
            &grant,
            None,
            None,
            &preparations,
            opened.size(),
            opened.modified_ns(),
        )
        .await
        .unwrap();
        assert!(
            result["streams"]
                .as_array()
                .unwrap()
                .iter()
                .any(|stream| stream["codec_name"] == "av1")
        );
        assert!(
            result["streams"]
                .as_array()
                .unwrap()
                .iter()
                .any(|stream| stream["codec_name"] == "opus")
        );
        assert!(result["format"].get("filename").is_none());
    }

    #[tokio::test]
    async fn image_task_warms_the_same_resized_representation_served_to_clients() {
        use http_body_util::BodyExt;
        let temp = tempfile::tempdir().unwrap();
        image::DynamicImage::new_rgb8(640, 960)
            .save(temp.path().join("poster.jpg"))
            .unwrap();
        let root = tokio::fs::canonicalize(temp.path()).await.unwrap();
        let opened = LocalNode
            .open_media(&root, Path::new("poster.jpg"))
            .await
            .unwrap();
        let grant = ReadGrant {
            target: ReadTarget::Image {
                id: ImageId::new("poster"),
                expected_size: opened.size(),
                expected_modified_ns: opened.modified_ns().into(),
            },
            library_id: LibraryId::new("library"),
            node_id: NodeId::new("node"),
            root_snapshot: root.to_string_lossy().into_owned(),
            relative_path: "poster.jpg".into(),
            mime: "image/jpeg".into(),
        };
        let preparations = Preparations::default();
        let result = run_task_on_grant(
            "image",
            &grant,
            None,
            Some(320),
            &preparations,
            opened.size(),
            opened.modified_ns(),
        )
        .await
        .unwrap();
        assert_eq!(result["cacheWarm"], true);
        let request = http::Request::builder()
            .uri("/image?width=320")
            .body(())
            .unwrap();
        let response = LocalNode
            .serve_grant_for_node(NodeId::new("node"), &grant, &request)
            .await
            .unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!(image.width(), 320);
        assert_eq!(image.height(), 480);
        assert!(
            run_task_on_grant(
                "image",
                &grant,
                None,
                Some(321),
                &preparations,
                opened.size(),
                opened.modified_ns()
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn fingerprint_cache_and_invalidation_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join("fp_cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        unsafe {
            std::env::set_var("MEDIA_FINGERPRINT_CACHE_DIR", &cache_dir);
        }

        let asset_id = "test-asset-123";
        let size = 1000;
        let mod1 = "123456789";
        let mod2 = "987654321";

        let path1 = fingerprint_cache_path(asset_id, size, mod1);
        let path2 = fingerprint_cache_path(asset_id, size, mod2);

        // Cache paths differ on modified source
        assert_ne!(path1, path2);

        // Write mock cached fingerprint
        let mock_val = json!({
            "fingerprints": [11, 22, 33],
            "fingerprintRate": FINGERPRINT_RATE,
            "algorithm": "chromaprint-v1"
        });
        std::fs::write(&path1, mock_val.to_string()).unwrap();
        assert!(path1.exists());

        // Clear cache
        let cleared = clear_fingerprint_cache().await.unwrap();
        assert!(cleared > 0);
        assert!(!path1.exists());
    }
}
