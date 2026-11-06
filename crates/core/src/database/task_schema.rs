//! Widen existing task constraints without changing task identity or queue state.
use sqlx::{Any, Transaction};

async fn table_columns(
    tx: &mut Transaction<'_, Any>,
    table: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
    )))
    .fetch_all(&mut **tx)
    .await
}

/// Rebuild one SQLite table from the current schema definition, keeping rows,
/// rowids, indexes and triggers. Callers run with foreign keys suspended.
async fn rebuild_sqlite_table(
    tx: &mut Transaction<'_, Any>,
    schema: &str,
    table: &str,
) -> Result<(), sqlx::Error> {
    let definitions: Vec<String> = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE tbl_name=? AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name")
        .bind(table)
        .fetch_all(&mut **tx)
        .await?;
    let marker = format!("CREATE TABLE {table} (");
    let created = schema
        .split_once(marker.as_str())
        .and_then(|(_, rest)| {
            let (columns, tail) = rest.split_once("\n)")?;
            let (suffix, _) = tail.split_once(';')?;
            Some(format!(
                "CREATE TABLE {table}_widened ({columns}\n){suffix};"
            ))
        })
        .ok_or_else(|| sqlx::Error::Protocol(format!("Missing current {table} schema")))?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(&*created))
        .execute(&mut **tx)
        .await?;
    // Copy by column name: a legacy table whose columns were appended by ALTER TABLE has a
    // different order from the current definition, and `SELECT *` would shift values silently.
    let old_columns = table_columns(tx, table).await?;
    let new_columns = table_columns(tx, &format!("{table}_widened")).await?;
    if let Some(lost) = old_columns.iter().find(|c| !new_columns.contains(c)) {
        return Err(sqlx::Error::Protocol(format!(
            "{table}.{lost} is absent from the current schema; refusing to drop user data"
        )));
    }
    let columns = new_columns
        .iter()
        .filter(|c| old_columns.contains(c))
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("INSERT INTO {table}_widened({columns}) SELECT {columns} FROM {table}; DROP TABLE {table}; ALTER TABLE {table}_widened RENAME TO {table};")))
        .execute(&mut **tx)
        .await?;
    for definition in definitions {
        sqlx::raw_sql(sqlx::AssertSqlSafe(&*definition))
            .execute(&mut **tx)
            .await?;
    }
    // Only relations touching the rebuilt table are validated: unrelated
    // historical orphans must not prevent the server from starting.
    let violations = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut **tx)
        .await?;
    if violations.iter().any(|row| {
        use sqlx::Row;
        let child: String = row.try_get(0).unwrap_or_default();
        let parent: String = row.try_get(2).unwrap_or_default();
        child == table || parent == table
    }) {
        return Err(sqlx::Error::Protocol(format!(
            "{table} constraint upgrade failed foreign key validation"
        )));
    }
    Ok(())
}

