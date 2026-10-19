//! Administrator-only account lifecycle; no destructive account deletion.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::Role,
};
use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

type ApiError = (StatusCode, Json<serde_json::Value>);
fn error(status: StatusCode, code: &str) -> ApiError {
    (status, Json(serde_json::json!({"error":code})))
}
fn db(_: sqlx::Error) -> ApiError {
    error(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
}
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    login: String,
    role: String,
    display_name: Option<String>,
    disabled_at: Option<i64>,
    #[sqlx(try_from = "crate::database::DbBool")]
    sso: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Create {
    login: String,
    password: String,
    role: String,
    display_name: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Update {
    role: String,
    disabled: bool,
    display_name: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Password {
    password: String,
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/users", get(list).post(create))
        .route("/api/v1/admin/users/{id}", put(update).patch(update))
        .route("/api/v1/admin/users/{id}/password", put(password))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}
async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, ApiError> {
    let identity = auth
        .authenticate(headers)
        .await
        .map_err(|s| error(s, "authentication_required"))?;
    if identity.user.role != Role::Administrator {
        return Err(error(StatusCode::FORBIDDEN, "administrator_required"));
    }
    if mutation {
        auth.require_csrf(headers, &identity)
            .map_err(|s| error(s, "csrf_failed"))?;
    }
    Ok(identity)
}
fn validate(role: &str, display: &Option<String>) -> Result<(), ApiError> {
    if !matches!(role, "administrator" | "user") {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_role"));
    }
    if display
        .as_ref()
        .is_some_and(|v| v.chars().count() > 128 || v.chars().any(char::is_control))
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_display_name"));
    }
    Ok(())
}
async fn hash_password(auth: &AuthState, password: String) -> Result<String, ApiError> {
    let password = Zeroizing::new(password);
    if !(15..=128).contains(&password.chars().count())
        || password.len() > 512
        || password.chars().any(char::is_control)
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_password"));
    }
    let permit = auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| error(StatusCode::TOO_MANY_REQUESTS, "password_workers_busy"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| ())?;
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(password.as_bytes())
            .map(|v| v.to_string())
            .map_err(|_| ())
    })
    .await
    .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "password_hash_failed"))?
    .map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "password_hash_failed"))
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    administrator(&auth, &headers, false).await?;
    let users:Vec<Account>=query_as("SELECT u.id,COALESCE(u.sso_username,c.login,u.display_name,'sso-' || u.id) AS login,u.role,u.display_name,u.disabled_at,CAST((c.login IS NULL) AS INTEGER) AS sso FROM users u LEFT JOIN local_credentials c ON c.user_id=u.id ORDER BY login,u.id").fetch_all(&auth.store.pool).await.map_err(db)?;
    Ok(Json(serde_json::json!({"users":users})))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let actor = administrator(&auth, &headers, true).await?;
    validate(&input.role, &input.display_name)?;
    if !(3..=64).contains(&input.login.len())
        || !input
            .login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_login"));
    }
    let hash = hash_password(&auth, input.password).await?;
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    recheck_actor(&mut tx, &actor).await?;
    let exists: i64 = query_scalar("SELECT COUNT(*) FROM local_credentials WHERE login=?")
        .bind(&input.login)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if exists > 0 {
        return Err(error(StatusCode::CONFLICT, "login_exists"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO users(id,role,display_name) VALUES(?,?,?)")
        .bind(&id)
        .bind(&input.role)
        .bind(&input.display_name)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    query("INSERT INTO local_credentials(user_id,login,password_hash) VALUES(?,?,?)")
        .bind(&id)
        .bind(input.login)
        .bind(hash)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::CREATED, Json(serde_json::json!({"id":id}))))
}
async fn recheck_actor(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    actor: &Authenticated,
) -> Result<(), ApiError> {
    let valid:i64=query_scalar("SELECT COUNT(*) FROM users u JOIN sessions s ON s.user_id=u.id WHERE u.id=? AND u.role='administrator' AND u.disabled_at IS NULL AND s.token_hash=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-1800").bind(&actor.user.id).bind(&actor.token_hash).fetch_one(&mut **tx).await.map_err(db)?;
    if valid != 1 {
        return Err(error(StatusCode::FORBIDDEN, "administrator_required"));
    }
    Ok(())
}
async fn update(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Update>,
) -> Result<StatusCode, ApiError> {
    let actor = administrator(&auth, &headers, true).await?;
    validate(&input.role, &input.display_name)?;
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    recheck_actor(&mut tx, &actor).await?;
    let old: Option<(String, Option<i64>)> =
        query_as("SELECT role,disabled_at FROM users WHERE id=?")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?;
    let (role, disabled) = old.ok_or_else(|| error(StatusCode::NOT_FOUND, "user_not_found"))?;
    if id == actor.user.id && (input.disabled || input.role != "administrator") {
        return Err(error(StatusCode::CONFLICT, "self_protection"));
    }
    if role == "administrator"
        && disabled.is_none()
        && (input.disabled || input.role != "administrator")
    {
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM users WHERE role='administrator' AND disabled_at IS NULL",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if count <= 1 {
            return Err(error(StatusCode::CONFLICT, "last_administrator"));
        }
    }
    query("UPDATE users SET role=?,display_name=?,disabled_at=CASE WHEN ?=1 THEN COALESCE(disabled_at,unixepoch()) ELSE NULL END WHERE id=?").bind(&input.role).bind(input.display_name).bind(i64::from(input.disabled)).bind(&id).execute(&mut *tx).await.map_err(db)?;
    if role != input.role || disabled.is_some() != input.disabled {
        query("DELETE FROM sessions WHERE user_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn password(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Password>,
) -> Result<StatusCode, ApiError> {
    let actor = administrator(&auth, &headers, true).await?;
    let hash = hash_password(&auth, input.password).await?;
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    recheck_actor(&mut tx, &actor).await?;
    let changed = query(
        "UPDATE local_credentials SET password_hash=?,updated_at=unixepoch() WHERE user_id=?",
    )
    .bind(hash)
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if changed.rows_affected() != 1 {
        return Err(error(StatusCode::CONFLICT, "local_credentials_required"));
    }
    query("DELETE FROM sessions WHERE user_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
