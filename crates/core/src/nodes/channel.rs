//! Outbound Node control channels, bounded relay uplinks and heartbeats.
use super::NodesState;
use super::channel_owner::{self, CHANNEL_OWNED_ELSEWHERE, ChannelOwnership};
use super::registry::{load_node, refresh_node_in_registry};
use super::transport::{signed_core_response, verify_signed_node_call};
use crate::error;
use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::Method;
use axum::http::StatusCode;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use media_contracts::protocol::HDR_KEY_ID;
use media_contracts::protocol::HDR_NONCE;
use media_contracts::protocol::HDR_SIGNATURE;
use media_contracts::protocol::HDR_TIMESTAMP;
use media_contracts::protocol::HeartbeatPayload;
use media_contracts::protocol::NodeCommandEnvelope;
use media_contracts::protocol::NodeCommandType;
use media_contracts::protocol::NodeStatus;
use media_contracts::protocol::unix_now;
use media_contracts::protocol::verify_internal_request;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::Notify;
use tokio::sync::RwLock;
use tokio::sync::oneshot;

pub struct RelayStreamResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Body,
}

/// Longest time one control poll waits for a command before answering with a ping.
pub(super) const POLL_WAIT: Duration = Duration::from_secs(25);
/// A Node that re-polls immediately leaves a gap of a few milliseconds between two polls;
/// it is still considered connected for this long after its last poll ended.
const POLL_GRACE_MS: i64 = 5_000;
/// Commands queued for one Node. A full queue refuses new commands instead of growing.
const MAILBOX_CAPACITY: usize = 32;

/// The command was refused: the Node is not polling (any more) or its queue is full.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandRefused;

struct MailboxInner {
    queue: std::sync::Mutex<VecDeque<NodeCommandEnvelope>>,
    wake: Notify,
    /// Incremented by every new poll: a poll that finds a newer generation hands over.
    generation: AtomicU64,
    polling: AtomicU32,
    last_poll_end_ms: AtomicI64,
    expired: AtomicU64,
    /// Channel ownership epoch under which this Core serves the Node (0: not acquired yet).
    epoch: AtomicI64,
    /// Set once this Core lost the channel to another Core: the mailbox never delivers again.
    fenced: AtomicBool,
}

/// Durable-for-the-process mailbox of one Node.
///
/// The previous design created a fresh `mpsc` pair per poll, so every command queued behind
/// the first one, and every command accepted between two polls, was dropped together with the
/// receiver. The mailbox outlives polls: a command is either delivered to the next poll, expires,
/// or is refused up front. Delivery stays at-most-once (a response lost on the wire is not
/// replayed): durable work (`durable_tasks`, scans) recovers through its own lease.
#[derive(Clone)]
pub struct NodeChannel(Arc<MailboxInner>);

/// Held for the lifetime of one poll request; dropping it (including on cancellation)
/// marks the end of the poll for liveness purposes.
pub struct PollGuard {
    channel: NodeChannel,
    generation: u64,
}

