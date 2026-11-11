//! Transactional outbox feeding the existing leased task queue. No independent scheduler.

use crate::{
    database::{begin_write_serialized, query, query_as, query_scalar},
    identity::IdentityStore,
    task_engine::Job,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Any, Transaction};

mod cache;
mod completion;
mod drain;
mod events;
mod fingerprint;
mod metadata;
mod policy;
pub use cache::*;
pub use completion::*;
pub use drain::*;
pub use events::*;
pub use fingerprint::*;
pub use metadata::*;
pub use policy::*;

type MusicSnapshotRow = (String, i64, String, i64, String, String, String, String);

type MusicMetadataRow = (
    String,
    String,
    String,
    String,
    String,
    Option<f64>,
    String,
    Option<String>,
);

type MusicCurrentRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    String,
    String,
    String,
    String,
);

pub struct EventInput<'a> {
    pub key: &'a str,
    pub kind: &'a str,
    pub library_id: &'a str,
    pub asset_id: &'a str,
    pub fingerprint: &'a str,
    pub payload: &'a Value,
}

pub(crate) fn hash(value: &impl Serialize) -> Result<String, sqlx::Error> {
    let bytes = serde_json::to_vec(value).map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

type CurrentFingerprintRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

#[cfg(test)]
mod marker_source_tests {
    use super::*;

    #[tokio::test]
    async fn marker_inputs_must_still_match_their_observed_revisions() {
        sqlx::any::install_default_drivers();
        let pool = sqlx::any::AnyPoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for sql in [
            "CREATE TABLE libraries(id TEXT PRIMARY KEY, root TEXT)",
            "CREATE TABLE inventory_sources(id TEXT PRIMARY KEY, root_snapshot TEXT)",
            "CREATE TABLE inventory_assets(id TEXT PRIMARY KEY, source_id TEXT, library_id TEXT, available INTEGER)",
            "CREATE TABLE automation_observations(asset_id TEXT PRIMARY KEY, fingerprint TEXT)",
            "INSERT INTO libraries VALUES('lib','/read-only')",
            "INSERT INTO inventory_sources VALUES('s1','/read-only'),('s2','/read-only')",
            "INSERT INTO inventory_assets VALUES('a','s1','lib',1),('b','s2','lib',1)",
            "INSERT INTO automation_observations VALUES('a','rev-a'),('b','rev-b')",
        ] {
            query(sql).execute(&pool).await.unwrap();
        }
        let sources = vec![("a".into(), "rev-a".into()), ("b".into(), "rev-b".into())];
        let mut tx = pool.begin().await.unwrap();
        assert!(
            lock_marker_sources(&mut tx, "lib", "a", &sources)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        query("UPDATE automation_observations SET fingerprint='new' WHERE asset_id='b'")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            !lock_marker_sources(&mut tx, "lib", "a", &sources)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            !lock_marker_sources(&mut tx, "other", "a", &sources)
                .await
                .unwrap()
        );
        assert!(
            !lock_marker_sources(&mut tx, "lib", "b", &sources)
                .await
                .unwrap()
        );
        tx.rollback().await.unwrap();
    }
}

#[cfg(test)]
mod provider_cache_cleanup_tests {
    use super::*;

    #[tokio::test]
    async fn cleanup_is_bounded_and_wraps_without_deleting_live_entries() {
        let temp = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&temp.path().join("cache.sqlite"))
            .await
            .unwrap();
        for index in 0..130 {
            query("INSERT INTO automation_provider_cache(cache_key,provider,operation,query_hash,response_json,expires_at,created_at) VALUES(?, 'fixture', 'search', 'hash', '{}', ?, 0)")
                .bind(format!("key-{index:03}")).bind(if index % 2 == 0 { 10i64 } else { 100i64 }).execute(store.pool()).await.unwrap();
        }
        let mut cursor = String::new();
        assert_eq!(
            purge_provider_cache_batch(&store, &mut cursor, 20)
                .await
                .unwrap(),
            32
        );
        assert_eq!(cursor, "key-063");
        assert_eq!(
            purge_provider_cache_batch(&store, &mut cursor, 20)
                .await
                .unwrap(),
            32
        );
        assert_eq!(
            purge_provider_cache_batch(&store, &mut cursor, 20)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            purge_provider_cache_batch(&store, &mut cursor, 20)
                .await
                .unwrap(),
            0
        );
        assert!(cursor.is_empty());
        let remaining: i64 = query_scalar("SELECT COUNT(*) FROM automation_provider_cache")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(remaining, 65);
        assert_eq!(
            purge_provider_cache_batch(&store, &mut cursor, 200)
                .await
                .unwrap(),
            64
        );
        store.close().await;
    }
}
