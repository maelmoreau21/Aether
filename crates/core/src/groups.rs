//! Local groups grant library access only; membership never grants a server role.
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

fn system_role(id: &str) -> Option<&'static str> {
    match id {
        "builtin-role-user" => Some("user"),
        "builtin-role-administrator" => Some("administrator"),
        _ => None,
    }
}

/// Keep custom groups and their grants intact, even if they already use these names.
pub(crate) async fn initialize_role_groups(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
) -> Result<(), sqlx::Error> {
    for (id, label) in [
        ("builtin-role-user", "Utilisateurs"),
        ("builtin-role-administrator", "Administrateurs"),
    ] {
        let exists: i64 = query_scalar("SELECT count(*) FROM user_groups WHERE id=?")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
        if exists != 0 {
            continue;
        }
        let mut suffix = 0;
        loop {
            let name = if suffix == 0 {
                label.to_owned()
            } else {
                format!("{label} (système {suffix})")
            };
            let used: i64 = query_scalar("SELECT count(*) FROM user_groups WHERE name=?")
                .bind(&name)
                .fetch_one(&mut **tx)
                .await?;
            if used == 0 {
                query("INSERT INTO user_groups(id,name,is_default) VALUES(?,?,0)")
                    .bind(id)
                    .bind(name)
                    .execute(&mut **tx)
                    .await?;
                break;
            }
            suffix += 1;
        }
    }
    Ok(())
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/groups", get(list).post(create))
        .route(
            "/api/v1/admin/groups/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route(
            "/api/v1/admin/groups/{id}/parental",
            get(get_parental).put(save_parental).delete(delete_parental),
        )
        .layer(DefaultBodyLimit::max(32768))
        .with_state(auth)
}
pub(crate) async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<Authenticated, StatusCode> {
    let identity = auth.authenticate(headers).await?;
    if identity.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &identity)?;
    }
    Ok(identity)
}
pub(crate) async fn recheck_administrator(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    actor: &Authenticated,
) -> Result<(), StatusCode> {
    let count: i64 = query_scalar(
        "SELECT count(*) FROM sessions s JOIN users u ON u.id=s.user_id          WHERE s.token_hash=? AND s.user_id=? AND s.expires_at>unixepoch()          AND s.last_seen_at>unixepoch()-? AND u.disabled_at IS NULL AND u.role='administrator'",
    )
    .bind(&actor.token_hash)
    .bind(&actor.user.id)
    .bind(crate::auth::IDLE_SECONDS)
    .fetch_one(&mut **tx).await.map_err(database)?;
    if count != 1 {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(())
}
fn database(error: sqlx::Error) -> StatusCode {
    if error
        .as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        StatusCode::CONFLICT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    name: String,
    #[serde(default)]
    is_default: bool,
    user_ids: Vec<String>,
    library_ids: Vec<String>,
    /// Media types allowed to members; omitted keeps the current rule.
    #[serde(default)]
    allowed_kinds: Option<Vec<String>>,
    /// Original-download permission for members (null = inherit, omitted = keep).
    #[serde(default)]
    allow_media_download: Option<Value>,
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    let mut tx = auth.store.pool.begin().await.map_err(database)?;
    let rows: Vec<(String, String, crate::database::DbBool, Option<i64>)> = query_as(
        "SELECT id,name,is_default,allow_media_download FROM user_groups ORDER BY name,id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(database)?;
    let members: Vec<(String, String)> =
        query_as("SELECT group_id,user_id FROM effective_user_group_members ORDER BY user_id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let libraries: Vec<(String, String)> =
        query_as("SELECT group_id,library_id FROM user_group_libraries ORDER BY library_id")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let denials: Vec<(String, String)> =
        query_as("SELECT group_id,kind FROM group_media_kind_denials")
            .fetch_all(&mut *tx)
            .await
            .map_err(database)?;
    let values: Vec<_>=rows.into_iter().map(|(id,name,is_default,allow_dl)|{
        let allowed: Vec<_> = crate::user_media_policy::MEDIA_KINDS.iter().filter(|kind| !denials.iter().any(|(group, denied)| group == &id && denied == *kind)).collect();
        json!({"id":id,"name":name,"isDefault":bool::from(is_default),"systemRole":system_role(&id),"userIds":members.iter().filter(|r|r.0==id).map(|r|&r.1).collect::<Vec<_>>(),"libraryIds":libraries.iter().filter(|r|r.0==id).map(|r|&r.1).collect::<Vec<_>>(),"allowedKinds":allowed,"allowMediaDownload":allow_dl.map(|v|v!=0) })
    }).collect();
    tx.commit().await.map_err(database)?;
    Ok(Json(json!({"groups":values})))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    let id = uuid::Uuid::new_v4().to_string();
    save(&auth, &actor, &id, input, true).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Input>,
) -> Result<Json<Value>, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    save(&auth, &actor, &id, input, false).await?;
    Ok(Json(json!({"id":id})))
}
async fn save(
    auth: &AuthState,
    actor: &Authenticated,
    id: &str,
    input: Input,
    create: bool,
) -> Result<(), StatusCode> {
    let name = input.name.trim();
    if name.is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || input.user_ids.len() > 500
        || input.library_ids.len() > 500
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let denied = input
        .allowed_kinds
        .as_deref()
        .map(crate::user_media_policy::denied_kinds)
        .transpose()?;
    let download_setting = match input.allow_media_download {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::Bool(b)) => Some(Some(i64::from(b))),
        Some(_) => return Err(StatusCode::BAD_REQUEST),
    };
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, actor).await?;
    // System group members follow the server role; ignore stale member lists
    // supplied by clients and reject making either group global.
    if system_role(id).is_some() && input.is_default {
        return Err(StatusCode::BAD_REQUEST);
    }
    for user in &input.user_ids {
        let exists: i64 = query_scalar("SELECT count(*) FROM users WHERE id=?")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
        if exists != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    for library in &input.library_ids {
        let exists: i64 = query_scalar("SELECT count(*) FROM libraries WHERE id=?")
            .bind(library)
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
        if exists != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    if create {
        let count: i64 = query_scalar("SELECT count(*) FROM user_groups")
            .fetch_one(&mut *tx)
            .await
            .map_err(database)?;
        if count >= 500 {
            return Err(StatusCode::CONFLICT);
        }
        if let Some(dl) = download_setting {
            query(
                "INSERT INTO user_groups(id,name,is_default,allow_media_download) VALUES(?,?,?,?)",
            )
            .bind(id)
            .bind(name)
            .bind(i64::from(input.is_default))
            .bind(dl)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        } else {
            query("INSERT INTO user_groups(id,name,is_default) VALUES(?,?,?)")
                .bind(id)
                .bind(name)
                .bind(i64::from(input.is_default))
                .execute(&mut *tx)
                .await
                .map_err(database)?;
        }
    } else {
        if query("UPDATE user_groups SET name=?,is_default=? WHERE id=?")
            .bind(name)
            .bind(i64::from(input.is_default))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?
            .rows_affected()
            != 1
        {
            return Err(StatusCode::NOT_FOUND);
        }
        if let Some(dl) = download_setting {
            query("UPDATE user_groups SET allow_media_download=? WHERE id=?")
                .bind(dl)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(database)?;
        }
        query("DELETE FROM user_group_members WHERE group_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        query("DELETE FROM user_group_libraries WHERE group_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    for user in input
        .user_ids
        .into_iter()
        .filter(|_| system_role(id).is_none())
    {
        query(
            "INSERT INTO user_group_members(group_id,user_id) VALUES(?,?) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(user)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    }
    for library in input.library_ids {
        query("INSERT INTO user_group_libraries(group_id,library_id) VALUES(?,?) ON CONFLICT DO NOTHING").bind(id).bind(library).execute(&mut *tx).await.map_err(database)?;
    }
    if let Some(denied) = denied {
        query("DELETE FROM group_media_kind_denials WHERE group_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        for kind in denied {
            query("INSERT INTO group_media_kind_denials(group_id,kind) VALUES(?,?)")
                .bind(id)
                .bind(kind)
                .execute(&mut *tx)
                .await
                .map_err(database)?;
        }
    }
    // Library or type grants changed: per-user catalogue caches are stale.
    query("UPDATE inventory_cache_generation SET gen=gen+1 WHERE id=1")
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    let group_parental: Option<(i64, i64, String, String)> = query_as(
        "SELECT maximum_age,allow_unrated,blocked_tags_json,access_schedule_json FROM group_parental_policies WHERE group_id=?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(database)?;
    if let Some((max_age, allow_unrated, tags_json, schedule_json)) = group_parental {
        apply_parental_to_members(
            &mut tx,
            id,
            max_age,
            allow_unrated,
            &tags_json,
            &schedule_json,
        )
        .await?;
    }
    tx.commit().await.map_err(database)?;
    Ok(())
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    if system_role(&id).is_some() {
        return Err(StatusCode::CONFLICT);
    }
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, &actor).await?;
    let affected = query("DELETE FROM user_groups WHERE id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(database)?
        .rows_affected();
    if affected == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    tx.commit().await.map_err(database)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParentalAccessWindow {
    day: i64,
    start_minute: i64,
    end_minute: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GroupParentalInput {
    maximum_age: i64,
    allow_unrated: bool,
    #[serde(default)]
    blocked_tags: Vec<String>,
    #[serde(default)]
    access_schedule: Vec<ParentalAccessWindow>,
}
async fn apply_parental_to_members(
    tx: &mut sqlx::Transaction<'_, sqlx::Any>,
    group_id: &str,
    maximum_age: i64,
    allow_unrated: i64,
    tags_json: &str,
    schedule_json: &str,
) -> Result<(), StatusCode> {
    let member_ids: Vec<String> = query_scalar(
        "SELECT u.id FROM effective_user_group_members m JOIN users u ON u.id=m.user_id WHERE m.group_id=? AND u.role!='administrator'",
    )
    .bind(group_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(database)?;
    let tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
    for uid in member_ids {
        query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated,blocked_tags_json,access_schedule_json) VALUES(?,?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET maximum_age=excluded.maximum_age,allow_unrated=excluded.allow_unrated,blocked_tags_json=excluded.blocked_tags_json,access_schedule_json=excluded.access_schedule_json")
            .bind(&uid)
            .bind(maximum_age)
            .bind(allow_unrated)
            .bind(tags_json)
            .bind(schedule_json)
            .execute(&mut **tx)
            .await
            .map_err(database)?;
        query("DELETE FROM parental_blocked_tags WHERE user_id=?")
            .bind(&uid)
            .execute(&mut **tx)
            .await
            .map_err(database)?;
        for tag in &tags {
            query("INSERT INTO parental_blocked_tags(user_id,tag_json) VALUES(?,?) ON CONFLICT DO NOTHING")
                .bind(&uid)
                .bind(json!(tag).to_string())
                .execute(&mut **tx)
                .await
                .map_err(database)?;
        }
    }
    Ok(())
}
async fn get_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    let row: Option<(i64, i64, String, String)> = query_as(
        "SELECT maximum_age,allow_unrated,blocked_tags_json,access_schedule_json FROM group_parental_policies WHERE group_id=?",
    )
    .bind(&id)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(database)?;
    Ok(Json(
        row.map(|(age, unrated, tags, schedule)| {
            json!({
                "maximumAge": age,
                "allowUnrated": unrated != 0,
                "blockedTags": serde_json::from_str::<Value>(&tags).unwrap_or(Value::Null),
                "accessSchedule": serde_json::from_str::<Value>(&schedule).unwrap_or(Value::Null)
            })
        })
        .unwrap_or(Value::Null),
    ))
}
async fn save_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<GroupParentalInput>,
) -> Result<StatusCode, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    if !(0..=21).contains(&input.maximum_age)
        || input.blocked_tags.len() > 64
        || input
            .blocked_tags
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 100 || v.chars().any(char::is_control))
        || input.access_schedule.len() > 56
        || input.access_schedule.iter().any(|w| {
            !(0..=6).contains(&w.day)
                || !(0..1440).contains(&w.start_minute)
                || !(1..=1440).contains(&w.end_minute)
                || w.start_minute >= w.end_minute
        })
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let tags: Vec<String> = input
        .blocked_tags
        .iter()
        .map(|v| v.trim().to_lowercase())
        .collect();
    let tags_json = json!(tags).to_string();
    let schedule_json = json!(
        input
            .access_schedule
            .iter()
            .map(
                |w| json!({"day": w.day, "startMinute": w.start_minute, "endMinute": w.end_minute})
            )
            .collect::<Vec<_>>()
    )
    .to_string();
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, &actor).await?;
    let exists: i64 = query_scalar("SELECT count(*) FROM user_groups WHERE id=?")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database)?;
    if exists != 1 {
        return Err(StatusCode::NOT_FOUND);
    }
    let allow_unrated = i64::from(input.allow_unrated);
    query("INSERT INTO group_parental_policies(group_id,maximum_age,allow_unrated,blocked_tags_json,access_schedule_json) VALUES(?,?,?,?,?) ON CONFLICT(group_id) DO UPDATE SET maximum_age=excluded.maximum_age,allow_unrated=excluded.allow_unrated,blocked_tags_json=excluded.blocked_tags_json,access_schedule_json=excluded.access_schedule_json")
        .bind(&id)
        .bind(input.maximum_age)
        .bind(allow_unrated)
        .bind(&tags_json)
        .bind(&schedule_json)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    apply_parental_to_members(
        &mut tx,
        &id,
        input.maximum_age,
        allow_unrated,
        &tags_json,
        &schedule_json,
    )
    .await?;
    tx.commit().await.map_err(database)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_parental(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let actor = administrator(&auth, &headers, true).await?;
    let mut conn = auth.store.pool.acquire().await.map_err(database)?;
    let mut tx = crate::database::begin_write(&mut conn)
        .await
        .map_err(database)?;
    recheck_administrator(&mut tx, &actor).await?;
    query("DELETE FROM group_parental_policies WHERE group_id=?")
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    let member_ids: Vec<String> = query_scalar(
        "SELECT u.id FROM effective_user_group_members m JOIN users u ON u.id=m.user_id WHERE m.group_id=? AND u.role!='administrator'",
    )
    .bind(&id)
    .fetch_all(&mut *tx)
    .await
    .map_err(database)?;
    for uid in member_ids {
        query("DELETE FROM parental_policies WHERE user_id=?")
            .bind(&uid)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        query("DELETE FROM parental_blocked_tags WHERE user_id=?")
            .bind(&uid)
            .execute(&mut *tx)
            .await
            .map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    crate::inventory::invalidate_catalogue_cache();
    Ok(StatusCode::NO_CONTENT)
}
