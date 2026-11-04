//! Offline contracts and a real HTTP/SQLite/signed-Wasm acquisition path.
use super::*;
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

const DOWNLOAD_URL: &str = "https://subtitle-fixture.example.org/subtitle.srt";

fn track() -> Value {
    json!({"id":"42","language":"fr","label":"Synthetic release","forced":true,"sdh":false,"match":"identity"})
}

async fn fixture(store: &crate::identity::IdentityStore) {
    for statement in [
        "INSERT INTO media_nodes(id,name,kind,status) VALUES('subtitle-node','Fixture Node','remote','connected')",
        "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('subtitle-library','Fixture films','films','subtitle-node','/read-only',1)",
        "INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('subtitle-work','subtitle-library','fixture-identity','Fixture film','{}')",
        "INSERT INTO inventory_editions(id,work_id) VALUES('subtitle-edition','subtitle-work')",
        "INSERT INTO inventory_versions(id,edition_id) VALUES('subtitle-version','subtitle-edition')",
        "INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('subtitle-source','subtitle-version','subtitle-library','/read-only')",
    ] {
        query(statement).execute(store.pool()).await.unwrap();
    }
    for id in ["asset-1", "asset-2", "asset-3"] {
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,'subtitle-source','subtitle-library',?,100,'1','video/mp4','{}',1)")
            .bind(id).bind(format!("{id}.mp4")).execute(store.pool()).await.unwrap();
    }
}

