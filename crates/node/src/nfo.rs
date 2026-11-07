//! Bounded, local-only film NFO ingestion. No URL or external entity resolver exists.
//!
//! Reads the NFO dialects written by Radarr, Sonarr, Jellyfin and Kodi (`<movie>`, `<tvshow>`,
//! `<episodedetails>`, legacy `<id TMDB=…>` attributes, `<uniqueid type=…>`, `<tmdbid>`-style
//! elements, and the URL-only / URL-suffixed files older scrapers produce). External identifiers
//! are never trusted blindly: placeholders (`0`) are dropped, values are normalised, and a provider
//! that appears with two different values is removed and reported in
//! `FilmMetadata::external_id_conflicts` instead of picking a winner or discarding the whole file.
pub use media_contracts::media::{FilmMetadata, NfoOutcome};
use quick_xml::{Reader, events::Event};
use std::path::Path;
use tokio::io::AsyncReadExt;

use crate::{files::OpenedFile, scanner::FolderVideos};

pub const MAX_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_FIELD: usize = 16 * 1024;
/// Text outside the XML root (URL-only or URL-suffixed NFOs) is scanned up to this length.
const MAX_OUTSIDE_TEXT: usize = 4096;

/// Provider aliases seen in the wild, mapped to Aether's canonical names. `None` marks a
/// provider name that carries no identity (Kodi writes `type="unknown"`).
fn canonical_provider(provider: &str) -> Option<String> {
    let lower = provider.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" | "unknown" | "default" | "none" | "null" => None,
        "tmdb" | "themoviedb" | "tmdbid" | "tmdb_id" => Some("tmdb".into()),
        "imdb" | "imdbid" | "imdb_id" => Some("imdb".into()),
        "tvdb" | "thetvdb" | "tvdbid" | "tvdb_id" => Some("tvdb".into()),
        _ => Some(lower),
    }
}

/// `Ok(None)` for a placeholder value (Radarr/Sonarr write `0` when the id is unknown),
/// `Err` for a malformed value, otherwise the canonical form (no leading zeros, lowercase `tt…`).
fn normalize_id(provider: &str, value: &str) -> Result<Option<String>, String> {
    let value = value.trim();
    let invalid = || Err("invalid explicit provider identifier".to_owned());
    match provider {
        "tmdb" | "tvdb" => {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return invalid();
            }
            let trimmed = value.trim_start_matches('0');
            Ok((!trimmed.is_empty()).then(|| trimmed.to_owned()))
        }
        "imdb" => {
            let lower = value.to_ascii_lowercase();
            match lower.strip_prefix("tt") {
                Some(digits)
                    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    Ok((!digits.bytes().all(|b| b == b'0')).then_some(lower))
                }
                _ => invalid(),
            }
        }
        // Unknown explicitly named providers remain opaque, never interpreted as URLs.
        _ => {
            if provider.is_empty() || provider.len() > 64 || value.len() > 256 || value.is_empty() {
                invalid()
            } else {
                Ok(Some(value.to_owned()))
            }
        }
    }
}

fn insert_id(metadata: &mut FilmMetadata, provider: &str, value: &str) -> Result<(), String> {
    let Some(provider) = canonical_provider(provider) else {
        return Ok(());
    };
    let Some(value) = normalize_id(&provider, value)? else {
        return Ok(());
    };
    if metadata.external_id_conflicts.contains(&provider) {
        return Ok(());
    }
    match metadata.external_ids.get(&provider) {
        Some(previous) if *previous != value => {
            // Two sources in one file disagree: trust neither.
            metadata.external_ids.remove(&provider);
            metadata.external_id_conflicts.push(provider);
        }
        Some(_) => {}
        None => {
            metadata.external_ids.insert(provider, value);
        }
    }
    Ok(())
}

