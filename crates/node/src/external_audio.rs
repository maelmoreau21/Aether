//! Independent Ogg Opus sidecars. Detection does not imply browser playback.
use crate::{files::OpenedFile, probe, subtitles::suffix};
use media_contracts::inventory::{ExternalAudio, ScanIssue};
use std::path::Path;

pub async fn discover(
    root: &Path,
    video: &Path,
    folder: Option<&crate::scanner::FolderVideos>,
) -> (Vec<ExternalAudio>, Vec<ScanIssue>) {
    let parent = video.parent().unwrap_or(Path::new(""));
    let manifest = match crate::sidecar_manifest::load(root, video).await {
        Ok(manifest) => manifest,
        Err(message) => {
            return (
                Vec::new(),
                vec![ScanIssue {
                    relative_path: video.to_string_lossy().into(),
                    message,
                }],
            );
        }
    };
    let mut tracks = Vec::new();
    let mut issues = Vec::new();
    let loaded;
    let folder = match folder {
        Some(folder) => folder,
        None => match crate::scanner::FolderVideos::load(root, parent).await {
            Ok(entries) => {
                loaded = entries;
                &loaded
            }
            Err(_) => return (tracks, issues),
        },
    };
    let mut videos = Vec::new();
    let mut candidates = Vec::new();
    for path in &folder.sidecar_paths {
        let path = path.clone();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "mkv" | "mp4" | "webm" => videos.push((path.file_name().map(|s| s.to_owned()), stem)),
            "opus" | "ogg" => candidates.push((path, stem)),
            _ => {}
        }
        if videos.len() + candidates.len() > 10000 {
            return (tracks, issues);
        }
    }
    if let Some(manifest) = &manifest {
        candidates = manifest
            .audio
            .iter()
            .map(|track| (std::path::PathBuf::from(&track.file), String::new()))
            .collect();
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, stem) in candidates {
        let explicit = manifest
            .as_ref()
            .and_then(|m| m.audio.iter().find(|track| path == Path::new(&track.file)));
        let metadata = if let Some(track) = explicit {
            Some((track.language.clone(), false, false))
        } else {
            let matching: Vec<_> = videos
                .iter()
                .filter_map(|(name, stem_video)| suffix(stem_video, &stem).map(|meta| (name, meta)))
                .collect();
            if !matching
                .iter()
                .any(|(name, _)| name.as_deref() == video.file_name())
            {
                continue;
            }
            if matching.len() == 1 {
                Some(matching[0].1.clone())
            } else {
                None
            }
        };
        let relative = parent.join(path.file_name().unwrap_or_default());
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let result = async {
            if metadata.is_none() {
                return Err("ambiguous external audio association".to_owned());
            }
            if tracks.len() >= 8 {
                return Err("external audio track limit exceeded".into());
            }
            let opened = OpenedFile::open_in_root(root, &relative)
                .await
                .map_err(|_| "external audio unavailable or linked")?;
            let probe = probe::probe_opened(&opened)
                .await
                .map_err(|_| "external audio analysis failed")?;
            if probe.streams.len() != 1
                || probe.streams[0].codec_type.as_deref() != Some("audio")
                || probe.streams[0].codec_name.as_deref() != Some("opus")
            {
                return Err("external audio must contain exactly one Opus track".into());
            }
            if !probe
                .format
                .get("format_name")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.split(',').any(|s| s == "ogg"))
            {
                return Err(
                    "external Opus must be packaged in Ogg; raw packets need an explicit index"
                        .into(),
                );
            }
            let tagged = probe.streams[0]
                .tags
                .get("language")
                .and_then(|v| v.as_str());
            let language = if metadata.as_ref().is_some_and(|m| m.0 == "und") {
                match tagged {
                    Some("fra" | "fre" | "fr") => "fr",
                    Some("eng" | "en") => "en",
                    _ => "und",
                }
                .to_owned()
            } else {
                metadata
                    .as_ref()
                    .map(|m| m.0.clone())
                    .unwrap_or_else(|| "und".into())
            };
            Ok(ExternalAudio {
                relative_path: relative_path.clone(),
                size: opened.size(),
                modified_ns: opened.modified_ns().to_owned(),
                language,
                probe,
                offset_seconds: explicit.map_or(0.0, |track| track.offset_seconds),
            })
        }
        .await;
        match result {
            Ok(track) => tracks.push(track),
            Err(message) => issues.push(ScanIssue {
                relative_path,
                message,
            }),
        }
    }
    (tracks, issues)
}
