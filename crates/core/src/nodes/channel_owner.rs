//! Durable ownership of each Media Node's command channel (multi-Core, roadmap §2.1).
//!
//! At any time one Core owns the control channel of a Node: a `node_channel_owner` row under a
//! lease, renewed by every control poll the Node makes to that Core. The row carries an `epoch`
//! that never decreases and grows at every change of owner; every command delivered on the
//! channel carries it, and the Node refuses commands of an epoch lower than the highest it has
//! seen (roadmap §2.3). A Core that loses the compare-and-set refuses the poll
//! ([`CHANNEL_OWNED_ELSEWHERE`]); the Node then reconnects through its configured Core URL.
use super::channel::POLL_WAIT;
use crate::core_instances::HEARTBEAT_STALE_SECONDS;
use crate::database::{query, query_as};
use crate::identity::IdentityStore;

/// A channel lease outlives three control polls (`POLL_WAIT`, 25 s) without renewal.
pub(crate) const CHANNEL_LEASE_SECONDS: i64 = 3 * POLL_WAIT.as_secs() as i64;

/// Error code of the signed `409 Conflict` answering a poll that reached a Core which does not
/// own the Node's channel. A Node retries shortly through its configured (logical) Core URL.
pub(crate) const CHANNEL_OWNED_ELSEWHERE: &str = "channel_owned_elsewhere";

/// Outcome of [`acquire`] for this Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChannelOwnership {
    /// This Core owns the channel at `epoch` until the lease written now expires.
    Owned { epoch: i64 },
    /// Another live Core holds a valid lease.
    OwnedElsewhere { core_id: String, epoch: i64 },
}

/// Acquire or renew this Core's ownership of `node_id`'s channel in one compare-and-set.
///
/// The row is taken over when this Core already owns it, when the owner's lease expired, when
/// the owner is not a live Core (`core_instances` heartbeat older than
/// [`HEARTBEAT_STALE_SECONDS`], or row removed by a graceful shutdown), and always on SQLite,
/// which is a single Core by construction (a restart is a new instance whose predecessor is
/// gone, even if it crashed seconds ago and still looks alive).
///
/// The epoch stays the same while the owner stays the same. A change of owner sets it to
/// `max(previous + 1, now)`: strictly increasing, and also above the epochs a Node may have
/// seen before the row was lost (Node deleted and paired again, database restored from a
/// backup), which a plain `+1` from a smaller value would never reach.
pub(crate) async fn acquire(
    store: &IdentityStore,
    node_id: &str,
    now: i64,
) -> Result<ChannelOwnership, sqlx::Error> {
    acquire_with(store, node_id, now, !store.is_postgres()).await
}

