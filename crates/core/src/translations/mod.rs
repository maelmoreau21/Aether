//! Storage and read-time application of translated texts for works, episodes and seasons.
//!
//! Real rows live in `work_translations` / `season_translations`. Data written before
//! those tables existed stays readable without rewriting it: the loader derives rows
//! from `editorial_json` (NFO / provider origin) and `manual_json` (hand edits), so a
//! rollback or a half-finished migration never changes what the catalogue shows.

use crate::database::{normalize_sql, query};
use crate::identity::IdentityStore;
use crate::metadata_locale::{self as ml, Row as TrRow};
use serde_json::{Value, json};
use sqlx::{Execute, Row};
use std::collections::HashMap;

pub mod search;

const MAX_TITLE: usize = 1_000;
const MAX_OVERVIEW: usize = 20_000;

/// One translation to store. Empty strings are stored as absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Translation {
    pub locale: String,
    pub source: String,
    pub title: Option<String>,
    pub sort_title: Option<String>,
    pub overview: Option<String>,
    pub tagline: Option<String>,
    /// Tag exactly as the provider returned it, kept for diagnostics.
    pub provider_locale: Option<String>,
}

fn clip(value: &Option<String>, max: usize) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.chars().take(max).collect())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[derive(Clone, Copy)]
enum Table {
    Work,
    Season,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Self::Work => "work_translations",
            Self::Season => "season_translations",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Work => "work_id",
            Self::Season => "season_id",
        }
    }
}

async fn upsert(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    table: Table,
    id: &str,
    item: &Translation,
) -> Result<bool, sqlx::Error> {
    let Some(locale) = ml::normalize(&item.locale) else {
        return Ok(false);
    };
    let source = item.source.trim();
    let valid_source = matches!(
        source,
        ml::SOURCE_MANUAL | ml::SOURCE_NFO | ml::SOURCE_LEGACY
    ) || (source.starts_with("provider:")
        && ml::provider_source(&source[9..]).is_some());
    if !valid_source {
        return Ok(false);
    }
    let title = clip(&item.title, MAX_TITLE);
    let overview = clip(&item.overview, MAX_OVERVIEW);
    let tagline = clip(&item.tagline, MAX_TITLE);
    if title.is_none() && overview.is_none() && tagline.is_none() {
        return Ok(false);
    }
    let sort_title =
        clip(&item.sort_title, MAX_TITLE).or_else(|| title.as_ref().map(|t| t.to_lowercase()));
    let sql = format!(
        "INSERT INTO {t}({k},locale,language,source,priority,title,sort_title,overview,tagline,provider_locale,updated_at) \
         VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT({k},locale,source) DO UPDATE SET \
         language=excluded.language,priority=excluded.priority,title=excluded.title,sort_title=excluded.sort_title,\
         overview=excluded.overview,tagline=excluded.tagline,provider_locale=excluded.provider_locale,updated_at=excluded.updated_at",
        t = table.name(),
        k = table.key()
    );
    // Table and key names are crate-internal constants; every value is a bound parameter.
    query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(&locale)
        .bind(ml::language_of(&locale))
        .bind(source)
        .bind(ml::priority_for(source))
        .bind(title)
        .bind(sort_title)
        .bind(overview)
        .bind(tagline)
        .bind(clip(&item.provider_locale, 35))
        .bind(now())
        .execute(&mut **tx)
        .await?;
    if matches!(table, Table::Work) {
        search::refresh(tx, id).await?;
    }
    Ok(true)
}

/// Insert or replace one (locale, source) translation of a work or episode.
/// Returns `false` when the locale, source or content is not storable.
pub async fn upsert_work(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    work_id: &str,
    item: &Translation,
) -> Result<bool, sqlx::Error> {
    let stored = upsert(tx, Table::Work, work_id, item).await?;
    if stored {
        bump_generation(tx).await?;
    }
    Ok(stored)
}

/// Insert or replace one (locale, source) translation of a season.
pub async fn upsert_season(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    season_id: &str,
    item: &Translation,
) -> Result<bool, sqlx::Error> {
    let stored = upsert(tx, Table::Season, season_id, item).await?;
    if stored {
        bump_generation(tx).await?;
    }
    Ok(stored)
}

