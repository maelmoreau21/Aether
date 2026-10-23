use media_poc::{
    database::query,
    identity::IdentityStore,
    inventory::{CatalogueFilter, catalogue_filtered, catalogue_page},
};
#[tokio::test]
async fn authenticated_catalogue_10000_entries() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("bench.sqlite"))
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('reader','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('bench','Bench','films','local','/bench',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('bench','reader')")
        .execute(store.pool())
        .await
        .unwrap();
    query("WITH RECURSIVE nums(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM nums WHERE n<10000) INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json) SELECT 'w'||n,'bench','w'||n,'Film '||n,'film '||n,'film','{}' FROM nums").execute(store.pool()).await.unwrap();
    for sql in [
        "INSERT INTO inventory_editions(id,work_id) SELECT id,id FROM inventory_works",
        "INSERT INTO inventory_versions(id,edition_id) SELECT id,id FROM inventory_works",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) SELECT id,id,'bench','/bench' FROM inventory_works",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) SELECT id,id,'bench',id||'.mp4',1,'0','video/mp4','{}',1 FROM inventory_works",
    ] {
        query(sql).execute(store.pool()).await.unwrap();
    }
    let filter = CatalogueFilter {
        user_id: Some("reader".into()),
        ..Default::default()
    };
    for term in ["", "999"] {
        let mut times = vec![];
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let page = catalogue_filtered(
                &store,
                &["bench".into()],
                Some("film"),
                term,
                "title-asc",
                0,
                24,
                &filter,
            )
            .await
            .unwrap();
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(page["total"], if term.is_empty() { 10000 } else { 19 });
        }
        println!("authenticated catalogue query={term:?} milliseconds={times:?}");
    }
    for (offset, expected_len, has_more) in [(0, 24, true), (9990, 10, false), (10000, 0, false)] {
        let start = std::time::Instant::now();
        let page = catalogue_page(
            &store,
            &["bench".into()],
            Some("film"),
            "",
            "title-asc",
            offset,
            24,
            &filter,
            false,
        )
        .await
        .unwrap();
        println!(
            "progressive catalogue offset={offset} milliseconds={}",
            start.elapsed().as_secs_f64() * 1000.0
        );
        assert!(page["total"].is_null());
        assert_eq!(page["items"].as_array().unwrap().len(), expected_len);
        assert_eq!(page["hasMore"], has_more);
        assert!(serde_json::to_vec(&page).unwrap().len() < 64 * 1024);
    }
    query("UPDATE inventory_assets SET available=0 WHERE id='w1'")
        .execute(store.pool())
        .await
        .unwrap();
    let page = catalogue_page(
        &store,
        &["bench".into()],
        Some("film"),
        "Film 1",
        "title-asc",
        0,
        24,
        &filter,
        false,
    )
    .await
    .unwrap();
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != "w1")
    );
}
