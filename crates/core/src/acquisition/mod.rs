//! Optional requests application: administrator-owned connections and durable acquisitions.
mod http;
mod remote;
mod singles;
pub mod transport;
mod worker;

use crate::{
    applications::{Error, db, err},
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use http::router;
pub use worker::tick;
pub const LOCK: i64 = 0x41435155495245;
pub fn now() -> i64 {
    media_contracts::protocol::unix_now()
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Episode {
    pub season: i64,
    pub episode: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestInput {
    pub kind: String,
    pub title: String,
    pub external_id: String,
    pub external_provider: String,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub seasons: Vec<i64>,
    #[serde(default)]
    pub episodes: Vec<Episode>,
    #[serde(default)]
    pub album_ids: Vec<String>,
    #[serde(default)]
    pub monitor_future: bool,
}
impl RequestInput {
    fn valid(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "film" | "series" | "artist" | "album" | "track"
        ) && !self.title.trim().is_empty()
            && self.title.len() <= 512
            && !self.external_id.is_empty()
            && self.external_id.len() <= 128
            && !self.external_id.chars().any(char::is_control)
            && matches!(
                self.external_provider.as_str(),
                "tmdb" | "tvdb" | "musicbrainz"
            )
            && self.seasons.len() <= 200
            && self.seasons.iter().all(|s| *s >= 0 && *s <= 10000)
            && self
                .seasons
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.seasons.len()
            && self.episodes.len() <= 2000
            && self
                .episodes
                .iter()
                .all(|e| e.season >= 0 && e.season <= 10000 && e.episode > 0 && e.episode <= 100000)
            && self
                .episodes
                .iter()
                .map(|e| (e.season, e.episode))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.episodes.len()
            && self.album_ids.len() <= 200
            && self
                .album_ids
                .iter()
                .all(|s| uuid::Uuid::parse_str(s).is_ok())
            && self
                .album_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.album_ids.len()
            && (self.kind == "series" || (self.seasons.is_empty() && self.episodes.is_empty()))
            && (self.kind == "artist" || self.album_ids.is_empty())
    }
    fn service(&self) -> &str {
        match self.kind.as_str() {
            "film" => "radarr",
            "series" => "sonarr",
            _ => "lidarr",
        }
    }
}

#[derive(Clone, sqlx::FromRow)]
pub(crate) struct Server {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    pub allowed_ips_json: String,
    pub library_id: String,
    pub root_folder: String,
    pub quality_profile_id: i64,
    pub metadata_profile_id: Option<i64>,
    pub is_default: i64,
    pub enabled: i64,
    pub generation: String,
    #[sqlx(skip)]
    pub guard: Option<worker::DispatchGuard>,
}
impl Server {
    fn connection(&self) -> Result<transport::Connection, Error> {
        Ok(transport::Connection {
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            allowed_ips: serde_json::from_str(&self.allowed_ips_json).map_err(|_| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "invalid_connection_state",
                )
            })?,
        })
    }
    fn public(&self) -> Value {
        json!({"id":self.id,"name":self.name,"kind":self.kind,"baseUrl":self.base_url,"hasApiKey":!self.api_key.is_empty(),"libraryId":self.library_id,"rootFolder":self.root_folder,"qualityProfileId":self.quality_profile_id,"metadataProfileId":self.metadata_profile_id,"isDefault":self.is_default==1,"enabled":self.enabled==1})
    }
    fn api(&self) -> &str {
        if self.kind == "lidarr" {
            "api/v1"
        } else {
            "api/v3"
        }
    }
    async fn call(
        &self,
        method: &str,
        path: &str,
        params: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value, Error> {
        if let Some(guard) = &self.guard {
            guard.verify().await?;
        }
        transport::request(
            &self.connection()?,
            method,
            &format!("{}/{path}", self.api()),
            params,
            body,
        )
        .await
    }
}
const SERVER_COLUMNS: &str = "id,name,kind,base_url,api_key,allowed_ips_json,library_id,root_folder,quality_profile_id,metadata_profile_id,is_default,enabled,generation";
pub(crate) async fn server(store: &IdentityStore, id: &str) -> Result<Server, Error> {
    let sql = format!("SELECT {SERVER_COLUMNS} FROM acquisition_servers WHERE id=?");
    query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_optional(store.pool())
        .await
        .map_err(db)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "acquisition_server_not_found"))
}

