//! Remote task/scan dispatch, fenced result ingestion and inventory synchronization.
use super::NodesState;
use super::channel::ACTIVE_CHANNELS;
use super::registry::{load_node, refresh_node_in_registry};
use super::transport::{signed_core_response, verify_signed_node_call};
use crate::error;
use crate::identity::IdentityStore;
use crate::inventory;
use axum::extract::Path as AxumPath;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::Response;
use bytes::Bytes;
use media_contracts::ids::AssetId;
use media_contracts::ids::ImageId;
use media_contracts::ids::LibraryId;
use media_contracts::ids::NodeId;
use media_contracts::ids::ScanId;
use media_contracts::inventory::BatchAck;
use media_contracts::protocol::LibraryInventorySync;
use media_contracts::protocol::NodeCommandEnvelope;
use media_contracts::protocol::NodeCommandType;
use media_contracts::protocol::NodeInventoryAssetEntry;
use media_contracts::protocol::NodeInventoryImageEntry;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::RemoteBatchSubmission;
use media_contracts::protocol::RemoteScanCompleteResponse;
use media_contracts::protocol::RemoteScanCompletion;
use media_contracts::protocol::RemoteScanFailure;
use media_contracts::protocol::RemoteScanStartRequest;
use media_contracts::protocol::unix_now;
use sqlx::Row;
use std::io;

/// Every result must belong to the current physical scan and, for durable tasks,
/// to an unexpired running attempt. Legacy scans also receive a unique fence.
pub(crate) async fn scan_run_authorized(
    store: &IdentityStore,
    library: &str,
    node: &str,
    scan: &str,
) -> bool {
    let valid: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM active_scan_runs r JOIN libraries l ON l.id=r.library_id JOIN media_nodes n ON n.id=l.node_id WHERE r.library_id=? AND r.node_id=? AND r.scan_id=? AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) AND l.node_id=r.node_id AND n.status!='revoked'")
        .bind(library).bind(node).bind(scan).fetch_one(&store.pool).await.unwrap_or(0);
    if valid != 1 {
        return false;
    }
    if let Some((id, attempt)) = scan.split_once(':') {
        let Ok(attempt) = attempt.parse::<i64>() else {
            return false;
        };
        let live:i64=crate::database::query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE id=? AND attempt=? AND node_id=? AND library_id=? AND status='running' AND lease_until>?")
            .bind(id).bind(attempt).bind(node).bind(library).bind(unix_now()).fetch_one(&store.pool).await.unwrap_or(0);
        return live == 1;
    }
    true
}

/// Durable non-scan task results must match their own run fence, the current
/// library owner and an unexpired running attempt.
pub(crate) async fn task_run_authorized(
    store: &IdentityStore,
    library: &str,
    node: &str,
    run: &str,
) -> bool {
    let Some((id, attempt)) = run.split_once(':') else {
        return false;
    };
    let Ok(attempt) = attempt.parse::<i64>() else {
        return false;
    };
    let valid: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM active_task_runs r JOIN libraries l ON l.id=r.library_id JOIN media_nodes n ON n.id=l.node_id JOIN durable_tasks j ON j.id=? AND j.attempt=? AND j.node_id=r.node_id AND j.library_id=r.library_id WHERE r.run_id=? AND r.library_id=? AND r.node_id=? AND r.status='running' AND j.status='running' AND j.lease_until>? AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) AND l.node_id=r.node_id AND n.status!='revoked'")
        .bind(id).bind(attempt).bind(run).bind(library).bind(node).bind(unix_now()).fetch_one(&store.pool).await.unwrap_or(0);
    valid == 1
}

