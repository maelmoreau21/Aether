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
