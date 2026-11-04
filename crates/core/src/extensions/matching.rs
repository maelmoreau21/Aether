use serde_json::Value;

pub(super) fn lyrics_result_matches_input(input: &Value, result: &Value) -> bool {
    let Some(track) = input["track"]
        .as_str()
        .map(normalize_title)
        .filter(|s| !s.is_empty())
    else {
        return false;
    };
    let Some(artist) = input["artist"]
        .as_str()
        .map(normalize_title)
        .filter(|s| !s.is_empty() && s != "unknown artist")
    else {
        return false;
    };
    if result["trackName"]
        .as_str()
        .is_none_or(|s| normalize_title(s) != track)
        || result["artistName"]
            .as_str()
            .is_none_or(|s| normalize_title(s) != artist)
    {
        return false;
    }
    match (
        input["durationSecs"].as_f64(),
        result["durationSecs"].as_f64(),
    ) {
        (Some(a), Some(b)) => {
            a > 0.0 && b > 0.0 && ((a - b).abs() <= 5.0 || (a - b).abs() / a <= 0.05)
        }
        _ => true,
    }
}

pub(super) fn normalize_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(super) fn matchable_title(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_whitespace() || matches!(c, '.' | '_' | '-' | ':' | '/' | '\'') {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn invocation_year_matches(details: &Value, candidate: &Value) -> bool {
    details["year"].as_i64().is_none_or(|year| {
        candidate["year"]
            .as_i64()
            .is_none_or(|expected| year == expected)
    })
}

pub(super) fn known_metadata_id<'a>(input: &'a Value, provider: &str) -> Option<&'a str> {
    input["externalIds"][provider]
        .as_str()
        .filter(|id| !id.trim().is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
}

pub(super) fn external_id_candidate<'a>(items: &'a [Value], input: &Value) -> Option<&'a Value> {
    if !input["externalIds"]
        .as_object()
        .is_some_and(|ids| !ids.is_empty())
        || items.len() != 1
    {
        return None;
    }
    let candidate = &items[0];
    (candidate["mediaType"] == input["mediaType"]
        && candidate["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id.len() <= 256))
    .then_some(candidate)
}

pub(super) fn metadata_external_ids_match(details: &Value, input: &Value) -> bool {
    let Some(expected) = input["externalIds"].as_object() else {
        return false;
    };
    let mut confirmed = false;
    for (provider, id) in expected {
        if let Some(actual) = details["externalIds"].get(provider) {
            if actual != id {
                return false;
            }
            confirmed = true;
        }
    }
    confirmed
}

pub(super) fn metadata_external_ids_conflict(details: &Value, input: &Value) -> bool {
    input["externalIds"].as_object().is_some_and(|expected| {
        expected.iter().any(|(provider, id)| {
            details["externalIds"]
                .get(provider)
                .is_some_and(|actual| actual != id)
        })
    })
}

pub(super) fn trusted_metadata_candidate<'a>(
    items: &'a [Value],
    input: &Value,
) -> Option<&'a Value> {
    let raw_title = input["title"].as_str()?;
    let title = normalize_title(raw_title);
    let matchable = matchable_title(raw_title);
    let media_type = input["mediaType"].as_str()?;
    let year = input["year"].as_i64();

    let title_matches = |name: &str| {
        normalize_title(name) == title
            || (!matchable.is_empty() && matchable_title(name) == matchable)
    };

    if let Some(expected) = year
        && let Some(candidate) = items.iter().find(|item| {
            (item["title"].as_str().is_some_and(title_matches)
                || item["originalTitle"].as_str().is_some_and(title_matches))
                && item["mediaType"].as_str() == Some(media_type)
                && item["year"].as_i64() == Some(expected)
        })
    {
        return Some(candidate);
    }
    items.iter().find(|item| {
        (item["title"].as_str().is_some_and(title_matches)
            || item["originalTitle"].as_str().is_some_and(title_matches))
            && item["mediaType"].as_str() == Some(media_type)
            && year.is_none_or(|expected| item["year"].as_i64().is_none_or(|y| y == expected))
    })
}

pub fn trusted_music_candidate<'a>(items: &'a [Value], input: &Value) -> Option<&'a Value> {
    let title = normalize_title(input["title"].as_str()?);
    let artist = normalize_title(input["artist"].as_str()?);
    if title.is_empty() || artist.is_empty() || artist == "unknown artist" {
        return None;
    }
    let duration = input["duration"].as_f64();
    let mut matches = items.iter().filter(|item| {
        let id_ok = item["id"]
            .as_str()
            .is_some_and(|id| !id.trim().is_empty() && id.len() <= 256);
        let title_ok = item["title"]
            .as_str()
            .is_some_and(|name| normalize_title(name) == title);
        let artist_ok = item["artist"]
            .as_str()
            .is_some_and(|name| normalize_title(name) == artist);
        let duration_ok = match (
            duration,
            item["duration"]
                .as_f64()
                .or_else(|| item["length"].as_f64().map(|ms| ms / 1000.0)),
        ) {
            (Some(a), Some(b)) => {
                a.is_finite()
                    && b.is_finite()
                    && a > 0.0
                    && b > 0.0
                    && ((a - b).abs() <= 5.0 || (a - b).abs() / a <= 0.05)
            }
            _ => true,
        };
        id_ok && item["mediaType"].as_str() == Some("music") && title_ok && artist_ok && duration_ok
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

pub(super) fn music_details_match(details: &Value, candidate: &Value) -> bool {
    let id_matches = candidate["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .is_some_and(|id| details["providerId"].as_str() == Some(id));
    let title_matches = details["title"].as_str().is_some_and(|title| {
        candidate["title"]
            .as_str()
            .is_some_and(|expected| normalize_title(title) == normalize_title(expected))
    });
    let artists = details["actors"].as_array().map(|credits| {
        credits
            .iter()
            .filter(|credit| credit["role"].as_str() == Some("artist"))
            .filter_map(|credit| credit["name"].as_str())
            .collect::<Vec<_>>()
            .join(", ")
    });
    let artist_matches = candidate["artist"]
        .as_str()
        .zip(artists.as_deref())
        .is_some_and(|(expected, actual)| {
            !actual.is_empty() && normalize_title(expected) == normalize_title(actual)
        });
    id_matches && title_matches && artist_matches
}