impl Drop for PollGuard {
    fn drop(&mut self) {
        self.channel
            .0
            .last_poll_end_ms
            .store(now_ms(), Ordering::Release);
        self.channel.0.polling.fetch_sub(1, Ordering::AcqRel);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl NodeChannel {
    pub fn new() -> Self {
        Self(Arc::new(MailboxInner {
            queue: std::sync::Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            generation: AtomicU64::new(0),
            polling: AtomicU32::new(0),
            last_poll_end_ms: AtomicI64::new(0),
            expired: AtomicU64::new(0),
            epoch: AtomicI64::new(0),
            fenced: AtomicBool::new(false),
        }))
    }

    /// True when no poll is in flight and the last one ended more than the grace ago, or when
    /// this Core lost the channel to another Core.
    pub fn is_closed(&self) -> bool {
        if self.0.fenced.load(Ordering::Acquire) {
            return true;
        }
        if self.0.polling.load(Ordering::Acquire) > 0 {
            return false;
        }
        let last = self.0.last_poll_end_ms.load(Ordering::Acquire);
        last == 0 || now_ms().saturating_sub(last) > POLL_GRACE_MS
    }

    pub fn queued(&self) -> usize {
        self.0.queue.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn expired_count(&self) -> u64 {
        self.0.expired.load(Ordering::Relaxed)
    }

    /// Ownership epoch this mailbox delivers under (0 before the first acquisition).
    pub fn epoch(&self) -> i64 {
        self.0.epoch.load(Ordering::Acquire)
    }

    /// Record the epoch of the ownership acquired by the current poll. Commands queued under
    /// another epoch (an earlier ownership of this Core, lost meanwhile) are dropped undelivered:
    /// they were decided without knowing what the other owner did with the Node.
    fn adopt_epoch(&self, epoch: i64) {
        let previous = self.0.epoch.swap(epoch, Ordering::AcqRel);
        if previous != 0 && previous != epoch {
            self.drop_queued();
        }
    }

    fn drop_queued(&self) {
        let dropped = {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            let dropped = queue.len();
            queue.clear();
            dropped
        };
        self.0.expired.fetch_add(dropped as u64, Ordering::Relaxed);
    }

    /// This Core lost the Node's channel: refuse new commands, drop the queued ones (counted as
    /// expired) and end any poll still waiting here with a ping.
    pub fn fence(&self) {
        // Set under the queue lock: a sender that passed `is_closed()` just before cannot push
        // into this mailbox afterwards and be told its command was accepted.
        let dropped = {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            self.0.fenced.store(true, Ordering::Release);
            let dropped = queue.len();
            queue.clear();
            dropped
        };
        self.0.expired.fetch_add(dropped as u64, Ordering::Relaxed);
        self.0.generation.fetch_add(1, Ordering::AcqRel);
        self.0.wake.notify_waiters();
    }

    /// Enqueue without waiting. Acceptance means "buffered for the next poll", not "executed".
    pub fn try_send(&self, command: NodeCommandEnvelope) -> Result<(), CommandRefused> {
        if self.is_closed() {
            return Err(CommandRefused);
        }
        let command = command.with_default_expiry(unix_now());
        {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            // Re-checked under the lock `fence()` takes: refused, never silently dropped.
            if self.0.fenced.load(Ordering::Acquire) || queue.len() >= MAILBOX_CAPACITY {
                return Err(CommandRefused);
            }
            queue.push_back(command);
        }
        self.0.wake.notify_waiters();
        Ok(())
    }

    /// Same contract as [`try_send`]; kept `async` for existing call sites.
    pub async fn send(&self, command: NodeCommandEnvelope) -> Result<(), CommandRefused> {
        self.try_send(command)
    }

    /// Register a poll. A newer poll supersedes any older one still waiting for the same Node
    /// (a reconnecting Node, or a duplicated process), which then answers with a ping.
    pub fn begin_poll(&self) -> PollGuard {
        self.0.polling.fetch_add(1, Ordering::AcqRel);
        let generation = self.0.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.0.wake.notify_waiters();
        PollGuard {
            channel: self.clone(),
            generation,
        }
    }

    fn pop_live(&self, now: i64) -> Option<NodeCommandEnvelope> {
        let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(command) = queue.pop_front() {
            if command.is_expired(now) {
                self.0.expired.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            return Some(command);
        }
        None
    }

    /// Wait up to `wait` for the next live command. `None` means "answer with a ping".
    pub async fn next_command(
        &self,
        guard: &PollGuard,
        wait: Duration,
    ) -> Option<NodeCommandEnvelope> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            // Register interest before inspecting the queue so a concurrent send is never missed.
            let notified = self.0.wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.0.generation.load(Ordering::Acquire) != guard.generation
                || self.0.fenced.load(Ordering::Acquire)
            {
                return None;
            }
            if let Some(command) = self.pop_live(unix_now()) {
                return Some(command);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return None;
            }
        }
    }
}

impl Default for NodeChannel {
    fn default() -> Self {
        Self::new()
    }
}

pub static ACTIVE_CHANNELS: LazyLock<RwLock<HashMap<String, NodeChannel>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The live channel of a Node, if its control poll is currently connected.
pub async fn live_channel(node_id: &str) -> Option<NodeChannel> {
    ACTIVE_CHANNELS
        .read()
        .await
        .get(node_id)
        .filter(|channel| !channel.is_closed())
        .cloned()
}

/// Whether the Node currently holds a control poll open on this Core.
pub async fn channel_is_active(node_id: &str) -> bool {
    live_channel(node_id).await.is_some()
}

pub static PENDING_STREAMS: LazyLock<Mutex<HashMap<String, oneshot::Sender<RelayStreamResponse>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn register_pending_stream(
    pending: &mut HashMap<String, oneshot::Sender<RelayStreamResponse>>,
    id: String,
    sender: oneshot::Sender<RelayStreamResponse>,
) -> bool {
    // A disconnected browser can drop the receiver before Node sends headers.
    // Reap those entries before admitting another request, keeping abandoned
    // metadata requests bounded even when the Node is slow or disconnected.
    pending.retain(|_, sender| !sender.is_closed());
    if pending.len() >= 64 {
        return false;
    }
    pending.insert(id, sender);
    true
}

pub(super) async fn node_channel(
    State(state): State<NodesState>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/core/nodes/channel");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(v) => v,
            Err(res) => return res,
        };
    if rec.status == NodeStatus::Revoked.as_str() {
        return error(StatusCode::FORBIDDEN, "node_revoked");
    }
    // The signature authenticates the Node named in the headers: it may only poll its own
    // channel, never collect the commands (secret rotations included) queued for another Node.
    if id != rec.id {
        return error(StatusCode::FORBIDDEN, "node_id_mismatch");
    }

    let now = unix_now();
    // Acquire or renew this Core's ownership of the channel before serving it (multi-Core).
    let epoch = match channel_owner::acquire(&state.auth.store, &id, now).await {
        Ok(ChannelOwnership::Owned { epoch }) => epoch,
        Ok(ChannelOwnership::OwnedElsewhere { .. }) => {
            fence_local_channel(&id).await;
            // Signed, so the Node can trust the refusal; it reconnects through its Core URL.
            return signed_core_response(
                &rec,
                &nonce,
                StatusCode::CONFLICT,
                &serde_json::json!({ "error": CHANNEL_OWNED_ELSEWHERE }),
            );
        }
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "database_error"),
    };

