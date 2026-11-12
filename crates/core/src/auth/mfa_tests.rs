//! End-to-end second factor and passkey ceremonies against the real router and SQLite.
use super::{http::router, mfa, passkey::b64, state::AuthState};
use crate::identity::IdentityStore;
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use ring::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use zeroize::Zeroizing;

const ORIGIN: &str = "http://localhost:3000";
const PASSWORD: &str = "synthetic mfa test password";

async fn setup() -> (tempfile::TempDir, AuthState) {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("auth.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    (temp, AuthState::new(store, ORIGIN.into()).await.unwrap())
}

async fn call(
    state: &AuthState,
    method: &str,
    path: &str,
    session: Option<&(String, String)>,
    ip: &str,
    body: Value,
) -> (StatusCode, Option<String>, Value) {
    let mut req = Request::builder()
        .uri(path)
        .method(method)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, ORIGIN)
        .header("x-forwarded-for", ip);
    if let Some((cookie, csrf)) = session {
        req = req
            .header(header::COOKIE, cookie)
            .header("x-csrf-token", csrf);
    }
    let response: Response = router(state.clone())
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        cookie,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn password_login(state: &AuthState, ip: &str) -> (StatusCode, Option<String>, Value) {
    call(
        state,
        "POST",
        "/api/v1/auth/login",
        None,
        ip,
        json!({"login":"admin","password":PASSWORD}),
    )
    .await
}

async fn current_code(state: &AuthState) -> String {
    let secret: String = crate::database::query_scalar("SELECT secret FROM user_totp")
        .fetch_one(&state.store.pool)
        .await
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    mfa::totp_at(&mfa::unhex(&secret).unwrap(), now / 30)
}

fn client_data(kind: &str, challenge: &str) -> Vec<u8> {
    json!({"type": kind, "challenge": challenge, "origin": ORIGIN, "crossOrigin": false})
        .to_string()
        .into_bytes()
}

#[tokio::test]
async fn totp_recovery_and_passkey_ceremonies() {
    let (_temp, state) = setup().await;
    let (status, cookie, body) = password_login(&state, "10.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    let session = (
        cookie.unwrap(),
        body["csrfToken"].as_str().unwrap().to_owned(),
    );
    let user_id = body["user"]["id"].as_str().unwrap().to_owned();

    // Enrolment needs the password, then a valid code; it yields ten recovery codes.
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/account/security/totp/setup",
        Some(&session),
        "10.0.0.1",
        json!({"password":"wrong"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, setup) = call(
        &state,
        "POST",
        "/api/v1/account/security/totp/setup",
        Some(&session),
        "10.0.0.1",
        json!({"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        setup["uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/Aether:admin?secret=")
    );
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/account/security/totp/confirm",
        Some(&session),
        "10.0.0.1",
        json!({"code":"000000"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let code = current_code(&state).await;
    let (status, _, confirmed) = call(
        &state,
        "POST",
        "/api/v1/account/security/totp/confirm",
        Some(&session),
        "10.0.0.1",
        json!({"code":code}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let recovery: Vec<String> = serde_json::from_value(confirmed["recoveryCodes"].clone()).unwrap();
    assert_eq!(recovery.len(), 10);

    // Password alone no longer opens a session.
    let (status, cookie, challenge) = password_login(&state, "10.0.0.2").await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_none());
    assert_eq!(challenge["mfaRequired"], true);
    let token = challenge["mfaToken"].as_str().unwrap().to_owned();
    // The code spent by the enrolment cannot be replayed.
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.2",
        json!({"mfaToken":token,"code":code}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, cookie, _) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.2",
        json!({"mfaToken":token,"recoveryCode":recovery[0].to_lowercase()}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_some());
    // The token and the recovery code are both single-use.
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.2",
        json!({"mfaToken":token,"recoveryCode":recovery[1]}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, _, again) = password_login(&state, "10.0.0.3").await;
    let token = again["mfaToken"].as_str().unwrap().to_owned();
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.3",
        json!({"mfaToken":token,"recoveryCode":recovery[0]}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A fresh TOTP code works once its step has not been spent yet.
    crate::database::query("UPDATE user_totp SET last_step=0")
        .execute(&state.store.pool)
        .await
        .unwrap();
    let code = current_code(&state).await;
    let (status, cookie, _) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.3",
        json!({"mfaToken":token,"code":code}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_some());

    // Five wrong answers spend the token.
    let (_, _, again) = password_login(&state, "10.0.0.4").await;
    let token = again["mfaToken"].as_str().unwrap().to_owned();
    for _ in 0..5 {
        let (status, _, body) = call(
            &state,
            "POST",
            "/api/v1/auth/login/mfa",
            None,
            "10.0.0.4",
            json!({"mfaToken":token,"code":"000001"}),
        )
        .await;
        assert_eq!(
            (status, body["error"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("invalid_second_factor"))
        );
    }
    let (_, _, body) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.4",
        json!({"mfaToken":token,"recoveryCode":recovery[2]}),
    )
    .await;
    assert_eq!(body["error"], "mfa_expired");

    // Passkey registration (ES256, attestation "none").
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
    let pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    let mut spki = vec![
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ];
    spki.extend_from_slice(pair.public_key().as_ref());
    let credential = [9u8; 16];
    let rp_hash = Sha256::digest(b"localhost");
    let (status, _, options) = call(
        &state,
        "POST",
        "/api/v1/account/security/passkeys/options",
        Some(&session),
        "10.0.0.1",
        json!({"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(options["rp"]["id"], "localhost");
    let mut attested = rp_hash.to_vec();
    attested.push(0x45);
    attested.extend([0, 0, 0, 0]);
    attested.extend([0u8; 16]);
    attested.extend([0, 16]);
    attested.extend(credential);
    attested.extend([0xa0]);
    let register = |challenge: &str, origin_kind: &str| {
        json!({
            "name": "Test key",
            "credentialId": b64(&credential),
            "clientDataJson": b64(&client_data(origin_kind, challenge)),
            "authenticatorData": b64(&attested),
            "publicKey": b64(&spki),
            "publicKeyAlgorithm": -7,
        })
    };
    let challenge = options["challenge"].as_str().unwrap().to_owned();
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/account/security/passkeys",
        Some(&session),
        "10.0.0.1",
        register(&challenge, "webauthn.get"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // New options replace the pending registration challenge.
    let (_, _, options) = call(
        &state,
        "POST",
        "/api/v1/account/security/passkeys/options",
        Some(&session),
        "10.0.0.1",
        json!({"password":PASSWORD}),
    )
    .await;
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/account/security/passkeys",
        Some(&session),
        "10.0.0.1",
        register(&challenge, "webauthn.create"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let challenge = options["challenge"].as_str().unwrap().to_owned();
    let (status, _, added) = call(
        &state,
        "POST",
        "/api/v1/account/security/passkeys",
        Some(&session),
        "10.0.0.1",
        register(&challenge, "webauthn.create"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");

    // Passwordless sign-in with a signed assertion; the challenge cannot be reused.
    let assert_with = |challenge: &str, counter: u8| {
        let mut auth_data = rp_hash.to_vec();
        auth_data.push(0x05);
        auth_data.extend([0, 0, 0, counter]);
        let client = client_data("webauthn.get", challenge);
        let mut signed = auth_data.clone();
        signed.extend_from_slice(&Sha256::digest(&client));
        let signature = pair.sign(&rng, &signed).unwrap();
        json!({
            "credentialId": b64(&credential),
            "clientDataJson": b64(&client),
            "authenticatorData": b64(&auth_data),
            "signature": b64(signature.as_ref()),
            "userHandle": b64(user_id.as_bytes()),
        })
    };
    let (_, _, options) = call(
        &state,
        "POST",
        "/api/v1/auth/passkey/options",
        None,
        "10.0.0.5",
        json!({}),
    )
    .await;
    let challenge = options["challenge"].as_str().unwrap().to_owned();
    let assertion = assert_with(&challenge, 1);
    let (status, cookie, body) = call(
        &state,
        "POST",
        "/api/v1/auth/passkey/login",
        None,
        "10.0.0.5",
        assertion.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(cookie.is_some());
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/auth/passkey/login",
        None,
        "10.0.0.5",
        assertion,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A counter that goes backwards is refused (cloned authenticator).
    let (_, _, options) = call(
        &state,
        "POST",
        "/api/v1/auth/passkey/options",
        None,
        "10.0.0.5",
        json!({}),
    )
    .await;
    let (status, _, body) = call(
        &state,
        "POST",
        "/api/v1/auth/passkey/login",
        None,
        "10.0.0.5",
        assert_with(options["challenge"].as_str().unwrap(), 1),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("passkey_counter"))
    );

    // The passkey also answers the second step after a password.
    let (_, _, challenge) = password_login(&state, "10.0.0.6").await;
    let methods = challenge["methods"].to_string();
    assert!(methods.contains("passkey") && methods.contains("totp"));
    let token = challenge["mfaToken"].as_str().unwrap();
    let assertion = assert_with(challenge["passkey"]["challenge"].as_str().unwrap(), 2);
    let (status, cookie, _) = call(
        &state,
        "POST",
        "/api/v1/auth/login/mfa",
        None,
        "10.0.0.6",
        json!({"mfaToken":token,"passkey":assertion}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_some());

    // Disabling TOTP and removing the passkey both need the password; then login is single-step.
    let (status, _, _) = call(
        &state,
        "POST",
        "/api/v1/account/security/totp/disable",
        Some(&session),
        "10.0.0.1",
        json!({"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Six password checks within a minute: the fifth already used the per-account budget.
    crate::database::query("DELETE FROM login_throttle")
        .execute(&state.store.pool)
        .await
        .unwrap();
    let path = format!("/api/v1/account/security/passkeys/{}", b64(&credential));
    let (status, _, _) = call(
        &state,
        "DELETE",
        &path,
        Some(&session),
        "10.0.0.1",
        json!({"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, cookie, _) = password_login(&state, "10.0.0.7").await;
    assert_eq!(status, StatusCode::OK);
    assert!(cookie.is_some());
}
