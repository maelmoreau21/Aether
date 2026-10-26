//! Local authentication. Session secrets never enter the database or application logs.
use crate::identity::{IdentityError, IdentityStore, Role};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Connection;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, Semaphore};
use zeroize::Zeroizing;

pub(crate) const ABSOLUTE_SECONDS: i64 = 30 * 24 * 60 * 60;
pub(crate) const IDLE_SECONDS: i64 = 7 * 24 * 60 * 60;
const TOUCH_INTERVAL_SECONDS: i64 = 60;
const MAX_PENDING_TOUCHES: usize = 4096;
const TOUCH_BATCH_SIZE: usize = 64;
const TOUCH_COALESCE_MS: u64 = 20;
const TOUCH_RETRY_BACKOFF_MS: u64 = 150;
const COOKIE: &str = "media_session";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionTouchStats {
    pub pending_count: usize,
    pub write_batches_succeeded: u64,
    pub rows_updated: u64,
    pub write_conflicts: u64,
    pub dropped_overflow: u64,
}

struct TouchShared {
    pending: Mutex<HashMap<String, i64>>,
    notify: Notify,
    flush_done: Notify,
    closed: AtomicBool,
    interval_seconds: i64,
    max_pending: usize,
    write_batches_succeeded: AtomicU64,
    rows_updated: AtomicU64,
    write_conflicts: AtomicU64,
    dropped_overflow: AtomicU64,
}

impl TouchShared {
    fn record(&self, token_hash: &str, now: i64) {
        let mut pending = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(existing) = pending.get_mut(token_hash) {
            if now > *existing {
                *existing = now;
            }
            return;
        }
        if pending.len() >= self.max_pending {
            self.dropped_overflow.fetch_add(1, Ordering::Relaxed);
            return;
        }
        pending.insert(token_hash.to_owned(), now);
        drop(pending);
        self.notify.notify_one();
    }

    fn cancel(&self, token_hash: &str) {
        let mut pending = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.remove(token_hash);
    }

    fn stats(&self) -> SessionTouchStats {
        let pending_count = match self.pending.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        };
        SessionTouchStats {
            pending_count,
            write_batches_succeeded: self.write_batches_succeeded.load(Ordering::Relaxed),
            rows_updated: self.rows_updated.load(Ordering::Relaxed),
            write_conflicts: self.write_conflicts.load(Ordering::Relaxed),
            dropped_overflow: self.dropped_overflow.load(Ordering::Relaxed),
        }
    }
}

struct SessionToucher {
    shared: Arc<TouchShared>,
}

impl Drop for SessionToucher {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.notify.notify_one();
    }
}

enum FlushError {
    BusyOrTimeout,
    Permanent,
}

impl SessionToucher {
    fn new(pool: sqlx::AnyPool, interval_seconds: i64, max_pending: usize) -> Arc<Self> {
        let shared = Arc::new(TouchShared {
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            flush_done: Notify::new(),
            closed: AtomicBool::new(false),
            interval_seconds: interval_seconds.max(1),
            max_pending: max_pending.max(1),
            write_batches_succeeded: AtomicU64::new(0),
            rows_updated: AtomicU64::new(0),
            write_conflicts: AtomicU64::new(0),
            dropped_overflow: AtomicU64::new(0),
        });
        let worker_shared = shared.clone();
        tokio::spawn(async move {
            Self::worker_loop(pool, worker_shared).await;
        });
        Arc::new(Self { shared })
    }

