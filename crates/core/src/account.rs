//! Personal credentials and live session revocation. Session hashes never leave Core.
use crate::{
    auth::{AuthState, Authenticated, hash},
    database::{query, query_as, query_scalar},
};
use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

type Error = (StatusCode, Json<Value>);
fn failure(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error": code})))
}
fn db(_: sqlx::Error) -> Error {
    failure(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
}
async fn actor(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = auth
        .authenticate(headers)
        .await
        .map_err(|s| failure(s, "authentication_required"))?;
    if mutation {
        auth.require_csrf(headers, &actor)
            .map_err(|s| failure(s, "csrf_failed"))?;
    }
    Ok(actor)
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/account/password", put(password))
        .route(
            "/api/v1/account/local-credentials",
            get(local_policy).post(create_local),
        )
        .route("/api/v1/account/sessions", get(sessions).delete(revoke_all))
        .route(
            "/api/v1/account/sessions/{id}",
            axum::routing::delete(revoke),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}
async fn local_policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = actor(&auth, &headers, false).await?;
    let exists: i64 = query_scalar("SELECT COUNT(*) FROM local_credentials WHERE user_id=?")
        .bind(&actor.user.id)
        .fetch_one(&auth.store.pool)
        .await
        .map_err(db)?;
    let blocked = crate::server_settings::load(&auth.store)
        .await
        .map_err(|s| failure(s, "settings_unavailable"))?
        .block_sso_local_accounts;
    Ok(Json(json!({"canCreate":exists==0 && !blocked})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalCredentials {
    login: String,
    password: String,
}
async fn create_local(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<LocalCredentials>,
) -> Result<StatusCode, Error> {
    let actor = actor(&auth, &headers, true).await?;
    if crate::server_settings::load(&auth.store)
        .await
        .map_err(|s| failure(s, "settings_unavailable"))?
        .block_sso_local_accounts
    {
        return Err(failure(StatusCode::FORBIDDEN, "sso_local_accounts_blocked"));
    }
    if !(3..=64).contains(&input.login.len())
        || !input
            .login
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
    {
        return Err(failure(StatusCode::BAD_REQUEST, "invalid_login"));
    }
    let recent: i64 = query_scalar(
        "SELECT COUNT(*) FROM sessions WHERE token_hash=? AND created_at>unixepoch()-300",
    )
    .bind(&actor.token_hash)
    .fetch_one(&auth.store.pool)
    .await
    .map_err(db)?;
    if recent == 0 {
        return Err(failure(StatusCode::FORBIDDEN, "recent_login_required"));
    }
    let hash = crate::admin_users::hash_password(&auth, input.password).await?;
    let result=query("INSERT INTO local_credentials(user_id,login,password_hash) SELECT ?,?,? WHERE EXISTS(SELECT 1 FROM oidc_identities WHERE user_id=?) AND NOT EXISTS(SELECT 1 FROM local_credentials WHERE user_id=?)").bind(&actor.user.id).bind(input.login).bind(hash).bind(&actor.user.id).bind(&actor.user.id).execute(&auth.store.pool).await.map_err(|_|failure(StatusCode::CONFLICT,"local_credentials_conflict"))?;
    if result.rows_affected() != 1 {
        return Err(failure(StatusCode::CONFLICT, "local_credentials_conflict"));
    }
    Ok(StatusCode::NO_CONTENT)
}
fn public_id(token_hash: &str) -> String {
    hash(&format!("device:{token_hash}"))
}
async fn sessions(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let actor = actor(&auth, &headers, false).await?;
    let rows: Vec<(String,String,i64,i64,i64)> = query_as("SELECT token_hash,device_label,created_at,last_seen_at,expires_at FROM sessions WHERE user_id=? AND expires_at>unixepoch() AND last_seen_at>unixepoch()-604800 ORDER BY last_seen_at DESC LIMIT 200")
        .bind(&actor.user.id).fetch_all(&auth.store.pool).await.map_err(db)?;
    Ok(Json(
        json!({"sessions":rows.into_iter().map(|(key,device,created,last,expires)|json!({"id":public_id(&key),"device":device,"createdAt":created,"lastSeenAt":last,"expiresAt":expires,"current":key==actor.token_hash})).collect::<Vec<_>>()}),
    ))
}
async fn revoke(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    let actor = actor(&auth, &headers, true).await?;
    let keys: Vec<String> = query_scalar("SELECT token_hash FROM sessions WHERE user_id=?")
        .bind(&actor.user.id)
        .fetch_all(&auth.store.pool)
        .await
        .map_err(db)?;
    let key = keys
        .into_iter()
        .find(|key| public_id(key) == id)
        .ok_or_else(|| failure(StatusCode::NOT_FOUND, "session_not_found"))?;
    query("DELETE FROM sessions WHERE user_id=? AND token_hash=?")
        .bind(&actor.user.id)
        .bind(key)
        .execute(&auth.store.pool)
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn revoke_all(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let actor = actor(&auth, &headers, true).await?;
    query("DELETE FROM sessions WHERE user_id=?")
        .bind(&actor.user.id)
        .execute(&auth.store.pool)
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Password {
    current_password: String,
    new_password: String,
}
async fn password(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Password>,
) -> Result<StatusCode, Error> {
    let actor = actor(&auth, &headers, true).await?;
    if input.current_password.len() > 512 {
        return Err(failure(StatusCode::BAD_REQUEST, "invalid_password"));
    }
    let allowed: Option<i64> = query_scalar("INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES(?,unixepoch(),1) ON CONFLICT(throttle_key) DO UPDATE SET window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN unixepoch() ELSE login_throttle.window_started END,attempts=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 ELSE login_throttle.attempts+1 END WHERE login_throttle.window_started<=unixepoch()-60 OR login_throttle.attempts<5 RETURNING attempts")
        .bind(format!("password:{}",actor.user.id)).fetch_optional(&auth.store.pool).await.map_err(db)?;
    if allowed.is_none() {
        return Err(failure(StatusCode::TOO_MANY_REQUESTS, "try_later"));
    }
    let encoded: String =
        query_scalar("SELECT password_hash FROM local_credentials WHERE user_id=?")
            .bind(&actor.user.id)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(db)?
            .ok_or_else(|| failure(StatusCode::CONFLICT, "local_credentials_required"))?;
    let old = encoded.clone();
    let current = Zeroizing::new(input.current_password);
    let permit = auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&old).is_ok_and(|parsed| {
            old.starts_with("$argon2id$")
                && Argon2::default()
                    .verify_password(current.as_bytes(), &parsed)
                    .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !verified {
        return Err(failure(StatusCode::FORBIDDEN, "invalid_current_password"));
    }
    let replacement = crate::admin_users::hash_password(&auth, input.new_password).await?;
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let changed = query("UPDATE local_credentials SET password_hash=?,updated_at=unixepoch() WHERE user_id=? AND password_hash=? AND EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.user_id=? AND s.expires_at>unixepoch() AND s.last_seen_at>unixepoch()-604800 AND u.disabled_at IS NULL)")
        .bind(replacement).bind(&actor.user.id).bind(encoded).bind(&actor.token_hash).bind(&actor.user.id).execute(&mut *tx).await.map_err(db)?;
    if changed.rows_affected() != 1 {
        return Err(failure(StatusCode::CONFLICT, "credentials_changed"));
    }
    query("DELETE FROM sessions WHERE user_id=?")
        .bind(&actor.user.id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