pub(crate) async fn dispatch_remote_task(
    store: &IdentityStore,
    request: &media_contracts::protocol::NodeTaskRequest,
) -> Result<(), String> {
    let node = request.node_id.as_str();
    let rec = load_node(store, node)
        .await
        .map_err(|_| "Node unavailable")?
        .ok_or("Node unavailable")?;
    if rec.status == "revoked" {
        return Err("Node revoked".into());
    }
    if let Some(sender) = ACTIVE_CHANNELS.read().await.get(node).cloned() {
        return sender
            .try_send(NodeCommandEnvelope {
                command_id: format!("task-{}", request.run_id),
                command_type: NodeCommandType::Task,
                payload: serde_json::to_value(request).map_err(|_| "Task encoding failed")?,
            })
            .map_err(|_| "node_busy_playback_priority".into());
    }
    let base = rec.base_url.as_deref().ok_or("Node offline")?;
    let body = serde_json::to_vec(request).map_err(|_| "Task encoding failed")?;
    let response = media_node::server::send_signed_http_request(
        base,
        "core-to-node",
        Method::POST,
        "/internal/v1/node/tasks",
        &request.node_id,
        &rec.key_id,
        &rec.shared_secret,
        None,
        Bytes::from(body),
    )
    .await;
    match response {
        Ok(_) => Ok(()),
        Err(error) if error.to_string().starts_with("HTTP 429 ") => {
            Err("node_busy_playback_priority".into())
        }
        Err(error) if error.to_string().starts_with("HTTP ") => Err("Node task rejected".into()),
        Err(_) => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status!='revoked'",
            )
            .bind(node)
            .execute(&store.pool)
            .await;
            store.node_registry.record_failure(node, true);
            Err("Node task unavailable; waiting for reconnection".into())
        }
    }
}

pub(super) async fn receive_task_complete(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    if body.len() > 70 * 1024 {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "task_result_too_large");
    }
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(v) => v,
            Err(e) => return e,
        };
    let result: media_contracts::protocol::NodeTaskResult = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_task_result"),
    };
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    if result.node_id.as_str() != rec.id
        || result.run_id != scan_id
        || !task_run_authorized(
            &state.auth.store,
            result.library_id.as_str(),
            &rec.id,
            &scan_id,
        )
        .await
    {
        return error(StatusCode::CONFLICT, "stale_task_result");
    }
    let Some((id, attempt)) = scan_id.split_once(':') else {
        return error(StatusCode::BAD_REQUEST, "invalid_task_run");
    };
    let Ok(attempt) = attempt.parse::<i64>() else {
        return error(StatusCode::BAD_REQUEST, "invalid_task_run");
    };
    let job_category: Option<String> =
        crate::database::query_scalar("SELECT category FROM durable_tasks WHERE id=?")
            .bind(id)
            .fetch_optional(&state.auth.store.pool)
            .await
            .ok()
            .flatten();
    if !matches!(
        job_category.as_deref(),
        Some("analyze" | "prepare" | "image" | "trickplay")
    ) {
        return error(StatusCode::CONFLICT, "invalid_task_category");
    }
    if result.success
        && !crate::task_engine::record_result(
            &state.auth.store,
            id,
            attempt,
            &rec.id,
            unix_now(),
            &result.result_json,
        )
        .await
        .unwrap_or(false)
    {
        return error(StatusCode::CONFLICT, "task_result_denied");
    }
    let failure = (!result.success).then(|| result.error.chars().take(1024).collect::<String>());
    if crate::task_execution::set_task_result(&state.auth.store, &scan_id, failure.as_deref())
        .await
        .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "task_result_failed");
    }
    crate::task_engine::WAKE.notify_one();
    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({"accepted":true}),
    )
}

