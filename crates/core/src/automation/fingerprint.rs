use super::*;

/// Owner of every audio fingerprint task, whichever path queues it (automatic chapter pass,
/// detection or an administrator's Analyze). Builds before November 2026 also queued them under
/// the first administrator, so every lookup below matches the key under any owner.
pub(crate) const FINGERPRINT_OWNER: &str = "system";

/// Durable key of the audio fingerprint of one file revision.
pub(crate) fn fingerprint_key(asset: &str, source: &str) -> Result<String, sqlx::Error> {
    Ok(format!("fp:{}", hash(&(asset, source, "chromaprint-v1"))?))
}

/// The completed fingerprint task of this file revision, under any owner.
pub(crate) async fn completed_fingerprint(
    tx: &mut Transaction<'_, Any>,
    asset: &str,
    source: &str,
) -> Result<Option<String>, sqlx::Error> {
    query_scalar("SELECT id FROM durable_tasks WHERE idempotency_key=? AND target_id=? AND category='analyze' AND component='fingerprint' AND status='completed' ORDER BY COALESCE(finished_at,0) DESC,id LIMIT 1")
        .bind(fingerprint_key(asset, source)?)
        .bind(asset)
        .fetch_optional(&mut **tx)
        .await
}

/// What `queue_fingerprint` found or did for one file revision.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FingerprintState {
    /// A task was inserted or queued again.
    Queued,
    /// A task is already queued or running (or a cancelled one still holds its lease).
    Pending,
    /// The extraction is done: the id of its task.
    Completed(String),
}

/// One file whose audio fingerprint is wanted.
pub(crate) struct FingerprintRequest<'a> {
    pub library: &'a str,
    pub node: &'a str,
    pub asset: &'a str,
    pub source: &'a str,
    /// Extract again a completed result that has no tail although the file is longer than the
    /// shortest head window (builds before November 2026 often lost the tail on slow storage), or
    /// that was made with the windows of those builds (300 s head, 186 s tail).
    pub reextract_tailless: bool,
}

/// Queue the audio fingerprint of one file revision unless it exists: a completed extraction is
/// reused, a pending one is left alone and a failed or cancelled one is queued again (with its
/// error and backoff cleared). Rows created under another owner by older builds count as well.
pub(crate) async fn queue_fingerprint(
    tx: &mut Transaction<'_, Any>,
    request: &FingerprintRequest<'_>,
    now: i64,
) -> Result<FingerprintState, sqlx::Error> {
    let key = fingerprint_key(request.asset, request.source)?;
    let rows: Vec<(String, String, Option<i64>, i64)> = query_as(
        "SELECT id,status,lease_until,CAST(CASE WHEN result_json LIKE '%\"tailFingerprints\"%' \
            AND result_json LIKE '%\"window\":2%' THEN 1 ELSE 0 END AS BIGINT) \
            FROM durable_tasks \
            WHERE idempotency_key=? AND target_id=? AND category='analyze' \
            AND component='fingerprint' \
            ORDER BY COALESCE(finished_at,0) DESC,updated_at DESC,id",
    )
    .bind(&key)
    .bind(request.asset)
    .fetch_all(&mut **tx)
    .await?;
    if let Some((id, _, _, has_tail)) = rows.iter().find(|row| row.1 == "completed") {
        if !request.reextract_tailless || *has_tail == 1 {
            return Ok(FingerprintState::Completed(id.clone()));
        }
        let probe: Option<String> =
            query_scalar("SELECT probe_json FROM inventory_assets WHERE id=?")
                .bind(request.asset)
                .fetch_optional(&mut **tx)
                .await?;
        let long = probe
            .and_then(|probe| serde_json::from_str::<Value>(&probe).ok())
            .and_then(|probe| {
                probe["format"]["duration"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
            })
            .is_some_and(|duration| duration > media_node::tasks::HEAD_MIN_SECONDS);
        if !long {
            return Ok(FingerprintState::Completed(id.clone()));
        }
        let requeued = query("UPDATE durable_tasks SET status='queued',failure_count=0,error='',finished_at=NULL,available_at=?,updated_at=? WHERE id=? AND status='completed' AND lease_until IS NULL")
            .bind(now).bind(now).bind(id).execute(&mut **tx).await?.rows_affected();
        return Ok(if requeued > 0 {
            FingerprintState::Queued
        } else {
            FingerprintState::Completed(id.clone())
        });
    }
    if rows
        .iter()
        .any(|row| matches!(row.1.as_str(), "queued" | "running" | "paused"))
    {
        return Ok(FingerprintState::Pending);
    }
    if let Some((id, ..)) = rows.iter().find(|row| row.2.is_none()) {
        let requeued = query("UPDATE durable_tasks SET status='queued',failure_count=0,error='',finished_at=NULL,available_at=?,updated_at=? WHERE id=? AND status IN ('error','cancelled') AND lease_until IS NULL")
            .bind(now).bind(now).bind(id).execute(&mut **tx).await?.rows_affected();
        return Ok(if requeued > 0 {
            FingerprintState::Queued
        } else {
            FingerprintState::Pending
        });
    }
    if !rows.is_empty() {
        return Ok(FingerprintState::Pending);
    }
    let inserted = query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,component,priority,created_at,updated_at,available_at) VALUES(?,?,?,?,?,'analyze',?,'fingerprint',20,?,?,?) ON CONFLICT(owner,idempotency_key) DO NOTHING")
        .bind(uuid::Uuid::new_v4().to_string()).bind(request.library).bind(FINGERPRINT_OWNER).bind(&key).bind(request.node).bind(request.asset).bind(now).bind(now).bind(now)
        .execute(&mut **tx).await?.rows_affected();
    Ok(if inserted > 0 {
        FingerprintState::Queued
    } else {
        FingerprintState::Pending
    })
}

