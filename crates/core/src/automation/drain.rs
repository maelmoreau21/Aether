use super::*;

#[derive(sqlx::FromRow)]
pub(super) struct Candidate {
    id: String,
    package_json: String,
    generation: String,
    mode: String,
    priority: i64,
    operation: String,
    config_revision: String,
    algorithm_version: String,
    settings_json: String,
}

/// Bounded admissions; invoked by the task worker's existing tick. Cursor and jobs
/// commit together, so a crash cannot acknowledge an event without its tasks.
pub async fn drain(store: &IdentityStore, now: i64, limit: i64) -> Result<u64, sqlx::Error> {
    let _writer = store.task_writes.lock().await;
    let mut conn = store.pool().acquire().await?;
    let mut tx = begin_write_serialized(&mut conn, crate::task_engine::TASK_ADMISSION_LOCK).await?;
    let owner:Option<String>=query_scalar("SELECT id FROM users WHERE role='administrator' AND disabled_at IS NULL ORDER BY id LIMIT 1").fetch_optional(&mut *tx).await?;
    let Some(owner) = owner else { return Ok(0) };
    pump_triggers(&mut tx, now, limit.clamp(1, 100)).await?;
    query(
        "UPDATE durable_tasks \
            SET status='error',error='Prerequisite failed or cancelled',updated_at=?,finished_at=? \
            WHERE status='queued' AND lease_until IS NULL AND EXISTS(SELECT 1 \
            FROM automation_dependencies d JOIN durable_tasks p ON p.id=d.prerequisite_id \
            WHERE d.task_id=durable_tasks.id AND p.status IN ('error','cancelled'))",
    )
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let mut pending: i64 = query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE status IN ('queued','running','paused')",
    )
    .fetch_one(&mut *tx)
    .await?;
    let events:Vec<(String,String,String,String,String,String,String)>=query_as("SELECT id,kind,library_id,asset_id,fingerprint,cursor_extension,payload_json FROM automation_events WHERE dispatched_at IS NULL ORDER BY created_at,id LIMIT ?").bind(limit.clamp(1,100)).fetch_all(&mut *tx).await?;
    let mut admitted = 0;
    let budget = limit.clamp(1, 100);
    let mut examined = 0;
    for (event, kind, library, asset, file_fingerprint, cursor, payload) in events {
        let event_payload = serde_json::from_str::<Value>(&payload).unwrap_or(Value::Null);
        let selected = event_payload["extensionId"].as_str().map(str::to_owned);
        let fingerprint_ready =
            event_payload["fingerprintTaskId"].as_str().is_some() && event_payload["stale"] != true;
        // A film's end picture was measured: its chapters are read again with it.
        let credits_event = event_payload.get("creditsTaskId").is_some();
        let credits_ready = credits_event && event_payload["stale"] != true;
        if kind == "media.removed" {
            query(
                "UPDATE durable_tasks \
                    SET status='cancelled',updated_at=?,finished_at=CASE WHEN lease_until IS NULL \
                    THEN ? ELSE NULL END \
                    WHERE target_id=? AND status IN ('queued','running','paused') \
                    AND id IN (SELECT task_id FROM automation_task_links)",
            )
            .bind(now)
            .bind(now)
            .bind(&asset)
            .execute(&mut *tx)
            .await?;
            query("UPDATE automation_events SET dispatched_at=? WHERE id=?")
                .bind(now)
                .bind(&event)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        if kind == "media.modified" {
            query(
                "UPDATE durable_tasks \
                    SET status='cancelled',updated_at=?,finished_at=CASE WHEN lease_until IS NULL \
                    THEN ? ELSE NULL END \
                    WHERE target_id=? AND status IN ('queued','running','paused') \
                    AND id IN (SELECT l.task_id FROM automation_task_links l \
                    JOIN automation_events original ON original.id=l.event_id \
                    WHERE original.fingerprint!=?)",
            )
            .bind(now)
            .bind(now)
            .bind(&asset)
            .bind(&file_fingerprint)
            .execute(&mut *tx)
            .await?;
        }
        let (options, library_kind): (String, String) =
            query_as("SELECT options_json, kind FROM libraries WHERE id=?")
                .bind(&library)
                .fetch_one(&mut *tx)
                .await?;
        let options: Value = serde_json::from_str(&options)
            .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
        let candidates: Vec<Candidate> = query_as(
            "SELECT e.id,e.package_json,e.generation,COALESCE(p.mode,'automatic') AS \
                mode,COALESCE(p.priority,100) AS priority,COALESCE(p.operation,'') AS \
                operation,COALESCE(p.config_revision,'') AS \
                config_revision,COALESCE(p.algorithm_version,'1') AS \
                algorithm_version,COALESCE(s.settings_json,'{}') AS settings_json \
                FROM extensions e \
                JOIN extension_publishers publisher ON publisher.id=e.publisher_id \
                LEFT \
                JOIN automation_policies p ON p.extension_id=e.id \
                AND p.library_id=COALESCE((SELECT own.library_id FROM automation_policies own \
                WHERE own.extension_id=e.id AND own.library_id=?),(SELECT parent_id \
                FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)) \
                LEFT \
                JOIN extension_settings s ON s.extension_id=e.id \
                WHERE e.enabled=1 AND publisher.active=1 AND e.id>? AND (e.all_libraries=1 \
                OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id \
                AND (g.library_id=? OR EXISTS(SELECT 1 FROM library_folders granted \
                WHERE granted.library_id=? AND granted.parent_id=g.library_id \
                AND granted.active=1)))) \
                ORDER BY e.id \
                LIMIT ?",
        )
        .bind(&library)
        .bind(&library)
        .bind(&cursor)
        .bind(&library)
        .bind(&library)
        .bind(budget - examined + 1)
        .fetch_all(&mut *tx)
        .await?;
        let more = candidates.len() as i64 > budget - examined;
        for mut c in candidates.into_iter().take((budget - examined) as usize) {
            examined += 1;
            query("UPDATE automation_events SET cursor_extension=? WHERE id=?")
                .bind(&c.id)
                .bind(&event)
                .execute(&mut *tx)
                .await?;
            if selected.as_ref().is_some_and(|id| id != &c.id) {
                continue;
            }
            if c.mode == "disabled"
                || (c.mode == "manual" && !matches!(kind.as_str(), "manual" | "metadata.missing"))
                || (c.mode == "scheduled" && kind != "scheduled")
            {
                continue;
            }
            let package: media_extension_sdk::Package = match serde_json::from_str(&c.package_json)
            {
                Ok(p) => p,
                Err(_) => continue,
            };
            let marker_plugin = package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "playback.markers");
            if kind == "analysis.completed" && marker_plugin && !fingerprint_ready && !credits_ready
            {
                continue;
            }
            if (fingerprint_ready || credits_event) && !marker_plugin {
                continue;
            }
            if !package
                .manifest
                .permissions
                .iter()
                .any(|p| p == "metadata.read" || p == "playback.markers")
            {
                continue;
            }
            if marker_plugin {
                let episode: i64 = query_scalar("SELECT COUNT(*) FROM inventory_episodes ep JOIN inventory_editions e ON e.work_id=ep.work_id JOIN inventory_versions v ON v.edition_id=e.id JOIN inventory_sources s ON s.version_id=v.id JOIN inventory_assets a ON a.source_id=s.id WHERE a.id=?")
                    .bind(&asset).fetch_one(&mut *tx).await?;
                if episode == 0 {
                    // A film has no season to compare its audio with: its segments come from
                    // its chapters (and the administrator's corrections) only.
                    let film: i64 = query_scalar(
                        "SELECT COUNT(*) \
                            FROM inventory_assets a \
                            JOIN inventory_sources s ON s.id=a.source_id \
                            JOIN inventory_versions v ON v.id=s.version_id \
                            JOIN inventory_editions e ON e.id=v.edition_id \
                            JOIN inventory_works w ON w.id=e.work_id \
                            WHERE a.id=? AND w.kind='film'",
                    )
                    .bind(&asset)
                    .fetch_one(&mut *tx)
                    .await?;
                    if film == 0 || fingerprint_ready {
                        continue;
                    }
                } else if credits_ready {
                    continue;
                }
                if episode > 0 && fingerprint_ready {
                    // Only peers the detection itself would accept count: a stale extraction
                    // or one from another library would make it fail for good.
                    let peers: std::collections::HashSet<String> =
                        usable_fingerprint_peers(&mut tx, &asset, &library)
                            .await?
                            .into_iter()
                            .map(|peer| peer.work_id)
                            .collect();
                    if peers.is_empty() {
                        // Episodes whose chapters name every segment were not fingerprinted
                        // by older builds: give them one, so this episode has peers once they
                        // complete (their own fingerprint-ready events re-offer it).
                        let enabled = serde_json::from_value::<
                            crate::library_options::LibraryOptions,
                        >(options.clone())
                        .is_ok_and(|options| options.intro_detection_enabled);
                        if enabled {
                            for (peer, peer_library, node, source) in
                                season_fingerprint_candidates(&mut tx, &asset, &library, 64).await?
                            {
                                queue_fingerprint(
                                    &mut tx,
                                    &FingerprintRequest {
                                        library: &peer_library,
                                        node: &node,
                                        asset: &peer,
                                        source: &source,
                                        reextract_tailless: false,
                                    },
                                    now,
                                )
                                .await?;
                            }
                        }
                        continue;
                    }
                }
            }
            if marker_plugin
                && matches!(
                    c.operation.as_str(),
                    "" | "intro.parse_chapters" | "intro.detect"
                )
            {
                c.operation = if fingerprint_ready {
                    "intro.detect"
                } else {
                    "intro.parse_chapters"
                }
                .into();
            } else if c.operation.is_empty() {
                c.operation = if marker_plugin {
                    if fingerprint_ready {
                        "intro.detect"
                    } else {
                        "intro.parse_chapters"
                    }
                } else if package.manifest.category.as_deref() == Some("Lyrics")
                    || package.manifest.permissions.iter().any(|p| p == "lyrics")
                {
                    "lyrics.get"
                } else if package.manifest.http_hosts.is_empty() {
                    "metadata.propose"
                } else {
                    "metadata.search"
                }
                .into();
            }
            let library_options: crate::library_options::LibraryOptions =
                serde_json::from_value(options.clone())
                    .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
            if marker_plugin && !library_options.intro_detection_enabled {
                continue;
            }
            let priority = if marker_plugin {
                crate::library_options::LibraryOptions::provider_priority(
                    &library_options.segment_providers,
                    &c.id,
                    c.priority,
                )
            } else if c.operation == "metadata.images" {
                crate::library_options::LibraryOptions::provider_priority(
                    &library_options.image_providers,
                    &c.id,
                    c.priority,
                )
            } else if c.operation == "lyrics.get" || selected.is_some() {
                crate::library_options::LibraryOptions::provider_priority(
                    &library_options.metadata_providers,
                    &c.id,
                    c.priority,
                )
            } else {
                library_options.metadata_provider_priority_for_kind(
                    &library_kind,
                    &c.id,
                    c.priority,
                )
            };
            let Some(priority) = priority else {
                continue;
            };
            c.priority = priority;
            if kind == "scheduled" && marker_plugin {
                continue;
            }
            if c.operation == "lyrics.get" {
                let existing: Option<String> = query_scalar(
                    "SELECT lyrics FROM music_tracks WHERE id=? AND library_id=? AND available=1",
                )
                .bind(&asset)
                .bind(&library)
                .fetch_optional(&mut *tx)
                .await?;
                if existing
                    .as_deref()
                    .is_none_or(|lyrics| !lyrics.trim().is_empty() && kind != "scheduled")
                {
                    continue;
                }
            }
            if marker_plugin
                && c.operation
                    != if fingerprint_ready {
                        "intro.detect"
                    } else {
                        "intro.parse_chapters"
                    }
            {
                continue;
            }
            let settings: Value = serde_json::from_str(&c.settings_json)
                .map_err(|_| sqlx::Error::Protocol("invalid extension settings".into()))?;
            let fingerprint = hash(&(
                &asset,
                &file_fingerprint,
                &c.id,
                &c.generation,
                &package.sha256,
                &c.config_revision,
                &c.algorithm_version,
                &settings,
                &c.operation,
                &options,
            ))?;
            let exists: i64 = query_scalar(
                "SELECT COUNT(*) FROM automation_task_links WHERE extension_id=? AND fingerprint=?",
            )
            .bind(&c.id)
            .bind(&fingerprint)
            .fetch_one(&mut *tx)
            .await?;
            if exists > 0 {
                // A new fingerprint (the episode's own, or a peer's) gives a failed detection
                // a new chance, and reruns a completed one when the event asks for it (a
                // re-extraction, or a detection that ran with fewer peers than a full batch).
                let fingerprint_event = kind == "analysis.completed"
                    && marker_plugin
                    && (fingerprint_ready || credits_ready);
                if (fingerprint_event
                    || matches!(kind.as_str(), "manual" | "metadata.missing" | "scheduled"))
                    && pending < 100000
                {
                    // An explicit reanalysis reruns a completed marker task: the acoustic
                    // detection of an episode, or the chapter reading of a film.
                    let reanalyze_markers = (kind == "manual"
                        && marker_plugin
                        && event_payload["reanalyzeMarkers"] == true)
                        || (fingerprint_event
                            && (event_payload["redetect"] == true || credits_ready));
                    // A restarted task runs under an administrator who can still publish its
                    // result: the one that created it may have been disabled or demoted since.
                    let restarted=query("UPDATE durable_tasks \
                            SET status='queued',failure_count=0,available_at=?,updated_at=?,finished_at=NULL,error='',owner=CASE WHEN EXISTS(SELECT 1 FROM users u WHERE u.id=durable_tasks.owner AND u.role='administrator' AND u.disabled_at IS NULL) THEN durable_tasks.owner ELSE ? END \
                            WHERE (durable_tasks.status IN ('error','cancelled') \
                            OR (durable_tasks.status='completed' AND ?=1)) \
                            AND durable_tasks.lease_until IS NULL \
                            AND durable_tasks.id IN (SELECT task_id FROM automation_task_links \
                            WHERE extension_id=? AND fingerprint=?)")
                    .bind(now).bind(now).bind(&owner).bind(i64::from(reanalyze_markers || kind == "scheduled")).bind(&c.id).bind(&fingerprint).execute(&mut *tx).await?.rows_affected();
                    pending += restarted as i64;
                    admitted += restarted;
                }
                continue;
            }
            if pending >= 100000 {
                return Ok(0);
            }
            let id = uuid::Uuid::new_v4().to_string();
            let lib_kind: String = query_scalar("SELECT kind FROM libraries WHERE id=?")
                .bind(&library)
                .fetch_one(&mut *tx)
                .await?;
            let node: Option<String> = if lib_kind == "music" {
                query_scalar(
                    "SELECT l.node_id \
                        FROM libraries l \
                        JOIN media_nodes n ON n.id=l.node_id \
                        JOIN music_tracks t ON t.library_id=l.id \
                        WHERE l.id=? AND t.id=? AND t.available=1 AND n.status!='revoked' \
                        AND (l.enabled=1 OR EXISTS(SELECT 1 FROM library_folders f \
                        JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id \
                        AND f.active=1 AND parent.enabled=1))",
                )
                .bind(&library)
                .bind(&asset)
                .fetch_optional(&mut *tx)
                .await?
            } else {
                query_scalar(
                    "SELECT l.node_id \
                        FROM libraries l \
                        JOIN media_nodes n ON n.id=l.node_id \
                        JOIN inventory_assets a ON a.library_id=l.id \
                        WHERE l.id=? AND a.id=? AND a.available=1 AND n.status!='revoked' \
                        AND (l.enabled=1 OR EXISTS(SELECT 1 FROM library_folders f \
                        JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id \
                        AND f.active=1 AND parent.enabled=1))",
                )
                .bind(&library)
                .bind(&asset)
                .fetch_optional(&mut *tx)
                .await?
            };
            let Some(node) = node else { continue };
            query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,priority,created_at,updated_at,available_at) VALUES(?,?,?,?,?,'analyze',?,?,?,?,?)")
 .bind(&id).bind(&library).bind(&owner).bind(format!("auto:{fingerprint}")).bind(node).bind(&asset).bind(c.priority).bind(now).bind(now).bind(now).execute(&mut *tx).await?;
            let provider = package
                .manifest
                .http_hosts
                .first()
                .cloned()
                .unwrap_or_else(|| c.id.clone());
            query("INSERT INTO automation_task_links(task_id,event_id,extension_id,operation,fingerprint,generation,provider) VALUES(?,?,?,?,?,?,?)").bind(id).bind(&event).bind(c.id).bind(c.operation).bind(fingerprint).bind(c.generation).bind(provider).execute(&mut *tx).await?;
            admitted += 1;
            pending += 1;
        }
        if !more {
            query("UPDATE automation_events SET dispatched_at=? WHERE id=?")
                .bind(now)
                .bind(&event)
                .execute(&mut *tx)
                .await?;
        }
        if examined >= budget {
            break;
        }
    }
    crate::database::commit(tx).await?;
    if admitted > 0 {
        crate::task_engine::WAKE.notify_one();
    }
    Ok(admitted)
}

