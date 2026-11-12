//! Optional, account-owned local ranking. No provider, persistent profile or result cache.
#[cfg(test)]
mod http_tests;
use crate::{
    auth::AuthState,
    database::{query, query_as},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Execute, Row};
use std::collections::{HashMap, HashSet};

const POOL_LIMIT: usize = 1000;
type Error = (StatusCode, Json<Value>);
fn error(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error":code})))
}
fn db(_: sqlx::Error) -> Error {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "recommendations_unavailable",
    )
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    enabled: bool,
    exclude_watched: bool,
    history_days: u16,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            exclude_watched: true,
            history_days: 180,
        }
    }
}
impl Preferences {
    fn valid(&self) -> bool {
        (1..=3650).contains(&self.history_days)
    }
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/account/recommendations", get(recommend))
        .route(
            "/api/v1/account/recommendations/preferences",
            get(read_preferences).put(write_preferences),
        )
        .layer(DefaultBodyLimit::max(2048))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
async fn preferences(auth: &AuthState, user: &str) -> Result<Preferences, Error> {
    let row:Option<(i64,i64,i64)>=query_as("SELECT enabled,exclude_watched,history_days FROM recommendation_preferences WHERE user_id=?").bind(user).fetch_optional(auth.store.pool()).await.map_err(db)?;
    Ok(row
        .map(|(enabled, exclude, days)| Preferences {
            enabled: enabled == 1,
            exclude_watched: exclude == 1,
            history_days: days as u16,
        })
        .unwrap_or_default())
}
async fn read_preferences(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Preferences>, Error> {
    let actor = auth
        .authenticate(&headers)
        .await
        .map_err(|s| error(s, "authentication_required"))?;
    Ok(Json(preferences(&auth, &actor.user.id).await?))
}
async fn write_preferences(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Preferences>,
) -> Result<Json<Preferences>, Error> {
    let actor = auth
        .authenticate(&headers)
        .await
        .map_err(|s| error(s, "authentication_required"))?;
    auth.require_csrf(&headers, &actor)
        .map_err(|s| error(s, "csrf_required"))?;
    if !input.valid() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_recommendation_preferences",
        ));
    }
    query("INSERT INTO recommendation_preferences(user_id,enabled,exclude_watched,history_days) VALUES(?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET enabled=excluded.enabled,exclude_watched=excluded.exclude_watched,history_days=excluded.history_days")
        .bind(&actor.user.id).bind(i64::from(input.enabled)).bind(i64::from(input.exclude_watched)).bind(i64::from(input.history_days)).execute(auth.store.pool()).await.map_err(db)?;
    Ok(Json(input))
}

