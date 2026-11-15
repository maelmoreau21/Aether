//! Durable command outbox of the Media Nodes (multi-Core, roadmap §2.2).
//!
//! Any Core emits a control command for a Node by writing a `node_commands` row ([`enqueue`]);
//! only the Core that owns the Node's channel (`node_channel_owner`) delivers it, from the Node's
//! control poll ([`claim_next`]), stamped with the ownership epoch. The Node acknowledges the
//! commands it handled in its next poll ([`acknowledge`]) and ignores a command id it already
//! handled. A delivered command that is not acknowledged within [`ACK_TIMEOUT_SECONDS`] (or that
//! was delivered under an older epoch) is delivered again when it is redeliverable, and expires
//! otherwise ([`maintain_node`]).
//!
//! Relay requests stay in the in-memory mailbox of the owning Core: they meet a browser request
//! parked on that same Core (`PENDING_STREAMS`, roadmap §2.4).
//!
//! States: `queued` → `delivered` → `acked`; `queued`/`delivered` → `expired` (deadline passed,
//! or no acknowledgement and no redelivery allowed); `cancelled` (superseded, Node revoked).
use super::channel::POLL_WAIT;
use crate::database::{query, query_as, query_scalar};
use crate::identity::IdentityStore;
use media_contracts::protocol::{
    MAX_ACKED_COMMAND_IDS, MAX_COMMAND_ID_LEN, NodeCommandEnvelope, NodeCommandType, unix_now,
};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

/// A delivered command is acknowledged by the Node's next poll, which starts as soon as the
/// previous one answered; past one full poll (`POLL_WAIT`) and a margin, it is considered lost.
pub const ACK_TIMEOUT_SECONDS: i64 = POLL_WAIT.as_secs() as i64 + 5;
/// While a poll is parked, how often the owning Core looks for commands written by another Core
/// (PostgreSQL only).
pub(crate) const PROBE_INTERVAL: Duration = Duration::from_secs(1);
/// Queued commands per Node beyond which emission is refused (back-pressure, like the bounded
/// in-memory mailbox before). Checked before the insertion: approximate under concurrency.
pub const MAX_QUEUED_PER_NODE: i64 = 256;
/// Finished rows (acknowledged, expired, cancelled) are deleted after a day.
const PRUNE_AFTER_SECONDS: i64 = 86_400;
/// A Node polling another Core is reachable while its last poll started this recently.
const REACHABLE_SINCE_SECONDS: i64 = POLL_WAIT.as_secs() as i64 + 5;

/// Emission refused.
#[derive(Debug)]
pub enum EnqueueError {
    /// [`MAX_QUEUED_PER_NODE`] commands already wait for this Node.
    Full,
    Database(sqlx::Error),
}

impl std::fmt::Display for EnqueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => f.write_str("node_outbox_full"),
            Self::Database(err) => write!(f, "node_outbox_database_error: {err}"),
        }
    }
}

impl From<sqlx::Error> for EnqueueError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

/// Whether a command of this kind may be delivered again when its acknowledgement is missing.
///
/// Every durable kind is: a scan or a task is keyed by its `id:attempt` (the Node refuses one
/// already running, the Core fences batches and results by attempt), a cancellation, a secret
/// rotation to the same key or a revocation can be applied twice, and an inventory sync is a
/// full snapshot of one library (an older one is superseded at emission, see [`enqueue`]). The
/// Node's command-id memory removes the duplicates first. Relay requests and pings never enter
/// the outbox. A Node that does not acknowledge (older release) is never redelivered to.
pub fn redeliverable(kind: NodeCommandType) -> bool {
    match kind {
        NodeCommandType::Scan
        | NodeCommandType::Task
        | NodeCommandType::CancelScan
        | NodeCommandType::SyncInventory
        | NodeCommandType::RotateSecret
        | NodeCommandType::Revoke => true,
        NodeCommandType::Ping | NodeCommandType::StreamRelay => false,
    }
}

fn kind_name(kind: NodeCommandType) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn parse_kind(name: &str) -> Option<NodeCommandType> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).ok()
}

/// Emission order: microseconds, strictly increasing within this process.
fn next_seq() -> i64 {
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0);
    let mut last = LAST.load(Ordering::Acquire);
    loop {
        let next = now.max(last + 1);
        match LAST.compare_exchange_weak(last, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return next,
            Err(current) => last = current,
        }
    }
}

