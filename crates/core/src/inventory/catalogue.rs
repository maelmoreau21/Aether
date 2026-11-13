//! Catalogue resource lookups, filtered pages, work details and source availability.

mod cache;

use crate::database::{query, query_scalar};
use crate::identity::IdentityStore;
use cache::CATALOGUE_CACHE;
pub use cache::{
    BoundedCatalogueCache, catalogue_cache_stats, estimate_json_bytes, invalidate_catalogue_cache,
};
use serde_json::{Value, json};
use sqlx::{Execute, Row};
use std::collections::HashMap;

pub async fn resource_library(
    store: &IdentityStore,
    kind: &str,
    resource_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let query = match kind {
        "work" => "SELECT library_id FROM inventory_works WHERE id=?",
        "image" => {
            "SELECT i.library_id FROM inventory_images i JOIN libraries l ON l.id=i.library_id JOIN media_nodes n ON n.id=l.node_id WHERE i.id=? AND i.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'"
        }
        "asset" | "media" => {
            "SELECT a.library_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id JOIN media_nodes n ON n.id=l.node_id WHERE a.id=? AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') AND n.status != 'revoked'"
        }
        _ => return Ok(None),
    };
    query_scalar(query)
        .bind(resource_id)
        .fetch_optional(&store.pool)
        .await
}

fn work(row: &sqlx::any::AnyRow) -> Value {
    let mut metadata: Value =
        serde_json::from_str(row.get("editorial_json")).unwrap_or_else(|_| json!({}));
    if let Some(manual) = row
        .try_get::<Option<String>, _>("manual_json")
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        && let Some(object) = metadata.as_object_mut()
    {
        object.extend(manual);
    }
    let kind: String = row.try_get("kind").unwrap_or_else(|_| "film".to_string());
    let season_count: Option<i64> = row.try_get("season_count").ok().flatten();
    let episode_count: Option<i64> = row.try_get("episode_count").ok().flatten();
    let node_id: Option<String> = row.try_get("node_id").ok().flatten();
    let node_status: Option<String> = row.try_get("node_status").ok().flatten();
    let node_available = node_status.as_deref().unwrap_or("connected") == "connected";
    let mut val = json!({
        "id": row.get::<String,_>("id"),
        "libraryId": row.try_get::<String,_>("display_library_id").unwrap_or_else(|_|row.get("library_id")),
        "kind": kind,
        "title": metadata["title"].as_str().map(str::to_owned).unwrap_or_else(||row.get::<String,_>("title")),
        "sortTitle": metadata["sort_title"].as_str().map(str::to_owned).or_else(||row.try_get::<String,_>("display_title_sort").ok().filter(|s|!s.is_empty())),
        "year": metadata["year"].as_i64().or_else(||row.get::<Option<i64>,_>("year")),
        "minimumAge":row.try_get::<Option<i64>,_>("minimum_age").ok().flatten(),
        "addedAt": metadata["added_at"].as_i64().or_else(||row.try_get::<Option<i64>,_>("added_at").ok().flatten()),
        "description": metadata["plot"].as_str().or_else(||metadata["outline"].as_str()).unwrap_or(""),
        "plot": metadata["plot"],
        "genres": metadata["genres"].as_array().cloned().unwrap_or_default(),
        "tags": metadata["tags"].as_array().cloned().unwrap_or_default(),
        "studios": metadata["studios"].as_array().cloned().unwrap_or_default(),
        "countries": metadata["countries"].as_array().cloned().unwrap_or_default(),
        "originalTitle": metadata["original_title"],
        "originalLanguage": metadata["original_language"],
        "outline": metadata["outline"],
        "premiered": metadata["premiered"],
        "rating": metadata["rating"],
        "criticRating": metadata["critic_rating"],
        "mpaa": metadata["mpaa"],
        "collection": metadata["collection"],
        "locked": metadata["locked"].as_bool().unwrap_or(false),
        "lockedFields": metadata["locked_fields"].as_array().cloned().unwrap_or_default(),
        "actors": crate::extensions::person_images::decorate_credits(
            &row.get::<String, _>("id"),
            metadata["actors"].as_array().map(Vec::as_slice).unwrap_or_default(),
        ),
        "crewCredits": crate::extensions::person_images::decorate_credits(
            &row.get::<String, _>("id"),
            metadata["crewCredits"].as_array().map(Vec::as_slice).unwrap_or_default(),
        ),
        "directors": metadata["directors"].as_array().cloned().unwrap_or_default(),
        "writers": metadata["writers"].as_array().cloned().unwrap_or_default(),
        "ratings": metadata["ratings"].as_array().cloned().unwrap_or_default(),
        "externalIds": metadata["external_ids"],
        "metadataProvenance": metadata["provenance"].as_str().unwrap_or("local"),
        "metadataWarnings": metadata["warnings"].as_array().cloned().unwrap_or_default(),
        "synthetic": false,
        "episode": row.try_get::<String,_>("kind").is_ok_and(|kind| kind == "episode"),
        "posterUrl": row.try_get::<Option<String>,_>("poster_url").ok().flatten()
            .or_else(|| metadata["poster_url"].as_str().or_else(|| metadata["posterUrl"].as_str()).map(str::to_owned)),
        "artwork": {
            "backdrop": row.try_get::<Option<String>,_>("backdrop_url").ok().flatten()
                .or_else(|| metadata["backdrop_url"].as_str().or_else(|| metadata["backdropUrl"].as_str()).map(str::to_owned)),
            "logo": row.try_get::<Option<String>,_>("logo_url").ok().flatten()
                .or_else(|| metadata["logo_url"].as_str().or_else(|| metadata["logoUrl"].as_str()).map(str::to_owned)),
            "thumb": row.try_get::<Option<String>,_>("thumb_url").ok().flatten()
                .or_else(|| metadata["thumb_url"].as_str().or_else(|| metadata["thumbUrl"].as_str()).or_else(|| metadata["still_url"].as_str()).or_else(|| metadata["stillUrl"].as_str()).map(str::to_owned))
        },
        "translations": metadata.get("translations").cloned().unwrap_or_else(|| json!({})),
        "nodeId": node_id.unwrap_or_else(|| "local".to_string()),
        "nodeStatus": node_status.unwrap_or_else(|| "connected".to_string()),
        "available": node_available
    });
    if kind == "series" {
        val["seasonCount"] = json!(season_count.unwrap_or(0));
        val["episodeCount"] = json!(episode_count.unwrap_or(0));
    }
    val
}

#[derive(Debug, Clone, Default)]
pub struct CatalogueFilter {
    /// Account-sensitive derived views can require fresh permission/catalogue reads.
    pub bypass_cache: bool,
    pub user_id: Option<String>,
    pub hide_watched: bool,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub quality: Option<i64>,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub person: Option<String>,
    /// Seed of the `random` sort: one seed gives one stable order (pagination), another seed
    /// another sample. Ignored by the other sorts.
    pub seed: Option<u64>,
    /// `random` sort only: keep works whose effective rating (manual, else provider) is at
    /// least this value.
    pub min_rating: Option<f64>,
    /// Metadata locale chain of the caller. Part of the cache key through `Debug`:
    /// two languages never share a rendered page. Empty keeps the stored language.
    pub locale_chain: Vec<String>,
}
fn push_search_filter(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    search_clean: &str,
    use_fts: bool,
    fast_avail: bool,
) {
    if search_clean.is_empty() {
        return;
    }
    // A work is also found by its translated and original titles. They live in
    // `work_search_titles` (one row per distinct folded title), so a work is never duplicated.
    const ALTERNATE_TITLES: &str =
        " OR w.id IN (SELECT st.work_id FROM work_search_titles st WHERE instr(st.title_search, ";
    if use_fts && search_clean.chars().count() >= 3 {
        let fts_query = format!("\"{}\"", search_clean.replace('"', "\"\""));
        builder
            .push(" AND (w.id IN (SELECT work_id FROM inventory_works_fts WHERE inventory_works_fts.search_text MATCH ")
            .push_bind(fts_query)
            .push(")")
            .push(ALTERNATE_TITLES)
            .push_bind(search_clean.to_owned())
            .push(") > 0)) ");
        return;
    }
    if fast_avail {
        builder
            .push(" AND (instr(w.display_title_sort, ")
            .push_bind(search_clean.to_owned())
            .push(") > 0")
            .push(ALTERNATE_TITLES)
            .push_bind(search_clean.to_owned())
            .push(") > 0)) ");
    } else {
        builder
            .push(" AND (instr(CASE WHEN w.display_title_sort != '' THEN w.display_title_sort ELSE lower(COALESCE(json_extract(w.manual_json,'$.title'),w.title_sort)) END, ")
            .push_bind(search_clean.to_owned())
            .push(") > 0")
            .push(ALTERNATE_TITLES)
            .push_bind(search_clean.to_owned())
            .push(") > 0)) ");
    }
}

