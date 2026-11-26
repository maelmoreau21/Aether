use super::*;
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    routing::{get, post},
};

pub fn router(auth: AuthState) -> Router {
    let main = Router::new()
        .route("/api/v1/requests", get(list).post(create))
        .route("/api/v1/requests/search", get(search))
        .route("/api/v1/requests/discover", get(discover))
        .route("/api/v1/requests/destinations", get(destinations))
        .route("/api/v1/requests/{id}", get(detail))
        .route("/api/v1/requests/{id}/action", post(action))
        .route("/api/v1/admin/acquisition/locations", get(locations))
        .route(
            "/api/v1/admin/acquisition/settings",
            get(settings).put(save_settings),
        )
        .route(
            "/api/v1/admin/acquisition/servers",
            get(servers).post(add_server),
        )
        .route(
            "/api/v1/admin/acquisition/servers/test",
            post(test_connection),
        )
        .route(
            "/api/v1/admin/acquisition/servers/{id}",
            axum::routing::put(update_server).delete(delete_server),
        )
        .route(
            "/api/v1/admin/acquisition/servers/{id}/test",
            post(test_server),
        )
        .route(
            "/api/v1/admin/acquisition/servers/{id}/options",
            get(options),
        )
        .merge(policy::routes())
        .layer(DefaultBodyLimit::max(65536))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth.clone());
    // Posters are images, cacheable by the browser: they live outside the no-store layer.
    main.merge(
        Router::new()
            .route("/api/v1/requests/poster", get(poster))
            .with_state(auth),
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    auto_approve: bool,
    max_pending_per_user: i64,
}
async fn settings(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let (auto, max): (i64, i64) = query_as(
        "SELECT auto_approve,max_pending_per_user FROM acquisition_settings WHERE singleton=1",
    )
    .fetch_one(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(Json(json!({"autoApprove":auto==1,"maxPendingPerUser":max})))
}
async fn save_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<SettingsInput>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if !(1..=1000).contains(&input.max_pending_per_user) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_quota",
        ));
    }
    query(
        "UPDATE acquisition_settings SET auto_approve=?,max_pending_per_user=? WHERE singleton=1",
    )
    .bind(i64::from(input.auto_approve))
    .bind(input.max_pending_per_user)
    .execute(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(Json(
        json!({"autoApprove":input.auto_approve,"maxPendingPerUser":input.max_pending_per_user}),
    ))
}
async fn servers(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let values: Vec<Server> = query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SERVER_COLUMNS} FROM acquisition_servers ORDER BY name,id LIMIT 100"
    )))
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(Json(
        json!({"servers":values.iter().map(Server::public).collect::<Vec<_>>()}),
    ))
}
async fn locations(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, String, String, String, String)> = query_as(
        "SELECT l.id,COALESCE(parent.name,l.name),l.kind,n.id,n.name,l.root \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            LEFT JOIN library_folders f ON f.library_id=l.id \
            LEFT JOIN libraries parent ON parent.id=f.parent_id \
            WHERE l.enabled=1 AND n.status!='revoked' \
            AND (f.library_id IS NULL OR (f.active=1 AND parent.enabled=1)) \
            AND NOT EXISTS(SELECT 1 FROM deleted_libraries d WHERE d.library_id=l.id) \
            ORDER BY COALESCE(parent.name,l.name),l.root,l.id",
    )
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    Ok(Json(
        json!({"locations":rows.into_iter().map(|(id,name,kind,node_id,node_name,root)|json!({"id":id,"name":name,"kind":kind,"nodeId":node_id,"nodeName":node_name,"root":root})).collect::<Vec<_>>()}),
    ))
}
async fn destinations(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", false).await?;
    let values:Vec<Server>=query_as(sqlx::AssertSqlSafe(format!("SELECT {SERVER_COLUMNS} FROM acquisition_servers WHERE enabled=1 ORDER BY name,id LIMIT 100"))).fetch_all(auth.store.pool()).await.map_err(db)?;
    let lib_names: Vec<(String, String)> = query_as(
        "SELECT l.id,COALESCE(parent.name,l.name) \
            FROM libraries l \
            LEFT JOIN library_folders f ON f.library_id=l.id AND f.active=1 \
            LEFT JOIN libraries parent ON parent.id=f.parent_id",
    )
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    let lib_map: std::collections::BTreeMap<String, String> = lib_names.into_iter().collect();
    let mut out = vec![];
    for server in values {
        if authorize(&auth, &actor, &server.library_id).await.is_ok() {
            out.push(json!({
                "id": server.id,
                "name": server.name,
                "kind": server.kind,
                "libraryId": server.library_id,
                "libraryName": lib_map.get(&server.library_id),
                "isDefault": server.is_default == 1,
                "qualityProfileId": server.quality_profile_id,
                "qualityProfiles": server.quality_profiles()
            }));
        }
    }
    Ok(Json(json!({"servers":out})))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServerInput {
    #[serde(default)]
    id: Option<String>,
    name: String,
    kind: String,
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
    library_id: String,
    root_folder: String,
    quality_profile_id: i64,
    #[serde(default)]
    quality_profiles: Vec<QualityProfile>,
    #[serde(default)]
    settings: Option<ServerSettings>,
    #[serde(default)]
    metadata_profile_id: Option<i64>,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}
async fn add_server(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<ServerInput>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    save_server(&auth, None, input).await
}
async fn update_server(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ServerInput>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    save_server(&auth, Some(id), input).await
}
async fn save_server(
    auth: &AuthState,
    id: Option<String>,
    input: ServerInput,
) -> Result<Json<Value>, Error> {
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || !matches!(input.kind.as_str(), "radarr" | "sonarr" | "lidarr")
        || (input.enabled
            && (input.quality_profile_id <= 0
                || input.root_folder.is_empty()
                || (input.kind == "lidarr" && input.metadata_profile_id.is_none())))
        || input.root_folder.len() > 2048
        || input.root_folder.chars().any(char::is_control)
        || input.metadata_profile_id.is_some_and(|v| v <= 0)
        || input.id.as_ref().is_some_and(|v| id.as_ref() != Some(v))
        || input.quality_profiles.len() > 100
        || input.quality_profiles.iter().any(|p| {
            p.id <= 0
                || p.name.trim().is_empty()
                || p.name.len() > 128
                || p.name.chars().any(char::is_control)
        })
    {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_acquisition_server",
        ));
    }
    let mut seen_profiles = std::collections::BTreeSet::new();
    let quality_profiles: Vec<QualityProfile> = input
        .quality_profiles
        .into_iter()
        .filter(|p| seen_profiles.insert(p.id))
        .map(|p| QualityProfile {
            id: p.id,
            name: p.name.trim().to_owned(),
        })
        .collect();
    let quality_profiles_json = serde_json::to_string(&quality_profiles).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_acquisition_server",
        )
    })?;
    let settings = normalize_server_settings(input.settings.unwrap_or_default())?;
    let settings_json = serde_json::to_string(&settings).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_acquisition_server",
        )
    })?;
    let library: Option<String> = query_scalar(
        "SELECT l.kind FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            LEFT JOIN library_folders f ON f.library_id=l.id \
            LEFT JOIN libraries parent ON parent.id=f.parent_id \
            WHERE l.id=? AND l.enabled=1 AND n.status!='revoked' \
            AND (f.library_id IS NULL OR (f.active=1 AND parent.enabled=1))",
    )
    .bind(&input.library_id)
    .fetch_optional(auth.store.pool())
    .await
    .map_err(db)?;
    let expected = match input.kind.as_str() {
        "radarr" => "films",
        "sonarr" => "series",
        _ => "music",
    };
    if library.as_deref() != Some(expected) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "server_library_kind_mismatch",
        ));
    }
    let previous = match id.as_deref() {
        Some(id) => Some(server(&auth.store, id).await?),
        None => None,
    };
    let key = match input.api_key.filter(|s| !s.is_empty()) {
        Some(key) => key,
        None => previous
            .as_ref()
            .filter(|old| {
                old.kind == input.kind
                    && old.base_url.trim_end_matches('/') == input.base_url.trim_end_matches('/')
            })
            .map(|s| s.api_key.clone())
            .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "api_key_required"))?,
    };
    let connection = transport::configure(&input.base_url, &key).await?;
    let id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let generation = uuid::Uuid::new_v4().to_string();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let active:i64=query_scalar("SELECT COUNT(*) FROM media_requests WHERE server_id=? AND status NOT IN ('available','failed','rejected','cancelled','suspended')").bind(&id).fetch_one(&mut *tx).await.map_err(db)?;
    if active > 0
        && previous.as_ref().is_some_and(|old| {
            old.base_url != connection.base_url
                || old.kind != input.kind
                || old.library_id != input.library_id
                || old.root_folder != input.root_folder
        })
    {
        return Err(err(StatusCode::CONFLICT, "server_has_active_requests"));
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM acquisition_servers")
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if previous.is_none() && count >= 100 {
        return Err(err(StatusCode::CONFLICT, "server_limit_reached"));
    }
    if input.is_default {
        query("UPDATE acquisition_servers SET is_default=0 WHERE kind=? AND id!=?")
            .bind(&input.kind)
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    query("INSERT INTO acquisition_servers(id,name,kind,base_url,api_key,allowed_ips_json,library_id,root_folder,quality_profile_id,quality_profiles_json,settings_json,metadata_profile_id,is_default,enabled,generation) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE \
            SET name=excluded.name,kind=excluded.kind,base_url=excluded.base_url,\
            api_key=excluded.api_key,allowed_ips_json=excluded.allowed_ips_json,\
            library_id=excluded.library_id,root_folder=excluded.root_folder,\
            quality_profile_id=excluded.quality_profile_id,\
            quality_profiles_json=excluded.quality_profiles_json,\
            settings_json=excluded.settings_json,\
            metadata_profile_id=excluded.metadata_profile_id,is_default=excluded.is_default,\
            enabled=excluded.enabled,generation=excluded.generation")
        .bind(&id).bind(input.name.trim()).bind(input.kind).bind(connection.base_url).bind(key).bind(json!(connection.allowed_ips).to_string()).bind(input.library_id).bind(input.root_folder).bind(input.quality_profile_id).bind(&quality_profiles_json).bind(&settings_json).bind(input.metadata_profile_id).bind(i64::from(input.is_default)).bind(i64::from(input.enabled)).bind(generation).execute(&mut *tx).await.map_err(db)?;
    crate::database::commit(tx).await.map_err(db)?;
    Ok(Json(
        json!({"server":server(&auth.store,&id).await?.public()}),
    ))
}
async fn delete_server(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM media_requests WHERE server_id=?")
        .bind(&id)
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    if count > 0 {
        return Err(err(
            StatusCode::CONFLICT,
            "server_has_request_history_disable_instead",
        ));
    }
    query("DELETE FROM acquisition_servers WHERE id=?")
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
fn normalize_tag_ids(ids: Vec<i64>) -> Result<Vec<i64>, Error> {
    if ids.len() > 50 || ids.iter().any(|id| *id <= 0) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_acquisition_server",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    Ok(ids.into_iter().filter(|id| seen.insert(*id)).collect())
}

fn normalize_server_settings(mut settings: ServerSettings) -> Result<ServerSettings, Error> {
    settings.external_url = match settings.external_url {
        Some(url) if !url.trim().is_empty() => {
            let trimmed = url.trim();
            let parsed = url::Url::parse(trimmed)
                .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_connection_url"))?;
            if trimmed.len() > 2048
                || trimmed.chars().any(char::is_control)
                || !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
            {
                return Err(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_connection_url",
                ));
            }
            Some(trimmed.to_owned())
        }
        _ => None,
    };
    if !matches!(
        settings.minimum_availability.as_str(),
        "announced" | "inCinemas" | "released" | "preDB"
    ) || !matches!(
        settings.series_type.as_str(),
        "standard" | "daily" | "anime"
    ) || !matches!(settings.monitor_new_items.as_str(), "none" | "all" | "new")
        || settings.language_profile_id.is_some_and(|id| id <= 0)
        || settings.anime_quality_profile_id.is_some_and(|id| id <= 0)
        || settings.anime_language_profile_id.is_some_and(|id| id <= 0)
        || settings.available_tags.len() > 200
        || settings.available_tags.iter().any(|tag| {
            tag.id <= 0
                || tag.label.trim().is_empty()
                || tag.label.len() > 128
                || tag.label.chars().any(char::is_control)
        })
    {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_acquisition_server",
        ));
    }
    settings.anime_root_folder = match settings.anime_root_folder {
        Some(path) if !path.trim().is_empty() => {
            let trimmed = path.trim();
            if trimmed.len() > 2048 || trimmed.chars().any(char::is_control) {
                return Err(err(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "invalid_acquisition_server",
                ));
            }
            Some(trimmed.to_owned())
        }
        _ => None,
    };
    settings.tags = normalize_tag_ids(settings.tags)?;
    settings.anime_tags = normalize_tag_ids(settings.anime_tags)?;
    let mut seen_tags = std::collections::BTreeSet::new();
    settings.available_tags = settings
        .available_tags
        .into_iter()
        .filter(|t| seen_tags.insert(t.id))
        .map(|t| ArrTag {
            id: t.id,
            label: t.label.trim().to_owned(),
        })
        .collect();
    Ok(settings)
}