/// `LIKE` pattern matching exactly the ids [`enqueue`] derives from `prefix`: the prefix, a dash
/// and the 32 hexadecimal digits of the random suffix (one `_` each), so that the ids of a
/// longer prefix (`sync-a-b` for `sync-a`) never match.
fn id_prefix_pattern(prefix: &str) -> String {
    let mut pattern = String::with_capacity(prefix.len() + 40);
    for c in prefix.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('-');
    pattern.push_str(&"_".repeat(32));
    pattern
}

/// Write a durable command for `node_id`, from any Core. Returns the id sent to the Node.
///
/// The id is the envelope's `command_id` (truncated) followed by a random suffix: two emissions
/// of the same logical command (an inventory sync of one library after each reconnection) are
/// two commands, which the Node's duplicate memory must not merge. The default deadline of the
/// command type applies when the envelope has none. An inventory sync cancels the older syncs
/// of the same library still waiting or unacknowledged: a redelivered older snapshot would
/// otherwise overwrite a newer one.
pub async fn enqueue(
    store: &IdentityStore,
    node_id: &str,
    envelope: NodeCommandEnvelope,
) -> Result<String, EnqueueError> {
    enqueue_at(store, node_id, envelope, unix_now()).await
}

pub(crate) async fn enqueue_at(
    store: &IdentityStore,
    node_id: &str,
    envelope: NodeCommandEnvelope,
    now: i64,
) -> Result<String, EnqueueError> {
    let envelope = envelope.with_default_expiry(now);
    let queued: i64 = query_scalar(
        "SELECT COUNT(*) FROM node_commands WHERE node_id=? AND state='queued' AND expires_at>?",
    )
    .bind(node_id)
    .bind(now)
    .fetch_one(store.pool())
    .await?;
    if queued >= MAX_QUEUED_PER_NODE {
        return Err(EnqueueError::Full);
    }
    let prefix: String = envelope
        .command_id
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(MAX_COMMAND_ID_LEN - 33)
        .collect();
    let id = format!("{prefix}-{}", uuid::Uuid::new_v4().simple());
    let kind = kind_name(envelope.command_type);
    if envelope.command_type == NodeCommandType::SyncInventory {
        query(
            "UPDATE node_commands SET state='cancelled',payload_json='null' WHERE node_id=? \
                AND command_type=? AND state IN ('queued','delivered') AND id LIKE ? ESCAPE '\\'",
        )
        .bind(node_id)
        .bind(&kind)
        .bind(id_prefix_pattern(&prefix))
        .execute(store.pool())
        .await?;
    }
    if envelope.command_type == NodeCommandType::CancelScan
        && let Some(scan) = envelope.payload.get("scanId").and_then(|v| v.as_str())
    {
        // The scan or task this cancels must never be (re)delivered after it: the Node forgets
        // a cancellation of an id it does not run, so a redelivered start would run anyway.
        query(
            "UPDATE node_commands SET state='cancelled',payload_json='null' WHERE node_id=? \
                AND command_type IN (?,?) AND state IN ('queued','delivered') \
                AND (id LIKE ? ESCAPE '\\' OR id LIKE ? ESCAPE '\\')",
        )
        .bind(node_id)
        .bind(kind_name(NodeCommandType::Scan))
        .bind(kind_name(NodeCommandType::Task))
        .bind(id_prefix_pattern(&format!("scan-{scan}")))
        .bind(id_prefix_pattern(&format!("task-{scan}")))
        .execute(store.pool())
        .await?;
    }
    let payload = serde_json::to_string(&envelope.payload).unwrap_or_else(|_| "null".to_owned());
    query(
        "INSERT INTO node_commands(id,node_id,command_type,payload_json,state,attempts,\
            idempotent,expires_at,created_at,seq,origin_core) VALUES(?,?,?,?,'queued',0,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(node_id)
    .bind(&kind)
    .bind(payload)
    .bind(i64::from(redeliverable(envelope.command_type)))
    .bind(envelope.expires_at.unwrap_or(now))
    .bind(now)
    .bind(next_seq())
    .bind(store.instance_id())
    .execute(store.pool())
    .await?;
    // Same-process emission: the Node's poll parked here reads it now, not at the next probe.
    super::channel::signal_outbox(node_id).await;
    Ok(id)
}

/// Expire and redeliver the commands of `node_id`, once per control poll of its owner at
/// `epoch`: a queued command past its deadline expires; a delivered command without
/// acknowledgement for [`ACK_TIMEOUT_SECONDS`], or delivered under an older epoch (by a former
/// owner, which the Node refuses), is queued again while redeliverable and live, else expires.
pub(crate) async fn maintain_node(
    store: &IdentityStore,
    node_id: &str,
    epoch: i64,
    now: i64,
) -> Result<u64, sqlx::Error> {
    let result = query(
        "UPDATE node_commands SET \
            state=CASE WHEN state='delivered' AND idempotent=1 AND expires_at>? \
            THEN 'queued' ELSE 'expired' END,\
            payload_json=CASE WHEN state='delivered' AND idempotent=1 AND expires_at>? \
            THEN payload_json ELSE 'null' END \
            WHERE node_id=? AND ((state='queued' AND expires_at<=?) OR (state='delivered' \
            AND (delivered_at<=? OR epoch<?)))",
    )
    .bind(now)
    .bind(now)
    .bind(node_id)
    .bind(now)
    .bind(now.saturating_sub(ACK_TIMEOUT_SECONDS))
    .bind(epoch)
    .execute(store.pool())
    .await?;
    Ok(result.rows_affected())
}

/// Whether a live command waits for `node_id` (the cheap probe of a parked poll).
pub(crate) async fn has_queued(
    store: &IdentityStore,
    node_id: &str,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let found: Option<String> = query_scalar(
        "SELECT id FROM node_commands WHERE node_id=? AND state='queued' AND expires_at>? LIMIT 1",
    )
    .bind(node_id)
    .bind(now)
    .fetch_optional(store.pool())
    .await?;
    Ok(found.is_some())
}

/// Take the oldest live queued command of `node_id` for delivery by this Core at `epoch`.
///
/// The `queued` → `delivered` transition is one conditional `UPDATE` that also requires this
/// Core to own the channel at `epoch` (`node_channel_owner`): two Cores never deliver the same
/// command, and a Core that lost the channel since its poll began delivers nothing. A Node that
/// does not acknowledge (`acks_supported` false) gets the command at most once: its row is
/// marked non-redeliverable.
pub(crate) async fn claim_next(
    store: &IdentityStore,
    node_id: &str,
    epoch: i64,
    acks_supported: bool,
    now: i64,
) -> Result<Option<NodeCommandEnvelope>, sqlx::Error> {
    // A few rounds absorb a row taken by a concurrent poll; contention is a takeover race.
    for _ in 0..4 {
        let row: Option<(String, String, String, i64)> = query_as(
            "SELECT id,command_type,payload_json,expires_at FROM node_commands \
                WHERE node_id=? AND state='queued' AND expires_at>? ORDER BY seq LIMIT 1",
        )
        .bind(node_id)
        .bind(now)
        .fetch_optional(store.pool())
        .await?;
        let Some((id, kind, payload, expires_at)) = row else {
            return Ok(None);
        };
        let (Some(command_type), Ok(payload)) = (
            parse_kind(&kind),
            serde_json::from_str::<serde_json::Value>(&payload),
        ) else {
            // Written by a newer Core with a type this one does not know: never delivered here.
            query("UPDATE node_commands SET state='expired',payload_json='null' WHERE id=? AND state='queued'")
                .bind(&id)
                .execute(store.pool())
                .await?;
            continue;
        };
        // A secret rotation is delivered only while its key is the Node's pending key: one
        // superseded (newer rotation, re-pairing, direct rotation) would switch the Node to a
        // key the Core no longer accepts.
        let rotation_key = if command_type == NodeCommandType::RotateSecret {
            payload
                .get("newKeyId")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .to_owned()
        } else {
            String::new()
        };
        let claim = || {
            query(
                "UPDATE node_commands SET state='delivered',epoch=?,delivered_at=?,\
                attempts=attempts+1,idempotent=CASE WHEN ?=1 THEN idempotent ELSE 0 END \
                WHERE id=? AND node_id=? AND state='queued' AND EXISTS(SELECT 1 FROM \
                node_channel_owner o WHERE o.node_id=? AND o.core_id=? AND o.epoch=?) \
                AND (?='' OR EXISTS(SELECT 1 FROM media_nodes n WHERE n.id=? \
                AND n.pending_key_id=?))",
            )
            .bind(epoch)
            .bind(now)
            .bind(i64::from(acks_supported))
            .bind(&id)
            .bind(node_id)
            .bind(node_id)
            .bind(store.instance_id())
            .bind(epoch)
            .bind(&rotation_key)
            .bind(node_id)
            .bind(&rotation_key)
        };
        let claimed = if rotation_key.is_empty() {
            claim().execute(store.pool()).await?.rows_affected()
        } else {
            // Serialized with [`stage_rotation_key`]: a newer rotation can never replace the
            // pending key between this check and the delivery (write skew on PostgreSQL).
            let mut conn = store.pool().acquire().await?;
            let mut tx =
                crate::database::begin_write_scoped(&mut conn, ROTATION_SCOPE, node_id).await?;
            let claimed = claim().execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
            claimed
        };
        if claimed == 0 && !rotation_key.is_empty() {
            let cancelled = query(
                "UPDATE node_commands SET state='cancelled',payload_json='null' WHERE id=? \
                    AND state='queued' AND NOT EXISTS(SELECT 1 FROM media_nodes n \
                    WHERE n.id=? AND n.pending_key_id=?)",
            )
            .bind(&id)
            .bind(node_id)
            .bind(&rotation_key)
            .execute(store.pool())
            .await?
            .rows_affected();
            if cancelled == 1 {
                continue;
            }
        }
        if claimed == 1 {
            return Ok(Some(NodeCommandEnvelope {
                command_id: id,
                command_type,
                payload,
                expires_at: Some(expires_at),
                epoch: Some(epoch),
            }));
        }
        let owner: Option<(String, i64)> =
            query_as("SELECT core_id,epoch FROM node_channel_owner WHERE node_id=?")
                .bind(node_id)
                .fetch_optional(store.pool())
                .await?;
        if owner.is_none_or(|(core, current)| core != store.instance_id() || current != epoch) {
            return Ok(None);
        }
    }
    Ok(None)
}

/// Record the acknowledgements a Node sent in its poll. Only that Node's commands change
/// (ids are bounded in number and length); unknown ids are ignored. A late acknowledgement
/// of a command already queued again or expired still closes it.
pub(crate) async fn acknowledge(
    store: &IdentityStore,
    node_id: &str,
    command_ids: &[String],
    now: i64,
) -> Result<u64, sqlx::Error> {
    let ids: Vec<&str> = command_ids
        .iter()
        .map(String::as_str)
        .filter(|id| !id.is_empty() && id.len() <= MAX_COMMAND_ID_LEN)
        .take(MAX_ACKED_COMMAND_IDS)
        .collect();
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "UPDATE node_commands SET state='acked',acked_at=?,payload_json='null' WHERE node_id=? \
            AND state IN ('queued','delivered','expired') AND id IN ({placeholders})"
    );
    let mut statement = query(sqlx::AssertSqlSafe(sql)).bind(now).bind(node_id);
    for id in ids {
        statement = statement.bind(id);
    }
    Ok(statement.execute(store.pool()).await?.rows_affected())
}

