//! Persistent library configuration. Filesystem policy is delegated to the local Node.
use crate::{
    auth::AuthState,
    identity::{IdentityStore, Role},
    node::validate_library_root,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{
    path::{Path as DiskPath, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

/// Upper bound on folders aggregated by one library (each is validated against its node roots).
pub(crate) const MAX_LIBRARY_FOLDERS: usize = 256;
const MAX_ROOT_SUBDIRS_PER_MOUNT: usize = 256;
const ROOTS_CACHE_TTL: Duration = Duration::from_secs(30);

type RootsCacheEntry = (Vec<PathBuf>, Instant, Vec<String>);

static ROOTS_CACHE: LazyLock<Mutex<Option<RootsCacheEntry>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Clone)]
struct LibraryState {
    auth: AuthState,
    root: PathBuf,
}
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Library {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub node_id: String,
    pub root: String,
    #[sqlx(skip)]
    pub folders: Vec<LibraryFolder>,
    #[sqlx(try_from = "crate::database::DbBool")]
    pub enabled: bool,
    #[sqlx(default)]
    pub excluded_patterns: String,
    #[sqlx(skip)]
    pub user_ids: Vec<String>,
    #[serde(skip)]
    pub options_json: String,
    #[sqlx(skip)]
    pub options: crate::library_options::LibraryOptions,
    #[sqlx(skip)]
    pub cover_urls: Vec<String>,
    #[sqlx(skip)]
    pub image_kinds: Vec<String>,
    #[sqlx(skip)]
    pub image_versions: std::collections::BTreeMap<String, String>,
    #[sqlx(skip)]
    pub works_count: i64,
    #[sqlx(skip)]
    pub episodes_count: i64,
}
#[derive(Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LibraryFolder {
    node_id: String,
    root: String,
}
fn default_enabled() -> bool {
    true
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibraryInput {
    name: String,
    kind: String,
    #[serde(default)]
    node_id: String,
    #[serde(default)]
    root: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    excluded_patterns: String,
    #[serde(default)]
    user_ids: Vec<String>,
    #[serde(default)]
    options: Option<crate::library_options::LibraryOptions>,
    #[serde(default)]
    folders: Option<Vec<LibraryFolder>>,
}

pub fn router(auth: AuthState, local_root: PathBuf) -> Router {
    Router::new()
        .route("/api/v1/libraries", get(accessible))
        .route(
            "/api/v1/libraries/{id}/images/{kind}",
            get(accessible_image),
        )
        .route("/api/v1/admin/libraries", get(list).post(create))
        .route(
            "/api/v1/admin/libraries/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route("/api/v1/admin/roots", get(roots))
        .route("/api/v1/admin/filesystem/browse", get(browse_filesystem))
        .route(
            "/api/v1/admin/filesystem/check",
            axum::routing::post(check_filesystem),
        )
        .route(
            "/api/v1/admin/libraries/{id}/images/generate",
            axum::routing::post(generate_images),
        )
        .route(
            "/api/v1/admin/libraries/{id}/images/{kind}",
            get(image).put(upload_image).delete(remove_image),
        )
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(LibraryState {
            auth,
            root: local_root,
        })
}
async fn accessible(
    State(state): State<LibraryState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let actor = state.auth.authenticate(&headers).await?;
    let rows: Vec<(String, String, String, String)> = crate::database::query_as(
        "SELECT l.id,l.name,l.kind,l.options_json \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id) \
            AND l.enabled=1 AND (n.status!='revoked' OR EXISTS(SELECT 1 FROM library_folders f \
            JOIN libraries child ON child.id=f.library_id JOIN media_nodes cn \
            ON cn.id=child.node_id WHERE f.parent_id=l.id AND f.active=1 AND cn.status!='revoked')) \
            AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
            JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
            JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
            AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
            OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id \
            AND p.user_id=?)) \
            ORDER BY l.name,l.id",
    )
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .fetch_all(&state.auth.store.pool)
    .await
    .map_err(database)?;
    let mut items = Vec::with_capacity(rows.len());
    for (id, name, kind, options_json) in rows {
        let anime = serde_json::from_str::<crate::library_options::LibraryOptions>(&options_json)
            .is_ok_and(|opts| opts.anime);
        let art: Option<(String, String)> = crate::database::query_as(
            "SELECT kind,revision FROM library_artwork WHERE library_id=? ORDER BY CASE kind WHEN 'backdrop' THEN 0 ELSE 1 END LIMIT 1",
        )
        .bind(&id)
        .fetch_optional(&state.auth.store.pool)
        .await
        .map_err(database)?;
        let image_url = art.map(|(art_kind, rev)| {
            format!("/api/v1/libraries/{id}/images/{art_kind}?width=320&v={rev}")
        });
        let cover_urls: Vec<String> = crate::database::query_scalar(
            "SELECT COALESCE((SELECT '/api/v1/works/' || w.id || '/images/' || wi.id || \
                '?width=320' FROM work_image_entries wi WHERE wi.work_id=w.id AND wi.kind='poster' \
                ORDER BY wi.position,wi.id LIMIT 1),(SELECT '/api/v1/works/' || w.id || '/poster' \
                FROM work_posters wp \
                WHERE wp.work_id=w.id),(SELECT '/api/v1/assets/' || i.id || '/image?width=320' \
                FROM inventory_images i WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 \
                AND i.root_snapshot=l.root ORDER BY i.priority,i.id LIMIT 1)) AS url \
                FROM inventory_works w \
                JOIN libraries l ON l.id=w.library_id \
                JOIN media_nodes n ON n.id=l.node_id \
                WHERE (l.id=? OR l.id IN (SELECT library_id FROM library_folders WHERE parent_id=? \
                AND active=1)) AND l.enabled=1 AND n.status='connected' \
                AND w.kind IN ('film','series') AND w.has_available_asset = 1 \
                AND (EXISTS(SELECT 1 FROM work_image_entries wi \
                WHERE wi.work_id=w.id AND wi.kind='poster') OR EXISTS(SELECT 1 FROM work_posters wp \
                WHERE wp.work_id=w.id) OR EXISTS(SELECT 1 FROM inventory_images i \
                WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 \
                AND i.root_snapshot=l.root)) \
                ORDER BY w.title_sort,w.id \
                LIMIT 4",
        )
        .bind(&id)
        .bind(&id)
        .fetch_all(&state.auth.store.pool)
        .await
        .map_err(database)?;
        items.push(serde_json::json!({
            "id": id,
            "name": name,
            "kind": kind,
            "anime": anime,
            "imageUrl": image_url,
            "coverUrls": cover_urls,
        }));
    }
    Ok(Json(serde_json::json!({ "libraries": items })))
}
async fn accessible_image(
    State(state): State<LibraryState>,
    Path((id, kind)): Path<(String, String)>,
    headers: HeaderMap,
    Query(options): Query<crate::artwork::PreviewOptions>,
) -> Result<Response, StatusCode> {
    let actor = state.auth.authenticate(&headers).await?;
    let allowed: i64 = crate::database::query_scalar(
        "SELECT COUNT(*) FROM libraries l \
         WHERE l.id=? AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id) \
         AND l.enabled=1 AND ((?='administrator' AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT \
         JOIN library_folders hxf ON hxf.library_id=hx.id AND hxf.active=1 LEFT \
         JOIN libraries hxp ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
         AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
         OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?))",
    )
    .bind(&id)
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .fetch_one(&state.auth.store.pool)
    .await
    .map_err(database)?;
    if allowed == 0 {
        return Err(StatusCode::FORBIDDEN);
    }
    serve_library_image(&state, &id, &kind, &headers, options).await
}
async fn administrator(
    state: &LibraryState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let identity = state.auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        state.auth.require_csrf(headers, &identity)?;
    }
    Ok(())
}
fn database(_: sqlx::Error) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Marker is transactional: restarts must never restore a disabled library or revoked grant.
pub async fn initialize(
    store: &IdentityStore,
    root: &DiskPath,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = validate_library_root(root, root).await?;
    let mut connection = store.pool.acquire().await?;
    let mut tx = crate::database::begin_write(&mut connection).await?;
    let initialized: i64 = crate::database::query_scalar(
        "SELECT CAST(EXISTS(SELECT 1 FROM library_initialization) AS INTEGER)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if initialized == 0 {
        for (id, name, kind) in [
            ("fixture-films", "Films de test", "films"),
            ("fixture-series", "Séries de test", "series"),
        ] {
            crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES (?,?,?,'local',?,1)")
                .bind(id).bind(name).bind(kind).bind(root.to_string_lossy().as_ref()).execute(&mut *tx).await?;
        }
        crate::database::query("INSERT INTO library_initialization(singleton) VALUES(1)")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A real local library starts empty: experimental fixture libraries are never seeded.
pub async fn initialize_local(store: &IdentityStore) -> Result<(), sqlx::Error> {
    crate::database::query(
        "INSERT INTO library_initialization(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING",
    )
    .execute(&store.pool)
    .await?;
    Ok(())
}
async fn load(store: &IdentityStore) -> Result<Vec<Library>, StatusCode> {
    // Same transaction gives a coherent configuration/grants snapshot.
    let mut tx = store.pool.begin().await.map_err(database)?;
    let mut libraries: Vec<Library> = crate::database::query_as(
        "SELECT id,name,kind,node_id,root,enabled,excluded_patterns,options_json FROM libraries WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=libraries.id) AND NOT EXISTS(SELECT 1 FROM deleted_libraries d WHERE d.library_id=libraries.id) ORDER BY name,id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(database)?;
    for library in &mut libraries {
        library.folders = vec![LibraryFolder {
            node_id: library.node_id.clone(),
            root: library.root.clone(),
        }];
        let additional = crate::database::query_as::<_, LibraryFolder>("SELECT l.node_id,l.root FROM library_folders f JOIN libraries l ON l.id=f.library_id WHERE f.parent_id=? AND f.active=1 ORDER BY l.id")
            .bind(&library.id).fetch_all(&mut *tx).await.map_err(database)?;
        library.folders.extend(additional);
        library.options = serde_json::from_str(&library.options_json)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        library.image_kinds = crate::database::query_scalar(
            "SELECT kind FROM library_artwork WHERE library_id=? ORDER BY kind",
        )
        .bind(&library.id)
        .fetch_all(&mut *tx)
        .await
        .map_err(database)?;
        let revisions: Vec<(String, String)> = crate::database::query_as(
            "SELECT kind,revision FROM library_artwork WHERE library_id=?",
        )
        .bind(&library.id)
        .fetch_all(&mut *tx)
        .await
        .map_err(database)?;
        library.image_versions = revisions.into_iter().collect();
        library.cover_urls = crate::database::query_scalar("SELECT COALESCE((SELECT '/api/v1/works/' || w.id || '/images/' || wi.id || \
                '?width=320' FROM work_image_entries wi WHERE wi.work_id=w.id AND wi.kind='poster' \
                ORDER BY wi.position,wi.id LIMIT 1),(SELECT '/api/v1/works/' || w.id || '/poster' \
                FROM work_posters wp \
                WHERE wp.work_id=w.id),(SELECT '/api/v1/assets/' || i.id || '/image?width=320' \
                FROM inventory_images i WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 \
                AND i.root_snapshot=l.root ORDER BY i.priority,i.id LIMIT 1)) AS url \
                FROM inventory_works w \
                JOIN libraries l ON l.id=w.library_id \
                JOIN media_nodes n ON n.id=l.node_id \
                WHERE (l.id=? OR l.id IN (SELECT library_id FROM library_folders WHERE parent_id=? \
                AND active=1)) AND l.enabled=1 AND n.status='connected' \
                AND w.kind IN ('film','series') AND w.has_available_asset = 1 \
                AND (EXISTS(SELECT 1 FROM work_image_entries wi \
                WHERE wi.work_id=w.id AND wi.kind='poster') OR EXISTS(SELECT 1 FROM work_posters wp \
                WHERE wp.work_id=w.id) OR EXISTS(SELECT 1 FROM inventory_images i \
                WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 \
                AND i.root_snapshot=l.root)) \
                ORDER BY w.title_sort,w.id \
                LIMIT 4")
            .bind(&library.id).bind(&library.id).fetch_all(&mut *tx).await.map_err(database)?;
        if library.kind == "music" {
            let (albums, tracks): (i64, i64) = crate::database::query_as(
                "SELECT COUNT(DISTINCT album), COUNT(*) FROM music_tracks \
                 WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND available=1",
            )
            .bind(&library.id)
            .bind(&library.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap_or((0, 0));
            library.works_count = albums;
            library.episodes_count = tracks;
        } else {
            let (works, episodes): (i64, i64) = crate::database::query_as(
                "SELECT COALESCE(SUM(CASE WHEN kind IN ('film','series') THEN 1 ELSE 0 END),0), \
                        COALESCE(SUM(CASE WHEN kind='episode' THEN 1 ELSE 0 END),0) \
                 FROM inventory_works \
                 WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) AND has_available_asset=1",
            )
            .bind(&library.id)
            .bind(&library.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap_or((0, 0));
            library.works_count = works;
            library.episodes_count = episodes;
        }
    }
    let grants = crate::database::query(
        "SELECT library_id,user_id FROM library_permissions ORDER BY user_id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(database)?;
    for grant in grants {
        let id: String = grant.get("library_id");
        if let Some(library) = libraries.iter_mut().find(|l| l.id == id) {
            library.user_ids.push(grant.get("user_id"));
        }
    }
    tx.commit().await.map_err(database)?;
    Ok(libraries)
}
async fn list(
    State(state): State<LibraryState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    administrator(&state, &headers, false).await?;
    Ok(Json(
        serde_json::json!({"libraries":load(&state.auth.store).await?}),
    ))
}
async fn discover_local_roots(allowed_roots: &[PathBuf]) -> Vec<String> {
    let mut local_roots = Vec::new();
    for allowed in allowed_roots {
        let Ok(local_root) = validate_library_root(allowed, allowed).await else {
            continue;
        };
        let root_str = local_root.to_string_lossy().into_owned();
        if !local_roots.contains(&root_str) {
            local_roots.push(root_str);
        }
        let Ok(mut entries) = tokio::fs::read_dir(&local_root).await else {
            continue;
        };
        let mut subdirs = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(ft) = entry.file_type().await else {
                continue;
            };
            if ft.is_file() {
                continue;
            }
            let is_dir = if ft.is_symlink() {
                entry.metadata().await.is_ok_and(|m| m.is_dir())
            } else {
                ft.is_dir()
            };
            if !is_dir {
                continue;
            }
            if subdirs >= MAX_ROOT_SUBDIRS_PER_MOUNT {
                break;
            }
            subdirs += 1;
            let path = entry.path();
            if let Ok(sub) = validate_library_root(&path, &local_root).await {
                let sub_str = sub.to_string_lossy().into_owned();
                if !local_roots.contains(&sub_str) {
                    local_roots.push(sub_str);
                }
            }
        }
    }
    local_roots
}
async fn roots(
    State(state): State<LibraryState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    administrator(&state, &headers, false).await?;
    let allowed_roots = crate::local_mounts::allowed_roots(&state.root).await;
    let cached_local_roots = {
        let guard = match ROOTS_CACHE.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((key, cached_at, roots)) = guard.as_ref()
            && *key == allowed_roots
            && cached_at.elapsed() < ROOTS_CACHE_TTL
        {
            Some(roots.clone())
        } else {
            None
        }
    };
    let local_roots = match cached_local_roots {
        Some(roots) => roots,
        None => {
            let computed = discover_local_roots(&allowed_roots).await;
            if !computed.is_empty() {
                let mut guard = match ROOTS_CACHE.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };
                *guard = Some((allowed_roots, Instant::now(), computed.clone()));
            }
            computed
        }
    };
    let mut all_roots = local_roots.clone();
    if let Ok(rows) = crate::database::query(
        "SELECT roots_json FROM media_nodes WHERE kind='remote' AND status != 'revoked' ORDER BY name, id",
    )
    .fetch_all(&state.auth.store.pool)
    .await
    {
        for r in rows {
            let roots_json: String = r.get("roots_json");
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&roots_json) {
                for p in parsed {
                    if !all_roots.contains(&p) {
                        all_roots.push(p);
                    }
                }
            }
        }
    }
    if all_roots.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(Json(
        serde_json::json!({"roots": all_roots, "localRoots": local_roots}),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrowseQuery {
    path: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryEntry {
    name: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowseResult {
    path: String,
    parent: Option<String>,
    directories: Vec<DirectoryEntry>,
}

async fn browse_filesystem(
    State(state): State<LibraryState>,
    headers: HeaderMap,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowseResult>, (StatusCode, Json<serde_json::Value>)> {
    administrator(&state, &headers, false)
        .await
        .map_err(|status| (status, Json(serde_json::json!({ "error": "Accès refusé" }))))?;

    let node_id = query.node_id.as_deref().unwrap_or("local");
    if node_id != "local" {
        let node_row: Option<(String, String)> = crate::database::query_as(
            "SELECT status, roots_json FROM media_nodes WHERE id=? AND kind='remote'",
        )
        .bind(node_id)
        .fetch_optional(&state.auth.store.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Erreur interne" })),
            )
        })?;

        let Some((status, roots_json)) = node_row else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Media Node introuvable" })),
            ));
        };
        if status == "revoked" {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Media Node révoqué" })),
            ));
        }

        let allowed_roots: Vec<String> = serde_json::from_str(&roots_json).unwrap_or_default();
        let entries = allowed_roots
            .into_iter()
            .map(|r| DirectoryEntry {
                name: r.clone(),
                path: r,
            })
            .collect();

        return Ok(Json(BrowseResult {
            path: query.path.unwrap_or_default(),
            parent: None,
            directories: entries,
        }));
    }

    let raw_path = query.path.unwrap_or_default().trim().to_string();

    #[cfg(windows)]
    {
        if raw_path.is_empty() {
            let mut drives = Vec::new();
            for c in b'A'..=b'Z' {
                let drive_str = format!("{}:\\", c as char);
                if DiskPath::new(&drive_str).exists() {
                    drives.push(DirectoryEntry {
                        name: format!("{}:", c as char),
                        path: drive_str,
                    });
                }
            }
            if drives.is_empty() {
                drives.push(DirectoryEntry {
                    name: "C:".to_string(),
                    path: "C:\\".to_string(),
                });
            }
            return Ok(Json(BrowseResult {
                path: "".to_string(),
                parent: None,
                directories: drives,
            }));
        }
    }

    let target_path_str = if raw_path.is_empty() {
        "/".to_string()
    } else {
        raw_path
    };

    let Some(target_path) = resolve_directory(&target_path_str).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "Le dossier spécifié n'existe pas ou n'est pas accessible." }),
            ),
        ));
    };
    let target_path_str = target_path.to_string_lossy().into_owned();
    let Ok(metadata) = tokio::fs::metadata(&target_path).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                serde_json::json!({ "error": "Le dossier spécifié n'existe pas ou n'est pas accessible." }),
            ),
        ));
    };

    if !metadata.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Le chemin spécifié n'est pas un dossier." })),
        ));
    }

    let Ok(mut entries) = tokio::fs::read_dir(&target_path).await else {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Impossible de lire le contenu de ce dossier." })),
        ));
    };

    let mut directories = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await
            && meta.is_dir()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.')
                && !name.starts_with('$')
                && !name.starts_with("@eaDir")
                && name != "#recycle"
            {
                let child_path = entry.path().to_string_lossy().into_owned();
                directories.push(DirectoryEntry {
                    name,
                    path: child_path,
                });
            }
        }
    }
    directories.sort_by_key(|a| a.name.to_lowercase());

    let parent = target_path
        .parent()
        .map(|p| {
            let s = p.to_string_lossy().into_owned();
            #[cfg(windows)]
            {
                if s.is_empty() {
                    return "".to_string();
                }
            }
            s
        })
        .filter(|p| p != &target_path_str);

    #[cfg(windows)]
    let parent = parent.or_else(|| {
        let trimmed = target_path_str.trim_end_matches(['/', '\\']);
        if trimmed.len() == 2 && trimmed.ends_with(':') {
            Some("".to_string())
        } else {
            None
        }
    });

    Ok(Json(BrowseResult {
        path: target_path_str,
        parent,
        directories,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckFilesystemInput {
    path: String,
    #[serde(default)]
    node_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckFilesystemResult {
    readable: bool,
    path: String,
    entries_count: usize,
    media_files_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn is_media_extension(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mkv"
            | "mp4"
            | "avi"
            | "mov"
            | "webm"
            | "m4v"
            | "ts"
            | "flac"
            | "mp3"
            | "ogg"
            | "opus"
            | "m4a"
            | "wav"
            | "aac"
    )
}

/// Tests whether a candidate library folder exists and is readable by the Core/Node.
async fn check_filesystem(
    State(state): State<LibraryState>,
    headers: HeaderMap,
    Json(input): Json<CheckFilesystemInput>,
) -> Result<Json<CheckFilesystemResult>, StatusCode> {
    administrator(&state, &headers, true).await?;
    let trimmed = input.path.trim().to_string();
    if trimmed.is_empty() {
        return Ok(Json(CheckFilesystemResult {
            readable: false,
            path: trimmed,
            entries_count: 0,
            media_files_count: 0,
            error: Some("Le chemin du dossier est requis.".to_string()),
        }));
    }
    let node_id = input.node_id.as_deref().unwrap_or("local");
    if node_id != "local" {
        return Ok(Json(CheckFilesystemResult {
            readable: false,
            path: trimmed,
            entries_count: 0,
            media_files_count: 0,
            error: Some(
                "Le test de lecture direct n'est disponible que pour le stockage local."
                    .to_string(),
            ),
        }));
    }
    let validated = match crate::local_mounts::validate(DiskPath::new(&trimmed), &state.root).await
    {
        Ok(path) => path,
        Err(_) => {
            return Ok(Json(CheckFilesystemResult {
                readable: false,
                path: trimmed,
                entries_count: 0,
                media_files_count: 0,
                error: Some(
                    "Ce dossier n'existe pas ou se trouve hors des racines autorisées.".to_string(),
                ),
            }));
        }
    };
    let mut entries = match tokio::fs::read_dir(&validated).await {
        Ok(entries) => entries,
        Err(_) => {
            return Ok(Json(CheckFilesystemResult {
                readable: false,
                path: validated.to_string_lossy().into_owned(),
                entries_count: 0,
                media_files_count: 0,
                error: Some("Permissions insuffisantes pour lire ce dossier.".to_string()),
            }));
        }
    };
    let mut entries_count = 0usize;
    let mut media_files_count = 0usize;
    while entries_count < 500 {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                entries_count += 1;
                let name = entry.file_name().to_string_lossy().into_owned();
                if is_media_extension(&name) {
                    media_files_count += 1;
                }
            }
            Ok(None) => break,
            Err(_) => {
                return Ok(Json(CheckFilesystemResult {
                    readable: false,
                    path: validated.to_string_lossy().into_owned(),
                    entries_count,
                    media_files_count,
                    error: Some("Erreur de lecture pendant l'inspection du dossier.".to_string()),
                }));
            }
        }
    }
    Ok(Json(CheckFilesystemResult {
        readable: true,
        path: validated.to_string_lossy().into_owned(),
        entries_count,
        media_files_count,
        error: None,
    }))
}

/// The directory `requested` names, rebuilt from directory listings alone: each
/// component must be an existing child of the one before it, `..` and relative
/// paths are refused. The path the disk is asked about never comes from the
/// request text itself, only from the names it selects.
async fn resolve_directory(requested: &str) -> Option<std::path::PathBuf> {
    use std::path::{Component, PathBuf};
    let mut current: Option<PathBuf> = None;
    for component in DiskPath::new(requested).components() {
        match component {
            Component::Prefix(prefix) => {
                // A Windows drive, among the letters only.
                let wanted = prefix.as_os_str().to_string_lossy().to_ascii_uppercase();
                current = (b'A'..=b'Z')
                    .map(|letter| format!("{}:", letter as char))
                    .find(|drive| *drive == wanted)
                    .map(|drive| PathBuf::from(format!("{drive}\\")));
                current.as_ref()?;
            }
            Component::RootDir => {
                if current.is_none() {
                    current = Some(PathBuf::from(std::path::MAIN_SEPARATOR_STR));
                }
            }
            Component::CurDir => {}
            Component::ParentDir => return None,
            Component::Normal(name) => {
                let directory = current.take()?;
                let mut entries = tokio::fs::read_dir(&directory).await.ok()?;
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let found = entry.file_name();
                    // Windows names are case-insensitive.
                    let same = if cfg!(windows) {
                        found.to_string_lossy().to_lowercase()
                            == name.to_string_lossy().to_lowercase()
                    } else {
                        found.as_os_str() == name
                    };
                    if same {
                        current = Some(entry.path());
                        break;
                    }
                }
                current.as_ref()?;
            }
        }
    }
    current
}

async fn create(
    State(state): State<LibraryState>,
    headers: HeaderMap,
    Json(input): Json<LibraryInput>,
) -> Result<(StatusCode, Json<Library>), StatusCode> {
    administrator(&state, &headers, true).await?;
    let library = save(&state, uuid::Uuid::new_v4().to_string(), input, false).await?;
    Ok((StatusCode::CREATED, Json(library)))
}
async fn image_target(state: &LibraryState, id: &str, kind: &str) -> Result<(), StatusCode> {
    if !matches!(kind, "poster" | "backdrop") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let count: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM libraries WHERE id=?")
        .bind(id)
        .fetch_one(&state.auth.store.pool)
        .await
        .map_err(database)?;
    if count == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(())
}
async fn image(
    State(state): State<LibraryState>,
    Path((id, kind)): Path<(String, String)>,
    headers: HeaderMap,
    Query(options): Query<crate::artwork::PreviewOptions>,
) -> Result<Response, StatusCode> {
    administrator(&state, &headers, false).await?;
    serve_library_image(&state, &id, &kind, &headers, options).await
}
async fn serve_library_image(
    state: &LibraryState,
    id: &str,
    kind: &str,
    headers: &HeaderMap,
    options: crate::artwork::PreviewOptions,
) -> Result<Response, StatusCode> {
    image_target(state, id, kind).await?;
    let width = options.width.unwrap_or(0);
    if ![0, 320, 640, 1280].contains(&width) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let sha256: String = crate::database::query_scalar(
        "SELECT COALESCE((SELECT sha256 FROM library_artwork_previews WHERE library_id=? AND kind=? AND width=?),sha256) FROM library_artwork WHERE library_id=? AND kind=?",
    )
    .bind(id)
    .bind(kind)
    .bind(i64::from(width))
    .bind(id)
    .bind(kind)
    .fetch_optional(&state.auth.store.pool)
    .await
    .map_err(database)?
    .ok_or(StatusCode::NOT_FOUND)?;
    let mut response = state
        .auth
        .store
        .assets
        .response_after_authorization(&sha256, headers)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}
async fn save_library_image_bytes(
    store: &IdentityStore,
    id: &str,
    kind: &str,
    bytes: Bytes,
    content_type: String,
    revision: String,
) -> Result<(), StatusCode> {
    let encoded = crate::artwork::prepare(bytes, content_type, kind.to_owned()).await?;
    let mut stored = Vec::new();
    let mut leases = Vec::new();
    for (width, mime, data) in encoded.variants {
        let meta = store
            .assets
            .put_bytes(&data, mime)
            .await
            .map_err(|_| StatusCode::INSUFFICIENT_STORAGE)?;
        leases.push(
            store
                .assets
                .lease(&meta.sha256)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        );
        stored.push((width, meta.sha256));
    }
    let mut tx = store.pool.begin().await.map_err(database)?;
    crate::database::query("INSERT INTO library_artwork(library_id,kind,sha256,revision) VALUES(?,?,?,?) ON CONFLICT(library_id,kind) DO UPDATE SET sha256=excluded.sha256,revision=excluded.revision")
        .bind(id).bind(kind).bind(&stored[0].1).bind(revision).execute(&mut *tx).await.map_err(database)?;
    for (width, sha) in stored.iter().filter(|v| v.0 != 0) {
        crate::database::query("INSERT INTO library_artwork_previews(library_id,kind,width,sha256) VALUES(?,?,?,?) ON CONFLICT(library_id,kind,width) DO UPDATE SET sha256=excluded.sha256")
            .bind(id).bind(kind).bind(width).bind(sha).execute(&mut *tx).await.map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    Ok(())
}
async fn upload_image(
    State(state): State<LibraryState>,
    Path((id, kind)): Path<(String, String)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<StatusCode, StatusCode> {
    administrator(&state, &headers, true).await?;
    image_target(&state, &id, &kind).await?;
    let content = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    save_library_image_bytes(
        &state.auth.store,
        &id,
        &kind,
        bytes,
        content,
        uuid::Uuid::new_v4().to_string(),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn generate_images(
    State(state): State<LibraryState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    administrator(&state, &headers, true).await?;
    generate_library_artwork(&state.auth.store, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Automatically generate `poster` and `backdrop` cover artwork for a library when it has none yet
/// (or when its existing auto-generated image had no work posters and work posters are now available).
/// Manual uploads are never overwritten.
pub(crate) async fn ensure_generated_library_artwork(
    store: &IdentityStore,
    id: &str,
) -> Result<(), StatusCode> {
    let target_id: String = crate::database::query_scalar(
        "SELECT COALESCE((SELECT parent_id FROM library_folders WHERE library_id=? AND active=1), ?)",
    )
    .bind(id)
    .bind(id)
    .fetch_one(&store.pool)
    .await
    .map_err(database)?;
    if target_id.starts_with("fixture-") {
        return Ok(());
    }
    let revisions: Vec<String> =
        crate::database::query_scalar("SELECT revision FROM library_artwork WHERE library_id=?")
            .bind(&target_id)
            .fetch_all(&store.pool)
            .await
            .map_err(database)?;
    // Never overwrite manual uploads (revisions that do not start with "auto-").
    if revisions.iter().any(|rev| !rev.starts_with("auto-")) {
        return Ok(());
    }
    let has_collage =
        revisions.len() >= 2 && revisions.iter().all(|rev| rev.starts_with("auto-collage-"));
    if has_collage {
        return Ok(());
    }
    generate_library_artwork(store, &target_id).await
}

pub(crate) async fn generate_library_artwork(
    store: &IdentityStore,
    id: &str,
) -> Result<(), StatusCode> {
    let row: Option<(String, String, String)> = crate::database::query_as(
        "SELECT name,kind,options_json FROM libraries WHERE id=? AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=libraries.id)",
    )
    .bind(id)
    .fetch_optional(&store.pool)
    .await
    .map_err(database)?;
    let (name, kind, options_json) = row.ok_or(StatusCode::NOT_FOUND)?;
    let is_anime = serde_json::from_str::<crate::library_options::LibraryOptions>(&options_json)
        .is_ok_and(|opts| opts.anime);
    let content_kind = if kind == "series" && is_anime {
        "anime"
    } else {
        kind.as_str()
    };

    let mut shas: Vec<String> = crate::database::query_scalar(
        "SELECT sha FROM (SELECT w.title_sort,w.id,COALESCE( \
            (SELECT d.sha256 FROM work_image_entries wi JOIN work_image_data d ON d.image_id=wi.id \
             WHERE wi.work_id=w.id AND wi.kind='poster' AND d.width IN (640,320,0) \
             ORDER BY CASE d.width WHEN 640 THEN 0 WHEN 320 THEN 1 ELSE 2 END, wi.pinned DESC, wi.position, wi.id LIMIT 1), \
            (SELECT wp.sha256 FROM work_posters wp WHERE wp.work_id=w.id) \
         ) AS sha \
         FROM inventory_works w \
         WHERE (w.library_id=? OR w.library_id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) \
         AND w.has_available_asset=1) sub \
         WHERE sha IS NOT NULL \
         ORDER BY title_sort,id \
         LIMIT 64",
    )
    .bind(id)
    .bind(id)
    .fetch_all(&store.pool)
    .await
    .map_err(database)?;

    let mut seed = (uuid::Uuid::new_v4().as_u128() as u64) | 1;
    let mut next_rand = || -> usize {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed as usize
    };
    for i in (1..shas.len()).rev() {
        shas.swap(i, next_rand() % (i + 1));
    }

    let mut posters = Vec::new();
    for sha in shas {
        if posters.len() >= 4 {
            break;
        }
        if let Ok((_, bytes)) = store.assets.read_all(&sha).await
            && let Ok(decoded) = image::load_from_memory(&bytes)
        {
            posters.push(decoded);
        }
    }
    if posters.len() < 4 {
        let mut local_paths: Vec<(String, String)> = crate::database::query_as(
            "SELECT i.root_snapshot,i.relative_path \
             FROM inventory_images i \
             JOIN inventory_works w ON w.id=i.work_id \
             JOIN libraries l ON l.id=w.library_id \
             WHERE (l.id=? OR l.id IN (SELECT library_id FROM library_folders WHERE parent_id=? AND active=1)) \
             AND l.node_id='local' AND i.role='poster' AND i.available=1 AND i.root_snapshot=l.root \
             ORDER BY w.title_sort,w.id,i.priority \
             LIMIT 64",
        )
        .bind(id)
        .bind(id)
        .fetch_all(&store.pool)
        .await
        .map_err(database)?;
        for i in (1..local_paths.len()).rev() {
            local_paths.swap(i, next_rand() % (i + 1));
        }
        for (root_snap, rel) in local_paths {
            if posters.len() >= 4 {
                break;
            }
            let full = std::path::Path::new(&root_snap).join(&rel);
            if let Ok(bytes) = tokio::fs::read(&full).await
                && let Ok(decoded) = image::load_from_memory(&bytes)
            {
                posters.push(decoded);
            }
        }
    }

    let rev_prefix = if posters.is_empty() {
        "auto-empty"
    } else {
        "auto-collage"
    };
    let id_owned = id.to_owned();
    let name_owned = name.clone();
    let kind_owned = content_kind.to_owned();
    let (backdrop_png, poster_png) = tokio::task::spawn_blocking(move || {
        let backdrop = render_library_card(640, 360, &id_owned, &name_owned, &kind_owned, &posters);
        let poster = render_library_card(400, 600, &id_owned, &name_owned, &kind_owned, &posters);
        (backdrop, poster)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let backdrop_png = backdrop_png.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let poster_png = poster_png.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    save_library_image_bytes(
        store,
        id,
        "backdrop",
        Bytes::from(backdrop_png),
        "image/png".into(),
        format!("{rev_prefix}-{}", uuid::Uuid::new_v4()),
    )
    .await?;
    save_library_image_bytes(
        store,
        id,
        "poster",
        Bytes::from(poster_png),
        "image/png".into(),
        format!("{rev_prefix}-{}", uuid::Uuid::new_v4()),
    )
    .await?;
    Ok(())
}

fn render_library_card(
    width: u32,
    height: u32,
    id: &str,
    name: &str,
    content_kind: &str,
    posters: &[image::DynamicImage],
) -> Option<Vec<u8>> {
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage, imageops};
    let hash = id
        .bytes()
        .chain(name.bytes())
        .fold(0x811c_9dc5_u32, |acc, b| {
            (acc ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
    let shift = ((hash % 37) as i32) - 18;
    let clamp = |v: i32| v.clamp(0, 255) as u8;
    let (bg_dark, bg_mid, accent) = match content_kind {
        "films" => (
            [clamp(22 + shift / 2), 14, 18],
            [clamp(142 + shift), clamp(46 + shift / 2), 34],
            [clamp(245 + shift / 3), clamp(158 + shift), 24],
        ),
        "series" => (
            [12, 18, clamp(34 + shift / 2)],
            [28, clamp(86 + shift), clamp(195 + shift)],
            [56, clamp(189 + shift / 2), 248],
        ),
        "anime" => (
            [clamp(24 + shift / 2), 12, 36],
            [clamp(138 + shift), 44, clamp(198 + shift / 2)],
            [244, clamp(114 + shift), 182],
        ),
        _ => (
            [10, clamp(26 + shift / 2), 24],
            [16, clamp(135 + shift), clamp(108 + shift)],
            [52, 211, clamp(153 + shift)],
        ),
    };

    let mut canvas = RgbaImage::new(width, height);
    let w_f = width.max(1) as f32;
    let h_f = height.max(1) as f32;
    for y in 0..height {
        let ty = y as f32 / h_f;
        for x in 0..width {
            let tx = x as f32 / w_f;
            let diag = (tx * 0.65 + ty * 0.35).clamp(0.0, 1.0);
            let dx = tx - 0.78;
            let dy = ty - 0.22;
            let glow = (1.0 - (dx * dx + dy * dy).sqrt() * 1.45).clamp(0.0, 1.0);
            let r = (f32::from(bg_dark[0]) * (1.0 - diag)
                + f32::from(bg_mid[0]) * diag
                + f32::from(accent[0]) * glow * 0.25)
                .clamp(0.0, 255.0) as u8;
            let g = (f32::from(bg_dark[1]) * (1.0 - diag)
                + f32::from(bg_mid[1]) * diag
                + f32::from(accent[1]) * glow * 0.25)
                .clamp(0.0, 255.0) as u8;
            let b = (f32::from(bg_dark[2]) * (1.0 - diag)
                + f32::from(bg_mid[2]) * diag
                + f32::from(accent[2]) * glow * 0.25)
                .clamp(0.0, 255.0) as u8;
            canvas.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    if !posters.is_empty() {
        let landscape = width >= height;
        let pad = 14u32;
        let gap = 8u32;
        let slots: Vec<(u32, u32, u32, u32)> = if landscape {
            let cols = posters.len().min(4) as u32;
            let avail_w = width.saturating_sub(pad * 2 + gap * cols.saturating_sub(1));
            let tile_w = (avail_w / cols).max(16);
            let tile_h = height.saturating_sub(pad * 2 + 6).max(16);
            (0..cols)
                .map(|i| (pad + i * (tile_w + gap), pad, tile_w, tile_h))
                .collect()
        } else if posters.len() == 1 {
            vec![(
                pad,
                pad,
                width.saturating_sub(pad * 2),
                height.saturating_sub(pad * 2 + 6),
            )]
        } else {
            let cols = 2u32;
            let rows = if posters.len() >= 3 { 2u32 } else { 1u32 };
            let tile_w = (width.saturating_sub(pad * 2 + gap)) / cols;
            let tile_h = (height.saturating_sub(pad * 2 + 6 + gap * rows.saturating_sub(1))) / rows;
            let mut list = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    list.push((
                        pad + c * (tile_w + gap),
                        pad + r * (tile_h + gap),
                        tile_w,
                        tile_h,
                    ));
                }
            }
            list
        };

        for (idx, (sx, sy, sw, sh)) in slots.into_iter().enumerate() {
            let src = &posters[idx % posters.len()];
            let (iw, ih) = (src.width().max(1), src.height().max(1));
            let target_ratio = sw as f32 / sh.max(1) as f32;
            let src_ratio = iw as f32 / ih as f32;
            let (cx, cy, cw, ch) = if src_ratio > target_ratio {
                let new_w = ((ih as f32 * target_ratio).round() as u32).clamp(1, iw);
                ((iw - new_w) / 2, 0, new_w, ih)
            } else {
                let new_h = ((iw as f32 / target_ratio).round() as u32).clamp(1, ih);
                (0, (ih - new_h) / 2, iw, new_h)
            };
            let cropped = src.crop_imm(cx, cy, cw, ch);
            let resized = imageops::resize(&cropped, sw, sh, imageops::FilterType::Triangle);
            let radius = 10i32;
            for ty in 0..sh {
                for tx in 0..sw {
                    let dx = if (tx as i32) < radius {
                        radius - (tx as i32)
                    } else if (tx as i32) >= (sw as i32) - radius {
                        (tx as i32) - ((sw as i32) - radius - 1)
                    } else {
                        0
                    };
                    let dy = if (ty as i32) < radius {
                        radius - (ty as i32)
                    } else if (ty as i32) >= (sh as i32) - radius {
                        (ty as i32) - ((sh as i32) - radius - 1)
                    } else {
                        0
                    };
                    if dx * dx + dy * dy > radius * radius {
                        continue;
                    }
                    let px = sx + tx;
                    let py = sy + ty;
                    if px < width && py < height {
                        let p = resized.get_pixel(tx, ty);
                        let v_fade = (py as f32 / h_f).powi(2) * 0.38;
                        let r = (f32::from(p[0]) * (1.0 - v_fade) + f32::from(bg_dark[0]) * v_fade)
                            as u8;
                        let g = (f32::from(p[1]) * (1.0 - v_fade) + f32::from(bg_dark[1]) * v_fade)
                            as u8;
                        let b = (f32::from(p[2]) * (1.0 - v_fade) + f32::from(bg_dark[2]) * v_fade)
                            as u8;
                        canvas.put_pixel(px, py, Rgba([r, g, b, 255]));
                    }
                }
            }
        }
    } else {
        // Stylized decorative cards & emblem when the library has no work posters yet.
        let cx = (width / 2) as i32;
        let cy = (height / 2) as i32;
        let card_w = (width.min(height) as i32 * 34 / 100).max(40);
        let card_h = (card_w * 135 / 100).max(56);
        for (offset_x, offset_y, alpha) in [
            (-card_w / 2, 8, 0.22_f32),
            (card_w / 2, 8, 0.22),
            (0, -4, 0.38),
        ] {
            let x0 = (cx + offset_x - card_w / 2).max(12) as u32;
            let y0 = (cy + offset_y - card_h / 2).max(12) as u32;
            let x1 = (cx + offset_x + card_w / 2).min(width as i32 - 12) as u32;
            let y1 = (cy + offset_y + card_h / 2).min(height as i32 - 16) as u32;
            for py in y0..y1 {
                for px in x0..x1 {
                    let border = px == x0 || px + 1 == x1 || py == y0 || py + 1 == y1;
                    let weight = if border {
                        (alpha + 0.25).min(0.85)
                    } else {
                        alpha
                    };
                    let cur = canvas.get_pixel(px, py);
                    let r =
                        (f32::from(cur[0]) * (1.0 - weight) + f32::from(accent[0]) * weight) as u8;
                    let g =
                        (f32::from(cur[1]) * (1.0 - weight) + f32::from(accent[1]) * weight) as u8;
                    let b =
                        (f32::from(cur[2]) * (1.0 - weight) + f32::from(accent[2]) * weight) as u8;
                    canvas.put_pixel(px, py, Rgba([r, g, b, 255]));
                }
            }
        }
    }

    // Signature accent bar along the bottom edge so each library's color identity pops.
    let bar_h = 6.min(height);
    for py in (height - bar_h)..height {
        for px in 0..width {
            let t = px as f32 / w_f;
            let r =
                (f32::from(accent[0]) * (1.0 - t * 0.25) + f32::from(bg_mid[0]) * (t * 0.25)) as u8;
            let g =
                (f32::from(accent[1]) * (1.0 - t * 0.25) + f32::from(bg_mid[1]) * (t * 0.25)) as u8;
            let b =
                (f32::from(accent[2]) * (1.0 - t * 0.25) + f32::from(bg_mid[2]) * (t * 0.25)) as u8;
            canvas.put_pixel(px, py, Rgba([r, g, b, 255]));
        }
    }

    let mut out = Vec::new();
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(out)
}
async fn remove_image(
    State(state): State<LibraryState>,
    Path((id, kind)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    administrator(&state, &headers, true).await?;
    image_target(&state, &id, &kind).await?;
    crate::database::query("DELETE FROM library_artwork WHERE library_id=? AND kind=?")
        .bind(id)
        .bind(kind)
        .execute(&state.auth.store.pool)
        .await
        .map_err(database)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn update(
    State(state): State<LibraryState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<LibraryInput>,
) -> Result<Json<Library>, StatusCode> {
    administrator(&state, &headers, true).await?;
    let saved = save(&state, id, input, true).await?;
    // Image settings may have changed (kinds, multi-image mode): fill on the next pass.
    crate::extensions::request_artwork_check();
    Ok(Json(saved))
}
/// Removes a library from Aether without touching the media files: the library
/// and its folders are disabled, every grant is revoked and it is hidden.
async fn remove(
    State(state): State<LibraryState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    administrator(&state, &headers, true).await?;
    delete_library(&state, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_library(state: &LibraryState, id: &str) -> Result<(), StatusCode> {
    let mut conn = state.auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    let visible: i64 = crate::database::query_scalar("SELECT COUNT(*) FROM libraries l WHERE l.id=? AND NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=l.id) AND NOT EXISTS(SELECT 1 FROM deleted_libraries d WHERE d.library_id=l.id)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
    if visible != 1 {
        return Err(StatusCode::NOT_FOUND);
    }
    let active: i64 = crate::database::query_scalar(
        "SELECT COUNT(*) FROM scan_jobs WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?)) AND status IN ('queued','running')",
    )
    .bind(id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(database)?;
    if active != 0 {
        return Err(StatusCode::CONFLICT);
    }
    for statement in [
        "UPDATE libraries SET enabled=0 WHERE id=? OR id IN (SELECT library_id FROM library_folders WHERE parent_id=?)",
        "UPDATE library_folders SET active=0 WHERE parent_id=? OR parent_id=?",
        "DELETE FROM library_permissions WHERE library_id=? OR library_id=?",
        "DELETE FROM user_group_libraries WHERE library_id=? OR library_id=?",
    ] {
        crate::database::query(statement)
            .bind(id)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    crate::database::query("INSERT INTO deleted_libraries(library_id,deleted_at) VALUES(?,?)")
        .bind(id)
        .bind(media_contracts::protocol::unix_now())
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    crate::database::query("UPDATE inventory_cache_generation SET gen=gen+1 WHERE id=1")
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    tx.commit().await.map_err(database)?;
    // Permission tables are fingerprinted by aggregates that a swap of rows can leave unchanged: invalidate here and on the other Cores (shared generation).
    crate::inventory::catalogue_changed(&state.auth.store).await;
    Ok(())
}
async fn validate_folder(
    state: &LibraryState,
    node_id: &str,
    root: &str,
) -> Result<String, StatusCode> {
    Ok(if node_id == "local" {
        let validated = crate::local_mounts::validate(DiskPath::new(root), &state.root)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        validated
            .to_str()
            .ok_or(StatusCode::BAD_REQUEST)?
            .to_owned()
    } else {
        let node_row: Option<(String, String)> = crate::database::query_as(
            "SELECT status, roots_json FROM media_nodes WHERE id=? AND kind='remote'",
        )
        .bind(node_id)
        .fetch_optional(&state.auth.store.pool)
        .await
        .map_err(database)?;
        let Some((status, roots_json)) = node_row else {
            return Err(StatusCode::BAD_REQUEST);
        };
        if status == "revoked" {
            return Err(StatusCode::BAD_REQUEST);
        }
        let allowed_roots: Vec<String> =
            serde_json::from_str(&roots_json).map_err(|_| StatusCode::BAD_REQUEST)?;
        let requested_root = root.trim();
        if requested_root.is_empty()
            || requested_root.contains("..")
            || requested_root.chars().any(char::is_control)
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let is_under_allowed = allowed_roots.iter().any(|allowed| {
            let a = allowed.trim_end_matches(['/', '\\']);
            let r = requested_root.trim_end_matches(['/', '\\']);
            r == a
                || r.strip_prefix(a)
                    .is_some_and(|rest| rest.starts_with('/') || rest.starts_with('\\'))
        });
        if !is_under_allowed {
            return Err(StatusCode::BAD_REQUEST);
        }
        requested_root.to_owned()
    })
}

async fn save(
    state: &LibraryState,
    id: String,
    mut input: LibraryInput,
    existing: bool,
) -> Result<Library, StatusCode> {
    let mut folders = if let Some(folders) = input.folders.take() {
        folders
    } else {
        let mut folders = vec![LibraryFolder {
            node_id: input.node_id.clone(),
            root: input.root.clone(),
        }];
        if existing {
            folders.extend(crate::database::query_as::<_,LibraryFolder>("SELECT l.node_id,l.root FROM libraries l JOIN library_folders f ON f.library_id=l.id WHERE f.parent_id=? AND f.active=1 ORDER BY l.id").bind(&id).fetch_all(&state.auth.store.pool).await.map_err(database)?);
        }
        folders
    };
    if folders.is_empty() || folders.len() > MAX_LIBRARY_FOLDERS {
        return Err(StatusCode::BAD_REQUEST);
    }
    for folder in &mut folders {
        folder.node_id = folder.node_id.trim().to_owned();
        folder.root = validate_folder(state, &folder.node_id, &folder.root).await?;
    }
    let unique: std::collections::BTreeSet<_> =
        folders.iter().map(|f| (&f.node_id, &f.root)).collect();
    if unique.len() != folders.len() {
        return Err(StatusCode::BAD_REQUEST);
    }
    input.node_id = folders[0].node_id.clone();
    input.root = folders[0].root.clone();
    input.name = input.name.trim().to_owned();
    input.node_id = input.node_id.trim().to_owned();
    if input.name.is_empty()
        || input.name.chars().count() > 100
        || input.name.chars().any(char::is_control)
        || !matches!(input.kind.as_str(), "films" | "series" | "music")
        || input.node_id.is_empty()
        || input.user_ids.len() > 1000
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input.options.as_ref().is_some_and(|v| !v.validate()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let root = input.root.clone();
    input.user_ids.sort();
    input.user_ids.dedup();
    // Read before the write transaction opens: the server-wide values a library in
    // `inherit` mode follows.
    let media_defaults = crate::server_settings::load(&state.auth.store)
        .await?
        .media_defaults();
    let mut conn = state.auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    let hidden: i64 =
        crate::database::query_scalar("SELECT (SELECT COUNT(*) FROM library_folders WHERE library_id=?)+(SELECT COUNT(*) FROM deleted_libraries WHERE library_id=?)")
            .bind(&id)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
    if hidden != 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    if existing {
        let active: i64 = crate::database::query_scalar(
            "SELECT COUNT(*) FROM scan_jobs WHERE (library_id=? OR library_id IN (SELECT library_id FROM library_folders WHERE parent_id=?)) AND status IN ('queued','running')",
        )
        .bind(&id)
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
        if active != 0 {
            let current: Option<(String, String, String)> =
                crate::database::query_as("SELECT kind,node_id,root FROM libraries WHERE id=?")
                    .bind(&id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(database)?;
            let Some((current_kind, current_node, current_root)) = current else {
                return Err(StatusCode::NOT_FOUND);
            };
            let mut current_folders = vec![(current_node, current_root)];
            let child_folders: Vec<(String, String)> = crate::database::query_as(
                "SELECT l.node_id,l.root FROM libraries l JOIN library_folders f ON f.library_id=l.id WHERE f.parent_id=? AND f.active=1 ORDER BY l.id",
            )
            .bind(&id)
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
            current_folders.extend(child_folders);
            let requested_folders: Vec<(String, String)> = folders
                .iter()
                .map(|f| (f.node_id.clone(), f.root.clone()))
                .collect();
            if current_kind != input.kind || current_folders != requested_folders {
                return Err(StatusCode::CONFLICT);
            }
        }
    }
    for user in &input.user_ids {
        let valid: i64 = crate::database::query_scalar(
            "SELECT CAST(EXISTS(SELECT 1 FROM users WHERE id=? AND role='user' AND disabled_at IS NULL) AS INTEGER)",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
        if valid == 0 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let previous: Option<String> =
        crate::database::query_scalar("SELECT options_json FROM libraries WHERE id=?")
            .bind(&id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database)?;
    let mut options = match input.options {
        Some(options) => options,
        None => serde_json::from_str(previous.as_deref().unwrap_or("{}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    options.resolve(&media_defaults, &input.kind);
    let options_json =
        serde_json::to_string(&options).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let previous_options: crate::library_options::LibraryOptions =
        serde_json::from_str(previous.as_deref().unwrap_or("{}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if existing {
        let result = crate::database::query(
            "UPDATE libraries SET name=?,kind=?,node_id=?,root=?,enabled=?,excluded_patterns=? WHERE id=?",
        )
        .bind(&input.name)
        .bind(&input.kind)
        .bind(&input.node_id)
        .bind(&root)
        .bind(i64::from(input.enabled))
        .bind(input.excluded_patterns.trim())
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
        if result.rows_affected() == 0 {
            return Err(StatusCode::NOT_FOUND);
        }
        crate::database::query("DELETE FROM library_permissions WHERE library_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    } else {
        crate::database::query(
            "INSERT INTO libraries(id,name,kind,node_id,root,enabled,excluded_patterns) VALUES(?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.kind)
        .bind(&input.node_id)
        .bind(&root)
        .bind(i64::from(input.enabled))
        .bind(input.excluded_patterns.trim())
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    }
    for user in &input.user_ids {
        crate::database::query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
            .bind(&id)
            .bind(user)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    crate::database::query("UPDATE libraries SET options_json=? WHERE id=?")
        .bind(&options_json)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    if options.automatic_refresh_interval_days != previous_options.automatic_refresh_interval_days {
        crate::database::query(
            "UPDATE libraries SET next_metadata_refresh_at=unixepoch()+? WHERE id=?",
        )
        .bind(i64::from(options.automatic_refresh_interval_days) * 86400)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    }
    // Child rows are physical scan scopes, never independently exposed or granted.
    crate::database::query("UPDATE libraries SET enabled=0 WHERE id IN (SELECT library_id FROM library_folders WHERE parent_id=?)").bind(&id).execute(&mut *tx).await.map_err(database)?;
    crate::database::query("UPDATE library_folders SET active=0 WHERE parent_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    for folder in folders.iter().skip(1) {
        let previous_id: Option<String> = crate::database::query_scalar("SELECT l.id FROM libraries l JOIN library_folders f ON f.library_id=l.id WHERE f.parent_id=? AND l.node_id=? AND l.root=?")
            .bind(&id).bind(&folder.node_id).bind(&folder.root).fetch_optional(&mut *tx).await.map_err(database)?;
        let folder_id = previous_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        crate::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled,excluded_patterns,options_json) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE \
                SET name=excluded.name,kind=excluded.kind,enabled=excluded.enabled,excluded_patterns=excluded.excluded_patterns,options_json=excluded.options_json")
            .bind(&folder_id).bind(&input.name).bind(&input.kind).bind(&folder.node_id).bind(&folder.root).bind(i64::from(input.enabled)).bind(input.excluded_patterns.trim()).bind(&options_json).execute(&mut *tx).await.map_err(database)?;
        crate::database::query("INSERT INTO library_folders(library_id,parent_id,active) VALUES(?,?,1) ON CONFLICT(library_id) DO UPDATE SET active=1")
            .bind(&folder_id).bind(&id).execute(&mut *tx).await.map_err(database)?;
    }
    if previous.as_deref() != Some(options_json.as_str()) {
        crate::automation::schedule_library(
            &mut tx,
            &id,
            "metadata.missing",
            &uuid::Uuid::new_v4().to_string(),
            None,
            media_contracts::protocol::unix_now(),
        )
        .await
        .map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    // Permission tables are fingerprinted by aggregates that a swap of rows can leave unchanged: invalidate here and on the other Cores (shared generation).
    crate::inventory::catalogue_changed(&state.auth.store).await;
    Ok(Library {
        id,
        name: input.name,
        kind: input.kind,
        node_id: input.node_id,
        root,
        folders,
        enabled: input.enabled,
        excluded_patterns: input.excluded_patterns,
        user_ids: input.user_ids,
        options_json,
        options,
        cover_urls: vec![],
        image_kinds: vec![],
        image_versions: Default::default(),
        works_count: 0,
        episodes_count: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn setup() -> (tempfile::TempDir, LibraryState, String) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("media");
        std::fs::create_dir(&root).unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        initialize(&store, &root).await.unwrap();
        let user = "synthetic-user".to_owned();
        crate::database::query("INSERT INTO users(id,role) VALUES(?,'user')")
            .bind(&user)
            .execute(&store.pool)
            .await
            .unwrap();
        let auth = AuthState::new(store, "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        (temp, LibraryState { auth, root }, user)
    }
    fn input(state: &LibraryState, users: Vec<String>) -> LibraryInput {
        LibraryInput {
            name: "Configured library".into(),
            kind: "films".into(),
            node_id: "local".into(),
            root: state.root.to_string_lossy().into_owned(),
            enabled: true,
            excluded_patterns: String::new(),
            user_ids: users,
            options: None,
            folders: None,
        }
    }
    #[tokio::test]
    async fn deleted_library_is_hidden_disabled_and_ungranted() {
        let (_temp, state, user) = setup().await;
        save(
            &state,
            "gone".into(),
            input(&state, vec![user.clone()]),
            false,
        )
        .await
        .unwrap();
        let store = &state.auth.store;
        delete_library(&state, "gone").await.unwrap();
        assert_eq!(
            delete_library(&state, "gone").await.unwrap_err(),
            StatusCode::NOT_FOUND
        );
        let grants: i64 = crate::database::query_scalar(
            "SELECT COUNT(*) FROM library_permissions WHERE library_id='gone'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(grants, 0);
        assert!(load(store).await.unwrap().iter().all(|l| l.id != "gone"));
        assert!(matches!(
            save(&state, "gone".into(), input(&state, vec![]), true).await,
            Err(StatusCode::NOT_FOUND)
        ));
    }
    #[tokio::test]
    async fn multiple_nodes_folders_permissions_and_removal_are_persistent() {
        let (temp, state, user) = setup().await;
        crate::database::query("INSERT INTO media_nodes(id,name,kind,roots_json) VALUES('remote','NAS','remote','[\"/media\"]')").execute(&state.auth.store.pool).await.unwrap();
        let local = LibraryFolder {
            node_id: "local".into(),
            root: state.root.to_string_lossy().into(),
        };
        let remote = LibraryFolder {
            node_id: "remote".into(),
            root: "/media/uhd".into(),
        };
        let mut configured = input(&state, vec![user.clone()]);
        configured.folders = Some(vec![local.clone(), remote.clone()]);
        save(&state, "uhd".into(), configured, false).await.unwrap();
        let child: String = crate::database::query_scalar(
            "SELECT library_id FROM library_folders WHERE parent_id='uhd' AND active=1",
        )
        .fetch_one(&state.auth.store.pool)
        .await
        .unwrap();
        let grants: i64 = crate::database::query_scalar(
            "SELECT COUNT(*) FROM effective_library_permissions WHERE library_id=? AND user_id=?",
        )
        .bind(&child)
        .bind(&user)
        .fetch_one(&state.auth.store.pool)
        .await
        .unwrap();
        assert_eq!(grants, 1);
        let mut invalid = input(&state, vec![]);
        invalid.folders = Some(vec![
            local.clone(),
            LibraryFolder {
                node_id: "remote".into(),
                root: "/outside".into(),
            },
        ]);
        assert_eq!(
            save(&state, "uhd".into(), invalid, true).await.err(),
            Some(StatusCode::BAD_REQUEST)
        );
        let mut duplicate = input(&state, vec![]);
        duplicate.folders = Some(vec![local.clone(), local.clone()]);
        assert_eq!(
            save(&state, "uhd".into(), duplicate, true).await.err(),
            Some(StatusCode::BAD_REQUEST)
        );
        // Legacy updates keep the configured additional folders.
        save(
            &state,
            "uhd".into(),
            input(&state, vec![user.clone()]),
            true,
        )
        .await
        .unwrap();
        let listed = load(&state.auth.store).await.unwrap();
        assert!(!listed.iter().any(|l| l.id == child));
        assert_eq!(
            listed.iter().find(|l| l.id == "uhd").unwrap().folders.len(),
            2
        );
        // Child scan scopes cannot be modified as independent libraries.
        assert_eq!(
            save(&state, child.clone(), input(&state, vec![]), true)
                .await
                .err(),
            Some(StatusCode::NOT_FOUND)
        );
        let mut removed = input(&state, vec![user.clone()]);
        removed.folders = Some(vec![local]);
        save(&state, "uhd".into(), removed, true).await.unwrap();
        let enabled: i64 =
            crate::database::query_scalar("SELECT enabled FROM libraries WHERE id=?")
                .bind(&child)
                .fetch_one(&state.auth.store.pool)
                .await
                .unwrap();
        assert_eq!(enabled, 0);
        let grants: i64 = crate::database::query_scalar(
            "SELECT COUNT(*) FROM effective_library_permissions WHERE library_id=?",
        )
        .bind(&child)
        .fetch_one(&state.auth.store.pool)
        .await
        .unwrap();
        assert_eq!(grants, 0);
        state.auth.store.close().await;
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        assert_eq!(
            load(&store)
                .await
                .unwrap()
                .iter()
                .find(|l| l.id == "uhd")
                .unwrap()
                .folders
                .len(),
            1
        );
    }
    #[tokio::test]
    async fn options_validation_legacy_update_and_restart() {
        let (temp, state, _) = setup().await;
        let mut configured = input(&state, vec![]);
        configured.options = Some(crate::library_options::LibraryOptions {
            read_local_nfo: false,
            fetch_logo: false,
            automatic_refresh_interval_days: 30,
            prefer_embedded_titles: true,
            embedded_subtitle_policy: crate::library_options::EmbeddedSubtitlePolicy::AllowText,
            metadata_providers: vec![crate::library_options::ProviderPreference {
                id: "tmdb".into(),
                enabled: true,
            }],
            subtitle_providers: vec![crate::library_options::ProviderPreference {
                id: "opensubtitles".into(),
                enabled: true,
            }],
            trickplay_enabled: true,
            trickplay_width: 640,
            subtitle_languages: vec!["fr-CA".into(), "en".into()],
            intro_detection_enabled: true,
            subtitle_download_enabled: true,
            ..Default::default()
        });
        save(&state, "fixture-films".into(), configured, true)
            .await
            .unwrap();
        // Old clients omitting options must preserve the new configuration.
        save(&state, "fixture-films".into(), input(&state, vec![]), true)
            .await
            .unwrap();
        let next: i64 = crate::database::query_scalar(
            "SELECT next_metadata_refresh_at FROM libraries WHERE id='fixture-films'",
        )
        .fetch_one(&state.auth.store.pool)
        .await
        .unwrap();
        assert!(next > 0);
        let mut invalid = input(&state, vec![]);
        invalid.options = Some(crate::library_options::LibraryOptions {
            metadata_country_code: "fra".into(),
            ..Default::default()
        });
        assert_eq!(
            save(&state, "fixture-films".into(), invalid, true)
                .await
                .err(),
            Some(StatusCode::BAD_REQUEST)
        );
        state.auth.store.close().await;
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        let films = load(&store)
            .await
            .unwrap()
            .into_iter()
            .find(|l| l.id == "fixture-films")
            .unwrap();
        assert!(!films.options.read_local_nfo);
        assert!(!films.options.fetch_logo);
        assert_eq!(films.options.automatic_refresh_interval_days, 30);
        assert!(films.options.prefer_embedded_titles);
        assert!(films.options.trickplay_enabled);
        assert!(films.options.intro_detection_enabled);
        assert!(films.options.subtitle_download_enabled);
        assert_eq!(films.options.trickplay_width, 640);
        assert_eq!(films.options.subtitle_languages, ["fr-CA", "en"]);
        assert_eq!(films.options.metadata_providers[0].id, "tmdb");
        assert_eq!(films.options.subtitle_providers[0].id, "opensubtitles");
        assert_eq!(
            films.options.embedded_subtitle_policy,
            crate::library_options::EmbeddedSubtitlePolicy::AllowText
        );
        // Unknown legacy names remain rejected; reserved settings use explicit names.
        assert!(
            serde_json::from_value::<crate::library_options::LibraryOptions>(
                serde_json::json!({"saveLocalNfo":true})
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn persistence_revocation_and_restart_do_not_restore_defaults() {
        let (temp, state, user) = setup().await;
        let result = save(
            &state,
            "fixture-films".into(),
            input(&state, vec![user.clone()]),
            true,
        )
        .await
        .unwrap();
        assert_eq!(result.user_ids, vec![user.clone()]);
        let mut replacement = input(&state, vec![]);
        replacement.enabled = false;
        save(&state, "fixture-films".into(), replacement, true)
            .await
            .unwrap();
        state.auth.store.close().await;
        let reopened = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        initialize(&reopened, &state.root).await.unwrap();
        let libraries = load(&reopened).await.unwrap();
        assert_eq!(libraries.len(), 2);
        let films = libraries.iter().find(|l| l.id == "fixture-films").unwrap();
        assert!(!films.enabled);
        assert!(films.user_ids.is_empty());
        assert_eq!(films.name, "Configured library");
    }
    #[tokio::test]
    async fn invalid_grants_roll_back_configuration_and_existing_grants() {
        let (_temp, state, user) = setup().await;
        save(
            &state,
            "fixture-films".into(),
            input(&state, vec![user.clone()]),
            true,
        )
        .await
        .unwrap();
        let mut invalid = input(&state, vec!["unknown-user".into()]);
        invalid.enabled = false;
        assert_eq!(
            save(&state, "fixture-films".into(), invalid, true)
                .await
                .err(),
            Some(StatusCode::BAD_REQUEST)
        );
        let films = load(&state.auth.store)
            .await
            .unwrap()
            .into_iter()
            .find(|l| l.id == "fixture-films")
            .unwrap();
        assert!(films.enabled);
        assert_eq!(films.user_ids, vec![user]);
        let mut invalid = input(&state, vec![]);
        invalid.node_id = "remote".into();
        assert_eq!(
            save(&state, "new".into(), invalid, false).await.err(),
            Some(StatusCode::BAD_REQUEST)
        );
        let mut invalid = input(&state, vec![]);
        invalid.root = state.root.parent().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            save(&state, "new".into(), invalid, false).await.err(),
            Some(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            save(&state, "missing".into(), input(&state, vec![]), true)
                .await
                .err(),
            Some(StatusCode::NOT_FOUND)
        );
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, header},
        response::Response,
    };
    use http_body_util::BodyExt;
    use sha2::{Digest, Sha256};
    use tower::ServiceExt;

    async fn request(
        app: &Router,
        method: &str,
        path: &str,
        cookie: Option<&str>,
        csrf: Option<&str>,
        body: &serde_json::Value,
    ) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, "http://127.0.0.1:3000");
        if let Some(cookie) = cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = csrf {
            builder = builder.header("x-csrf-token", csrf);
        }
        app.clone()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
    async fn json(response: Response) -> serde_json::Value {
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }
    #[tokio::test]
    async fn library_http_roles_csrf_crud_and_grants() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("media");
        std::fs::create_dir(&root).unwrap();
        let store = IdentityStore::open(&temp.path().join("db.sqlite"))
            .await
            .unwrap();
        initialize(&store, &root).await.unwrap();
        let mut cookies = Vec::new();
        for (id, role, raw) in [
            ("admin", "administrator", "a".repeat(64)),
            ("reader", "user", "b".repeat(64)),
        ] {
            crate::database::query("INSERT INTO users(id,role) VALUES(?,?)")
                .bind(id)
                .bind(role)
                .execute(&store.pool)
                .await
                .unwrap();
            crate::database::query("INSERT INTO local_credentials(user_id,login,password_hash) VALUES(?,?,'unused-test-hash')").bind(id).bind(id).execute(&store.pool).await.unwrap();
            let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
            crate::database::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)").bind(hash).bind(id).execute(&store.pool).await.unwrap();
            cookies.push(format!("aether_session={raw}"));
        }
        let csrf = format!(
            "{:x}",
            Sha256::digest(format!("csrf:{}", "a".repeat(64)).as_bytes())
        );
        let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
            .await
            .unwrap();
        let app = router(auth.clone(), root.clone()).merge(crate::admin_users::router(auth));
        let null = serde_json::Value::Null;
        for path in [
            "/api/v1/admin/libraries",
            "/api/v1/admin/users",
            "/api/v1/admin/roots",
            "/api/v1/admin/filesystem/browse",
        ] {
            assert_eq!(
                request(&app, "GET", path, None, None, &null).await.status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                request(&app, "GET", path, Some(&cookies[1]), None, &null)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                request(&app, "GET", path, Some(&cookies[0]), None, &null)
                    .await
                    .status(),
                StatusCode::OK
            );
        }
        let mut body = serde_json::json!({"name":"Test library","kind":"films","nodeId":"local","root":root,"enabled":true,"userIds":["reader"]});
        for (method, path) in [
            ("POST", "/api/v1/admin/libraries"),
            ("PUT", "/api/v1/admin/libraries/fixture-films"),
        ] {
            assert_eq!(
                request(&app, method, path, None, None, &body)
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                request(&app, method, path, Some(&cookies[1]), Some(&csrf), &body)
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            for token in [None, Some("wrong")] {
                assert_eq!(
                    request(&app, method, path, Some(&cookies[0]), token, &body)
                        .await
                        .status(),
                    StatusCode::FORBIDDEN
                );
            }
        }
        let created = request(
            &app,
            "POST",
            "/api/v1/admin/libraries",
            Some(&cookies[0]),
            Some(&csrf),
            &body,
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = json(created).await;
        assert_eq!(created["userIds"], serde_json::json!(["reader"]));
        let id = created["id"].as_str().unwrap();
        let path = format!("/api/v1/admin/libraries/{id}");
        body["enabled"] = serde_json::json!(false);
        body["userIds"] = serde_json::json!([]);
        let updated = request(&app, "PUT", &path, Some(&cookies[0]), Some(&csrf), &body).await;
        assert_eq!(updated.status(), StatusCode::OK);
        let updated = json(updated).await;
        assert_eq!(updated["enabled"], false);
        assert_eq!(updated["userIds"], serde_json::json!([]));
        let grants: i64 = crate::database::query_scalar(
            "SELECT count(*) FROM library_permissions WHERE library_id=?",
        )
        .bind(id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(grants, 0);
        assert_eq!(
            request(
                &app,
                "PUT",
                "/api/v1/admin/libraries/missing",
                Some(&cookies[0]),
                Some(&csrf),
                &body
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        body["userIds"] = serde_json::json!(["nonexistent"]);
        assert_eq!(
            request(&app, "PUT", &path, Some(&cookies[0]), Some(&csrf), &body)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/v1/admin/libraries",
                Some(&cookies[0]),
                Some(&csrf),
                &body
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        let browse_resp = request(
            &app,
            "GET",
            &format!(
                "/api/v1/admin/filesystem/browse?path={}",
                root.to_string_lossy()
            ),
            Some(&cookies[0]),
            None,
            &null,
        )
        .await;
        assert_eq!(browse_resp.status(), StatusCode::OK);
        let browse_json = json(browse_resp).await;
        assert!(browse_json["directories"].is_array());

        let invalid_browse = request(
            &app,
            "GET",
            "/api/v1/admin/filesystem/browse?path=/nonexistent-random-dir-404",
            Some(&cookies[0]),
            None,
            &null,
        )
        .await;
        assert_eq!(invalid_browse.status(), StatusCode::NOT_FOUND);
    }
}

#[cfg(test)]
mod browse_tests {
    #[tokio::test]
    async fn directories_resolve_from_listings_only() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("Films").join("Action");
        std::fs::create_dir_all(&nested).unwrap();
        let root = temp.path().canonicalize().unwrap();
        let text = |path: &std::path::Path| path.to_string_lossy().into_owned();

        let found = super::resolve_directory(&text(&root.join("Films").join("Action")))
            .await
            .unwrap();
        assert_eq!(found, root.join("Films").join("Action"));
        assert!(
            super::resolve_directory(&text(&root.join("Films").join("..").join("Films")))
                .await
                .is_none()
        );
        assert!(
            super::resolve_directory(&text(&root.join("Missing")))
                .await
                .is_none()
        );
        assert!(super::resolve_directory("Films/Action").await.is_none());
    }

    #[tokio::test]
    async fn filesystem_check_verifies_readability_and_media_count() {
        use axum::{
            body::Body,
            http::{Request, StatusCode},
        };
        use http_body_util::BodyExt;
        use serde_json::json;
        use tower::ServiceExt;

        let temp = tempfile::tempdir().unwrap();
        let media_dir = temp.path().join("Films");
        std::fs::create_dir_all(&media_dir).unwrap();
        std::fs::write(media_dir.join("movie.mkv"), b"fake").unwrap();
        std::fs::write(media_dir.join("notes.txt"), b"note").unwrap();

        let store = crate::identity::IdentityStore::open(&temp.path().join("check.sqlite"))
            .await
            .unwrap();
        store
            .bootstrap(
                "admin",
                zeroize::Zeroizing::new("very-long-admin-password".into()),
            )
            .await
            .unwrap();
        let origin = "http://127.0.0.1:3000";
        let auth = crate::auth::AuthState::new(store, origin.into())
            .await
            .unwrap();
        let app =
            crate::auth::router(auth.clone()).merge(super::router(auth, temp.path().to_path_buf()));

        // Login as admin to get session + csrf
        let login_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/login")
                    .method("POST")
                    .header("origin", origin)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"login":"admin","password":"very-long-admin-password"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login_res.status(), StatusCode::OK);
        let cookie = login_res.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let login_body: serde_json::Value =
            serde_json::from_slice(&login_res.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        let csrf = login_body["csrfToken"].as_str().unwrap();

        // Unauthenticated -> 401
        let unauth = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/filesystem/check")
                    .method("POST")
                    .header("origin", origin)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"path": media_dir}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // Valid readable folder -> readable: true, entriesCount: 2, mediaFilesCount: 1
        let ok_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/filesystem/check")
                    .method("POST")
                    .header("origin", origin)
                    .header("cookie", &cookie)
                    .header("x-csrf-token", csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"path": media_dir}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok_res.status(), StatusCode::OK);
        let ok_json: serde_json::Value =
            serde_json::from_slice(&ok_res.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(ok_json["readable"], true);
        assert_eq!(ok_json["entriesCount"], 2);
        assert_eq!(ok_json["mediaFilesCount"], 1);

        // Missing folder -> readable: false
        let missing_res = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/filesystem/check")
                    .method("POST")
                    .header("origin", origin)
                    .header("cookie", &cookie)
                    .header("x-csrf-token", csrf)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"path": temp.path().join("DoesNotExist")}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_res.status(), StatusCode::OK);
        let missing_json: serde_json::Value =
            serde_json::from_slice(&missing_res.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(missing_json["readable"], false);
        assert!(missing_json["error"].as_str().is_some());
    }
}
