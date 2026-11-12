//! The catalogue response cache must be shared by every clone of one store.
//!
//! Axum's `Extension<IdentityStore>` extractor hands each request its own clone, stored
//! inside that request's heap-allocated handler future. These tests reproduce that shape
//! (one boxed clone per request, kept alive as concurrent home-page shelves would be) on
//! synthetic SQLite catalogues of several sizes. Timings are printed for comparison
//! between builds on the same machine; they are not NAS measurements.

use aether_core::{
    database::query,
    identity::IdentityStore,
    inventory::{
        CatalogueFilter, catalogue_cache_stats, catalogue_page, invalidate_catalogue_cache,
    },
};
use std::time::Instant;

async fn seeded_store(dir: &std::path::Path, works: usize, title_prefix: &str) -> IdentityStore {
    let store = IdentityStore::open(&dir.join("catalogue.sqlite"))
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('reader','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('bench','Bench','films','local','/bench',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('bench','reader')")
        .execute(store.pool())
        .await
        .unwrap();
    query(
        "WITH RECURSIVE nums(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM nums WHERE n<?) \
         INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,added_at,has_available_asset) \
         SELECT 'w'||n,'bench','w'||n,?||' '||n,lower(?||' '||n),'film','{}',n,1 FROM nums",
    )
    .bind(works as i64)
    .bind(title_prefix)
    .bind(title_prefix)
    .execute(store.pool())
    .await
    .unwrap();
    // Works are seeded as available: the asset insert trigger then has nothing to flip, which
    // keeps seeding linear (it scans every unavailable work per inserted asset otherwise).
    for sql in [
        "INSERT INTO inventory_editions(id,work_id) SELECT id,id FROM inventory_works",
        "INSERT INTO inventory_versions(id,edition_id) SELECT id,id FROM inventory_works",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) SELECT id,id,'bench','/bench' FROM inventory_works",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) SELECT id,id,'bench',id||'.mp4',1,'0','video/mp4','{}',1 FROM inventory_works",
    ] {
        query(sql).execute(store.pool()).await.unwrap();
    }
    store
}

async fn shelf(store: &IdentityStore) -> serde_json::Value {
    let filter = CatalogueFilter {
        user_id: Some("reader".into()),
        ..Default::default()
    };
    catalogue_page(
        store,
        &["bench".into()],
        Some("film"),
        "",
        "added-desc",
        0,
        24,
        &filter,
        false,
    )
    .await
    .unwrap()
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

async fn shared_by_per_request_store_clones() {
    const REQUESTS: usize = 20;
    for works in [1_000usize, 10_000, 50_000] {
        let temp = tempfile::tempdir().unwrap();
        let store = seeded_store(temp.path(), works, "Film").await;
        invalidate_catalogue_cache();

        let mut alive = Vec::with_capacity(REQUESTS);
        let mut first = 0.0;
        let mut repeats = Vec::with_capacity(REQUESTS - 1);
        for request in 0..REQUESTS {
            let per_request = Box::new(store.clone());
            let start = Instant::now();
            let page = shelf(&per_request).await;
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            if request == 0 {
                first = elapsed;
            } else {
                repeats.push(elapsed);
            }
            assert_eq!(page["items"].as_array().unwrap().len(), 24);
            assert_eq!(page["items"][0]["id"], format!("w{works}"));
            alive.push(per_request);
        }
        let (bytes, _, entries) = catalogue_cache_stats();
        println!(
            "catalogue cache works={works} requests={REQUESTS} first_ms={first:.3} \
             repeat_median_ms={:.3} entries={entries} bytes={bytes}",
            median(&mut repeats)
        );
        assert_eq!(
            entries, 1,
            "identical shelves served from distinct store clones must share one cache entry"
        );
    }
}

async fn never_crosses_stores() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first = seeded_store(first_dir.path(), 30, "Alpha").await;
    let second = seeded_store(second_dir.path(), 30, "Beta").await;
    // Both databases were seeded identically, so their cache generations match too.
    let first_page = shelf(&first).await;
    let second_page = shelf(&second).await;
    assert_eq!(first_page["items"][0]["title"], "Alpha 30");
    assert_eq!(second_page["items"][0]["title"], "Beta 30");
    assert_eq!(shelf(&first.clone()).await["items"][0]["title"], "Alpha 30");
    assert_eq!(shelf(&second.clone()).await["items"][0]["title"], "Beta 30");
}

// One test: the response cache is process-wide, so its entry count is only meaningful
// while nothing else in this binary queries the catalogue.
#[tokio::test]
async fn catalogue_cache_scope() {
    shared_by_per_request_store_clones().await;
    never_crosses_stores().await;
}
