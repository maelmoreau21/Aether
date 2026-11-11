//! Personal lists and explicit editorial controls. No changes to source media or sidecars.

use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

mod grouping;
mod lists;
mod metadata;
mod parental;
pub(crate) use grouping::*;
pub(crate) use lists::*;
pub(crate) use metadata::*;
pub(crate) use parental::*;

type Error = StatusCode;

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/lists", get(lists).post(create_list))
        .route("/api/v1/lists/{id}", get(list).delete(delete_list))
        .route(
            "/api/v1/lists/{id}/items/{work}",
            axum::routing::put(add_item).delete(remove_item),
        )
        .route(
            "/api/v1/admin/works/{work}/metadata",
            get(get_metadata).put(metadata).delete(reset_metadata),
        )
        .route(
            "/api/v1/works/{work}/poster",
            get(poster).put(upload_poster).delete(remove_poster),
        )
        .route(
            "/api/v1/admin/users/{user}/parental",
            get(parental).put(set_parental).delete(reset_parental),
        )
        .route("/api/v1/admin/media-groups", axum::routing::post(group))
        .route(
            "/api/v1/admin/media-groups/candidates",
            get(list_candidates),
        )
        .route(
            "/api/v1/admin/media-groups/candidates/{id}/confirm",
            axum::routing::post(confirm_candidate),
        )
        .route(
            "/api/v1/admin/media-groups/candidates/{id}/reject",
            axum::routing::post(reject_candidate),
        )
        .route(
            "/api/v1/admin/media-groups/merges/{id}/undo",
            axum::routing::post(undo_merge),
        )
        .route(
            "/api/v1/admin/media-groups/{work}",
            axum::routing::delete(ungroup),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(auth)
}

async fn admin(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(actor)
}

async fn owner(
    auth: &AuthState,
    headers: &HeaderMap,
    id: &str,
    mutation: bool,
) -> Result<String, Error> {
    let actor = auth.authenticate(headers).await?;
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    let count: i64 = query_scalar("SELECT COUNT(*) FROM personal_lists WHERE id=? AND user_id=?")
        .bind(id)
        .bind(&actor.user.id)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count != 1 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(actor.user.id)
}

#[cfg(test)]
mod automatic_collection_tests;

#[cfg(test)]
mod tests;
