//! Explicit original-download permission. Playback bytes cannot be made copy-proof.
use crate::{
    auth::AuthState,
    database::{query, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
pub const MEDIA_KINDS: [&str; 3] = ["films", "series", "music"];

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    allow_media_download: bool,
    /// Types allowed by this account's own rule. Omitted on save: unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_kinds: Option<Vec<String>>,
    /// Effective types after group rules (read-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effective_kinds: Option<Vec<String>>,
}

/// Validates a requested allow-list and returns the denied media types.
pub fn denied_kinds(allowed: &[String]) -> Result<Vec<&'static str>, StatusCode> {
    if allowed.len() > 8
        || allowed
            .iter()
            .any(|kind| !MEDIA_KINDS.contains(&kind.as_str()))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(MEDIA_KINDS
        .into_iter()
        .filter(|kind| !allowed.iter().any(|allowed| allowed == kind))
        .collect())
}

async fn own_kinds(store: &IdentityStore, user: &str) -> Result<Vec<String>, StatusCode> {
    let denied: Vec<String> =
        query_scalar("SELECT kind FROM user_media_kind_denials WHERE user_id=?")
            .bind(user)
            .fetch_all(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(MEDIA_KINDS
        .iter()
        .filter(|kind| !denied.iter().any(|d| d == *kind))
        .map(|kind| (*kind).to_owned())
        .collect())
}

/// Media types a user may reach once user and group denials are combined.
pub async fn effective_kinds(store: &IdentityStore, user: &str) -> Result<Vec<String>, StatusCode> {
    let denied: Vec<String> = query_scalar("SELECT kind FROM user_media_kind_denials WHERE user_id=? UNION SELECT x.kind FROM group_media_kind_denials x JOIN effective_user_group_members m ON m.group_id=x.group_id WHERE m.user_id=? UNION SELECT x.kind FROM group_media_kind_denials x JOIN user_groups g ON g.id=x.group_id WHERE g.is_default=1")
        .bind(user)
        .bind(user)
        .fetch_all(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(MEDIA_KINDS
        .iter()
        .filter(|kind| !denied.iter().any(|d| d == *kind))
        .map(|kind| (*kind).to_owned())
        .collect())
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/me/media-policy", get(own))
        .route("/api/v1/admin/users/{id}/media-policy", get(read).put(save))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(auth)
}
/// Downloads need both the server switch and the user's own permission.
pub async fn can_download(store: &IdentityStore, user: &str) -> Result<bool, StatusCode> {
    if !crate::server_settings::load(store).await?.downloads_enabled {
        return Ok(false);
    }
    user_flag(store, user).await
}
/// The user's own permission, as the administrator edits it.
async fn user_flag(store: &IdentityStore, user: &str) -> Result<bool, StatusCode> {
    let flag: Option<i64> =
        query_scalar("SELECT allow_media_download FROM user_media_policies WHERE user_id=?")
            .bind(user)
            .fetch_optional(&store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(flag.unwrap_or(1) == 1)
}
async fn own(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Policy>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    // Administrators bypass library rules; report every type for them.
    let effective = if actor.user.role == Role::Administrator {
        MEDIA_KINDS.iter().map(|kind| (*kind).to_owned()).collect()
    } else {
        effective_kinds(&auth.store, &actor.user.id).await?
    };
    Ok(Json(Policy {
        // Downloads belong to the native applications (bearer token), never the web.
        allow_media_download: actor.bearer && can_download(&auth.store, &actor.user.id).await?,
        allowed_kinds: None,
        effective_kinds: Some(effective),
    }))
}
async fn admin(auth: &AuthState, headers: &HeaderMap, mutation: bool) -> Result<(), StatusCode> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(())
}
async fn exists(store: &IdentityStore, id: &str) -> Result<(), StatusCode> {
    let count: i64 = query_scalar("SELECT COUNT(*) FROM users WHERE id=?")
        .bind(id)
        .fetch_one(&store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if count == 0 {
        Err(StatusCode::NOT_FOUND)
    } else {
        Ok(())
    }
}
async fn read(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Policy>, StatusCode> {
    admin(&auth, &headers, false).await?;
    exists(&auth.store, &id).await?;
    Ok(Json(Policy {
        allow_media_download: user_flag(&auth.store, &id).await?,
        allowed_kinds: Some(own_kinds(&auth.store, &id).await?),
        effective_kinds: Some(effective_kinds(&auth.store, &id).await?),
    }))
}
async fn save(
    State(auth): State<AuthState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Policy>,
) -> Result<StatusCode, StatusCode> {
    admin(&auth, &headers, true).await?;
    exists(&auth.store, &id).await?;
    let denied = input
        .allowed_kinds
        .as_deref()
        .map(denied_kinds)
        .transpose()?;
    let failed = |_| StatusCode::INTERNAL_SERVER_ERROR;
    let mut conn = auth.store.pool.acquire().await.map_err(failed)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(failed)?;
    query("INSERT INTO user_media_policies(user_id,allow_media_download) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET allow_media_download=excluded.allow_media_download")
        .bind(&id).bind(i64::from(input.allow_media_download)).execute(&mut *tx).await.map_err(failed)?;
    if let Some(denied) = denied {
        query("DELETE FROM user_media_kind_denials WHERE user_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
        for kind in denied {
            query("INSERT INTO user_media_kind_denials(user_id,kind) VALUES(?,?)")
                .bind(&id)
                .bind(kind)
                .execute(&mut *tx)
                .await
                .map_err(failed)?;
        }
        // Per-user catalogue caches must not serve a now-denied type.
        query("UPDATE inventory_cache_generation SET gen=gen+1 WHERE id=1")
            .execute(&mut *tx)
            .await
            .map_err(failed)?;
    }
    tx.commit().await.map_err(failed)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::database::{query, query_scalar};
    use crate::identity::IdentityStore;

    #[tokio::test]
    async fn media_kind_denials_and_age_ratings_filter_every_permission_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&dir.path().join("kinds.sqlite"))
            .await
            .unwrap();
        for sql in [
            "INSERT INTO users(id,role) VALUES('kid','user'),('adult','user')",
            "INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('f','Films','films','local','/f',1),('s','Séries','series','local','/s',1),('m','Musique','music','local','/m',1)",
            "INSERT INTO library_permissions(library_id,user_id) VALUES('f','kid'),('s','kid'),('m','kid'),('f','adult'),('s','adult'),('m','adult')",
            "INSERT INTO user_groups(id,name,is_default) VALUES('children','Enfants',0)",
            "INSERT INTO user_group_members(group_id,user_id) VALUES('children','kid')",
            "INSERT INTO user_media_kind_denials(user_id,kind) VALUES('kid','music')",
            "INSERT INTO group_media_kind_denials(group_id,kind) VALUES('children','series')",
        ] {
            query(sql).execute(store.pool()).await.unwrap();
        }
        let visible = |user: &'static str| {
            let store = store.clone();
            async move {
                query_scalar::<_, String>("SELECT library_id FROM effective_library_permissions WHERE user_id=? ORDER BY library_id")
                    .bind(user)
                    .fetch_all(store.pool())
                    .await
                    .unwrap()
            }
        };
        assert_eq!(visible("kid").await, ["f"]);
        assert_eq!(visible("adult").await, ["f", "m", "s"]);
        assert_eq!(
            super::effective_kinds(&store, "kid").await.unwrap(),
            ["films"]
        );
        // A default group denial applies to every account.
        query("INSERT INTO user_groups(id,name,is_default) VALUES('everyone','Tous',1)")
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO group_media_kind_denials(group_id,kind) VALUES('everyone','music')")
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(visible("adult").await, ["f", "s"]);
        assert!(super::denied_kinds(&["films".into(), "anime".into()]).is_err());

        for (label, age) in [
            ("FR:-16", Some(16)),
            ("-16 ans", Some(16)),
            ("FSK 16", Some(16)),
            ("TV-MA", Some(18)),
            ("Rated R", Some(17)),
            ("US:PG-13", Some(13)),
            ("FR:Tous publics", Some(0)),
            ("GB:15", Some(15)),
            ("NR", None),
        ] {
            let sql = format!(
                "SELECT {} FROM (SELECT NULL AS minimum_age, ? AS manual_json, '{{}}' AS editorial_json, 'none' AS id) w",
                crate::product::AGE_SQL
            );
            let value: Option<i64> = query_scalar(sqlx::AssertSqlSafe(sql))
                .bind(serde_json::json!({ "mpaa": label }).to_string())
                .fetch_one(store.pool())
                .await
                .unwrap();
            assert_eq!(value, age, "{label}");
        }
    }
}
