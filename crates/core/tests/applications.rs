//! Host authorization, real disposable-worker ABI and persisted report lifecycle.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::BodyExt;
use media_extension_sdk::{Manifest, Package};
use media_poc::{
    applications,
    auth::AuthState,
    database::{query, query_scalar},
    extensions,
    identity::IdentityStore,
    reports,
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

// Independent databases still share the process-wide per-extension worker admission.
static WORKER_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    _dir: tempfile::TempDir,
    store: IdentityStore,
    app: Router,
    sessions: Vec<(String, String)>,
}
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("applications.sqlite"))
        .await
        .unwrap();
    let now = media_contracts::protocol::unix_now();
    let mut sessions = Vec::new();
    for (id, role, raw) in [
        ("admin", "administrator", "a".repeat(64)),
        ("alice", "user", "b".repeat(64)),
        ("bob", "user", "c".repeat(64)),
    ] {
        query("INSERT INTO users(id,role) VALUES(?,?)")
            .bind(id)
            .bind(role)
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,?,?,?)")
            .bind(format!("{:x}",Sha256::digest(&raw))).bind(id).bind(now).bind(now).bind(now+3600).execute(store.pool()).await.unwrap();
        sessions.push((
            format!("media_session={raw}"),
            format!("{:x}", Sha256::digest(format!("csrf:{raw}"))),
        ));
    }
    // The fresh schema already seeds the 'local' media node.
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','/test',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO library_permissions(user_id,library_id) VALUES('alice','films')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work-1','films','film:1','Synthetic','{}')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = applications::router(auth.clone())
        .merge(reports::router(auth.clone()))
        .merge(extensions::router(auth));
    Fixture {
        _dir: dir,
        store,
        app,
        sessions,
    }
}
async fn install(store: &IdentityStore, cap: &str) {
    let (package, public_key) = signed_package(cap);
    extensions::install_internal_with_key(store, package, &public_key)
        .await
        .unwrap();
}
fn signed_package(cap: &str) -> (Package, String) {
    let wasm = if let Ok(directory) = std::env::var("MEDIA_APPLICATION_TEST_WASM_DIR") {
        std::fs::read(std::path::Path::new(&directory).join(format!("aether_plugin_{cap}.wasm")))
            .unwrap()
    } else {
        let output = r#"{"apiVersion":1,"result":{"valid":true}}"#;
        let data = output
            .bytes()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        wat::parse_str(format!("(module (memory (export \"memory\") 2) (data (i32.const 0) \"{data}\") (func (export \"media_alloc\") (param i32) (result i32) i32.const 65536) (func (export \"media_run\") (param i32 i32) (result i64) i64.const {}))",output.len())).unwrap()
    };
    let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap();
    let mut package = Package {
        manifest: Manifest {
            api_version: 1,
            id: format!("media-{cap}"),
            name: cap.into(),
            version: "0.1.0".into(),
            description: "Synthetic host fixture".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/test".into(),
            permissions: vec![cap.into()],
            ..Default::default()
        },
        publisher_id: "test".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
    (package, STANDARD.encode(key.public_key().as_ref()))
}
async fn send(
    f: &Fixture,
    who: Option<usize>,
    method: &str,
    path: &str,
    csrf: bool,
    value: Value,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "application/json");
    if let Some(who) = who {
        request = request.header("cookie", &f.sessions[who].0);
        if csrf {
            request = request.header("x-csrf-token", &f.sessions[who].1);
        }
    }
    f.app
        .clone()
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response) -> Value {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| panic!("{status}: {}", String::from_utf8_lossy(&bytes)))
}
#[tokio::test]
async fn optional_features_require_active_signed_publisher_and_authentication() {
    let _worker = WORKER_TEST.lock().await;
    let f = fixture().await;
    assert_eq!(
        send(
            &f,
            None,
            "GET",
            "/api/v1/extensions/features",
            false,
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        body(
            send(
                &f,
                Some(1),
                "GET",
                "/api/v1/extensions/features",
                false,
                Value::Null
            )
            .await
        )
        .await,
        json!({"requests":false,"reports":false})
    );
    install(&f.store, "reports").await;
    assert!(
        body(
            send(
                &f,
                Some(1),
                "GET",
                "/api/v1/extensions/features",
                false,
                Value::Null
            )
            .await
        )
        .await["reports"]
            .as_bool()
            .unwrap()
    );
    query("UPDATE extension_publishers SET active=0")
        .execute(f.store.pool())
        .await
        .unwrap();
    assert_eq!(
        send(&f, Some(1), "GET", "/api/v1/reports", false, Value::Null)
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
#[tokio::test]
async fn reports_are_resource_owned_csrf_protected_persistent_and_reopenable() {
    let _worker = WORKER_TEST.lock().await;
    let f = fixture().await;
    install(&f.store, "reports").await;
    let value = json!({"category":"audio","description":"Audio absent à ce moment","workId":"work-1","positionSeconds":12.5,"endSeconds":13.0,"context":{"engine":"native","errorCode":"audio_missing"}});
    assert_eq!(
        send(&f, Some(1), "POST", "/api/v1/reports", false, value.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&f, Some(2), "POST", "/api/v1/reports", true, value.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = send(&f, Some(1), "POST", "/api/v1/reports", true, value).await;
    let status = response.status();
    let report = body(response).await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    let id = report["id"].as_str().unwrap();
    let path = format!("/api/v1/reports/{id}");
    assert_eq!(report["positionSeconds"], 12.5);
    assert_eq!(
        send(&f, Some(2), "GET", &path, false, Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &f,
            Some(1),
            "POST",
            &format!("{path}/status"),
            true,
            json!({"status":"resolved"})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &f,
            Some(0),
            "POST",
            &format!("{path}/status"),
            true,
            json!({"status":"resolved"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &f,
            Some(1),
            "POST",
            &format!("{path}/status"),
            true,
            json!({"status":"open"})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &f,
            Some(1),
            "POST",
            &format!("{path}/comments"),
            true,
            json!({"message":"Le problème persiste."})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let saved = body(send(&f, Some(1), "GET", &path, false, Value::Null).await).await;
    assert_eq!(saved["status"], "open");
    assert_eq!(saved["comments"].as_array().unwrap().len(), 1);
    query("DELETE FROM library_permissions WHERE user_id='alice'")
        .execute(f.store.pool())
        .await
        .unwrap();
    assert_eq!(
        send(&f, Some(1), "GET", &path, false, Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let persisted: i64 = query_scalar("SELECT COUNT(*) FROM media_reports WHERE id=?")
        .bind(id)
        .fetch_one(f.store.pool())
        .await
        .unwrap();
    assert_eq!(persisted, 1);
    query("UPDATE extensions SET enabled=0 WHERE id='media-reports'")
        .execute(f.store.pool())
        .await
        .unwrap();
    assert_eq!(
        send(&f, Some(0), "GET", &path, false, Value::Null)
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
#[tokio::test]
async fn report_context_is_bounded_and_cannot_reference_another_resource() {
    let _worker = WORKER_TEST.lock().await;
    let f = fixture().await;
    install(&f.store, "reports").await;
    for input in [
        json!({"category":"audio","description":"Pas de ressource"}),
        json!({"category":"audio","description":"Erreur","workId":"work-1","positionSeconds":20,"endSeconds":2}),
        json!({"category":"audio","description":"Erreur","workId":"work-1","context":{"engine":"https://private/path?key=secret"}}),
        json!({"category":"audio","description":"Erreur","workId":"work-1","context":{"audioTrackId":"arbitrary-track"}}),
    ] {
        let response = send(&f, Some(1), "POST", "/api/v1/reports", true, input).await;
        assert!(response.status().is_client_error());
    }
}

#[tokio::test]
async fn reports_preserve_exact_inventory_track_and_reject_wrong_kind_or_asset() {
    let _worker = WORKER_TEST.lock().await;
    let f = fixture().await;
    install(&f.store, "reports").await;
    query("INSERT INTO inventory_editions(id,work_id) VALUES('edition-1','work-1')")
        .execute(f.store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_versions(id,edition_id) VALUES('version-1','edition-1')")
        .execute(f.store.pool())
        .await
        .unwrap();
    query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES('source-1','version-1','films','/test')").execute(f.store.pool()).await.unwrap();
    for asset in ["asset-1", "asset-2"] {
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,'source-1','films',?,1,'1','video/mp4','{}',1)").bind(asset).bind(format!("{asset}.mp4")).execute(f.store.pool()).await.unwrap();
    }
    query("INSERT INTO inventory_tracks(id,asset_id,stream_index,metadata_json) VALUES('audio-1','asset-1',1,?)").bind(json!({"index":1,"codec_type":"audio","codec_name":"opus"}).to_string()).execute(f.store.pool()).await.unwrap();
    let valid = json!({"workId":"work-1","assetId":"asset-1","category":"audio","description":"Piste audio exacte","positionSeconds":30,"context":{"audioTrackId":"audio-1"}});
    let response = send(&f, Some(1), "POST", "/api/v1/reports", true, valid.clone()).await;
    let status = response.status();
    let report = body(response).await;
    assert_eq!(status, StatusCode::CREATED, "{report}");
    assert_eq!(report["context"]["audioTrackId"], "audio-1");
    let mut wrong_asset = valid.clone();
    wrong_asset["assetId"] = json!("asset-2");
    assert_eq!(
        send(&f, Some(1), "POST", "/api/v1/reports", true, wrong_asset)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut wrong_kind = valid;
    wrong_kind["context"] = json!({"subtitleTrackId":"audio-1"});
    assert_eq!(
        send(&f, Some(1), "POST", "/api/v1/reports", true, wrong_kind)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}

async fn all_libraries(store: &IdentityStore, id: &str) -> i64 {
    query_scalar("SELECT CAST(all_libraries AS BIGINT) FROM extensions WHERE id=?")
        .bind(id)
        .fetch_one(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn reports_extension_installed_by_an_administrator_accepts_reports() {
    let _worker = WORKER_TEST.lock().await;
    let f = fixture().await;
    let (package, public_key) = signed_package("reports");
    let mut install = serde_json::to_value(&package).unwrap();
    install["publicKey"] = json!(public_key);
    let response = send(
        &f,
        Some(0),
        "POST",
        "/api/v1/admin/extensions",
        true,
        install,
    )
    .await;
    let status = response.status();
    let installed = body(response).await;
    assert_eq!(status, StatusCode::CREATED, "{installed}");
    // Application surfaces follow each caller's library permissions, not a grant list.
    assert_eq!(all_libraries(&f.store, "media-reports").await, 1);
    // Same body as the administration toggle sends for an extension without grants.
    assert_eq!(
        send(
            &f,
            Some(0),
            "PUT",
            "/api/v1/admin/extensions/media-reports",
            true,
            json!({"enabled":true,"libraryIds":[],"allLibraries":true})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let report = json!({"category":"video","description":"Image figée à la reprise","workId":"work-1","positionSeconds":42});
    for who in [1, 0] {
        let response = send(
            &f,
            Some(who),
            "POST",
            "/api/v1/reports",
            true,
            report.clone(),
        )
        .await;
        let status = response.status();
        let created = body(response).await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
    }
    // A user without permission on the library is still refused.
    let response = send(&f, Some(2), "POST", "/api/v1/reports", true, report).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(body(response).await["error"], "library_permission_denied");
}

#[tokio::test]
async fn startup_repairs_application_extensions_left_without_library_scope() {
    let _worker = WORKER_TEST.lock().await;
    let f = fixture().await;
    install(&f.store, "reports").await;
    install(&f.store, "requests").await;
    // Former default: no library at all for application extensions.
    query("UPDATE extensions SET all_libraries=0")
        .execute(f.store.pool())
        .await
        .unwrap();
    // An explicit grant list is an administrator decision and must survive.
    query("INSERT INTO extension_grants(extension_id,library_id) VALUES('media-requests','films')")
        .execute(f.store.pool())
        .await
        .unwrap();
    let report = json!({"category":"audio","description":"Son désynchronisé","workId":"work-1"});
    let response = send(&f, Some(1), "POST", "/api/v1/reports", true, report.clone()).await;
    assert_eq!(body(response).await["error"], "library_permission_denied");
    let reopened = IdentityStore::open(&f._dir.path().join("applications.sqlite"))
        .await
        .unwrap();
    assert_eq!(all_libraries(&reopened, "media-reports").await, 1);
    assert_eq!(all_libraries(&reopened, "media-requests").await, 0);
    let response = send(&f, Some(1), "POST", "/api/v1/reports", true, report).await;
    let status = response.status();
    let created = body(response).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}
