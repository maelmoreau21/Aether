//! Local identity persistence. No HTTP routes, sessions or OIDC implementation.
use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};

pub fn argon2_memory_cost_kib() -> u32 {
    std::env::var("MEDIA_ARGON2_MEMORY_KIB")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(19456, 262144))
        .unwrap_or(65536)
}
use sqlx::{AnyPool, any::AnyPoolOptions};
use std::{path::Path, time::Duration};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error(
        "invalid database configuration: use MEDIA_DB_ENGINE=sqlite (default) or postgres with MEDIA_DB_URL"
    )]
    Configuration,
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("initial bootstrap is already closed")]
    AlreadyInitialized,
    #[error(
        "login must contain 3..64 lowercase ASCII letters, digits, dots, underscores or hyphens"
    )]
    InvalidLogin,
    #[error(
        "password must contain 15..128 characters, at most 512 bytes, without control characters"
    )]
    InvalidPassword,
    #[error("password hashing failed")]
    Hash,
    #[error("password hashing worker failed")]
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(rename_all = "lowercase")]
pub enum Role {
    Administrator,
    User,
}
impl sqlx::Type<sqlx::Any> for Role {
    fn type_info() -> sqlx::any::AnyTypeInfo {
        <String as sqlx::Type<sqlx::Any>>::type_info()
    }
    fn compatible(ty: &sqlx::any::AnyTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Any>>::compatible(ty)
    }
}
impl<'r> sqlx::Decode<'r, sqlx::Any> for Role {
    fn decode(value: sqlx::any::AnyValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        match <String as sqlx::Decode<sqlx::Any>>::decode(value)?.as_str() {
            "administrator" => Ok(Self::Administrator),
            "user" => Ok(Self::User),
            _ => Err("invalid persisted role".into()),
        }
    }
}
impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Administrator => "administrator",
            Self::User => "user",
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub role: Role,
    pub created_at: i64,
    pub disabled_at: Option<i64>,
}

#[derive(Clone)]
pub struct IdentityStore {
    pub(crate) pool: AnyPool,
    pub(crate) plugins_path: std::path::PathBuf,
    pub(crate) assets: std::sync::Arc<crate::assets::AssetStore>,
    pub(crate) task_writes: std::sync::Arc<tokio::sync::Mutex<()>>,
    pub(crate) node_registry: std::sync::Arc<crate::nodes::NodeRuntimeRegistry>,
}
fn plugin_path(database: Option<&Path>) -> Result<std::path::PathBuf, IdentityError> {
    let path = std::env::var_os("MEDIA_PLUGINS_PATH")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            if let Some(parent) = database.and_then(Path::parent) {
                parent.join("plugins")
            } else if Path::new("/data").is_dir() {
                std::path::PathBuf::from("/data/plugins")
            } else {
                Path::new("data").join("plugins")
            }
        });
    std::path::absolute(path).map_err(|_| IdentityError::Configuration)
}
/// Local asset directory: `MEDIA_ASSETS_PATH`, else `assets` next to the database.
fn assets_path(database: Option<&Path>) -> Result<std::path::PathBuf, IdentityError> {
    let path = std::env::var_os("MEDIA_ASSETS_PATH")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            if let Some(parent) = database.and_then(Path::parent) {
                parent.join("assets")
            } else if Path::new("/data").is_dir() {
                std::path::PathBuf::from("/data/assets")
            } else {
                Path::new("data").join("assets")
            }
        });
    std::path::absolute(path).map_err(|_| IdentityError::Configuration)
}
fn open_assets(
    pool: &AnyPool,
    database: Option<&Path>,
) -> Result<std::sync::Arc<crate::assets::AssetStore>, IdentityError> {
    let backend = crate::assets::LocalFs::open(&assets_path(database)?, None)
        .map_err(|_| IdentityError::Configuration)?;
    let config =
        crate::assets::AssetConfig::from_env().map_err(|_| IdentityError::Configuration)?;
    Ok(std::sync::Arc::new(crate::assets::AssetStore::new(
        pool.clone(),
        std::sync::Arc::new(backend),
        config,
    )))
}