/// The title a work sorts under: its best translation in `language` (manual corrections first,
/// then by source priority) or the stored library-language key.
fn push_title_key(builder: &mut sqlx::QueryBuilder<sqlx::Any>, language: Option<&str>) {
    match language {
        Some(language) => {
            builder
                .push("COALESCE((SELECT COALESCE(NULLIF(t.sort_title,''), lower(t.title)) FROM work_translations t WHERE t.work_id=w.id AND t.title IS NOT NULL AND t.title<>'' AND (t.language=")
                .push_bind(language.to_owned())
                .push(" OR (t.locale='und' AND t.source='manual')) ORDER BY t.priority DESC, CASE WHEN t.language=")
                .push_bind(language.to_owned())
                .push(" THEN 0 ELSE 1 END, t.locale LIMIT 1), w.display_title_sort)");
        }
        None => {
            builder.push("w.display_title_sort");
        }
    }
}

fn push_availability_filter(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    target_kind: Option<&str>,
    fast_avail: bool,
) {
    if fast_avail {
        match target_kind {
            Some("film") => {
                builder.push(" AND w.kind = 'film' AND w.has_available_asset = 1 ");
            }
            Some("series") => {
                builder.push(" AND w.kind = 'series' AND w.has_available_asset = 1 ");
            }
            _ => {
                builder.push(" AND w.kind IN ('film', 'series') AND w.has_available_asset = 1 ");
            }
        }
    } else {
        match target_kind {
            Some("film") => {
                builder.push(
                    " AND w.kind = 'film' AND EXISTS ( \
                       SELECT 1 FROM inventory_editions e \
                       JOIN inventory_versions v ON v.edition_id=e.id \
                       JOIN inventory_sources s ON s.version_id=v.id \
                       JOIN inventory_assets a ON a.source_id=s.id \
                       JOIN libraries l ON l.id=s.library_id \
                       JOIN media_nodes n ON n.id=l.node_id \
                       WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND n.status != 'revoked' AND l.enabled=1 \
                     ) ",
                );
            }
            Some("series") => {
                builder.push(
                    " AND w.kind = 'series' AND EXISTS ( \
                       SELECT 1 FROM inventory_seasons s \
                       JOIN inventory_episodes ep ON ep.season_id=s.id \
                       JOIN inventory_editions e ON e.work_id=ep.work_id \
                       JOIN inventory_versions v ON v.edition_id=e.id \
                       JOIN inventory_sources src ON src.version_id=v.id \
                       JOIN inventory_assets a ON a.source_id=src.id \
                       JOIN libraries l ON l.id=src.library_id \
                       JOIN media_nodes n ON n.id=l.node_id \
                       WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
                     ) ",
                );
            }
            _ => {
                builder.push(
                    " AND ( \
                       (w.kind = 'film' AND EXISTS ( \
                          SELECT 1 FROM inventory_editions e \
                          JOIN inventory_versions v ON v.edition_id=e.id \
                          JOIN inventory_sources s ON s.version_id=v.id \
                          JOIN inventory_assets a ON a.source_id=s.id \
                          JOIN libraries l ON l.id=s.library_id \
                          JOIN media_nodes n ON n.id=l.node_id \
                          WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND n.status != 'revoked' AND l.enabled=1 \
                       )) \
                       OR \
                       (w.kind = 'series' AND EXISTS ( \
                          SELECT 1 FROM inventory_seasons s \
                          JOIN inventory_episodes ep ON ep.season_id=s.id \
                          JOIN inventory_editions e ON e.work_id=ep.work_id \
                          JOIN inventory_versions v ON v.edition_id=e.id \
                          JOIN inventory_sources src ON src.version_id=v.id \
                          JOIN inventory_assets a ON a.source_id=src.id \
                          JOIN libraries l ON l.id=src.library_id \
                          JOIN media_nodes n ON n.id=l.node_id \
                          WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
                       )) \
                     ) ",
                );
            }
        }
    }
}

