//! Locale normalisation, fallback chains and translation selection for work,
//! season and episode texts. Pure functions only: storage lives in `translations`.
//!
//! Rules (see `docs/METADATA_LOCALIZATION.md`):
//! * locales are canonical BCP-47-like tags: `fr`, `fr-FR`, `fr-CA`, `zh-Hant-TW`, `und`;
//! * the chain is requested locale, the account's fallbacks, the server default;
//!   `und` (language-neutral) rows apply after the chain;
//! * for each chain entry an exact match wins, then the bare language, then a sibling
//!   region of the same language (`en-GB` falls back to `en-US` before leaving English);
//!   at one chain position the source priority comes first (an NFO beats a provider row),
//!   the more precise region only separates rows of equal priority;
//! * manual rows beat provider rows: a manual row scoped to a chain locale first, then
//!   a manual language-neutral row, then everything else by chain order and priority.

use std::cmp::Reverse;

/// Language-neutral locale: the row applies to every requested locale.
pub const NEUTRAL: &str = "und";
/// Priorities stored with each row; higher wins inside one locale.
pub const PRIORITY_MANUAL: i64 = 100;
pub const PRIORITY_NFO: i64 = 80;
pub const PRIORITY_PROVIDER: i64 = 50;
pub const PRIORITY_LEGACY: i64 = 40;
/// Most chain entries ever considered; bounds queries and cache keys.
pub const MAX_CHAIN: usize = 6;

/// Source tag for hand-written corrections.
pub const SOURCE_MANUAL: &str = "manual";
pub const SOURCE_NFO: &str = "nfo";
pub const SOURCE_LEGACY: &str = "legacy";

pub fn provider_source(provider: &str) -> Option<String> {
    let id = provider.trim().to_ascii_lowercase();
    (!id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.'))
    .then(|| format!("provider:{id}"))
}

pub fn priority_for(source: &str) -> i64 {
    match source {
        SOURCE_MANUAL => PRIORITY_MANUAL,
        SOURCE_NFO => PRIORITY_NFO,
        SOURCE_LEGACY => PRIORITY_LEGACY,
        _ => PRIORITY_PROVIDER,
    }
}

/// ISO 639-2/T three-letter codes seen in TVDB and media tags, mapped to two letters.
fn two_letter(language: &str) -> Option<&'static str> {
    Some(match language {
        "fra" | "fre" => "fr",
        "eng" => "en",
        "deu" | "ger" => "de",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "jpn" => "ja",
        "zho" | "chi" => "zh",
        "kor" => "ko",
        "rus" => "ru",
        "nld" | "dut" => "nl",
        "ara" => "ar",
        "pol" => "pl",
        "ces" | "cze" => "cs",
        "dan" => "da",
        "fin" => "fi",
        "swe" => "sv",
        "tur" => "tr",
        "ukr" => "uk",
        "heb" => "he",
        "ell" | "gre" => "el",
        _ => return None,
    })
}

/// Canonical form of a language tag, or `None` when it is not a plausible tag.
/// `fr_fr` -> `fr-FR`, `FRA` -> `fr`, `zh-hant-tw` -> `zh-Hant-TW`, `UND` -> `und`.
pub fn normalize(tag: &str) -> Option<String> {
    let tag = tag.trim().replace('_', "-");
    if tag.is_empty() || tag.len() > 35 {
        return None;
    }
    let mut parts = tag.split('-');
    let language = parts.next()?.to_ascii_lowercase();
    if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let language = if language == NEUTRAL {
        return parts.next().is_none().then(|| NEUTRAL.to_owned());
    } else if language.len() == 3 {
        two_letter(&language).map_or(language, str::to_owned)
    } else {
        language
    };
    let mut script = None;
    let mut region = None;
    for part in parts {
        if script.is_none()
            && region.is_none()
            && part.len() == 4
            && part.bytes().all(|b| b.is_ascii_alphabetic())
        {
            let lower = part.to_ascii_lowercase();
            script = Some(lower[..1].to_ascii_uppercase() + &lower[1..]);
        } else if region.is_none()
            && ((part.len() == 2 && part.bytes().all(|b| b.is_ascii_alphabetic()))
                || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit())))
        {
            region = Some(part.to_ascii_uppercase());
        } else {
            // Variants and extensions are not stored: they would fragment the data.
            return None;
        }
    }
    let mut out = language;
    for part in [script, region].into_iter().flatten() {
        out.push('-');
        out.push_str(&part);
    }
    Some(out)
}

