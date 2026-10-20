//! Local identity persistence. No HTTP routes, sessions or OIDC implementation.
use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
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
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
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
}
impl IdentityStore {
    /// Explicit external PostgreSQL connection; never transfers SQLite data.
    pub async fn open_postgres(url: &str) -> Result<Self, IdentityError> {
        sqlx::any::install_default_drivers();
        let parsed = url::Url::parse(url).map_err(|_| IdentityError::Configuration)?;
        if !matches!(parsed.scheme(), "postgres" | "postgresql") {
            return Err(IdentityError::Configuration);
        }
        let pool = AnyPoolOptions::new()
            .max_connections(4)
            .connect(url)
            .await?;
        sqlx::migrate!("./migrations-postgres").run(&pool).await?;
        let mut conn = pool.acquire().await?;
        crate::inventory::backfill_title_sort_unicode(&mut conn).await?;
        drop(conn);
        Ok(Self { pool })
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
                Self::open(&path).await
            }
            "postgres" => {
                let url = zeroize::Zeroizing::new(
                    std::env::var("MEDIA_DB_URL").map_err(|_| IdentityError::Configuration)?,
                );
                Self::open_postgres(&url).await
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
            match AnyPoolOptions::new()
                .max_connections(4)
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
        // SQLx SQLite migration lock is a no-op: serialize the entire check/apply sequence.
        let mut conn = pool.acquire().await?;
        crate::database::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let mut tx = crate::database::begin_write(&mut conn).await?;
        sqlx::migrate!("./migrations").run(&mut *tx).await?;
        crate::inventory::backfill_title_sort_unicode(&mut tx).await?;
        tx.commit().await?;
        crate::database::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await?;
        drop(conn);
        Ok(Self { pool })
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
        self.pool.close().await;
    }

    pub fn pool(&self) -> &AnyPool {
        &self.pool
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
            // 64 MiB, 3 passes, one lane; PHC includes version, costs and random OS salt.
            let params = Params::new(65536, 3, 1, Some(32)).map_err(|_| IdentityError::Hash)?;
            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password(password.as_bytes())
                .map(|h| h.to_string())
                .map_err(|_| IdentityError::Hash)
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