/// Store validated output as a separate automatic proposal. Manual editorial
/// fields, rights and language choices are never overwritten here.
pub async fn persist_result(
    tx: &mut Transaction<'_, Any>,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let encoded =
        serde_json::to_string(result).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    if encoded.len() > 65536 {
        return Err(sqlx::Error::Protocol("automation result too large".into()));
    }
    let is_music_target: i64 = query_scalar("SELECT COUNT(*) FROM music_tracks WHERE id=?")
        .bind(&job.target_id)
        .fetch_one(&mut **tx)
        .await?;
    let valid: i64 = if is_music_target > 0 {
        query_scalar(
            "SELECT COUNT(*) \
                FROM durable_tasks t \
                JOIN extensions e ON e.id=? \
                JOIN extension_publishers p ON p.id=e.publisher_id \
                JOIN music_tracks a ON a.id=t.target_id \
                WHERE t.id=? AND t.attempt=? AND t.node_id=? AND t.status='running' \
                AND t.lease_until>? AND e.enabled=1 AND e.generation=? AND p.active=1 \
                AND a.available=1 AND EXISTS(SELECT 1 FROM libraries lib JOIN media_nodes node \
                ON node.id=lib.node_id WHERE lib.id=t.library_id AND node.id=t.node_id \
                AND node.status!='revoked' AND lib.enabled=1) AND EXISTS(SELECT 1 FROM users u \
                WHERE u.id=t.owner AND u.role='administrator' AND u.disabled_at IS NULL) \
                AND NOT EXISTS(SELECT 1 FROM automation_policies policy \
                WHERE policy.extension_id=e.id AND policy.library_id=t.library_id \
                AND policy.mode='disabled') AND (e.all_libraries=1 OR EXISTS(SELECT 1 \
                FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=t.library_id))",
        )
        .bind(&link.extension_id)
        .bind(&job.id)
        .bind(job.attempt)
        .bind(&job.node_id)
        .bind(now)
        .bind(&link.generation)
        .fetch_one(&mut **tx)
        .await?
    } else {
        query_scalar(
            "SELECT COUNT(*) \
                FROM durable_tasks t \
                JOIN extensions e ON e.id=? \
                JOIN extension_publishers p ON p.id=e.publisher_id \
                JOIN inventory_assets a ON a.id=t.target_id \
                WHERE t.id=? AND t.attempt=? AND t.node_id=? AND t.status='running' \
                AND t.lease_until>? AND e.enabled=1 AND e.generation=? AND p.active=1 \
                AND a.available=1 AND EXISTS(SELECT 1 FROM libraries lib JOIN media_nodes node \
                ON node.id=lib.node_id WHERE lib.id=t.library_id AND node.id=t.node_id \
                AND node.status!='revoked' AND ((lib.enabled=1 AND NOT EXISTS(SELECT 1 \
                FROM library_folders own WHERE own.library_id=lib.id)) OR EXISTS(SELECT 1 \
                FROM library_folders own JOIN libraries parent ON parent.id=own.parent_id \
                WHERE own.library_id=lib.id AND own.active=1 AND parent.enabled=1))) \
                AND EXISTS(SELECT 1 FROM users u WHERE u.id=t.owner AND u.role='administrator' \
                AND u.disabled_at IS NULL) AND NOT EXISTS(SELECT 1 FROM automation_policies policy \
                WHERE policy.extension_id=e.id AND policy.library_id=t.library_id \
                AND policy.mode='disabled') AND (e.all_libraries=1 OR EXISTS(SELECT 1 \
                FROM extension_grants g WHERE g.extension_id=e.id AND (g.library_id=t.library_id \
                OR EXISTS(SELECT 1 FROM library_folders granted \
                WHERE granted.library_id=t.library_id AND granted.parent_id=g.library_id \
                AND granted.active=1))))",
        )
        .bind(&link.extension_id)
        .bind(&job.id)
        .bind(job.attempt)
        .bind(&job.node_id)
        .bind(now)
        .bind(&link.generation)
        .fetch_one(&mut **tx)
        .await?
    };
    if valid != 1 {
        return Ok(false);
    }
    if !current_fingerprint_matches(tx, &job.library_id, &job.target_id, link).await? {
        return Ok(false);
    }
    query("INSERT INTO automation_results(extension_id,asset_id,fingerprint,task_id,result_json,updated_at) VALUES(?,?,?,?,?,?) ON CONFLICT(extension_id,asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,task_id=excluded.task_id,result_json=excluded.result_json,updated_at=excluded.updated_at")
 .bind(&link.extension_id).bind(&job.target_id).bind(&link.fingerprint).bind(&job.id).bind(encoded).bind(now).execute(&mut **tx).await?;
    if matches!(
        link.operation.as_str(),
        "intro.parse_chapters" | "intro.detect"
    ) {
        persist_timeline_markers(tx, job, link, result, now).await?;
    }
    if !matches!(
        link.operation.as_str(),
        "intro.parse_chapters" | "intro.detect"
    ) && can_write_catalogue(tx, &link.extension_id).await?
    {
        let is_music: i64 = query_scalar("SELECT COUNT(*) FROM music_tracks WHERE id=?")
            .bind(&job.target_id)
            .fetch_one(&mut **tx)
            .await?;
        if is_music > 0 {
            apply_missing_music_metadata(
                tx,
                &job.target_id,
                &link.extension_id,
                &link.fingerprint,
                result,
                now,
            )
            .await?;
        } else {
            apply_missing_metadata(
                tx,
                &job.target_id,
                &link.extension_id,
                &link.fingerprint,
                result,
                now,
            )
            .await?;
        }
    }
    Ok(true)
}

