use super::util::*;
use crate::{
    database::{query, query_as},
    identity::IdentityStore,
};

// ---------------------------------------------------------------------------
// Background cleanup task for expired parties and invites
// ---------------------------------------------------------------------------

/// A party nobody has been connected to for this long is abandoned and ends:
/// the host closed the tab or the player, or everyone left.
pub const PARTY_ABANDONED_SECONDS: i64 = 5 * 60;

/// A party left open for a whole day ends even if a tab is still connected to it.
pub const PARTY_MAX_IDLE_SECONDS: i64 = 86_400;

pub async fn clean_expired_parties(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let ended = end_abandoned_parties(store).await?;

    // Clean expired invites older than 7 days
    let _ = query("DELETE FROM watch_party_invites WHERE expires_at < ?")
        .bind(current_epoch_s() - 604800)
        .execute(&store.pool)
        .await;

    Ok(ended)
}

/// Ends the parties nobody is connected to any more (and the ones open for a day
/// without a playback command), so they stop being listed as in progress.
pub async fn end_abandoned_parties(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let now = current_epoch_s();
    let presence = super::hub::room_presence().await;
    // Before the members of a party could reconnect after a restart, its database
    // activity is not enough to call it abandoned.
    let started = super::hub::hub_started_s();
    let rows: Vec<(String, i64, i64, Option<i64>)> = query_as(
        "SELECT p.id, p.created_at, p.position_updated_at_ms, \
                (SELECT MAX(m.last_seen_at) FROM watch_party_members m WHERE m.party_id = p.id) \
         FROM watch_parties p WHERE p.ended_at IS NULL",
    )
    .fetch_all(&store.pool)
    .await?;

    let mut stale = Vec::new();
    for (id, created_at, position_updated_ms, last_seen) in rows {
        let last_command = position_updated_ms / 1000;
        let abandoned = match presence.get(&id) {
            Some(room) => {
                room.online == 0 && room.last_online_ms / 1000 < now - PARTY_ABANDONED_SECONDS
            }
            None => {
                created_at
                    .max(last_command)
                    .max(last_seen.unwrap_or(0))
                    .max(started)
                    < now - PARTY_ABANDONED_SECONDS
            }
        };
        let too_old = created_at < now - PARTY_MAX_IDLE_SECONDS
            && last_command < now - PARTY_MAX_IDLE_SECONDS;
        if abandoned || too_old {
            stale.push(id);
        }
    }

    // Out of memory first, so nobody can join a party while it ends.
    super::hub::forget_rooms(&stale).await;
    let mut ended = 0;
    for id in stale {
        ended += query(
            "UPDATE watch_parties SET state = 'ended', ended_at = ? WHERE id = ? AND ended_at IS NULL",
        )
        .bind(now)
        .bind(&id)
        .execute(&store.pool)
        .await?
        .rows_affected();
    }
    Ok(ended)
}
