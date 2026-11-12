//! Durable, opt-in notifications from task completion and inventory addition facts.
use crate::{
    auth::AuthState,
    database::{query, query_as, query_scalar},
    identity::{IdentityStore, Role},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests;
mod transport;
type Error = (StatusCode, Json<Value>);
type Outcome<T> = Result<T, Error>;
const LOOKBACK: i64 = 7 * 86400;
const RETENTION: i64 = 30 * 86400;
const LOCK: i64 = 0x4e4f54494659;
fn now() -> i64 {
    media_contracts::protocol::unix_now()
}
fn err(status: StatusCode, code: &str) -> Error {
    (status, Json(json!({"error":code})))
}
fn db(_: sqlx::Error) -> Error {
    err(
        StatusCode::INTERNAL_SERVER_ERROR,
        "notification_storage_error",
    )
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    destination: String,
    smtp_host: String,
    smtp_port: u16,
    sender: String,
    recipient: String,
    username: String,
    secret: Option<String>,
    #[cfg(test)]
    #[serde(skip)]
    local_test: bool,
}
impl Config {
    fn local_test(&self) -> bool {
        #[cfg(test)]
        {
            let targets = transport::test_destinations().lock().unwrap();
            self.local_test
                || targets.contains(&self.destination)
                || targets.contains(&format!("{}:{}", self.smtp_host, self.smtp_port))
        }
        #[cfg(not(test))]
        {
            false
        }
    }
    fn redacted(&self) -> Value {
        json!({"destination":self.destination,"smtpHost":self.smtp_host,"smtpPort":self.smtp_port,"sender":self.sender,"recipient":self.recipient,"username":self.username,"hasSecret":self.secret.as_ref().is_some_and(|s|!s.is_empty())})
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChannelInput {
    name: String,
    recipient_id: String,
    kind: String,
    enabled: bool,
    config: Config,
}
#[derive(sqlx::FromRow)]
struct Channel {
    id: String,
    name: String,
    recipient_id: String,
    kind: String,
    enabled: i64,
    config_json: String,
    generation: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Preferences {
    enabled: bool,
    scans: bool,
    tasks: bool,
    content: bool,
    requests: bool,
    reports: bool,
}
#[derive(Clone, sqlx::FromRow)]
struct Event {
    id: String,
    kind: String,
    task_id: String,
    library_id: Option<String>,
    occurred_at: i64,
}
impl Serialize for Event {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = json!({"id":self.id,"kind":self.kind,"libraryId":self.library_id,"occurredAt":self.occurred_at});
        value[if self.kind.starts_with("request.") {
            "requestId"
        } else if self.kind.starts_with("report.") {
            "reportId"
        } else if self.kind == "content.added" {
            "workId"
        } else {
            "taskId"
        }] = json!(self.task_id);
        value.serialize(serializer)
    }
}
#[derive(sqlx::FromRow)]
struct Delivery {
    id: String,
    event_id: String,
    channel_id: String,
    generation: String,
    attempt: i64,
    lease_token: String,
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/notifications/channels",
            get(channels).post(create),
        )
        .route(
            "/api/v1/admin/notifications/channels/{id}",
            axum::routing::put(save).delete(remove),
        )
        .route("/api/v1/admin/notifications/history", get(history))
        .route(
            "/api/v1/account/notifications",
            get(preferences).put(save_preferences),
        )
        .layer(DefaultBodyLimit::max(16384))
        .layer(
            tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("private, no-store"),
            ),
        )
        .with_state(auth)
}
async fn authorize(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
    administrator: bool,
) -> Outcome<crate::auth::Authenticated> {
    let actor = auth
        .authenticate(headers)
        .await
        .map_err(|s| err(s, "authentication_required"))?;
    if administrator && actor.user.role != Role::Administrator {
        return Err(err(StatusCode::FORBIDDEN, "administrator_required"));
    }
    if mutation {
        auth.require_csrf(headers, &actor)
            .map_err(|s| err(s, "csrf_required"))?;
    }
    Ok(actor)
}
async fn channels(State(auth): State<AuthState>, headers: HeaderMap) -> Outcome<Json<Value>> {
    authorize(&auth, &headers, false, true).await?;
    let rows: Vec<Channel> =
        query_as("SELECT * FROM notification_channels ORDER BY name,id LIMIT 32")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let users: Vec<(String, String,String)> =
        query_as("SELECT u.id,u.role,COALESCE(u.display_name,u.sso_username,c.login,u.id) FROM users u LEFT JOIN local_credentials c ON c.user_id=u.id WHERE u.disabled_at IS NULL ORDER BY u.id LIMIT 1000")
            .fetch_all(auth.store.pool())
            .await
            .map_err(db)?;
    let items:Vec<Value>=rows.into_iter().map(|r| {
        let config:Config=serde_json::from_str(&r.config_json).unwrap_or_default();
        json!({"id":r.id,"name":r.name,"recipientId":r.recipient_id,"kind":r.kind,"enabled":r.enabled==1,"config":config.redacted()})
    }).collect();
    Ok(Json(
        json!({"items":items,"users":users.into_iter().map(|(id,role,name)|json!({"id":id,"role":role,"name":name})).collect::<Vec<_>>(),"events":["task.completed","task.failed","scan.completed","content.added"],"unavailableEvents":["node.unavailable","provider.error"],"retentionDays":30}),
    ))
}
async fn create(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<ChannelInput>,
) -> Outcome<Json<Value>> {
    authorize(&auth, &headers, true, true).await?;
    let id = uuid::Uuid::new_v4().to_string();
    persist_channel(&auth.store, &id, input, false).await?;
    Ok(Json(json!({"id":id})))
}
async fn save(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ChannelInput>,
) -> Outcome<StatusCode> {
    authorize(&auth, &headers, true, true).await?;
    persist_channel(&auth.store, &id, input, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn persist_channel(
    store: &IdentityStore,
    id: &str,
    mut input: ChannelInput,
    update: bool,
) -> Outcome<()> {
    if input.name.trim().is_empty()
        || input.name.len() > 100
        || input.name.chars().any(char::is_control)
    {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_notification_name"));
    }
    if update && input.config.secret.is_none() {
        let previous: Option<(String, String)> =
            query_as("SELECT kind,config_json FROM notification_channels WHERE id=?")
                .bind(id)
                .fetch_optional(store.pool())
                .await
                .map_err(db)?;
        if let Some((kind, previous)) = previous
            && let Ok(previous) = serde_json::from_str::<Config>(&previous)
            && kind == input.kind
            && previous.destination == input.config.destination
            && previous.smtp_host == input.config.smtp_host
        {
            input.config.secret = previous.secret;
        }
    }
    transport::validate(&input.kind, &input.config)
        .map_err(|code| err(StatusCode::BAD_REQUEST, code))?;
    let valid: i64 = query_scalar("SELECT COUNT(*) FROM users WHERE id=? AND disabled_at IS NULL")
        .bind(&input.recipient_id)
        .fetch_one(store.pool())
        .await
        .map_err(db)?;
    if valid != 1 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "notification_recipient_unavailable",
        ));
    }
    let mut conn = store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    if update {
        let changed=query("UPDATE notification_channels SET name=?,recipient_id=?,kind=?,enabled=?,config_json=?,generation=? WHERE id=?")
            .bind(input.name).bind(input.recipient_id).bind(input.kind).bind(i64::from(input.enabled)).bind(json!(input.config).to_string()).bind(uuid::Uuid::new_v4().to_string()).bind(id).execute(&mut *tx).await.map_err(db)?;
        if changed.rows_affected() != 1 {
            return Err(err(StatusCode::NOT_FOUND, "notification_channel_unknown"));
        }
        query("UPDATE notification_deliveries SET status='suppressed',error_code='channel_changed',lease_until=NULL,completed_at=? WHERE channel_id=? AND status='pending'").bind(now()).bind(id).execute(&mut *tx).await.map_err(db)?;
    } else {
        let count: i64 = query_scalar("SELECT COUNT(*) FROM notification_channels")
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
        if count >= 32 {
            return Err(err(StatusCode::CONFLICT, "notification_channel_limit"));
        }
        query("INSERT INTO notification_channels(id,name,recipient_id,kind,enabled,config_json,generation,created_at) VALUES(?,?,?,?,?,?,?,?)")
            .bind(id).bind(input.name).bind(input.recipient_id).bind(input.kind).bind(i64::from(input.enabled)).bind(json!(input.config).to_string()).bind(uuid::Uuid::new_v4().to_string()).bind(now()).execute(&mut *tx).await.map_err(db)?;
    }
    tx.commit().await.map_err(db)
}
async fn remove(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Outcome<StatusCode> {
    authorize(&auth, &headers, true, true).await?;
    query("DELETE FROM notification_channels WHERE id=?")
        .bind(id)
        .execute(auth.store.pool())
        .await
        .map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn preferences(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Outcome<Json<Preferences>> {
    let actor = authorize(&auth, &headers, false, false).await?;
    Ok(Json(
        load_preferences(&auth.store, &actor.user.id)
            .await
            .map_err(db)?,
    ))
}
async fn load_preferences(store: &IdentityStore, user: &str) -> Result<Preferences, sqlx::Error> {
    let applications: Vec<(String, i64)> =
        query_as("SELECT kind,enabled FROM application_notification_preferences WHERE user_id=?")
            .bind(user)
            .fetch_all(store.pool())
            .await?;
    let row: Option<(i64, i64, i64)> =
        query_as("SELECT enabled,scans,tasks FROM notification_preferences WHERE user_id=?")
            .bind(user)
            .fetch_optional(store.pool())
            .await?;
    let content: Option<i64> =
        query_scalar("SELECT enabled FROM notification_content_preferences WHERE user_id=?")
            .bind(user)
            .fetch_optional(store.pool())
            .await?;
    Ok(row
        .map(|(e, s, t)| Preferences {
            enabled: e == 1,
            scans: s == 1,
            tasks: t == 1,
            content: content == Some(1),
            requests: applications
                .iter()
                .any(|(kind, enabled)| kind == "requests" && *enabled == 1),
            reports: applications
                .iter()
                .any(|(kind, enabled)| kind == "reports" && *enabled == 1),
        })
        .unwrap_or_default())
}
async fn save_preferences(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(mut input): Json<Preferences>,
) -> Outcome<StatusCode> {
    let actor = authorize(&auth, &headers, true, false).await?;
    if actor.user.role != Role::Administrator {
        input.tasks = false;
    }
    let mut conn = auth.store.pool().acquire().await.map_err(db)?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK)
        .await
        .map_err(db)?;
    query("INSERT INTO notification_preferences(user_id,enabled,scans,tasks) VALUES(?,?,?,?) ON CONFLICT(user_id) DO UPDATE SET enabled=excluded.enabled,scans=excluded.scans,tasks=excluded.tasks")
        .bind(&actor.user.id).bind(i64::from(input.enabled)).bind(i64::from(input.scans)).bind(i64::from(input.tasks)).execute(&mut *tx).await.map_err(db)?;
    // Starting/restarting a subscription never sends the old catalogue as new content.
    for (kind, subscribed) in [("requests", input.requests), ("reports", input.reports)] {
        query(
            "INSERT INTO application_notification_preferences(user_id,kind,enabled,since_at) \
                VALUES(?,?,?,?) ON CONFLICT(user_id,kind) DO UPDATE \
                SET enabled=excluded.enabled,since_at=CASE \
                WHEN application_notification_preferences.enabled=1 AND excluded.enabled=1 \
                THEN application_notification_preferences.since_at ELSE excluded.since_at END",
        )
        .bind(&actor.user.id)
        .bind(kind)
        .bind(i64::from(input.enabled && subscribed))
        .bind(now())
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    query(
        "INSERT INTO notification_content_preferences(user_id,enabled,since_at) VALUES(?,?,?) \
            ON CONFLICT(user_id) DO UPDATE \
            SET enabled=excluded.enabled,since_at=CASE \
            WHEN notification_content_preferences.enabled=1 AND excluded.enabled=1 \
            THEN notification_content_preferences.since_at ELSE excluded.since_at END",
    )
    .bind(&actor.user.id)
    .bind(i64::from(input.enabled && input.content))
    .bind(now())
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if !input.enabled {
        query("UPDATE notification_deliveries SET status='suppressed',error_code='recipient_opted_out',completed_at=? WHERE status='pending' AND channel_id IN (SELECT id FROM notification_channels WHERE recipient_id=?)").bind(now()).bind(&actor.user.id).execute(&mut *tx).await.map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn history(State(auth): State<AuthState>, headers: HeaderMap) -> Outcome<Json<Value>> {
    authorize(&auth, &headers, false, true).await?;
    let rows:Vec<HistoryRow>=query_as("SELECT d.id,d.event_id,d.channel_id,d.status,d.attempt,d.error_code,e.occurred_at FROM notification_deliveries d JOIN notification_events e ON e.id=d.event_id ORDER BY e.occurred_at DESC,d.id LIMIT 100")
        .fetch_all(auth.store.pool()).await.map_err(db)?;
    Ok(Json(json!({"items":rows})))
}

#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
struct HistoryRow {
    id: String,
    event_id: String,
    channel_id: String,
    status: String,
    attempt: i64,
    error_code: Option<String>,
    occurred_at: i64,
}

async fn recipient_allowed(
    store: &IdentityStore,
    channel: &Channel,
    event: &Event,
) -> Result<bool, sqlx::Error> {
    let role: Option<String> =
        query_scalar("SELECT role FROM users WHERE id=? AND disabled_at IS NULL")
            .bind(&channel.recipient_id)
            .fetch_optional(store.pool())
            .await?;
    let Some(role) = role else { return Ok(false) };
    let prefs = load_preferences(store, &channel.recipient_id).await?;
    if !prefs.enabled {
        return Ok(false);
    }
    let application = application_notifications::kind(&event.kind);
    if let Some(cap) = application
        && !application_notifications::allowed(store, channel, event, &role, cap).await?
    {
        return Ok(false);
    }
    if event.kind != "scan.completed" && event.kind != "content.added" && application.is_none() {
        return Ok(role == "administrator" && prefs.tasks);
    }
    if event.kind == "scan.completed" && !prefs.scans {
        return Ok(false);
    }
    if event.kind == "content.added" {
        let subscribed: i64 = query_scalar("SELECT COUNT(*) FROM notification_content_preferences WHERE user_id=? AND enabled=1 AND since_at<?")
            .bind(&channel.recipient_id).bind(event.occurred_at).fetch_one(store.pool()).await?;
        if !prefs.content || subscribed != 1 {
            return Ok(false);
        }
        let source: i64 = query_scalar("SELECT COUNT(*) FROM inventory_works WHERE id=? AND library_id=? AND has_available_asset=1 AND kind IN ('film','series','episode')")
            .bind(&event.task_id).bind(&event.library_id).fetch_one(store.pool()).await?;
        if source != 1
            || !crate::product::parental_allowed(store, &channel.recipient_id, &event.task_id)
                .await
                .unwrap_or(false)
        {
            return Ok(false);
        }
    }
    let Some(library) = event.library_id.as_ref() else {
        return Ok(false);
    };
    let count: i64 = query_scalar(
        "SELECT COUNT(*) \
            FROM libraries l \
            JOIN media_nodes n ON n.id=l.node_id \
            WHERE l.id=? AND l.enabled=1 AND n.status!='revoked' AND ((?='administrator' \
            AND NOT EXISTS(SELECT 1 FROM libraries hx LEFT JOIN library_folders hxf \
            ON hxf.library_id=hx.id AND hxf.active=1 LEFT JOIN libraries hxp \
            ON hxp.id=hxf.parent_id WHERE hx.id=l.id \
            AND COALESCE(hxp.options_json,hx.options_json) LIKE '%adminAccess__false%')) \
            OR EXISTS(SELECT 1 FROM effective_library_permissions p WHERE p.library_id=l.id \
            AND p.user_id=?)) AND NOT EXISTS(SELECT 1 FROM library_folders f JOIN libraries parent \
            ON parent.id=f.parent_id WHERE f.library_id=l.id AND (f.active=0 OR parent.enabled=0))",
    )
    .bind(library)
    .bind(role)
    .bind(&channel.recipient_id)
    .fetch_one(store.pool())
    .await?;
    Ok(count == 1)
}

async fn still_allowed(
    store: &IdentityStore,
    channel: &Channel,
    event: &Event,
) -> Result<bool, sqlx::Error> {
    let active: i64 = query_scalar(
        "SELECT COUNT(*) FROM notification_channels WHERE id=? AND enabled=1 AND generation=?",
    )
    .bind(&channel.id)
    .bind(&channel.generation)
    .fetch_one(store.pool())
    .await?;
    Ok(active == 1 && recipient_allowed(store, channel, event).await?)
}

/// Bounded reconciliation of durable task facts. No raw errors, titles, paths or provider data leave the Core.
pub(crate) async fn collect(store: &IdentityStore, timestamp: i64) -> Result<(), sqlx::Error> {
    let rows: Vec<(String, String, String, Option<String>, i64, i64)> = query_as(
        "SELECT id,status,category,library_id,attempt,finished_at \
            FROM durable_tasks j \
            WHERE status IN ('completed','error') AND finished_at>? AND NOT EXISTS(SELECT 1 \
            FROM notification_events e WHERE e.task_id=j.id AND e.occurred_at=j.finished_at \
            AND e.source_attempt=j.attempt AND e.source_status=j.status) \
            ORDER BY finished_at,id \
            LIMIT 100",
    )
    .bind(timestamp - LOOKBACK)
    .fetch_all(store.pool())
    .await?;
    for (task, status, category, library, attempt, at) in rows {
        let kind = if status == "error" {
            "task.failed"
        } else if category == "scan" {
            "scan.completed"
        } else {
            "task.completed"
        };
        let id = format!(
            "event-{:x}",
            Sha256::digest(format!("{task}:{attempt}:{status}:{at}"))
        );
        let event = Event {
            id,
            kind: kind.into(),
            task_id: task,
            library_id: library,
            occurred_at: at,
        };
        enqueue(store, timestamp, &event, attempt, &status).await?;
    }
    // Only additions after a subscription started qualify; existing catalogue is not announced.
    let content: Vec<(String, String, i64)> = query_as("SELECT w.id,w.library_id,w.added_at \
            FROM inventory_works w \
            WHERE w.added_at>? AND w.added_at<=? AND w.kind IN ('film','series','episode') \
            AND w.has_available_asset=1 AND EXISTS(SELECT 1 FROM notification_content_preferences p \
            JOIN notification_preferences base ON base.user_id=p.user_id WHERE p.enabled=1 \
            AND base.enabled=1 AND p.since_at<w.added_at) AND NOT EXISTS(SELECT 1 \
            FROM notification_events e WHERE e.kind='content.added' AND e.task_id=w.id \
            AND e.occurred_at=w.added_at) \
            ORDER BY w.added_at,w.id \
            LIMIT 100")
        .bind(timestamp-LOOKBACK).bind(timestamp).fetch_all(store.pool()).await?;
    for (work, library, at) in content {
        let event = Event {
            id: format!("event-{:x}", Sha256::digest(format!("content:{work}:{at}"))),
            kind: "content.added".into(),
            task_id: work,
            library_id: Some(library),
            occurred_at: at,
        };
        enqueue(store, timestamp, &event, 0, "added").await?;
    }
    application_notifications::collect(store, timestamp).await?;
    query("DELETE FROM notification_events WHERE occurred_at<?")
        .bind(timestamp - RETENTION)
        .execute(store.pool())
        .await?;
    Ok(())
}

mod application_notifications;

async fn enqueue(
    store: &IdentityStore,
    timestamp: i64,
    event: &Event,
    attempt: i64,
    status: &str,
) -> Result<(), sqlx::Error> {
    let mut conn = store.pool().acquire().await?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK).await?;
    let inserted = query(
        "INSERT INTO notification_events(id,kind,task_id,library_id,occurred_at,source_attempt,\
            source_status) \
            VALUES(?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING",
    )
    .bind(&event.id)
    .bind(&event.kind)
    .bind(&event.task_id)
    .bind(&event.library_id)
    .bind(event.occurred_at)
    .bind(attempt)
    .bind(status)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 1 {
        let channels: Vec<(String, String)> = query_as(
            "SELECT id,generation \
                FROM notification_channels c \
                WHERE enabled=1 AND created_at<=? AND (?!='content.added' OR EXISTS(SELECT 1 \
                FROM notification_content_preferences p JOIN notification_preferences base \
                ON base.user_id=p.user_id WHERE p.user_id=c.recipient_id AND p.enabled=1 \
                AND base.enabled=1 AND p.since_at<?)) \
                ORDER BY id \
                LIMIT 32",
        )
        .bind(event.occurred_at)
        .bind(&event.kind)
        .bind(event.occurred_at)
        .fetch_all(&mut *tx)
        .await?;
        for (id, generation) in channels {
            // Audience is rechecked before delivery as well; queued payloads contain no metadata.
            let pending:i64=query_scalar("SELECT COUNT(*) FROM notification_deliveries WHERE channel_id=? AND status IN ('pending','sending')").bind(&id).fetch_one(&mut *tx).await?;
            let (state, error) = if pending >= 100 {
                ("suppressed", Some("notification_queue_limit"))
            } else {
                ("pending", None)
            };
            query("INSERT INTO notification_deliveries(id,event_id,channel_id,generation,status,available_at,error_code,completed_at) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(event_id,channel_id) DO NOTHING")
                    .bind(uuid::Uuid::new_v4().to_string()).bind(&event.id).bind(id).bind(generation).bind(state).bind(timestamp).bind(error).bind((state=="suppressed").then_some(timestamp)).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

async fn claim(store: &IdentityStore, timestamp: i64) -> Result<Option<Delivery>, sqlx::Error> {
    let mut conn = store.pool().acquire().await?;
    let mut tx = crate::database::begin_write_serialized(&mut conn, LOCK).await?;
    // An ambiguous crash after a remote acceptance can duplicate delivery. Stable event IDs permit receiver deduplication.
    query("UPDATE notification_deliveries SET status=CASE WHEN attempt>=5 THEN 'failed' ELSE 'pending' END,available_at=?,lease_until=NULL,lease_token=NULL,error_code='delivery_interrupted' WHERE status='sending' AND lease_until<=?")
        .bind(timestamp+30).bind(timestamp).execute(&mut *tx).await?;
    let id:Option<String>=query_scalar("SELECT d.id FROM notification_deliveries d JOIN notification_channels c ON c.id=d.channel_id WHERE d.status='pending' AND d.available_at<=? AND c.next_allowed_at<=? ORDER BY d.available_at,d.id LIMIT 1")
        .bind(timestamp).bind(timestamp).fetch_optional(&mut *tx).await?;
    let Some(id) = id else {
        tx.commit().await?;
        return Ok(None);
    };
    let token = uuid::Uuid::new_v4().to_string();
    let row:Delivery=query_as("UPDATE notification_deliveries SET status='sending',attempt=attempt+1,lease_until=?,lease_token=? WHERE id=? RETURNING id,event_id,channel_id,generation,attempt,lease_token")
        .bind(timestamp+30).bind(token).bind(id).fetch_one(&mut *tx).await?;
    query("UPDATE notification_channels SET next_allowed_at=? WHERE id=?")
        .bind(timestamp + 60)
        .bind(&row.channel_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(row))
}

pub(crate) async fn run_once(store: &IdentityStore, timestamp: i64) -> Result<bool, sqlx::Error> {
    collect(store, timestamp).await?;
    let Some(delivery) = claim(store, timestamp).await? else {
        return Ok(false);
    };
    let channel: Option<Channel> =
        query_as("SELECT * FROM notification_channels WHERE id=? AND enabled=1 AND generation=?")
            .bind(&delivery.channel_id)
            .bind(&delivery.generation)
            .fetch_optional(store.pool())
            .await?;
    let event: Option<Event> = query_as("SELECT * FROM notification_events WHERE id=?")
        .bind(&delivery.event_id)
        .fetch_optional(store.pool())
        .await?;
    let mut result = Err("notification_revoked");
    let mut suppressed = true;
    if let (Some(channel), Some(event)) = (channel, event)
        && recipient_allowed(store, &channel, &event).await?
    {
        suppressed = false;
        result = match serde_json::from_str::<Config>(&channel.config_json) {
            Ok(config) => {
                transport::deliver_checked(&channel.kind, &config, &event, store, &channel).await
            }
            Err(_) => Err("notification_invalid_config"),
        };
    }
    let (status, error) = match result {
        Ok(()) => ("sent", None),
        Err(code) if suppressed || code == "notification_revoked" => ("suppressed", Some(code)),
        Err(code)
            if delivery.attempt >= 5
                || matches!(
                    code,
                    "notification_destination_denied"
                        | "notification_invalid_config"
                        | "notification_auth_denied"
                ) =>
        {
            ("failed", Some(code))
        }
        Err(code) => ("pending", Some(code)),
    };
    let delay = 30 * 2_i64.pow(delivery.attempt.clamp(0, 5) as u32);
    query("UPDATE notification_deliveries SET status=?,error_code=?,available_at=?,completed_at=?,lease_until=NULL,lease_token=NULL WHERE id=? AND status='sending' AND lease_token=? AND lease_until>?")
        .bind(status).bind(error).bind(timestamp+delay).bind((status!="pending").then_some(timestamp)).bind(delivery.id).bind(delivery.lease_token).bind(now()).execute(store.pool()).await?;
    Ok(true)
}

pub(crate) async fn tick(store: &IdentityStore) -> Result<(), sqlx::Error> {
    // Sequential bounded sends keep notifications away from playback CPU and sockets.
    let _ = run_once(store, now()).await?;
    Ok(())
}
