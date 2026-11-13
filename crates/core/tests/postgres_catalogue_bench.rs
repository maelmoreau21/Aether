//! Opt-in PostgreSQL timing of the screens a viewer opens first: home shelves, a film's page and
//! its player descriptor, through the real HTTP stack (authentication and permissions included).
//! Run with `MEDIA_TEST_POSTGRES_URL=... cargo test --test postgres_catalogue_bench -- --ignored --nocapture`.
use aether_core::{auth::AuthState, database::query, identity::IdentityStore};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use sha2::{Digest, Sha256};
use std::time::Instant;
use tower::ServiceExt;

const FILMS: i64 = 30_000;
const SERIES: i64 = 1_000;
const EPISODES_PER_SERIES: i64 = 20;

async fn get(app: &Router, path: &str, token: &str) -> (StatusCode, serde_json::Value, f64) {
    let start = Instant::now();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("host", "127.0.0.1:3000")
                .header("cookie", format!("aether_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        elapsed,
    )
}

async fn seed(store: &IdentityStore) {
    let token_hash = format!("{:x}", Sha256::digest("b".repeat(64).as_bytes()));
    for sql in [
        "INSERT INTO users(id,role,display_name) VALUES('reader','user','Reader')".to_owned(),
        format!("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES('{token_hash}','reader',unixepoch(),unixepoch(),unixepoch()+3600)"),
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lf','Films','films','local','/films',1),('ls','Series','series','local','/series',1)".to_owned(),
        // Thousands of accounts reach the libraries through the built-in "users" group.
        "INSERT INTO users(id,role,display_name) SELECT 'u'||n,'user','User '||n FROM generate_series(1,10000) n".to_owned(),
        "INSERT INTO user_groups(id,name,is_default) VALUES('builtin-role-user','Utilisateurs',0) ON CONFLICT DO NOTHING".to_owned(),
        "INSERT INTO user_group_libraries(group_id,library_id) VALUES('builtin-role-user','lf'),('builtin-role-user','ls')".to_owned(),
        format!("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,display_title_sort,kind,external_ids_json,editorial_json,year,effective_year,added_at,has_available_asset,genres_csv) \
            SELECT 'f'||n,'lf','f'||n,'Film '||n,'film '||n,'film '||n,'film','{{}}','{{\"plot\":\"Synopsis\",\"genres\":[\"Drame\"]}}',1950+n%75,1950+n%75,1700000000+n,1,CASE WHEN n%50=0 THEN '|drame|western|' WHEN n%2=0 THEN '|drame|' ELSE '|comédie|' END FROM generate_series(1,{FILMS}) n"),
        format!("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,display_title_sort,kind,external_ids_json,year,effective_year,added_at,has_available_asset,season_count_cached,episode_count_cached) \
            SELECT 's'||n,'ls','s'||n,'Serie '||n,'serie '||n,'serie '||n,'series','{{}}',1990+n%30,1990+n%30,1700000000+n,1,2,{EPISODES_PER_SERIES} FROM generate_series(1,{SERIES}) n"),
        format!("INSERT INTO inventory_seasons(id,series_id,season_number) SELECT 's'||n||'-'||k,'s'||n,k FROM generate_series(1,{SERIES}) n, generate_series(1,2) k"),
        format!("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,display_title_sort,kind,external_ids_json,has_available_asset) \
            SELECT 's'||n||'e'||e,'ls','s'||n||'e'||e,'Episode '||e,'episode '||e,'episode '||e,'episode','{{}}',1 FROM generate_series(1,{SERIES}) n, generate_series(1,{EPISODES_PER_SERIES}) e"),
        format!("INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) SELECT 's'||n||'e'||e,'s'||n||'-'||(1+(e-1)*2/{EPISODES_PER_SERIES}),e,'s'||n||'e'||e FROM generate_series(1,{SERIES}) n, generate_series(1,{EPISODES_PER_SERIES}) e"),
        "INSERT INTO inventory_editions(id,work_id) SELECT id,id FROM inventory_works WHERE kind IN ('film','episode')".to_owned(),
        "INSERT INTO inventory_versions(id,edition_id) SELECT id,id FROM inventory_editions".to_owned(),
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) SELECT e.id,e.id,w.library_id,CASE w.library_id WHEN 'lf' THEN '/films' ELSE '/series' END FROM inventory_editions e JOIN inventory_works w ON w.id=e.work_id".to_owned(),
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) SELECT 'a'||id,id,library_id,id||'.mkv',1000000000,'0','video/x-matroska','{\"format\":{\"duration\":\"5400\"},\"streams\":[{\"index\":0,\"codec_type\":\"video\",\"codec_name\":\"h264\",\"width\":1920,\"height\":1080},{\"index\":1,\"codec_type\":\"audio\",\"codec_name\":\"aac\",\"channels\":2}]}',1 FROM inventory_sources".to_owned(),
        "INSERT INTO inventory_images(id,work_id,library_id,root_snapshot,relative_path,size,modified_ns,mime,priority,available) SELECT 'i'||id,id,library_id,CASE library_id WHEN 'lf' THEN '/films' ELSE '/series' END,id||'/poster.jpg',50000,'0','image/jpeg',1,1 FROM inventory_works WHERE kind IN ('film','series')".to_owned(),
        "INSERT INTO playback_progress(user_id,work_id,position_seconds,duration_seconds,watched,revision,updated_at) SELECT 'reader','f'||n,600,5400,0,1,1700000000+n FROM generate_series(1,200) n".to_owned(),
        "UPDATE inventory_cache_generation SET gen=gen+1 WHERE id=1".to_owned(),
        "ANALYZE".to_owned(),
    ] {
        query(sqlx::AssertSqlSafe(sql.clone()))
            .execute(store.pool())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a disposable PostgreSQL database in MEDIA_TEST_POSTGRES_URL"]
async fn postgres_first_screens_stay_fast() {
    let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL URL");
    let store = IdentityStore::open_postgres(&url).await.unwrap();
    let seeding = Instant::now();
    seed(&store).await;
    println!(
        "seeded in {:.0} ms",
        seeding.elapsed().as_secs_f64() * 1000.0
    );
    let temp = tempfile::tempdir().unwrap();
    let inventory = media_node::Inventory::local(temp.path().to_path_buf())
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let jobs = std::sync::Arc::new(aether_core::scans::ScanJobs::default());
    let app = aether_core::secure_app_with_jobs(inventory, 3000, auth, jobs)
        .await
        .unwrap();
    let token = "b".repeat(64);
    let hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    aether_core::inventory::descriptor_for_user(
        &store,
        "af15000",
        &hash,
        Some("reader"),
        Some(&["lf".to_owned(), "ls".to_owned()]),
    )
    .await
    .expect("player descriptor");
    let reset = std::env::var("BENCH_STATEMENTS").is_ok();

    let home = [
        "/api/v1/auth/me",
        "/api/v1/catalog?scope=latest&kind=film&q=&sort=added-desc&offset=0&limit=24&count=false",
        "/api/v1/catalog?scope=latest&kind=series&q=&sort=added-desc&offset=0&limit=24&count=false",
        "/api/v1/catalog?kind=film&q=&sort=title-asc&offset=0&limit=24&count=false&genre=drame",
        "/api/v1/catalog?kind=film&q=&sort=title-asc&offset=0&limit=24&count=false&genre=western",
        "/api/v1/catalog?kind=film&q=&sort=added-desc&offset=600&limit=30&count=false",
        "/api/v1/catalog?kind=film&q=1234&sort=title-asc&offset=0&limit=24",
        "/api/v1/catalog/f15000?episodeLimit=24&episodeOffset=0",
        "/api/v1/catalog/s500?episodeLimit=24&episodeOffset=0",
        "/api/v1/media/af15000",
    ];
    for round in ["cold", "warm", "hot"] {
        if reset {
            let _ = query("SELECT pg_stat_statements_reset()")
                .execute(store.pool())
                .await;
        }
        let only = std::env::var("BENCH_ONLY").unwrap_or_default();
        for path in home.iter().filter(|p| p.contains(only.as_str())) {
            let (status, body, ms) = get(&app, path, &token).await;
            let items = body["items"].as_array().map_or(0, Vec::len);
            println!("{round:>4} {ms:>8.1} ms  {status}  items={items:<3} {path}");
            assert!(status.is_success(), "{path}: {status} {body}");
        }
        if reset {
            let rows: Vec<(String, i64, f64, f64)> = sqlx::query_as(
                "SELECT left(regexp_replace(query,'\\s+',' ','g'),140), calls, total_exec_time, total_plan_time \
                 FROM pg_stat_statements WHERE query NOT LIKE '%pg_stat_statements%' \
                 ORDER BY total_exec_time+total_plan_time DESC LIMIT 15",
            )
            .fetch_all(store.pool())
            .await
            .unwrap_or_default();
            let calls: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(calls),0)::BIGINT FROM pg_stat_statements WHERE query NOT LIKE '%pg_stat_statements%'",
            )
            .fetch_one(store.pool())
            .await
            .unwrap_or(0);
            println!(
                "--- {round}: {calls} SQL statements for {} requests",
                home.len()
            );
            for (sql, calls, total, plan) in rows {
                println!("exec {total:>7.1} plan {plan:>7.1} ms {calls:>4}x  {sql}");
            }
        }
    }

    // Many viewers opening the home page at once.
    let start = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..200 {
        let app = app.clone();
        let token = token.clone();
        handles.push(tokio::spawn(
            async move { get(&app, home[1], &token).await.2 },
        ));
    }
    let mut times = Vec::new();
    for handle in handles {
        times.push(handle.await.unwrap());
    }
    times.sort_by(f64::total_cmp);
    println!(
        "200 concurrent home shelves: wall {:.0} ms, p50 {:.1} ms, p99 {:.1} ms",
        start.elapsed().as_secs_f64() * 1000.0,
        times[100],
        times[197]
    );
}