fn key(item: &Value) -> &str {
    item["globalMediaId"]
        .as_str()
        .or_else(|| item["id"].as_str())
        .unwrap_or("")
}
fn terms(item: &Value, field: &str) -> HashSet<String> {
    item[field]
        .as_array()
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|v| v.as_str().or_else(|| v["name"].as_str()))
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}
struct Features([HashSet<String>; 3]);
impl Features {
    fn new(item: &Value) -> Self {
        Self([
            terms(item, "genres"),
            terms(item, "directors"),
            terms(item, "actors"),
        ])
    }
    fn affinity(&self, other: &Self) -> f64 {
        [2.0, 4.0, 1.0]
            .iter()
            .enumerate()
            .map(|(i, weight)| self.0[i].intersection(&other.0[i]).count().min(4) as f64 * weight)
            .sum()
    }
}
#[derive(Default)]
struct Signal {
    weight: f64,
    watched: bool,
    /// Explicit positive feedback (favorite or good rating), as opposed to a recent watch.
    liked: bool,
}
struct Scored {
    item: Value,
    score: f64,
    reason: String,
    /// Localizable explanation: `{kind: "liked"|"watched", title}`; null for plain discovery.
    because: Value,
    seed_id: Option<String>,
    genres: HashSet<String>,
}
fn rank(
    pool: &[Value],
    signals: &HashMap<String, Signal>,
    exclude_watched: bool,
) -> (Vec<Value>, bool) {
    let seeds: Vec<(&Value, f64, Features, bool)> = pool
        .iter()
        .filter_map(|item| {
            signals
                .get(key(item))
                .filter(|s| s.weight != 0.0)
                .map(|s| (item, s.weight, Features::new(item), s.liked))
        })
        .take(100)
        .collect();
    let personalized = seeds.iter().any(|(_, w, _, _)| *w > 0.0);
    let mut seen = HashSet::new();
    let mut scored: Vec<Scored> = pool
        .iter()
        .filter(|item| seen.insert(key(item).to_owned()))
        .filter(|item| !exclude_watched || !signals.get(key(item)).is_some_and(|s| s.watched))
        .filter(|item| !signals.get(key(item)).is_some_and(|s| s.weight < 0.0))
        .map(|item| {
            let features = Features::new(item);
            let mut score = item["rating"]
                .as_f64()
                .filter(|n| n.is_finite())
                .unwrap_or(0.0)
                .clamp(0.0, 10.0)
                * 0.03;
            let mut best = 0.0;
            let mut reason = "À découvrir dans votre catalogue".to_owned();
            let mut because = Value::Null;
            let mut seed_id = None;
            for (seed, weight, seed_features, liked) in &seeds {
                if key(seed) == key(item) {
                    continue;
                }
                let contribution = features.affinity(seed_features) * weight;
                score += contribution;
                if contribution > best {
                    best = contribution;
                    reason = format!(
                        "Selon votre intérêt pour {}",
                        seed["title"]
                            .as_str()
                            .unwrap_or("un titre de votre catalogue")
                    );
                    because = json!({
                        "kind": if *liked { "liked" } else { "watched" },
                        "title": seed["title"],
                    });
                    seed_id = seed["id"].as_str().map(str::to_owned);
                }
            }
            Scored {
                item: item.clone(),
                score,
                reason,
                because,
                seed_id,
                genres: features.0[0].clone(),
            }
        })
        .collect();
    let mut result = Vec::new();
    let mut genres: HashMap<String, usize> = HashMap::new();
    while !scored.is_empty() && result.len() < 16 {
        // Penalize repeated genres, while keeping small/single-genre catalogues usable.
        let adjusted = |entry: &Scored| {
            entry.score
                - entry
                    .genres
                    .iter()
                    .map(|g| *genres.get(g).unwrap_or(&0) as f64)
                    .sum::<f64>()
                    * 0.8
        };
        scored.sort_by(|a, b| {
            adjusted(b)
                .total_cmp(&adjusted(a))
                .then_with(|| key(&a.item).cmp(key(&b.item)))
        });
        let mut chosen = scored.remove(0);
        for genre in chosen.genres {
            *genres.entry(genre).or_default() += 1;
        }
        chosen.item["recommendationReason"] = json!(chosen.reason);
        chosen.item["recommendationBecause"] = chosen.because;
        chosen.item["recommendationSeedId"] = json!(chosen.seed_id);
        result.push(chosen.item);
    }
    (result, personalized)
}

async fn recommend(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth
        .authenticate(&headers)
        .await
        .map_err(|s| error(s, "authentication_required"))?;
    let options = preferences(&auth, &actor.user.id).await?;
    if !options.enabled {
        return Ok(Json(
            json!({"enabled":false,"items":[],"mode":"disabled","inspectedCount":0}),
        ));
    }
    let visible: Vec<String> = query_as::<_, (String,)>(
        "SELECT l.id \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE l.enabled=1 AND n.status!='revoked' AND ((?='administrator' \
            AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf \
            ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp \
            ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
            AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
            OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id \
            AND p.user_id=?)) AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent \
            ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))",
    )
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?
    .into_iter()
    .map(|(id,)| id)
    .collect();
    let locale_chain = crate::translations::locale_chain(&auth.store, Some(&actor.user.id), None)
        .await
        .map_err(db)?;
    let filter = crate::inventory::CatalogueFilter {
        locale_chain,
        bypass_cache: true,
        user_id: Some(actor.user.id.clone()),
        ..Default::default()
    };
    let catalogue = crate::inventory::catalogue_page(
        &auth.store,
        &visible,
        None,
        "",
        "added-desc",
        0,
        POOL_LIMIT,
        &filter,
        false,
    )
    .await
    .map_err(db)?;
    let pool = catalogue["items"].as_array().cloned().unwrap_or_default();
    let mut signals: HashMap<String, Signal> = HashMap::new();
    // IDs/feedback belong exclusively to this account; only visible pool metadata can become a seed.
    let feedback = signal_rows(&auth, &actor.user.id, &pool, false).await?;
    for row in feedback {
        let id: String = row.get("work_id");
        let favorite: i64 = row.get("favorite");
        let rating: Option<f64> = row.get("rating");
        let signal = signals.entry(id).or_default();
        signal.weight = if rating.is_some_and(|n| n <= 2.0) {
            -2.0
        } else {
            f64::from(favorite as i32) * 3.0 + rating.filter(|n| *n >= 3.5).unwrap_or(0.0) * 0.5
        };
        signal.liked = signal.weight > 0.0;
    }
    let history = signal_rows(&auth, &actor.user.id, &pool, true).await?;
    let cutoff = media_contracts::protocol::unix_now() - i64::from(options.history_days) * 86400;
    for row in history {
        let id: String = row.get("work_id");
        let watched: i64 = row.get("watched");
        let updated: i64 = row.get("updated_at");
        let signal = signals.entry(id).or_default();
        signal.watched = watched == 1;
        if watched == 1 && updated >= cutoff && signal.weight == 0.0 {
            signal.weight = 1.0;
        }
    }
    let (mut items, personalized) = rank(&pool, &signals, options.exclude_watched);
    // Recheck live permissions/parental rules before returning explanations or media metadata.
    let mut allowed = Vec::new();
    for mut item in items.drain(..) {
        if crate::feedback::authorized(&auth, &headers, item["id"].as_str().unwrap_or(""), false)
            .await
            .is_ok()
        {
            if let Some(seed) = item["recommendationSeedId"].as_str()
                && crate::feedback::authorized(&auth, &headers, seed, false)
                    .await
                    .is_err()
            {
                item["recommendationReason"] = json!("À découvrir dans votre catalogue");
                item["recommendationBecause"] = Value::Null;
            }
            if let Some(object) = item.as_object_mut() {
                object.remove("recommendationSeedId");
            }
            allowed.push(item);
        }
    }
    if auth.authenticate(&headers).await.is_err() {
        return Err(error(StatusCode::UNAUTHORIZED, "authentication_required"));
    }
    Ok(Json(
        json!({"enabled":true,"items":allowed,"mode":if personalized {"personalized"} else {"discovery"},"inspectedCount":pool.len(),"candidateLimit":POOL_LIMIT,"truncated":catalogue["hasMore"]}),
    ))
}

