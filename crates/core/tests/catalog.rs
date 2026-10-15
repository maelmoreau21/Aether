use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use media_poc::{app, node::Inventory};
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;

async fn router() -> axum::Router {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    app(
        Inventory::load(root).await.expect("prepared fixtures"),
        3000,
    )
}

async fn get(router: axum::Router, path: &str) -> (StatusCode, Value) {
    let response = router
        .oneshot(
            Request::builder()
                .uri(path)
                .header("host", "127.0.0.1:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn catalog_separates_films_series_and_playable_episodes() {
    let router = router().await;
    for (kind, id) in [("film", "fixture-film"), ("series", "fixture-series")] {
        let (status, page) = get(router.clone(), &format!("/api/v1/catalog?kind={kind}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], 1);
        assert_eq!(page["items"][0]["id"], id);
        assert_eq!(page["items"][0]["kind"], kind);
        assert!(page["items"][0]["year"].is_null());
    }
    let (_, film) = get(router.clone(), "/api/v1/catalog/fixture-film").await;
    assert!(film["seasons"].as_array().unwrap().is_empty());
    assert_eq!(film["editions"][0]["versions"].as_array().unwrap().len(), 1);
    let sources = film["editions"][0]["versions"][0]["sources"]
        .as_array()
        .unwrap();
    assert_eq!(sources.len(), 2);
    let (_, demo) = get(router.clone(), "/api/v1/demo/media").await;
    for source in sources {
        assert!(
            demo["media"]
                .as_array()
                .unwrap()
                .iter()
                .any(|media| media["id"] == source["playbackMediaId"])
        );
        for asset in source["assetIds"].as_array().unwrap() {
            let request = Request::builder()
                .method("HEAD")
                .uri(format!(
                    "/api/v1/assets/{}/content",
                    asset.as_str().unwrap()
                ))
                .header("host", "127.0.0.1:3000")
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                StatusCode::OK
            );
        }
    }
    let (_, series) = get(router.clone(), "/api/v1/catalog/fixture-series").await;
    assert!(series["editions"].as_array().unwrap().is_empty());
    let episode = &series["seasons"][0]["episodes"][0];
    assert_eq!(episode["kind"], "episode");
    assert_eq!(episode["number"], 1);
    let (_, detail) = get(router.clone(), "/api/v1/catalog/fixture-episode-1").await;
    assert_eq!(detail["id"], episode["id"]);
    assert_eq!(detail["editions"], episode["editions"]);
    let serialized = serde_json::to_string(&series).unwrap();
    assert!(!serialized.contains("filename"));
    assert!(!serialized.contains("C:\\"));
    assert!(!serialized.contains("generated/"));
}

#[tokio::test]
async fn catalog_search_pagination_errors_and_origin_guards() {
    let router = router().await;
    let (_, first) = get(router.clone(), "/api/v1/catalog?limit=1&sort=title-asc").await;
    let (_, second) = get(
        router.clone(),
        "/api/v1/catalog?limit=1&offset=1&sort=title-asc",
    )
    .await;
    assert_eq!(first["total"], 2);
    assert_ne!(first["items"][0]["id"], second["items"][0]["id"]);
    let (_, reverse) = get(router.clone(), "/api/v1/catalog?limit=1&sort=title-desc").await;
    assert_eq!(reverse["items"], second["items"]);
    for query in ["q=missing", "offset=1000", "kind=film&q=series"] {
        let (_, page) = get(router.clone(), &format!("/api/v1/catalog?{query}")).await;
        assert!(page["items"].as_array().unwrap().is_empty());
    }
    for query in [
        "kind=episode",
        "kind=all",
        "sort=recent",
        "limit=0",
        "limit=101",
        "offset=-1",
        "limit=invalid",
        "path=private",
    ] {
        let (status, body) = get(router.clone(), &format!("/api/v1/catalog?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body["error"], "invalid_catalog_query");
    }
    assert_eq!(
        get(router.clone(), "/api/v1/catalog/unknown").await.0,
        StatusCode::NOT_FOUND
    );
    for route in ["/api/v1/catalog", "/api/v1/catalog/fixture-film"] {
        for (header, value) in [
            ("host", "evil.test"),
            ("origin", "https://evil.test"),
            ("sec-fetch-site", "cross-site"),
        ] {
            let request = Request::builder()
                .uri(route)
                .header("host", "127.0.0.1:3000");
            let request = if header == "host" {
                Request::builder().uri(route).header(header, value)
            } else {
                request.header(header, value)
            };
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(route)
                    .header("host", "127.0.0.1:3000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}

#[tokio::test]
async fn unprepared_p1_inventory_exposes_an_empty_catalog_and_no_details() {
    let temp = tempfile::tempdir().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/generated");
    for (_, name, _) in media_poc::node::FIXTURES {
        std::fs::copy(root.join(name), temp.path().join(name)).unwrap();
    }
    let router = app(Inventory::load(temp.path().to_owned()).await.unwrap(), 3000);
    let (status, page) = get(router.clone(), "/api/v1/catalog").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 0);
    assert!(page["items"].as_array().unwrap().is_empty());
    let (status, error) = get(router, "/api/v1/catalog/fixture-film").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"], "unknown_work");
}
