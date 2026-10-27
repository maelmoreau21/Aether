//! Administrator-imported avatar collections. Only normalized images are stored;
//! URLs are resolved by a bounded HTTPS broker, never by the browser or Wasm.
use super::*;

use futures_util::{StreamExt, stream};
use std::collections::BTreeMap;
static IMPORTS: Semaphore = Semaphore::const_new(1);
const IMAGE_MAX: usize = 2 * 1024 * 1024;

pub(super) fn router() -> Router<AuthState> {
    Router::new()
        // Native admin avatar-packs routes
        .route(
            "/api/v1/admin/avatar-packs/upload",
            axum::routing::post(native_upload).layer(DefaultBodyLimit::max(65 * 1024 * 1024)),
        )
        .route(
            "/api/v1/admin/avatar-packs",
            get(native_list).post(native_import),
        )
        .route(
            "/api/v1/admin/avatar-packs/source",
            axum::routing::post(native_source),
        )
        .route(
            "/api/v1/admin/avatar-packs/{pack}",
            axum::routing::delete(native_remove).put(native_rename),
        )
        .route(
            "/api/v1/admin/avatar-packs/{pack}/images",
            get(native_admin_images),
        )
        .route(
            "/api/v1/admin/avatar-packs/{pack}/images/{image}",
            get(native_admin_image).delete(native_remove_image),
        )
        .route(
            "/api/v1/admin/avatars/assign-missing",
            axum::routing::post(native_assign_missing),
        )
        // Extension-scoped routes (preserved for compatibility)
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
            axum::routing::delete(remove).put(rename),
        )
        .route(
            "/api/v1/admin/extensions/{id}/avatar-packs/{pack}/images",
            get(admin_images),
        )
        .route(
            "/api/v1/admin/extensions/{id}/avatar-packs/{pack}/images/{image}",
            get(admin_image).delete(remove_image),
        )
        .route(
            "/api/v1/admin/extensions/{id}/avatars/assign-missing",
            axum::routing::post(assign_missing),
        )
        .route(
            "/api/v1/account/avatar/gravatar",
            get(gravatar_policy).post(gravatar),
        )
        .route("/api/v1/account/avatar/collections", get(account_packs))
        .route("/api/v1/account/avatar/collections/{pack}", get(images))
        .route(
            "/api/v1/account/avatar/collections/{pack}/{image}",
            get(image).post(apply),
        )
}

async fn native_upload(
    state: State<AuthState>,
    headers: HeaderMap,
    body: axum::extract::Multipart,
) -> Result<Json<Value>, Error> {
    upload(state, headers, Path("\u{1}".into()), body).await
}

async fn native_list(state: State<AuthState>, headers: HeaderMap) -> Result<Json<Value>, Error> {
    list(state, headers, Path("\u{1}".into())).await
}

async fn native_import(
    state: State<AuthState>,
    headers: HeaderMap,
    body: Json<Import>,
) -> Result<Json<Value>, Error> {
    import(state, headers, Path("\u{1}".into()), body).await
}

async fn native_source(
    state: State<AuthState>,
    headers: HeaderMap,
    body: Json<Source>,
) -> Result<Json<Value>, Error> {
    source(state, headers, Path("\u{1}".into()), body).await
}

async fn native_remove(
    state: State<AuthState>,
    headers: HeaderMap,
    Path(pack): Path<String>,
) -> Result<StatusCode, Error> {
    remove(state, headers, Path(("\u{1}".into(), pack))).await
}

async fn native_rename(
    state: State<AuthState>,
    headers: HeaderMap,
    Path(pack): Path<String>,
    body: Json<Rename>,
) -> Result<StatusCode, Error> {
    rename(state, headers, Path(("\u{1}".into(), pack)), body).await
}

async fn native_admin_images(
    state: State<AuthState>,
    headers: HeaderMap,
    Path(pack): Path<String>,
) -> Result<Json<Value>, Error> {
    admin_images(state, headers, Path(("\u{1}".into(), pack))).await
}

