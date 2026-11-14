//! Small shared SQL surface; values always remain bound parameters.
use sqlx::{Any, AnyConnection, Connection, Database, FromRow, SqlSafeStr};
use std::fmt::Write as _;

#[cfg(feature = "query-metrics")]
static STATEMENT_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[inline]
fn count_statement() {
    #[cfg(feature = "query-metrics")]
    STATEMENT_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
/// Statement construction calls plus instrumented engine transaction boundaries.
/// This is not a SQLite page-I/O counter; arbitrary raw SQLx calls are excluded.
pub fn statement_calls() -> Option<u64> {
    #[cfg(feature = "query-metrics")]
    {
        Some(STATEMENT_CALLS.load(std::sync::atomic::Ordering::Relaxed))
    }
    #[cfg(not(feature = "query-metrics"))]
    {
        None
    }
}

pub async fn commit(tx: sqlx::Transaction<'_, Any>) -> Result<(), sqlx::Error> {
    count_statement();
    tx.commit().await
}

/// Number anonymous bind markers, ignoring quoted strings and comments.
/// Both PostgreSQL and SQLite accept numbered `$n` parameters.
pub fn normalize_sql(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    let mut quote = None;
    let mut index = 0;
    let mut line_comment = false;
    let mut block_depth = 0;
    while let Some(c) = chars.next() {
        if line_comment {
            out.push(c);
            if c == '\n' {
                line_comment = false;
            }
        } else if block_depth > 0 {
            out.push(c);
            if c == '/' && chars.peek() == Some(&'*') {
                out.push(chars.next().unwrap_or('*'));
                block_depth += 1;
            } else if c == '*' && chars.peek() == Some(&'/') {
                out.push(chars.next().unwrap_or('/'));
                block_depth -= 1;
            }
        } else if let Some(q) = quote {
            out.push(c);
            if c == q {
                if chars.peek() == Some(&q) {
                    out.push(chars.next().unwrap_or(q));
                } else {
                    quote = None;
                }
            }
        } else if c == '-' && chars.peek() == Some(&'-') {
            out.push(c);
            out.push(chars.next().unwrap_or('-'));
            line_comment = true;
        } else if c == '/' && chars.peek() == Some(&'*') {
            out.push(c);
            out.push(chars.next().unwrap_or('*'));
            block_depth = 1;
        } else if c == '\'' || c == '"' {
            quote = Some(c);
            out.push(c);
        } else if c == '?' {
            index += 1;
            let _ = write!(&mut out, "${index}");
        } else {
            out.push(c);
        }
    }
    out
}

pub fn query<'q, DB: Database>(sql: impl SqlSafeStr) -> sqlx::query::Query<'q, DB, DB::Arguments> {
    count_statement();
    sqlx::query(sqlx::AssertSqlSafe(normalize_sql(
        sql.into_sql_str().as_str(),
    )))
}

pub fn query_as<'q, DB: Database, O>(
    sql: impl SqlSafeStr,
) -> sqlx::query::QueryAs<'q, DB, O, DB::Arguments>
where
    O: for<'r> FromRow<'r, DB::Row>,
{
    count_statement();
    sqlx::query_as(sqlx::AssertSqlSafe(normalize_sql(
        sql.into_sql_str().as_str(),
    )))
}

pub fn query_scalar<'q, DB: Database, O>(
    sql: impl SqlSafeStr,
) -> sqlx::query::QueryScalar<'q, DB, O, DB::Arguments>
where
    (O,): for<'r> FromRow<'r, DB::Row>,
{
    count_statement();
    sqlx::query_scalar(sqlx::AssertSqlSafe(normalize_sql(
        sql.into_sql_str().as_str(),
    )))
}

/// Start an engine-appropriate write transaction.
/// On SQLite, uses `BEGIN IMMEDIATE` to acquire the reserved write lock immediately,
/// preventing upgrade deadlocks.
/// On PostgreSQL, uses a standard ACID transaction (`conn.begin()`) with MVCC and row-level locking,
/// avoiding arbitrary global advisory lock bottlenecks.
pub async fn begin_write(
    conn: &mut AnyConnection,
) -> Result<sqlx::Transaction<'_, Any>, sqlx::Error> {
    count_statement();
    if conn.backend_name() == "SQLite" {
        conn.begin_with("BEGIN IMMEDIATE").await
    } else {
        conn.begin().await
    }
}

