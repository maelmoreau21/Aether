use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub extension_id: String,
    pub library_id: String,
    pub mode: String,
    pub priority: i64,
    pub operation: String,
    pub config_revision: String,
    pub algorithm_version: String,
    pub interval_seconds: i64,
    pub next_run_at: i64,
    pub max_concurrent: i64,
}

pub async fn policies(store: &IdentityStore) -> Result<Vec<Policy>, sqlx::Error> {
    query_as("SELECT * FROM automation_policies ORDER BY library_id,extension_id LIMIT 1000")
        .fetch_all(store.pool())
        .await
}

pub async fn set_policy(store: &IdentityStore, p: &Policy, now: i64) -> Result<(), sqlx::Error> {
    if !matches!(
        p.mode.as_str(),
        "automatic" | "manual" | "disabled" | "scheduled"
    ) || !(0..=1000).contains(&p.priority)
        || !(1..=16).contains(&p.max_concurrent)
        || !(60..=31536000).contains(&p.interval_seconds)
        || p.config_revision.len() > 128
        || p.algorithm_version.len() > 128
        || !matches!(
            p.operation.as_str(),
            "metadata.propose" | "metadata.search" | "intro.parse_chapters"
        )
    {
        return Err(sqlx::Error::Protocol("invalid automation policy".into()));
    }
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write_serialized(&mut conn, crate::task_engine::TASK_ADMISSION_LOCK).await?;
    query("INSERT INTO automation_policies(extension_id,library_id,mode,priority,operation,config_revision,algorithm_version,interval_seconds,next_run_at,max_concurrent) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(extension_id,library_id) DO UPDATE \
            SET mode=excluded.mode,priority=excluded.priority,operation=excluded.operation,config_revision=excluded.config_revision,algorithm_version=excluded.algorithm_version,interval_seconds=excluded.interval_seconds,next_run_at=excluded.next_run_at,max_concurrent=excluded.max_concurrent")
 .bind(&p.extension_id).bind(&p.library_id).bind(&p.mode).bind(p.priority).bind(&p.operation).bind(&p.config_revision).bind(&p.algorithm_version).bind(p.interval_seconds).bind(p.next_run_at).bind(p.max_concurrent).execute(&mut *tx).await?;
    if p.mode == "automatic" {
        schedule_library(
            &mut tx,
            &p.library_id,
            "extension.activated",
            &uuid::Uuid::new_v4().to_string(),
            Some(&p.extension_id),
            now,
        )
        .await?;
    }
    crate::database::commit(tx).await?;
    crate::task_engine::WAKE.notify_one();
    Ok(())
}

/// Persist a bounded, keyset-paginated fanout request, not one future per asset.
pub async fn schedule_library(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    kind: &str,
    key: &str,
    extension: Option<&str>,
    now: i64,
) -> Result<String, sqlx::Error> {
    if !matches!(
        kind,
        "manual" | "scheduled" | "extension.activated" | "metadata.missing"
    ) {
        return Err(sqlx::Error::Protocol("invalid automation trigger".into()));
    }
    let id = hash(&(library, kind, key, extension))?;
    query("INSERT INTO automation_sweeps(id,library_id,extension_id,kind,created_at) VALUES(?,?,?,?,?) ON CONFLICT(id) DO NOTHING")
 .bind(&id).bind(library).bind(extension.unwrap_or("")).bind(kind).bind(now).execute(&mut **tx).await?;
    query("INSERT INTO automation_sweeps(id,library_id,extension_id,kind,created_at) SELECT ? || ':' || library_id,library_id,?,?,? FROM library_folders WHERE parent_id=? AND active=1 ON CONFLICT(id) DO NOTHING")
    .bind(&id).bind(extension.unwrap_or("")).bind(kind).bind(now).bind(library).execute(&mut **tx).await?;
    Ok(id)
}

