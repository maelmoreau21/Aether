//! Passkeys (WebAuthn level 2) for local accounts: registration, passwordless sign-in and
//! use as a second factor. No attestation is requested (`"none"`): the browser of an already
//! authenticated, re-authenticated user hands over the public key (SPKI, from
//! `getPublicKey()`), and every later sign-in proves possession of the private key with a
//! signature over the authenticator data and the client data, checked here with `ring`.
//!
//! Checked on every ceremony: ceremony type, single-use server challenge (hash only stored),
//! origin, RP ID hash (host of the public origin), user presence and user verification,
//! credential ownership, signature and (when the authenticator keeps one) the sign counter.
//! Supported algorithms: ES256 (-7), Ed25519 (-8), RS256 (-257).
use super::{
    http::issue_session,
    mfa::{Error, Reauth, actor, db, fail, no_store, random_bytes, reauth, throttle},
    state::AuthState,
    util::{TokenType, constant_equal, extract_client_key, hash},
};
use crate::database::{query, query_as, query_scalar};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{delete, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{
    ECDSA_P256_SHA256_ASN1, ED25519, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey,
    VerificationAlgorithm,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const TIMEOUT_MS: i64 = 300_000;
const CHALLENGE_SECONDS: i64 = 300;
const MAX_PASSKEYS: i64 = 20;
const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_AT: u8 = 0x40;

pub(super) fn router() -> Router<super::AuthState> {
    Router::new()
        .route("/api/v1/auth/passkey/options", post(login_options))
        .route("/api/v1/auth/passkey/login", post(login))
        .route(
            "/api/v1/account/security/passkeys/options",
            post(register_options),
        )
        .route("/api/v1/account/security/passkeys", post(register))
        .route("/api/v1/account/security/passkeys/{id}", delete(remove))
}

pub(super) fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}
fn unb64(text: &str, max: usize) -> Result<Vec<u8>, Error> {
    if text.len() > max.div_ceil(3) * 4 {
        return Err(fail(StatusCode::BAD_REQUEST, "invalid_passkey"));
    }
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid_passkey"))
}

/// RP ID: the host of the public origin. WebAuthn refuses IP addresses as RP IDs, so
/// passkeys need the server to be reached by name (localhost included).
fn rp_id(state: &AuthState) -> Result<String, Error> {
    url::Url::parse(state.origin())
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .ok_or_else(|| fail(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"))
}

/// `PublicKeyCredentialRequestOptions` (JSON form) for a challenge.
pub(super) fn request_options(state: &AuthState, challenge: &str) -> Value {
    json!({
        "challenge": challenge,
        "rpId": rp_id(state).unwrap_or_default(),
        "timeout": TIMEOUT_MS,
        "userVerification": "required",
    })
}

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    #[serde(default, rename = "crossOrigin")]
    cross_origin: Option<bool>,
}

/// Parses and checks `clientDataJSON`; returns the challenge it signs.
fn client_data(state: &AuthState, raw: &[u8], kind: &str) -> Result<String, Error> {
    let data: ClientData = serde_json::from_slice(raw)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid_passkey"))?;
    if data.kind != kind
        || data.cross_origin == Some(true)
        || !state.is_origin_allowed(&data.origin)
    {
        return Err(fail(StatusCode::UNAUTHORIZED, "invalid_passkey"));
    }
    Ok(data.challenge)
}

/// Checks RP ID hash and the user-present + user-verified flags; returns (flags, counter).
fn authenticator_data(state: &AuthState, data: &[u8]) -> Result<(u8, u32), Error> {
    if data.len() < 37 {
        return Err(fail(StatusCode::BAD_REQUEST, "invalid_passkey"));
    }
    let expected = Sha256::digest(rp_id(state)?.as_bytes());
    let flags = data[32];
    if !constant_equal(&data[..32], &expected) || flags & FLAG_UP == 0 || flags & FLAG_UV == 0 {
        return Err(fail(StatusCode::UNAUTHORIZED, "invalid_passkey"));
    }
    Ok((
        flags,
        u32::from_be_bytes([data[33], data[34], data[35], data[36]]),
    ))
}

fn algorithm(alg: i64) -> Option<&'static dyn VerificationAlgorithm> {
    match alg {
        -7 => Some(&ECDSA_P256_SHA256_ASN1),
        -8 => Some(&ED25519),
        -257 => Some(&RSA_PKCS1_2048_8192_SHA256),
        _ => None,
    }
}

/// One DER TLV with a short or two-byte long length: (tag, content, rest).
fn der(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, mut rest) = rest.split_first()?;
    let len = if first < 0x80 {
        usize::from(first)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 2 || rest.len() < n {
            return None;
        }
        let len = rest[..n]
            .iter()
            .fold(0usize, |l, &b| (l << 8) | usize::from(b));
        rest = &rest[n..];
        len
    };
    (rest.len() >= len).then(|| (tag, &rest[..len], &rest[len..]))
}

