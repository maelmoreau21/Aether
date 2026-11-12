//! Fences publication of invocation history. This does not fence all guest side effects.
use crate::{
    database::{query, query_scalar},
    identity::IdentityStore,
};
use serde_json::Value;
use std::sync::OnceLock;

pub(super) const LEASE_SECONDS: i64 = 30;
pub(super) const HEARTBEAT_SECONDS: u64 = 5;
fn now() -> i64 {
    media_contracts::protocol::unix_now()
}
fn owner() -> &'static str {
    static OWNER: OnceLock<String> = OnceLock::new();
    OWNER.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

#[derive(Clone)]
pub(super) struct Lease {
    pub id: String,
    owner: String,
    token: String,
}
impl Lease {
    pub async fn begin(
        store: &IdentityStore,
        id: String,
        extension: &str,
        library: &str,
        work: &str,
        actor: &str,
    ) -> Result<Self, sqlx::Error> {
        let lease = Self {
            id,
            owner: owner().into(),
            token: uuid::Uuid::new_v4().to_string(),
        };
        let mut conn = store.pool().acquire().await?;
        let mut tx = crate::database::begin_write(&mut conn).await?;
        query("INSERT INTO extension_runs(id,extension_id,library_id,work_id,actor_id,started_at,status) VALUES(?,?,?,?,?,?,'running')")
            .bind(&lease.id).bind(extension).bind(library).bind(work).bind(actor).bind(now()).execute(&mut *tx).await?;
        query("INSERT INTO extension_run_owners(run_id,owner_id,fence_token,lease_until) VALUES(?,?,?,?)")
            .bind(&lease.id).bind(&lease.owner).bind(&lease.token).bind(now()+LEASE_SECONDS).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(lease)
    }
    pub async fn live(&self, store: &IdentityStore) -> Result<bool, sqlx::Error> {
        let count:i64=query_scalar("SELECT COUNT(*) FROM extension_run_owners o JOIN extension_runs r ON r.id=o.run_id WHERE o.run_id=? AND o.owner_id=? AND o.fence_token=? AND o.lease_until>? AND r.status='running'")
            .bind(&self.id).bind(&self.owner).bind(&self.token).bind(now()).fetch_one(store.pool()).await?;
        Ok(count == 1)
    }
    pub async fn renew(&self, store: &IdentityStore) -> Result<bool, sqlx::Error> {
        let updated=query("UPDATE extension_run_owners SET lease_until=? WHERE run_id=? AND owner_id=? AND fence_token=? AND lease_until>? AND EXISTS(SELECT 1 FROM extension_runs r WHERE r.id=extension_run_owners.run_id AND r.status='running')")
            .bind(now()+LEASE_SECONDS).bind(&self.id).bind(&self.owner).bind(&self.token).bind(now()).execute(store.pool()).await?;
        Ok(updated.rows_affected() == 1)
    }
    pub async fn finish(
        &self,
        store: &IdentityStore,
        status: &str,
        details: &Value,
        duration: i64,
    ) -> Result<bool, sqlx::Error> {
        let mut conn = store.pool().acquire().await?;
        let mut tx = crate::database::begin_write(&mut conn).await?;
        let updated=query("UPDATE extension_runs SET status=?,result_json=?,duration_ms=? WHERE id=? AND status='running' AND EXISTS(SELECT 1 FROM extension_run_owners o WHERE o.run_id=extension_runs.id AND o.owner_id=? AND o.fence_token=? AND o.lease_until>?)")
            .bind(status).bind(details.to_string()).bind(duration).bind(&self.id).bind(&self.owner).bind(&self.token).bind(now()).execute(&mut *tx).await?;
        if updated.rows_affected() == 1 {
            query(
                "DELETE FROM extension_run_owners WHERE run_id=? AND owner_id=? AND fence_token=?",
            )
            .bind(&self.id)
            .bind(&self.owner)
            .bind(&self.token)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(updated.rows_affected() == 1)
    }
}

/// Never replay an invocation: only mark expired history interrupted. Legacy
/// ownerless rows keep the former conservative hour on shared PostgreSQL.
pub(super) async fn recover(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let mut conn = store.pool().acquire().await?;
    let legacy_cutoff = if conn.backend_name() == "SQLite" {
        now() + 1
    } else {
        now() - 3600
    };
    let mut tx = crate::database::begin_write(&mut conn).await?;
    let expired=query("UPDATE extension_runs \
            SET status='interrupted' \
            WHERE status='running' AND (EXISTS(SELECT 1 FROM extension_run_owners o \
            WHERE o.run_id=extension_runs.id AND o.lease_until<=?) OR (started_at<? \
            AND NOT EXISTS(SELECT 1 FROM extension_run_owners o WHERE o.run_id=extension_runs.id)))")
        .bind(now()).bind(legacy_cutoff).execute(&mut *tx).await?;
    query("DELETE FROM extension_run_owners WHERE EXISTS(SELECT 1 FROM extension_runs r WHERE r.id=extension_run_owners.run_id AND r.status!='running')")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(expired.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn contract(first: &IdentityStore, second: &IdentityStore) {
        let lease = Lease::begin(
            first,
            "live-run".into(),
            "fixture",
            "library",
            "work",
            "actor",
        )
        .await
        .unwrap();
        assert_eq!(
            recover(second).await.unwrap(),
            0,
            "another pool must not interrupt a live owner"
        );
        assert!(lease.live(second).await.unwrap());
        assert!(lease.renew(first).await.unwrap());
        let stale = Lease {
            id: lease.id.clone(),
            owner: "different-process".into(),
            token: "stale-token".into(),
        };
        assert!(
            !stale
                .finish(second, "completed", &json!({"bad":true}), 1)
                .await
                .unwrap()
        );
        query("UPDATE extension_run_owners SET lease_until=? WHERE run_id=?")
            .bind(now() - 1)
            .bind(&lease.id)
            .execute(second.pool())
            .await
            .unwrap();
        assert!(
            !lease.renew(first).await.unwrap(),
            "an expired lease cannot resurrect"
        );
        assert_eq!(recover(second).await.unwrap(), 1);
        assert!(
            !lease
                .finish(first, "completed", &json!({"late":true}), 1)
                .await
                .unwrap()
        );
        let (status, result): (String, String) = crate::database::query_as(
            "SELECT status,result_json FROM extension_runs WHERE id='live-run'",
        )
        .fetch_one(first.pool())
        .await
        .unwrap();
        assert_eq!(status, "interrupted");
        assert_eq!(result, "{}");
        let finished = Lease::begin(
            first,
            "completed-run".into(),
            "fixture",
            "library",
            "work",
            "actor",
        )
        .await
        .unwrap();
        assert!(
            finished
                .finish(second, "completed", &json!({"accepted":true}), 2)
                .await
                .unwrap()
        );
        assert!(
            !finished
                .finish(first, "error", &json!({"overwrite":true}), 3)
                .await
                .unwrap()
        );
        assert_eq!(recover(second).await.unwrap(), 0);
        let value: String =
            query_scalar("SELECT result_json FROM extension_runs WHERE id='completed-run'")
                .fetch_one(second.pool())
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&value).unwrap(),
            json!({"accepted":true})
        );
    }
    #[tokio::test]
    async fn sqlite_history_owner_is_fenced_and_expired_runs_are_not_replayed() {
        let temp = tempfile::tempdir().unwrap();
        let first = IdentityStore::open(&temp.path().join("ownership.sqlite"))
            .await
            .unwrap();
        let second = IdentityStore::open(&temp.path().join("ownership.sqlite"))
            .await
            .unwrap();
        contract(&first, &second).await;
    }
    #[tokio::test]
    #[ignore = "requires a disposable PostgreSQL server and MEDIA_TEST_POSTGRES_URL"]
    async fn postgres_history_owner_is_fenced_between_independent_pools() {
        let url = zeroize::Zeroizing::new(
            std::env::var("MEDIA_TEST_POSTGRES_URL").expect("set disposable PostgreSQL URL"),
        );
        let first = IdentityStore::open_postgres(&url).await.unwrap();
        let second = IdentityStore::open_postgres(&url).await.unwrap();
        contract(&first, &second).await;
        first.close().await;
        second.close().await;
    }
}
