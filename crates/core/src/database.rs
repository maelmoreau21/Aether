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
        sqlx::query("SELECT CAST(pg_advisory_xact_lock($1) AS TEXT)")
            .bind(lock_id)
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
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
}