/// The key bytes `ring` expects, out of a SubjectPublicKeyInfo: the uncompressed point
/// (P-256), the 32-byte key (Ed25519) or the PKCS#1 `RSAPublicKey` (RSA).
fn spki_key(spki: &[u8], alg: i64) -> Option<Vec<u8>> {
    let (0x30, info, []) = der(spki)? else {
        return None;
    };
    let (0x30, _, rest) = der(info)? else {
        return None;
    };
    let (0x03, bits, []) = der(rest)? else {
        return None;
    };
    let (&0, key) = bits.split_first()? else {
        return None;
    };
    let fits = match alg {
        -7 => key.len() == 65 && key[0] == 0x04,
        -8 => key.len() == 32,
        -257 => key.first() == Some(&0x30) && (200..=1100).contains(&key.len()),
        _ => false,
    };
    fits.then(|| key.to_vec())
}

// ---------------------------------------------------------------------------
// Assertion (sign-in, second factor)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Assertion {
    credential_id: String,
    client_data_json: String,
    authenticator_data: String,
    signature: String,
    #[serde(default)]
    user_handle: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum Expected<'a> {
    /// Passwordless sign-in: the challenge row is consumed here.
    Login,
    /// After a password: the challenge was issued with the `mfaToken`, for this user only.
    SecondFactor {
        challenge_hash: &'a str,
        user_id: &'a str,
    },
}

/// Verifies an assertion and returns the user it authenticates.
pub(super) async fn verify_assertion(
    state: &AuthState,
    assertion: &Assertion,
    expected: Expected<'_>,
) -> Result<String, Error> {
    let denied = || fail(StatusCode::UNAUTHORIZED, "invalid_passkey");
    let client_json = unb64(&assertion.client_data_json, 1024)?;
    let auth_data = unb64(&assertion.authenticator_data, 512)?;
    let signature = unb64(&assertion.signature, 1024)?;
    let credential = b64(&unb64(&assertion.credential_id, 1023)?);
    let challenge = client_data(state, &client_json, "webauthn.get")?;
    // The challenge is spent before any other check, so a failed attempt cannot be retried.
    match expected {
        Expected::Login => {
            let spent = query("DELETE FROM auth_challenges WHERE challenge_hash=? AND purpose='passkey_login' AND expires_at>unixepoch()")
                .bind(hash(&challenge))
                .execute(&state.store.pool)
                .await
                .map_err(db)?;
            if spent.rows_affected() != 1 {
                return Err(denied());
            }
        }
        Expected::SecondFactor { challenge_hash, .. } => {
            if !constant_equal(hash(&challenge).as_bytes(), challenge_hash.as_bytes()) {
                return Err(denied());
            }
        }
    }
    let (_, counter) = authenticator_data(state, &auth_data)?;
    let row: Option<(String, i64, String, i64)> = query_as(
        "SELECT p.user_id,p.algorithm,p.public_key,p.sign_count FROM user_passkeys p JOIN users u ON u.id=p.user_id JOIN local_credentials c ON c.user_id=u.id WHERE p.credential_id=? AND u.disabled_at IS NULL",
    )
    .bind(&credential)
    .fetch_optional(&state.store.pool)
    .await
    .map_err(db)?;
    let (user_id, alg, public_key, stored) = row.ok_or_else(denied)?;
    if let Expected::SecondFactor { user_id: owner, .. } = expected
        && owner != user_id
    {
        return Err(denied());
    }
    if let Some(handle) = assertion.user_handle.as_deref().filter(|h| !h.is_empty())
        && unb64(handle, 64)? != user_id.as_bytes()
    {
        return Err(denied());
    }
    let key = unb64(&public_key, 1100).map_err(|_| denied())?;
    let mut signed = auth_data.clone();
    signed.extend_from_slice(&Sha256::digest(&client_json));
    let verifier = algorithm(alg).ok_or_else(denied)?;
    UnparsedPublicKey::new(verifier, &key)
        .verify(&signed, &signature)
        .map_err(|_| denied())?;
    // A counter that does not move forward means a cloned authenticator. Synced passkeys
    // report 0 forever, which is accepted as long as the stored value is 0 too.
    let counter = i64::from(counter);
    if (counter != 0 || stored != 0) && counter <= stored {
        return Err(fail(StatusCode::UNAUTHORIZED, "passkey_counter"));
    }
    query("UPDATE user_passkeys SET sign_count=?,last_used_at=unixepoch() WHERE credential_id=?")
        .bind(counter)
        .bind(&credential)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    Ok(user_id)
}

