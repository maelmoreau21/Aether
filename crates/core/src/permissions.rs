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
    // Inventory is explicitly fixture-only. A new library does not acquire assets
    // merely by selecting the same root; only these two seeded associations exist.
    let rows = sqlx::query_as::<_, (String,String,String)>("SELECT id,root,kind FROM libraries WHERE enabled=1 AND node_id='local' AND ( ?='administrator' OR EXISTS(SELECT 1 FROM library_permissions p WHERE p.library_id=libraries.id AND p.user_id=?))")
        .bind(identity.user.role.as_str()).bind(&identity.user.id).fetch_all(&state.auth.store.pool).await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let root = state.root.to_string_lossy();
    let visible: Vec<String> = rows
        .into_iter()
        .filter(|(id, r, kind)| {
            r == root.as_ref()
                && ((id == "fixture-films" && kind == "films")
                    || (id == "fixture-series" && kind == "series"))
        })
        .map(|r| r.0)
        .collect();
    let path = request.uri().path();
    let allowed = if path == "/api/v1/catalog" {
        !visible.is_empty()
    } else if let Some(id) = path.strip_prefix("/api/v1/catalog/") {
        let library = match id {
            "fixture-film" => "fixture-films",
            "fixture-series" | "fixture-episode-1" => "fixture-series",
            _ => "",
        };
        visible.iter().any(|v| v == library)
    } else if path.starts_with("/api/v1/demo/") || path.starts_with("/api/v1/assets/demo-") {
        !visible.is_empty()
    } else {
        visible.iter().any(|v| v == "fixture-films")
    };
    if !allowed {
        return error(StatusCode::FORBIDDEN, "permission_denied");
    }
    request.extensions_mut().insert(VisibleLibraries(visible));
    next.run(request).await
}
