use super::*;

pub async fn complete(
    store: &IdentityStore,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    now: i64,
) -> Result<bool, sqlx::Error> {
    complete_with_marker_sources(store, job, link, result, &[], now).await
}

pub(super) async fn lock_marker_sources(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    target: &str,
    marker_sources: &[(String, String)],
) -> Result<bool, sqlx::Error> {
    if marker_sources.len() < 2 || marker_sources.len() > 32 || marker_sources[0].0 != target {
        return Ok(false);
    }
    let mut unique = std::collections::HashSet::new();
    for (asset, _) in marker_sources {
        if !unique.insert(asset.as_str()) {
            return Ok(false);
        }
    }
    // Lock in a stable order until commit so a scan cannot update either
    // revision after validation.
    let mut sources: Vec<_> = marker_sources.iter().collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    for (asset, fingerprint) in sources {
        let locked = query(
            "UPDATE automation_observations \
                SET fingerprint=fingerprint \
                WHERE asset_id=? AND fingerprint=? AND EXISTS(SELECT 1 FROM inventory_assets a \
                JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id \
                WHERE a.id=automation_observations.asset_id AND a.library_id=? AND a.available=1 \
                AND s.root_snapshot=l.root)",
        )
        .bind(asset)
        .bind(fingerprint)
        .bind(library)
        .execute(&mut **tx)
        .await?;
        if locked.rows_affected() != 1 {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) async fn complete_with_marker_sources(
    store: &IdentityStore,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    marker_sources: &[(String, String)],
    now: i64,
) -> Result<bool, sqlx::Error> {
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write_serialized(&mut conn, crate::task_engine::TASK_ADMISSION_LOCK).await?;
    if link.operation == "intro.detect"
        && !lock_marker_sources(&mut tx, &job.library_id, &job.target_id, marker_sources).await?
    {
        return Ok(false);
    }
    if !persist_result(&mut tx, job, link, result, now).await? {
        return Ok(false);
    }
    let is_episode: i64 = if link.operation == "intro.parse_chapters" {
        query_scalar("SELECT COUNT(*) FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE a.id=?")
            .bind(&job.target_id).fetch_one(&mut *tx).await?
    } else {
        0
    };
    if link.operation == "intro.parse_chapters" && is_episode == 0 {
        queue_film_credits_scan(&mut tx, job, now).await?;
    }
    if link.operation == "intro.parse_chapters" && is_episode > 0 {
        let source: Option<String> =
            query_scalar("SELECT fingerprint FROM automation_observations WHERE asset_id=?")
                .bind(&job.target_id)
                .fetch_optional(&mut *tx)
                .await?;
        // The chapters were read again (new settings or a new plugin version) while the
        // episode's audio is already fingerprinted: run the acoustic detection again too, or
        // it would keep the result of the old settings. The completed extraction is reused.
        // A chapter task is restarted in place, so the key names the attempt: each re-read
        // asks for its own detection.
        if let Some(source) = &source
            && let Some(completed) = completed_fingerprint(&mut tx, &job.target_id, source).await?
        {
            emit(
                &mut tx,
                &EventInput {
                    key: &format!("marker-chapters-reread:{}:{}", job.id, job.attempt),
                    kind: "manual",
                    library_id: &job.library_id,
                    asset_id: &job.target_id,
                    fingerprint: source,
                    payload: &serde_json::json!({"fingerprintTaskId": completed, "reanalyzeMarkers": true, "stale": false, "extensionId": link.extension_id}),
                },
                now,
            )
            .await?;
        }
        let intro_enabled =
            query_scalar::<_, String>("SELECT options_json FROM libraries WHERE id=?")
                .bind(&job.library_id)
                .fetch_optional(&mut *tx)
                .await?
                .and_then(|encoded| {
                    serde_json::from_str::<crate::library_options::LibraryOptions>(&encoded).ok()
                })
                .is_none_or(|options| options.intro_detection_enabled);
        // Every episode is fingerprinted, even one whose chapters name its intro and credits:
        // it is the peer that its season's chapterless episodes are compared with. A failed
        // extraction (e.g. a Node without the chromaprint muxer) is queued again instead of
        // blocking detection for good.
        if let Some(source) = source.filter(|_| intro_enabled) {
            queue_fingerprint(
                &mut tx,
                &FingerprintRequest {
                    library: &job.library_id,
                    node: &job.node_id,
                    asset: &job.target_id,
                    source: &source,
                    reextract_tailless: false,
                },
                now,
            )
            .await?;
        }
    }
    // An acoustic detection records how many episodes it compared, so that one run with fewer
    // than a full batch is offered again when more peers are fingerprinted.
    let processed = if link.operation == "intro.detect" {
        marker_sources.len().max(1) as i64
    } else {
        1
    };
    query("UPDATE durable_tasks SET status='completed',result_json=?,progress=100,processed_items=?,lease_until=NULL,finished_at=?,updated_at=? WHERE id=? AND attempt=? AND status='running'")
 .bind(result.to_string()).bind(processed).bind(now).bind(now).bind(&job.id).bind(job.attempt).execute(&mut *tx).await?;
    let key = format!("analysis:{}", job.id);
    let fingerprint: String = query_scalar("SELECT fingerprint FROM automation_events WHERE id=?")
        .bind(&link.event_id)
        .fetch_one(&mut *tx)
        .await?;
    emit(
        &mut tx,
        &EventInput {
            key: &key,
            kind: "analysis.completed",
            library_id: &job.library_id,
            asset_id: &job.target_id,
            fingerprint: &fingerprint,
            payload: &serde_json::json!({"taskId":job.id}),
        },
        now,
    )
    .await?;
    crate::database::commit(tx).await?;
    crate::task_engine::WAKE.notify_one();
    Ok(true)
}

/// Restore still-current automatic fields after the scanner rebuilds editorial
/// metadata. Revalidation prevents reusing outputs from changed source/config.
pub async fn reapply_results(
    tx: &mut Transaction<'_, Any>,
    work_ids: &[String],
) -> Result<(), sqlx::Error> {
    for work in work_ids {
        let rows: Vec<(String, String, String, String, String)> = query_as(
            "SELECT r.asset_id,r.result_json,r.task_id,r.fingerprint,l.extension_id \
                FROM automation_results r \
                JOIN automation_task_links l ON l.task_id=r.task_id \
                JOIN inventory_assets a ON a.id=r.asset_id \
                JOIN inventory_sources s ON s.id=a.source_id \
                JOIN inventory_versions v ON v.id=s.version_id \
                JOIN inventory_editions e ON e.id=v.edition_id \
                JOIN extensions x ON x.id=l.extension_id \
                JOIN automation_events event ON event.id=l.event_id \
                JOIN automation_observations o ON o.asset_id=a.id \
                WHERE e.work_id=? AND a.available=1 AND x.enabled=1 AND x.generation=l.generation \
                AND o.fingerprint=event.fingerprint AND (x.all_libraries=1 OR EXISTS(SELECT 1 \
                FROM extension_grants g WHERE g.extension_id=x.id AND (g.library_id=a.library_id \
                OR EXISTS(SELECT 1 FROM library_folders granted \
                WHERE granted.library_id=a.library_id AND granted.parent_id=g.library_id \
                AND granted.active=1)))) \
                ORDER BY r.updated_at DESC \
                LIMIT 100",
        )
        .bind(work)
        .fetch_all(&mut **tx)
        .await?;
        for (asset, raw, task, _, _) in rows {
            let link: TaskLink = query_as("SELECT * FROM automation_task_links WHERE task_id=?")
                .bind(&task)
                .fetch_one(&mut **tx)
                .await?;
            let library: String =
                query_scalar("SELECT library_id FROM inventory_assets WHERE id=?")
                    .bind(&asset)
                    .fetch_one(&mut **tx)
                    .await?;
            if !current_fingerprint_matches(tx, &library, &asset, &link).await? {
                continue;
            }
            if can_write_catalogue(tx, &link.extension_id).await?
                && let Ok(result) = serde_json::from_str::<Value>(&raw)
            {
                apply_missing_metadata(
                    tx,
                    &asset,
                    &link.extension_id,
                    &link.fingerprint,
                    &result,
                    media_contracts::protocol::unix_now(),
                )
                .await?;
            }
        }
    }
    Ok(())
}

pub(super) async fn can_write_catalogue(
    tx: &mut Transaction<'_, Any>,
    extension: &str,
) -> Result<bool, sqlx::Error> {
    let raw: Option<String> = query_scalar("SELECT package_json FROM extensions WHERE id=?")
        .bind(extension)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(raw
        .and_then(|raw| serde_json::from_str::<media_extension_sdk::Package>(&raw).ok())
        .is_some_and(|package| {
            package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "metadata.write")
        }))
}

/// Dependencies are attached before admission to a worker. Reject cycles and
/// edits to running jobs; prerequisite failures become explicit terminal errors.
pub async fn add_dependency(
    tx: &mut Transaction<'_, Any>,
    task: &str,
    prerequisite: &str,
) -> Result<(), sqlx::Error> {
    if task == prerequisite {
        return Err(sqlx::Error::Protocol("dependency cycle".into()));
    }
    let ready: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE id=? AND status='queued' AND lease_until IS NULL",
    )
    .bind(task)
    .fetch_one(&mut **tx)
    .await?;
    if ready != 1 {
        return Err(sqlx::Error::Protocol(
            "dependency task is not queued".into(),
        ));
    }
    let cycle:i64=query_scalar("WITH RECURSIVE ancestors(id) AS (SELECT prerequisite_id FROM automation_dependencies WHERE task_id=? UNION SELECT d.prerequisite_id FROM automation_dependencies d JOIN ancestors a ON d.task_id=a.id) SELECT COUNT(*) FROM ancestors WHERE id=?")
 .bind(prerequisite).bind(task).fetch_one(&mut **tx).await?;
    if cycle != 0 {
        return Err(sqlx::Error::Protocol("dependency cycle".into()));
    }
    query("INSERT INTO automation_dependencies(task_id,prerequisite_id) VALUES(?,?) ON CONFLICT(task_id,prerequisite_id) DO NOTHING").bind(task).bind(prerequisite).execute(&mut **tx).await?;
    Ok(())
}
