//! Read-only, bounded sidecar ingestion. Only exact, unique video stems associate.
use crate::files::OpenedFile;
use media_contracts::inventory::{ExternalSubtitle, ScanIssue};
use std::path::Path;
use tokio::io::AsyncReadExt;

const MAX_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRACKS: usize = 64;

pub(crate) fn language(token: &str) -> Option<&str> {
    match token {
        "fr" | "fra" | "fre" => Some("fr"),
        "en" | "eng" => Some("en"),
        "de" | "deu" | "ger" => Some("de"),
        "es" | "spa" => Some("es"),
        "it" | "ita" => Some("it"),
        "ja" | "jpn" => Some("ja"),
        "und" => Some("und"),
        // ISO 639-1 and optional region/script tags; never infer a language from free text.
        s if s.len() == 2 && s.bytes().all(|b| b.is_ascii_lowercase()) => Some(s),
        _ => None,
    }
}

pub(crate) fn suffix(video: &str, sidecar: &str) -> Option<(String, bool, bool)> {
    let rest = sidecar.strip_prefix(video)?;
    if !rest.is_empty() && !rest.starts_with('.') {
        return None;
    }
    let mut lang = None;
    let mut forced = false;
    let mut sdh = false;
    for token in rest
        .trim_start_matches('.')
        .split('.')
        .filter(|s| !s.is_empty())
    {
        match token {
            "forced" => forced = true,
            "sdh" | "cc" => sdh = true,
            s => {
                if lang.is_some() {
                    return None;
                }
                lang = Some(language(s)?.to_owned());
            }
        }
    }
    Some((lang.unwrap_or_else(|| "und".into()), forced, sdh))
}

fn timestamp(value: &str) -> Result<u64, &'static str> {
    let normalized = value.replace(',', ".");
    let parts: Vec<_> = normalized.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [m, s] => ("0", *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return Err("invalid subtitle timestamp"),
    };
    let (seconds, millis) = seconds
        .split_once('.')
        .ok_or("missing subtitle milliseconds")?;
    if millis.len() != 3 {
        return Err("invalid subtitle milliseconds");
    }
    let number = |v: &str| -> Result<u64, &'static str> {
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err("invalid subtitle timestamp");
        }
        v.parse().map_err(|_| "invalid subtitle timestamp")
    };
    let h = number(hours)?;
    let m = number(minutes)?;
    let s = number(seconds)?;
    if h > 999 || m >= 60 || s >= 60 {
        return Err("subtitle timestamp out of range");
    }
    Ok(((h * 60 + m) * 60 + s) * 1000 + number(millis)?)
}

fn stamp(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3600000,
        ms / 60000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Normalize text cues. Markup is escaped, never interpreted as HTML/ASS commands.
pub fn to_webvtt(text: &str, format: &str) -> Result<String, &'static str> {
    if matches!(format, "ass" | "ssa") {
        return ass_to_webvtt(text);
    }
    if !matches!(format, "srt" | "vtt") {
        return Err(
            "ASS/SSA styling and positioning require a dedicated renderer; sidecar not playable",
        );
    }
    let text = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if text.contains('\0') {
        return Err("subtitle contains NUL");
    }
    let text = if format == "vtt" {
        let (header, body) = text.split_once('\n').ok_or("invalid WebVTT header")?;
        if header != "WEBVTT" && !header.starts_with("WEBVTT ") {
            return Err("invalid WebVTT header");
        }
        body
    } else {
        &text
    };
    let mut output = String::from("WEBVTT\n\n");
    let mut count = 0;
    for block in text.trim().split("\n\n").filter(|b| !b.trim().is_empty()) {
        if block.starts_with("NOTE") {
            continue;
        }
        let lines: Vec<_> = block.lines().collect();
        let index = if lines[0].contains(" --> ") { 0 } else { 1 };
        let timing = lines.get(index).ok_or("missing subtitle timing")?;
        let (start, end) = timing
            .split_once(" --> ")
            .ok_or("invalid subtitle timing")?;
        let end = end
            .split_whitespace()
            .next()
            .ok_or("invalid subtitle end")?;
        let start = timestamp(start)?;
        let end = timestamp(end)?;
        if end <= start {
            // Skip zero or negative duration cues to preserve playback of the remaining valid cues
            continue;
        }
        let payload = lines
            .get(index + 1..)
            .filter(|p| !p.is_empty())
            .ok_or("empty subtitle cue")?;
        count += 1;
        if count > 20000 {
            return Err("too many subtitle cues");
        }
        output.push_str(&format!("{} --> {}\n", stamp(start), stamp(end)));
        for line in payload {
            output.push_str(
                &line
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            );
            output.push('\n');
        }
        output.push('\n');
        if output.len() > MAX_TOTAL_BYTES {
            return Err("normalized subtitle exceeds size limit");
        }
    }
    if count == 0 {
        return Err("subtitle contains no timed cues");
    }
    Ok(output)
}

