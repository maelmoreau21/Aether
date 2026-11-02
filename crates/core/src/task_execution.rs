//! Confined executors for inventory-targeted analysis and cache preparation.
use crate::{
    auth::AuthState,
    database::{query, query_as},
    scans::ScanJobs,
    task_engine::{self, Job},
};
use media_contracts::{
    ids::{AssetId, ImageId, LibraryId, NodeId},
    inventory::{ReadGrant, ReadTarget},
    protocol::NodeTaskRequest,
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) async fn dispatch(
    auth: &AuthState,
    root: &Path,
    jobs: &Arc<ScanJobs>,
    job: &Job,
) -> Result<(), sqlx::Error> {
    let run = format!("{}:{}", job.id, job.attempt);
    let row: Option<(String, String, String, i64, String, String)> = if job.category == "image" {
        query_as("SELECT i.root_snapshot,i.relative_path,i.mime,i.size,i.modified_ns,l.node_id FROM inventory_images i JOIN libraries l ON l.id=i.library_id WHERE i.id=? AND i.library_id=? AND i.available=1 AND i.root_snapshot=l.root AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))")
            .bind(&job.target_id).bind(&job.library_id).fetch_optional(auth.store.pool()).await?
    } else {
        query_as("SELECT s.root_snapshot,a.relative_path,a.mime,a.size,a.modified_ns,l.node_id FROM inventory_assets a JOIN inventory_sources s ON s.id=a.source_id JOIN libraries l ON l.id=a.library_id WHERE a.id=? AND a.library_id=? AND a.available=1 AND s.root_snapshot=l.root AND ((l.enabled=1 AND NOT EXISTS(SELECT 1 FROM library_folders own WHERE own.library_id=l.id)) OR EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent ON parent.id=f.parent_id WHERE f.library_id=l.id AND f.active=1 AND parent.enabled=1))")
            .bind(&job.target_id).bind(&job.library_id).fetch_optional(auth.store.pool()).await?
    };
    let Some((snapshot, relative, mime, size, modified, node)) = row else {
        task_engine::finish(
            &auth.store,
            &job.id,
            job.attempt,
            &job.node_id,
            media_contracts::protocol::unix_now(),
            Some("Inventory target unavailable"),
        )
        .await?;
        return Ok(());
    };
    if node != job.node_id || size < 0 {
        return Err(sqlx::Error::Protocol("Task inventory mismatch".into()));
    }
    if node == "local" {
        crate::local_mounts::validate(Path::new(&snapshot), root)
            .await
            .map_err(|_| sqlx::Error::Protocol("Local task root denied".into()))?;
    }
    query("INSERT INTO scan_jobs(library_id,status,errors_count,warnings_count) VALUES(?,'running',0,0) ON CONFLICT(library_id) DO UPDATE SET status='running',errors_json='[]',errors_count=0,warnings_count=0")
        .bind(&job.library_id).execute(auth.store.pool()).await?;
    query("INSERT INTO active_scan_runs(library_id,scan_id,node_id) VALUES(?,?,?) ON CONFLICT(library_id) DO UPDATE SET scan_id=excluded.scan_id,node_id=excluded.node_id")
        .bind(&job.library_id).bind(&run).bind(&node).execute(auth.store.pool()).await?;
    let cancel = Arc::new(AtomicBool::new(false));
    jobs.cancellations
        .lock()
        .map_err(|_| sqlx::Error::Protocol("Task cancellation state unavailable".into()))?
        .insert(run.clone(), cancel.clone());
    let request = NodeTaskRequest {
        run_id: run.clone(),
        node_id: NodeId::new(&node),
        library_id: LibraryId::new(&job.library_id),
        category: job.category.clone(),
        target_id: job.target_id.clone(),
        component: (!job.component.is_empty()).then(|| job.component.clone()),
        width: Some(job.width as u32),
        core_base_url: std::env::var("MEDIA_CORE_INTERNAL_URL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| auth.origin().to_owned()),
    };
    let target = if job.category == "image" {
        ReadTarget::Image {
            id: ImageId::new(&job.target_id),
            expected_size: size as u64,
            expected_modified_ns: modified.clone(),
        }
    } else {
        ReadTarget::Asset(AssetId::new(&job.target_id))
    };
    let grant = ReadGrant {
        target,
        library_id: LibraryId::new(&job.library_id),
        node_id: NodeId::new(&node),
        root_snapshot: snapshot,
        relative_path: relative,
        mime,
    };
    let store = auth.store.clone();
    let jobs_task = jobs.clone();
    let job = job.clone();
    let task = tokio::spawn(async move {
        if node != "local" {
            if let Err(error) = crate::nodes::dispatch_remote_task(&store, &request).await {
                let _ = set_scan_result(&store, &job.library_id, &run, Some(&error)).await;
            }
        } else {
            let outcome=async {
                let _permit=jobs_task.slots.acquire().await.map_err(|_|"Task worker stopped".to_owned())?;
                let future=media_node::tasks::run_task_on_grant(&job.category,&grant,request.component.as_deref(),request.width,&jobs_task.preparations,size as u64,&modified);
                tokio::pin!(future);
                let mut interval=tokio::time::interval(std::time::Duration::from_millis(100));
                loop { tokio::select! {
                    result=&mut future=>return result.map_err(|error| operation_error(&error,&job.category)),
                    _=interval.tick()=> {
                        if cancel.load(Ordering::Acquire)||jobs_task.is_cancelled() { return Err("Task cancelled".to_owned()); }
                        if crate::scans::LOCAL_STREAMS.load(Ordering::Relaxed)>0 { return Err("preempted for playback".to_owned()); }
                    }
                }}
            }.await;
            let _effects = task_engine::EFFECT_GATE.lock().await;
            if crate::nodes::scan_run_authorized(&store, &job.library_id, &node, &run).await {
                let error = match outcome {
                    Ok(result) => match task_engine::record_result(
                        &store,
                        &job.id,
                        job.attempt,
                        &node,
                        media_contracts::protocol::unix_now(),
                        &result,
                    )
                    .await
                    {
                        Ok(true) => None,
                        _ => Some("Task result could not be persisted".to_owned()),
                    },
                    Err(error) => Some(error),
                };
                let _ = set_scan_result(&store, &job.library_id, &run, error.as_deref()).await;
            }
        }
        if let Ok(mut tokens) = jobs_task.cancellations.lock() {
            tokens.remove(&run);
        }
        task_engine::WAKE.notify_one();
    });
    jobs.track_control(task);
    Ok(())
}

pub(crate) async fn set_scan_result(
    store: &crate::identity::IdentityStore,
    library: &str,
    run: &str,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    let errors = error
        .map(|e| serde_json::json!([e]).to_string())
        .unwrap_or_else(|| "[]".into());
    query("UPDATE scan_jobs SET status=?,errors_json=?,errors_count=?,discovered=1,probed=1 WHERE library_id=? AND EXISTS(SELECT 1 FROM active_scan_runs r WHERE r.library_id=scan_jobs.library_id AND r.scan_id=?)")
        .bind(if error.is_some(){"error"}else{"completed"}).bind(errors).bind(i64::from(error.is_some())).bind(library).bind(run).execute(store.pool()).await?;
    Ok(())
}

fn operation_error(error: &std::io::Error, category: &str) -> String {
    if error.to_string() == "source_changed_rescan_required" {
        return "source_changed_rescan_required".into();
    }
    if category == "prepare" {
        return media_node::packaging::preparation_error_code(error).into();
    }
    format!("task_operation_failed:{:?}", error.kind())
}