fn push_catalogue_filters(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    filter: &CatalogueFilter,
    has_parental_policy: bool,
    fast_avail: bool,
) {
    if let Some(person) = &filter.person {
        // JSON string quoting prevents partial-name matches and handles quotes/backslashes.
        // Manual fields override their corresponding provider fields independently.
        let quoted = serde_json::to_string(person.trim()).unwrap_or_default();
        builder.push(" AND (instr(COALESCE(json_extract(w.manual_json,'$.actors'),json_extract(w.editorial_json,'$.actors'),''), ")
            .push_bind(format!("\"name\":{quoted}"))
            .push(")>0 OR instr(COALESCE(json_extract(w.manual_json,'$.actors'),json_extract(w.editorial_json,'$.actors'),''), ")
            .push_bind(format!("\"name\": {quoted}"))
            .push(")>0 OR instr(COALESCE(json_extract(w.manual_json,'$.directors'),json_extract(w.editorial_json,'$.directors'),''), ")
            .push_bind(quoted.clone())
            .push(")>0 OR instr(COALESCE(json_extract(w.manual_json,'$.writers'),json_extract(w.editorial_json,'$.writers'),''), ")
            .push_bind(quoted)
            .push(")>0) ");
    }
    if filter.hide_watched
        && let Some(user) = &filter.user_id
    {
        builder
            .push(" AND NOT EXISTS(SELECT 1 FROM playback_progress watched WHERE watched.user_id=")
            .push_bind(user)
            .push(" AND watched.watched=1 AND watched.work_id=COALESCE((SELECT gm.group_id FROM media_group_members gm WHERE gm.work_id=w.id),w.id)) ");
    }
    if let Some(genre) = &filter.genre {
        let g = genre.trim().to_lowercase();
        if fast_avail {
            builder
                .push(" AND instr(w.genres_csv, ")
                .push_bind(format!("|{g}|"))
                .push(")>0 ");
        } else {
            builder
                .push(" AND (instr(w.genres_csv, ")
                .push_bind(format!("|{g}|"))
                .push(")>0 OR (w.genres_csv = '|' AND instr(lower(json_extract(COALESCE(w.manual_json,w.editorial_json),'$.genres')), ")
                .push_bind(format!("\"{g}\""))
                .push(")>0)) ");
        }
    }
    if let Some(year) = filter.year {
        if fast_avail {
            builder
                .push(" AND w.effective_year=")
                .push_bind(year)
                .push(" ");
        } else {
            builder
                .push(" AND COALESCE(w.effective_year, CAST(json_extract(w.manual_json,'$.year') AS BIGINT), w.year)=")
                .push_bind(year)
                .push(" ");
        }
    }
    if filter.quality.is_some() || filter.codec.is_some() || filter.language.is_some() {
        if fast_avail {
            if let Some(height) = filter.quality {
                builder
                    .push(" AND (w.max_height >= ")
                    .push_bind(height)
                    .push(" OR (w.max_height = 0 AND EXISTS(SELECT 1 FROM inventory_editions fe \
                            JOIN inventory_versions fv ON fv.edition_id=fe.id \
                            JOIN inventory_sources fs ON fs.version_id=fv.id \
                            JOIN inventory_assets fa ON fa.source_id=fs.id JOIN inventory_tracks ft \
                            ON ft.asset_id=fa.id WHERE (fe.work_id=w.id \
                            OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep \
                            JOIN inventory_seasons fseason ON fseason.id=fep.season_id \
                            WHERE fseason.series_id=w.id)) AND fa.available=1 \
                            AND CAST(json_extract(ft.metadata_json,'$.height') AS BIGINT)>=")
                    .push_bind(height)
                    .push("))) ");
            }
            if let Some(codec) = &filter.codec {
                let c = codec.trim().to_ascii_lowercase();
                builder
                    .push(" AND (instr(w.codecs_csv, ")
                    .push_bind(format!(",{c},"))
                    .push(")>0 OR (w.codecs_csv = ',' AND EXISTS(SELECT 1 \
                            FROM inventory_editions fe JOIN inventory_versions fv \
                            ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id \
                            JOIN inventory_assets fa ON fa.source_id=fs.id JOIN inventory_tracks ft \
                            ON ft.asset_id=fa.id WHERE (fe.work_id=w.id \
                            OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep \
                            JOIN inventory_seasons fseason ON fseason.id=fep.season_id \
                            WHERE fseason.series_id=w.id)) AND fa.available=1 \
                            AND lower(json_extract(ft.metadata_json,'$.codec_name'))=")
                    .push_bind(c)
                    .push("))) ");
            }
            if let Some(language) = &filter.language {
                let l = language.trim().to_ascii_lowercase();
                builder
                    .push(" AND (instr(w.languages_csv, ")
                    .push_bind(format!(",{l},"))
                    .push(")>0 OR (w.languages_csv = ',' AND EXISTS(SELECT 1 \
                            FROM inventory_editions fe JOIN inventory_versions fv \
                            ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id \
                            JOIN inventory_assets fa ON fa.source_id=fs.id JOIN inventory_tracks ft \
                            ON ft.asset_id=fa.id WHERE (fe.work_id=w.id \
                            OR fe.work_id IN (SELECT fep.work_id FROM inventory_episodes fep \
                            JOIN inventory_seasons fseason ON fseason.id=fep.season_id \
                            WHERE fseason.series_id=w.id)) AND fa.available=1 \
                            AND lower(json_extract(json_extract(ft.metadata_json,'$.tags'),\
                            '$.language'))=")
                    .push_bind(l)
                    .push("))) ");
            }
        } else {
            builder.push(
                " AND EXISTS(SELECT 1 FROM inventory_editions fe JOIN inventory_versions fv \
                    ON fv.edition_id=fe.id JOIN inventory_sources fs ON fs.version_id=fv.id \
                    JOIN inventory_assets fa ON fa.source_id=fs.id JOIN libraries fl \
                    ON fl.id=fs.library_id JOIN media_nodes fn ON fn.id=fl.node_id \
                    WHERE (fe.work_id=w.id OR fe.work_id IN (SELECT fep.work_id \
                    FROM inventory_episodes fep JOIN inventory_seasons fseason \
                    ON fseason.id=fep.season_id WHERE fseason.series_id=w.id)) AND fa.available=1 \
                    AND fl.enabled=1 AND fn.status!='revoked' AND fs.root_snapshot=fl.root ",
            );
            if let Some(height) = filter.quality {
                builder.push(" AND EXISTS(SELECT 1 FROM inventory_tracks ft WHERE ft.asset_id=fa.id AND CAST(json_extract(ft.metadata_json,'$.height') AS BIGINT)>=").push_bind(height).push(") ");
            }
            if let Some(codec) = &filter.codec {
                builder.push(" AND EXISTS(SELECT 1 FROM inventory_tracks ft WHERE ft.asset_id=fa.id AND lower(json_extract(ft.metadata_json,'$.codec_name'))=").push_bind(codec.to_lowercase()).push(") ");
            }
            if let Some(language) = &filter.language {
                builder.push(" AND EXISTS(SELECT 1 FROM inventory_tracks ft WHERE ft.asset_id=fa.id AND lower(json_extract(json_extract(ft.metadata_json,'$.tags'),'$.language'))=").push_bind(language.to_lowercase()).push(") ");
            }
            builder.push(") ");
        }
    }
    if has_parental_policy && let Some(user) = &filter.user_id {
        builder
            .push(" AND NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=")
            .push_bind(user)
            .push(" AND ((")
            .push(crate::product::AGE_SQL)
            .push(")>policy.maximum_age OR ((")
            .push(crate::product::AGE_SQL)
            .push(") IS NULL AND policy.allow_unrated=0) OR ")
            .push(crate::product::BLOCKED_TAGS_SQL)
            .push(")) ");
    }
}

