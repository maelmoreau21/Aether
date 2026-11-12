//! "Who's watching?" profiles inside one account, each with an optional PIN.
//!
//! A profile is a full user without credentials, attached to the account that
//! created it (`account_profiles.owner_id`). Progress, ratings, preferences,
//! recommendations and parental rules therefore stay per profile with no extra
//! code, and library access is inherited from the owner by
//! `effective_library_permissions`. Switching opens a new session for the
//! chosen profile and closes the current one; a PIN, when set, is checked
//! against a durable per-profile throttle.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

/// Profiles per account, the owner excluded.
const MAX_PROFILES: i64 = 8;
/// PIN attempts per profile and minute, shared by every device.
const PIN_ATTEMPTS: i64 = 5;

type Error = (StatusCode, Json<Value>);
fn failure(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error": code})))
}
fn db(_: sqlx::Error) -> Error {
    failure(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/account/profiles", get(list).post(create))
        .route("/api/v1/account/profiles/{id}", delete(remove))
        .route("/api/v1/account/profiles/{id}/pin", put(set_pin))
        .route("/api/v1/account/profiles/{id}/switch", post(switch))
        .layer(DefaultBodyLimit::max(4096))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}

async fn actor(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let identity = auth
        .authenticate(headers)
        .await
        .map_err(|s| failure(s, "authentication_required"))?;
    if mutation {
        auth.require_csrf(headers, &identity)
            .map_err(|s| failure(s, "csrf_failed"))?;
    }
    Ok(identity)
}

/// The account a user belongs to: its owner, or the user itself.
async fn owner_of(auth: &AuthState, user: &str) -> Result<String, Error> {
    let owner: Option<String> =
        query_scalar("SELECT owner_id FROM account_profiles WHERE user_id=?")
            .bind(user)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(db)?;
    Ok(owner.unwrap_or_else(|| user.to_owned()))
}

/// Whether `target` is the owner itself or one of its active profiles.
async fn in_account(auth: &AuthState, owner: &str, target: &str) -> Result<bool, Error> {
    let found: i64 = query_scalar(
        "SELECT COUNT(*) FROM users u WHERE u.id=? AND u.disabled_at IS NULL \
         AND (u.id=? OR EXISTS(SELECT 1 FROM account_profiles p WHERE p.user_id=u.id AND p.owner_id=?))",
    )
    .bind(target)
    .bind(owner)
    .bind(owner)
    .fetch_one(&auth.store.pool)
    .await
    .map_err(db)?;
    Ok(found > 0)
}

fn valid_name(name: &str) -> bool {
    let count = name.chars().count();
    (1..=40).contains(&count) && name.trim() == name && !name.chars().any(char::is_control)
}

fn valid_pin(pin: &str) -> bool {
    (4..=6).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

async fn list(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let identity = actor(&auth, &headers, false).await?;
    let current = identity.user.id;
    let owner = owner_of(&auth, &current).await?;
    let rows: Vec<(String, String, i64, i64)> = query_as(
        "SELECT u.id, COALESCE(u.display_name, c.login, u.sso_username, u.id) AS name, \
           CAST(CASE WHEN p.pin_hash IS NOT NULL THEN 1 ELSE 0 END AS BIGINT) AS has_pin, \
           CAST(CASE WHEN EXISTS(SELECT 1 FROM parental_policies pp WHERE pp.user_id=u.id) THEN 1 ELSE 0 END AS BIGINT) AS restricted \
         FROM users u \
         LEFT JOIN account_profiles p ON p.user_id=u.id \
         LEFT JOIN local_credentials c ON c.user_id=u.id \
         WHERE u.disabled_at IS NULL AND (u.id=? OR p.owner_id=?) \
         ORDER BY CASE WHEN u.id=? THEN 0 ELSE 1 END, COALESCE(p.created_at,0), u.id",
    )
    .bind(&owner)
    .bind(&owner)
    .bind(&owner)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(db)?;
    let profiles: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, has_pin, restricted)| {
            json!({
                "id": id,
                "name": name,
                "hasPin": has_pin != 0,
                "restricted": restricted != 0,
                "isOwner": id == owner,
                "isCurrent": id == current,
            })
        })
        .collect();
    Ok(Json(json!({
        "ownerId": owner,
        "currentId": current,
        "canManage": owner == current,
        "maxProfiles": MAX_PROFILES,
        "profiles": profiles,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NewProfile {
    name: String,
    /// Creates a restricted profile (e.g. for children) with this maximum age rating.
    #[serde(default)]
    maximum_age: Option<i64>,
}

async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<NewProfile>,
) -> Result<Response, Error> {
    let identity = actor(&auth, &headers, true).await?;
    let owner = identity.user.id;
    if owner_of(&auth, &owner).await? != owner {
        return Err(failure(StatusCode::FORBIDDEN, "owner_only"));
    }
    if !valid_name(&input.name) {
        return Err(failure(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    if input
        .maximum_age
        .is_some_and(|age| !(0..=21).contains(&age))
    {
        return Err(failure(StatusCode::BAD_REQUEST, "invalid_maximum_age"));
    }
    let mut conn = auth.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let count: i64 = query_scalar(
        "SELECT COUNT(*) FROM account_profiles WHERE owner_id=? AND user_id<>owner_id",
    )
    .bind(&owner)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if count >= MAX_PROFILES {
        return Err(failure(StatusCode::CONFLICT, "profile_limit"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO users(id,role,display_name) VALUES(?,'user',?)")
        .bind(&id)
        .bind(&input.name)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    query("INSERT INTO account_profiles(user_id,owner_id,created_at) VALUES(?,?,unixepoch())")
        .bind(&id)
        .bind(&owner)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    if let Some(age) = input.maximum_age {
        query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated,blocked_tags_json,access_schedule_json) VALUES(?,?,0,'[]','[]')")
            .bind(&id)
            .bind(age)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok((StatusCode::CREATED, Json(json!({"id": id}))).into_response())
}

async fn remove(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let identity = actor(&auth, &headers, true).await?;
    let owner = identity.user.id;
    if owner_of(&auth, &owner).await? != owner {
        return Err(failure(StatusCode::FORBIDDEN, "owner_only"));
    }
    // Only a profile of this account is deleted, never the owner or another account.
    let deleted = query(
        "DELETE FROM users WHERE id=? AND id IN (SELECT user_id FROM account_profiles WHERE owner_id=? AND user_id<>owner_id)",
    )
    .bind(&id)
    .bind(&owner)
    .execute(&auth.store.pool)
    .await
    .map_err(db)?;
    if deleted.rows_affected() == 0 {
        return Err(failure(StatusCode::NOT_FOUND, "profile_not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PinInput {
    /// New PIN (4 to 6 digits), or null to remove it.
    pin: Option<String>,
    /// Required when a profile changes its own existing PIN.
    #[serde(default)]
    current_pin: Option<String>,
}

async fn set_pin(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<PinInput>,
) -> Result<StatusCode, Error> {
    let identity = actor(&auth, &headers, true).await?;
    let current = identity.user.id;
    let owner = owner_of(&auth, &current).await?;
    if !in_account(&auth, &owner, &id).await? {
        return Err(failure(StatusCode::NOT_FOUND, "profile_not_found"));
    }
    // The owner manages every PIN; a profile only its own, proving the old one.
    if current != owner {
        if current != id {
            return Err(failure(StatusCode::FORBIDDEN, "owner_only"));
        }
        if let Some(encoded) = pin_hash(&auth, &id).await? {
            let given = input.current_pin.clone().unwrap_or_default();
            throttle(&auth, &id).await?;
            if !verify_pin(&auth, encoded, given).await? {
                return Err(failure(StatusCode::FORBIDDEN, "invalid_pin"));
            }
        }
    }
    match input.pin {
        Some(pin) => {
            if !valid_pin(&pin) {
                return Err(failure(StatusCode::BAD_REQUEST, "invalid_pin_format"));
            }
            let encoded = hash_pin(&auth, pin).await?;
            query("INSERT INTO account_profiles(user_id,owner_id,pin_hash,created_at) VALUES(?,?,?,unixepoch()) ON CONFLICT(user_id) DO UPDATE SET pin_hash=excluded.pin_hash")
                .bind(&id)
                .bind(&owner)
                .bind(encoded)
                .execute(&auth.store.pool)
                .await
                .map_err(db)?;
        }
        None => {
            query("UPDATE account_profiles SET pin_hash=NULL WHERE user_id=?")
                .bind(&id)
                .execute(&auth.store.pool)
                .await
                .map_err(db)?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SwitchInput {
    #[serde(default)]
    pin: Option<String>,
}

async fn switch(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<SwitchInput>,
) -> Result<Response, Error> {
    let identity = actor(&auth, &headers, true).await?;
    let owner = owner_of(&auth, &identity.user.id).await?;
    if !in_account(&auth, &owner, &id).await? {
        return Err(failure(StatusCode::NOT_FOUND, "profile_not_found"));
    }
    if let Some(encoded) = pin_hash(&auth, &id).await? {
        throttle(&auth, &id).await?;
        let given = input.pin.unwrap_or_default();
        if !valid_pin(&given) || !verify_pin(&auth, encoded, given).await? {
            return Err(failure(StatusCode::FORBIDDEN, "invalid_pin"));
        }
    }
    let response = crate::auth::issue_session(&auth, &headers, &id, identity.bearer).await;
    if response.status().is_success() {
        // The previous profile's session ends; a native app keeps it a minute for requests in flight.
        let closed = if identity.bearer {
            query(
                "UPDATE sessions SET expires_at=CASE WHEN expires_at>unixepoch()+60 THEN unixepoch()+60 ELSE expires_at END WHERE token_hash=?",
            )
        } else {
            query("DELETE FROM sessions WHERE token_hash=?")
        };
        closed
            .bind(&identity.token_hash)
            .execute(&auth.store.pool)
            .await
            .map_err(db)?;
    }
    Ok(response)
}

/// Whether switching to this account on a shared device asks for its PIN.
pub(crate) async fn has_pin(auth: &AuthState, id: &str) -> bool {
    matches!(pin_hash(auth, id).await, Ok(Some(_)))
}

/// Checks the short PIN guarding a switch to `id` among the accounts of one device.
/// An account without a PIN switches as before; the budget is the per-account one
/// shared by every device.
pub(crate) async fn check_switch_pin(
    auth: &AuthState,
    id: &str,
    pin: Option<String>,
) -> Result<(), Error> {
    let Some(encoded) = pin_hash(auth, id).await? else {
        return Ok(());
    };
    let Some(given) = pin.filter(|pin| !pin.is_empty()) else {
        return Err(failure(StatusCode::FORBIDDEN, "pin_required"));
    };
    throttle(auth, id).await?;
    if !valid_pin(&given) || !verify_pin(auth, encoded, given).await? {
        return Err(failure(StatusCode::FORBIDDEN, "invalid_pin"));
    }
    Ok(())
}

async fn pin_hash(auth: &AuthState, id: &str) -> Result<Option<String>, Error> {
    let encoded: Option<Option<String>> =
        query_scalar("SELECT pin_hash FROM account_profiles WHERE user_id=?")
            .bind(id)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(db)?;
    Ok(encoded.flatten())
}

/// Durable per-profile budget, so a 4-digit PIN cannot be brute-forced from many devices.
async fn throttle(auth: &AuthState, id: &str) -> Result<(), Error> {
    let allowed: Option<i64> = query_scalar(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) \
                VALUES(?,unixepoch(),1) ON CONFLICT(throttle_key) DO UPDATE \
                SET window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 \
                THEN unixepoch() ELSE login_throttle.window_started END, attempts=CASE \
                WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 \
                ELSE login_throttle.attempts+1 END \
                WHERE login_throttle.window_started<=unixepoch()-60 OR login_throttle.attempts<? \
                RETURNING login_throttle.attempts",
    )
    .bind(format!("profile-pin:{id}"))
    .bind(PIN_ATTEMPTS)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(db)?;
    allowed
        .map(|_| ())
        .ok_or_else(|| failure(StatusCode::TOO_MANY_REQUESTS, "try_later"))
}

pub(crate) async fn hash_pin(auth: &AuthState, pin: String) -> Result<String, Error> {
    let pin = Zeroizing::new(pin);
    let permit = auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let params = Params::new(19456, 2, 1, Some(32)).map_err(|_| ())?;
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(pin.as_bytes())
            .map(|v| v.to_string())
            .map_err(|_| ())
    })
    .await
    .map_err(|_| failure(StatusCode::INTERNAL_SERVER_ERROR, "pin_hash_failed"))?
    .map_err(|_| failure(StatusCode::INTERNAL_SERVER_ERROR, "pin_hash_failed"))
}

async fn verify_pin(auth: &AuthState, encoded: String, pin: String) -> Result<bool, Error> {
    let pin = Zeroizing::new(pin);
    let permit = auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&encoded).is_ok_and(|parsed| {
            Argon2::default()
                .verify_password(pin.as_bytes(), &parsed)
                .is_ok()
        })
    })
    .await
    .map_err(|_| failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))
}

#[cfg(test)]
#[path = "profiles_tests.rs"]
mod tests;
