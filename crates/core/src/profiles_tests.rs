//! Account profiles through the real router: ownership, PIN, switching and inherited access.
use super::*;
use axum::{
    body::Body,
    http::{Request, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    (cookie, csrf): &(String, String),
    body: Value,
) -> (StatusCode, Value, Option<String>) {
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
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        set_cookie,
    )
}

async fn session(auth: &AuthState, user: &str, token: &str) -> (String, String) {
    let now = media_contracts::protocol::unix_now();
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,?,?,?)")
        .bind(crate::auth::hash(token)).bind(user).bind(now).bind(now).bind(now + 3600)
        .execute(auth.store.pool()).await.unwrap();
    let cookie = format!("media_session={token}");
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie.parse().unwrap());
    let csrf = auth.authenticate(&headers).await.unwrap().csrf_token;
    (cookie, csrf)
}

async fn count(auth: &AuthState, sql: &'static str, bind: &str) -> i64 {
    query_scalar(sql)
        .bind(bind)
        .fetch_one(auth.store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn profiles_are_owned_pin_protected_and_inherit_library_access() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("profiles.sqlite"))
        .await
        .unwrap();
    for id in ["owner", "stranger"] {
        query("INSERT INTO users(id,role,display_name) VALUES(?,'user',?)")
            .bind(id)
            .bind(id)
            .execute(store.pool())
            .await
            .unwrap();
    }
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib','lib','films','local','/read-only/lib',1)")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('lib','owner')")
        .execute(store.pool())
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = router(auth.clone());
    let owner = session(&auth, "owner", &"a".repeat(64)).await;
    let stranger = session(&auth, "stranger", &"b".repeat(64)).await;
    let base = "/api/v1/account/profiles";

    let (status, _, _) = call(&app, "POST", base, &owner, json!({"name":" padded"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, created, _) = call(
        &app,
        "POST",
        base,
        &owner,
        json!({"name":"Kid","maximumAge":10}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let kid = created["id"].as_str().unwrap().to_owned();
    assert_eq!(
        count(
            &auth,
            "SELECT maximum_age FROM parental_policies WHERE user_id=?",
            &kid
        )
        .await,
        10
    );
    // Library access follows the owner, without any grant of its own.
    assert_eq!(
        count(&auth, "SELECT COUNT(*) FROM effective_library_permissions WHERE library_id='lib' AND user_id=?", &kid).await,
        1
    );

    let (_, listed, _) = call(&app, "GET", base, &owner, Value::Null).await;
    assert_eq!(listed["canManage"], true);
    assert_eq!(listed["profiles"].as_array().unwrap().len(), 2);
    assert_eq!(listed["profiles"][0]["isOwner"], true);
    assert_eq!(listed["profiles"][1]["restricted"], true);
    // Another account sees only itself and cannot touch this account's profiles.
    let (_, other, _) = call(&app, "GET", base, &stranger, Value::Null).await;
    assert_eq!(other["profiles"].as_array().unwrap().len(), 1);
    let profile = format!("{base}/{kid}");
    assert_eq!(
        call(&app, "DELETE", &profile, &stranger, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{profile}/switch"),
            &stranger,
            json!({})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let pin = format!("{profile}/pin");
    assert_eq!(
        call(&app, "PUT", &pin, &owner, json!({"pin":"12a4"}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "PUT", &pin, &owner, json!({"pin":"1234"}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let switch = format!("{profile}/switch");
    let (status, denied, _) = call(&app, "POST", &switch, &owner, json!({"pin":"0000"})).await;
    assert_eq!(
        (status, denied["error"].as_str()),
        (StatusCode::FORBIDDEN, Some("invalid_pin"))
    );
    let (status, switched, cookie) =
        call(&app, "POST", &switch, &owner, json!({"pin":"1234"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(switched["user"]["id"], kid);
    assert!(cookie.unwrap().starts_with("media_session="));
    // The owner's session on this device is closed by the switch.
    assert_eq!(
        count(
            &auth,
            "SELECT COUNT(*) FROM sessions WHERE token_hash=?",
            &crate::auth::hash(&"a".repeat(64))
        )
        .await,
        0
    );

    // A profile cannot manage the account, nor read another profile's PIN-protected session.
    let as_kid = session(&auth, &kid, &"c".repeat(64)).await;
    let (_, kid_view, _) = call(&app, "GET", base, &as_kid, Value::Null).await;
    assert_eq!(kid_view["canManage"], false);
    assert_eq!(kid_view["currentId"], kid);
    assert_eq!(
        call(&app, "POST", base, &as_kid, json!({"name":"Sneaky"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("{base}/owner/pin"),
            &as_kid,
            json!({"pin":"9999"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // Changing its own PIN requires the current one.
    assert_eq!(
        call(&app, "PUT", &pin, &as_kid, json!({"pin":"5678"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    // The owner protects itself, then wrong guesses are throttled per profile.
    let owner = session(&auth, "owner", &"d".repeat(64)).await;
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("{base}/owner/pin"),
            &owner,
            json!({"pin":"4321"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let to_owner = format!("{base}/owner/switch");
    for _ in 0..PIN_ATTEMPTS {
        assert_eq!(
            call(&app, "POST", &to_owner, &as_kid, json!({"pin":"0000"}))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        call(&app, "POST", &to_owner, &as_kid, json!({"pin":"4321"}))
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );

    assert_eq!(
        call(&app, "DELETE", &profile, &owner, Value::Null).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        count(&auth, "SELECT COUNT(*) FROM users WHERE id=?", &kid).await,
        0
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("{base}/owner"),
            &owner,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
