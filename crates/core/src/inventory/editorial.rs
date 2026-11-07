//! Editorial reconciliation and diagnostics; mutations use the caller transaction.

use crate::database::{query, query_scalar};
use crate::identity::IdentityStore;
use serde_json::{Value, json};
use sqlx::{Execute, Row};

pub async fn refresh_editorial_for_works(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    work_ids: &std::collections::BTreeSet<String>,
) -> Result<(), sqlx::Error> {
    if work_ids.is_empty() {
        return Ok(());
    }

    let work_ids_vec: Vec<&String> = work_ids.iter().collect();
    for chunk in work_ids_vec.chunks(250) {
        let mut builder = sqlx::QueryBuilder::<sqlx::Any>::new(
            "SELECT e.work_id, m.fallback_json, m.nfo_json, a.relative_path, m.nfo_priority, m.nfo_path 
             FROM inventory_editorial m 
             JOIN inventory_assets a ON a.id=m.asset_id 
             JOIN inventory_sources s ON s.id=a.source_id 
             JOIN inventory_versions v ON v.id=s.version_id 
             JOIN inventory_editions e ON e.id=v.edition_id 
             JOIN libraries l ON l.id=s.library_id 
             JOIN media_nodes n ON n.id=l.node_id
             WHERE a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked' AND e.work_id IN ("
        );
        let mut separated = builder.separated(", ");
        for id in chunk {
            separated.push_bind(id);
        }
        separated.push_unseparated(") ");
        builder.push(
            "ORDER BY e.work_id, m.nfo_json IS NULL, m.nfo_priority NULLS FIRST, m.nfo_path NULLS FIRST, a.relative_path",
        );

        let sql = crate::database::normalize_sql(builder.sql().as_str());
        let arguments = builder
            .build()
            .take_arguments()
            .map_err(sqlx::Error::Encode)?
            .unwrap_or_default();
        let rows = sqlx::query_with::<sqlx::Any, _>(sqlx::AssertSqlSafe(sql), arguments)
            .fetch_all(&mut **tx)
            .await?;
        let mut grouped: std::collections::BTreeMap<String, Vec<sqlx::any::AnyRow>> =
            std::collections::BTreeMap::new();
        for r in rows {
            let wid: String = r.get("work_id");
            grouped.entry(wid).or_default().push(r);
        }

        let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({}));

        for (wid, work_rows) in grouped {
            let Some(first) = work_rows.first() else {
                continue;
            };
            let fallback = parse(&first.get::<String, _>("fallback_json"));
            let nfo = first
                .get::<Option<String>, _>("nfo_json")
                .map(|t| parse(&t));
            let title = nfo
                .as_ref()
                .and_then(|n| n.get("title"))
                .and_then(Value::as_str)
                .or_else(|| fallback.get("title").and_then(Value::as_str))
                .unwrap_or("Film local")
                .to_owned();
            let year = nfo
                .as_ref()
                .and_then(|n| n.get("year"))
                .and_then(Value::as_i64)
                .or_else(|| fallback.get("year").and_then(Value::as_i64));
            let mut ids = std::collections::BTreeMap::<String, String>::new();
            let mut conflicts = std::collections::BTreeSet::new();
            for row in &work_rows {
                let mut candidates = vec![parse(&row.get::<String, _>("fallback_json"))];
                if let Some(text) = row.get::<Option<String>, _>("nfo_json") {
                    candidates.push(parse(&text));
                }
                for candidate in candidates {
                    // An NFO that contradicts itself already dropped the provider; it must not be
                    // revived by the other source and is reported like any other conflict.
                    for provider in candidate["external_id_conflicts"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        conflicts.insert(provider.to_owned());
                    }
                    if let Some(values) = candidate["external_ids"].as_object() {
                        for (provider, value) in values {
                            if let Some(value) = value.as_str() {
                                if ids.get(provider).is_some_and(|existing| existing != value) {
                                    conflicts.insert(provider.clone());
                                }
                                ids.entry(provider.clone())
                                    .or_insert_with(|| value.to_owned());
                            }
                        }
                    }
                }
            }
            for provider in &conflicts {
                ids.remove(provider);
            }
            let mut metadata = nfo.unwrap_or_else(|| json!({}));
            metadata["external_ids"] = json!(ids);
            metadata["provenance"] =
                json!(if first.get::<Option<String>, _>("nfo_json").is_some() {
                    "nfo"
                } else {
                    "local"
                });
            metadata["warnings"] = json!(
                conflicts
                    .into_iter()
                    .map(|provider| format!(
                        "Conflicting {provider} identifiers; no automatic merge"
                    ))
                    .collect::<Vec<_>>()
            );
            let title_sort = title.to_lowercase();
            query(
                "UPDATE inventory_works SET title=?,year=?,external_ids_json=?,editorial_json=?,title_sort=? WHERE id=?",
            )
            .bind(title)
            .bind(year)
            .bind(json!(ids).to_string())
            .bind(metadata.to_string())
            .bind(title_sort)
            .bind(&wid)
            .execute(&mut **tx)
            .await?;
        }
    }

    crate::automation::reapply_results(tx, &work_ids.iter().cloned().collect::<Vec<_>>()).await?;

    let mut parent_series = std::collections::BTreeSet::new();
    for wid in work_ids {
        crate::consolidation::consolidate_work_tx(tx, wid).await?;
        let s_id: Option<String> = query_scalar(
            "SELECT s.series_id FROM inventory_episodes ep \
             JOIN inventory_seasons s ON s.id = ep.season_id \
             WHERE ep.work_id = ?",
        )
        .bind(wid)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(sid) = s_id
            && !work_ids.contains(&sid)
        {
            parent_series.insert(sid);
        }
    }
    for sid in parent_series {
        crate::consolidation::consolidate_work_tx(tx, &sid).await?;
    }
    Ok(())
}

pub(super) async fn refresh_editorial(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    library_id: &str,
) -> Result<(), sqlx::Error> {
    let works: Vec<String> = query_scalar("SELECT id FROM inventory_works WHERE library_id=?")
        .bind(library_id)
        .fetch_all(&mut **tx)
        .await?;
    let set: std::collections::BTreeSet<String> = works.into_iter().collect();
    refresh_editorial_for_works(tx, &set).await
}

pub async fn refresh_work_editorial(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    work_id: &str,
) -> Result<(), sqlx::Error> {
    let mut set = std::collections::BTreeSet::new();
    set.insert(work_id.to_string());
    refresh_editorial_for_works(tx, &set).await
}

/// Administrator scan diagnostics. Only currently available sources contribute;
/// paths and opaque internal identifiers are never included in these messages.
pub async fn editorial_warnings(
    store: &IdentityStore,
    library_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = query("SELECT w.editorial_json FROM inventory_works w WHERE w.library_id=? AND EXISTS(SELECT 1 FROM inventory_editions e JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id JOIN libraries l ON l.id=s.library_id JOIN media_nodes n ON n.id=l.node_id WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.enabled=1 AND l.kind IN ('films', 'series') AND n.status != 'revoked') ORDER BY w.id")
        .bind(library_id).fetch_all(&store.pool).await?;
    let mut warnings = Vec::new();
    for row in rows {
        if let Ok(metadata) = serde_json::from_str::<Value>(row.get("editorial_json"))
            && let Some(messages) = metadata["warnings"].as_array()
        {
            for message in messages.iter().filter_map(Value::as_str) {
                if warnings.len() == 100 {
                    return Ok(warnings);
                }
                warnings.push(message.to_owned());
            }
        }
    }
    Ok(warnings)
}