#[test]
fn contracts_strip_secrets_reject_bad_results_and_require_subtitle_permission() {
    let mut candidate = track();
    candidate["url"] = json!("https://example.invalid/private?token=synthetic");
    candidate["token"] = json!("synthetic-private-token");
    let result = validate_result(
        "subtitles.search",
        json!({"items":[candidate],"totalPages":1,"private":"hidden"}),
    )
    .unwrap();
    assert!(result["items"][0].get("url").is_none());
    assert!(result["items"][0].get("token").is_none());
    assert!(result.get("private").is_none());
    for (field, invalid) in [
        ("id", json!("../42")),
        ("language", json!("fr/../../")),
        ("label", json!("line\nbreak")),
        ("forced", json!("true")),
        ("sdh", Value::Null),
        ("match", json!("hash")),
    ] {
        let mut item = track();
        item[field] = invalid;
        assert!(
            validate_result("subtitles.search", json!({"items":[item],"totalPages":1})).is_err(),
            "{field}"
        );
    }
    for output in [
        json!({"items":[track(),track()],"totalPages":1}),
        json!({"items":vec![track();61],"totalPages":1}),
        json!({"items":[],"totalPages":10001}),
    ] {
        assert!(validate_result("subtitles.search", output).is_err());
    }
    assert!(validate_result("subtitles.download", json!({"url":"invalid url"})).is_err());
    assert!(
        serde_json::from_value::<DownloadInput>(
            json!({"searchId":"s","fileId":"42","url":DOWNLOAD_URL})
        )
        .is_err()
    );
    let mut manifest = Manifest {
        permissions: vec!["metadata.read".into()],
        ..Default::default()
    };
    for operation in ["subtitles.search", "subtitles.download"] {
        assert_eq!(
            adapters::authorize_operation(&manifest, operation)
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
    manifest.permissions.push("subtitles".into());
    assert!(adapters::authorize_operation(&manifest, "subtitles.download").is_ok());
    let mut options = crate::library_options::LibraryOptions::default();
    assert_eq!(availability(&options), Some("subtitle_download_disabled"));
    options.subtitle_download_enabled = true;
    assert_eq!(availability(&options), Some("subtitle_hash_required"));
    options.subtitle_require_hash = false;
    assert_eq!(availability(&options), None);
}

#[tokio::test]
async fn duplicate_reservations_are_serialized_and_quotas_include_pending_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("quota.sqlite"))
        .await
        .unwrap();
    fixture(&store).await;
    let candidate = track();
    let (a, b) = tokio::join!(
        reserve(
            &store,
            "attempt-a",
            "asset-1",
            "fixture.provider",
            "42",
            "100:1",
            &candidate
        ),
        reserve(
            &store,
            "attempt-b",
            "asset-1",
            "fixture.provider",
            "42",
            "100:1",
            &candidate
        )
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let error = if let Err(error) = a {
        error
    } else {
        b.unwrap_err()
    };
    assert_eq!(error.1.0["error"], "subtitle_attempt_exists");
    query("UPDATE subtitle_downloads SET status='failed'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        reserve(
            &store,
            "retry",
            "asset-1",
            "fixture.provider",
            "42",
            "100:1",
            &candidate
        )
        .await
        .unwrap_err()
        .1
        .0["error"],
        "subtitle_attempt_exists"
    );
    for index in 0..15 {
        reserve(
            &store,
            &format!("pending-{index}"),
            "asset-1",
            "fixture.provider",
            &index.to_string(),
            "100:1",
            &candidate,
        )
        .await
        .unwrap();
        query("UPDATE subtitle_downloads SET created_at=0 WHERE status='pending'")
            .execute(store.pool())
            .await
            .unwrap();
    }
    assert_eq!(
        reserve(
            &store,
            "overflow",
            "asset-1",
            "fixture.provider",
            "17",
            "100:1",
            &candidate
        )
        .await
        .unwrap_err()
        .1
        .0["error"],
        "subtitle_storage_quota"
    );
    query("WITH RECURSIVE sequence(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM sequence WHERE n<240) INSERT INTO subtitle_downloads(id,asset_id,provider_id,file_id,fingerprint,track_json,status,created_at) SELECT 'global-'||n,'asset-2','fixture.provider',CAST(n AS TEXT),'100:1',?,'pending',0 FROM sequence")
        .bind(candidate.to_string()).execute(store.pool()).await.unwrap();
    assert_eq!(
        reserve(
            &store,
            "global-overflow",
            "asset-3",
            "fixture.provider",
            "42",
            "100:1",
            &candidate
        )
        .await
        .unwrap_err()
        .1
        .0["error"],
        "subtitle_storage_quota"
    );
    record_quota(
        &store,
        "quota.fixture",
        &json!({"remaining":0,"resetTimeUtc":"2099-01-01T00:00:00Z"}),
    )
    .await
    .unwrap();
    assert_eq!(
        check_download_limit(&store, "quota.fixture")
            .await
            .unwrap_err()
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    store.pool().close().await;
    drop(store);
    let reopened = crate::identity::IdentityStore::open(&temp.path().join("quota.sqlite"))
        .await
        .unwrap();
    let error = check_download_limit(&reopened, "quota.fixture")
        .await
        .unwrap_err();
    assert_eq!(error.1.0["error"], "subtitle_download_quota");
    assert!(error.1.0["retryAfterSeconds"].as_i64().unwrap() > 0);
    record_quota(&reopened, "quota.fixture", &json!({"remaining":1}))
        .await
        .unwrap();
    assert!(
        check_download_limit(&reopened, "quota.fixture")
            .await
            .is_ok()
    );
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> (StatusCode, Value, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::COOKIE, cookie)
                .header("x-csrf-token", csrf)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_owned();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

fn signed_module() -> (ring::signature::Ed25519KeyPair, Package) {
    use ring::signature::KeyPair;
    let key =
        ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let key = ring::signature::Ed25519KeyPair::from_pkcs8(key.as_ref()).unwrap();
    let response=json!({"apiVersion":1,"result":{"items":[track()],"totalPages":1,"url":DOWNLOAD_URL,"remaining":9}}).to_string();
    let data = response
        .bytes()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let wasm=wat::parse_str(format!(r#"(module (memory (export "memory") 1 256) (data (i32.const 4096) "{data}") (func (export "media_alloc") (param i32) (result i32) i32.const 1024) (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#,(4096u64<<32)|response.len() as u64)).unwrap();
    let mut package = Package {
        manifest: Manifest {
            api_version: 1,
            id: "fixture.subtitles".into(),
            name: "Offline subtitles".into(),
            version: "1.0.0".into(),
            description: "Synthetic fixture".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/fixture".into(),
            permissions: vec!["subtitles".into()],
            http_hosts: vec!["subtitle-fixture.example.org".into()],
            ..Default::default()
        },
        publisher_id: "fixture-signing".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
    assert!(!key.public_key().as_ref().is_empty());
    (key, package)
}

fn subtitle_media_router(auth: AuthState, root: std::path::PathBuf) -> Router {
    Router::new()
        .route(
            "/api/v1/assets/{id}/subtitles/{index}",
            get(crate::indexed_subtitle).head(crate::indexed_subtitle),
        )
        .layer(axum::Extension(auth.store.clone()))
        .route_layer(axum::middleware::from_fn_with_state(
            crate::permissions::MediaAccess { auth, root },
            crate::permissions::require_media,
        ))
}

#[tokio::test]
async fn signed_wasm_http_download_survives_reopen_and_requires_admin_csrf() {
    use ring::signature::KeyPair;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("vertical.sqlite");
    let store = crate::identity::IdentityStore::open(&path).await.unwrap();
    let admin_id = store
        .bootstrap(
            "subtitle-admin",
            zeroize::Zeroizing::new("synthetic subtitle password".into()),
        )
        .await
        .unwrap();
    fixture(&store).await;
    query("UPDATE libraries SET options_json=? WHERE id='subtitle-library'")
        .bind(json!({"subtitleDownloadEnabled":true,"subtitleRequireHash":false}).to_string())
        .execute(store.pool())
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = super::super::router(auth.clone())
        .merge(crate::auth::router(auth.clone()))
        .merge(subtitle_media_router(auth.clone(), temp.path().to_owned()));
    let (status, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"subtitle-admin","password":"synthetic subtitle password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let (key, package) = signed_module();
    let mut install = serde_json::to_value(package).unwrap();
    install["publicKey"] = json!(STANDARD.encode(key.public_key().as_ref()));
    let (status, output, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions",
        &cookie,
        csrf,
        install,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{output}");
    let (status, output, _) = call(
        &app,
        "PUT",
        "/api/v1/admin/extensions/fixture.subtitles",
        &cookie,
        csrf,
        json!({"enabled":true,"libraryIds":["subtitle-library"]}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{output}");
    let search_path = "/api/v1/admin/subtitles/asset-1/search";
    let input = json!({"providerId":"fixture.subtitles","language":"fr"});
    assert_eq!(
        call(&app, "POST", search_path, "", "", input.clone())
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "POST", search_path, &cookie, "", input.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, search, _) = call(&app, "POST", search_path, &cookie, csrf, input).await;
    assert_eq!(status, StatusCode::OK, "{search}");
    assert!(search["items"][0].get("url").is_none());
    crate::provider_download::test_documents()
        .lock()
        .unwrap()
        .insert(
            DOWNLOAD_URL.into(),
            b"1\n00:00:00,000 --> 00:00:02,000\nSynthetic subtitle\n".to_vec(),
        );
    let download_path = "/api/v1/admin/subtitles/asset-1/download";
    let input = json!({"searchId":search["searchId"],"fileId":"42"});
    let (status, result, _) = call(&app, "POST", download_path, &cookie, csrf, input.clone()).await;
    crate::provider_download::test_documents()
        .lock()
        .unwrap()
        .remove(DOWNLOAD_URL);
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        call(&app, "POST", download_path, &cookie, csrf, input)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let id = result["id"].as_str().unwrap().to_owned();
    assert_eq!(descriptors(&store, "asset-1").await.unwrap().len(), 1);
    let content_path = format!("/api/v1/assets/asset-1/subtitles/downloaded-{id}");
    assert_eq!(
        call(&app, "GET", &content_path, "", "", json!({})).await.0,
        StatusCode::UNAUTHORIZED
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&content_path)
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "private, no-store"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("Synthetic subtitle")
    );
    drop(app);
    drop(auth);
    store.pool().close().await;
    drop(store);
    let store = crate::identity::IdentityStore::open(&path).await.unwrap();
    assert_eq!(descriptors(&store, "asset-1").await.unwrap().len(), 1);
    assert_eq!(
        content(&store, "asset-1", &id, true).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        content(&store, "asset-2", &id, false).await.status(),
        StatusCode::NOT_FOUND
    );
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = router()
        .with_state(auth.clone())
        .merge(crate::auth::router(auth.clone()))
        .merge(subtitle_media_router(auth, temp.path().to_owned()));
    query("UPDATE inventory_assets SET modified_ns='2' WHERE id='asset-1'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(descriptors(&store, "asset-1").await.unwrap().is_empty());
    assert_eq!(
        content(&store, "asset-1", &id, false).await.status(),
        StatusCode::NOT_FOUND
    );
    let (status, listed, _) = call(
        &app,
        "GET",
        "/api/v1/admin/subtitles/asset-1",
        &cookie,
        "",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["downloads"][0]["status"], "stale");
    query("UPDATE inventory_assets SET modified_ns='1' WHERE id='asset-1'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(&app, "HEAD", &content_path, &cookie, "", json!({}))
            .await
            .0,
        StatusCode::OK
    );
    let endpoint = format!("/api/v1/admin/subtitles/asset-1/{id}");
    assert_eq!(
        call(&app, "DELETE", &endpoint, &cookie, "", json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    query("UPDATE users SET role='user' WHERE id=?")
        .bind(&admin_id)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(&app, "DELETE", &endpoint, &cookie, csrf, json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    query("UPDATE users SET role='administrator' WHERE id=?")
        .bind(admin_id)
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE inventory_assets SET available=0 WHERE id='asset-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE libraries SET enabled=0 WHERE id='subtitle-library'")
        .execute(store.pool())
        .await
        .unwrap();
    let (status, listed, _) = call(
        &app,
        "GET",
        "/api/v1/admin/subtitles/asset-1",
        &cookie,
        "",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["enabled"], false);
    assert_eq!(listed["unavailableCode"], "subtitle_asset_unavailable");
    assert_eq!(listed["downloads"][0]["id"], id);
    assert!(listed["providers"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            &app,
            "POST",
            search_path,
            &cookie,
            csrf,
            json!({"providerId":"fixture.subtitles","language":"fr"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "DELETE", &endpoint, &cookie, csrf, json!({}))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        content(&store, "asset-1", &id, false).await.status(),
        StatusCode::NOT_FOUND
    );
    query("UPDATE inventory_assets SET available=1 WHERE id='asset-1'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE libraries SET enabled=1 WHERE id='subtitle-library'")
        .execute(store.pool())
        .await
        .unwrap();
    reserve(
        &store,
        "active",
        "asset-1",
        "fixture.provider",
        "43",
        "100:1",
        &track(),
    )
    .await
    .unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/subtitles/asset-1/active",
            &cookie,
            csrf,
            json!({})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    query("UPDATE subtitle_downloads SET created_at=? WHERE id='active'")
        .bind(now() - RESERVATION_SECONDS - 1)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/admin/subtitles/asset-1/active",
            &cookie,
            csrf,
            json!({})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_subtitle_schema_reservations_and_derivatives_survive_reopen() {
    let url = zeroize::Zeroizing::new(
        std::env::var("MEDIA_TEST_POSTGRES_URL")
            .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL database"),
    );
    let first = crate::identity::IdentityStore::open_postgres(&url)
        .await
        .unwrap();
    fixture(&first).await;
    // A second independently opened pool reruns additive schema initialization.
    let second = crate::identity::IdentityStore::open_postgres(&url)
        .await
        .unwrap();
    let tables:i64=query_scalar("SELECT COUNT(*)::bigint FROM information_schema.tables WHERE table_schema=current_schema() AND table_name IN ('subtitle_searches','subtitle_downloads','subtitle_provider_limits','subtitle_download_limits')")
        .fetch_one(second.pool()).await.unwrap();
    assert_eq!(tables, 4);
    let candidate = track();
    let (a, b) = tokio::join!(
        reserve(
            &first,
            "pg-attempt-a",
            "asset-1",
            "fixture.provider",
            "42",
            "100:1",
            &candidate
        ),
        reserve(
            &second,
            "pg-attempt-b",
            "asset-1",
            "fixture.provider",
            "42",
            "100:1",
            &candidate
        ),
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let error = if let Err(error) = a {
        error
    } else {
        b.unwrap_err()
    };
    assert_eq!(error.1.0["error"], "subtitle_attempt_exists");
    let id: String = query_scalar("SELECT id FROM subtitle_downloads WHERE asset_id='asset-1'")
        .fetch_one(first.pool())
        .await
        .unwrap();
    query("UPDATE subtitle_downloads SET status='completed',content=? WHERE id=?")
        .bind("WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nPostgreSQL derivative\n")
        .bind(&id)
        .execute(first.pool())
        .await
        .unwrap();
    assert_eq!(descriptors(&second, "asset-1").await.unwrap().len(), 1);
    record_quota(
        &first,
        "fixture.provider",
        &json!({"remaining":0,"resetTimeUtc":"2099-01-01T00:00:00Z"}),
    )
    .await
    .unwrap();
    first.close().await;
    second.close().await;
    let reopened = crate::identity::IdentityStore::open_postgres(&url)
        .await
        .unwrap();
    assert_eq!(descriptors(&reopened, "asset-1").await.unwrap().len(), 1);
    assert_eq!(
        check_download_limit(&reopened, "fixture.provider")
            .await
            .unwrap_err()
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let response = content(&reopened, "asset-1", &id, false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("PostgreSQL derivative")
    );
    assert_eq!(
        content(&reopened, "asset-1", &id, true).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        content(&reopened, "asset-2", &id, false).await.status(),
        StatusCode::NOT_FOUND
    );
    query("UPDATE inventory_assets SET modified_ns='2' WHERE id='asset-1'")
        .execute(reopened.pool())
        .await
        .unwrap();
    assert!(descriptors(&reopened, "asset-1").await.unwrap().is_empty());
    assert_eq!(
        content(&reopened, "asset-1", &id, false).await.status(),
        StatusCode::NOT_FOUND
    );
    reopened.close().await;
}
