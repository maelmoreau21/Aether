use super::*;
use axum::{
    body::Body,
    http::{Request, header},
    response::Response,
};
use http_body_util::BodyExt;
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use tower::ServiceExt;
const ORIGIN: &str = "http://127.0.0.1:3000";
async fn request(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:3000")
                .header("origin", ORIGIN)
                .header(header::COOKIE, cookie)
                .header("x-csrf-token", csrf)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[test]
fn github_catalogue_links_keep_the_selected_branch() {
    for (page, endpoint) in [
        (
            "https://github.com/owner/plugins",
            "https://raw.githubusercontent.com/owner/plugins/main/repository/manifest.json",
        ),
        (
            "https://github.com/owner/plugins/tree/develop",
            "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
        ),
        (
            "https://github.com/owner/plugins/blob/develop/repository/manifest.json",
            "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
        ),
        (
            "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
            "https://raw.githubusercontent.com/owner/plugins/develop/repository/manifest.json",
        ),
        (
            "https://catalogue.example.com/custom.json",
            "https://catalogue.example.com/custom.json",
        ),
    ] {
        assert_eq!(catalogue_url(page).unwrap().as_str(), endpoint);
    }
    assert!(catalogue_url("http://github.com/owner/plugins/tree/develop").is_err());
    assert!(catalogue_url("https://user:secret@github.com/owner/plugins").is_err());
}

#[test]
fn repository_url_and_address_policy_rejects_local_redirection_targets() {
    for url in [
        "http://example.com/manifest.json",
        "https://localhost/manifest.json",
        "https://127.0.0.1/x",
        "https://[::1]/x",
        "https://user:secret@example.com/x",
        "https://example.com:8443/x",
        "https://example.com/x#fragment",
    ] {
        assert!(safe_url(url).is_err(), "{url}");
    }
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "100.64.0.1",
        "198.18.0.1",
        "192.0.2.1",
        "::1",
        "::ffff:127.0.0.1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "2002:7f00:1::1",
    ] {
        assert!(!public_ip(ip.parse().unwrap()), "{ip}");
    }
    assert!(public_ip("8.8.8.8".parse().unwrap()));
    assert!(public_ip("2001:4860:4860::8888".parse().unwrap()));
    assert!(safe_url("https://example.com/manifest.json").is_ok());
}

