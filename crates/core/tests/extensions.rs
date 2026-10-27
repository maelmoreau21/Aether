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
    let mut private = p.clone();
    private.manifest.permissions.push("storage.private".into());
    private.signature = STANDARD.encode(key.sign(&private.signing_bytes().unwrap()).as_ref());
    assert!(extensions::validate_package(&private, key.public_key().as_ref()).is_ok());
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
    let mut with_network = p.clone();
    with_network.manifest.http_hosts = vec!["api.provider.example".into()];
    assert_eq!(
        extensions::validate_package(&with_network, key.public_key().as_ref()),
        Err("invalid_signature")
    );
    with_network.signature =
        STANDARD.encode(key.sign(&with_network.signing_bytes().unwrap()).as_ref());
    assert!(extensions::validate_package(&with_network, key.public_key().as_ref()).is_ok());
    with_network.manifest.http_hosts = vec!["*.provider.example".into()];
    assert_eq!(
        extensions::validate_package(&with_network, key.public_key().as_ref()),
        Err("invalid_manifest")
    );
    assert!(
        serde_json::to_value(&p.manifest)
            .unwrap()
            .get("httpHosts")
            .is_none()
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
async fn poster_rotation_policy_follows_install_grants_settings_and_disable() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "poster-admin",
            Zeroizing::new("synthetic poster password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    for id in ["films", "other"] {
        media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,'films','local','synthetic',1)")
            .bind(id).bind(id).execute(store.pool()).await.unwrap();
    }
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone()).merge(auth::router(auth));
    let endpoint = "/api/v1/artwork/rotation-policy";
    assert_eq!(
        call(&app, "GET", endpoint, "", "", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"poster-admin","password":"synthetic poster password"}),
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap();
    assert_eq!(
        call(&app, "GET", endpoint, &cookie, "", Value::Null)
            .await
            .1["enabled"],
        false
    );
    let signing = key();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            csrf,
            json!({"id":"test","publicKey":STANDARD.encode(signing.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let mut pkg = package(&signing, good_module());
    pkg.manifest.id = "poster-rotator".into();
    pkg.manifest.permissions = vec!["images".into()];
    pkg.signature = STANDARD.encode(signing.sign(&pkg.signing_bytes().unwrap()).as_ref());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            csrf,
            json!(pkg)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/poster-rotator",
            &cookie,
            csrf,
            json!({"enabled":true,"libraryIds":["films"],"allLibraries":false})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/poster-rotator/settings",
            &cookie,
            csrf,
            json!({"rotation_mode":"sequential","pool_size":3,"preferred_language":"fr-CA"})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, policy, _) = call(&app, "GET", endpoint, &cookie, "", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(policy["libraries"], json!(["films"]));
    assert_eq!(policy["rotationMode"], "sequential");
    assert_eq!(policy["poolSize"], 3);
    assert_eq!(policy["preferredLanguage"], "fr-ca");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/poster-rotator",
            &cookie,
            csrf,
            json!({"enabled":false,"libraryIds":["films"],"allLibraries":false})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", endpoint, &cookie, "", Value::Null)
            .await
            .1["enabled"],
        false
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn image_collection_uses_only_enabled_providers_and_skips_known_sources() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "image-admin",
            Zeroizing::new("synthetic collection password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic',1)").execute(store.pool()).await.unwrap();
    media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','films','work','Film','{\"test.metadata\":\"known\"}')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone()).merge(auth::router(auth));
    let (_, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"image-admin","password":"synthetic collection password"}),
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let signing = key();
    let output = json!({"apiVersion":1,"result":[{"url":"https://images.example.org/known.png","imageType":"poster","language":"fr","isPrimary":false}]}).to_string();
    let data = output
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let wasm = wat::parse_str(format!(
        r#"(module (memory (export "memory") 1 256) (data (i32.const 4096) "{data}")
        (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#,
        (4096u64 << 32) | output.len() as u64
    ))
    .unwrap();
    let mut p = package(&signing, wasm);
    p.manifest.permissions = vec!["images".into(), "metadata.write".into()];
    p.manifest.http_hosts = vec!["images.example.org".into()];
    p.signature = STANDARD.encode(signing.sign(&p.signing_bytes().unwrap()).as_ref());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            csrf,
            json!({"id":"test","publicKey":STANDARD.encode(signing.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            csrf,
            serde_json::to_value(p).unwrap()
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let endpoint = "/api/v1/admin/extensions/images/collect";
    let request = json!({"workId":"work","languages":["fr","en"]});
    let (status, result, _) = call(&app, "POST", endpoint, &cookie, csrf, request.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["providers"], 0);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/test.metadata",
            &cookie,
            csrf,
            json!({"enabled":true,"libraryIds":["films"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    media_poc::database::query("INSERT INTO work_image_entries(id,work_id,kind,position,width,height) VALUES('known-image','work','poster',0,12,18)").execute(store.pool()).await.unwrap();
    let hash = format!(
        "{:x}",
        Sha256::digest(b"https://images.example.org/known.png")
    );
    media_poc::database::query("INSERT INTO artwork_image_sources(work_id,source_url_hash,image_id) VALUES('work',?,'known-image')").bind(hash).execute(store.pool()).await.unwrap();
    let (status, result, _) = call(&app, "POST", endpoint, &cookie, csrf, request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["providers"], 1, "{result}");
    assert_eq!(result["skipped"], 1, "{result}");
    assert_eq!(
        result["attempts"], 0,
        "No request may reach the synthetic external host: {result}"
    );
    assert_eq!(result["errors"], json!([]), "{result}");
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/test.metadata",
            &cookie,
            csrf,
            json!({"enabled":false,"libraryIds":["films"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, result, _) = call(&app, "POST", endpoint, &cookie, csrf, request).await;
    assert_eq!(result["providers"], 0);
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
        StatusCode::BAD_REQUEST
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

    // All-library scope includes another authorized work without per-library checkboxes.
    assert_eq!(
        call(
            &app,
            "PUT",
            &endpoint,
            &cookie,
            csrf,
            json!({"enabled":true,"allLibraries":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, all_works, _) = call(
        &app,
        "GET",
        &format!("{endpoint}/works"),
        &cookie,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(all_works["items"].as_array().unwrap().len(), 2);
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
        StatusCode::OK
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
    assert_eq!(history["runs"].as_array().unwrap().len(), 4);
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
    let mut avatars = package(&key, STANDARD.decode(&p.wasm).unwrap());
    avatars.manifest.id = "test.avatars".into();
    avatars.manifest.name = "Avatars de test".into();
    avatars.manifest.permissions = vec!["avatars".into()];
    avatars.signature = STANDARD.encode(key.sign(&avatars.signing_bytes().unwrap()).as_ref());
    let avatar_path = temp.path().join("avatar-package.json");
    std::fs::write(&avatar_path, serde_json::to_vec(&avatars).unwrap()).unwrap();
    let status = tokio::process::Command::new("node")
        .arg("scripts/test-extensions-live.mjs")
        .current_dir(root.join("apps/web"))
        .env("MEDIA_TEST_ORIGIN", origin)
        .env("MEDIA_TEST_PACKAGE", path)
        .env("MEDIA_TEST_AVATAR_PACKAGE", avatar_path)
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
        allowed_hosts: vec!["api.provider.example".into()],
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
        allowed_hosts: vec!["api.provider.example".into()],
        auth_headers: std::collections::BTreeMap::new(),
        http_count: 0,
        max_http_requests: 10,
        max_response_size: 1024,
        last_http_at: None,
        retry_after_seconds: None,
    };

    // 1. Non-HTTPS scheme is forbidden
    let res = execute_host_http(
        &mut state,
        HostHttpRequest {
            url: "http://api.provider.example/3/movie/550".into(),
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
            url: "https://api.provider.example/3/movie/550".into(),
            method: "GET".into(),
            headers: Default::default(),
            body: None,
            timeout_secs: Some(1),
        },
    );
    assert_eq!(res.unwrap_err().code, "quota_exceeded");
}

#[test]
#[ignore = "requires MEDIA_TEST_EXTENSION_WASM pointing to an explicitly built external extension"]
fn external_extension_wasm_manifest_uses_generic_runtime() {
    use media_poc::extension_runtime::{Invocation, execute};
    let path =
        std::env::var_os("MEDIA_TEST_EXTENSION_WASM").expect("external test artifact required");
    let bytes = std::fs::read(path).unwrap();
    let output = execute(Invocation::new(
        STANDARD.encode(bytes),
        ExtensionRequest {
            api_version: 1,
            operation: "manifest".into(),
            input: json!({}),
        },
    ))
    .unwrap();
    assert_eq!(output.api_version, 1);
    assert!(!output.result["id"].as_str().unwrap().is_empty());
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
    let app = extensions::router(auth.clone())
        .merge(media_poc::artwork::router(auth.clone()))
        .merge(auth::router(auth));

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

    let endpoint = "/api/v1/admin/extensions/test.metadata/apply";
    let apply_input = json!({"workId":"work-1", "metadata":{"title":"Synthetic enriched film"}});
    assert_eq!(
        call(&app, "POST", endpoint, &cookie, csrf, apply_input.clone())
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let signing_key = key();
    let mut installed = package(&signing_key, good_module());
    installed.manifest.permissions.push("metadata.write".into());
    installed.signature = STANDARD.encode(
        signing_key
            .sign(&installed.signing_bytes().unwrap())
            .as_ref(),
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            csrf,
            json!({"id":"test","publicKey":STANDARD.encode(signing_key.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            csrf,
            json!(installed)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(&app, "POST", endpoint, &cookie, csrf, apply_input.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut read_only = package(&signing_key, good_module());
    read_only.manifest.id = "test.readonly".into();
    read_only.signature = STANDARD.encode(
        signing_key
            .sign(&read_only.signing_bytes().unwrap())
            .as_ref(),
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            csrf,
            json!(read_only)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/test.readonly",
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
            "/api/v1/admin/extensions/test.readonly/apply",
            &cookie,
            csrf,
            apply_input.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/test.metadata",
            &cookie,
            csrf,
            json!({"enabled":true,"libraryIds":["films"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );

    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let uploaded = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/works/work-1/images/poster")
                .header(header::CONTENT_TYPE, "image/png")
                .header(header::ORIGIN, "http://127.0.0.1:3000")
                .header(header::COOKIE, &cookie)
                .header("x-csrf-token", csrf)
                .body(Body::from(png.into_inner()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(uploaded.status(), StatusCode::CREATED);

    let rich_metadata = json!({
        "title": "Synthetic enriched film",
        "plot": "Synthetic extension proposal",
        "year": 2026,
        "actors": [
            {"name": "Synthetic actor one", "role": "Narrator"},
            {"name": "Synthetic actor two", "role": "Pilot"}
        ],
        "ratings": [
            {"provider": "test.metadata", "value": 8.4, "max": 10.0}
        ],
        "posterUrl": "https://images.provider.example/poster.jpg"
    });

    let (status, _, _) = call(
        &app,
        "POST",
        endpoint,
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
    assert_eq!(detail["title"], "Synthetic enriched film");
    assert_eq!(detail["description"], "Synthetic extension proposal");
    assert_eq!(detail["actors"][0]["name"], "Synthetic actor one");
    assert_eq!(detail["actors"][1]["name"], "Synthetic actor two");
    assert_eq!(detail["ratings"][0]["provider"], "test.metadata");
    // Metadata cannot bypass the protected artwork pipeline or overwrite a chosen poster.
    assert!(
        detail["posterUrl"]
            .as_str()
            .unwrap()
            .starts_with("/api/v1/works/work-1/images/")
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/test.metadata",
            &cookie,
            csrf,
            json!({"enabled":false,"libraryIds":[]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "POST", endpoint, &cookie, csrf, apply_input)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rich_operations_and_account_avatars_use_signed_permissions() {
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
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic',1)").execute(store.pool()).await.unwrap();
    media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','films','work','Original','{}')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone())
        .merge(media_poc::avatars::router(auth.clone()))
        .merge(media_poc::admin_users::router(auth.clone()))
        .merge(auth::router(auth));
    let (_, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"extension-admin","password":"synthetic extension password"}),
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let signing = key();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            csrf,
            json!({"id":"test","publicKey":STANDARD.encode(signing.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><circle cx="128" cy="128" r="100" fill="#abcdef"/></svg>"##;
    let cases = [
        (
            "search",
            "metadata.search",
            json!([{"id":"candidate","title":"Selected","mediaType":"movie","year":2020}]),
            vec!["metadata.read"],
        ),
        (
            "details",
            "metadata.details",
            json!({"provider":"synthetic","providerId":"candidate","title":"Selected","synopsis":"Full plot","externalIds":{"synthetic":"candidate"}}),
            vec!["metadata.read", "metadata.write"],
        ),
        (
            "images",
            "metadata.images",
            json!([{"url":"https://example.invalid/image.png","imageType":"poster","isPrimary":true}]),
            vec!["images"],
        ),
        (
            "failure",
            "metadata.search",
            json!({"error":"missing_api_key","message":"secret=not-for-history"}),
            vec!["metadata.read"],
        ),
        (
            "avatar",
            "avatars.generate",
            json!({"id":"one","label":"One","svg":svg}),
            vec!["avatars"],
        ),
    ];
    for (id, operation, result, permissions) in cases {
        let response = json!({"apiVersion":1,"result":result}).to_string();
        let data = response
            .bytes()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        let wasm=wat::parse_str(format!(r#"(module (memory (export "memory") 1 256) (data (i32.const 4096) "{data}") (func (export "media_alloc") (param i32) (result i32) i32.const 1024) (func (export "media_run") (param i32 i32) (result i64) i64.const {}))"#,(4096u64<<32)|response.len() as u64)).unwrap();
        let mut p = package(&signing, wasm);
        p.manifest.id = format!("test.{id}");
        p.manifest.permissions = permissions.into_iter().map(str::to_owned).collect();
        p.signature = STANDARD.encode(signing.sign(&p.signing_bytes().unwrap()).as_ref());
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/admin/extensions",
                &cookie,
                csrf,
                json!(p)
            )
            .await
            .0,
            StatusCode::CREATED
        );
        let endpoint = format!("/api/v1/admin/extensions/test.{id}");
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
        if id == "avatar" {
            assert_eq!(call(&app,"POST","/api/v1/admin/users",&cookie,csrf,json!({"login":"avatar-user","password":"synthetic regular password","role":"user","displayName":null,"libraryIds":[]})).await.0,StatusCode::CREATED);
            let (_, user_login, user_cookie) = call(
                &app,
                "POST",
                "/api/v1/auth/login",
                "",
                "",
                json!({"login":"avatar-user","password":"synthetic regular password"}),
            )
            .await;
            let user_csrf = user_login["csrfToken"].as_str().unwrap();
            let route = "/api/v1/account/avatar/extensions/test.avatar";
            assert_eq!(
                call(
                    &app,
                    "POST",
                    route,
                    &user_cookie,
                    "",
                    json!({"operation":operation,"apply":true})
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
            let (status, output, _) = call(
                &app,
                "POST",
                route,
                &user_cookie,
                user_csrf,
                json!({"operation":operation,"apply":true}),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{output}");
            let sha: String = media_poc::database::query_scalar(
                "SELECT sha256 FROM user_avatars WHERE user_id=?",
            )
            .bind(user_login["user"]["id"].as_str().unwrap())
            .fetch_one(store.pool())
            .await
            .unwrap();
            let (_, data) = store.assets().read_all(&sha).await.unwrap();
            assert_eq!(data, svg.as_bytes());
            assert_eq!(
                call(
                    &app,
                    "POST",
                    route,
                    &user_cookie,
                    user_csrf,
                    json!({"operation":"metadata.details"})
                )
                .await
                .0,
                StatusCode::BAD_REQUEST
            );
            call(
                &app,
                "PUT",
                &endpoint,
                &cookie,
                csrf,
                json!({"enabled":false,"libraryIds":[]}),
            )
            .await;
            assert_eq!(
                call(
                    &app,
                    "POST",
                    route,
                    &user_cookie,
                    user_csrf,
                    json!({"operation":operation})
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
        } else {
            let (status, output, _) = call(
                &app,
                "POST",
                &format!("{endpoint}/run"),
                &cookie,
                csrf,
                json!({"workId":"work","operation":operation,"input":{"id":"candidate"}}),
            )
            .await;
            if id == "failure" {
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
                assert_eq!(output["pluginError"], "missing_api_key");
                assert!(!output.to_string().contains("not-for-history"));
            } else {
                assert_eq!(status, StatusCode::OK, "{output}");
                assert_eq!(output["proposal"], result);
            }
            if id == "details" {
                assert_eq!(
                    call(
                        &app,
                        "POST",
                        &format!("{endpoint}/apply"),
                        &cookie,
                        csrf,
                        json!({"workId":"work","metadata":result})
                    )
                    .await
                    .0,
                    StatusCode::NO_CONTENT
                );
                let saved: String = media_poc::database::query_scalar(
                    "SELECT manual_json FROM inventory_works WHERE id='work'",
                )
                .fetch_one(store.pool())
                .await
                .unwrap();
                let saved: Value = serde_json::from_str(&saved).unwrap();
                assert_eq!(saved["plot"], "Full plot");
                assert_eq!(saved["external_ids"]["synthetic"], "candidate");
            }
        }
    }
}

#[tokio::test]
async fn signed_file_import_requires_explicit_trust_and_never_enables_or_overwrites() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("inbox");
    std::fs::create_dir(&root).unwrap();
    let store = IdentityStore::open(&temp.path().join("extensions.db"))
        .await
        .unwrap();
    let key = key();
    let package = package(&key, good_module());
    let file = root.join("fixture.package.json");
    std::fs::write(&file, serde_json::to_vec(&package).unwrap()).unwrap();
    assert!(
        media_poc::extension_files::import(&store, &root)
            .await
            .is_err()
    );
    std::fs::write(
        root.join("trusted-publishers.json"),
        serde_json::to_vec(
            &json!([{"id":"test","publicKey":STANDARD.encode(key.public_key().as_ref())}]),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        media_poc::extension_files::import(&store, &root)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>("SELECT enabled FROM extensions")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>("SELECT COUNT(*) FROM extension_grants")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        0
    );
    media_poc::database::query("UPDATE extensions SET enabled=1")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        media_poc::extension_files::import(&store, &root)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        media_poc::database::query_scalar::<_, i64>("SELECT enabled FROM extensions")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        1
    );
    media_poc::database::query("UPDATE extension_publishers SET active=0")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        media_poc::extension_files::import(&store, &root)
            .await
            .is_err()
    );
    media_poc::database::query("UPDATE extension_publishers SET active=1")
        .execute(store.pool())
        .await
        .unwrap();
    let mut tampered = serde_json::to_value(&package).unwrap();
    tampered["sha256"] = json!("0".repeat(64));
    std::fs::write(file, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(
        media_poc::extension_files::import(&store, &root)
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn persistent_storage_synthetic_guest_survives_worker_restart_and_database_reopen() {
    use media_extension_sdk::storage::{StorageOperation, StorageRequest, StorageScope};
    use media_poc::{
        extension_data,
        extension_runtime::{Invocation, read_frame, write_frame},
    };
    use std::process::{Command, Stdio};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("storage.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    let signing = key();
    let mut package = package(&signing, good_module());
    package.manifest.permissions.push("storage.private".into());
    package.signature = STANDARD.encode(signing.sign(&package.signing_bytes().unwrap()).as_ref());
    assert!(extensions::validate_package(&package, signing.public_key().as_ref()).is_ok());
    media_poc::database::query(
        "INSERT INTO extension_publishers(id,public_key,active) VALUES('test','fixture',1)",
    )
    .execute(store.pool())
    .await
    .unwrap();
    media_poc::database::query("INSERT INTO extensions(id,publisher_id,package_json,generation,enabled) VALUES('test.metadata','test',?,'first',1)").bind(serde_json::to_string(&package).unwrap()).execute(store.pool()).await.unwrap();
    store
        .bootstrap(
            "storage-admin",
            Zeroizing::new("synthetic storage password".into()),
        )
        .await
        .unwrap();
    let user_id: String = media_poc::database::query_scalar(
        "SELECT user_id FROM local_credentials WHERE login='storage-admin'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    let context = extension_data::Context {
        user_id,
        extension_id: "test.metadata".into(),
        generation: "first".into(),
        ..Default::default()
    };
    let scope = StorageScope {
        kind: "global".into(),
        id: String::new(),
    };
    for iteration in 0..2 {
        let reopened = IdentityStore::open(&path).await.unwrap();
        let request = StorageRequest {
            api_version: 1,
            scope: scope.clone(),
            operation: if iteration == 0 {
                StorageOperation::Put {
                    key: "survives".into(),
                    value: "durable fixture".into(),
                    expected_version: None,
                    expires_at: None,
                    durability: "durable".into(),
                }
            } else {
                StorageOperation::Get {
                    key: "survives".into(),
                }
            },
        };
        let bytes = serde_json::to_vec(&request).unwrap();
        let data = bytes
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        let output = r#"{"apiVersion":1,"result":{"title":"storage","summary":"completed"}}"#;
        let encoded = output
            .bytes()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        let wasm = wat::parse_str(format!(
            r#"(module
            (import "media" "media_host_storage_v1" (func $storage (param i32 i32) (result i64)))
            (memory (export "memory") 1)
            (data (i32.const 4096) "{data}") (data (i32.const 8192) "{encoded}")
            (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
            (func (export "media_run") (param i32 i32) (result i64)
                i32.const 4096 i32.const {} call $storage i64.eqz if unreachable end
                i64.const {}))"#,
            bytes.len(),
            (8192u64 << 32) | output.len() as u64
        ))
        .unwrap();
        let worker = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(if cfg!(windows) {
                "media-extension-worker.exe"
            } else {
                "media-extension-worker"
            });
        let mut child = Command::new(worker)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        write_frame(&mut stdin, &Invocation::new(STANDARD.encode(wasm), input())).unwrap();
        let frame: Value = read_frame(&mut stdout, media_extension_sdk::MAX_JSON).unwrap();
        assert_eq!(frame["kind"], "storage");
        let incoming = serde_json::from_value(frame["request"].clone()).unwrap();
        let result = extension_data::execute(&reopened, &context, incoming, 1_000).await;
        assert_eq!(result.as_ref().unwrap().entries[0].value, "durable fixture");
        write_frame(
            &mut stdin,
            &result.map_err(media_extension_sdk::storage::StorageError::from),
        )
        .unwrap();
        let finished: Value = read_frame(&mut stdout, media_extension_sdk::MAX_JSON).unwrap();
        assert_eq!(finished["kind"], "finished");
        assert!(child.wait().unwrap().success());
    }
}

#[tokio::test]
async fn signed_private_storage_permission_installs_without_implicit_activation() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("install.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "storage-admin",
            Zeroizing::new("synthetic install password".into()),
        )
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
        json!({"login":"storage-admin","password":"synthetic install password"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let csrf = login["csrfToken"].as_str().unwrap();
    let signing = key();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            csrf,
            json!({"id":"test","publicKey":STANDARD.encode(signing.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let mut signed = package(&signing, good_module());
    signed.manifest.permissions.push("storage.private".into());
    signed.signature = STANDARD.encode(signing.sign(&signed.signing_bytes().unwrap()).as_ref());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            csrf,
            json!(signed)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let enabled: i64 = media_poc::database::query_scalar(
        "SELECT enabled FROM extensions WHERE id='test.metadata'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(enabled, 0);
    signed.manifest.id = "test.bad-storage".into();
    signed.manifest.permissions.push("storage.sql".into());
    signed.signature = STANDARD.encode(signing.sign(&signed.signing_bytes().unwrap()).as_ref());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            csrf,
            json!(signed)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

async fn private_storage_http_fixture(
    wasm: Vec<u8>,
) -> (tempfile::TempDir, IdentityStore, Router, String, String) {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("http-storage.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "storage-admin",
            Zeroizing::new("synthetic HTTP password".into()),
        )
        .await
        .unwrap();
    media_poc::libraries::initialize_local(&store)
        .await
        .unwrap();
    media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('films','Films','films','local','synthetic',1)").execute(store.pool()).await.unwrap();
    media_poc::database::query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('work','films','work','Synthetic','{}')").execute(store.pool()).await.unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone()).merge(auth::router(auth));
    let (_, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"storage-admin","password":"synthetic HTTP password"}),
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap().to_owned();
    let signing = key();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            &csrf,
            json!({"id":"test","publicKey":STANDARD.encode(signing.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let mut signed = package(&signing, wasm);
    signed.manifest.permissions.push("storage.private".into());
    signed.signature = STANDARD.encode(signing.sign(&signed.signing_bytes().unwrap()).as_ref());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions",
            &cookie,
            &csrf,
            json!(signed)
        )
        .await
        .0,
        StatusCode::CREATED
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/admin/extensions/test.metadata",
            &cookie,
            &csrf,
            json!({"enabled":true,"libraryIds":["films"]})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    (temp, store, app, cookie, csrf)
}

#[tokio::test(flavor = "multi_thread")]
async fn malicious_storage_guest_hits_host_operation_quota() {
    let request = json!({"apiVersion":1,"scope":{"kind":"global","id":""},"operation":{"op":"get","key":"absent"}}).to_string();
    let data = request
        .bytes()
        .map(|b| format!("\\{b:02x}"))
        .collect::<String>();
    let wasm = wat::parse_str(format!(
        r#"(module
        (import "media" "media_host_storage_v1" (func $storage (param i32 i32) (result i64)))
        (memory (export "memory") 1) (data (i32.const 4096) "{data}")
        (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
        (func (export "media_run") (param i32 i32) (result i64) (local $n i32)
            (loop $again i32.const 4096 i32.const {} call $storage drop
                local.get $n i32.const 1 i32.add local.tee $n i32.const 129 i32.lt_u br_if $again)
            i64.const 0))"#,
        request.len()
    ))
    .unwrap();
    let (_temp, _store, app, cookie, csrf) = private_storage_http_fixture(wasm).await;
    let (status, result, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions/test.metadata/run",
        &cookie,
        &csrf,
        json!({"workId":"work"}),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{result}");
    assert_eq!(result["error"], "storage_operation_quota");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Rust SDK Wasm fixture via MEDIA_TEST_STORAGE_WASM"]
async fn rust_sdk_storage_guest_runs_through_signed_http_install_and_worker_broker() {
    let path = std::env::var_os("MEDIA_TEST_STORAGE_WASM")
        .expect("set MEDIA_TEST_STORAGE_WASM to compiled synthetic_persistent_extension.wasm");
    let wasm = std::fs::read(path).unwrap();
    let (_temp, _store, app, cookie, csrf) = private_storage_http_fixture(wasm).await;
    for expected in 1..=2 {
        let (status, result, _) = call(
            &app,
            "POST",
            "/api/v1/admin/extensions/test.metadata/run",
            &cookie,
            &csrf,
            json!({"workId":"work"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(
            result["proposal"]["summary"],
            format!("Successful durable invocation {expected}")
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_extension_upgrade_preserves_durable_storage_and_updates_generation() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("db.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap(
            "admin-upgrade",
            Zeroizing::new("synthetic upgrade password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = extensions::router(auth.clone()).merge(auth::router(auth));
    let (_, login, cookie) = call(
        &app,
        "POST",
        "/api/v1/auth/login",
        "",
        "",
        json!({"login":"admin-upgrade","password":"synthetic upgrade password"}),
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let signing = key();

    let wasm1 = wat::parse_str(r#"(module (memory (export "memory") 1 256))"#).unwrap();
    let mut p1 = package(&signing, wasm1);
    p1.manifest.id = "upgradable.ext".into();
    p1.manifest.version = "1.0.0".into();
    p1.manifest.permissions = vec!["storage.private".into()];
    p1.signature = STANDARD.encode(signing.sign(&p1.signing_bytes().unwrap()).as_ref());

    // 0. Trust publisher
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/extensions/publishers",
            &cookie,
            csrf,
            json!({"id": p1.publisher_id, "publicKey": STANDARD.encode(signing.public_key().as_ref())})
        )
        .await
        .0,
        StatusCode::CREATED
    );

    // 1. Install v1
    let (status, resp, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions",
        &cookie,
        csrf,
        serde_json::to_value(&p1).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{resp}");

    call(
        &app,
        "PUT",
        "/api/v1/admin/extensions/upgradable.ext",
        &cookie,
        csrf,
        json!({"enabled": true, "allLibraries": true}),
    )
    .await;

    media_poc::database::query(
        "INSERT INTO extension_data_namespaces(extension_id,publisher_id,publisher_key,entries,bytes) VALUES('upgradable.ext','test','key',1,18)"
    )
    .execute(store.pool())
    .await
    .unwrap();

    media_poc::database::query(
        "INSERT INTO extension_data(extension_id,scope,scope_id,key,value,version,durability,value_bytes,sha256) VALUES('upgradable.ext','global','','state','durable_v1_payload','v1-uuid','durable',18,'sha')"
    )
    .execute(store.pool())
    .await
    .unwrap();

    let gen_v1: String = media_poc::database::query_scalar(
        "SELECT generation FROM extensions WHERE id='upgradable.ext'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();

    // 2. Prepare v2 package
    let wasm2 = wat::parse_str(r#"(module (memory (export "memory") 2 256))"#).unwrap();
    let mut p2 = package(&signing, wasm2.clone());
    p2.manifest.id = "upgradable.ext".into();
    p2.manifest.version = "2.0.0".into();
    p2.manifest.permissions = vec!["storage.private".into()];
    p2.signature = STANDARD.encode(signing.sign(&p2.signing_bytes().unwrap()).as_ref());

    // 3. Upgrade to v2 via POST /api/v1/admin/extensions/{id}/upgrade
    let (status, resp, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions/upgradable.ext/upgrade",
        &cookie,
        csrf,
        serde_json::to_value(&p2).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["version"], "2.0.0");
    assert_eq!(resp["upgraded"], true);

    // 4. Verify generation updated
    let gen_v2: String = media_poc::database::query_scalar(
        "SELECT generation FROM extensions WHERE id='upgradable.ext'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_ne!(gen_v1, gen_v2, "Generation must be updated on upgrade");

    // 5. Verify durable storage was preserved!
    let saved_state: String = media_poc::database::query_scalar(
        "SELECT value FROM extension_data WHERE extension_id='upgradable.ext' AND key='state'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        saved_state, "durable_v1_payload",
        "Durable data must be preserved on upgrade"
    );

    // 6. Verify mismatched ID is rejected
    let mut p_bad_id = p2.clone();
    p_bad_id.manifest.id = "wrong.id".into();
    p_bad_id.signature = STANDARD.encode(signing.sign(&p_bad_id.signing_bytes().unwrap()).as_ref());
    let (bad_status, _, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions/upgradable.ext/upgrade",
        &cookie,
        csrf,
        serde_json::to_value(&p_bad_id).unwrap(),
    )
    .await;
    assert_eq!(bad_status, StatusCode::BAD_REQUEST);

    // 7. Verify mismatched publisher key is rejected
    let other_key = key();
    let mut p_bad_key = package(&other_key, wasm2);
    p_bad_key.manifest.id = "upgradable.ext".into();
    p_bad_key.manifest.version = "3.0.0".into();
    p_bad_key.publisher_id = "other_publisher".into();
    p_bad_key.signature =
        STANDARD.encode(other_key.sign(&p_bad_key.signing_bytes().unwrap()).as_ref());
    let (key_status, resp, _) = call(
        &app,
        "POST",
        "/api/v1/admin/extensions/upgradable.ext/upgrade",
        &cookie,
        csrf,
        json!({
            "manifest": p_bad_key.manifest,
            "publisherId": p_bad_key.publisher_id,
            "sha256": p_bad_key.sha256,
            "wasm": p_bad_key.wasm,
            "signature": p_bad_key.signature,
            "publicKey": STANDARD.encode(other_key.public_key().as_ref()),
        }),
    )
    .await;
    assert_eq!(key_status, StatusCode::FORBIDDEN, "{resp}");
}