    async fn worker_loop(pool: sqlx::AnyPool, shared: Arc<TouchShared>) {
        loop {
            if shared.closed.load(Ordering::Acquire) || pool.is_closed() {
                shared.flush_done.notify_waiters();
                break;
            }
            let notified = shared.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let is_empty = match shared.pending.lock() {
                Ok(guard) => guard.is_empty(),
                Err(poisoned) => poisoned.into_inner().is_empty(),
            };
            if is_empty {
                notified.await;
                if shared.closed.load(Ordering::Acquire) || pool.is_closed() {
                    shared.flush_done.notify_waiters();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(TOUCH_COALESCE_MS)).await;
            }

            let batch: Vec<(String, i64)> = {
                let guard = match shared.pending.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                guard
                    .iter()
                    .take(TOUCH_BATCH_SIZE)
                    .map(|(k, v)| (k.clone(), *v))
                    .collect()
            };
            if batch.is_empty() {
                shared.flush_done.notify_waiters();
                continue;
            }

            match Self::flush_batch(&pool, &batch).await {
                Ok(rows_affected) => {
                    shared
                        .write_batches_succeeded
                        .fetch_add(1, Ordering::Relaxed);
                    shared
                        .rows_updated
                        .fetch_add(rows_affected, Ordering::Relaxed);
                    {
                        let mut guard = match shared.pending.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        for (token_hash, flushed_at) in &batch {
                            if let Some(&current_at) = guard.get(token_hash)
                                && current_at.saturating_sub(*flushed_at) < shared.interval_seconds
                            {
                                guard.remove(token_hash);
                            }
                        }
                    }
                    shared.flush_done.notify_waiters();
                }
                Err(FlushError::BusyOrTimeout) => {
                    shared.write_conflicts.fetch_add(1, Ordering::Relaxed);
                    shared.flush_done.notify_waiters();
                    if shared.closed.load(Ordering::Acquire) || pool.is_closed() {
                        break;
                    }
                    let sleep_notified = shared.notify.notified();
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(TOUCH_RETRY_BACKOFF_MS)) => {}
                        _ = sleep_notified, if shared.closed.load(Ordering::Acquire) => break,
                    }
                }
                Err(FlushError::Permanent) => {
                    {
                        let mut guard = match shared.pending.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        for (token_hash, _) in &batch {
                            guard.remove(token_hash);
                        }
                    }
                    shared.flush_done.notify_waiters();
                    if pool.is_closed() || shared.closed.load(Ordering::Acquire) {
                        break;
                    }
                }
            }
        }
    }

    async fn flush_batch(pool: &sqlx::AnyPool, batch: &[(String, i64)]) -> Result<u64, FlushError> {
        let mut conn = match tokio::time::timeout(Duration::from_millis(50), pool.acquire()).await {
            Ok(Ok(conn)) => conn,
            Ok(Err(sqlx::Error::PoolTimedOut)) | Err(_) => return Err(FlushError::BusyOrTimeout),
            Ok(Err(_)) => return Err(FlushError::Permanent),
        };
        let sqlite = conn.backend_name() == "SQLite";
        let orig_timeout: i64 = if sqlite {
            crate::database::query_scalar("PRAGMA busy_timeout")
                .fetch_one(&mut *conn)
                .await
                .unwrap_or(15_000)
        } else {
            0
        };
        if sqlite {
            let _ = crate::database::query("PRAGMA busy_timeout = 15")
                .execute(&mut *conn)
                .await;
        }
        let mut tx = conn.begin().await.map_err(|_| FlushError::Permanent)?;
        if !sqlite {
            crate::database::query("SET LOCAL lock_timeout = '15ms'")
                .execute(&mut *tx)
                .await
                .map_err(|_| FlushError::Permanent)?;
        }

        let mut qb = sqlx::QueryBuilder::<sqlx::Any>::new(
            "UPDATE sessions SET last_seen_at = CASE token_hash ",
        );
        for (token_hash, seen_at) in batch {
            qb.push("WHEN ");
            qb.push_bind(token_hash);
            qb.push(
                " THEN CASE WHEN last_seen_at > unixepoch() THEN unixepoch() WHEN last_seen_at > ",
            );
            qb.push_bind(*seen_at);
            qb.push(" THEN last_seen_at ELSE CASE WHEN unixepoch() < ");
            qb.push_bind(*seen_at);
            qb.push(" THEN unixepoch() ELSE ");
            qb.push_bind(*seen_at);
            qb.push(" END END ");
        }
        qb.push("ELSE last_seen_at END WHERE token_hash IN (");
        let mut separated = qb.separated(", ");
        for (token_hash, _) in batch {
            separated.push_bind(token_hash);
        }
        separated
            .push_unseparated(") AND expires_at > unixepoch() AND last_seen_at > unixepoch() - ");
        qb.push_bind(IDLE_SECONDS);
        qb.push(
            " AND EXISTS(SELECT 1 FROM users WHERE users.id = sessions.user_id AND disabled_at IS NULL)",
        );

        use sqlx::Execute;
        let mut built = qb.build();
        let args = built.take_arguments().map_err(|_| FlushError::Permanent)?;
        let sql = crate::database::normalize_sql(built.sql().as_str());
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql), args.unwrap_or_default())
            .execute(&mut *tx)
            .await;
        let result = match result {
            Ok(result) => tx.commit().await.map(|_| result),
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        };
        if sqlite {
            let mut restore_qb = sqlx::QueryBuilder::<sqlx::Any>::new("PRAGMA busy_timeout = ");
            restore_qb.push(orig_timeout.max(0));
            let _ = restore_qb.build().execute(&mut *conn).await;
        }

        match result {
            Ok(res) => Ok(res.rows_affected()),
            Err(sqlx::Error::PoolTimedOut) => Err(FlushError::BusyOrTimeout),
            Err(sqlx::Error::Database(err)) => {
                let is_busy = err
                    .code()
                    .is_some_and(|code| code == "55P03" || code == "57014")
                    || err
                        .code()
                        .and_then(|c| c.parse::<i32>().ok())
                        .is_some_and(|code| matches!(code & 0xff, 5 | 6))
                    || {
                        let msg = err.message().to_ascii_lowercase();
                        msg.contains("busy") || msg.contains("locked")
                    };
                if is_busy {
                    Err(FlushError::BusyOrTimeout)
                } else {
                    Err(FlushError::Permanent)
                }
            }
            Err(_) => Err(FlushError::Permanent),
        }
    }
}