async fn native_admin_image(
    state: State<AuthState>,
    headers: HeaderMap,
    Path((pack, image)): Path<(String, String)>,
) -> Result<axum::response::Response, Error> {
    admin_image(state, headers, Path(("\u{1}".into(), pack, image))).await
}

async fn native_remove_image(
    state: State<AuthState>,
    headers: HeaderMap,
    Path((pack, image)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    remove_image(state, headers, Path(("\u{1}".into(), pack, image))).await
}

async fn native_assign_missing(
    state: State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    assign_missing(state, headers, Path("\u{1}".into())).await
}

async fn check(auth: &AuthState, id: &str) -> Result<(), Error> {
    if id == "\u{1}" {
        return Ok(());
    }
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
    let rows: Vec<(String, String, String, i64)> = if id == "\u{1}" {
        query_as("SELECT p.id,p.name,p.source,COUNT(i.id) FROM avatar_packs p LEFT JOIN avatar_images i ON i.pack_id=p.id GROUP BY p.id,p.name,p.source ORDER BY p.name")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?
    } else {
        query_as("SELECT p.id,p.name,p.source,COUNT(i.id) FROM extension_avatar_packs p LEFT JOIN extension_avatar_images i ON i.pack_id=p.id WHERE p.extension_id=? GROUP BY p.id,p.name,p.source ORDER BY p.name")
            .bind(id)
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?
    };
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
        let mut stored_images = Vec::with_capacity(staged.len());
        for (name, bytes) in staged {
            let mime = crate::avatars::content_type(&bytes);
            let meta = auth
                .store
                .assets
                .put_bytes(&bytes, mime)
                .await
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "asset_store_error"))?;
            stored_images.push((name, meta.sha256));
        }
        // Serialize only the final authorization check/commit with revocations and uninstall.
        let _lock = MUTATIONS.lock().await;
        check(&auth, &id).await?;
        let mut tx = auth.store.pool().begin().await.map_err(db)?;
        let packs: i64 = query_scalar("SELECT (SELECT COUNT(*) FROM extension_avatar_packs) + (SELECT COUNT(*) FROM avatar_packs)")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        if packs >= 32 {
            return Err(err(StatusCode::CONFLICT, "avatar_pack_limit"));
        }
        let mut insert = query(if id == "\u{1}" { "INSERT INTO avatar_packs(id,name,source) VALUES(?,?,?)" } else { "INSERT INTO extension_avatar_packs(id,extension_id,name,source) VALUES(?,?,?,?)" }).bind(&pack);
        if id != "\u{1}" { insert = insert.bind(&id); }
        insert.bind(input.name.trim())
            .bind(format!(
                "https://github.com/{owner}/{repo}/tree/{sha}/{}",
                input.folder
            ))
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        for (name, sha256) in stored_images {
            query(if id == "\u{1}" { "INSERT INTO avatar_images(id,pack_id,name,sha256) VALUES(?,?,?,?)" } else { "INSERT INTO extension_avatar_images(id,pack_id,name,sha256) VALUES(?,?,?,?)" })
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&pack)
                .bind(name)
                .bind(&sha256)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        audit(&mut tx, &actor.user.id, if id == "\u{1}" { "native-avatar" } else { &id }, "avatars.pack.imported").await?;
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
    let mut stored_images = Vec::with_capacity(staged.len());
    for (name, bytes) in staged {
        let mime = crate::avatars::content_type(&bytes);
        let meta = auth
            .store
            .assets
            .put_bytes(&bytes, mime)
            .await
            .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "asset_store_error"))?;
        stored_images.push((name, meta.sha256));
    }
    let _lock = MUTATIONS.lock().await;
    check(&auth, &id).await?;
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    let count: i64 = query_scalar("SELECT (SELECT COUNT(*) FROM extension_avatar_packs) + (SELECT COUNT(*) FROM avatar_packs)")
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if count >= 32 {
        return Err(err(StatusCode::CONFLICT, "avatar_pack_limit"));
    }
    let pack = uuid::Uuid::new_v4().to_string();
    let count = stored_images.len();
    let mut insert = query(if id == "\u{1}" { "INSERT INTO avatar_packs(id,name,source) VALUES(?,?,'upload')" } else { "INSERT INTO extension_avatar_packs(id,extension_id,name,source) VALUES(?,?,?,'upload')" }).bind(&pack);
    if id != "\u{1}" { insert = insert.bind(&id); }
    insert.bind(name.trim())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    for (name, sha256) in stored_images {
        query(if id == "\u{1}" { "INSERT INTO avatar_images(id,pack_id,name,sha256) VALUES(?,?,?,?)" } else { "INSERT INTO extension_avatar_images(id,pack_id,name,sha256) VALUES(?,?,?,?)" })
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&pack)
            .bind(name)
            .bind(&sha256)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
    }
    audit(&mut tx, &actor.user.id, if id == "\u{1}" { "native-avatar" } else { &id }, "avatars.pack.uploaded").await?;
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
    if id == "\u{1}" {
        query("DELETE FROM avatar_packs WHERE id=?")
            .bind(pack)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    } else {
        query("DELETE FROM extension_avatar_packs WHERE id=? AND extension_id=?")
            .bind(pack)
            .bind(id)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    name: String,
}
async fn rename(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, pack)): Path<(String, String)>,
    Json(input): Json<Rename>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    if input.name.trim().is_empty()
        || input.name.len() > 100
        || input.name.chars().any(char::is_control)
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_avatar_pack"));
    }
    let _lock = MUTATIONS.lock().await;
    check(&auth, &id).await?;
    if id == "\u{1}" {
        query("UPDATE avatar_packs SET name=? WHERE id=?")
            .bind(input.name.trim())
            .bind(pack)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    } else {
        query("UPDATE extension_avatar_packs SET name=? WHERE id=? AND extension_id=?")
            .bind(input.name.trim())
            .bind(pack)
            .bind(id)
            .execute(auth.store.pool())
            .await
            .map_err(db)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn admin_images(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, pack)): Path<(String, String)>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, false).await?;
    check(&auth, &id).await?;
    let rows: Vec<(String, String)> = if id == "\u{1}" {
        query_as("SELECT i.id,i.name FROM avatar_images i JOIN avatar_packs p ON p.id=i.pack_id WHERE p.id=? ORDER BY i.name LIMIT 500")
            .bind(&pack)
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?
    } else {
        query_as("SELECT i.id,i.name FROM extension_avatar_images i JOIN extension_avatar_packs p ON p.id=i.pack_id WHERE p.id=? AND p.extension_id=? ORDER BY i.name LIMIT 500")
            .bind(&pack)
            .bind(&id)
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?
    };
    let base = if id == "\u{1}" {
        format!("/api/v1/admin/avatar-packs/{pack}/images")
    } else {
        format!("/api/v1/admin/extensions/{id}/avatar-packs/{pack}/images")
    };
    Ok(Json(
        json!({"images":rows.into_iter().map(|(image,name)|json!({"id":image,"name":name,"url":format!("{base}/{image}")})).collect::<Vec<_>>()}),
    ))
}
async fn admin_image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, pack, image)): Path<(String, String, String)>,
) -> Result<axum::response::Response, Error> {
    admin(&auth, &headers, false).await?;
    check(&auth, &id).await?;
    let query_str = if id == "\u{1}" {
        "SELECT i.sha256 FROM avatar_images i JOIN avatar_packs p ON p.id=i.pack_id WHERE p.id=? AND i.id=?"
    } else {
        "SELECT i.sha256 FROM extension_avatar_images i JOIN extension_avatar_packs p ON p.id=i.pack_id WHERE p.extension_id=? AND p.id=? AND i.id=?"
    };
    let sha256: String = if id == "\u{1}" {
        query_scalar(query_str)
            .bind(pack)
            .bind(image)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
    } else {
        query_scalar(query_str)
            .bind(id)
            .bind(pack)
            .bind(image)
            .fetch_optional(auth.store.pool())
            .await
            .map_err(db)?
    }
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "avatar_not_found"))?;
    auth.store
        .assets
        .response_after_authorization(&sha256, &headers)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "asset_store_error"))
}
async fn remove_image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((id, pack, image)): Path<(String, String, String)>,
) -> Result<StatusCode, Error> {
    admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    check(&auth, &id).await?;
    if id == "\u{1}" {
        query("DELETE FROM avatar_images WHERE id=? AND pack_id=? AND EXISTS(SELECT 1 FROM avatar_packs p WHERE p.id=?)").bind(image).bind(&pack).bind(pack).execute(auth.store.pool()).await.map_err(db)?;
    } else {
        query("DELETE FROM extension_avatar_images WHERE id=? AND pack_id=? AND EXISTS(SELECT 1 FROM extension_avatar_packs p WHERE p.id=? AND p.extension_id=?)").bind(image).bind(&pack).bind(pack).bind(id).execute(auth.store.pool()).await.map_err(db)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn assign_missing(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    admin(&auth, &headers, true).await?;
    let _lock = MUTATIONS.lock().await;
    check(&auth, &id).await?;
    let images: Vec<String> = if id == "\u{1}" {
        query_scalar(
            "SELECT i.sha256 FROM avatar_images i JOIN avatar_packs p ON p.id=i.pack_id LIMIT 500",
        )
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?
    } else {
        query_scalar("SELECT i.sha256 FROM extension_avatar_images i JOIN extension_avatar_packs p ON p.id=i.pack_id WHERE p.extension_id=? LIMIT 500")
            .bind(id)
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?
    };
    if images.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "avatar_gallery_empty"));
    }
    let mut tx = auth.store.pool().begin().await.map_err(db)?;
    let users: Vec<String> = query_scalar("SELECT id FROM users WHERE disabled_at IS NULL AND NOT EXISTS(SELECT 1 FROM user_avatars a WHERE a.user_id=users.id) LIMIT 10000").fetch_all(&mut *tx).await.map_err(db)?;
    let mut count = 0;
    for user in users {
        let index = (uuid::Uuid::new_v4().as_u128() % images.len() as u128) as usize;
        count += query(
            "INSERT INTO user_avatars(user_id,sha256) VALUES(?,?) ON CONFLICT(user_id) DO NOTHING",
        )
        .bind(user)
        .bind(&images[index])
        .execute(&mut *tx)
        .await
        .map_err(db)?
        .rows_affected();
    }
    tx.commit().await.map_err(db)?;
    Ok(Json(json!({"count":count})))
}
// Used by local and OIDC account creation; existing user photos are never overwritten.
pub(crate) async fn assign_new_user(auth: &AuthState, user: &str) -> Result<(), Error> {
    if crate::avatars::load_policy(auth)
        .await
        .map_err(|s| err(s, "avatar_policy_unavailable"))?
        .assign_on_create
    {
        let images: Vec<String> = query_scalar("SELECT sha256 FROM avatar_images LIMIT 16000")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
        if !images.is_empty() {
            let index = (uuid::Uuid::new_v4().as_u128() % images.len() as u128) as usize;
            query("INSERT INTO user_avatars(user_id,sha256) VALUES(?,?) ON CONFLICT(user_id) DO NOTHING").bind(user).bind(&images[index]).execute(auth.store.pool()).await.map_err(db)?;
        }
    }
    Ok(())
}
async fn allows_gravatar(auth: &AuthState) -> Result<bool, Error> {
    let policy = crate::avatars::load_policy(auth)
        .await
        .map_err(|s| err(s, "avatar_policy_unavailable"))?;
    Ok(policy.allow_custom && policy.allow_gravatar)
}
async fn gravatar_policy(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    Ok(Json(json!({"allowed":allows_gravatar(&auth).await?})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gravatar {
    email: String,
}
async fn gravatar(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Gravatar>,
) -> Result<StatusCode, Error> {
    let actor = auth
        .authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    auth.require_csrf(&headers, &actor)
        .map_err(|s| err(s, "csrf_required"))?;
    if !allows_gravatar(&auth).await? {
        return Err(err(StatusCode::FORBIDDEN, "gravatar_disabled"));
    }
    let email = input.email.trim().to_lowercase();
    if email.len() > 254 || !email.contains('@') || email.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_email"));
    }
    let _slot = IMPORTS
        .try_acquire()
        .map_err(|_| err(StatusCode::CONFLICT, "avatar_import_busy"))?;
    let digest = Sha256::digest(email.as_bytes());
    let hash = digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let data = repositories::fetch(
        &format!("https://www.gravatar.com/avatar/{hash}?s=256&d=404"),
        IMAGE_MAX,
    )
    .await?;
    let data = tokio::task::spawn_blocking(move || {
        let mime = match image::guess_format(&data) {
            Ok(image::ImageFormat::Png) => "image/png",
            Ok(image::ImageFormat::Jpeg) => "image/jpeg",
            Ok(image::ImageFormat::WebP) => "image/webp",
            _ => return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE),
        };
        crate::avatars::normalize(data.into(), mime)
    })
    .await
    .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "avatar_decode_failed"))?
    .map_err(|s| err(s, "invalid_avatar_image"))?;
    let _lock = MUTATIONS.lock().await;
    let _policy_guard = crate::avatars::POLICY_LOCK.read().await;
    if !allows_gravatar(&auth).await? {
        return Err(err(StatusCode::FORBIDDEN, "gravatar_disabled"));
    }
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    let meta = auth
        .store
        .assets
        .put_bytes(&data, "image/png")
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "asset_store_error"))?;
    query("INSERT INTO user_avatars(user_id,sha256) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET sha256=excluded.sha256").bind(actor.user.id).bind(&meta.sha256).execute(auth.store.pool()).await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn account_packs(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    let rows:Vec<(String,String,i64)>=query_as("SELECT a.id,a.name,COUNT(i.id) FROM avatar_packs a JOIN avatar_images i ON i.pack_id=a.id GROUP BY a.id,a.name UNION ALL SELECT a.id,a.name,COUNT(i.id) FROM extension_avatar_packs a JOIN extensions e ON e.id=a.extension_id JOIN extension_publishers p ON p.id=e.publisher_id JOIN extension_avatar_images i ON i.pack_id=a.id WHERE e.enabled=1 AND p.active=1 GROUP BY a.id,a.name ORDER BY 2").fetch_all(auth.store.pool()).await.map_err(db)?;
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
    let rows:Vec<(String,String)>=query_as("SELECT i.id,i.name FROM avatar_images i JOIN avatar_packs a ON a.id=i.pack_id WHERE a.id=? UNION ALL SELECT i.id,i.name FROM extension_avatar_images i JOIN extension_avatar_packs a ON a.id=i.pack_id JOIN extensions e ON e.id=a.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE a.id=? AND e.enabled=1 AND p.active=1 ORDER BY 2 LIMIT 500").bind(&pack).bind(&pack).fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(
        json!({"images":rows.into_iter().map(|(id,name)|json!({"id":id,"name":name,"url":format!("/api/v1/account/avatar/collections/{pack}/{id}")})).collect::<Vec<_>>()}),
    ))
}
async fn sha256(auth: &AuthState, pack: &str, image: &str) -> Result<String, Error> {
    query_scalar("SELECT i.sha256 FROM avatar_images i JOIN avatar_packs a ON a.id=i.pack_id WHERE a.id=? AND i.id=? UNION ALL SELECT i.sha256 FROM extension_avatar_images i JOIN extension_avatar_packs a ON a.id=i.pack_id JOIN extensions e ON e.id=a.extension_id JOIN extension_publishers p ON p.id=e.publisher_id WHERE a.id=? AND i.id=? AND e.enabled=1 AND p.active=1").bind(pack).bind(image).bind(pack).bind(image).fetch_optional(auth.store.pool()).await.map_err(db)?.ok_or_else(||err(StatusCode::NOT_FOUND,"avatar_not_found"))
}
async fn image(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path((pack, id)): Path<(String, String)>,
) -> Result<axum::response::Response, Error> {
    auth.authenticate(&headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    let sha = sha256(&auth, &pack, &id).await?;
    auth.store
        .assets
        .response_after_authorization(&sha, &headers)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "asset_store_error"))
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
    let sha = sha256(&auth, &pack, &id).await?;
    query("INSERT INTO user_avatars(user_id,sha256) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET sha256=excluded.sha256").bind(actor.user.id).bind(sha).execute(auth.store.pool()).await.map_err(db)?;
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
    async fn native_gallery_works_without_extensions() {
        use axum::{body::Body, http::Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let root = tempfile::tempdir().unwrap();
        let store = crate::identity::IdentityStore::open(&root.path().join("native.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap(
                "admin",
                zeroize::Zeroizing::new("disposable avatar password".into()),
            )
            .await
            .unwrap();
        let auth = AuthState::new(store.clone(), "http://localhost".into())
            .await
            .unwrap();
        let app = crate::auth::router(auth.clone())
            .merge(super::super::router(auth.clone()))
            .merge(crate::avatars::router(auth.clone()));
        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("origin", "http://localhost")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"login":"admin","password":"disposable avatar password"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let body = login.into_body().collect().await.unwrap().to_bytes();
        let csrf = serde_json::from_slice::<Value>(&body).unwrap()["csrfToken"]
            .as_str()
            .unwrap()
            .to_owned();
        let count: i64 = query_scalar("SELECT COUNT(*) FROM extensions")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let policy = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/avatar-policy")
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = policy.into_body().collect().await.unwrap().to_bytes();
        let policy: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            policy,
            json!({"allowCustom":true,"allowGravatar":false,"assignOnCreate":false})
        );
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8, 8)
            .write_to(&mut png, image::ImageFormat::Jpeg)
            .unwrap();
        let boundary = "fixture-boundary";
        let mut multipart=format!("--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nPack\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"images\"; filename=\"one.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n").into_bytes();
        multipart.extend(png.into_inner());
        multipart.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/avatar-packs/upload")
                    .header(header::COOKIE, &cookie)
                    .header(header::ORIGIN, "http://localhost")
                    .header("x-csrf-token", &csrf)
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(multipart))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let pack = serde_json::from_slice::<Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/account/avatar/collections/{pack}"))
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let image = serde_json::from_slice::<Value>(&body).unwrap()["images"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let image_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/account/avatar/collections/{pack}/{image}"))
                    .header(header::COOKIE, &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(image_response.status(), StatusCode::OK);
        assert_eq!(image_response.headers()[header::CONTENT_TYPE], "image/png");
        let image_bytes = image_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert!(image_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/account/avatar/collections/{pack}/{image}"))
                    .header(header::COOKIE, &cookie)
                    .header(header::ORIGIN, "http://localhost")
                    .header("x-csrf-token", &csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM user_avatars")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            1
        );
        let settings = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/admin/avatar-policy")
                    .header(header::COOKIE, &cookie)
                    .header(header::ORIGIN, "http://localhost")
                    .header("x-csrf-token", &csrf)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"allowCustom":false,"allowGravatar":true,"assignOnCreate":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(settings.status(), StatusCode::NO_CONTENT);
        assert!(!allows_gravatar(&auth).await.unwrap());
        query("INSERT INTO users(id,role) VALUES('new-user','user')")
            .execute(store.pool())
            .await
            .unwrap();
        assign_new_user(&auth, "new-user").await.unwrap();
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM user_avatars WHERE user_id='new-user'")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            1
        );
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
        let app = crate::auth::router(auth.clone()).merge(super::super::router(auth.clone()));
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
        let sha: String =
            query_scalar("SELECT sha256 FROM extension_avatar_images WHERE pack_id=?")
                .bind(pack)
                .fetch_one(store.pool())
                .await
                .unwrap();
        let (_, data) = store.assets.read_all(&sha).await.unwrap();
        assert_eq!(image::load_from_memory(&data).unwrap().width(), 256);
        // The administrator can curate a disabled extension's gallery.
        let image_list = format!("{endpoint}/{pack}/images");
        assert_eq!(
            call(&app, &image_list, "GET", "", "", json!(null)).await.0,
            StatusCode::UNAUTHORIZED
        );
        let (status, body, _) = call(&app, &image_list, "GET", cookie, "", json!(null)).await;
        assert_eq!(status, StatusCode::OK);
        let listed: Value = serde_json::from_slice(&body).unwrap();
        let gallery_url = listed["images"][0]["url"].as_str().unwrap();
        assert_eq!(
            call(&app, gallery_url, "GET", cookie, "", json!(null))
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                &format!("{endpoint}/{pack}"),
                "PUT",
                cookie,
                csrf,
                json!({"name":"Renamed gallery"})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(
                &app,
                &format!("{endpoint}/{pack}"),
                "PUT",
                cookie,
                csrf,
                json!({"name":""})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        query("INSERT INTO users(id,role) VALUES('missing-avatar','user')")
            .execute(store.pool())
            .await
            .unwrap();
        let assign = "/api/v1/admin/extensions/any-author-avatar/avatars/assign-missing";
        assert_eq!(
            call(&app, assign, "POST", cookie, "", json!(null)).await.0,
            StatusCode::FORBIDDEN
        );
        let (status, body, _) = call(&app, assign, "POST", cookie, csrf, json!(null)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["count"], 1);
        let (_, body, _) = call(&app, assign, "POST", cookie, csrf, json!(null)).await;
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["count"], 0);
        assert_eq!(
            call(
                &app,
                "/api/v1/account/avatar/gravatar",
                "POST",
                cookie,
                csrf,
                json!({"email":"fixture@example.invalid"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );

        assert_eq!(
            call(
                &app,
                "/api/v1/admin/extensions/any-author-avatar",
                "PUT",
                cookie,
                csrf,
                json!({"enabled":true})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(
                &app,
                "/api/v1/admin/extensions/any-author-avatar/settings",
                "PUT",
                cookie,
                csrf,
                json!({"allow_gravatar":true})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        query("INSERT INTO users(id,role) VALUES('automatic-avatar','user')")
            .execute(store.pool())
            .await
            .unwrap();
        assign_new_user(&auth, "automatic-avatar").await.unwrap();
        assert_eq!(
            query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM user_avatars WHERE user_id='automatic-avatar'"
            )
            .fetch_one(store.pool())
            .await
            .unwrap(),
            0
        );
        query("INSERT INTO avatar_policy(singleton,allow_custom,allow_gravatar,assign_on_create) VALUES(1,1,1,0)").execute(store.pool()).await.unwrap();
        let gravatar_url = format!(
            "https://www.gravatar.com/avatar/{:x}?s=256&d=404",
            Sha256::digest(b"fixture@example.invalid")
        );
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::load_from_memory(&data).unwrap().to_rgb8())
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        repositories::test_documents()
            .lock()
            .unwrap()
            .insert(gravatar_url.clone(), jpeg.into_inner());
        assert_eq!(
            call(
                &app,
                "/api/v1/account/avatar/gravatar",
                "POST",
                cookie,
                "",
                json!({"email":"fixture@example.invalid"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &app,
                "/api/v1/account/avatar/gravatar",
                "POST",
                cookie,
                csrf,
                json!({"email":" Fixture@Example.Invalid "})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(
                &app,
                "/api/v1/admin/extensions/any-author-avatar/settings",
                "PUT",
                cookie,
                csrf,
                json!({"allow_gravatar":true,"disallow_custom_avatars":true})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        query("UPDATE avatar_policy SET allow_custom=0 WHERE singleton=1")
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(
            call(
                &app,
                "/api/v1/account/avatar/gravatar",
                "POST",
                cookie,
                csrf,
                json!({"email":"fixture@example.invalid"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        repositories::test_documents()
            .lock()
            .unwrap()
            .remove(&gravatar_url);
        assert_eq!(
            call(&app, gallery_url, "DELETE", cookie, "", json!(null))
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(&app, gallery_url, "DELETE", cookie, csrf, json!(null))
                .await
                .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(&app, gallery_url, "GET", cookie, "", json!(null))
                .await
                .0,
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