/// Text fallback only: ASS animation, drawings, fonts and positioning are not reproduced.
/// Respect the declared Events columns and preserve commas inside the final Text field.
fn ass_to_webvtt(text: &str) -> Result<String, &'static str> {
    if text.contains('\0') {
        return Err("subtitle contains NUL");
    }
    let mut events = false;
    let mut columns: Vec<String> = Vec::new();
    let mut srt = String::new();
    let mut count = 0;
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.starts_with('[') {
            events = line.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if !events {
            continue;
        }
        if let Some(format) = line.strip_prefix("Format:") {
            columns = format
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .collect();
            if columns.last().map(String::as_str) != Some("text") || columns.len() > 32 {
                return Err("ASS Text must be the last event field");
            }
        }
        let Some(dialogue) = line.strip_prefix("Dialogue:") else {
            continue;
        };
        let start = columns
            .iter()
            .position(|s| s == "start")
            .ok_or("ASS Start field missing")?;
        let end = columns
            .iter()
            .position(|s| s == "end")
            .ok_or("ASS End field missing")?;
        let fields: Vec<_> = dialogue.trim().splitn(columns.len(), ',').collect();
        if fields.len() != columns.len() {
            return Err("invalid ASS event");
        }
        let time = |s: &str| -> Result<u64, &'static str> {
            let s = s.trim();
            let (head, fraction) = s.rsplit_once('.').ok_or("invalid ASS timestamp")?;
            if fraction.len() != 2 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
                return Err("invalid ASS centiseconds");
            }
            timestamp(&format!("{head}.{fraction}0"))
        };
        let a = time(fields[start])?;
        let b = time(fields[end])?;
        if b <= a {
            continue;
        }
        let mut payload = String::new();
        let mut override_tag = false;
        for c in fields.last().ok_or("ASS Text missing")?.chars() {
            match c {
                '{' if !override_tag => override_tag = true,
                '}' if override_tag => override_tag = false,
                _ if !override_tag => payload.push(c),
                _ => {}
            }
        }
        if override_tag {
            return Err("unclosed ASS override");
        }
        let payload = payload
            .replace("\\N", "\n")
            .replace("\\n", "\n")
            .replace("\\h", "\u{a0}");
        if payload.trim().is_empty() {
            continue;
        }
        count += 1;
        srt.push_str(&format!(
            "{count}\n{} --> {}\n{payload}\n\n",
            stamp(a),
            stamp(b)
        ));
        if srt.len() > MAX_TOTAL_BYTES || count > 20000 {
            return Err("ASS exceeds subtitle limits");
        }
    }
    to_webvtt(&srt, "srt")
}

pub async fn discover(root: &Path, video: &Path) -> (Vec<ExternalSubtitle>, Vec<ScanIssue>) {
    discover_with_folder(root, video, None).await
}

