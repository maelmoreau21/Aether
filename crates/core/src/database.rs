//! Small shared SQL surface; values always remain bound parameters.
use sqlx::{Any, AnyConnection, Connection, Database, FromRow, SqlSafeStr};
use std::fmt::Write as _;
mod journal;
mod task_schema;

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
    // Widening legacy SQLite task constraints recreates `durable_tasks`. SQLite
    // ignores `PRAGMA foreign_keys` inside a transaction and a DROP with
    // enforcement enabled would cascade into dependent queue rows, so every
    // caller gets enforcement suspended around the schema transaction only.
    let sqlite = conn.backend_name() == "SQLite";
    let enforced: i64 = if sqlite {
        sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut *conn)
            .await?
    } else {
        0
    };
    if enforced != 0 {
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&mut *conn)
            .await?;
    }
    let result = initialize_schema_transaction(conn).await;
    if enforced != 0 {
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&mut *conn)
            .await?;
    }
    result
}

/// Checksummed text of the one-shot translation backfill; bump it only with the step's meaning.
const COMPAT_SEARCH_TITLES_BACKFILL: &str =
    "search_titles_backfill v1: translated and original titles -> work_search_titles";
const COMPAT_TRANSLATIONS_BACKFILL: &str = "translations_backfill v1: editorial_json/manual_json 'translations' -> work_translations (source legacy, ON CONFLICT DO NOTHING)";

/// Re-run on every start, in this order; each file is additive and idempotent.
const IDEMPOTENT_STEPS: &[(&str, &str)] = &[
    (
        "baseline_gaps",
        include_str!("../migrations/baseline_gaps.sql"),
    ),
    (
        "extension_update",
        include_str!("../migrations/extension_update.sql"),
    ),
    (
        "extension_run",
        include_str!("../migrations/extension_run.sql"),
    ),
    ("subtitle", include_str!("../migrations/subtitle.sql")),
    (
        "recommendation",
        include_str!("../migrations/recommendation.sql"),
    ),
    (
        "notification",
        include_str!("../migrations/notification.sql"),
    ),
    ("statistics", include_str!("../migrations/statistics.sql")),
    ("acquisition", include_str!("../migrations/acquisition.sql")),
    ("reports", include_str!("../migrations/reports.sql")),
    (
        "person_images",
        include_str!("../migrations/person_images.sql"),
    ),
    (
        "metadata_translations",
        include_str!("../migrations/metadata_translations.sql"),
    ),
    (
        "quick_connect",
        include_str!("../migrations/quick_connect.sql"),
    ),
    (
        "application_scope",
        include_str!("../migrations/application_scope.sql"),
    ),
    ("task_runs", include_str!("../migrations/task_runs.sql")),
    (
        "task_triggers",
        include_str!("../migrations/task_triggers.sql"),
    ),
    (
        "artwork_webp",
        include_str!("../migrations/artwork_webp.sql"),
    ),
];
/// Depend on the role-group seed that Rust performs between the two lists.
const IDEMPOTENT_STEPS_AFTER_GROUPS: &[(&str, &str)] = &[
    ("media_kinds", include_str!("../migrations/media_kinds.sql")),
    ("role_groups", include_str!("../migrations/role_groups.sql")),
    (
        "default_avatars",
        include_str!("../migrations/default_avatars.sql"),
    ),
    (
        "avatar_requirements",
        include_str!("../migrations/avatar_requirements.sql"),
    ),
    (
        "library_deletions",
        include_str!("../migrations/library_deletions.sql"),
    ),
    (
        "content_fingerprints",
        include_str!("../migrations/content_fingerprints.sql"),
    ),
];
/// Journal text for the Rust-coded constraint upgrades in `task_schema`.
const COMPAT_TASK_CONSTRAINTS: &str =
    "durable_tasks trickplay category and width; timeline marker recap/preview kinds";
/// Early PostgreSQL schemas stored epoch timestamps as 32-bit `integer`, which overflows in 2038.
/// Widening is lossless. The two views are recreated by `role_groups.sql` later in the same
/// transaction (a view blocks `ALTER COLUMN TYPE` on the columns it reads).
const POSTGRES_WIDEN_TIMESTAMPS: &str = r#"
DROP VIEW IF EXISTS effective_library_permissions;
DROP VIEW IF EXISTS effective_user_group_members;
DO $$
DECLARE target record;
BEGIN
  FOR target IN
    SELECT c.table_name, c.column_name
    FROM information_schema.columns c
    JOIN information_schema.tables t ON t.table_schema=c.table_schema AND t.table_name=c.table_name
    WHERE c.table_schema=current_schema() AND t.table_type='BASE TABLE' AND c.data_type='integer'
      AND (c.column_name LIKE '%!_at' ESCAPE '!' OR c.column_name='window_started')
  LOOP
    EXECUTE format('ALTER TABLE %I ALTER COLUMN %I TYPE bigint', target.table_name, target.column_name);
  END LOOP;
