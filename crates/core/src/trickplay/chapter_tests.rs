//! Real Core HTTP authorization and catalogue chapter references, without FFmpeg.
use super::*;
use crate::{
    auth::{self, AuthState},
    database::{query, query_scalar},
    identity::IdentityStore,
};
use axum::{
    Router,
    body::Body,
    http::{Request as HttpRequest, header},
    middleware,
    routing::get,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get_json(app: &Router, path: &str, cookie: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .uri(path)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
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
async fn chapter_images_use_only_real_boundaries_and_live_resource_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("chapters.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "chapter-admin",
            zeroize::Zeroizing::new("synthetic chapter password".into()),
        )
        .await
        .unwrap();
    for statement in [
        "INSERT INTO media_nodes(id,name,kind,status) VALUES('chapter-node','Fixture','remote','connected')",
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('chapter-library','Fixture','films','chapter-node','/read-only',1)",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('chapter-work','chapter-library','fixture','Fixture','{}')",
        "INSERT INTO inventory_editions(id,work_id) VALUES('chapter-edition','chapter-work')",
        "INSERT INTO inventory_versions(id,edition_id) VALUES('chapter-version','chapter-edition')",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('chapter-source','chapter-version','chapter-library','/read-only')",
        "INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('chapter-asset','chapter-source','chapter-library','private-film.mp4',100,'1','video/mp4','{}',1)",
    ] {
        query(statement).execute(store.pool()).await.unwrap();
    }
    query("UPDATE libraries SET options_json='{\"chapterImagesEnabled\":false}' WHERE id='chapter-library'")
        .execute(store.pool()).await.unwrap();
    let probe=json!({"format":{"duration":"120"},"chapters":[{"start_time":"60","tags":{"title":"Second"}},{"start_time":"0","tags":{"title":"First"}},{"start_time":"NaN"},{"start_time":"-1"},{"start_time":"120"}]}).to_string();
    query("UPDATE inventory_assets SET probe_json=? WHERE id='chapter-asset'")
        .bind(&probe)
        .execute(store.pool())
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let media = Router::new()
        .route(
            "/api/v1/assets/{id}/chapter-images",
            get(crate::indexed_chapter_manifest),
        )
        .route(
            "/api/v1/assets/{id}/chapter-images/{index}",
            get(crate::indexed_chapter_image),
        )
        .layer(axum::Extension(store.clone()))
        .layer(middleware::from_fn_with_state(
            crate::permissions::MediaAccess {
                auth: auth.clone(),
                root: temp.path().into(),
            },
            crate::permissions::require_media,
        ));
    let app = media.merge(auth::router(auth));
    let login = app
        .clone()
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .body(Body::from(
                    json!({"login":"chapter-admin","password":"synthetic chapter password"})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let path = "/api/v1/assets/chapter-asset/chapter-images";
    assert_eq!(get_json(&app, path, "").await.0, StatusCode::UNAUTHORIZED);
    let (status, body) = get_json(&app, path, &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "chapter_images_disabled");
    query("UPDATE libraries SET options_json=? WHERE id='chapter-library'")
        .bind(json!({"chapterImagesEnabled":true}).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let (status, body) = get_json(&app, path, &cookie).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(body["chapters"][0]["title"], "First");
    assert_eq!(body["chapters"][0]["start"], 0.0);
    assert_eq!(
        body["chapters"][0]["imageUrl"],
        "/api/v1/assets/chapter-asset/chapter-images/0"
    );
    assert!(!body.to_string().contains("private-film"));
    assert!(!body.to_string().contains("read-only"));
    assert_eq!(
        get_json(&app, &format!("{path}/999"), &cookie).await.0,
        StatusCode::NOT_FOUND
    );
    query("UPDATE inventory_assets SET probe_json=? WHERE id='chapter-asset'")
        .bind(json!({"format":{"duration":"120"},"chapters":[]}).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(get_json(&app, path, &cookie).await.1["chapters"], json!([]));
    assert_eq!(
        get_json(&app, &format!("{path}/0"), &cookie).await.0,
        StatusCode::NOT_FOUND
    );
    query("UPDATE inventory_assets SET probe_json=? WHERE id='chapter-asset'")
        .bind(probe)
        .execute(store.pool())
        .await
        .unwrap();
    let user: String = query_scalar("SELECT id FROM users WHERE role='administrator'")
        .fetch_one(store.pool())
        .await
        .unwrap();
    query("UPDATE users SET role='user' WHERE id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(get_json(&app, path, &cookie).await.0, StatusCode::FORBIDDEN);
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('chapter-library',?)")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(get_json(&app, path, &cookie).await.0, StatusCode::OK);
    assert_eq!(
        get_json(&app, "/api/v1/assets/foreign-asset/chapter-images", &cookie)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    query("UPDATE media_nodes SET status='revoked' WHERE id='chapter-node'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(get_json(&app, path, &cookie).await.0, StatusCode::FORBIDDEN);
    query("UPDATE media_nodes SET status='connected' WHERE id='chapter-node'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE libraries SET enabled=0 WHERE id='chapter-library'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(get_json(&app, path, &cookie).await.0, StatusCode::FORBIDDEN);
}