pub(crate) async fn cancel_remote_scan(
    store: &IdentityStore,
    node: &str,
    scan: &str,
) -> Result<(), String> {
    let rec = load_node(store, node)
        .await
        .map_err(|_| "Node unavailable")?
        .ok_or("Node unavailable")?;
    if rec.status == "revoked" {
        return Err("Node revoked".into());
    }
    if let Some(sender) = ACTIVE_CHANNELS.read().await.get(node).cloned() {
        sender
            .try_send(NodeCommandEnvelope {
                command_id: format!("cancel-{scan}"),
                command_type: NodeCommandType::CancelScan,
                payload: serde_json::json!({"scanId":scan}),
            })
            .map_err(|_| "Node channel busy")?;
        // Sending is not an execution acknowledgement: retain capacity until expiry.
        return Err("Cancellation sent; awaiting worker expiry".into());
    }
    let base = rec.base_url.as_deref().ok_or("Node offline")?;
    media_node::server::send_signed_http_request(
        base,
        "core-to-node",
        Method::POST,
        &format!("/internal/v1/node/scans/{scan}/cancel"),
        &NodeId::new(node),
        &rec.key_id,
        &rec.shared_secret,
        None,
        Bytes::new(),
    )
    .await
    .map_err(|_| "Cancel failed")?;
    // Node cooperatively stops at its next scan boundary; fencing already rejects results.
    Ok(())
}

pub(super) async fn receive_music_track(
    State(state): State<NodesState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(value) => value,
            Err(response) => return response,
        };
    let submission: media_node::music::Submission = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_music_track"),
    };
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    if !scan_run_authorized(
        &state.auth.store,
        &submission.library_id,
        &rec.id,
        &submission.scan_id,
    )
    .await
    {
        return error(StatusCode::CONFLICT, "stale_scan_attempt");
    }
    let valid:i64=crate::database::query_scalar("SELECT COUNT(*) FROM libraries l JOIN music_scan_runs run ON run.library_id=l.id WHERE l.id=? AND l.node_id=? AND l.kind='music' AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1)) AND l.root=? AND run.scan_id=?").bind(&submission.library_id).bind(&rec.id).bind(&submission.root_snapshot).bind(&submission.scan_id).fetch_one(&state.auth.store.pool).await.unwrap_or(0);
    let path = std::path::Path::new(&submission.track.relative_path);
    if valid != 1
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || submission.track.size > i64::MAX as u64
        || !submission.track.mime.starts_with("audio/")
    {
        return error(StatusCode::FORBIDDEN, "music_track_denied");
    }
    if crate::music::publish_track(
        &state.auth.store,
        &submission.library_id,
        &submission.root_snapshot,
        &submission.scan_id,
        submission.track,
    )
    .await
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "music_track_failed");
    }
    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({"accepted":true}),
    )
}

pub(super) async fn receive_scan_batch(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let sub: RemoteBatchSubmission = match serde_json::from_slice(&body) {
        Ok(s) => s,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_batch_payload"),
    };
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    if sub.node_id.as_str() != rec.id
        || !scan_run_authorized(
            &state.auth.store,
            sub.library_id.as_str(),
            &rec.id,
            &scan_id,
        )
        .await
    {
        return error(StatusCode::CONFLICT, "stale_scan_attempt");
    }
    if sub.node_id.as_str() != rec.id || sub.batch.scan_id.as_str() != scan_id {
        return error(StatusCode::FORBIDDEN, "scan_identity_mismatch");
    }

    // Verify library belongs to this Node
    let owns_lib: i64 = crate::database::query_scalar(
        "SELECT CAST(EXISTS(SELECT 1 FROM libraries l WHERE l.id=? AND l.node_id=? AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))) AS INTEGER)",
    )
    .bind(sub.library_id.as_str())
    .bind(&rec.id)
    .fetch_one(&state.auth.store.pool)
    .await
    .unwrap_or(0);
    if owns_lib == 0 {
        return error(StatusCode::FORBIDDEN, "library_not_owned_by_node");
    }

    if inventory::stage_batch(
        &state.auth.store,
        &scan_id,
        sub.library_id.as_str(),
        &sub.batch,
    )
    .await
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "stage_batch_failed");
    }

    let ack = BatchAck {
        scan_id: ScanId::new(scan_id),
        sequence: sub.batch.sequence,
    };
    signed_core_response(&rec, &nonce, StatusCode::OK, &ack)
}