/// Remove a hand-written correction so the next best source shows again.
pub async fn delete_work_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    work_id: &str,
    locale: &str,
    source: &str,
) -> Result<u64, sqlx::Error> {
    let Some(locale) = ml::normalize(locale) else {
        return Ok(0);
    };
    let removed = query("DELETE FROM work_translations WHERE work_id=? AND locale=? AND source=?")
        .bind(work_id)
        .bind(locale)
        .bind(source)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    if removed > 0 {
        search::refresh(tx, work_id).await?;
        bump_generation(tx).await?;
    }
    Ok(removed)
}

fn item_from(locale: &str, source: &str, value: &Value) -> Translation {
    Translation {
        locale: locale.to_owned(),
        source: source.to_owned(),
        title: text(value, "title").or_else(|| text(value, "name")),
        sort_title: None,
        overview: text(value, "overview").or_else(|| text(value, "plot")),
        tagline: text(value, "tagline"),
        provider_locale: Some(locale.to_owned()),
    }
}

/// Store the per-language texts of a provider proposal: the work's `translations`, and for a
/// series each season's and episode's `translations`. Keys may be bare (`fr`, as provider
/// plugins built on the first contract send) or regional (`fr-CA`). Rows are keyed by
/// provider, so a refresh replaces that provider's rows and nobody else's: manual rows and
/// other providers survive. Returns the number of rows stored.
pub async fn store_proposal_translations(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    provider: &str,
    work_id: &str,
    proposal: &Value,
) -> Result<usize, sqlx::Error> {
    let Some(source) = ml::provider_source(provider) else {
        return Ok(0);
    };
    let mut stored = 0;
    let work_items = |value: &Value| -> Vec<Translation> {
        value
            .get("translations")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    // Bounded: a provider cannot flood the table.
                    .take(64)
                    .filter_map(|(key, item)| {
                        ml::normalize(key).map(|locale| item_from(&locale, &source, item))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    // The provider says which language its main fields are in; take it at its word.
    if let Some(locale) = proposal
        .get("locale")
        .and_then(Value::as_str)
        .and_then(ml::normalize)
    {
        let main = Value::Object(
            [
                ("title", "title"),
                ("overview", "synopsis"),
                ("tagline", "tagline"),
            ]
            .iter()
            .filter_map(|(to, from)| Some((to.to_string(), proposal.get(*from)?.clone())))
            .collect(),
        );
        stored += usize::from(
            upsert(
                tx,
                Table::Work,
                work_id,
                &item_from(&locale, &source, &main),
            )
            .await?,
        );
    }
    for item in work_items(proposal) {
        stored += usize::from(upsert(tx, Table::Work, work_id, &item).await?);
    }
    for season in proposal
        .get("seasons")
        .and_then(Value::as_array)
        .map(|v| &v[..v.len().min(200)])
        .unwrap_or_default()
    {
        let Some(number) = season.get("seasonNumber").and_then(Value::as_i64) else {
            continue;
        };
        let season_id: Option<String> = crate::database::query_scalar(
            "SELECT id FROM inventory_seasons WHERE series_id=? AND season_number=?",
        )
        .bind(work_id)
        .bind(number)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(season_id) = season_id {
            for item in work_items(season) {
                stored += usize::from(upsert(tx, Table::Season, &season_id, &item).await?);
            }
        }
        for episode in season
            .get("episodes")
            .and_then(Value::as_array)
            .map(|v| &v[..v.len().min(2_000)])
            .unwrap_or_default()
        {
            let Some(episode_number) = episode.get("episodeNumber").and_then(Value::as_i64) else {
                continue;
            };
            let items = work_items(episode);
            if items.is_empty() {
                continue;
            }
            let episode_work: Option<String> = crate::database::query_scalar(
                "SELECT ep.work_id FROM inventory_episodes ep JOIN inventory_seasons s ON s.id=ep.season_id                  WHERE s.series_id=? AND s.season_number=? AND ep.episode_number=?",
            )
            .bind(work_id)
            .bind(number)
            .bind(episode_number)
            .fetch_optional(&mut **tx)
            .await?;
            if let Some(episode_work) = episode_work {
                for item in items {
                    stored += usize::from(upsert(tx, Table::Work, &episode_work, &item).await?);
                }
            }
        }
    }
    if stored > 0 {
        bump_generation(tx).await?;
    }
    Ok(stored)
}

/// Cached catalogue pages are keyed by this counter; translation writes must move it.
pub async fn bump_generation(tx: &mut sqlx::Transaction<'_, sqlx::Any>) -> Result<(), sqlx::Error> {
    query("UPDATE inventory_cache_generation SET gen=gen+1 WHERE id=1")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(str::to_owned)
}

/// Rows implied by `editorial_json` / `manual_json` as written before translation tables.
/// `library_locale` is the library's metadata language; texts written by a scan or a
/// provider are in that language, hand edits made without a locale are language-neutral.
pub fn legacy_rows(editorial: &Value, manual: &Value, library_locale: Option<&str>) -> Vec<TrRow> {
    let mut rows = Vec::new();
    let mut push = |locale: &str, source: String, value: &Value| {
        let title = text(value, "title");
        let overview = text(value, "plot").or_else(|| text(value, "outline"));
        let tagline = text(value, "tagline");
        if title.is_none() && overview.is_none() && tagline.is_none() {
            return;
        }
        rows.push(TrRow {
            locale: locale.to_owned(),
            priority: ml::priority_for(&source),
            source,
            updated_at: 0,
            title,
            sort_title: None,
            overview,
            tagline,
        });
    };
    let from_provenance = |value: &Value| -> Option<String> {
        let provenance = value
            .get("provenance")
            .and_then(Value::as_str)
            .unwrap_or("");
        let (_, provider) = provenance.split_once(':')?;
        ml::provider_source(provider)
    };
    if let Some(locale) = library_locale {
        match editorial.get("provenance").and_then(Value::as_str) {
            Some("nfo") => push(locale, ml::SOURCE_NFO.into(), editorial),
            Some(p) if p.starts_with("identify:") || p.starts_with("extension:") => {
                if let Some(source) = from_provenance(editorial) {
                    push(locale, source, editorial);
                }
            }
            _ => {}
        }
    }
    if manual.is_object() {
        match (
            manual.get("provenance").and_then(Value::as_str),
            from_provenance(manual),
        ) {
            // Applied by a provider or by identification: not a human decision.
            (Some(p), Some(source))
                if p.starts_with("identify:") || p.starts_with("extension:") =>
            {
                if let Some(locale) = library_locale {
                    push(locale, source, manual);
                }
            }
            // Edited by an administrator without a language: neutral and manual.
            _ => push(ml::NEUTRAL, ml::SOURCE_MANUAL.into(), manual),
        }
    }
    rows
}

fn chunked<T: Clone>(ids: &[T]) -> impl Iterator<Item = Vec<T>> + '_ {
    ids.chunks(200).map(<[T]>::to_vec)
}

async fn fetch(
    store: &IdentityStore,
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
) -> Result<Vec<sqlx::any::AnyRow>, sqlx::Error> {
    let sql = normalize_sql(builder.sql().as_str());
    let arguments = std::mem::replace(builder, sqlx::QueryBuilder::new(""))
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(sql), arguments)
        .fetch_all(&store.pool)
        .await
}

async fn load_rows(
    store: &IdentityStore,
    table: Table,
    ids: &[String],
    chain: &[String],
) -> Result<HashMap<String, Vec<TrRow>>, sqlx::Error> {
    let mut out: HashMap<String, Vec<TrRow>> = HashMap::new();
    let languages = ml::languages_to_load(chain);
    for chunk in chunked(ids) {
        let mut b = sqlx::QueryBuilder::<sqlx::Any>::new(format!(
            "SELECT {k} AS entity_id, locale, source, priority, updated_at, title, sort_title, overview, tagline FROM {t} WHERE {k} IN (",
            k = table.key(),
            t = table.name()
        ));
        let mut sep = b.separated(", ");
        for id in &chunk {
            sep.push_bind(id.clone());
        }
        sep.push_unseparated(") AND language IN (");
        let mut sep = b.separated(", ");
        for language in &languages {
            sep.push_bind(language.clone());
        }
        sep.push_unseparated(")");
        for row in fetch(store, &mut b).await? {
            out.entry(row.get("entity_id")).or_default().push(TrRow {
                locale: row.get("locale"),
                source: row.get("source"),
                priority: row.get("priority"),
                updated_at: row.get("updated_at"),
                title: row.get("title"),
                sort_title: row.get("sort_title"),
                overview: row.get("overview"),
                tagline: row.get("tagline"),
            });
        }
    }
    Ok(out)
}

/// Library metadata language as a locale, from the stored `options_json`.
fn library_locale(options_json: &str) -> Option<String> {
    let options: crate::library_options::LibraryOptions =
        serde_json::from_str(options_json).ok()?;
    ml::normalize(&options.preferred_metadata_language)
}

async fn load_legacy(
    store: &IdentityStore,
    ids: &[String],
) -> Result<HashMap<String, Vec<TrRow>>, sqlx::Error> {
    let mut out: HashMap<String, Vec<TrRow>> = HashMap::new();
    let mut languages: HashMap<String, Option<String>> = HashMap::new();
    for chunk in chunked(ids) {
        let mut b = sqlx::QueryBuilder::<sqlx::Any>::new(
            "SELECT w.id, w.library_id, w.editorial_json, w.manual_json, l.options_json FROM inventory_works w \
             JOIN libraries l ON l.id=w.library_id WHERE w.id IN (",
        );
        let mut sep = b.separated(", ");
        for id in &chunk {
            sep.push_bind(id.clone());
        }
        sep.push_unseparated(")");
        for row in fetch(store, &mut b).await? {
            let library: String = row.get("library_id");
            let locale = languages
                .entry(library)
                .or_insert_with(|| library_locale(&row.get::<String, _>("options_json")))
                .clone();
            let editorial = serde_json::from_str(&row.get::<String, _>("editorial_json"))
                .unwrap_or(Value::Null);
            let manual = row
                .get::<Option<String>, _>("manual_json")
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or(Value::Null);
            out.insert(
                row.get("id"),
                legacy_rows(&editorial, &manual, locale.as_deref()),
            );
        }
    }
    Ok(out)
}

/// Locale resolution result for one rendered entity, kept in the JSON as `localization`.
fn describe(chain: &[String], selected: &ml::Selected) -> Value {
    json!({
        "requested": chain.first(),
        "title": selected.title.as_ref().map(|v| &v.1),
        "overview": selected.overview.as_ref().map(|v| &v.1),
    })
}

fn overlay(item: &mut Value, selected: &ml::Selected, chain: &[String]) {
    let Some(object) = item.as_object_mut() else {
        return;
    };
    if let Some((title, _)) = &selected.title {
        object.insert("title".into(), json!(title));
    }
    if let Some((overview, _)) = &selected.overview {
        object.insert("plot".into(), json!(overview));
        object.insert("description".into(), json!(overview));
    }
    if let Some((tagline, _)) = &selected.tagline {
        object.insert("outline".into(), json!(tagline));
        object.insert("tagline".into(), json!(tagline));
    }
    // The raw per-language blob is replaced by the single resolved language.
    object.insert("translations".into(), json!({}));
    object.insert("localization".into(), describe(chain, selected));
}

/// Languages worth fetching from a provider besides the library language: those the
/// enabled accounts actually read (metadata locale, interface language, fallbacks), most
/// common first. Bounded so one refresh costs a fixed number of provider requests.
pub async fn wanted_locales(
    store: &IdentityStore,
    library_locale: &str,
    limit: usize,
) -> Result<Vec<String>, sqlx::Error> {
    let rows: Vec<String> = crate::database::query_scalar(
        "SELECT p.preferences_json FROM user_preferences p JOIN users u ON u.id=p.user_id \
         WHERE u.disabled_at IS NULL LIMIT 1000",
    )
    .fetch_all(&store.pool)
    .await?;
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for prefs in rows
        .iter()
        .filter_map(|r| serde_json::from_str::<Value>(r).ok())
    {
        let own = ["metadataLocale", "locale"]
            .iter()
            .filter_map(|k| prefs.get(*k).and_then(Value::as_str))
            .map(str::to_owned);
        let fallbacks = prefs
            .get("metadataFallbackLocales")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned);
        // An account's first choice counts for more than its fallbacks.
        for (weight, tag) in own.map(|t| (3, t)).chain(fallbacks.map(|t| (1, t))) {
            if let Some(tag) = ml::normalize(&tag).filter(|t| t != ml::NEUTRAL) {
                *counts.entry(tag).or_default() += weight;
            }
        }
    }
    let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut out: Vec<String> = ml::normalize(library_locale).into_iter().collect();
    for (tag, _) in ranked {
        if out.len() >= limit {
            break;
        }
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    Ok(out)
}

/// Locale chain for one account: explicit request, then the account's metadata locale,
/// then its interface language, then its fallbacks and the server default. Anonymous or
/// preference-less callers get the server default only, so every caller of the
/// catalogue is deterministic.
pub async fn locale_chain(
    store: &IdentityStore,
    user_id: Option<&str>,
    requested: Option<&str>,
) -> Result<Vec<String>, sqlx::Error> {
    let saved: Option<String> = if let Some(user) = user_id {
        crate::database::query_scalar(
            "SELECT preferences_json FROM user_preferences WHERE user_id=?",
        )
        .bind(user)
        .fetch_optional(&store.pool)
        .await?
    } else {
        None
    };
    locale_chain_with_prefs(store, saved.as_deref(), requested).await
}

/// Build a caller's locale chain from already-loaded `user_preferences.preferences_json`.
pub(crate) async fn locale_chain_with_prefs(
    store: &IdentityStore,
    saved_prefs: Option<&str>,
    requested: Option<&str>,
) -> Result<Vec<String>, sqlx::Error> {
    let mut preferred: Vec<String> = requested.map(str::to_owned).into_iter().collect();
    let mut fallbacks: Vec<String> = Vec::new();
    if let Some(prefs) = saved_prefs.and_then(|s| serde_json::from_str::<Value>(s).ok()) {
        for key in ["metadataLocale", "locale"] {
            if let Some(v) = prefs.get(key).and_then(Value::as_str) {
                preferred.push(v.to_owned());
            }
        }
        if let Some(list) = prefs
            .get("metadataFallbackLocales")
            .and_then(Value::as_array)
        {
            fallbacks.extend(list.iter().filter_map(Value::as_str).map(str::to_owned));
        }
    }
    let default = crate::server_settings::load(store)
        .await
        .ok()
        .map(|s| s.preferred_metadata_language);
    Ok(ml::chain(
        None,
        preferred.iter().chain(fallbacks.iter()),
        default.as_deref(),
    ))
}

/// Localize a work detail: the work itself, its seasons and every episode on the page.
pub async fn localize_detail(
    store: &IdentityStore,
    detail: &mut Value,
    chain: &[String],
) -> Result<(), sqlx::Error> {
    if chain.is_empty() {
        return Ok(());
    }
    localize_values(store, std::slice::from_mut(detail), chain).await?;
    let Some(seasons) = detail.get_mut("seasons").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    localize_seasons(store, seasons, chain).await?;
    // One episode query for the whole page instead of one per season.
    let mut episodes: Vec<Value> = Vec::new();
    let mut counts: Vec<usize> = Vec::with_capacity(seasons.len());
    for season in seasons.iter_mut() {
        let own = season
            .get_mut("episodes")
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
            .unwrap_or_default();
        counts.push(own.len());
        episodes.extend(own);
    }
    localize_values(store, &mut episodes, chain).await?;
    let mut rest = episodes.into_iter();
    for (season, count) in seasons.iter_mut().zip(counts) {
        if let Some(target) = season.get_mut("episodes").and_then(Value::as_array_mut) {
            target.extend(rest.by_ref().take(count));
        }
    }
    Ok(())
}

/// Apply the caller's language to works, episodes and series seasons already rendered.
/// An empty chain leaves the legacy single-language output untouched.
pub async fn localize_values(
    store: &IdentityStore,
    items: &mut [Value],
    chain: &[String],
) -> Result<(), sqlx::Error> {
    if chain.is_empty() || items.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = items
        .iter()
        .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let mut stored = load_rows(store, Table::Work, &ids, chain).await?;
    let mut legacy = load_legacy(store, &ids).await?;
    for item in items.iter_mut() {
        let Some(id) = item.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let mut rows = stored.remove(&id).unwrap_or_default();
        rows.extend(legacy.remove(&id).unwrap_or_default());
        overlay(item, &ml::select(&rows, chain), chain);
    }
    Ok(())
}

/// Same for the `seasons` array of a series detail (each season has its own `id`).
pub async fn localize_seasons(
    store: &IdentityStore,
    seasons: &mut [Value],
    chain: &[String],
) -> Result<(), sqlx::Error> {
    if chain.is_empty() || seasons.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = seasons
        .iter()
        .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let mut stored = load_rows(store, Table::Season, &ids, chain).await?;
    for season in seasons.iter_mut() {
        let Some(id) = season.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        let selected = ml::select(&stored.remove(&id).unwrap_or_default(), chain);
        if let (Some((title, _)), Some(object)) = (&selected.title, season.as_object_mut()) {
            object.insert("title".into(), json!(title));
            object.insert("localization".into(), describe(chain, &selected));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nfo_and_provider_editorial_use_the_library_language() {
        let nfo = json!({"provenance": "nfo", "title": "Les Évadés", "plot": "Résumé"});
        let rows = legacy_rows(&nfo, &Value::Null, Some("fr"));
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (
                rows[0].locale.as_str(),
                rows[0].source.as_str(),
                rows[0].priority
            ),
            ("fr", "nfo", ml::PRIORITY_NFO)
        );
        let provider = json!({"provenance": "identify:tmdb", "title": "T", "outline": "tag"});
        let rows = legacy_rows(&provider, &provider, Some("en"));
        assert!(
            rows.iter()
                .all(|r| r.source == "provider:tmdb" && r.locale == "en")
        );
        assert_eq!(rows[0].overview.as_deref(), Some("tag"));
    }

    #[test]
    fn a_hand_edit_is_neutral_and_manual_but_an_identification_is_not() {
        let human = json!({"provenance": "extension", "title": "Corrigé"});
        let rows = legacy_rows(&Value::Null, &human, Some("fr"));
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].locale.as_str(), rows[0].source.as_str()),
            ("und", "manual")
        );
        // No provenance at all is also treated as a human decision.
        let rows = legacy_rows(&Value::Null, &json!({"title": "X"}), None);
        assert_eq!(rows[0].source, "manual");
        // The identification copies its result into manual_json: it must not outrank
        // other languages' provider translations.
        let identified = json!({"provenance": "identify:tmdb", "title": "Titre"});
        let rows = legacy_rows(&Value::Null, &identified, Some("fr"));
        assert_eq!(
            (rows[0].locale.as_str(), rows[0].source.as_str()),
            ("fr", "provider:tmdb")
        );
        assert!(legacy_rows(&Value::Null, &identified, None).is_empty());
    }

    #[test]
    fn overlay_replaces_the_blob_and_reports_the_resolved_locale() {
        let mut item = json!({"id": "w", "title": "Base", "plot": "p", "translations": {"fr": {"title": "x"}}});
        let selected = ml::Selected {
            title: Some(("Titre".into(), "fr-CA".into())),
            ..Default::default()
        };
        overlay(&mut item, &selected, &["fr-CA".into(), "fr".into()]);
        assert_eq!(item["title"], "Titre");
        assert_eq!(
            item["plot"], "p",
            "a missing translation keeps the stored text"
        );
        assert_eq!(item["translations"], json!({}));
        assert_eq!(
            item["localization"],
            json!({"requested": "fr-CA", "title": "fr-CA", "overview": null})
        );
    }
}

#[cfg(test)]
mod store_tests;
