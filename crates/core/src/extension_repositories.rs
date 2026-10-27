//! Explicit administrator-owned HTTPS catalogues. A catalogue never grants publisher trust.
use super::{Error, admin, db, err, identifier, now};
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use media_extension_sdk::{Manifest, Package};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::sync::Semaphore;

const MAX_MANIFEST: usize = 256 * 1024;
const MAX_PACKAGE: usize = 400_000;
static DOWNLOADS: Semaphore = Semaphore::const_new(2);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    manifest: Manifest,
    publisher_id: String,
    public_key: String,
    package_url: String,
    /// SHA-256 of the complete package JSON bytes, distinct from its Wasm digest.
    package_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RepositoryManifest {
    api_version: u32,
    extensions: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryInput {
    name: String,
    url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallInput {
    extension_id: String,
    version: String,
}

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route("/api/v1/admin/extensions/repositories", get(list).post(add))
        .route(
            "/api/v1/admin/extensions/repositories/{id}",
            axum::routing::delete(remove),
        )
        .route(
            "/api/v1/admin/extensions/repositories/{id}/refresh",
            post(refresh),
        )
        .route(
            "/api/v1/admin/extensions/repositories/{id}/install",
            post(install),
        )
        .route("/api/v1/admin/extensions/catalogue", get(catalogue))
}

fn safe_url(raw: &str) -> Result<url::Url, Error> {
    let url =
        url::Url::parse(raw).map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_repository_url"))?;
    if raw.len() > 2048
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
        || !matches!(url.host(),Some(url::Host::Domain(host)) if !host.eq_ignore_ascii_case("localhost") && !host.ends_with(".localhost"))
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "repository_https_public_host_required",
        ));
    }
    Ok(url)
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, _, _] = v.octets();
            !v.is_private()
                && !v.is_loopback()
                && !v.is_link_local()
                && !v.is_unspecified()
                && !v.is_broadcast()
                && !v.is_documentation()
                && !v.is_multicast()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (b == 18 || b == 19))
        }
        // Permit only global unicast; reject mapped IPv4, ULA, local and documentation ranges.
        IpAddr::V6(v) => {
            let s = v.segments();
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] <= 0x1ff || s[1] == 0xdb8))
        }
    }
}
pub(crate) async fn fetch(raw: &str, maximum: usize) -> Result<Vec<u8>, Error> {
    let url = safe_url(raw)?;
    #[cfg(test)]
    if let Some(bytes) = test_documents().lock().unwrap().get(raw).cloned() {
        return if bytes.len() <= maximum {
            Ok(bytes)
        } else {
            Err(err(
                StatusCode::BAD_GATEWAY,
                "repository_response_too_large",
            ))
        };
    }
    let _permit = DOWNLOADS
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "repository_busy"))?;
    let host = url
        .host_str()
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "invalid_repository_url"))?;
    let addresses: Vec<SocketAddr> =
        tokio::time::timeout(Duration::from_secs(3), tokio::net::lookup_host((host, 443)))
            .await
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
            .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
            .collect();
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses.iter().any(|address| !public_ip(address.ip()))
    {
        return Err(err(StatusCode::BAD_REQUEST, "repository_address_forbidden"));
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(3))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?;
    let mut response = client
        .get(url)
        .header("accept", "application/json")
        .header("user-agent", "Media-Server-Avatar-Import")
        .send()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?;
    if !response.status().is_success() {
        return Err(err(StatusCode::BAD_GATEWAY, "repository_http_error"));
    }
    if response
        .content_length()
        .is_some_and(|size| size > maximum as u64)
    {
        return Err(err(
            StatusCode::BAD_GATEWAY,
            "repository_response_too_large",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "repository_unavailable"))?
    {
        if bytes.len().saturating_add(chunk.len()) > maximum {
            return Err(err(
                StatusCode::BAD_GATEWAY,
                "repository_response_too_large",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn version(raw: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = raw.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty() || part.len() > 8 || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}
pub(super) fn compatible(manifest: &Manifest) -> bool {
    let current = media_contracts::version::current_version();
    let Some(current) = version(current.split('-').next().unwrap_or(current)) else {
        return false;
    };
    manifest.api_version == 1
        && manifest
            .target_abi
            .as_deref()
            .is_none_or(|abi| abi == "1.0")
        && manifest
            .min_server_version
            .as_deref()
            .is_none_or(|minimum| version(minimum).is_some_and(|minimum| current >= minimum))
        && manifest
            .max_server_version
            .as_deref()
            .is_none_or(|maximum| version(maximum).is_some_and(|maximum| current <= maximum))
}
fn parse_manifest(bytes: &[u8]) -> Result<RepositoryManifest, Error> {
    let document: RepositoryManifest = serde_json::from_slice(bytes)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_repository_manifest"))?;
    if document.api_version != 1 || document.extensions.len() > 100 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_repository_manifest"));
    }
    let mut unique = std::collections::HashSet::new();
    for entry in &document.extensions {
        let m = &entry.manifest;
        if !identifier(&m.id)
            || !identifier(&entry.publisher_id)
            || version(&m.version).is_none()
            || m.name.is_empty()
            || m.name.len() > 120
            || m.description.len() > 2048
            || entry.public_key.len() > 64
            || STANDARD
                .decode(&entry.public_key)
                .map_or(true, |key| key.len() != 32)
            || entry.package_sha256.len() != 64
            || !entry
                .package_sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            || !unique.insert((m.id.clone(), m.version.clone()))
        {
            return Err(err(StatusCode::BAD_REQUEST, "invalid_repository_manifest"));
        }
        safe_url(&entry.package_url)?;
    }
    Ok(document)
}
async fn list(State(auth): State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, String, i64)> =
        query_as("SELECT id,name,url,refreshed_at FROM extension_repositories ORDER BY name,id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    Ok(Json(
        json!({"repositories":rows.into_iter().map(|(id,name,url,time)|json!({"id":id,"name":name,"url":url,"refreshedAt":time})).collect::<Vec<_>>()}),
    ))
}
async fn add(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<RepositoryInput>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let name = input.name.trim();
    let url = safe_url(input.url.trim())?.to_string();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_repository_name"));
    }
    let bytes = fetch(&url, MAX_MANIFEST).await?;
    parse_manifest(&bytes)?;
    admin(&auth, &headers, true).await?;
    let _lock = super::MUTATIONS.lock().await;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_repositories")
        .fetch_one(auth.store.pool())
        .await
        .map_err(db)?;
    if count >= 16 {
        return Err(err(StatusCode::CONFLICT, "repository_limit"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    query("INSERT INTO extension_repositories(id,name,url,manifest_json,refreshed_at) VALUES(?,?,?,?,?)")
        .bind(&id).bind(name).bind(url).bind(String::from_utf8(bytes).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_repository_manifest"))?).bind(now())
        .execute(auth.store.pool()).await.map_err(|_|err(StatusCode::CONFLICT,"repository_exists"))?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn refresh(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let url: Option<String> = query_scalar("SELECT url FROM extension_repositories WHERE id=?")
        .bind(&id)
        .fetch_optional(auth.store.pool())
        .await
        .map_err(db)?;
    let bytes = fetch(
        &url.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_repository"))?,
        MAX_MANIFEST,
    )
    .await?;
    parse_manifest(&bytes)?;
    admin(&auth, &headers, true).await?;
    let changed =
        query("UPDATE extension_repositories SET manifest_json=?,refreshed_at=? WHERE id=?")
            .bind(
                String::from_utf8(bytes)
                    .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_repository_manifest"))?,
            )
            .bind(now())
            .bind(id)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    if changed.rows_affected() != 1 {
        return Err(err(StatusCode::NOT_FOUND, "unknown_repository"));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    query("DELETE FROM extension_repositories WHERE id=?")
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn catalogue(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    let rows: Vec<(String, String, String)> =
        query_as("SELECT id,name,manifest_json FROM extension_repositories ORDER BY name,id")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let mut entries = Vec::new();
    for (id, name, bytes) in rows {
        for entry in parse_manifest(bytes.as_bytes())?.extensions {
            entries.push(json!({"repositoryId":id,"repositoryName":name,"compatible":compatible(&entry.manifest),"manifest":entry.manifest,"publisherId":entry.publisher_id,"publicKey":entry.public_key}));
        }
    }
    Ok(Json(json!({"entries":entries})))
}
async fn install(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<InstallInput>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let snapshot: Option<String> =
        query_scalar("SELECT manifest_json FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    let snapshot = snapshot.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_repository"))?;
    let entry = parse_manifest(snapshot.as_bytes())?
        .extensions
        .into_iter()
        .find(|e| e.manifest.id == input.extension_id && e.manifest.version == input.version)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_catalogue_version"))?;
    if !compatible(&entry.manifest) {
        return Err(err(StatusCode::BAD_REQUEST, "incompatible_server_version"));
    }
    let bytes = fetch(&entry.package_url, MAX_PACKAGE).await?;
    if format!("{:x}", Sha256::digest(&bytes)) != entry.package_sha256 {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_package_digest"));
    }
    let package: Package = serde_json::from_slice(&bytes)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_package"))?;
    if package.publisher_id != entry.publisher_id
        || serde_json::to_value(&package.manifest).ok()
            != serde_json::to_value(&entry.manifest).ok()
    {
        return Err(err(StatusCode::BAD_REQUEST, "catalogue_package_mismatch"));
    }
    let current: Option<String> =
        query_scalar("SELECT manifest_json FROM extension_repositories WHERE id=?")
            .bind(&id)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?;
    if current.as_deref() != Some(snapshot.as_str()) {
        return Err(err(StatusCode::CONFLICT, "repository_changed"));
    }
    super::install_with_key(auth, headers, package, entry.public_key).await
}

#[cfg(test)]
pub(super) fn test_documents()
-> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>> {
    static DOCUMENTS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &DOCUMENTS
}

#[cfg(test)]
mod tests {
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
            package.signature =
                STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
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
        assert!(
            parse_manifest(snapshot.as_bytes())
                .unwrap()
                .extensions
                .is_empty()
        );
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
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM extension_repositories WHERE id='community-default'",
        )
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
}
