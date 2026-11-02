//! Explicit uninstall semantics; called inside the Core's serialized mutation transaction.
use crate::database::{query, query_as};
use sqlx::AnyConnection;

pub async fn uninstall(
    conn: &mut AnyConnection,
    id: &str,
    delete_data: bool,
) -> Result<(), sqlx::Error> {
    if delete_data {
        query("DELETE FROM extension_data WHERE extension_id=?")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        query("DELETE FROM extension_objects WHERE extension_id=?")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        query("DELETE FROM extension_data_namespaces WHERE extension_id=?")
            .bind(id)
            .execute(&mut *conn)
            .await?;
    } else {
        // Cache and incomplete uploads are regenerable. Durable completed results survive.
        query("DELETE FROM extension_data WHERE extension_id=? AND durability='cache'")
            .bind(id)
            .execute(&mut *conn)
            .await?;
        query("DELETE FROM extension_objects WHERE extension_id=? AND (durability='cache' OR complete=0)").bind(id).execute(&mut *conn).await?;
        let totals: (i64, i64) = query_as(
            "SELECT COUNT(*),CAST(COALESCE(SUM(value_bytes),0) AS BIGINT) FROM extension_data WHERE extension_id=?",
        )
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
        query("UPDATE extension_data_namespaces SET entries=?,bytes=? WHERE extension_id=?")
            .bind(totals.0)
            .bind(totals.1)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