/// Durable key of the end credits picture measurement of one file revision.
pub fn credits_scan_key(asset: &str, source: &str) -> Result<String, sqlx::Error> {
    Ok(format!(
        "cs:{}",
        hash(&(asset, source, media_node::end_credits::ALGORITHM))?
    ))
}

/// Queue the picture measurement of the end of a film (black keyframes with lettering, see
/// `media_node::end_credits`) for one file revision unless it exists. A completed or pending
/// one is left alone; a failed or cancelled one is queued again.
pub(crate) async fn queue_credits_scan(
    tx: &mut Transaction<'_, Any>,
    request: &FingerprintRequest<'_>,
    now: i64,
) -> Result<(), sqlx::Error> {
    let key = credits_scan_key(request.asset, request.source)?;
    let rows: Vec<(String, String, Option<i64>)> = query_as("SELECT id,status,lease_until FROM durable_tasks WHERE idempotency_key=? AND target_id=? AND category='analyze' AND component='credits' ORDER BY updated_at DESC,id")
        .bind(&key)
        .bind(request.asset)
        .fetch_all(&mut **tx)
        .await?;
    if rows.iter().any(|row| {
        matches!(
            row.1.as_str(),
            "completed" | "queued" | "running" | "paused"
        )
    }) {
        return Ok(());
    }
    if let Some((id, ..)) = rows.iter().find(|row| row.2.is_none()) {
        query("UPDATE durable_tasks SET status='queued',failure_count=0,error='',finished_at=NULL,available_at=?,updated_at=? WHERE id=? AND status IN ('error','cancelled') AND lease_until IS NULL")
            .bind(now).bind(now).bind(id).execute(&mut **tx).await?;
        return Ok(());
    }
    if rows.is_empty() {
        query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,target_id,component,priority,created_at,updated_at,available_at) VALUES(?,?,?,?,?,'analyze',?,'credits',15,?,?,?) ON CONFLICT(owner,idempotency_key) DO NOTHING")
            .bind(uuid::Uuid::new_v4().to_string()).bind(request.library).bind(FINGERPRINT_OWNER).bind(&key).bind(request.node).bind(request.asset).bind(now).bind(now).bind(now)
            .execute(&mut **tx).await?;
    }
    Ok(())
}

/// Most black runs passed on to the segment plugin (the Node keeps the latest 128).
pub(super) const CREDITS_SCAN_MAX_RUNS: usize = 256;

/// The completed end credits measurement of the current revision of a film's file, bounded and
/// checked, as the segment plugin reads it (`blackFrames`): `None` when there is none.
pub async fn film_credits_scan(
    store: &IdentityStore,
    asset: &str,
) -> Result<Option<Value>, sqlx::Error> {
    let rows: Vec<(String, String, String)> = query_as(
        "SELECT t.result_json,t.idempotency_key,obs.fingerprint \
            FROM durable_tasks t \
            JOIN automation_observations obs ON obs.asset_id=t.target_id \
            WHERE t.target_id=? AND t.category='analyze' AND t.component='credits' \
            AND t.status='completed' \
            ORDER BY COALESCE(t.finished_at,0) DESC,t.id \
            LIMIT 4",
    )
    .bind(asset)
    .fetch_all(store.pool())
    .await?;
    for (raw, key, source) in rows {
        if key != credits_scan_key(asset, &source)? {
            continue;
        }
        return Ok(serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|result| sanitized_credits_scan(&result)));
    }
    Ok(None)
}

