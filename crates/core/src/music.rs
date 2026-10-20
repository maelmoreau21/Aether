//! Music catalogue, independent of film identities. Node owns scanning and file resolution.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::IdentityStore,
};
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{path::Path as DiskPath, sync::atomic::AtomicBool};
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/music", get(list))
        .route(
            "/api/v1/music/tracks/{id}/content",
            get(content).head(content),
        )
        .with_state(auth)
}
async fn allowed(auth: &AuthState, headers: &HeaderMap, library: &str) -> Result<(), StatusCode> {
    let actor = auth.authenticate(headers).await?;
    let exists:i64=query_scalar("SELECT COUNT(*) FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.kind='music' AND l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=? AND policy.allow_unrated=0) AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?))")
        .bind(library).bind(&actor.user.id).bind(actor.user.role.as_str()).bind(&actor.user.id).fetch_one(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    if exists == 0 {
        Err(StatusCode::FORBIDDEN)
    } else {
        Ok(())
    }
}
#[derive(Default, Deserialize)]
struct Filter {
    q: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(filter): Query<Filter>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    let limit = filter.limit.unwrap_or(100);
    let offset = filter.offset.unwrap_or(0);
    if !(1..=200).contains(&limit) || offset < 0 || filter.q.as_ref().is_some_and(|q| q.len() > 256)
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let term = format!(
        "%{}%",
        filter
            .q
            .unwrap_or_default()
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let total: i64 = query_scalar("SELECT COUNT(*) FROM music_tracks t JOIN libraries l ON l.id=t.library_id JOIN media_nodes n ON n.id=l.node_id WHERE t.available=1 AND t.root_snapshot=l.root AND l.kind='music' AND l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=? AND policy.allow_unrated=0) AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND lower(t.title || ' ' || t.artist || ' ' || t.album) LIKE ? ESCAPE '\\'")
        .bind(&actor.user.id)
        .bind(actor.user.role.as_str())
        .bind(&actor.user.id)
        .bind(&term)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows=query("SELECT t.* FROM music_tracks t JOIN libraries l ON l.id=t.library_id JOIN media_nodes n ON n.id=l.node_id WHERE t.available=1 AND t.root_snapshot=l.root AND l.kind='music' AND l.enabled=1 AND n.status!='revoked' AND NOT EXISTS(SELECT 1 FROM parental_policies policy WHERE policy.user_id=? AND policy.allow_unrated=0) AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) AND lower(t.title || ' ' || t.artist || ' ' || t.album) LIKE ? ESCAPE '\\' ORDER BY t.album_artist,t.album,t.disc_number,t.track_number,t.title,t.id LIMIT ? OFFSET ?").bind(&actor.user.id).bind(actor.user.role.as_str()).bind(&actor.user.id).bind(term).bind(limit).bind(offset).fetch_all(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?;
    let items:Vec<Value>=rows.into_iter().map(|r|json!({"id":r.get::<String,_>("id"),"libraryId":r.get::<String,_>("library_id"),"title":r.get::<String,_>("title"),"artist":r.get::<String,_>("artist"),"albumArtist":r.get::<String,_>("album_artist"),"album":r.get::<String,_>("album"),"trackNumber":r.get::<Option<i64>,_>("track_number"),"duration":r.get::<Option<f64>,_>("duration"),"codec":r.get::<String,_>("codec"),"mime":r.get::<String,_>("mime")})).collect();
    Ok(Json(
        json!({"items":items,"total":total,"offset":offset,"limit":limit}),
    ))
}
async fn content(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, StatusCode> {
    auth.authenticate(request.headers()).await?;
    let row=query("SELECT t.*,l.root,l.node_id FROM music_tracks t JOIN libraries l ON l.id=t.library_id WHERE t.id=? AND t.available=1 AND t.root_snapshot=l.root").bind(id).fetch_optional(&auth.store.pool).await.map_err(|_|StatusCode::INTERNAL_SERVER_ERROR)?.ok_or(StatusCode::NOT_FOUND)?;
    allowed(
        &auth,
        request.headers(),
        &row.get::<String, _>("library_id"),
    )
    .await?;
    let node_id = row.get::<String, _>("node_id");
    if node_id != "local" {
        let node = crate::nodes::load_node(&auth.store, &node_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let id = row.get::<String, _>("id");
        return Ok(crate::nodes::relay_from_remote_node(
            &auth.store,
            &node,
            &format!("/internal/v1/node/assets/{id}/content"),
            request,
        )
        .await);
    }
    let opened = media_node::OpenedFile::open_in_root(
        DiskPath::new(&row.get::<String, _>("root")),
        DiskPath::new(&row.get::<String, _>("relative_path")),
    )
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;
    if opened.size() != row.get::<i64, _>("size") as u64
        || opened.modified_ns() != row.get::<String, _>("modified_ns")
    {
        return Err(StatusCode::CONFLICT);
    }
    let response = media_node::serve_opened(opened, &request, &row.get::<String, _>("mime"))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(response.map(axum::body::Body::new))
}
pub async fn scan(
    store: &IdentityStore,
    library: &str,
    root: &DiskPath,
    cancel: &AtomicBool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let patterns: String = query_scalar("SELECT excluded_patterns FROM libraries WHERE id=?")
        .bind(library)
        .fetch_one(&store.pool)
        .await?;
    let patterns: Vec<String> = patterns
        .split([',', ';', '\n', '\r'])
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect();
    let scan = uuid::Uuid::new_v4().to_string();
    let count = media_node::music::scan_with_exclusions(root, cancel, &patterns, |track| {
        let store = store.clone();
        let library = library.to_owned();
        let root = root.to_string_lossy().into_owned();
        let scan = scan.clone();
        async move {
            publish_track(&store, &library, &root, &scan, track).await?;
            Ok(())
        }
    })
    .await?;
    query("UPDATE music_tracks SET available=0 WHERE library_id=? AND scan_id!=?")
        .bind(library)
        .bind(scan)
        .execute(&store.pool)
        .await?;
    Ok(count)
}

pub async fn publish_track(
    store: &IdentityStore,
    library: &str,
    root: &str,
    scan: &str,
    track: media_node::music::MusicTrack,
) -> std::io::Result<()> {
    query("INSERT INTO music_tracks(id,library_id,root_snapshot,relative_path,size,modified_ns,title,artist,album,album_artist,track_number,disc_number,duration,codec,mime,scan_id,available) SELECT ?,id,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1 FROM libraries WHERE id=? AND root=? AND enabled=1 AND kind='music' ON CONFLICT(library_id,root_snapshot,relative_path) DO UPDATE SET size=excluded.size,modified_ns=excluded.modified_ns,title=excluded.title,artist=excluded.artist,album=excluded.album,album_artist=excluded.album_artist,track_number=excluded.track_number,disc_number=excluded.disc_number,duration=excluded.duration,codec=excluded.codec,mime=excluded.mime,scan_id=excluded.scan_id,available=1")
                .bind(uuid::Uuid::new_v4().to_string()).bind(root).bind(track.relative_path).bind(track.size as i64).bind(track.modified_ns).bind(track.title).bind(track.artist).bind(track.album).bind(track.album_artist).bind(track.track_number).bind(track.disc_number).bind(track.duration).bind(track.codec).bind(track.mime).bind(scan).bind(library).bind(root).execute(&store.pool).await.map_err(std::io::Error::other)?;

    Ok(())
}
