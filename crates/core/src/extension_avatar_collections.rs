//! Administrator-imported avatar collections. Only normalized images are stored;
//! URLs are resolved by a bounded HTTPS broker, never by the browser or Wasm.
use super::*;
use axum::response::IntoResponse;
use futures_util::{StreamExt, stream};
use std::collections::BTreeMap;
static IMPORTS: Semaphore = Semaphore::const_new(1);
const IMAGE_MAX: usize = 2 * 1024 * 1024;

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        .route(
            "/api/v1/admin/extensions/{id}/avatar-packs/upload",
            axum::routing::post(upload).layer(DefaultBodyLimit::max(65 * 1024 * 1024)),
        )
        .route(
            "/api/v1/admin/extensions/{id}/avatar-packs",
            get(list).post(import),
        )
        .route(
            "/api/v1/admin/extensions/{id}/avatar-packs/source",
            axum::routing::post(source),
        )
        .route(
            "/api/v1/admin/extensions/{id}/avatar-packs/{pack}",
            axum::routing::delete(remove),
        )
        .route("/api/v1/account/avatar/collections", get(account_packs))
        .route("/api/v1/account/avatar/collections/{pack}", get(images))
        .route(
            "/api/v1/account/avatar/collections/{pack}/{image}",
            get(image).post(apply),
        )
}
async fn check(auth: &AuthState, id: &str) -> Result<(), Error> {
    let stored: Option<String> = query_scalar("SELECT e.package_json FROM extensions e JOIN extension_publishers p ON p.id=e.publisher_id WHERE e.id=? AND p.active=1")
        .bind(id).fetch_optional(auth.store.pool()).await.map_err(db)?;
    let package: Package = serde_json::from_str(
        &stored.ok_or_else(|| err(StatusCode::NOT_FOUND, "extension_unavailable"))?,
    )
    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "invalid_package"))?;
    adapters::authorize_operation(&package.manifest, "avatars.list")
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Source {
    url: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Import {
    url: String,
    folder: String,
    name: String,
}
#[derive(Deserialize)]
struct Tree {
    sha: String,
    truncated: bool,
    tree: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    size: Option<usize>,
}
fn repository(raw: &str) -> Result<(String, String, String), Error> {
    let invalid = || err(StatusCode::BAD_REQUEST, "github_avatar_repository_required");
    let url = reqwest::Url::parse(raw).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    let valid = |s: &str| {
        !s.is_empty()
            && s.len() <= 100
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    };
    if parts.len() != 2 || !parts.iter().all(|s| valid(s)) {
        return Err(invalid());
    }
    let owner = parts[0].to_string();
    let repo = parts[1].trim_end_matches(".git").to_string();
    if !valid(&repo) {
        return Err(invalid());
    }
    Ok((
        owner.clone(),
        repo.clone(),
        format!("https://api.github.com/repos/{owner}/{repo}/git/trees/HEAD?recursive=1"),
    ))
}
async fn tree(raw: &str) -> Result<(String, String, Tree), Error> {
    let (owner, repo, url) = repository(raw)?;
    let bytes = repositories::fetch(&url, 2 * 1024 * 1024).await?;
    let tree: Tree = serde_json::from_slice(&bytes)
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "invalid_avatar_source"))?;
    if tree.truncated
        || tree.tree.len() > 10000
        || tree.sha.len() != 40
        || !tree.sha.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(err(StatusCode::BAD_REQUEST, "avatar_source_too_large"));
    }
    Ok((owner, repo, tree))
}
fn image_entry(entry: &Entry) -> bool {
    entry.kind == "blob"
        && entry
            .path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && entry.size.is_some_and(|s| s > 0 && s <= IMAGE_MAX)
        && [".png", ".jpg", ".jpeg", ".webp"]
            .iter()
            .any(|e| entry.path.to_lowercase().ends_with(e))
}
async fn source(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Source>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    check(&auth, &id).await?;
    let (_, _, tree) = tree(&input.url).await?;
    let mut folders = BTreeMap::<String, usize>::new();
    for entry in tree.tree.iter().filter(|e| image_entry(e)) {
        let folder = entry
            .path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        *folders.entry(folder.to_string()).or_default() += 1;
    }
    Ok(Json(
        json!({"folders":folders.into_iter().map(|(folder,count)|json!({"folder":folder,"count":count})).collect::<Vec<_>>()}),
    ))
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    check(&auth, &id).await?;
    let rows:Vec<(String,String,String,i64)>=query_as("SELECT p.id,p.name,p.source,COUNT(i.id) FROM extension_avatar_packs p LEFT JOIN extension_avatar_images i ON i.pack_id=p.id WHERE p.extension_id=? GROUP BY p.id,p.name,p.source ORDER BY p.name").bind(id).fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"packs":rows.into_iter().map(|(id,name,source,count)|json!({"id":id,"name":name,"source":source,"count":count})).collect::<Vec<_>>()}),
    ))
}
async fn import(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Import>,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;
    check(&auth, &id).await?;
    if input.name.trim().is_empty()
        || input.name.len() > 100
        || input.name.chars().any(char::is_control)
        || input.folder.len() > 500
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_avatar_pack"));
    }
    let _slot = IMPORTS
        .try_acquire()
        .map_err(|_| err(StatusCode::CONFLICT, "avatar_import_busy"))?;
    tokio::time::timeout(Duration::from_secs(300), async {
        let (owner, repo, tree) = tree(&input.url).await?;
        let entries: Vec<_> = tree
            .tree
            .into_iter()
            .filter(|entry| {
                image_entry(entry)
                    && entry.path.rsplit_once('/').map(|(p, _)| p).unwrap_or("") == input.folder
            })
            .collect();
        if entries.is_empty()
            || entries.len() > 500
            || entries.iter().map(|e| e.size.unwrap_or(0)).sum::<usize>() > 64 * 1024 * 1024
        {
            return Err(err(StatusCode::BAD_REQUEST, "avatar_pack_size_limit"));
        }
        let sha = tree.sha;
        let pack = uuid::Uuid::new_v4().to_string();
        let mut total = 0usize;
        let mut count = 0;
        let mut staged = Vec::new();
        let mut pending = stream::iter(entries.into_iter().map(|entry| {
            let mut url = reqwest::Url::parse(&format!(
                "https://raw.githubusercontent.com/{owner}/{repo}/{sha}/"
            ))
            .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_avatar_source"));
            if let Ok(url) = &mut url
                && let Ok(mut segments) = url.path_segments_mut()
            {
                segments.pop_if_empty();
                for part in entry.path.split('/') {
                    segments.push(part);
                }
            }
            async move {
                let bytes = repositories::fetch(url?.as_str(), IMAGE_MAX).await?;
                let name = entry
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or("Avatar")
                    .to_string();
                let bytes = tokio::task::spawn_blocking(move || {
                    let format = image::guess_format(&bytes).map_err(|_| {
                        err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_avatar_image")
                    })?;
                    let mime = match format {
                        image::ImageFormat::Png => "image/png",
                        image::ImageFormat::Jpeg => "image/jpeg",
                        image::ImageFormat::WebP => "image/webp",
                        _ => {
                            return Err(err(
                                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                                "invalid_avatar_image",
                            ));
                        }
                    };
                    crate::avatars::normalize(bytes.into(), mime)
                        .map_err(|status| err(status, "invalid_avatar_image"))
                })
                .await
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "avatar_decode_failed"))??;
                Ok::<_, Error>((name, bytes))
            }
        }))
        .buffered(2);
        while let Some(result) = pending.next().await {
            let (name, bytes) = result?;
            total += bytes.len();
            if total > 32 * 1024 * 1024 {
                return Err(err(StatusCode::BAD_REQUEST, "avatar_pack_size_limit"));
            }
            staged.push((name, bytes));
            count += 1;
        }
        // Serialize only the final authorization check/commit with revocations and uninstall.
        let _lock = MUTATIONS.lock().await;
        check(&auth, &id).await?;
        let mut tx = auth.store.pool().begin().await.map_err(db)?;
        let packs: i64 = query_scalar("SELECT COUNT(*) FROM extension_avatar_packs")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        if packs >= 32 {
            return Err(err(StatusCode::CONFLICT, "avatar_pack_limit"));
        }
        query("INSERT INTO extension_avatar_packs(id,extension_id,name,source) VALUES(?,?,?,?)")
            .bind(&pack)
            .bind(&id)
            .bind(input.name.trim())
            .bind(format!(
                "https://github.com/{owner}/{repo}/tree/{sha}/{}",
                input.folder
            ))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        for (name, bytes) in staged {
            query(
                "INSERT INTO extension_avatar_images(id,pack_id,name,image_data) VALUES(?,?,?,?)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&pack)
            .bind(name)
            .bind(bytes)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }
        audit(&mut tx, &actor.user.id, &id, "avatars.pack.imported").await?;
        tx.commit().await.map_err(db)?;
        Ok(Json(json!({"id":pack,"count":count})))
    })
    .await
    .map_err(|_| err(StatusCode::GATEWAY_TIMEOUT, "avatar_import_timeout"))?
}
async fn upload(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut form: axum::extract::Multipart,
) -> Result<Json<Value>, Error> {
    let actor = admin(&auth, &headers, true).await?;
    check(&auth, &id).await?;
    let _slot = IMPORTS
        .try_acquire()
        .map_err(|_| err(StatusCode::CONFLICT, "avatar_import_busy"))?;
    tokio::time::timeout(Duration::from_secs(300),async {
    let invalid = || err(StatusCode::BAD_REQUEST, "invalid_avatar_upload");
    let mut name = None;
    let mut staged = Vec::new();
    let mut original = 0usize;
    let mut normalized = 0usize;
    while let Some(mut field) = form.next_field().await.map_err(|_| invalid())? {
        if field.name() == Some("name") && name.is_none() {
            let mut bytes = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(|_| invalid())? {
                if bytes.len() + chunk.len() > 100 {
                    return Err(invalid());
                }
                bytes.extend_from_slice(&chunk);
            }
            name = Some(String::from_utf8(bytes).map_err(|_| invalid())?);
            continue;
        }
        if field.name() != Some("images") || staged.len() >= 500 {
            return Err(invalid());
        }
        let filename = field
            .file_name()
            .unwrap_or("Avatar")
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("Avatar")
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>();
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|_| invalid())? {
            original += chunk.len();
            if bytes.len() + chunk.len() > IMAGE_MAX || original > 64 * 1024 * 1024 {
                return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "avatar_pack_size_limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let bytes = tokio::task::spawn_blocking(move || {
            let format = image::guess_format(&bytes)
                .map_err(|_| err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_avatar_image"))?;
            let mime = match format {
                image::ImageFormat::Png => "image/png",
                image::ImageFormat::Jpeg => "image/jpeg",
                image::ImageFormat::WebP => "image/webp",
                _ => {
                    return Err(err(
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "invalid_avatar_image",
                    ));
                }
            };
            crate::avatars::normalize(bytes.into(), mime)
                .map_err(|status| err(status, "invalid_avatar_image"))
        })
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "avatar_decode_failed"))??;
        normalized += bytes.len();
        if normalized > 32 * 1024 * 1024 {
            return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "avatar_pack_size_limit"));
        }
        staged.push((filename, bytes));
    }
    let name = name
        .filter(|name| !name.trim().is_empty() && !name.chars().any(char::is_control))
        .ok_or_else(invalid)?;
    if staged.is_empty() {
        return Err(invalid());
    }
    let _lock = MUTATIONS.lock().await;
    check(&auth, &id).await?;
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM extension_avatar_packs")
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if count >= 32 {
        return Err(err(StatusCode::CONFLICT, "avatar_pack_limit"));
    }
    let pack = uuid::Uuid::new_v4().to_string();
    let count = staged.len();
    query("INSERT INTO extension_avatar_packs(id,extension_id,name,source) VALUES(?,?,?,'upload')")
        .bind(&pack)
        .bind(&id)
        .bind(name.trim())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    for (name, bytes) in staged {
        query("INSERT INTO extension_avatar_images(id,pack_id,name,image_data) VALUES(?,?,?,?)")
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&pack)
            .bind(name)
            .bind(bytes)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    audit(&mut tx, &actor.user.id, &id, "avatars.pack.uploaded").await?;
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"id":pack,"count":count})))
    }).await.map_err(|_|err(StatusCode::GATEWAY_TIMEOUT,"avatar_import_timeout"))?
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, pack)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    check(&auth, &id).await?;
    query("DELETE FROM extension_avatar_packs WHERE id=? AND extension_id=?")
        .bind(pack)
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn account_packs(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    let rows:Vec<(String,String,i64)>=query_as("SELECT a.id,a.name,COUNT(i.id) FROM extension_avatar_packs a JOIN extensions e ON e.id=a.extension_id JOIN extension_publishers p ON p.id=e.publisher_id JOIN extension_avatar_images i ON i.pack_id=a.id WHERE e.enabled=1 AND p.active=1 GROUP BY a.id,a.name ORDER BY a.name").fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"packs":rows.into_iter().map(|(id,name,count)|json!({"id":id,"name":name,"count":count})).collect::<Vec<_>>()}),
    ))
}
async fn images(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(pack): Path<String>,
) -> Result<Json<Value>, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    let rows:Vec<(String,String)>=query_as("SELECT i.id,i.name FROM extension_avatar_images i JOIN extension_avatar_packs a ON a.id=i.pack_id JOIN extensions e ON e.id=a.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE a.id=? AND e.enabled=1 AND p.active=1 ORDER BY i.name LIMIT 500").bind(&pack).fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"images":rows.into_iter().map(|(id,name)|json!({"id":id,"name":name,"url":format!("/api/v1/account/avatar/collections/{pack}/{id}")})).collect::<Vec<_>>()}),
    ))
}
async fn bytes(auth: &AuthState, pack: &str, image: &str) -> Result<Vec<u8>, Error> {
    query_scalar("SELECT i.image_data FROM extension_avatar_images i JOIN extension_avatar_packs a ON a.id=i.pack_id JOIN extensions e ON e.id=a.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE a.id=? AND i.id=? AND e.enabled=1 AND p.active=1").bind(pack).bind(image).fetch_optional(auth.store.pool()).await.map_err(db)?.ok_or_else(||err(StatusCode::NOT_FOUND,"avatar_not_found"))
}
async fn image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((pack, id)): Path<(String, String)>,
) -> Result<axum::response::Response, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes(&auth, &pack, &id).await?,
    )
        .into_response())
}
async fn apply(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((pack, id)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    let actor = auth
        .authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    auth.require_csrf(&headers, &actor)
        .map_err(|s| err(s, "csrf_required"))?;
    let _lock = MUTATIONS.lock().await;
    let bytes = bytes(&auth, &pack, &id).await?;
    query("INSERT INTO user_avatars(user_id,image_data) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET image_data=excluded.image_data").bind(actor.user.id).bind(bytes).execute(auth.store.pool()).await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_repository_sources() {
        for value in [
            "http://github.com/a/b",
            "https://localhost/a/b",
            "https://github.com/a/b/tree/main",
            "https://github.com/a/../b",
            "https://github.com/a/b?token=x",
        ] {
            assert!(repository(value).is_err(), "{value}");
        }
        assert!(repository("https://github.com/cedev-1/jellyfin-avatars").is_ok());
    }
    #[tokio::test]
    async fn imports_normalized_collections_without_approval_and_enforces_activation() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use ring::{
            rand::SystemRandom,
            signature::{Ed25519KeyPair, KeyPair},
        };
        use tower::ServiceExt;
        async fn call(
            app: &Router,
            path: &str,
            method: &str,
            cookie: &str,
            csrf: &str,
            value: Value,
        ) -> (StatusCode, Vec<u8>, HeaderMap) {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::COOKIE, cookie)
                        .header(header::ORIGIN, "http://localhost")
                        .header("x-csrf-token", csrf)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(value.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec();
            (status, bytes, headers)
        }
        let root = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&root.path().join("avatars.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap(
                "admin",
                zeroize::Zeroizing::new("collection fixture password".into()),
            )
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), "http://localhost".into())
            .await
            .unwrap();
        let app = crate::auth::router(auth.clone()).merge(super::super::router(auth));
        let (status, body, headers) = call(
            &app,
            "/api/v1/auth/login",
            "POST",
            "",
            "",
            json!({"login":"admin","password":"collection fixture password"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let cookie = headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let login: Value = serde_json::from_slice(&body).unwrap();
        let csrf = login["csrfToken"].as_str().unwrap();
        let key = Ed25519KeyPair::from_pkcs8(
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .unwrap()
                .as_ref(),
        )
        .unwrap();
        let wasm=wat::parse_str("(module (memory (export \"memory\") 1 256) (func (export \"media_alloc\") (param i32) (result i32) i32.const 0) (func (export \"media_run\") (param i32 i32) (result i64) i64.const 0))").unwrap();
        let mut package = Package {
            manifest: Manifest {
                api_version: 1,
                id: "any-author-avatar".into(),
                name: "Créateur indépendant".into(),
                version: "1.0.0".into(),
                description: "Collection de test".into(),
                author: "Anyone".into(),
                license: "MIT".into(),
                repository: "https://example.org/plugin".into(),
                permissions: vec!["avatars".into()],
                ..Default::default()
            },
            publisher_id: "any-author".into(),
            sha256: format!("{:x}", Sha256::digest(&wasm)),
            wasm: STANDARD.encode(wasm),
            signature: String::new(),
        };
        package.signature = STANDARD.encode(key.sign(&package.signing_bytes().unwrap()).as_ref());
        let mut input = serde_json::to_value(package).unwrap();
        input["publicKey"] = json!(STANDARD.encode(key.public_key().as_ref()));
        assert_eq!(
            call(
                &app,
                "/api/v1/admin/extensions",
                "POST",
                cookie,
                csrf,
                input
            )
            .await
            .0,
            StatusCode::CREATED
        );
        let source_url = "https://github.com/tests/avatar-fixture";
        let tree_url =
            "https://api.github.com/repos/tests/avatar-fixture/git/trees/HEAD?recursive=1";
        let image_url = "https://raw.githubusercontent.com/tests/avatar-fixture/1111111111111111111111111111111111111111/pack/test.png";
        let mut cursor = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 48)
            .write_to(&mut cursor, image::ImageFormat::Png)
            .unwrap();
        let png = cursor.into_inner();
        repositories::test_documents().lock().unwrap().insert(tree_url.into(),serde_json::to_vec(&json!({"sha":"1111111111111111111111111111111111111111","truncated":false,"tree":[{"type":"blob","path":"pack/test.png","size":png.len()}]})).unwrap());
        repositories::test_documents()
            .lock()
            .unwrap()
            .insert(image_url.into(), b"invalid image".to_vec());
        let endpoint = "/api/v1/admin/extensions/any-author-avatar/avatar-packs";
        let payload = json!({"url":source_url,"folder":"pack","name":"Mes avatars"});
        assert_eq!(
            call(&app, endpoint, "POST", "", "", payload.clone())
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(&app, endpoint, "POST", cookie, "", payload.clone())
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&app, endpoint, "POST", cookie, csrf, payload.clone())
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM extension_avatar_packs")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            0
        );
        repositories::test_documents()
            .lock()
            .unwrap()
            .insert(image_url.into(), png);
        let (status, body, _) = call(&app, endpoint, "POST", cookie, csrf, payload).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let pack: Value = serde_json::from_slice(&body).unwrap();
        let pack = pack["id"].as_str().unwrap();
        let (_, body, _) = call(
            &app,
            "/api/v1/account/avatar/collections",
            "GET",
            cookie,
            "",
            json!(null),
        )
        .await;
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["packs"],
            json!([])
        );
        assert_eq!(
            call(
                &app,
                "/api/v1/admin/extensions/any-author-avatar",
                "PUT",
                cookie,
                csrf,
                json!({"enabled":true,"allLibraries":true})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        let (_, body, _) = call(
            &app,
            &format!("/api/v1/account/avatar/collections/{pack}"),
            "GET",
            cookie,
            "",
            json!(null),
        )
        .await;
        let images: Value = serde_json::from_slice(&body).unwrap();
        let url = images["images"][0]["url"].as_str().unwrap();
        assert_eq!(
            call(&app, url, "GET", "", "", json!(null)).await.0,
            StatusCode::UNAUTHORIZED
        );
        let (status, png, headers) = call(&app, url, "GET", cookie, "", json!(null)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "image/png");
        assert_eq!(image::load_from_memory(&png).unwrap().width(), 256);
        assert_eq!(
            call(&app, url, "POST", cookie, "", json!(null)).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&app, url, "POST", cookie, csrf, json!(null)).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM user_avatars")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            call(
                &app,
                "/api/v1/admin/extensions/any-author-avatar",
                "PUT",
                cookie,
                csrf,
                json!({"enabled":false})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(&app, url, "GET", cookie, "", json!(null)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(
                &app,
                &format!("{endpoint}/{pack}"),
                "DELETE",
                cookie,
                csrf,
                json!(null)
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM extension_avatar_images")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            0
        );

        // Multipart upload exceeds the general 400 kB extension limit while remaining
        // under its explicit image/upload bounds; all data is synthetic.
        let mut seed = 17u32;
        let noise: Vec<u8> = (0..400 * 400 * 3)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 24) as u8
            })
            .collect();
        let rgb = image::RgbImage::from_raw(400, 400, noise).unwrap();
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(rgb)
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let upload_png = output.into_inner();
        assert!(upload_png.len() > 400000);
        let boundary = "avatar-fixture-boundary";
        let mut multipart=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nImages locales\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"images\"; filename=\"local.png\"\r\nContent-Type: image/png\r\n\r\n").into_bytes();
        multipart.extend_from_slice(&upload_png);
        multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        for (session, token, expected) in [
            ("", "", StatusCode::UNAUTHORIZED),
            (cookie, "", StatusCode::FORBIDDEN),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("{endpoint}/upload"))
                        .header(header::COOKIE, session)
                        .header(header::ORIGIN, "http://localhost")
                        .header("x-csrf-token", token)
                        .header(
                            header::CONTENT_TYPE,
                            format!("multipart/form-data; boundary={boundary}"),
                        )
                        .body(Body::from(multipart.clone()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
        let uploaded = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("{endpoint}/upload"))
                    .header(header::COOKIE, cookie)
                    .header(header::ORIGIN, "http://localhost")
                    .header("x-csrf-token", csrf)
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(multipart))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = uploaded.status();
        let body = uploaded.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        let uploaded: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(uploaded["count"], 1);
        let pack = uploaded["id"].as_str().unwrap();
        let data: Vec<u8> =
            query_scalar("SELECT image_data FROM extension_avatar_images WHERE pack_id=?")
                .bind(pack)
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert_eq!(image::load_from_memory(&data).unwrap().width(), 256);
        assert_eq!(
            call(
                &app,
                &format!("{endpoint}/{pack}"),
                "DELETE",
                cookie,
                csrf,
                json!(null)
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        if std::env::var("MEDIA_TEST_AVATAR_LIVE").as_deref() == Ok("1") {
            let live = json!({"url":"https://github.com/cedev-1/jellyfin-avatars"});
            let (status, body, _) = call(
                &app,
                &format!("{endpoint}/source"),
                "POST",
                cookie,
                csrf,
                live,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let value: Value = serde_json::from_slice(&body).unwrap();
            let folder_count = value["folders"].as_array().unwrap().len();
            let expected = value["folders"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["folder"] == "sonny")
                .unwrap()["count"]
                .as_u64()
                .unwrap();
            let(status,body,_)=call(&app,endpoint,"POST",cookie,csrf,json!({"url":"https://github.com/cedev-1/jellyfin-avatars","folder":"sonny","name":"Sonny - live fixture"})).await;
            assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["count"].as_u64(), Some(expected));
            println!(
                "LIVE GitHub: {} folders discovered, {} normalized Sonny avatars imported into temporary SQLite",
                folder_count, expected
            );
        }
        for url in [tree_url, image_url] {
            repositories::test_documents().lock().unwrap().remove(url);
        }
    }
}