/// Start a write transaction scoped to a specific entity or logical partition.
/// On SQLite, relies on SQLite's engine-level write lock (`BEGIN IMMEDIATE`).
/// On PostgreSQL, acquires a transaction-scoped advisory lock specifically on
/// the hash of `(scope, entity_id)`, serializing only conflicting operations on
/// the same entity while allowing full concurrency across all other entities.
pub async fn begin_write_scoped<'a>(
    conn: &'a mut AnyConnection,
    scope: &str,
    entity_id: &str,
) -> Result<sqlx::Transaction<'a, Any>, sqlx::Error> {
    count_statement();
    if conn.backend_name() == "SQLite" {
        conn.begin_with("BEGIN IMMEDIATE").await
    } else {
        let mut tx = conn.begin().await?;
        let lock_key = format!("{scope}:{entity_id}");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(&lock_key)
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

/// Explicit serialized transaction with a designated numeric lock ID for global synchronization.
pub async fn begin_write_serialized(
    conn: &mut AnyConnection,
    lock_id: i64,
) -> Result<sqlx::Transaction<'_, Any>, sqlx::Error> {
    count_statement();
    if conn.backend_name() == "SQLite" {
        conn.begin_with("BEGIN IMMEDIATE").await
    } else {
        let mut tx = conn.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_id)
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

/// Ordered schema upgrade step between persisted schema versions (`v1 -> v2 -> ...`).
pub struct MigrationStep {
    pub version: i64,
    pub name: &'static str,
    pub sqlite_sql: &'static str,
    pub postgres_sql: &'static str,
}

/// Current persisted schema version.
pub const CURRENT_SCHEMA_VERSION: i64 = 3;

/// Sequential migrations applied after the frozen v1 baseline.
pub const MIGRATIONS: &[MigrationStep] = &[
    MigrationStep {
        version: 2,
        name: "0002_library_media_task_runs_identify",
        sqlite_sql: "CREATE TABLE library_media_task_runs_new (
 library_id TEXT NOT NULL,
 task TEXT NOT NULL CHECK(task IN ('identify','trickplay','intros','artwork')),
 started_at BIGINT NOT NULL,
 origin TEXT NOT NULL DEFAULT 'manual' CHECK(origin IN ('manual','scan')),
 PRIMARY KEY(library_id, task)
);
INSERT INTO library_media_task_runs_new(library_id,task,started_at,origin) SELECT library_id,task,started_at,origin FROM library_media_task_runs;
DROP TABLE library_media_task_runs;
ALTER TABLE library_media_task_runs_new RENAME TO library_media_task_runs;",
        postgres_sql: "ALTER TABLE library_media_task_runs DROP CONSTRAINT IF EXISTS library_media_task_runs_task_check;
ALTER TABLE library_media_task_runs ADD CONSTRAINT library_media_task_runs_task_check CHECK(task IN ('identify','trickplay','intros','artwork'));",
    },
    MigrationStep {
        version: 3,
        name: "0003_statistics_retention_unlimited",
        sqlite_sql: "UPDATE statistics_settings SET event_retention_days = 0 WHERE singleton = 1 AND event_retention_days = 365;",
        postgres_sql: "ALTER TABLE statistics_settings ALTER COLUMN event_retention_days SET DEFAULT 0;
UPDATE statistics_settings SET event_retention_days = 0 WHERE singleton = 1 AND event_retention_days = 365;",
    },
];

/// Install the frozen v1 schema (`crates/core/migrations/0001_initial_v1.sql`) on an empty
/// database, apply pending migrations, and refresh derived views/indexes.
pub async fn initialize_fresh_schema(conn: &mut AnyConnection) -> Result<(), sqlx::Error> {
    upgrade_schema_with_steps(conn, CURRENT_SCHEMA_VERSION, MIGRATIONS).await?;
    Ok(())
}

