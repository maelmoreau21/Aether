//! Two real Core processes, signed Wasm, a blocked private write and abrupt owner loss.
//! Only the disposable PostgreSQL runner may provide MEDIA_TEST_POSTGRES_URL.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{Manifest, Package};
use media_poc::{
    database::{begin_write_scoped, query, query_as, query_scalar},
    identity::IdentityStore,
};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, time::Duration};

const EXTENSION: &str = "fixture.failover";

fn package() -> (Package, String) {
    let signing = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    let signing = Ed25519KeyPair::from_pkcs8(signing.as_ref()).unwrap();
    let request=json!({"apiVersion":1,"scope":{"kind":"global","id":""},"operation":{"op":"put","key":"one-effect","value":"durable","expected_version":null,"expires_at":null,"durability":"durable"}}).to_string();
    let response=json!({"apiVersion":1,"result":{"title":"Synthetic failover result","summary":"A durable private write"}}).to_string();
    let escape = |s: &str| s.bytes().map(|b| format!("\\{b:02x}")).collect::<String>();
    let wasm = wat::parse_str(format!(
        r#"(module
      (import "media" "media_host_storage_v1" (func $put (param i32 i32) (result i64)))
      (memory (export "memory") 1 256)
      (data (i32.const 8192) "{}") (data (i32.const 16384) "{}")
      (func (export "media_alloc") (param i32) (result i32) i32.const 1024)
      (func (export "media_run") (param i32 i32) (result i64)
        i32.const 8192 i32.const {} call $put drop i64.const {}))"#,
        escape(&request),
        escape(&response),
        request.len(),
        (16384u64 << 32) | response.len() as u64
    ))
    .unwrap();
    let mut package = Package {
        manifest: Manifest {
            api_version: 1,
            id: EXTENSION.into(),
            name: "Synthetic failover".into(),
            version: "1.0.0".into(),
            description: "Legal isolated fixture".into(),
            author: "Test".into(),
            license: "MIT".into(),
            repository: "https://example.invalid/fixture".into(),
            permissions: vec!["metadata.read".into(), "storage.private".into()],
            ..Default::default()
        },
        publisher_id: "fixture.publisher".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(signing.sign(&package.signing_bytes().unwrap()).as_ref());
    (package, STANDARD.encode(signing.public_key().as_ref()))
}

async fn start(directory: &Path, url: &str, port: u16) -> tokio::process::Child {
    std::fs::create_dir_all(directory.join("empty-media")).unwrap();
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_aether-core"));
    command
        .current_dir(directory)
        .env("MEDIA_DB_ENGINE", "postgres")
        .env("MEDIA_OIDC_ENABLED", "false")
        .env("MEDIA_DB_URL", url)
        .env("MEDIA_DB_PATH", directory.join("bootstrap-only.sqlite"))
        .env("MEDIA_LIBRARY_ROOT", directory.join("empty-media"))
        .env("MEDIA_ASSETS_PATH", directory.join("assets"))
        .env("MEDIA_PLUGINS_PATH", directory.join("plugins"))
        .env("MEDIA_BACKUP_DIR", directory.join("backups"))
        .env("MEDIA_PUBLIC_URL", format!("http://127.0.0.1:{port}"))
        .env("MEDIA_PORT", port.to_string())
        .env("MEDIA_BIND_ADDR", "127.0.0.1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "Core exited before readiness"
            );
            if reqwest::get(format!("http://127.0.0.1:{port}/api/v1/config"))
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("Core readiness deadline");
    child
}

fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn identity(client: &reqwest::Client, origin: &str, cookie: &str) -> String {
    let reply: Value = client
        .get(format!("{origin}/api/v1/auth/me"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    reply["csrfToken"]
        .as_str()
        .expect("authenticated Core session")
        .to_string()
}
async fn invoke(
    client: &reqwest::Client,
    origin: &str,
    cookie: &str,
    csrf: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    client
        .post(format!("{origin}/api/v1/admin/extensions/{EXTENSION}/run"))
        .header("origin", origin)
        .header("cookie", cookie)
        .header("x-csrf-token", csrf)
        .json(&json!({"workId":"failover-work","operation":"metadata.propose"}))
        .send()
        .await
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL, built Core/worker and no real provider calls"]
async fn two_real_cores_fence_an_inflight_extension_after_owner_crash() {
    let url = zeroize::Zeroizing::new(
        std::env::var("MEDIA_TEST_POSTGRES_URL").expect("disposable PostgreSQL runner required"),
    );
    let store = IdentityStore::open_postgres(&url).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let password = zeroize::Zeroizing::new(
        uuid::Uuid::new_v4().to_string() + &uuid::Uuid::new_v4().to_string(),
    );
    store
        .bootstrap("failover-admin", password.clone())
        .await
        .unwrap();
    media_poc::extensions::bootstrap_essential_plugins(&store).await;
    query("UPDATE extensions SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE system_task_schedules SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('failover-library','Synthetic','films','local','/unmounted-test-media',1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,external_ids_json) VALUES('failover-work','failover-library','failover','Synthetic','synthetic','{}')").execute(store.pool()).await.unwrap();
    let (signed, key) = package();
    media_poc::extensions::install_internal_with_key(&store, signed, &key)
        .await
        .unwrap();
    query("INSERT INTO automation_policies(extension_id,library_id,mode) VALUES(?,'failover-library','disabled') ON CONFLICT(extension_id,library_id) DO UPDATE SET mode='disabled'")
        .bind(EXTENSION).execute(store.pool()).await.unwrap();
    let first_port = port();
    let mut second_port = port();
    while second_port == first_port {
        second_port = port();
    }
    let mut first = start(&temp.path().join("first"), &url, first_port).await;
    let mut second = start(&temp.path().join("second"), &url, second_port).await;
    let origins = [
        format!("http://127.0.0.1:{first_port}"),
        format!("http://127.0.0.1:{second_port}"),
    ];
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(70))
        .build()
        .unwrap();
    let login = client
        .post(format!("{}/api/v1/auth/login", origins[0]))
        .header("origin", &origins[0])
        .json(&json!({"login":"failover-admin","password":password.as_str()}))
        .send()
        .await
        .unwrap();
    assert!(login.status().is_success(), "synthetic login failed");
    let cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = identity(&client, &origins[0], &cookie).await;
    let peer_csrf = identity(&client, &origins[1], &cookie).await;
    let mut conn = store.pool().acquire().await.unwrap();
    let held = begin_write_scoped(&mut conn, "extension_state", EXTENSION)
        .await
        .unwrap();
    let request_client = client.clone();
    let request_origin = origins[0].clone();
    let request_cookie = cookie.clone();
    let pending = tokio::spawn(async move {
        invoke(&request_client, &request_origin, &request_cookie, &csrf).await
    });
    let run_id=tokio::time::timeout(Duration::from_secs(15),async {
        loop {
            let blocked:i64=query_scalar("SELECT COUNT(*) FROM pg_locks WHERE locktype='advisory' AND NOT granted AND objid::bigint=(hashtext('extension_state:fixture.failover')::bigint & 4294967295) AND classid::bigint=CASE WHEN hashtext('extension_state:fixture.failover')<0 THEN 4294967295 ELSE 0 END")
                .fetch_one(store.pool()).await.unwrap();
            let run:Option<String>=query_scalar("SELECT r.id FROM extension_runs r JOIN extension_run_owners o ON o.run_id=r.id WHERE r.extension_id=? AND r.status='running'")
                .bind(EXTENSION).fetch_optional(store.pool()).await.unwrap();
            if blocked>0 && let Some(id)=run {break id;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("real guest private write must be waiting on the held PostgreSQL lock");
    first.kill().await.unwrap();
    let _ = first.wait().await;
    pending.abort();
    let _ = pending.await;
    held.rollback().await.unwrap();
    drop(conn);
    tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            let response = client
                .get(format!("{}/api/v1/admin/extensions/runs", origins[1]))
                .header("cookie", &cookie)
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
            let status: String = query_scalar("SELECT status FROM extension_runs WHERE id=?")
                .bind(&run_id)
                .fetch_one(store.pool())
                .await
                .unwrap();
            if status == "interrupted" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("surviving Core must recover expired invocation history");
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_data WHERE extension_id=?")
        .bind(EXTENSION)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "killed blocked writer must not publish a durable effect"
    );
    let reply = invoke(&client, &origins[1], &cookie, &peer_csrf)
        .await
        .unwrap();
    let status = reply.status();
    let body: Value = reply.json().await.unwrap();
    assert!(
        status.is_success(),
        "survivor explicit retry failed: {body}"
    );
    let effects: Vec<(String, String)> =
        query_as("SELECT key,value FROM extension_data WHERE extension_id=?")
            .bind(EXTENSION)
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(effects, vec![("one-effect".into(), "durable".into())]);
    let completed: i64 = query_scalar(
        "SELECT COUNT(*) FROM extension_runs WHERE extension_id=? AND status='completed'",
    )
    .bind(EXTENSION)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(completed, 1);
    let crashed_status: String = query_scalar("SELECT status FROM extension_runs WHERE id=?")
        .bind(&run_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(crashed_status, "interrupted");
    second.kill().await.unwrap();
    let _ = second.wait().await;
    store.close().await;
    println!(
        "PASS two real Core processes + PostgreSQL: blocked signed-Wasm write, abrupt owner loss, expired history interrupted without replay, zero crash effect, one durable effect after explicit survivor invocation. This does not prove Node routing, all task effects, partitions or production readiness."
    );
}
