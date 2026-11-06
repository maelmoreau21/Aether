//! Journal of schema steps, kept beside the existing `schema_version` marker.
//!
//! `schema_version` still gates which databases Core accepts. This table only
//! adds names and checksums so that an operator (and future steps) can tell
//! which step produced which part of a database. Existing databases gain the
//! journal on their next start; nothing already applied is renamed or replayed.
//!
//! Step kinds:
//! * `baseline`   – hash of the engine section of `schema.sql` used to create a fresh database.
//! * `idempotent` – `migrations/*.sql` files re-run on every start. Editing one is how a
//!   compatible change reaches old databases, so a new checksum is recorded, never rejected.
//! * `compat`     – upgrades coded in Rust (constraint widening, column types).
//! * `versioned`  – future steps applied exactly once. Editing one after it was applied is an
//!   error: write a new step instead.
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};

const CREATE_JOURNAL: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    name TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('baseline','idempotent','compat','versioned')),
    checksum TEXT NOT NULL CHECK(length(checksum)=64),
    applied_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
)";

/// A step applied once per database, with one script per engine.
pub struct VersionedStep {
    /// Stable identifier, never renamed after release (e.g. `0001_localized_metadata`).
    pub name: &'static str,
    pub sqlite: &'static str,
    pub postgres: &'static str,
}

/// Steps that run exactly once. Empty today: every existing change is an idempotent step.
pub(super) const VERSIONED_STEPS: &[VersionedStep] = &[];

/// Line endings must not change the checksum of a file checked out on another platform.
pub(super) fn checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.replace("\r\n", "\n").as_bytes()))
}

pub(super) async fn ensure(tx: &mut Transaction<'_, Any>) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(CREATE_JOURNAL).execute(&mut **tx).await?;
    Ok(())
}

/// Insert the step, or refresh its checksum when the text changed. Unchanged steps write nothing.
pub(super) async fn record(
    tx: &mut Transaction<'_, Any>,
    name: &str,
    kind: &str,
    text: &str,
) -> Result<(), sqlx::Error> {
    super::query("INSERT INTO schema_migrations(name,kind,checksum,applied_at,updated_at) VALUES(?,?,?,unixepoch(),unixepoch()) ON CONFLICT(name) DO UPDATE SET checksum=excluded.checksum,updated_at=excluded.updated_at WHERE schema_migrations.checksum<>excluded.checksum")
        .bind(name)
        .bind(kind)
        .bind(checksum(text))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Whether a step name is already in the journal; one-shot Rust data steps use it so
/// the step and its journal row commit (or roll back) together in the caller's transaction.
pub(super) async fn recorded(
    tx: &mut Transaction<'_, Any>,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let found: Option<String> =
        super::query_scalar("SELECT name FROM schema_migrations WHERE name=?")
            .bind(name)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(found.is_some())
}

/// Run an idempotent script and journal it.
pub(super) async fn run_idempotent(
    tx: &mut Transaction<'_, Any>,
    name: &str,
    sql: &'static str,
) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(sql).execute(&mut **tx).await?;
    record(tx, name, "idempotent", sql).await
}

pub(super) async fn apply_versioned(
    tx: &mut Transaction<'_, Any>,
    sqlite: bool,
    steps: &[VersionedStep],
) -> Result<(), sqlx::Error> {
    for step in steps {
        let sql = if sqlite { step.sqlite } else { step.postgres };
        let expected = checksum(sql);
        let recorded: Option<(String, String)> =
            super::query_as("SELECT kind,checksum FROM schema_migrations WHERE name=?")
                .bind(step.name)
                .fetch_optional(&mut **tx)
                .await?;
        match recorded {
            Some((kind, found)) if kind == "versioned" && found == expected => {}
            Some((kind, found)) if kind == "versioned" => {
                return Err(sqlx::Error::Protocol(format!(
                    "Migration {} was modified after it was applied (recorded {found}, current {expected}); add a new step instead",
                    step.name
                )));
            }
            Some(_) => {
                return Err(sqlx::Error::Protocol(format!(
                    "Migration name {} is already used by a non-versioned step",
                    step.name
                )));
            }
            None => {
                sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(&mut **tx)
                    .await?;
                record(tx, step.name, "versioned", sql).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Connection, Row};

    async fn apply(conn: &mut sqlx::AnyConnection, steps: &[VersionedStep]) -> Result<(), String> {
        let mut tx = conn.begin().await.map_err(|e| e.to_string())?;
        ensure(&mut tx).await.map_err(|e| e.to_string())?;
        apply_versioned(&mut tx, true, steps)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())
    }

    #[test]
    fn checksum_ignores_line_endings() {
        assert_eq!(checksum("a\r\nb"), checksum("a\nb"));
        assert_ne!(checksum("a\nb"), checksum("a\nc"));
    }

    #[tokio::test]
    async fn versioned_steps_run_once_and_reject_edits() {
        sqlx::any::install_default_drivers();
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        let first = VersionedStep {
            name: "0001_probe",
            sqlite: "CREATE TABLE probe(id TEXT PRIMARY KEY); INSERT INTO probe VALUES('kept');",
            postgres: "",
        };
        apply(&mut conn, std::slice::from_ref(&first))
            .await
            .unwrap();
        // A second start must not replay the INSERT (it would violate the primary key).
        apply(&mut conn, std::slice::from_ref(&first))
            .await
            .unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM probe")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(rows, 1);

        let edited = VersionedStep {
            name: "0001_probe",
            sqlite: "CREATE TABLE probe(id TEXT PRIMARY KEY, extra TEXT);",
            postgres: "",
        };
        let error = apply(&mut conn, &[edited]).await.unwrap_err();
        assert!(error.contains("modified after it was applied"), "{error}");

        // A failing step rolls back with its journal row, so the next start retries it.
        let broken = VersionedStep {
            name: "0002_broken",
            sqlite: "CREATE TABLE half(id TEXT); INSERT INTO missing_table VALUES(1);",
            postgres: "",
        };
        assert!(apply(&mut conn, &[first, broken]).await.is_err());
        let leftovers: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE name='half')+(SELECT COUNT(*) FROM schema_migrations WHERE name='0002_broken')",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(leftovers, 0);
        let names: Vec<String> = sqlx::query("SELECT name FROM schema_migrations ORDER BY name")
            .fetch_all(&mut conn)
            .await
            .unwrap()
            .iter()
            .map(|row| row.get(0))
            .collect();
        assert_eq!(names, ["0001_probe"]);
    }
}
