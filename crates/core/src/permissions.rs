//! Live, fail-closed library permissions before every media request.
use crate::{auth::AuthState, error, identity::Role};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::path::PathBuf;

#[derive(Clone)]
pub struct MediaAccess {
    pub auth: AuthState,
    pub root: PathBuf,
}
#[derive(Clone)]
pub struct VisibleLibraries(pub Vec<String>);
// Retained for callers describing the role default; actual rights also require an active library.
pub fn can_read_fixture_library(role: Role) -> bool {
    matches!(role, Role::Administrator)
}

pub async fn require_media(
    State(state): State<MediaAccess>,
    mut request: Request,
    next: Next,
) -> Response {
    let identity = match state.auth.authenticate(request.headers()).await {
        Ok(identity) => identity,
        Err(status) => return error(status, "authentication_required"),
    };
    // Read permissions are live; fixture associations remain separate from indexed assets.
    let rows = crate::database::query_as::<_, (String,String,String)>("SELECT id,root,kind FROM libraries WHERE enabled=1 AND EXISTS(SELECT 1 FROM media_nodes n WHERE n.id=libraries.node_id AND n.status != 'revoked') AND ( ?='administrator' OR EXISTS(SELECT 1 FROM library_permissions p WHERE p.library_id=libraries.id AND p.user_id=?))")
        .bind(identity.user.role.as_str()).bind(&identity.user.id).fetch_all(&state.auth.store.pool).await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let root = state.root.to_string_lossy();
    let visible: Vec<String> = rows
        .into_iter()
        .filter(|(id, r, kind)| {
            !id.starts_with("fixture-")
                || (r == root.as_ref()
                    && ((id == "fixture-films" && kind == "films")
                        || (id == "fixture-series" && kind == "series")))
        })
        .map(|r| r.0)
        .collect();
    let path = request.uri().path();
    let allowed = if path == "/api/v1/catalog" {
        !visible.is_empty() || identity.user.role == Role::Administrator
    } else if let Some(id) = path.strip_prefix("/api/v1/catalog/") {
        let library = match id {
            "fixture-film" => "fixture-films",
            "fixture-series" | "fixture-episode-1" => "fixture-series",
            _ => "",
        };
        if library.is_empty() {
            match crate::inventory::resource_library(&state.auth.store, "work", id).await {
                Ok(Some(library)) => visible.iter().any(|v| v == &library),
                Ok(None) => false,
                Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
            }
        } else {
            visible.iter().any(|v| v == library)
        }
    } else if path.starts_with("/api/v1/media/")
        || (path.starts_with("/api/v1/assets/")
            && !path.starts_with("/api/v1/assets/demo-")
            && !path.starts_with("/api/v1/assets/fixture-"))
    {
        let id = path
            .strip_prefix("/api/v1/media/")
            .or_else(|| {
                path.strip_prefix("/api/v1/assets/").and_then(|value| {
                    value
                        .strip_suffix("/content")
                        .or_else(|| value.strip_suffix("/image"))
                        .or_else(|| value.split_once("/subtitles/").map(|(id, _)| id))
                        .or_else(|| value.split_once("/audio/").map(|(id, _)| id))
                        .or_else(|| value.split_once("/prepared/").map(|(id, _)| id))
                })
            })
            .unwrap_or("");
        match crate::inventory::resource_library(
            &state.auth.store,
            if path.ends_with("/image") {
                "image"
            } else {
                "asset"
            },
            id,
        )
        .await
        {
            Ok(Some(library)) => visible.iter().any(|v| v == &library),
            Ok(None) => false,
            Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
        }
    } else if path.starts_with("/api/v1/demo/") || path.starts_with("/api/v1/assets/demo-") {
        visible
            .iter()
            .any(|v| v == "fixture-films" || v == "fixture-series")
    } else {
        visible.iter().any(|v| v == "fixture-films")
    };
    if !allowed {
        return error(StatusCode::FORBIDDEN, "permission_denied");
    }
    request.extensions_mut().insert(VisibleLibraries(visible));
    request.extensions_mut().insert(identity);
    next.run(request).await
}
