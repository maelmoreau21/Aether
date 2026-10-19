use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    middleware,
    response::Response,
    routing::get,
};
use http_body_util::BodyExt;
use media_poc::{
    admin_users, api_keys,
    auth::{self, AuthState},
    database::{query, query_scalar},
    groups,
    identity::IdentityStore,
    permissions::{self, MediaAccess},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;
const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "integration account strong secret";
async fn req(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    token: &str,
    body: Value,
) -> Response {
    let mut request = Request::builder()
        .uri(path)
        .method(method)
        .header("content-type", "application/json")
        .header("origin", ORIGIN);
    if !cookie.is_empty() {
        request = request.header("cookie", cookie);
    }
    if !csrf.is_empty() {
        request = request.header("x-csrf-token", csrf);
    }
    if !token.is_empty() {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn login(app: &Router, name: &str) -> (String, String) {
    let response = req(
        app,
        "/api/v1/auth/login",
        "POST",
        "",
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
async fn setup() -> (
    tempfile::TempDir,
    IdentityStore,
    Router,
    String,
    String,
    String,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("access.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    for lib in ["first", "second"] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','/private/media',1)").bind(lib).bind(lib).execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind,title_sort) VALUES(?,?,?,?,'{}','film',?)").bind(format!("work-{lib}")).bind(lib).bind(lib).bind(format!("Film {lib}")).bind(lib).execute(store.pool()).await.unwrap();
    }
    let state = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let protected = Router::new()
        .route(
            "/api/v1/catalog/{id}",
            get(|| async { Json(json!({"ok":true})) }),
        )
        .layer(middleware::from_fn_with_state(
            MediaAccess {
                auth: state.clone(),
                root: dir.path().to_owned(),
            },
            permissions::require_media,
        ));
    let app = auth::router(state.clone())
        .merge(admin_users::router(state.clone()))
        .merge(groups::router(state.clone()))
        .merge(api_keys::router(state))
        .merge(protected);
    let (cookie, csrf) = login(&app, "admin").await;
    let response = req(
        &app,
        "/api/v1/admin/users",
        "POST",
        &cookie,
        &csrf,
        "",
        json!({"login":"viewer","password":PASSWORD,"role":"user"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = body(response).await["id"].as_str().unwrap().into();
    let (viewer, _) = login(&app, "viewer").await;
    (dir, store, app, cookie, csrf, id, viewer)
}
#[tokio::test]
async fn group_membership_grants_live_permissions_without_roles_or_loss_of_direct_grants() {
    let (_dir, store, app, cookie, csrf, user, viewer) = setup().await;
    let input = json!({"name":"Famille","userIds":[user],"libraryIds":["first"]});
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/groups",
            "POST",
            "",
            "",
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/groups",
            "POST",
            &cookie,
            "",
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/groups",
            "POST",
            &viewer,
            &csrf,
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/catalog/work-first",
            "GET",
            &viewer,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let response = req(
        &app,
        "/api/v1/admin/groups",
        "POST",
        &cookie,
        &csrf,
        "",
        input.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = body(response).await["id"].as_str().unwrap().to_owned();
    assert_eq!(
        req(
            &app,
            "/api/v1/catalog/work-first",
            "GET",
            &viewer,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/catalog/work-second",
            "GET",
            &viewer,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let role: String = query_scalar("SELECT role FROM users WHERE id=?")
        .bind(&user)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(role, "user");
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('second',?)")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    let path = format!("/api/v1/admin/groups/{id}");
    assert_eq!(
        req(
            &app,
            &path,
            "PUT",
            &cookie,
            &csrf,
            "",
            json!({"name":"Famille","userIds":[],"libraryIds":["first"]})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/catalog/work-first",
            "GET",
            &viewer,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/catalog/work-second",
            "GET",
            &viewer,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        req(&app, &path, "DELETE", &cookie, &csrf, "", json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/catalog/work-second",
            "GET",
            &viewer,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/groups",
            "POST",
            &cookie,
            &csrf,
            "",
            json!({"name":"Invalid","userIds":["missing"],"libraryIds":["first"]})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn integration_keys_are_scoped_hashed_live_and_never_administrator_sessions() {
    let (_dir, store, app, cookie, csrf, user, viewer) = setup().await;
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('first',?)")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    let input = json!({"name":"Catalogue","userId":user,"libraryIds":["first"],"expiresInDays":30});
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/api-keys",
            "POST",
            &cookie,
            "",
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/api-keys",
            "POST",
            &viewer,
            &csrf,
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/api-keys",
            "POST",
            &cookie,
            &csrf,
            "",
            json!({"name":"Denied","userId":user,"libraryIds":["second"],"expiresInDays":30})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = req(
        &app,
        "/api/v1/admin/api-keys",
        "POST",
        &cookie,
        &csrf,
        "",
        input,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let key = body(response).await;
    let id = key["id"].as_str().unwrap();
    let token = key["token"].as_str().unwrap();
    let stored: String = query_scalar("SELECT token_hash FROM api_keys WHERE id=?")
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_ne!(stored, token);
    assert_eq!(stored.len(), 64);
    let response = req(
        &app,
        "/api/v1/integrations/catalog",
        "GET",
        "",
        "",
        token,
        json!(null),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let catalog = body(response).await;
    assert_eq!(catalog["items"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["items"][0]["libraryId"], "first");
    assert!(!catalog.to_string().contains("/private/media"));
    let listed = body(
        req(
            &app,
            "/api/v1/admin/api-keys",
            "GET",
            &cookie,
            "",
            "",
            json!(null),
        )
        .await,
    )
    .await;
    assert!(!listed.to_string().contains(token));
    assert!(!listed.to_string().contains(&stored));
    assert_eq!(
        req(
            &app,
            "/api/v1/admin/users",
            "GET",
            "",
            "",
            token,
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/integrations/catalog",
            "GET",
            &cookie,
            "",
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/integrations/catalog?limit=101",
            "GET",
            "",
            "",
            token,
            json!(null)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    query("UPDATE users SET disabled_at=unixepoch() WHERE id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        req(
            &app,
            "/api/v1/integrations/catalog",
            "GET",
            "",
            "",
            token,
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    query("UPDATE users SET disabled_at=NULL WHERE id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    query("DELETE FROM library_permissions WHERE user_id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    let empty = body(
        req(
            &app,
            "/api/v1/integrations/catalog",
            "GET",
            "",
            "",
            token,
            json!(null),
        )
        .await,
    )
    .await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('first',?)")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE libraries SET enabled=0 WHERE id='first'")
        .execute(store.pool())
        .await
        .unwrap();
    let empty = body(
        req(
            &app,
            "/api/v1/integrations/catalog",
            "GET",
            "",
            "",
            token,
            json!(null),
        )
        .await,
    )
    .await;
    assert!(empty["items"].as_array().unwrap().is_empty());
    query("UPDATE libraries SET enabled=1 WHERE id='first'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE api_keys SET expires_at=unixepoch()-1 WHERE id=?")
        .bind(id)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        req(
            &app,
            "/api/v1/integrations/catalog",
            "GET",
            "",
            "",
            token,
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    query("UPDATE api_keys SET expires_at=unixepoch()+3600 WHERE id=?")
        .bind(id)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        req(
            &app,
            &format!("/api/v1/admin/api-keys/{id}"),
            "DELETE",
            &cookie,
            &csrf,
            "",
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        req(
            &app,
            "/api/v1/integrations/catalog",
            "GET",
            "",
            "",
            token,
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}