fn arr_api_prefix(kind: &str) -> &'static str {
    if kind == "lidarr" { "api/v1" } else { "api/v3" }
}

fn normalize_root_folders(raw: &Value) -> Vec<Value> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let path = item["path"].as_str()?;
                    Some(json!({
                        "id": item["id"].as_i64(),
                        "path": path,
                        "accessible": item["accessible"].as_bool().unwrap_or(true),
                        "freeSpace": item["freeSpace"].as_i64()
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_named_profiles(raw: &Value) -> Vec<Value> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item["id"].as_i64()?;
                    let name = item["name"].as_str()?;
                    Some(json!({ "id": id, "name": name }))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_tags(raw: &Value) -> Vec<Value> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item["id"].as_i64()?;
                    let label = item["label"].as_str()?;
                    Some(json!({ "id": id, "label": label }))
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_arr_options(connection: &transport::Connection, kind: &str) -> Result<Value, Error> {
    let prefix = arr_api_prefix(kind);
    let roots_path = format!("{prefix}/rootfolder");
    let profiles_path = format!("{prefix}/qualityprofile");
    let tags_path = format!("{prefix}/tag");
    let metadata_path = format!("{prefix}/metadataprofile");
    let language_path = format!("{prefix}/languageprofile");
    let roots_fut = transport::request(connection, "GET", &roots_path, &[], None);
    let profiles_fut = transport::request(connection, "GET", &profiles_path, &[], None);
    let tags_fut = transport::request(connection, "GET", &tags_path, &[], None);
    let metadata_fut = async {
        if kind == "lidarr" {
            transport::request(connection, "GET", &metadata_path, &[], None).await
        } else {
            Ok(json!([]))
        }
    };
    let language_fut = async {
        if kind == "sonarr" {
            // Sonarr v4 removed /languageprofile; treat absence as empty list.
            transport::request(connection, "GET", &language_path, &[], None)
                .await
                .unwrap_or_else(|_| json!([]))
        } else {
            json!([])
        }
    };
    let (roots, profiles, tags, metadata, languages) = tokio::join!(
        roots_fut,
        profiles_fut,
        tags_fut,
        metadata_fut,
        language_fut
    );
    let roots = roots?;
    let profiles = profiles?;
    let metadata = metadata?;
    let tags = tags.unwrap_or_else(|_| json!([]));
    Ok(json!({
        "rootFolders": normalize_root_folders(&roots),
        "qualityProfiles": normalize_named_profiles(&profiles),
        "metadataProfiles": normalize_named_profiles(&metadata),
        "languageProfiles": normalize_named_profiles(&languages),
        "tags": normalize_tags(&tags),
    }))
}

async fn discover_arr(connection: &transport::Connection, kind: &str) -> Result<Value, Error> {
    let prefix = arr_api_prefix(kind);
    let status = transport::request(
        connection,
        "GET",
        &format!("{prefix}/system/status"),
        &[],
        None,
    )
    .await?;
    let options = fetch_arr_options(connection, kind).await?;
    Ok(json!({
        "ok": true,
        "version": status["version"],
        "appName": status["appName"],
        "instanceName": status["instanceName"],
        "system": {
            "appName": status["appName"],
            "version": status["version"],
            "instanceName": status["instanceName"]
        },
        "options": options
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TestConnectionInput {
    #[serde(default)]
    id: Option<String>,
    kind: String,
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
}

async fn test_connection(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<TestConnectionInput>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if !matches!(input.kind.as_str(), "radarr" | "sonarr" | "lidarr") {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_acquisition_server",
        ));
    }
    let key = match input.api_key.filter(|s| !s.is_empty()) {
        Some(key) => key,
        None => {
            let previous = match input.id.as_deref() {
                Some(id) => Some(server(&auth.store, id).await?),
                None => None,
            };
            previous
                .filter(|old| {
                    old.kind == input.kind
                        && old.base_url.trim_end_matches('/')
                            == input.base_url.trim_end_matches('/')
                })
                .map(|s| s.api_key)
                .ok_or_else(|| err(StatusCode::UNPROCESSABLE_ENTITY, "api_key_required"))?
        }
    };
    let connection = transport::configure(&input.base_url, &key).await?;
    let payload = discover_arr(&connection, &input.kind).await?;
    Ok(Json(payload))
}

async fn sync_server_metadata(
    store: &IdentityStore,
    server: &Server,
    discovered: &Value,
) -> Result<(), Error> {
    let mut settings = server.settings();
    if let Ok(status) = serde_json::from_value::<ServerSystemStatus>(discovered["system"].clone()) {
        settings.last_status = Some(status);
    }
    if let Ok(tags) = serde_json::from_value::<Vec<ArrTag>>(discovered["options"]["tags"].clone()) {
        settings.available_tags = tags;
    }
    let live_profiles: Vec<QualityProfile> =
        serde_json::from_value(discovered["options"]["qualityProfiles"].clone())
            .unwrap_or_default();
    let existing = server.quality_profiles();
    let profiles: Vec<QualityProfile> = if existing.is_empty() {
        live_profiles
    } else {
        let live_names: std::collections::BTreeMap<i64, String> = live_profiles
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect();
        let mut updated: Vec<QualityProfile> = existing
            .into_iter()
            .map(|p| QualityProfile {
                id: p.id,
                name: live_names.get(&p.id).cloned().unwrap_or(p.name),
            })
            .collect();
        if !updated.iter().any(|p| p.id == server.quality_profile_id)
            && let Some(name) = live_names.get(&server.quality_profile_id)
        {
            updated.insert(
                0,
                QualityProfile {
                    id: server.quality_profile_id,
                    name: name.clone(),
                },
            );
        }
        updated
    };
    let settings_json = serde_json::to_string(&settings).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_acquisition_server",
        )
    })?;
    let profiles_json = serde_json::to_string(&profiles).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_acquisition_server",
        )
    })?;
    query("UPDATE acquisition_servers SET settings_json=?,quality_profiles_json=? WHERE id=?")
        .bind(settings_json)
        .bind(profiles_json)
        .bind(&server.id)
        .execute(store.pool())
        .await
        .map_err(db)?;
    Ok(())
}