/// Cancel the commands still waiting for `node_id`, except `keep` (a revocation cancels what
/// can no longer be delivered after it).
pub(crate) async fn cancel_pending(
    store: &IdentityStore,
    node_id: &str,
    keep: &str,
) -> Result<u64, sqlx::Error> {
    Ok(query(
        "UPDATE node_commands SET state='cancelled',payload_json='null' WHERE node_id=? \
            AND state='queued' AND id<>?",
    )
    .bind(node_id)
    .bind(keep)
    .execute(store.pool())
    .await?
    .rows_affected())
}

/// Prepare a new secret rotation of `node_id` (two-phase rotation): the rotation commands not
/// delivered yet are cancelled, the new rotation replaces them. Returns `false`, cancelling
/// nothing else, while a rotation command was delivered and is not settled (acknowledged,
/// expired or cancelled): the Node may already sign with its key, which must stay the pending
/// one until the Node uses it, or until the command expires.
pub(crate) async fn supersede_rotation(
    store: &IdentityStore,
    node_id: &str,
) -> Result<bool, sqlx::Error> {
    let kind = kind_name(NodeCommandType::RotateSecret);
    query(
        "UPDATE node_commands SET state='cancelled',payload_json='null' WHERE node_id=? \
            AND command_type=? AND state='queued' AND attempts=0",
    )
    .bind(node_id)
    .bind(&kind)
    .execute(store.pool())
    .await?;
    let in_flight: i64 = query_scalar(
        "SELECT COUNT(*) FROM node_commands WHERE node_id=? AND command_type=? \
            AND state IN ('queued','delivered')",
    )
    .bind(node_id)
    .bind(&kind)
    .fetch_one(store.pool())
    .await?;
    Ok(in_flight == 0)
}

