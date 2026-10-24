use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::{self, AuthState},
    database::query,
    identity::IdentityStore,
    task_engine,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:3000");
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    if let Some(csrf) = csrf {
        req = req.header("x-csrf-token", csrf);
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn jobs_require_live_admin_csrf_validate_input_and_deduplicate() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("http.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "task-admin",
            Zeroizing::new("synthetic task HTTP test secret".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic-root',1)").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = task_engine::router(auth.clone()).merge(auth::router(auth));
    let response = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        None,
        json!({"login":"task-admin","password":"synthetic task HTTP test secret"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let login = body(response).await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let path = "/api/v1/admin/tasks/jobs";
    let input = json!({"libraryId":"films","idempotencyKey":"retry-safe","priority":500});
    assert_eq!(
        call(&app, "GET", path, None, None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "POST", path, Some(&cookie), None, input.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            json!({"libraryId":"films","idempotencyKey":"bad","priority":1001})
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            json!({"libraryId":"missing","idempotencyKey":"missing"})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            json!({"libraryId":"films","idempotencyKey":"path","root":"C:/secret"})
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let response = call(&app, "POST", path, Some(&cookie), Some(csrf), input.clone()).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let created = body(response).await;
    let duplicate = body(call(&app, "POST", path, Some(&cookie), Some(csrf), input).await).await;
    assert_eq!(created["id"], duplicate["id"]);
    assert_eq!(created["refreshMode"], "incremental");
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            json!({"libraryId":"films","idempotencyKey":"retry-safe","priority":500,"mode":"full"})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            json!({"libraryId":"films","idempotencyKey":"bad-mode","mode":"shell"})
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            json!({"libraryId":"films","idempotencyKey":"retry-safe","priority":501})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let id = created["id"].as_str().unwrap();
    let cancel = format!("{path}/{id}/cancel");
    assert_eq!(
        call(&app, "POST", &cancel, Some(&cookie), None, Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &cancel,
            Some(&cookie),
            Some(csrf),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let list = body(
        call(
            &app,
            "GET",
            &format!("{path}?limit=9999&offset=-10"),
            Some(&cookie),
            None,
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(list["limit"], 200);
    assert_eq!(list["offset"], 0);
    assert_eq!(list["jobs"][0]["status"], "cancelled");
    assert!(list["jobs"][0]["finishedAt"].is_i64());
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('child','Child','films','local','synthetic-root-two',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO library_folders(library_id,parent_id,active) VALUES('child','films',1)")
        .execute(store.pool())
        .await
        .unwrap();
    let grouped = json!({"libraryId":"films","idempotencyKey":"k".repeat(128),"mode":"full"});
    assert_eq!(
        call(
            &app,
            "POST",
            path,
            Some(&cookie),
            Some(csrf),
            grouped.clone()
        )
        .await
        .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        call(&app, "POST", path, Some(&cookie), Some(csrf), grouped)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    let grouped_count: i64 = media_poc::database::query_scalar(
        "SELECT COUNT(*) FROM durable_tasks WHERE refresh_mode='full'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(grouped_count, 2);
    for sql in [
        "INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','films','fixture','Fixture','{}')",
        "INSERT INTO inventory_editions(id,work_id) VALUES('edition','work')",
        "INSERT INTO inventory_versions(id,edition_id) VALUES('version','edition')",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('source','version','films','synthetic-root')",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('asset','source','films','fixture.mp4',42,'0','video/mp4','{}',1)",
        "INSERT INTO inventory_tracks(id,asset_id,stream_index,metadata_json) VALUES('track','asset',0,'{\"codec_type\":\"audio\"}')",
        "INSERT INTO inventory_images(id,work_id,library_id,root_snapshot,relative_path,size,modified_ns,mime,priority,available) VALUES('image','work','films','synthetic-root','fixture.png',42,'0','image/png',0,1)",
    ] {
        query(sql).execute(store.pool()).await.unwrap();
    }
    for (input, status) in [
        (
            json!({"libraryId":"films","idempotencyKey":"invalid-category","category":"shell"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"libraryId":"child","idempotencyKey":"other-library","category":"analyze","targetId":"asset"}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"libraryId":"films","idempotencyKey":"invalid-component","category":"prepare","targetId":"asset","component":"../secret"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"libraryId":"films","idempotencyKey":"wrong-track","category":"prepare","targetId":"asset","component":"subtitle-0"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"libraryId":"films","idempotencyKey":"invalid-width","category":"image","targetId":"image","width":9999}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            json!({"libraryId":"films","idempotencyKey":"analyze","category":"analyze","targetId":"asset","priority":1000}),
            StatusCode::ACCEPTED,
        ),
        (
            json!({"libraryId":"films","idempotencyKey":"prepare","category":"prepare","targetId":"asset","component":"audio-0"}),
            StatusCode::ACCEPTED,
        ),
        (
            json!({"libraryId":"films","idempotencyKey":"image","category":"image","targetId":"image","width":320}),
            StatusCode::ACCEPTED,
        ),
    ] {
        assert_eq!(
            call(&app, "POST", path, Some(&cookie), Some(csrf), input)
                .await
                .status(),
            status
        );
    }
    let timestamp = media_contracts::protocol::unix_now();
    let claimed = task_engine::claim(&store, "local", timestamp, 60, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.category, "analyze");
    let result = json!({"validated":"fixture"});
    assert!(
        !task_engine::record_result(
            &store,
            &claimed.id,
            claimed.attempt + 1,
            "local",
            timestamp,
            &result
        )
        .await
        .unwrap()
    );
    assert!(
        !task_engine::record_result(
            &store,
            &claimed.id,
            claimed.attempt,
            "local",
            timestamp,
            &json!({"tooLarge":"x".repeat(65536)})
        )
        .await
        .unwrap()
    );
    assert!(
        task_engine::record_result(
            &store,
            &claimed.id,
            claimed.attempt,
            "local",
            timestamp,
            &result
        )
        .await
        .unwrap()
    );
    let detail_path = format!("{path}/{}", claimed.id);
    assert_eq!(
        call(&app, "GET", &detail_path, None, None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let detail =
        body(call(&app, "GET", &detail_path, Some(&cookie), None, Value::Null).await).await;
    assert_eq!(
        serde_json::from_str::<Value>(detail["resultJson"].as_str().unwrap()).unwrap(),
        result
    );
    let summary = body(call(&app, "GET", path, Some(&cookie), None, Value::Null).await).await;
    assert!(
        summary["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|job| job["resultJson"] == "{}")
    );
    query("UPDATE inventory_assets SET available=0 WHERE id='asset'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(call(&app,"POST",path,Some(&cookie),Some(csrf),json!({"libraryId":"films","idempotencyKey":"unavailable","category":"analyze","targetId":"asset"})).await.status(),StatusCode::NOT_FOUND);
    query("UPDATE users SET role='user'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", path, Some(&cookie), None, Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &cancel,
            Some(&cookie),
            Some(csrf),
            Value::Null
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}