pub(super) async fn receive_scan_complete(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let comp: RemoteScanCompletion = match serde_json::from_slice(&body) {
        Ok(c) => c,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_completion_payload"),
    };
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    if comp.node_id.as_str() != rec.id
        || !scan_run_authorized(
            &state.auth.store,
            comp.library_id.as_str(),
            &rec.id,
            &scan_id,
        )
        .await
    {
        return error(StatusCode::CONFLICT, "stale_scan_attempt");
    }
    if comp.node_id.as_str() != rec.id || comp.finished.scan_id.as_str() != scan_id {
        return error(StatusCode::FORBIDDEN, "scan_identity_mismatch");
    }
    let lib_id = comp.library_id.as_str();

    // Update library root to canonical root_snapshot reported by the Node if needed
    let lib_row: Option<(String,)> = crate::database::query_as(
        "SELECT root FROM libraries l WHERE l.id=? AND l.node_id=? AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))",
    )
    .bind(lib_id)
    .bind(&rec.id)
    .fetch_optional(&state.auth.store.pool)
    .await
    .ok()
    .flatten();
    let Some((configured_root,)) = lib_row else {
        let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
        return error(StatusCode::FORBIDDEN, "library_not_owned_by_node");
    };
    if comp.config_revision != format!("{lib_id}:{configured_root}") {
        // A scan started on the previous configuration cannot undo an admin edit.
        let errors =
            serde_json::json!(["Library configuration changed; retry required"]).to_string();
        let _=crate::database::query("UPDATE scan_jobs SET status='error',errors_json=?,errors_count=1 WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)")
            .bind(errors).bind(lib_id).bind(&scan_id).execute(&state.auth.store.pool).await;
        let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
        return error(StatusCode::CONFLICT, "scan_configuration_changed");
    }

    if !comp.root_snapshot.is_empty() {
        let _ = crate::database::query("UPDATE libraries SET root=? WHERE id=? AND node_id=?")
            .bind(&comp.root_snapshot)
            .bind(lib_id)
            .bind(&rec.id)
            .execute(&state.auth.store.pool)
            .await;
    }

    let library_kind: Option<String> =
        crate::database::query_scalar("SELECT kind FROM libraries WHERE id=?")
            .bind(lib_id)
            .fetch_optional(&state.auth.store.pool)
            .await
            .ok()
            .flatten();
    if library_kind.as_deref() == Some("music") {
        if comp.finished.complete {
            let _=crate::database::query("UPDATE music_tracks SET available=0 WHERE library_id=? AND scan_id!=? AND EXISTS(SELECT 1 FROM music_scan_runs WHERE library_id=? AND scan_id=?)").bind(lib_id).bind(&scan_id).bind(lib_id).bind(&scan_id).execute(&state.auth.store.pool).await;
        }
    } else {
        let root_path = std::path::Path::new(&comp.root_snapshot);
        if let Err(err) = inventory::publish_staged_scan(
            &state.auth.store,
            &scan_id,
            lib_id,
            root_path,
            comp.finished.complete,
        )
        .await
        {
            eprintln!("[REMOTE SCAN PUBLISH ERROR in {lib_id}]: {err:?}");
            let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
            let _ = crate::database::query(
            "UPDATE scan_jobs SET status='error', errors_json='[\"Échec de publication du scan distant\"]', errors_count=1 WHERE library_id=?",
        )
        .bind(lib_id)
        .execute(&state.auth.store.pool)
        .await;
            return error(StatusCode::INTERNAL_SERVER_ERROR, "publish_scan_failed");
        }
    }
    let conflicts = inventory::editorial_warnings(&state.auth.store, lib_id)
        .await
        .unwrap_or_default();
    let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;

    let errors_json = serde_json::to_string(&conflicts).unwrap_or_else(|_| "[]".to_owned());
    let _ = crate::database::query(
        "UPDATE scan_jobs SET status=?, discovered=?, probed=?, reused=?, errors_json=?, errors_count=?, warnings_count=0 WHERE library_id=?",
    )
    // Older Nodes do not report interruption: keep their former "incomplete means error".
    .bind(if comp.finished.interrupted.map_or(comp.finished.complete, |interrupted| !interrupted) { "completed" } else { "error" })
    .bind(comp.finished.discovered as i64)
    .bind(comp.finished.probed as i64)
    .bind(comp.finished.reused as i64)
    .bind(errors_json)
    .bind(conflicts.len() as i64)
    .bind(lib_id)
    .execute(&state.auth.store.pool)
    .await;

    let inventory_sync = build_library_inventory_sync(&state.auth.store, lib_id)
        .await
        .unwrap_or_default();
    let resp = RemoteScanCompleteResponse {
        ack: BatchAck {
            scan_id: ScanId::new(scan_id),
            sequence: comp.finished.last_sequence,
        },
        inventory_sync,
    };
    signed_core_response(&rec, &nonce, StatusCode::OK, &resp)
}

