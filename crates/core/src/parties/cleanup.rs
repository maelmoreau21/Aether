use super::util::*;
use crate::{database::query, identity::IdentityStore};

// ---------------------------------------------------------------------------
// Background cleanup task for expired parties and invites
// ---------------------------------------------------------------------------

pub async fn clean_expired_parties(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let now = current_epoch_s();

    // 1. Mark parties ended if older than 24 hours without activity
    let done = query(
        "UPDATE watch_parties SET state = 'ended', ended_at = ? \
         WHERE ended_at IS NULL AND created_at < ?",
    )
    .bind(now)
    .bind(now - 86400)
    .execute(&store.pool)
    .await?;
    let ended = done.rows_affected();

    // 2. Clean expired invites older than 7 days
    let _ = query("DELETE FROM watch_party_invites WHERE expires_at < ?")
        .bind(now - 604800)
        .execute(&store.pool)
        .await;

    Ok(ended)
}
