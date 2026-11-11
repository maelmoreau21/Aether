//! Second factor for local accounts: TOTP (RFC 6238, SHA-1, 6 digits, 30 s), single-use
//! recovery codes and the "password accepted, now prove the second factor" login step.
//!
//! A password login for an account with TOTP or a passkey returns a short-lived, single-use
//! `mfaToken` instead of a session; `POST /api/v1/auth/login/mfa` exchanges it, with a TOTP
//! code, a recovery code or a passkey assertion, for the session. Only hashes of tokens and
//! recovery codes are stored. SSO accounts are excluded: their identity provider owns MFA.
use super::{
    http::issue_session,
    passkey::{self, Assertion},
    state::{AuthState, Authenticated},
    util::{constant_equal, extract_client_key, hash, random_token},
};
use crate::{
    database::{query, query_as, query_scalar},
    identity::Role,
};
use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::{Value, json};
use zeroize::Zeroizing;

pub(super) type Error = (StatusCode, Json<Value>);
pub(super) fn fail(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({ "error": code })))
}
pub(super) fn db(_: sqlx::Error) -> Error {
    fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
}

const STEP_SECONDS: u64 = 30;
const RECOVERY_CODES: usize = 10;
/// Wrong second factors accepted per `mfaToken` before it is spent.
const MFA_ATTEMPTS: i64 = 5;
const CHALLENGE_SECONDS: i64 = 300;

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route("/api/v1/auth/login/mfa", post(login_mfa))
        .route("/api/v1/account/security", get(status))
        .route("/api/v1/account/security/totp/setup", post(totp_setup))
        .route("/api/v1/account/security/totp/confirm", post(totp_confirm))
        .route("/api/v1/account/security/totp/disable", post(totp_disable))
        .route(
            "/api/v1/account/security/recovery-codes",
            post(recovery_regenerate),
        )
        .route("/api/v1/admin/users/{id}/security", delete(admin_reset))
}

// ---------------------------------------------------------------------------
// Shared helpers (also used by `passkey`)
// ---------------------------------------------------------------------------

pub(super) fn random_bytes<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0u8; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?;
    Ok(bytes)
}

/// Sliding one-minute budget per key, durable across restarts (same table as login).
pub(super) async fn throttle(state: &AuthState, key: &str, limit: i64) -> Result<(), Error> {
    let allowed: Option<i64> = query_scalar(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES(?,unixepoch(),1) \
            ON CONFLICT(throttle_key) DO UPDATE \
            SET window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 \
            THEN unixepoch() ELSE login_throttle.window_started END, attempts=CASE \
            WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 \
            ELSE login_throttle.attempts+1 END \
            WHERE login_throttle.window_started<=unixepoch()-60 OR login_throttle.attempts<? \
            RETURNING login_throttle.attempts",
    )
    .bind(key)
    .bind(limit)
    .fetch_optional(&state.store.pool)
    .await
    .map_err(db)?;
    allowed
        .map(|_| ())
        .ok_or_else(|| fail(StatusCode::TOO_MANY_REQUESTS, "try_later"))
}

pub(super) async fn actor(
    state: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, Error> {
    let actor = state
        .authenticate(headers)
        .await
        .map_err(|s| fail(s, "authentication_required"))?;
    if mutation {
        state
            .require_csrf(headers, &actor)
            .map_err(|s| fail(s, "csrf_failed"))?;
    }
    Ok(actor)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Reauth {
    pub(super) password: String,
}

/// Security changes (adding or removing a factor) need the current password, so a
/// stolen session alone can neither plant a persistent passkey nor strip the second factor.
pub(super) async fn reauth(
    state: &AuthState,
    headers: &HeaderMap,
    password: String,
) -> Result<Authenticated, Error> {
    let actor = actor(state, headers, true).await?;
    let password = Zeroizing::new(password);
    if password.len() > 512 {
        return Err(fail(StatusCode::FORBIDDEN, "invalid_current_password"));
    }
    throttle(state, &format!("password:{}", actor.user.id), 5).await?;
    let encoded: String =
        query_scalar("SELECT password_hash FROM local_credentials WHERE user_id=?")
            .bind(&actor.user.id)
            .fetch_optional(&state.store.pool)
            .await
            .map_err(db)?
            .ok_or_else(|| fail(StatusCode::CONFLICT, "local_credentials_required"))?;
    let permit = state
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| fail(StatusCode::TOO_MANY_REQUESTS, "try_later"))?;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let ok = PasswordHash::new(&encoded).is_ok_and(|parsed| {
            encoded.starts_with("$argon2id$")
                && Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
        });
        crate::system_memory::trim_memory();
        ok
    })
    .await
    .unwrap_or(false);
    if !verified {
        return Err(fail(StatusCode::FORBIDDEN, "invalid_current_password"));
    }
    Ok(actor)
}

