use super::*;
use crate::core_instances::{HEARTBEAT_STALE_SECONDS, deregister, heartbeat};
use crate::nodes::channel_owner::{ChannelOwnership, acquire};
use std::collections::HashMap;

/// Far in the future of the real clock (the epoch of a first acquisition is `now`).
const NOW: i64 = 4_000_000_000;

async fn add_node(store: &IdentityStore, id: &str) {
    query("INSERT INTO media_nodes(id,name,kind,status) VALUES(?,?,'remote','connected')")
        .bind(id)
        .bind(id)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn store_with_node() -> (tempfile::TempDir, IdentityStore, i64) {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    add_node(&store, "nas").await;
    let ChannelOwnership::Owned { epoch } = acquire(&store, "nas", NOW).await.unwrap() else {
        panic!("a single Core owns the channel")
    };
    (temp, store, epoch)
}

fn command(id: &str, kind: NodeCommandType, expires_at: Option<i64>) -> NodeCommandEnvelope {
    NodeCommandEnvelope {
        command_id: id.to_owned(),
        command_type: kind,
        payload: serde_json::json!({ "id": id }),
        expires_at,
        epoch: None,
    }
}

/// `(state, attempts, epoch, idempotent, payload_json)` of the command `id`.
async fn row(store: &IdentityStore, id: &str) -> (String, i64, Option<i64>, i64, String) {
    query_as("SELECT state,attempts,epoch,idempotent,payload_json FROM node_commands WHERE id=?")
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

async fn state(store: &IdentityStore, id: &str) -> String {
    row(store, id).await.0
}

#[tokio::test]
async fn commands_are_delivered_oldest_first_under_the_owner_epoch() {
    let (_temp, store, epoch) = store_with_node().await;
    let mut ids = Vec::new();
    for (name, kind) in [
        ("scan-1", NodeCommandType::Scan),
        ("cancel-1", NodeCommandType::CancelScan),
        ("task-1", NodeCommandType::Task),
    ] {
        let id = enqueue_at(&store, "nas", command(name, kind, None), NOW)
            .await
            .unwrap();
        assert!(id.starts_with(&format!("{name}-")), "{id}");
        assert!(id.len() <= MAX_COMMAND_ID_LEN);
        ids.push(id);
    }
    for (index, id) in ids.iter().enumerate() {
        let delivered = claim_next(&store, "nas", epoch, true, NOW + 1)
            .await
            .unwrap()
            .expect("queued command");
        assert_eq!(&delivered.command_id, id, "delivery {index} out of order");
        assert_eq!(delivered.epoch, Some(epoch));
        // The command type's default deadline was stamped at emission.
        assert_eq!(
            delivered.expires_at,
            Some(NOW + delivered.command_type.default_ttl_secs())
        );
        assert_eq!(
            delivered.payload["id"],
            id.rsplit_once('-').unwrap().0,
            "payload preserved"
        );
        let (state, attempts, stamped, idempotent, _) = row(&store, id).await;
        assert_eq!(
            (state.as_str(), attempts, stamped, idempotent),
            ("delivered", 1, Some(epoch), 1)
        );
    }
    assert!(
        claim_next(&store, "nas", epoch, true, NOW + 1)
            .await
            .unwrap()
            .is_none()
    );
    let origin: String = query_scalar("SELECT origin_core FROM node_commands LIMIT 1")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(origin, store.instance_id());
    store.close().await;
}

#[tokio::test]
async fn only_the_owner_at_its_current_epoch_delivers() {
    let (_temp, store, epoch) = store_with_node().await;
    let id = enqueue_at(
        &store,
        "nas",
        command("scan", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    // A poll that acquired an older epoch (ownership changed since) delivers nothing.
    assert!(
        claim_next(&store, "nas", epoch - 1, true, NOW)
            .await
            .unwrap()
            .is_none()
    );
    // Another Core owns the channel: this one delivers nothing, the command stays queued.
    query("UPDATE node_channel_owner SET core_id='other' WHERE node_id='nas'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        claim_next(&store, "nas", epoch, true, NOW)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(state(&store, &id).await, "queued");
    store.close().await;
}

#[tokio::test]
async fn acknowledgements_close_the_commands_of_that_node_only() {
    let (_temp, store, epoch) = store_with_node().await;
    add_node(&store, "other").await;
    let mine = enqueue_at(
        &store,
        "nas",
        command("sync-a", NodeCommandType::SyncInventory, None),
        NOW,
    )
    .await
    .unwrap();
    let theirs = enqueue_at(
        &store,
        "other",
        command("scan", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    claim_next(&store, "nas", epoch, true, NOW).await.unwrap();
    // A Node cannot acknowledge another Node's command, nor send oversized ids.
    let foreign = vec![
        theirs.clone(),
        "x".repeat(MAX_COMMAND_ID_LEN + 1),
        String::new(),
    ];
    assert_eq!(acknowledge(&store, "nas", &foreign, NOW).await.unwrap(), 0);
    assert_eq!(state(&store, &theirs).await, "queued");
    // Its own acknowledgement closes the command and drops the payload.
    assert_eq!(
        acknowledge(&store, "nas", &[mine.clone(), "unknown".into()], NOW + 1)
            .await
            .unwrap(),
        1
    );
    let (state_now, attempts, _, _, payload) = row(&store, &mine).await;
    assert_eq!(
        (state_now.as_str(), attempts, payload.as_str()),
        ("acked", 1, "null")
    );
    // A repeated acknowledgement (the poll's response was lost) changes nothing.
    assert_eq!(
        acknowledge(&store, "nas", std::slice::from_ref(&mine), NOW + 2)
            .await
            .unwrap(),
        0
    );
    // The bound applies.
    let many: Vec<String> = (0..MAX_ACKED_COMMAND_IDS + 50)
        .map(|i| format!("id-{i}"))
        .collect();
    assert_eq!(acknowledge(&store, "nas", &many, NOW).await.unwrap(), 0);
    store.close().await;
}

#[tokio::test]
async fn unacknowledged_commands_are_redelivered_only_when_allowed() {
    let (_temp, store, epoch) = store_with_node().await;
    let long = Some(NOW + 3_600);
    let id = enqueue_at(
        &store,
        "nas",
        command("scan", NodeCommandType::Scan, long),
        NOW,
    )
    .await
    .unwrap();
    claim_next(&store, "nas", epoch, true, NOW)
        .await
        .unwrap()
        .unwrap();
    // Within the acknowledgement timeout nothing moves.
    assert_eq!(
        maintain_node(&store, "nas", epoch, NOW + ACK_TIMEOUT_SECONDS - 1)
            .await
            .unwrap(),
        0
    );
    assert_eq!(state(&store, &id).await, "delivered");
    // Past it, a redeliverable command is queued again and delivered a second time.
    maintain_node(&store, "nas", epoch, NOW + ACK_TIMEOUT_SECONDS)
        .await
        .unwrap();
    assert_eq!(state(&store, &id).await, "queued");
    let again = claim_next(&store, "nas", epoch, true, NOW + ACK_TIMEOUT_SECONDS)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.command_id, id);
    assert_eq!(row(&store, &id).await.1, 2, "second attempt");

    // A Node that does not acknowledge (older release) gets it at most once: then it expires.
    let old = enqueue_at(
        &store,
        "nas",
        command("task", NodeCommandType::Task, long),
        NOW,
    )
    .await
    .unwrap();
    claim_next(&store, "nas", epoch, false, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row(&store, &old).await.3,
        0,
        "no redelivery for this delivery"
    );
    maintain_node(&store, "nas", epoch, NOW + ACK_TIMEOUT_SECONDS)
        .await
        .unwrap();
    let (state_now, _, _, _, payload) = row(&store, &old).await;
    assert_eq!((state_now.as_str(), payload.as_str()), ("expired", "null"));

    // Delivered under an older epoch (a former owner, refused by the Node): queued again at
    // the new owner's first poll, without waiting for the timeout.
    let fenced = enqueue_at(
        &store,
        "nas",
        command("cancel", NodeCommandType::CancelScan, long),
        NOW,
    )
    .await
    .unwrap();
    let ChannelOwnership::Owned { epoch: first } = acquire(&store, "nas", NOW).await.unwrap()
    else {
        panic!()
    };
    claim_next(&store, "nas", first, true, NOW)
        .await
        .unwrap()
        .unwrap();
    query("UPDATE node_channel_owner SET epoch=? WHERE node_id='nas'")
        .bind(first + 1)
        .execute(store.pool())
        .await
        .unwrap();
    maintain_node(&store, "nas", first + 1, NOW + 1)
        .await
        .unwrap();
    assert_eq!(state(&store, &fenced).await, "queued");
    let redelivered = claim_next(&store, "nas", first + 1, true, NOW + 1)
        .await
        .unwrap()
        .unwrap();
    // `id` was acknowledged in neither case; it is the oldest: delivered again first.
    assert_eq!(redelivered.command_id, id);
    assert_eq!(redelivered.epoch, Some(first + 1));
    store.close().await;
}

#[tokio::test]
async fn a_command_past_its_deadline_is_never_delivered() {
    let (_temp, store, epoch) = store_with_node().await;
    let id = enqueue_at(
        &store,
        "nas",
        command("scan", NodeCommandType::Scan, Some(NOW + 1)),
        NOW,
    )
    .await
    .unwrap();
    assert!(has_queued(&store, "nas", NOW).await.unwrap());
    assert!(!has_queued(&store, "nas", NOW + 1).await.unwrap());
    assert!(
        claim_next(&store, "nas", epoch, true, NOW + 1)
            .await
            .unwrap()
            .is_none()
    );
    maintain_node(&store, "nas", epoch, NOW + 1).await.unwrap();
    let (state_now, attempts, _, _, payload) = row(&store, &id).await;
    assert_eq!(
        (state_now.as_str(), attempts, payload.as_str()),
        ("expired", 0, "null")
    );
    // A delivered command whose deadline passed before its acknowledgement is not redelivered.
    let late = enqueue_at(
        &store,
        "nas",
        command("sync", NodeCommandType::SyncInventory, Some(NOW + 10)),
        NOW,
    )
    .await
    .unwrap();
    claim_next(&store, "nas", epoch, true, NOW)
        .await
        .unwrap()
        .unwrap();
    maintain_node(&store, "nas", epoch, NOW + ACK_TIMEOUT_SECONDS)
        .await
        .unwrap();
    assert_eq!(state(&store, &late).await, "expired");
    store.close().await;
}

#[tokio::test]
async fn the_outbox_is_bounded_per_node() {
    let (_temp, store, _) = store_with_node().await;
    add_node(&store, "other").await;
    for index in 0..MAX_QUEUED_PER_NODE {
        enqueue_at(
            &store,
            "nas",
            command(&format!("c{index}"), NodeCommandType::Scan, None),
            NOW,
        )
        .await
        .unwrap();
    }
    assert!(matches!(
        enqueue_at(
            &store,
            "nas",
            command("over", NodeCommandType::Scan, None),
            NOW
        )
        .await,
        Err(EnqueueError::Full)
    ));
    // Other Nodes are not affected; expired commands do not count.
    enqueue_at(
        &store,
        "other",
        command("free", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    enqueue_at(
        &store,
        "nas",
        command("later", NodeCommandType::Scan, None),
        NOW + 3_600,
    )
    .await
    .unwrap();
    store.close().await;
}

#[tokio::test]
async fn a_cancellation_voids_the_unacknowledged_start_it_cancels() {
    let (_temp, store, epoch) = store_with_node().await;
    let cancel = |scan: &str| NodeCommandEnvelope {
        command_id: format!("cancel-{scan}"),
        command_type: NodeCommandType::CancelScan,
        payload: serde_json::json!({ "scanId": scan }),
        expires_at: None,
        epoch: None,
    };
    // Delivered, but the response was lost: without the cancellation it would be redelivered
    // after the acknowledgement timeout and start after its own cancellation.
    let lost = enqueue_at(
        &store,
        "nas",
        command("scan-x:1", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    claim_next(&store, "nas", epoch, true, NOW)
        .await
        .unwrap()
        .unwrap();
    let task = enqueue_at(
        &store,
        "nas",
        command("task-x:1", NodeCommandType::Task, None),
        NOW,
    )
    .await
    .unwrap();
    let other = enqueue_at(
        &store,
        "nas",
        command("scan-x:10", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    let cancelled = enqueue_at(&store, "nas", cancel("x:1"), NOW).await.unwrap();
    assert_eq!(state(&store, &lost).await, "cancelled");
    assert_eq!(state(&store, &task).await, "cancelled");
    assert_eq!(state(&store, &other).await, "queued", "another scan id");
    maintain_node(&store, "nas", epoch, NOW + ACK_TIMEOUT_SECONDS + 1)
        .await
        .unwrap();
    // Only the remaining scan and the cancellation itself are delivered.
    let mut delivered = Vec::new();
    while let Some(command) = claim_next(&store, "nas", epoch, true, NOW + ACK_TIMEOUT_SECONDS + 1)
        .await
        .unwrap()
    {
        delivered.push(command.command_id);
    }
    assert_eq!(delivered, vec![other, cancelled]);
    store.close().await;
}

#[tokio::test]
async fn a_newer_inventory_sync_supersedes_the_older_ones_of_the_same_library() {
    let (_temp, store, epoch) = store_with_node().await;
    let sync = |library: &str| {
        command(
            &format!("sync-{library}"),
            NodeCommandType::SyncInventory,
            None,
        )
    };
    let delivered = enqueue_at(&store, "nas", sync("lib_1"), NOW).await.unwrap();
    claim_next(&store, "nas", epoch, true, NOW)
        .await
        .unwrap()
        .unwrap();
    let queued = enqueue_at(&store, "nas", sync("lib_1"), NOW).await.unwrap();
    // `_` is not a wildcard: `lib01` is another library.
    let other = enqueue_at(&store, "nas", sync("lib01"), NOW).await.unwrap();
    let longer = enqueue_at(&store, "nas", sync("lib_1-b"), NOW)
        .await
        .unwrap();
    let newest = enqueue_at(&store, "nas", sync("lib_1"), NOW).await.unwrap();
    assert_ne!(queued, newest, "every emission is a distinct command");
    assert_eq!(state(&store, &delivered).await, "cancelled");
    assert_eq!(state(&store, &queued).await, "cancelled");
    assert_eq!(state(&store, &other).await, "queued");
    assert_eq!(state(&store, &newest).await, "queued");
    // `lib_1-b` is another library although its ids also start with `sync-lib_1-`.
    assert_eq!(state(&store, &longer).await, "queued");
    // Other command types are never superseded.
    let scan = enqueue_at(
        &store,
        "nas",
        command("sync-lib_1", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    enqueue_at(&store, "nas", sync("lib_1"), NOW).await.unwrap();
    assert_eq!(state(&store, &scan).await, "queued");
    store.close().await;
}

#[tokio::test]
async fn finished_commands_are_pruned_after_a_day() {
    let (_temp, store, epoch) = store_with_node().await;
    let old = NOW - 86_400 - 1;
    let acked = enqueue_at(
        &store,
        "nas",
        command("a", NodeCommandType::Scan, Some(NOW + 999_999)),
        old,
    )
    .await
    .unwrap();
    claim_next(&store, "nas", epoch, true, old)
        .await
        .unwrap()
        .unwrap();
    acknowledge(&store, "nas", std::slice::from_ref(&acked), old)
        .await
        .unwrap();
    let expired = enqueue_at(
        &store,
        "nas",
        command("e", NodeCommandType::Scan, Some(old + 1)),
        old,
    )
    .await
    .unwrap();
    let pending = enqueue_at(
        &store,
        "nas",
        command("p", NodeCommandType::Scan, Some(NOW + 999_999)),
        old,
    )
    .await
    .unwrap();
    let recent = enqueue_at(
        &store,
        "nas",
        command("r", NodeCommandType::Scan, Some(NOW - 1)),
        NOW - 10,
    )
    .await
    .unwrap();
    maintain(&store, NOW).await.unwrap();
    let left: Vec<String> = query_scalar("SELECT id FROM node_commands ORDER BY seq")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert_eq!(left, [pending.clone(), recent.clone()], "{acked} {expired}");
    assert_eq!(state(&store, &recent).await, "expired");
    assert_eq!(state(&store, &pending).await, "queued");
    store.close().await;
}

#[tokio::test]
async fn reachability_follows_this_cores_channel_or_another_cores_lease() {
    let (_temp, store, _) = store_with_node().await;
    let now = unix_now();
    // This Core owns the row but holds no live poll: not reachable (its own view wins).
    query("UPDATE node_channel_owner SET lease_until=? WHERE node_id='nas'")
        .bind(now + 60)
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE media_nodes SET last_seen_at=? WHERE id='nas'")
        .bind(now)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!node_reachable(&store, "nas").await);
    // Another Core holds a valid lease and the Node polled recently: reachable.
    query("UPDATE node_channel_owner SET core_id='other' WHERE node_id='nas'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(node_reachable(&store, "nas").await);
    // Silent for longer than a poll, lease expired or Node revoked: not reachable.
    query("UPDATE media_nodes SET last_seen_at=? WHERE id='nas'")
        .bind(now - REACHABLE_SINCE_SECONDS - 5)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!node_reachable(&store, "nas").await);
    query("UPDATE media_nodes SET last_seen_at=? WHERE id='nas'")
        .bind(now)
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE node_channel_owner SET lease_until=? WHERE node_id='nas'")
        .bind(now - 1)
        .execute(store.pool())
        .await
        .unwrap();
    assert!(!node_reachable(&store, "nas").await);
    store.close().await;
}

#[tokio::test]
async fn cancel_pending_keeps_only_the_revocation() {
    let (_temp, store, _) = store_with_node().await;
    let scan = enqueue_at(
        &store,
        "nas",
        command("scan", NodeCommandType::Scan, None),
        NOW,
    )
    .await
    .unwrap();
    let revoke = enqueue_at(
        &store,
        "nas",
        command("revoke", NodeCommandType::Revoke, None),
        NOW,
    )
    .await
    .unwrap();
    assert_eq!(cancel_pending(&store, "nas", &revoke).await.unwrap(), 1);
    assert_eq!(state(&store, &scan).await, "cancelled");
    assert_eq!(state(&store, &revoke).await, "queued");
    store.close().await;
}

/// Simulated Node for the PostgreSQL proof: the real duplicate memory and acknowledgement list
/// of `media_node`, plus the epoch fence (highest epoch seen, lower ones refused unacked).
struct SimulatedNode {
    log: media_node::server::CommandLog,
    highest_epoch: i64,
    /// Executions per logical command (`payload.n`).
    executions: HashMap<u64, u32>,
    duplicates: u64,
    stale_refused: u64,
}

impl SimulatedNode {
    fn receive(&mut self, command: &NodeCommandEnvelope) {
        let epoch = command.epoch.unwrap_or(0);
        if epoch < self.highest_epoch {
            self.stale_refused += 1;
            return;
        }
        self.highest_epoch = epoch;
        if !self.log.admit(&command.command_id) {
            self.duplicates += 1;
            return;
        }
        // Only the stress commands are counted (a former owner's late emissions are not).
        if let Some(n) = command.payload["n"].as_u64() {
            *self.executions.entry(n).or_default() += 1;
        }
    }
}

/// Roadmap §6, step 4 (in one process, simulated Node): two Cores on one PostgreSQL schema.
/// Commands emitted from the Core that does not own the channel are delivered by the owner; then
/// a stress run emits commands from both Cores while the simulated Node polls through a load
/// balancer, with lost responses, withheld acknowledgements, a command still in flight from the
/// former owner at each takeover, and three forced ownership changes (owner dead, owner
/// deregistered, owner dead again). Every command is executed exactly once.
///
/// `OUTBOX_STRESS_COMMANDS` overrides the number of commands (default 10 000).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_outbox_delivers_every_command_once_across_ownership_changes() {
    let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
        .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let schema = format!("node_outbox_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let a = IdentityStore::open_postgres(url.as_str()).await.unwrap();
    let b = IdentityStore::open_postgres(url.as_str()).await.unwrap();
    add_node(&a, "nas").await;
    let mut now = NOW;
    heartbeat(&a, now).await.unwrap();
    heartbeat(&b, now).await.unwrap();
    // One process, one host name: give A its own so that B never takes A for its predecessor.
    query("UPDATE core_instances SET hostname='core-a' WHERE id=?")
        .bind(a.instance_id())
        .execute(a.pool())
        .await
        .unwrap();

    // Exit proof: B does not own the channel and emits; A, the owner, delivers.
    let ChannelOwnership::Owned { epoch: a_epoch } = acquire(&a, "nas", now).await.unwrap() else {
        panic!("A owns the channel")
    };
    assert!(matches!(
        acquire(&b, "nas", now).await.unwrap(),
        ChannelOwnership::OwnedElsewhere { .. }
    ));
    let mut emitted = Vec::new();
    for index in 0..3 {
        emitted.push(
            enqueue_at(
                &b,
                "nas",
                command(&format!("exit-{index}"), NodeCommandType::CancelScan, None),
                now,
            )
            .await
            .unwrap(),
        );
    }
    assert!(
        has_queued(&a, "nas", now).await.unwrap(),
        "seen by the owner's probe"
    );
    assert!(
        claim_next(&b, "nas", a_epoch, true, now)
            .await
            .unwrap()
            .is_none(),
        "the non-owner never delivers"
    );
    let mut received = Vec::new();
    while let Some(delivered) = claim_next(&a, "nas", a_epoch, true, now).await.unwrap() {
        assert_eq!(delivered.epoch, Some(a_epoch));
        received.push(delivered.command_id);
    }
    assert_eq!(received, emitted);
    assert_eq!(acknowledge(&a, "nas", &received, now).await.unwrap(), 3);

    // Stress.
    let total: u64 = std::env::var("OUTBOX_STRESS_COMMANDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let started = std::time::Instant::now();
    let producer = {
        let (a, b) = (a.clone(), b.clone());
        tokio::spawn(async move {
            let mut full = 0u64;
            for n in 0..total {
                let store = if n % 2 == 0 { &a } else { &b };
                let mut envelope = command(
                    &format!("stress-{n}"),
                    NodeCommandType::CancelScan,
                    Some(NOW + 100_000_000),
                );
                envelope.payload = serde_json::json!({ "n": n });
                loop {
                    match enqueue_at(store, "nas", envelope.clone(), NOW).await {
                        Ok(_) => break,
                        Err(EnqueueError::Full) => {
                            full += 1;
                            tokio::time::sleep(Duration::from_millis(2)).await;
                        }
                        Err(EnqueueError::Database(err)) => panic!("enqueue: {err}"),
                    }
                }
            }
            full
        })
    };

    let cores = [a.clone(), b.clone()];
    let mut alive = [true, true];
    let mut node = SimulatedNode {
        log: media_node::server::CommandLog::new(),
        highest_epoch: 0,
        executions: HashMap::new(),
        duplicates: 0,
        stale_refused: 0,
    };
    let change_at = [total / 4, total / 2, 3 * total / 4];
    let mut changes = 0usize;
    let mut in_flight: Option<NodeCommandEnvelope> = None;
    let mut acks_held_until = 0i64;
    let (mut polls, mut deliveries, mut lost, mut refused_polls) = (0u64, 0u64, 0u64, 0u64);
    let mut stale_claims_refused = 0u64;
    loop {
        polls += 1;
        assert!(
            polls < total * 50 + 10_000,
            "no progress after {polls} polls"
        );
        now += 1;
        if now % 10 == 0 {
            for (core, live) in cores.iter().zip(alive) {
                if live {
                    heartbeat(core, now).await.unwrap();
                }
            }
        }
        // The load balancer spreads polls over the live Cores; only the owner serves them.
        let index = (polls % 2) as usize;
        let index = if alive[index] { index } else { 1 - index };
        let core = &cores[index];
        let epoch = match acquire(core, "nas", now).await.unwrap() {
            ChannelOwnership::Owned { epoch } => epoch,
            ChannelOwnership::OwnedElsewhere { .. } => {
                refused_polls += 1;
                continue;
            }
        };
        if now >= acks_held_until {
            let acks = node.log.pending_acks();
            acknowledge(core, "nas", &acks, now).await.unwrap();
            node.log.acknowledged(&acks);
        }
        maintain_node(core, "nas", epoch, now).await.unwrap();
        match claim_next(core, "nas", epoch, true, now).await.unwrap() {
            Some(delivered) => {
                deliveries += 1;
                if deliveries % 97 == 0 {
                    // The response never reached the Node.
                    lost += 1;
                } else {
                    node.receive(&delivered);
                    let executed = node.executions.len() as u64;
                    if executed.is_multiple_of(499) && now >= acks_held_until {
                        // The Node's polls stop carrying acknowledgements for a while (they
                        // time out): its commands are redelivered and must be deduplicated.
                        acks_held_until = now + ACK_TIMEOUT_SECONDS + 10;
                    }
                }
            }
            // A ping: it carries the epoch and raises the Node's fence too.
            None => node.highest_epoch = node.highest_epoch.max(epoch),
        }
        // The former owner's last command reaches the Node after the new owner spoke.
        if let Some(stale) = in_flight.take_if(|c| c.epoch.unwrap_or(0) < node.highest_epoch) {
            let before = node.stale_refused;
            node.receive(&stale);
            assert_eq!(
                node.stale_refused,
                before + 1,
                "a former owner's command was run"
            );
        }

        if changes < change_at.len() && node.executions.len() as u64 >= change_at[changes] {
            let owner: String =
                query_scalar("SELECT core_id FROM node_channel_owner WHERE node_id='nas'")
                    .fetch_one(a.pool())
                    .await
                    .unwrap();
            let old = usize::from(owner == b.instance_id());
            let new = 1 - old;
            let ChannelOwnership::Owned { epoch: old_epoch } =
                acquire(&cores[old], "nas", now).await.unwrap()
            else {
                panic!("the owner renews")
            };
            // Delivered by the old owner, still on the wire when it loses the channel.
            in_flight = claim_next(&cores[old], "nas", old_epoch, true, now)
                .await
                .unwrap();
            alive[new] = true;
            heartbeat(&cores[new], now).await.unwrap();
            if changes == 1 {
                // Graceful shutdown: the channel is released at once.
                deregister(&cores[old]).await.unwrap();
            } else {
                // Crash: the owner stops beating; the other Core waits for its stale heartbeat.
                now += HEARTBEAT_STALE_SECONDS + 1;
                heartbeat(&cores[new], now).await.unwrap();
            }
            alive[old] = false;
            let ChannelOwnership::Owned { epoch: new_epoch } =
                acquire(&cores[new], "nas", now).await.unwrap()
            else {
                panic!("the other Core takes the channel over (change {changes})")
            };
            assert!(new_epoch > old_epoch);
            // The former owner, unaware, can no longer deliver anything.
            // It may still emit: the command waits for the new owner.
            match enqueue_at(
                &cores[old],
                "nas",
                command("late", NodeCommandType::CancelScan, Some(NOW + 100_000_000)),
                NOW,
            )
            .await
            {
                Ok(_) | Err(EnqueueError::Full) => {}
                Err(err) => panic!("{err}"),
            }
            assert!(
                claim_next(&cores[old], "nas", old_epoch, true, now)
                    .await
                    .unwrap()
                    .is_none()
            );
            stale_claims_refused += 1;
            changes += 1;
        }

        if node.executions.len() as u64 == total && in_flight.is_none() && polls % 50 == 0 {
            let open: i64 = query_scalar(
                "SELECT COUNT(*) FROM node_commands WHERE command_type='cancel_scan' \
                    AND id LIKE 'stress-%' AND state<>'acked'",
            )
            .fetch_one(a.pool())
            .await
            .unwrap();
            if open == 0 && producer.is_finished() {
                break;
            }
        }
    }
    let full = producer.await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(changes, 3);
    assert_eq!(node.executions.len() as u64, total, "no command lost");
    let repeated: Vec<_> = node
        .executions
        .iter()
        .filter(|(_, count)| **count != 1)
        .collect();
    assert!(repeated.is_empty(), "executed more than once: {repeated:?}");
    assert!(node.stale_refused >= 1, "the in-flight injection ran");
    assert!(node.duplicates >= 1 && lost >= 1, "redeliveries happened");
    let (attempts, rows): (i64, i64) = query_as(
        "SELECT CAST(COALESCE(SUM(attempts),0) AS BIGINT),COUNT(*) FROM node_commands WHERE id LIKE 'stress-%'",
    )
    .fetch_one(a.pool())
    .await
    .unwrap();
    assert_eq!(rows as u64, total);
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "outbox_stress",
            "commands": total,
            "elapsedMs": elapsed.as_millis() as u64,
            "polls": polls,
            "refusedPolls": refused_polls,
            "deliveries": deliveries,
            "attempts": attempts,
            "lostResponses": lost,
            "duplicatesIgnored": node.duplicates,
            "staleRefused": node.stale_refused,
            "staleClaimsRefused": stale_claims_refused,
            "enqueueFullRetries": full,
            "ownershipChanges": changes,
        })
    );

    a.close().await;
    b.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}
