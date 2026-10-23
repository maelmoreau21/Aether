//! Explicit fixture associations, never a filename/title based media classifier.
use crate::{error, node::Inventory};
use axum::{
    Extension, Json,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const NOTICE: &str = "Catalogue de fixtures synthétiques uniquement. La série réutilise explicitement la mire du film pour tester la navigation ; aucune identification ou métadonnée réelle.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Work {
    id: &'static str,
    kind: &'static str,
    title: &'static str,
    year: Option<u16>,
    description: &'static str,
    library_id: &'static str,
    synthetic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    season_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episode_count: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Source {
    id: String,
    asset_ids: Vec<String>,
    playback_media_id: String,
}

#[derive(Serialize)]
struct MediaVersion {
    id: String,
    label: &'static str,
    sources: Vec<Source>,
}

#[derive(Serialize)]
struct Edition {
    id: String,
    label: &'static str,
    versions: Vec<MediaVersion>,
}

#[derive(Serialize)]
struct Episode {
    #[serde(flatten)]
    work: Work,
    number: u16,
    editions: Vec<Edition>,
}

#[derive(Serialize)]
struct Season {
    id: &'static str,
    number: u16,
    title: &'static str,
    episodes: Vec<Episode>,
}

#[derive(Serialize)]
struct WorkDetail {
    #[serde(flatten)]
    work: Work,
    editions: Vec<Edition>,
    seasons: Vec<Season>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogQuery {
    library: Option<String>,
    genre: Option<String>,
    year: Option<i64>,
    quality: Option<i64>,
    codec: Option<String>,
    language: Option<String>,
    kind: Option<String>,
    q: Option<String>,
    sort: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    count: Option<bool>,
}

fn works(available: bool) -> Vec<Work> {
    if !available {
        return Vec::new();
    }
    vec![
        Work {
            id: "fixture-film",
            kind: "film",
            title: "Mire AV1 — film synthétique",
            year: None,
            description: "Clip de test synthétique AV1 10 bits et Opus. Les copies MKV et MP4 sont associées explicitement à cette seule œuvre, sans identification automatique.",
            library_id: "fixture-films",
            synthetic: true,
            season_count: None,
            episode_count: None,
        },
        Work {
            id: "fixture-series",
            kind: "series",
            title: "Mire AV1 — série synthétique",
            year: None,
            description: "Structure de test : une saison et un épisode réutilisant explicitement le clip synthétique du film. Ce regroupement ne provient pas de métadonnées ni d'une détection de série.",
            library_id: "fixture-series",
            synthetic: true,
            season_count: Some(1),
            episode_count: Some(1),
        },
    ]
}

fn episode() -> Work {
    Work {
        id: "fixture-episode-1",
        kind: "episode",
        title: "Épisode de test — mire AV1",
        year: None,
        description: "Même clip synthétique que le film, réutilisé explicitement pour vérifier le parcours série → saison → épisode → lecteur.",
        library_id: "fixture-series",
        synthetic: true,
        season_count: None,
        episode_count: None,
    }
}

fn editions(work_id: &str) -> Vec<Edition> {
    vec![Edition {
        id: format!("{work_id}-edition"),
        label: "Clip synthétique original",
        versions: vec![MediaVersion {
            id: format!("{work_id}-version-av1"),
            label: "AV1 10 bits · Opus · copies MKV / MP4",
            sources: ["mkv", "mp4"]
                .into_iter()
                .map(|container| Source {
                    id: format!("{work_id}-source-{container}"),
                    asset_ids: ["fr", "en"]
                        .into_iter()
                        .map(|language| format!("demo-{container}-{language}"))
                        .collect(),
                    playback_media_id: format!("demo-{container}"),
                })
                .collect(),
        }],
    }]
}

fn available(inventory: &Inventory) -> bool {
    let assets = inventory.demo_descriptors();
    crate::node::DEMO_FIXTURES
        .iter()
        .all(|(id, _, _)| assets.iter().any(|asset| asset.id == *id))
}

pub async fn list(
    State(inventory): State<Arc<Inventory>>,
    query: Result<Query<CatalogQuery>, QueryRejection>,
    mut visible: Option<Extension<crate::permissions::VisibleLibraries>>,
    store: Option<Extension<crate::identity::IdentityStore>>,
    actor: Option<Extension<crate::auth::Authenticated>>,
) -> Response {
    let Ok(Query(query)) = query else {
        return error(StatusCode::BAD_REQUEST, "invalid_catalog_query");
    };
    let offset = query.offset.unwrap_or(0);
    if let Some(library) = &query.library {
        if library.is_empty() || library.len() > 128 {
            return error(StatusCode::BAD_REQUEST, "invalid_library");
        }
        if let Some(Extension(visible)) = &mut visible {
            let children: Vec<String> = if let Some(Extension(store)) = &store {
                match crate::database::query_scalar(
                    "SELECT library_id FROM library_folders WHERE parent_id=? AND active=1",
                )
                .bind(library)
                .fetch_all(&store.pool)
                .await
                {
                    Ok(ids) => ids,
                    Err(_) => {
                        return error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable");
                    }
                }
            } else {
                vec![]
            };
            if !visible
                .0
                .iter()
                .any(|id| id == library || children.contains(id))
            {
                return error(StatusCode::FORBIDDEN, "permission_denied");
            }
            visible
                .0
                .retain(|id| id == library || children.contains(id));
        }
    }
    let limit = query.limit.unwrap_or(24);
    if !matches!(query.kind.as_deref(), None | Some("film" | "series"))
        || !matches!(
            query.sort.as_deref(),
            None | Some(
                "title-asc" | "title-desc" | "year-asc" | "year-desc" | "added-asc" | "added-desc"
            )
        )
        || !(1..=100).contains(&limit)
        || query.q.as_ref().is_some_and(|q| q.len() > 200)
        || query.genre.as_ref().is_some_and(|v| v.len() > 100)
        || query.codec.as_ref().is_some_and(|v| v.len() > 64)
        || query.language.as_ref().is_some_and(|v| v.len() > 35)
        || query.year.is_some_and(|v| !(1800..=2200).contains(&v))
        || query.quality.is_some_and(|v| !(1..=16384).contains(&v))
    {
        return error(StatusCode::BAD_REQUEST, "invalid_catalog_query");
    }
    let search = query.q.unwrap_or_default().trim().to_lowercase();
    if let (Some(Extension(store)), Some(Extension(visible))) = (&store, &visible)
        && (!inventory.is_fixture_bench() || visible.0.iter().any(|id| !id.starts_with("fixture-")))
    {
        let filter = crate::inventory::CatalogueFilter {
            user_id: actor.map(|v| v.0.user.id),
            genre: query.genre,
            year: query.year,
            quality: query.quality,
            codec: query.codec,
            language: query.language,
        };
        return match crate::inventory::catalogue_page(
            store,
            &visible.0,
            query.kind.as_deref(),
            &search,
            query.sort.as_deref().unwrap_or("title-asc"),
            offset,
            limit,
            &filter,
            query.count.unwrap_or(true),
        )
        .await
        {
            Ok(value) => Json(value).into_response(),
            Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
        };
    }
    let mut items: Vec<_> = works(available(&inventory))
        .into_iter()
        .filter(|work| {
            visible
                .as_ref()
                .is_none_or(|v| v.0.0.iter().any(|id| id == work.library_id))
                && query.kind.as_deref().is_none_or(|kind| work.kind == kind)
                && work.title.to_lowercase().contains(&search)
        })
        .collect();
    items.sort_by_key(|work| work.title.to_lowercase());
    if query.sort.as_deref() == Some("title-desc") {
        items.reverse();
    }
    let total = items.len();
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
    Json(serde_json::json!({
        "items":items, "total": if query.count == Some(false) { None } else { Some(total) },
        "hasMore": offset.saturating_add(limit) < total, "offset":offset, "limit":limit,
        "mode":"localhost-fixtures", "notice":NOTICE
    }))
    .into_response()
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailQuery {
    season: Option<i64>,
    episode_offset: Option<usize>,
    episode_limit: Option<usize>,
    episode_after: Option<String>,
}

pub async fn detail(
    State(inventory): State<Arc<Inventory>>,
    Path(id): Path<String>,
    Query(params): Query<DetailQuery>,
    store: Option<Extension<crate::identity::IdentityStore>>,
    actor: Option<Extension<crate::auth::Authenticated>>,
    visible: Option<Extension<crate::permissions::VisibleLibraries>>,
) -> Response {
    if params
        .episode_limit
        .is_some_and(|limit| limit == 0 || limit > 100)
        || params
            .episode_offset
            .is_some_and(|offset| offset > 1_000_000)
        || params
            .episode_after
            .as_ref()
            .is_some_and(|id| id.len() > 256)
    {
        return error(StatusCode::BAD_REQUEST, "invalid_episode_page");
    }
    let page = params.episode_limit.and_then(|limit| {
        actor
            .as_ref()
            .map(|Extension(actor)| crate::inventory::EpisodePage {
                season: params.season,
                offset: params.episode_offset.unwrap_or(0),
                limit,
                after: params.episode_after.clone(),
                user: actor.user.id.clone(),
                role: actor.user.role.as_str().to_owned(),
            })
    });
    if !id.starts_with("fixture-") {
        return match store {
            Some(Extension(store)) => {
                match crate::inventory::detail_window(&store, &id, page.as_ref()).await {
                    Ok(Some(mut value)) => {
                        if let (Some(Extension(actor)), Some(Extension(visible))) =
                            (&actor, &visible)
                            && crate::product::expand_detail(
                                &store,
                                &actor.user.id,
                                &visible.0,
                                &mut value,
                            )
                            .await
                            .is_err()
                        {
                            return error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "catalogue_unavailable",
                            );
                        }
                        Json(value).into_response()
                    }
                    Ok(None) => error(StatusCode::NOT_FOUND, "unknown_work"),
                    Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
                }
            }
            None => error(StatusCode::NOT_FOUND, "unknown_work"),
        };
    }
    if !available(&inventory) {
        return error(StatusCode::NOT_FOUND, "unknown_work");
    }
    let work = works(true)
        .into_iter()
        .chain(std::iter::once(episode()))
        .find(|work| work.id == id);
    let Some(work) = work else {
        return error(StatusCode::NOT_FOUND, "unknown_work");
    };
    let is_series = work.kind == "series";
    let detail = WorkDetail {
        editions: if is_series { vec![] } else { editions(work.id) },
        seasons: if is_series {
            vec![Season {
                id: "fixture-season-1",
                number: 1,
                title: "Saison de test 1",
                episodes: vec![Episode {
                    work: episode(),
                    number: 1,
                    editions: editions("fixture-episode-1"),
                }],
            }]
        } else {
            vec![]
        },
        work,
    };
    let Ok(mut value) = serde_json::to_value(detail) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "catalogue_unavailable");
    };
    if let Some(page) = page {
        let mut count = 0;
        if let Some(seasons) = value["seasons"].as_array_mut() {
            for season in seasons {
                season["episodeCount"] = serde_json::json!(1);
                if let Some(episodes) = season["episodes"].as_array_mut() {
                    if page.after.is_some()
                        || page.offset > 0
                        || page.season.is_some_and(|number| number != 1)
                    {
                        episodes.clear();
                    }
                    count += episodes.len();
                }
            }
        }
        value["episodesPage"] = serde_json::json!({"season":1,"offset":page.offset,"limit":page.limit,"nextOffset":page.offset+count,"hasMore":false});
    }
    Json(value).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_prepared_media_never_creates_catalog_items() {
        assert!(works(false).is_empty());
    }

    #[test]
    fn fixture_associations_distinguish_work_version_source_and_asset() {
        let items = works(true);
        assert_eq!(items.len(), 2);
        assert_eq!(items.iter().filter(|item| item.kind == "film").count(), 1);
        let editions = editions(items[0].id);
        assert_eq!(editions.len(), 1);
        assert_eq!(editions[0].versions.len(), 1);
        let sources = &editions[0].versions[0].sources;
        assert_eq!(sources.len(), 2);
        assert_ne!(sources[0].id, sources[1].id);
        assert_eq!(sources[0].asset_ids, ["demo-mkv-fr", "demo-mkv-en"]);
        assert_ne!(editions[0].id, editions[0].versions[0].id);
    }
}