#[derive(Clone)]
pub struct AuthState {
    pub(crate) store: IdentityStore,
    origin: Arc<str>,
    allowed_origins: Arc<[String]>,
    pub(crate) secure: bool,
    dummy_hash: Arc<str>,
    pub(crate) workers: Arc<Semaphore>,
    toucher: Arc<SessionToucher>,
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
#[derive(Deserialize)]
struct Login {
    login: String,
    password: String,
}
#[derive(Deserialize)]
struct BootstrapInput {
    login: String,
    password: String,
    #[serde(default)]
    bootstrap_token: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IdentityResponse {
    user: AuthUser,
    csrf_token: String,
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
pub fn router(state: AuthState) -> Router {
    Router::new()
        .route("/api/v1/auth/bootstrap/status", get(bootstrap_status))
        .route("/api/v1/auth/bootstrap", post(bootstrap))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(state)
}

pub fn is_local_login_enabled() -> bool {
    let disabled = std::env::var("MEDIA_AUTH_LOCAL_ENABLED")
        .map(|v| v.trim().eq_ignore_ascii_case("false"))
        .unwrap_or(false)
        || std::env::var("MEDIA_AUTH_DISABLE_LOCAL_LOGIN")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        || std::env::var("MEDIA_OIDC_ONLY")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    !disabled
}

pub fn is_http_bootstrap_disabled() -> bool {
    std::env::var("MEDIA_DISABLE_HTTP_BOOTSTRAP")
        .map(|v| v.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
        || std::env::var("MEDIA_BOOTSTRAP_HTTP_ENABLED")
            .map(|v| v.trim().eq_ignore_ascii_case("false"))
            .unwrap_or(false)
}

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

async fn bootstrap_status(State(state): State<AuthState>) -> Response {
    let closed: i64 = crate::database::query_scalar(
        "SELECT CAST((EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM users)) AS INTEGER)",
    )
    .fetch_one(&state.store.pool)
    .await
    .unwrap_or(1);
    let oidc_admin_configured = std::env::var("MEDIA_OIDC_ADMIN_GROUPS")
        .or_else(|_| std::env::var("MEDIA_OIDC_ADMIN_GROUP"))
        .or_else(|_| std::env::var("OIDC_ADMIN_GROUP"))
        .or_else(|_| std::env::var("OIDC_ADMIN_GROUPS"))
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let local_enabled = is_local_login_enabled();
    let http_bootstrap_enabled = !is_http_bootstrap_disabled();
    Json(serde_json::json!({
        "needsBootstrap": closed == 0 && !oidc_admin_configured,
        "localLoginEnabled": local_enabled,
        "oidcAdminConfigured": oidc_admin_configured,
        "httpBootstrapEnabled": http_bootstrap_enabled
    }))
    .into_response()
}

async fn bootstrap(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<BootstrapInput>,
) -> Response {
    if !is_local_login_enabled() {
        return failure(StatusCode::FORBIDDEN, "local_login_disabled");
    }
    if is_http_bootstrap_disabled() {
        return failure(StatusCode::FORBIDDEN, "http_bootstrap_disabled");
    }
    if let Err(status) = state.check_origin(&headers) {
        return failure(status, "forbidden");
    }
    let Ok(_permit) = state.workers.clone().try_acquire_owned() else {
        return failure(StatusCode::TOO_MANY_REQUESTS, "try_later");
    };

    let expected = state.get_bootstrap_token();
    let Some(expected_token) = expected else {
        return failure(StatusCode::FORBIDDEN, "bootstrap_closed");
    };

    let provided = input.bootstrap_token.as_deref().unwrap_or("").trim();
    if provided.is_empty()
        || provided.len() != expected_token.len()
        || !constant_equal(hash(provided).as_bytes(), hash(&expected_token).as_bytes())
    {
        return failure(StatusCode::UNAUTHORIZED, "invalid_bootstrap_token");
    }

    let login_str = input.login.trim();
    if login_str.is_empty()
        || login_str.len() > 64
        || !login_str
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    {
        return failure(StatusCode::BAD_REQUEST, "invalid_login");
    }
    if input.password.chars().count() < 15
        || input.password.len() > 512
        || input.password.chars().any(char::is_control)
    {
        return failure(StatusCode::BAD_REQUEST, "invalid_password");
    }
    let id = match state
        .store
        .bootstrap(login_str, Zeroizing::new(input.password))
        .await
    {
        Ok(id) => id,
        Err(IdentityError::AlreadyInitialized) => {
            return failure(StatusCode::FORBIDDEN, "bootstrap_closed");
        }
        Err(IdentityError::InvalidLogin) => {
            return failure(StatusCode::BAD_REQUEST, "invalid_login");
        }
        Err(IdentityError::InvalidPassword) => {
            return failure(StatusCode::BAD_REQUEST, "invalid_password");
        }
        Err(_) => {
            return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
        }
    };

    state.set_bootstrap_token(None);
    if let Some(path) = state
        .bootstrap_file_path
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        let _ = std::fs::remove_file(path);
    }

    let raw = random_token();
    if crate::database::query(
        "INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+? FROM users WHERE id=? AND disabled_at IS NULL",
    )
    .bind(hash(&raw))
    .bind(ABSOLUTE_SECONDS)
    .bind(&id)
    .execute(&state.store.pool)
    .await
    .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    let _ = crate::database::query("UPDATE sessions SET device_label=? WHERE token_hash=?")
        .bind(device_label(&headers))
        .bind(hash(&raw))
        .execute(&state.store.pool)
        .await;
    let mut auth_headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("{COOKIE}={raw}")) {
        auth_headers.insert(header::COOKIE, value);
    }
    let identity = match state.authenticate(&auth_headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "internal_error"),
    };
    let mut response = Json(IdentityResponse {
        user: identity.user,
        csrf_token: identity.csrf_token,
    })
    .into_response();
    if let Ok(value) = HeaderValue::from_str(&state.cookie(&raw, ABSOLUTE_SECONDS)) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}
pub async fn authenticate(
    state: &AuthState,
    headers: &HeaderMap,
) -> Result<Authenticated, StatusCode> {
    state.authenticate(headers).await
}
async fn login(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Response {
    if !is_local_login_enabled() {
        return failure(StatusCode::FORBIDDEN, "local_login_disabled");
    }
    if let Err(status) = state.check_origin(&headers) {
        return failure(status, "forbidden");
    }
    let Ok(_permit) = state.workers.clone().try_acquire_owned() else {
        return failure(StatusCode::TOO_MANY_REQUESTS, "try_later");
    };
    // Scoped throttle: budget per client IP, durable across restarts.
    let client_key = extract_client_key(&headers);
    let allowed = crate::database::query_scalar::<_, i64>(
        "INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES(?,unixepoch(),1) ON CONFLICT(throttle_key) DO UPDATE SET window_started=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN unixepoch() ELSE login_throttle.window_started END, attempts=CASE WHEN login_throttle.window_started<=unixepoch()-60 THEN 1 ELSE login_throttle.attempts+1 END WHERE login_throttle.window_started<=unixepoch()-60 OR login_throttle.attempts<10 RETURNING login_throttle.attempts",
    )
    .bind(&client_key)
    .fetch_optional(&state.store.pool)
    .await;
    match allowed {
        Ok(Some(_)) => {}
        Ok(None) => return failure(StatusCode::TOO_MANY_REQUESTS, "try_later"),
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    }
    let _ = crate::database::query(
        "DELETE FROM login_throttle WHERE window_started <= unixepoch() - 300",
    )
    .execute(&state.store.pool)
    .await;
    if input.login.len() > 64 || input.password.len() > 512 {
        return failure(StatusCode::UNAUTHORIZED, "invalid_credentials");
    }
    let row=crate::database::query_as::<_,(String,String)>("SELECT c.user_id,c.password_hash FROM local_credentials c JOIN users u ON u.id=c.user_id WHERE c.login=? AND u.disabled_at IS NULL").bind(&input.login).fetch_optional(&state.store.pool).await;
    let row = match row {
        Ok(r) => r,
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
    };
    let encoded = row
        .as_ref()
        .map(|r| r.1.clone())
        .unwrap_or_else(|| state.dummy_hash.to_string());
    let password = Zeroizing::new(input.password);
    let verified = tokio::task::spawn_blocking(move || {
        // Keep the permit inside the worker even if its HTTP caller disconnects.
        let _permit = _permit;
        let Ok(parsed) = PasswordHash::new(&encoded) else {
            return false;
        };
        let ok = encoded.starts_with("$argon2id$")
            && Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok();
        crate::system_memory::trim_memory();
        ok
    })
    .await
    .unwrap_or(false);
    let Some((id, _)) = row.filter(|_| verified) else {
        return failure(StatusCode::UNAUTHORIZED, "invalid_credentials");
    };
    let raw = random_token();
    // Opportunistic bounded-state cleanup; no secret material is persisted.
    if crate::database::query(
        "DELETE FROM sessions WHERE expires_at<=unixepoch() OR last_seen_at<=unixepoch()-?",
    )
    .bind(IDLE_SECONDS)
    .execute(&state.store.pool)
    .await
    .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    if crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) SELECT ?,id,unixepoch(),unixepoch(),unixepoch()+? FROM users WHERE id=? AND disabled_at IS NULL").bind(hash(&raw)).bind(ABSOLUTE_SECONDS).bind(&id).execute(&state.store.pool).await.is_err(){return failure(StatusCode::INTERNAL_SERVER_ERROR,"internal_error")}
    let _ = crate::database::query("UPDATE sessions SET device_label=? WHERE token_hash=?")
        .bind(device_label(&headers))
        .bind(hash(&raw))
        .execute(&state.store.pool)
        .await;
    let mut auth_headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("{COOKIE}={raw}")) {
        auth_headers.insert(header::COOKIE, value);
    }
    let identity = match state.authenticate(&auth_headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "invalid_credentials"),
    };
    let mut response = Json(IdentityResponse {
        user: identity.user,
        csrf_token: identity.csrf_token,
    })
    .into_response();
    if let Ok(cookie) = HeaderValue::from_str(&state.cookie(&raw, ABSOLUTE_SECONDS)) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}