    let hb: Result<HeartbeatPayload, _> = serde_json::from_slice(&body);
    if let Ok(ref h) = hb {
        let roots_json = serde_json::to_string(&h.roots).unwrap_or_else(|_| rec.roots_json.clone());
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', last_seen_at=?, roots_json=?, version=COALESCE(?, version) WHERE id=? AND status != 'revoked'",
        )
        .bind(now)
        .bind(roots_json)
        .bind(h.version.as_deref())
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await;
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        state
            .auth
            .store
            .node_registry
            .record_heartbeat(&id, h, Some(1), now);
    } else {
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(now)
        .bind(&id)
        .execute(&state.auth.store.pool)
        .await;
        let _ = refresh_node_in_registry(&state.auth.store, &id).await;
        state
            .auth
            .store
            .node_registry
            .record_success(&id, Some(1), now);
    }

    // The mailbox outlives polls: commands accepted between two polls, or queued behind the
    // one delivered now, wait for the next poll instead of vanishing with a dropped receiver.
    let channel = {
        let mut channels = ACTIVE_CHANNELS.write().await;
        // Reap mailboxes of Nodes that stopped polling so the map stays bounded.
        channels.retain(|key, existing| key == &id || !existing.is_closed());
        channels.entry(id.clone()).or_default().clone()
    };
    channel.adopt_epoch(epoch);
    let guard = channel.begin_poll();
    let mut command = channel
        .next_command(&guard, POLL_WAIT)
        .await
        .unwrap_or_else(|| NodeCommandEnvelope {
            command_id: "ping".to_owned(),
            command_type: NodeCommandType::Ping,
            payload: serde_json::Value::Null,
            expires_at: None,
            epoch: None,
        });
    drop(guard);
    // Every delivered command, the ping included, carries the epoch of this ownership: the
    // Node raises its fence on it and refuses whatever a former owner still sends.
    command.epoch = Some(epoch);

    signed_core_response(&rec, &nonce, StatusCode::OK, &command)
}

