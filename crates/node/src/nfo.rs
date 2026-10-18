//! Bounded, local-only film NFO ingestion. No URL or external entity resolver exists.
pub use media_contracts::media::{FilmMetadata, NfoOutcome};
use quick_xml::{Reader, events::Event};
use std::path::Path;
use tokio::io::AsyncReadExt;

use crate::{files::OpenedFile, scanner::FolderVideos};

pub const MAX_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_FIELD: usize = 16 * 1024;

fn insert_id(metadata: &mut FilmMetadata, provider: &str, value: &str) -> Result<(), String> {
    let valid = match provider {
        "tmdb" | "tvdb" => !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()),
        "imdb" => value
            .strip_prefix("tt")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
        // Unknown explicitly named providers remain opaque, never interpreted as URLs.
        _ => {
            !provider.is_empty() && provider.len() <= 64 && value.len() <= 256 && !value.is_empty()
        }
    };
    if !valid {
        return Err("invalid explicit provider identifier".into());
    }
    if metadata
        .external_ids
        .get(provider)
        .is_some_and(|previous| previous != value)
    {
        return Err(format!("conflicting {provider} identifiers"));
    }
    metadata
        .external_ids
        .insert(provider.to_owned(), value.to_owned());
    Ok(())
}

fn apply(
    metadata: &mut FilmMetadata,
    name: &str,
    provider: &str,
    text: &str,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    match name {
        "title" => metadata.title = Some(text.into()),
        "originaltitle" => metadata.original_title = Some(text.into()),
        "plot" => metadata.plot = Some(text.into()),
        "outline" => metadata.outline = Some(text.into()),
        "mpaa" => metadata.mpaa = Some(text.into()),
        "genre" => {
            if metadata.genres.len() >= 64 {
                return Err("too many genres".into());
            }
            if !metadata.genres.iter().any(|g| g == text) {
                metadata.genres.push(text.into());
            }
        }
        "year" => {
            metadata.year = text
                .parse::<u16>()
                .ok()
                .filter(|v| (1888..=2100).contains(v))
        }
        "premiered" => {
            let bytes = text.as_bytes();
            if bytes.len() == 10
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
            {
                let year = text[..4].parse::<u16>().unwrap_or(0);
                let month = text[5..7].parse::<u8>().unwrap_or(0);
                let day = text[8..].parse::<u8>().unwrap_or(0);
                let leap = year.is_multiple_of(4)
                    && (!year.is_multiple_of(100) || year.is_multiple_of(400));
                let max = match month {
                    2 => {
                        if leap {
                            29
                        } else {
                            28
                        }
                    }
                    4 | 6 | 9 | 11 => 30,
                    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                    _ => 0,
                };
                if (1888..=2100).contains(&year) && day > 0 && day <= max {
                    metadata.premiered = Some(text.into());
                }
            }
        }
        "rating" => {
            metadata.rating = text
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && (0.0..=10.0).contains(n))
        }
        "season" => metadata.season = text.parse::<u16>().ok(),
        "episode" => metadata.episode = text.parse::<u16>().ok(),
        "aired" => {
            let bytes = text.as_bytes();
            if bytes.len() == 10
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
            {
                let year = text[..4].parse::<u16>().unwrap_or(0);
                let month = text[5..7].parse::<u8>().unwrap_or(0);
                let day = text[8..].parse::<u8>().unwrap_or(0);
                let leap = year.is_multiple_of(4)
                    && (!year.is_multiple_of(100) || year.is_multiple_of(400));
                let max = match month {
                    2 => {
                        if leap {
                            29
                        } else {
                            28
                        }
                    }
                    4 | 6 | 9 | 11 => 30,
                    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                    _ => 0,
                };
                if (1888..=2100).contains(&year) && day > 0 && day <= max {
                    metadata.premiered = Some(text.into());
                }
            }
        }
        "uniqueid" if !provider.is_empty() => insert_id(metadata, provider, text)?,
        "tmdbid" | "tmdb" => insert_id(metadata, "tmdb", text)?,
        "imdbid" | "imdb" => insert_id(metadata, "imdb", text)?,
        "id" if text.starts_with("tt") => insert_id(metadata, "imdb", text)?,
        _ => (),
    }
    Ok(())
}

