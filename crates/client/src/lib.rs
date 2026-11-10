//! Aether client core, shared by the native applications (Android, iOS, TV,
//! GPUI desktop). It speaks the Core's public API with a device token
//! (`Authorization: Bearer`), never a cookie, and leaves storage of that token
//! to the platform keychain: [`Client::token`] / [`Client::with_token`].
//!
//! Playback goes through HLS capability URLs ([`Client::playback_url`]) that a
//! native player, a Chromecast or AirPlay opens without any credential.

mod model;

pub use model::*;

use reqwest::{Method, StatusCode, header};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    sync::RwLock,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid server address")]
    InvalidUrl,
    #[error("not signed in")]
    SignedOut,
    #[error("the pairing request expired")]
    PairingExpired,
    /// The Core answered with an error status and, usually, an error code.
    #[error("server refused the request: {status} {code}")]
    Server { status: u16, code: String },
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
}

impl Error {
    /// The token is no longer valid: the application must sign in again.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::SignedOut | Error::Server { status: 401, .. })
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The Core names a device after its `User-Agent`; keep it printable and short.
fn user_agent(device: &str) -> String {
    let device: String = device
        .chars()
        .filter(|c| !c.is_control() && *c != '(' && *c != ')')
        .take(96)
        .collect();
    format!("Aether/{} ({})", env!("CARGO_PKG_VERSION"), device.trim())
}

/// Mozilla's roots through `ring`: identical on every platform, no native
/// verifier to initialize. A self-signed Core is not trusted (use a real
/// certificate, or plain HTTP on a private network).
fn tls() -> Result<rustls::ClientConfig> {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::InvalidUrl)?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

pub struct Client {
    base: Url,
    http: reqwest::Client,
    token: RwLock<Option<Token>>,
}

impl Client {
    /// `server` is the Core's public URL, e.g. `https://media.example`;
    /// `device` names this device in the account's sessions ("Pixel 9", "Salon").
    pub fn new(server: &str, device: &str) -> Result<Self> {
        let mut base = Url::parse(server).map_err(|_| Error::InvalidUrl)?;
        if !matches!(base.scheme(), "http" | "https") || base.host().is_none() {
            return Err(Error::InvalidUrl);
        }
        base.set_path("/");
        base.set_query(None);
        base.set_fragment(None);
        let http = reqwest::Client::builder()
            .user_agent(user_agent(device))
            .tls_backend_preconfigured(tls()?)
            .build()?;
        Ok(Self {
            base,
            http,
            token: RwLock::new(None),
        })
    }
    /// Restores a token read back from the platform keychain.
    pub fn with_token(self, token: Token) -> Self {
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = Some(token);
        self
    }
    /// The current token, to save in the keychain after a sign-in or refresh.
    pub fn token(&self) -> Option<Token> {
        self.token.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn set_token(&self, token: Option<Token>) {
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = token;
    }
    fn grant(&self, body: &Value) -> Result<User> {
        let access = body["accessToken"].as_str().ok_or_else(missing)?.to_owned();
        let expires_in = body["expiresIn"].as_i64().ok_or_else(missing)?;
        self.set_token(Some(Token {
            access,
            expires_at: now() + expires_in,
        }));
        serde_json::from_value(body["user"].clone()).map_err(|_| missing())
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        signed: bool,
    ) -> Result<(StatusCode, Value)> {
        let url = self
            .base
            .join(path.trim_start_matches('/'))
            .map_err(|_| Error::InvalidUrl)?;
        let mut request = self.http.request(method, url);
        if signed {
            let token = self.token().ok_or(Error::SignedOut)?;
            request = request.header(header::AUTHORIZATION, format!("Bearer {}", token.access));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok((status, value));
        }
        Err(Error::Server {
            status: status.as_u16(),
            code: value["error"].as_str().unwrap_or("").to_owned(),
        })
    }
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (_, value) = self.call(Method::GET, path, None, true).await?;
        serde_json::from_value(value).map_err(|_| missing())
    }

    // ---- Sign-in -------------------------------------------------------------

    /// Local account sign-in; stores the device token.
    pub async fn login(&self, login: &str, password: &str) -> Result<User> {
        let body = json!({"login": login, "password": password, "tokenType": "bearer"});
        let (_, value) = self
            .call(Method::POST, "/api/v1/auth/login", Some(body), false)
            .await?;
        self.grant(&value)
    }
    /// Starts a QR pairing: show [`Pairing::url`] (or its QR modules) and
    /// [`Pairing::code`], then call [`Client::poll_pairing`] every few seconds.
    pub async fn start_pairing(&self) -> Result<Pairing> {
        let (_, value) = self
            .call(
                Method::POST,
                "/api/v1/auth/device/start",
                Some(json!({})),
                false,
            )
            .await?;
        serde_json::from_value(value).map_err(|_| missing())
    }
    /// `None` while nobody approved yet; the signed-in user once approved.
    pub async fn poll_pairing(&self, pairing: &Pairing) -> Result<Option<User>> {
        let body = json!({"secret": pairing.secret, "tokenType": "bearer"});
        match self
            .call(Method::POST, "/api/v1/auth/device/poll", Some(body), false)
            .await
        {
            Ok((_, value)) if value["status"] == "pending" => Ok(None),
            Ok((_, value)) => self.grant(&value).map(Some),
            Err(Error::Server { status: 410, .. }) => Err(Error::PairingExpired),
            Err(error) => Err(error),
        }
    }
    /// From a signed-in device (a phone), approves the code another one shows (a TV).
    pub async fn approve_pairing(&self, code: &str) -> Result<()> {
        let body = json!({"code": code});
        self.call(
            Method::POST,
            "/api/v1/auth/device/approve",
            Some(body),
            true,
        )
        .await?;
        Ok(())
    }
    /// Rotates the token (30 more days). The previous one stays valid one minute.
    pub async fn refresh(&self) -> Result<User> {
        let (_, value) = self
            .call(Method::POST, "/api/v1/auth/token/refresh", None, true)
            .await?;
        self.grant(&value)
    }
    /// Refreshes when the token expires within `margin_seconds`; call it at start-up
    /// and from a daily background task (the Core also expires idle tokens after 7 days).
    pub async fn refresh_if_due(&self, margin_seconds: i64) -> Result<bool> {
        let token = self.token().ok_or(Error::SignedOut)?;
        if token.expires_at - now() > margin_seconds {
            return Ok(false);
        }
        self.refresh().await.map(|_| true)
    }
    pub async fn me(&self) -> Result<User> {
        let value: Value = self.get("/api/v1/auth/me").await?;
        serde_json::from_value(value["user"].clone()).map_err(|_| missing())
    }
    /// Ends this device's session on the Core and forgets the token.
    pub async fn logout(&self) -> Result<()> {
        let result = self
            .call(Method::POST, "/api/v1/auth/logout", None, true)
            .await;
        self.set_token(None);
        result.map(|_| ())
    }

