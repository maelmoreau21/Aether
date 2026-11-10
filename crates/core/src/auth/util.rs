//! Session token, cookie, hashing and client-key helpers.
use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};

pub(crate) const ABSOLUTE_SECONDS: i64 = 30 * 24 * 60 * 60;
pub(crate) const IDLE_SECONDS: i64 = 7 * 24 * 60 * 60;
pub(super) const COOKIE: &str = "media_session";

pub fn extract_client_key(headers: &HeaderMap) -> String {
    if let Some(ip) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|f| f.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let sanitized = ip
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
            .take(64)
            .collect::<String>();
        if !sanitized.is_empty() {
            return format!("ip:{sanitized}");
        }
    }
    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let sanitized = real_ip
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
            .take(64)
            .collect::<String>();
        if !sanitized.is_empty() {
            return format!("ip:{sanitized}");
        }
    }
    "ip:127.0.0.1".to_string()
}
pub(crate) fn device_label(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("Navigateur")
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}
pub(crate) fn random_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
pub(crate) fn hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}
pub(super) fn csrf(raw: &str) -> String {
    hash(&format!("csrf:{raw}"))
}
pub(super) fn constant_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}
/// How a sign-in hands its session back: a browser cookie (default) or, for
/// native applications, a token in the JSON body sent later as `Authorization: Bearer`.
#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum TokenType {
    #[default]
    Cookie,
    Bearer,
}
/// A single `Authorization: Bearer <session token>` header. Anything else (another
/// scheme, a reverse proxy's JWT, repeated headers) is ignored rather than rejected,
/// so it can never break cookie authentication behind such a proxy.
pub(super) fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer")
        && token.len() == 64
        && token.bytes().all(|b| b.is_ascii_hexdigit()))
    .then_some(token)
}
pub(super) fn session_token(headers: &HeaderMap) -> Option<&str> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for pair in value.to_str().ok()?.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if name == COOKIE {
                if found.is_some()
                    || value.len() != 64
                    || !value.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}