async fn test_server(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    let server = server(&auth.store, &id).await?;
    let connection = server.connection()?;
    let payload = discover_arr(&connection, &server.kind).await?;
    sync_server_metadata(&auth.store, &server, &payload).await?;
    Ok(Json(payload))
}
async fn options(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let server = server(&auth.store, &id).await?;
    let connection = server.connection()?;
    let payload = fetch_arr_options(&connection, &server.kind).await?;
    Ok(Json(payload))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Search {
    q: String,
    kind: String,
    server_id: Option<String>,
}
async fn choose(
    auth: &AuthState,
    actor: &Authenticated,
    kind: &str,
    id: Option<&str>,
) -> Result<Server, Error> {
    let service = match kind {
        "film" => "radarr",
        "series" => "sonarr",
        "artist" | "album" | "track" => "lidarr",
        _ => {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_request_kind",
            ));
        }
    };
    if let Some(id) = id {
        let server = server(&auth.store, id).await?;
        if server.enabled != 1 || server.kind != service {
            return Err(err(StatusCode::CONFLICT, "acquisition_server_incompatible"));
        }
        authorize(auth, actor, &server.library_id).await?;
        return Ok(server);
    }
    let sql = format!(
        "SELECT {SERVER_COLUMNS} FROM acquisition_servers WHERE kind=? AND enabled=1 ORDER BY is_default DESC,name,id LIMIT 100"
    );
    let candidates: Vec<Server> = query_as(sqlx::AssertSqlSafe(sql))
        .bind(service)
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
    if candidates.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "acquisition_server_not_configured",
        ));
    }
    for server in candidates {
        if authorize(auth, actor, &server.library_id).await.is_ok() {
            return Ok(server);
        }
    }
    Err(err(StatusCode::FORBIDDEN, "library_permission_denied"))
}
async fn search(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(input): Query<Search>,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", false).await?;
    unrestricted(&auth.store, &actor.user.id).await?;
    if input.q.trim().len() < 2 || input.q.len() > 200 {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_search_query",
        ));
    }
    let effective = policy::effective(
        &auth.store,
        &actor.user.id,
        actor.user.role == Role::Administrator,
    )
    .await?;
    let grant = effective.grant(&input.kind);
    if !grant.allow {
        return Err(err(StatusCode::FORBIDDEN, "request_permission_denied"));
    }
    let server = choose(&auth, &actor, &input.kind, input.server_id.as_deref()).await?;
    let mut results = remote::search(&server, &input.kind, input.q.trim()).await?;
    annotate(&auth, &actor, &input.kind, &mut results).await?;
    for result in &mut results {
        result["canRequest"] = json!(true);
        result["autoApprove"] = json!(grant.auto_approve);
    }
    Ok(Json(json!({"results":results})))
}
/// Codes that only mean "this person cannot request this media class here": the combined
/// search stays quiet about them instead of showing an error beside the library results.
const QUIET: [&str; 4] = [
    "acquisition_server_not_configured",
    "library_permission_denied",
    "external_metadata_parental_unverified",
    "request_permission_denied",
];
async fn tmdb_enabled(pool: &sqlx::AnyPool) -> bool {
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions WHERE id='tmdb' AND enabled=1")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    count > 0
}