/// Install the v1 baseline if the database is empty, then apply any pending sequential
/// `MigrationStep` entries up to `target_version` inside a single serialized transaction.
/// Returns the resulting schema version.
pub async fn upgrade_schema_with_steps(
    conn: &mut AnyConnection,
    target_version: i64,
    steps: &[MigrationStep],
) -> Result<i64, sqlx::Error> {
    const SQLITE_SCHEMA: &str = include_str!("../migrations/0001_initial_v1.sql");
    const POSTGRES_SCHEMA: &str = include_str!("../migrations/postgres/0001_initial_v1.sql");
    const SEED: &str = include_str!("../schema/seed.sql");
    const DERIVED: &str = include_str!("../schema/derived.sql");
    const SCHEMA_LOCK_ID: i64 = 0x4d45444941534348;

    let sqlite = conn.backend_name() == "SQLite";
    let mut tx = begin_write_serialized(conn, SCHEMA_LOCK_ID).await?;
    let (tables, has_users): (i64, i64) = if sqlite {
        sqlx::query_as("SELECT COUNT(*),COALESCE(SUM(CASE WHEN name='users' THEN 1 ELSE 0 END),0) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .fetch_one(&mut *tx)
            .await?
    } else {
        sqlx::query_as("SELECT COUNT(*)::bigint,COALESCE(SUM(CASE WHEN tablename='users' THEN 1 ELSE 0 END),0)::bigint FROM pg_tables WHERE schemaname=current_schema()")
            .fetch_one(&mut *tx)
            .await?
    };

    let mut current_version: i64;
    if tables == 0 {
        sqlx::raw_sql(if sqlite {
            SQLITE_SCHEMA
        } else {
            POSTGRES_SCHEMA
        })
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(DERIVED).execute(&mut *tx).await?;
        crate::groups::initialize_role_groups(&mut tx).await?;
        sqlx::raw_sql(SEED).execute(&mut *tx).await?;
        current_version = 1;
    } else if has_users == 0 {
        return Err(sqlx::Error::Protocol(
            "Database schema is not empty; choose a fresh database".into(),
        ));
    } else {
        let marker: i64 = if sqlite {
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
            )
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM pg_tables WHERE schemaname=current_schema() AND tablename='schema_version'")
                .fetch_one(&mut *tx)
                .await?
        };
        if marker != 1 {
            return Err(sqlx::Error::Protocol(
                "Existing database uses an unsupported schema; choose a fresh database".into(),
            ));
        }
        current_version = sqlx::query_scalar("SELECT CAST(version AS BIGINT) FROM schema_version")
            .fetch_one(&mut *tx)
            .await?;
        if current_version < 1 || current_version > target_version {
            return Err(sqlx::Error::Protocol(
                "Unsupported database schema version; choose a fresh database".into(),
            ));
        }
    }

    if current_version < target_version {
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at BIGINT NOT NULL)",
        )
        .execute(&mut *tx)
        .await?;
        for step in steps {
            if step.version <= current_version || step.version > target_version {
                continue;
            }
            if step.version != current_version + 1 {
                return Err(sqlx::Error::Protocol(format!(
                    "Non-sequential migration step {} after version {}",
                    step.version, current_version
                )));
            }
            let sql = if sqlite {
                step.sqlite_sql
            } else {
                step.postgres_sql
            };
            if !sql.trim().is_empty() {
                sqlx::raw_sql(sql).execute(&mut *tx).await?;
            }
            // v1 created `schema_version` with `CHECK(version=1)`; replace it when upgrading past v1.
            sqlx::raw_sql(
                "DROP TABLE IF EXISTS schema_version; CREATE TABLE schema_version(version INTEGER PRIMARY KEY);",
            )
            .execute(&mut *tx)
            .await?;
            query("INSERT INTO schema_version(version) VALUES(?)")
                .bind(step.version)
                .execute(&mut *tx)
                .await?;
            query("INSERT INTO schema_migrations(version, name, applied_at) VALUES(?, ?, ?)")
                .bind(step.version)
                .bind(step.name)
                .bind(media_contracts::protocol::unix_now())
                .execute(&mut *tx)
                .await?;
            current_version = step.version;
        }
        if current_version != target_version {
            return Err(sqlx::Error::Protocol(format!(
                "Missing migration steps to reach target schema version {}",
                target_version
            )));
        }
    }

    if tables != 0 {
        // Views and indexes carry no data: refreshing them lets a database already in
        // service pick up their current, faster definition.
        sqlx::raw_sql(DERIVED).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(current_version)
}

/// Integer flags retain the existing SQLite schema and JSON boolean contract.
pub struct DbBool(bool);
impl sqlx::Type<Any> for DbBool {
    fn type_info() -> sqlx::any::AnyTypeInfo {
        <i64 as sqlx::Type<Any>>::type_info()
    }
    fn compatible(ty: &sqlx::any::AnyTypeInfo) -> bool {
        <i64 as sqlx::Type<Any>>::compatible(ty) || <bool as sqlx::Type<Any>>::compatible(ty)
    }
}
impl<'r> sqlx::Decode<'r, Any> for DbBool {
    fn decode(value: sqlx::any::AnyValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        use sqlx::ValueRef;
        if value.type_info().as_ref() == &<bool as sqlx::Type<Any>>::type_info() {
            Ok(Self(<bool as sqlx::Decode<Any>>::decode(value)?))
        } else {
            Ok(Self(<i64 as sqlx::Decode<Any>>::decode(value)? != 0))
        }
    }
}
impl From<DbBool> for bool {
    fn from(value: DbBool) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use sqlx::Connection;

