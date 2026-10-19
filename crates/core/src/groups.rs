//! Local groups grant library access only; membership never grants a server role.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/groups", get(list).post(create))
        .route(
            "/api/v1/admin/groups/{id}",
            axum::routing::put(update).delete(remove),
        )
        .layer(DefaultBodyLimit::max(32768))
        .with_state(auth)
}
pub(crate) async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, StatusCode> {
    let identity = auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &identity)?;
    }
    Ok(identity)
}
pub(crate) async fn recheck_administrator(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    actor: &Authenticated,
) -> Result<(), StatusCode> {
    let count: i64 = query_scalar("SELECT count(*) FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.user_id=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-1800 AND u.disabled_at IS NULL AND u.role='administrator'").bind(&actor.token_hash).bind(&actor.user.id).fetch_one(&mut **tx).await.map_err(database)?;
    if count != 1 {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}
fn database(error: sqlx::Error) -> StatusCode {
    if error
        .as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        StatusCode::CONFLICT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    name: String,
    #[serde(default)]
    is_default: bool,
    user_ids: Vec<String>,
    library_ids: Vec<String>,
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    let mut tx = auth.store.pool.begin().await.map_err(database)?;
    let rows: Vec<(String, String, bool)> =
        query_as("SELECT id,name,is_default FROM user_groups ORDER BY name,id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let members: Vec<(String, String)> =
        query_as("SELECT group_id,user_id FROM user_group_members ORDER BY user_id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let libraries: Vec<(String, String)> =
        query_as("SELECT group_id,library_id FROM user_group_libraries ORDER BY library_id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let values: Vec<_>=rows.into_iter().map(|(id,name,is_default)|json!({"id":id,"name":name,"isDefault":is_default,"userIds":members.iter().filter(|r|r.0==id).map(|r|&r.1).collect::<Vec<_>>(),"libraryIds":libraries.iter().filter(|r|r.0==id).map(|r|&r.1).collect::<Vec<_>>() })).collect();
    tx.commit().await.map_err(database)?;
    Ok(Json(json!({"groups":values})))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    let id = uuid::Uuid::new_v4().to_string();
    save(&auth, &actor, &id, input, true).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Input>,
) -> Result<Json<Value>, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    save(&auth, &actor, &id, input, false).await?;
    Ok(Json(json!({"id":id})))
}
async fn save(
    auth: &AuthState,
    actor: &Authenticated,
    id: &str,
    input: Input,
    create: bool,
) -> Result<(), StatusCode> {
    let name = input.name.trim();
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || input.user_ids.len() > 500
        || input.library_ids.len() > 500
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, actor).await?;
    for user in &input.user_ids {
        let exists: i64 = query_scalar("SELECT count(*) FROM users WHERE id=?")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
        if exists != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    for library in &input.library_ids {
        let exists: i64 = query_scalar("SELECT count(*) FROM libraries WHERE id=?")
            .bind(library)
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
        if exists != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    if create {
        let count: i64 = query_scalar("SELECT count(*) FROM user_groups")
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
        if count >= 500 {
            return Err(StatusCode::CONFLICT);
        }
        query("INSERT INTO user_groups(id,name,is_default) VALUES(?,?,?)")
            .bind(id)
            .bind(name)
            .bind(input.is_default)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    } else {
        if query("UPDATE user_groups SET name=?,is_default=? WHERE id=?")
            .bind(name)
            .bind(input.is_default)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?
            .rows_affected()
            != 1
        {
            return Err(StatusCode::NOT_FOUND);
        }
        query("DELETE FROM user_group_members WHERE group_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        query("DELETE FROM user_group_libraries WHERE group_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    for user in input.user_ids {
        query(
            "INSERT INTO user_group_members(group_id,user_id) VALUES(?,?) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(user)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    }
    for library in input.library_ids {
        query("INSERT INTO user_group_libraries(group_id,library_id) VALUES(?,?) ON CONFLICT DO NOTHING").bind(id).bind(library).execute(&mut *tx).await.map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    Ok(())
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, &actor).await?;
    let affected = query("DELETE FROM user_groups WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(database)?
        .rows_affected();
    if affected == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    tx.commit().await.map_err(database)?;
    Ok(StatusCode::NO_CONTENT)
}