async fn signal_rows(
    auth: &AuthState,
    user: &str,
    pool: &[Value],
    history: bool,
) -> Result<Vec<sqlx::any::AnyRow>, Error> {
    if pool.is_empty() {
        return Ok(Vec::new());
    }
    let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(if history {
        "SELECT work_id,watched,updated_at FROM playback_progress WHERE user_id="
    } else {
        "SELECT work_id,favorite,rating FROM work_feedback WHERE user_id="
    });
    builder.push_bind(user).push(" AND work_id IN (");
    let mut binds = builder.separated(",");
    for item in pool {
        binds.push_bind(key(item));
    }
    binds.push_unseparated(")");
    let sql = crate::database::normalize_sql(builder.sql().as_str());
    let args = builder
        .build()
        .take_arguments()
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "recommendations_unavailable",
            )
        })?
        .unwrap_or_default();
    sqlx::query_with(sqlx::AssertSqlSafe(sql), args)
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_rank_is_explained_diverse_deduplicated_and_respects_watched() {
        let pool = vec![
            json!({"id":"liked","title":"Liked","genres":["Drama"],"directors":["Director"]}),
            json!({"id":"near","title":"Near","genres":["Drama"],"directors":["Director"]}),
            json!({"id":"near","title":"Copy","genres":["Drama"]}),
            json!({"id":"other","title":"Other","genres":["Comedy"]}),
            json!({"id":"disliked","title":"Disliked","genres":["Drama"]}),
        ];
        let signals = HashMap::from([
            (
                "liked".into(),
                Signal {
                    weight: 3.0,
                    watched: true,
                    liked: true,
                },
            ),
            (
                "disliked".into(),
                Signal {
                    weight: -2.0,
                    watched: false,
                    liked: false,
                },
            ),
        ]);
        let (ranked, personal) = rank(&pool, &signals, true);
        assert!(personal);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0]["id"], "near");
        assert!(
            ranked[0]["recommendationReason"]
                .as_str()
                .unwrap()
                .contains("Liked")
        );
        assert_eq!(ranked[0]["recommendationBecause"]["kind"], "liked");
        assert_eq!(ranked[0]["recommendationBecause"]["title"], "Liked");
        let (ranked, personal) = rank(&pool, &HashMap::new(), true);
        assert!(!personal);
        assert_eq!(ranked.len(), 4);
    }
    #[test]
    fn preferences_are_enabled_by_default_and_history_window_is_bounded() {
        assert!(Preferences::default().enabled);
        for days in [0, 3651] {
            assert!(
                !Preferences {
                    history_days: days,
                    ..Default::default()
                }
                .valid()
            );
        }
        assert!(Preferences::default().valid());
    }
}