async fn resolve_tmdb_api_key(pool: &sqlx::AnyPool) -> String {
    let stored: Option<String> =
        query_scalar("SELECT settings_json FROM extension_settings WHERE extension_id='tmdb'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some(json) = stored
        && let Ok(val) = serde_json::from_str::<Value>(&json)
        && let Some(key) = val
            .get("api_key")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
    {
        return key.trim().to_string();
    }
    std::env::var("MEDIA_TMDB_API_KEY")
        .or_else(|_| std::env::var("TMDB_API_KEY"))
        .ok()
        .filter(|k| !k.trim().is_empty())
        .unwrap_or_else(|| crate::extensions::DEFAULT_COMMUNITY_TMDB_API_KEY.to_string())
}

async fn discover_kind(
    auth: &AuthState,
    actor: &Authenticated,
    effective: &policy::Effective,
    kind: &str,
    q: &str,
) -> Result<Vec<Value>, String> {
    let grant = effective.grant(kind);
    if !grant.allow {
        return Ok(vec![]);
    }
    let found = async {
        // Without a destination the person can read, the reason is kept: it is
        // reported (or kept quiet) exactly as before the public search existed.
        let (server, unavailable) = match choose(auth, actor, kind, None).await {
            Ok(server) => (Some(server), None),
            Err(error) => (None, Some(error)),
        };
        let use_tmdb = matches!(kind, "film" | "series") && tmdb_enabled(auth.store.pool()).await;

        let mut results = if use_tmdb {
            let api_key = resolve_tmdb_api_key(auth.store.pool()).await;
            match transport::public_tmdb_search(kind, q, &api_key).await {
                Ok(res) => res,
                Err(error) => match &server {
                    Some(srv) => remote::search(srv, kind, q).await?,
                    None => return Err(error),
                },
            }
        } else {
            match &server {
                Some(srv) => remote::search(srv, kind, q).await?,
                None => {
                    return Err(unavailable
                        .unwrap_or_else(|| err(StatusCode::NOT_FOUND, "no_acquisition_server")));
                }
            }
        };

        results.truncate(12);
        if let Some(srv) = &server {
            for item in &mut results {
                if item.get("serverId").is_none() {
                    item["serverId"] = json!(srv.id);
                }
            }
        }
        annotate(auth, actor, kind, &mut results).await?;
        Ok::<_, Error>(results)
    }
    .await;
    match found {
        Ok(mut results) => {
            for result in &mut results {
                result["canRequest"] = json!(true);
                result["autoApprove"] = json!(grant.auto_approve);
            }
            Ok(results)
        }
        Err((_, Json(body))) => {
            let code = body["error"].as_str().unwrap_or("search_failed");
            if QUIET.contains(&code) {
                Ok(vec![])
            } else {
                Err(code.to_owned())
            }
        }
    }
}
#[derive(Deserialize)]
struct Discover {
    q: String,
}
/// One search for the whole interface: films and series known to the configured destinations,
/// each marked with what the caller may do (request now, wait for approval) and with what
/// already exists. Titles already in the library are returned too: the library search shows
/// them, this lists what is still missing next to it.
async fn discover(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(input): Query<Discover>,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", false).await?;
    let q = input.q.trim();
    if q.chars().count() < 2 || input.q.len() > 200 {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_search_query",
        ));
    }
    if unrestricted(&auth.store, &actor.user.id).await.is_err() {
        return Ok(Json(json!({"results":[],"errors":[],"restricted":true})));
    }
    let administrator = actor.user.role == Role::Administrator;
    let effective = policy::effective(&auth.store, &actor.user.id, administrator).await?;
    let (films, series) = tokio::join!(
        discover_kind(&auth, &actor, &effective, "film", q),
        discover_kind(&auth, &actor, &effective, "series", q)
    );
    let mut results = vec![];
    let mut errors = vec![];
    for found in [films, series] {
        match found {
            Ok(mut values) => results.append(&mut values),
            Err(code) => errors.push(json!({"error":code})),
        }
    }
    Ok(Json(
        json!({"results":results,"errors":errors,"restricted":false}),
    ))
}
#[derive(Deserialize)]
struct PosterQuery {
    src: String,
    path: String,
}
#[derive(Clone)]
struct CachedPoster {
    bytes: axum::body::Bytes,
    content_type: &'static str,
    etag: String,
}

