//! Local identity persistence. No HTTP routes, sessions or OIDC implementation.
use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
use sqlx::{
    Connection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use std::{path::Path, time::Duration};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
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
    pub(crate) pool: SqlitePool,
}
impl IdentityStore {
    /// Operator-controlled local file, never supplied by an HTTP request.
    pub async fn open(path: &Path) -> Result<Self, IdentityError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(15));
        // Concurrent first opens may race on PRAGMA journal_mode=WAL before the
        // busy handler can wait. Retry only SQLite BUSY/LOCKED, never corruption,
        // permissions or schema errors. Each attempt retains its 15-second timeout.
        let mut retries = 0;
        let pool = loop {
            match SqlitePoolOptions::new()
                .max_connections(4)
                .connect_with(options.clone())
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
        let mut tx = conn.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::migrate!("./migrations").run(&mut *tx).await?;
        tx.commit().await?;
        drop(conn);
        Ok(Self { pool })
    }

    pub async fn user(&self, id: &str) -> Result<Option<User>, IdentityError> {
        Ok(
            sqlx::query_as("SELECT id, role, created_at, disabled_at FROM users WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub fn pool(&self) -> &SqlitePool {
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
        let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
        let closed: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM bootstrap_state) OR EXISTS(SELECT 1 FROM users)",
        )
        .fetch_one(&mut *tx)
        .await?;
        if closed != 0 {
            return Err(IdentityError::AlreadyInitialized);
        }
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id, role) VALUES (?, ?)")
            .bind(&id)
            .bind(Role::Administrator.as_str())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(login)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO bootstrap_state(singleton) VALUES (1)")
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
