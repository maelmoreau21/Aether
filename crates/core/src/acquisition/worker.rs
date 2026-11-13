use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(crate) struct DispatchGuard {
    store: IdentityStore,
    request_id: String,
    user_id: String,
    library_id: String,
    server_id: String,
    generation: String,
    lease: String,
}
impl DispatchGuard {
    pub async fn verify(&self) -> Result<(), Error> {
        crate::applications::authorize_background(
            &self.store,
            "requests",
            &self.library_id,
            &self.user_id,
        )
        .await?;
        unrestricted(&self.store, &self.user_id).await?;
        let count: i64 = query_scalar(
            "SELECT COUNT(*) \
                FROM acquisition_jobs j \
                JOIN media_requests r ON r.id=j.request_id \
                JOIN acquisition_servers s ON s.id=r.server_id \
                WHERE j.request_id=? AND j.lease_token=? AND j.lease_until>? \
                AND r.status IN ('approved','submitted','acquiring','imported','partial') \
                AND s.id=? AND s.generation=? AND s.enabled=1",
        )
        .bind(&self.request_id)
        .bind(&self.lease)
        .bind(now())
        .bind(&self.server_id)
        .bind(&self.generation)
        .fetch_one(self.store.pool())
        .await
        .map_err(db)?;
        if count != 1 {
            return Err(err(
                StatusCode::CONFLICT,
                "acquisition_lease_or_connection_changed",
            ));
        }
        Ok(())
    }
}
struct Claimed {
    request: RequestRecord,
    lease: String,
    phase: String,
    attempts: i64,
}
async fn claim(store: &IdentityStore) -> Result<Option<Claimed>, Error> {
    let mut conn = store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let next: Option<(String, String, i64)> = query_as(
        "SELECT j.request_id,j.phase,j.attempts \
            FROM acquisition_jobs j \
            JOIN media_requests r ON r.id=j.request_id \
            WHERE j.available_at<=? AND j.lease_until<=? \
            AND r.status IN ('approved','submitted','acquiring','imported','partial') \
            ORDER BY j.available_at,j.request_id \
            LIMIT 1",
    )
    .bind(now())
    .bind(now())
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;
    let Some((id, phase, attempts)) = next else {
        crate::database::commit(tx).await.map_err(db)?;
        return Ok(None);
    };
    let lease = uuid::Uuid::new_v4().to_string();
    query("UPDATE acquisition_jobs SET lease_token=?,lease_until=?,attempts=attempts+1 WHERE request_id=?").bind(&lease).bind(now()+120).bind(&id).execute(&mut *tx).await.map_err(db)?;
    crate::database::commit(tx).await.map_err(db)?;
    Ok(Some(Claimed {
        request: record(store, &id).await?,
        lease,
        phase,
        attempts: attempts + 1,
    }))
}
async fn persist(
    store: &IdentityStore,
    job: &Claimed,
    status: &str,
    code: Option<&str>,
    remote_id: Option<&str>,
    work_id: Option<&str>,
    terminal: bool,
) -> Result<(), Error> {
    let mut conn = store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    let live:i64=query_scalar("SELECT COUNT(*) FROM acquisition_jobs WHERE request_id=? AND lease_token=? AND lease_until>?").bind(&job.request.id).bind(&job.lease).bind(now()).fetch_one(&mut *tx).await.map_err(db)?;
    if live != 1 {
        return Err(err(StatusCode::CONFLICT, "acquisition_lease_lost"));
    }
    let previous: String = query_scalar("SELECT status FROM media_requests WHERE id=?")
        .bind(&job.request.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    query("UPDATE media_requests SET status=?,error_code=?,remote_id=COALESCE(?,remote_id),work_id=COALESCE(?,work_id),updated_at=? WHERE id=?").bind(status).bind(code).bind(remote_id).bind(work_id).bind(now()).bind(&job.request.id).execute(&mut *tx).await.map_err(db)?;
    if previous != status || code.is_some() {
        event(&mut tx, &job.request.id, None, status, code).await?;
    }
    if terminal {
        query("DELETE FROM acquisition_jobs WHERE request_id=? AND lease_token=?")
            .bind(&job.request.id)
            .bind(&job.lease)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    crate::database::commit(tx).await.map_err(db)?;
    Ok(())
}
async fn validate_target(server: &Server, quality_profile_id: i64) -> Result<(), Error> {
    let roots = server.call("GET", "rootfolder", &[], None).await?;
    let profiles = server.call("GET", "qualityprofile", &[], None).await?;
    if !roots.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|v| v["path"].as_str() == Some(server.root_folder.as_str()))
    }) || !profiles.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|v| v["id"].as_i64() == Some(quality_profile_id))
    }) {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "remote_destination_configuration_changed",
        ));
    }
    if server.kind == "lidarr" {
        let profiles = server.call("GET", "metadataprofile", &[], None).await?;
        if !profiles.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|v| v["id"].as_i64() == server.metadata_profile_id)
        }) {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "remote_metadata_profile_missing",
            ));
        }
    }
    Ok(())
}
fn relative_path(root: &str, path: &str) -> Option<String> {
    let root = root.replace('\\', "/");
    let path = path.replace('\\', "/");
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let relative = path.strip_prefix(&prefix)?;
    if relative.is_empty() || relative.split('/').any(|s| s == ".." || s == ".") {
        return None;
    }
    Some(relative.to_owned())
}
pub(super) async fn indexed(
    store: &IdentityStore,
    server: &Server,
    kind: &str,
    paths: &[String],
) -> Result<(usize, Option<String>), Error> {
    let mut found = 0;
    let mut work_id = None;
    for path in paths
        .iter()
        .take(2000)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let Some(path) = relative_path(&server.root_folder, path) else {
            continue;
        };
        if matches!(kind, "artist" | "album" | "track") {
            let count:i64=query_scalar("SELECT COUNT(*) FROM music_tracks t JOIN libraries l ON l.id=t.library_id WHERE t.library_id=? AND t.relative_path=? AND t.available=1 AND t.root_snapshot=l.root").bind(&server.library_id).bind(&path).fetch_one(store.pool()).await.map_err(db)?;
            if count > 0 {
                found += 1;
            }
        } else {
            let work: Option<String> = query_scalar(
                "SELECT e.work_id \
                    FROM inventory_assets a \
                    JOIN inventory_sources s ON s.id=a.source_id \
                    JOIN inventory_versions v ON v.id=s.version_id \
                    JOIN inventory_editions e ON e.id=v.edition_id \
                    JOIN libraries l ON l.id=a.library_id \
                    WHERE a.library_id=? AND a.relative_path=? AND a.available=1 \
                    AND s.root_snapshot=l.root \
                    LIMIT 1",
            )
            .bind(&server.library_id)
            .bind(&path)
            .fetch_optional(store.pool())
            .await
            .map_err(db)?;
            if let Some(work) = work {
                found += 1;
                if work_id.is_none() {
                    work_id = Some(work);
                }
            }
        }
    }
    Ok((found, work_id))
}
async fn run(store: &IdentityStore, job: &Claimed) -> Result<(), Error> {
    let user = job
        .request
        .user_id
        .as_deref()
        .ok_or_else(|| err(StatusCode::FORBIDDEN, "request_author_missing"))?;
    let server_id = job
        .request
        .server_id
        .as_deref()
        .ok_or_else(|| err(StatusCode::CONFLICT, "acquisition_server_not_configured"))?;
    let mut server = server(store, server_id).await?;
    server.guard = Some(DispatchGuard {
        store: store.clone(),
        request_id: job.request.id.clone(),
        user_id: user.into(),
        library_id: job
            .request
            .library_id
            .clone()
            .ok_or_else(|| err(StatusCode::CONFLICT, "request_library_required"))?,
        server_id: server.id.clone(),
        generation: server.generation.clone(),
        lease: job.lease.clone(),
    });
    if let Some(guard) = &server.guard {
        guard.verify().await?;
    }
    let input: RequestInput = serde_json::from_str(&job.request.input_json)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_request_state"))?;
    if !input.valid() || input.service() != server.kind {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request_selection",
        ));
    }
    let single = if input.kind == "track" {
        Some(singles::resolve(&server, &input).await?)
    } else {
        None
    };
    let mut plan_input = json!(input);
    if let Some(single) = &single {
        let proof = single.proof();
        if let (Some(input), Some(proof)) = (plan_input.as_object_mut(), proof.as_object()) {
            input.extend(proof.clone());
        }
    }
    let plan = crate::applications::invoke(store, "requests", "requests.plan", plan_input).await?;
    if let Some(code) = plan["error"].as_str() {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, code));
    }
    if let Some(single) = &single
        && (plan["acquisitionKind"] != "album"
            || plan["acquisitionExternalId"] != single.selection.external_id)
    {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "single_plan_unverified",
        ));
    }
    let effect = single
        .as_ref()
        .map(|single| &single.selection)
        .unwrap_or(&input);
    let id = if let Some(id) = job.request.remote_id.as_deref() {
        remote::verify_identity(&server, effect, id).await?;
        id.to_owned()
    } else {
        validate_target(
            &server,
            effect
                .quality_profile_id
                .unwrap_or(server.quality_profile_id),
        )
        .await?;
        let canonical = if let Some(single) = &single {
            single.canonical.clone()
        } else {
            remote::lookup(&server, effect).await?
        };
        let id = remote::ensure(&server, effect, &canonical).await?;
        persist(store, job, "approved", None, Some(&id), None, false).await?;
        id
    };
    if let Some(single) = &single {
        single.prepare(&server, &id).await?;
    }
    if job.phase == "initial" {
        let already_imported = remote::progress(&server, effect, &id).await?.imported;
        if already_imported {
            remote::monitor_future(&server, &input, &id).await?;
        }
        // Write before sending the search. An uncertain search is explicit, never replayed automatically.
        let changed=query("UPDATE acquisition_jobs SET phase='search_issued' WHERE request_id=? AND lease_token=? AND lease_until>?").bind(&job.request.id).bind(&job.lease).bind(now()).execute(store.pool()).await.map_err(db)?.rows_affected();
        if changed != 1 {
            return Err(err(StatusCode::CONFLICT, "acquisition_lease_lost"));
        }
        if !already_imported && remote::search_missing(&server, effect, &id).await.is_err() {
            return Err(err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "search_submission_unconfirmed",
            ));
        }
        query("UPDATE acquisition_jobs SET phase='monitor' WHERE request_id=? AND lease_token=?")
            .bind(&job.request.id)
            .bind(&job.lease)
            .execute(store.pool())
            .await
            .map_err(db)?;
        persist(store, job, "submitted", None, Some(&id), None, false).await?;
    } else if job.phase == "search_issued" {
        return Err(err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "search_submission_unconfirmed",
        ));
    }
    let progress = remote::progress(&server, effect, &id).await?;
    let (indexed, work_id) = indexed(store, &server, &input.kind, &progress.paths).await?;
    if !progress.paths.is_empty() && indexed < progress.expected {
        // The existing distributed engine selects the owning Node; Core never reads its disk.
        let key = format!(
            "request-scan:{}:{:x}:{}",
            job.request.id,
            Sha256::digest(json!(progress.paths).to_string().as_bytes()),
            now() / 300
        );
        crate::task_engine::enqueue(store, &server.library_id, user, &key, 100, now())
            .await
            .map_err(|_| {
                err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "inventory_scan_unavailable",
                )
            })?;
    }
    let status = if indexed >= progress.expected && progress.imported {
        "available"
    } else if indexed > 0 {
        "partial"
    } else if progress.imported {
        "imported"
    } else {
        "acquiring"
    };
    // Future artist monitoring is an ongoing subscription, not a completed discography request.
    let status = if input.kind == "artist" && input.monitor_future && input.album_ids.is_empty() {
        "submitted"
    } else {
        status
    };
    let progress_json = if status == "available" {
        None
    } else {
        progress
            .download
            .as_ref()
            .and_then(|d| serde_json::to_string(d).ok())
    };
    query("UPDATE media_requests SET progress_json=? WHERE id=?")
        .bind(progress_json)
        .bind(&job.request.id)
        .execute(store.pool())
        .await
        .map_err(db)?;
    persist(
        store,
        job,
        status,
        None,
        Some(&id),
        work_id.as_deref(),
        status == "available",
    )
    .await?;
    Ok(())
}

