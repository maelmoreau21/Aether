//! HTTP contract of insights, profiles and the yearly Wrapped.
use super::aggregate::{civil_from_days, days_from_civil};
use super::tests::{call, fixture, login};
use super::*;

async fn work(store: &IdentityStore, id: &str, kind: &str, editorial: &str) {
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,editorial_json) VALUES(?,'stats-films',?,?,'x',?,'{}',?)")
        .bind(id).bind(id).bind(format!("Fixture {id}")).bind(kind).bind(editorial)
        .execute(&store.pool).await.unwrap();
}
#[allow(clippy::too_many_arguments)]
async fn session(
    store: &IdentityStore,
    id: &str,
    user: &str,
    media: &str,
    kind: &str,
    started: i64,
    seconds: f64,
    completed: bool,
) {
    let (library, artist, album) = if kind == "music" {
        ("stats-music", "Fixture artist", "Fixture album")
    } else {
        ("stats-films", "", "")
    };
    query("INSERT INTO statistics_sessions(id,user_id,owner_hash,generation,media_id,canonical_id,asset_id,library_id,node_id,kind,title,artist,album,device,started_at,updated_at,ended_at,state_json,seconds,covered_seconds,duration,completed,end_reason) VALUES(?,?,'fixture',0,?,?,?,?,'local',?,?,?,?,'Mozilla/5.0 (Windows NT 10.0) Firefox/131.0',?,?,?,'{\"method\":\"direct\"}',?,?,3600.0,?,'ended')")
        .bind(id).bind(user).bind(media).bind(media).bind(media).bind(library).bind(kind).bind(format!("Fixture {media}"))
        .bind(artist).bind(album).bind(started).bind(started + seconds as i64).bind(started + seconds as i64)
        .bind(seconds).bind(seconds).bind(i64::from(completed))
        .execute(&store.pool).await.unwrap();
}