// ---------------------------------------------------------------------------
// TOTP primitives
// ---------------------------------------------------------------------------

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            out.push(BASE32[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

pub(super) fn totp_at(secret: &[u8], counter: u64) -> String {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let tag = ring::hmac::sign(&key, &counter.to_be_bytes());
    let digest = tag.as_ref();
    let offset = usize::from(digest[digest.len() - 1] & 0x0f);
    let binary = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    format!("{:06}", binary % 1_000_000)
}

/// The time step (within one step of clock drift) whose code matches, if any.
fn matching_step(secret: &[u8], code: &str, now: u64) -> Option<u64> {
    let code = code.trim();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let current = now / STEP_SECONDS;
    let mut found = None;
    // Every candidate is computed and compared, so timing does not reveal which one matched.
    for step in [current.saturating_sub(1), current, current + 1] {
        if constant_equal(totp_at(secret, step).as_bytes(), code.as_bytes()) && found.is_none() {
            found = Some(step);
        }
    }
    found
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Checks a code against the user's confirmed seed and spends its time step, so a code
/// observed once (shoulder surfing, a proxy log) cannot be replayed within its window.
async fn verify_totp(state: &AuthState, user_id: &str, code: &str) -> Result<bool, Error> {
    let secret: Option<String> =
        query_scalar("SELECT secret FROM user_totp WHERE user_id=? AND confirmed_at IS NOT NULL")
            .bind(user_id)
            .fetch_optional(&state.store.pool)
            .await
            .map_err(db)?;
    let Some(secret) = secret.and_then(|s| unhex(&s)) else {
        return Ok(false);
    };
    let Some(step) = matching_step(&secret, code, now()) else {
        return Ok(false);
    };
    let spent = query("UPDATE user_totp SET last_step=? WHERE user_id=? AND last_step<?")
        .bind(step as i64)
        .bind(user_id)
        .bind(step as i64)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    Ok(spent.rows_affected() == 1)
}

fn normalize_recovery(code: &str) -> Option<String> {
    let code: String = code
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (code.len() == 10 && code.bytes().all(|b| BASE32.contains(&b))).then_some(code)
}

fn recovery_hash(user_id: &str, code: &str) -> String {
    hash(&format!("recovery:{user_id}:{code}"))
}

async fn issue_recovery_codes(state: &AuthState, user_id: &str) -> Result<Vec<String>, Error> {
    let mut codes = Vec::with_capacity(RECOVERY_CODES);
    for _ in 0..RECOVERY_CODES {
        let raw = base32(&random_bytes::<7>()?);
        codes.push(format!("{}-{}", &raw[..5], &raw[5..10]));
    }
    let mut conn = state.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    query("DELETE FROM user_recovery_codes WHERE user_id=?")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    for code in &codes {
        let normalized = normalize_recovery(code).unwrap_or_default();
        query(
            "INSERT INTO user_recovery_codes(user_id,code_hash) VALUES(?,?) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(recovery_hash(user_id, &normalized))
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(codes)
}

// ---------------------------------------------------------------------------
// Login: second step
// ---------------------------------------------------------------------------

/// Called once the password is verified. `None`: no second factor, open the session.
pub(super) async fn second_factor_challenge(
    state: &AuthState,
    user_id: &str,
    bearer: bool,
) -> Result<Option<Response>, Error> {
    let (totp, passkeys): (i64, i64) = query_as(
        "SELECT (SELECT COUNT(*) FROM user_totp WHERE user_id=? AND confirmed_at IS NOT NULL), (SELECT COUNT(*) FROM user_passkeys WHERE user_id=?)",
    )
    .bind(user_id)
    .bind(user_id)
    .fetch_one(&state.store.pool)
    .await
    .map_err(db)?;
    if totp == 0 && passkeys == 0 {
        return Ok(None);
    }
    let token = random_token();
    let webauthn = (passkeys > 0)
        .then(|| random_bytes::<32>().map(|b| passkey::b64(&b)))
        .transpose()?;
    query("DELETE FROM auth_challenges WHERE expires_at<=unixepoch()")
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    query("INSERT INTO auth_challenges(challenge_hash,purpose,user_id,webauthn_hash,bearer,attempts,expires_at) VALUES(?,'mfa',?,?,?,0,unixepoch()+?)")
        .bind(hash(&token))
        .bind(user_id)
        .bind(webauthn.as_deref().map(hash))
        .bind(i64::from(bearer))
        .bind(CHALLENGE_SECONDS)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    let mut methods = Vec::new();
    if totp > 0 {
        methods.extend(["totp", "recovery"]);
    }
    let mut body =
        json!({ "mfaRequired": true, "mfaToken": token, "expiresIn": CHALLENGE_SECONDS });
    if let Some(challenge) = webauthn {
        methods.push("passkey");
        body["passkey"] = passkey::request_options(state, &challenge);
    }
    body["methods"] = json!(methods);
    Ok(Some(no_store(Json(body))))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MfaInput {
    mfa_token: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    recovery_code: Option<String>,
    #[serde(default)]
    passkey: Option<Assertion>,
}

async fn login_mfa(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<MfaInput>,
) -> Result<Response, Error> {
    if !super::is_local_login_enabled() {
        return Err(fail(StatusCode::FORBIDDEN, "local_login_disabled"));
    }
    throttle(&state, &extract_client_key(&headers), 10).await?;
    if input.mfa_token.len() != 64 {
        return Err(fail(StatusCode::UNAUTHORIZED, "mfa_expired"));
    }
    let token_hash = hash(&input.mfa_token);
    let row: Option<(String, Option<String>, i64)> = query_as("UPDATE auth_challenges SET attempts=attempts+1 WHERE challenge_hash=? AND purpose='mfa' AND expires_at>unixepoch() AND attempts<? RETURNING user_id,webauthn_hash,bearer")
        .bind(&token_hash)
        .bind(MFA_ATTEMPTS)
        .fetch_optional(&state.store.pool)
        .await
        .map_err(db)?;
    let Some((user_id, webauthn_hash, bearer)) = row else {
        return Err(fail(StatusCode::UNAUTHORIZED, "mfa_expired"));
    };
    let bearer = bearer != 0;
    let origin = if bearer {
        state.check_origin_or_native(&headers)
    } else {
        state.check_origin(&headers)
    };
    origin.map_err(|s| fail(s, "forbidden"))?;

    let verified = if let Some(code) = input.code.as_deref() {
        verify_totp(&state, &user_id, code).await?
    } else if let Some(code) = input.recovery_code.as_deref() {
        match normalize_recovery(code) {
            Some(code) => {
                query("DELETE FROM user_recovery_codes WHERE user_id=? AND code_hash=?")
                    .bind(&user_id)
                    .bind(recovery_hash(&user_id, &code))
                    .execute(&state.store.pool)
                    .await
                    .map_err(db)?
                    .rows_affected()
                    == 1
            }
            None => false,
        }
    } else if let (Some(assertion), Some(expected)) = (input.passkey.as_ref(), webauthn_hash) {
        passkey::verify_assertion(
            &state,
            assertion,
            passkey::Expected::SecondFactor {
                challenge_hash: &expected,
                user_id: &user_id,
            },
        )
        .await
        .is_ok()
    } else {
        false
    };
    if !verified {
        return Err(fail(StatusCode::UNAUTHORIZED, "invalid_second_factor"));
    }
    // Spend the token; a concurrent request that also verified loses here.
    let spent = query("DELETE FROM auth_challenges WHERE challenge_hash=? AND purpose='mfa'")
        .bind(&token_hash)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    if spent.rows_affected() != 1 {
        return Err(fail(StatusCode::UNAUTHORIZED, "mfa_expired"));
    }
    Ok(issue_session(&state, &headers, &user_id, bearer).await)
}

// ---------------------------------------------------------------------------
// Account management
// ---------------------------------------------------------------------------

async fn status(State(state): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    let actor = actor(&state, &headers, false).await?;
    let id = &actor.user.id;
    let (local, totp, codes): (i64, i64, i64) = query_as(
        "SELECT (SELECT COUNT(*) FROM local_credentials WHERE user_id=?), (SELECT COUNT(*) FROM user_totp WHERE user_id=? AND confirmed_at IS NOT NULL), (SELECT COUNT(*) FROM user_recovery_codes WHERE user_id=?)",
    )
    .bind(id)
    .bind(id)
    .bind(id)
    .fetch_one(&state.store.pool)
    .await
    .map_err(db)?;
    let passkeys: Vec<(String, String, i64, Option<i64>)> = query_as(
        "SELECT credential_id,name,created_at,last_used_at FROM user_passkeys WHERE user_id=? ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.store.pool)
    .await
    .map_err(db)?;
    Ok(Json(json!({
        "localAccount": local > 0,
        "totpEnabled": totp > 0,
        "recoveryCodesLeft": codes,
        "passkeys": passkeys.into_iter().map(|(id, name, created, used)| json!({
            "id": id, "name": name, "createdAt": created, "lastUsedAt": used
        })).collect::<Vec<_>>(),
    })))
}

async fn totp_setup(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Reauth>,
) -> Result<Response, Error> {
    let actor = reauth(&state, &headers, input.password).await?;
    let secret = random_bytes::<20>()?;
    // Replaces a pending enrolment, never a confirmed one.
    let stored = query("INSERT INTO user_totp(user_id,secret,confirmed_at,last_step,created_at) VALUES(?,?,NULL,0,unixepoch()) ON CONFLICT(user_id) DO UPDATE SET secret=excluded.secret,created_at=excluded.created_at,last_step=0 WHERE user_totp.confirmed_at IS NULL")
        .bind(&actor.user.id)
        .bind(hex(&secret))
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    if stored.rows_affected() != 1 {
        return Err(fail(StatusCode::CONFLICT, "totp_already_enabled"));
    }
    let encoded = base32(&secret);
    let label: String = actor
        .user
        .login
        .bytes()
        .filter(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
        .map(char::from)
        .collect();
    let uri = format!(
        "otpauth://totp/Aether:{label}?secret={encoded}&issuer=Aether&algorithm=SHA1&digits=6&period=30"
    );
    let qr = qrcode::QrCode::new(uri.as_bytes())
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))?;
    let width = qr.width();
    let modules: Vec<bool> = qr
        .into_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Ok(no_store(Json(json!({
        "secret": encoded, "uri": uri, "qrWidth": width, "qrModules": modules
    }))))
}

/// Secrets (seed, recovery codes, tokens) must never sit in an HTTP cache.
pub(super) fn no_store(body: Json<Value>) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

#[derive(Deserialize)]
struct CodeInput {
    code: String,
}

async fn totp_confirm(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<CodeInput>,
) -> Result<Response, Error> {
    let actor = actor(&state, &headers, true).await?;
    let id = &actor.user.id;
    throttle(&state, &format!("totp:{id}"), 5).await?;
    let secret: Option<String> = query_scalar(
        "SELECT secret FROM user_totp WHERE user_id=? AND confirmed_at IS NULL AND created_at>unixepoch()-600",
    )
    .bind(id)
    .fetch_optional(&state.store.pool)
    .await
    .map_err(db)?;
    let secret = secret
        .and_then(|s| unhex(&s))
        .ok_or_else(|| fail(StatusCode::CONFLICT, "totp_setup_expired"))?;
    let step = matching_step(&secret, &input.code, now())
        .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "invalid_code"))?;
    let confirmed = query("UPDATE user_totp SET confirmed_at=unixepoch(),last_step=? WHERE user_id=? AND confirmed_at IS NULL")
        .bind(step as i64)
        .bind(id)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    if confirmed.rows_affected() != 1 {
        return Err(fail(StatusCode::CONFLICT, "totp_setup_expired"));
    }
    let codes = issue_recovery_codes(&state, id).await?;
    Ok(no_store(Json(json!({ "recoveryCodes": codes }))))
}

async fn totp_disable(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Reauth>,
) -> Result<StatusCode, Error> {
    let actor = reauth(&state, &headers, input.password).await?;
    for sql in [
        "DELETE FROM user_totp WHERE user_id=?",
        "DELETE FROM user_recovery_codes WHERE user_id=?",
    ] {
        query(sql)
            .bind(&actor.user.id)
            .execute(&state.store.pool)
            .await
            .map_err(db)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn recovery_regenerate(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Reauth>,
) -> Result<Response, Error> {
    let actor = reauth(&state, &headers, input.password).await?;
    let enabled: i64 =
        query_scalar("SELECT COUNT(*) FROM user_totp WHERE user_id=? AND confirmed_at IS NOT NULL")
            .bind(&actor.user.id)
            .fetch_one(&state.store.pool)
            .await
            .map_err(db)?;
    if enabled == 0 {
        return Err(fail(StatusCode::CONFLICT, "totp_not_enabled"));
    }
    let codes = issue_recovery_codes(&state, &actor.user.id).await?;
    Ok(no_store(Json(json!({ "recoveryCodes": codes }))))
}

/// Lost device: an administrator removes every second factor and passkey of an account.
/// The user's sessions end too, so whoever held the lost device is signed out.
async fn admin_reset(
    State(state): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let actor = actor(&state, &headers, true).await?;
    if actor.user.role != Role::Administrator {
        return Err(fail(StatusCode::FORBIDDEN, "admin_required"));
    }
    let mut conn = state.store.pool.acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    for sql in [
        "DELETE FROM user_totp WHERE user_id=?",
        "DELETE FROM user_recovery_codes WHERE user_id=?",
        "DELETE FROM user_passkeys WHERE user_id=?",
        "DELETE FROM auth_challenges WHERE user_id=?",
    ] {
        query(sql).bind(&id).execute(&mut *tx).await.map_err(db)?;
    }
    if id != actor.user.id {
        query("DELETE FROM sessions WHERE user_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn rfc6238_sha1_vectors() {
        // RFC 6238 appendix B, truncated to 6 digits.
        let secret = b"12345678901234567890";
        assert_eq!(totp_at(secret, 59 / 30), "287082");
        assert_eq!(totp_at(secret, 1_111_111_109 / 30), "081804");
        assert_eq!(totp_at(secret, 1_234_567_890 / 30), "005924");
        assert_eq!(totp_at(secret, 2_000_000_000 / 30), "279037");
    }

    #[test]
    fn drift_window_and_format() {
        let secret = b"12345678901234567890";
        let code = totp_at(secret, 1000);
        assert_eq!(matching_step(secret, &code, 1000 * 30 + 29), Some(1000));
        assert_eq!(matching_step(secret, &code, 1001 * 30), Some(1000));
        assert_eq!(matching_step(secret, &code, 1002 * 30), None);
        assert_eq!(matching_step(secret, "12345", 1000 * 30), None);
        assert_eq!(matching_step(secret, "abcdef", 1000 * 30), None);
    }

    #[test]
    fn base32_matches_rfc4648() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(
            base32(b"12345678901234567890"),
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        );
        assert_eq!(
            unhex(&hex(b"\x00\xffa")).as_deref(),
            Some(&b"\x00\xffa"[..])
        );
    }

    #[test]
    fn recovery_normalization() {
        assert_eq!(
            normalize_recovery("abcde-fgh23").as_deref(),
            Some("ABCDEFGH23")
        );
        assert_eq!(normalize_recovery("ABCDE FGH2"), None);
        assert_eq!(normalize_recovery("ABCDE-FGH21"), None);
    }
}
