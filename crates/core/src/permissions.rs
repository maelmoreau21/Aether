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

/// The file a `/api/v1/media/{id}` request is served from, with its library: the id itself
/// for a file, the first playable file of a film, an episode or a series otherwise (the
/// descriptor handler resolves the same way). An unknown id comes back unchanged and without
/// a library, so it fails the library check.
async fn media_asset(
    store: &crate::identity::IdentityStore,
    id: &str,
) -> Result<(String, Option<String>), sqlx::Error> {
    if let Some(library) = crate::inventory::resource_library(store, "asset", id).await? {
        return Ok((id.to_owned(), Some(library)));
    }
    let Some(asset) = crate::inventory::playable_asset_for_work(store, id).await? else {
        return Ok((id.to_owned(), None));
    };
    let library = crate::inventory::resource_library(store, "asset", &asset).await?;
    Ok((asset, library))
}

/// `/api/v1/assets/{asset}/hls/{grant}/{component}`: (asset, grant). Such a URL
/// carries its own authorization, so a cast device can follow it with no cookie.
pub fn hls_grant(path: &str) -> Option<(&str, &str)> {
    let mut parts = path.strip_prefix("/api/v1/assets/")?.split('/');
    let (asset, kind, grant, component) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (kind == "hls" && !asset.is_empty() && !component.is_empty() && parts.next().is_none())
        .then_some((asset, grant))
}

pub async fn require_media(
    State(state): State<MediaAccess>,
    mut request: Request,
    next: Next,
) -> Response {
    // A capability URL never falls back to a cookie, and a cookie never reaches it.
    let authenticated = match hls_grant(request.uri().path()) {
        Some((asset, grant)) => state.auth.authenticate_grant(grant, asset).await,
        None => state.auth.authenticate(request.headers()).await,
    };
    let identity = match authenticated {
        Ok(identity) => identity,
        Err(status) => return error(status, "authentication_required"),
    };
    // Read permissions are live; fixture associations remain separate from indexed assets.
    // A library with `adminAccess: false` reaches administrators only through an explicit grant.
    let rows = crate::database::query_as::<_, (String,String,String)>("SELECT id,root,kind FROM libraries WHERE enabled=1 AND EXISTS(SELECT 1 FROM media_nodes n WHERE n.id=libraries.node_id AND n.status != 'revoked') AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=libraries.id AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=libraries.id AND p.user_id=?))")
        .bind(identity.user.role.as_str()).bind(&identity.user.id).fetch_all(&state.auth.store.pool).await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let root = state.root.to_string_lossy();
    let path = request.uri().path().to_owned();
    // A "watch together" session points at a film, an episode or a series, and its
    // descriptor is that of the work's first playable file: the gate checks that file.
    let media_target = match path.strip_prefix("/api/v1/media/") {
        Some(id) => match media_asset(&state.auth.store, id).await {
            Ok(target) => Some(target),
            Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
        },
        None => None,
    };
    let path = path.as_str();
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
    let allowed = if path == "/api/v1/catalog" {
        !visible.is_empty() || identity.user.role == Role::Administrator
    } else if let Some(id) = path.strip_prefix("/api/v1/catalog/") {
        let library = match id {
            "fixture-film" => "fixture-films",
            "fixture-series" | "fixture-episode-1" => "fixture-series",
            _ => "",
        };
        if library.is_empty() {
            let direct_ok = match crate::inventory::resource_library(&state.auth.store, "work", id)
                .await
            {
                Ok(Some(library)) => visible.iter().any(|v| v == &library),
                Ok(None) => false,
                Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "inventory_unavailable"),
            };
            if direct_ok {
                true
            } else {
                match crate::consolidation::authorized_group_members(
                    &state.auth.store,
                    id,
                    Some(&identity.user.id),
                    Some(identity.user.role.as_str()),
                )
                .await
                {
                    Ok(members) => !members.is_empty(),
                    Err(_) => false,
                }
            }
        } else {
            visible.iter().any(|v| v == library)
        }
    } else if path.starts_with("/api/v1/media/")
        || (path.starts_with("/api/v1/assets/")
            && !path.starts_with("/api/v1/assets/demo-")
            && !path.starts_with("/api/v1/assets/fixture-"))
    {
        // `/api/v1/media/{id}`: the library was read with the file it resolves to.
        let library = if let Some((_, library)) = &media_target {
            Ok(library.clone())
        } else {
            let id = path
                .strip_prefix("/api/v1/assets/")
                .and_then(|value| {
                    value
                        .strip_suffix("/content")
                        .or_else(|| value.strip_suffix("/download"))
                        .or_else(|| value.strip_suffix("/image"))
                        .or_else(|| value.strip_suffix("/trickplay"))
                        .or_else(|| value.strip_suffix("/chapter-images"))
                        .or_else(|| value.strip_suffix("/details"))
                        .or_else(|| value.split_once("/chapter-images/").map(|(id, _)| id))
                        .or_else(|| value.split_once("/trickplay/").map(|(id, _)| id))
                        .or_else(|| value.split_once("/subtitles/").map(|(id, _)| id))
                        .or_else(|| value.split_once("/audio/").map(|(id, _)| id))
                        .or_else(|| value.split_once("/prepared/").map(|(id, _)| id))
                        .or_else(|| value.strip_suffix("/hls"))
                        .or_else(|| value.split_once("/hls/").map(|(id, _)| id))
                })
                .unwrap_or("");
            crate::inventory::resource_library(
                &state.auth.store,
                if path.ends_with("/image") {
                    "image"
                } else {
                    "asset"
                },
                id,
            )
            .await
        };
        match library {
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
    if path != "/api/v1/catalog" {
        let (kind, target) = if let Some(id) = path.strip_prefix("/api/v1/catalog/") {
            ("work", id)
        } else if let Some((asset, _)) = &media_target {
            ("asset", asset.as_str())
        } else if let Some(id) = path.strip_prefix("/api/v1/assets/") {
            (
                if path.ends_with("/image") {
                    "image"
                } else {
                    "asset"
                },
                id.split('/').next().unwrap_or(""),
            )
        } else {
            ("asset", "")
        };
        match crate::product::target_allowed(&state.auth.store, &identity.user.id, kind, target)
            .await
        {
            Ok(true) => {}
            Ok(false) => return error(StatusCode::FORBIDDEN, "parental_restriction"),
            Err(status) => return error(status, "parental_policy_unavailable"),
        }
    }
    let downloading = path.ends_with("/download");
    if downloading {
        match crate::user_media_policy::can_download(&state.auth.store, &identity.user.id).await {
            Ok(true) => {}
            Ok(false) => return error(StatusCode::FORBIDDEN, "media_download_disabled"),
            Err(status) => return error(status, "media_policy_unavailable"),
        }
    }
    request.extensions_mut().insert(VisibleLibraries(visible));
    request.extensions_mut().insert(identity);
    let mut response = next.run(request).await;
    if downloading && response.status().is_success() {
        response.headers_mut().insert(
            axum::http::header::CONTENT_DISPOSITION,
            axum::http::HeaderValue::from_static("attachment"),
        );
    }
    response
}