/// Bounded scheduler pass; leases and pinned destinations survive restarts.
pub async fn tick(store: &IdentityStore) -> Result<(), Error> {
    if !crate::applications::enabled(store, "requests").await? {
        return Ok(());
    }
    for _ in 0..4 {
        let Some(job) = claim(store).await? else {
            break;
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(90), run(store, &job))
            .await
            .unwrap_or_else(|_| Err(err(StatusCode::GATEWAY_TIMEOUT, "acquisition_timeout")));
        match result {
            Ok(()) => {
                query("UPDATE acquisition_jobs SET lease_token=NULL,lease_until=0,attempts=0,available_at=? WHERE request_id=? AND lease_token=?").bind(now()+20).bind(&job.request.id).bind(&job.lease).execute(store.pool()).await.map_err(db)?;
            }
            Err((status, Json(error))) => {
                let code = error["error"].as_str().unwrap_or("acquisition_failed");
                let permanent = status.is_client_error() || job.attempts >= 3;
                if permanent {
                    persist(store, &job, "failed", Some(code), None, None, true).await?;
                } else {
                    // Keep destination and any remote id. The next attempt reconciles before creation.
                    persist(
                        store,
                        &job,
                        &job.request.status,
                        Some(code),
                        None,
                        None,
                        false,
                    )
                    .await?;
                    query("UPDATE acquisition_jobs SET lease_token=NULL,lease_until=0,available_at=? WHERE request_id=? AND lease_token=?").bind(now()+60*job.attempts).bind(&job.request.id).bind(&job.lease).execute(store.pool()).await.map_err(db)?;
                }
            }
        }
    }
    Ok(())
}