/// Bare language of a canonical tag (`fr-CA` -> `fr`).
pub fn language_of(locale: &str) -> &str {
    locale.split('-').next().unwrap_or(locale)
}

/// Ordered, deduplicated list of locales to try: requested, fallbacks, default.
/// Invalid tags are dropped; the list never exceeds [`MAX_CHAIN`].
pub fn chain<I, S>(requested: Option<&str>, fallbacks: I, default: Option<&str>) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: Vec<String> = Vec::new();
    let candidates = requested
        .map(str::to_owned)
        .into_iter()
        .chain(fallbacks.into_iter().map(|v| v.as_ref().to_owned()))
        .chain(default.map(str::to_owned));
    for candidate in candidates {
        if let Some(tag) = normalize(&candidate)
            && tag != NEUTRAL
            && !out.contains(&tag)
        {
            out.push(tag);
            if out.len() == MAX_CHAIN {
                break;
            }
        }
    }
    out
}

/// Stable cache discriminator: two requests with the same chain share it and
/// nothing else does (the chain is the only locale input to a rendered page).
pub fn cache_tag(chain: &[String]) -> String {
    if chain.is_empty() {
        "-".into()
    } else {
        chain.join(">")
    }
}

/// One stored translation row, as read by the selection logic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub locale: String,
    pub source: String,
    pub priority: i64,
    pub updated_at: i64,
    pub title: Option<String>,
    pub sort_title: Option<String>,
    pub overview: Option<String>,
    pub tagline: Option<String>,
}

/// Selected texts and the locale each one came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selected {
    pub title: Option<(String, String)>,
    pub sort_title: Option<(String, String)>,
    pub overview: Option<(String, String)>,
    pub tagline: Option<(String, String)>,
}

/// Region served first when only sibling regions exist (`en-GB` is answered by `en-US`).
fn is_default_region(locale: &str) -> bool {
    matches!(
        locale,
        "fr-FR"
            | "en-US"
            | "es-ES"
            | "pt-PT"
            | "de-DE"
            | "it-IT"
            | "nl-NL"
            | "zh-CN"
            | "ja-JP"
            | "ko-KR"
            | "ru-RU"
    )
}

/// Position of `locale` in `chain`: (entry index, 0 exact | 1 bare language | 2 sibling region).
fn match_rank(locale: &str, chain: &[String]) -> Option<(usize, u8)> {
    let language = language_of(locale);
    let bare = !locale.contains('-');
    // The first chain entry of the same language decides the position, so a later
    // entry such as the server default can never outrank the user's own choice.
    chain.iter().enumerate().find_map(|(index, entry)| {
        if entry == locale {
            Some((index, 0))
        } else if language_of(entry) == language {
            Some((index, if bare { 1 } else { 2 }))
        } else {
            None
        }
    })
}

type SortKey = (u8, usize, Reverse<i64>, u8, Reverse<i64>);

fn sort_key(row: &Row, chain: &[String]) -> Option<SortKey> {
    let manual = row.source == SOURCE_MANUAL;
    if row.locale == NEUTRAL {
        // Neutral non-manual rows (e.g. a provider's original title) are the last resort.
        return Some((
            if manual { 1 } else { 5 },
            0,
            Reverse(row.priority),
            0,
            Reverse(row.updated_at),
        ));
    }
    let (index, mut kind) = match_rank(&row.locale, chain)?;
    if kind == 2 && !is_default_region(&row.locale) {
        kind = 3;
    }
    // A manual correction scoped to the exact chain locale beats everything; a manual
    // correction for a sibling still ranks as a manual row inside its chain position.
    let tier = if manual && kind == 0 {
        0
    } else if manual {
        2
    } else {
        4
    };
    Some((
        tier,
        index,
        Reverse(row.priority),
        kind,
        Reverse(row.updated_at),
    ))
}