/// A Node's measurement reduced to what the plugin reads, or `None` when it is malformed.
pub(crate) fn sanitized_credits_scan(result: &Value) -> Option<Value> {
    if result["algorithm"] != media_node::end_credits::ALGORITHM {
        return None;
    }
    let number = |value: &Value| value.as_f64().filter(|n| n.is_finite() && *n >= 0.0);
    let runs = result["runs"].as_array()?;
    if runs.len() > CREDITS_SCAN_MAX_RUNS {
        return None;
    }
    let mut clean = Vec::with_capacity(runs.len());
    for run in runs {
        let (start, end) = (number(&run["start"])?, number(&run["end"])?);
        let pages = run["pages"].as_u64().filter(|n| *n <= 100_000)?;
        let lettered = run["lettered"].as_u64().filter(|n| *n <= pages)?;
        if end < start {
            return None;
        }
        clean.push(serde_json::json!({"start":start,"end":end,"pages":pages,"lettered":lettered}));
    }
    Some(serde_json::json!({
        "algorithm": media_node::end_credits::ALGORITHM,
        "windowStart": number(&result["windowStart"])?,
        "windowEnd": number(&result["windowEnd"])?,
        "runs": clean,
    }))
}

/// One season peer whose completed fingerprint an acoustic detection can use.
pub(crate) struct FingerprintPeer {
    pub task_id: String,
    pub asset_id: String,
    pub source: String,
    pub work_id: String,
}

/// The season peers of `asset` whose completed fingerprint `intro.detect` can compare it with:
/// the rule `run_automated` applies when it builds the batch (same library, current folder
/// snapshot, and an extraction of the peer's current revision). Admission and the re-offer of
/// earlier episodes use it too, so they never queue a detection that is bound to fail.
pub(crate) async fn usable_fingerprint_peers(
    tx: &mut Transaction<'_, Any>,
    asset: &str,
    library: &str,
) -> Result<Vec<FingerprintPeer>, sqlx::Error> {
    let rows: Vec<(String, String, String, String, String)> = query_as("SELECT fp.id,peer_asset.id,obs.fingerprint,fp.idempotency_key,peer.work_id \
            FROM inventory_assets current_asset \
            JOIN inventory_sources current_source ON current_source.id=current_asset.source_id \
            JOIN inventory_versions current_version ON current_version.id=current_source.version_id \
            JOIN inventory_editions current_edition \
            ON current_edition.id=current_version.edition_id \
            JOIN inventory_episodes current_ep ON current_ep.work_id=current_edition.work_id \
            JOIN inventory_episodes peer ON peer.season_id=current_ep.season_id \
            AND peer.work_id!=current_ep.work_id \
            JOIN inventory_editions peer_edition ON peer_edition.work_id=peer.work_id \
            JOIN inventory_versions peer_version ON peer_version.edition_id=peer_edition.id \
            JOIN inventory_sources peer_source ON peer_source.version_id=peer_version.id \
            JOIN inventory_assets peer_asset ON peer_asset.source_id=peer_source.id \
            JOIN libraries peer_library ON peer_library.id=peer_asset.library_id \
            JOIN automation_observations obs ON obs.asset_id=peer_asset.id \
            JOIN durable_tasks fp ON fp.target_id=peer_asset.id AND fp.category='analyze' \
            AND fp.component='fingerprint' AND fp.status='completed' \
            WHERE current_asset.id=? AND peer_asset.library_id=? AND peer_asset.available=1 \
            AND peer_source.root_snapshot=peer_library.root \
            ORDER BY fp.id \
            LIMIT 256")
        .bind(asset)
        .bind(library)
        .fetch_all(&mut **tx)
        .await?;
    let mut peers = Vec::new();
    for (task_id, asset_id, source, key, work_id) in rows {
        if key == fingerprint_key(&asset_id, &source)? {
            peers.push(FingerprintPeer {
                task_id,
                asset_id,
                source,
                work_id,
            });
        }
    }
    Ok(peers)
}

