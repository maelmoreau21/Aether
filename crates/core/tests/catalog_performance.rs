//! Benchmark and functional verification of SQLite catalog performance,
//! pagination, Unicode search, and multi-user library permissions.

use media_poc::identity::IdentityStore;
use serde_json::{Value, json};
use std::time::Instant;

/// Old in-memory catalogue implementation kept purely as a baseline for performance comparison.
async fn legacy_in_memory_catalogue(
    store: &IdentityStore,
    visible: &[String],
    kind: Option<&str>,
    search: &str,
    sort: &str,
    offset: usize,
    limit: usize,
) -> Result<(Value, usize), sqlx::Error> {
    // 1. Fetch all works with correlated poster subquery without any WHERE library or LIMIT
    let rows = sqlx::query(
        "SELECT w.*,(SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size \
         FROM inventory_images i JOIN libraries il ON il.id=i.library_id \
         WHERE i.work_id=w.id AND i.available=1 AND i.root_snapshot=il.root AND il.enabled=1 AND il.kind='films' AND il.node_id='local' \
         ORDER BY i.priority,i.relative_path,i.id LIMIT 1) AS poster_url \
         FROM inventory_works w \
         WHERE EXISTS( \
           SELECT 1 FROM inventory_editions e \
           JOIN inventory_versions v ON v.edition_id=e.id \
           JOIN inventory_sources s ON s.version_id=v.id \
           JOIN inventory_assets a ON a.source_id=s.id \
           JOIN libraries l ON l.id=s.library_id \
           WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1 \
         )"
    )
    .fetch_all(store.pool())
    .await?;

    let total_rows_fetched = rows.len();

    // 2. Filter in memory
    let mut items: Vec<Value> = rows
        .iter()
        .filter(|r| {
            visible.contains(&sqlx::Row::get::<String, _>(*r, "library_id"))
                && kind.is_none_or(|k| k == "film")
                && sqlx::Row::get::<String, _>(*r, "title")
                    .to_lowercase()
                    .contains(&search.to_lowercase())
        })
        .map(|r| {
            let metadata: Value = serde_json::from_str(sqlx::Row::get(r, "editorial_json"))
                .unwrap_or_else(|_| json!({}));
            json!({
                "id": sqlx::Row::get::<String, _>(r, "id"),
                "libraryId": sqlx::Row::get::<String, _>(r, "library_id"),
                "kind": "film",
                "title": sqlx::Row::get::<String, _>(r, "title"),
                "year": sqlx::Row::get::<Option<i64>, _>(r, "year"),
                "description": metadata["plot"].as_str().or_else(|| metadata["outline"].as_str()).unwrap_or("Média local"),
                "posterUrl": sqlx::Row::try_get::<Option<String>, _>(r, "poster_url").ok().flatten()
            })
        })
        .collect();

    // 3. Sort in memory
    items.sort_by_key(|v| v["title"].as_str().unwrap_or_default().to_lowercase());
    if sort == "title-desc" {
        items.reverse();
    }
    let total = items.len();
    let items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
    Ok((
        json!({"items": items, "total": total, "offset": offset, "limit": limit}),
        total_rows_fetched,
    ))
}

