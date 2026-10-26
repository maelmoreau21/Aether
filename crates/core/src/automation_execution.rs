//! Leased automation work runs in disposable Core Wasm processes. Physical media
//! work remains on the owning Node through the existing task executor.
use crate::{
    auth::AuthState,
    scans::ScanJobs,
    task_engine::{self, Job},
};
use std::sync::Arc;

pub(crate) async fn dispatch(
    auth: &AuthState,
    jobs: &Arc<ScanJobs>,
    job: &Job,
) -> Result<bool, sqlx::Error> {
    let Some(link) = crate::automation::task_link(&auth.store, &job.id).await? else {
        return Ok(false);
    };
    let Some(permit) = crate::extension_runtime::try_acquire_execution(
        &link.extension_id,
        crate::extension_runtime::ExecutionClass::Background,
    ) else {
        // Saturation is backpressure, not an extension failure; no future waits for a worker.
        task_engine::finish(
            &auth.store,
            &job.id,
            job.attempt,
            &job.node_id,
            media_contracts::protocol::unix_now(),
            Some("playback_preempted"),
        )
        .await?;
        return Ok(true);
    };
    let auth = auth.clone();
    let job = job.clone();
    let jobs_task = jobs.clone();
    jobs.track_control(tokio::spawn(async move {
        let _permit = permit;
        let result = {
        let operation = crate::extensions::run_automated(&auth, &job, &link.extension_id, &link.generation, &link.operation);
        tokio::pin!(operation);
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(200));
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(50));
        tokio::pin!(deadline);
        loop { tokio::select! {
            _ = &mut deadline => break Err("extension_timeout".into()),
            result = &mut operation => break result.map_err(|(_,body)| body.0["error"].as_str().unwrap_or("extension_failed").to_owned()),
            _ = tick.tick() => {
                if jobs_task.is_cancelled() || crate::scans::LOCAL_STREAMS.load(std::sync::atomic::Ordering::Relaxed)>0 || auth.store.node_registry.get(&job.node_id).is_some_and(|node| node.telemetry.reported_active_streams>0 || node.telemetry.core_active_relays>0) {
                    break Err("playback_preempted".into());
                }
                match task_engine::get(&auth.store, &job.id).await {
                    Ok(Some(current)) if current.status=="running" && current.attempt==job.attempt && current.lease_until.is_some_and(|t|t>media_contracts::protocol::unix_now()) => {},
                    _ => break Err("automation_cancelled".into()),
                }
            }
        }}
        };
        // Dropping the invocation kills its child before releasing admission.
        let now = media_contracts::protocol::unix_now();
        let outcome = match result {
            Ok(value) => crate::automation::complete(&auth.store, &job, &link, &value, now).await.and_then(|accepted| if accepted { Ok(()) } else { Err(sqlx::Error::Protocol("stale result".into())) }).map_err(|_|"automation_stale_or_persistence_failed".to_owned()),
            Err(error) => Err(error),
        };
        if let Err(error) = outcome
            && let Err(failure) = task_engine::finish(&auth.store,&job.id,job.attempt,&job.node_id,now,Some(&error)).await {
            eprintln!("automation task could not persist failure: {failure}");
        }
        task_engine::WAKE.notify_one();
    }));
    Ok(true)
}
