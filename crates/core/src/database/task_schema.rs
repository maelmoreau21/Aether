//! Widen existing task constraints without changing task identity or queue state.
use sqlx::{Any, Transaction};

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
        let definitions: Vec<String> = sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE tbl_name='durable_tasks' AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY type,name")
            .fetch_all(&mut **tx).await?;
        let table = schema
            .split_once("CREATE TABLE durable_tasks (")
            .and_then(|(_, rest)| rest.split_once("\n);"))
            .map(|(columns, _)| format!("CREATE TABLE durable_tasks_trickplay ({columns}\n);"))
            .ok_or_else(|| sqlx::Error::Protocol("Missing current task schema".into()))?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(&*table))
            .execute(&mut **tx)
            .await?;
        sqlx::raw_sql("INSERT INTO durable_tasks_trickplay SELECT * FROM durable_tasks; DROP TABLE durable_tasks; ALTER TABLE durable_tasks_trickplay RENAME TO durable_tasks;")
            .execute(&mut **tx).await?;
        for definition in definitions {
            sqlx::raw_sql(sqlx::AssertSqlSafe(&*definition))
                .execute(&mut **tx)
                .await?;
        }
        // No media/catalogue tables are rewritten. Dependencies, automation links
        // and their FK targets keep the same table name and original task IDs.
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut **tx)
            .await?;
        if !violations.is_empty() {
            return Err(sqlx::Error::Protocol(
                "Task constraint upgrade failed foreign key validation".into(),
            ));
        }
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
        let legacy=schema.replace("'image','trickplay','system'","'image','system'").replace("CHECK((category='trickplay' AND width BETWEEN 160 AND 1920) OR width IN (320,640,1280))","CHECK(width IN (320,640,1280))");
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
        let dependencies:i64=sqlx::query_scalar("SELECT COUNT(*) FROM automation_dependencies WHERE task_id='child' AND prerequisite_id='parent'").fetch_one(&mut conn).await.unwrap();
        assert_eq!(dependencies, 1);
        let schema_version: i64 = sqlx::query_scalar("PRAGMA schema_version")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        super::super::initialize_fresh_schema(&mut conn)
            .await
            .unwrap();
        assert_eq!(snapshot(&mut conn).await, before);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA schema_version")
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
}
