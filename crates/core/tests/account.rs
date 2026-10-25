use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use media_poc::{
    account,
    auth::{self, AuthState},
    identity::IdentityStore,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;
const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "personal account integration secret";
async fn req(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .header("origin", ORIGIN)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .header("user-agent", "Integration browser")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn body(r: axum::response::Response) -> Value {
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn login(app: &Router, password: &str) -> (String, String) {
    let r = req(
        app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":"admin","password":password}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    (cookie, body(r).await["csrfToken"].as_str().unwrap().into())
}
#[tokio::test]
async fn password_sessions_revocation_and_csrf() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("accounts.db"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store, ORIGIN.into()).await.unwrap();
    let app = auth::router(auth.clone()).merge(account::router(auth));
    let (a, csrf) = login(&app, PASSWORD).await;
    let (b, _) = login(&app, PASSWORD).await;
    let listed =
        body(req(&app, "/api/v1/account/sessions", "GET", &a, "", json!(null)).await).await;
    let devices = listed["sessions"].as_array().unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["device"], "Integration browser");
    let id = devices.iter().find(|d| d["current"] == false).unwrap()["id"]
        .as_str()
        .unwrap();
    let path = format!("/api/v1/account/sessions/{id}");
    assert_eq!(
        req(&app, &path, "DELETE", &a, "", json!(null))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(&app, &path, "DELETE", &a, &csrf, json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(&app, "/api/v1/auth/me", "GET", &b, "", json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/account/password",
            "PUT",
            &a,
            &csrf,
            json!({"currentPassword":"wrong","newPassword":"replacement integration secret"})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/account/password",
            "PUT",
            &a,
            &csrf,
            json!({"currentPassword":PASSWORD,"newPassword":"replacement integration secret"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(&app, "/api/v1/auth/me", "GET", &a, "", json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (c, new_csrf) = login(&app, "replacement integration secret").await;
    assert_eq!(
        req(
            &app,
            "/api/v1/account/sessions/unknown",
            "DELETE",
            &c,
            &new_csrf,
            json!(null)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/account/sessions",
            "DELETE",
            &c,
            &new_csrf,
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(&app, "/api/v1/auth/me", "GET", &c, "", json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn persistent_session_survives_return_and_expires_after_seven_days() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("persistent.db"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let pool = store.pool().clone();
    let auth = AuthState::new(store, ORIGIN.into()).await.unwrap();
    let app = auth::router(auth.clone()).merge(account::router(auth));
    let response = req(
        &app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":"admin","password":PASSWORD}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(cookie.contains("Max-Age=2592000"));
    let cookie = cookie.split(';').next().unwrap();
    sqlx::query("UPDATE sessions SET last_seen_at=unixepoch()-86400")
        .execute(&pool)
        .await
        .unwrap();
    let listed = req(
        &app,
        "/api/v1/account/sessions",
        "GET",
        cookie,
        "",
        json!(null),
    )
    .await;
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(body(listed).await["sessions"].as_array().unwrap().len(), 1);
    sqlx::query("UPDATE sessions SET last_seen_at=unixepoch()-604801")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        req(&app, "/api/v1/auth/me", "GET", cookie, "", json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn sso_local_credentials_policy_csrf_and_recent_login() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("policy.db"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(auth.clone())
        .merge(account::router(auth.clone()))
        .merge(media_poc::server_settings::router(auth));
    let (cookie, csrf) = login(&app, PASSWORD).await;
    let user: String = media_poc::database::query_scalar(
        "SELECT user_id FROM local_credentials WHERE login='admin'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    media_poc::database::query("INSERT INTO oidc_identities(issuer,subject,user_id) VALUES('https://idp.invalid','synthetic',?)").bind(&user).execute(store.pool()).await.unwrap();
    media_poc::database::query("DELETE FROM local_credentials WHERE user_id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    let path = "/api/v1/account/local-credentials";
    let input = json!({"login":"new-local","password":"new synthetic local password"});
    assert_eq!(
        req(&app, path, "POST", &cookie, &csrf, input.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/settings",
            "PUT",
            &cookie,
            &csrf,
            json!({"serverName":"Test","welcomeMessage":"","blockSsoLocalAccounts":false})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(req(&app, path, "GET", &cookie, "", json!(null)).await).await["canCreate"],
        true
    );
    assert_eq!(
        req(&app, path, "POST", &cookie, "", input.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    media_poc::database::query("UPDATE sessions SET created_at=unixepoch()-301 WHERE user_id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        req(&app, path, "POST", &cookie, &csrf, input.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    media_poc::database::query("UPDATE sessions SET created_at=unixepoch() WHERE user_id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        req(&app, path, "POST", &cookie, &csrf, input.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(&app, path, "POST", &cookie, &csrf, input)
            .await
            .status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn avatar_policy_enforced_for_active_plugins() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("avatar-policy.db"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = auth::router(auth.clone()).merge(media_poc::avatars::router(auth));
    let (cookie, csrf) = login(&app, PASSWORD).await;
    let path = "/api/v1/account/avatar/policy";
    assert_eq!(
        req(&app, path, "GET", "", "", json!(null)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        body(req(&app, path, "GET", &cookie, "", json!(null)).await).await,
        json!({"uploadAllowed":true,"removeAllowed":true})
    );
    let package = media_extension_sdk::Package {
        manifest: media_extension_sdk::Manifest {
            permissions: vec!["avatars".into()],
            ..Default::default()
        },
        publisher_id: "fixture".into(),
        sha256: String::new(),
        wasm: String::new(),
        signature: String::new(),
    };
    media_poc::database::query(
        "INSERT INTO extension_publishers(id,public_key) VALUES('fixture','synthetic')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    media_poc::database::query("INSERT INTO extensions(id,publisher_id,package_json,enabled,generation) VALUES('fixture','fixture',?,1,'fixture')").bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    media_poc::database::query(
        "INSERT INTO extension_settings(extension_id,settings_json) VALUES('fixture',?)",
    )
    .bind(json!({"disallow_custom_avatars":true,"force_random_on_creation":true}).to_string())
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        body(req(&app, path, "GET", &cookie, "", json!(null)).await).await,
        json!({"uploadAllowed":false,"removeAllowed":false})
    );
    for method in ["PUT", "DELETE"] {
        assert_eq!(
            req(
                &app,
                "/api/v1/account/avatar",
                method,
                &cookie,
                &csrf,
                json!(null)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    media_poc::database::query("UPDATE extensions SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        body(req(&app, path, "GET", &cookie, "", json!(null)).await).await,
        json!({"uploadAllowed":true,"removeAllowed":true})
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/account/avatar",
            "DELETE",
            &cookie,
            &csrf,
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
}