pub(super) async fn receive_scan_fail(
    State(state): State<NodesState>,
    AxumPath(scan_id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri.path_and_query().map(|v| v.as_str()).unwrap_or("");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    let fail: RemoteScanFailure = match serde_json::from_slice(&body) {
        Ok(f) => f,
        Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_failure_payload"),
    };
    let _effects = crate::task_engine::EFFECT_GATE.lock().await;
    if fail.node_id.as_str() != rec.id
        || !scan_run_authorized(
            &state.auth.store,
            fail.library_id.as_str(),
            &rec.id,
            &scan_id,
        )
        .await
    {
        return error(StatusCode::CONFLICT, "stale_scan_attempt");
    }
    let _ = inventory::clean_staging(&state.auth.store, &scan_id).await;
    let err_json = serde_json::to_string(&vec![format!("Scan Node échoué: {}", fail.message)])
        .unwrap_or_else(|_| "[\"Scan Node échoué\"]".to_owned());
    let _ = crate::database::query(
        "UPDATE scan_jobs SET status='error', errors_json=?, errors_count=1 WHERE library_id=?",
    )
    .bind(err_json)
    .bind(fail.library_id.as_str())
    .execute(&state.auth.store.pool)
    .await;

    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "recorded" }),
    )
}

pub async fn build_library_inventory_sync(
    store: &IdentityStore,
    library_id: &str,
) -> Result<LibraryInventorySync, sqlx::Error> {
    let root: String = crate::database::query_scalar("SELECT root FROM libraries WHERE id=?")
        .bind(library_id)
        .fetch_optional(&store.pool)
        .await?
        .unwrap_or_default();

    let asset_rows = crate::database::query(
        "SELECT a.id, a.library_id, s.root_snapshot, a.relative_path, a.mime, a.size, a.modified_ns, au.audios_json
         FROM inventory_assets a
         LEFT JOIN inventory_audio au ON au.asset_id=a.id
         JOIN inventory_sources s ON s.id = a.source_id
         WHERE a.library_id=? AND a.available=1 UNION ALL SELECT id,library_id,root_snapshot,relative_path,mime,size,modified_ns,NULL AS audios_json FROM music_tracks WHERE library_id=? AND available=1",
    )
    .bind(library_id)
    .bind(library_id)
    .fetch_all(&store.pool)
    .await?;

    let mut assets = Vec::with_capacity(asset_rows.len());
    for r in asset_rows {
        let audios: Vec<media_contracts::inventory::ExternalAudio> = serde_json::from_str(
            r.get::<Option<String>, _>("audios_json")
                .as_deref()
                .unwrap_or("[]"),
        )
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        for audio in audios {
            assets.push(NodeInventoryAssetEntry {
                asset_id: AssetId::new(crate::inventory::audio_operational_id(
                    &r.get::<String, _>("id"),
                    &audio,
                )),
                library_id: LibraryId::new(r.get::<String, _>("library_id")),
                root_snapshot: r.get("root_snapshot"),
                relative_path: audio.relative_path,
                mime: "audio/ogg".into(),
                size: audio.size,
                modified_ns: audio.modified_ns,
            });
        }
        assets.push(NodeInventoryAssetEntry {
            asset_id: AssetId::new(r.get::<String, _>("id")),
            library_id: LibraryId::new(r.get::<String, _>("library_id")),
            root_snapshot: r.get("root_snapshot"),
            relative_path: r.get("relative_path"),
            mime: r.get("mime"),
            size: r.get::<i64, _>("size") as u64,
            modified_ns: r.get("modified_ns"),
        });
    }

    let image_rows = crate::database::query(
        "SELECT id, library_id, root_snapshot, relative_path, mime, size, modified_ns
         FROM inventory_images
         WHERE library_id=? AND available=1",
    )
    .bind(library_id)
    .fetch_all(&store.pool)
    .await?;

    let mut images = Vec::with_capacity(image_rows.len());
    for r in image_rows {
        images.push(NodeInventoryImageEntry {
            image_id: ImageId::new(r.get::<String, _>("id")),
            library_id: LibraryId::new(r.get::<String, _>("library_id")),
            root_snapshot: r.get("root_snapshot"),
            relative_path: r.get("relative_path"),
            mime: r.get("mime"),
            size: r.get::<i64, _>("size") as u64,
            modified_ns: r.get("modified_ns"),
        });
    }

    Ok(LibraryInventorySync {
        library_id: LibraryId::new(library_id),
        root_snapshot: root,
        assets,
        images,
    })
}