/// Season peers of `asset` in its library that are available and observed, with their Node:
/// candidates for a fingerprint so that the episode's detection has something to compare with.
pub(crate) async fn season_fingerprint_candidates(
    tx: &mut Transaction<'_, Any>,
    asset: &str,
    library: &str,
    limit: i64,
) -> Result<Vec<(String, String, String, String)>, sqlx::Error> {
    query_as("SELECT DISTINCT peer_asset.id,peer_asset.library_id,peer_library.node_id,obs.fingerprint \
            FROM inventory_assets current_asset \
            JOIN inventory_sources current_source ON current_source.id=current_asset.source_id \
            JOIN inventory_versions current_version ON current_version.id=current_source.version_id \
            JOIN inventory_editions current_edition \
            ON current_edition.id=current_version.edition_id \
            JOIN inventory_episodes current_ep ON current_ep.work_id=current_edition.work_id \
            JOIN inventory_episodes peer ON peer.season_id=current_ep.season_id \
            AND peer.work_id!=current_ep.work_id \
            JOIN inventory_editions peer_edition ON peer_edition.work_id=peer.work_id \
            JOIN inventory_versions peer_version ON peer_version.edition_id=peer_edition.id \
            JOIN inventory_sources peer_source ON peer_source.version_id=peer_version.id \
            JOIN inventory_assets peer_asset ON peer_asset.source_id=peer_source.id \
            JOIN libraries peer_library ON peer_library.id=peer_asset.library_id \
            JOIN media_nodes node ON node.id=peer_library.node_id \
            JOIN automation_observations obs ON obs.asset_id=peer_asset.id \
            WHERE current_asset.id=? AND peer_asset.library_id=? AND peer_asset.available=1 \
            AND peer_source.root_snapshot=peer_library.root AND node.status!='revoked' \
            ORDER BY peer_asset.id \
            LIMIT ?")
        .bind(asset)
        .bind(library)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
}

