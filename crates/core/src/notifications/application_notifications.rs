//! Explicit opt-ins and live authorization for optional application events.
use super::*;
pub(super) fn kind(event: &str) -> Option<&'static str> {
    if event.starts_with("request.") {
        Some("requests")
    } else if event.starts_with("report.") {
        Some("reports")
    } else {
        None
    }
}
pub(super) async fn allowed(
    store: &IdentityStore,
    channel: &Channel,
    event: &Event,
    role: &str,
    cap: &str,
) -> Result<bool, sqlx::Error> {
    if !crate::applications::enabled(store, cap)
        .await
        .unwrap_or(false)
    {
        return Ok(false);
    }
    let subscribed:i64=query_scalar("SELECT COUNT(*) FROM application_notification_preferences WHERE user_id=? AND kind=? AND enabled=1 AND since_at<=?").bind(&channel.recipient_id).bind(cap).bind(event.occurred_at).fetch_one(store.pool()).await?;
    if subscribed != 1 {
        return Ok(false);
    }
    let sql = if cap == "requests" {
        "SELECT user_id,work_id FROM media_requests WHERE id=? AND imported=0"
    } else {
        "SELECT user_id,work_id FROM media_reports WHERE id=? AND source_key IS NULL"
    };
    let row: Option<(Option<String>, Option<String>)> = query_as(sql)
        .bind(&event.task_id)
        .fetch_optional(store.pool())
        .await?;
    let Some((owner, work)) = row else {
        return Ok(false);
    };
    if role != "administrator" && owner.as_deref() != Some(&channel.recipient_id) {
        return Ok(false);
    }
    let Some(library) = &event.library_id else {
        return Ok(false);
    };
    let id = if cap == "requests" {
        "media-requests"
    } else {
        "media-reports"
    };
    let granted:i64=query_scalar("SELECT COUNT(*) FROM extensions e WHERE e.id=? AND (e.all_libraries=1 OR EXISTS(SELECT 1 FROM extension_grants g WHERE g.extension_id=e.id AND g.library_id=?) OR EXISTS(SELECT 1 FROM extension_grants g JOIN library_folders f ON f.parent_id=g.library_id WHERE g.extension_id=e.id AND f.library_id=? AND f.active=1))").bind(id).bind(library).bind(library).fetch_one(store.pool()).await?;
    if granted != 1
        || !crate::product::access_now(
            store,
            &channel.recipient_id,
            media_contracts::protocol::unix_now(),
        )
        .await
        .unwrap_or(false)
    {
        return Ok(false);
    }
    if let Some(work) = work {
        if !crate::product::parental_allowed(store, &channel.recipient_id, &work)
            .await
            .unwrap_or(false)
        {
            return Ok(false);
        }
    } else {
        let policies: i64 = query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=?")
            .bind(&channel.recipient_id)
            .fetch_one(store.pool())
            .await?;
        if policies != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) async fn collect(store: &IdentityStore, timestamp: i64) -> Result<(), sqlx::Error> {
    for cap in ["requests", "reports"] {
        if !crate::applications::enabled(store, cap)
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let sql = if cap == "requests" {
            "SELECT e.id,e.request_id,r.library_id,e.status,e.created_at FROM acquisition_events e JOIN media_requests r ON r.id=e.request_id WHERE r.imported=0 AND e.created_at>? AND NOT EXISTS(SELECT 1 FROM notification_events n WHERE n.id=e.id) ORDER BY e.created_at,e.id LIMIT 100"
        } else {
            "SELECT e.id,e.report_id,r.library_id,e.kind,e.created_at FROM media_report_events e JOIN media_reports r ON r.id=e.report_id WHERE r.source_key IS NULL AND e.kind!='report.priority' AND e.created_at>? AND NOT EXISTS(SELECT 1 FROM notification_events n WHERE n.id=e.id) ORDER BY e.created_at,e.id LIMIT 100"
        };
        let rows: Vec<(String, String, Option<String>, String, i64)> = query_as(sql)
            .bind(timestamp - LOOKBACK)
            .fetch_all(store.pool())
            .await?;
        for (id, resource, library, status, at) in rows {
            let event = Event {
                id,
                kind: if cap == "requests" {
                    format!("request.{status}")
                } else {
                    status.clone()
                },
                task_id: resource,
                library_id: library,
                occurred_at: at,
            };
            enqueue(store, timestamp, &event, 0, &status).await?;
        }
    }
    Ok(())
}
