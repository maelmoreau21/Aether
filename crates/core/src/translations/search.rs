//! Folded titles that make a work findable by any of its translated titles and by its original
//! title, without duplicating the work: `work_search_titles` holds one row per distinct folded
//! title and points back at the single `inventory_works` row.
//!
//! The index is rebuilt for one work whenever something that feeds it changes (a translation is
//! stored or removed, the work's editorial or manual metadata is consolidated) and for every
//! work once, in a journaled step, on databases created before the table existed.

use crate::database::query;
use serde_json::Value;
use sqlx::{AnyConnection, Row};
use std::collections::BTreeSet;

const MAX_TITLE: usize = 1_000;
const BATCH: i64 = 500;

/// The folding used for `inventory_works.display_title_sort`: trimmed and Unicode-lowercased, so
/// the same search text matches either column.
pub fn fold(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Original titles a provider or an NFO file recorded, in either spelling used by the code base.
fn original_titles(editorial: &Value, manual: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for source in [manual, editorial] {
        for key in ["original_title", "originalTitle"] {
            if let Some(title) = source.get(key).and_then(Value::as_str) {
                out.push(title.to_owned());
            }
        }
    }
    out
}

/// Rebuild the searchable titles of one work. A work that no longer exists is ignored.
pub async fn refresh(conn: &mut AnyConnection, work_id: &str) -> Result<(), sqlx::Error> {
    let Some(row) = query("SELECT editorial_json, manual_json FROM inventory_works WHERE id=?")
        .bind(work_id)
        .fetch_optional(&mut *conn)
        .await?
    else {
        return Ok(());
    };
    let parse = |text: Option<String>| {
        text.and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .unwrap_or(Value::Null)
    };
    let editorial = parse(row.get::<Option<String>, _>("editorial_json"));
    let manual = parse(row.get::<Option<String>, _>("manual_json"));
    let mut titles: BTreeSet<String> = original_titles(&editorial, &manual)
        .iter()
        .map(|title| fold(title))
        .collect();
    for row in query(
        "SELECT title FROM work_translations WHERE work_id=? AND title IS NOT NULL AND title<>''",
    )
    .bind(work_id)
    .fetch_all(&mut *conn)
    .await?
    {
        titles.insert(fold(&row.get::<String, _>("title")));
    }
    titles.retain(|title| !title.is_empty() && title.chars().count() <= MAX_TITLE);

    let current: BTreeSet<String> =
        query("SELECT title_search FROM work_search_titles WHERE work_id=?")
            .bind(work_id)
            .fetch_all(&mut *conn)
            .await?
            .iter()
            .map(|row| row.get::<String, _>("title_search"))
            .collect();
    if current == titles {
        return Ok(());
    }
    query("DELETE FROM work_search_titles WHERE work_id=?")
        .bind(work_id)
        .execute(&mut *conn)
        .await?;
    for title in &titles {
        query("INSERT INTO work_search_titles(work_id,title_search) VALUES(?,?) ON CONFLICT DO NOTHING")
            .bind(work_id)
            .bind(title)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Fill the index for every existing work. Safe to run again: `refresh` only writes differences.
pub async fn backfill(conn: &mut AnyConnection) -> Result<u64, sqlx::Error> {
    let mut after = String::new();
    let mut seen = 0;
    loop {
        let ids: Vec<String> =
            query("SELECT id FROM inventory_works WHERE id>? ORDER BY id LIMIT ?")
                .bind(&after)
                .bind(BATCH)
                .fetch_all(&mut *conn)
                .await?
                .iter()
                .map(|row| row.get::<String, _>("id"))
                .collect();
        let Some(last) = ids.last().cloned() else {
            return Ok(seen);
        };
        for id in &ids {
            refresh(conn, id).await?;
            seen += 1;
        }
        after = last;
    }
}
