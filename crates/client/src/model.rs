//! Values exchanged with the Core, in the shape its JSON uses. Unknown fields
//! are ignored, so a newer Core keeps working with an older application.
use serde::{Deserialize, Serialize};

/// A device token. Store it in the platform keychain, never in plain files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub access: String,
    /// Unix seconds; refresh well before (see `Client::refresh_if_due`).
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub login: String,
    /// `administrator` or `user`.
    pub role: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

/// How this Core lets people sign in, read before any account is known
/// (`GET /api/v1/auth/oidc/status`, public).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInOptions {
    /// An OpenID Connect provider (SSO) is configured.
    #[serde(default)]
    pub enabled: bool,
    /// Local identifier and password sign-in is allowed.
    #[serde(default = "yes")]
    pub local_login_enabled: bool,
    /// Label of the SSO button, chosen by the administrator of the Core.
    #[serde(default)]
    pub button_label: Option<String>,
    /// Identity provider ("Authentik", "Keycloak"… or "SSO" when unknown).
    #[serde(default)]
    pub provider: Option<String>,
}

fn yes() -> bool {
    true
}

impl Default for SignInOptions {
    /// What to assume when a Core cannot say (an older one): no SSO, passwords allowed.
    fn default() -> Self {
        Self {
            enabled: false,
            local_login_enabled: true,
            button_label: None,
            provider: None,
        }
    }
}

impl SignInOptions {
    /// The SSO button text, with a default when the Core sets none.
    pub fn sso_label(&self) -> String {
        self.button_label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .unwrap_or("Se connecter avec SSO")
            .to_owned()
    }
}

/// What the sign-in screen shows about a server (see `Client::discover`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerInfo {
    /// Brand name of the server, shown above the sign-in form.
    pub name: String,
    pub version: String,
    pub sign_in: SignInOptions,
    /// No administrator exists yet: the server must be set up from its web page.
    pub needs_bootstrap: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    /// Shown to the person, typed on the approving device if not scanned.
    pub code: String,
    /// Kept by this device only; it proves the request is ours when polling.
    pub secret: String,
    /// Opened by the approving phone (also encoded in the QR modules).
    pub url: String,
    pub expires_in: i64,
    pub qr_width: usize,
    /// Row-major, `qr_width × qr_width`, `true` for a dark module.
    pub qr_modules: Vec<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogQuery {
    /// `film` or `series`; `None` for both.
    pub kind: Option<String>,
    pub search: Option<String>,
    /// A library id from `Client::libraries`; `None` for every library.
    pub library: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub description: String,
    /// Server-relative and versioned (`/api/v1/assets/…/image?v=…`): cache it
    /// forever, fetch it with the token (`Client::url` makes it absolute).
    #[serde(default)]
    pub poster_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    /// The episode's own work id: play it with `Client::media`.
    pub id: String,
    #[serde(default)]
    pub number: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub id: String,
    #[serde(default)]
    pub number: Option<i64>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub episodes: Vec<Episode>,
}

/// A work with its seasons (empty for a film).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkDetail {
    #[serde(flatten)]
    pub work: Work,
    #[serde(default)]
    pub seasons: Vec<Season>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub items: Vec<Work>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub total: Option<usize>,
}

/// A library the signed-in user may browse.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    pub name: String,
    /// `films`, `series`, `music`… as configured by the administrator.
    #[serde(default)]
    pub kind: String,
}

/// A saved position, most recent first in `Client::continue_watching`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    /// The film or episode to play.
    pub work_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub kind: String,
    /// The series of an episode.
    #[serde(default)]
    pub series_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    #[serde(default)]
    pub watched: bool,
    /// Unix seconds.
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    /// The asset (file) id, used for `Client::playback_url`.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub work_id: Option<String>,
    #[serde(default)]
    pub available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackUrl {
    pub url: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Seconds.
    pub position: f64,
    pub duration: f64,
    #[serde(default)]
    pub watched: bool,
    /// Pass it back as `ProgressUpdate::expected_revision`.
    pub revision: i64,
}

/// One entry of `Client::recent_progress`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProgress {
    pub work_id: String,
    #[serde(default)]
    pub title: String,
    /// `film` or `episode`.
    #[serde(default)]
    pub kind: String,
    /// The series of an episode.
    #[serde(default)]
    pub series_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    #[serde(default)]
    pub watched: bool,
    /// Unix seconds.
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Deserialize)]
pub(crate) struct RecentProgressPage {
    #[serde(default)]
    pub items: Vec<RecentProgress>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgressUpdate {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    pub expected_revision: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProgressWrite {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    pub expected_revision: i64,
}

#[cfg(test)]
mod tests {
    use super::SignInOptions;
    use serde_json::json;

    #[test]
    fn sign_in_options_follow_the_core_and_default_safely() {
        let sso: SignInOptions = serde_json::from_value(json!({
            "enabled": true, "provider": "Authentik",
            "localLoginEnabled": false, "buttonLabel": "Entrer avec Authentik"
        }))
        .unwrap();
        assert!(sso.enabled && !sso.local_login_enabled);
        assert_eq!(sso.sso_label(), "Entrer avec Authentik");

        // A Core that says nothing: passwords allowed, no SSO, default label.
        let bare: SignInOptions = serde_json::from_value(json!({})).unwrap();
        assert_eq!(bare, SignInOptions::default());
        assert!(bare.local_login_enabled && !bare.enabled);
        let blank = SignInOptions {
            button_label: Some("  ".into()),
            ..SignInOptions::default()
        };
        assert_eq!(blank.sso_label(), "Se connecter avec SSO");
    }
}
