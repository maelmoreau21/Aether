//! `AuthState`, authenticated identity types and session / CSRF / origin checks.
use super::{
    config::{is_http_bootstrap_disabled, resolve_allowed_origins},
    touch::{MAX_PENDING_TOUCHES, SessionTouchStats, SessionToucher, TOUCH_INTERVAL_SECONDS},
    util::{COOKIE, IDLE_SECONDS, constant_equal, csrf, hash, random_token, session_token},
};
use crate::identity::{IdentityError, IdentityStore, Role};
use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
use axum::http::{HeaderMap, StatusCode, header};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AuthState {
    pub(crate) store: IdentityStore,
    pub(super) origin: Arc<str>,
    pub(super) allowed_origins: Arc<[String]>,
    pub(crate) secure: bool,
    pub(super) dummy_hash: Arc<str>,
    pub(crate) workers: Arc<Semaphore>,
    pub(super) toucher: Arc<SessionToucher>,
    pub(crate) bootstrap_token: Arc<Mutex<Option<String>>>,
    pub(crate) bootstrap_file_path: Arc<Mutex<Option<std::path::PathBuf>>>,
}
#[derive(Debug, thiserror::Error)]
pub enum AuthConfigError {
    #[error("public origin must be HTTPS or HTTP on loopback, without path or credentials")]
    Origin,
    #[error(transparent)]
    Identity(#[from] IdentityError),
}
#[derive(Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    pub id: String,
    pub login: String,
    #[serde(serialize_with = "serialize_role")]
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sso: Option<bool>,
}
fn serialize_role<S: serde::Serializer>(role: &Role, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(role.as_str())
}
#[derive(Clone)]
pub struct Authenticated {
    pub user: AuthUser,
    pub csrf_token: String,
    pub(crate) token_hash: String,
}
#[derive(sqlx::FromRow)]
struct SessionUserRow {
    id: String,
    login: String,
    role: Role,
    display_name: Option<String>,
    sso: i64,
    last_seen_at: i64,
    now: i64,
}