#[allow(clippy::too_many_arguments)]
fn push_group_filter(
    builder: &mut sqlx::QueryBuilder<sqlx::Any>,
    filter: &CatalogueFilter,
    valid_libs: &[String],
    search: &str,
    has_groups: bool,
    has_parental_policy: bool,
    fast_avail: bool,
    use_fts: bool,
) {
    if !has_groups {
        return;
    }
    builder.push(
        " AND (NOT EXISTS(SELECT 1 FROM media_group_members own_group \
            WHERE own_group.work_id=w.id) OR w.id IN (SELECT COALESCE(MIN(CASE \
            WHEN gn.status='connected' THEN w.id END),MIN(w.id)) FROM media_group_members gm \
            JOIN inventory_works w ON w.id=gm.work_id JOIN libraries gl ON gl.id=w.library_id \
            JOIN media_nodes gn ON gn.id=gl.node_id WHERE gl.enabled=1 AND gn.status!='revoked' ",
    );
    if let Some(user) = &filter.user_id {
        builder
            .push(" AND (EXISTS(SELECT 1 FROM users gu WHERE gu.id=")
            .push_bind(user)
            .push(
                " AND gu.role='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
                JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
                JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=gl.id \
                AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
                OR EXISTS(SELECT 1 \
                FROM effective_library_permissions gp \
                WHERE gp.library_id=gl.id AND gp.user_id=",
            )
            .push_bind(user)
            .push(")) ");
    }
    if fast_avail {
        builder.push(" AND (w.kind!='film' OR w.has_available_asset=1) ");
    } else {
        builder.push(
            " AND (w.kind!='film' OR EXISTS(SELECT 1 FROM inventory_editions ge \
                JOIN inventory_versions gv ON gv.edition_id=ge.id JOIN inventory_sources gs \
                ON gs.version_id=gv.id JOIN inventory_assets ga ON ga.source_id=gs.id \
                WHERE ge.work_id=w.id AND ga.available=1 AND gs.root_snapshot=gl.root)) ",
        );
    }
    builder.push(" AND w.library_id IN (");
    let mut ids = builder.separated(",");
    for id in valid_libs {
        ids.push_bind(id);
    }
    ids.push_unseparated(") ");
    push_search_filter(builder, search, use_fts, fast_avail);
    push_catalogue_filters(builder, filter, has_parental_policy, fast_avail);
    builder.push(" GROUP BY gm.group_id)) ");
}

pub async fn catalogue(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
) -> Result<Value, sqlx::Error> {
    catalogue_filtered(
        store,
        visible,
        kind,
        search,
        sort,
        offset,
        limit,
        &CatalogueFilter::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn catalogue_filtered(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
    filter: &CatalogueFilter,
) -> Result<Value, sqlx::Error> {
    catalogue_page(
        store, visible, kind, search, sort, offset, limit, filter, true,
    )
    .await
}

/// Bounded shelf queries can omit the full-library count and fetch one lookahead row.
#[allow(clippy::too_many_arguments)]
pub async fn catalogue_page(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
    filter: &CatalogueFilter,
    exact_count: bool,
) -> Result<Value, sqlx::Error> {
    let target_kind = match kind {
        Some("film") | Some("films") => Some("film"),
        Some("series") => Some("series"),
        None => None,
        Some(_) => {
            return Ok(json!({
                "items": [],
                "total": if exact_count { Some(0usize) } else { None },
                "hasMore": false,
                "offset": offset,
                "limit": limit,
                "mode": "local-library",
                "notice": "Bibliothèques locales indexées"
            }));
        }
    };

    if visible.is_empty() {
        return Ok(json!({
            "items": [],
            "total": if exact_count { Some(0usize) } else { None },
            "hasMore": false,
            "offset": offset,
            "limit": limit,
            "mode": "local-library",
            "notice": "Bibliothèques locales indexées"
        }));
    }

    // Key on the database, not on this clone's address: each request owns its own clone.
    let cache_key = format!(
        "{}|{:?}|{:?}|{}|{}|{}|{}|{:?}|{}",
        store.cache_scope, visible, target_kind, search, sort, offset, limit, filter, exact_count
    );

    let state_fp: i64 = query_scalar("SELECT gen FROM inventory_cache_generation WHERE id = 1")
        .fetch_optional(&store.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

    let random = sort == "random";
    // Watched status changes independently of inventory generation. Never reuse
    // inventory-only cached pages for this account-owned filter. A random page is drawn
    // afresh from the (cached) eligible pool instead of being cached itself.
    let pool_cacheable = state_fp > 0 && !filter.bypass_cache && !filter.hide_watched;
    let cacheable = pool_cacheable && !random;
    if cacheable
        && let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(mut guard) = lock.lock()
        && let Some(cached_val) = guard.get(&cache_key, state_fp)
    {
        return Ok(cached_val);
    }
    let flight = cacheable.then(|| cache::flight_lock(&cache_key));
    let _computing = match &flight {
        Some(lock) => Some(lock.lock().await),
        None => None,
    };
    // Another request may have filled the page while this one waited.
    if cacheable
        && let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(mut guard) = lock.lock()
        && let Some(cached_val) = guard.get(&cache_key, state_fp)
    {
        return Ok(cached_val);
    }

    // 1. Pre-filter visible libraries to enabled libraries on non-revoked nodes matching target_kind.
    let mut lib_builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT l.id FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.enabled=1 AND n.status!='revoked' AND l.kind IN ('films','series') ",
    );
    match target_kind {
        Some("film") => {
            lib_builder.push("AND l.kind='films' ");
        }
        Some("series") => {
            lib_builder.push("AND l.kind='series' ");
        }
        _ => {}
    }
    lib_builder.push("AND l.id IN (");
    let mut lib_sep = lib_builder.separated(", ");
    for lib in visible {
        lib_sep.push_bind(lib);
    }
    lib_sep.push_unseparated(")");
    let lib_sql = crate::database::normalize_sql(lib_builder.sql().as_str());
    let lib_args = lib_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let valid_libs: Vec<String> =
        sqlx::query_scalar_with::<sqlx::Any, String, _>(sqlx::AssertSqlSafe(lib_sql), lib_args)
            .fetch_all(&store.pool)
            .await?;

    if valid_libs.is_empty() {
        return Ok(json!({
            "items": [],
            "total": if exact_count { Some(0usize) } else { None },
            "hasMore": false,
            "offset": offset,
            "limit": limit,
            "mode": "local-library",
            "notice": "Bibliothèques locales indexées"
        }));
    }

    // 2. Check if any work in valid_libs has `has_available_asset = 1` (using covering index `inventory_works_lib_avail_sort` in O(1))
    //    and whether any library root has diverged from its works' sources.
    let mut fast_check_builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT (CASE WHEN EXISTS(SELECT 1 FROM inventory_works w WHERE w.library_id IN (",
    );
    let mut fc_sep = fast_check_builder.separated(", ");
    for lib in &valid_libs {
        fc_sep.push_bind(lib);
    }
    fc_sep.push_unseparated(
        ") AND w.has_available_asset = 1) THEN 1 ELSE 0 END) - \
         2 * (CASE WHEN EXISTS(SELECT 1 FROM libraries l WHERE l.id IN (",
    );
    let mut fc_sep2 = fast_check_builder.separated(", ");
    for lib in &valid_libs {
        fc_sep2.push_bind(lib);
    }
    fc_sep2.push_unseparated(
        ") AND EXISTS(SELECT 1 FROM inventory_sources s WHERE s.library_id = l.id AND s.root_snapshot != l.root)) THEN 1 ELSE 0 END)",
    );
    let fc_sql = crate::database::normalize_sql(fast_check_builder.sql().as_str());
    let fc_args = fast_check_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let fc_val: i64 =
        sqlx::query_scalar_with::<sqlx::Any, i64, _>(sqlx::AssertSqlSafe(fc_sql), fc_args)
            .fetch_one(&store.pool)
            .await
            .unwrap_or(0);
    let fast_avail = fc_val > 0;

    // 3. Check if any media groups or parental policies exist.
    let has_groups: bool = query_scalar::<_, i64>(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM media_group_members) THEN 1 ELSE 0 END",
    )
    .fetch_one(&store.pool)
    .await?
        != 0;

    let has_parental_policy: bool = if let Some(uid) = &filter.user_id {
        query_scalar::<_, i64>(
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM parental_policies WHERE user_id=?) THEN 1 ELSE 0 END",
        )
        .bind(uid)
        .fetch_one(&store.pool)
        .await?
            != 0
    } else {
        false
    };

    let use_fts: bool = query_scalar::<_, i64>(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='inventory_works_fts') THEN 1 ELSE 0 END",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or(0)
        != 0;
    let search_clean = search.trim().to_lowercase();
    let use_fts_index = use_fts && search_clean.chars().count() >= 3;

    let mut count_builder = sqlx::QueryBuilder::<sqlx::Any>::new(if use_fts_index {
        "SELECT COUNT(*) FROM inventory_works w INDEXED BY inventory_works_fts_sort WHERE w.library_id IN ("
    } else {
        "SELECT COUNT(*) FROM inventory_works w WHERE w.library_id IN ("
    });
    let mut separated = count_builder.separated(", ");
    for lib in &valid_libs {
        separated.push_bind(lib);
    }
    separated.push_unseparated(") ");

    push_search_filter(&mut count_builder, &search_clean, use_fts, fast_avail);
    push_availability_filter(&mut count_builder, target_kind, fast_avail);
    push_catalogue_filters(&mut count_builder, filter, has_parental_policy, fast_avail);
    push_group_filter(
        &mut count_builder,
        filter,
        &valid_libs,
        &search_clean,
        has_groups,
        has_parental_policy,
        fast_avail,
        use_fts,
    );

    let count_sql = crate::database::normalize_sql(count_builder.sql().as_str());
    let arguments = count_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    // A random page counts its eligible pool itself.
    let total = if exact_count && !random {
        Some(
            sqlx::query_scalar_with::<sqlx::Any, i64, _>(sqlx::AssertSqlSafe(count_sql), arguments)
                .fetch_one(&store.pool)
                .await? as usize,
        )
    } else {
        None
    };

    if limit == 0 || total.is_some_and(|total| total == 0 || offset >= total) {
        return Ok(json!({
            "items": [], "total": total, "hasMore": false,
            "offset": offset, "limit": limit,
            "mode": "local-library", "notice": "Bibliothèques locales indexées"
        }));
    }

    // Phase 1: Select only matching `w.id`s ordered by the sort key using the covering index
    // (no table heap fetch or joins to libraries/media_nodes during pagination scan).
    // The rating lives in the metadata JSON: read as text and parsed in Rust, so a malformed
    // value is skipped instead of failing a PostgreSQL cast.
    let rated = random && filter.min_rating.is_some();
    let mut id_builder = sqlx::QueryBuilder::<sqlx::Any>::new(format!(
        "SELECT w.id{} FROM inventory_works w{} WHERE w.library_id IN (",
        if rated {
            ", CAST(COALESCE(json_extract(w.manual_json,'$.rating'),json_extract(w.editorial_json,'$.rating')) AS TEXT) AS rating"
        } else {
            ""
        },
        if use_fts_index {
            " INDEXED BY inventory_works_fts_sort"
        } else {
            ""
        }
    ));
    let mut separated = id_builder.separated(", ");
    for lib in &valid_libs {
        separated.push_bind(lib);
    }
    separated.push_unseparated(") ");

    push_search_filter(&mut id_builder, &search_clean, use_fts, fast_avail);
    push_availability_filter(&mut id_builder, target_kind, fast_avail);
    push_catalogue_filters(&mut id_builder, filter, has_parental_policy, fast_avail);
    push_group_filter(
        &mut id_builder,
        filter,
        &valid_libs,
        &search_clean,
        has_groups,
        has_parental_policy,
        fast_avail,
        use_fts,
    );

    let (page_ids, total, has_more) = if random {
        // Random shelves: the eligible ids (permission, parental and filter checks already in
        // the SQL) come from one index scan, cached per inventory generation; the page is a
        // seeded sample of them, so no full-table ORDER BY RANDOM() is ever run.
        let pool_key = pool_cacheable.then(|| {
            let shape = CatalogueFilter {
                seed: None,
                locale_chain: Vec::new(),
                ..filter.clone()
            };
            format!(
                "pool|{}|{:?}|{:?}|{}|{:?}",
                store.cache_scope, visible, target_kind, search, shape
            )
        });
        let pool = random_pool(store, id_builder, filter.min_rating, pool_key, state_fp).await?;
        let page_ids: Vec<String> = seeded_sample(
            &pool,
            filter.seed.unwrap_or(0),
            offset.saturating_add(limit),
        )
        .into_iter()
        .skip(offset)
        .cloned()
        .collect();
        let has_more = offset.saturating_add(limit) < pool.len();
        (page_ids, exact_count.then_some(pool.len()), has_more)
    } else {
        // Titles sort in the reader's own language when translations exist for it; otherwise the
        // library-language title keeps its place and the index-friendly column is used as before.
        let sort_language = match filter.locale_chain.first() {
            Some(first) => {
                let language = crate::metadata_locale::language_of(first);
                let translated = query_scalar::<_, i64>(
                    "SELECT CASE WHEN EXISTS(SELECT 1 FROM work_translations WHERE language=?) THEN 1 ELSE 0 END",
                )
                .bind(language)
                .fetch_one(&store.pool)
                .await
                .unwrap_or(0)
                    != 0;
                translated.then_some(language)
            }
            None => None,
        };
        let language = sort_language;
        match sort {
            "title-desc" => {
                id_builder.push("ORDER BY ");
                push_title_key(&mut id_builder, language);
                id_builder.push(" DESC, w.id DESC ");
            }
            "year-asc" => {
                id_builder.push("ORDER BY w.effective_year IS NULL, w.effective_year ASC, ");
                push_title_key(&mut id_builder, language);
                id_builder.push(" ASC, w.id ASC ");
            }
            "year-desc" => {
                id_builder.push("ORDER BY w.effective_year IS NULL, w.effective_year DESC, ");
                push_title_key(&mut id_builder, language);
                id_builder.push(" ASC, w.id ASC ");
            }
            "added-desc" => {
                id_builder.push("ORDER BY w.added_at IS NULL, w.added_at DESC, ");
                push_title_key(&mut id_builder, language);
                id_builder.push(" ASC, w.id ASC ");
            }
            "added-asc" => {
                id_builder.push("ORDER BY w.added_at IS NULL, w.added_at ASC, ");
                push_title_key(&mut id_builder, language);
                id_builder.push(" ASC, w.id ASC ");
            }
            _ => {
                id_builder.push("ORDER BY ");
                push_title_key(&mut id_builder, language);
                id_builder.push(" ASC, w.id ASC ");
            }
        }

        id_builder.push("LIMIT ");
        id_builder.push_bind(limit.saturating_add(usize::from(!exact_count)) as i64);
        id_builder.push(" OFFSET ");
        id_builder.push_bind(offset as i64);

        let id_sql = crate::database::normalize_sql(id_builder.sql().as_str());
        let id_args = id_builder
            .build()
            .take_arguments()
            .map_err(sqlx::Error::Encode)?
            .unwrap_or_default();
        let page_ids: Vec<String> =
            sqlx::query_scalar_with::<sqlx::Any, String, _>(sqlx::AssertSqlSafe(id_sql), id_args)
                .fetch_all(&store.pool)
                .await?;

        let has_more = total.map_or(page_ids.len() > limit, |total| {
            offset.saturating_add(page_ids.len()) < total
        });
        (page_ids, total, has_more)
    };
    let fetch_ids: Vec<&String> = page_ids.iter().take(limit).collect();
    if fetch_ids.is_empty() {
        return Ok(json!({
            "items": [],
            "total": total,
            "hasMore": false,
            "offset": offset,
            "limit": limit,
            "mode": "local-library",
            "notice": "Bibliothèques locales indexées"
        }));
    }

    // Phase 2: Fetch full work details only for the <= `limit` selected work IDs.
    let mut data_builder = sqlx::QueryBuilder::<sqlx::Any>::new(
        "SELECT w.*, COALESCE((SELECT f.parent_id FROM library_folders f WHERE f.library_id=w.library_id AND f.active=1),w.library_id) AS display_library_id, wl.node_id AS node_id, wn.status AS node_status, \
           CASE WHEN w.kind = 'series' THEN ( \
             CASE WHEN w.season_count_cached > 0 AND ",
    );
    data_builder.push(if fast_avail { "1=1" } else { "1=0" });
    data_builder.push(
        " THEN w.season_count_cached ELSE ( \
             SELECT COUNT(DISTINCT s.id) FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_editions e ON e.work_id=ep.work_id \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources src ON src.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=src.id \
             JOIN libraries l ON l.id=src.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
           ) END) ELSE NULL END AS season_count, \
           CASE WHEN w.kind = 'series' THEN ( \
             CASE WHEN w.episode_count_cached > 0 AND ",
    );
    data_builder.push(if fast_avail { "1=1" } else { "1=0" });
    data_builder.push(
        " THEN w.episode_count_cached ELSE ( \
             SELECT COUNT(DISTINCT ep.id) FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_editions e ON e.work_id=ep.work_id \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources src ON src.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=src.id \
             JOIN libraries l ON l.id=src.library_id \
             JOIN media_nodes n ON n.id=l.node_id \
             WHERE s.series_id=w.id AND a.available=1 AND src.root_snapshot=l.root AND l.kind='series' AND n.status != 'revoked' AND l.enabled=1 \
           ) END) ELSE NULL END AS episode_count, \
           CAST(NULL AS TEXT) AS poster_url, CAST(NULL AS TEXT) AS backdrop_url, CAST(NULL AS TEXT) AS logo_url \
         FROM inventory_works w \
         JOIN libraries wl ON wl.id=w.library_id \
         JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE w.id IN (",
    );
    let mut id_sep = data_builder.separated(", ");
    for id in &fetch_ids {
        id_sep.push_bind(*id);
    }
    id_sep.push_unseparated(")");

    let data_sql = crate::database::normalize_sql(data_builder.sql().as_str());
    let arguments = data_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let rows = sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(data_sql), arguments)
        .fetch_all(&store.pool)
        .await?;
    let mut row_map = std::collections::HashMap::with_capacity(rows.len());
    for row in &rows {
        let wid: String = row.get("id");
        row_map.insert(wid, work(row));
    }
    let mut items: Vec<Value> = fetch_ids
        .iter()
        .filter_map(|id| row_map.remove(*id))
        .collect();
    crate::product::decorate_many(store, &mut items).await?;
    crate::translations::localize_values(store, &mut items, &filter.locale_chain).await?;

    let response = json!({
        "items": items,
        "total": total,
        "hasMore": has_more,
        "offset": offset,
        "limit": limit,
        "mode": "local-library",
        "notice": "Bibliothèques locales indexées"
    });

    if state_fp > 0 && !filter.hide_watched && !filter.bypass_cache {
        let lock = CATALOGUE_CACHE
            .get_or_init(|| std::sync::Mutex::new(BoundedCatalogueCache::from_env()));
        if let Ok(mut guard) = lock.lock() {
            guard.insert(cache_key, state_fp, response.clone());
        }
    }

    Ok(response)
}

/// Ids eligible for a random page, from cache when the inventory has not changed.
async fn random_pool(
    store: &IdentityStore,
    mut id_builder: sqlx::QueryBuilder<sqlx::Any>,
    min_rating: Option<f64>,
    cache_key: Option<String>,
    state_fp: i64,
) -> Result<Vec<String>, sqlx::Error> {
    if let Some(key) = &cache_key
        && let Some(lock) = CATALOGUE_CACHE.get()
        && let Ok(mut guard) = lock.lock()
        && let Some(Value::Array(ids)) = guard.get(key, state_fp)
    {
        return Ok(ids
            .into_iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect());
    }
    let sql = crate::database::normalize_sql(id_builder.sql().as_str());
    let args = id_builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let pool: Vec<String> = if let Some(minimum) = min_rating {
        sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(sql), args)
            .fetch_all(&store.pool)
            .await?
            .iter()
            .filter(|row| {
                row.try_get::<Option<String>, _>("rating")
                    .ok()
                    .flatten()
                    .and_then(|text| text.trim().parse::<f64>().ok())
                    .is_some_and(|rating| rating.is_finite() && rating >= minimum)
            })
            .map(|row| row.get::<String, _>("id"))
            .collect()
    } else {
        sqlx::query_scalar_with::<sqlx::Any, String, _>(sqlx::AssertSqlSafe(sql), args)
            .fetch_all(&store.pool)
            .await?
    };
    if let Some(key) = cache_key {
        let lock = CATALOGUE_CACHE
            .get_or_init(|| std::sync::Mutex::new(BoundedCatalogueCache::from_env()));
        if let Ok(mut guard) = lock.lock() {
            guard.insert(key, state_fp, json!(pool));
        }
    }
    Ok(pool)
}