pub(super) async fn persist_timeline_markers(
    tx: &mut Transaction<'_, Any>,
    job: &Job,
    link: &TaskLink,
    result: &Value,
    now: i64,
) -> Result<(), sqlx::Error> {
    let results: Vec<(String, &Vec<Value>)> = if link.operation == "intro.parse_chapters" {
        vec![(
            job.target_id.clone(),
            result
                .as_array()
                .ok_or_else(|| sqlx::Error::Protocol("invalid marker result".into()))?,
        )]
    } else {
        result
            .as_array()
            .ok_or_else(|| sqlx::Error::Protocol("invalid marker result".into()))?
            .iter()
            .map(|episode| {
                let id = episode["episodeId"]
                    .as_str()
                    .ok_or_else(|| sqlx::Error::Protocol("missing episode id".into()))?;
                let markers = episode["markers"]
                    .as_array()
                    .ok_or_else(|| sqlx::Error::Protocol("missing markers".into()))?;
                Ok((id.to_owned(), markers))
            })
            .collect::<Result<_, sqlx::Error>>()?
    };
    for (asset, markers) in results {
        // The task's fingerprint fence protects its own target only. A peer
        // can change while Wasm runs, so its result must be handled by its own
        // fenced task instead of attributing it to its current revision here.
        if asset != job.target_id {
            continue;
        }
        // Episodes and films alike: the marker belongs to this file revision.
        let state: Option<(String, String)> = query_as("SELECT obs.fingerprint,a.probe_json FROM inventory_assets a JOIN automation_observations obs ON obs.asset_id=a.id WHERE a.id=? AND a.available=1")
            .bind(&asset).fetch_optional(&mut **tx).await?;
        let Some((source_fingerprint, probe)) = state else {
            return Err(sqlx::Error::Protocol("marker source missing".into()));
        };
        let probe = serde_json::from_str::<Value>(&probe).unwrap_or(Value::Null);
        let duration = probe["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| sqlx::Error::Protocol("marker duration missing".into()))?;
        // Which pass may replace or delete which row. No column records the writer: the row's
        // task_id is joined to automation_task_links.operation.
        // - intro.detect reads the chapters as well as the audio, so for this file revision it
        //   has the last word: it replaces any row and deletes the kinds it no longer reports,
        //   except, when the file has more chapters than the detection was sent, the rows the
        //   chapter pass wrote (they may come from a chapter the detection never saw).
        // - intro.parse_chapters replaces and deletes only the rows it wrote itself, or rows of
        //   another file revision or plugin generation (which the player ignores anyway). It never
        //   touches a current acoustic result: a re-read that no detection follows (no peers, a
        //   detection already running) must not leave the episode with a worse segment.
        let chapter_pass = link.operation == "intro.parse_chapters";
        let upsert = if chapter_pass {
            "INSERT INTO automatic_timeline_markers(asset_id,extension_id,marker_type,start_seconds,end_seconds,confidence,source_fingerprint,generation,task_id,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(asset_id,extension_id,marker_type) DO UPDATE \
                    SET start_seconds=excluded.start_seconds,end_seconds=excluded.end_seconds,confidence=excluded.confidence,source_fingerprint=excluded.source_fingerprint,generation=excluded.generation,task_id=excluded.task_id,updated_at=excluded.updated_at \
                    WHERE automatic_timeline_markers.source_fingerprint!=excluded.source_fingerprint \
                    OR automatic_timeline_markers.generation!=excluded.generation \
                    OR NOT EXISTS(SELECT 1 FROM automation_task_links writer \
                    WHERE writer.task_id=automatic_timeline_markers.task_id \
                    AND writer.operation='intro.detect')"
        } else {
            "INSERT INTO automatic_timeline_markers(asset_id,extension_id,marker_type,start_seconds,end_seconds,confidence,source_fingerprint,generation,task_id,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?) ON CONFLICT(asset_id,extension_id,marker_type) DO UPDATE \
                    SET start_seconds=excluded.start_seconds,end_seconds=excluded.end_seconds,confidence=excluded.confidence,source_fingerprint=excluded.source_fingerprint,generation=excluded.generation,task_id=excluded.task_id,updated_at=excluded.updated_at"
        };
        let mut reported = std::collections::HashSet::new();
        for marker in markers {
            let Some(kind) = marker["markerType"]
                .as_str()
                .filter(|kind| matches!(*kind, "intro" | "credits" | "recap" | "preview"))
            else {
                continue;
            };
            let (Some(start), Some(end), Some(confidence)) = (
                marker["start"].as_f64(),
                marker["end"].as_f64(),
                marker["confidence"].as_f64(),
            ) else {
                continue;
            };
            // One malformed segment is dropped; it must not discard the others.
            if !start.is_finite()
                || !end.is_finite()
                || !confidence.is_finite()
                || start < 0.0
                || end <= start
                || end > duration
                || end - start > max_marker_length(kind)
                || !(0.0..=1.0).contains(&confidence)
                || !reported.insert(kind)
            {
                continue;
            }
            query(upsert)
                .bind(&asset)
                .bind(&link.extension_id)
                .bind(kind)
                .bind(start)
                .bind(end)
                .bind(confidence)
                .bind(&source_fingerprint)
                .bind(&link.generation)
                .bind(&job.id)
                .bind(now)
                .execute(&mut **tx)
                .await?;
        }
        // A kind no longer reported (after a settings change or a better analysis) is stale and
        // must not keep reaching the player, within the ownership rule above.
        let protected_writer = if chapter_pass {
            Some("intro.detect")
        } else if probe["chapters"]
            .as_array()
            .is_some_and(|chapters| chapters.len() > crate::extensions::MARKER_CHAPTERS_MAX)
        {
            Some("intro.parse_chapters")
        } else {
            None
        };
        for kind in ["intro", "credits", "recap", "preview"] {
            if reported.contains(kind) {
                continue;
            }
            match protected_writer {
                Some(writer) => query("DELETE \
                        FROM automatic_timeline_markers \
                        WHERE asset_id=? AND extension_id=? AND marker_type=? \
                        AND (source_fingerprint!=? OR generation!=? OR NOT EXISTS(SELECT 1 \
                        FROM automation_task_links writer \
                        WHERE writer.task_id=automatic_timeline_markers.task_id \
                        AND writer.operation=?))")
                    .bind(&asset).bind(&link.extension_id).bind(kind).bind(&source_fingerprint).bind(&link.generation).bind(writer)
                    .execute(&mut **tx).await?,
                None => query("DELETE FROM automatic_timeline_markers WHERE asset_id=? AND extension_id=? AND marker_type=?")
                    .bind(&asset).bind(&link.extension_id).bind(kind)
                    .execute(&mut **tx).await?,
            };
        }
    }
    Ok(())
}

/// Longest automatic or manual segment of a kind, in seconds: a film's credits often run
/// 8 to 15 minutes, any other segment stays under 10.
pub(crate) fn max_marker_length(kind: &str) -> f64 {
    if kind == "credits" { 1200.0 } else { 600.0 }
}