async fn acquire_with(
    store: &IdentityStore,
    node_id: &str,
    now: i64,
    single_core: bool,
) -> Result<ChannelOwnership, sqlx::Error> {
    query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO node_channel_owner(node_id,core_id,epoch,lease_until,uplink_url) \
            VALUES(?,?,?,?,'') ON CONFLICT(node_id) DO UPDATE SET \
            epoch=CASE WHEN node_channel_owner.core_id=excluded.core_id \
            THEN node_channel_owner.epoch WHEN node_channel_owner.epoch>=excluded.epoch \
            THEN node_channel_owner.epoch+1 ELSE excluded.epoch END,\
            core_id=excluded.core_id,lease_until=excluded.lease_until,\
            uplink_url=excluded.uplink_url \
            WHERE node_channel_owner.core_id=excluded.core_id OR node_channel_owner.lease_until<? \
            OR ?=1 OR NOT {}",
        crate::core_instances::alive_sql("node_channel_owner.core_id")
    )))
    .bind(node_id)
    .bind(store.instance_id())
    .bind(now)
    .bind(now.saturating_add(CHANNEL_LEASE_SECONDS))
    .bind(now)
    .bind(i64::from(single_core))
    .bind(now.saturating_sub(HEARTBEAT_STALE_SECONDS))
    .bind(crate::core_instances::hostname())
    .bind(store.instance_id())
    .execute(store.pool())
    .await?;
    // Read back rather than trusting the affected-row count: a takeover by another Core between
    // the two statements is then reported as lost, never as owned.
    let (core_id, epoch): (String, i64) =
        query_as("SELECT core_id,epoch FROM node_channel_owner WHERE node_id=?")
            .bind(node_id)
            .fetch_one(store.pool())
            .await?;
    Ok(if core_id == store.instance_id() {
        ChannelOwnership::Owned { epoch }
    } else {
        ChannelOwnership::OwnedElsewhere { core_id, epoch }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_instances::heartbeat;

    /// Far in the future of the real clock: the epoch of a first acquisition is `now`.
    const NOW: i64 = 4_000_000_000;

    async fn add_node(store: &IdentityStore) {
        query(
            "INSERT INTO media_nodes(id,name,kind,status) VALUES('nas','NAS','remote','connected')",
        )
        .execute(store.pool())
        .await
        .unwrap();
    }

    async fn store_with_node() -> (tempfile::TempDir, IdentityStore) {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        add_node(&store).await;
        (temp, store)
    }

    async fn owner_row(store: &IdentityStore) -> (String, i64, i64) {
        query_as("SELECT core_id,epoch,lease_until FROM node_channel_owner WHERE node_id='nas'")
            .fetch_one(store.pool())
            .await
            .unwrap()
    }

    /// Another Core owns the channel at `epoch` until `lease_until`, last seen alive at `seen`
    /// (`None`: no `core_instances` row, i.e. deregistered).
    async fn foreign_owner(store: &IdentityStore, epoch: i64, lease_until: i64, seen: Option<i64>) {
        query("UPDATE node_channel_owner SET core_id='other',epoch=?,lease_until=? WHERE node_id='nas'")
            .bind(epoch)
            .bind(lease_until)
            .execute(store.pool())
            .await
            .unwrap();
        query("DELETE FROM core_instances WHERE id='other'")
            .execute(store.pool())
            .await
            .unwrap();
        if let Some(seen) = seen {
            query("INSERT INTO core_instances(id,started_at,heartbeat_at) VALUES('other',?,?)")
                .bind(seen)
                .bind(seen)
                .execute(store.pool())
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn first_poll_acquires_and_renewals_keep_the_epoch() {
        let (_temp, store) = store_with_node().await;
        let first = acquire(&store, "nas", NOW).await.unwrap();
        assert_eq!(first, ChannelOwnership::Owned { epoch: NOW });
        for later in [NOW + 30, NOW + 1_000] {
            assert_eq!(
                acquire_with(&store, "nas", later, false).await.unwrap(),
                first,
                "a renewal never changes the epoch"
            );
        }
        assert_eq!(
            owner_row(&store).await,
            (
                store.instance_id().to_owned(),
                NOW,
                NOW + 1_000 + CHANNEL_LEASE_SECONDS
            )
        );
        assert_eq!(CHANNEL_LEASE_SECONDS, 75);
        store.close().await;
    }

    #[tokio::test]
    async fn takeover_after_lease_expiry_bumps_the_epoch() {
        let (_temp, store) = store_with_node().await;
        acquire(&store, "nas", NOW).await.unwrap();
        // A live Core owned it last, at a higher epoch, and its lease just ran out.
        foreign_owner(&store, NOW + 500, NOW + 10, Some(NOW + 11)).await;
        assert_eq!(
            acquire_with(&store, "nas", NOW + 11, false).await.unwrap(),
            ChannelOwnership::Owned { epoch: NOW + 501 }
        );
        // A takeover long afterwards jumps to the clock: still strictly increasing.
        foreign_owner(&store, NOW + 501, NOW + 600, Some(NOW + 5_000)).await;
        assert_eq!(
            acquire_with(&store, "nas", NOW + 5_000, false)
                .await
                .unwrap(),
            ChannelOwnership::Owned { epoch: NOW + 5_000 }
        );
        store.close().await;
    }

    #[tokio::test]
    async fn a_live_owner_keeps_its_channel_until_it_dies_or_deregisters() {
        let (_temp, store) = store_with_node().await;
        acquire(&store, "nas", NOW).await.unwrap();
        // Alive with a valid lease: refused, and nothing changes.
        foreign_owner(&store, NOW + 3, NOW + 60, Some(NOW)).await;
        assert_eq!(
            acquire_with(&store, "nas", NOW + 1, false).await.unwrap(),
            ChannelOwnership::OwnedElsewhere {
                core_id: "other".into(),
                epoch: NOW + 3
            }
        );
        assert_eq!(owner_row(&store).await, ("other".into(), NOW + 3, NOW + 60));
        // Its heartbeat goes stale: taken over before the lease ends (the clock is above
        // the previous epoch, so the new epoch is the clock).
        let stale = NOW + HEARTBEAT_STALE_SECONDS + 1;
        assert_eq!(
            acquire_with(&store, "nas", stale, false).await.unwrap(),
            ChannelOwnership::Owned { epoch: stale }
        );
        // A graceful shutdown removes the owner's row: taken over at once.
        foreign_owner(&store, NOW + 9, NOW + 60, None).await;
        assert_eq!(
            acquire_with(&store, "nas", NOW + 2, false).await.unwrap(),
            ChannelOwnership::Owned { epoch: NOW + 10 }
        );
        store.close().await;
    }

    #[tokio::test]
    async fn a_core_restarted_on_the_same_host_takes_its_predecessors_channel_at_once() {
        let (_temp, store) = store_with_node().await;
        assert!(
            !crate::core_instances::hostname().is_empty(),
            "the test needs a host name"
        );
        heartbeat(&store, NOW).await.unwrap();
        acquire(&store, "nas", NOW).await.unwrap();
        let predecessor = |host: &'static str| {
            let store = store.clone();
            async move {
                let host = if host.is_empty() {
                    crate::core_instances::hostname()
                } else {
                    host.to_owned()
                };
                query("UPDATE core_instances SET hostname=?,started_at=? WHERE id='other'")
                    .bind(host)
                    .bind(NOW - 100)
                    .execute(store.pool())
                    .await
                    .unwrap();
            }
        };
        // Crashed seconds ago on this host (container restarted): heartbeat still fresh, lease
        // still valid, yet it is this Core's predecessor and is taken over at once.
        foreign_owner(&store, NOW + 3, NOW + 60, Some(NOW)).await;
        predecessor("").await;
        assert_eq!(
            acquire_with(&store, "nas", NOW + 1, false).await.unwrap(),
            ChannelOwnership::Owned { epoch: NOW + 4 }
        );
        // A live Core on another host keeps its channel.
        foreign_owner(&store, NOW + 5, NOW + 60, Some(NOW)).await;
        predecessor("another-host").await;
        assert_eq!(
            acquire_with(&store, "nas", NOW + 2, false).await.unwrap(),
            ChannelOwnership::OwnedElsewhere {
                core_id: "other".into(),
                epoch: NOW + 5
            }
        );
        store.close().await;
    }

    #[tokio::test]
    async fn sqlite_restart_takes_over_at_once_with_a_higher_epoch() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let before = IdentityStore::open(&path).await.unwrap();
        add_node(&before).await;
        // The previous process looks alive (fresh heartbeat, valid lease): it crashed just now.
        heartbeat(&before, NOW).await.unwrap();
        let ChannelOwnership::Owned { epoch } = acquire(&before, "nas", NOW).await.unwrap() else {
            panic!("first acquisition")
        };
        before.close().await;
        let after = IdentityStore::open(&path).await.unwrap();
        assert!(!after.is_postgres());
        // SQLite is a single Core: the new instance never waits for the old lease.
        assert_eq!(
            acquire(&after, "nas", NOW + 1).await.unwrap(),
            ChannelOwnership::Owned { epoch: epoch + 1 }
        );
        after.close().await;
    }

    #[tokio::test]
    #[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
    async fn postgres_two_cores_own_a_node_channel_one_at_a_time() {
        let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
            .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
        let pool = sqlx::PgPool::connect(&connection).await.unwrap();
        let schema = format!("channel_owner_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&pool)
            .await
            .unwrap();
        let mut url = url::Url::parse(&connection).unwrap();
        url.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));

        // Two stores on one database are two Cores.
        let a = IdentityStore::open_postgres(url.as_str()).await.unwrap();
        let b = IdentityStore::open_postgres(url.as_str()).await.unwrap();
        assert!(a.is_postgres() && b.is_postgres());
        add_node(&a).await;
        heartbeat(&a, NOW).await.unwrap();
        heartbeat(&b, NOW).await.unwrap();

        // A owns N; B cannot take it while A is alive and its lease valid.
        let owned_by_a = acquire(&a, "nas", NOW).await.unwrap();
        assert_eq!(owned_by_a, ChannelOwnership::Owned { epoch: NOW });
        assert_eq!(
            acquire(&b, "nas", NOW + 1).await.unwrap(),
            ChannelOwnership::OwnedElsewhere {
                core_id: a.instance_id().to_owned(),
                epoch: NOW
            }
        );
        assert_eq!(acquire(&a, "nas", NOW + 25).await.unwrap(), owned_by_a);

        // A dies: its heartbeat goes stale while its lease (renewed at NOW + 25) still runs.
        let later = NOW + HEARTBEAT_STALE_SECONDS + 1;
        assert!(later < NOW + 25 + CHANNEL_LEASE_SECONDS);
        heartbeat(&b, later).await.unwrap();
        let ChannelOwnership::Owned { epoch: b_epoch } = acquire(&b, "nas", later).await.unwrap()
        else {
            panic!("B takes over a dead owner before its lease ends")
        };
        assert!(b_epoch > NOW, "a new owner always raises the epoch");
        // A's renewal fails: it learns that B owns the channel.
        assert_eq!(
            acquire(&a, "nas", later + 1).await.unwrap(),
            ChannelOwnership::OwnedElsewhere {
                core_id: b.instance_id().to_owned(),
                epoch: b_epoch
            }
        );

        // A comes back; B shuts down gracefully: A takes over at once, at a higher epoch.
        heartbeat(&a, later + 2).await.unwrap();
        crate::core_instances::deregister(&b).await.unwrap();
        let ChannelOwnership::Owned { epoch: a_epoch } =
            acquire(&a, "nas", later + 3).await.unwrap()
        else {
            panic!("a deregistered owner releases the channel immediately")
        };
        assert!(a_epoch > b_epoch);

        // Concurrent first polls through a load balancer: exactly one Core wins.
        heartbeat(&b, later + 3).await.unwrap();
        query(
            "INSERT INTO media_nodes(id,name,kind,status) VALUES('raced','R','remote','connected')",
        )
        .execute(a.pool())
        .await
        .unwrap();
        let (from_a, from_b) = tokio::join!(
            acquire(&a, "raced", later + 4),
            acquire(&b, "raced", later + 4)
        );
        let owners = [from_a.unwrap(), from_b.unwrap()];
        assert_eq!(
            owners
                .iter()
                .filter(|o| matches!(o, ChannelOwnership::Owned { .. }))
                .count(),
            1,
            "{owners:?}"
        );

        a.close().await;
        b.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    #[tokio::test]
    async fn a_lost_row_restarts_above_the_epochs_already_handed_out() {
        let (_temp, store) = store_with_node().await;
        acquire(&store, "nas", NOW).await.unwrap();
        // Node deleted and paired again (cascade), or a database restored from a backup.
        query("DELETE FROM node_channel_owner WHERE node_id='nas'")
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            acquire(&store, "nas", NOW + 100).await.unwrap(),
            ChannelOwnership::Owned { epoch: NOW + 100 }
        );
        store.close().await;
    }
}
