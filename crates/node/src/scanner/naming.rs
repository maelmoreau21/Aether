use std::{collections::BTreeMap, path::Path};

pub(super) const RELEASE_TAGS: &[&str] = &[
    "1080p",
    "1080i",
    "720p",
    "720i",
    "2160p",
    "4k",
    "2k",
    "8k",
    "uhd",
    "ultrahd",
    "480p",
    "480i",
    "576p",
    "576i",
    "hdr",
    "hdr10",
    "hdr10plus",
    "dv",
    "dovi",
    "dolbyvision",
    "dolby-vision",
    "bluray",
    "blu-ray",
    "bdrip",
    "brrip",
    "bdr",
    "bd",
    "hdtv",
    "hdrip",
    "hdtvrip",
    "dvd",
    "dvdrip",
    "dvdscr",
    "dvdscreener",
    "screener",
    "webrip",
    "web-rip",
    "webdl",
    "web-dl",
    "web",
    "remux",
    "proper",
    "repack",
    "rerip",
    "unrated",
    "retail",
    "internal",
    "limited",
    "cam",
    "camrip",
    "telesync",
    "telecine",
    "x264",
    "x265",
    "h264",
    "h265",
    "hevc",
    "av1",
    "xvid",
    "divx",
    "divx5",
    "aac",
    "ac3",
    "eac3",
    "dts",
    "dtshd",
    "dts-hd",
    "truehd",
    "atmos",
    "flac",
    "multi",
    "multisubs",
    "vostfr",
    "vff",
    "vfq",
    "truefrench",
    "subfrench",
];

pub(super) fn is_delim(b: u8) -> bool {
    matches!(b, b'.' | b'_' | b'-' | b' ' | b'(' | b')' | b'[' | b']')
}

