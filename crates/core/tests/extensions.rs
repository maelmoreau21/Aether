use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::BodyExt;
use media_extension_sdk::{ExtensionRequest, Manifest, Package};
use media_poc::{
    auth::{self, AuthState},
    extensions,
    identity::IdentityStore,
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use zeroize::Zeroizing;

fn key() -> Ed25519KeyPair {
    let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap()
}
fn package(key: &Ed25519KeyPair, wasm: Vec<u8>) -> Package {
    let mut p = Package {
        manifest: Manifest {
            api_version: 1,
            id: "test.metadata".into(),
            name: "Test metadata".into(),
            version: "0.1.0".into(),
            description: "Synthetic legal fixture".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/test".into(),
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        },
        publisher_id: "test".into(),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        wasm: STANDARD.encode(wasm),
        signature: String::new(),
    };
    p.signature = STANDARD.encode(key.sign(&p.signing_bytes().unwrap()).as_ref());
    p
}
fn module(body: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module (memory (export "memory") 1 256)
      (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
      (func (export "media_run") (param i32 i32) (result i64) {body}))"#
    ))
    .unwrap()
}
fn good_module() -> Vec<u8> {
    memory_module(1, false)
}
fn memory_module(initial: u32, grow: bool) -> Vec<u8> {
    let result =
        r#"{"apiVersion":1,"result":{"title":"Synthetic title","summary":"Synthetic proposal"}}"#;
    let data = result
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let growth = if grow {
        "i32.const 256 memory.grow i32.const -1 i32.eq if unreachable end"
    } else {
        ""
    };
    wat::parse_str(format!(
        r#"(module (memory (export "memory") {initial} 512) (data (i32.const 4096) "{data}")
      (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
      (func (export "media_run") (param i32 i32) (result i64) {growth} i64.const {}))"#,
        (4096u64 << 32) | result.len() as u64
    ))
    .unwrap()
}
fn input() -> ExtensionRequest {
    ExtensionRequest {
        api_version: 1,
        operation: "metadata.propose".into(),
        input: json!({"apiVersion":1,"workId":"work","libraryId":"films","title":"Original"}),
    }
}

#[test]
fn generic_abi_does_not_require_a_metadata_payload() {
    let result = r#"{"apiVersion":1,"result":{"accepted":true,"value":7}}"#;
    let data = result
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let wasm = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1) (data (i32.const 4096) "{data}")
        (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#,
        (4096u64 << 32) | result.len() as u64
    ))
    .unwrap();
    let response =
        media_poc::extension_runtime::execute(media_poc::extension_runtime::Invocation::new(
            STANDARD.encode(wasm),
            ExtensionRequest {
                api_version: 1,
                operation: "test.compute".into(),
                input: json!({"a":3}),
            },
        ))
        .unwrap();
    assert_eq!(response.result, json!({"accepted":true,"value":7}));
}

#[test]
fn sandbox_bounds_recursion_separately_from_value_stack_bytes() {
    use media_poc::extension_runtime::{Invocation, execute};
    let wasm = |depth: u32| {
        wat::parse_str(format!(
            r#"(module
        (memory (export "memory") 1)
        (data (i32.const 4096) "{{\22apiVersion\22:1,\22result\22:{{}}}}")
        (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
        (func $recur (param $n i32) (result i32)
            local.get $n i32.eqz if (result i32) i32.const 0 else
            local.get $n i32.const 1 i32.sub call $recur i32.const 1 i32.add end)
        (func (export "media_run") (param i32 i32) (result i64)
            i32.const {depth} call $recur drop i64.const {}))"#,
            (4096u64 << 32) | r#"{"apiVersion":1,"result":{}}"#.len() as u64
        ))
        .unwrap()
    };
    assert!(execute(Invocation::new(STANDARD.encode(wasm(16)), input())).is_ok());
    assert_eq!(
        execute(Invocation::new(STANDARD.encode(wasm(2000)), input())).unwrap_err(),
        "execution_trapped"
    );
}

