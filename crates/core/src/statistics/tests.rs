use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, header},
};
use tower::ServiceExt;
pub(super) const ORIGIN: &str = "http://127.0.0.1:3000";
pub(super) async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> (StatusCode, Value, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::COOKIE, cookie)
                .header(header::ORIGIN, ORIGIN)
                .header("x-csrf-token", csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        String::from_utf8_lossy(&bytes).into(),
    )
}
pub(super) async fn login(auth: &AuthState, user: &str, token: &str) -> (String, String) {
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at,device_label) VALUES(?,?,?,?,?,'Fixture browser')")
        .bind(crate::auth::hash(token)).bind(user).bind(now()).bind(now()).bind(now()+3600).execute(&auth.store.pool).await.unwrap();
    let cookie = format!("media_session={token}");
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse().unwrap());
    let csrf = auth.authenticate(&headers).await.unwrap().csrf_token;
    (cookie, csrf)
}
pub(super) async fn fixture(store: &IdentityStore) {
    for (id, role) in [
        ("stats-admin", "administrator"),
        ("stats-reader", "user"),
        ("stats-other", "user"),
    ] {
        query("INSERT INTO users(id,role) VALUES(?,?)")
            .bind(id)
            .bind(role)
            .execute(&store.pool)
            .await
            .unwrap();
    }
    for (id, kind) in [
        ("stats-films", "films"),
        ("stats-music", "music"),
        ("stats-hidden", "films"),
    ] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,?,'local',?,1)")
            .bind(id)
            .bind(id)
            .bind(kind)
            .bind(format!("/ro/{id}"))
            .execute(&store.pool)
            .await
            .unwrap();
    }
    for user in ["stats-reader", "stats-other"] {
        for library in ["stats-films", "stats-music"] {
            query("INSERT INTO library_permissions(user_id,library_id) VALUES(?,?)")
                .bind(user)
                .bind(library)
                .execute(&store.pool)
                .await
                .unwrap();
        }
    }
    for (id, library, age) in [
        ("stats-film", "stats-films", 0),
        ("stats-adult", "stats-films", 18),
        ("stats-private", "stats-hidden", 0),
    ] {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,minimum_age) VALUES(?,?,?,?,'fixture','film','{}',?)").bind(id).bind(library).bind(id).bind(format!("=Fixture {id}")).bind(age).execute(&store.pool).await.unwrap();
        query("INSERT INTO inventory_editions(id,work_id) VALUES(?,?)")
            .bind(id)
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        query("INSERT INTO inventory_versions(id,edition_id) VALUES(?,?)")
            .bind(id)
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        query(
            "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,?,?)",
        )
        .bind(id)
        .bind(id)
        .bind(library)
        .bind(format!("/ro/{library}"))
        .execute(&store.pool)
        .await
        .unwrap();
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,?,?,?,100,'0','video/x-matroska','{}',1)").bind(id).bind(id).bind(library).bind(format!("{id}.mkv")).execute(&store.pool).await.unwrap();
    }
    query("INSERT INTO music_tracks(id,library_id,root_snapshot,relative_path,size,modified_ns,title,artist,album,album_artist,duration,codec,mime,scan_id,available) VALUES('stats-track','stats-music','/ro/stats-music','fixture.opus',100,'0','Fixture music','Fixture artist','Fixture album','Fixture artist',100.0,'opus','audio/ogg','fixture',1)").execute(&store.pool).await.unwrap();
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('stats-reader',12,1)").execute(&store.pool).await.unwrap();
}
fn event(sequence: i64, elapsed: f64, position: f64, kind: &str) -> Value {
    json!({"sequence":sequence,"kind":kind,"elapsedMs":elapsed*1000.0,"position":position,"duration":100.0,"playing":kind!="pause"&&kind!="stop"&&kind!="ended","buffering":false,"rate":1.0,"origin":"manual"})
}
async fn contract(store: IdentityStore) {
    fixture(&store).await;
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = router(auth.clone());
    let (admin, admin_csrf) = login(&auth, "stats-admin", &"a".repeat(64)).await;
    let (reader, csrf) = login(&auth, "stats-reader", &"b".repeat(64)).await;
    let (other, other_csrf) = login(&auth, "stats-other", &"c".repeat(64)).await;
    let settings_path = "/api/v1/admin/statistics/settings";
    let settings = json!({"enabled":true,"eventRetentionDays":365,"summaryRetentionDays":0});
    let id = uuid::Uuid::new_v4().to_string();
    let start = json!({"id":id,"mediaId":"stats-film","music":false});
    let create = "/api/v1/playback/sessions";
    let ingest = format!("{create}/{id}/events");
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/account/statistics",
            "",
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/statistics",
            &reader,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/playback/statistics",
            &reader,
            "",
            Value::Null
        )
        .await
        .1["enabled"],
        false
    );
    assert_eq!(
        call(&app, "POST", create, &reader, &csrf, start.clone())
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "PUT", settings_path, &admin, "", settings.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "PUT", settings_path, &reader, &csrf, settings.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            settings_path,
            &admin,
            &admin_csrf,
            settings.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "POST", create, &reader, &csrf, start.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", create, &reader, &csrf, start.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", create, &other, &other_csrf, start.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for media in ["stats-adult", "stats-private"] {
        assert_eq!(
            call(
                &app,
                "POST",
                create,
                &reader,
                &csrf,
                json!({"id":uuid::Uuid::new_v4().to_string(),"mediaId":media,"music":false})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    query("UPDATE statistics_sessions SET started_at=? WHERE id=?")
        .bind(now() - 180)
        .bind(&id)
        .execute(&store.pool)
        .await
        .unwrap();
    let batch = json!({"events":[event(1,0.0,0.0,"start"),event(2,15.0,15.0,"heartbeat"),event(3,30.0,30.0,"pause")]});
    assert_eq!(
        call(&app, "POST", &ingest, &other, &other_csrf, batch.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "POST", &ingest, &reader, "", batch.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "POST", &ingest, &reader, &csrf, batch.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", &ingest, &reader, &csrf, batch).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &ingest,
            &reader,
            &csrf,
            json!({"events":[event(5,40.0,40.0,"heartbeat")]})
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let ending = json!({"events":[event(4,60.0,30.0,"resume"),event(5,75.0,45.0,"heartbeat"),event(6,90.0,60.0,"heartbeat"),event(7,105.0,75.0,"heartbeat"),event(8,120.0,90.0,"ended")]});
    assert_eq!(
        call(&app, "POST", &ingest, &reader, &csrf, ending.clone())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", &ingest, &reader, &csrf, ending).await.0,
        StatusCode::OK
    );
    let (status, report, raw) = call(
        &app,
        "GET",
        "/api/v1/account/statistics",
        &reader,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{raw}");
    assert_eq!(report["summary"]["plays"], 1);
    assert_eq!(report["summary"]["seconds"], 90.0);
    assert_eq!(report["summary"]["completed"], 1);
    assert_eq!(report["popular"][0]["rating"], 0.0);
    assert!(
        !raw.contains("owner_hash")
            && !raw.contains("token_hash")
            && !raw.contains("/ro/")
            && !raw.contains("stats-private")
    );
    let personal_detail = format!("/api/v1/account/statistics/sessions/{id}");
    assert_eq!(
        call(&app, "GET", &personal_detail, &other, "", Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", &personal_detail, &reader, "", Value::Null)
            .await
            .1["events"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/account/statistics?userId=stats-other",
            &reader,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, other_report, _) = call(
        &app,
        "GET",
        "/api/v1/account/statistics",
        &other,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(other_report["summary"]["plays"], 0);
    let (_, _, csv) = call(
        &app,
        "GET",
        "/api/v1/account/statistics/export",
        &reader,
        "",
        Value::Null,
    )
    .await;
    assert!(csv.contains("\"'=Fixture"));
    query(
        "DELETE FROM library_permissions WHERE user_id='stats-reader' AND library_id='stats-films'",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        call(&app, "GET", &personal_detail, &reader, "", Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/account/statistics",
            &reader,
            "",
            Value::Null
        )
        .await
        .1["summary"]["plays"],
        0
    );
    query("UPDATE users SET role='administrator' WHERE id='stats-reader'")
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", &personal_detail, &reader, "", Value::Null)
            .await
            .0,
        StatusCode::OK,
        "administrator's personal history must honor implicit library access"
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/account/statistics",
            &reader,
            "",
            Value::Null
        )
        .await
        .1["summary"]["plays"],
        1
    );
    query("UPDATE users SET role='user' WHERE id='stats-reader'")
        .execute(&store.pool)
        .await
        .unwrap();
    query(
        "INSERT INTO library_permissions(user_id,library_id) VALUES('stats-reader','stats-films')",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    query("UPDATE inventory_works SET minimum_age=18 WHERE id='stats-film'")
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", &personal_detail, &reader, "", Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    query("UPDATE inventory_works SET minimum_age=0 WHERE id='stats-film'")
        .execute(&store.pool)
        .await
        .unwrap();
    let track_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            create,
            &reader,
            &csrf,
            json!({"id":track_id,"mediaId":"stats-track","music":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    let music_ingest = format!("{create}/{track_id}/events");
    assert_eq!(
        call(
            &app,
            "POST",
            &music_ingest,
            &reader,
            &csrf,
            json!({"events":[event(1,0.0,0.0,"start"),event(2,10.0,10.0,"heartbeat")]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, music, raw) = call(
        &app,
        "GET",
        "/api/v1/account/statistics?kind=music",
        &reader,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{raw}");
    assert_eq!(music["music"]["artist"][0]["label"], "Fixture artist");
    query("UPDATE statistics_sessions SET updated_at=? WHERE id=?")
        .bind(now() - 91)
        .bind(&track_id)
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &music_ingest,
            &reader,
            &csrf,
            json!({"events":[event(3,15.0,15.0,"heartbeat")]})
        )
        .await
        .0,
        StatusCode::GONE
    );
    maintain(&store).await.unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/account/statistics/sessions/{track_id}"),
            &reader,
            "",
            Value::Null
        )
        .await
        .1["session"]["status"],
        "interrupted"
    );
    let other_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        call(
            &app,
            "POST",
            create,
            &other,
            &other_csrf,
            json!({"id":other_id,"mediaId":"stats-track","music":true})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{create}/{other_id}/events"),
            &other,
            &other_csrf,
            json!({"events":[event(1,0.0,0.0,"start"),event(2,10.0,10.0,"heartbeat")]})
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut off = settings.clone();
    off["enabled"] = json!(false);
    assert_eq!(
        call(&app, "PUT", settings_path, &admin, &admin_csrf, off)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            create,
            &reader,
            &csrf,
            json!({"id":uuid::Uuid::new_v4().to_string(),"mediaId":"stats-track","music":true})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/account/statistics",
            &reader,
            "",
            Value::Null
        )
        .await
        .1["summary"]["plays"],
        2
    );
    // Detailed events expire; session summaries and daily totals survive indefinitely.
    query("UPDATE statistics_events SET received_at=?")
        .bind(now() - 366 * 86400)
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(maintain(&store).await.unwrap(), 12);
    let detail = call(&app, "GET", &personal_detail, &reader, "", Value::Null)
        .await
        .1;
    assert_eq!(detail["detailsExpired"], true);
    assert_eq!(detail["session"]["seconds"], 90.0);
    assert_eq!(
        query_scalar::<sqlx::Any, i64>("SELECT COUNT(*) FROM statistics_daily")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/account/statistics",
            &reader,
            &csrf,
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/statistics",
            &admin,
            "",
            Value::Null
        )
        .await
        .1["summary"]["plays"],
        1
    );
    assert_eq!(
        query_scalar::<sqlx::Any, i64>("SELECT COUNT(*) FROM statistics_daily")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        1
    );
}
#[tokio::test]
async fn statistics_sqlite_http_is_private_idempotent_optional_and_durable() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("statistics.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    contract(store.clone()).await;
    drop(store);
    let reopened = IdentityStore::open(&path).await.unwrap();
    assert_eq!(settings(&reopened).await.unwrap().event_retention_days, 365);
    assert_eq!(settings(&reopened).await.unwrap().enabled, 0);
    assert_eq!(
        query_scalar::<sqlx::Any, i64>(
            "SELECT COUNT(*) FROM statistics_collection_periods WHERE ended_at IS NOT NULL"
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap(),
        1
    );
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL: MEDIA_TEST_POSTGRES_URL"]
async fn statistics_postgres_http_contract() {
    let url = std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL URL");
    contract(IdentityStore::open_postgres(&url).await.unwrap()).await;
}