/// Pick each text field independently: an empty or missing field in the best row
/// falls through to the next candidate instead of blanking the result.
pub fn select(rows: &[Row], chain: &[String]) -> Selected {
    let mut ranked: Vec<(SortKey, &Row)> = rows
        .iter()
        .filter_map(|row| sort_key(row, chain).map(|key| (key, row)))
        .collect();
    // Locale then source make equal keys deterministic whatever order storage returns.
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.locale.cmp(&b.1.locale))
            .then_with(|| a.1.source.cmp(&b.1.source))
    });
    let pick = |field: fn(&Row) -> &Option<String>| {
        ranked.iter().find_map(|(_, row)| {
            field(row)
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(|v| (v.to_owned(), row.locale.clone()))
        })
    };
    Selected {
        title: pick(|r| &r.title),
        sort_title: pick(|r| &r.sort_title),
        overview: pick(|r| &r.overview),
        tagline: pick(|r| &r.tagline),
    }
}

/// Bare languages to read from storage for a chain (bounded `IN` list).
pub fn languages_to_load(chain: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![NEUTRAL.into()];
    for locale in chain {
        let language = language_of(locale).to_owned();
        if !out.contains(&language) {
            out.push(language);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(locale: &str, source: &str, title: Option<&str>, overview: Option<&str>) -> Row {
        Row {
            locale: locale.into(),
            source: source.into(),
            priority: priority_for(source),
            updated_at: 1,
            title: title.map(str::to_owned),
            overview: overview.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn normalises_regional_and_three_letter_tags() {
        for (input, expected) in [
            ("fr", "fr"),
            ("fr_fr", "fr-FR"),
            ("FR-ca", "fr-CA"),
            ("en-gb", "en-GB"),
            ("fra", "fr"),
            ("ENG", "en"),
            ("zh-hant-tw", "zh-Hant-TW"),
            ("es-419", "es-419"),
            ("UND", "und"),
        ] {
            assert_eq!(normalize(input).as_deref(), Some(expected), "{input}");
        }
        for bad in [
            "",
            "f",
            "fr-",
            "fr-FR-x-private",
            "fr--FR",
            "../etc",
            "fr-FRA",
            "und-FR",
        ] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
    }

    #[test]
    fn chain_is_ordered_deduplicated_and_bounded() {
        let c = chain(
            Some("fr_CA"),
            ["fr-CA", "en-GB", "bad tag", "en"],
            Some("fr"),
        );
        assert_eq!(c, ["fr-CA", "en-GB", "en", "fr"]);
        let long: Vec<String> = (0..20)
            .map(|i| format!("a{}", (b'a' + i) as char))
            .collect();
        assert!(chain(None, long.iter(), None).len() <= MAX_CHAIN);
        assert_eq!(cache_tag(&[]), "-");
        assert_ne!(cache_tag(&c), cache_tag(&c[..2]));
    }

    #[test]
    fn exact_region_then_bare_language_then_sibling_region() {
        let rows = [
            row("fr-FR", "provider:tmdb", Some("Les Évadés"), None),
            row("fr-CA", "provider:tmdb", Some("Les Évadés (QC)"), None),
            row(
                "en-US",
                "provider:tmdb",
                Some("The Shawshank Redemption"),
                None,
            ),
        ];
        let pick = |chain: &[&str]| {
            let chain: Vec<String> = chain.iter().map(|c| (*c).into()).collect();
            select(&rows, &chain).title.map(|t| t.0)
        };
        assert_eq!(pick(&["fr-CA"]).as_deref(), Some("Les Évadés (QC)"));
        assert_eq!(pick(&["fr-FR"]).as_deref(), Some("Les Évadés"));
        // fr-BE has no row: a sibling of the same language beats leaving French.
        assert_eq!(pick(&["fr-BE", "en-US"]).as_deref(), Some("Les Évadés"));
        // en-GB is served by the en-US sibling.
        assert_eq!(
            pick(&["en-GB"]).as_deref(),
            Some("The Shawshank Redemption")
        );
        // No match anywhere in the chain: nothing, the caller keeps the stored title.
        assert_eq!(pick(&["ja"]), None);
    }

    #[test]
    fn bare_language_row_is_preferred_to_a_sibling_region() {
        let rows = [
            row("fr-FR", "provider:tvdb", Some("Régional"), None),
            row("fr", "provider:tvdb", Some("Neutre"), None),
        ];
        let got = select(&rows, &["fr-CA".to_owned()]);
        assert_eq!(got.title, Some(("Neutre".into(), "fr".into())));
    }

    #[test]
    fn fields_fall_back_independently() {
        let rows = [
            row("fr-FR", "provider:tmdb", Some("Titre"), None),
            row(
                "en-US",
                "provider:tmdb",
                Some("Title"),
                Some("English plot"),
            ),
        ];
        let got = select(&rows, &["fr-FR".to_owned(), "en-US".to_owned()]);
        assert_eq!(got.title, Some(("Titre".into(), "fr-FR".into())));
        assert_eq!(got.overview, Some(("English plot".into(), "en-US".into())));
        // An empty string is a missing translation, never a blank title.
        let blank = [row("fr-FR", "provider:tmdb", Some("  "), Some(""))];
        assert_eq!(select(&blank, &["fr-FR".to_owned()]), Selected::default());
    }

    #[test]
    fn manual_corrections_outrank_nfo_and_providers() {
        let rows = [
            row(
                "fr-FR",
                "provider:tmdb",
                Some("Provider"),
                Some("Résumé provider"),
            ),
            row("fr-FR", "nfo", Some("NFO"), None),
            row("fr-FR", "manual", Some("Manuel"), None),
        ];
        let got = select(&rows, &["fr-FR".to_owned()]);
        assert_eq!(got.title, Some(("Manuel".into(), "fr-FR".into())));
        // The manual row has no overview: the provider's survives per field.
        assert_eq!(
            got.overview.map(|o| o.0).as_deref(),
            Some("Résumé provider")
        );
        let no_manual = select(&rows[..2], &["fr-FR".to_owned()]);
        assert_eq!(no_manual.title.map(|t| t.0).as_deref(), Some("NFO"));
    }

    #[test]
    fn neutral_manual_applies_to_everyone_but_loses_to_a_scoped_manual() {
        let rows = [
            row(NEUTRAL, "manual", Some("Titre unique"), None),
            row("en-US", "provider:tmdb", Some("English"), None),
            row("fr-FR", "manual", Some("Corrigé FR"), None),
        ];
        let en = select(&rows, &["en-US".to_owned()]);
        assert_eq!(en.title, Some(("Titre unique".into(), NEUTRAL.into())));
        let fr = select(&rows, &["fr-FR".to_owned()]);
        assert_eq!(fr.title, Some(("Corrigé FR".into(), "fr-FR".into())));
        // A neutral provider row is the very last resort, after localized rows.
        let neutral_provider = [
            row(NEUTRAL, "provider:tmdb", Some("Original"), None),
            row("fr-FR", "provider:tmdb", Some("Français"), None),
        ];
        assert_eq!(
            select(&neutral_provider, &["fr-FR".to_owned()])
                .title
                .map(|t| t.0)
                .as_deref(),
            Some("Français")
        );
        assert_eq!(
            select(&neutral_provider, &["ja".to_owned()])
                .title
                .map(|t| t.0)
                .as_deref(),
            Some("Original")
        );
    }

    #[test]
    fn newer_row_wins_inside_one_source_and_priority() {
        let mut old = row("fr-FR", "provider:tmdb", Some("Ancien"), None);
        old.updated_at = 1;
        let mut new = row("fr-FR", "provider:tmdb", Some("Nouveau"), None);
        new.updated_at = 2;
        assert_eq!(
            select(&[old, new], &["fr-FR".to_owned()])
                .title
                .map(|t| t.0)
                .as_deref(),
            Some("Nouveau")
        );
    }

    #[test]
    fn storage_read_is_bounded_by_chain_languages() {
        let c = chain(Some("fr-CA"), ["fr-FR", "en-GB"], None);
        assert_eq!(languages_to_load(&c), ["und", "fr", "en"]);
    }

    #[test]
    fn provider_sources_are_validated() {
        assert_eq!(provider_source("TMDb").as_deref(), Some("provider:tmdb"));
        assert_eq!(provider_source("../x"), None);
        assert_eq!(provider_source(""), None);
        assert_eq!(priority_for("provider:tmdb"), PRIORITY_PROVIDER);
        assert!(priority_for(SOURCE_MANUAL) > priority_for(SOURCE_NFO));
        assert!(priority_for(SOURCE_NFO) > priority_for("provider:tmdb"));
    }
}
