use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RepositoryInput {
    pub(super) name: String,
    pub(super) url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct InstallInput {
    pub(super) extension_id: String,
    pub(super) version: String,
    #[serde(default)]
    pub(super) upgrade: bool,
    #[serde(default)]
    pub(super) confirm_downgrade: bool,
    pub(super) expected_version: Option<String>,
    pub(super) expected_sha256: Option<String>,
    /// New permissions and HTTP hosts of this version the administrator approved.
    #[serde(default)]
    pub(super) approved_access: super::super::AccessApproval,
}

pub(in super::super) fn router() -> Router<AuthState> {
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
        .route(
            "/api/v1/admin/extensions/repositories/{id}/image/{extension}",
            get(image),
        )
        .route("/api/v1/admin/extensions/catalogue", get(catalogue))
}

pub(super) async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
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

pub(super) async fn add(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<RepositoryInput>,
) -> Result<(StatusCode, Json<Value>), Error> {
    admin(&auth, &headers, true).await?;
    let name = input.name.trim();
    let url = catalogue_url(input.url.trim())?.to_string();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_repository_name"));
    }
    let bytes = fetch(&url, MAX_MANIFEST).await?;
    parse_manifest(&bytes)?;
    admin(&auth, &headers, true).await?;
    let _lock = super::super::MUTATIONS.lock().await;
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

pub(super) async fn refresh(
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

pub(super) async fn remove(
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

/// Catalogue identifiers whose feature the Core implements natively: avatars
/// and poster rotation. The catalogue hides them and refuses to install them,
/// so a published package can never shadow or duplicate the native feature.
pub(super) const NATIVE_FEATURES: &[&str] = &["avatars", "poster-rotator"];

pub(super) fn is_native_feature(id: &str) -> bool {
    NATIVE_FEATURES.contains(&id)
}

pub(super) async fn catalogue(
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
        // One entry per published version, so earlier versions stay installable.
        for item in parse_manifest(bytes.as_bytes())? {
            if is_native_feature(&item.id) {
                continue;
            }
            entries.push(json!({
                "repositoryId": id,
                "repositoryName": name,
                "compatible": item_compatible(&item),
                "manifest": display_manifest(&item),
                "publisherId": item.publisher_id,
                "publicKey": item.public_key,
                "version": item.version,
                "overview": item.overview,
                "owner": item.owner,
                "hasImage": item.image_url.is_some(),
                "imageUrl": item.image_url,
                "sourceUrl": item.source_url,
                "timestamp": item.timestamp,
                "changelog": item.changelog,
            }));
        }
    }
    Ok(Json(json!({"entries":entries})))
}

pub(super) async fn install(
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
    let item = parse_manifest(snapshot.as_bytes())?
        .into_iter()
        .find(|item| item.id == input.extension_id && item.version == input.version)
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_catalogue_version"))?;
    if is_native_feature(&item.id) {
        return Err(err(StatusCode::BAD_REQUEST, "feature_is_native"));
    }
    if !item_compatible(&item) {
        return Err(err(StatusCode::BAD_REQUEST, "incompatible_server_version"));
    }
    let asset = fetch_asset(&item.source_url, MAX_DOWNLOAD).await?;
    if format!("{:x}", Sha256::digest(&asset)) != item.checksum {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_package_digest"));
    }
    let package: Package = serde_json::from_slice(&package_bytes(&asset)?)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_package"))?;
    // The signature (checked by the install paths) is the authority. The catalogue
    // entry must still describe this exact package, so an administrator never
    // installs something other than what was shown.
    let described = match &item.manifest {
        Some(manifest) => {
            serde_json::to_value(&package.manifest).ok() == serde_json::to_value(manifest).ok()
        }
        None => {
            package.manifest.id == item.id
                && package.manifest.version == item.version
                && (item.permissions.is_empty()
                    || same_set(&item.permissions, &package.manifest.permissions))
                && (item.http_hosts.is_empty()
                    || same_set(&item.http_hosts, &package.manifest.http_hosts))
        }
    };
    if package.publisher_id != item.publisher_id || !described {
        return Err(err(StatusCode::BAD_REQUEST, "catalogue_package_mismatch"));
    }
    if !compatible(&package.manifest) {
        return Err(err(StatusCode::BAD_REQUEST, "incompatible_server_version"));
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
    if input.confirm_downgrade {
        if !input.upgrade {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "extension_downgrade_confirmation_required",
            ));
        }
        let guard = super::super::DowngradeGuard {
            expected_version: input.expected_version.ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "extension_downgrade_confirmation_required",
                )
            })?,
            expected_sha256: input.expected_sha256.ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "extension_downgrade_confirmation_required",
                )
            })?,
        };
        super::super::change_version_with_key(
            auth,
            headers,
            input.extension_id,
            package,
            item.public_key,
            Some(guard),
            &input.approved_access,
        )
        .await
    } else if input.upgrade {
        super::super::upgrade_with_key(
            auth,
            headers,
            input.extension_id,
            package,
            item.public_key,
            &input.approved_access,
        )
        .await
    } else {
        super::super::install_with_key(auth, headers, package, item.public_key).await
    }
}

#[cfg(test)]
pub(in super::super) fn test_documents()
-> &'static std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>> {
    static DOCUMENTS: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    > = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    &DOCUMENTS
}
