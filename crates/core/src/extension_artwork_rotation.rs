//! Bounded administrator-directed global rotation of existing provider posters.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RotateRequest {
    library_id: String,
    #[serde(default)]
    after: Option<String>,
}

/// One work per call, allowing a client to stop between works. No media files
/// are changed. Negative order marks the server-selected provider poster.
pub(super) async fn rotate(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<RotateRequest>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    if input
        .after
        .as_ref()
        .is_some_and(|v| v.len() > 256 || v.chars().any(char::is_control))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_cursor"));
    }
    let exists: i64 = query_scalar(
        "SELECT COUNT(*) FROM libraries WHERE id=? AND kind IN ('films','series') AND enabled=1",
    )
    .bind(&input.library_id)
    .fetch_one(auth.store.pool())
    .await
    .map_err(db)?;
    if exists == 0 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_library"));
    }
    let works: Vec<String> = query_scalar("SELECT id FROM inventory_works WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND id>? ORDER BY id LIMIT 2")
        .bind(&input.library_id).bind(&input.library_id).bind(input.after.as_deref().unwrap_or(""))
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    let Some(work) = works.first() else {
        return Ok(Json(json!({"items":[],"nextCursor":null})));
    };
    crate::feedback::authorized(&auth, &headers, work, false)
        .await
        .map_err(|s| err(s, "work_denied"))?;
    let _guard = MUTATIONS.lock().await;
    // Serialize concurrent requests within the Core and acquire the DB's write
    // lock before examining order, to prevent half-applied selection updates.
    let mut connection = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut connection)
        .await
        .map_err(db)?;
    let manual: i64 = query_scalar("SELECT (SELECT COUNT(*) FROM work_image_entries WHERE work_id=? AND kind='poster' AND provider IS NULL) + (SELECT COUNT(*) FROM work_posters WHERE work_id=?)")
        .bind(work).bind(work).fetch_one(&mut *tx).await.map_err(db)?;
    let mut changed = false;
    if manual == 0 {
        let images: Vec<(String, i64)> = query_as("SELECT e.id,COALESCE(o.position,e.position) FROM work_image_entries e LEFT JOIN work_image_order o ON o.work_id=e.work_id AND o.image_id=e.id WHERE e.work_id=? AND e.kind='poster' AND e.provider IS NOT NULL ORDER BY e.id LIMIT 13")
            .bind(work).fetch_all(&mut *tx).await.map_err(db)?;
        if images.len() >= 2 && images.len() <= 12 {
            let current = images.iter().position(|(_, position)| *position < 0);
            let next = current.map_or(0, |i| (i + 1) % images.len());
            query("DELETE FROM work_image_order WHERE work_id=? AND position<0 AND image_id IN (SELECT id FROM work_image_entries WHERE work_id=? AND kind='poster' AND provider IS NOT NULL)")
                .bind(work).bind(work).execute(&mut *tx).await.map_err(db)?;
            query("INSERT INTO work_image_order(work_id,image_id,position) VALUES(?,?,-1) ON CONFLICT(work_id,image_id) DO UPDATE SET position=-1")
                .bind(work).bind(&images[next].0).execute(&mut *tx).await.map_err(db)?;
            changed = true;
        }
    }
    tx.commit().await.map_err(db)?;
    Ok(Json(
        json!({"items":[{"workId":work,"changed":changed}],"nextCursor":if works.len()>1 {Some(work)} else {None}}),
    ))
}