pub async fn discover_with_folder(
    root: &Path,
    video: &Path,
    folder: Option<&crate::scanner::FolderVideos>,
) -> (Vec<ExternalSubtitle>, Vec<ScanIssue>) {
    let mut tracks = Vec::new();
    let mut issues = Vec::new();
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
    let loaded;
    let folder = match folder {
        Some(folder) => folder,
        None => match crate::scanner::FolderVideos::load(root, parent).await {
            Ok(entries) => {
                loaded = entries;
                &loaded
            }
            Err(_) => {
                return (
                    tracks,
                    vec![ScanIssue {
                        relative_path: video.to_string_lossy().into(),
                        message: "subtitle directory unavailable".into(),
                    }],
                );
            }
        },
    };
    let mut videos = Vec::new();
    let mut candidates = Vec::new();
    for path in &folder.sidecar_paths {
        let path = path.clone();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match extension.as_str() {
            "mp4" | "mkv" | "webm" => videos.push((path.file_name().map(|s| s.to_owned()), stem)),
            "srt" | "vtt" | "ass" | "ssa" => candidates.push((path, stem, extension)),
            _ => {}
        }
        if videos.len() + candidates.len() > 10000 {
            return (
                tracks,
                vec![ScanIssue {
                    relative_path: video.to_string_lossy().into(),
                    message: "too many sidecar candidates in directory".into(),
                }],
            );
        }
    }
    if let Some(manifest) = &manifest {
        candidates = manifest
            .subtitles
            .iter()
            .map(|track| {
                let path = std::path::PathBuf::from(&track.file);
                let extension = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                (path, String::new(), extension)
            })
            .collect();
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    let mut total = 0;
    for (path, stem, extension) in candidates {
        let explicit = manifest.as_ref().and_then(|m| {
            m.subtitles
                .iter()
                .find(|track| path == Path::new(&track.file))
        });
        let metadata = if let Some(track) = explicit {
            Some((track.language.clone(), track.forced, track.sdh))
        } else {
            let matches: Vec<_> = videos
                .iter()
                .filter_map(|(name, video_stem)| {
                    suffix(video_stem, &stem).map(|metadata| (name, metadata))
                })
                .collect();
            if !matches
                .iter()
                .any(|(name, _)| name.as_deref() == video.file_name())
            {
                continue;
            }
            if matches.len() == 1 {
                Some(matches[0].1.clone())
            } else {
                None
            }
        };
        let relative = parent.join(path.file_name().unwrap_or_default());
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let result = async {
            if metadata.is_none() {
                return Err("ambiguous subtitle association; use a unique video stem".to_owned());
            }
            if tracks.len() >= MAX_TRACKS {
                return Err("subtitle track limit exceeded".to_owned());
            }
            let opened = OpenedFile::open_in_root(root, &relative)
                .await
                .map_err(|_| "subtitle unavailable or linked".to_owned())?;
            if opened.size() > MAX_BYTES {
                return Err(format!("subtitle exceeds {} KiB", MAX_BYTES / 1024));
            }
            let mut bytes = Vec::new();
            tokio::fs::File::from_std(opened.try_clone_std().map_err(|_| "subtitle read failed")?)
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "subtitle read failed")?;
            opened
                .verify_unchanged_sync()
                .map_err(|_| "subtitle changed during analysis")?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(format!("subtitle exceeds {} KiB", MAX_BYTES / 1024));
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| "subtitle must be UTF-8")?;
            let webvtt = to_webvtt(text, &extension).map_err(str::to_owned)?;
            if total + webvtt.len() > MAX_TOTAL_BYTES {
                return Err(format!(
                    "subtitles exceed {} KiB per video",
                    MAX_TOTAL_BYTES / 1024
                ));
            }
            let (language, forced, sdh) =
                metadata.clone().ok_or("ambiguous subtitle association")?;
            Ok(ExternalSubtitle {
                label: format!(
                    "{}{}{} · {}",
                    language.to_uppercase(),
                    if forced { " forcés" } else { "" },
                    if sdh { " SDH" } else { "" },
                    if matches!(extension.as_str(), "ass" | "ssa") {
                        format!(
                            "{} (texte, styles non reproduits)",
                            extension.to_uppercase()
                        )
                    } else {
                        extension.to_uppercase()
                    }
                ),
                language,
                forced,
                sdh,
                source_format: extension,
                webvtt,
            })
        }
        .await;
        match result {
            Ok(track) => {
                total += track.webvtt.len();
                tracks.push(track);
            }
            Err(message) => issues.push(ScanIssue {
                relative_path,
                message,
            }),
        }
    }
    (tracks, issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ass_and_ssa_text_fallback_preserves_times_commas_and_escapes() {
        for format in ["ass", "ssa"] {
            let vtt = to_webvtt("[Script Info]\nTitle: Test\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.25,0:00:03.50,Default,,0,0,0,,{\\i1}Bonjour, monde{\\i0}\\N<script> & suite", format).unwrap();
            assert!(vtt.contains("00:00:01.250 --> 00:00:03.500"));
            assert!(vtt.contains("Bonjour, monde\n&lt;script&gt; &amp; suite"));
            assert!(!vtt.contains("\\i1"));
        }
        assert!(
            to_webvtt(
                "[Events]\nFormat: Start, End, Text\nDialogue: 0:00:03.00,0:00:01.00,wrong",
                "ass"
            )
            .is_err()
        );
    }
    #[test]
    fn conversion_validates_times_and_escapes_markup() {
        assert_eq!(
            to_webvtt(
                "1\r\n00:00:01,250 --> 00:00:03,000\r\n<script>A & B</script>\r\n",
                "srt"
            )
            .unwrap(),
            "WEBVTT\n\n00:00:01.250 --> 00:00:03.000\n&lt;script&gt;A &amp; B&lt;/script&gt;\n\n"
        );
        assert!(to_webvtt("1\n00:00:03,000 --> 00:00:01,000\nOops", "srt").is_err());
        let partial = to_webvtt(
            "1\n00:00:03,000 --> 00:00:01,000\nOops\n\n2\n00:00:04,000 --> 00:00:05,000\nValid",
            "srt",
        )
        .unwrap();
        assert!(partial.contains("00:00:04.000 --> 00:00:05.000"));
        assert!(!partial.contains("Oops"));
        assert!(to_webvtt("WEBVTT\n\n00:61.000 --> 00:62.000\nOops", "vtt").is_err());
        assert!(to_webvtt("[Events]", "ass").is_err());
    }
    #[test]
    fn association_is_exact_and_suffixes_are_explicit() {
        assert_eq!(
            suffix("film", "film.fr.forced"),
            Some(("fr".into(), true, false))
        );
        assert!(suffix("film", "film2.fr").is_none());
        assert!(suffix("film", "film.directors.cut.fr").is_none());
        assert!(suffix("film", "film.fr.en").is_none());
    }
    #[tokio::test]
    async fn discovery_rejects_ambiguous_stems_and_refreshes_sidecars() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        tokio::fs::write(temp.path().join("film.mp4"), b"video").await?;
        tokio::fs::write(
            temp.path().join("film.fr.forced.srt"),
            b"1\n00:00:01,000 --> 00:00:03,000\nBonjour",
        )
        .await?;
        let (tracks, issues) = discover(temp.path(), Path::new("film.mp4")).await;
        assert_eq!(tracks.len(), 1);
        assert!(issues.is_empty());
        assert!(tracks[0].forced);
        assert_eq!(tracks[0].label, "FR forcés · SRT");
        tokio::fs::write(temp.path().join("film.mkv"), b"other video").await?;
        let (tracks, issues) = discover(temp.path(), Path::new("film.mp4")).await;
        assert!(tracks.is_empty());
        assert!(issues[0].message.contains("ambiguous"));
        Ok(())
    }
}