pub fn parse(bytes: &[u8]) -> Result<FilmMetadata, String> {
    if bytes.len() > MAX_BYTES {
        return Err("NFO exceeds 1 MiB".into());
    }
    let xml = std::str::from_utf8(bytes)
        .map_err(|_| "NFO must be valid UTF-8")?
        .trim_start_matches('\u{feff}');
    if xml
        .chars()
        .any(|c| (c as u32) < 0x20 && !matches!(c, '\t' | '\r' | '\n'))
    {
        return Err("invalid XML control character".into());
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_comments = true;
    let mut stack: Vec<(String, String, String)> = Vec::new();
    let mut metadata = FilmMetadata::default();
    let mut roots = 0;
    loop {
        let event = reader.read_event().map_err(|_| "malformed NFO XML")?;
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                if stack.is_empty() {
                    roots += 1;
                    if roots != 1
                        || (name != "movie" && name != "tvshow" && name != "episodedetails")
                    {
                        return Err(
                            "NFO requires one valid root (movie, tvshow, or episodedetails)".into(),
                        );
                    }
                }
                if stack.len() >= MAX_DEPTH {
                    return Err("NFO XML depth limit".into());
                }
                let mut provider = String::new();
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(|_| "invalid XML attribute")?;
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| "invalid XML attribute entity")?;
                    if value.len() > MAX_FIELD {
                        return Err("XML attribute too large".into());
                    }
                    if attribute.key.as_ref() == "type" {
                        provider = value.to_ascii_lowercase();
                    }
                }
                if !empty {
                    stack.push((name, provider, String::new()));
                }
            }
            Event::End(_) => {
                let Some((name, provider, text)) = stack.pop() else {
                    return Err("unbalanced NFO".into());
                };
                if stack.len() == 1 {
                    apply(&mut metadata, &name, &provider, &text)?;
                }
                if name == "value"
                    && stack.len() == 3
                    && stack[1].0 == "ratings"
                    && stack[2].0 == "rating"
                {
                    apply(&mut metadata, "rating", "", &text)?;
                }
            }
            Event::Text(e) => {
                let text = e.xml10_content();
                append(&mut stack, &text)?;
            }
            Event::CData(e) => append(&mut stack, &e.xml10_content())?,
            Event::GeneralRef(e) => {
                let reference = e.as_ref();
                let entity = format!("&{reference};");
                let text = quick_xml::escape::unescape(&entity)
                    .map_err(|_| "unknown or external entity forbidden")?;
                append(&mut stack, &text)?;
            }
            Event::DocType(_) => return Err("DTD and entity declarations forbidden".into()),
            Event::Decl(e) => {
                if let Some(encoding) = e.encoding() {
                    let encoding = encoding.map_err(|_| "invalid encoding")?;
                    if !encoding.eq_ignore_ascii_case("utf-8") {
                        return Err("only UTF-8 NFO is supported".into());
                    }
                }
            }
            Event::Eof => break,
            _ => (),
        }
    }
    if roots != 1 || !stack.is_empty() {
        return Err("incomplete NFO XML".into());
    }
    if metadata.year.is_none() {
        metadata.year = metadata
            .premiered
            .as_ref()
            .and_then(|v| v[..4].parse().ok());
    }
    Ok(metadata)
}

fn append(stack: &mut [(String, String, String)], text: &str) -> Result<(), String> {
    if text.chars().any(|c| {
        !matches!(c, '\t' | '\r' | '\n')
            && ((c as u32) < 0x20 || matches!(c, '\u{fffe}' | '\u{ffff}'))
    }) {
        return Err("invalid XML character reference".into());
    }
    if stack.is_empty() && !text.trim().is_empty() {
        return Err("text outside root".into());
    }
    for (_, _, field) in stack.iter_mut().skip(1) {
        if field.len().saturating_add(text.len()) > MAX_FIELD {
            return Err("NFO field limit".into());
        }
        field.push_str(text);
    }
    Ok(())
}

/// Exact filename sidecar wins. Shared movie.nfo is used only for one unambiguous
/// existing local identity group in a non-root directory.
pub async fn discover(root: &Path, video: &Path) -> NfoOutcome {
    discover_with_folder(root, video, None).await
}