    #[test]
    fn placeholders_preserve_literals() {
        assert_eq!(
            super::normalize_sql("SELECT '?', \"?\", ?, 'it''s?', ?"),
            "SELECT '?', \"?\", $1, 'it''s?', $2"
        );
    }
    #[test]
    fn placeholders_preserve_comments() {
        assert_eq!(
            super::normalize_sql("SELECT ? -- ?\n, ? /* ? /* ? */ */ , ?"),
            "SELECT $1 -- ?\n, $2 /* ? /* ? */ */ , $3"
        );
    }

    #[tokio::test]
    async fn fresh_schema_reopens_without_rewriting_data() {
        sqlx::any::install_default_drivers();
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        super::initialize_fresh_schema(&mut conn).await.unwrap();
        super::query("INSERT INTO users(id,role) VALUES('kept','user')")
            .execute(&mut conn)
            .await
            .unwrap();
        super::initialize_fresh_schema(&mut conn).await.unwrap();
        let count: i64 = super::query_scalar("SELECT COUNT(*) FROM users WHERE id='kept'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn older_schema_is_rejected_without_data_loss() {
        sqlx::any::install_default_drivers();
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        super::query("CREATE TABLE users(id TEXT PRIMARY KEY)")
            .execute(&mut conn)
            .await
            .unwrap();
        super::query("INSERT INTO users(id) VALUES('kept')")
            .execute(&mut conn)
            .await
            .unwrap();
        assert!(super::initialize_fresh_schema(&mut conn).await.is_err());
        let count: i64 = super::query_scalar("SELECT COUNT(*) FROM users WHERE id='kept'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn another_schema_version_is_rejected_without_data_loss() {
        sqlx::any::install_default_drivers();
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        for statement in [
            "CREATE TABLE users(id TEXT PRIMARY KEY)",
            "CREATE TABLE schema_version(version INTEGER PRIMARY KEY)",
            "INSERT INTO schema_version(version) VALUES(0)",
            "INSERT INTO users(id) VALUES('kept')",
        ] {
            super::query(statement).execute(&mut conn).await.unwrap();
        }
        assert!(super::initialize_fresh_schema(&mut conn).await.is_err());
        let count: i64 = super::query_scalar("SELECT COUNT(*) FROM users WHERE id='kept'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn sqlite_v1_to_v2_migration_preserves_data_and_is_idempotent() {
        sqlx::any::install_default_drivers();
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        // 1. Start at current schema version
        super::initialize_fresh_schema(&mut conn).await.unwrap();
        super::query(
            "INSERT INTO library_media_task_runs(library_id,task,started_at,origin) VALUES('lib1','identify',100,'scan')",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        super::query(
            "INSERT INTO users(id,role,display_name) VALUES('u1','administrator','Admin')",
        )
        .execute(&mut conn)
        .await
        .unwrap();

        // 2. Upgrade to next version
        let next_version = super::CURRENT_SCHEMA_VERSION + 1;
        let steps = [super::MigrationStep {
            version: next_version,
            name: "0003_test_migration",
            sqlite_sql: "ALTER TABLE users ADD COLUMN note TEXT DEFAULT '';",
            postgres_sql: "ALTER TABLE users ADD COLUMN note TEXT DEFAULT '';",
        }];
        let version = super::upgrade_schema_with_steps(&mut conn, next_version, &steps)
            .await
            .unwrap();
        assert_eq!(version, next_version);

        // 3. Re-running is idempotent and existing data is intact
        let version_again = super::upgrade_schema_with_steps(&mut conn, next_version, &steps)
            .await
            .unwrap();
        assert_eq!(version_again, next_version);
        let kept: String = super::query_scalar("SELECT display_name FROM users WHERE id='u1'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(kept, "Admin");
        super::query("UPDATE users SET note='upgraded' WHERE id='u1'")
            .execute(&mut conn)
            .await
            .unwrap();
        let note: String = super::query_scalar("SELECT note FROM users WHERE id='u1'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(note, "upgraded");
    }
}
