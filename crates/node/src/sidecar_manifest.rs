//! Explicit, bounded associations; files remain confined and independently probed.
use crate::files::OpenedFile;
use serde::Deserialize;
use std::{collections::HashSet, io, path::Path};
use tokio::io::AsyncReadExt;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    version: u32,
    #[serde(default)]
    pub audio: Vec<Audio>,
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Audio {
    pub file: String,
    pub language: String,
    #[serde(default)]
    pub offset_seconds: f64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Subtitle {
    pub file: String,
    pub language: String,
    #[serde(default)]
    pub forced: bool,
    #[serde(default)]
    pub sdh: bool,
}
fn filename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value != "."
        && value != ".."
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
fn language(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let mut parts = lower.split('-');
    let first = crate::subtitles::language(parts.next()?)?;
    let mut normalized = first.to_owned();
    for part in parts {
        if !(2..=8).contains(&part.len()) || !part.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return None;
        }
        normalized.push('-');
        normalized.push_str(part);
    }
    Some(normalized)
}
fn parse(bytes: &[u8]) -> Result<Manifest, String> {
    let mut manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "invalid media association manifest")?;
    if manifest.version != 1 || manifest.audio.len() > 8 || manifest.subtitles.len() > 64 {
        return Err("unsupported manifest version or track limit".into());
    }
    let mut files = HashSet::new();
    for track in &mut manifest.audio {
        if !filename(&track.file)
            || !files.insert(track.file.to_ascii_lowercase())
            || !track.offset_seconds.is_finite()
            || track.offset_seconds.abs() > 30.0
            || !matches!(
                Path::new(&track.file)
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("opus" | "ogg")
            )
        {
            return Err("invalid manifest audio file, duplicate or offset".into());
        }
        track.language = language(&track.language).ok_or("invalid manifest language")?;
    }
    for track in &mut manifest.subtitles {
        if !filename(&track.file)
            || !files.insert(track.file.to_ascii_lowercase())
            || !matches!(
                Path::new(&track.file)
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("srt" | "vtt" | "ass" | "ssa")
            )
        {
            return Err("invalid manifest subtitle file or duplicate".into());
        }
        track.language = language(&track.language).ok_or("invalid manifest language")?;
    }
    Ok(manifest)
}
/// `<video filename>.media.json` is authoritative when present; invalid metadata never falls back to guessed associations.
pub(crate) async fn load(root: &Path, video: &Path) -> Result<Option<Manifest>, String> {
    let name = video
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid video filename")?;
    let relative = video.with_file_name(format!("{name}.media.json"));
    let opened = match OpenedFile::open_in_root(root, &relative).await {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("media association manifest unavailable or linked".into()),
    };
    const LIMIT: u64 = 65536;
    if opened.size() > LIMIT {
        return Err("media association manifest exceeds 64 KiB".into());
    }
    let mut bytes = Vec::new();
    tokio::fs::File::from_std(opened.try_clone_std().map_err(|_| "manifest read failed")?)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "manifest read failed")?;
    opened
        .verify_unchanged_sync()
        .map_err(|_| "manifest changed during analysis")?;
    if bytes.len() as u64 > LIMIT {
        return Err("media association manifest exceeds 64 KiB".into());
    }
    parse(&bytes).map(Some)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn associations_are_versioned_confined_and_explicit() {
        let manifest = parse(br#"{"version":1,"audio":[{"file":"voice.opus","language":"fra","offset_seconds":0.25}],"subtitles":[{"file":"text.srt","language":"fr-FR","forced":true}]}"#).unwrap();
        assert_eq!(manifest.audio[0].language, "fr");
        assert_eq!(manifest.subtitles[0].language, "fr-fr");
        assert!(manifest.subtitles[0].forced);
        for file in [
            "../voice.opus",
            "a/voice.opus",
            "a\\voice.opus",
            "C:voice.opus",
            "https://host/voice.opus",
        ] {
            assert!(
                parse(
                    serde_json::json!({"version":1,"audio":[{"file":file,"language":"fr"}]})
                        .to_string()
                        .as_bytes()
                )
                .is_err()
            );
        }
        for value in [
            serde_json::json!({"version":2}),
            serde_json::json!({"version":1,"fps":25}),
            serde_json::json!({"version":1,"audio":[{"file":"a.opus","language":"fr","offset_seconds":31}]}),
            serde_json::json!({"version":1,"audio":[{"file":"a.opus","language":"fr"},{"file":"A.opus","language":"en"}]}),
        ] {
            assert!(parse(value.to_string().as_bytes()).is_err());
        }
    }
    #[tokio::test]
    async fn explicit_associations_override_ambiguous_stems_without_bypassing_probe() {
        let root = tempfile::tempdir().unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/generated/representative/representative.mp4");
        // Copy packets only: this creates independent assets, never changes a source fixture.
        for (name, args) in [
            ("film.mp4", vec!["-map", "0:v:0", "-an", "-c:v", "copy"]),
            ("film.webm", vec!["-map", "0:v:0", "-an", "-c:v", "copy"]),
            ("voice.opus", vec!["-map", "0:1", "-vn", "-c:a", "copy"]),
        ] {
            let output = tokio::process::Command::new("ffmpeg")
                .args(["-v", "error", "-nostdin", "-n", "-i"])
                .arg(&fixture)
                .args(args)
                .arg(root.path().join(name))
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        tokio::fs::write(
            root.path().join("words.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nBonjour",
        )
        .await
        .unwrap();
        let manifest = br#"{"version":1,"audio":[{"file":"voice.opus","language":"fra","offset_seconds":0.25}],"subtitles":[{"file":"words.srt","language":"fr","forced":true,"sdh":true}]}"#;
        tokio::fs::write(root.path().join("film.webm.media.json"), manifest)
            .await
            .unwrap();
        let (audio, issues) =
            crate::external_audio::discover(root.path(), Path::new("film.webm"), None).await;
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].language, "fr");
        assert_eq!(audio[0].offset_seconds, 0.25);
        assert_eq!(audio[0].probe.streams[0].details["channels"], 6);
        let (subs, issues) = crate::subtitles::discover(root.path(), Path::new("film.webm")).await;
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(subs.len(), 1);
        assert!(subs[0].forced && subs[0].sdh);
        let (audio, _) =
            crate::external_audio::discover(root.path(), Path::new("film.mp4"), None).await;
        assert!(
            audio.is_empty(),
            "explicit tracks must not leak to other videos"
        );
        tokio::fs::write(root.path().join("voice.opus"), b"not ogg or raw opus")
            .await
            .unwrap();
        let (audio, issues) =
            crate::external_audio::discover(root.path(), Path::new("film.webm"), None).await;
        assert!(audio.is_empty());
        assert!(
            issues
                .iter()
                .any(|issue| issue.message.contains("analysis failed"))
        );
        tokio::fs::write(
            root.path().join("film.webm.media.json"),
            br#"{"version":2}"#,
        )
        .await
        .unwrap();
        let (subs, issues) = crate::subtitles::discover(root.path(), Path::new("film.webm")).await;
        assert!(subs.is_empty());
        assert!(issues[0].message.contains("version"));
    }
}