/// Stable pseudo-random rank of an id for a seed (FNV-1a then the SplitMix64 finaliser).
fn seeded_rank(seed: u64, id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^ (hash >> 31)
}

/// The first `count` ids of the pool in the seed's order, in O(n) without sorting the pool.
pub(crate) fn seeded_sample(pool: &[String], seed: u64, count: usize) -> Vec<&String> {
    let mut ranked: Vec<(u64, &String)> =
        pool.iter().map(|id| (seeded_rank(seed, id), id)).collect();
    if count < ranked.len() {
        ranked.select_nth_unstable(count);
        ranked.truncate(count);
    }
    ranked.sort_unstable();
    ranked.into_iter().map(|(_, id)| id).collect()
}

pub(crate) fn sort_edition_sources_by_score(editions: &mut [Value]) {
    for edition in editions.iter_mut() {
        if let Some(versions) = edition["versions"].as_array_mut() {
            for version in versions.iter_mut() {
                if let Some(sources) = version["sources"].as_array_mut() {
                    sources.sort_by(|a, b| {
                        let sa = a["score"].as_i64().unwrap_or(0);
                        let sb = b["score"].as_i64().unwrap_or(0);
                        sb.cmp(&sa).then_with(|| {
                            let ida = a["id"].as_str().unwrap_or_default();
                            let idb = b["id"].as_str().unwrap_or_default();
                            ida.cmp(idb)
                        })
                    });
                }
            }
            versions.sort_by(|a, b| {
                let sa = a["sources"]
                    .as_array()
                    .and_then(|s| s.first())
                    .and_then(|s| s["score"].as_i64())
                    .unwrap_or(0);
                let sb = b["sources"]
                    .as_array()
                    .and_then(|s| s.first())
                    .and_then(|s| s["score"].as_i64())
                    .unwrap_or(0);
                sb.cmp(&sa).then_with(|| {
                    let ida = a["id"].as_str().unwrap_or_default();
                    let idb = b["id"].as_str().unwrap_or_default();
                    ida.cmp(idb)
                })
            });
        }
    }
}

