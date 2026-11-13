//! Resource-scoped reports: structured observations, never automatic repairs.
use crate::{
    applications::{self, Error, db, err},
    auth::{AuthState, Authenticated},
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
pub fn now() -> i64 {
    media_contracts::protocol::unix_now()
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub engine: Option<String>,
    pub error_code: Option<String>,
    pub audio_track_id: Option<String>,
    pub subtitle_track_id: Option<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Create {
    pub category: String,
    pub description: String,
    pub work_id: Option<String>,
    pub track_id: Option<String>,
    pub asset_id: Option<String>,
    pub position_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
    #[serde(default)]
    pub context: Context,
}
#[derive(sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub user_id: Option<String>,
    pub library_id: Option<String>,
    pub work_id: Option<String>,
    pub track_id: Option<String>,
    pub asset_id: Option<String>,
    pub category: String,
    pub description: String,
    pub status: String,
    pub position_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
    #[serde(skip_serializing)]
    pub context_json: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source_key: Option<String>,
    pub author_label: Option<String>,
}
impl Record {
    fn value(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or_else(|_| json!({}));
        value["context"] = serde_json::from_str(&self.context_json).unwrap_or_else(|_| json!({}));
        value
    }
}
pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/reports", get(list).post(create))
        .route("/api/v1/reports/{id}", get(detail))
        .route("/api/v1/reports/{id}/comments", post(comment))
        .route("/api/v1/reports/{id}/status", post(status))
        .route("/api/v1/reports/{id}/priority", post(priority))
        .layer(DefaultBodyLimit::max(32768))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
async fn music_allowed(store: &IdentityStore, user: &str) -> Result<(), Error> {
    let denied: i64 =
        query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=? AND allow_unrated=0")
            .bind(user)
            .fetch_one(store.pool())
            .await
            .map_err(db)?;
    if denied != 0 {
        return Err(err(StatusCode::FORBIDDEN, "parental_restriction"));
    }
    Ok(())
}
fn valid(input: &Create) -> bool {
    matches!(
        input.category.as_str(),
        "video" | "audio" | "subtitles" | "metadata" | "missing" | "other"
    ) && (3..=8000).contains(&input.description.trim().len())
        && [
            &input.work_id,
            &input.track_id,
            &input.asset_id,
            &input.context.audio_track_id,
            &input.context.subtitle_track_id,
        ]
        .iter()
        .all(|v| v.as_ref().is_none_or(|s| valid_id(s)))
        && [&input.context.engine, &input.context.error_code]
            .iter()
            .all(|v| {
                v.as_ref().is_none_or(|s| {
                    s.len() <= 128
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_.:+ -".contains(&c))
                })
            })
        && [input.position_seconds, input.end_seconds]
            .iter()
            .all(|v| v.is_none_or(|n| n.is_finite() && (0.0..=604800.0).contains(&n)))
        && input
            .end_seconds
            .is_none_or(|end| input.position_seconds.is_some_and(|start| end >= start))
}
async fn resource(
    store: &IdentityStore,
    actor: &Authenticated,
    input: &Create,
) -> Result<String, Error> {
    let mut library: Option<String> = None;
    for (kind, id) in [
        ("work", &input.work_id),
        ("track", &input.track_id),
        ("asset", &input.asset_id),
    ] {
        if let Some(id) = id {
            if kind == "track" {
                music_allowed(store, &actor.user.id).await?;
            }
            let found = if kind == "track" {
                query_scalar::<_, String>("SELECT library_id FROM music_tracks WHERE id=?")
                    .bind(id)
                    .fetch_optional(store.pool())
                    .await
                    .map_err(db)?
            } else {
                crate::inventory::resource_library(store, kind, id)
                    .await
                    .map_err(db)?
            };
            let found = found.ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_resource"))?;
            if library.as_ref().is_some_and(|previous| previous != &found) {
                return Err(err(StatusCode::BAD_REQUEST, "resource_mismatch"));
            }
            applications::authorize_library(store, actor, "reports", &found).await?;
            if kind != "track"
                && !crate::product::target_allowed(store, &actor.user.id, kind, id)
                    .await
                    .map_err(|s| err(s, "parental_policy_unavailable"))?
            {
                return Err(err(StatusCode::FORBIDDEN, "parental_restriction"));
            }
            library = Some(found);
        }
    }
    if input.track_id.is_some() && (input.work_id.is_some() || input.asset_id.is_some()) {
        return Err(err(StatusCode::BAD_REQUEST, "resource_mismatch"));
    }
    if let (Some(asset), Some(work)) = (&input.asset_id, &input.work_id) {
        let matches: i64 = query_scalar(
            "SELECT COUNT(*) \
                FROM inventory_assets a \
                JOIN inventory_sources s ON s.id=a.source_id \
                JOIN inventory_versions v ON v.id=s.version_id \
                JOIN inventory_editions e ON e.id=v.edition_id \
                WHERE a.id=? AND e.work_id=?",
        )
        .bind(asset)
        .bind(work)
        .fetch_one(store.pool())
        .await
        .map_err(db)?;
        if matches != 1 {
            return Err(err(StatusCode::BAD_REQUEST, "resource_mismatch"));
        }
    }
    for (kind, track) in [
        ("audio", &input.context.audio_track_id),
        ("subtitle", &input.context.subtitle_track_id),
    ] {
        let Some(track) = track else {
            continue;
        };
        let Some(asset) = &input.asset_id else {
            return Err(err(StatusCode::BAD_REQUEST, "asset_required"));
        };
        if !crate::inventory::report_track_allowed(store, asset, kind, track)
            .await
            .map_err(db)?
        {
            return Err(err(StatusCode::BAD_REQUEST, "track_mismatch"));
        }
    }
    library.ok_or_else(|| err(StatusCode::BAD_REQUEST, "resource_required"))
}
async fn read(store: &IdentityStore, actor: &Authenticated, id: &str) -> Result<Record, Error> {
    let record: Record = query_as("SELECT * FROM media_reports WHERE id=?")
        .bind(id)
        .fetch_optional(store.pool())
        .await
        .map_err(db)?
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_report"))?;
    if actor.user.role != Role::Administrator && record.user_id.as_deref() != Some(&actor.user.id) {
        return Err(err(StatusCode::NOT_FOUND, "unknown_report"));
    }
    if let Some(library) = &record.library_id {
        applications::authorize_library(store, actor, "reports", library).await?;
        if record.track_id.is_some() {
            music_allowed(store, &actor.user.id).await?;
        }
        let target = record
            .work_id
            .as_ref()
            .map(|id| ("work", id))
            .or_else(|| record.asset_id.as_ref().map(|id| ("asset", id)));
        if let Some((kind, id)) = target
            && !crate::product::target_allowed(store, &actor.user.id, kind, id)
                .await
                .map_err(|s| err(s, "parental_policy_unavailable"))?
        {
            return Err(err(StatusCode::FORBIDDEN, "parental_restriction"));
        }
    } else if actor.user.role != Role::Administrator {
        let policies: i64 = query_scalar("SELECT COUNT(*) FROM parental_policies WHERE user_id=?")
            .bind(&actor.user.id)
            .fetch_one(store.pool())
            .await
            .map_err(db)?;
        if policies != 0 {
            return Err(err(StatusCode::FORBIDDEN, "parental_restriction"));
        }
    }
    Ok(record)
}
const PRIORITIES: [&str; 3] = ["high", "normal", "low"];
const STATUSES: [&str; 4] = ["open", "in_progress", "resolved", "closed"];
const CATEGORIES: [&str; 6] = [
    "video",
    "audio",
    "subtitles",
    "metadata",
    "missing",
    "other",
];

/// Triage proposed by `reports.classify`. An older or unavailable extension yields
/// the neutral default; only bounded identifier codes are kept.
fn triage_from(result: Option<&Value>) -> (&'static str, Option<String>, Vec<String>) {
    let code_like = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && value
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._".contains(&c))
    };
    let priority = result
        .and_then(|v| v["priority"].as_str())
        .and_then(|p| PRIORITIES.into_iter().find(|known| *known == p))
        .unwrap_or("normal");
    let code = result
        .and_then(|v| v["code"].as_str())
        .filter(|c| code_like(c, 64))
        .map(str::to_owned);
    let checks = result
        .and_then(|v| v["checks"].as_array())
        .map(|checks| {
            checks
                .iter()
                .filter_map(Value::as_str)
                .filter(|c| code_like(c, 48))
                .take(8)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    (priority, code, checks)
}

/// Adds what a reader needs to act on a report: the media it concerns, a readable
/// author and, for administrators, the triage hint.
async fn present(
    store: &IdentityStore,
    actor: &Authenticated,
    record: &Record,
    labels: &std::collections::BTreeMap<String, String>,
) -> Result<Value, Error> {
    let mut value = record.value();
    if let Some(work) = &record.work_id {
        let row: Option<(String, Option<i64>, String)> =
            query_as("SELECT title,year,kind FROM inventory_works WHERE id=?")
                .bind(work)
                .fetch_optional(store.pool())
                .await
                .map_err(db)?;
        if let Some((title, year, kind)) = row {
            value["subject"] = json!({"kind":kind,"title":title,"year":year,"workId":work,
                "image":format!("/api/v1/works/{work}/poster")});
        }
    } else if let Some(track) = &record.track_id {
        let row: Option<(String, String, String)> =
            query_as("SELECT title,artist,album FROM music_tracks WHERE id=?")
                .bind(track)
                .fetch_optional(store.pool())
                .await
                .map_err(db)?;
        if let Some((title, artist, album)) = row {
            value["subject"] = json!({"kind":"track","title":title,"artist":artist,"album":album,
                "trackId":track,"image":format!("/api/v1/music/tracks/{track}/cover")});
        }
    }
    if record.author_label.is_none()
        && let Some(label) = record.user_id.as_ref().and_then(|id| labels.get(id))
    {
        value["authorLabel"] = json!(label);
    }
    if actor.user.role == Role::Administrator {
        let row: Option<(String, Option<String>, String)> =
            query_as("SELECT priority,code,checks_json FROM media_report_triage WHERE report_id=?")
                .bind(&record.id)
                .fetch_optional(store.pool())
                .await
                .map_err(db)?;
        let (priority, code, checks) = row.unwrap_or_else(|| ("normal".into(), None, "[]".into()));
        value["priority"] = json!(priority);
        value["triage"] = json!({"code":code,
            "checks":serde_json::from_str::<Value>(&checks).unwrap_or_else(|_| json!([]))});
    }
    Ok(value)
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    #[serde(default)]
    offset: u32,
    /// One status, or `active` for open and in progress.
    status: Option<String>,
    category: Option<String>,
    priority: Option<String>,
    /// `priority` orders the administrator queue; otherwise most recent activity first.
    sort: Option<String>,
    /// Lets an administrator see only their own reports.
    #[serde(default)]
    mine: bool,
}
async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>, Error> {
    let actor = applications::require(&auth, &headers, "reports", false).await?;
    let administrator = actor.user.role == Role::Administrator;
    if page.offset > 1_000_000
        || page
            .status
            .as_deref()
            .is_some_and(|s| s != "active" && !STATUSES.contains(&s))
        || page
            .category
            .as_deref()
            .is_some_and(|c| !CATEGORIES.contains(&c))
        || page
            .priority
            .as_deref()
            .is_some_and(|p| !administrator || !PRIORITIES.contains(&p))
        || page
            .sort
            .as_deref()
            .is_some_and(|s| !matches!(s, "updated" | "priority"))
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_page"));
    }
    let order = if page.sort.as_deref() == Some("priority") && administrator {
        "CASE COALESCE(t.priority,'normal') WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END,r.updated_at DESC,r.id"
    } else {
        "r.updated_at DESC,r.id"
    };
    let scope = "(?='administrator' OR r.user_id=?) AND (?=0 OR r.user_id=?)";
    let mine = i64::from(page.mine);
    let sql = format!(
        "SELECT r.* \
                FROM media_reports r \
                LEFT \
                JOIN media_report_triage t ON t.report_id=r.id \
                WHERE {scope} AND (? IS NULL OR r.status=? OR (?='active' \
                AND r.status IN ('open','in_progress'))) AND (? IS NULL OR r.category=?) \
                AND (? IS NULL OR COALESCE(t.priority,'normal')=?) \
                ORDER BY {order} \
                LIMIT 100 OFFSET ?"
    );
    let rows: Vec<Record> = query_as(sqlx::AssertSqlSafe(sql))
        .bind(actor.user.role.as_str())
        .bind(&actor.user.id)
        .bind(mine)
        .bind(&actor.user.id)
        .bind(&page.status)
        .bind(&page.status)
        .bind(&page.status)
        .bind(&page.category)
        .bind(&page.category)
        .bind(&page.priority)
        .bind(&page.priority)
        .bind(i64::from(page.offset))
        .fetch_all(auth.store.pool())
        .await
        .map_err(db)?;
    let next_offset = (rows.len() == 100).then_some(page.offset + 100);
    let labels = applications::user_labels(
        &auth.store,
        rows.iter().filter_map(|r| r.user_id.as_deref()),
    )
    .await?;
    let mut items = Vec::new();
    for row in rows {
        match read(&auth.store, &actor, &row.id).await {
            Ok(v) => items.push(present(&auth.store, &actor, &v, &labels).await?),
            Err((StatusCode::FORBIDDEN | StatusCode::NOT_FOUND, _)) => {}
            Err(e) => return Err(e),
        }
    }
    // Counts cover the caller's own scope (all reports for an administrator).
    let grouped: Vec<(String, String, i64)> = query_as(sqlx::AssertSqlSafe(format!(
        "SELECT r.status,COALESCE(t.priority,'normal'),COUNT(*) FROM media_reports r LEFT JOIN media_report_triage t ON t.report_id=r.id WHERE {scope} GROUP BY r.status,COALESCE(t.priority,'normal')"
    )))
    .bind(actor.user.role.as_str())
    .bind(&actor.user.id)
    .bind(mine)
    .bind(&actor.user.id)
    .fetch_all(auth.store.pool())
    .await
    .map_err(db)?;
    let mut counts = serde_json::Map::new();
    for status in STATUSES {
        counts.insert(status.into(), json!(0));
    }
    let mut urgent = 0;
    for (status, priority, count) in grouped {
        if let Some(total) = counts.get_mut(&status) {
            *total = json!(total.as_i64().unwrap_or(0) + count);
        }
        if priority == "high" && matches!(status.as_str(), "open" | "in_progress") {
            urgent += count;
        }
    }
    if administrator {
        counts.insert("urgent".into(), json!(urgent));
    }
    Ok(Json(
        json!({"reports":items,"nextOffset":next_offset,"counts":counts}),
    ))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Result<(StatusCode, Json<Value>), Error> {
    let actor = applications::require(&auth, &headers, "reports", true).await?;
    if !valid(&input) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_report"));
    }
    let library = resource(&auth.store, &actor, &input).await?;
    let payload =
        serde_json::to_value(&input).map_err(|_| err(StatusCode::BAD_REQUEST, "invalid_report"))?;
    // Triage is advisory: a busy or older extension leaves the neutral priority.
    let classified = applications::invoke(&auth.store, "reports", "reports.classify", payload)
        .await
        .ok();
    let (priority, code, checks) = triage_from(classified.as_ref());
    let id = Uuid::new_v4().to_string();
    let at = now();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let count: i64 = query_scalar(
        "SELECT COUNT(*) FROM media_reports WHERE user_id=? AND status IN ('open','in_progress')",
    )
    .bind(&actor.user.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    if count >= 100 {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "report_limit_reached"));
    }
    query("INSERT INTO media_reports(id,user_id,library_id,work_id,track_id,asset_id,category,description,status,position_seconds,end_seconds,context_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,'open',?,?,?,?,?)")
        .bind(&id).bind(&actor.user.id).bind(&library).bind(&input.work_id).bind(&input.track_id).bind(&input.asset_id).bind(&input.category).bind(input.description.trim())
        .bind(input.position_seconds).bind(input.end_seconds).bind(serde_json::to_string(&input.context).map_err(|_|err(StatusCode::BAD_REQUEST,"invalid_report"))?).bind(at).bind(at).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.created','open',?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&actor.user.id).bind(at).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO media_report_triage(report_id,priority,code,checks_json,updated_at) VALUES(?,?,?,?,?)")
        .bind(&id).bind(priority).bind(code).bind(json!(checks).to_string()).bind(at).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    let record = read(&auth.store, &actor, &id).await?;
    let labels = applications::user_labels(&auth.store, record.user_id.as_deref()).await?;
    Ok((
        StatusCode::CREATED,
        Json(present(&auth.store, &actor, &record, &labels).await?),
    ))
}
/// id, user_id, author_label, message, created_at
type CommentRow = (String, Option<String>, Option<String>, String, i64);