/// Identifiers embedded in provider URLs (`https://www.themoviedb.org/movie/603`,
/// `https://www.imdb.com/title/tt0133093/`, `https://thetvdb.com/?tab=series&id=81189`).
/// Text only: nothing is ever fetched.
fn ids_from_urls(metadata: &mut FilmMetadata, text: &str) -> Result<(), String> {
    fn after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
        text.find(marker).map(|i| &text[i + marker.len()..])
    }
    fn digits(text: &str) -> &str {
        let end = text.bytes().take_while(u8::is_ascii_digit).count();
        &text[..end]
    }
    let lower = text.to_ascii_lowercase();
    for marker in ["themoviedb.org/movie/", "themoviedb.org/tv/"] {
        if let Some(rest) = after(&lower, marker) {
            let id = digits(rest);
            if !id.is_empty() {
                insert_id(metadata, "tmdb", id)?;
            }
        }
    }
    if let Some(rest) = after(&lower, "imdb.com/title/tt") {
        let id = digits(rest);
        if !id.is_empty() {
            insert_id(metadata, "imdb", &format!("tt{id}"))?;
        }
    }
    if lower.contains("thetvdb.com")
        && let Some(rest) = after(&lower, "id=").or_else(|| after(&lower, "/series/"))
    {
        let id = digits(rest);
        if !id.is_empty() {
            insert_id(metadata, "tvdb", id)?;
        }
    }
    Ok(())
}

