//! UniFFI bindings of `aether-client`: Kotlin for Android, Swift for iOS.
//! Every network call is a `suspend` function (Kotlin) / `async` (Swift),
//! driven by Tokio inside the library.
use aether_client as core;
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ClientError {
    /// Sign in again: the token is missing, expired or revoked.
    #[error("unauthorized: {detail}")]
    Unauthorized { detail: String },
    /// Start a new pairing: this one expired or was already used.
    #[error("pairing expired")]
    PairingExpired,
    #[error("server error {status}: {code}")]
    Server { status: u16, code: String },
    #[error("{detail}")]
    Network { detail: String },
    #[error("invalid server address")]
    InvalidUrl,
}

impl From<core::Error> for ClientError {
    fn from(error: core::Error) -> Self {
        if error.is_unauthorized() {
            return Self::Unauthorized {
                detail: error.to_string(),
            };
        }
        match error {
            core::Error::InvalidUrl => Self::InvalidUrl,
            core::Error::PairingExpired => Self::PairingExpired,
            core::Error::Server { status, code } => Self::Server { status, code },
            other => Self::Network {
                detail: other.to_string(),
            },
        }
    }
}

type Result<T> = std::result::Result<T, ClientError>;

#[derive(uniffi::Record)]
pub struct Token {
    pub access: String,
    pub expires_at: i64,
}
#[derive(uniffi::Record)]
pub struct User {
    pub id: String,
    pub login: String,
    pub role: String,
    pub display_name: Option<String>,
}
#[derive(uniffi::Record)]
pub struct Pairing {
    pub code: String,
    pub secret: String,
    pub url: String,
    pub expires_in: i64,
    pub qr_width: u32,
    pub qr_modules: Vec<bool>,
}
#[derive(uniffi::Record)]
pub struct Work {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub year: Option<i64>,
    pub description: String,
    /// Server-relative; make it absolute with `AetherClient::url`.
    pub poster_url: Option<String>,
}
#[derive(uniffi::Record)]
pub struct Episode {
    pub id: String,
    pub number: Option<i64>,
    pub title: String,
    pub description: String,
}
#[derive(uniffi::Record)]
pub struct Season {
    pub id: String,
    pub number: Option<i64>,
    pub title: String,
    pub episodes: Vec<Episode>,
}
#[derive(uniffi::Record)]
pub struct WorkDetail {
    pub work: Work,
    pub seasons: Vec<Season>,
}
#[derive(uniffi::Record)]
pub struct CatalogPage {
    pub items: Vec<Work>,
    pub has_more: bool,
    pub total: Option<u64>,
}
#[derive(uniffi::Record)]
pub struct Media {
    pub id: String,
    pub title: String,
    pub work_id: Option<String>,
    pub available: bool,
}
#[derive(uniffi::Record)]
pub struct MediaLibrary {
    pub id: String,
    pub name: String,
    pub kind: String,
}
#[derive(uniffi::Record)]
pub struct ResumePoint {
    pub work_id: String,
    pub title: String,
    pub kind: String,
    pub series_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub updated_at: i64,
}
#[derive(uniffi::Record)]
pub struct PlaybackUrl {
    pub url: String,
    pub expires_at: i64,
}
#[derive(uniffi::Record)]
pub struct Progress {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    pub revision: i64,
}

impl From<core::Work> for Work {
    fn from(w: core::Work) -> Self {
        Self {
            id: w.id,
            kind: w.kind,
            title: w.title,
            year: w.year,
            description: w.description,
            poster_url: w.poster_url,
        }
    }
}

impl From<core::User> for User {
    fn from(u: core::User) -> Self {
        Self {
            id: u.id,
            login: u.login,
            role: u.role,
            display_name: u.display_name,
        }
    }
}
impl From<core::Progress> for Progress {
    fn from(p: core::Progress) -> Self {
        Self {
            position: p.position,
            duration: p.duration,
            watched: p.watched,
            revision: p.revision,
        }
    }
}

#[derive(uniffi::Object)]
pub struct AetherClient {
    inner: core::Client,
}