#[derive(Default)]
struct PosterCache {
    entries: std::collections::HashMap<(String, String), (u64, CachedPoster)>,
    order: std::collections::VecDeque<((String, String), u64)>,
    seq: u64,
    bytes: usize,
}

impl PosterCache {
    const MAX_ENTRIES: usize = 256;
    const MAX_BYTES: usize = 16 * 1024 * 1024;

    fn get(&mut self, key: &(String, String)) -> Option<CachedPoster> {
        let entry = self.entries.get(key)?.1.clone();
        self.seq = self.seq.wrapping_add(1).max(1);
        let seq = self.seq;
        if let Some(slot) = self.entries.get_mut(key) {
            slot.0 = seq;
        }
        self.order.push_back((key.clone(), seq));
        Some(entry)
    }

    fn insert(&mut self, key: (String, String), value: CachedPoster) {
        let size = value.bytes.len();
        if size > Self::MAX_BYTES {
            return;
        }
        if let Some((_, old)) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.bytes.len());
        }
        while self.bytes.saturating_add(size) > Self::MAX_BYTES
            || self.entries.len() >= Self::MAX_ENTRIES
        {
            let evicted = loop {
                let Some((cand, cand_seq)) = self.order.pop_front() else {
                    break false;
                };
                if self.entries.get(&cand).is_some_and(|e| e.0 == cand_seq) {
                    if let Some((_, old)) = self.entries.remove(&cand) {
                        self.bytes = self.bytes.saturating_sub(old.bytes.len());
                    }
                    break true;
                }
            };
            if !evicted {
                if let Some(k) = self.entries.keys().next().cloned() {
                    if let Some((_, old)) = self.entries.remove(&k) {
                        self.bytes = self.bytes.saturating_sub(old.bytes.len());
                    }
                } else {
                    break;
                }
            }
        }
        self.seq = self.seq.wrapping_add(1).max(1);
        let seq = self.seq;
        self.bytes = self.bytes.saturating_add(size);
        self.order.push_back((key.clone(), seq));
        self.entries.insert(key, (seq, value));
    }
}

static POSTER_CACHE: std::sync::LazyLock<std::sync::Mutex<PosterCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(PosterCache::default()));

