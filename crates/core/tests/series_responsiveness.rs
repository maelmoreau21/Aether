use media_poc::{
    database::query,
    identity::IdentityStore,
    inventory::{EpisodePage, detail_window},
};

#[tokio::test]
async fn series_pages_bound_10000_episodes_and_skip_forbidden_before_paging() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("series.sqlite"))
        .await
        .unwrap();
    for sql in [
        "INSERT INTO users(id,role) VALUES('reader','user')",
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('bench','Bench','series','local','/bench',1)",
        "INSERT INTO library_permissions(library_id,user_id) VALUES('bench','reader')",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,minimum_age) VALUES('show','bench','show','Show','show','series','{}',0)",
        "INSERT INTO inventory_seasons(id,series_id,season_number,title) VALUES('s1','show',1,'First'),('s2','show',2,'Second')",
        "WITH RECURSIVE nums(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM nums WHERE n<10000) INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,minimum_age) SELECT 'e'||n,'bench','e'||n,'Episode '||n,'episode '||n,'episode','{}',0 FROM nums",
        "INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) SELECT id,CASE WHEN CAST(substr(id,2) AS INTEGER)<=5000 THEN 's1' ELSE 's2' END,CAST(substr(id,2) AS INTEGER),id FROM inventory_works WHERE kind='episode'",
        "INSERT INTO inventory_editions(id,work_id) SELECT id,id FROM inventory_works WHERE kind='episode'",
        "INSERT INTO inventory_versions(id,edition_id) SELECT id,id FROM inventory_editions",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) SELECT id,id,'bench','/bench' FROM inventory_versions",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) SELECT id,id,'bench',id||'.mp4',1,'0','video/mp4','{}',1 FROM inventory_sources",
        "INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('reader',12,1)",
        "UPDATE inventory_works SET minimum_age=18 WHERE id='e25'",
    ] {
        query(sql).execute(store.pool()).await.unwrap();
    }
    let mut page = EpisodePage {
        season: None,
        offset: 0,
        limit: 24,
        after: None,
        user: "reader".into(),
        role: "user".into(),
    };
    let start = std::time::Instant::now();
    let initial = detail_window(&store, "show", Some(&page))
        .await
        .unwrap()
        .unwrap();
    println!(
        "10k episode initial page: {}ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(
        initial["seasons"][0]["episodes"].as_array().unwrap().len(),
        24
    );
    assert!(
        initial["seasons"][1]["episodes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(initial["episodeCount"], 9999);
    assert_eq!(initial["episodesPage"]["nextOffset"], 24);
    assert_eq!(initial["episodesPage"]["hasMore"], true);
    assert!(serde_json::to_vec(&initial).unwrap().len() < 64 * 1024);
    page.offset = 24;
    let second = detail_window(&store, "show", Some(&page))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second["seasons"][0]["episodes"][0]["id"], "e26");
    page.offset = 0;
    page.after = Some("e24".into());
    page.limit = 1;
    let next = detail_window(&store, "show", Some(&page))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next["seasons"][0]["episodes"][0]["id"], "e26");
    page.after = Some("e5000".into());
    let boundary = detail_window(&store, "show", Some(&page))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(boundary["seasons"][1]["episodes"][0]["id"], "e5001");
    assert_eq!(boundary["episodesPage"]["season"], 2);
    page.after = None;
    page.season = Some(2);
    page.offset = 4999;
    let end = detail_window(&store, "show", Some(&page))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(end["seasons"][1]["episodes"][0]["id"], "e10000");
    assert_eq!(end["episodesPage"]["hasMore"], false);
    query("DELETE FROM library_permissions WHERE user_id='reader'")
        .execute(store.pool())
        .await
        .unwrap();
    let forbidden = detail_window(&store, "show", Some(&page))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(forbidden["episodeCount"], 0);
    assert!(forbidden["seasons"].as_array().unwrap().is_empty());
}