async fn contract(store: IdentityStore) {
    fixture(&store).await;
    query("UPDATE statistics_settings SET enabled=1")
        .execute(&store.pool)
        .await
        .unwrap();
    query("UPDATE statistics_visibility SET users_can_view=1")
        .execute(&store.pool)
        .await
        .unwrap();
    work(&store, "stats-show", "series", r#"{"genres":["drame"]}"#).await;
    for episode in ["stats-ep1", "stats-ep2"] {
        work(&store, episode, "episode", "{}").await;
    }
    query("INSERT INTO inventory_seasons(id,series_id,season_number,title) VALUES('stats-season','stats-show',1,'S1')")
        .execute(&store.pool).await.unwrap();
    for (episode, number) in [("stats-ep1", 1), ("stats-ep2", 2)] {
        query("INSERT INTO inventory_episodes(id,season_id,episode_number,work_id) VALUES(?,'stats-season',?,?)")
            .bind(episode).bind(number).bind(episode).execute(&store.pool).await.unwrap();
    }
    let auth = AuthState::new(store.clone(), super::tests::ORIGIN.into())
        .await
        .unwrap();
    let app = router(auth.clone());
    let (admin, admin_csrf) = login(&auth, "stats-admin", &"a".repeat(64)).await;
    let (reader, _) = login(&auth, "stats-reader", &"b".repeat(64)).await;
    let year = civil_from_days(now().div_euclid(86400)).0 - 1;
    let at = |y: i64, m: u32, d: u32, h: i64| days_from_civil(y, m, d) * 86400 + h * 3600;
    // Last complete year: zap, film, a two-episode binge, a rewatch, music, and
    // a film at 23:30 UTC on 31 December (already next year in Paris).
    session(
        &store,
        "z",
        "stats-reader",
        "stats-film",
        "film",
        at(year, 3, 1, 20),
        30.0,
        false,
    )
    .await;
    session(
        &store,
        "f",
        "stats-reader",
        "stats-film",
        "film",
        at(year, 3, 1, 21),
        4000.0,
        true,
    )
    .await;
    session(
        &store,
        "e1",
        "stats-reader",
        "stats-ep1",
        "episode",
        at(year, 6, 10, 20),
        1500.0,
        true,
    )
    .await;
    session(
        &store,
        "e2",
        "stats-reader",
        "stats-ep2",
        "episode",
        at(year, 6, 10, 21),
        1500.0,
        true,
    )
    .await;
    session(
        &store,
        "e3",
        "stats-reader",
        "stats-ep1",
        "episode",
        at(year, 6, 11, 20),
        1500.0,
        false,
    )
    .await;
    session(
        &store,
        "m",
        "stats-reader",
        "stats-track",
        "music",
        at(year, 6, 11, 8),
        200.0,
        true,
    )
    .await;
    session(
        &store,
        "edge",
        "stats-reader",
        "stats-film",
        "film",
        at(year, 12, 31, 23) + 1800,
        3000.0,
        false,
    )
    .await;
    session(
        &store,
        "old",
        "stats-reader",
        "stats-film",
        "film",
        at(year - 1, 5, 5, 12),
        1000.0,
        false,
    )
    .await;
    session(
        &store,
        "other",
        "stats-other",
        "stats-film",
        "film",
        at(year, 3, 2, 12),
        3000.0,
        false,
    )
    .await;

    let path = format!("/api/v1/account/statistics/wrapped?year={year}&tz=UTC");
    let (status, w, raw) = call(&app, "GET", &path, &reader, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{raw}");
    assert_eq!(w["visible"], true);
    assert_eq!(w["totals"]["plays"], 6, "the 30 s zap never counts: {raw}");
    assert_eq!(w["series"][0]["id"], "stats-show");
    assert_eq!(w["series"][0]["items"], 2);
    assert_eq!(w["binge"]["episodes"], 2);
    assert_eq!(w["streak"]["days"], 2);
    assert_eq!(w["previous"]["plays"], 1);
    assert_eq!(w["genres"][0]["title"], "Drame");
    assert_eq!(w["topDevice"], "Firefox · Windows");
    assert_eq!(w["films"][0]["image"], "/api/v1/works/stats-film/poster");
    assert!(
        w["availableYears"]
            .as_array()
            .unwrap()
            .contains(&json!(year - 1))
    );
    assert!(!raw.contains("stats-other") && !raw.contains("owner_hash"));
    let paris = format!("/api/v1/account/statistics/wrapped?year={year}&tz=Europe/Paris");
    assert_eq!(
        call(&app, "GET", &paris, &reader, "", Value::Null).await.1["totals"]["plays"],
        5
    );
    let music = format!("/api/v1/account/statistics/wrapped?year={year}&kind=music&tz=0");
    let (_, w, _) = call(&app, "GET", &music, &reader, "", Value::Null).await;
    assert_eq!(w["totals"]["plays"], 1);
    assert_eq!(w["artists"][0]["title"], "Fixture artist");
    for bad in ["kind=books", "tz=Mars/Olympus", "year=1900"] {
        let bad = format!("/api/v1/account/statistics/wrapped?{bad}");
        assert_eq!(
            call(&app, "GET", &bad, &reader, "", Value::Null).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let admin_path = format!("/api/v1/admin/statistics/wrapped/stats-reader?year={year}");
    assert_eq!(
        call(&app, "GET", &admin_path, &reader, "", Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "GET", &admin_path, &admin, "", Value::Null)
            .await
            .1["totals"]["plays"],
        6
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/statistics/wrapped/nobody",
            &admin,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Insights for the year: opened sessions keep the zap, plays do not.
    let (from, to) = (at(year, 1, 1, 0), at(year + 1, 1, 1, 0));
    let insights = format!("/api/v1/account/statistics/insights?from={from}&to={to}&tz=UTC");
    let (status, v, raw) = call(&app, "GET", &insights, &reader, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{raw}");
    assert_eq!(v["totals"]["opened"], 7);
    assert_eq!(v["totals"]["plays"], 6);
    assert_eq!(v["series"][0]["id"], "stats-show");
    assert_eq!(v["growth"]["previous"]["plays"], 1);
    assert_eq!(v["concurrency"]["peak"], 1);
    let summary = format!("/api/v1/account/statistics?from={from}&to={to}");
    let report = call(&app, "GET", &summary, &reader, "", Value::Null)
        .await
        .1;
    assert_eq!(report["summary"]["plays"], 7);
    assert_eq!(report["summary"]["qualifiedPlays"], 6);
    assert_eq!(report["wrappedAvailable"], true);
    let profile = format!("/api/v1/admin/statistics/users/stats-reader?from={from}&to={to}");
    assert_eq!(
        call(&app, "GET", &profile, &reader, "", Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, p, raw) = call(&app, "GET", &profile, &admin, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{raw}");
    assert_eq!(p["user"]["id"], "stats-reader");
    assert_eq!(p["totals"]["plays"], 6);
    assert_eq!(p["recent"].as_array().unwrap().len(), 7);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/statistics/users/nobody",
            &admin,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    // Visibility: hidden for accounts, still available to administrators.
    let settings = "/api/v1/admin/statistics/wrapped-settings";
    let hidden = json!({"visible":false,"periodEnabled":false,"startMonth":12,"startDay":1,"endMonth":1,"endDay":31});
    assert_eq!(
        call(&app, "PUT", settings, &admin, "", hidden.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "PUT", settings, &admin, &admin_csrf, hidden)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let invalid = json!({"visible":true,"periodEnabled":true,"startMonth":2,"startDay":30,"endMonth":1,"endDay":31});
    assert_eq!(
        call(&app, "PUT", settings, &admin, &admin_csrf, invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "GET", &path, &reader, "", Value::Null).await.1["visible"],
        false
    );
    assert_eq!(
        call(&app, "GET", &admin_path, &admin, "", Value::Null)
            .await
            .1["visible"],
        true
    );
    assert_eq!(
        call(&app, "GET", settings, &admin, "", Value::Null).await.1["visible"],
        false
    );
}

#[tokio::test]
async fn statistics_wrapped_and_insights_sqlite() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("wrapped.sqlite"))
        .await
        .unwrap();
    contract(store).await;
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL: MEDIA_TEST_POSTGRES_URL"]
async fn statistics_wrapped_and_insights_postgres() {
    let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL URL");
    contract(IdentityStore::open_postgres(&url).await.unwrap()).await;
}
