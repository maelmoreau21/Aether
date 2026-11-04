//! Coalesced, batched `last_seen_at` session activity writes.
use super::util::IDLE_SECONDS;
use sqlx::Connection;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

pub(super) const TOUCH_INTERVAL_SECONDS: i64 = 60;
pub(super) const MAX_PENDING_TOUCHES: usize = 4096;
pub(super) const TOUCH_BATCH_SIZE: usize = 64;
pub(super) const TOUCH_COALESCE_MS: u64 = 20;
pub(super) const TOUCH_RETRY_BACKOFF_MS: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionTouchStats {
    pub pending_count: usize,
    pub write_batches_succeeded: u64,
    pub rows_updated: u64,
    pub write_conflicts: u64,
    pub dropped_overflow: u64,
}

pub(super) struct TouchShared {
    pending: Mutex<HashMap<String, i64>>,
    pub(super) notify: Notify,
    pub(super) flush_done: Notify,
    closed: AtomicBool,
    pub(super) interval_seconds: i64,
    max_pending: usize,
    write_batches_succeeded: AtomicU64,
    rows_updated: AtomicU64,
    write_conflicts: AtomicU64,
    dropped_overflow: AtomicU64,
}

impl TouchShared {
    pub(super) fn record(&self, token_hash: &str, now: i64) {
        let mut pending = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(existing) = pending.get_mut(token_hash) {
            if now > *existing {
                *existing = now;
            }
            return;
        }
        if pending.len() >= self.max_pending {
            self.dropped_overflow.fetch_add(1, Ordering::Relaxed);
            return;
        }
        pending.insert(token_hash.to_owned(), now);
        drop(pending);
        self.notify.notify_one();
    }

    pub(super) fn cancel(&self, token_hash: &str) {
        let mut pending = match self.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.remove(token_hash);
    }

    pub(super) fn stats(&self) -> SessionTouchStats {
        let pending_count = match self.pending.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        };
        SessionTouchStats {
            pending_count,
            write_batches_succeeded: self.write_batches_succeeded.load(Ordering::Relaxed),
            rows_updated: self.rows_updated.load(Ordering::Relaxed),
            write_conflicts: self.write_conflicts.load(Ordering::Relaxed),
            dropped_overflow: self.dropped_overflow.load(Ordering::Relaxed),
        }
    }
}

pub(super) struct SessionToucher {
    pub(super) shared: Arc<TouchShared>,
}

impl Drop for SessionToucher {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.notify.notify_one();
    }
}

enum FlushError {
    BusyOrTimeout,
    Permanent,
}

impl SessionToucher {
    pub(super) fn new(pool: sqlx::AnyPool, interval_seconds: i64, max_pending: usize) -> Arc<Self> {
        let shared = Arc::new(TouchShared {
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            flush_done: Notify::new(),
            closed: AtomicBool::new(false),
            interval_seconds: interval_seconds.max(1),
            max_pending: max_pending.max(1),
            write_batches_succeeded: AtomicU64::new(0),
            rows_updated: AtomicU64::new(0),
            write_conflicts: AtomicU64::new(0),
            dropped_overflow: AtomicU64::new(0),
        });
        let worker_shared = shared.clone();
        tokio::spawn(async move {
            Self::worker_loop(pool, worker_shared).await;
        });
        Arc::new(Self { shared })
    }