/// Artwork for titles that are not in the library yet. Only fixed providers and plain image
/// paths are served; the browser never contacts them directly.
async fn poster(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(input): Query<PosterQuery>,
) -> Result<axum::response::Response, Error> {
    use axum::{http::header, response::IntoResponse};
    crate::applications::require(&auth, &headers, "requests", false).await?;
    let (host, prefix) = match input.src.as_str() {
        "tmdb" => ("image.tmdb.org", "/t/p/w342"),
        "tvdb" => ("artworks.thetvdb.com", "/banners"),
        _ => return Err(err(StatusCode::NOT_FOUND, "not_found")),
    };
    if !remote::valid_poster_path(&input.path) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_poster"));
    }
    let full_path = if input.src == "tvdb" && input.path.starts_with("/banners/") {
        input.path.clone()
    } else {
        format!("{prefix}{}", input.path)
    };
    let cache_key = (host.to_owned(), full_path.clone());
    let cached = POSTER_CACHE.lock().ok().and_then(|mut c| c.get(&cache_key));
    let entry = match cached {
        Some(hit) => hit,
        None => {
            let (bytes, content_type) = transport::public_image(host, &full_path).await?;
            let etag = format!("\"{}\"", media_contracts::protocol::sha256_hex(&bytes));
            let entry = CachedPoster {
                bytes: axum::body::Bytes::from(bytes),
                content_type,
                etag,
            };
            if let Ok(mut c) = POSTER_CACHE.lock() {
                c.insert(cache_key, entry.clone());
            }
            entry
        }
    };
    let matches = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .map(|part| part.trim().trim_start_matches("W/"))
                .any(|part| part == entry.etag || part == "*")
        });
    if matches {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [
                (header::ETAG, entry.etag.as_str()),
                (header::CACHE_CONTROL, "private, max-age=86400"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
        )
            .into_response());
    }
    Ok((
        [
            (header::CONTENT_TYPE, entry.content_type),
            (header::ETAG, entry.etag.as_str()),
            (header::CACHE_CONTROL, "private, max-age=86400"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        entry.bytes,
    )
        .into_response())
}
/// Marks results already present in a readable Aether library or already requested,
/// so nobody requests twice what exists. Another account's request only shows its state.
async fn annotate(
    auth: &AuthState,
    actor: &Authenticated,
    kind: &str,
    results: &mut [Value],
) -> Result<(), Error> {
    let ids: Vec<String> = results
        .iter()
        .filter_map(|r| r["externalId"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT external_id,id,status,user_id,server_id FROM media_requests WHERE kind=? AND external_id IN ({marks}) AND status NOT IN ('rejected','cancelled','failed') ORDER BY created_at DESC,id"
    );
    let mut statement = query_as::<_, (String, String, String, Option<String>, Option<String>)>(
        sqlx::AssertSqlSafe(sql),
    )
    .bind(kind);
    for id in &ids {
        statement = statement.bind(id);
    }
    let requests = statement.fetch_all(auth.store.pool()).await.map_err(db)?;
    let works: Vec<(String, String, String)> = match kind {
        "film" => {
            let sql = format!(
                "SELECT CAST(json_extract(external_ids_json,'$.tmdb') AS TEXT),id,library_id FROM inventory_works WHERE kind='film' AND CAST(json_extract(external_ids_json,'$.tmdb') AS TEXT) IN ({marks}) ORDER BY id LIMIT 200"
            );
            let mut statement = query_as::<_, (String, String, String)>(sqlx::AssertSqlSafe(sql));
            for id in &ids {
                statement = statement.bind(id);
            }
            statement.fetch_all(auth.store.pool()).await.map_err(db)?
        }
        "series" => {
            let sql = format!(
                "SELECT CAST(json_extract(external_ids_json,'$.tmdb') AS TEXT),id,library_id FROM inventory_works WHERE kind='series' AND CAST(json_extract(external_ids_json,'$.tmdb') AS TEXT) IN ({marks}) \
                 UNION ALL \
                 SELECT CAST(json_extract(external_ids_json,'$.tvdb') AS TEXT),id,library_id FROM inventory_works WHERE kind='series' AND CAST(json_extract(external_ids_json,'$.tvdb') AS TEXT) IN ({marks}) \
                 LIMIT 200"
            );
            let mut statement = query_as::<_, (String, String, String)>(sqlx::AssertSqlSafe(sql));
            for id in &ids {
                statement = statement.bind(id);
            }
            for id in &ids {
                statement = statement.bind(id);
            }
            statement.fetch_all(auth.store.pool()).await.map_err(db)?
        }
        _ => vec![],
    };
    let administrator = actor.user.role == Role::Administrator;
    let mut readable = std::collections::BTreeMap::new();
    for (external, work, library) in works {
        if readable.contains_key(&external) {
            continue;
        }
        if authorize(auth, actor, &library).await.is_ok() {
            readable.insert(external, work);
        }
    }
    for result in results.iter_mut() {
        let Some(external) = result["externalId"].as_str().map(str::to_owned) else {
            continue;
        };
        if let Some(work) = readable.get(&external) {
            result["inLibrary"] = json!({"workId":work});
        }
        let target_server = result["serverId"].as_str();
        let matched = target_server
            .and_then(|sid| {
                requests
                    .iter()
                    .find(|r| r.0 == external && r.4.as_deref() == Some(sid))
            })
            .or_else(|| requests.iter().find(|r| r.0 == external));
        if let Some((_, id, status, owner, server_id)) = matched {
            let mine = owner.as_deref() == Some(actor.user.id.as_str());
            result["request"] = json!({
                "status": status,
                "mine": mine,
                "serverId": server_id,
                "id": (mine || administrator).then_some(id)
            });
        }
    }
    Ok(())
}
fn same_scope(a: &RequestInput, b: &RequestInput) -> bool {
    if a.kind != b.kind || a.external_id != b.external_id {
        return false;
    }
    match a.kind.as_str() {
        "series" => {
            let mut sa = a.seasons.clone();
            let mut sb = b.seasons.clone();
            sa.sort_unstable();
            sb.sort_unstable();
            let mut ea: Vec<(i64, i64)> =
                a.episodes.iter().map(|e| (e.season, e.episode)).collect();
            let mut eb: Vec<(i64, i64)> =
                b.episodes.iter().map(|e| (e.season, e.episode)).collect();
            ea.sort_unstable();
            eb.sort_unstable();
            sa == sb && ea == eb && a.monitor_future == b.monitor_future
        }
        "artist" => {
            let mut aa = a.album_ids.clone();
            let mut ab = b.album_ids.clone();
            aa.sort();
            ab.sort();
            aa == ab && a.monitor_future == b.monitor_future
        }
        _ => true,
    }
}

async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(mut input): Json<RequestInput>,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", true).await?;
    unrestricted(&auth.store, &actor.user.id).await?;
    if !input.valid()
        || (input.kind == "artist" && input.album_ids.is_empty() && !input.monitor_future)
    {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_selection",
        ));
    }
    let administrator = actor.user.role == Role::Administrator;
    let effective = policy::effective(&auth.store, &actor.user.id, administrator).await?;
    let grant = effective.grant(&input.kind);
    if !grant.allow {
        return Err(err(StatusCode::FORBIDDEN, "request_permission_denied"));
    }
    let server = choose(&auth, &actor, &input.kind, input.server_id.as_deref()).await?;
    if !administrator && !grant.advanced {
        let default_server = choose(&auth, &actor, &input.kind, None).await?;
        if server.id != default_server.id {
            return Err(err(
                StatusCode::FORBIDDEN,
                "request_advanced_permission_denied",
            ));
        }
        if let Some(profile_id) = input.quality_profile_id
            && profile_id != server.quality_profile_id
        {
            return Err(err(
                StatusCode::FORBIDDEN,
                "request_advanced_permission_denied",
            ));
        }
    }
    if let Some(profile_id) = input.quality_profile_id {
        let allowed = profile_id == server.quality_profile_id
            || server.quality_profiles().iter().any(|p| p.id == profile_id);
        if !allowed {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_quality_profile",
            ));
        }
    }
    let validation =
        crate::applications::invoke(&auth.store, "requests", "requests.validate", json!(input))
            .await?;
    if validation["valid"] != true {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            validation["error"].as_str().unwrap_or("invalid_request"),
        ));
    }
    let canonical = remote::lookup(&server, &input).await?;
    input.title = remote::title(&canonical, &input.kind)
        .unwrap_or(&input.title)
        .chars()
        .take(512)
        .collect();
    input.server_id = Some(server.id.clone());
    let id = insert_request(&auth, &actor.user.id, &effective, &server, &input).await?;
    Ok(Json(json!(record(&auth.store, &id).await?)))
}
/// Admission of one validated request: duplicate, open-request limit and window quota are
/// checked inside the write transaction, then the author's rights decide whether it starts
/// approved (an administrator, or a rule that approves automatically) or waits for review.
pub(super) async fn insert_request(
    auth: &AuthState,
    user_id: &str,
    effective: &policy::Effective,
    server: &Server,
    input: &RequestInput,
) -> Result<String, Error> {
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let existing_inputs: Vec<String> = query_scalar("SELECT input_json FROM media_requests WHERE server_id=? AND kind=? AND external_id=? AND status NOT IN ('rejected','cancelled','failed') LIMIT 50")
        .bind(&server.id).bind(&input.kind).bind(&input.external_id).fetch_all(&mut *tx).await.map_err(db)?;
    let is_duplicate = existing_inputs.iter().any(|raw| {
        serde_json::from_str::<RequestInput>(raw)
            .map(|other| same_scope(input, &other))
            .unwrap_or(true)
    });
    if is_duplicate {
        return Err(err(StatusCode::CONFLICT, "request_already_exists"));
    }
    let pending: i64 = query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM media_requests WHERE user_id=? AND status IN ({})",
        policy::OPEN_STATUSES
    )))
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if pending >= effective.max_pending {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "request_quota_exceeded"));
    }
    policy::enforce_quota(&mut tx, effective, &input.kind).await?;
    let (status, code) = policy::admission(effective, &input.kind);
    let id = uuid::Uuid::new_v4().to_string();
    let timestamp = now();
    query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(user_id).bind(&server.id).bind(&server.library_id).bind(&input.kind).bind(&input.title).bind(&input.external_id).bind(&input.external_provider).bind(json!(input).to_string()).bind(status).bind(timestamp).bind(timestamp).execute(&mut *tx).await.map_err(db)?;
    event(&mut tx, &id, Some(user_id), status, code).await?;
    if status == "approved" {
        query("INSERT INTO acquisition_jobs(request_id,available_at) VALUES(?,?)")
            .bind(&id)
            .bind(timestamp)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    crate::database::commit(tx).await.map_err(db)?;
    Ok(id)
}
#[derive(Deserialize, Default)]
struct Page {
    /// Opaque cursor returned as `nextCursor` (`created_at:id`).
    before: Option<String>,
    limit: Option<i64>,
    /// One status, or a group: `active`, `attention` (failed/suspended), `done`, `closed`.
    status: Option<String>,
    kind: Option<String>,
    /// Lets an administrator see only their own requests.
    #[serde(default)]
    mine: bool,
}
const STATUSES: [&str; 11] = [
    "pending",
    "approved",
    "submitted",
    "acquiring",
    "imported",
    "partial",
    "available",
    "rejected",
    "cancelled",
    "failed",
    "suspended",
];
fn status_filter(value: Option<&str>) -> Result<Vec<&'static str>, Error> {
    Ok(match value {
        None | Some("all") => STATUSES.to_vec(),
        Some("active") => vec![
            "pending",
            "approved",
            "submitted",
            "acquiring",
            "imported",
            "partial",
        ],
        Some("attention") => vec!["failed", "suspended"],
        Some("done") => vec!["available"],
        Some("closed") => vec!["rejected", "cancelled"],
        Some(status) => vec![
            STATUSES
                .into_iter()
                .find(|known| *known == status)
                .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_page"))?,
        ],
    })
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", false).await?;
    let statuses = status_filter(page.status.as_deref())?;
    if page
        .kind
        .as_deref()
        .is_some_and(|k| !matches!(k, "film" | "series" | "artist" | "album" | "track"))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_page"));
    }
    // Requests are listed newest first; ids are random, so the cursor carries the date.
    let (before_at, before_id) = match page.before.as_deref() {
        None => (i64::MAX, String::new()),
        Some(cursor) => cursor
            .split_once(':')
            .and_then(|(at, id)| Some((at.parse::<i64>().ok()?, id.to_owned())))
            .filter(|(_, id)| id.len() <= 64)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_cursor"))?,
    };
    let marks = vec!["?"; statuses.len()].join(",");
    let scope = "(?='administrator' OR user_id=?) AND (?=0 OR user_id=?)";
    let sql = format!(
        "SELECT {REQUEST_COLUMNS} FROM media_requests WHERE {scope} AND status IN ({marks}) AND (? IS NULL OR kind=?) AND (created_at<? OR (created_at=? AND id<?)) ORDER BY created_at DESC,id DESC LIMIT ?"
    );
    let mine = i64::from(page.mine);
    let mut statement = query_as::<_, RequestRecord>(sqlx::AssertSqlSafe(sql))
        .bind(actor.user.role.as_str())
        .bind(&actor.user.id)
        .bind(mine)
        .bind(&actor.user.id);
    for status in &statuses {
        statement = statement.bind(*status);
    }
    let limit = page.limit.unwrap_or(100).clamp(1, 200);
    let rows: Vec<RequestRecord> = statement
        .bind(&page.kind)
        .bind(&page.kind)
        .bind(before_at)
        .bind(before_at)
        .bind(&before_id)
        .bind(limit)
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
    let cursor = (rows.len() as i64 == limit)
        .then(|| rows.last().map(|r| format!("{}:{}", r.created_at, r.id)))
        .flatten();
    let labels = crate::applications::user_labels(
        &auth.store,
        rows.iter().filter_map(|r| r.user_id.as_deref()),
    )
    .await?;
    let mut allowed_libraries = std::collections::BTreeMap::<String, bool>::new();
    let mut visible = vec![];
    for row in rows {
        let can_see = match &row.library_id {
            Some(library) => {
                if let Some(&cached) = allowed_libraries.get(library) {
                    cached
                } else {
                    let ok = authorize(&auth, &actor, library).await.is_ok();
                    allowed_libraries.insert(library.clone(), ok);
                    ok
                }
            }
            None => row.imported == 1,
        };
        if can_see {
            visible.push(present(&row, &actor, &labels));
        }
    }
    let grouped: Vec<(String, Option<String>, i64, i64)> = query_as(sqlx::AssertSqlSafe(format!(
        "SELECT status,library_id,imported,COUNT(*) FROM media_requests WHERE {scope} GROUP BY status,library_id,imported"
    )))
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .bind(mine)
    .bind(&actor.user.id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    let mut counts_map = std::collections::BTreeMap::<String, i64>::new();
    for (status, library_id, imported, count) in grouped {
        let can_see = match library_id {
            Some(library) => {
                if let Some(&cached) = allowed_libraries.get(&library) {
                    cached
                } else {
                    let ok = authorize(&auth, &actor, &library).await.is_ok();
                    allowed_libraries.insert(library, ok);
                    ok
                }
            }
            None => imported == 1,
        };
        if can_see {
            *counts_map.entry(status).or_default() += count;
        }
    }
    let counts: serde_json::Map<String, Value> = counts_map
        .into_iter()
        .map(|(status, count)| (status, json!(count)))
        .collect();
    Ok(Json(
        json!({"requests":visible,"nextCursor":cursor,"counts":counts}),
    ))
}
/// A record as listed: the requester's name, and whether it belongs to the caller.
fn present(
    row: &RequestRecord,
    actor: &Authenticated,
    labels: &std::collections::BTreeMap<String, String>,
) -> Value {
    let mut value = json!(row);
    if row.author_label.is_none()
        && let Some(label) = row.user_id.as_ref().and_then(|id| labels.get(id))
    {
        value["authorLabel"] = json!(label);
    }
    value["own"] = json!(row.user_id.as_deref() == Some(actor.user.id.as_str()));
    if let Ok(input) = serde_json::from_str::<Value>(&row.input_json) {
        value["selection"] = json!({"seasons":input["seasons"],"episodes":input["episodes"],
            "albumIds":input["albumIds"],"monitorFuture":input["monitorFuture"],
            "qualityProfileId":input["qualityProfileId"]});
    }
    if let Some(raw) = &row.progress_json
        && let Ok(progress) = serde_json::from_str::<Value>(raw)
    {
        value["progress"] = progress;
    }
    value
}
async fn own(auth: &AuthState, actor: &Authenticated, id: &str) -> Result<RequestRecord, Error> {
    let row = record(&auth.store, id).await?;
    if actor.user.role != Role::Administrator
        && row.user_id.as_deref() != Some(actor.user.id.as_str())
    {
        return Err(err(StatusCode::NOT_FOUND, "request_not_found"));
    }
    if let Some(library) = &row.library_id {
        authorize(auth, actor, library).await?;
    } else if row.imported != 1 {
        return Err(err(StatusCode::FORBIDDEN, "request_library_required"));
    }
    Ok(row)
}
/// id, actor_id, status, code, created_at
type EventRow = (String, Option<String>, String, Option<String>, i64);

