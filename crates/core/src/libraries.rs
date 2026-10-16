//! Persistent library configuration. Filesystem policy is delegated to the local Node.
use crate::{
    auth::{AuthState, AuthUser},
    identity::{IdentityStore, Role},
    node::validate_library_root,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, Row};
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
    pub enabled: bool,
    #[sqlx(skip)]
    pub user_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibraryInput {
    name: String,
    kind: String,
    node_id: String,
    root: String,
    enabled: bool,
    user_ids: Vec<String>,
}

pub fn router(auth: AuthState, local_root: PathBuf) -> Router {
    Router::new()
        .route("/api/v1/admin/libraries", get(list).post(create))
        .route("/api/v1/admin/libraries/{id}", axum::routing::put(update))
        .route("/api/v1/admin/users", get(users))
        .route("/api/v1/admin/roots", get(roots))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(LibraryState {
            auth,
            root: local_root,
        })
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
    let mut tx = connection.begin_with("BEGIN IMMEDIATE").await?;
    let initialized: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM library_initialization)")
            .fetch_one(&mut *tx)
            .await?;
    if !initialized {
        for (id, name, kind) in [
            ("fixture-films", "Films de test", "films"),
            ("fixture-series", "Séries de test", "series"),
        ] {
            sqlx::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES (?,?,?,'local',?,1)")
                .bind(id).bind(name).bind(kind).bind(root.to_string_lossy().as_ref()).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO library_initialization(singleton) VALUES(1)")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn load(store: &IdentityStore) -> Result<Vec<Library>, StatusCode> {
    // Same transaction gives a coherent configuration/grants snapshot.
    let mut tx = store.pool.begin().await.map_err(database)?;
    let mut libraries: Vec<Library> =
        sqlx::query_as("SELECT id,name,kind,node_id,root,enabled FROM libraries ORDER BY name,id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let grants = sqlx::query("SELECT library_id,user_id FROM library_permissions ORDER BY user_id")
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
async fn users(
    State(state): State<LibraryState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    administrator(&state, &headers, false).await?;
    let users: Vec<AuthUser> = sqlx::query_as("SELECT u.id,c.login,u.role FROM users u JOIN local_credentials c ON c.user_id=u.id WHERE u.disabled_at IS NULL ORDER BY c.login").fetch_all(&state.auth.store.pool).await.map_err(database)?;
    Ok(Json(serde_json::json!({"users":users})))
}
async fn roots(
    State(state): State<LibraryState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    administrator(&state, &headers, false).await?;
    let root = validate_library_root(&state.root, &state.root)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(serde_json::json!({"roots":[root.to_string_lossy()]})))
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
    if input.name.is_empty()
        || input.name.chars().count() > 100
        || input.name.chars().any(char::is_control)
        || !matches!(input.kind.as_str(), "films" | "series")
        || input.node_id != "local"
        || input.user_ids.len() > 1000
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let root = validate_library_root(DiskPath::new(&input.root), &state.root)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    input.user_ids.sort();
    input.user_ids.dedup();
    let mut tx = state.auth.store.pool.begin().await.map_err(database)?;
    for user in &input.user_ids {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND role='user' AND disabled_at IS NULL)",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
        if !valid {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let root = root.to_str().ok_or(StatusCode::BAD_REQUEST)?.to_owned();
    if existing {
        let result =
            sqlx::query("UPDATE libraries SET name=?,kind=?,node_id=?,root=?,enabled=? WHERE id=?")
                .bind(&input.name)
                .bind(&input.kind)
                .bind(&input.node_id)
                .bind(&root)
                .bind(input.enabled)
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(database)?;
        if result.rows_affected() == 0 {
            return Err(StatusCode::NOT_FOUND);
        }
        sqlx::query("DELETE FROM library_permissions WHERE library_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    } else {
        sqlx::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,?,?,?,?)")
            .bind(&id)
            .bind(&input.name)
            .bind(&input.kind)
            .bind(&input.node_id)
            .bind(&root)
            .bind(input.enabled)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    for user in &input.user_ids {
        sqlx::query("INSERT INTO library_permissions(library_id,user_id) VALUES(?,?)")
            .bind(&id)
            .bind(user)
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
        user_ids: input.user_ids,
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
        sqlx::query("INSERT INTO users(id,role) VALUES(?,'user')")
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
            user_ids: users,
        }
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
            sqlx::query("INSERT INTO users(id,role) VALUES(?,?)")
                .bind(id)
                .bind(role)
                .execute(&store.pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO local_credentials(user_id,login,password_hash) VALUES(?,?,'unused-test-hash')").bind(id).bind(id).execute(&store.pool).await.unwrap();
            let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
            sqlx::query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,?,unixepoch(),unixepoch(),unixepoch()+3600)").bind(hash).bind(id).execute(&store.pool).await.unwrap();
            cookies.push(format!("media_session={raw}"));
        }
        let csrf = format!(
            "{:x}",
            Sha256::digest(format!("csrf:{}", "a".repeat(64)).as_bytes())
        );
        let app = router(
            AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
                .await
                .unwrap(),
            root.clone(),
        );
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
        let grants: i64 =
            sqlx::query_scalar("SELECT count(*) FROM library_permissions WHERE library_id=?")
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