async fn login_options(
    State(state): State<AuthState>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    if !super::is_local_login_enabled() {
        return Err(fail(StatusCode::FORBIDDEN, "local_login_disabled"));
    }
    state
        .check_origin(&headers)
        .map_err(|s| fail(s, "forbidden"))?;
    throttle(&state, &extract_client_key(&headers), 10).await?;
    let challenge = b64(&random_bytes::<32>()?);
    query("DELETE FROM auth_challenges WHERE expires_at<=unixepoch()")
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    let created = query("INSERT INTO auth_challenges(challenge_hash,purpose,user_id,webauthn_hash,bearer,attempts,expires_at) SELECT ?,'passkey_login',NULL,NULL,0,0,unixepoch()+? WHERE (SELECT COUNT(*) FROM auth_challenges WHERE purpose='passkey_login')<1024")
        .bind(hash(&challenge))
        .bind(CHALLENGE_SECONDS)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    if created.rows_affected() != 1 {
        return Err(fail(StatusCode::TOO_MANY_REQUESTS, "try_later"));
    }
    Ok(no_store(Json(request_options(&state, &challenge))))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginInput {
    #[serde(flatten)]
    assertion: Assertion,
    #[serde(default)]
    token_type: TokenType,
}

async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> Result<Response, Error> {
    if !super::is_local_login_enabled() {
        return Err(fail(StatusCode::FORBIDDEN, "local_login_disabled"));
    }
    let bearer = input.token_type == TokenType::Bearer;
    let origin = if bearer {
        state.check_origin_or_native(&headers)
    } else {
        state.check_origin(&headers)
    };
    origin.map_err(|s| fail(s, "forbidden"))?;
    throttle(&state, &extract_client_key(&headers), 10).await?;
    let user_id = verify_assertion(&state, &input.assertion, Expected::Login).await?;
    // A passkey with user verification is itself multi-factor (possession + PIN/biometrics).
    Ok(issue_session(&state, &headers, &user_id, bearer).await)
}

// ---------------------------------------------------------------------------
// Registration and management
// ---------------------------------------------------------------------------

async fn register_options(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Reauth>,
) -> Result<Response, Error> {
    let actor = reauth(&state, &headers, input.password).await?;
    let id = &actor.user.id;
    let existing: Vec<(String,)> =
        query_as("SELECT credential_id FROM user_passkeys WHERE user_id=?")
            .bind(id)
            .fetch_all(&state.store.pool)
            .await
            .map_err(db)?;
    if existing.len() as i64 >= MAX_PASSKEYS {
        return Err(fail(StatusCode::CONFLICT, "passkey_limit"));
    }
    let challenge = b64(&random_bytes::<32>()?);
    query("DELETE FROM auth_challenges WHERE expires_at<=unixepoch() OR (purpose='passkey_register' AND user_id=?)")
        .bind(id)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    query("INSERT INTO auth_challenges(challenge_hash,purpose,user_id,webauthn_hash,bearer,attempts,expires_at) VALUES(?,'passkey_register',?,NULL,0,0,unixepoch()+?)")
        .bind(hash(&challenge))
        .bind(id)
        .bind(CHALLENGE_SECONDS)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    let name = actor.user.login.clone();
    let display = actor
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| name.clone());
    Ok(no_store(Json(json!({
        "challenge": challenge,
        "rp": { "id": rp_id(&state)?, "name": "Aether" },
        "user": { "id": b64(id.as_bytes()), "name": name, "displayName": display },
        "pubKeyCredParams": [
            { "type": "public-key", "alg": -7 },
            { "type": "public-key", "alg": -8 },
            { "type": "public-key", "alg": -257 }
        ],
        "timeout": TIMEOUT_MS,
        "attestation": "none",
        "authenticatorSelection": {
            "residentKey": "required",
            "requireResidentKey": true,
            "userVerification": "required"
        },
        "excludeCredentials": existing.into_iter().map(|(id,)| json!({ "type": "public-key", "id": id })).collect::<Vec<_>>(),
    }))))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterInput {
    #[serde(default)]
    name: Option<String>,
    credential_id: String,
    client_data_json: String,
    authenticator_data: String,
    public_key: String,
    public_key_algorithm: i64,
}

async fn register(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<RegisterInput>,
) -> Result<Json<Value>, Error> {
    let actor = actor(&state, &headers, true).await?;
    let id = &actor.user.id;
    let client_json = unb64(&input.client_data_json, 1024)?;
    let auth_data = unb64(&input.authenticator_data, 2048)?;
    let raw_id = unb64(&input.credential_id, 1023)?;
    let spki = unb64(&input.public_key, 1100)?;
    let challenge = client_data(&state, &client_json, "webauthn.create")?;
    let spent = query("DELETE FROM auth_challenges WHERE challenge_hash=? AND purpose='passkey_register' AND user_id=? AND expires_at>unixepoch()")
        .bind(hash(&challenge))
        .bind(id)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    if spent.rows_affected() != 1 {
        return Err(fail(StatusCode::UNAUTHORIZED, "invalid_passkey"));
    }
    let (flags, counter) = authenticator_data(&state, &auth_data)?;
    // Attested credential data: AAGUID (16) | id length (2) | credential id | COSE key.
    let id_len = auth_data
        .get(53..55)
        .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])));
    let attested = id_len.and_then(|len| auth_data.get(55..55 + len));
    if flags & FLAG_AT == 0
        || raw_id.is_empty()
        || attested.is_none_or(|attested| !constant_equal(attested, &raw_id))
    {
        return Err(fail(StatusCode::BAD_REQUEST, "invalid_passkey"));
    }
    if algorithm(input.public_key_algorithm).is_none() {
        return Err(fail(StatusCode::BAD_REQUEST, "unsupported_algorithm"));
    }
    let key = spki_key(&spki, input.public_key_algorithm)
        .ok_or_else(|| fail(StatusCode::BAD_REQUEST, "invalid_passkey"))?;
    let name: String = input
        .name
        .as_deref()
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    let name = if name.is_empty() {
        "Passkey".to_owned()
    } else {
        name
    };
    let credential = b64(&raw_id);
    let inserted = query("INSERT INTO user_passkeys(credential_id,user_id,name,algorithm,public_key,sign_count,created_at) SELECT ?,?,?,?,?,?,unixepoch() WHERE (SELECT COUNT(*) FROM user_passkeys WHERE user_id=?)<? AND EXISTS(SELECT 1 FROM local_credentials WHERE user_id=?) ON CONFLICT(credential_id) DO NOTHING")
        .bind(&credential)
        .bind(id)
        .bind(&name)
        .bind(input.public_key_algorithm)
        .bind(b64(&key))
        .bind(i64::from(counter))
        .bind(id)
        .bind(MAX_PASSKEYS)
        .bind(id)
        .execute(&state.store.pool)
        .await
        .map_err(db)?;
    if inserted.rows_affected() != 1 {
        return Err(fail(StatusCode::CONFLICT, "passkey_rejected"));
    }
    Ok(Json(json!({ "id": credential, "name": name })))
}

