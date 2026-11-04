use super::*;
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) const DEFAULT_COMMUNITY_TMDB_API_KEY: &str = "4219e299c89411838049ab0dab19ebd5";

pub(super) fn resolve_effective_extension_settings(
    extension_id: &str,
    mut settings: Value,
) -> Value {
    if extension_id == "tmdb"
        && settings
            .get("api_key")
            .and_then(Value::as_str)
            .is_none_or(|k| k.trim().is_empty())
    {
        let env_key = std::env::var("MEDIA_TMDB_API_KEY")
            .or_else(|_| std::env::var("TMDB_API_KEY"))
            .ok()
            .filter(|k| !k.trim().is_empty());
        let default_key = env_key.unwrap_or_else(|| DEFAULT_COMMUNITY_TMDB_API_KEY.to_string());
        if let Some(obj) = settings.as_object_mut() {
            obj.insert("api_key".into(), json!(default_key));
        }
    }
    settings
}

pub(super) fn metadata_input_with_settings(input: Value, mut settings: Value) -> Value {
    // Historical forms saved an empty endpoint. Omit it so the provider can use
    // its official default rather than attempting a request against an empty URL.
    if let Some(fields) = settings.as_object_mut() {
        for key in ["base_url", "baseUrl"] {
            if fields
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim().is_empty())
            {
                fields.remove(key);
            }
        }
    }
    // Configuration belongs only to this installed extension. It travels over the
    // private worker pipe and is never included in package files or run history.
    let mut value = input;
    if settings
        .as_object()
        .is_some_and(|fields| !fields.is_empty())
    {
        value["config"] = settings;
    }
    value
}

pub(super) fn music_album_search_input(track: &str, artist: &str, album: &str) -> Option<Value> {
    if artist.trim().is_empty()
        || artist.eq_ignore_ascii_case("unknown artist")
        || album.trim().is_empty()
        || album.eq_ignore_ascii_case("unknown album")
    {
        return None;
    }
    Some(
        json!({"title":album, "artist":artist, "album":album, "trackTitle":track,
        "mediaType":"music", "externalIds":{"artist":artist}}),
    )
}

pub(super) fn lyrics_request_input(
    title: &str,
    artist: &str,
    album: &str,
    duration: Option<f64>,
) -> Value {
    json!({
        "track": title,
        "artist": artist,
        "album": album,
        "durationSecs": duration.filter(|value| value.is_finite() && *value >= 0.0 && *value <= u32::MAX as f64).map(|value| value.round() as u32),
    })
}

// A process-local key prevents offline guessing of low-entropy provider secrets
// from cache keys. Old cache entries naturally miss after restart.
pub(super) fn provider_cache_context(
    payload: &Value,
    config: &Value,
    generation: &str,
    package_sha: &str,
    music_search: bool,
) -> Result<Value, serde_json::Error> {
    static KEY: std::sync::OnceLock<ring::hmac::Key> = std::sync::OnceLock::new();
    let key = KEY.get_or_init(|| {
        ring::hmac::Key::new(ring::hmac::HMAC_SHA256, uuid::Uuid::new_v4().as_bytes())
    });
    let tag = ring::hmac::sign(key, &serde_json::to_vec(config)?);
    Ok(
        json!({"request": payload, "generation": generation, "packageSha256": package_sha,
        "settingsTag": STANDARD.encode(tag.as_ref()), "musicMatchingVersion": if music_search { 2 } else { 0 }}),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Configuration {
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) library_ids: Vec<String>,
    #[serde(default)]
    pub(super) all_libraries: bool,
}

pub(super) async fn configure(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Configuration>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    if input.library_ids.len() > 100 {
        return Err(err(StatusCode::BAD_REQUEST, "grant_limit"));
    }
    for library in &input.library_ids {
        let count: i64 = query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND n.status!='revoked'")
            .bind(library).fetch_one(auth.store.pool()).await.map_err(db)?;
        if count != 1 {
            return Err(err(StatusCode::BAD_REQUEST, "invalid_library"));
        }
    }
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let changed = query("UPDATE extensions SET enabled=?,generation=?,all_libraries=? WHERE id=? AND (?=0 OR EXISTS(SELECT 1 FROM extension_publishers p WHERE p.id=extensions.publisher_id AND p.active=1))")
        .bind(i64::from(input.enabled)).bind(uuid::Uuid::new_v4().to_string()).bind(i64::from(input.all_libraries)).bind(&id).bind(i64::from(input.enabled)).execute(&mut *tx).await.map_err(db)?;
    if changed.rows_affected() == 0 {
        return Err(err(StatusCode::NOT_FOUND, "extension_unavailable"));
    }
    query("DELETE FROM extension_grants WHERE extension_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    for library in &input.library_ids {
        query("INSERT INTO extension_grants(extension_id,library_id) VALUES(?,?) ON CONFLICT DO NOTHING").bind(&id).bind(library).execute(&mut *tx).await.map_err(db)?;
    }
    let action = json!({"action": if input.enabled { "permissions.enabled" } else { "permissions.disabled" }, "libraryIds": input.library_ids, "allLibraries": input.all_libraries}).to_string();
    audit(&mut tx, &actor.user.id, &id, &action).await?;
    if input.enabled {
        crate::automation::emit_extension(&mut tx, &id, now())
            .await
            .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn get_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let exists: Option<i64> = query_scalar("SELECT 1 FROM extensions WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    if exists.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "extension_not_found"));
    }
    let settings_str: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let settings: Value = settings_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));

    Ok(Json(json!({
        "apiVersion": 1,
        "extensionId": id,
        "settings": settings,
    })))
}

pub(super) async fn put_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(settings): Json<Value>,
) -> Result<StatusCode, Error> {
    let actor = admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    let exists: Option<i64> = query_scalar("SELECT 1 FROM extensions WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    if exists.is_none() {
        return Err(err(StatusCode::NOT_FOUND, "extension_not_found"));
    }
    if !settings.is_object() {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_settings"));
    }
    let settings_str = serde_json::to_string(&settings)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_settings"))?;
    if settings_str.len() > 65_536 {
        return Err(err(StatusCode::BAD_REQUEST, "settings_too_large"));
    }
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    query("INSERT INTO extension_settings(extension_id, settings_json) VALUES(?, ?) ON CONFLICT(extension_id) DO UPDATE SET settings_json=excluded.settings_json")
        .bind(&id)
        .bind(&settings_str)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    audit(&mut tx, &actor.user.id, &id, "settings.updated").await?;
    crate::automation::emit_extension(&mut tx, &id, now())
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
