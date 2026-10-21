//! Persistent library configuration. Filesystem policy is delegated to the local Node.
use crate::{
    auth::AuthState,
    identity::{IdentityStore, Role},
    node::validate_library_root,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::path::{Path as DiskPath, PathBuf};

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
}
fn default_enabled() -> bool {
    true
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibraryInput {
    name: String,
    kind: String,
    node_id: String,
    root: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    excluded_patterns: String,
    #[serde(default)]
    user_ids: Vec<String>,
    #[serde(default)]
    options: Option<crate::library_options::LibraryOptions>,
}

pub fn router(auth: AuthState, local_root: PathBuf) -> Router {
    Router::new()
        .route("/api/v1/libraries", get(accessible))
        .route("/api/v1/admin/libraries", get(list).post(create))
        .route("/api/v1/admin/libraries/{id}", axum::routing::put(update))
        .route("/api/v1/admin/roots", get(roots))
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
    let rows:Vec<(String,String,String)> = crate::database::query_as("SELECT l.id,l.name,l.kind FROM libraries l JOIN media_nodes n ON n.id=l.node_id WHERE l.enabled=1 AND n.status!='revoked' AND (?='administrator' OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id AND p.user_id=?)) ORDER BY l.name,l.id")
        .bind(actor.user.role.as_str()).bind(actor.user.id).fetch_all(&state.auth.store.pool).await.map_err(database)?;
    Ok(Json(
        serde_json::json!({"libraries":rows.into_iter().map(|(id,name,kind)|serde_json::json!({"id":id,"name":name,"kind":kind})).collect::<Vec<_>>()}),
    ))
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
        "SELECT id,name,kind,node_id,root,enabled,excluded_patterns,options_json FROM libraries ORDER BY name,id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(database)?;
    for library in &mut libraries {
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
        library.cover_urls = crate::database::query_scalar("SELECT COALESCE((SELECT '/api/v1/works/' || w.id || '/images/' || wi.id || '?width=320' FROM work_image_entries wi WHERE wi.work_id=w.id AND wi.kind='poster' ORDER BY wi.position,wi.id LIMIT 1),(SELECT '/api/v1/works/' || w.id || '/poster' FROM work_posters wp WHERE wp.work_id=w.id),(SELECT '/api/v1/assets/' || i.id || '/image?width=320' FROM inventory_images i WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 AND i.root_snapshot=l.root ORDER BY i.priority,i.id LIMIT 1)) AS url FROM inventory_works w JOIN libraries l ON l.id=w.library_id JOIN media_nodes n ON n.id=l.node_id WHERE l.id=? AND l.enabled=1 AND n.status='connected' AND w.kind IN ('film','series') AND (w.kind='series' OR EXISTS(SELECT 1 FROM inventory_editions ce JOIN inventory_versions cv ON cv.edition_id=ce.id JOIN inventory_sources cs ON cs.version_id=cv.id JOIN inventory_assets ca ON ca.source_id=cs.id WHERE ce.work_id=w.id AND ca.available=1 AND cs.root_snapshot=l.root)) AND (EXISTS(SELECT 1 FROM work_image_entries wi WHERE wi.work_id=w.id AND wi.kind='poster') OR EXISTS(SELECT 1 FROM work_posters wp WHERE wp.work_id=w.id) OR EXISTS(SELECT 1 FROM inventory_images i WHERE i.work_id=w.id AND i.role='poster' AND i.available=1 AND i.root_snapshot=l.root)) ORDER BY w.title_sort,w.id LIMIT 4")
            .bind(&library.id).fetch_all(&mut *tx).await.map_err(database)?;
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
async fn roots(
    State(state): State<LibraryState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    administrator(&state, &headers, false).await?;
    let mut all_roots = Vec::new();
    if let Ok(local_root) = validate_library_root(&state.root, &state.root).await {
        all_roots.push(local_root.to_string_lossy().into_owned());
        if let Ok(mut entries) = tokio::fs::read_dir(&local_root).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await
                    && meta.is_dir()
                {
                    let path = entry.path();
                    if let Ok(sub) = validate_library_root(&path, &local_root).await {
                        let sub_str = sub.to_string_lossy().into_owned();
                        if !all_roots.contains(&sub_str) {
                            all_roots.push(sub_str);
                        }
                    }
                }
            }
        }
    }
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
    Ok(Json(serde_json::json!({"roots": all_roots})))
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
) -> Result<Response, StatusCode> {
    administrator(&state, &headers, false).await?;
    image_target(&state, &id, &kind).await?;
    let bytes: Vec<u8> = crate::database::query_scalar(
        "SELECT image_data FROM library_artwork WHERE library_id=? AND kind=?",
    )
    .bind(id)
    .bind(kind)
    .fetch_optional(&state.auth.store.pool)
    .await
    .map_err(database)?
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
static IMAGE_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
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
    let permit = IMAGE_WORKERS
        .try_acquire()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let normalized = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::avatars::normalize_image(bytes, &content, true)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    crate::database::query("INSERT INTO library_artwork(library_id,kind,image_data,revision) VALUES(?,?,?,?) ON CONFLICT(library_id,kind) DO UPDATE SET image_data=excluded.image_data,revision=excluded.revision")
        .bind(id).bind(kind).bind(normalized).bind(uuid::Uuid::new_v4().to_string()).execute(&state.auth.store.pool).await.map_err(database)?;
    Ok(StatusCode::NO_CONTENT)
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
    Ok(Json(save(&state, id, input, true).await?))
}
async fn save(
    state: &LibraryState,
    id: String,
    mut input: LibraryInput,
    existing: bool,
) -> Result<Library, StatusCode> {
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
    let root = if input.node_id == "local" {
        let validated = validate_library_root(DiskPath::new(&input.root), &state.root)
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
        .bind(&input.node_id)
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
        let requested_root = input.root.trim();
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
    };
    input.user_ids.sort();
    input.user_ids.dedup();
    let mut conn = state.auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    if existing {
        let active: i64 = crate::database::query_scalar(
            "SELECT COUNT(*) FROM scan_jobs WHERE library_id=? AND status IN ('queued','running')",
        )
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
        if active != 0 {
            return Err(StatusCode::CONFLICT);
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
    let options = match input.options {
        Some(options) => options,
        None => serde_json::from_str(previous.as_deref().unwrap_or("{}"))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    };
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
    tx.commit().await.map_err(database)?;
    Ok(Library {
        id,
        name: input.name,
        kind: input.kind,
        node_id: input.node_id,
        root,
        enabled: input.enabled,
        excluded_patterns: input.excluded_patterns,
        user_ids: input.user_ids,
        options_json,
        options,
        cover_urls: vec![],
        image_kinds: vec![],
        image_versions: Default::default(),
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
        }
    }
    #[tokio::test]
    async fn options_validation_legacy_update_and_restart() {
        let (temp, state, _) = setup().await;
        let mut configured = input(&state, vec![]);
        configured.options = Some(crate::library_options::LibraryOptions {
            read_local_nfo: false,
            fetch_logo: false,
            automatic_refresh_interval_days: 30,
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
        // Unsafe writers / unsupported features cannot be silently accepted as active.
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
            cookies.push(format!("media_session={raw}"));
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
    }
}