    async fn worker_loop(pool: sqlx::AnyPool, shared: Arc<TouchShared>) {
        loop {
            if shared.closed.load(Ordering::Acquire) || pool.is_closed() {
                shared.flush_done.notify_waiters();
                break;
            }
            let notified = shared.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            let is_empty = match shared.pending.lock() {
                Ok(guard) => guard.is_empty(),
                Err(poisoned) => poisoned.into_inner().is_empty(),
            };
            if is_empty {
                notified.await;
                if shared.closed.load(Ordering::Acquire) || pool.is_closed() {
                    shared.flush_done.notify_waiters();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(TOUCH_COALESCE_MS)).await;
            }

            let batch: Vec<(String, i64)> = {
                let guard = match shared.pending.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                guard
                    .iter()
                    .take(TOUCH_BATCH_SIZE)
                    .map(|(k, v)| (k.clone(), *v))
                    .collect()
            };
            if batch.is_empty() {
                shared.flush_done.notify_waiters();
                continue;
            }

            match Self::flush_batch(&pool, &batch).await {
                Ok(rows_affected) => {
                    shared
                        .write_batches_succeeded
                        .fetch_add(1, Ordering::Relaxed);
                    shared
                        .rows_updated
                        .fetch_add(rows_affected, Ordering::Relaxed);
                    {
                        let mut guard = match shared.pending.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        for (token_hash, flushed_at) in &batch {
                            if let Some(&current_at) = guard.get(token_hash)
                                && current_at.saturating_sub(*flushed_at) < shared.interval_seconds
                            {
                                guard.remove(token_hash);
                            }
                        }
                    }
                    shared.flush_done.notify_waiters();
                }
                Err(FlushError::BusyOrTimeout) => {
                    shared.write_conflicts.fetch_add(1, Ordering::Relaxed);
                    shared.flush_done.notify_waiters();
                    if shared.closed.load(Ordering::Acquire) || pool.is_closed() {
                        break;
                    }
                    let sleep_notified = shared.notify.notified();
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(TOUCH_RETRY_BACKOFF_MS)) => {}
                        _ = sleep_notified, if shared.closed.load(Ordering::Acquire) => break,
                    }
                }
                Err(FlushError::Permanent) => {
                    {
                        let mut guard = match shared.pending.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        for (token_hash, _) in &batch {
                            guard.remove(token_hash);
                        }
                    }
                    shared.flush_done.notify_waiters();
                    if pool.is_closed() || shared.closed.load(Ordering::Acquire) {
                        break;
                    }
                }
            }
        }
    }

    async fn flush_batch(pool: &sqlx::AnyPool, batch: &[(String, i64)]) -> Result<u64, FlushError> {
        let mut conn = match tokio::time::timeout(Duration::from_millis(50), pool.acquire()).await {
            Ok(Ok(conn)) => conn,
            Ok(Err(sqlx::Error::PoolTimedOut)) | Err(_) => return Err(FlushError::BusyOrTimeout),
            Ok(Err(_)) => return Err(FlushError::Permanent),
        };
        let sqlite = conn.backend_name() == "SQLite";
        let orig_timeout: i64 = if sqlite {
            crate::database::query_scalar("PRAGMA busy_timeout")
                .fetch_one(&mut *conn)
                .await
                .unwrap_or(15_000)
        } else {
            0
        };
        if sqlite {
            let _ = crate::database::query("PRAGMA busy_timeout = 15")
                .execute(&mut *conn)
                .await;
        }
        let mut tx = conn.begin().await.map_err(|_| FlushError::Permanent)?;
        if !sqlite {
            crate::database::query("SET LOCAL lock_timeout = '15ms'")
                .execute(&mut *tx)
                .await
                .map_err(|_| FlushError::Permanent)?;
        }

        let mut qb = sqlx::QueryBuilder::<sqlx::Any>::new(
            "UPDATE sessions SET last_seen_at = CASE token_hash ",
        );
        for (token_hash, seen_at) in batch {
            qb.push("WHEN ");
            qb.push_bind(token_hash);
            qb.push(
                " THEN CASE WHEN last_seen_at > unixepoch() THEN unixepoch() WHEN last_seen_at > ",
            );
            qb.push_bind(*seen_at);
            qb.push(" THEN last_seen_at ELSE CASE WHEN unixepoch() < ");
            qb.push_bind(*seen_at);
            qb.push(" THEN unixepoch() ELSE ");
            qb.push_bind(*seen_at);
            qb.push(" END END ");
        }
        qb.push("ELSE last_seen_at END WHERE token_hash IN (");
        let mut separated = qb.separated(", ");
        for (token_hash, _) in batch {
            separated.push_bind(token_hash);
        }
        separated
            .push_unseparated(") AND expires_at > unixepoch() AND last_seen_at > unixepoch() - ");
        qb.push_bind(IDLE_SECONDS);
        qb.push(
            " AND EXISTS(SELECT 1 FROM users WHERE users.id = sessions.user_id AND disabled_at IS NULL)",
        );

        use sqlx::Execute;
        let mut built = qb.build();
        let args = built.take_arguments().map_err(|_| FlushError::Permanent)?;
        let sql = crate::database::normalize_sql(built.sql().as_str());
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql), args.unwrap_or_default())
            .execute(&mut *tx)
            .await;
        let result = match result {
            Ok(result) => tx.commit().await.map(|_| result),
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        };
        if sqlite {
            let mut restore_qb = sqlx::QueryBuilder::<sqlx::Any>::new("PRAGMA busy_timeout = ");
            restore_qb.push(orig_timeout.max(0));
            let _ = restore_qb.build().execute(&mut *conn).await;
        }

        match result {
            Ok(res) => Ok(res.rows_affected()),
            Err(sqlx::Error::PoolTimedOut) => Err(FlushError::BusyOrTimeout),
            Err(sqlx::Error::Database(err)) => {
                let is_busy = err
                    .code()
                    .is_some_and(|code| code == "55P03" || code == "57014")
                    || err
                        .code()
                        .and_then(|c| c.parse::<i32>().ok())
                        .is_some_and(|code| matches!(code & 0xff, 5 | 6))
                    || {
                        let msg = err.message().to_ascii_lowercase();
                        msg.contains("busy") || msg.contains("locked")
                    };
                if is_busy {
                    Err(FlushError::BusyOrTimeout)
                } else {
                    Err(FlushError::Permanent)
                }
            }
            Err(_) => Err(FlushError::Permanent),
        }
    }
}