pub(crate) fn refresh_work_availability_from_editions(value: &mut Value) {
    let best_from_editions = value["editions"].as_array().and_then(|editions| {
        if editions.is_empty() {
            return None;
        }
        editions
            .iter()
            .flat_map(|ed| ed["versions"].as_array().into_iter().flatten())
            .flat_map(|ver| ver["sources"].as_array().into_iter().flatten())
            .max_by_key(|src| src["score"].as_i64().unwrap_or(i64::MIN))
            .map(|best_src| {
                (
                    best_src["nodeStatus"]
                        .as_str()
                        .unwrap_or("connected")
                        .to_string(),
                    best_src["nodeId"].as_str().map(str::to_string),
                )
            })
    });
    if let Some((best_status, best_nid)) = best_from_editions {
        let any_connected = best_status == "connected";
        value["available"] = json!(any_connected);
        value["nodeStatus"] = json!(best_status);
        if let Some(nid) = best_nid {
            value["nodeId"] = json!(nid);
        }
    } else if let Some(seasons) = value["seasons"].as_array()
        && !seasons.is_empty()
    {
        let any_ep_available = seasons
            .iter()
            .flat_map(|s| s["episodes"].as_array().into_iter().flatten())
            .any(|ep| ep["available"].as_bool().unwrap_or(false));
        if any_ep_available {
            value["available"] = json!(true);
            value["nodeStatus"] = json!("connected");
        }
    }
}

const EDITION_COLUMNS: &str = "SELECT e.work_id, e.id AS edition_id, COALESCE(em.label, 'Édition locale') AS edition_label, \
        v.id AS version_id, COALESCE(vm.label, 'Version locale') AS version_label, \
        s.id AS source_id, a.id AS asset_id, a.mime AS mime, l.node_id AS node_id ";
const EDITION_JOINS: &str = "LEFT JOIN inventory_edition_metadata em ON em.edition_id=e.id \
        JOIN inventory_versions v ON v.edition_id=e.id \
        LEFT JOIN inventory_version_metadata vm ON vm.version_id=v.id \
        JOIN inventory_sources s ON s.version_id=v.id \
        JOIN inventory_assets a ON a.source_id=s.id \
        JOIN libraries l ON l.id=s.library_id \
        JOIN media_nodes n ON n.id=l.node_id \
        WHERE a.available=1 AND s.root_snapshot=l.root AND l.kind IN ('films', 'series') \
        AND n.status != 'revoked' AND l.enabled=1 ";

async fn load_work_editions(
    store: &IdentityStore,
    work_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    Ok(load_editions(store, &[work_id.to_owned()])
        .await?
        .remove(work_id)
        .unwrap_or_default())
}

/// Editions of several works (a page of episodes) in one query instead of one per work.
async fn load_editions(
    store: &IdentityStore,
    work_ids: &[String],
) -> Result<HashMap<String, Vec<Value>>, sqlx::Error> {
    if work_ids.is_empty() {
        return Ok(HashMap::new());
    }
    crate::nodes::ensure_node_registry_warm(store).await?;
    let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(EDITION_COLUMNS);
    builder
        .push("FROM inventory_editions e ")
        .push(EDITION_JOINS)
        .push("AND e.work_id IN (");
    let mut ids = builder.separated(", ");
    for id in work_ids {
        ids.push_bind(id);
    }
    ids.push_unseparated(") ORDER BY e.work_id, e.id, v.id, s.id, a.id");
    let sql = crate::database::normalize_sql(builder.sql().as_str());
    let arguments = builder
        .build()
        .take_arguments()
        .map_err(sqlx::Error::Encode)?
        .unwrap_or_default();
    let rows = sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(sql), arguments)
        .fetch_all(&store.pool)
        .await?;
    Ok(group_edition_rows(store, rows))
}