fn configure_pool_options(is_sqlite: bool) -> Result<AnyPoolOptions, IdentityError> {
    let default_max = if is_sqlite { 8 } else { 16 };
    let default_min = if is_sqlite { 1 } else { 2 };
    let default_acquire = if is_sqlite { 15 } else { 30 };

    let max = match std::env::var("MEDIA_DB_MAX_CONNECTIONS")
        .or_else(|_| std::env::var("MEDIA_DB_POOL_MAX"))
    {
        Ok(v) => match v.parse::<u32>() {
            Ok(n) if n > 0 => n,
            _ => return Err(IdentityError::Configuration),
        },
        Err(std::env::VarError::NotPresent) => default_max,
        Err(_) => return Err(IdentityError::Configuration),
    };

    let min = match std::env::var("MEDIA_DB_MIN_CONNECTIONS")
        .or_else(|_| std::env::var("MEDIA_DB_POOL_MIN"))
    {
        Ok(v) => match v.parse::<u32>() {
            Ok(n) => n.min(max),
            _ => return Err(IdentityError::Configuration),
        },
        Err(std::env::VarError::NotPresent) => default_min.min(max),
        Err(_) => return Err(IdentityError::Configuration),
    };

    let acquire_secs = match std::env::var("MEDIA_DB_ACQUIRE_TIMEOUT_SECS") {
        Ok(v) => match v.parse::<u64>() {
            Ok(n) if n > 0 => n,
            _ => return Err(IdentityError::Configuration),
        },
        Err(std::env::VarError::NotPresent) => default_acquire,
        Err(_) => return Err(IdentityError::Configuration),
    };

    let idle_secs = match std::env::var("MEDIA_DB_IDLE_TIMEOUT_SECS") {
        Ok(v) => match v.parse::<u64>() {
            Ok(n) if n > 0 => n,
            _ => return Err(IdentityError::Configuration),
        },
        Err(std::env::VarError::NotPresent) => 600,
        Err(_) => return Err(IdentityError::Configuration),
    };

    Ok(AnyPoolOptions::new()
        .max_connections(max)
        .min_connections(min)
        .acquire_timeout(Duration::from_secs(acquire_secs))
        .idle_timeout(Some(Duration::from_secs(idle_secs))))
}

impl IdentityStore {
    /// Shared store for durable application binaries (avatars, artwork, derived data).
    pub fn assets(&self) -> &std::sync::Arc<crate::assets::AssetStore> {
        &self.assets
    }
    /// Fail closed when the asset directory overlaps the read-only media root.
    pub fn check_assets_outside(&self, media_root: &Path) -> Result<(), IdentityError> {
        self.assets
            .ensure_outside(media_root)
            .map_err(|_| IdentityError::Configuration)
    }
    pub fn plugins_path(&self) -> &Path {
        &self.plugins_path
    }
    /// Explicit external PostgreSQL connection; never transfers SQLite data.
    pub async fn open_postgres(url: &str) -> Result<Self, IdentityError> {
        sqlx::any::install_default_drivers();
        let parsed = url::Url::parse(url).map_err(|_| IdentityError::Configuration)?;
        if !matches!(parsed.scheme(), "postgres" | "postgresql") {
            return Err(IdentityError::Configuration);
        }
        let pool = configure_pool_options(false)?.connect(url).await?;
        let mut conn = pool.acquire().await?;
        crate::database::initialize_fresh_schema(&mut conn).await?;
        crate::inventory::backfill_title_sort_unicode(&mut conn).await?;
        drop(conn);
        Ok(Self {
            assets: open_assets(&pool, None)?,
            pool,
            plugins_path: plugin_path(None)?,
            task_writes: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            node_registry: std::sync::Arc::new(crate::nodes::NodeRuntimeRegistry::new()),
        })
    }

    pub async fn open_configured(media_root: &Path) -> Result<Self, IdentityError> {
        match std::env::var("MEDIA_DB_ENGINE")
            .as_deref()
            .unwrap_or("sqlite")
        {
            "sqlite" => {
                if std::env::var("MEDIA_DB_URL").is_ok_and(|value| !value.is_empty()) {
                    return Err(IdentityError::Configuration);
                }
                let path = configured_path(media_root).map_err(|_| IdentityError::Configuration)?;
                let store = Self::open(&path).await?;
                store.check_assets_outside(media_root)?;
                Ok(store)
            }
            "postgres" => {
                let url = zeroize::Zeroizing::new(
                    std::env::var("MEDIA_DB_URL").map_err(|_| IdentityError::Configuration)?,
                );
                let store = Self::open_postgres(&url).await?;
                store.check_assets_outside(media_root)?;
                Ok(store)
            }
            _ => Err(IdentityError::Configuration),
        }
    }

