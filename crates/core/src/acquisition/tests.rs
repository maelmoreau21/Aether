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
    query(
        "INSERT INTO acquisition_servers(id,name,kind,base_url,api_key,allowed_ips_json,\
            library_id,root_folder,quality_profile_id,is_default,enabled,generation) \
            VALUES('radarr-a','Radarr A','radarr','http://127.0.0.1:7878/','test-key',\
            '[\"127.0.0.1\"]','request-films','/movies',1,1,1,'g')",
    )
    .execute(store.pool())
    .await
    .unwrap();
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
    query(
        "INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,\
            external_provider,input_json,status,created_at,updated_at) \
            VALUES('fixture','owner','radarr-a','request-films','film','Fixture','123','tmdb',?,\
            'pending',?,?)",
    )
    .bind(input.to_string())
    .bind(now())
    .bind(now())
    .execute(store.pool())
    .await
    .unwrap();
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
    let tvdb_alt = json!({"images":[{"coverType":"Poster","url":"https://thetvdb.com/banners/posters/12345.JPG"}]});
    assert_eq!(
        remote::poster_source(&tvdb_alt),
        Some(("tvdb", "/posters/12345.JPG".into()))
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
#[tokio::test]
async fn destinations_expose_quality_profiles_and_input_validates_profile_id() {
    let (_dir, store, app, sessions) = fixture().await;
    let owner = &sessions[0].0;
    query("UPDATE acquisition_servers SET quality_profiles_json=? WHERE id='radarr-a'")
        .bind(json!([{"id":1,"name":"HD-1080p"},{"id":4,"name":"Ultra-HD"}]).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let (status, body) = request(
        &app,
        "GET",
        "/api/v1/requests/destinations",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["servers"][0]["qualityProfileId"], 1);
    assert_eq!(
        body["servers"][0]["qualityProfiles"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let mut valid_input = film("101");
    valid_input.quality_profile_id = Some(4);
    assert!(valid_input.valid());
    valid_input.quality_profile_id = Some(0);
    assert!(!valid_input.valid());
}
#[tokio::test]
async fn test_connection_discovers_arr_options_and_persists_server_settings() {
    let (_dir, store, app, sessions) = fixture().await;
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('request-series','Series','series','local','/series',1)")
        .execute(store.pool())
        .await
        .unwrap();
    let (admin, csrf) = (&sessions[2].0, &sessions[2].1);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mock_arr = Router::new()
        .route(
            "/api/v3/system/status",
            axum::routing::get(|| async {
                Json(
                    json!({"appName":"Sonarr","version":"4.0.9.2244","instanceName":"Sonarr Main"}),
                )
            }),
        )
        .route(
            "/api/v3/rootfolder",
            axum::routing::get(|| async {
                Json(json!([{"id":1,"path":"/tv","accessible":true,"freeSpace":500000000000_i64}]))
            }),
        )
        .route(
            "/api/v3/qualityprofile",
            axum::routing::get(|| async { Json(json!([{"id":1,"name":"HD-1080p"}])) }),
        )
        .route(
            "/api/v3/tag",
            axum::routing::get(|| async { Json(json!([{"id":3,"label":"aether"}])) }),
        )
        .route(
            "/api/v3/languageprofile",
            axum::routing::get(|| async { StatusCode::NOT_FOUND }),
        );
    let task = tokio::spawn(async move { axum::serve(listener, mock_arr).await.unwrap() });

    let (status, body) = request(
        &app,
        "POST",
        "/api/v1/admin/acquisition/servers/test",
        admin,
        csrf,
        json!({
            "kind": "sonarr",
            "baseUrl": format!("http://{addr}"),
            "apiKey": "test-key"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(body["system"]["appName"], "Sonarr");
    assert_eq!(body["options"]["rootFolders"][0]["path"], "/tv");
    assert_eq!(body["options"]["qualityProfiles"][0]["name"], "HD-1080p");
    assert_eq!(body["options"]["tags"][0]["label"], "aether");
    assert_eq!(body["options"]["languageProfiles"], json!([]));
    task.abort();

    let (status, _) = request(
        &app,
        "POST",
        "/api/v1/admin/acquisition/servers",
        admin,
        csrf,
        json!({
            "name": "Sonarr Main",
            "kind": "sonarr",
            "baseUrl": format!("http://{addr}"),
            "apiKey": "test-key",
            "libraryId": "request-series",
            "rootFolder": "/tv",
            "qualityProfileId": 1,
            "qualityProfiles": [{"id":1,"name":"HD-1080p"}],
            "settings": {
                "tags": [3],
                "availableTags": [{"id":3,"label":"aether"}],
                "searchOnAdd": false,
                "seriesType": "anime",
                "seasonFolder": true,
                "animeQualityProfileId": 1,
                "animeRootFolder": "/anime",
                "animeTags": [3]
            },
            "isDefault": false,
            "enabled": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, invalid) = request(
        &app,
        "POST",
        "/api/v1/admin/acquisition/servers",
        admin,
        csrf,
        json!({
            "name": "Bad Radarr",
            "kind": "radarr",
            "baseUrl": format!("http://{addr}"),
            "apiKey": "test-key",
            "libraryId": "request-films",
            "rootFolder": "/movies",
            "qualityProfileId": 1,
            "settings": { "minimumAvailability": "bogus" },
            "enabled": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["error"], "invalid_acquisition_server");
}
#[tokio::test]
async fn locations_keep_primary_and_child_folders_and_test_server_syncs_profiles() {
    let (_dir, store, app, sessions) = fixture().await;
    let (admin, admin_csrf) = &sessions[2];
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('child-films','Films','films','local','/media-2',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO library_folders(library_id,parent_id,active) VALUES('child-films','request-films',1)")
        .execute(store.pool())
        .await
        .unwrap();
    let (status, locs) = request(
        &app,
        "GET",
        "/api/v1/admin/acquisition/locations",
        admin,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<&str> = locs["locations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"request-films"),
        "primary folder must remain selectable: {ids:?}"
    );
    assert!(
        ids.contains(&"child-films"),
        "child folder must be selectable: {ids:?}"
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mock_arr = Router::new()
        .route(
            "/api/v3/system/status",
            axum::routing::get(|| async {
                Json(json!({"appName":"Radarr","version":"5.14.0","instanceName":"Radarr"}))
            }),
        )
        .route(
            "/api/v3/rootfolder",
            axum::routing::get(|| async {
                Json(json!([{"id":1,"path":"/movies","accessible":true}]))
            }),
        )
        .route(
            "/api/v3/qualityprofile",
            axum::routing::get(|| async {
                Json(json!([{"id":1,"name":"HD-1080p"},{"id":6,"name":"Ultra-HD"}]))
            }),
        )
        .route(
            "/api/v3/tag",
            axum::routing::get(|| async { Json(json!([{"id":2,"label":"4k"}])) }),
        );
    let task = tokio::spawn(async move { axum::serve(listener, mock_arr).await.unwrap() });
    query("UPDATE acquisition_servers SET base_url=?,quality_profile_id=6,quality_profiles_json='[]' WHERE id='radarr-a'")
        .bind(format!("http://{addr}/"))
        .execute(store.pool())
        .await
        .unwrap();
    let (status, _) = request(
        &app,
        "POST",
        "/api/v1/admin/acquisition/servers/radarr-a/test",
        admin,
        admin_csrf,
        Value::Null,
    )
    .await;
    task.abort();
    assert_eq!(status, StatusCode::OK);
    let synced = server(&store, "radarr-a").await.unwrap();
    assert_eq!(
        synced.quality_profiles(),
        vec![
            QualityProfile {
                id: 1,
                name: "HD-1080p".into()
            },
            QualityProfile {
                id: 6,
                name: "Ultra-HD".into()
            },
        ]
    );
}
#[tokio::test]
async fn authorization_picks_authorized_destination_and_enforces_advanced_and_author_access() {
    let (_dir, store, app, sessions) = fixture().await;
    let (owner, owner_csrf) = &sessions[0];
    let (other, _) = &sessions[1];
    let (admin, admin_csrf) = &sessions[2];
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films-4k','Films 4K','films','local','/media-4k',1)")
        .execute(store.pool())
        .await
        .unwrap();
    query(
        "INSERT INTO acquisition_servers(id,name,kind,base_url,api_key,allowed_ips_json,\
            library_id,root_folder,quality_profile_id,quality_profiles_json,is_default,enabled,generation) \
            VALUES('radarr-4k','Radarr 4K','radarr','http://127.0.0.1:7879/','test-key',\
            '[\"127.0.0.1\"]','films-4k','/movies-4k',6,'[{\"id\":6,\"name\":\"Ultra-HD\"},{\"id\":7,\"name\":\"REMUX\"}]',1,1,'g2')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    query("UPDATE acquisition_servers SET is_default=0 WHERE id='radarr-a'")
        .execute(store.pool())
        .await
        .unwrap();
    // `owner` only has access to `request-films` (radarr-a), NOT `films-4k` (the default radarr-4k).
    let (status, dests) = request(
        &app,
        "GET",
        "/api/v1/requests/destinations",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dests["servers"].as_array().unwrap().len(), 1);
    assert_eq!(dests["servers"][0]["id"], "radarr-a");
    assert_eq!(dests["servers"][0]["libraryName"], "Films");
    // `choose(None)` inside `discover` / `policy` must pick `radarr-a` instead of failing on `radarr-4k`.
    let (_, mine) = request(
        &app,
        "GET",
        "/api/v1/requests/policy",
        owner,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(mine["classes"]["film"]["destination"], true);
    assert_eq!(mine["classes"]["film"]["advanced"], true);

    // Grant `owner` access to `films-4k` too, then test `advanced: false` enforcement.
    query("INSERT INTO library_permissions(user_id,library_id) VALUES('owner','films-4k')")
        .execute(store.pool())
        .await
        .unwrap();
    let (status, _) = request(
        &app,
        "PUT",
        "/api/v1/admin/acquisition/policies/users/owner",
        admin,
        admin_csrf,
        json!({"film":{"advanced":false}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, denied_server) = request(
        &app,
        "POST",
        "/api/v1/requests",
        owner,
        owner_csrf,
        json!({"kind":"film","title":"X","externalId":"10","externalProvider":"tmdb","serverId":"radarr-a"}),
    )
    .await;
    assert_eq!(
        (status, &denied_server["error"]),
        (
            StatusCode::FORBIDDEN,
            &json!("request_advanced_permission_denied")
        )
    );
    let (status, denied_profile) = request(
        &app,
        "POST",
        "/api/v1/requests",
        owner,
        owner_csrf,
        json!({"kind":"film","title":"X","externalId":"10","externalProvider":"tmdb","qualityProfileId":7}),
    )
    .await;
    assert_eq!(
        (status, &denied_profile["error"]),
        (
            StatusCode::FORBIDDEN,
            &json!("request_advanced_permission_denied")
        )
    );

    // An admin cannot route a pending request to a library the requester cannot access.
    query("DELETE FROM library_permissions WHERE user_id='owner' AND library_id='films-4k'")
        .execute(store.pool())
        .await
        .unwrap();
    let input = json!({"kind":"film","title":"Pending","externalId":"77","externalProvider":"tmdb","serverId":"radarr-a"});
    query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,created_at,updated_at) VALUES('req-perm','owner','radarr-a','request-films','film','Pending','77','tmdb',?,'pending',?,?)")
        .bind(input.to_string())
        .bind(now())
        .bind(now())
        .execute(store.pool())
        .await
        .unwrap();
    let (status, err_body) = request(
        &app,
        "POST",
        "/api/v1/requests/req-perm/action",
        admin,
        admin_csrf,
        json!({"action":"approve","serverId":"radarr-4k"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{err_body}");
    assert_eq!(err_body["error"], "library_permission_denied");

    // Duplicate films on the same server are rejected even if qualityProfileId differs.
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let eff = policy::effective(&store, "owner", false).await.unwrap();
    let srv = server(&store, "radarr-a").await.unwrap();
    let mut dup = film("77");
    dup.quality_profile_id = Some(9);
    let dup_err = http::insert_request(&auth, "owner", &eff, &srv, &dup)
        .await
        .unwrap_err();
    assert_eq!(error_code(&dup_err), "request_already_exists");

    // `list()` counts must ignore requests in unauthorized libraries.
    query("INSERT INTO media_requests(id,user_id,server_id,library_id,kind,title,external_id,external_provider,input_json,status,created_at,updated_at) VALUES('req-other','other','radarr-4k','films-4k','film','Other','88','tmdb',?,'pending',?,?)")
        .bind(input.to_string())
        .bind(now())
        .bind(now())
        .execute(store.pool())
        .await
        .unwrap();
    let (_, other_list) = request(&app, "GET", "/api/v1/requests", other, "", Value::Null).await;
    assert_eq!(other_list["requests"], json!([]));
    assert_eq!(other_list["counts"], json!({}));
}
