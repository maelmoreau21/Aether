use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use media_poc::{
    auth::{self, AuthState},
    identity::IdentityStore,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use zeroize::Zeroizing;
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> (StatusCode, Value, String) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "http://127.0.0.1:3000")
        .header(header::COOKIE, cookie)
        .header("x-csrf-token", csrf)
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string())
        .unwrap_or_default();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn variant_imports_preserve_other_images_and_deduplicate_by_language() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "art-admin",
            Zeroizing::new("synthetic artwork password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic',1)").execute(store.pool()).await.unwrap();
    media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','films','work','Film','{}')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = media_poc::artwork::router(auth.clone())
        .merge(media_poc::extensions::router(auth.clone()))
        .merge(auth::router(auth));
    let (status, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"art-admin","password":"synthetic artwork password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let collect = "/api/v1/admin/extensions/images/collect";
    for (body, expected) in [
        (json!({"workId":"missing"}), StatusCode::NOT_FOUND),
        (json!({"workId":"work"}), StatusCode::UNPROCESSABLE_ENTITY),
        (
            json!({"workId":"work","maxImages":13}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"workId":"work","languages":["fr--CA"]}),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        assert_eq!(
            call(&app, "POST", collect, &cookie, csrf, body).await.0,
            expected
        );
    }
    assert_eq!(
        call(&app, "POST", collect, &cookie, "", json!({"workId":"work"}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "POST", collect, "", "", json!({"workId":"work"}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let upload = |url: String, color: [u8; 3]| {
        let app = app.clone();
        let cookie = cookie.clone();
        let csrf = csrf.to_owned();
        async move {
            let mut png = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(12, 18, image::Rgb(color)))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(url)
                        .header(header::CONTENT_TYPE, "image/png")
                        .header(header::ORIGIN, "http://127.0.0.1:3000")
                        .header(header::COOKIE, cookie)
                        .header("x-csrf-token", csrf)
                        .body(Body::from(png.into_inner()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap_or(Value::Null);
            (status, body)
        }
    };
    let base = "/api/v1/works/work/images/poster";
    let (_, first) = upload(base.into(), [255, 0, 0]).await;
    let (status, duplicate) = upload(format!("{base}?append=true"), [255, 0, 0]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(duplicate["id"], first["id"]);
    assert_eq!(duplicate["duplicate"], true);
    assert_eq!(
        upload(format!("{base}?append=true&language=fr"), [255, 0, 0])
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        upload(format!("{base}?append=true&language=fr"), [0, 0, 255])
            .await
            .0,
        StatusCode::CREATED
    );
    let (status, images, _) = call(
        &app,
        "GET",
        "/api/v1/works/work/images",
        &cookie,
        csrf,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(images["images"].as_array().unwrap().len(), 3);
    assert!(
        images["images"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["language"] == "fr")
    );
    assert_eq!(
        upload(format!("{base}?append=true&language=fr--CA"), [0, 0, 255])
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // Mark the harvested variants as provider-owned and verify a manual override retains their pool.
    media_poc::database::query(
        "UPDATE work_image_entries SET provider='test.provider' WHERE language='fr'",
    )
    .execute(store.pool())
    .await
    .unwrap();
    assert_eq!(
        upload(base.into(), [0, 255, 0]).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/works/work/images",
            &cookie,
            csrf,
            Value::Null
        )
        .await
        .1["images"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/works/work/images",
            "",
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