#[derive(Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRecord {
    pub id: String,
    pub user_id: Option<String>,
    pub server_id: Option<String>,
    pub library_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub external_id: String,
    pub external_provider: String,
    #[serde(skip)]
    pub input_json: String,
    pub status: String,
    pub error_code: Option<String>,
    pub remote_id: Option<String>,
    pub work_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub imported: i64,
    pub source_key: Option<String>,
    pub author_label: Option<String>,
    pub source_status: Option<String>,
    pub source_remote_id: Option<String>,
}
pub(crate) const REQUEST_COLUMNS: &str = "id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,error_code,remote_id,work_id,created_at,updated_at,imported,source_key,author_label,source_status,source_remote_id";
pub(crate) async fn record(store: &IdentityStore, id: &str) -> Result<RequestRecord, Error> {
    query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_COLUMNS} FROM media_requests WHERE id=?"
    )))
    .bind(id)
    .fetch_optional(store.pool())
    .await
    .map_err(db)?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "request_not_found"))
}
pub(crate) async fn admin(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = crate::applications::require(auth, headers, "requests", mutation).await?;
    if actor.user.role != Role::Administrator {
        return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
    }
    Ok(actor)
}
pub(crate) async fn authorize(
    auth: &AuthState,
    actor: &Authenticated,
    library: &str,
) -> Result<(), Error> {
    crate::applications::authorize_library(&auth.store, actor, "requests", library).await
}
pub(crate) async fn unrestricted(store: &IdentityStore, user: &str) -> Result<(), Error> {
    // External results lack trusted classification/tags. Restricted accounts fail closed.
    let count: i64 = query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=?")
        .bind(user)
        .fetch_one(store.pool())
        .await
        .map_err(db)?;
    if count > 0 {
        return Err(err(
            StatusCode::FORBIDDEN,
            "external_metadata_parental_unverified",
        ));
    }
    Ok(())
}
pub(crate) async fn event(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    id: &str,
    actor: Option<&str>,
    status: &str,
    code: Option<&str>,
) -> Result<(), Error> {
    query("INSERT INTO acquisition_events(id,request_id,actor_id,status,code,created_at) VALUES(?,?,?,?,?,?)")
        .bind(uuid::Uuid::now_v7().to_string()).bind(id).bind(actor).bind(status).bind(code).bind(now()).execute(&mut **tx).await.map_err(db)?;
    Ok(())
}

