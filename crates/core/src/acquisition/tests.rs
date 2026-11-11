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
            format!("aether_session={token}"),
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
async fn list_filters_counts_pages_by_date_and_rejects_with_a_fixed_reason() {
    let (_dir, store, app, sessions) = fixture().await;
    let (owner, csrf) = &sessions[0];
    let (other, _) = &sessions[1];
    let (admin, admin_csrf) = &sessions[2];
    query("UPDATE users SET display_name='Owner Name' WHERE id='owner'")
        .execute(store.pool())
        .await
        .unwrap();
    // Random ids sort against creation order: the list must follow the dates.
    for (id, external, status, at) in [
        ("zz-old", "1", "pending", 100),
        ("aa-mid", "2", "available", 200),
        ("mm-new", "3", "failed", 300),
    ] {
        let input = json!({"kind":"film","title":id,"externalId":external,"externalProvider":"tmdb","serverId":"radarr-a"});
        query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,created_at,updated_at) VALUES(?,'owner','radarr-a','request-films','film',?,?,'tmdb',?,?,?,?)")
            .bind(id).bind(id).bind(external).bind(input.to_string()).bind(status).bind(at).bind(at).execute(store.pool()).await.unwrap();
    }
    let (status, page) = request(
        &app,
        "GET",
        "/api/v1/requests?limit=2",
        admin,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let ids: Vec<&str> = page["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["mm-new", "aa-mid"]);
    assert_eq!(page["requests"][0]["authorLabel"], "Owner Name");
    assert_eq!(page["requests"][0]["own"], false);
    assert_eq!(
        page["counts"],
        json!({"pending":1,"available":1,"failed":1})
    );
    let cursor = page["nextCursor"].as_str().unwrap().to_owned();
    let (_, next) = request(
        &app,
        "GET",
        &format!("/api/v1/requests?limit=2&before={cursor}"),
        admin,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(next["requests"][0]["id"], "zz-old");
    assert_eq!(next["nextCursor"], Value::Null);
    let (_, attention) = request(
        &app,
        "GET",
        "/api/v1/requests?status=attention",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(attention["requests"].as_array().unwrap().len(), 1);
    assert_eq!(attention["requests"][0]["own"], true);
    let (_, mine) = request(
        &app,
        "GET",
        "/api/v1/requests?mine=true",
        admin,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(mine["requests"], json!([]));
    assert_eq!(
        request(&app, "GET", "/api/v1/requests", other, "", Value::Null)
            .await
            .1["counts"],
        json!({})
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests?status=deleted",
            admin,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // A reason is accepted only when declining, and only from the fixed list.
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/requests/zz-old/action",
            admin,
            admin_csrf,
            json!({"action":"reject","reason":"because I said so"})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/v1/requests/zz-old/action",
            owner,
            csrf,
            json!({"action":"cancel","reason":"duplicate"})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (status, rejected) = request(
        &app,
        "POST",
        "/api/v1/requests/zz-old/action",
        admin,
        admin_csrf,
        json!({"action":"reject","reason":"already_available"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["errorCode"], "already_available");
    let (_, detail) = request(
        &app,
        "GET",
        "/api/v1/requests/zz-old",
        owner,
        "",
        Value::Null,
    )
    .await;
    let last = detail["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["code"], "already_available");
    assert!(last.get("actorId").is_none());
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

fn film(external: &str) -> RequestInput {
    serde_json::from_value(json!({"kind":"film","title":external,"externalId":external,"externalProvider":"tmdb","serverId":"radarr-a"})).unwrap()
}
/// Admission as the author's rights decide it: the starting status and its audit code.
async fn admit(
    store: &IdentityStore,
    auth: &AuthState,
    user: &str,
    external: &str,
) -> Result<(String, Option<String>), Error> {
    let role: String = query_scalar("SELECT role FROM users WHERE id=?")
        .bind(user)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let effective = policy::effective(store, user, role == "administrator")
        .await
        .unwrap();
    let server = server(store, "radarr-a").await.unwrap();
    let id = http::insert_request(auth, user, &effective, &server, &film(external)).await?;
    let status: String = query_scalar("SELECT status FROM media_requests WHERE id=?")
        .bind(&id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    let code: Option<String> = query_scalar(
        "SELECT code FROM acquisition_events WHERE request_id=? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    Ok((status, code))
}
fn error_code(error: &Error) -> &str {
    let (_, Json(body)) = error;
    body["error"].as_str().unwrap_or("")
}
#[tokio::test]
async fn rights_decide_who_requests_what_and_whether_it_is_approved() {
    let (_dir, store, app, sessions) = fixture().await;
    let (owner, owner_csrf) = &sessions[0];
    let (admin, admin_csrf) = &sessions[2];
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let rules = "/api/v1/admin/acquisition/policies";
    // Nobody configured anything: requests wait for review, administrators are approved.
    assert_eq!(
        admit(&store, &auth, "owner", "1").await.unwrap(),
        ("pending".into(), None)
    );
    assert_eq!(
        admit(&store, &auth, "admin", "2").await.unwrap(),
        ("approved".into(), None)
    );
    let (status, mine) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{mine}");
    assert_eq!(mine["classes"]["film"]["allow"], true);
    assert_eq!(mine["classes"]["film"]["autoApprove"], false);
    assert_eq!(mine["classes"]["film"]["destination"], true);
    assert_eq!(mine["classes"]["series"]["destination"], false);
    assert_eq!(mine["restricted"], false);
    // Only an administrator, with its CSRF token, edits rights.
    let group = json!({"film":{"autoApprove":true,"quota":{"limit":2,"days":7}}});
    query("INSERT INTO user_groups(id,name,is_default) VALUES('movie-fans','Movie fans',0)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO user_group_members(group_id,user_id) VALUES('movie-fans','owner')")
        .execute(store.pool())
        .await
        .unwrap();
    let path = format!("{rules}/groups/movie-fans");
    assert_eq!(
        request(&app, "PUT", &path, owner, owner_csrf, group.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "PUT", &path, admin, "", group.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, saved) = request(&app, "PUT", &path, admin, admin_csrf, group).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("{rules}/groups/missing"),
            admin,
            admin_csrf,
            json!({"film":{"allow":false}})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for invalid in [
        json!({"film":{"quota":{"limit":3,"days":0}}}),
        json!({"film":{"quota":{"limit":-1,"days":7}}}),
        json!({"film":{"admin":true}}),
        json!({"games":{}}),
    ] {
        let (status, _) = request(&app, "PUT", &path, admin, admin_csrf, invalid).await;
        assert!(status.is_client_error(), "{status}");
    }
    // The group approves automatically and allows two requests per week.
    let (_, mine) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(mine["classes"]["film"]["autoApprove"], true);
    // The pending request made before the rule already uses one of the two places.
    assert_eq!(mine["classes"]["film"]["quota"]["remaining"], 1);
    assert_eq!(mine["classes"]["series"]["autoApprove"], false);
    assert_eq!(
        admit(&store, &auth, "owner", "3").await.unwrap(),
        ("approved".into(), Some("auto_approved".into()))
    );
    let jobs: i64 = query_scalar("SELECT COUNT(*) FROM acquisition_jobs j JOIN media_requests r ON r.id=j.request_id WHERE r.external_id='3'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(jobs, 1);
    let refused = admit(&store, &auth, "owner", "4").await.unwrap_err();
    assert_eq!(
        (refused.0, error_code(&refused)),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "request_period_quota_exceeded"
        )
    );
    let (_, mine) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(mine["classes"]["film"]["quota"]["remaining"], 0);
    assert!(
        mine["classes"]["film"]["quota"]["resetsAt"]
            .as_i64()
            .unwrap()
            > now()
    );
    // A cancelled request frees its slot.
    query("UPDATE media_requests SET status='cancelled' WHERE external_id='1'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        admit(&store, &auth, "owner", "5").await.unwrap().0,
        "approved"
    );
    // The account's own rule beats its groups: no more film requests, even for the search.
    let own = format!("{rules}/users/owner");
    assert_eq!(
        request(
            &app,
            "PUT",
            &own,
            admin,
            admin_csrf,
            json!({"film":{"allow":false}})
        )
        .await
        .0,
        StatusCode::OK
    );
    let body = json!({"kind":"film","title":"X","externalId":"9","externalProvider":"tmdb","serverId":"radarr-a"});
    let (status, denied) = request(&app, "POST", "/api/v1/requests", owner, owner_csrf, body).await;
    assert_eq!(
        (status, &denied["error"]),
        (StatusCode::FORBIDDEN, &json!("request_permission_denied"))
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/search?q=matrix&kind=film",
            owner,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, effective) = request(
        &app,
        "GET",
        &format!("{own}/effective"),
        admin,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(effective["rule"]["film"]["allow"], false);
    assert_eq!(effective["effective"]["classes"]["film"]["allow"], false);
    assert_eq!(effective["effective"]["classes"]["series"]["allow"], true);
    let (_, listed) = request(&app, "GET", rules, admin, "", Value::Null).await;
    assert_eq!(listed["users"][0]["id"], "owner");
    assert!(
        listed["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "movie-fans" && g["rule"]["film"]["autoApprove"] == true)
    );
    assert!(
        !listed["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "builtin-role-administrator")
    );
    // Removing the rule (or saving an empty one) restores inheritance.
    assert_eq!(
        request(&app, "PUT", &own, admin, admin_csrf, json!({}))
            .await
            .0,
        StatusCode::OK
    );
    let (_, listed) = request(&app, "GET", rules, admin, "", Value::Null).await;
    assert_eq!(listed["users"], json!([]));
    // The server default applies to everyone without a more specific rule.
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("{rules}/default"),
            admin,
            admin_csrf,
            json!({"series":{"allow":false}})
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, mine) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(mine["classes"]["series"]["allow"], false);
    let (_, admins) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        admin,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(admins["classes"]["series"]["allow"], true);
    assert_eq!(admins["classes"]["film"]["quota"], Value::Null);
}
#[tokio::test]
async fn deleting_a_group_or_user_removes_its_rules_and_profiles_follow_their_owner() {
    let (_dir, store, app, sessions) = fixture().await;
    let (admin, admin_csrf) = &sessions[2];
    query("INSERT INTO user_groups(id,name,is_default) VALUES('g','G',0)")
        .execute(store.pool())
        .await
        .unwrap();
    for target in ["groups/g", "users/owner"] {
        let path = format!("/api/v1/admin/acquisition/policies/{target}");
        assert_eq!(
            request(
                &app,
                "PUT",
                &path,
                admin,
                admin_csrf,
                json!({"film":{"allow":false}})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            request(&app, "DELETE", &path, admin, admin_csrf, Value::Null)
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
    query("INSERT INTO request_group_policies(group_id,policy_json,updated_at) VALUES('g','{}',0)")
        .execute(store.pool())
        .await
        .unwrap();
    query("DELETE FROM user_groups WHERE id='g'")
        .execute(store.pool())
        .await
        .unwrap();
    let left: i64 = query_scalar("SELECT COUNT(*) FROM request_group_policies")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
    // A profile is a user of its owner's account: rules attach to the owner and are shared.
    query("INSERT INTO users(id,role) VALUES('kid','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO account_profiles(user_id,owner_id,created_at) VALUES('kid','owner',0)")
        .execute(store.pool())
        .await
        .unwrap();
    let path = "/api/v1/admin/acquisition/policies/users/kid";
    assert_eq!(
        request(
            &app,
            "PUT",
            path,
            admin,
            admin_csrf,
            json!({"film":{"allow":false}})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            "/api/v1/admin/acquisition/policies/users/owner",
            admin,
            admin_csrf,
            json!({"film":{"allow":false}})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        !policy::effective(&store, "kid", false)
            .await
            .unwrap()
            .grant("film")
            .allow
    );
    // Quotas count the owner's requests and its profiles' together.
    query("DELETE FROM request_user_policies")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO request_default_policy(singleton,policy_json) VALUES(1,'{}') ON CONFLICT(singleton) DO UPDATE SET policy_json='{\"film\":{\"quota\":{\"limit\":1,\"days\":30}}}'").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    admit(&store, &auth, "owner", "1").await.unwrap();
    let refused = admit(&store, &auth, "kid", "2").await.unwrap_err();
    assert_eq!(error_code(&refused), "request_period_quota_exceeded");
}
#[tokio::test]
async fn combined_search_is_quiet_about_what_a_person_cannot_request() {
    let (_dir, _store, app, sessions) = fixture().await;
    let (owner, _) = &sessions[0];
    let (other, _) = &sessions[1];
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/discover?q=matrix",
            "",
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/discover?q=m",
            owner,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // No readable destination: nothing to offer and nothing to complain about.
    let (status, quiet) = request(
        &app,
        "GET",
        "/api/v1/requests/discover?q=matrix",
        other,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(
        (status, &quiet["results"], &quiet["errors"]),
        (StatusCode::OK, &json!([]), &json!([]))
    );
    // A destination that does not answer is reported, not hidden.
    let (status, broken) = request(
        &app,
        "GET",
        "/api/v1/requests/discover?q=matrix",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{broken}");
    assert_eq!(broken["errors"][0]["error"], "connection_failed");
    assert_eq!(broken["restricted"], false);
}
#[tokio::test]
async fn parental_restrictions_hide_external_results() {
    let (_dir, store, app, sessions) = fixture().await;
    let (owner, _) = &sessions[0];
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('owner',12,0)")
        .execute(store.pool())
        .await
        .unwrap();
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/requests/discover?q=matrix",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(
        (status, &body["restricted"], &body["results"]),
        (StatusCode::OK, &json!(true), &json!([]))
    );
    let (_, mine) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(mine["restricted"], true);
    assert_eq!(mine["classes"]["film"]["destination"], false);
}
#[tokio::test]
async fn posters_are_served_only_from_fixed_providers_and_plain_paths() {
    let (_dir, _store, app, sessions) = fixture().await;
    let (owner, _) = &sessions[0];
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/poster?src=tmdb&path=/a.jpg",
            "",
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/v1/requests/poster?src=evil&path=/a.jpg",
            owner,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    for path in ["/../a.jpg", "/a.exe", "a.jpg", "//evil.example/a.jpg"] {
        let uri = format!("/api/v1/requests/poster?src=tmdb&path={path}");
        assert_eq!(
            request(&app, "GET", &uri, owner, "", Value::Null).await.0,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
    }
}
#[test]
fn a_lookup_poster_is_reduced_to_a_fixed_provider_and_a_plain_path() {
    let tmdb = json!({"remotePoster":"https://image.tmdb.org/t/p/original/AbC-123_x.jpg"});
    assert_eq!(
        remote::poster_source(&tmdb),
        Some(("tmdb", "/AbC-123_x.jpg".into()))
    );
    let tvdb = json!({"images":[{"coverType":"fanart","remoteUrl":"https://artworks.thetvdb.com/banners/fan.jpg"},{"coverType":"poster","remoteUrl":"https://artworks.thetvdb.com/banners/posters/79488-1.jpg"}]});
    assert_eq!(
        remote::poster_source(&tvdb),
        Some(("tvdb", "/posters/79488-1.jpg".into()))
    );
    for url in [
        "https://evil.example/t/p/original/a.jpg",
        "https://image.tmdb.org.evil.example/t/p/original/a.jpg",
        "https://image.tmdb.org/t/p/original/a.svg",
        "https://image.tmdb.org/t/p/original/../a.jpg",
        "file:///etc/passwd",
        "ftp://image.tmdb.org/t/p/original/a.jpg",
    ] {
        assert_eq!(
            remote::poster_source(&json!({"remotePoster":url})),
            None,
            "{url}"
        );
    }
    assert_eq!(remote::poster_source(&json!({})), None);
}