async fn load_series_editions(
    store: &IdentityStore,
    series_id: &str,
) -> Result<HashMap<String, Vec<Value>>, sqlx::Error> {
    crate::nodes::ensure_node_registry_warm(store).await?;
    let sql = format!(
        "{EDITION_COLUMNS}FROM inventory_seasons se \
         JOIN inventory_episodes ep ON ep.season_id=se.id \
         JOIN inventory_editions e ON e.work_id=ep.work_id \
         {EDITION_JOINS}AND se.series_id=? \
         ORDER BY e.work_id, e.id, v.id, s.id, a.id"
    );
    let rows = query(sqlx::AssertSqlSafe(sql))
        .bind(series_id)
        .fetch_all(&store.pool)
        .await?;
    Ok(group_edition_rows(store, rows))
}

/// Rows ordered by work, edition, version and source, nested as each work's editions.
fn group_edition_rows(
    store: &IdentityStore,
    rows: Vec<sqlx::any::AnyRow>,
) -> HashMap<String, Vec<Value>> {
    let now = media_contracts::protocol::unix_now();
    let mut map: HashMap<String, Vec<Value>> = HashMap::new();
    let mut current_work_id = String::new();
    let mut editions = Vec::new();
    let mut current_edition_id = String::new();
    let mut current_edition_label = String::from("Édition locale");
    let mut current_version_id = String::new();
    let mut current_version_label = String::from("Version locale");
    let mut current_versions = Vec::new();
    let mut current_sources = Vec::new();

    for row in rows {
        let work_id: String = row.get("work_id");
        let edition_id: String = row.get("edition_id");
        let edition_label: String = row.get("edition_label");
        let version_id: String = row.get("version_id");
        let version_label: String = row.get("version_label");
        let source_id: String = row.get("source_id");
        let asset_id: String = row.get("asset_id");
        let mime: String = row.get("mime");
        let node_id: String = row.get("node_id");

        let (node_status, conn_mode, score) = match store.node_registry.get(&node_id) {
            Some(entry) => crate::nodes::score_node_entry(&entry, Some(&mime), None, now),
            None => ("connected".to_owned(), "local".to_owned(), 12_000),
        };

        if work_id != current_work_id || edition_id != current_edition_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            if !current_versions.is_empty() {
                editions.push(json!({
                    "id": current_edition_id,
                    "label": current_edition_label,
                    "versions": std::mem::take(&mut current_versions)
                }));
            }
            if work_id != current_work_id {
                if !editions.is_empty() {
                    sort_edition_sources_by_score(&mut editions);
                    map.insert(current_work_id, std::mem::take(&mut editions));
                }
                current_work_id = work_id;
            }
            current_edition_id = edition_id;
            current_edition_label = edition_label;
            current_version_id = version_id;
            current_version_label = version_label;
        } else if version_id != current_version_id {
            if !current_sources.is_empty() {
                current_versions.push(json!({
                    "id": current_version_id,
                    "label": current_version_label,
                    "sources": std::mem::take(&mut current_sources)
                }));
            }
            current_version_id = version_id;
            current_version_label = version_label;
        }

        current_sources.push(json!({
            "id": source_id,
            "assetIds": [asset_id.clone()],
            "playbackMediaId": asset_id,
            "nodeId": node_id,
            "nodeStatus": node_status,
            "connectionMode": conn_mode,
            "score": score
        }));
    }

    if !current_sources.is_empty() {
        current_versions.push(json!({
            "id": current_version_id,
            "label": current_version_label,
            "sources": current_sources
        }));
    }
    if !current_versions.is_empty() {
        editions.push(json!({
            "id": current_edition_id,
            "label": current_edition_label,
            "versions": current_versions
        }));
    }
    if !editions.is_empty() {
        sort_edition_sources_by_score(&mut editions);
        map.insert(current_work_id, editions);
    }
    map
}

pub struct EpisodePage {
    pub season: Option<i64>,
    pub offset: usize,
    pub limit: usize,
    pub after: Option<String>,
    pub user: String,
    pub role: String,
}

pub async fn detail(store: &IdentityStore, work_id: &str) -> Result<Option<Value>, sqlx::Error> {
    detail_window(store, work_id, None).await
}

/// `detail_window` rendered in the caller's metadata language.
pub async fn detail_window_localized(
    store: &IdentityStore,
    work_id: &str,
    page: Option<&EpisodePage>,
    chain: &[String],
) -> Result<Option<Value>, sqlx::Error> {
    let mut value = detail_window(store, work_id, page).await?;
    if let Some(value) = value.as_mut() {
        crate::translations::localize_detail(store, value, chain).await?;
    }
    Ok(value)
}

pub async fn detail_window(
    store: &IdentityStore,
    work_id: &str,
    page: Option<&EpisodePage>,
) -> Result<Option<Value>, sqlx::Error> {
    let Some(row) = query(
        "SELECT w.*, COALESCE((SELECT f.parent_id FROM library_folders f WHERE f.library_id=w.library_id AND f.active=1),w.library_id) AS display_library_id, wl.node_id AS node_id, wn.status AS node_status, \
         (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size \
          FROM inventory_images i JOIN libraries il ON il.id=i.library_id JOIN media_nodes in_node ON in_node.id=il.node_id \
          WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 AND i.root_snapshot=il.root AND il.enabled=1 AND il.kind IN ('films', 'series') AND in_node.status='connected' \
          ORDER BY i.priority, i.relative_path, i.id LIMIT 1) AS poster_url, \
           (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size \
           FROM inventory_images i WHERE i.work_id=w.id AND i.role='backdrop' AND i.available=1 \
           AND i.root_snapshot=wl.root AND wn.status='connected' \
           ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS backdrop_url, \
           (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='logo' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS logo_url \
         FROM inventory_works w \
         JOIN libraries wl ON wl.id=w.library_id \
         JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE w.id=?",
    )
    .bind(work_id)
    .fetch_optional(&store.pool)
    .await?
    else {
        return Ok(None);
    };

    let mut value = work(&row);
    let images = query(
        "SELECT i.id,i.role,i.modified_ns,i.size \
            FROM inventory_images i \
            JOIN libraries l ON l.id=i.library_id \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE i.work_id=? AND i.available=1 AND i.root_snapshot=l.root AND l.enabled=1 \
            AND n.status='connected' AND i.role!='poster' \
            ORDER BY i.priority,i.relative_path,i.id",
    )
    .bind(work_id)
    .fetch_all(&store.pool)
    .await?;
    let mut artwork = serde_json::Map::new();
    for image in images {
        let role: String = image.get("role");
        artwork.entry(role).or_insert_with(|| {
            json!(format!(
                "/api/v1/assets/{}/image?v={}-{}",
                image.get::<String, _>("id"),
                image.get::<String, _>("modified_ns"),
                image.get::<i64, _>("size")
            ))
        });
    }
    value["artwork"] = json!(artwork);
    crate::product::decorate(store, &mut value).await?;

    let kind: String = row.try_get("kind").unwrap_or_else(|_| "film".to_string());

    if kind == "series"
        && let Some(page) = page
    {
        populate_episode_page(store, work_id, &mut value, page).await?;
    } else if kind == "series" {
        let seasons_rows = query(
            "SELECT id, season_number, title FROM inventory_seasons WHERE series_id=? ORDER BY season_number ASC",
        )
        .bind(work_id)
        .fetch_all(&store.pool)
        .await?;

        let ep_rows = query(
            "SELECT ep.season_id, ep.episode_number, w.*, wl.node_id AS node_id, wn.status AS node_status, \
             (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='thumb' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS thumb_url \
             FROM inventory_seasons s \
             JOIN inventory_episodes ep ON ep.season_id=s.id \
             JOIN inventory_works w ON w.id=ep.work_id \
             JOIN libraries wl ON wl.id=w.library_id \
             JOIN media_nodes wn ON wn.id=wl.node_id \
             WHERE s.series_id=? \
             ORDER BY s.season_number ASC, ep.episode_number ASC",
        )
        .bind(work_id)
        .fetch_all(&store.pool)
        .await?;

        let mut editions_by_work = load_series_editions(store, work_id).await?;

        let mut episodes_by_season: HashMap<String, Vec<Value>> = HashMap::new();
        let mut total_episodes = 0;

        for ep_row in ep_rows {
            let season_id: String = ep_row.get("season_id");
            let ep_work_id: String = ep_row.get("id");
            let ep_num: i64 = ep_row.get("episode_number");

            if let Some(editions) = editions_by_work.remove(&ep_work_id)
                && !editions.is_empty()
            {
                let mut ep_val = work(&ep_row);
                ep_val["kind"] = json!("episode");
                ep_val["number"] = json!(ep_num);
                ep_val["editions"] = json!(editions);
                refresh_work_availability_from_editions(&mut ep_val);
                episodes_by_season
                    .entry(season_id)
                    .or_default()
                    .push(ep_val);
                total_episodes += 1;
            }
        }

        let mut seasons_val = Vec::new();
        for s_row in seasons_rows {
            let season_id: String = s_row.get("id");
            let season_num: i64 = s_row.get("season_number");
            let season_title: String = s_row.get("title");

            if let Some(episodes) = episodes_by_season.remove(&season_id)
                && !episodes.is_empty()
            {
                seasons_val.push(json!({
                    "id": season_id,
                    "number": season_num,
                    "title": season_title,
                    "episodes": episodes
                }));
            }
        }

        value["seasonCount"] = json!(seasons_val.len());
        value["episodeCount"] = json!(total_episodes);
        value["seasons"] = json!(seasons_val);
        value["editions"] = json!([]);
        refresh_work_availability_from_editions(&mut value);
    } else {
        let editions = load_work_editions(store, work_id).await?;
        value["editions"] = json!(editions);
        value["seasons"] = json!([]);
        refresh_work_availability_from_editions(&mut value);
    }

    Ok(Some(value))
}