fn valid_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return false;
    }
    let year = text[..4].parse::<u16>().unwrap_or(0);
    let month = text[5..7].parse::<u8>().unwrap_or(0);
    let day = text[8..].parse::<u8>().unwrap_or(0);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
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
    (1888..=2100).contains(&year) && day > 0 && day <= max
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
        "originallanguage" | "original_language" => {
            // Explicit language metadata only; title/script/audio do not prove VO.
            if text.len() <= 35
                && text.split('-').enumerate().all(|(i, part)| {
                    if i == 0 {
                        (2..=8).contains(&part.len())
                            && part.bytes().all(|b| b.is_ascii_alphabetic())
                    } else {
                        !part.is_empty()
                            && part.len() <= 8
                            && part.bytes().all(|b| b.is_ascii_alphanumeric())
                    }
                })
            {
                metadata.original_language = Some(text.to_ascii_lowercase());
            }
        }
        "plot" => metadata.plot = Some(text.into()),
        "outline" => metadata.outline = Some(text.into()),
        "mpaa" => metadata.mpaa = Some(text.into()),
        "tag" => {
            if metadata.tags.len() >= 64 || text.len() > 100 {
                return Err("too many tags or tag too long".into());
            }
            let tag = text.to_lowercase();
            if !metadata.tags.contains(&tag) {
                metadata.tags.push(tag);
            }
        }
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
            if valid_date(text) {
                metadata.premiered = Some(text.into());
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
            // `premiered` is the authoritative date when both are present, whatever their order.
            if valid_date(text) && metadata.premiered.is_none() {
                metadata.premiered = Some(text.into());
            }
        }
        "uniqueid" if !provider.is_empty() => insert_id(metadata, provider, text)?,
        "tmdbid" | "tmdb" | "tmdb_id" => insert_id(metadata, "tmdb", text)?,
        "imdbid" | "imdb" | "imdb_id" => insert_id(metadata, "imdb", text)?,
        "tvdbid" | "tvdb" | "tvdb_id" => insert_id(metadata, "tvdb", text)?,
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
    // Kodi-era scrapers wrote a provider URL before or after the XML (or instead of it). The
    // text is only searched for identifiers; anything else outside the root stays an error.
    let (xml, outside) = split_outside_root(xml);
    let mut url_ids = FilmMetadata::default();
    // Stray text is only reported once the XML is known to be complete: in a truncated file
    // the text after the last `>` is the unfinished content of an element, not outside it.
    let mut stray_outside = false;
    if !outside.trim().is_empty() {
        if outside.len() > MAX_OUTSIDE_TEXT {
            return Err("text outside root".into());
        }
        ids_from_urls(&mut url_ids, &outside)?;
        stray_outside = url_ids.external_ids.is_empty() && url_ids.external_id_conflicts.is_empty();
        if xml.is_empty() {
            return if stray_outside {
                Err("text outside root".into())
            } else {
                Ok(url_ids)
            };
        }
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_comments = true;
    let mut stack: Vec<(String, String, String)> = Vec::new();
    let mut metadata = FilmMetadata::default();
    let mut roots = 0;
    let mut first_root = String::new();
    let mut extra_episode_roots = 0usize;
    // Depth inside an additional <episodedetails> of a multi-episode file (Sonarr writes one
    // per episode); only the first one describes this file, the others are skipped.
    let mut ignored_depth = 0usize;
    let mut actor = media_contracts::media::Credit::default();
    let mut rating_max = 10.0;
    loop {
        let event = reader.read_event().map_err(|_| "malformed NFO XML")?;
        if ignored_depth > 0 {
            match event {
                Event::Start(_) => {
                    ignored_depth += 1;
                    if ignored_depth > MAX_DEPTH {
                        return Err("NFO XML depth limit".into());
                    }
                }
                Event::End(_) => ignored_depth -= 1,
                Event::DocType(_) => {
                    return Err("DTD and entity declarations forbidden".into());
                }
                Event::Eof => return Err("incomplete NFO XML".into()),
                _ => (),
            }
            continue;
        }
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().as_ref().to_ascii_lowercase();
                if stack.is_empty() {
                    if roots == 0 {
                        roots = 1;
                        if name != "movie" && name != "tvshow" && name != "episodedetails" {
                            return Err(
                                "NFO requires one valid root (movie, tvshow, or episodedetails)"
                                    .into(),
                            );
                        }
                        first_root = name.clone();
                    } else if name == "episodedetails" && first_root == "episodedetails" {
                        extra_episode_roots += 1;
                        if extra_episode_roots > 32 {
                            return Err("too many episodes in one NFO".into());
                        }
                        if !empty {
                            ignored_depth = 1;
                        }
                        continue;
                    } else {
                        return Err(
                            "NFO requires one valid root (movie, tvshow, or episodedetails)".into(),
                        );
                    }
                }
                if stack.len() >= MAX_DEPTH {
                    return Err("NFO XML depth limit".into());
                }
                if name == "rating" {
                    rating_max = 10.0;
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
                    // Jellyfin/Kodi legacy movie NFOs may carry IDs on <id>,
                    // including an empty element. Never interpret its numeric text as IMDb.
                    if name == "id" && stack.len() == 1 {
                        let key = attribute.key.as_ref().to_ascii_lowercase();
                        if matches!(key.as_str(), "tmdb" | "imdb" | "tvdb")
                            && !value.trim().is_empty()
                        {
                            insert_id(&mut metadata, &key, value.trim())?;
                        }
                    }
                    if name == "rating" && attribute.key.as_ref() == "max" {
                        rating_max = value
                            .parse::<f64>()
                            .ok()
                            .filter(|v| v.is_finite() && *v > 0.0 && *v <= 100.0)
                            .unwrap_or(10.0);
                    }
                    if attribute.key.as_ref() == "type" || attribute.key.as_ref() == "name" {
                        provider = value.to_ascii_lowercase();
                    }
                }
                if name == "actor" {
                    actor = Default::default();
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
                let text = text.trim();
                if stack.len() == 2 && stack[1].0 == "actor" {
                    if name == "name" {
                        actor.name = text.to_owned();
                    }
                    if name == "role" {
                        actor.role = text.to_owned();
                    }
                    if name == "thumb"
                        && actor.thumb.is_none()
                        && text.len() <= 2048
                        && text.starts_with("https://")
                    {
                        actor.thumb = Some(text.to_owned());
                    }
                }
                if name == "actor"
                    && stack.len() == 1
                    && !actor.name.is_empty()
                    && metadata.actors.len() < 100
                {
                    metadata.actors.push(std::mem::take(&mut actor));
                }
                if stack.len() == 1 && !text.is_empty() {
                    if name == "director" && metadata.directors.len() < 40 {
                        metadata.directors.push(text.into());
                    }
                    if name == "credits" && metadata.writers.len() < 40 {
                        metadata.writers.push(text.into());
                    }
                }
                if name == "value"
                    && stack.len() == 3
                    && stack[1].0 == "ratings"
                    && stack[2].0 == "rating"
                    && let Ok(value) = text.parse::<f64>()
                    && value.is_finite()
                    && (0.0..=rating_max).contains(&value)
                    && metadata.ratings.len() < 20
                {
                    metadata
                        .ratings
                        .push(media_contracts::media::ProviderRating {
                            provider: stack[2].1.clone(),
                            value,
                            max: rating_max,
                        });
                    if metadata.rating.is_none() {
                        metadata.rating = Some(value * 10.0 / rating_max);
                    }
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
    if stray_outside {
        return Err("text outside root".into());
    }
    for (provider, value) in &url_ids.external_ids {
        insert_id(&mut metadata, provider, value)?;
    }
    for provider in url_ids.external_id_conflicts {
        if !metadata.external_id_conflicts.contains(&provider) {
            metadata.external_ids.remove(&provider);
            metadata.external_id_conflicts.push(provider);
        }
    }
    if metadata.year.is_none() {
        metadata.year = metadata
            .premiered
            .as_ref()
            .and_then(|v| v[..4].parse().ok());
    }
    Ok(metadata)
}

/// The XML between the first `<` and the last `>`, and the text around it. Without any markup the
/// whole input is "outside" (URL-only NFO).
fn split_outside_root(text: &str) -> (&str, String) {
    match (text.find('<'), text.rfind('>')) {
        (Some(first), Some(last)) if first < last => (
            &text[first..=last],
            format!("{} {}", &text[..first], &text[last + 1..]),
        ),
        _ => ("", text.to_owned()),
    }
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

/// Identifiers carried by a path name and by an NFO. Agreement and one-sided values are kept; a
/// provider with two different values is dropped from the result and reported, because neither
/// side is more trustworthy than the other. A provider the NFO itself contradicts keeps the
/// name's value (a single explicit source) but is still reported.
pub fn reconcile_ids(
    from_name: &std::collections::BTreeMap<String, String>,
    nfo: &FilmMetadata,
) -> (std::collections::BTreeMap<String, String>, Vec<String>) {
    let mut merged = from_name.clone();
    let mut conflicts: Vec<String> = nfo.external_id_conflicts.clone();
    for (provider, value) in &nfo.external_ids {
        match merged.get(provider) {
            Some(existing) if existing != value => {
                merged.remove(provider);
                if !conflicts.contains(provider) {
                    conflicts.push(provider.clone());
                }
            }
            Some(_) => {}
            None if conflicts.contains(provider) => {}
            None => {
                merged.insert(provider.clone(), value.clone());
            }
        }
    }
    (merged, conflicts)
}

/// Human-readable warning for an NFO whose identifiers contradict each other, if any.
pub fn conflict_message(providers: &[String]) -> Option<String> {
    (!providers.is_empty()).then(|| {
        format!(
            "contradictory {} identifiers ignored; no automatic match on them",
            providers.join(", ")
        )
    })
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
    #[test]
    fn reads_only_explicit_original_language_and_never_infers_it_from_title_or_audio() {
        assert_eq!(
            parse(br#"<movie><original_language>ja</original_language></movie>"#)
                .unwrap()
                .original_language
                .as_deref(),
            Some("ja")
        );
        assert_eq!(
            parse(br#"<movie><originallanguage>en-US</originallanguage></movie>"#)
                .unwrap()
                .original_language
                .as_deref(),
            Some("en-us")
        );
        assert!(parse(br#"<movie><originaltitle>English title</originaltitle><language>fr</language></movie>"#).unwrap().original_language.is_none());
        assert!(
            parse(br#"<movie><originallanguage>fr--CA</originallanguage></movie>"#)
                .unwrap()
                .original_language
                .is_none()
        );
    }
    #[test]
    fn parses_jellyfin_legacy_id_attributes_and_provider_aliases() {
        let m =
            parse(br#"<movie><id TMDB="27205" IMDB="tt1375666" TVDB="100" /></movie>"#).unwrap();
        assert_eq!(m.external_ids["tmdb"], "27205");
        assert_eq!(m.external_ids["imdb"], "tt1375666");
        assert_eq!(m.external_ids["tvdb"], "100");
        let m =
            parse(br#"<movie><imdb_id>tt1375666</imdb_id><tvdbid>100</tvdbid></movie>"#).unwrap();
        assert_eq!(m.external_ids["imdb"], "tt1375666");
        assert_eq!(m.external_ids["tvdb"], "100");
        // Contradictory ids no longer reject the whole NFO: the provider is dropped and reported.
        let conflicted =
            parse(br#"<movie><title>Kept</title><id TMDB="27205"/><tmdbid>1</tmdbid></movie>"#)
                .unwrap();
        assert_eq!(conflicted.title.as_deref(), Some("Kept"));
        assert!(!conflicted.external_ids.contains_key("tmdb"));
        assert_eq!(conflicted.external_id_conflicts, ["tmdb"]);
        assert!(parse(br#"<movie><id IMDB="123"/></movie>"#).is_err());
        assert!(
            parse(br#"<movie><id>27205</id></movie>"#)
                .unwrap()
                .external_ids
                .is_empty()
        );
    }
    use super::*;
    #[test]
    fn truncated_documents_are_incomplete_and_stray_text_is_outside_root() {
        // The unfinished title is inside the unclosed root, not text after it.
        assert_eq!(
            parse(b"<tvshow><title>Unclosed tag").unwrap_err(),
            "incomplete NFO XML"
        );
        assert_eq!(
            parse(b"<tvshow><title>Kept</title></tvshow> trailing words").unwrap_err(),
            "text outside root"
        );
        assert_eq!(parse(b"only words").unwrap_err(), "text outside root");
        // A provider URL beside a complete document is still read.
        let with_url =
            parse(b"<movie><title>Kept</title></movie>\nhttps://www.themoviedb.org/movie/27205")
                .unwrap();
        assert_eq!(with_url.external_ids["tmdb"], "27205");
    }
    #[test]
    fn keeps_only_remote_https_actor_portraits() {
        let meta = parse(br#"<movie><actor><name>A</name><thumb>https://image.example.org/a.jpg</thumb></actor><actor><name>B</name><thumb>/config/metadata/People/B/folder.jpg</thumb></actor><actor><name>C</name><thumb>http://image.example.org/c.jpg</thumb></actor></movie>"#).unwrap();
        assert_eq!(
            meta.actors[0].thumb.as_deref(),
            Some("https://image.example.org/a.jpg")
        );
        assert_eq!(meta.actors[1].thumb, None);
        assert_eq!(meta.actors[2].thumb, None);
    }
    #[test]
    fn preserves_credits_and_provider_scales() {
        let meta=parse(br#"<tvshow><actor><name>Actor</name><role>Role</role></actor><director>Director</director><credits>Writer</credits><ratings><rating name="tomatometer" max="100"><value>95</value></rating><rating name="imdb"><value>8.4</value></rating></ratings></tvshow>"#).unwrap();
        assert_eq!(meta.actors[0].role, "Role");
        assert_eq!(meta.actors[0].thumb, None);
        assert_eq!(meta.writers, ["Writer"]);
        assert_eq!(meta.ratings[0].max, 100.0);
        assert_eq!(meta.ratings[1].max, 10.0);
        assert_eq!(meta.ratings[1].provider, "imdb");
    }

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
    #[test]
    fn radarr_sonarr_jellyfin_and_kodi_dialects_yield_the_same_reliable_ids() {
        // Radarr: uniqueid + explicit elements, placeholder imdb.
        let radarr = parse(br#"<movie><title>Heat</title><uniqueid type="tmdb" default="true">949</uniqueid><uniqueid type="imdb">tt0113277</uniqueid><tmdbid>949</tmdbid><imdbid>tt0113277</imdbid></movie>"#).unwrap();
        assert_eq!(radarr.external_ids["tmdb"], "949");
        assert_eq!(radarr.external_ids["imdb"], "tt0113277");
        assert!(radarr.external_id_conflicts.is_empty());
        // Sonarr: tvdb + imdb on the show, tvdb on the episode; `0` is "unknown", not an id.
        let sonarr = parse(br#"<tvshow><title>Show</title><tvdbid>81189</tvdbid><uniqueid type="tvdb" default="true">81189</uniqueid><uniqueid type="imdb">tt0903747</uniqueid><tmdbid>0</tmdbid></tvshow>"#).unwrap();
        assert_eq!(sonarr.external_ids["tvdb"], "81189");
        assert!(!sonarr.external_ids.contains_key("tmdb"));
        // Jellyfin: upper-case provider attributes and bare elements.
        let jellyfin = parse(
            br#"<movie><uniqueid type="Tmdb">00949</uniqueid><IMDbId>TT0113277</IMDbId></movie>"#,
        )
        .unwrap();
        assert_eq!(
            jellyfin.external_ids["tmdb"], "949",
            "leading zeros are not part of the id"
        );
        assert_eq!(jellyfin.external_ids["imdb"], "tt0113277");
        // Kodi: `type="unknown"` carries no identity; themoviedb/thetvdb are aliases.
        let kodi = parse(br#"<movie><uniqueid type="unknown">123456</uniqueid><uniqueid type="themoviedb">949</uniqueid></movie>"#).unwrap();
        assert_eq!(kodi.external_ids.len(), 1);
        assert_eq!(kodi.external_ids["tmdb"], "949");
    }

    #[test]
    fn contradictory_ids_are_dropped_and_reported_not_trusted() {
        let m = parse(br#"<movie><title>Film</title><uniqueid type="tmdb">1</uniqueid><tmdbid>2</tmdbid><uniqueid type="tvdb">9</uniqueid></movie>"#).unwrap();
        assert!(!m.external_ids.contains_key("tmdb"));
        assert_eq!(m.external_ids["tvdb"], "9", "unrelated providers survive");
        assert_eq!(m.external_id_conflicts, ["tmdb"]);
        // A third value cannot resurrect a dropped provider.
        let m = parse(br#"<movie><tmdbid>1</tmdbid><tmdbid>2</tmdbid><tmdbid>1</tmdbid></movie>"#)
            .unwrap();
        assert!(m.external_ids.is_empty());
        assert_eq!(m.external_id_conflicts, ["tmdb"]);
        // Malformed values remain hard errors: that file is not an NFO we understand.
        assert!(parse(br#"<movie><imdbid>12</imdbid></movie>"#).is_err());
        assert!(parse(br#"<movie><tmdbid>abc</tmdbid></movie>"#).is_err());
    }

    #[test]
    fn url_only_and_url_suffixed_kodi_nfos_provide_ids_without_resolving_anything() {
        let only = parse(b"https://www.themoviedb.org/movie/949-heat\n").unwrap();
        assert_eq!(only.external_ids["tmdb"], "949");
        assert!(only.title.is_none());
        let imdb = parse(b"https://www.imdb.com/title/tt0113277/").unwrap();
        assert_eq!(imdb.external_ids["imdb"], "tt0113277");
        let tvdb = parse(b"https://thetvdb.com/?tab=series&id=81189").unwrap();
        assert_eq!(tvdb.external_ids["tvdb"], "81189");
        let suffixed =
            parse(b"<movie><title>Heat</title></movie>\nhttps://www.themoviedb.org/movie/949\n")
                .unwrap();
        assert_eq!(suffixed.title.as_deref(), Some("Heat"));
        assert_eq!(suffixed.external_ids["tmdb"], "949");
        // The URL disagrees with the XML: neither is trusted.
        let disagree =
            parse(b"<movie><tmdbid>1</tmdbid></movie>\nhttps://www.themoviedb.org/movie/949\n")
                .unwrap();
        assert!(disagree.external_ids.is_empty());
        assert_eq!(disagree.external_id_conflicts, ["tmdb"]);
        // Arbitrary text outside the root is still rejected.
        assert!(parse(b"<movie/> trailing garbage").is_err());
        assert!(parse(b"just some words").is_err());
        assert!(parse(b"").is_err());
    }

    #[test]
    fn multi_episode_nfo_uses_the_first_episode_and_stays_bounded() {
        let m = parse(br#"<episodedetails><title>Part 1</title><season>1</season><episode>1</episode><uniqueid type="tvdb">11</uniqueid></episodedetails>
<episodedetails><title>Part 2</title><season>1</season><episode>2</episode><uniqueid type="tvdb">22</uniqueid></episodedetails>"#).unwrap();
        assert_eq!(m.title.as_deref(), Some("Part 1"));
        assert_eq!(m.episode, Some(1));
        assert_eq!(m.external_ids["tvdb"], "11");
        assert!(m.external_id_conflicts.is_empty());
        // A different second root, an unbalanced extra episode and a flood are all rejected.
        assert!(parse(b"<episodedetails/><movie/>").is_err());
        assert!(parse(b"<episodedetails/><episodedetails><x>").is_err());
        let flood = "<episodedetails/>".repeat(40);
        assert!(parse(flood.as_bytes()).is_err());
        assert!(parse(b"<movie/><episodedetails/>").is_err());
    }

    #[test]
    fn premiered_wins_over_aired_in_any_order() {
        for xml in [
            "<episodedetails><aired>2020-01-01</aired><premiered>2019-05-05</premiered></episodedetails>",
            "<episodedetails><premiered>2019-05-05</premiered><aired>2020-01-01</aired></episodedetails>",
        ] {
            assert_eq!(
                parse(xml.as_bytes()).unwrap().premiered.as_deref(),
                Some("2019-05-05")
            );
        }
        assert_eq!(
            parse(b"<episodedetails><aired>2021-02-29</aired></episodedetails>")
                .unwrap()
                .premiered,
            None
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
