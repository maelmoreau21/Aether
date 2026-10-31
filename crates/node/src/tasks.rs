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

async fn audio_fingerprint(opened: &crate::files::OpenedFile) -> io::Result<Value> {
    let input = opened.try_clone_std()?;
    let mut child = tokio::process::Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "fd,pipe",
            "-probesize",
            "5000000",
            "-analyzeduration",
            "5000000",
            "-i",
            "-",
            "-map",
            "0:a:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "11025",
            "-t",
            "600",
            "-f",
            "chromaprint",
            "-fp_format",
            "raw",
            "-",
        ])
        .stdin(Stdio::from(input))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("fingerprint_output_unavailable"))?
        .take(32_769);
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(std::time::Duration::from_secs(90), async {
        output.read_to_end(&mut bytes).await?;
        if bytes.len() > 32_768 || bytes.len() % 4 != 0 {
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
    let fingerprints: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_le_bytes(*chunk))
        .collect();
    if fingerprints.is_empty() {
        return Err(io::Error::other("fingerprint_empty"));
    }
    let analyzed_seconds = fingerprints.len() as f64 / 8.61328125;
    let result = json!({"fingerprints":fingerprints,"fingerprintRate":8.61328125,"analyzedSeconds":analyzed_seconds,"algorithm":"chromaprint-v1"});
    if result.to_string().len() > 64 * 1024 {
        return Err(io::Error::other("fingerprint_result_too_large"));
    }
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
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid task target or options",
        )
    };
    let root = Path::new(&grant.root_snapshot);
    let relative = Path::new(&grant.relative_path);
    let opened = LocalNode.open_media(root, relative).await?;
    if opened.size() != expected_size || opened.modified_ns() != expected_modified_ns {
        return Err(io::Error::other("source_changed_rescan_required"));
    }
    match (category, &grant.target) {
        ("analyze", ReadTarget::Asset(_)) => {
            if component == Some("fingerprint") {
                return audio_fingerprint(&opened).await;
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
            let mgr = crate::trickplay::global_manager();
            let req = crate::trickplay::TrickplayRequest {
                asset_id: asset_id.as_str().to_owned(),
                media_root: root.to_path_buf(),
                relative_path: relative.to_path_buf(),
                duration,
                size: opened.size(),
                modified_ns: opened.modified_ns().to_owned(),
                interval_ms: None,
                width,
                quality: None,
            };
            let key = mgr.fingerprint(&req);
            mgr.execute_generation(&req, &key)
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
}