/// After a film's chapters were read: measure the end of its picture, unless that is done for
/// this revision. When the measurement completes, the chapters are read again with it (see
/// `pump_triggers`), so a film whose chapters say nothing still gets its credits.
pub(super) async fn queue_film_credits_scan(
    tx: &mut Transaction<'_, Any>,
    job: &Job,
    now: i64,
) -> Result<(), sqlx::Error> {
    let film: i64 = query_scalar("SELECT COUNT(*) FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN inventory_versions v ON v.id=s.version_id JOIN inventory_editions e ON e.id=v.edition_id JOIN inventory_works w ON w.id=e.work_id WHERE a.id=? AND w.kind='film'")
        .bind(&job.target_id)
        .fetch_one(&mut **tx)
        .await?;
    if film == 0 {
        return Ok(());
    }
    let intro_enabled = query_scalar::<_, String>("SELECT options_json FROM libraries WHERE id=?")
        .bind(&job.library_id)
        .fetch_optional(&mut **tx)
        .await?
        .and_then(|encoded| {
            serde_json::from_str::<crate::library_options::LibraryOptions>(&encoded).ok()
        })
        .is_none_or(|options| options.intro_detection_enabled);
    let source: Option<String> =
        query_scalar("SELECT fingerprint FROM automation_observations WHERE asset_id=?")
            .bind(&job.target_id)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(source) = source.filter(|_| intro_enabled) {
        queue_credits_scan(
            tx,
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
    Ok(())
}

pub(super) async fn current_fingerprint_matches(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    asset: &str,
    link: &TaskLink,
) -> Result<bool, sqlx::Error> {
    let current: Option<CurrentFingerprintRow> = query_as(
        "SELECT e.package_json,COALESCE(p.config_revision,''),COALESCE(p.algorithm_version,'1'),\
            COALESCE(s.settings_json,'{}'),a.modified_ns,CAST(a.size AS TEXT),CAST(a.available AS \
            TEXT),source.root_snapshot,COALESCE(p.operation,''),lib.options_json,a.probe_json \
            FROM extensions e \
            LEFT \
            JOIN automation_policies p ON p.extension_id=e.id \
            AND p.library_id=COALESCE((SELECT own.library_id FROM automation_policies own \
            WHERE own.extension_id=e.id AND own.library_id=?),(SELECT parent_id \
            FROM library_folders WHERE library_id=? AND active=1 LIMIT 1)) \
            LEFT \
            JOIN extension_settings s ON s.extension_id=e.id \
            JOIN inventory_assets a ON a.id=? \
            JOIN inventory_sources source ON source.id=a.source_id \
            JOIN libraries lib ON lib.id=a.library_id \
            WHERE e.id=? AND source.root_snapshot=lib.root \
            AND COALESCE(p.mode,'automatic')!='disabled'",
    )
    .bind(library)
    .bind(library)
    .bind(asset)
    .bind(&link.extension_id)
    .fetch_optional(&mut **tx)
    .await?;
    let mut music_fingerprint = None;
    let (
        package,
        config,
        algorithm,
        settings,
        modified,
        size,
        available,
        root,
        operation,
        options,
        probe,
    ) = match current {
        Some(c) => c,
        None => {
            let music_curr: Option<MusicCurrentRow> = query_as(
                "SELECT e.package_json,COALESCE(p.config_revision,''),COALESCE(p.algorithm_version,'1'),COALESCE(s.settings_json,'{}'),t.modified_ns,t.size,t.available,t.root_snapshot,COALESCE(p.operation,''),lib.options_json,t.title,t.artist,t.album \
                        FROM extensions e \
                        LEFT \
                        JOIN automation_policies p ON p.extension_id=e.id \
                        AND p.library_id=COALESCE((SELECT own.library_id \
                        FROM automation_policies own WHERE own.extension_id=e.id \
                        AND own.library_id=?),(SELECT parent_id FROM library_folders \
                        WHERE library_id=? AND active=1 LIMIT 1)) \
                        LEFT \
                        JOIN extension_settings s ON s.extension_id=e.id \
                        JOIN music_tracks t ON t.id=? \
                        JOIN libraries lib ON lib.id=t.library_id \
                        WHERE e.id=? AND t.root_snapshot=lib.root \
                        AND COALESCE(p.mode,'automatic')!='disabled'"
            )
            .bind(library).bind(library).bind(asset).bind(&link.extension_id).fetch_optional(&mut **tx).await?;
            let Some((pkg, cfg, alg, sett, modif, sz, avail, rt, op, opt, title, artist, album)) =
                music_curr
            else {
                return Ok(false);
            };
            music_fingerprint = Some(hash(&(
                asset, sz, &modif, avail, &rt, &title, &artist, &album,
            ))?);
            (
                pkg,
                cfg,
                alg,
                sett,
                modif,
                sz.to_string(),
                avail.to_string(),
                rt,
                op,
                opt,
                String::new(),
            )
        }
    };
    let package: media_extension_sdk::Package = serde_json::from_str(&package)
        .map_err(|_| sqlx::Error::Protocol("invalid extension package".into()))?;
    let operation = if operation.is_empty() {
        if package
            .manifest
            .permissions
            .iter()
            .any(|p| p == "playback.markers")
        {
            "intro.parse_chapters"
        } else if package.manifest.category.as_deref() == Some("Lyrics")
            || package.manifest.permissions.iter().any(|p| p == "lyrics")
        {
            "lyrics.get"
        } else if package.manifest.http_hosts.is_empty() {
            "metadata.propose"
        } else {
            "metadata.search"
        }
    } else {
        &operation
    };
    if operation != link.operation
        && !(operation == "intro.parse_chapters" && link.operation == "intro.detect")
    {
        return Ok(false);
    }
    let size = size
        .parse::<i64>()
        .map_err(|_| sqlx::Error::Protocol("invalid source size".into()))?;
    let available = available
        .parse::<i64>()
        .map_err(|_| sqlx::Error::Protocol("invalid source availability".into()))?;
    let file_fingerprint = if let Some(fingerprint) = music_fingerprint {
        fingerprint
    } else {
        let probe_digest = format!("{:x}", Sha256::digest(probe.as_bytes()));
        hash(&(asset, size, &modified, available, &root, probe_digest))?
    };
    let settings: Value = serde_json::from_str(&settings)
        .map_err(|_| sqlx::Error::Protocol("invalid extension settings".into()))?;
    let options: Value = serde_json::from_str(&options)
        .map_err(|_| sqlx::Error::Protocol("invalid library options".into()))?;
    let fingerprint = hash(&(
        asset,
        &file_fingerprint,
        &link.extension_id,
        &link.generation,
        &package.sha256,
        &config,
        &algorithm,
        &settings,
        &link.operation,
        &options,
    ))?;
    Ok(fingerprint == link.fingerprint)
}