impl AuthState {
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn allowed_origins(&self) -> &[String] {
        &self.allowed_origins
    }
    pub fn is_origin_allowed(&self, candidate: &str) -> bool {
        candidate == &*self.origin || self.allowed_origins.iter().any(|o| o.as_str() == candidate)
    }
    pub fn touch_stats(&self) -> SessionTouchStats {
        self.toucher.shared.stats()
    }
    pub async fn flush_session_touches(&self) {
        let notified = self.toucher.shared.flush_done.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.toucher.shared.stats().pending_count == 0 {
            return;
        }
        self.toucher.shared.notify.notify_one();
        let _ = tokio::time::timeout(Duration::from_secs(2), notified).await;
    }
    pub async fn new(store: IdentityStore, origin: String) -> Result<Self, AuthConfigError> {
        Self::new_with_touch_config(store, origin, TOUCH_INTERVAL_SECONDS, MAX_PENDING_TOUCHES)
            .await
    }
    pub async fn new_with_touch_config(
        store: IdentityStore,
        origin: String,
        touch_interval_seconds: i64,
        max_pending_touches: usize,
    ) -> Result<Self, AuthConfigError> {
        let uri: axum::http::Uri = origin.parse().map_err(|_| AuthConfigError::Origin)?;
        let scheme = uri.scheme_str().ok_or(AuthConfigError::Origin)?;
        let host = uri.host().ok_or(AuthConfigError::Origin)?;
        if uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
            || !(scheme == "https"
                || (scheme == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")))
        {
            return Err(AuthConfigError::Origin);
        }
        let secure = scheme == "https";
        let origin = origin.trim_end_matches('/').to_owned();
        if let Err(error) = crate::avatars::defaults::initialize(&store).await {
            // A full/unavailable asset volume must not prevent recovery login.
            // The marker is committed with the images, so a later start retries.
            eprintln!("Default avatar gallery initialization failed: {error}");
        }
        if let Err(error) = crate::avatars::defaults::ensure_required_missing(&store).await {
            // The read endpoint still serves an in-memory local fallback, even
            // if the asset volume cannot currently persist an assignment.
            eprintln!("Mandatory avatar assignment failed: {error}");
        }
        let dummy_hash = tokio::task::spawn_blocking(|| {
            let memory_kib = crate::identity::argon2_memory_cost_kib();
            let params =
                Params::new(memory_kib, 3, 1, Some(32)).map_err(|_| IdentityError::Hash)?;
            let h = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password(random_token().as_bytes())
                .map(|h| h.to_string())
                .map_err(|_| IdentityError::Hash);
            crate::system_memory::trim_memory();
            h
        })
        .await
        .map_err(|_| IdentityError::Worker)??;
        let toucher = SessionToucher::new(
            store.pool.clone(),
            touch_interval_seconds,
            max_pending_touches,
        );
        crate::identity::register_active_store(&store);
        let closed: i64 = crate::database::query_scalar(
            "SELECT CAST((EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM users)) AS INTEGER)",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or(1);

        let (bootstrap_token, bootstrap_file_path) = if closed != 0 || is_http_bootstrap_disabled()
        {
            (Arc::new(Mutex::new(None)), Arc::new(Mutex::new(None)))
        } else {
            let token = std::env::var("MEDIA_BOOTSTRAP_TOKEN")
                .or_else(|_| std::env::var("MEDIA_SETUP_TOKEN"))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(random_token);

            let token_path = std::env::var("MEDIA_DB_PATH")
                .ok()
                .map(std::path::PathBuf::from)
                .and_then(|p| p.parent().map(|d| d.join(".bootstrap_token")))
                .or_else(|| Some(std::path::PathBuf::from(".bootstrap_token")));

            if let Some(mut f) = token_path
                .as_ref()
                .and_then(|p| std::fs::File::create(p).ok())
            {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
                }
                let _ = std::io::Write::write_all(&mut f, token.as_bytes());
            }

            if !std::env::var("MEDIA_SILENT_BOOTSTRAP_BANNER")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                println!(
                    "================================================================================"
                );
                println!("[AETHER BOOTSTRAP] Initialisation sécurisée requise");
                println!("Aucun administrateur n'est encore configuré.");
                println!("Code d'initialisation unique : {}", token);
                if let Some(ref p) = token_path {
                    println!("Fichier de jeton temporaire : {}", p.display());
                }
                println!(
                    "Saisissez ce code dans l'interface Web ou transmettez-le via l'API bootstrap."
                );
                println!(
                    "================================================================================"
                );
            }

            (
                Arc::new(Mutex::new(Some(token))),
                Arc::new(Mutex::new(token_path)),
            )
        };

        let allowed_origins: Arc<[String]> =
            resolve_allowed_origins(&origin).into_boxed_slice().into();

        Ok(Self {
            store,
            origin: origin.into(),
            allowed_origins,
            secure,
            dummy_hash: dummy_hash.into(),
            workers: Arc::new(Semaphore::new(2)),
            toucher,
            bootstrap_token,
            bootstrap_file_path,
        })
    }
    pub fn get_bootstrap_token(&self) -> Option<String> {
        self.bootstrap_token
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn set_bootstrap_token(&self, token: Option<String>) {
        let mut guard = self
            .bootstrap_token
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *guard = token;
    }
    pub async fn authenticate(&self, headers: &HeaderMap) -> Result<Authenticated, StatusCode> {
        let raw = session_token(headers).ok_or(StatusCode::UNAUTHORIZED)?;
        let token_hash = hash(raw);
        let user = self.session_from_hash(&token_hash).await?;
        Ok(Authenticated {
            user,
            csrf_token: csrf(raw),
            token_hash,
        })
    }

    /// Internal capability checks share live validation and coalesced activity with P1.
    pub(crate) async fn session_from_hash(&self, token_hash: &str) -> Result<AuthUser, StatusCode> {
        // Validate session, user, role, active state, absolute expiry and idle expiry
        // via a pure SQLite read (WAL snapshot) so authenticated reads never block on write locks.
        let row: Option<SessionUserRow> = crate::database::query_as(
            "SELECT u.id, COALESCE(u.sso_username, c.login, u.display_name, 'sso-' || u.id) AS login, u.role, u.display_name, CAST((c.login IS NULL) AS INTEGER) AS sso, s.last_seen_at, unixepoch() AS now \
             FROM sessions s \
             JOIN users u ON u.id = s.user_id \
             LEFT JOIN local_credentials c ON c.user_id = u.id \
             WHERE s.token_hash = ? \
               AND s.expires_at > unixepoch() \
               AND s.last_seen_at > unixepoch() - ? \
               AND u.disabled_at IS NULL",
        )
        .bind(token_hash)
        .bind(IDLE_SECONDS)
        .fetch_optional(&self.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let row = row.ok_or(StatusCode::UNAUTHORIZED)?;
        if row.role == Role::User
            && !crate::product::access_now(&self.store, &row.id, row.now).await?
        {
            return Err(StatusCode::FORBIDDEN);
        }
        if row.now.saturating_sub(row.last_seen_at) >= self.toucher.shared.interval_seconds {
            self.toucher.shared.record(token_hash, row.now);
        }
        Ok(AuthUser {
            id: row.id,
            login: row.login,
            role: row.role,
            display_name: row.display_name,
            sso: Some(row.sso != 0),
        })
    }
    pub fn require_csrf(
        &self,
        headers: &HeaderMap,
        identity: &Authenticated,
    ) -> Result<(), StatusCode> {
        self.check_origin(headers)?;
        let provided = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .ok_or(StatusCode::FORBIDDEN)?;
        // Hash both equal-length values to avoid comparing unbounded attacker input.
        if provided.len() != 64
            || !constant_equal(
                hash(provided).as_bytes(),
                hash(&identity.csrf_token).as_bytes(),
            )
        {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(())
    }
    pub(crate) fn check_origin(&self, headers: &HeaderMap) -> Result<(), StatusCode> {
        let origin_header = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
        let is_primary_origin = origin_header == Some(&*self.origin);
        let is_trusted_external = origin_header
            .is_some_and(|cand| self.allowed_origins.iter().any(|ao| ao.as_str() == cand));

        let is_cross_site = headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site");

        // Allowed if primary origin and not cross-site, OR if explicitly trusted external origin
        let allowed = (is_primary_origin && !is_cross_site) || is_trusted_external;
        if !allowed {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(())
    }
    pub(crate) fn cookie(&self, token: &str, max_age: i64) -> String {
        format!(
            "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
            if self.secure { "; Secure" } else { "" }
        )
    }
}