#[uniffi::export(async_runtime = "tokio")]
impl AetherClient {
    /// `token` is the one saved in the keychain by a previous session, if any.
    #[uniffi::constructor]
    pub fn new(server: String, device: String, token: Option<Token>) -> Result<Arc<Self>> {
        let mut inner = core::Client::new(&server, &device)?;
        if let Some(token) = token {
            inner = inner.with_token(core::Token {
                access: token.access,
                expires_at: token.expires_at,
            });
        }
        Ok(Arc::new(Self { inner }))
    }
    /// Save it in the keychain after every sign-in and refresh.
    pub fn token(&self) -> Option<Token> {
        self.inner.token().map(|t| Token {
            access: t.access,
            expires_at: t.expires_at,
        })
    }
    pub async fn login(&self, login: String, password: String) -> Result<User> {
        Ok(self.inner.login(&login, &password).await?.into())
    }
    pub async fn start_pairing(&self) -> Result<Pairing> {
        let p = self.inner.start_pairing().await?;
        Ok(Pairing {
            code: p.code,
            secret: p.secret,
            url: p.url,
            expires_in: p.expires_in,
            qr_width: p.qr_width as u32,
            qr_modules: p.qr_modules,
        })
    }
    /// `None` while pending; poll every 2–3 seconds until `expires_in`.
    pub async fn poll_pairing(&self, pairing: Pairing) -> Result<Option<User>> {
        let pairing = core::Pairing {
            code: pairing.code,
            secret: pairing.secret,
            url: pairing.url,
            expires_in: pairing.expires_in,
            qr_width: pairing.qr_width as usize,
            qr_modules: pairing.qr_modules,
        };
        Ok(self.inner.poll_pairing(&pairing).await?.map(Into::into))
    }
    pub async fn approve_pairing(&self, code: String) -> Result<()> {
        Ok(self.inner.approve_pairing(&code).await?)
    }
    /// Call at start-up and daily from a background job (e.g. 7 days of margin).
    pub async fn refresh_if_due(&self, margin_seconds: i64) -> Result<bool> {
        Ok(self.inner.refresh_if_due(margin_seconds).await?)
    }
    pub async fn me(&self) -> Result<User> {
        Ok(self.inner.me().await?.into())
    }
    pub async fn logout(&self) -> Result<()> {
        Ok(self.inner.logout().await?)
    }
    pub async fn catalog(
        &self,
        kind: Option<String>,
        search: Option<String>,
        offset: u32,
        limit: u32,
    ) -> Result<CatalogPage> {
        let page = self
            .inner
            .catalog(&core::CatalogQuery {
                kind,
                search,
                library: None,
                offset: offset as usize,
                limit: limit as usize,
            })
            .await?;
        Ok(CatalogPage {
            items: page.items.into_iter().map(Into::into).collect(),
            has_more: page.has_more,
            total: page.total.map(|t| t as u64),
        })
    }
    /// The libraries this account may browse.
    pub async fn libraries(&self) -> Result<Vec<MediaLibrary>> {
        Ok(self
            .inner
            .libraries()
            .await?
            .into_iter()
            .map(|l| MediaLibrary {
                id: l.id,
                name: l.name,
                kind: l.kind,
            })
            .collect())
    }
    /// One library's works, films and series together.
    pub async fn library_catalog(
        &self,
        library: String,
        search: Option<String>,
        offset: u32,
        limit: u32,
    ) -> Result<CatalogPage> {
        let page = self
            .inner
            .catalog(&core::CatalogQuery {
                kind: None,
                search,
                library: Some(library),
                offset: offset as usize,
                limit: limit as usize,
            })
            .await?;
        Ok(CatalogPage {
            items: page.items.into_iter().map(Into::into).collect(),
            has_more: page.has_more,
            total: page.total.map(|t| t as u64),
        })
    }
    /// Started and unfinished films and episodes, most recent first.
    pub async fn continue_watching(&self) -> Result<Vec<ResumePoint>> {
        Ok(self
            .inner
            .continue_watching()
            .await?
            .into_iter()
            .map(|p| ResumePoint {
                work_id: p.work_id,
                title: p.title,
                kind: p.kind,
                series_id: p.series_id,
                position: p.position,
                duration: p.duration,
                updated_at: p.updated_at,
            })
            .collect())
    }
    /// A work with its seasons and episodes (no seasons for a film).
    pub async fn work(&self, id: String) -> Result<WorkDetail> {
        let detail = self.inner.work(&id).await?;
        Ok(WorkDetail {
            work: detail.work.into(),
            seasons: detail
                .seasons
                .into_iter()
                .map(|season| Season {
                    id: season.id,
                    number: season.number,
                    title: season.title,
                    episodes: season
                        .episodes
                        .into_iter()
                        .map(|e| Episode {
                            id: e.id,
                            number: e.number,
                            title: e.title,
                            description: e.description,
                        })
                        .collect(),
                })
                .collect(),
        })
    }
    /// Absolute URL of a server-relative path such as a poster.
    pub fn url(&self, path: String) -> Result<String> {
        Ok(self.inner.url(&path)?)
    }
    pub async fn media(&self, id: String) -> Result<Media> {
        let m = self.inner.media(&id).await?;
        Ok(Media {
            id: m.id,
            title: m.title,
            work_id: m.work_id,
            available: m.available,
        })
    }
    /// HLS URL for ExoPlayer, AVPlayer, Chromecast or AirPlay; no credential needed.
    pub async fn playback_url(&self, asset_id: String) -> Result<PlaybackUrl> {
        let p = self.inner.playback_url(&asset_id).await?;
        Ok(PlaybackUrl {
            url: p.url,
            expires_at: p.expires_at,
        })
    }
    pub async fn progress(&self, work_id: String) -> Result<Progress> {
        Ok(self.inner.progress(&work_id).await?.into())
    }
    pub async fn save_progress(
        &self,
        work_id: String,
        position: f64,
        duration: f64,
        watched: bool,
        expected_revision: i64,
    ) -> Result<Progress> {
        let update = core::ProgressUpdate {
            position,
            duration,
            watched,
            expected_revision,
        };
        Ok(self.inner.save_progress(&work_id, &update).await?.into())
    }
}
