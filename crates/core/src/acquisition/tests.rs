use super::*;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

pub(super) async fn fixture() -> (
    tempfile::TempDir,
    IdentityStore,
    Router,
    Vec<(String, String)>,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("requests.sqlite"))
        .await
        .unwrap();
    // Package is inserted as a fixture only; admission reads its signed capability projection.
    let package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            id: "media-requests".into(),
            permissions: vec!["requests".into()],
            ..Default::default()
        },
        publisher_id: "test".into(),
        wasm: String::new(),
        sha256: String::new(),
        signature: String::new(),
    };
    query("INSERT INTO extension_publishers(id,public_key,active) VALUES('test','synthetic',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation,all_libraries) VALUES('media-requests','test',?,1,'first',1)").bind(json!(package).to_string()).execute(store.pool()).await.unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('request-films','Films','films','local','/media',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO acquisition_servers(id,name,kind,base_url,api_key,allowed_ips_json,library_id,root_folder,quality_profile_id,is_default,enabled,generation) VALUES('radarr-a','Radarr A','radarr','http://127.0.0.1:7878/','test-key','[\"127.0.0.1\"]','request-films','/movies',1,1,1,'g')").execute(store.pool()).await.unwrap();
    let mut sessions = vec![];
    for (id, role, token) in [
        ("owner", "user", "a".repeat(64)),
        ("other", "user", "b".repeat(64)),
        ("admin", "administrator", "c".repeat(64)),
    ] {
        query("INSERT INTO users(id,role) VALUES(?,?)")
            .bind(id)
            .bind(role)
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,?,?,?)").bind(format!("{:x}",Sha256::digest(token.as_bytes()))).bind(id).bind(now()).bind(now()).bind(now()+3600).execute(store.pool()).await.unwrap();
        sessions.push((
            format!("media_session={token}"),
            format!("{:x}", Sha256::digest(format!("csrf:{token}"))),
        ));
    }
    query("INSERT INTO library_permissions(user_id,library_id) VALUES('owner','request-films')")
        .execute(store.pool())
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = router(auth);
    (dir, store, app, sessions)
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    value: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "application/json");
    if !cookie.is_empty() {
        req = req.header("cookie", cookie);
    }
    if !csrf.is_empty() {
        req = req.header("x-csrf-token", csrf);
    }
    let response = app
        .clone()
        .oneshot(req.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn routes_require_feature_csrf_administrator_and_ownership() {
    let (_dir, store, app, sessions) = fixture().await;
    let (owner, csrf) = &sessions[0];
    let (other, _) = &sessions[1];
    let (admin, admin_csrf) = &sessions[2];
    assert_eq!(
        request(&app, "GET", "/api/v1/requests", "", "", Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/v1/admin/acquisition/settings",
            admin,
            "",
            json!({"autoApprove":true,"maxPendingPerUser":20})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/v1/admin/acquisition/settings",
            owner,
            csrf,
            json!({"autoApprove":true,"maxPendingPerUser":20})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/v1/admin/acquisition/settings",
            admin,
            admin_csrf,
            json!({"autoApprove":true,"maxPendingPerUser":20})
        )
        .await
        .0,
        StatusCode::OK
    );
    let destinations = request(
        &app,
        "GET",
        "/api/v1/requests/destinations",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(destinations.1["servers"].as_array().unwrap().len(), 1);
    assert!(!destinations.1.to_string().contains("test-key"));
    assert!(!destinations.1.to_string().contains("127.0.0.1"));
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/destinations",
            other,
            "",
            Value::Null
        )
        .await
        .1["servers"],
        json!([])
    );
    let input = json!({"kind":"film","title":"Fixture","externalId":"123","externalProvider":"tmdb","serverId":"radarr-a"});
    query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,created_at,updated_at) VALUES('fixture','owner','radarr-a','request-films','film','Fixture','123','tmdb',?,'pending',?,?)").bind(input.to_string()).bind(now()).bind(now()).execute(store.pool()).await.unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/fixture",
            other,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/requests/fixture/action",
            owner,
            csrf,
            json!({"action":"approve"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/requests/fixture/action",
            admin,
            admin_csrf,
            json!({"action":"approve"})
        )
        .await
        .0,
        StatusCode::OK
    );
    let jobs: i64 =
        query_scalar("SELECT COUNT(*) FROM acquisition_jobs WHERE request_id='fixture'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(jobs, 1);
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/requests/fixture/action",
            admin,
            admin_csrf,
            json!({"action":"approve"})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    query("UPDATE extensions SET enabled=0 WHERE id='media-requests'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        request(&app, "GET", "/api/v1/requests", owner, "", Value::Null)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}
#[tokio::test]
async fn legacy_import_is_suspended_idempotent_and_delta_safe() {
    let (_dir, store, _app, _sessions) = fixture().await;
    let mut value = json!({"id":12,"status":2,"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","media":{"mediaType":"movie","tmdbId":123,"externalServiceId":42},"requestedBy":{"displayName":"Historical"}});
    assert!(
        import_legacy(&store, "source", &value, Some("owner"), Some("radarr-a"))
            .await
            .unwrap()
    );
    assert!(
        !import_legacy(&store, "source", &value, Some("owner"), Some("radarr-a"))
            .await
            .unwrap()
    );
    value["status"] = json!(5);
    value["updatedAt"] = json!("2026-01-02T00:00:00Z");
    assert!(
        import_legacy(&store, "source", &value, Some("owner"), Some("radarr-a"))
            .await
            .unwrap()
    );
    let (status, source_status): (String, String) =
        query_as("SELECT status,source_status FROM media_requests")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(status, "suspended");
    assert_eq!(source_status, "completed");
    let jobs: i64 = query_scalar("SELECT COUNT(*) FROM acquisition_jobs")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(jobs, 0);
    query("UPDATE media_requests SET status='approved'")
        .execute(store.pool())
        .await
        .unwrap();
    value["updatedAt"] = json!("2026-01-03T00:00:00Z");
    assert!(
        !import_legacy(&store, "source", &value, Some("owner"), Some("radarr-a"))
            .await
            .unwrap()
    );
}
#[test]
fn invalid_selections_never_expand_other_media_types() {
    let base =
        json!({"kind":"album","title":"Album","externalId":"123","externalProvider":"musicbrainz"});
    let mut input: RequestInput = serde_json::from_value(base).unwrap();
    input.seasons = vec![1];
    assert!(!input.valid());
    input.seasons.clear();
    input.album_ids = vec![uuid::Uuid::new_v4().to_string()];
    assert!(!input.valid());
}
