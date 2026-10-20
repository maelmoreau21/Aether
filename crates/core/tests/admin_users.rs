use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use media_poc::{
    admin_users,
    auth::{self, AuthState},
    identity::IdentityStore,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;
const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "account test secret long enough";
async fn request(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> Response {
    let mut req = Request::builder()
        .uri(path)
        .method(method)
        .header("content-type", "application/json")
        .header("origin", ORIGIN);
    if !cookie.is_empty() {
        req = req.header("cookie", cookie);
    }
    if !csrf.is_empty() {
        req = req.header("x-csrf-token", csrf);
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn login(app: &Router, name: &str) -> (String, String) {
    let response = request(
        app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":name,"password":PASSWORD}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    (
        cookie,
        body(response).await["csrfToken"].as_str().unwrap().into(),
    )
}
#[tokio::test]
async fn admin_account_lifecycle_authorization_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("accounts.sqlite"))
        .await
        .unwrap();
    let admin_id = store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let state = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(state.clone()).merge(admin_users::router(state));
    assert_eq!(
        request(&app, "/api/v1/admin/users", "GET", "", "", json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (cookie, csrf) = login(&app, "admin").await;
    let create =
        json!({"login":"viewer","password":PASSWORD,"role":"user","displayName":"Lecteur"});
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users",
            "POST",
            &cookie,
            "",
            create.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let response = request(
        &app,
        "/api/v1/admin/users",
        "POST",
        &cookie,
        &csrf,
        create.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let user_id = body(response).await["id"].as_str().unwrap().to_owned();
    assert_eq!(
        request(&app, "/api/v1/admin/users", "POST", &cookie, &csrf, create)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let listing =
        body(request(&app, "/api/v1/admin/users", "GET", &cookie, "", json!(null)).await).await;
    let viewer = listing["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["id"] == user_id)
        .unwrap();
    assert!(viewer["lastSeenAt"].is_null());
    assert_eq!(viewer["hasAvatar"], false);
    let avatar_path = format!("/api/v1/admin/users/{user_id}/avatar");
    assert_eq!(
        request(&app, &avatar_path, "GET", "", "", json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, &avatar_path, "GET", &cookie, "", json!(null))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let avatar_data = vec![137u8, 80, 78, 71];
    media_poc::database::query("INSERT INTO user_avatars(user_id,image_data) VALUES(?,?)")
        .bind(&user_id)
        .bind(avatar_data.clone())
        .execute(store.pool())
        .await
        .unwrap();
    let avatar = request(&app, &avatar_path, "GET", &cookie, "", json!(null)).await;
    assert_eq!(avatar.status(), StatusCode::OK);
    assert_eq!(avatar.headers()["content-type"], "image/png");
    assert_eq!(avatar.headers()["cache-control"], "private, no-store");
    assert_eq!(
        avatar
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .as_ref(),
        avatar_data.as_slice()
    );
    let (user_cookie, _) = login(&app, "viewer").await;
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users",
            "GET",
            &user_cookie,
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, &avatar_path, "GET", &user_cookie, "", json!(null))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let listing =
        body(request(&app, "/api/v1/admin/users", "GET", &cookie, "", json!(null)).await).await;
    let viewer = listing["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|user| user["id"] == user_id)
        .unwrap();
    assert_eq!(viewer["hasAvatar"], true);
    assert!(viewer["lastSeenAt"].as_i64().unwrap() > 0);
    let self_update = json!({"role":"user","disabled":false,"displayName":null});
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/admin/users/{admin_id}"),
            "PUT",
            &cookie,
            &csrf,
            self_update
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let update = json!({"role":"user","disabled":true,"displayName":"Lecteur inactif"});
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/admin/users/{user_id}"),
            "PUT",
            &cookie,
            &csrf,
            update
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/auth/me",
            "GET",
            &user_cookie,
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let users =
        body(request(&app, "/api/v1/admin/users", "GET", &cookie, "", json!(null)).await).await;
    assert!(
        users["users"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u["id"] == user_id && u["disabledAt"].is_number())
    );
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/admin/users/{user_id}"),
            "PUT",
            &cookie,
            &csrf,
            json!({"role":"user","disabled":false,"displayName":null})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let (user_cookie, _) = login(&app, "viewer").await;
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/admin/users/{user_id}/password"),
            "PUT",
            &cookie,
            &csrf,
            json!({"password":"a replacement secret long enough"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/auth/me",
            "GET",
            &user_cookie,
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    // Password-only identities cannot be introduced onto an OIDC account.
    media_poc::database::query("INSERT INTO users(id,role) VALUES('sso-test','user')")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users/sso-test/password",
            "PUT",
            &cookie,
            &csrf,
            json!({"password":PASSWORD})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/users",
            "POST",
            &cookie,
            &csrf,
            json!({"login":"other","password":PASSWORD,"role":"guest"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn concurrent_administrators_cannot_disable_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("race.sqlite"))
        .await
        .unwrap();
    let first = store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let state = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(state.clone()).merge(admin_users::router(state));
    let (cookie, csrf) = login(&app, "admin").await;
    let created = request(
        &app,
        "/api/v1/admin/users",
        "POST",
        &cookie,
        &csrf,
        json!({"login":"second","password":PASSWORD,"role":"administrator"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let second = body(created).await["id"].as_str().unwrap().to_owned();
    let (other_cookie, other_csrf) = login(&app, "second").await;
    let update = json!({"role":"administrator","disabled":true,"displayName":null});
    let path_second = format!("/api/v1/admin/users/{second}");
    let path_first = format!("/api/v1/admin/users/{first}");
    let (a, b) = tokio::join!(
        request(&app, &path_second, "PUT", &cookie, &csrf, update.clone()),
        request(&app, &path_first, "PUT", &other_cookie, &other_csrf, update)
    );
    assert_eq!(
        usize::from(a.status() == StatusCode::NO_CONTENT)
            + usize::from(b.status() == StatusCode::NO_CONTENT),
        1
    );
    let count: i64 = media_poc::database::query_scalar(
        "SELECT COUNT(*) FROM users WHERE role='administrator' AND disabled_at IS NULL",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn user_library_permissions_create_and_update() {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("users_libs.sqlite"))
        .await
        .unwrap();
    let _admin = store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();

    // Insert dummy libraries in database
    media_poc::database::query(
        "INSERT INTO libraries(id, name, kind, node_id, root, enabled) VALUES('lib-films', 'Films', 'films', 'local', '/media/films', 1)"
    )
    .execute(store.pool())
    .await
    .unwrap();
    media_poc::database::query(
        "INSERT INTO libraries(id, name, kind, node_id, root, enabled) VALUES('lib-series', 'Séries', 'series', 'local', '/media/series', 1)"
    )
    .execute(store.pool())
    .await
    .unwrap();

    let state = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(state.clone()).merge(admin_users::router(state));
    let (cookie, csrf) = login(&app, "admin").await;

    // 1. Create user with libraryIds
    let create = json!({
        "login": "tester",
        "password": PASSWORD,
        "role": "user",
        "libraryIds": ["lib-films"]
    });
    let res = request(&app, "/api/v1/admin/users", "POST", &cookie, &csrf, create).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let user_id = body(res).await["id"].as_str().unwrap().to_owned();

    let has_films: i64 = media_poc::database::query_scalar(
        "SELECT COUNT(*) FROM library_permissions WHERE user_id=? AND library_id='lib-films'",
    )
    .bind(&user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(has_films, 1);

    // 2. Update user to have both libraries
    let update = json!({
        "role": "user",
        "disabled": false,
        "displayName": "Tester User",
        "libraryIds": ["lib-films", "lib-series"]
    });
    let res = request(
        &app,
        &format!("/api/v1/admin/users/{user_id}"),
        "PUT",
        &cookie,
        &csrf,
        update,
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let count: i64 = media_poc::database::query_scalar(
        "SELECT COUNT(*) FROM library_permissions WHERE user_id=?",
    )
    .bind(&user_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count, 2);

    // 3. Update with invalid library -> 400 Bad Request
    let bad_update = json!({
        "role": "user",
        "disabled": false,
        "libraryIds": ["nonexistent"]
    });
    let res = request(
        &app,
        &format!("/api/v1/admin/users/{user_id}"),
        "PUT",
        &cookie,
        &csrf,
        bad_update,
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}