async fn me(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    match state.authenticate(&headers).await {
        Ok(i) => Json(IdentityResponse {
            user: i.user,
            csrf_token: i.csrf_token,
        })
        .into_response(),
        Err(s) => failure(s, "authentication_required"),
    }
}
async fn logout(State(state): State<AuthState>, headers: HeaderMap) -> Response {
    let identity = match state.authenticate(&headers).await {
        Ok(i) => i,
        Err(s) => return failure(s, "authentication_required"),
    };
    if let Err(s) = state.require_csrf(&headers, &identity) {
        return failure(s, "forbidden");
    }
    state.toucher.shared.cancel(&identity.token_hash);
    if crate::database::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(&identity.token_hash)
        .execute(&state.store.pool)
        .await
        .is_err()
    {
        return failure(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");
    }
    state.toucher.shared.cancel(&identity.token_hash);
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(cookie) = HeaderValue::from_str(&state.cookie("", 0)) {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}
fn failure(status: StatusCode, code: &str) -> Response {
    (status, Json(serde_json::json!({"error":code}))).into_response()
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
pub(crate) fn resolve_allowed_origins(primary_origin: &str) -> Vec<String> {
    let mut origins = Vec::new();

    let env_val =
        std::env::var("MEDIA_ALLOWED_ORIGINS").or_else(|_| std::env::var("MEDIA_TRUSTED_ORIGINS"));
    if let Ok(raw) = env_val {
        for item in raw.split(',') {
            let trimmed = item.trim().trim_end_matches('/');
            if !trimmed.is_empty()
                && trimmed != primary_origin
                && !origins.iter().any(|o| o == trimmed)
                && let Ok(uri) = trimmed.parse::<axum::http::Uri>()
                && uri.scheme().is_some()
                && uri.authority().is_some()
            {
                origins.push(trimmed.to_string());
            }
        }
    }

    if let Ok(issuer) = std::env::var("MEDIA_OIDC_ISSUER")
        && let Ok(parsed) = url::Url::parse(issuer.trim())
    {
        let issuer_origin = parsed.origin().ascii_serialization();
        if issuer_origin != "null"
            && issuer_origin != primary_origin
            && !origins.iter().any(|o| o == &issuer_origin)
        {
            origins.push(issuer_origin);
        }
    }

    origins
}

fn csrf(raw: &str) -> String {
    hash(&format!("csrf:{raw}"))
}
fn constant_equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}
fn session_token(headers: &HeaderMap) -> Option<&str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    const ORIGIN: &str = "http://127.0.0.1:3000";
    const PASSWORD: &str = "synthetic auth test password";
    async fn setup() -> (tempfile::TempDir, AuthState) {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("auth.sqlite3"))
            .await
            .unwrap();
        store
            .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
            .await
            .unwrap();
        let state = AuthState::new(store, ORIGIN.into()).await.unwrap();
        (temp, state)
    }
    async fn request(
        state: &AuthState,
        path: &str,
        method: &str,
        cookie: Option<&str>,
        origin: Option<&str>,
        csrf: Option<&str>,
        body: serde_json::Value,
    ) -> Response {
        let mut req = Request::builder()
            .uri(path)
            .method(method)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(v) = cookie {
            req = req.header(header::COOKIE, v)
        }
        if let Some(v) = origin {
            req = req.header(header::ORIGIN, v)
        }
        if let Some(v) = csrf {
            req = req.header("x-csrf-token", v)
        }
        router(state.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
    #[allow(clippy::too_many_arguments)]
    async fn request_with_ip(
        state: &AuthState,
        path: &str,
        method: &str,
        cookie: Option<&str>,
        origin: Option<&str>,
        csrf: Option<&str>,
        ip: Option<&str>,
        body: serde_json::Value,
    ) -> Response {
        let mut req = Request::builder()
            .uri(path)
            .method(method)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(v) = cookie {
            req = req.header(header::COOKIE, v);
        }
        if let Some(v) = origin {
            req = req.header(header::ORIGIN, v);
        }
        if let Some(v) = csrf {
            req = req.header("x-csrf-token", v);
        }
        if let Some(v) = ip {
            req = req.header("x-forwarded-for", v);
        }
        router(state.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
    async fn sign_in(state: &AuthState) -> (String, String) {
        let response = request(
            state,
            "/api/v1/auth/login",
            "POST",
            None,
            Some(ORIGIN),
            None,
            serde_json::json!({"login":"admin","password":PASSWORD}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly; SameSite=Strict"));
        assert!(!cookie.contains("Secure"));
        let body: serde_json::Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["user"]["role"], "administrator");
        (
            cookie.split(';').next().unwrap().into(),
            body["csrfToken"].as_str().unwrap().into(),
        )
    }
    #[tokio::test]
    async fn login_persistence_expiry_csrf_and_revocation() {
        let (temp, state) = setup().await;
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/me",
                "GET",
                None,
                None,
                None,
                serde_json::json!(null)
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let (cookie, csrf) = sign_in(&state).await;
        let (second_cookie, _) = sign_in(&state).await;
        assert_ne!(cookie, second_cookie);
        let raw = cookie.split('=').nth(1).unwrap();
        let hashes: Vec<String> = crate::database::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&state.store.pool)
            .await
            .unwrap();
        assert!(hashes.contains(&hash(raw)));
        assert_eq!(hashes.len(), 2);
        assert_ne!(hashes[0], raw);
        state.store.close().await;
        let reopened = AuthState::new(
            IdentityStore::open(&temp.path().join("auth.sqlite3"))
                .await
                .unwrap(),
            ORIGIN.into(),
        )
        .await
        .unwrap();
        let headers =
            HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
        assert!(reopened.authenticate(&headers).await.is_ok());
        for (origin, token) in [
            (None, Some(csrf.as_str())),
            (Some("https://attacker.invalid"), Some(csrf.as_str())),
            (Some(ORIGIN), None),
            (Some(ORIGIN), Some("wrong")),
        ] {
            assert_eq!(
                request(
                    &reopened,
                    "/api/v1/auth/logout",
                    "POST",
                    Some(&cookie),
                    origin,
                    token,
                    serde_json::json!(null)
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert!(reopened.authenticate(&headers).await.is_ok());
        assert_eq!(
            request(
                &reopened,
                "/api/v1/auth/logout",
                "POST",
                Some(&cookie),
                Some(ORIGIN),
                Some(&csrf),
                serde_json::json!(null)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert!(matches!(
            reopened.authenticate(&headers).await,
            Err(StatusCode::UNAUTHORIZED)
        ));
        for query in [
            "UPDATE sessions SET last_seen_at=0",
            "UPDATE sessions SET expires_at=0",
        ] {
            let (cookie, _) = sign_in(&reopened).await;
            crate::database::query(query)
                .execute(&reopened.store.pool)
                .await
                .unwrap();
            let headers =
                HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
            assert!(matches!(
                reopened.authenticate(&headers).await,
                Err(StatusCode::UNAUTHORIZED)
            ));
        }
        let (cookie, _) = sign_in(&reopened).await;
        crate::database::query("UPDATE users SET disabled_at=unixepoch()")
            .execute(&reopened.store.pool)
            .await
            .unwrap();
        let headers =
            HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);
        assert!(matches!(
            reopened.authenticate(&headers).await,
            Err(StatusCode::UNAUTHORIZED)
        ));
    }
    #[tokio::test]
    async fn generic_credentials_durable_throttle_and_origin() {
        let (temp, state) = setup().await;
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                None,
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let mut bodies = Vec::new();
        for login in ["admin", "missing"] {
            let response = request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":login,"password":"wrong"}),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            bodies.push(response.into_body().collect().await.unwrap().to_bytes());
        }
        assert_eq!(bodies[0], bodies[1]);
        crate::database::query("UPDATE login_throttle SET attempts=8")
            .execute(&state.store.pool)
            .await
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                request(
                    &state,
                    "/api/v1/auth/login",
                    "POST",
                    None,
                    Some(ORIGIN),
                    None,
                    serde_json::json!({"login":"missing","password":"wrong"})
                )
                .await
                .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        state.store.close().await;
        let reopened_store = IdentityStore::open(&temp.path().join("auth.sqlite3"))
            .await
            .unwrap();
        // Keep throttle window fresh across test restarts under heavy CI load
        crate::database::query("UPDATE login_throttle SET window_started = unixepoch()")
            .execute(&reopened_store.pool)
            .await
            .unwrap();
        let restarted = AuthState::new(reopened_store, ORIGIN.into()).await.unwrap();
        assert_eq!(
            request(
                &restarted,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );

        // Different client IP is NOT affected by the throttle on 127.0.0.1 (Anti-DoS)
        let other_ip_res = request_with_ip(
            &restarted,
            "/api/v1/auth/login",
            "POST",
            None,
            Some(ORIGIN),
            None,
            Some("198.51.100.42"),
            serde_json::json!({"login":"admin","password":PASSWORD}),
        )
        .await;
        assert_eq!(other_ip_res.status(), StatusCode::OK);

        crate::database::query("UPDATE login_throttle SET window_started=0")
            .execute(&restarted.store.pool)
            .await
            .unwrap();
        sign_in(&restarted).await;
        assert!(
            AuthState::new(restarted.store.clone(), "http://192.168.1.2:3000".into())
                .await
                .is_err()
        );
        let secure = AuthState::new(restarted.store.clone(), "https://media.example".into())
            .await
            .unwrap();
        assert!(secure.cookie("test", 100).ends_with("; Secure"));
        let permit1 = restarted.workers.clone().acquire_owned().await.unwrap();
        let permit2 = restarted.workers.clone().acquire_owned().await.unwrap();
        assert_eq!(
            request(
                &restarted,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(ORIGIN),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD})
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        drop((permit1, permit2));
    }

    #[tokio::test]
    async fn edge_cases_coalescing_expiry_saturation_logout_and_restart() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("auth_edges.sqlite3");
        let store = IdentityStore::open(&db_path).await.unwrap();
        let admin_id = store
            .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
            .await
            .unwrap();
        // Configure max_pending_touches = 4 to test saturation and backpressure deterministically.
        let state = AuthState::new_with_touch_config(store.clone(), ORIGIN.into(), 60, 4)
            .await
            .unwrap();
        let (cookie, csrf) = sign_in(&state).await;
        let raw = cookie.split('=').nth(1).unwrap();
        let token_hash = hash(raw);
        let headers =
            HeaderMap::from_iter([(header::COOKIE, HeaderValue::from_str(&cookie).unwrap())]);

        // 1. Fresh session (< 60s old): 20 concurrent requests cause 0 SQLite writes.
        let mut tasks = Vec::new();
        for _ in 0..20 {
            let s = state.clone();
            let h = headers.clone();
            tasks.push(tokio::spawn(
                async move { s.authenticate(&h).await.unwrap() },
            ));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap().user.role, Role::Administrator);
        }
        assert_eq!(state.touch_stats().write_batches_succeeded, 0);
        assert_eq!(state.touch_stats().pending_count, 0);

        // 2. Activity near idle expiration boundary (last_seen_at = now - 604795s, 5s before 604800s limit):
        // 30 concurrent requests from the same user coalesce into 1 single SQLite write batch.
        crate::database::query(
            "UPDATE sessions SET last_seen_at = unixepoch() - 604795 WHERE token_hash = ?",
        )
        .bind(&token_hash)
        .execute(&store.pool)
        .await
        .unwrap();
        let before_seen: i64 =
            crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
                .bind(&token_hash)
                .fetch_one(&store.pool)
                .await
                .unwrap();

        let mut concurrent = Vec::new();
        for _ in 0..30 {
            let s = state.clone();
            let h = headers.clone();
            concurrent.push(tokio::spawn(
                async move { s.authenticate(&h).await.unwrap() },
            ));
        }
        for t in concurrent {
            t.await.unwrap();
        }
        state.flush_session_touches().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let after_seen: i64 =
            crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
                .bind(&token_hash)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert!(
            after_seen > before_seen + 1700,
            "Session near expiration must be refreshed to current unixepoch"
        );
        assert_eq!(
            state.touch_stats().write_batches_succeeded,
            1,
            "30 concurrent requests must coalesce into exactly 1 SQLite write batch"
        );
        assert_eq!(state.touch_stats().rows_updated, 1);

        // 3. Delayed touch never revives a session that expired while SQLite was write-locked:
        let expired_raw = "e".repeat(64);
        let expired_hash = hash(&expired_raw);
        crate::database::query(
            "INSERT INTO sessions(token_hash, user_id, created_at, last_seen_at, expires_at) \
             VALUES(?, ?, unixepoch() - 2000, unixepoch() - 604790, unixepoch() + 3600)",
        )
        .bind(&expired_hash)
        .bind(&admin_id)
        .execute(&store.pool)
        .await
        .unwrap();

        let mut lock_conn = store.pool.acquire().await.unwrap();
        let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();

        let exp_headers = HeaderMap::from_iter([(
            header::COOKIE,
            HeaderValue::from_str(&format!("{COOKIE}={expired_raw}")).unwrap(),
        )]);
        // Still valid during the lock (604790 < 604800) -> queues touch in memory
        assert!(state.authenticate(&exp_headers).await.is_ok());
        assert_eq!(state.touch_stats().pending_count, 1);
        lock_tx.rollback().await.unwrap();
        drop(lock_conn);

        // Simulate session crossing the 604800s boundary before worker flushes
        crate::database::query(
            "UPDATE sessions SET last_seen_at = unixepoch() - 604805 WHERE token_hash = ?",
        )
        .bind(&expired_hash)
        .execute(&store.pool)
        .await
        .unwrap();
        state.flush_session_touches().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            matches!(
                state.authenticate(&exp_headers).await,
                Err(StatusCode::UNAUTHORIZED)
            ),
            "Deferred touch must NEVER revive an already-expired session"
        );

        // 4. Queue saturation (max_pending = 4) under write lock:
        let mut lock_conn = store.pool.acquire().await.unwrap();
        let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();

        for i in 0..8u8 {
            let tok = format!("{:02x}", i).repeat(32);
            state.toucher.shared.record(&hash(&tok), 2_000_000_000);
        }
        let stats_under_lock = state.touch_stats();
        assert_eq!(
            stats_under_lock.pending_count, 4,
            "Queue must be strictly bounded to 4"
        );
        assert_eq!(
            stats_under_lock.dropped_overflow, 4,
            "Overflow touches beyond capacity must be dropped with backpressure counter"
        );
        // Reads still succeed immediately while queue is saturated and DB is write-locked!
        assert!(state.authenticate(&headers).await.is_ok());

        lock_tx.rollback().await.unwrap();
        drop(lock_conn);
        state.flush_session_touches().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(state.touch_stats().pending_count, 0);

        // 5. Logout while an activity update is pending:
        crate::database::query(
            "UPDATE sessions SET last_seen_at = unixepoch() - 300 WHERE token_hash = ?",
        )
        .bind(&token_hash)
        .execute(&store.pool)
        .await
        .unwrap();
        let mut lock_conn = store.pool.acquire().await.unwrap();
        let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();
        assert!(state.authenticate(&headers).await.is_ok());
        assert_eq!(state.touch_stats().pending_count, 1);
        lock_tx.rollback().await.unwrap();
        drop(lock_conn);

        assert_eq!(
            request(
                &state,
                "/api/v1/auth/logout",
                "POST",
                Some(&cookie),
                Some(ORIGIN),
                Some(&csrf),
                serde_json::json!(null),
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            state.touch_stats().pending_count,
            0,
            "Logout must immediately cancel pending touch"
        );
        state.flush_session_touches().await;
        assert!(
            matches!(
                state.authenticate(&headers).await,
                Err(StatusCode::UNAUTHORIZED)
            ),
            "Logged-out session must remain revoked after flush"
        );

        // 6. Server restart before activity persistence (unflushed touch in memory):
        let (restart_cookie, _) = sign_in(&state).await;
        let restart_raw = restart_cookie.split('=').nth(1).unwrap();
        let restart_hash = hash(restart_raw);
        let restart_headers = HeaderMap::from_iter([(
            header::COOKIE,
            HeaderValue::from_str(&restart_cookie).unwrap(),
        )]);
        crate::database::query(
            "UPDATE sessions SET last_seen_at = unixepoch() - 600000 WHERE token_hash = ?",
        )
        .bind(&restart_hash)
        .execute(&store.pool)
        .await
        .unwrap();
        let persisted_before_restart: i64 =
            crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
                .bind(&restart_hash)
                .fetch_one(&store.pool)
                .await
                .unwrap();

        // Hold write lock so touch stays in memory, then simulate abrupt restart before flush
        let mut lock_conn = store.pool.acquire().await.unwrap();
        let lock_tx = crate::database::begin_write(&mut lock_conn).await.unwrap();
        assert!(state.authenticate(&restart_headers).await.is_ok());
        assert_eq!(state.touch_stats().pending_count, 1);
        drop(state);
        lock_tx.rollback().await.unwrap();
        drop(lock_conn);
        store.close().await;

        let reopened_store = IdentityStore::open(&db_path).await.unwrap();
        let reopened_state = AuthState::new(reopened_store.clone(), ORIGIN.into())
            .await
            .unwrap();
        let persisted_after_restart: i64 =
            crate::database::query_scalar("SELECT last_seen_at FROM sessions WHERE token_hash = ?")
                .bind(&restart_hash)
                .fetch_one(&reopened_store.pool)
                .await
                .unwrap();
        assert_eq!(
            persisted_before_restart, persisted_after_restart,
            "Unpersisted in-memory touch is discarded on abrupt restart"
        );
        // Because persisted last_seen_at is still < 604800s old, session remains valid after restart
        assert!(reopened_state.authenticate(&restart_headers).await.is_ok());
    }

    #[tokio::test]
    async fn origin_configuration_http_loopback_http_lan_and_https_matrix() {
        let (_temp, base_state) = setup().await;
        let store = base_state.store.clone();

        // 1. HTTP loopback origins (127.0.0.1, localhost, [::1], with or without trailing slash) are accepted
        for (input, canonical) in [
            ("http://127.0.0.1:3000", "http://127.0.0.1:3000"),
            ("http://127.0.0.1:3000/", "http://127.0.0.1:3000"),
            ("http://localhost:3000", "http://localhost:3000"),
            ("http://[::1]:3000", "http://[::1]:3000"),
        ] {
            let state = AuthState::new(store.clone(), input.into()).await.unwrap();
            assert_eq!(state.origin(), canonical);
            assert!(!state.cookie("tok", 300).contains("; Secure"));
        }

        // 2. HTTP LAN and non-loopback origins are explicitly rejected with AuthConfigError::Origin
        for rejected in [
            "http://192.168.1.100:3000",
            "http://10.0.0.10:3000",
            "http://172.16.0.5:3000",
            "http://0.0.0.0:3000",
            "http://nas.local:3000",
            "http://aether.mondomaine.fr",
            "https://admin@aether.mondomaine.fr",
            "https://aether.mondomaine.fr/app",
            "https://aether.mondomaine.fr?foo=bar",
            "ws://127.0.0.1:3000",
        ] {
            assert!(
                matches!(
                    AuthState::new(store.clone(), rejected.into()).await,
                    Err(AuthConfigError::Origin)
                ),
                "Expected AuthConfigError::Origin for {rejected}"
            );
        }

        // 3. HTTPS LAN / Internet origins (standard port 443 and custom port e.g. 8443 on Synology DSM) are accepted
        for (input, canonical) in [
            (
                "https://aether.mondomaine.fr",
                "https://aether.mondomaine.fr",
            ),
            (
                "https://aether.mondomaine.fr/",
                "https://aether.mondomaine.fr",
            ),
            ("https://aether.nas.lan:8443", "https://aether.nas.lan:8443"),
            ("https://192.168.1.100:8443", "https://192.168.1.100:8443"),
            ("https://[::1]:8443", "https://[::1]:8443"),
        ] {
            let state = AuthState::new(store.clone(), input.into()).await.unwrap();
            assert_eq!(state.origin(), canonical);
            let set_cookie = state.cookie("tok", 300);
            assert!(set_cookie.contains("HttpOnly; SameSite=Strict"));
            assert!(set_cookie.ends_with("; Secure"));

            // Reset login throttle before testing login on this HTTPS origin
            crate::database::query("DELETE FROM login_throttle")
                .execute(&store.pool)
                .await
                .unwrap();

            // Login with exact canonical HTTPS Origin succeeds and emits a Secure cookie
            let login_res = request(
                &state,
                "/api/v1/auth/login",
                "POST",
                None,
                Some(canonical),
                None,
                serde_json::json!({"login":"admin","password":PASSWORD}),
            )
            .await;
            assert_eq!(login_res.status(), StatusCode::OK);
            let raw_set_cookie = login_res.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .to_owned();
            assert!(raw_set_cookie.ends_with("; Secure"));
            let cookie_pair = raw_set_cookie.split(';').next().unwrap().to_owned();
            let body: serde_json::Value =
                serde_json::from_slice(&login_res.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            let csrf_tok = body["csrfToken"].as_str().unwrap().to_owned();

            // Mismatched scheme (http:// instead of https://) or mismatched port is rejected with 403
            let wrong_scheme = canonical.replacen("https://", "http://", 1);
            assert_eq!(
                request(
                    &state,
                    "/api/v1/auth/logout",
                    "POST",
                    Some(&cookie_pair),
                    Some(&wrong_scheme),
                    Some(&csrf_tok),
                    serde_json::json!(null),
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );

            // Exact canonical HTTPS Origin + valid CSRF token succeeds
            assert_eq!(
                request(
                    &state,
                    "/api/v1/auth/logout",
                    "POST",
                    Some(&cookie_pair),
                    Some(canonical),
                    Some(&csrf_tok),
                    serde_json::json!(null),
                )
                .await
                .status(),
                StatusCode::NO_CONTENT
            );
        }
    }

    #[tokio::test]
    async fn bootstrap_status_and_bootstrap_web_flow() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("bootstrap_test.sqlite3");
        let store = IdentityStore::open(&db).await.unwrap();
        let state = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();

        // 1. Initial state: needsBootstrap is true, httpBootstrapEnabled is true
        let status_res = request(
            &state,
            "/api/v1/auth/bootstrap/status",
            "GET",
            None,
            None,
            None,
            serde_json::json!(null),
        )
        .await;
        assert_eq!(status_res.status(), StatusCode::OK);
        let status_bytes = status_res.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&status_bytes).unwrap();
        assert_eq!(body["needsBootstrap"], true);
        assert_eq!(body["httpBootstrapEnabled"], true);

        let token = state
            .get_bootstrap_token()
            .expect("bootstrap token must be generated");

        // 2. Missing or wrong token rejected with 401 Unauthorized
        let wrong_token_res = request(
            &state,
            "/api/v1/auth/bootstrap",
            "POST",
            None,
            Some("http://127.0.0.1:3000"),
            None,
            serde_json::json!({
                "login": "admin",
                "password": "my-secure-admin-password-1234",
                "bootstrap_token": "wrong-token-value"
            }),
        )
        .await;
        assert_eq!(wrong_token_res.status(), StatusCode::UNAUTHORIZED);

        // 3. Invalid password (< 15 chars) with valid token rejected
        let invalid_res = request(
            &state,
            "/api/v1/auth/bootstrap",
            "POST",
            None,
            Some("http://127.0.0.1:3000"),
            None,
            serde_json::json!({
                "login": "admin",
                "password": "short",
                "bootstrap_token": token
            }),
        )
        .await;
        assert_eq!(invalid_res.status(), StatusCode::BAD_REQUEST);

        // 4. Valid bootstrap with valid token succeeds
        let valid_res = request(
            &state,
            "/api/v1/auth/bootstrap",
            "POST",
            None,
            Some("http://127.0.0.1:3000"),
            None,
            serde_json::json!({
                "login": "admin",
                "password": "my-secure-admin-password-1234",
                "bootstrap_token": token
            }),
        )
        .await;
        assert_eq!(valid_res.status(), StatusCode::OK);
        let cookie_header = valid_res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie_header.contains("media_session="));
        let valid_bytes = valid_res.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&valid_bytes).unwrap();
        assert_eq!(body["user"]["role"], "administrator");
        assert_eq!(body["user"]["login"], "admin");
        assert!(state.get_bootstrap_token().is_none());

        // 5. After bootstrap, needsBootstrap is false
        let status_res2 = request(
            &state,
            "/api/v1/auth/bootstrap/status",
            "GET",
            None,
            None,
            None,
            serde_json::json!(null),
        )
        .await;
        let status_bytes2 = status_res2.into_body().collect().await.unwrap().to_bytes();
        let body2: serde_json::Value = serde_json::from_slice(&status_bytes2).unwrap();
        assert_eq!(body2["needsBootstrap"], false);

        // 6. Repeated bootstrap attempt is rejected with 403 Forbidden
        let repeated_res = request(
            &state,
            "/api/v1/auth/bootstrap",
            "POST",
            None,
            Some("http://127.0.0.1:3000"),
            None,
            serde_json::json!({
                "login": "hacker",
                "password": "another-password-12345",
                "bootstrap_token": token
            }),
        )
        .await;
        assert_eq!(repeated_res.status(), StatusCode::FORBIDDEN);
    }
}