pub async fn discover_with_folder(
    root: &Path,
    video: &Path,
    folder_videos: Option<&FolderVideos>,
) -> NfoOutcome {
    let exact = video.with_extension("nfo");
    let parent = video.parent().unwrap_or(Path::new(""));
    let shared = parent.join("movie.nfo");
    let candidates = [(exact, 0u8), (shared, 1u8)];
    for (relative, priority) in candidates {
        let exists = tokio::fs::symlink_metadata(root.join(&relative)).await;
        if exists
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        let path = relative.to_string_lossy().replace('\\', "/");
        let result = async {
            let opened = OpenedFile::open_in_root(root, &relative)
                .await
                .map_err(|_| "NFO unavailable or unconfined")?;
            if priority == 1 {
                if parent.as_os_str().is_empty() {
                    return Err("shared movie.nfo forbidden at library root".to_owned());
                }
                let key = super::scanner::identity(video).3;
                let folder_key =
                    super::scanner::identity(&parent.join("__unqualified_media__.mp4")).3;
                if !folder_key.starts_with("file:") && folder_key != key {
                    return Err("shared movie.nfo conflicts with the identified film folder".into());
                }
                let loaded_folder;
                let videos = match folder_videos {
                    Some(v) => v,
                    None => {
                        loaded_folder = FolderVideos::load(root, parent)
                            .await
                            .map_err(|_| "NFO directory unreadable")?;
                        &loaded_folder
                    }
                };
                if !videos.all_match_identity(&key) {
                    return Err("ambiguous shared movie.nfo: distinct films in directory".into());
                }
            }

            let check_handle = opened
                .try_clone()
                .map_err(|_| "NFO unreadable".to_owned())?;
            let file = opened.into_tokio_file();
            let mut bytes = Vec::new();
            file.take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| "NFO read failed")?;
            check_handle
                .verify_unchanged_sync()
                .map_err(|_| "NFO changed during read".to_owned())?;
            parse(&bytes)
        }
        .await;
        return match result {
            Ok(metadata) => NfoOutcome::Valid {
                metadata,
                path,
                priority,
            },
            Err(message) => NfoOutcome::Invalid { path, message },
        };
    }
    NfoOutcome::Missing
}

/// Discover tvshow.nfo in the series directory.
pub async fn discover_series_nfo(root: &Path, series_dir: &Path) -> NfoOutcome {
    let relative = series_dir.join("tvshow.nfo");
    read_and_parse_nfo(root, &relative, 0).await
}

/// Discover episode NFO with the exact same stem as the video file.
pub async fn discover_episode_nfo(root: &Path, video: &Path) -> NfoOutcome {
    let relative = video.with_extension("nfo");
    read_and_parse_nfo(root, &relative, 0).await
}

