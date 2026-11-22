use super::*;
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSeriesEpisode {
    pub series_dir: PathBuf,
    pub series_raw_title: String,
    pub series_year: Option<u16>,
    pub series_external_ids: BTreeMap<String, String>,
    pub series_identity_key: String,
    pub season_number: u32,
    pub episode_number: u32,
    pub episode_title: String,
}

pub(super) fn clean_episode_title(raw: &str) -> Option<String> {
    let trimmed = raw
        .trim()
        .trim_start_matches('-')
        .trim_matches(['.', '_', '-', ' '])
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut working = trimmed;
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
    let mut end = working.find('[').unwrap_or(working.len());
    let lower = working.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    for &tag in RELEASE_TAGS {
        let tag_len = tag.len();
        let mut pos = 0;
        while let Some(idx) = lower[pos..end].find(tag) {
            let actual_idx = pos + idx;
            let prev_delim = actual_idx == 0 || is_delim(lower_bytes[actual_idx - 1]);
            let next_delim = actual_idx + tag_len == lower_bytes.len()
                || actual_idx + tag_len == end
                || is_delim(lower_bytes[actual_idx + tag_len]);
            if prev_delim && next_delim {
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
    let cleaned: String = raw_slice
        .chars()
        .map(|c| if c == '.' || c == '_' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['-', '.', '_', ' '])
        .to_owned();

    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub(super) fn extract_s_e_pattern(stem: &str) -> Option<(u32, u32, Option<String>)> {
    let bytes = stem.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if (bytes[i] == b's' || bytes[i] == b'S') && i + 1 < len && bytes[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(s_val) = stem[i + 1..j].parse::<u32>() {
                let mut k = j;
                while k < len && matches!(bytes[k], b'.' | b' ' | b'_' | b'-') {
                    k += 1;
                }
                if k < len
                    && (bytes[k] == b'e' || bytes[k] == b'E')
                    && k + 1 < len
                    && bytes[k + 1].is_ascii_digit()
                {
                    let mut m = k + 1;
                    while m < len && bytes[m].is_ascii_digit() {
                        m += 1;
                    }
                    if let Ok(e_val) = stem[k + 1..m].parse::<u32>() {
                        // Check if multi-episode suffix like -E02 or E02
                        let mut after = m;
                        if after < len
                            && (bytes[after] == b'-'
                                || bytes[after] == b'e'
                                || bytes[after] == b'E')
                        {
                            if bytes[after] == b'-'
                                && after + 1 < len
                                && (bytes[after + 1] == b'e' || bytes[after + 1] == b'E')
                            {
                                after += 2;
                                while after < len && bytes[after].is_ascii_digit() {
                                    after += 1;
                                }
                            } else if bytes[after] == b'e' || bytes[after] == b'E' {
                                after += 1;
                                while after < len && bytes[after].is_ascii_digit() {
                                    after += 1;
                                }
                            }
                        }
                        let rem = stem[after..].trim();
                        let title = clean_episode_title(rem);
                        return Some((s_val, e_val, title));
                    }
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

pub(super) fn extract_x_pattern(stem: &str) -> Option<(u32, u32, Option<String>)> {
    let bytes = stem.as_bytes();
    let len = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        if (b == b'x' || b == b'X') && i > 0 && i + 1 < len {
            let mut start = i;
            while start > 0 && bytes[start - 1].is_ascii_digit() {
                start -= 1;
            }
            if start == i {
                continue;
            }
            if start > 0 && !matches!(bytes[start - 1], b'.' | b' ' | b'_' | b'-' | b'[' | b'(') {
                continue;
            }
            let Ok(s_val) = stem[start..i].parse::<u32>() else {
                continue;
            };

            let mut end = i + 1;
            while end < len && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end == i + 1 {
                continue;
            }
            if end < len && !matches!(bytes[end], b'.' | b' ' | b'_' | b'-' | b']' | b')') {
                continue;
            }
            let Ok(e_val) = stem[i + 1..end].parse::<u32>() else {
                continue;
            };

            let rem = stem[end..].trim();
            let title = clean_episode_title(rem);
            return Some((s_val, e_val, title));
        }
    }
    None
}

pub(super) fn extract_season_from_folder(parent: &str) -> Option<u32> {
    let lower = parent.to_ascii_lowercase();
    let lower = lower.trim();
    if lower == "specials"
        || lower == "special"
        || lower == "season 0"
        || lower == "saison 0"
        || lower == "s0"
        || lower == "s00"
        || lower == "hors-série"
        || lower == "hors série"
    {
        return Some(0);
    }
    for prefix in ["season", "saison", "staffel"] {
        if let Some(rem) = lower.strip_prefix(prefix) {
            let digits: String = rem.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(num) = digits.parse::<u32>() {
                return Some(num);
            }
        }
    }
    if let Some(rem) = lower.strip_prefix('s') {
        let digits: String = rem.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(num) = digits.parse::<u32>() {
            return Some(num);
        }
    }
    None
}

pub(super) fn extract_episode_from_filename(stem: &str) -> Option<(u32, Option<String>)> {
    let lower = stem.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    for prefix in ["episode", "ep", "e"] {
        let mut search_from = 0;
        while let Some(rel_idx) = lower[search_from..].find(prefix) {
            let idx = search_from + rel_idx;
            search_from = idx + 1;
            let boundary_before = idx == 0
                || matches!(
                    lower_bytes[idx - 1],
                    b'.' | b' ' | b'_' | b'-' | b'[' | b'('
                );
            if !boundary_before {
                continue;
            }
            let after_start = idx + prefix.len();
            let after = &lower[after_start..];
            let after_trimmed = if prefix == "e" {
                // Single-letter "e" must be immediately followed by digits (e.g. "E03")
                // so titles like "Wall-E - 2008" do not match.
                after
            } else {
                after.trim_start_matches(['.', ' ', '_', '-'])
            };
            let digits_len = after_trimmed
                .bytes()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits_len == 0 || digits_len > 4 {
                continue;
            }
            let rem_idx = after_start + (after.len() - after_trimmed.len()) + digits_len;
            let next_byte = lower_bytes.get(rem_idx).copied();
            if let Some(b) = next_byte
                && b.is_ascii_alphabetic()
                && !(b == b'v'
                    && lower_bytes
                        .get(rem_idx + 1)
                        .is_some_and(|n| n.is_ascii_digit()))
            {
                continue;
            }
            if let Ok(num) = after_trimmed[..digits_len].parse::<u32>() {
                let rem = stem[rem_idx..].trim();
                let title = clean_episode_title(rem);
                return Some((num, title));
            }
        }
    }
    let trimmed = stem.trim();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty()
        && digits.len() <= 4
        && let Ok(num) = digits.parse::<u32>()
    {
        let rem = trimmed[digits.len()..].trim();
        let title = clean_episode_title(rem);
        return Some((num, title));
    }
    // Dash-separated episode number (e.g. "Show - 03", "[Group] Show - 14 [1080p]", "Show - 03 - Title")
    let mut search_from = 0;
    while let Some(rel_idx) = lower[search_from..].find(" - ") {
        let idx = search_from + rel_idx + 3;
        search_from = idx;
        let after = &lower[idx..];
        let after_trimmed = after.trim_start();
        let digits_len = after_trimmed
            .bytes()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits_len == 0 || digits_len > 4 {
            continue;
        }
        let rem_idx = idx + (after.len() - after_trimmed.len()) + digits_len;
        let next_byte = lower_bytes.get(rem_idx).copied();
        let valid_after = match next_byte {
            None => true,
            Some(b' ' | b'.' | b'_' | b'-' | b'[' | b'(' | b']' | b')') => true,
            Some(b'v') => lower_bytes
                .get(rem_idx + 1)
                .is_some_and(|n| n.is_ascii_digit()),
            _ => false,
        };
        if !valid_after {
            continue;
        }
        if let Ok(num) = after_trimmed[..digits_len].parse::<u32>() {
            if digits_len == 4 && (1900..=2100).contains(&num) {
                continue;
            }
            let mut title_start = rem_idx;
            if lower_bytes.get(title_start) == Some(&b'v')
                && lower_bytes
                    .get(title_start + 1)
                    .is_some_and(|n| n.is_ascii_digit())
            {
                title_start += 1;
                while lower_bytes
                    .get(title_start)
                    .is_some_and(|n| n.is_ascii_digit())
                {
                    title_start += 1;
                }
            }
            let rem = stem[title_start..].trim();
            let title = clean_episode_title(rem);
            return Some((num, title));
        }
    }
    None
}

pub fn parse_series_episode(relative: &Path) -> Option<ParsedSeriesEpisode> {
    let mut components = Vec::new();
    for c in relative.components() {
        if let Component::Normal(s) = c
            && let Some(str_val) = s.to_str()
        {
            components.push(str_val);
        }
    }
    if components.is_empty() {
        return None;
    }

    let (series_dir, series_folder_name) = if components.len() >= 2 {
        (PathBuf::from(components[0]), components[0])
    } else {
        (PathBuf::new(), components[0])
    };

    let (series_raw_title, series_year, series_external_ids) = metadata_name(series_folder_name);
    let series_identity_key = ["tvdb", "tmdb", "imdb"]
        .iter()
        .find_map(|p| series_external_ids.get(*p).map(|v| format!("{p}:{v}")))
        .unwrap_or_else(|| {
            if series_dir.as_os_str().is_empty() {
                format!("series:{}", series_raw_title)
            } else {
                format!("folder:{}", series_dir.to_string_lossy().replace('\\', "/"))
            }
        });

    let stem = relative.file_stem().and_then(|s| s.to_str())?;
    let parent_name = relative
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // 1. Pattern S(\d+)E(\d+)
    if let Some((s, e, remaining_title)) = extract_s_e_pattern(stem) {
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    // 2. Pattern (\d+)x(\d+)
    if let Some((s, e, remaining_title)) = extract_x_pattern(stem) {
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    // 3. Parent folder provides season, filename provides episode
    let season_from_parent = extract_season_from_folder(parent_name);
    if let Some(s) = season_from_parent
        && let Some((e, remaining_title)) = extract_episode_from_filename(stem)
    {
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    // 4. Filename provides episode number directly (e.g. "01 - Pilot" or "1. Currahee")
    if let Some((e, remaining_title)) = extract_episode_from_filename(stem) {
        let s = season_from_parent.unwrap_or(1);
        let ep_title = remaining_title.unwrap_or_else(|| format!("S{:02}E{:02}", s, e));
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: e,
            episode_title: ep_title,
        });
    }

    // 5. Standalone video file in a folder (e.g. a movie or single episode in a folder)
    if components.len() >= 2 {
        let (file_title, _, _) = metadata_name(stem);
        let ep_title =
            if is_generic_video_stem(stem) || file_title.eq_ignore_ascii_case(&series_raw_title) {
                series_raw_title.clone()
            } else {
                file_title
            };
        let s = season_from_parent.unwrap_or(1);
        return Some(ParsedSeriesEpisode {
            series_dir,
            series_raw_title,
            series_year,
            series_external_ids,
            series_identity_key,
            season_number: s,
            episode_number: 1,
            episode_title: ep_title,
        });
    }

    // 6. Direct root video file in a series library
    let (file_title, file_year, file_ids) = metadata_name(stem);
    let ep_title = file_title.clone();
    Some(ParsedSeriesEpisode {
        series_dir,
        series_raw_title: file_title,
        series_year: file_year,
        series_external_ids: file_ids,
        series_identity_key: format!("file:{}", relative.to_string_lossy().replace('\\', "/")),
        season_number: 1,
        episode_number: 1,
        episode_title: ep_title,
    })
}
