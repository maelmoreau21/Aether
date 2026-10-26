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

/// Install the one current schema on an empty database. Existing databases are
/// never rewritten here; incompatible older schemas fail explicitly.
pub async fn initialize_fresh_schema(conn: &mut AnyConnection) -> Result<(), sqlx::Error> {
    const SOURCE: &str = include_str!("../schema.sql");
    const SQLITE_START: &str = "-- AETHER_SCHEMA_SQLITE_BEGIN";
    const SQLITE_END: &str = "-- AETHER_SCHEMA_SQLITE_END";
    const POSTGRES_START: &str = "-- AETHER_SCHEMA_POSTGRES_BEGIN";
    const POSTGRES_END: &str = "-- AETHER_SCHEMA_POSTGRES_END";
    let sqlite = conn.backend_name() == "SQLite";
    let (start, end) = if sqlite {
        (SQLITE_START, SQLITE_END)
    } else {
        (POSTGRES_START, POSTGRES_END)
    };
    let section = SOURCE
        .split_once(start)
        .and_then(|(_, rest)| rest.split_once(end))
        .map(|(schema, _)| schema)
        .ok_or_else(|| sqlx::Error::Protocol("Missing database schema section".into()))?;
    let mut tx = begin_write_serialized(conn, 0x4d45444941534348).await?;
    let (tables, current): (i64, i64) = if sqlite {
        sqlx::query_as("SELECT COUNT(*),COALESCE(SUM(CASE WHEN name='users' THEN 1 ELSE 0 END),0) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            .fetch_one(&mut *tx)
            .await?
    } else {
        sqlx::query_as("SELECT COUNT(*)::bigint,COALESCE(SUM(CASE WHEN tablename='users' THEN 1 ELSE 0 END),0)::bigint FROM pg_tables WHERE schemaname=current_schema()")
            .fetch_one(&mut *tx)
            .await?
    };
    if current != 0 {
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
        let version: i64 = sqlx::query_scalar("SELECT CAST(version AS BIGINT) FROM schema_version")
            .fetch_one(&mut *tx)
            .await?;
        if version != 1 {
            return Err(sqlx::Error::Protocol(
                "Unsupported database schema version; choose a fresh database".into(),
            ));
        }
    } else if tables != 0 {
        return Err(sqlx::Error::Protocol(
            "Database schema is not empty; choose a fresh database".into(),
        ));
    } else {
        sqlx::raw_sql(section).execute(&mut *tx).await?;
    }
    tx.commit().await
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
}