END $$;
"#;
/// Journal text for the watch-party tables and 64-bit sizes created for databases that predate them.
const COMPAT_WATCH_PARTIES: &str =
    "watch_party tables; PostgreSQL size and timestamp columns widened to bigint";

async fn initialize_schema_transaction(conn: &mut AnyConnection) -> Result<(), sqlx::Error> {
    const SOURCE: &str = include_str!("../migrations/schema.sql");
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
    let fresh = current == 0 && tables == 0;
    journal::ensure(&mut tx).await?;
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
        if sqlite {
            sqlx::raw_sql(
                "CREATE TABLE IF NOT EXISTS watch_parties (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    host_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    media_id TEXT NOT NULL,
                    control_mode TEXT NOT NULL DEFAULT 'host_only' CHECK(control_mode IN ('host_only', 'shared', 'request')),
                    state TEXT NOT NULL DEFAULT 'waiting' CHECK(state IN ('waiting', 'playing', 'paused', 'ended')),
                    current_position_ms INTEGER NOT NULL DEFAULT 0,
                    position_updated_at_ms INTEGER NOT NULL DEFAULT 0,
                    is_locked INTEGER NOT NULL DEFAULT 0 CHECK(is_locked IN (0, 1)),
                    wait_for_buffering INTEGER NOT NULL DEFAULT 1 CHECK(wait_for_buffering IN (0, 1)),
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    ended_at INTEGER
                );
                CREATE TABLE IF NOT EXISTS watch_party_members (
                    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
                    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    role TEXT NOT NULL DEFAULT 'participant' CHECK(role IN ('host', 'participant')),
                    is_ready INTEGER NOT NULL DEFAULT 0 CHECK(is_ready IN (0, 1)),
                    is_buffering INTEGER NOT NULL DEFAULT 0 CHECK(is_buffering IN (0, 1)),
                    joined_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    last_seen_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    PRIMARY KEY(party_id, user_id)
                );
                CREATE TABLE IF NOT EXISTS watch_party_invites (
                    id TEXT PRIMARY KEY NOT NULL,
                    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
                    token TEXT NOT NULL UNIQUE,
                    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    target_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    expires_at INTEGER NOT NULL,
                    revoked_at INTEGER
                );
                CREATE TABLE IF NOT EXISTS watch_party_messages (
                    id TEXT PRIMARY KEY NOT NULL,
                    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
                    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    content TEXT NOT NULL CHECK(length(content) BETWEEN 1 AND 500),
                    kind TEXT NOT NULL DEFAULT 'chat' CHECK(kind IN ('chat', 'system', 'reaction')),
                    created_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE IF NOT EXISTS watch_party_queue (
                    id TEXT PRIMARY KEY NOT NULL,
                    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
                    media_id TEXT NOT NULL,
                    added_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    position INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE IF NOT EXISTS watch_party_polls (
                    id TEXT PRIMARY KEY NOT NULL,
                    party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE,
                    question TEXT NOT NULL CHECK(length(question) BETWEEN 1 AND 200),
                    options_json TEXT NOT NULL,
                    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    is_closed INTEGER NOT NULL DEFAULT 0 CHECK(is_closed IN (0, 1)),
                    created_at INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE TABLE IF NOT EXISTS watch_party_poll_votes (
                    poll_id TEXT NOT NULL REFERENCES watch_party_polls(id) ON DELETE CASCADE,
                    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    option_index INTEGER NOT NULL,
                    PRIMARY KEY(poll_id, user_id)
                );",
            )
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::raw_sql(
                "ALTER TABLE inventory_assets ALTER COLUMN size TYPE bigint; \
                 ALTER TABLE scan_staging_files ALTER COLUMN size TYPE bigint; \
                 ALTER TABLE scan_staging_files ALTER COLUMN poster_size TYPE bigint; \
                 ALTER TABLE scan_staging_files ALTER COLUMN series_poster_size TYPE bigint; \
                 ALTER TABLE inventory_images ALTER COLUMN size TYPE bigint; \
                 ALTER TABLE assets ALTER COLUMN size TYPE bigint; \
                 ALTER TABLE music_tracks ALTER COLUMN size TYPE bigint; \
                 ALTER TABLE music_tracks ALTER COLUMN cover_size TYPE bigint; \
                 CREATE TABLE IF NOT EXISTS watch_parties ( \
                     id TEXT PRIMARY KEY NOT NULL, \
                     name TEXT NOT NULL, \
                     host_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     media_id TEXT NOT NULL, \
                     control_mode TEXT NOT NULL DEFAULT 'host_only' CHECK(control_mode IN ('host_only', 'shared', 'request')), \
                     state TEXT NOT NULL DEFAULT 'waiting' CHECK(state IN ('waiting', 'playing', 'paused', 'ended')), \
                     current_position_ms BIGINT NOT NULL DEFAULT 0, \
                     position_updated_at_ms BIGINT NOT NULL DEFAULT 0, \
                     is_locked BIGINT NOT NULL DEFAULT 0 CHECK(is_locked IN (0, 1)), \
                     wait_for_buffering BIGINT NOT NULL DEFAULT 1 CHECK(wait_for_buffering IN (0, 1)), \
                     created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT), \
                     ended_at BIGINT \
                 ); \
                 CREATE TABLE IF NOT EXISTS watch_party_members ( \
                     party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE, \
                     user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     role TEXT NOT NULL DEFAULT 'participant' CHECK(role IN ('host', 'participant')), \
                     is_ready BIGINT NOT NULL DEFAULT 0 CHECK(is_ready IN (0, 1)), \
                     is_buffering BIGINT NOT NULL DEFAULT 0 CHECK(is_buffering IN (0, 1)), \
                     joined_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT), \
                     last_seen_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT), \
                     PRIMARY KEY(party_id, user_id) \
                 ); \
                 CREATE TABLE IF NOT EXISTS watch_party_invites ( \
                     id TEXT PRIMARY KEY NOT NULL, \
                     party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE, \
                     token TEXT NOT NULL UNIQUE, \
                     created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     target_user_id TEXT REFERENCES users(id) ON DELETE CASCADE, \
                     created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT), \
                     expires_at BIGINT NOT NULL, \
                     revoked_at BIGINT \
                 ); \
                 CREATE TABLE IF NOT EXISTS watch_party_messages ( \
                     id TEXT PRIMARY KEY NOT NULL, \
                     party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE, \
                     user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     content TEXT NOT NULL CHECK(length(content) BETWEEN 1 AND 500), \
                     kind TEXT NOT NULL DEFAULT 'chat' CHECK(kind IN ('chat', 'system', 'reaction')), \
                     created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT) \
                 ); \
                 CREATE TABLE IF NOT EXISTS watch_party_queue ( \
                     id TEXT PRIMARY KEY NOT NULL, \
                     party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE, \
                     media_id TEXT NOT NULL, \
                     added_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     position BIGINT NOT NULL DEFAULT 0, \
                     created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT) \
                 ); \
                 CREATE TABLE IF NOT EXISTS watch_party_polls ( \
                     id TEXT PRIMARY KEY NOT NULL, \
                     party_id TEXT NOT NULL REFERENCES watch_parties(id) ON DELETE CASCADE, \
                     question TEXT NOT NULL CHECK(length(question) BETWEEN 1 AND 200), \
                     options_json TEXT NOT NULL, \
                     created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     is_closed BIGINT NOT NULL DEFAULT 0 CHECK(is_closed IN (0, 1)), \
                     created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT) \
                 ); \
                 CREATE TABLE IF NOT EXISTS watch_party_poll_votes ( \
                     poll_id TEXT NOT NULL REFERENCES watch_party_polls(id) ON DELETE CASCADE, \
                     user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, \
                     option_index BIGINT NOT NULL, \
                     PRIMARY KEY(poll_id, user_id) \
                 );",
            )
            .execute(&mut *tx)
            .await?;
            sqlx::raw_sql(POSTGRES_WIDEN_TIMESTAMPS)
                .execute(&mut *tx)
                .await?;
        }
    } else if tables != 0 {
        return Err(sqlx::Error::Protocol(
            "Database schema is not empty; choose a fresh database".into(),
        ));
    } else {
        sqlx::raw_sql(section).execute(&mut *tx).await?;
    }
    // Journalled on fresh installs too, so the journal does not depend on how many starts happened.
    journal::record(
        &mut tx,
        "compat:watch_parties",
        "compat",
        COMPAT_WATCH_PARTIES,
    )
    .await?;
    if fresh {
        journal::record(&mut tx, "baseline", "baseline", section).await?;
    }
    task_schema::allow_trickplay(&mut tx, sqlite, section).await?;
    task_schema::allow_marker_kinds(&mut tx, sqlite, section).await?;
    journal::record(
        &mut tx,
        "compat:task_constraints",
        "compat",
        COMPAT_TASK_CONSTRAINTS,
    )
    .await?;
    for (name, sql) in IDEMPOTENT_STEPS {
        journal::run_idempotent(&mut tx, name, sql).await?;
    }
    crate::groups::initialize_role_groups(&mut tx).await?;
    for (name, sql) in IDEMPOTENT_STEPS_AFTER_GROUPS {
        journal::run_idempotent(&mut tx, name, sql).await?;
    }
    journal::apply_versioned(&mut tx, sqlite, journal::VERSIONED_STEPS).await?;
    // Per-language texts kept as JSON by older versions become rows; the JSON stays.
    if !journal::recorded(&mut tx, "compat:translations_backfill").await? {
        crate::translations::backfill_legacy_blobs(&mut tx).await?;
        journal::record(
            &mut tx,
            "compat:translations_backfill",
            "compat",
            COMPAT_TRANSLATIONS_BACKFILL,
        )
        .await?;
    }
    // Titles that make a work findable by its translated and original names.
    if !journal::recorded(&mut tx, "compat:search_titles_backfill").await? {
        crate::translations::search::backfill(&mut tx).await?;
        journal::record(
            &mut tx,
            "compat:search_titles_backfill",
            "compat",
            COMPAT_SEARCH_TITLES_BACKFILL,
        )
        .await?;
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