async fn remove(
    State(state): State<AuthState>,
    Path(credential): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Reauth>,
) -> Result<StatusCode, Error> {
    let actor = reauth(&state, &headers, input.password).await?;
    let removed: Option<String> = query_scalar(
        "DELETE FROM user_passkeys WHERE credential_id=? AND user_id=? RETURNING credential_id",
    )
    .bind(&credential)
    .bind(&actor.user.id)
    .fetch_optional(&state.store.pool)
    .await
    .map_err(db)?;
    removed
        .map(|_| StatusCode::NO_CONTENT)
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "passkey_not_found"))
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn spki_extracts_ring_keys() {
        // Ed25519 SPKI: SEQUENCE { SEQUENCE { OID 1.3.101.112 }, BIT STRING (0 unused) key }.
        let mut ed = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        ed.extend([7u8; 32]);
        assert_eq!(spki_key(&ed, -8), Some(vec![7u8; 32]));
        assert_eq!(spki_key(&ed, -7), None);
        let mut trailing = ed.clone();
        trailing.push(0);
        assert_eq!(spki_key(&trailing, -8), None);
        assert_eq!(spki_key(&ed[..20], -8), None);
    }

    #[test]
    fn es256_signature_round_trip() {
        use ring::{
            rand::SystemRandom,
            signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair},
        };
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        // P-256 SPKI prefix (id-ecPublicKey, prime256v1) followed by the uncompressed point.
        let mut spki = vec![
            0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06,
            0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
        ];
        spki.extend_from_slice(pair.public_key().as_ref());
        let key = spki_key(&spki, -7).unwrap();
        let signature = pair.sign(&rng, b"message").unwrap();
        let verifier = algorithm(-7).unwrap();
        assert!(
            UnparsedPublicKey::new(verifier, &key)
                .verify(b"message", signature.as_ref())
                .is_ok()
        );
        assert!(
            UnparsedPublicKey::new(verifier, &key)
                .verify(b"other", signature.as_ref())
                .is_err()
        );
    }
}