/// Recap and preview markers from chapters/plugins were rejected by legacy CHECKs.
pub(super) async fn allow_marker_kinds(
    tx: &mut Transaction<'_, Any>,
    sqlite: bool,
    schema: &str,
) -> Result<(), sqlx::Error> {
    for table in ["manual_timeline_markers", "automatic_timeline_markers"] {
        if sqlite {
            let previous: Option<String> =
                sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='table' AND name=?")
                    .bind(table)
                    .fetch_optional(&mut **tx)
                    .await?;
            if previous.is_none_or(|sql| sql.contains("'recap'")) {
                continue;
            }
            rebuild_sqlite_table(tx, schema, table).await?;
        } else {
            let constraint = format!("{table}_marker_type_check");
            let current: Option<String> = sqlx::query_scalar("SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid=$1::regclass AND conname=$2")
                .bind(table)
                .bind(&constraint)
                .fetch_optional(&mut **tx)
                .await?;
            if current.is_some_and(|definition| definition.contains("recap")) {
                continue;
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("ALTER TABLE {table} DROP CONSTRAINT IF EXISTS {constraint}; ALTER TABLE {table} ADD CONSTRAINT {constraint} CHECK(marker_type IN ('intro','credits','recap','preview'));")))
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

pub(super) async fn allow_trickplay(
    tx: &mut Transaction<'_, Any>,
    sqlite: bool,
    schema: &str,
) -> Result<(), sqlx::Error> {
    if sqlite {
        let previous: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='durable_tasks'",
        )
        .fetch_one(&mut **tx)
        .await?;
        if previous.contains("'trickplay'") && previous.contains("1920") {
            return Ok(());
        }
        // IdentityStore disables FK enforcement before entering initialization.
        // Refuse other callers rather than risk cascading dependent queue records.
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **tx)
            .await?;
        if foreign_keys != 0 {
            return Err(sqlx::Error::Protocol(
                "Task constraint upgrade requires foreign_keys=OFF before the schema transaction"
                    .into(),
            ));
        }
        // No media/catalogue tables are rewritten. Dependencies, automation links
        // and their FK targets keep the same table name and original task IDs.
        rebuild_sqlite_table(tx, schema, "durable_tasks").await?;
    } else {
        let constraints: Vec<String> = sqlx::query_scalar("SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid='durable_tasks'::regclass AND conname IN ('durable_tasks_category_check','durable_tasks_width_check')")
            .fetch_all(&mut **tx).await?;
        if constraints.iter().any(|c| c.contains("trickplay"))
            && constraints.iter().any(|c| c.contains("1920"))
        {
            return Ok(());
        }
        sqlx::raw_sql("ALTER TABLE durable_tasks DROP CONSTRAINT IF EXISTS durable_tasks_category_check; ALTER TABLE durable_tasks DROP CONSTRAINT IF EXISTS durable_tasks_width_check; ALTER TABLE durable_tasks ADD CONSTRAINT durable_tasks_category_check CHECK(category IN ('scan','analyze','prepare','image','trickplay','system')); ALTER TABLE durable_tasks ADD CONSTRAINT durable_tasks_width_check CHECK((category='trickplay' AND width BETWEEN 160 AND 1920) OR width IN (320,640,1280));")
            .execute(&mut **tx).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::{Connection, Row};

    async fn snapshot(conn: &mut sqlx::AnyConnection) -> String {
        let columns = sqlx::query("PRAGMA table_info(durable_tasks)")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
        let members = columns
            .iter()
            .map(|column| {
                let name: String = column.get("name");
                format!("'{name}',{name}")
            })
            .collect::<Vec<_>>()
            .join(",");
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT json_group_array(json_object({members})) FROM (SELECT * FROM durable_tasks ORDER BY id)")))
            .fetch_one(conn).await.unwrap()
    }

    #[tokio::test]
    async fn legacy_task_constraints_upgrade_once_without_losing_queue_or_dependencies() {
        sqlx::any::install_default_drivers();
        let source = include_str!("../../migrations/schema.sql");
        let schema = source
            .split_once("-- AETHER_SCHEMA_SQLITE_BEGIN")
            .unwrap()
            .1
            .split_once("-- AETHER_SCHEMA_SQLITE_END")
            .unwrap()
            .0;
        let legacy=schema.replace("'image','trickplay','system'","'image','system'").replace("CHECK((category='trickplay' AND width BETWEEN 160 AND 1920) OR width IN (320,640,1280))","CHECK(width IN (320,640,1280))").replace("CHECK(marker_type IN ('intro','credits','recap','preview'))","CHECK(marker_type IN ('intro','credits'))");
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(&*legacy))
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::raw_sql("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('kept','Kept','films','local','/read-only',1); INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,component,width,result_json,refresh_mode,status,priority,attempt,failure_count,max_attempts,progress,processed_items,created_at,updated_at,started_at,finished_at,lease_until,available_at,error) VALUES('parent','kept','test','parent','local','scan','','',640,'{\"kept\":true}','full','running',321,2,1,7,43,99,1,2,1,NULL,900,3,'old warning'); INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,created_at,updated_at,available_at) VALUES('child','kept','test','child','local','analyze',4,5,6); INSERT INTO automation_dependencies(task_id,prerequisite_id) VALUES('child','parent'); CREATE INDEX preserved_task_index ON durable_tasks(owner,priority); CREATE TABLE task_probe(value TEXT); CREATE TRIGGER preserved_task_trigger AFTER UPDATE OF priority ON durable_tasks BEGIN INSERT INTO task_probe(value) VALUES(new.id); END;")
            .execute(&mut conn).await.unwrap();
        let before = snapshot(&mut conn).await;
        super::super::initialize_fresh_schema(&mut conn)
            .await
            .unwrap();
        assert_eq!(snapshot(&mut conn).await, before);
        for table in ["manual_timeline_markers", "automatic_timeline_markers"] {
            let sql: String =
                sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type='table' AND name=?")
                    .bind(table)
                    .fetch_one(&mut conn)
                    .await
                    .unwrap();
            assert!(
                sql.contains("'recap'") && sql.contains("'preview'"),
                "{table}"
            );
        }
        let dependencies:i64=sqlx::query_scalar("SELECT COUNT(*) FROM automation_dependencies WHERE task_id='child' AND prerequisite_id='parent'").fetch_one(&mut conn).await.unwrap();
        assert_eq!(dependencies, 1);
        // Shared views are recreated on every start; the rebuilt tables must not be.
        let table_sql = "SELECT group_concat(sql,';') FROM (SELECT sql FROM sqlite_master WHERE tbl_name IN ('durable_tasks','manual_timeline_markers','automatic_timeline_markers') AND sql IS NOT NULL ORDER BY type,name)";
        let schema_version: String = sqlx::query_scalar(table_sql)
            .fetch_one(&mut conn)
            .await
            .unwrap();
        super::super::initialize_fresh_schema(&mut conn)
            .await
            .unwrap();
        assert_eq!(snapshot(&mut conn).await, before);
        assert_eq!(
            sqlx::query_scalar::<_, String>(table_sql)
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            schema_version
        );
        sqlx::raw_sql("PRAGMA foreign_keys=ON; UPDATE durable_tasks SET priority=322 WHERE id='parent'; INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,width,created_at,updated_at,available_at) VALUES('preview','kept','test','preview','local','trickplay',410,7,8,9);").execute(&mut conn).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM task_probe WHERE value='parent'")
                .fetch_one(&mut conn)
                .await
                .unwrap(),
            1
        );
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut conn)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rebuild_copies_by_column_name_when_legacy_column_order_differs() {
        sqlx::any::install_default_drivers();
        let source = include_str!("../../migrations/schema.sql");
        let schema = source
            .split_once("-- AETHER_SCHEMA_SQLITE_BEGIN")
            .unwrap()
            .1
            .split_once("-- AETHER_SCHEMA_SQLITE_END")
            .unwrap()
            .0;
        let legacy = schema.replace("'image','trickplay','system'", "'image','system'").replace("CHECK((category='trickplay' AND width BETWEEN 160 AND 1920) OR width IN (320,640,1280))", "CHECK(width IN (320,640,1280))");
        let mut conn = sqlx::AnyConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(&*legacy))
            .execute(&mut conn)
            .await
            .unwrap();
        // Swap two adjacent columns, as a database whose columns were added by ALTER TABLE would have.
        let table_sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name='durable_tasks'")
                .fetch_one(&mut conn)
                .await
                .unwrap();
        let reordered = table_sql.replacen(
            "available_at BIGINT NOT NULL,
    error TEXT NOT NULL DEFAULT '',",
            "error TEXT NOT NULL DEFAULT '',
    available_at BIGINT NOT NULL,",
            1,
        );
        assert_ne!(
            reordered, table_sql,
            "fixture must actually reorder the columns"
        );
        sqlx::raw_sql("PRAGMA foreign_keys=OFF; DROP TABLE durable_tasks;")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(&*reordered))
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::raw_sql("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','L','films','local','/x',1); INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,created_at,updated_at,available_at,error) VALUES('t','lib','the-owner','key','local','scan',1,2,3,'kept error');").execute(&mut conn).await.unwrap();
        super::super::initialize_fresh_schema(&mut conn)
            .await
            .unwrap();
        let row: (String, i64, String) =
            sqlx::query_as("SELECT owner,available_at,error FROM durable_tasks WHERE id='t'")
                .fetch_one(&mut conn)
                .await
                .unwrap();
        assert_eq!(row, ("the-owner".into(), 3, "kept error".into()));
    }
}
