//! Aether client core, shared by the native applications (Android, iOS, TV,
//! GPUI desktop). It speaks the Core's public API with a device token
//! (`Authorization: Bearer`), never a cookie, and leaves storage of that token
//! to the platform keychain: [`Client::token`] / [`Client::with_token`].
//!
//! Playback goes through HLS capability URLs ([`Client::playback_url`]) that a
//! native player, a Chromecast or AirPlay opens without any credential.

mod model;
mod update;

pub use model::*;
pub use update::{APP_VERSION, RELEASES_URL, is_outdated};

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
    /// The address answers, but not like an Aether server (another service, or
    /// a web page): nothing to sign in to.
    #[error("not an Aether server")]
    NotAether,
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

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
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
/// certificate, or plain HTTP on a private network). Public so that an
/// application's other connections (the remote-control WebSocket) trust the
/// same roots.
pub fn tls_config() -> Result<rustls::ClientConfig> {
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
            .tls_backend_preconfigured(tls_config()?)
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

    /// Checks that the address is an Aether server and reads what the sign-in
    /// screen needs: its name, version and sign-in methods (with the SSO button
    /// label its administrator chose). Public endpoints only, no account needed.
    /// A service that is not Aether is [`Error::NotAether`]; an unreachable one
    /// is a network error.
    pub async fn discover(&self) -> Result<ServerInfo> {
        let identify = |error: Error| match error {
            Error::Server { .. } => Error::NotAether,
            other => other,
        };
        let (_, config) = self
            .call(Method::GET, "/api/v1/config", None, false)
            .await
            .map_err(identify)?;
        let text = |key: &str| config[key].as_str().map(str::to_owned);
        let (Some(name), Some(version), Some(_mode)) =
            (text("name"), text("version"), text("mode"))
        else {
            return Err(Error::NotAether);
        };
        let (_, status) = self
            .call(Method::GET, "/api/v1/auth/oidc/status", None, false)
            .await
            .map_err(identify)?;
        if !status["enabled"].is_boolean() || !status["localLoginEnabled"].is_boolean() {
            return Err(Error::NotAether);
        }
        let sign_in: SignInOptions =
            serde_json::from_value(status).map_err(|_| Error::NotAether)?;
        // First start: no administrator yet. Best effort, never blocks.
        let needs_bootstrap = match self
            .call(Method::GET, "/api/v1/auth/bootstrap/status", None, false)
            .await
        {
            Ok((_, value)) => value["needsBootstrap"].as_bool().unwrap_or(false),
            Err(_) => false,
        };
        Ok(ServerInfo {
            name,
            version,
            sign_in,
            needs_bootstrap,
        })
    }
    /// The newer release published by the server, when this application is older
    /// (see [`RELEASES_URL`]). Public endpoint; `None` for a development build or
    /// when the version cannot be compared. Call it after every sign-in or
    /// session restore, and ignore its errors: it must never block anything.
    pub async fn check_update(&self) -> Result<Option<String>> {
        let Some(app) = APP_VERSION.filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        let (_, config) = self
            .call(Method::GET, "/api/v1/config", None, false)
            .await?;
        Ok(config["version"]
            .as_str()
            .filter(|server| is_outdated(app, server))
            .map(str::to_owned))
    }
    /// Local account sign-in. Stores the device token, or, for an account with
    /// two-step sign-in, returns the challenge to finish with
    /// [`Client::login_second_factor`].
    pub async fn login(&self, login: &str, password: &str) -> Result<LoginStep> {
        let body = json!({"login": login, "password": password, "tokenType": "bearer"});
        let (_, value) = self
            .call(Method::POST, "/api/v1/auth/login", Some(body), false)
            .await?;
        if value["mfaRequired"].as_bool() == Some(true) {
            let token = value["mfaToken"].as_str().ok_or_else(missing)?.to_owned();
            let totp = value["methods"]
                .as_array()
                .is_some_and(|m| m.iter().any(|v| v == "totp"));
            return Ok(LoginStep::SecondFactor(SecondFactor { token, totp }));
        }
        self.grant(&value).map(LoginStep::SignedIn)
    }
    /// Second step: an authenticator code (6 digits) or, with `recovery`, a backup code.
    pub async fn login_second_factor(
        &self,
        challenge: &SecondFactor,
        code: &str,
        recovery: bool,
    ) -> Result<User> {
        let key = if recovery { "recoveryCode" } else { "code" };
        let body = json!({"mfaToken": challenge.token, key: code.trim()});
        let (_, value) = self
            .call(Method::POST, "/api/v1/auth/login/mfa", Some(body), false)
            .await?;
        self.grant(&value)
    }
    /// Single sign-on in the system browser, where the person's provider
    /// session and password manager live. Open [`SsoRequest::url`]; the browser
    /// comes back on `redirect` (`aether://sso` for a phone, or
    /// `http://127.0.0.1:<port>/sso` for a desktop application listening on
    /// that port) with a `code` to give to [`Client::finish_sso`], or an
    /// `error`. No network: the request only exists once the browser opens it.
    pub fn start_sso(&self, redirect: &str) -> Result<SsoRequest> {
        let mut secret = [0u8; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut secret)
            .map_err(|_| Error::InvalidUrl)?;
        let verifier = hex(&secret);
        let challenge =
            hex(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()).as_ref());
        let mut url = self
            .base
            .join("api/v1/auth/oidc/app/start")
            .map_err(|_| Error::InvalidUrl)?;
        url.query_pairs_mut()
            .append_pair("challenge", &challenge)
            .append_pair("redirect", redirect);
        Ok(SsoRequest {
            url: url.into(),
            verifier,
        })
    }
    /// Redeems the code the browser brought back, with the verifier of the
    /// request that was started here. Stores the device token.
    pub async fn finish_sso(&self, code: &str, verifier: &str) -> Result<User> {
        let body = json!({"code": code, "verifier": verifier, "tokenType": "bearer"});
        let (_, value) = self
            .call(
                Method::POST,
                "/api/v1/auth/oidc/app/token",
                Some(body),
                false,
            )
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
            if let Some(library) = query.library.as_deref().filter(|l| !l.is_empty()) {
                pairs.append_pair("library", library);
            }
            pairs.append_pair("offset", &query.offset.to_string());
            pairs.append_pair("limit", &query.limit.clamp(1, 100).to_string());
        }
        let path = format!("{}?{}", url.path(), url.query().unwrap_or(""));
        self.get(&path).await
    }
    /// The libraries this account may browse, by name.
    pub async fn libraries(&self) -> Result<Vec<Library>> {
        let value: Value = self.get("/api/v1/libraries").await?;
        serde_json::from_value(value["libraries"].clone()).map_err(|_| missing())
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
    /// Bytes of a server-relative image (a poster), fetched with the token from
    /// this server only. Versioned URLs never change: cache them for good.
    pub async fn image(&self, path: &str) -> Result<Vec<u8>> {
        let url = self.url(path)?;
        let token = self.token().ok_or(Error::SignedOut)?;
        let response = self
            .http
            .get(url)
            .header(header::AUTHORIZATION, format!("Bearer {}", token.access))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Server {
                status: status.as_u16(),
                code: String::new(),
            });
        }
        Ok(response.bytes().await?.to_vec())
    }
    /// URL of an asset's original file, for direct play by a player that can
    /// send `Authorization: Bearer` itself (libmpv on the desktop). Range
    /// requests are supported; nothing is transcoded.
    pub fn content_url(&self, asset_id: &str) -> Result<String> {
        self.url(&format!("/api/v1/assets/{}/content", segment(asset_id)))
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

    /// "Continue watching": started and unfinished films and episodes, most
    /// recently watched first (the Core keeps the latest 100 positions).
    pub async fn continue_watching(&self) -> Result<Vec<ResumePoint>> {
        let value: Value = self.get("/api/v1/progress").await?;
        let items: Vec<ResumePoint> =
            serde_json::from_value(value["items"].clone()).map_err(|_| missing())?;
        Ok(items
            .into_iter()
            .filter(|p| !p.watched && p.position > 0.0 && p.position < p.duration)
            .collect())
    }

    /// The account's latest positions, most recent first (at most 100), for a
    /// "Continue watching" row; finished and untouched items are included.
    pub async fn recent_progress(&self) -> Result<Vec<RecentProgress>> {
        let page: RecentProgressPage = self.get("/api/v1/progress").await?;
        Ok(page.items)
    }
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
        assert_eq!(
            client.content_url("a/b").unwrap(),
            "https://media.example/api/v1/assets/ab/content"
        );
        for foreign in ["https://evil.example/x", "//evil.example/x", "x"] {
            assert!(client.url(foreign).is_err(), "{foreign}");
        }
    }
}