async fn detail(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, Error> {
    let actor = applications::require(&auth, &headers, "reports", false).await?;
    let record = read(&auth.store, &actor, &id).await?;
    let comments: Vec<CommentRow> =query_as("SELECT id,user_id,author_label,message,created_at FROM media_report_comments WHERE report_id=? ORDER BY created_at,id LIMIT 500").bind(&id).fetch_all(auth.store.pool()).await.map_err(db)?;
    let history:Vec<(String,Option<String>,String,i64)>=query_as("SELECT kind,user_id,status,created_at FROM media_report_events WHERE report_id=? ORDER BY created_at,id LIMIT 500").bind(&id).fetch_all(auth.store.pool()).await.map_err(db)?;
    let labels = applications::user_labels(
        &auth.store,
        record
            .user_id
            .as_deref()
            .into_iter()
            .chain(comments.iter().filter_map(|c| c.1.as_deref()))
            .chain(history.iter().filter_map(|h| h.1.as_deref())),
    )
    .await?;
    // Staff replies are marked so the author can tell them apart from their own.
    let staff: std::collections::BTreeSet<String> = {
        let ids: Vec<&str> = comments.iter().filter_map(|c| c.1.as_deref()).collect();
        let mut staff = std::collections::BTreeSet::new();
        for id in ids {
            let role: Option<String> = query_scalar("SELECT role FROM users WHERE id=?")
                .bind(id)
                .fetch_optional(auth.store.pool())
                .await
                .map_err(db)?;
            if role.as_deref() == Some("administrator") {
                staff.insert(id.to_owned());
            }
        }
        staff
    };
    let mut value = present(&auth.store, &actor, &record, &labels).await?;
    value["comments"]=json!(comments.into_iter().map(|(id,user,author,message,at)|{
        let label = author.or_else(|| user.as_ref().and_then(|u| labels.get(u).cloned()));
        let staff = user.as_ref().is_some_and(|u| staff.contains(u));
        let own = user.as_deref() == Some(actor.user.id.as_str());
        json!({"id":id,"authorLabel":label,"staff":staff,"own":own,"message":message,"createdAt":at})
    }).collect::<Vec<_>>());
    value["history"] = json!(
        history
            .into_iter()
            .map(
                |(kind, user, status, at)| json!({"kind":kind,"status":status,"createdAt":at,
                "actorLabel":user.and_then(|u| labels.get(&u).cloned())})
            )
            .collect::<Vec<_>>()
    );
    Ok(Json(value))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Comment {
    message: String,
}
async fn comment(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Comment>,
) -> Result<StatusCode, Error> {
    let actor = applications::require(&auth, &headers, "reports", true).await?;
    let record = read(&auth.store, &actor, &id).await?;
    if !(1..=8000).contains(&input.message.trim().len()) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_comment"));
    }
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    let count: i64 = query_scalar("SELECT COUNT(*) FROM media_report_comments WHERE report_id=?")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
    if count >= 500 {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "comment_limit_reached"));
    }
    query("INSERT INTO media_report_comments(id,report_id,user_id,message,created_at) VALUES(?,?,?,?,?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&actor.user.id).bind(input.message.trim()).bind(now()).execute(&mut *tx).await.map_err(db)?;
    query("UPDATE media_reports SET updated_at=? WHERE id=?")
        .bind(now())
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.comment',?,?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&actor.user.id).bind(&record.status).bind(now()).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    status: String,
    /// Optional note posted with the transition (resolution, reason for reopening).
    #[serde(default)]
    message: Option<String>,
}
async fn status(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Status>,
) -> Result<StatusCode, Error> {
    let actor = applications::require(&auth, &headers, "reports", true).await?;
    let record = read(&auth.store, &actor, &id).await?;
    if !matches!(
        input.status.as_str(),
        "open" | "in_progress" | "resolved" | "closed"
    ) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_status"));
    }
    if actor.user.role != Role::Administrator
        && !(input.status == "open" && matches!(record.status.as_str(), "closed" | "resolved"))
    {
        return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
    }
    let note = input
        .message
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());
    if note.is_some_and(|m| m.len() > 8000) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_comment"));
    }
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    if let Some(note) = note {
        let count: i64 =
            query_scalar("SELECT COUNT(*) FROM media_report_comments WHERE report_id=?")
                .bind(&id)
                .fetch_one(&mut *tx)
                .await
                .map_err(db)?;
        if count >= 500 {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "comment_limit_reached"));
        }
        query("INSERT INTO media_report_comments(id,report_id,user_id,message,created_at) VALUES(?,?,?,?,?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&actor.user.id).bind(note).bind(now()).execute(&mut *tx).await.map_err(db)?;
    }
    let changed = query("UPDATE media_reports SET status=?,updated_at=? WHERE id=? AND status=?")
        .bind(&input.status)
        .bind(now())
        .bind(&id)
        .bind(&record.status)
        .execute(&mut *tx)
        .await
        .map_err(db)?
        .rows_affected();
    if changed != 1 {
        return Err(err(StatusCode::CONFLICT, "report_changed"));
    }
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.status',?,?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&actor.user.id).bind(&input.status).bind(now()).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Priority {
    priority: String,
}
/// Administrators reorder their queue; the author is not notified of it.
async fn priority(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Priority>,
) -> Result<StatusCode, Error> {
    let actor = applications::require(&auth, &headers, "reports", true).await?;
    if actor.user.role != Role::Administrator {
        return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
    }
    let record = read(&auth.store, &actor, &id).await?;
    if !PRIORITIES.contains(&input.priority.as_str()) {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "invalid_priority"));
    }
    let at = now();
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write(&mut conn).await.map_err(db)?;
    query("INSERT INTO media_report_triage(report_id,priority,checks_json,updated_at) VALUES(?,?,'[]',?) ON CONFLICT(report_id) DO UPDATE SET priority=excluded.priority,updated_at=excluded.updated_at")
        .bind(&id).bind(&input.priority).bind(at).execute(&mut *tx).await.map_err(db)?;
    query("INSERT INTO media_report_events(id,report_id,user_id,kind,status,created_at) VALUES(?,?,?,'report.priority',?,?)").bind(Uuid::now_v7().to_string()).bind(&id).bind(&actor.user.id).bind(&record.status).bind(at).execute(&mut *tx).await.map_err(db)?;
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