#[test]
fn sandbox_denies_imports_cpu_memory_invalid_pointers_and_output() {
    use media_poc::extension_runtime::{Invocation, execute};
    let run = |wasm: Vec<u8>| execute(Invocation::new(STANDARD.encode(wasm), input()));
    assert_eq!(
        run(good_module()).unwrap().result["title"],
        "Synthetic title"
    );
    assert_eq!(
        run(module("(loop br 0) i64.const 0")).unwrap_err(),
        "execution_trapped"
    );
    assert_eq!(
        run(memory_module(257, false)).unwrap_err(),
        "instantiation_failed"
    );
    assert_eq!(
        run(memory_module(1, true)).unwrap_err(),
        "execution_trapped"
    );
    assert!(run(module("i64.const -1")).is_err());
    assert_eq!(
        run(module("i64.const -4294967295")).unwrap_err(),
        "invalid_pointer"
    );
    assert!(run(module("i64.const 32769")).is_err());
    assert!(
        run(
            wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func)))"#)
                .unwrap()
        )
        .is_err()
    );
    assert!(run(wat::parse_str(r#"(module (memory 257))"#).unwrap()).is_err());
    assert!(
        run(wat::parse_str(r#"(module (func $start (loop br 0)) (start $start))"#).unwrap())
            .is_err()
    );
}
#[test]
fn package_authenticates_manifest_module_publisher_and_compatibility() {
    let key = key();
    let p = package(&key, good_module());
    assert!(extensions::validate_package(&p, key.public_key().as_ref()).is_ok());
    let mut bad = p.clone();
    bad.manifest.name = "Forged".into();
    assert_eq!(
        extensions::validate_package(&bad, key.public_key().as_ref()),
        Err("invalid_signature")
    );
    let mut bad = p.clone();
    bad.sha256 = "0".repeat(64);
    assert_eq!(
        extensions::validate_package(&bad, key.public_key().as_ref()),
        Err("invalid_digest")
    );
    let mut bad = p.clone();
    bad.manifest.api_version = 2;
    assert_eq!(
        extensions::validate_package(&bad, key.public_key().as_ref()),
        Err("incompatible_api")
    );
    let mut bad = p.clone();
    bad.manifest.id = "../escape".into();
    assert_eq!(
        extensions::validate_package(&bad, key.public_key().as_ref()),
        Err("invalid_manifest")
    );
    let mut bad = p.clone();
    bad.manifest.permissions.push("shell".into());
    assert_eq!(
        extensions::validate_package(&bad, key.public_key().as_ref()),
        Err("invalid_manifest")
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
async fn signed_package_real_worker_live_permissions_persistence_and_lifecycle() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "extension-admin",
            Zeroizing::new("synthetic extension password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    for id in ["films", "other"] {
        media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','synthetic',1)").bind(id).bind(id).execute(store.pool()).await.unwrap();
        media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES(?,?,?,'Original','{}')").bind(format!("work-{id}")).bind(id).bind(id).execute(store.pool()).await.unwrap();
    }
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone())
        .merge(media_poc::admin_users::router(auth.clone()))
        .merge(auth::router(auth));
    let base = "/api/v1/admin/extensions";
    assert_eq!(
        call(&app, "GET", base, "", "", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"extension-admin","password":"synthetic extension password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let key = key();
    let p = serde_json::to_value(package(&key, good_module())).unwrap();
    assert_eq!(
        call(&app, "POST", base, &cookie, "", p.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, "POST", base, &cookie, csrf, p.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/publishers"),
            &cookie,
            csrf,
            json!({"id":"test","publicKey":STANDARD.encode(key.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let mut bad = p.clone();
    bad["signature"] = json!(STANDARD.encode([0; 64]));
    assert_eq!(
        call(&app, "POST", base, &cookie, csrf, bad).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(&app, "POST", base, &cookie, csrf, p.clone()).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(&app, "POST", base, &cookie, csrf, p.clone()).await.0,
        StatusCode::CONFLICT
    );
    let endpoint = format!("{base}/test.metadata");
    let run = format!("{endpoint}/run");
    assert_eq!(
        call(
            &app,
            "POST",
            &run,
            &cookie,
            csrf,
            json!({"workId":"work-films"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &endpoint,
            &cookie,
            csrf,
            json!({"enabled":true,"libraryIds":["films"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &run,
            &cookie,
            csrf,
            json!({"workId":"work-other"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, result, _) = call(
        &app,
        "POST",
        &run,
        &cookie,
        csrf,
        json!({"workId":"work-films"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["proposal"]["title"], "Synthetic title");
    let (_, works, _) = call(
        &app,
        "GET",
        &format!("{endpoint}/works"),
        &cookie,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(works["items"].as_array().unwrap().len(), 1);
    assert_eq!(works["items"][0]["libraryId"], "films");

    // Verify extension settings GET and PUT
    let settings_endpoint = format!("{endpoint}/settings");
    let (status, initial_settings, _) =
        call(&app, "GET", &settings_endpoint, &cookie, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial_settings["settings"], json!({}));

    let (status, _, _) = call(
        &app,
        "PUT",
        &settings_endpoint,
        &cookie,
        csrf,
        json!({"api_key": "secret123", "language": "fr-FR", "disallow_custom_avatars": true}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, updated_settings, _) =
        call(&app, "GET", &settings_endpoint, &cookie, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_settings["settings"]["api_key"], "secret123");
    assert_eq!(
        updated_settings["settings"]["disallow_custom_avatars"],
        true
    );

    let (status, list_val, _) = call(&app, "GET", base, &cookie, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list_val["extensions"][0]["settings"]["api_key"],
        "secret123"
    );

    assert_eq!(call(&app,"POST","/api/v1/admin/users",&cookie,csrf,json!({"login":"extension-user","password":"synthetic regular user password","role":"user","displayName":null,"libraryIds":["films"]})).await.0,StatusCode::CREATED);
    let (status, _, regular) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"extension-user","password":"synthetic regular user password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&app, "GET", base, &regular, "", Value::Null).await.0,
        StatusCode::FORBIDDEN
    );
    // Core stays usable after a guest exhausts its fuel.
    assert_eq!(
        call(
            &app,
            "DELETE",
            &endpoint,
            &cookie,
            csrf,
            json!({"deleteData":false})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let malicious = serde_json::to_value(package(&key, module("(loop br 0) i64.const 0"))).unwrap();
    assert_eq!(
        call(&app, "POST", base, &cookie, csrf, malicious).await.0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &endpoint,
            &cookie,
            csrf,
            json!({"enabled":true,"libraryIds":["films"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &run,
            &cookie,
            csrf,
            json!({"workId":"work-films"})
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "GET", base, &cookie, "", Value::Null).await.0,
        StatusCode::OK
    );
    // Revoke then re-enable while the worker is starting: old generation stays invalid.
    let running_app = app.clone();
    let running_cookie = cookie.clone();
    let running_csrf = csrf.to_string();
    let running_url = run.clone();
    let in_flight = tokio::spawn(async move {
        call(
            &running_app,
            "POST",
            &running_url,
            &running_cookie,
            &running_csrf,
            json!({"workId":"work-films"}),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let active: i64 = media_poc::database::query_scalar(
                "SELECT COUNT(*) FROM extension_runs WHERE status='running'",
            )
            .fetch_one(store.pool())
            .await
            .unwrap();
            if active == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    for enabled in [false, true] {
        assert_eq!(
            call(
                &app,
                "PUT",
                &endpoint,
                &cookie,
                csrf,
                json!({"enabled":enabled,"libraryIds":["films"]})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(in_flight.await.unwrap().0, StatusCode::FORBIDDEN);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("{base}/publishers/test"),
            &cookie,
            csrf,
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &run,
            &cookie,
            csrf,
            json!({"workId":"work-films"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, history, _) = call(
        &app,
        "GET",
        &format!("{base}/runs"),
        &cookie,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(history["runs"].as_array().unwrap().len(), 3);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &endpoint,
            &cookie,
            csrf,
            json!({"deleteData":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, history, _) = call(
        &app,
        "GET",
        &format!("{base}/runs"),
        &cookie,
        "",
        Value::Null,
    )
    .await;
    assert!(history["runs"].as_array().unwrap().is_empty());
    store.close().await;
    let reopened = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    let count: i64 = media_poc::database::query_scalar("SELECT COUNT(*) FROM extension_audit")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert!(count >= 6);
    reopened.close().await;
}

#[test]
#[ignore = "requires cargo build -p media-extension-sdk-fixture --target wasm32-unknown-unknown --release"]
fn real_rust_sdk_module_executes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-unknown-unknown/release/media_extension_sdk_fixture.wasm");
    let bytes = std::fs::read(path).unwrap();
    let result = media_poc::extension_runtime::execute(
        media_poc::extension_runtime::Invocation::new(STANDARD.encode(bytes), input()),
    )
    .unwrap();
    assert_eq!(result.result["title"], "Original");
    assert!(result.result["summary"].as_str().unwrap().contains("work"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires built Web, Rust Wasm example and Chrome/Playwright"]
async fn browser_extension_installs_and_executes_real_rust_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "extension-admin",
            Zeroizing::new("synthetic extension password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films synthétiques','films','local',?,1)")
        .bind(media.to_str().unwrap()).execute(store.pool()).await.unwrap();
    media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work-films','films','synthetic','Titre original','{}')")
        .execute(store.pool()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://127.0.0.1:{port}");
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    std::env::set_current_dir(&root).unwrap();
    let app = media_poc::secure_app(
        media_poc::node::Inventory::local(media).await.unwrap(),
        port,
        auth,
    )
    .await
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let key = key();
    let wasm = std::fs::read(
        root.join("target/wasm32-unknown-unknown/release/media_extension_sdk_fixture.wasm"),
    )
    .unwrap();
    let p = package(&key, wasm);
    let path = temp.path().join("package.json");
    std::fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
    let status = tokio::process::Command::new("node")
        .arg("scripts/test-extensions-live.mjs")
        .current_dir(root.join("apps/web"))
        .env("MEDIA_TEST_ORIGIN", origin)
        .env("MEDIA_TEST_PACKAGE", path)
        .env(
            "MEDIA_TEST_PUBLIC_KEY",
            STANDARD.encode(key.public_key().as_ref()),
        )
        .status()
        .await
        .unwrap();
    server.abort();
    store.close().await;
    assert!(status.success());
}

#[test]
fn http_broker_security_enforces_allowed_imports_and_allowlist() {
    use media_poc::extension_runtime::{Invocation, execute};

    // 1. Module with unauthorized imports is rejected immediately
    let bad_import_module = wat::parse_str(
        r#"(module
            (import "env" "unauthorized_syscall" (func))
            (memory (export "memory") 1)
            (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "media_run") (param i32 i32) (result i64) i64.const 0)
        )"#,
    )
    .unwrap();
    let res = execute(Invocation::new(STANDARD.encode(bad_import_module), input()));
    assert_eq!(res.unwrap_err(), "imports_forbidden");

    // 2. Module with legitimate "aether.aether_host_http" import is permitted by the runtime
    let result_str = r#"{"apiVersion":1,"result":{"title":"Processed","summary":"With Broker"}}"#;
    let data = result_str
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let packed_val = (4096u64 << 32) | (result_str.len() as u64);
    let valid_import_module = wat::parse_str(format!(
        r#"(module
            (import "aether" "aether_host_http" (func $host_http (param i32 i32) (result i64)))
            (memory (export "memory") 1)
            (data (i32.const 4096) "{data}")
            (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "media_run") (param i32 i32) (result i64)
                (i64.const {packed_val})
            )
        )"#
    ))
    .unwrap();
    let res = execute(Invocation {
        wasm: STANDARD.encode(valid_import_module),
        input: input(),
        allowed_hosts: vec!["api.themoviedb.org".into()],
        auth_headers: std::collections::BTreeMap::new(),
    });
    let output = res.unwrap();
    assert_eq!(output.result["title"], "Processed");
}

#[test]
fn http_broker_rejects_ssrf_and_enforces_domain_allowlist() {
    use media_extension_sdk::HostHttpRequest;
    use media_poc::extension_runtime::{HostState, execute_host_http};
    use wasmi::StoreLimitsBuilder;

    let limits = StoreLimitsBuilder::new().build();
    let mut state = HostState {
        limits,
        allowed_hosts: vec!["api.themoviedb.org".into()],
        auth_headers: std::collections::BTreeMap::new(),
        http_count: 0,
        max_http_requests: 10,
        max_response_size: 1024,
    };

    // 1. Non-HTTPS scheme is forbidden
    let res = execute_host_http(
        &mut state,
        HostHttpRequest {
            url: "http://api.themoviedb.org/3/movie/550".into(),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            timeout_secs: Some(1),
        },
    );
    assert_eq!(res.unwrap_err().code, "scheme_forbidden");

    // 2. SSRF loopback is forbidden
    let res = execute_host_http(
        &mut state,
        HostHttpRequest {
            url: "https://127.0.0.1:8080/admin".into(),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            timeout_secs: Some(1),
        },
    );
    assert_eq!(res.unwrap_err().code, "host_forbidden");

    // 3. Localhost is forbidden
    let res = execute_host_http(
        &mut state,
        HostHttpRequest {
            url: "https://localhost:8080/admin".into(),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            timeout_secs: Some(1),
        },
    );
    assert_eq!(res.unwrap_err().code, "host_forbidden");

    // 4. Unauthorized domain is forbidden
    let res = execute_host_http(
        &mut state,
        HostHttpRequest {
            url: "https://evil-attacker.example.com/steal".into(),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            timeout_secs: Some(1),
        },
    );
    assert_eq!(res.unwrap_err().code, "domain_not_allowed");

    // 5. Quota exceeded
    state.http_count = 10;
    let res = execute_host_http(
        &mut state,
        HostHttpRequest {
            url: "https://api.themoviedb.org/3/movie/550".into(),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            timeout_secs: Some(1),
        },
    );
    assert_eq!(res.unwrap_err().code, "quota_exceeded");
}

#[test]
fn real_tmdb_wasm_executes_in_aether_runtime() {
    use media_poc::extension_runtime::{Invocation, execute};

    let tmdb_wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../../Aether-Plugins/target/wasm32-unknown-unknown/release/aether_plugin_tmdb.wasm",
    );

    if !tmdb_wasm_path.exists() {
        eprintln!("TMDb wasm not found at {:?}, skipping", tmdb_wasm_path);
        return;
    }

    let bytes = std::fs::read(&tmdb_wasm_path).unwrap();
    let res = execute(Invocation {
        wasm: STANDARD.encode(&bytes),
        input: ExtensionRequest {
            api_version: 1,
            operation: "manifest".into(),
            input: json!({}),
        },
        allowed_hosts: vec!["api.themoviedb.org".into(), "image.tmdb.org".into()],
        auth_headers: std::collections::BTreeMap::new(),
    });

    assert!(res.is_ok(), "Failed to execute TMDb Wasm: {:?}", res.err());
    let output = res.unwrap();
    assert_eq!(output.api_version, 1);
    assert_eq!(output.result["id"], "tmdb");
    assert_eq!(output.result["provider"], "tmdb");
    assert_eq!(output.result["capabilities"][0], "search");

    // A synthetic key supplied through the real settings wire format must get
    // past provider key validation. The broker rejects localhost before I/O.
    let configured = execute(Invocation {
        wasm: STANDARD.encode(&bytes),
        input: ExtensionRequest {
            api_version: 1,
            operation: "metadata.propose".into(),
            input: json!({"title":"Synthetic fixture", "config": {
                "api_key":"synthetic-test-key", "base_url":"https://localhost/3", "language":"en-US"
            }}),
        },
        allowed_hosts: vec!["api.themoviedb.org".into()],
        auth_headers: Default::default(),
    })
    .unwrap();
    assert_eq!(configured.result["error"], "host_forbidden");
}

#[test]
fn real_avatar_wasm_executes_in_aether_runtime() {
    use media_poc::extension_runtime::{Invocation, execute};

    let avatar_wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../../Aether-Plugins/target/wasm32-unknown-unknown/release/aether_plugin_avatar.wasm",
    );

    if !avatar_wasm_path.exists() {
        eprintln!("Avatar wasm not found at {:?}, skipping", avatar_wasm_path);
        return;
    }

    let bytes = std::fs::read(&avatar_wasm_path).unwrap();
    let res = execute(Invocation {
        wasm: STANDARD.encode(&bytes),
        input: ExtensionRequest {
            api_version: 1,
            operation: "avatars.generate".into(),
            input: json!({"seed": "alice"}),
        },
        allowed_hosts: vec![],
        auth_headers: std::collections::BTreeMap::new(),
    });

    assert!(
        res.is_ok(),
        "Failed to execute Avatar Wasm: {:?}",
        res.err()
    );
    let output = res.unwrap();
    assert_eq!(output.api_version, 1);
    assert!(output.result["svg"].as_str().unwrap().contains("<svg"));
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_extension_metadata_updates_inventory_work() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "extension-admin",
            Zeroizing::new("synthetic extension password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();

    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic',1)")
        .execute(store.pool())
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work-1','films','work-1','Original Film','{}')")
        .execute(store.pool())
        .await
        .unwrap();

    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone()).merge(auth::router(auth));

    let (status, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login": "extension-admin", "password": "synthetic extension password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();

    let rich_metadata = json!({
        "title": "Fight Club",
        "plot": "An insomniac office worker...",
        "year": 1999,
        "actors": [
            {"name": "Edward Norton", "role": "The Narrator"},
            {"name": "Brad Pitt", "role": "Tyler Durden"}
        ],
        "ratings": [
            {"provider": "tmdb", "value": 8.4, "max": 10.0}
        ],
        "posterUrl": "https://image.tmdb.org/t/p/original/poster.jpg"
    });

    let (status, _, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions/tmdb/apply",
        &cookie,
        csrf,
        json!({
            "workId": "work-1",
            "metadata": rich_metadata
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify detail reflects the applied metadata
    let detail = media_poc::inventory::detail(&store, "work-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail["title"], "Fight Club");
    assert_eq!(detail["description"], "An insomniac office worker...");
    assert_eq!(detail["actors"][0]["name"], "Edward Norton");
    assert_eq!(detail["actors"][1]["name"], "Brad Pitt");
    assert_eq!(
        detail["posterUrl"],
        "https://image.tmdb.org/t/p/original/poster.jpg"
    );
}