pub(super) fn metadata_name(name: &str) -> (String, Option<u16>, BTreeMap<String, String>) {
    let mut ids = BTreeMap::new();
    for token in name
        .split('[')
        .skip(1)
        .filter_map(|s| s.split_once(']').map(|p| p.0))
    {
        let pair = token
            .split_once('-')
            .or_else(|| token.split_once('='))
            .or_else(|| token.split_once(':'));
        if let Some((raw_provider, raw_id)) = pair {
            let provider = raw_provider
                .trim()
                .to_ascii_lowercase()
                .trim_end_matches("id")
                .to_string();
            let id = raw_id.trim();
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

    let mut working = name.trim();
    while working.starts_with('[') {
        if let Some(close_idx) = working.find(']') {
            let next = working[close_idx + 1..].trim_start();
            if !next.is_empty() {
                working = next;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    let bytes = working.as_bytes();
    let len = bytes.len();
    let mut year = None;
    let mut end = len;

    // 1. Check for explicit (YYYY) or [YYYY]
    for (start, _) in working.match_indices('(') {
        if let Some(value) = working.get(start + 1..start + 5)
            && working.as_bytes().get(start + 5) == Some(&b')')
            && let Ok(value) = value.parse::<u16>()
            && (1888..=2100).contains(&value)
        {
            year = Some(value);
            end = end.min(start);
            break;
        }
    }

    for (start, _) in working.match_indices('[') {
        if let Some(value) = working.get(start + 1..start + 5)
            && working.as_bytes().get(start + 5) == Some(&b']')
            && let Ok(value) = value.parse::<u16>()
            && (1888..=2100).contains(&value)
        {
            if year.is_none() {
                year = Some(value);
            }
            end = end.min(start);
            break;
        } else {
            end = end.min(start);
        }
    }

    // 2. If no parenthesized or bracketed year, search for 4-digit year 1900..=2099 delimited by dots/spaces/dashes
    if year.is_none() {
        let mut i = 0;
        while i + 4 <= end {
            let prev_delim = i == 0 || is_delim(bytes[i - 1]);
            let next_delim = i + 4 == len || is_delim(bytes[i + 4]);
            if prev_delim
                && next_delim
                && let Ok(val) = working[i..i + 4].parse::<u16>()
                && (1900..=2099).contains(&val)
                && i > 0
            {
                year = Some(val);
                end = end.min(i);
                break;
            }
            i += 1;
        }
    }

    // 3. Check for release tags before `end`
    let lower = working.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    for &tag in RELEASE_TAGS {
        let tag_len = tag.len();
        let mut pos = 0;
        while let Some(idx) = lower[pos..end].find(tag) {
            let actual_idx = pos + idx;
            let prev_delim = actual_idx == 0 || is_delim(lower_bytes[actual_idx - 1]);
            let next_delim = actual_idx + tag_len == len
                || actual_idx + tag_len == end
                || is_delim(lower_bytes[actual_idx + tag_len]);
            if prev_delim && next_delim && actual_idx > 0 {
                end = end.min(actual_idx);
                break;
            }
            pos = actual_idx + 1;
            if pos >= end {
                break;
            }
        }
    }

    let raw_slice = &working[..end];

    // Clean dots and underscores, replacing them with spaces
    let mut cleaned = String::with_capacity(raw_slice.len());
    for c in raw_slice.chars() {
        if c == '.' || c == '_' {
            cleaned.push(' ');
        } else {
            cleaned.push(c);
        }
    }

    // Collapse multiple consecutive spaces and trim
    let title: String = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['-', '.', '_', ' '])
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

pub(crate) fn is_generic_video_stem(stem: &str) -> bool {
    let s = stem.trim().to_ascii_lowercase();
    if matches!(
        s.as_str(),
        "movie"
            | "film"
            | "video"
            | "main"
            | "feature"
            | "featurette"
            | "play"
            | "default"
            | "untitled"
            | "complete"
    ) {
        return true;
    }
    if s.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | ' ' | '_' | '-'))
    {
        return true;
    }
    for prefix in ["cd", "part", "pt", "disc", "disk", "dvd"] {
        if let Some(rem) = s.strip_prefix(prefix) {
            let rem = rem.trim_start_matches(['.', ' ', '_', '-']);
            if !rem.is_empty() && rem.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
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

    let use_folder = match &parsed_folder {
        Some((folder_title, folder_year, folder_ids)) if !conflict => {
            let f_lower = folder_title.to_ascii_lowercase();
            let p_lower = parsed_file.0.to_ascii_lowercase();
            folder_year.is_some()
                || !folder_ids.is_empty()
                || is_generic_video_stem(stem)
                || parsed_file.0.eq_ignore_ascii_case("untitled")
                || f_lower == p_lower
                || (!f_lower.is_empty()
                    && (p_lower.starts_with(&f_lower) || f_lower.starts_with(&p_lower)))
        }
        _ => false,
    };

    let (title, year, ids, group) = if use_folder {
        let (folder_title, folder_year, folder_ids) = parsed_folder.unwrap();
        let year = folder_year.or(parsed_file.1);
        let mut merged_ids = folder_ids;
        for (k, v) in parsed_file.2 {
            merged_ids.entry(k).or_insert(v);
        }
        let title = if is_generic_video_stem(stem)
            || parsed_file.0.eq_ignore_ascii_case("untitled")
            || !folder_title.is_empty()
        {
            folder_title
        } else {
            parsed_file.0
        };
        let group = format!(
            "folder:{}",
            parent
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .replace('\\', "/")
        );
        (title, year, merged_ids, group)
    } else {
        let (t, y, ids) = parsed_file;
        let group = format!("file:{}", relative.to_string_lossy().replace('\\', "/"));
        (t, y, ids, group)
    };

    let key = ["tmdb", "imdb", "tvdb"]
        .iter()
        .find_map(|p| ids.get(*p).map(|v| format!("{p}:{v}")))
        .unwrap_or(group);
    (title, year, ids, key)
}
