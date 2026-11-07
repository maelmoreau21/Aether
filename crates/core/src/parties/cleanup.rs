use super::util::*;
use crate::{database::query, identity::IdentityStore};

// ---------------------------------------------------------------------------
// Background cleanup task for expired parties and invites
// ---------------------------------------------------------------------------

/// Inactivity after which a party nobody is connected to is ended.
pub const PARTY_IDLE_SECONDS: i64 = 6 * 3600;

pub async fn clean_expired_parties(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let now = current_epoch_s();

    // 1. End parties idle for six hours: no playback command since then and nobody
    //    connected. A party being watched (live room) is never ended here.
    let live = super::hub::live_party_ids().await;
    let idle: Vec<String> = crate::database::query_scalar(
        "SELECT id FROM watch_parties WHERE ended_at IS NULL \
         AND created_at < ? AND position_updated_at_ms < ?",
    )
    .bind(now - PARTY_IDLE_SECONDS)
    .bind((now - PARTY_IDLE_SECONDS) * 1000)
    .fetch_all(&store.pool)
    .await?;
    let mut ended = 0;
    let idle: Vec<String> = idle.into_iter().filter(|id| !live.contains(id)).collect();
    super::hub::forget_rooms(&idle).await;
    for id in idle {
        ended += query(
            "UPDATE watch_parties SET state = 'ended', ended_at = ? WHERE id = ? AND ended_at IS NULL",
        )
        .bind(now)
        .bind(&id)
        .execute(&store.pool)
        .await?
        .rows_affected();
    }
    // A party left open for a whole day ends even if a tab is still connected to it.
    let done = query(
        "UPDATE watch_parties SET state = 'ended', ended_at = ? \
         WHERE ended_at IS NULL AND created_at < ? AND position_updated_at_ms < ?",
    )
    .bind(now)
    .bind(now - 86400)
    .bind((now - 86400) * 1000)
    .execute(&store.pool)
    .await?;
    let ended = ended + done.rows_affected();

    // 2. Clean expired invites older than 7 days
    let _ = query("DELETE FROM watch_party_invites WHERE expires_at < ?")
        .bind(now - 604800)
        .execute(&store.pool)
        .await;

    Ok(ended)
}