async fn detail(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", false).await?;
    let row = own(&auth, &actor, &id).await?;
    let events: Vec<EventRow> =query_as("SELECT id,actor_id,status,code,created_at FROM acquisition_events WHERE request_id=? ORDER BY created_at,id LIMIT 500").bind(&id).fetch_all(auth.store.pool()).await.map_err(db)?;
    let input: Value = serde_json::from_str(&row.input_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_request_state"))?;
    let labels = crate::applications::user_labels(
        &auth.store,
        row.user_id
            .as_deref()
            .into_iter()
            .chain(events.iter().filter_map(|e| e.1.as_deref())),
    )
    .await?;
    let mut value = present(&row, &actor, &labels);
    value["selection"] = input;
    value["events"]=json!(events.iter().map(|(id,actor,status,code,created)|json!({"id":id,"actorLabel":actor.as_ref().and_then(|a|labels.get(a)),"status":status,"code":code,"createdAt":created})).collect::<Vec<_>>());
    Ok(Json(value))
}
/// Fixed reasons an administrator gives when refusing; shown to the requester.
const REJECT_REASONS: [&str; 6] = [
    "already_available",
    "not_available",
    "duplicate",
    "out_of_scope",
    "storage",
    "other",
];
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Action {
    action: String,
    server_id: Option<String>,
    #[serde(default)]
    quality_profile_id: Option<i64>,
    #[serde(default)]
    reason: Option<String>,
}
async fn action(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Action>,
) -> Result<Json<Value>, Error> {
    let actor = crate::applications::require(&auth, &headers, "requests", true).await?;
    let row = own(&auth, &actor, &id).await?;
    let administrator = actor.user.role == Role::Administrator;
    let status = match input.action.as_str() {
        "cancel" if matches!(row.status.as_str(), "pending" | "approved") => "cancelled",
        "approve" if administrator && row.status == "pending" => "approved",
        "reject" if administrator && row.status == "pending" => "rejected",
        "retry" if administrator && matches!(row.status.as_str(), "failed" | "suspended") => {
            "approved"
        }
        _ => return Err(err(StatusCode::CONFLICT, "request_action_not_allowed")),
    };
    let reason = match input.reason.as_deref() {
        None => None,
        Some(reason) if status == "rejected" && REJECT_REASONS.contains(&reason) => Some(reason),
        Some(_) => {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_reject_reason",
            ));
        }
    };
    let mut selection: RequestInput = serde_json::from_str(&row.input_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_request_state"))?;
    if let Some(server_id) = input.server_id.as_deref() {
        if !administrator {
            return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
        }
        if row.remote_id.is_some() {
            return Err(err(StatusCode::CONFLICT, "request_already_dispatched"));
        }
        let destination = choose(&auth, &actor, &selection.kind, Some(server_id)).await?;
        selection.server_id = Some(destination.id);
    }
    let server = choose(
        &auth,
        &actor,
        &selection.kind,
        selection.server_id.as_deref(),
    )
    .await?;
    if let Some(profile_id) = input.quality_profile_id {
        if !administrator {
            return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
        }
        if row.remote_id.is_some() {
            return Err(err(StatusCode::CONFLICT, "request_already_dispatched"));
        }
        let allowed = profile_id == server.quality_profile_id
            || server.quality_profiles().iter().any(|p| p.id == profile_id);
        if !allowed {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_quality_profile",
            ));
        }
        selection.quality_profile_id = Some(profile_id);
    }
    if status == "approved" {
        let owner = row
            .user_id
            .as_deref()
            .ok_or_else(|| err(StatusCode::CONFLICT, "historical_author_mapping_required"))?;
        unrestricted(&auth.store, owner).await?;
        crate::applications::authorize_background(
            &auth.store,
            "requests",
            &server.library_id,
            owner,
        )
        .await?;
    }
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let changed=query("UPDATE media_requests \
            SET status=?,error_code=?,server_id=?,library_id=?,input_json=?,updated_at=?,imported=CASE WHEN ?='approved' THEN 0 ELSE imported END \
            WHERE id=? AND status=?").bind(status).bind(reason).bind(&server.id).bind(&server.library_id).bind(json!(selection).to_string()).bind(now()).bind(status).bind(&id).bind(&row.status).execute(&mut *tx).await.map_err(db)?.rows_affected();
    if changed != 1 {
        return Err(err(StatusCode::CONFLICT, "request_changed"));
    }
    event(&mut tx, &id, Some(&actor.user.id), status, reason).await?;
    if status == "approved" {
        query("INSERT INTO acquisition_jobs(request_id,available_at) VALUES(?,?) ON CONFLICT(request_id) DO UPDATE SET available_at=excluded.available_at,lease_token=NULL,lease_until=0,attempts=0,phase='initial'").bind(&id).bind(now()).execute(&mut *tx).await.map_err(db)?;
    } else {
        query("DELETE FROM acquisition_jobs WHERE request_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    crate::database::commit(tx).await.map_err(db)?;
    Ok(Json(json!(record(&auth.store, &id).await?)))
}