    // ---- Library ------------------------------------------------------------

    pub async fn catalog(&self, query: &CatalogQuery) -> Result<CatalogPage> {
        let mut url = Url::parse("http://query/api/v1/catalog").map_err(|_| Error::InvalidUrl)?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(kind) = &query.kind {
                pairs.append_pair("kind", kind);
            }
            if let Some(search) = query.search.as_deref().filter(|s| !s.is_empty()) {
                pairs.append_pair("q", search);
            }
            pairs.append_pair("offset", &query.offset.to_string());
            pairs.append_pair("limit", &query.limit.clamp(1, 200).to_string());
        }
        let path = format!("{}?{}", url.path(), url.query().unwrap_or(""));
        self.get(&path).await
    }
    /// A work with its seasons and episodes.
    pub async fn work(&self, id: &str) -> Result<WorkDetail> {
        self.get(&format!("/api/v1/catalog/{}", segment(id))).await
    }
    /// Absolute URL of a server-relative path (posters). Images need the token:
    /// send it as `Authorization: Bearer`, to this server only.
    pub fn url(&self, path: &str) -> Result<String> {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(Error::InvalidUrl);
        }
        Ok(self
            .base
            .join(&path[1..])
            .map_err(|_| Error::InvalidUrl)?
            .to_string())
    }
    /// The playable file of a work (a film, an episode) or of an asset id.
    pub async fn media(&self, id: &str) -> Result<Media> {
        self.get(&format!("/api/v1/media/{}", segment(id))).await
    }
    /// HLS master playlist URL for an asset: hand it to the native player, a
    /// Chromecast or AirPlay. It needs no credential and lasts 12 hours at most.
    pub async fn playback_url(&self, asset_id: &str) -> Result<PlaybackUrl> {
        let path = format!("/api/v1/assets/{}/hls", segment(asset_id));
        let (_, value) = self.call(Method::POST, &path, None, true).await?;
        serde_json::from_value(value).map_err(|_| missing())
    }

    // ---- Progress -----------------------------------------------------------

    pub async fn progress(&self, work_id: &str) -> Result<Progress> {
        self.get(&format!("/api/v1/progress/{}", segment(work_id)))
            .await
    }
    /// Saves the position. Another device may have written in between: the
    /// latest position wins, so a revision conflict is retried once on top of it.
    pub async fn save_progress(&self, work_id: &str, update: &ProgressUpdate) -> Result<Progress> {
        let path = format!("/api/v1/progress/{}", segment(work_id));
        let mut revision = update.expected_revision;
        for attempt in 0..2 {
            let body = to_value(&ProgressWrite {
                position: update.position,
                duration: update.duration,
                watched: update.watched,
                expected_revision: revision,
            });
            match self.call(Method::PUT, &path, Some(body), true).await {
                Ok((_, value)) => return serde_json::from_value(value).map_err(|_| missing()),
                Err(Error::Server { status: 409, .. }) if attempt == 0 => {
                    revision = self.progress(work_id).await?.revision;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the loop returns on its second attempt")
    }
}

/// Ids come from the Core, but a path segment must never smuggle `/`, `?` or `#`.
fn segment(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect()
}
fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
fn missing() -> Error {
    Error::Server {
        status: 200,
        code: "unexpected_response".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_address_and_device_name_are_normalized() {
        let client = Client::new("https://media.example/app/?x=1#y", "Pixel 9 (Mael)\n").unwrap();
        assert_eq!(client.base.as_str(), "https://media.example/");
        assert_eq!(
            user_agent("Pixel 9 (Mael)\n"),
            format!("Aether/{} (Pixel 9 Mael)", env!("CARGO_PKG_VERSION"))
        );
        for bad in ["media.example", "ftp://media.example", "file:///etc"] {
            assert!(
                matches!(Client::new(bad, "x"), Err(Error::InvalidUrl)),
                "{bad}"
            );
        }
        assert_eq!(segment("a/../b?c#d"), "a..bcd");
        assert_eq!(
            client.url("/api/v1/assets/i/image?v=1-2").unwrap(),
            "https://media.example/api/v1/assets/i/image?v=1-2"
        );
        for foreign in ["https://evil.example/x", "//evil.example/x", "x"] {
            assert!(client.url(foreign).is_err(), "{foreign}");
        }
    }
}
