use super::*;

/// Call inside the transaction that changes the inventory, never after commit.
pub async fn emit(
    tx: &mut Transaction<'_, Any>,
    event: &EventInput<'_>,
    now: i64,
) -> Result<String, sqlx::Error> {
    let payload =
        serde_json::to_string(event.payload).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    if event.key.is_empty()
        || event.key.len() > 256
        || payload.len() > 65536
        || event.asset_id.len() > 128
        || !matches!(
            event.kind,
            "media.added"
                | "media.modified"
                | "media.removed"
                | "analysis.completed"
                | "metadata.missing"
                | "extension.activated"
                | "manual"
                | "scheduled"
        )
    {
        return Err(sqlx::Error::Protocol("invalid automation event".into()));
    }
    let id = hash(&(event.key, event.library_id))?;
    let existing: Option<(String, String, String, String, String,String)> = query_as(
        "SELECT id,kind,library_id,asset_id,fingerprint,payload_json FROM automation_events WHERE event_key=?",
    )
    .bind(event.key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((id, kind, library, asset, fingerprint, old_payload)) = existing {
        if kind != event.kind
            || library != event.library_id
            || asset != event.asset_id
            || fingerprint != event.fingerprint
            || serde_json::from_str::<Value>(&old_payload).ok().as_ref() != Some(event.payload)
        {
            return Err(sqlx::Error::Protocol(
                "automation event key conflict".into(),
            ));
        }
        return Ok(id);
    }
    query("INSERT INTO automation_events(id,event_key,kind,library_id,asset_id,fingerprint,payload_json,created_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(event_key) DO NOTHING")
 .bind(&id).bind(event.key).bind(event.kind).bind(event.library_id).bind(event.asset_id).bind(event.fingerprint).bind(payload).bind(now).execute(&mut **tx).await?;
    Ok(id)
}

/// Music snapshot emission: scans music_tracks for changes and queues media events.
pub async fn emit_music_snapshot(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let mut count = 0;
    let mut cursor = String::new();
    loop {
        let rows: Vec<MusicSnapshotRow> = query_as(
            "SELECT id,size,modified_ns,available,root_snapshot,title,artist,album FROM music_tracks WHERE library_id=? AND id>? ORDER BY id LIMIT 256",
        ).bind(library).bind(&cursor).fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            break;
        }
        for (track_id, size, modified, available, root, title, artist, album) in rows {
            cursor = track_id.clone();
            let fingerprint = hash(&(
                &track_id, size, &modified, available, &root, &title, &artist, &album,
            ))?;
            let old: Option<(String, i64)> = query_as(
                "SELECT fingerprint,revision FROM automation_observations WHERE asset_id=?",
            )
            .bind(&track_id)
            .fetch_optional(&mut **tx)
            .await?;
            if old
                .as_ref()
                .is_some_and(|(previous, _)| previous == &fingerprint)
            {
                continue;
            }
            let revision = old.as_ref().map_or(1, |(_, r)| r.saturating_add(1));
            let kind = if available == 0 {
                "media.removed"
            } else if old.is_none() {
                "media.added"
            } else {
                "media.modified"
            };
            let key = hash(&(&track_id, revision))?;
            emit(
                tx,
                &EventInput {
                    key: &key,
                    kind,
                    library_id: library,
                    asset_id: &track_id,
                    fingerprint: &fingerprint,
                    payload: &serde_json::json!({"revision": revision, "mediaType": "music"}),
                },
                now,
            )
            .await?;
            query("INSERT INTO automation_observations(asset_id,library_id,fingerprint,revision) VALUES(?,?,?,?) ON CONFLICT(asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,revision=excluded.revision")
                .bind(&track_id).bind(library).bind(&fingerprint).bind(revision).execute(&mut **tx).await?;
            count += 1;
        }
    }
    Ok(count)
}

pub async fn emit_snapshot(
    tx: &mut Transaction<'_, Any>,
    library: &str,
    _scan_id: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let mut count = 0;
    let mut cursor = String::new();
    loop {
        let rows: Vec<(String, i64, String, i64,String,String)> = query_as(
        "SELECT a.id,a.size,a.modified_ns,a.available,s.root_snapshot,a.probe_json FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id WHERE a.library_id=? AND a.id>? ORDER BY a.id LIMIT 256",
    ).bind(library).bind(&cursor).fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            break;
        }
        for (asset, size, modified, available, root, probe) in rows {
            cursor = asset.clone();
            let probe_digest = format!("{:x}", Sha256::digest(probe.as_bytes()));
            let fingerprint = hash(&(&asset, size, &modified, available, &root, probe_digest))?;
            let old: Option<(String, i64)> = query_as(
                "SELECT fingerprint,revision FROM automation_observations WHERE asset_id=?",
            )
            .bind(&asset)
            .fetch_optional(&mut **tx)
            .await?;
            if old
                .as_ref()
                .is_some_and(|(previous, _)| previous == &fingerprint)
            {
                continue;
            }
            let revision = old.as_ref().map_or(1, |(_, r)| r.saturating_add(1));
            let kind = if available == 0 {
                "media.removed"
            } else if old.is_none() {
                "media.added"
            } else {
                "media.modified"
            };
            let key = hash(&(&asset, revision))?;
            emit(
                tx,
                &EventInput {
                    key: &key,
                    kind,
                    library_id: library,
                    asset_id: &asset,
                    fingerprint: &fingerprint,
                    payload: &serde_json::json!({"revision":revision}),
                },
                now,
            )
            .await?;
            query("INSERT INTO automation_observations(asset_id,library_id,fingerprint,revision) VALUES(?,?,?,?) ON CONFLICT(asset_id) DO UPDATE SET fingerprint=excluded.fingerprint,revision=excluded.revision")
 .bind(&asset).bind(library).bind(fingerprint).bind(revision).execute(&mut **tx).await?;
            count += 1;
        }
    }
    Ok(count)
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaskLink {
    pub task_id: String,
    pub event_id: String,
    pub extension_id: String,
    pub operation: String,
    pub fingerprint: String,
    pub generation: String,
    pub provider: String,
}

pub async fn task_link(store: &IdentityStore, id: &str) -> Result<Option<TaskLink>, sqlx::Error> {
    query_as("SELECT * FROM automation_task_links WHERE task_id=?")
        .bind(id)
        .fetch_optional(store.pool())
        .await
}

/// Called in the same transaction as extension activation/configuration changes.
pub async fn emit_extension(
    tx: &mut Transaction<'_, Any>,
    extension: &str,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let nonce = uuid::Uuid::new_v4().to_string();
    Ok(query(
        "INSERT INTO automation_sweeps(id,library_id,extension_id,kind,created_at) \
            SELECT ? || ':' || l.id,l.id,e.id,'extension.activated',? \
            FROM extensions e \
            JOIN libraries l ON (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g \
            WHERE g.extension_id=e.id AND (g.library_id=l.id OR EXISTS(SELECT 1 \
            FROM library_folders granted WHERE granted.library_id=l.id \
            AND granted.parent_id=g.library_id AND granted.active=1)))) \
            WHERE e.id=? AND e.enabled=1 ON CONFLICT(id) DO NOTHING",
    )
    .bind(nonce)
    .bind(now)
    .bind(extension)
    .execute(&mut **tx)
    .await?
    .rows_affected())
}