/// Advisory-lock scope serializing the staging of a pending key with the delivery of rotations.
const ROTATION_SCOPE: &str = "node-rotation";

/// SQL condition (no bind) true when the pending key of the `media_nodes` row in scope was
/// delivered at least once: the Node may already sign with it.
const PENDING_KEY_DELIVERED: &str = "EXISTS(SELECT 1 FROM node_commands c \
    WHERE c.node_id=media_nodes.id AND c.command_type='rotate_secret' AND c.attempts>0 \
    AND c.id LIKE 'rotate-' || media_nodes.pending_key_id || '-%')";

/// Record `key_id` as the pending key of `node_id` (two-phase rotation), atomically with the
/// check that the current pending key was never delivered: once delivered, the Node may hold
/// it (applied, then offline before its first signed request), so replacing it would lock the
/// Node out. It stays pending until promoted by use, or cleared by pairing again or revoking.
/// `false`: refused (delivered pending key, revoked or unknown Node).
pub(crate) async fn stage_rotation_key(
    store: &IdentityStore,
    node_id: &str,
    key_id: &str,
    secret: &str,
    now: i64,
) -> Result<bool, sqlx::Error> {
    let mut conn = store.pool().acquire().await?;
    let mut tx = crate::database::begin_write_scoped(&mut conn, ROTATION_SCOPE, node_id).await?;
    let staged = query(sqlx::AssertSqlSafe(format!(
        "UPDATE media_nodes SET pending_key_id=?,pending_secret=?,pending_since=? \
            WHERE id=? AND status<>'revoked' AND (pending_key_id IS NULL \
            OR NOT {PENDING_KEY_DELIVERED})"
    )))
    .bind(key_id)
    .bind(secret)
    .bind(now)
    .bind(node_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(staged == 1)
}

/// Periodic pass of every Core (scheduler worker): expiry and timed-out redelivery for Nodes that
/// stopped polling, then removal of finished rows older than a day.
pub(crate) async fn maintain(store: &IdentityStore, now: i64) -> Result<(), sqlx::Error> {
    query(
        "UPDATE node_commands SET \
            state=CASE WHEN state='delivered' AND idempotent=1 AND expires_at>? \
            THEN 'queued' ELSE 'expired' END,\
            payload_json=CASE WHEN state='delivered' AND idempotent=1 AND expires_at>? \
            THEN payload_json ELSE 'null' END \
            WHERE (state='queued' AND expires_at<=?) OR (state='delivered' AND delivered_at<=?)",
    )
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now.saturating_sub(ACK_TIMEOUT_SECONDS))
    .execute(store.pool())
    .await?;
    query(
        "DELETE FROM node_commands WHERE created_at<? \
            AND state IN ('acked','expired','cancelled') AND NOT (attempts>0 \
            AND command_type='rotate_secret' AND EXISTS(SELECT 1 FROM media_nodes n \
            WHERE n.id=node_commands.node_id AND node_commands.id LIKE \
            'rotate-' || n.pending_key_id || '-%'))",
    )
    .bind(now.saturating_sub(PRUNE_AFTER_SECONDS))
    .execute(store.pool())
    .await?;
    Ok(())
}

/// Whether a durable command for `node_id` can be delivered soon: the Node polls this Core
/// (its in-memory view, authoritative for the Nodes it serves), or another Core holds a valid
/// lease on its channel and the Node polled recently (`media_nodes.last_seen_at`).
///
/// Emitters that refused to queue for an unconnected Node keep doing so with this check instead
/// of requiring a channel on this very Core.
pub(crate) async fn node_reachable(store: &IdentityStore, node_id: &str) -> bool {
    if super::channel::channel_is_active(node_id).await {
        return true;
    }
    let now = unix_now();
    query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM node_channel_owner o JOIN media_nodes n ON n.id=o.node_id \
            WHERE o.node_id=? AND o.core_id<>? AND o.lease_until>? AND n.last_seen_at>=? \
            AND n.status<>'revoked'",
    )
    .bind(node_id)
    .bind(store.instance_id())
    .bind(now)
    .bind(now.saturating_sub(REACHABLE_SINCE_SECONDS))
    .fetch_one(store.pool())
    .await
    .is_ok_and(|count| count > 0)
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