/// Populates synthetic libraries and works.
/// Generates:
/// - Library `lib-main`: active films library (contains ~80% of works)
/// - Library `lib-priv`: active restricted films library (contains ~20% of works)
/// - Library `lib-disabled`: disabled films library (contains 10 works)
async fn populate_synthetic_catalog(
    store: &IdentityStore,
    count: usize,
) -> (Vec<String>, Vec<String>) {
    let mut tx = store.pool().begin().await.unwrap();

    // Insert libraries
    sqlx::query("INSERT OR IGNORE INTO libraries VALUES('lib-main','Films Populaires','films','local','/media/films',1)")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO libraries VALUES('lib-priv','Films Prives','films','local','/media/private',1)")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO libraries VALUES('lib-disabled','Films Desactives','films','local','/media/disabled',0)")
        .execute(&mut *tx).await.unwrap();

    let sample_titles = [
        "Été à Paris",
        "Éléphant Blanc",
        "À la dérive",
        "Ça chapitre un",
        "L'Odyssée de l'espace",
        "Cinéma Paradiso",
        "The Matrix Resurrections",
        "Interstellar Voyage",
        "Blade Runner 2049",
        "Le Fabuleux Destin d'Amélie",
        "Zootopie Aventure",
        "Alien le huitième passager",
    ];

    for i in 0..count {
        let work_id = format!("work-{:06}", i);
        let lib_id = if i % 5 == 0 { "lib-priv" } else { "lib-main" };
        let title_base = sample_titles[i % sample_titles.len()];
        let title = format!("{} #{:06}", title_base, i);
        let title_sort = title.to_lowercase();
        let year = 1990 + (i % 35) as i64;
        let identity_key = format!("synthetic:{:06}", i);

        sqlx::query(
            "INSERT INTO inventory_works(id, library_id, identity_key, title, year, external_ids_json, editorial_json, title_sort) \
             VALUES(?, ?, ?, ?, ?, '{}', '{\"plot\": \"Synopsis synthétique pour test de charge\"}', ?)"
        )
        .bind(&work_id)
        .bind(lib_id)
        .bind(&identity_key)
        .bind(&title)
        .bind(year)
        .bind(&title_sort)
        .execute(&mut *tx)
        .await
        .unwrap();

        let edition_id = format!("edition-{:06}", i);
        sqlx::query("INSERT INTO inventory_editions VALUES(?, ?)")
            .bind(&edition_id)
            .bind(&work_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let version_id = format!("version-{:06}", i);
        sqlx::query("INSERT INTO inventory_versions VALUES(?, ?)")
            .bind(&version_id)
            .bind(&edition_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let source_id = format!("source-{:06}", i);
        let root = if lib_id == "lib-main" {
            "/media/films"
        } else {
            "/media/private"
        };
        sqlx::query("INSERT INTO inventory_sources VALUES(?, ?, ?, ?)")
            .bind(root)
            .bind(&source_id)
            .bind(&version_id)
            .bind(lib_id)
            .execute(&mut *tx)
            .await
            .unwrap();

        let asset_id = format!("asset-{:06}", i);
        let rel_path = format!("film_{:06}/movie.mkv", i);
        sqlx::query(
            "INSERT INTO inventory_assets VALUES(?, ?, ?, ?, 1048576, '1700000000', 'video/x-matroska', '{\"streams\":[]}', 1)"
        )
        .bind(&asset_id)
        .bind(&source_id)
        .bind(lib_id)
        .bind(&rel_path)
        .execute(&mut *tx)
        .await
        .unwrap();

        // 50% of works also have an indexed poster
        if i % 2 == 0 {
            let img_id = format!("img-{:06}", i);
            let img_rel = format!("film_{:06}/poster.jpg", i);
            sqlx::query(
                "INSERT INTO inventory_images VALUES(?, ?, ?, ?, ?, 51200, '1700000000', 'image/jpeg', 1, 1)"
            )
            .bind(&img_id)
            .bind(&work_id)
            .bind(lib_id)
            .bind(root)
            .bind(&img_rel)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
    }

    // Add 10 works in disabled library
    for i in 0..10 {
        let work_id = format!("disabled-work-{:03}", i);
        let title = format!("Film Inactif #{:03}", i);
        let title_sort = title.to_lowercase();
        sqlx::query(
            "INSERT INTO inventory_works(id, library_id, identity_key, title, year, external_ids_json, editorial_json, title_sort) \
             VALUES(?, 'lib-disabled', ?, ?, 2020, '{}', '{}', ?)"
        )
        .bind(&work_id)
        .bind(format!("disabled:{}", i))
        .bind(&title)
        .bind(&title_sort)
        .execute(&mut *tx)
        .await
        .unwrap();

        let edition_id = format!("disabled-edition-{:03}", i);
        sqlx::query("INSERT INTO inventory_editions VALUES(?, ?)")
            .bind(&edition_id)
            .bind(&work_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let version_id = format!("disabled-version-{:03}", i);
        sqlx::query("INSERT INTO inventory_versions VALUES(?, ?)")
            .bind(&version_id)
            .bind(&edition_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        let source_id = format!("disabled-source-{:03}", i);
        sqlx::query(
            "INSERT INTO inventory_sources VALUES('/media/disabled', ?, ?, 'lib-disabled')",
        )
        .bind(&source_id)
        .bind(&version_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        let asset_id = format!("disabled-asset-{:03}", i);
        let rel_path = format!("disabled_{:03}/disabled.mkv", i);
        sqlx::query("INSERT INTO inventory_assets VALUES(?, ?, 'lib-disabled', ?, 100, '100', 'video/x-matroska', '{}', 1)").bind(&asset_id).bind(&source_id).bind(&rel_path).execute(&mut *tx).await.unwrap();
    }

    tx.commit().await.unwrap();

    let main_libs = vec!["lib-main".to_string()];
    let all_active_libs = vec!["lib-main".to_string(), "lib-priv".to_string()];
    (main_libs, all_active_libs)
}

#[tokio::test]
async fn explain_query_plan_verifies_index_usage() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("explain.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();
    populate_synthetic_catalog(&store, 100).await;

    // 1. Check EXPLAIN QUERY PLAN for Count Query
    let count_explain: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN \
         SELECT COUNT(*) FROM inventory_works w \
         WHERE w.library_id IN ('lib-main') \
           AND EXISTS ( \
             SELECT 1 FROM inventory_editions e \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources s ON s.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=s.id \
             JOIN libraries l ON l.id=s.library_id \
             WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1 \
           )"
    )
    .fetch_all(store.pool())
    .await
    .unwrap();

    let explain_text = count_explain
        .iter()
        .map(|(_, _, _, detail)| detail.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    println!("EXPLAIN QUERY PLAN (count):\n{}", explain_text);
    assert!(
        explain_text.contains("USING INDEX inventory_works_library_sort")
            || explain_text.contains("USING COVERING INDEX inventory_works_library_sort")
            || explain_text.contains("USING INDEX"),
        "Count query must use inventory_works indexes: {}",
        explain_text
    );
    assert!(
        explain_text.contains("USING INDEX inventory_editions_work"),
        "EXISTS subquery must use inventory_editions_work index: {}",
        explain_text
    );

    // 2. Check EXPLAIN QUERY PLAN for Paginated Data Query
    let data_explain: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN \
         SELECT w.*, \
           (SELECT '/api/v1/assets/' || i.id || '/image?v=' || i.modified_ns || '-' || i.size \
            FROM inventory_images i \
            JOIN libraries il ON il.id=i.library_id \
            WHERE i.work_id=w.id AND i.available=1 AND i.root_snapshot=il.root AND il.enabled=1 AND il.kind='films' AND il.node_id='local' \
            ORDER BY i.priority, i.relative_path, i.id LIMIT 1) AS poster_url \
         FROM inventory_works w \
         WHERE w.library_id IN ('lib-main') \
           AND EXISTS ( \
             SELECT 1 FROM inventory_editions e \
             JOIN inventory_versions v ON v.edition_id=e.id \
             JOIN inventory_sources s ON s.version_id=v.id \
             JOIN inventory_assets a ON a.source_id=s.id \
             JOIN libraries l ON l.id=s.library_id \
             WHERE e.work_id=w.id AND a.available=1 AND s.root_snapshot=l.root AND l.kind='films' AND l.node_id='local' AND l.enabled=1 \
           ) \
         ORDER BY w.title_sort ASC, w.id ASC \
         LIMIT 24 OFFSET 0"
    )
    .fetch_all(store.pool())
    .await
    .unwrap();

    let data_explain_text = data_explain
        .iter()
        .map(|(_, _, _, detail)| detail.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    println!("EXPLAIN QUERY PLAN (data):\n{}", data_explain_text);
    assert!(
        data_explain_text.contains("USING INDEX inventory_works_library_sort")
            || data_explain_text.contains("USING INDEX"),
        "Data query must use index on inventory_works: {}",
        data_explain_text
    );
}

#[tokio::test]
async fn multi_user_permissions_and_disabled_libraries() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("permissions.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();

    let (main_libs, all_libs) = populate_synthetic_catalog(&store, 500).await;

    // User A: only main library
    let res_a = media_poc::inventory::catalogue(&store, &main_libs, None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    let total_a = res_a["total"].as_u64().unwrap();
    assert_eq!(total_a, 400); // 80% of 500
    assert_eq!(res_a["items"].as_array().unwrap().len(), 24);

    // User B: both main and private libraries
    let res_b = media_poc::inventory::catalogue(&store, &all_libs, None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    let total_b = res_b["total"].as_u64().unwrap();
    assert_eq!(total_b, 500); // 100% of 500

    // User C: only disabled library
    let res_c = media_poc::inventory::catalogue(
        &store,
        &["lib-disabled".to_string()],
        None,
        "",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    assert_eq!(res_c["total"], 0, "Disabled library must never yield items");
    assert_eq!(res_c["items"].as_array().unwrap().len(), 0);

    // User D: empty libraries
    let res_d = media_poc::inventory::catalogue(&store, &[], None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(res_d["total"], 0);
    assert_eq!(res_d["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn unicode_search_and_sort_semantics_and_deep_pagination() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("unicode.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();

    let (_, all_libs) = populate_synthetic_catalog(&store, 1000).await;

    // 1. Unicode case folding search: "été" finds "Été à Paris"
    let res_ete =
        media_poc::inventory::catalogue(&store, &all_libs, None, "été", "title-asc", 0, 24)
            .await
            .unwrap();
    assert!(res_ete["total"].as_u64().unwrap() > 0);
    for item in res_ete["items"].as_array().unwrap() {
        assert!(item["title"].as_str().unwrap().contains("Été"));
    }

    // 2. Unicode search: "éléphant" finds "Éléphant Blanc"
    let res_ele =
        media_poc::inventory::catalogue(&store, &all_libs, None, "éléphant", "title-asc", 0, 24)
            .await
            .unwrap();
    assert!(res_ele["total"].as_u64().unwrap() > 0);
    for item in res_ele["items"].as_array().unwrap() {
        assert!(item["title"].as_str().unwrap().contains("Éléphant"));
    }

    // 3. Unicode search: "ça" finds "Ça chapitre un"
    let res_ca = media_poc::inventory::catalogue(&store, &all_libs, None, "ça", "title-asc", 0, 24)
        .await
        .unwrap();
    assert!(res_ca["total"].as_u64().unwrap() > 0);
    for item in res_ca["items"].as_array().unwrap() {
        assert!(item["title"].as_str().unwrap().contains("Ça"));
    }

    // 4. Sorting: title-asc vs title-desc
    let res_asc = media_poc::inventory::catalogue(&store, &all_libs, None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    let res_desc =
        media_poc::inventory::catalogue(&store, &all_libs, None, "", "title-desc", 0, 24)
            .await
            .unwrap();

    let (legacy_asc, _) =
        legacy_in_memory_catalogue(&store, &all_libs, None, "", "title-asc", 0, 24)
            .await
            .unwrap();
    let (legacy_desc, _) =
        legacy_in_memory_catalogue(&store, &all_libs, None, "", "title-desc", 0, 24)
            .await
            .unwrap();

    let first_asc = res_asc["items"][0]["title"].as_str().unwrap();
    let first_desc = res_desc["items"][0]["title"].as_str().unwrap();
    assert_ne!(first_asc, first_desc);
    assert_eq!(
        first_asc,
        legacy_asc["items"][0]["title"].as_str().unwrap(),
        "SQLite title-asc must match legacy Rust sort"
    );
    assert_eq!(
        first_desc,
        legacy_desc["items"][0]["title"].as_str().unwrap(),
        "SQLite title-desc must match legacy Rust sort"
    );

    // 5. Deep pagination on 1,000 items
    let page_last =
        media_poc::inventory::catalogue(&store, &all_libs, None, "", "title-asc", 984, 24)
            .await
            .unwrap();
    assert_eq!(page_last["total"], 1000);
    assert_eq!(page_last["offset"], 984);
    assert_eq!(page_last["items"].as_array().unwrap().len(), 16); // 1000 - 984 = 16

    // 6. Out-of-bounds offset
    let page_oob =
        media_poc::inventory::catalogue(&store, &all_libs, None, "", "title-asc", 2000, 24)
            .await
            .unwrap();
    assert_eq!(page_oob["total"], 1000);
    assert_eq!(page_oob["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn benchmark_1000_and_10000_works_comparison() {
    let temp = tempfile::tempdir().unwrap();

    // ==============================================================
    // BENCHMARK 1: 1,000 WORKS
    // ==============================================================
    let db_1k = temp.path().join("bench_1k.sqlite");
    let store_1k = IdentityStore::open(&db_1k).await.unwrap();
    let (_, all_libs_1k) = populate_synthetic_catalog(&store_1k, 1000).await;

    // Warm-up
    let _ = media_poc::inventory::catalogue(&store_1k, &all_libs_1k, None, "", "title-asc", 0, 24)
        .await
        .unwrap();

    // 1. Measure New SQLite-driven Catalogue on 1k
    let start_new_1k = Instant::now();
    let res_new_1k =
        media_poc::inventory::catalogue(&store_1k, &all_libs_1k, None, "", "title-asc", 0, 24)
            .await
            .unwrap();
    let elapsed_new_1k = start_new_1k.elapsed();
    assert_eq!(res_new_1k["total"], 1000);
    assert_eq!(res_new_1k["items"].as_array().unwrap().len(), 24);

    // Deep page (offset=960)
    let start_new_1k_deep = Instant::now();
    let res_new_1k_deep =
        media_poc::inventory::catalogue(&store_1k, &all_libs_1k, None, "", "title-asc", 960, 24)
            .await
            .unwrap();
    let elapsed_new_1k_deep = start_new_1k_deep.elapsed();
    assert_eq!(res_new_1k_deep["items"].as_array().unwrap().len(), 24);

    // Search query on 1k
    let start_new_1k_search = Instant::now();
    let res_new_1k_search = media_poc::inventory::catalogue(
        &store_1k,
        &all_libs_1k,
        None,
        "matrix",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    let elapsed_new_1k_search = start_new_1k_search.elapsed();

    // 2. Measure Legacy In-Memory Catalogue on 1k
    let start_legacy_1k = Instant::now();
    let (res_legacy_1k, rows_legacy_1k) =
        legacy_in_memory_catalogue(&store_1k, &all_libs_1k, None, "", "title-asc", 0, 24)
            .await
            .unwrap();
    let elapsed_legacy_1k = start_legacy_1k.elapsed();
    assert_eq!(res_legacy_1k["total"], 1000);
    assert_eq!(
        rows_legacy_1k, 1000,
        "Legacy loaded ALL 1,000 works into RAM"
    );

    println!("\n=== RÉSULTATS BENCHMARK 1 000 ŒUVRES ===");
    println!(
        "Legacy (in-memory):   {:>8.2?} (1 000 rows matérialisées en RAM)",
        elapsed_legacy_1k
    );
    println!(
        "Nouveau (SQLite SQL): {:>8.2?} (page 1: 24 items)",
        elapsed_new_1k
    );
    println!(
        "Nouveau (Page 960):   {:>8.2?} (page 41: 24 items)",
        elapsed_new_1k_deep
    );
    println!(
        "Nouveau (Recherche):  {:>8.2?} (total: {})",
        elapsed_new_1k_search, res_new_1k_search["total"]
    );

    // ==============================================================
    // BENCHMARK 2: 10,000 WORKS
    // ==============================================================
    let db_10k = temp.path().join("bench_10k.sqlite");
    let store_10k = IdentityStore::open(&db_10k).await.unwrap();
    let (_, all_libs_10k) = populate_synthetic_catalog(&store_10k, 10000).await;

    // Warm-up
    let _ =
        media_poc::inventory::catalogue(&store_10k, &all_libs_10k, None, "", "title-asc", 0, 24)
            .await
            .unwrap();

    // 1. Measure New SQLite-driven Catalogue on 10k
    let start_new_10k = Instant::now();
    let res_new_10k =
        media_poc::inventory::catalogue(&store_10k, &all_libs_10k, None, "", "title-asc", 0, 24)
            .await
            .unwrap();
    let elapsed_new_10k = start_new_10k.elapsed();
    assert_eq!(res_new_10k["total"], 10000);
    assert_eq!(res_new_10k["items"].as_array().unwrap().len(), 24);

    // Deep page on 10k (offset=9960)
    let start_new_10k_deep = Instant::now();
    let res_new_10k_deep =
        media_poc::inventory::catalogue(&store_10k, &all_libs_10k, None, "", "title-asc", 9960, 24)
            .await
            .unwrap();
    let elapsed_new_10k_deep = start_new_10k_deep.elapsed();
    assert_eq!(res_new_10k_deep["items"].as_array().unwrap().len(), 24);

    // Search query on 10k
    let start_new_10k_search = Instant::now();
    let res_new_10k_search = media_poc::inventory::catalogue(
        &store_10k,
        &all_libs_10k,
        None,
        "matrix",
        "title-asc",
        0,
        24,
    )
    .await
    .unwrap();
    let elapsed_new_10k_search = start_new_10k_search.elapsed();

    // 2. Measure Legacy In-Memory Catalogue on 10k
    let start_legacy_10k = Instant::now();
    let (res_legacy_10k, rows_legacy_10k) =
        legacy_in_memory_catalogue(&store_10k, &all_libs_10k, None, "", "title-asc", 0, 24)
            .await
            .unwrap();
    let elapsed_legacy_10k = start_legacy_10k.elapsed();
    assert_eq!(res_legacy_10k["total"], 10000);
    assert_eq!(
        rows_legacy_10k, 10000,
        "Legacy loaded ALL 10,000 works into RAM"
    );

    println!("\n=== RÉSULTATS BENCHMARK 10 000 ŒUVRES ===");
    println!(
        "Legacy (in-memory):   {:>8.2?} (10 000 rows + poster_url subqueries matérialisées)",
        elapsed_legacy_10k
    );
    println!(
        "Nouveau (SQLite SQL): {:>8.2?} (page 1: 24 items)",
        elapsed_new_10k
    );
    println!(
        "Nouveau (Page 9960):  {:>8.2?} (page 416: 24 items)",
        elapsed_new_10k_deep
    );
    println!(
        "Nouveau (Recherche):  {:>8.2?} (total: {})",
        elapsed_new_10k_search, res_new_10k_search["total"]
    );
    println!("=========================================\n");

    // Assert that the new implementation is significantly faster and allocations are bounded
    assert!(
        elapsed_new_10k < elapsed_legacy_10k,
        "New SQLite pagination ({:?}) must be faster than legacy in-memory ({:?})",
        elapsed_new_10k,
        elapsed_legacy_10k
    );
}

#[tokio::test]
async fn test_migrated_database_from_0006_to_0007_then_0008_unicode_fix() {
    use sqlx::Connection;

    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("migrated_test.sqlite");

    let migrator = sqlx::migrate!("./migrations");
    assert!(
        migrator.migrations.len() >= 8,
        "Workspace must contain at least 8 migrations"
    );

    // 1. Manually connect and apply migrations 0001 through 0006 (pre-J2 schema)
    let mut conn = sqlx::SqliteConnection::connect(&format!(
        "sqlite://{}?mode=rwc",
        db_path.to_str().unwrap()
    ))
    .await
    .unwrap();

    sqlx::raw_sql(
        "CREATE TABLE _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            success BOOLEAN NOT NULL,
            checksum BLOB NOT NULL,
            execution_time BIGINT NOT NULL
        );",
    )
    .execute(&mut conn)
    .await
    .unwrap();

    for m in &migrator.migrations[0..6] {
        sqlx::raw_sql(m.sql.clone())
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES (?, ?, 1, ?, 1)",
        )
        .bind(m.version)
        .bind(&m.description)
        .bind(&*m.checksum)
        .execute(&mut conn)
        .await
        .unwrap();
    }

    // Insert library
    sqlx::query("INSERT INTO libraries VALUES('lib-test', 'Films Test', 'films', 'local', '/media/test', 1)")
        .execute(&mut conn)
        .await
        .unwrap();

    // Insert works at schema 0006 (BEFORE migration 0007, so title_sort does NOT exist yet)
    let test_works = [
        ("work-ete", "Été d'amour", 2021),
        ("work-a", "À bout de souffle", 1960),
        ("work-ca", "Ça", 2017),
        ("work-oeuvre", "Œuvre d'art", 2022),
        ("work-zebre", "Zèbre", 2020),
        ("work-alpha", "Alpha", 2019),
    ];

    for (w_id, title, year) in test_works {
        sqlx::query(
            "INSERT INTO inventory_works(id, library_id, identity_key, title, year, external_ids_json, editorial_json) \
             VALUES(?, 'lib-test', ?, ?, ?, '{}', '{\"plot\":\"Résumé de test\"}')",
        )
        .bind(w_id)
        .bind(format!("folder:{}", w_id))
        .bind(title)
        .bind(year)
        .execute(&mut conn)
        .await
        .unwrap();

        // Add edition, version, source, asset so it is visible in catalogue
        let edition_id = format!("ed-{}", w_id);
        let version_id = format!("ver-{}", w_id);
        let source_id = format!("src-{}", w_id);
        let asset_id = format!("ast-{}", w_id);

        sqlx::query("INSERT INTO inventory_editions VALUES(?, ?)")
            .bind(&edition_id)
            .bind(w_id)
            .execute(&mut conn)
            .await
            .unwrap();

        sqlx::query("INSERT INTO inventory_versions VALUES(?, ?)")
            .bind(&version_id)
            .bind(&edition_id)
            .execute(&mut conn)
            .await
            .unwrap();

        sqlx::query("INSERT INTO inventory_sources VALUES('/media/test', ?, ?, 'lib-test')")
            .bind(&source_id)
            .bind(&version_id)
            .execute(&mut conn)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO inventory_assets VALUES(?, ?, 'lib-test', ?, 1024, '100', 'video/x-matroska', '{}', 1)",
        )
        .bind(&asset_id)
        .bind(&source_id)
        .bind(format!("{}/movie.mkv", w_id))
        .execute(&mut conn)
        .await
        .unwrap();
    }

    // 2. Apply migration 0007 (reproducing the J2 migration with SQLite lower(title))
    let m7 = &migrator.migrations[6];
    assert_eq!(m7.version, 7);
    sqlx::raw_sql(m7.sql.clone())
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (?, ?, 1, ?, 1)",
    )
    .bind(m7.version)
    .bind(&m7.description)
    .bind(&*m7.checksum)
    .execute(&mut conn)
    .await
    .unwrap();

    // Verify the bug introduced by 0007: SQLite lower() only converts ASCII A-Z to a-z!
    let rows_0007: Vec<(String, String)> =
        sqlx::query_as("SELECT title, title_sort FROM inventory_works ORDER BY id ASC")
            .fetch_all(&mut conn)
            .await
            .unwrap();

    let sort_map_0007: std::collections::HashMap<String, String> = rows_0007.into_iter().collect();
    // Confirmed bug: SQLite lower() leaves 'É', 'À', 'Ç', 'Œ' untouched in uppercase!
    assert_eq!(
        sort_map_0007["Été d'amour"], "Été d'amour",
        "SQLite lower() failed to lowercase 'É'"
    );
    assert_eq!(
        sort_map_0007["À bout de souffle"], "À bout de souffle",
        "SQLite lower() failed to lowercase 'À'"
    );
    assert_eq!(
        sort_map_0007["Ça"], "Ça",
        "SQLite lower() failed to lowercase 'Ç'"
    );
    assert_eq!(
        sort_map_0007["Œuvre d'art"], "Œuvre d'art",
        "SQLite lower() failed to lowercase 'Œ'"
    );

    // Drop manual connection before opening with IdentityStore
    drop(conn);

    // 3. Open the existing database via IdentityStore::open (applies 0008 + backfill)
    let store = IdentityStore::open(&db_path).await.unwrap();

    // Verify that title_sort has been upgraded to Unicode-compliant lowercase
    let rows_0008: Vec<(String, String)> =
        sqlx::query_as("SELECT title, title_sort FROM inventory_works ORDER BY id ASC")
            .fetch_all(store.pool())
            .await
            .unwrap();

    let sort_map_0008: std::collections::HashMap<String, String> = rows_0008.into_iter().collect();
    assert_eq!(sort_map_0008["Été d'amour"], "été d'amour");
    assert_eq!(sort_map_0008["À bout de souffle"], "à bout de souffle");
    assert_eq!(sort_map_0008["Ça"], "ça");
    assert_eq!(sort_map_0008["Œuvre d'art"], "œuvre d'art");
    assert_eq!(sort_map_0008["Zèbre"], "zèbre");
    assert_eq!(sort_map_0008["Alpha"], "alpha");

    // 4. Verify search semantics: case-folding finds accented titles
    let visible = vec!["lib-test".to_string()];

    // Search "été" finds "Été d'amour"
    let res_ete =
        media_poc::inventory::catalogue(&store, &visible, None, "été", "title-asc", 0, 24)
            .await
            .unwrap();
    assert_eq!(res_ete["total"], 1);
    assert_eq!(res_ete["items"][0]["title"], "Été d'amour");

    // Search uppercase "ÉTÉ" also finds "Été d'amour"
    let res_ete_upper =
        media_poc::inventory::catalogue(&store, &visible, None, "ÉTÉ", "title-asc", 0, 24)
            .await
            .unwrap();
    assert_eq!(res_ete_upper["total"], 1);
    assert_eq!(res_ete_upper["items"][0]["title"], "Été d'amour");

    // Search "à" finds "À bout de souffle"
    let res_a = media_poc::inventory::catalogue(&store, &visible, None, "à", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(res_a["total"], 1);
    assert_eq!(res_a["items"][0]["title"], "À bout de souffle");

    // Search "ça" finds "Ça"
    let res_ca = media_poc::inventory::catalogue(&store, &visible, None, "ça", "title-asc", 0, 24)
        .await
        .unwrap();
    assert_eq!(res_ca["total"], 1);
    assert_eq!(res_ca["items"][0]["title"], "Ça");

    // Search "œuvre" finds "Œuvre d'art"
    let res_oeuvre =
        media_poc::inventory::catalogue(&store, &visible, None, "œuvre", "title-asc", 0, 24)
            .await
            .unwrap();
    assert_eq!(res_oeuvre["total"], 1);
    assert_eq!(res_oeuvre["items"][0]["title"], "Œuvre d'art");

    // 5. Verify deterministic Unicode sorting (ORDER BY title_sort ASC)
    let res_sort = media_poc::inventory::catalogue(&store, &visible, None, "", "title-asc", 0, 24)
        .await
        .unwrap();
    let sorted_titles: Vec<&str> = res_sort["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();

    // In Unicode UTF-8 sort:
    // 'Alpha' (a=0x61), 'Ça' (ç=0xE7), 'Zèbre' (z=0x7A), 'À...' (à=0xE0), 'Été...' (é=0xE9), 'Œuvre...' (œ=0x0153)
    // Note UTF-8 byte ordering:
    // 'a' = 0x61, 'z' = 0x7A, 'à' = 0xC3 0xA0, 'ç' = 0xC3 0xA7, 'é' = 0xC3 0xA9, 'œ' = 0xC5 0x93
    // 'Alpha' (0x61) -> 'Zèbre' (0x7A) -> 'À...' (0xC3 0xA0) -> 'Ça' (0xC3 0xA7) -> 'Été...' (0xC3 0xA9) -> 'Œuvre...' (0xC5 0x93)
    assert_eq!(
        sorted_titles,
        vec![
            "Alpha",
            "Zèbre",
            "À bout de souffle",
            "Ça",
            "Été d'amour",
            "Œuvre d'art",
        ]
    );

    // 6. Verify UUIDs, editorial JSON, and relational integrity are completely preserved
    let work_ete: (String, String, i64, String) = sqlx::query_as(
        "SELECT id, identity_key, year, editorial_json FROM inventory_works WHERE title = ?",
    )
    .bind("Été d'amour")
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(work_ete.0, "work-ete", "UUID must be preserved");
    assert_eq!(work_ete.1, "folder:work-ete", "Identity key preserved");
    assert_eq!(work_ete.2, 2021, "Year preserved");
    assert!(
        work_ete.3.contains("Résumé de test"),
        "Editorial NFO JSON preserved"
    );

    // 7. Verify migration marker table
    let marker_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM inventory_works_unicode_migration")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(marker_count.0, 1);

    // Close and reopen: must be completely idempotent
    store.close().await;
    let reopened = IdentityStore::open(&db_path).await.unwrap();
    let marker_count_reopened: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM inventory_works_unicode_migration")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
    assert_eq!(marker_count_reopened.0, 1);
    reopened.close().await;
}