pub(super) async fn pump_triggers(
    tx: &mut Transaction<'_, Any>,
    now: i64,
    limit: i64,
) -> Result<(), sqlx::Error> {
    let ready: Vec<(String, String, String, String, String, i64)> = query_as(
        "SELECT t.id,t.library_id,t.target_id,t.idempotency_key,obs.fingerprint,t.attempt \
            FROM durable_tasks t \
            JOIN automation_observations obs ON obs.asset_id=t.target_id \
            WHERE t.category='analyze' AND t.component='fingerprint' AND t.status='completed' \
            AND NOT EXISTS(SELECT 1 FROM automation_events ev WHERE ev.event_key=CASE \
            WHEN t.attempt<=1 THEN 'fingerprint-ready:' || t.id \
            ELSE 'fingerprint-ready:' || t.id || ':' || CAST(t.attempt AS TEXT) END) \
            ORDER BY t.finished_at,t.id \
            LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    for (task, library, asset, key, source, attempt) in ready {
        let stale = key != fingerprint_key(&asset, &source)?;
        let reextracted = attempt > 1;
        let event_key = if reextracted {
            format!("fingerprint-ready:{task}:{attempt}")
        } else {
            format!("fingerprint-ready:{task}")
        };
        let payload = serde_json::json!({
            "fingerprintTaskId": task,
            "stale": stale,
            "redetect": reextracted,
        });
        emit(
            tx,
            &EventInput {
                key: &event_key,
                kind: "analysis.completed",
                library_id: &library,
                asset_id: &asset,
                fingerprint: &source,
                payload: &payload,
            },
            now,
        )
        .await?;
        if stale {
            continue;
        }
        // Episodes fingerprinted before any season peer were skipped by
        // intro.detect, and episodes detected before a 3rd peer existed ran
        // with a 2-episode batch. Re-offer them now; task-link fingerprints deduplicate.
        for peer in usable_fingerprint_peers(tx, &asset, &library)
            .await?
            .into_iter()
            .take(8)
        {
            let existing: Option<(String, i64)> = query_as("SELECT detect.status,detect.processed_items FROM automation_task_links link JOIN durable_tasks detect ON detect.id=link.task_id WHERE link.operation='intro.detect' AND detect.target_id=? ORDER BY detect.updated_at DESC,detect.id LIMIT 1")
                .bind(&peer.asset_id)
                .fetch_optional(&mut **tx)
                .await?;
            let redetect = existing
                .as_ref()
                .is_some_and(|(status, items)| status == "completed" && *items > 0 && *items < 3);
            if existing.as_ref().is_some_and(|(status, _)| {
                !matches!(status.as_str(), "error" | "cancelled") && !redetect
            }) {
                continue;
            }
            let peer_payload = serde_json::json!({
                "fingerprintTaskId": peer.task_id,
                "stale": false,
                "redetect": redetect,
            });
            emit(
                tx,
                &EventInput {
                    key: &format!("fingerprint-peer:{}:{task}", peer.task_id),
                    kind: "analysis.completed",
                    library_id: &library,
                    asset_id: &peer.asset_id,
                    fingerprint: &peer.source,
                    payload: &peer_payload,
                },
                now,
            )
            .await?;
        }
    }
    // A film's end picture was measured: its chapters are read again with the measurement.
    let scans: Vec<(String, String, String, String, String, i64)> = query_as(
        "SELECT t.id,t.library_id,t.target_id,t.idempotency_key,obs.fingerprint,t.attempt \
            FROM durable_tasks t \
            JOIN automation_observations obs ON obs.asset_id=t.target_id \
            WHERE t.category='analyze' AND t.component='credits' AND t.status='completed' \
            AND NOT EXISTS(SELECT 1 FROM automation_events ev \
            WHERE ev.event_key='credits-ready:' || t.id || ':' || CAST(t.attempt AS TEXT)) \
            ORDER BY t.finished_at,t.id \
            LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?;
    for (task, library, asset, key, source, attempt) in scans {
        let stale = key != credits_scan_key(&asset, &source)?;
        emit(
            tx,
            &EventInput {
                key: &format!("credits-ready:{task}:{attempt}"),
                kind: "analysis.completed",
                library_id: &library,
                asset_id: &asset,
                fingerprint: &source,
                payload: &serde_json::json!({"creditsTaskId": task, "stale": stale}),
            },
            now,
        )
        .await?;
    }
    let due:Vec<(String,String,i64,i64)>=query_as("SELECT extension_id,library_id,interval_seconds,next_run_at FROM automation_policies WHERE mode='scheduled' AND next_run_at<=? ORDER BY next_run_at,extension_id,library_id LIMIT ?").bind(now).bind(limit).fetch_all(&mut **tx).await?;
    for (extension, library, interval, due) in due {
        schedule_library(
            tx,
            &library,
            "scheduled",
            &due.to_string(),
            Some(&extension),
            now,
        )
        .await?;
        query("UPDATE automation_policies SET next_run_at=? WHERE extension_id=? AND library_id=?")
            .bind(now.saturating_add(interval))
            .bind(extension)
            .bind(library)
            .execute(&mut **tx)
            .await?;
    }
    let sweeps:Vec<(String,String,String,String,String)>=query_as("SELECT id,library_id,extension_id,kind,cursor_asset FROM automation_sweeps WHERE completed_at IS NULL ORDER BY created_at,id LIMIT 1").fetch_all(&mut **tx).await?;
    for (id, library, extension, kind, cursor) in sweeps {
        let library_kind: String = query_scalar("SELECT kind FROM libraries WHERE id=?")
            .bind(&library)
            .fetch_one(&mut **tx)
            .await?;
        let rows: Vec<(String, String, String, String, i64)> = if library_kind == "music" {
            query_as(
                "SELECT t.id,obs.fingerprint,'{}','{}',CAST(CASE WHEN t.cover_path IS NULL \
                    OR t.cover_path='' THEN 0 ELSE 1 END AS BIGINT) \
                    FROM music_tracks t \
                    JOIN automation_observations obs ON obs.asset_id=t.id \
                    JOIN libraries l ON l.id=t.library_id \
                    WHERE t.library_id=? AND t.available=1 AND t.root_snapshot=l.root AND t.id>? \
                    ORDER BY t.id \
                    LIMIT ?",
            )
            .bind(&library)
            .bind(&cursor)
            .bind(limit + 1)
            .fetch_all(&mut **tx)
            .await?
        } else {
            query_as("SELECT a.id,obs.fingerprint,w.editorial_json,COALESCE(w.manual_json,'{}'),CAST((SELECT COUNT(*) FROM work_image_entries WHERE work_id=w.id AND kind='poster') + (SELECT COUNT(*) FROM work_posters WHERE work_id=w.id) AS INTEGER) \
                    FROM inventory_assets a \
                    JOIN automation_observations obs ON obs.asset_id=a.id \
                    JOIN inventory_sources source ON source.id=a.source_id \
                    JOIN inventory_versions version ON version.id=source.version_id \
                    JOIN inventory_editions edition ON edition.id=version.edition_id \
                    JOIN inventory_works w ON w.id=edition.work_id \
                    WHERE a.library_id=? AND a.available=1 AND a.id>? \
                    ORDER BY a.id \
                    LIMIT ?").bind(&library).bind(&cursor).bind(limit+1).fetch_all(&mut **tx).await?
        };
        let more = rows.len() as i64 > limit;
        for (asset, fingerprint, editorial, manual, poster_count) in
            rows.into_iter().take(limit as usize)
        {
            query("UPDATE automation_sweeps SET cursor_asset=? WHERE id=?")
                .bind(&asset)
                .bind(&id)
                .execute(&mut **tx)
                .await?;
            if kind == "metadata.missing"
                && poster_count > 0
                && !metadata_has_missing_fields(&editorial, &manual)
            {
                continue;
            }
            let key = hash(&(&id, &asset))?;
            let payload = if extension.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::json!({"extensionId":extension})
            };
            emit(
                tx,
                &EventInput {
                    key: &key,
                    kind: &kind,
                    library_id: &library,
                    asset_id: &asset,
                    fingerprint: &fingerprint,
                    payload: &payload,
                },
                now,
            )
            .await?;
        }
        if !more {
            query("UPDATE automation_sweeps SET completed_at=? WHERE id=?")
                .bind(now)
                .bind(id)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}