async fn read_and_parse_nfo(root: &Path, relative: &Path, priority: u8) -> NfoOutcome {
    let exists = tokio::fs::symlink_metadata(root.join(relative)).await;
    if exists
        .as_ref()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return NfoOutcome::Missing;
    }
    let path = relative.to_string_lossy().replace('\\', "/");
    let result = async {
        let opened = OpenedFile::open_in_root(root, relative)
            .await
            .map_err(|_| "NFO unavailable or unconfined")?;
        let check_handle = opened
            .try_clone()
            .map_err(|_| "NFO unreadable".to_owned())?;
        let file = opened.into_tokio_file();
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "NFO read failed")?;
        check_handle
            .verify_unchanged_sync()
            .map_err(|_| "NFO changed during read".to_owned())?;
        parse(&bytes)
    }
    .await;
    match result {
        Ok(metadata) => NfoOutcome::Valid {
            metadata,
            path,
            priority,
        },
        Err(message) => NfoOutcome::Invalid { path, message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_bounded_editorial_only_and_entities() {
        let m = parse(br#"<movie><title>A &amp; B &#233;</title><originaltitle>Original</originaltitle><plot><![CDATA[<script>evil()</script>]]></plot><genre>Drama</genre><genre>Comedy</genre><year>2024</year><premiered>2024-02-29</premiered><rating>8.5</rating><uniqueid type="tmdb">123</uniqueid><imdbid>tt123</imdbid><codec>fake</codec></movie>"#).unwrap();
        assert_eq!(m.title.as_deref(), Some("A & B é"));
        assert_eq!(m.genres.len(), 2);
        assert_eq!(m.external_ids["imdb"], "tt123");
        assert_eq!(m.rating, Some(8.5));
    }
    #[test]
    fn parses_tvshow_and_episodedetails_nfo() {
        let show = parse(br#"<tvshow><title>Breaking Code</title><plot>A thrilling drama.</plot><year>2024</year><genre>Drama</genre><uniqueid type="tvdb">81189</uniqueid></tvshow>"#).unwrap();
        assert_eq!(show.title.as_deref(), Some("Breaking Code"));
        assert_eq!(show.plot.as_deref(), Some("A thrilling drama."));
        assert_eq!(show.year, Some(2024));
        assert_eq!(show.external_ids["tvdb"], "81189");

        let ep = parse(br#"<episodedetails><title>Pilot</title><plot>The beginning.</plot><season>1</season><episode>1</episode><aired>2024-01-15</aired><rating>9.2</rating></episodedetails>"#).unwrap();
        assert_eq!(ep.title.as_deref(), Some("Pilot"));
        assert_eq!(ep.plot.as_deref(), Some("The beginning."));
        assert_eq!(ep.season, Some(1));
        assert_eq!(ep.episode, Some(1));
        assert_eq!(ep.premiered.as_deref(), Some("2024-01-15"));
        assert_eq!(ep.rating, Some(9.2));
    }
    #[test]
    fn rejects_hostile_malformed_and_resource_abuse() {
        for xml in [
            "<!DOCTYPE movie [<!ENTITY x SYSTEM 'file:///secret'>]><movie><title>&x;</title></movie>",
            "<!DOCTYPE movie [<!ENTITY a 'xxxx'>]><movie/>",
            "<movie><title>&unknown;</title></movie>",
            "<movie><title>&#0;</title></movie>",
            "<movie><title>&#x1;</title></movie>",
            "<movie><!-- invalid -- comment --></movie>",
            "<movie><title></movie>",
            "<movie/><movie/>",
            "<?xml version='1.0' encoding='ISO-8859-1'?><movie/>",
            "<movie><uniqueid type='tmdb'>1</uniqueid><tmdbid>2</tmdbid></movie>",
        ] {
            assert!(parse(xml.as_bytes()).is_err(), "{xml}");
        }
        assert!(parse(&vec![b'x'; MAX_BYTES + 1]).is_err());
        assert!(parse(b"<movie>\xff</movie>").is_err());
        assert!(
            parse(format!("<movie><plot>{}</plot></movie>", "a".repeat(MAX_FIELD + 1)).as_bytes())
                .is_err()
        );
        assert!(
            parse(format!("<movie>{}{}</movie>", "<x>".repeat(16), "</x>".repeat(16)).as_bytes())
                .is_err()
        );
    }
    #[tokio::test]
    async fn exact_priority_and_shared_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Film (2024)");
        tokio::fs::create_dir(&folder).await.unwrap();
        tokio::fs::write(folder.join("Film.mp4"), b"media")
            .await
            .unwrap();
        tokio::fs::write(
            folder.join("movie.nfo"),
            b"<movie><title>Shared</title></movie>",
        )
        .await
        .unwrap();
        assert!(matches!(
            discover(dir.path(), Path::new("Film (2024)/Film.mp4")).await,
            NfoOutcome::Valid { priority: 1, .. }
        ));
        tokio::fs::write(
            folder.join("Film.nfo"),
            b"<movie><title>Exact</title></movie>",
        )
        .await
        .unwrap();
        assert!(matches!(
            discover(dir.path(), Path::new("Film (2024)/Film.mp4")).await,
            NfoOutcome::Valid { priority: 0, .. }
        ));
        tokio::fs::remove_file(folder.join("Film.nfo"))
            .await
            .unwrap();
        tokio::fs::write(folder.join("Other (2025).mp4"), b"media")
            .await
            .unwrap();
        assert!(matches!(
            discover(dir.path(), Path::new("Film (2024)/Film.mp4")).await,
            NfoOutcome::Invalid { .. }
        ));
    }
    #[tokio::test]
    async fn rejects_oversize_sidecars_folder_conflicts_and_link_escape() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("library");
        let film = root.join("Film (2026)");
        let outside = temp.path().join("outside");
        tokio::fs::create_dir_all(&film).await.unwrap();
        tokio::fs::create_dir(&outside).await.unwrap();
        tokio::fs::write(film.join("Other (2025).mp4"), b"media")
            .await
            .unwrap();
        tokio::fs::write(
            film.join("movie.nfo"),
            b"<movie><title>Wrong film</title></movie>",
        )
        .await
        .unwrap();
        assert!(matches!(
            discover(&root, Path::new("Film (2026)/Other (2025).mp4")).await,
            NfoOutcome::Invalid { .. }
        ));
        tokio::fs::write(film.join("Other (2025).nfo"), vec![b'x'; MAX_BYTES + 1])
            .await
            .unwrap();
        assert!(
            matches!(discover(&root,Path::new("Film (2026)/Other (2025).mp4")).await,NfoOutcome::Invalid {message,..} if message.contains("1 MiB"))
        );
        tokio::fs::write(
            outside.join("movie.nfo"),
            b"<movie><title>Outside</title></movie>",
        )
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
        assert!(
            matches!(discover(&root,Path::new("escape/movie.mp4")).await,NfoOutcome::Invalid {message,..} if message.contains("unconfined"))
        );
    }
}