async fn populate_episode_page(
    store: &IdentityStore,
    series: &str,
    value: &mut Value,
    page: &EpisodePage,
) -> Result<(), sqlx::Error> {
    // The same permissions and parental policy apply before pagination as on direct work access.
    let eligible = format!(
        "wl.enabled=1 AND wn.status!='revoked' AND \
         ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf \
         ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id \
         WHERE hx.id=wl.id AND COALESCE(hxp.options_json,hx.options_json) LIKE \
         '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p \
         WHERE p.library_id=wl.id AND p.user_id=?)) \
         AND {} AND EXISTS(SELECT 1 FROM inventory_editions pe \
         JOIN inventory_versions pv ON pv.edition_id=pe.id JOIN inventory_sources ps ON ps.version_id=pv.id \
         JOIN inventory_assets pa ON pa.source_id=ps.id JOIN libraries pl ON pl.id=ps.library_id \
         JOIN media_nodes pn ON pn.id=pl.node_id WHERE pe.work_id=w.id AND pa.available=1 \
         AND ps.root_snapshot=pl.root AND pl.enabled=1 AND pn.status!='revoked' AND pl.kind IN ('films','series'))",
        crate::product::policy_sql()
    );
    let seasons_sql = format!(
        "SELECT s.id,s.season_number,s.title,COUNT(*) AS episode_count FROM inventory_seasons s \
         JOIN inventory_episodes ep ON ep.season_id=s.id JOIN inventory_works w ON w.id=ep.work_id \
         JOIN libraries wl ON wl.id=w.library_id JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE s.series_id=? AND {eligible} GROUP BY s.id,s.season_number,s.title ORDER BY s.season_number,s.id"
    );
    let seasons = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(
        &seasons_sql,
    )))
    .bind(series)
    .bind(&page.role)
    .bind(&page.user)
    .bind(&page.user)
    .fetch_all(&store.pool)
    .await?;
    let selected_season = page.season.or_else(|| {
        seasons
            .first()
            .map(|row| row.get::<i64, _>("season_number"))
    });
    let anchor: Option<(i64, i64, String)> = if let Some(after) = &page.after {
        crate::database::query_as("SELECT s.season_number,ep.episode_number,w.id FROM inventory_seasons s JOIN inventory_episodes ep ON ep.season_id=s.id JOIN inventory_works w ON w.id=ep.work_id WHERE s.series_id=? AND w.id=?")
            .bind(series).bind(after).fetch_optional(&store.pool).await?
    } else {
        None
    };
    let page_sql = format!(
        "SELECT ep.season_id,s.season_number,ep.episode_number,w.*,wl.node_id,wn.status AS node_status, \
         (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size FROM inventory_images i WHERE i.work_id=w.id AND i.role='thumb' AND i.available=1 AND i.root_snapshot=wl.root AND wn.status='connected' ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS thumb_url \
         FROM inventory_seasons s JOIN inventory_episodes ep ON ep.season_id=s.id JOIN inventory_works w ON w.id=ep.work_id \
         JOIN libraries wl ON wl.id=w.library_id JOIN media_nodes wn ON wn.id=wl.node_id \
         WHERE s.series_id=? AND {eligible} \
         AND (?=1 OR s.season_number=?) \
         AND (?=0 OR s.season_number>? OR (s.season_number=? AND ep.episode_number>?) OR (s.season_number=? AND ep.episode_number=? AND w.id>?)) \
         ORDER BY s.season_number,ep.episode_number,w.id LIMIT ? OFFSET ?"
    );
    let (anchor_season, anchor_episode, anchor_id) =
        anchor.unwrap_or((i64::MAX, i64::MAX, String::new()));
    let after = page.after.is_some();
    let mut rows = sqlx::query(sqlx::AssertSqlSafe(crate::database::normalize_sql(
        &page_sql,
    )))
    .bind(series)
    .bind(&page.role)
    .bind(&page.user)
    .bind(&page.user)
    .bind(i64::from(after))
    .bind(selected_season.unwrap_or(-1))
    .bind(i64::from(after))
    .bind(anchor_season)
    .bind(anchor_season)
    .bind(anchor_episode)
    .bind(anchor_season)
    .bind(anchor_episode)
    .bind(anchor_id)
    .bind((page.limit + 1) as i64)
    .bind(page.offset as i64)
    .fetch_all(&store.pool)
    .await?;
    let has_more = rows.len() > page.limit;
    rows.truncate(page.limit);
    let count = rows.len();
    let returned_season = if after {
        rows.first().map(|row| row.get::<i64, _>("season_number"))
    } else {
        selected_season
    };
    let mut by_season: HashMap<String, Vec<Value>> = HashMap::new();
    let page_ids: Vec<String> = rows.iter().map(|row| row.get("id")).collect();
    let mut page_editions = load_editions(store, &page_ids).await?;
    for row in rows {
        let id: String = row.get("id");
        let mut episode = work(&row);
        episode["kind"] = json!("episode");
        episode["number"] = json!(row.get::<i64, _>("episode_number"));
        episode["seasonNumber"] = json!(row.get::<i64, _>("season_number"));
        episode["editions"] = json!(page_editions.remove(&id).unwrap_or_default());
        refresh_work_availability_from_editions(&mut episode);
        by_season
            .entry(row.get("season_id"))
            .or_default()
            .push(episode);
    }
    let mut total = 0i64;
    let summaries: Vec<Value> = seasons.into_iter().map(|row| {
        let id: String = row.get("id");
        let count: i64 = row.get("episode_count");
        total += count;
        json!({"id":id,"number":row.get::<i64,_>("season_number"),"title":row.get::<String,_>("title"),"episodeCount":count,"episodes":by_season.remove(&id).unwrap_or_default()})
    }).collect();
    value["seasonCount"] = json!(summaries.len());
    value["episodeCount"] = json!(total);
    value["seasons"] = json!(summaries);
    value["editions"] = json!([]);
    value["episodesPage"] = json!({"season":returned_season,"offset":page.offset,"limit":page.limit,"nextOffset":page.offset + count,"hasMore":has_more});
    Ok(())
}