/// Historical import is suspended, repeatable and never inserts an execution job.
pub async fn import_legacy(
    store: &IdentityStore,
    source_id: &str,
    value: &Value,
    user_id: Option<&str>,
    server_id: Option<&str>,
) -> Result<bool, Error> {
    let source_request_id = value["id"].as_i64().filter(|v| *v > 0).ok_or_else(|| {
        err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "import_request_id_required",
        )
    })?;
    let source_key = format!("{source_id}:{source_request_id}");
    let server = match server_id {
        Some(id) => Some(server(store, id).await?),
        None => None,
    };
    let media = &value["media"];
    let kind = match media["mediaType"]
        .as_str()
        .or_else(|| value["type"].as_str())
    {
        Some("movie") => "film",
        Some("tv") => "series",
        _ => {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "import_media_type_unknown",
            ));
        }
    };
    let external_provider = if kind == "series" && media["tvdbId"].as_i64().is_some_and(|id| id > 0)
    {
        "tvdb"
    } else {
        "tmdb"
    };
    let external_id = media[if external_provider == "tvdb" {
        "tvdbId"
    } else {
        "tmdbId"
    }]
    .as_i64()
    .filter(|v| *v > 0)
    .ok_or_else(|| {
        err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "import_external_id_required",
        )
    })?
    .to_string();
    if server.as_ref().is_some_and(|server| {
        (kind == "film" && server.kind != "radarr") || (kind == "series" && server.kind != "sonarr")
    }) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "import_server_kind_mismatch",
        ));
    }
    let title = media["title"]
        .as_str()
        .or_else(|| value["title"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{kind} · {external_provider} {external_id}"));
    let seasons = value["seasons"]
        .as_array()
        .map(|s| {
            s.iter()
                .filter_map(|v| v["seasonNumber"].as_i64())
                .collect()
        })
        .unwrap_or_default();
    let mut input = RequestInput {
        kind: kind.into(),
        title: title.clone(),
        external_id: external_id.clone(),
        external_provider: external_provider.into(),
        server_id: server.as_ref().map(|server| server.id.clone()),
        seasons,
        episodes: vec![],
        album_ids: vec![],
        monitor_future: false,
    };
    let source_status = match value["status"].as_i64() {
        Some(1) => "pending",
        Some(2) => "approved",
        Some(3) => "rejected",
        Some(4) => "failed",
        Some(5) => "completed",
        _ => "unknown",
    };
    let created = value["createdAt"]
        .as_str()
        .and_then(|v| {
            time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|t| t.unix_timestamp())
        .unwrap_or_else(now);
    let updated = value["updatedAt"]
        .as_str()
        .and_then(|v| {
            time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc3339).ok()
        })
        .map(|t| t.unix_timestamp())
        .unwrap_or(created);
    let author = value["requestedBy"]["displayName"]
        .as_str()
        .or_else(|| value["requestedBy"]["username"].as_str())
        .unwrap_or("Auteur historique");
    let id = uuid::Uuid::new_v4().to_string();
    let mut conn = store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    type PreviousImport = (
        String,
        String,
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let existing: Option<PreviousImport> =
        query_as("SELECT id,status,updated_at,user_id,server_id,source_status FROM media_requests WHERE source_key=?")
            .bind(&source_key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    let source_remote_id = media[if value["is4k"] == true {
        "externalServiceId4k"
    } else {
        "externalServiceId"
    }]
    .as_i64()
    .filter(|v| *v > 0)
    .map(|v| v.to_string());
    if let Some((
        id,
        status,
        previous_updated,
        previous_user,
        previous_server,
        previous_source_status,
    )) = existing
    {
        if status == "suspended"
            && (updated > previous_updated
                || previous_user.is_none() && user_id.is_some()
                || previous_server.is_none() && server.is_some())
        {
            input.server_id = previous_server.or(input.server_id);
            let delta_status = if updated >= previous_updated {
                Some(source_status)
            } else {
                previous_source_status.as_deref()
            };
            query("UPDATE media_requests SET source_status=?,updated_at=?,user_id=COALESCE(user_id,?),server_id=COALESCE(server_id,?),library_id=COALESCE(library_id,?),input_json=?,source_remote_id=COALESCE(?,source_remote_id),author_label=?,external_id=?,external_provider=?,title=? WHERE id=? AND status='suspended'")
                .bind(delta_status).bind(updated.max(previous_updated)).bind(user_id).bind(server.as_ref().map(|s|s.id.as_str())).bind(server.as_ref().map(|s|s.library_id.as_str())).bind(json!(input).to_string()).bind(&source_remote_id).bind(author.chars().take(128).collect::<String>()).bind(&external_id).bind(external_provider).bind(title.chars().take(512).collect::<String>()).bind(&id).execute(&mut *tx).await.map_err(db)?;
            event(&mut tx, &id, None, "suspended", Some("legacy_delta_import")).await?;
            crate::database::commit(tx).await.map_err(db)?;
            return Ok(true);
        }
        crate::database::commit(tx).await.map_err(db)?;
        return Ok(false);
    }
    let result=query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,error_code,remote_id,created_at,updated_at,imported,source_key,author_label,source_status,source_remote_id) VALUES(?,?,?,?,?,?,?,?,?,'suspended',NULL,NULL,?,?,?,?,?,?,?) ON CONFLICT(source_key) DO NOTHING")
        .bind(&id).bind(user_id).bind(server.as_ref().map(|s|s.id.as_str())).bind(server.as_ref().map(|s|s.library_id.as_str())).bind(kind).bind(title.chars().take(512).collect::<String>()).bind(external_id).bind(external_provider).bind(json!(input).to_string())
        .bind(created).bind(updated).bind(1i64).bind(source_key).bind(author.chars().take(128).collect::<String>()).bind(source_status).bind(source_remote_id).execute(&mut *tx).await.map_err(db)?;
    let inserted = result.rows_affected() > 0;
    if inserted {
        event(&mut tx, &id, None, "suspended", Some("legacy_import")).await?;
    }
    crate::database::commit(tx).await.map_err(db)?;
    Ok(inserted)
}

#[cfg(test)]
mod remote_tests;
#[cfg(test)]
mod tests;