    /// Operator-controlled local file, never supplied by an HTTP request.
    pub async fn open(path: &Path) -> Result<Self, IdentityError> {
        let busy_timeout = std::env::var("MEDIA_SQLITE_BUSY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(15));
        Self::open_with_timeout(path, busy_timeout).await
    }

    pub async fn open_with_timeout(
        path: &Path,
        busy_timeout: Duration,
    ) -> Result<Self, IdentityError> {
        sqlx::any::install_default_drivers();
        // An opaque URL preserves Windows drive letters, spaces and Unicode.
        let filename = path.to_str().ok_or(IdentityError::Configuration)?;
        let encoded: String = filename
            .as_bytes()
            .iter()
            .map(|b| format!("%{b:02X}"))
            .collect();
        let database_url = format!("sqlite:{encoded}?mode=rwc");
        // Concurrent first opens may race on PRAGMA journal_mode=WAL before the
        // busy handler can wait. Retry only SQLite BUSY/LOCKED, never corruption,
        // permissions or schema errors. Each attempt retains its timeout.
        let mut retries = 0;
        let pool = loop {
            let pool_opts = configure_pool_options(true)?;
            match pool_opts
                .after_connect(move |conn, _| {
                    Box::pin(async move {
                        crate::database::query("PRAGMA foreign_keys = ON")
                            .execute(&mut *conn)
                            .await?;
                        let timeout = sqlx::AssertSqlSafe(format!(
                            "PRAGMA busy_timeout = {}",
                            busy_timeout.as_millis()
                        ));
                        crate::database::query(timeout).execute(&mut *conn).await?;
                        crate::database::query("PRAGMA journal_mode = WAL")
                            .execute(&mut *conn)
                            .await?;
                        crate::database::query("PRAGMA synchronous = NORMAL")
                            .execute(&mut *conn)
                            .await?;
                        let cache_kib = std::env::var("MEDIA_SQLITE_CACHE_KIB")
                            .ok()
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(-32000);
                        let pragma_cache =
                            sqlx::AssertSqlSafe(format!("PRAGMA cache_size = {cache_kib}"));
                        crate::database::query(pragma_cache)
                            .execute(&mut *conn)
                            .await?;
                        let mmap_bytes = std::env::var("MEDIA_SQLITE_MMAP_BYTES")
                            .ok()
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(67108864);
                        let pragma_mmap =
                            sqlx::AssertSqlSafe(format!("PRAGMA mmap_size = {mmap_bytes}"));
                        crate::database::query(pragma_mmap)
                            .execute(&mut *conn)
                            .await?;
                        crate::database::query("PRAGMA temp_store = MEMORY")
                            .execute(&mut *conn)
                            .await?;
                        Ok(())
                    })
                })
                .connect(&database_url)
                .await
            {
                Ok(pool) => break pool,
                Err(sqlx::Error::Database(error))
                    if retries < 2
                        && error
                            .code()
                            .and_then(|code| code.parse::<i32>().ok())
                            .is_some_and(|code| matches!(code & 0xff, 5 | 6)) =>
                {
                    retries += 1;
                    tokio::time::sleep(Duration::from_millis(50 * retries)).await;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let mut conn = pool.acquire().await?;
        crate::database::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        crate::database::initialize_fresh_schema(&mut conn).await?;
        let mut tx = crate::database::begin_write(&mut conn).await?;
        crate::inventory::backfill_title_sort_unicode(&mut tx).await?;
        tx.commit().await?;
        crate::database::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;
        drop(conn);
        Ok(Self {
            assets: open_assets(&pool, Some(path))?,
            pool,
            plugins_path: plugin_path(Some(path))?,
            task_writes: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            node_registry: std::sync::Arc::new(crate::nodes::NodeRuntimeRegistry::new()),
        })
    }

    pub async fn user(&self, id: &str) -> Result<Option<User>, IdentityError> {
        Ok(crate::database::query_as(
            "SELECT id, role, created_at, disabled_at FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn close(&self) {
        if let Ok(mut conn) = self.pool.acquire().await
            && conn.backend_name() == "SQLite"
        {
            let _ = crate::database::query("PRAGMA wal_checkpoint(TRUNCATE)")
                .execute(&mut *conn)
                .await;
        }
        self.pool.close().await;
    }

    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    pub fn node_registry(&self) -> &crate::nodes::NodeRuntimeRegistry {
        &self.node_registry
    }

    pub async fn bootstrap(
        &self,
        login: &str,
        password: Zeroizing<String>,
    ) -> Result<String, IdentityError> {
        if !(3..=64).contains(&login.len())
            || !login
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
        {
            return Err(IdentityError::InvalidLogin);
        }
        if !(15..=128).contains(&password.chars().count())
            || password.len() > 512
            || password.chars().any(char::is_control)
        {
            return Err(IdentityError::InvalidPassword);
        }
        let hash = tokio::task::spawn_blocking(move || {
            let memory_kib = argon2_memory_cost_kib();
            let params =
                Params::new(memory_kib, 3, 1, Some(32)).map_err(|_| IdentityError::Hash)?;
            let h = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password(password.as_bytes())
                .map(|h| h.to_string())
                .map_err(|_| IdentityError::Hash);
            crate::system_memory::trim_memory();
            h
        })
        .await
        .map_err(|_| IdentityError::Worker)??;
        let mut connection = self.pool.acquire().await?;
        // Acquire SQLite's writer lock BEFORE reading state, across connections/processes.
        let mut tx = crate::database::begin_write(&mut connection).await?;
        let closed: i64 = crate::database::query_scalar(
            "SELECT CAST((EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM users)) AS INTEGER)",
        )
        .fetch_one(&mut *tx)
        .await?;
        if closed != 0 {
            return Err(IdentityError::AlreadyInitialized);
        }
        let id = uuid::Uuid::new_v4().to_string();
        crate::database::query("INSERT INTO users(id, role) VALUES (?, ?)")
            .bind(&id)
            .bind(Role::Administrator.as_str())
            .execute(&mut *tx)
            .await?;
        crate::database::query(
            "INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(login)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
        crate::database::query("INSERT INTO bootstrap_state(singleton) VALUES (1)")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }
}

/// Fail closed if the database or its directory resolves into the media root.
/// Directory ownership/ACL and protection against local symlink races are operator responsibilities.
pub fn configured_path(media_root: &Path) -> Result<std::path::PathBuf, std::io::Error> {
    let path = std::path::PathBuf::from(
        std::env::var_os("MEDIA_DB_PATH").unwrap_or_else(|| "data/core.sqlite3".into()),
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }
    let parent = parent.canonicalize()?;
    let resolved = if path.exists() {
        path.canonicalize()?
    } else {
        parent.join(
            path.file_name()
                .ok_or_else(|| std::io::Error::other("database filename required"))?,
        )
    };
    // A missing media root cannot expose media; still forbid its lexical location later through setup documentation.
    if let Ok(root) = media_root.canonicalize()
        && resolved.starts_with(root)
    {
        return Err(std::io::Error::other(
            "database must be outside the media directory",
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_pool_configuration_defaults_and_validation() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("MEDIA_DB_MAX_CONNECTIONS");
            std::env::remove_var("MEDIA_DB_POOL_MAX");
            std::env::remove_var("MEDIA_DB_MIN_CONNECTIONS");
            std::env::remove_var("MEDIA_DB_POOL_MIN");
            std::env::remove_var("MEDIA_DB_ACQUIRE_TIMEOUT_SECS");
            std::env::remove_var("MEDIA_DB_IDLE_TIMEOUT_SECS");
        }

        assert!(configure_pool_options(true).is_ok());
        assert!(configure_pool_options(false).is_ok());

        unsafe {
            std::env::set_var("MEDIA_DB_MAX_CONNECTIONS", "0");
        }
        assert!(matches!(
            configure_pool_options(true),
            Err(IdentityError::Configuration)
        ));

        unsafe {
            std::env::set_var("MEDIA_DB_MAX_CONNECTIONS", "invalid");
        }
        assert!(matches!(
            configure_pool_options(true),
            Err(IdentityError::Configuration)
        ));

        unsafe {
            std::env::set_var("MEDIA_DB_MAX_CONNECTIONS", "24");
            std::env::set_var("MEDIA_DB_MIN_CONNECTIONS", "4");
            std::env::set_var("MEDIA_DB_ACQUIRE_TIMEOUT_SECS", "45");
        }
        assert!(configure_pool_options(true).is_ok());

        unsafe {
            std::env::remove_var("MEDIA_DB_MAX_CONNECTIONS");
            std::env::remove_var("MEDIA_DB_MIN_CONNECTIONS");
            std::env::remove_var("MEDIA_DB_ACQUIRE_TIMEOUT_SECS");
        }
    }
}