pub async fn sync_all_libraries_for_node(
    store: &IdentityStore,
    node_id: &str,
) -> Result<(), io::Error> {
    let Some(rec) = load_node(store, node_id).await.map_err(io::Error::other)? else {
        return Ok(());
    };
    if rec.status != NodeStatus::Connected.as_str() {
        return Ok(());
    }
    let is_channel_active = ACTIVE_CHANNELS.read().await.contains_key(node_id);
    if !is_channel_active && rec.base_url.is_none() {
        return Ok(());
    }
    let lib_ids: Vec<String> =
        crate::database::query_scalar("SELECT id FROM libraries WHERE node_id=? AND enabled=1")
            .bind(node_id)
            .fetch_all(&store.pool)
            .await
            .map_err(io::Error::other)?;
    for lib_id in lib_ids {
        let sync = build_library_inventory_sync(store, &lib_id)
            .await
            .map_err(io::Error::other)?;
        if sync.assets.is_empty() && sync.images.is_empty() {
            continue;
        }

        if is_channel_active {
            let cmd = NodeCommandEnvelope {
                command_id: format!("sync-{}", sync.library_id),
                command_type: NodeCommandType::SyncInventory,
                payload: serde_json::to_value(&sync).unwrap_or_default(),
            };
            let channels = ACTIVE_CHANNELS.read().await;
            if let Some(sender) = channels.get(node_id) {
                let _ = sender.send(cmd).await;
                continue;
            }
        }

        if let Some(base_url) = rec.base_url.as_deref() {
            let body = serde_json::to_vec(&sync).map_err(io::Error::other)?;
            let _ = media_node::server::send_signed_http_request(
                base_url,
                "core-to-node",
                Method::POST,
                "/internal/v1/node/inventory-sync",
                &NodeId::new(&rec.id),
                &rec.key_id,
                &rec.shared_secret,
                None,
                Bytes::from(body),
            )
            .await;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn dispatch_remote_scan(
    store: &IdentityStore,
    core_origin: &str,
    scan_id: &str,
    library_id: &str,
    node_id: &str,
    kind: &str,
    root: &str,
    force_refresh: bool,
    excluded_patterns: Vec<String>,
) -> Result<(), String> {
    let rec = load_node(store, node_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Node introuvable".to_owned())?;
    if rec.status == NodeStatus::Revoked.as_str() {
        return Err("Ce Node est révoqué".to_owned());
    }

    let core_internal_url = std::env::var("MEDIA_CORE_INTERNAL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| core_origin.to_owned());

    let cached_images: Vec<media_contracts::media::LocalImage> = if force_refresh {
        Vec::new()
    } else {
        inventory::load_image_cache(store, library_id)
            .await
            .unwrap_or_default()
            .into_values()
            .collect()
    };

    let cached_files: Vec<media_contracts::protocol::CachedFileMeta> = if force_refresh {
        Vec::new()
    } else {
        inventory::load_scan_cache(store, library_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(path, cache)| media_contracts::protocol::CachedFileMeta {
                relative_path: path,
                size: cache.size,
                modified_ns: cache.modified_ns,
            })
            .collect()
    };

    if kind == "music" {
        crate::database::query("INSERT INTO music_scan_runs(library_id,scan_id) VALUES(?,?) ON CONFLICT(library_id) DO UPDATE SET scan_id=excluded.scan_id").bind(library_id).bind(scan_id).execute(&store.pool).await.map_err(|e|e.to_string())?;
    }
    let start_req = RemoteScanStartRequest {
        scan_id: ScanId::new(scan_id),
        node_id: NodeId::new(node_id),
        library_id: LibraryId::new(library_id),
        kind: kind.to_owned(),
        root: root.to_owned(),
        config_revision: format!("{library_id}:{root}"),
        core_base_url: core_internal_url,
        batch_size: 32,
        cached_files,
        cached_images,
        excluded_patterns,
    };

    let is_channel_active = ACTIVE_CHANNELS.read().await.contains_key(node_id);
    if is_channel_active {
        let cmd = NodeCommandEnvelope {
            command_id: format!("scan-{scan_id}"),
            command_type: NodeCommandType::Scan,
            payload: serde_json::to_value(&start_req).unwrap_or_default(),
        };
        let sender = ACTIVE_CHANNELS.read().await.get(node_id).cloned();
        let sent = sender.is_some_and(|sender| sender.try_send(cmd).is_ok());
        if sent {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
            )
            .bind(unix_now())
            .bind(node_id)
            .execute(&store.pool)
            .await;
            return Ok(());
        }
    }

    let Some(base_url) = rec.base_url.as_deref() else {
        return Err("Media Node hors ligne (en attente de connexion sortante)".to_owned());
    };

    let body = serde_json::to_vec(&start_req).map_err(|e| e.to_string())?;
    match media_node::server::send_signed_http_request(
        base_url,
        "core-to-node",
        Method::POST,
        "/internal/v1/node/scans",
        &NodeId::new(&rec.id),
        &rec.key_id,
        &rec.shared_secret,
        None,
        Bytes::from(body),
    )
    .await
    {
        Ok(_) => {
            let now = unix_now();
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
            )
            .bind(now)
            .bind(node_id)
            .execute(&store.pool)
            .await;
            let _ = refresh_node_in_registry(store, node_id).await;
            store.node_registry.record_success(node_id, None, now);
            Ok(())
        }
        Err(err) if err.to_string().starts_with("HTTP 429 ") => {
            Err("node_busy_playback_priority".into())
        }
        Err(err) => {
            let _ = crate::database::query(
                "UPDATE media_nodes SET status='disconnected' WHERE id=? AND status != 'revoked'",
            )
            .bind(node_id)
            .execute(&store.pool)
            .await;
            store.node_registry.record_failure(node_id, true);
            let _ = refresh_node_in_registry(store, node_id).await;
            Err(format!("Node injoignable: {err}"))
        }
    }
}