#[test]
fn image_type_accepts_safe_svg_and_rejects_malicious_vectors() {
    assert_eq!(
        image_type(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>"),
        Some("image/svg+xml")
    );
    assert_eq!(image_type(b"<svg><script>alert(1)</script></svg>"), None);
    assert_eq!(image_type(b"<svg onload=\"alert(1)\"><rect/></svg>"), None);
    assert_eq!(image_type(b"<svg onerror=\"alert(1)\"><rect/></svg>"), None);
    assert_eq!(
        image_type(b"<svg><foreignObject><script>alert(1)</script></foreignObject></svg>"),
        None
    );
    assert_eq!(
        image_type(b"<svg><a href=\"javascript:alert(1)\"><rect/></a></svg>"),
        None
    );
    assert_eq!(
        image_type(b"<svg onmouseover=\"alert(1)\"><rect/></svg>"),
        None
    );
    assert_eq!(image_type(b"<svg onfocus=alert(1)><rect/></svg>"), None);
    assert_eq!(
        image_type(b"<svg><iframe src=\"https://evil.com\"/></svg>"),
        None
    );
    assert_eq!(
        image_type(b"<!DOCTYPE svg [<!ENTITY x \"test\">]><svg>&x;</svg>"),
        None
    );
    assert_eq!(
        image_type(b"<svg id=\"icon-one\"><polygon points=\"0,0 10,10\"/></svg>"),
        Some("image/svg+xml")
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires built Web assets and Chrome/Playwright; HTTPS documents injected only in this test build"]
async fn browser_repository_catalogue_version_trust_and_install() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    std::env::set_current_dir(&root).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap(
            "repo-browser",
            zeroize::Zeroizing::new("repository browser fixture password".into()),
        )
        .await
        .unwrap();
    let key_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(key_bytes.as_ref()).unwrap();
    let public_key = STANDARD.encode(key.public_key().as_ref());
    let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
    let manifest_url = format!(
        "https://repository.example.com/{}/manifest.json",
        uuid::Uuid::new_v4()
    );
    let mut entries = Vec::new();
    let mut package_urls = Vec::new();
    for version in ["1.0.0", "1.1.0"] {
        let mut package = Package {
            manifest: Manifest {
                api_version: 1,
                id: "repo.browser".into(),
                name: "Repository browser fixture".into(),
                version: version.into(),
                description: "Two signed versions for the repository browser flow".into(),
                author: "Fixture author".into(),
                license: "MIT".into(),
                repository: "https://example.com/source".into(),
                permissions: vec!["metadata.read".into()],
                ..Default::default()
            },
            publisher_id: "repository-browser".into(),
            wasm: STANDARD.encode(&wasm),
            sha256: format!("{:x}", Sha256::digest(&wasm)),
            signature: String::new(),
        };
        package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
        let bytes = serde_json::to_vec(&package).unwrap();
        let package_url = format!(
            "https://repository.example.com/{}/plugin-{version}.json",
            uuid::Uuid::new_v4()
        );
        entries.push(json!({"manifest":package.manifest,"publisherId":package.publisher_id,"publicKey":public_key,"packageUrl":package_url,"packageSha256":format!("{:x}",Sha256::digest(&bytes))}));
        test_documents()
            .lock()
            .unwrap()
            .insert(package_url.clone(), bytes);
        package_urls.push(package_url);
    }
    test_documents().lock().unwrap().insert(
        manifest_url.clone(),
        serde_json::to_vec(&json!({"apiVersion":1,"extensions":entries})).unwrap(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = format!("http://127.0.0.1:{port}");
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let app = crate::secure_app(
        crate::node::Inventory::local(media).await.unwrap(),
        port,
        auth,
    )
    .await
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let result = tokio::process::Command::new("node")
        .arg("scripts/test-repositories-live.mjs")
        .env("MEDIA_TEST_ORIGIN", origin)
        .env("MEDIA_TEST_REPOSITORY_URL", &manifest_url)
        .current_dir(root.join("apps/web"))
        .status()
        .await
        .unwrap();
    server.abort();
    store.close().await;
    let mut documents = test_documents().lock().unwrap();
    documents.remove(&manifest_url);
    for url in package_urls {
        documents.remove(&url);
    }
    assert!(result.success(), "repository catalogue browser flow failed");
}

#[tokio::test]
async fn default_repository_is_registered_once_and_deletion_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("default-repository.sqlite");
    let store = crate::identity::IdentityStore::open(&path).await.unwrap();
    let (url, snapshot): (String, String) = query_as(
        "SELECT url,manifest_json FROM extension_repositories WHERE id='community-default'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(
        url,
        "https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json"
    );
    assert!(parse_manifest(snapshot.as_bytes()).unwrap().is_empty());
    let installed: i64 = query_scalar("SELECT COUNT(*) FROM extensions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(installed, 0);
    query("DELETE FROM extension_repositories WHERE id='community-default'")
        .execute(store.pool())
        .await
        .unwrap();
    store.pool().close().await;
    let reopened = crate::identity::IdentityStore::open(&path).await.unwrap();
    let count: i64 =
        query_scalar("SELECT COUNT(*) FROM extension_repositories WHERE id='community-default'")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}
#[tokio::test]
async fn repository_catalogue_install_preserves_trust_signature_and_persistence() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap(
            "repo-admin",
            zeroize::Zeroizing::new("repository fixture password long".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = crate::auth::router(auth.clone()).merge(super::super::router(auth));
    let login = request(
        &app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":"repo-admin","password":"repository fixture password long"}),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf = body(login).await["csrfToken"].as_str().unwrap().to_owned();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let public = STANDARD.encode(key.public_key().as_ref());
    let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
    let mut package = Package {
        manifest: Manifest {
            api_version: 1,
            id: "repo.fixture".into(),
            name: "Repository fixture".into(),
            version: "1.0.0".into(),
            description: "Synthetic test".into(),
            author: "Tests".into(),
            license: "MIT".into(),
            repository: "https://example.com/source".into(),
            permissions: vec!["metadata.read".into()],
            ..Default::default()
        },
        publisher_id: "repository-test".into(),
        wasm: STANDARD.encode(&wasm),
        sha256: format!("{:x}", Sha256::digest(&wasm)),
        signature: String::new(),
    };
    package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
    let package_bytes = serde_json::to_vec(&package).unwrap();
    let package_url = format!(
        "https://packages.example.com/{}/plugin.json",
        uuid::Uuid::new_v4()
    );
    let manifest_url = format!(
        "https://packages.example.com/{}/manifest.json",
        uuid::Uuid::new_v4()
    );
    let manifest = json!({"apiVersion":1,"extensions":[{"manifest":package.manifest,"publisherId":package.publisher_id,"publicKey":public,"packageUrl":package_url,"packageSha256":format!("{:x}",Sha256::digest(&package_bytes))}]});
    test_documents()
        .lock()
        .unwrap()
        .insert(manifest_url.clone(), serde_json::to_vec(&manifest).unwrap());
    test_documents()
        .lock()
        .unwrap()
        .insert(package_url.clone(), package_bytes.clone());
    let input = json!({"name":"Repository test","url":manifest_url});
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/extensions/repositories",
            "POST",
            "",
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "/api/v1/admin/extensions/repositories",
            "POST",
            &cookie,
            "",
            input.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let created = request(
        &app,
        "/api/v1/admin/extensions/repositories",
        "POST",
        &cookie,
        &csrf,
        input,
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = body(created).await["id"].as_str().unwrap().to_owned();
    assert_eq!(
        body(
            request(
                &app,
                "/api/v1/admin/extensions/repositories",
                "GET",
                &cookie,
                "",
                json!(null)
            )
            .await
        )
        .await["repositories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "Repository test")
            .unwrap()["name"],
        "Repository test"
    );
    let available = body(
        request(
            &app,
            "/api/v1/admin/extensions/catalogue",
            "GET",
            &cookie,
            "",
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(available["entries"][0]["compatible"], true);
    let endpoint = format!("/api/v1/admin/extensions/repositories/{id}/install");
    let choice = json!({"extensionId":"repo.fixture","version":"1.0.0"});
    // A newly discovered publisher needs no approval request. Integrity is still checked.
    test_documents()
        .lock()
        .unwrap()
        .insert(package_url.clone(), b"tampered".to_vec());
    assert_eq!(
        request(&app, &endpoint, "POST", &cookie, &csrf, choice.clone())
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    test_documents()
        .lock()
        .unwrap()
        .insert(package_url.clone(), package_bytes);
    assert_eq!(
        request(&app, &endpoint, "POST", &cookie, &csrf, choice)
            .await
            .status(),
        StatusCode::CREATED
    );
    let installed = body(
        request(
            &app,
            "/api/v1/admin/extensions",
            "GET",
            &cookie,
            "",
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(installed["extensions"][0]["enabled"], false);
    assert_eq!(installed["extensions"][0]["libraryIds"], json!([]));
    // A catalogue update must use the guarded upgrade, never uninstall first.
    let before: (String, String) =
        query_as("SELECT package_json,generation FROM extensions WHERE id='repo.fixture'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    let mut newer = package.clone();
    newer.manifest.version = "1.0.1".into();
    newer.signature = STANDARD.encode(key.sign(&newer.signing_bytes().unwrap()).as_ref());
    let newer_bytes = serde_json::to_vec(&newer).unwrap();
    let newer_manifest = json!({"apiVersion":1,"extensions":[{"manifest":newer.manifest,"publisherId":newer.publisher_id,"publicKey":public,"packageUrl":package_url,"packageSha256":format!("{:x}",Sha256::digest(&newer_bytes))}]});
    query("UPDATE extension_repositories SET manifest_json=? WHERE id=?")
        .bind(newer_manifest.to_string())
        .bind(&id)
        .execute(store.pool())
        .await
        .unwrap();
    test_documents()
        .lock()
        .unwrap()
        .insert(package_url.clone(), newer_bytes);
    let upgraded = request(
        &app,
        &endpoint,
        "POST",
        &cookie,
        &csrf,
        json!({"extensionId":"repo.fixture","version":"1.0.1","upgrade":true}),
    )
    .await;
    assert_eq!(upgraded.status(), StatusCode::CREATED);
    assert_eq!(body(upgraded).await["id"], "repo.fixture");
    let after: (String, String) =
        query_as("SELECT package_json,generation FROM extensions WHERE id='repo.fixture'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_ne!(before.0, after.0);
    assert_ne!(before.1, after.1);
    query("UPDATE extension_repositories SET manifest_json=? WHERE id=?")
        .bind(manifest.to_string())
        .bind(&id)
        .execute(store.pool())
        .await
        .unwrap();

    // Downgrade is explicit, guarded by the current revision and preserves a snapshot.
    test_documents()
        .lock()
        .unwrap()
        .insert(package_url.clone(), serde_json::to_vec(&package).unwrap());
    let downgrade = json!({"extensionId":"repo.fixture","version":"1.0.0","upgrade":true,"confirmDowngrade":true,"expectedVersion":"1.0.1","expectedSha256":newer.sha256});
    assert_eq!(
        request(
            &app,
            &endpoint,
            "POST",
            &cookie,
            &csrf,
            json!({"extensionId":"repo.fixture","version":"1.0.0","upgrade":true})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let mut stale = downgrade.clone();
    stale["expectedVersion"] = json!("1.0.0");
    assert_eq!(
        request(&app, &endpoint, "POST", &cookie, &csrf, stale)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(&app, &endpoint, "POST", &cookie, "", downgrade.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    // A currently installed extension with private storage cannot be downgraded
    // without a verified inverse migration, even when its namespace is empty.
    let mut private_installed = newer.clone();
    private_installed
        .manifest
        .permissions
        .push("storage.private".into());
    query("UPDATE extensions SET package_json=? WHERE id='repo.fixture'")
        .bind(serde_json::to_string(&private_installed).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    let unsafe_attempt = request(&app, &endpoint, "POST", &cookie, &csrf, downgrade.clone()).await;
    assert_eq!(unsafe_attempt.status(), StatusCode::CONFLICT);
    assert_eq!(
        body(unsafe_attempt).await["error"],
        "extension_downgrade_incompatible"
    );
    query("UPDATE extensions SET package_json=? WHERE id='repo.fixture'")
        .bind(serde_json::to_string(&newer).unwrap())
        .execute(store.pool())
        .await
        .unwrap();
    // The 1.0.0 snapshot left by the upgrade holds no private data, so it is
    // replaced by a snapshot of 1.0.1 instead of blocking the downgrade.
    let snapshots: i64 = query_scalar(
        "SELECT COUNT(*) FROM extension_update_revisions WHERE extension_id='repo.fixture'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(snapshots, 1);
    assert_eq!(
        request(&app, &endpoint, "POST", &cookie, &csrf, downgrade)
            .await
            .status(),
        StatusCode::CREATED
    );
    let restored = body(
        request(
            &app,
            "/api/v1/admin/extensions",
            "GET",
            &cookie,
            "",
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(restored["extensions"][0]["manifest"]["version"], "1.0.0");
    assert_eq!(restored["extensions"][0]["enabled"], false);
    assert_eq!(restored["extensions"][0]["rollbackVersion"], "1.0.1");
    assert_eq!(request(&app, "/api/v1/admin/extensions/repo.fixture/rollback", "POST", &cookie, &csrf, json!({"expectedSha256":package.sha256,"expectedVersion":"1.0.0","confirmDataRestore":true})).await.status(), StatusCode::OK);

    assert_eq!(
        request(
            &app,
            &format!("/api/v1/admin/extensions/repositories/{id}/refresh"),
            "POST",
            &cookie,
            &csrf,
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_repositories WHERE id=?")
        .bind(&id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        request(
            &app,
            &format!("/api/v1/admin/extensions/repositories/{id}"),
            "DELETE",
            &cookie,
            &csrf,
            json!(null)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(
            request(
                &app,
                "/api/v1/admin/extensions",
                "GET",
                &cookie,
                "",
                json!(null)
            )
            .await
        )
        .await["extensions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    test_documents().lock().unwrap().remove(&manifest_url);
    test_documents().lock().unwrap().remove(&package_url);
}

fn zip(name: &str, content: &[u8], deflate: bool) -> Vec<u8> {
    use std::io::Write as _;
    let data = if deflate {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(content).unwrap();
        encoder.finish().unwrap()
    } else {
        content.to_vec()
    };
    let mut crc = flate2::Crc::new();
    crc.update(content);
    let method: u16 = if deflate { 8 } else { 0 };
    let mut out = Vec::new();
    out.extend_from_slice(b"PK\x03\x04");
    for value in [20u16, 0, method, 0, 0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for value in [crc.sum(), data.len() as u32, content.len() as u32] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&data);
    let directory = out.len();
    out.extend_from_slice(b"PK\x01\x02");
    for value in [20u16, 20, 0, method, 0, 0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for value in [crc.sum(), data.len() as u32, content.len() as u32] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for value in [name.len() as u16, 0, 0, 0, 0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    let size = out.len() - directory;
    out.extend_from_slice(b"PK\x05\x06");
    for value in [0u16, 0, 1, 1] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(directory as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn release_assets_may_be_zips_with_bounded_extraction() {
    let package = br#"{"manifest":{}}"#;
    for deflate in [false, true] {
        let archive = zip("dist/plugin-1.0.0.package.json", package, deflate);
        assert_eq!(package_bytes(&archive).unwrap(), package);
    }
    assert_eq!(package_bytes(package).unwrap(), package);
    assert!(package_bytes(&zip("readme.txt", package, true)).is_err());
    let mut corrupted = zip("plugin.package.json", package, false);
    // Flip one byte of the stored data: the CRC-32 no longer matches.
    corrupted[30 + "plugin.package.json".len() + 2] ^= 1;
    assert!(package_bytes(&corrupted).is_err());
    // A highly compressible bomb is refused from its declared size.
    let bomb = zip("plugin.package.json", &vec![b' '; MAX_PACKAGE + 1], true);
    assert!(bomb.len() < 64 * 1024);
    assert!(package_bytes(&bomb).is_err());
    assert!(package_bytes(&vec![b' '; MAX_PACKAGE + 1]).is_err());
}

#[test]
fn redirects_and_abi_follow_the_published_policy() {
    let url = |raw: &str| url::Url::parse(raw).unwrap();
    let release = url("https://github.com/o/r/releases/download/v1/p.zip");
    for allowed in [
        "https://objects.githubusercontent.com/x",
        "https://release-assets.githubusercontent.com/x?jwt=1",
        "https://codeload.github.com/x",
        "https://github.com/o/r/other",
    ] {
        assert!(redirect_allowed(&release, &url(allowed)), "{allowed}");
    }
    for refused in [
        "https://evil.example/x",
        "https://raw.githubusercontent.com/x",
    ] {
        assert!(!redirect_allowed(&release, &url(refused)), "{refused}");
    }
    assert!(!redirect_allowed(
        &url("https://example.com/manifest.json"),
        &url("https://objects.githubusercontent.com/x")
    ));
    assert!(redirect_allowed(
        &url("https://api.github.com/repos/o/r/releases/assets/1"),
        &url("https://release-assets.githubusercontent.com/x?jwt=1")
    ));
    assert_eq!(
        github_release_asset(
            "https://github.com/o/r/releases/download/p-v0.1.6-beta/p-0.1.6.package.json"
        ),
        Some((
            "o".into(),
            "r".into(),
            "p-v0.1.6-beta".into(),
            "p-0.1.6.package.json".into()
        ))
    );
    for other in [
        "https://raw.githubusercontent.com/o/r/main/p.json",
        "https://github.com/o/r/releases/download/v1/p.json?x=1",
        "https://github.com/o/r/releases/download/v1/../p.json",
        "https://github.com/o/r/blob/main/p.json",
    ] {
        assert_eq!(github_release_asset(other), None, "{other}");
    }
    assert!(abi_supported("1") && abi_supported("1.0") && abi_supported("1.0.0.0"));
    assert!(abi_supported("1.1") && abi_supported("1.1.0"));
    assert!(!abi_supported("1.2") && !abi_supported("2.0") && !abi_supported("0.9"));
    // Only a package built for 1.1 receives requests larger than the 1.0 size.
    assert!(super::accepts_large_input(Some("1.1")) && super::accepts_large_input(Some("1.1.0")));
    assert!(!super::accepts_large_input(Some("1.0")) && !super::accepts_large_input(None));
    assert!(!super::accepts_large_input(Some("x")));
    assert!(!abi_supported("1.0.0.0.0") && !abi_supported("x"));
}

#[test]
fn versioned_catalogue_lists_every_version_and_keeps_limits() {
    let key = STANDARD.encode([7u8; 32]);
    let version = |number: &str| {
        json!({"version":number,"targetAbi":"1.0","sourceUrl":format!("https://github.com/o/r/releases/download/x-v{number}/x-{number}.zip"),
            "checksum":format!("sha256:{}", "A".repeat(64)),"timestamp":"2026-11-02T09:44:30Z","changelog":"- fix"})
    };
    let plugin = json!({"guid":"poster.example","name":"Example","description":"d","overview":"o","owner":"Mael","category":"Metadata",
        "imageUrl":"https://raw.githubusercontent.com/o/r/main/logo.png","publisherId":"mael","publicKey":key,
        "versions":[version("1.2.0"),version("1.1.0")]});
    let items = parse_manifest(&serde_json::to_vec(&json!([plugin.clone()])).unwrap()).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1].version, "1.1.0");
    assert_eq!(items[0].checksum, "a".repeat(64));
    assert!(item_compatible(&items[0]));
    let object = json!({"apiVersion":2,"plugins":[plugin.clone()]});
    assert_eq!(
        parse_manifest(&serde_json::to_vec(&object).unwrap())
            .unwrap()
            .len(),
        2
    );
    let mut md5 = plugin.clone();
    md5["versions"][0]["checksum"] = json!("0".repeat(32));
    let mut duplicate = plugin.clone();
    duplicate["versions"][1]["version"] = json!("1.2.0");
    let mut redirect_url = plugin.clone();
    redirect_url["versions"][0]["sourceUrl"] = json!("http://github.com/x.zip");
    let mut algorithm = plugin.clone();
    algorithm["versions"][0]["checksumAlgorithm"] = json!("md5");
    for invalid in [md5, duplicate, redirect_url, algorithm] {
        assert!(parse_manifest(&serde_json::to_vec(&json!([invalid])).unwrap()).is_err());
    }
    let mut many = plugin;
    many["versions"] = json!(
        (0..=MAX_VERSIONS)
            .map(|n| version(&format!("1.0.{n}")))
            .collect::<Vec<_>>()
    );
    assert!(parse_manifest(&serde_json::to_vec(&json!([many])).unwrap()).is_err());
    let mut newer_abi = items[0].clone();
    newer_abi.target_abi = Some("1.2".into());
    assert!(!item_compatible(&newer_abi));
}

#[tokio::test]
async fn versioned_catalogue_installs_zip_assets_and_older_versions() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::identity::IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap(
            "repo-admin",
            zeroize::Zeroizing::new("repository fixture password long".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = crate::auth::router(auth.clone()).merge(super::super::router(auth));
    let login = request(
        &app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":"repo-admin","password":"repository fixture password long"}),
    )
    .await;
    let cookie = login.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf = body(login).await["csrfToken"].as_str().unwrap().to_owned();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    let public = STANDARD.encode(key.public_key().as_ref());
    let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
    let run = uuid::Uuid::new_v4();
    let mut versions = Vec::new();
    let mut urls = Vec::new();
    for (number, settings) in [("1.0.0", 1), ("1.1.0", 2)] {
        let mut package = Package {
            manifest: Manifest {
                api_version: 1,
                id: "repo.versioned".into(),
                name: "Versioned fixture".into(),
                version: number.into(),
                description: "Versioned catalogue".into(),
                author: "Tests".into(),
                license: "MIT".into(),
                repository: "https://example.com/source".into(),
                permissions: vec!["metadata.read".into()],
                target_abi: Some("1.0".into()),
                // The newer release adds an optional field: downgrading keeps saved values valid.
                config_schema: Some(
                    (0..settings)
                        .map(|n| media_extension_sdk::ConfigField {
                            key: format!("field_{n}"),
                            label: "Field".into(),
                            field_type: "text".into(),
                            ..Default::default()
                        })
                        .collect(),
                ),
                ..Default::default()
            },
            publisher_id: "repository-test".into(),
            wasm: STANDARD.encode(&wasm),
            sha256: format!("{:x}", Sha256::digest(&wasm)),
            signature: String::new(),
        };
        package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
        let json_bytes = serde_json::to_vec(&package).unwrap();
        // The older release is published as a zip, the newer as the signed JSON.
        let (asset, url) = if number == "1.0.0" {
            (
                zip("repo.versioned-1.0.0.package.json", &json_bytes, true),
                format!("https://github.com/{run}/releases/download/v{number}/plugin.zip"),
            )
        } else {
            (
                json_bytes,
                format!("https://github.com/{run}/releases/download/v{number}/plugin.package.json"),
            )
        };
        versions.push(json!({"version":number,"targetAbi":"1.0","sourceUrl":url,"checksum":format!("{:x}",Sha256::digest(&asset)),
            "timestamp":format!("2026-11-0{}T09:44:30Z", settings),"changelog":format!("- release {number}"),"permissions":["metadata.read"]}));
        test_documents().lock().unwrap().insert(url.clone(), asset);
        urls.push(url);
    }
    versions.reverse();
    let image_url = format!("https://raw.githubusercontent.com/{run}/main/logo.svg");
    test_documents().lock().unwrap().insert(
        image_url.clone(),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
    );
    let manifest_url = format!("https://raw.githubusercontent.com/{run}/main/manifest.json");
    let catalogue = json!({"apiVersion":2,"plugins":[{"id":"repo.versioned","name":"Versioned fixture","description":"Versioned catalogue",
        "owner":"Tests","category":"Metadata","imageUrl":image_url,"publisherId":"repository-test","publicKey":public,"versions":versions}]});
    test_documents().lock().unwrap().insert(
        manifest_url.clone(),
        serde_json::to_vec(&catalogue).unwrap(),
    );
    let created = request(
        &app,
        "/api/v1/admin/extensions/repositories",
        "POST",
        &cookie,
        &csrf,
        json!({"name":"Versioned","url":manifest_url}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = body(created).await["id"].as_str().unwrap().to_owned();
    let listed = body(
        request(
            &app,
            "/api/v1/admin/extensions/catalogue",
            "GET",
            &cookie,
            "",
            json!(null),
        )
        .await,
    )
    .await;
    let entries: Vec<_> = listed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["repositoryId"] == id)
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["manifest"]["version"], "1.0.0");
    assert_eq!(entries[1]["changelog"], "- release 1.0.0");
    assert_eq!(entries[1]["owner"], "Tests");
    assert_eq!(entries[0]["hasImage"], true);
    assert_eq!(entries[0]["compatible"], true);
    let image = body(
        request(
            &app,
            &format!("/api/v1/admin/extensions/repositories/{id}/image/repo.versioned"),
            "GET",
            &cookie,
            "",
            json!(null),
        )
        .await,
    )
    .await;
    assert!(
        image["dataUrl"]
            .as_str()
            .unwrap()
            .starts_with("data:image/svg+xml;base64,")
    );
    let endpoint = format!("/api/v1/admin/extensions/repositories/{id}/install");
    assert_eq!(
        request(
            &app,
            &endpoint,
            "POST",
            &cookie,
            &csrf,
            json!({"extensionId":"repo.versioned","version":"1.1.0"})
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let installed: String =
        query_scalar("SELECT package_json FROM extensions WHERE id='repo.versioned'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    let installed: Package = serde_json::from_str(&installed).unwrap();
    // The previous version is installed from its zip through the confirmed downgrade path.
    let downgrade = json!({"extensionId":"repo.versioned","version":"1.0.0","upgrade":true,"confirmDowngrade":true,
        "expectedVersion":"1.1.0","expectedSha256":installed.sha256});
    let response = request(&app, &endpoint, "POST", &cookie, &csrf, downgrade).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let version: String =
        query_scalar("SELECT package_json FROM extensions WHERE id='repo.versioned'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<Package>(&version)
            .unwrap()
            .manifest
            .version,
        "1.0.0"
    );
    // A catalogue that misdescribes the signed permissions is refused.
    let mut misleading = catalogue.clone();
    misleading["plugins"][0]["versions"][0]["permissions"] = json!(["images"]);
    query("UPDATE extension_repositories SET manifest_json=? WHERE id=?")
        .bind(misleading.to_string())
        .bind(&id)
        .execute(store.pool())
        .await
        .unwrap();
    let refused = request(
        &app,
        &endpoint,
        "POST",
        &cookie,
        &csrf,
        json!({"extensionId":"repo.versioned","version":"1.1.0","upgrade":true}),
    )
    .await;
    assert_eq!(body(refused).await["error"], "catalogue_package_mismatch");
    let mut documents = test_documents().lock().unwrap();
    for url in urls.iter().chain([&manifest_url, &image_url]) {
        documents.remove(url);
    }
}