/// This Core does not own the Node's channel (any more): its mailbox must stop delivering.
async fn fence_local_channel(node_id: &str) {
    if let Some(channel) = ACTIVE_CHANNELS.write().await.remove(node_id) {
        channel.fence();
    }
}

pub(super) async fn receive_stream_uplink(
    State(state): State<NodesState>,
    AxumPath((id, stream_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    body: Body,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/core/nodes/stream-uplink");
    let rec = match load_node(&state.auth.store, &id).await {
        Ok(Some(r)) => r,
        _ => return error(StatusCode::UNAUTHORIZED, "unknown_node"),
    };
    if rec.status == NodeStatus::Revoked.as_str() {
        return error(StatusCode::FORBIDDEN, "node_revoked");
    }

    let digest = media_node::transport::stream_metadata_digest(&headers);
    let req_ts = headers
        .get(HDR_TIMESTAMP)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let req_nonce = match headers.get(HDR_NONCE).and_then(|v| v.to_str().ok()) {
        Some(n) if !n.is_empty() => n,
        _ => return error(StatusCode::UNAUTHORIZED, "missing_nonce"),
    };
    let req_sig = headers
        .get(HDR_SIGNATURE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let req_kid = headers
        .get(HDR_KEY_ID)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if req_kid != rec.key_id {
        return error(StatusCode::UNAUTHORIZED, "invalid_key_id");
    }

    if verify_internal_request(
        &rec.shared_secret,
        "node-to-core",
        Method::POST.as_str(),
        pq,
        &id,
        &rec.key_id,
        req_ts,
        req_nonce,
        &digest,
        req_sig,
        unix_now(),
    )
    .is_err()
    {
        return error(StatusCode::UNAUTHORIZED, "invalid_signature");
    }

    let stream_status = headers
        .get("x-media-stream-status")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u16>().ok())
        .and_then(|s| StatusCode::from_u16(s).ok())
        .unwrap_or(StatusCode::OK);

    let sender = PENDING_STREAMS.lock().await.remove(&stream_id);
    if let Some(tx) = sender {
        let _ = tx.send(RelayStreamResponse {
            status: stream_status,
            headers: headers.clone(),
            body,
        });
        StatusCode::OK.into_response()
    } else {
        error(StatusCode::NOT_FOUND, "stream_not_found")
    }
}

pub(super) async fn receive_node_heartbeat(
    State(state): State<NodesState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let pq = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or("/internal/v1/core/heartbeat");
    let (rec, nonce) =
        match verify_signed_node_call(&state, &headers, &Method::POST, pq, &body).await {
            Ok(ok) => ok,
            Err(res) => return res,
        };
    if let Ok(hb) = serde_json::from_slice::<HeartbeatPayload>(&body) {
        let now = unix_now();
        let roots_json =
            serde_json::to_string(&hb.roots).unwrap_or_else(|_| rec.roots_json.clone());
        let _ = crate::database::query(
            "UPDATE media_nodes SET status='connected', roots_json=?, version=COALESCE(?, version), last_seen_at=? WHERE id=? AND status != 'revoked'",
        )
        .bind(roots_json)
        .bind(hb.version.as_deref())
        .bind(now)
        .bind(&rec.id)
        .execute(&state.auth.store.pool)
        .await;
        let _ = refresh_node_in_registry(&state.auth.store, &rec.id).await;
        state
            .auth
            .store
            .node_registry
            .record_heartbeat(&rec.id, &hb, Some(1), now);
    }
    signed_core_response(
        &rec,
        &nonce,
        StatusCode::OK,
        &serde_json::json!({ "status": "ok" }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use media_contracts::protocol::NodeCommandType;

    fn command(id: &str, kind: NodeCommandType, expires_at: Option<i64>) -> NodeCommandEnvelope {
        NodeCommandEnvelope {
            command_id: id.to_owned(),
            command_type: kind,
            payload: serde_json::Value::Null,
            expires_at,
            epoch: None,
        }
    }

    const SHORT: Duration = Duration::from_millis(80);

    #[tokio::test]
    async fn characterization_of_the_former_design_a_dropped_receiver_loses_the_queue() {
        // Documents why the mailbox exists: with a receiver recreated per poll, `try_send`
        // reported success for commands that were then discarded with that receiver.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<u8>(8);
        assert!(tx.try_send(1).is_ok());
        assert!(tx.try_send(2).is_ok());
        assert_eq!(rx.recv().await, Some(1));
        drop(rx);
        assert!(
            tx.try_send(3).is_err(),
            "the second command was already lost"
        );
    }

    #[tokio::test]
    async fn commands_queued_behind_the_first_are_delivered_by_the_next_polls_in_order() {
        let channel = NodeChannel::new();
        let first_poll = channel.begin_poll();
        for id in ["a", "b", "c"] {
            channel
                .try_send(command(id, NodeCommandType::Scan, None))
                .unwrap();
        }
        let a = channel.next_command(&first_poll, SHORT).await.unwrap();
        assert_eq!(a.command_id, "a");
        drop(first_poll);
        // Next polls: nothing was dropped with the first poll.
        for expected in ["b", "c"] {
            let poll = channel.begin_poll();
            let next = channel.next_command(&poll, SHORT).await.unwrap();
            assert_eq!(next.command_id, expected);
        }
        let poll = channel.begin_poll();
        assert!(channel.next_command(&poll, SHORT).await.is_none());
    }

    #[tokio::test]
    async fn a_command_accepted_between_two_polls_waits_for_the_next_one() {
        let channel = NodeChannel::new();
        drop(channel.begin_poll());
        assert!(
            !channel.is_closed(),
            "the Node just polled: still connected"
        );
        channel
            .try_send(command("gap", NodeCommandType::CancelScan, None))
            .unwrap();
        let poll = channel.begin_poll();
        let got = channel.next_command(&poll, SHORT).await.unwrap();
        assert_eq!(got.command_id, "gap");
    }

    #[tokio::test]
    async fn a_waiting_poll_wakes_up_as_soon_as_a_command_is_sent() {
        let channel = NodeChannel::new();
        let poll = channel.begin_poll();
        let sender = channel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            sender
                .try_send(command("late", NodeCommandType::StreamRelay, None))
                .unwrap();
        });
        let started = std::time::Instant::now();
        let got = channel
            .next_command(&poll, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(got.command_id, "late");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn expired_commands_are_dropped_and_counted_never_delivered() {
        let channel = NodeChannel::new();
        let poll = channel.begin_poll();
        let past = unix_now() - 1;
        channel
            .try_send(command(
                "stale-relay",
                NodeCommandType::StreamRelay,
                Some(past),
            ))
            .unwrap();
        channel
            .try_send(command("stale-scan", NodeCommandType::Scan, Some(past)))
            .unwrap();
        channel
            .try_send(command("live", NodeCommandType::Scan, None))
            .unwrap();
        let got = channel.next_command(&poll, SHORT).await.unwrap();
        assert_eq!(got.command_id, "live");
        assert_eq!(channel.expired_count(), 2);
        // A default deadline was stamped on the command that had none.
        assert!(got.expires_at.is_some_and(|at| at > unix_now()));
    }

    #[tokio::test]
    async fn a_node_that_stopped_polling_refuses_commands_instead_of_queueing_them_forever() {
        let channel = NodeChannel::new();
        // Never polled.
        assert!(channel.is_closed());
        assert_eq!(
            channel.try_send(command("x", NodeCommandType::Scan, None)),
            Err(CommandRefused)
        );
        // Polled long ago.
        drop(channel.begin_poll());
        channel
            .0
            .last_poll_end_ms
            .store(now_ms() - POLL_GRACE_MS - 1_000, Ordering::Release);
        assert!(channel.is_closed());
        assert_eq!(
            channel.try_send(command("y", NodeCommandType::Scan, None)),
            Err(CommandRefused)
        );
        assert_eq!(channel.queued(), 0);
    }

    #[tokio::test]
    async fn the_queue_is_bounded() {
        let channel = NodeChannel::new();
        let _poll = channel.begin_poll();
        for index in 0..MAILBOX_CAPACITY {
            channel
                .try_send(command(&format!("c{index}"), NodeCommandType::Scan, None))
                .unwrap();
        }
        assert_eq!(
            channel.try_send(command("overflow", NodeCommandType::Scan, None)),
            Err(CommandRefused)
        );
        assert_eq!(channel.queued(), MAILBOX_CAPACITY);
    }

    #[tokio::test]
    async fn a_newer_poll_supersedes_an_older_one_for_the_same_node() {
        let channel = NodeChannel::new();
        let old = channel.begin_poll();
        let waiting = {
            let channel = channel.clone();
            tokio::spawn(async move {
                let started = std::time::Instant::now();
                let got = channel.next_command(&old, Duration::from_secs(5)).await;
                (got, started.elapsed())
            })
        };
        tokio::time::sleep(Duration::from_millis(30)).await;
        let current = channel.begin_poll();
        let (superseded, waited) = waiting.await.unwrap();
        assert!(superseded.is_none(), "the old poll answers with a ping");
        assert!(waited < Duration::from_secs(2));
        // Only the newest poll receives work.
        channel
            .try_send(command("only-new", NodeCommandType::Scan, None))
            .unwrap();
        let got = channel.next_command(&current, SHORT).await.unwrap();
        assert_eq!(got.command_id, "only-new");
    }

    #[tokio::test]
    async fn a_core_that_lost_the_channel_closes_its_mailbox() {
        let node = format!("fenced-{}", uuid::Uuid::new_v4().simple());
        let channel = NodeChannel::new();
        channel.adopt_epoch(7);
        let poll = channel.begin_poll();
        ACTIVE_CHANNELS
            .write()
            .await
            .insert(node.clone(), channel.clone());
        // A poll parked on an empty mailbox, as the Node's long poll is.
        let waiting = {
            let channel = channel.clone();
            tokio::spawn(async move {
                let started = std::time::Instant::now();
                (
                    channel.next_command(&poll, Duration::from_secs(5)).await,
                    started.elapsed(),
                )
            })
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!waiting.is_finished(), "the poll is parked before the loss");
        // The renewal failed: another Core owns the Node now. The parked poll must be woken.
        fence_local_channel(&node).await;
        assert!(ACTIVE_CHANNELS.read().await.get(&node).is_none());
        assert!(live_channel(&node).await.is_none());
        let (delivered, waited) = waiting.await.unwrap();
        assert!(delivered.is_none(), "nothing is delivered after the loss");
        assert!(waited < Duration::from_secs(2), "woken, not timed out");
        // A clone still held by an emitter refuses, even while a poll is in flight.
        let _poll = channel.begin_poll();
        assert!(channel.is_closed());
        assert_eq!(
            channel.try_send(command("late", NodeCommandType::CancelScan, None)),
            Err(CommandRefused)
        );
        // Commands queued before the loss are dropped and counted as expired, never delivered.
        let queued = NodeChannel::new();
        let _poll = queued.begin_poll();
        queued
            .try_send(command("queued", NodeCommandType::Scan, None))
            .unwrap();
        queued.fence();
        assert_eq!(queued.queued(), 0);
        assert_eq!(queued.expired_count(), 1);
    }

    #[tokio::test]
    async fn commands_queued_under_a_former_epoch_are_never_delivered_under_a_new_one() {
        let channel = NodeChannel::new();
        assert_eq!(channel.epoch(), 0);
        channel.adopt_epoch(10);
        let poll = channel.begin_poll();
        channel
            .try_send(command("old", NodeCommandType::Scan, None))
            .unwrap();
        // Same ownership renewed: the queue is kept.
        channel.adopt_epoch(10);
        assert_eq!(channel.queued(), 1);
        // This Core lost the channel and won it back meanwhile: decided under the old epoch.
        channel.adopt_epoch(12);
        assert_eq!(channel.epoch(), 12);
        assert_eq!(channel.queued(), 0);
        assert!(channel.next_command(&poll, SHORT).await.is_none());
    }
}
