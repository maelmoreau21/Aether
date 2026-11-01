#![allow(clippy::type_complexity, clippy::collapsible_if)]

//! Synchronized Community Watch Parties (SyncPlay) for Aether.
//! Media playback coordination runs on Core via WebSockets while video/audio streaming remains strictly on Media Nodes.

use crate::{
    auth::{AuthState, Authenticated},
    database::{begin_write, query, query_as, query_scalar},
    identity::{IdentityStore, Role},
    product::parental_allowed,
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

type Error = StatusCode;

#[derive(Debug)]
pub struct PartyApiError(pub StatusCode, pub String);

impl IntoResponse for PartyApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<StatusCode> for PartyApiError {
    fn from(s: StatusCode) -> Self {
        let msg = match s {
            StatusCode::FORBIDDEN => "Accès refusé ou permissions insuffisantes.",
            StatusCode::NOT_FOUND => "Séance ou ressource introuvable.",
            StatusCode::BAD_REQUEST => "Données de requête invalides.",
            StatusCode::GONE => "La séance ou l'invitation a expiré ou est terminée.",
            StatusCode::UNAUTHORIZED => "Authentification requise.",
            _ => "Une erreur interne est survenue.",
        };
        Self(s, msg.to_string())
    }
}

const MAX_CHAT_LENGTH: usize = 500;
const MAX_STORED_MESSAGES: usize = 100;
const BROADCAST_CAPACITY: usize = 128;
const DEFAULT_INVITE_EXPIRY_SECONDS: i64 = 86400; // 24 hours

#[inline]
fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[inline]
fn current_epoch_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlMode {
    HostOnly,
    Shared,
    Request,
}

impl ControlMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HostOnly => "host_only",
            Self::Shared => "shared",
            Self::Request => "request",
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "shared" => Self::Shared,
            "request" => Self::Request,
            _ => Self::HostOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartyState {
    Waiting,
    Playing,
    Paused,
    Ended,
}

impl PartyState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Ended => "ended",
        }
    }
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "playing" => Self::Playing,
            "paused" => Self::Paused,
            "ended" => Self::Ended,
            _ => Self::Waiting,
        }
    }
}

// ---------------------------------------------------------------------------
// Data transfer objects
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartySummary {
    pub id: String,
    pub name: String,
    pub host_user_id: String,
    pub host_name: String,
    pub media_id: String,
    pub media_title: String,
    pub control_mode: String,
    pub state: String,
    pub current_position: f64,
    pub is_locked: bool,
    pub wait_for_buffering: bool,
    pub members_count: usize,
    pub created_at: i64,
    pub is_host: bool,
    pub is_member: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberPayload {
    pub user_id: String,
    pub display_name: String,
    pub role: String,
    pub is_ready: bool,
    pub is_buffering: bool,
    pub is_online: bool,
    pub joined_at: i64,
    pub has_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessagePayload {
    pub id: String,
    pub user_id: String,
    pub display_name: String,
    pub content: String,
    pub kind: String, // "chat" | "system" | "reaction"
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItemPayload {
    pub id: String,
    pub media_id: String,
    pub media_title: String,
    pub added_by_user_id: String,
    pub added_by_name: String,
    pub position: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollOptionPayload {
    pub text: String,
    pub votes: usize,
    pub voter_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollPayload {
    pub id: String,
    pub question: String,
    pub options: Vec<PollOptionPayload>,
    pub created_by: String,
    pub is_closed: bool,
    pub created_at: i64,
    pub total_votes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartyStatePayload {
    pub id: String,
    pub name: String,
    pub host_user_id: String,
    pub host_name: String,
    pub media_id: String,
    pub media_title: String,
    pub control_mode: String,
    pub state: String,
    pub position: f64,
    pub position_updated_server_ms: i64,
    pub is_locked: bool,
    pub wait_for_buffering: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClientMessage {
    Ping {
        client_epoch_ms: i64,
    },
    SetReady {
        ready: bool,
    },
    SetBuffering {
        buffering: bool,
        position: f64,
    },
    Play {
        position: f64,
        countdown: Option<u32>,
    },
    Pause {
        position: f64,
    },
    Seek {
        position: f64,
    },
    ReportPosition {
        position: f64,
    },
    ChatMessage {
        content: String,
    },
    Reaction {
        emoji: String,
    },
    RequestControl,
    GrantControl {
        user_id: String,
    },
    RevokeControl {
        user_id: String,
    },
    ChangeMedia {
        media_id: String,
    },
    AddToQueue {
        media_id: String,
    },
    RemoveFromQueue {
        queue_id: String,
    },
    CreatePoll {
        question: String,
        options: Vec<String>,
    },
    VotePoll {
        poll_id: String,
        option_index: usize,
    },
    ClosePoll {
        poll_id: String,
    },
    KickMember {
        user_id: String,
    },
    TransferHost {
        new_host_id: String,
    },
    SetSettings {
        name: Option<String>,
        control_mode: Option<String>,
        is_locked: Option<bool>,
        wait_for_buffering: Option<bool>,
    },
    LeaveParty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum ServerMessage {
    Pong {
        client_epoch_ms: i64,
        server_epoch_ms: i64,
    },
    SyncState {
        party: PartyStatePayload,
        members: Vec<MemberPayload>,
        messages: Vec<ChatMessagePayload>,
        queue: Vec<QueueItemPayload>,
        poll: Option<PollPayload>,
        server_epoch_ms: i64,
        seq: u64,
    },
    PlaybackUpdate {
        state: String,
        position: f64,
        server_epoch_ms: i64,
        caused_by_user_id: String,
        caused_by_name: String,
        seq: u64,
    },
    Countdown {
        seconds: u32,
        target_server_epoch_ms: i64,
        position: f64,
        caused_by_user_id: String,
        caused_by_name: String,
        seq: u64,
    },
    MemberJoined {
        member: MemberPayload,
    },
    MemberLeft {
        user_id: String,
        display_name: String,
        reason: Option<String>,
    },
    MemberStatus {
        user_id: String,
        is_ready: bool,
        is_buffering: bool,
        position: f64,
    },
    HostTransferred {
        new_host_id: String,
        new_host_name: String,
    },
    ControlGranted {
        user_id: String,
    },
    ControlRevoked {
        user_id: String,
    },
    ControlRequested {
        user_id: String,
        display_name: String,
    },
    Kicked {
        user_id: String,
    },
    NewMessage {
        message: ChatMessagePayload,
    },
    NewReaction {
        user_id: String,
        display_name: String,
        emoji: String,
    },
    QueueUpdated {
        queue: Vec<QueueItemPayload>,
    },
    PollUpdated {
        poll: Option<PollPayload>,
    },
    MediaChanged {
        media_id: String,
        media_title: String,
        caused_by_name: String,
    },
    PartyEnded {
        reason: String,
    },
    Error {
        code: String,
        message: String,
    },
}

// ---------------------------------------------------------------------------
// In-Memory Real-Time Room & Hub
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LiveParticipant {
    pub user_id: String,
    pub display_name: String,
    pub role: String,
    pub is_ready: bool,
    pub is_buffering: bool,
    pub last_position: f64,
    pub last_seen_ms: i64,
    pub joined_at: i64,
}

pub struct PartyRoom {
    pub id: String,
    pub name: String,
    pub host_user_id: String,
    pub media_id: String,
    pub media_title: String,
    pub control_mode: ControlMode,
    pub state: PartyState,
    pub position_s: f64,
    pub position_updated_ms: i64,
    pub is_locked: bool,
    pub wait_for_buffering: bool,
    pub created_at: i64,
    pub active_controllers: HashSet<String>,
    pub participants: HashMap<String, LiveParticipant>,
    pub online_user_ids: HashSet<String>,
    pub messages: VecDeque<ChatMessagePayload>,
    pub queue: Vec<QueueItemPayload>,
    pub current_poll: Option<PollPayload>,
    pub seq: u64,
    pub broadcast_tx: broadcast::Sender<ServerMessage>,
}

impl PartyRoom {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: String,
        host_user_id: String,
        media_id: String,
        media_title: String,
        control_mode: ControlMode,
        wait_for_buffering: bool,
        created_at: i64,
    ) -> (Self, broadcast::Receiver<ServerMessage>) {
        let (broadcast_tx, rx) = broadcast::channel(BROADCAST_CAPACITY);
        let room = Self {
            id,
            name,
            host_user_id,
            media_id,
            media_title,
            control_mode,
            state: PartyState::Waiting,
            position_s: 0.0,
            position_updated_ms: current_epoch_ms(),
            is_locked: false,
            wait_for_buffering,
            created_at,
            active_controllers: HashSet::new(),
            participants: HashMap::new(),
            online_user_ids: HashSet::new(),
            messages: VecDeque::with_capacity(MAX_STORED_MESSAGES),
            queue: Vec::new(),
            current_poll: None,
            seq: 1,
            broadcast_tx,
        };
        (room, rx)
    }

    /// Calculate the current live playhead position based on server time.
    pub fn computed_position(&self) -> f64 {
        if self.state == PartyState::Playing {
            let elapsed = (current_epoch_ms() - self.position_updated_ms) as f64 / 1000.0;
            (self.position_s + elapsed).max(0.0)
        } else {
            self.position_s
        }
    }

    pub fn can_user_control(&self, user_id: &str) -> bool {
        if user_id == self.host_user_id {
            return true;
        }
        match self.control_mode {
            ControlMode::Shared => true,
            ControlMode::Request => self.active_controllers.contains(user_id),
            ControlMode::HostOnly => false,
        }
    }

    pub fn to_state_payload(&self, host_name: String) -> PartyStatePayload {
        PartyStatePayload {
            id: self.id.clone(),
            name: self.name.clone(),
            host_user_id: self.host_user_id.clone(),
            host_name,
            media_id: self.media_id.clone(),
            media_title: self.media_title.clone(),
            control_mode: self.control_mode.as_str().to_string(),
            state: self.state.as_str().to_string(),
            position: self.computed_position(),
            position_updated_server_ms: self.position_updated_ms,
            is_locked: self.is_locked,
            wait_for_buffering: self.wait_for_buffering,
            created_at: self.created_at,
        }
    }

    pub fn members_payload(&self) -> Vec<MemberPayload> {
        self.participants
            .values()
            .map(|p| MemberPayload {
                user_id: p.user_id.clone(),
                display_name: p.display_name.clone(),
                role: p.role.clone(),
                is_ready: p.is_ready,
                is_buffering: p.is_buffering,
                is_online: self.online_user_ids.contains(&p.user_id),
                joined_at: p.joined_at,
                has_control: self.can_user_control(&p.user_id),
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct WatchPartyHub {
    pub store: IdentityStore,
    rooms: Arc<RwLock<HashMap<String, Arc<RwLock<PartyRoom>>>>>,
}

impl WatchPartyHub {
    pub fn new(store: IdentityStore) -> Self {
        Self {
            store,
            rooms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Retrieve or load an active room from DB into memory.
    pub async fn get_or_load_room(&self, party_id: &str) -> Option<Arc<RwLock<PartyRoom>>> {
        {
            let rooms = self.rooms.read().await;
            if let Some(room) = rooms.get(party_id) {
                return Some(room.clone());
            }
        }

        // Query database to check if party exists and is not ended
        let row: Option<(String, String, String, String, String, String, i64, i64, i64, i64, i64)> =
            query_as(
                "SELECT id, name, host_user_id, media_id, control_mode, state, \
                 current_position_ms, position_updated_at_ms, is_locked, wait_for_buffering, created_at \
                 FROM watch_parties WHERE id = ? AND ended_at IS NULL",
            )
            .bind(party_id)
            .fetch_optional(&self.store.pool)
            .await
            .ok()?;

        let (
            id,
            name,
            host_user_id,
            media_id,
            control_mode,
            state,
            pos_ms,
            pos_updated_ms,
            is_locked,
            wait_for_buffering,
            created_at,
        ) = row?;

        // Query media title
        let media_title: String = query_scalar("SELECT title FROM inventory_works WHERE id = ?")
            .bind(&media_id)
            .fetch_optional(&self.store.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "Séance Aether".to_string());

        let (mut room, _) = PartyRoom::new(
            id.clone(),
            name,
            host_user_id,
            media_id,
            media_title,
            ControlMode::from_str(&control_mode),
            wait_for_buffering != 0,
            created_at,
        );
        room.state = PartyState::from_str(&state);
        room.position_s = (pos_ms as f64) / 1000.0;
        room.position_updated_ms = pos_updated_ms;
        room.is_locked = is_locked != 0;

        // Load members from DB
        let members: Vec<(String, String, String, i64, i64, i64)> = query_as(
            "SELECT m.user_id, COALESCE(u.display_name, u.sso_username, m.user_id), m.role, m.is_ready, m.is_buffering, m.joined_at \
             FROM watch_party_members m \
             JOIN users u ON u.id = m.user_id \
             WHERE m.party_id = ?",
        )
        .bind(party_id)
        .fetch_all(&self.store.pool)
        .await
        .unwrap_or_default();

        for (u_id, disp_name, role, ready, buffering, joined) in members {
            room.participants.insert(
                u_id.clone(),
                LiveParticipant {
                    user_id: u_id,
                    display_name: disp_name,
                    role,
                    is_ready: ready != 0,
                    is_buffering: buffering != 0,
                    last_position: room.position_s,
                    last_seen_ms: current_epoch_ms(),
                    joined_at: joined,
                },
            );
        }

        // Load recent messages
        let msgs: Vec<(String, String, String, String, String, i64)> = query_as(
            "SELECT m.id, m.user_id, COALESCE(u.display_name, u.sso_username, m.user_id), m.content, m.kind, m.created_at \
             FROM watch_party_messages m \
             JOIN users u ON u.id = m.user_id \
             WHERE m.party_id = ? \
             ORDER BY m.created_at DESC LIMIT 50",
        )
        .bind(party_id)
        .fetch_all(&self.store.pool)
        .await
        .unwrap_or_default();

        for (m_id, u_id, u_name, content, kind, created) in msgs.into_iter().rev() {
            room.messages.push_back(ChatMessagePayload {
                id: m_id,
                user_id: u_id,
                display_name: u_name,
                content,
                kind,
                created_at: created,
            });
        }

        // Load queue
        let queue_items: Vec<(String, String, String, String, String, i64, i64)> = query_as(
            "SELECT q.id, q.media_id, COALESCE(w.title, 'Média'), q.added_by, COALESCE(u.display_name, u.sso_username, q.added_by), q.position, q.created_at \
             FROM watch_party_queue q \
             LEFT JOIN inventory_works w ON w.id = q.media_id \
             LEFT JOIN users u ON u.id = q.added_by \
             WHERE q.party_id = ? \
             ORDER BY q.position ASC",
        )
        .bind(party_id)
        .fetch_all(&self.store.pool)
        .await
        .unwrap_or_default();

        for (q_id, m_id, m_title, added_by, added_name, pos, created) in queue_items {
            room.queue.push(QueueItemPayload {
                id: q_id,
                media_id: m_id,
                media_title: m_title,
                added_by_user_id: added_by,
                added_by_name: added_name,
                position: pos,
                created_at: created,
            });
        }

        let room_arc = Arc::new(RwLock::new(room));
        let mut rooms = self.rooms.write().await;
        rooms.insert(party_id.to_string(), room_arc.clone());
        Some(room_arc)
    }

    /// Remove ended or abandoned rooms from memory.
    pub async fn evict_room(&self, party_id: &str) {
        let mut rooms = self.rooms.write().await;
        rooms.remove(party_id);
    }
}

// ---------------------------------------------------------------------------
// Authorization helper: Verifies library access and parental controls.
// ---------------------------------------------------------------------------

pub async fn can_user_access_media(
    store: &IdentityStore,
    user_id: &str,
    user_role: &Role,
    media_id: &str,
) -> bool {
    let raw_id = media_id.trim();
    if raw_id.is_empty() {
        return false;
    }

    // Resolve if raw_id is an episode ID
    let resolved_work_id: String =
        query_scalar("SELECT COALESCE((SELECT work_id FROM inventory_episodes WHERE id = ?), ?)")
            .bind(raw_id)
            .bind(raw_id)
            .fetch_optional(&store.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| raw_id.to_string());

    // 1. Check parental policy on resolved work
    let parental_ok = parental_allowed(store, user_id, &resolved_work_id)
        .await
        .unwrap_or(false);
    if !parental_ok {
        return false;
    }

    // 2. Check library permission on resolved work
    let allowed: Option<i64> = query_scalar(
        "SELECT COUNT(*) FROM inventory_works w \
         JOIN libraries l ON l.id = w.library_id \
         JOIN media_nodes n ON n.id = l.node_id \
         WHERE w.id = ? AND l.enabled = 1 AND n.status != 'revoked' \
         AND (? = 'administrator' OR EXISTS( \
             SELECT 1 FROM effective_library_permissions p \
             WHERE p.library_id = l.id AND p.user_id = ? \
         ))",
    )
    .bind(&resolved_work_id)
    .bind(user_role.as_str())
    .bind(user_id)
    .fetch_optional(&store.pool)
    .await
    .unwrap_or(None);

    allowed.unwrap_or(0) > 0
}

// ---------------------------------------------------------------------------
// Router & HTTP Endpoints
// ---------------------------------------------------------------------------

pub fn router(auth: AuthState) -> Router {
    let hub = WatchPartyHub::new(auth.store.clone());
    Router::new()
        .route("/api/v1/parties", get(list_parties).post(create_party))
        .route("/api/v1/parties/invites", get(list_user_invites))
        .route(
            "/api/v1/parties/invites/{token}/accept",
            post(accept_invite),
        )
        .route("/api/v1/parties/eligible-users", get(eligible_users))
        .route(
            "/api/v1/parties/{id}",
            get(get_party).put(update_party_settings),
        )
        .route("/api/v1/parties/{id}/join", post(join_party))
        .route("/api/v1/parties/{id}/leave", post(leave_party))
        .route("/api/v1/parties/{id}/end", post(end_party))
        .route(
            "/api/v1/parties/{id}/invites",
            get(list_party_invites).post(create_party_invite),
        )
        .route(
            "/api/v1/parties/{id}/invites/{invite_id}",
            delete(revoke_party_invite),
        )
        .route("/api/v1/parties/{id}/ws", get(ws_handler))
        .layer(DefaultBodyLimit::max(65536))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        ))
        .with_state((auth, hub))
}

// ---------------------------------------------------------------------------
// HTTP Handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePartyInput {
    pub name: Option<String>,
    pub media_id: String,
    pub control_mode: Option<String>,
    pub wait_for_buffering: Option<bool>,
    pub invite_user_ids: Option<Vec<String>>,
    pub invite_usernames: Option<Vec<String>>,
}

async fn create_party(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    headers: HeaderMap,
    Json(input): Json<CreatePartyInput>,
) -> Result<Json<Value>, PartyApiError> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let media_id = input.media_id.trim();
    if media_id.is_empty() {
        return Err(PartyApiError(
            StatusCode::BAD_REQUEST,
            "L'identifiant du média est obligatoire.".to_string(),
        ));
    }

    // Verify actor can access media
    if !can_user_access_media(&auth.store, &actor.user.id, &actor.user.role, media_id).await {
        return Err(PartyApiError(
            StatusCode::FORBIDDEN,
            "Accès au média refusé par le contrôle parental ou les autorisations de bibliothèque."
                .to_string(),
        ));
    }

    // Get media title (works for either a movie work ID or an episode ID)
    let media_title: String = query_scalar(
        "SELECT COALESCE( \
            (SELECT w.title || ' — ' || COALESCE(ep.title, 'Épisode ' || ep.episode_number) \
             FROM inventory_episodes ep JOIN inventory_works w ON w.id = ep.work_id WHERE ep.id = ?), \
            (SELECT title FROM inventory_works WHERE id = ?) \
         )",
    )
    .bind(media_id)
    .bind(media_id)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .unwrap_or_else(|| "Séance".to_string());

    let party_name = match input.name {
        Some(ref n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => format!("Séance : {media_title}"),
    };

    let control_mode = input
        .control_mode
        .as_deref()
        .map(ControlMode::from_str)
        .unwrap_or(ControlMode::HostOnly);
    let wait_buffering = input.wait_for_buffering.unwrap_or(true);

    let party_id = Uuid::new_v4().to_string();
    let now = current_epoch_s();
    let now_ms = current_epoch_ms();

    let mut conn = auth
        .store
        .pool
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut tx = begin_write(&mut conn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    query(
        "INSERT INTO watch_parties (id, name, host_user_id, media_id, control_mode, state, current_position_ms, position_updated_at_ms, is_locked, wait_for_buffering, created_at) \
         VALUES (?, ?, ?, ?, ?, 'waiting', 0, ?, 0, ?, ?)",
    )
    .bind(&party_id)
    .bind(&party_name)
    .bind(&actor.user.id)
    .bind(media_id)
    .bind(control_mode.as_str())
    .bind(now_ms)
    .bind(i64::from(wait_buffering))
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Add host as member
    query(
        "INSERT INTO watch_party_members (party_id, user_id, role, is_ready, is_buffering, joined_at, last_seen_at) \
         VALUES (?, ?, 'host', 1, 0, ?, ?)",
    )
    .bind(&party_id)
    .bind(&actor.user.id)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Collect all target user IDs (from explicit user IDs + resolved usernames/pseudos)
    let mut target_ids: HashSet<String> = HashSet::new();

    if let Some(invitees) = input.invite_user_ids {
        for target_id in invitees {
            let t = target_id.trim();
            if !t.is_empty() && t != actor.user.id {
                target_ids.insert(t.to_string());
            }
        }
    }

    if let Some(usernames) = input.invite_usernames {
        for uname in usernames {
            let clean = uname.trim().trim_start_matches('@').to_lowercase();
            if clean.is_empty() {
                continue;
            }
            let found_id: Option<String> = query_scalar(
                "SELECT u.id FROM users u \
                 LEFT JOIN local_credentials c ON c.user_id = u.id \
                 WHERE lower(COALESCE(u.sso_username, c.login, u.id)) = ? \
                    OR lower(COALESCE(u.display_name, '')) = ?",
            )
            .bind(&clean)
            .bind(&clean)
            .fetch_optional(&mut *tx)
            .await
            .unwrap_or(None);

            if let Some(uid) = found_id {
                if uid != actor.user.id {
                    target_ids.insert(uid);
                }
            }
        }
    }

    // Handle initial invites
    for target_id in target_ids {
        // Check target user role
        let target_role: Option<String> = query_scalar("SELECT role FROM users WHERE id = ?")
            .bind(&target_id)
            .fetch_optional(&mut *tx)
            .await
            .unwrap_or(None);

        if let Some(r_str) = target_role {
            let r = if r_str == "administrator" {
                Role::Administrator
            } else {
                Role::User
            };
            if can_user_access_media(&auth.store, &target_id, &r, media_id).await {
                let invite_token = Uuid::new_v4().to_string();
                let invite_id = Uuid::new_v4().to_string();
                let expiry = now + DEFAULT_INVITE_EXPIRY_SECONDS;
                let _ = query(
                    "INSERT INTO watch_party_invites (id, party_id, token, created_by, target_user_id, created_at, expires_at) \
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(invite_id)
                .bind(&party_id)
                .bind(invite_token)
                .bind(&actor.user.id)
                .bind(&target_id)
                .bind(now)
                .bind(expiry)
                .execute(&mut *tx)
                .await;
            }
        }
    }

    tx.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Preload into memory hub
    hub.get_or_load_room(&party_id).await;

    Ok(Json(json!({
        "id": party_id,
        "name": party_name,
        "mediaId": media_id,
        "mediaTitle": media_title,
        "controlMode": control_mode.as_str(),
        "state": "waiting",
        "createdAt": now
    })))
}

async fn list_parties(
    State((auth, _)): State<(AuthState, WatchPartyHub)>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    let now = current_epoch_s();

    // Parties where user is member, host, or invited to
    let rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
    )> = query_as(
        "SELECT p.id, p.name, p.host_user_id, COALESCE(hu.display_name, hu.sso_username, p.host_user_id), \
                p.media_id, COALESCE(w.title, 'Séance'), p.control_mode, p.state, \
                p.current_position_ms, p.is_locked, p.wait_for_buffering, p.created_at, \
                (SELECT COUNT(*) FROM watch_party_members m WHERE m.party_id = p.id) as members_count, \
                (CASE WHEN EXISTS(SELECT 1 FROM watch_party_members m WHERE m.party_id = p.id AND m.user_id = ?) THEN 1 ELSE 0 END) as is_member \
         FROM watch_parties p \
         JOIN users hu ON hu.id = p.host_user_id \
         LEFT JOIN inventory_works w ON w.id = p.media_id \
         WHERE p.ended_at IS NULL \
           AND (p.host_user_id = ? \
                OR EXISTS(SELECT 1 FROM watch_party_members m WHERE m.party_id = p.id AND m.user_id = ?) \
                OR EXISTS(SELECT 1 FROM watch_party_invites i WHERE i.party_id = p.id AND (i.target_user_id = ? OR i.target_user_id IS NULL) AND i.expires_at > ? AND i.revoked_at IS NULL)) \
         ORDER BY p.created_at DESC",
    )
    .bind(&actor.user.id)
    .bind(&actor.user.id)
    .bind(&actor.user.id)
    .bind(&actor.user.id)
    .bind(now)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut parties = Vec::new();
    for (
        id,
        name,
        host_id,
        host_name,
        media_id,
        media_title,
        control_mode,
        state,
        pos_ms,
        is_locked,
        wait_buffering,
        created,
        members_count,
        is_member,
    ) in rows
    {
        // Enforce media authorization
        if can_user_access_media(&auth.store, &actor.user.id, &actor.user.role, &media_id).await {
            parties.push(PartySummary {
                id,
                name,
                host_user_id: host_id.clone(),
                host_name,
                media_id,
                media_title,
                control_mode,
                state,
                current_position: (pos_ms as f64) / 1000.0,
                is_locked: is_locked != 0,
                wait_for_buffering: wait_buffering != 0,
                members_count: members_count as usize,
                created_at: created,
                is_host: host_id == actor.user.id,
                is_member: is_member != 0,
            });
        }
    }

    Ok(Json(json!({ "parties": parties })))
}

async fn get_party(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let room = room_arc.read().await;

    // Check media access
    if !can_user_access_media(
        &auth.store,
        &actor.user.id,
        &actor.user.role,
        &room.media_id,
    )
    .await
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let host_name: String =
        query_scalar("SELECT COALESCE(display_name, sso_username, id) FROM users WHERE id = ?")
            .bind(&room.host_user_id)
            .fetch_optional(&auth.store.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| room.host_user_id.clone());

    let state_payload = room.to_state_payload(host_name);
    let members = room.members_payload();

    Ok(Json(json!({
        "party": state_payload,
        "members": members,
        "isHost": room.host_user_id == actor.user.id,
        "isMember": room.participants.contains_key(&actor.user.id),
        "canControl": room.can_user_control(&actor.user.id)
    })))
}

async fn join_party(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut room = room_arc.write().await;

    if room.state == PartyState::Ended {
        return Err(StatusCode::GONE);
    }

    // Verify access to media
    if !can_user_access_media(
        &auth.store,
        &actor.user.id,
        &actor.user.role,
        &room.media_id,
    )
    .await
    {
        return Err(StatusCode::FORBIDDEN);
    }

    // If locked and not already a member, reject
    if room.is_locked && !room.participants.contains_key(&actor.user.id) {
        return Err(StatusCode::FORBIDDEN);
    }

    let now = current_epoch_s();
    let display_name = actor
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| actor.user.id.clone());

    // Record in DB
    query(
        "INSERT INTO watch_party_members (party_id, user_id, role, is_ready, is_buffering, joined_at, last_seen_at) \
         VALUES (?, ?, 'participant', 0, 0, ?, ?) \
         ON CONFLICT(party_id, user_id) DO UPDATE SET last_seen_at = excluded.last_seen_at",
    )
    .bind(&room.id)
    .bind(&actor.user.id)
    .bind(now)
    .bind(now)
    .execute(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let participant = LiveParticipant {
        user_id: actor.user.id.clone(),
        display_name: display_name.clone(),
        role: if actor.user.id == room.host_user_id {
            "host".to_string()
        } else {
            "participant".to_string()
        },
        is_ready: actor.user.id == room.host_user_id,
        is_buffering: false,
        last_position: room.computed_position(),
        last_seen_ms: current_epoch_ms(),
        joined_at: now,
    };
    room.participants
        .insert(actor.user.id.clone(), participant.clone());

    let member_payload = MemberPayload {
        user_id: participant.user_id,
        display_name: participant.display_name,
        role: participant.role,
        is_ready: participant.is_ready,
        is_buffering: participant.is_buffering,
        is_online: room.online_user_ids.contains(&actor.user.id),
        joined_at: participant.joined_at,
        has_control: room.can_user_control(&actor.user.id),
    };

    // Broadcast member joined
    let _ = room.broadcast_tx.send(ServerMessage::MemberJoined {
        member: member_payload.clone(),
    });

    Ok(Json(json!({
        "success": true,
        "partyId": room.id,
        "member": member_payload
    })))
}

async fn leave_party(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    if let Some(room_arc) = hub.get_or_load_room(&id).await {
        let mut room = room_arc.write().await;
        let display_name = actor
            .user
            .display_name
            .clone()
            .unwrap_or_else(|| actor.user.id.clone());

        room.participants.remove(&actor.user.id);
        room.online_user_ids.remove(&actor.user.id);
        room.active_controllers.remove(&actor.user.id);

        let _ = room.broadcast_tx.send(ServerMessage::MemberLeft {
            user_id: actor.user.id.clone(),
            display_name,
            reason: Some("A quitté la séance".to_string()),
        });

        // If host leaves, try to transfer to next online participant
        if room.host_user_id == actor.user.id {
            if let Some(next_host_id) = room.online_user_ids.iter().next().cloned() {
                let next_name = room
                    .participants
                    .get(&next_host_id)
                    .map(|p| p.display_name.clone())
                    .unwrap_or_else(|| next_host_id.clone());

                room.host_user_id = next_host_id.clone();
                if let Some(p) = room.participants.get_mut(&next_host_id) {
                    p.role = "host".to_string();
                }

                let _ = query("UPDATE watch_parties SET host_user_id = ? WHERE id = ?")
                    .bind(&next_host_id)
                    .bind(&room.id)
                    .execute(&auth.store.pool)
                    .await;
                let _ = query("UPDATE watch_party_members SET role = 'host' WHERE party_id = ? AND user_id = ?")
                    .bind(&room.id)
                    .bind(&next_host_id)
                    .execute(&auth.store.pool)
                    .await;

                let _ = room.broadcast_tx.send(ServerMessage::HostTransferred {
                    new_host_id: next_host_id,
                    new_host_name: next_name,
                });
            }
        }
    }

    let _ = query("DELETE FROM watch_party_members WHERE party_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&actor.user.id)
        .execute(&auth.store.pool)
        .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn end_party(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let now = current_epoch_s();

    if let Some(room_arc) = hub.get_or_load_room(&id).await {
        let mut room = room_arc.write().await;
        if room.host_user_id != actor.user.id && actor.user.role != Role::Administrator {
            return Err(StatusCode::FORBIDDEN);
        }
        room.state = PartyState::Ended;
        let _ = room.broadcast_tx.send(ServerMessage::PartyEnded {
            reason: "La séance a été terminée par l'organisateur.".to_string(),
        });
    }

    query("UPDATE watch_parties SET state = 'ended', ended_at = ? WHERE id = ?")
        .bind(now)
        .bind(&id)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    hub.evict_room(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSettingsInput {
    pub name: Option<String>,
    pub control_mode: Option<String>,
    pub is_locked: Option<bool>,
    pub wait_for_buffering: Option<bool>,
}

async fn update_party_settings(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<UpdateSettingsInput>,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut room = room_arc.write().await;

    if room.host_user_id != actor.user.id && actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(name) = input.name {
        if !name.trim().is_empty() {
            room.name = name.trim().to_string();
        }
    }
    if let Some(mode) = input.control_mode {
        room.control_mode = ControlMode::from_str(&mode);
    }
    if let Some(locked) = input.is_locked {
        room.is_locked = locked;
    }
    if let Some(wait) = input.wait_for_buffering {
        room.wait_for_buffering = wait;
    }

    query(
        "UPDATE watch_parties SET name = ?, control_mode = ?, is_locked = ?, wait_for_buffering = ? WHERE id = ?",
    )
    .bind(&room.name)
    .bind(room.control_mode.as_str())
    .bind(i64::from(room.is_locked))
    .bind(i64::from(room.wait_for_buffering))
    .bind(&room.id)
    .execute(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let host_name: String =
        query_scalar("SELECT COALESCE(display_name, sso_username, id) FROM users WHERE id = ?")
            .bind(&room.host_user_id)
            .fetch_optional(&auth.store.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| room.host_user_id.clone());

    let payload = room.to_state_payload(host_name);

    Ok(Json(json!({ "party": payload })))
}

// ---------------------------------------------------------------------------
// Invitations Management
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInviteInput {
    pub target_user_id: Option<String>,
    pub expires_in_seconds: Option<i64>,
}

async fn create_party_invite(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<CreateInviteInput>,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let room = room_arc.read().await;

    if room.host_user_id != actor.user.id && actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }

    if let Some(ref target_id) = input.target_user_id {
        let role_str: Option<String> = query_scalar("SELECT role FROM users WHERE id = ?")
            .bind(target_id)
            .fetch_optional(&auth.store.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(r_str) = role_str {
            let role = if r_str == "administrator" {
                Role::Administrator
            } else {
                Role::User
            };
            if !can_user_access_media(&auth.store, target_id, &role, &room.media_id).await {
                return Err(StatusCode::FORBIDDEN);
            }
        } else {
            return Err(StatusCode::NOT_FOUND);
        }
    }

    let token = Uuid::new_v4().to_string();
    let invite_id = Uuid::new_v4().to_string();
    let now = current_epoch_s();
    let expires = now
        + input
            .expires_in_seconds
            .unwrap_or(DEFAULT_INVITE_EXPIRY_SECONDS);

    query(
        "INSERT INTO watch_party_invites (id, party_id, token, created_by, target_user_id, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&invite_id)
    .bind(&room.id)
    .bind(&token)
    .bind(&actor.user.id)
    .bind(input.target_user_id.as_deref())
    .bind(now)
    .bind(expires)
    .execute(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "id": invite_id,
        "token": token,
        "partyId": room.id,
        "targetUserId": input.target_user_id,
        "expiresAt": expires,
        "inviteUrl": format!("/parties?invite={token}")
    })))
}

async fn list_party_invites(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let room = room_arc.read().await;

    if room.host_user_id != actor.user.id && actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }

    let rows: Vec<(String, String, Option<String>, Option<String>, i64, i64, Option<i64>)> = query_as(
        "SELECT i.id, i.token, i.target_user_id, COALESCE(u.display_name, u.sso_username, i.target_user_id), \
                i.created_at, i.expires_at, i.revoked_at \
         FROM watch_party_invites i \
         LEFT JOIN users u ON u.id = i.target_user_id \
         WHERE i.party_id = ? \
         ORDER BY i.created_at DESC",
    )
    .bind(&id)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let invites = rows
        .into_iter()
        .map(
            |(id, token, target_id, target_name, created, expires, revoked)| {
                json!({
                    "id": id,
                    "token": token,
                    "targetUserId": target_id,
                    "targetUserName": target_name,
                    "createdAt": created,
                    "expiresAt": expires,
                    "revokedAt": revoked,
                    "isExpired": expires < current_epoch_s(),
                    "isRevoked": revoked.is_some()
                })
            },
        )
        .collect::<Vec<_>>();

    Ok(Json(json!({ "invites": invites })))
}

async fn revoke_party_invite(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path((id, invite_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let room = room_arc.read().await;

    if room.host_user_id != actor.user.id && actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }

    let now = current_epoch_s();
    query("UPDATE watch_party_invites SET revoked_at = ? WHERE id = ? AND party_id = ?")
        .bind(now)
        .bind(&invite_id)
        .bind(&id)
        .execute(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_user_invites(
    State((auth, _)): State<(AuthState, WatchPartyHub)>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    let now = current_epoch_s();

    let rows: Vec<(String, String, String, String, String, String, i64, i64)> = query_as(
        "SELECT i.token, p.id, p.name, COALESCE(hu.display_name, hu.sso_username, p.host_user_id), \
                p.media_id, COALESCE(w.title, 'Séance'), i.created_at, i.expires_at \
         FROM watch_party_invites i \
         JOIN watch_parties p ON p.id = i.party_id \
         JOIN users hu ON hu.id = p.host_user_id \
         LEFT JOIN inventory_works w ON w.id = p.media_id \
         WHERE (i.target_user_id = ? OR i.target_user_id IS NULL) \
           AND i.expires_at > ? \
           AND i.revoked_at IS NULL \
           AND p.ended_at IS NULL \
         ORDER BY i.created_at DESC",
    )
    .bind(&actor.user.id)
    .bind(now)
    .fetch_all(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut invites = Vec::new();
    for (token, p_id, p_name, host_name, media_id, media_title, created, expires) in rows {
        if can_user_access_media(&auth.store, &actor.user.id, &actor.user.role, &media_id).await {
            invites.push(json!({
                "token": token,
                "partyId": p_id,
                "partyName": p_name,
                "hostName": host_name,
                "mediaId": media_id,
                "mediaTitle": media_title,
                "createdAt": created,
                "expiresAt": expires
            }));
        }
    }

    Ok(Json(json!({ "invites": invites })))
}

async fn accept_invite(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let now = current_epoch_s();
    let row: Option<(String, String, Option<String>, i64, Option<i64>)> = query_as(
        "SELECT id, party_id, target_user_id, expires_at, revoked_at \
         FROM watch_party_invites WHERE token = ?",
    )
    .bind(&token)
    .fetch_optional(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (_, party_id, target_user_id, expires_at, revoked_at) = row.ok_or(StatusCode::NOT_FOUND)?;

    if revoked_at.is_some() || expires_at <= now {
        return Err(StatusCode::GONE);
    }

    if let Some(target) = target_user_id {
        if target != actor.user.id {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    let room_arc = hub
        .get_or_load_room(&party_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut room = room_arc.write().await;

    if room.state == PartyState::Ended {
        return Err(StatusCode::GONE);
    }

    if !can_user_access_media(
        &auth.store,
        &actor.user.id,
        &actor.user.role,
        &room.media_id,
    )
    .await
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let display_name = actor
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| actor.user.id.clone());

    query(
        "INSERT INTO watch_party_members (party_id, user_id, role, is_ready, is_buffering, joined_at, last_seen_at) \
         VALUES (?, ?, 'participant', 0, 0, ?, ?) \
         ON CONFLICT(party_id, user_id) DO UPDATE SET last_seen_at = excluded.last_seen_at",
    )
    .bind(&room.id)
    .bind(&actor.user.id)
    .bind(now)
    .bind(now)
    .execute(&auth.store.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let participant = LiveParticipant {
        user_id: actor.user.id.clone(),
        display_name: display_name.clone(),
        role: "participant".to_string(),
        is_ready: false,
        is_buffering: false,
        last_position: room.computed_position(),
        last_seen_ms: current_epoch_ms(),
        joined_at: now,
    };
    room.participants
        .insert(actor.user.id.clone(), participant.clone());

    let member_payload = MemberPayload {
        user_id: participant.user_id,
        display_name: participant.display_name,
        role: participant.role,
        is_ready: participant.is_ready,
        is_buffering: participant.is_buffering,
        is_online: room.online_user_ids.contains(&actor.user.id),
        joined_at: participant.joined_at,
        has_control: room.can_user_control(&actor.user.id),
    };

    let _ = room.broadcast_tx.send(ServerMessage::MemberJoined {
        member: member_payload,
    });

    Ok(Json(json!({
        "success": true,
        "partyId": room.id
    })))
}

#[derive(Deserialize)]
struct EligibleQuery {
    pub media_id: Option<String>,
    pub q: Option<String>,
}

async fn eligible_users(
    State((auth, _)): State<(AuthState, WatchPartyHub)>,
    Query(query_params): Query<EligibleQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, PartyApiError> {
    let actor = auth.authenticate(&headers).await?;

    let q_filter = query_params
        .q
        .as_deref()
        .unwrap_or("")
        .trim()
        .trim_start_matches('@')
        .to_lowercase();

    let rows: Vec<(String, String, Option<String>, String)> = if q_filter.is_empty() {
        query_as(
            "SELECT u.id, COALESCE(u.sso_username, c.login, u.id) AS login, u.display_name, u.role \
             FROM users u \
             LEFT JOIN local_credentials c ON c.user_id = u.id \
             WHERE u.disabled_at IS NULL ORDER BY login LIMIT 50",
        )
        .fetch_all(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        let pattern = format!("%{q_filter}%");
        query_as(
            "SELECT u.id, COALESCE(u.sso_username, c.login, u.id) AS login, u.display_name, u.role \
             FROM users u \
             LEFT JOIN local_credentials c ON c.user_id = u.id \
             WHERE u.disabled_at IS NULL \
               AND (lower(COALESCE(u.sso_username, c.login, u.id)) LIKE ? OR lower(COALESCE(u.display_name, '')) LIKE ?) \
             ORDER BY login LIMIT 20",
        )
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&auth.store.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let mut eligible = Vec::new();
    for (id, login, disp, role_str) in rows {
        if id == actor.user.id {
            continue;
        }
        let role = if role_str == "administrator" {
            Role::Administrator
        } else {
            Role::User
        };

        let allowed = match query_params.media_id {
            Some(ref m_id) if !m_id.trim().is_empty() => {
                can_user_access_media(&auth.store, &id, &role, m_id.trim()).await
            }
            _ => true,
        };

        if allowed {
            eligible.push(json!({
                "id": id,
                "login": login,
                "displayName": disp.unwrap_or_else(|| login.clone()),
                "role": role_str
            }));
        }
    }

    Ok(Json(json!({ "users": eligible })))
}

// ---------------------------------------------------------------------------
// Real-Time WebSocket Handler
// ---------------------------------------------------------------------------

async fn ws_handler(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, Error> {
    let actor = auth.authenticate(&headers).await?;

    let room_arc = hub
        .get_or_load_room(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    // Check media access
    {
        let room = room_arc.read().await;
        if !can_user_access_media(
            &auth.store,
            &actor.user.id,
            &actor.user.role,
            &room.media_id,
        )
        .await
        {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, room_arc, actor, hub)))
}

async fn handle_socket(
    socket: WebSocket,
    room_arc: Arc<RwLock<PartyRoom>>,
    actor: Authenticated,
    hub: WatchPartyHub,
) {
    let (mut sender, mut receiver) = socket.split();
    let user_id = actor.user.id.clone();
    let display_name = actor
        .user
        .display_name
        .clone()
        .unwrap_or_else(|| actor.user.id.clone());

    // Subscribe to room broadcast channel
    let mut rx = {
        let mut room = room_arc.write().await;
        room.online_user_ids.insert(user_id.clone());

        // Ensure user is in participants map
        if !room.participants.contains_key(&user_id) {
            let p = LiveParticipant {
                user_id: user_id.clone(),
                display_name: display_name.clone(),
                role: if user_id == room.host_user_id {
                    "host".to_string()
                } else {
                    "participant".to_string()
                },
                is_ready: user_id == room.host_user_id,
                is_buffering: false,
                last_position: room.computed_position(),
                last_seen_ms: current_epoch_ms(),
                joined_at: current_epoch_s(),
            };
            room.participants.insert(user_id.clone(), p);
        }

        room.broadcast_tx.subscribe()
    };

    // Send initial SyncState snapshot to the newly connected client
    {
        let room = room_arc.read().await;
        let host_name: String =
            query_scalar("SELECT COALESCE(display_name, sso_username, id) FROM users WHERE id = ?")
                .bind(&room.host_user_id)
                .fetch_optional(&hub.store.pool)
                .await
                .unwrap_or(None)
                .unwrap_or_else(|| room.host_user_id.clone());

        let sync_msg = ServerMessage::SyncState {
            party: room.to_state_payload(host_name),
            members: room.members_payload(),
            messages: room.messages.iter().cloned().collect(),
            queue: room.queue.clone(),
            poll: room.current_poll.clone(),
            server_epoch_ms: current_epoch_ms(),
            seq: room.seq,
        };

        if let Ok(json_txt) = serde_json::to_string(&sync_msg) {
            let _ = sender.send(Message::Text(json_txt.into())).await;
        }
    }

    // Broadcast member online status
    {
        let room = room_arc.read().await;
        if let Some(p) = room.participants.get(&user_id) {
            let _ = room.broadcast_tx.send(ServerMessage::MemberStatus {
                user_id: user_id.clone(),
                is_ready: p.is_ready,
                is_buffering: p.is_buffering,
                position: p.last_position,
            });
        }
    }

    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Message>(32);

    // Task forwarding messages to client
    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Task listening to broadcast channel and sending to client
    let out_tx_clone = out_tx.clone();
    let bcast_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(json_str) = serde_json::to_string(&msg) {
                if out_tx_clone
                    .send(Message::Text(json_str.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    // Process incoming client messages
    while let Some(Ok(raw_msg)) = receiver.next().await {
        let text = match raw_msg {
            Message::Text(t) => t,
            Message::Ping(p) => {
                let _ = out_tx.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };

        let client_event: ClientMessage = match serde_json::from_str(&text) {
            Ok(evt) => evt,
            Err(_) => continue,
        };

        handle_client_event(
            client_event,
            &room_arc,
            &user_id,
            &display_name,
            &out_tx,
            &hub,
        )
        .await;
    }

    // Client disconnected: update online status
    {
        let mut room = room_arc.write().await;
        room.online_user_ids.remove(&user_id);
        if let Some(p) = room.participants.get_mut(&user_id) {
            p.last_seen_ms = current_epoch_ms();
        }

        let _ = room.broadcast_tx.send(ServerMessage::MemberLeft {
            user_id: user_id.clone(),
            display_name: display_name.clone(),
            reason: Some("Déconnecté".to_string()),
        });
    }

    bcast_task.abort();
    send_task.abort();
}

async fn handle_client_event(
    event: ClientMessage,
    room_arc: &Arc<RwLock<PartyRoom>>,
    user_id: &str,
    display_name: &str,
    out_tx: &tokio::sync::mpsc::Sender<Message>,
    hub: &WatchPartyHub,
) {
    match event {
        ClientMessage::Ping { client_epoch_ms } => {
            let pong = ServerMessage::Pong {
                client_epoch_ms,
                server_epoch_ms: current_epoch_ms(),
            };
            if let Ok(json_str) = serde_json::to_string(&pong) {
                let _ = out_tx.send(Message::Text(json_str.into())).await;
            }
        }

        ClientMessage::SetReady { ready } => {
            let mut room = room_arc.write().await;
            if let Some(p) = room.participants.get_mut(user_id) {
                p.is_ready = ready;
                let pos = p.last_position;
                let is_buffering = p.is_buffering;
                let _ = room.broadcast_tx.send(ServerMessage::MemberStatus {
                    user_id: user_id.to_string(),
                    is_ready: ready,
                    is_buffering,
                    position: pos,
                });
            }
            let _ = query(
                "UPDATE watch_party_members SET is_ready = ? WHERE party_id = ? AND user_id = ?",
            )
            .bind(i64::from(ready))
            .bind(&room.id)
            .bind(user_id)
            .execute(&hub.store.pool)
            .await;
        }

        ClientMessage::SetBuffering {
            buffering,
            position,
        } => {
            let mut room = room_arc.write().await;
            let mut auto_paused = false;
            if let Some(p) = room.participants.get_mut(user_id) {
                p.is_buffering = buffering;
                p.last_position = position;
                let is_ready = p.is_ready;
                let _ = room.broadcast_tx.send(ServerMessage::MemberStatus {
                    user_id: user_id.to_string(),
                    is_ready,
                    is_buffering: buffering,
                    position,
                });
            }

            // Auto-pause if wait_for_buffering is enabled and someone starts buffering
            if buffering && room.wait_for_buffering && room.state == PartyState::Playing {
                room.state = PartyState::Paused;
                room.position_s = position;
                room.position_updated_ms = current_epoch_ms();
                room.seq += 1;
                auto_paused = true;
                let seq = room.seq;
                let _ = room.broadcast_tx.send(ServerMessage::PlaybackUpdate {
                    state: "paused".to_string(),
                    position,
                    server_epoch_ms: current_epoch_ms(),
                    caused_by_user_id: user_id.to_string(),
                    caused_by_name: format!("{display_name} (chargement)"),
                    seq,
                });
            }

            if auto_paused {
                let _ = query("UPDATE watch_parties SET state = 'paused', current_position_ms = ?, position_updated_at_ms = ? WHERE id = ?")
                    .bind((position * 1000.0) as i64)
                    .bind(current_epoch_ms())
                    .bind(&room.id)
                    .execute(&hub.store.pool)
                    .await;
            }
        }

        ClientMessage::Play {
            position,
            countdown,
        } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            let countdown_s = countdown.unwrap_or(0);
            room.seq += 1;
            let seq = room.seq;
            let now_ms = current_epoch_ms();

            if countdown_s > 0 {
                let target_ms = now_ms + (countdown_s as i64 * 1000);
                room.state = PartyState::Playing;
                room.position_s = position;
                room.position_updated_ms = target_ms; // starts progressing at target_ms
                let _ = room.broadcast_tx.send(ServerMessage::Countdown {
                    seconds: countdown_s,
                    target_server_epoch_ms: target_ms,
                    position,
                    caused_by_user_id: user_id.to_string(),
                    caused_by_name: display_name.to_string(),
                    seq,
                });
            } else {
                room.state = PartyState::Playing;
                room.position_s = position;
                room.position_updated_ms = now_ms;
                let _ = room.broadcast_tx.send(ServerMessage::PlaybackUpdate {
                    state: "playing".to_string(),
                    position,
                    server_epoch_ms: now_ms,
                    caused_by_user_id: user_id.to_string(),
                    caused_by_name: display_name.to_string(),
                    seq,
                });
            }

            let _ = query("UPDATE watch_parties SET state = 'playing', current_position_ms = ?, position_updated_at_ms = ? WHERE id = ?")
                .bind((position * 1000.0) as i64)
                .bind(now_ms)
                .bind(&room.id)
                .execute(&hub.store.pool)
                .await;
        }

        ClientMessage::Pause { position } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            room.state = PartyState::Paused;
            room.position_s = position;
            room.position_updated_ms = current_epoch_ms();
            room.seq += 1;
            let seq = room.seq;

            let _ = room.broadcast_tx.send(ServerMessage::PlaybackUpdate {
                state: "paused".to_string(),
                position,
                server_epoch_ms: room.position_updated_ms,
                caused_by_user_id: user_id.to_string(),
                caused_by_name: display_name.to_string(),
                seq,
            });

            let _ = query("UPDATE watch_parties SET state = 'paused', current_position_ms = ?, position_updated_at_ms = ? WHERE id = ?")
                .bind((position * 1000.0) as i64)
                .bind(room.position_updated_ms)
                .bind(&room.id)
                .execute(&hub.store.pool)
                .await;
        }

        ClientMessage::Seek { position } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            room.position_s = position;
            room.position_updated_ms = current_epoch_ms();
            room.seq += 1;
            let seq = room.seq;
            let state_str = room.state.as_str().to_string();

            let _ = room.broadcast_tx.send(ServerMessage::PlaybackUpdate {
                state: state_str,
                position,
                server_epoch_ms: room.position_updated_ms,
                caused_by_user_id: user_id.to_string(),
                caused_by_name: display_name.to_string(),
                seq,
            });

            let _ = query("UPDATE watch_parties SET current_position_ms = ?, position_updated_at_ms = ? WHERE id = ?")
                .bind((position * 1000.0) as i64)
                .bind(room.position_updated_ms)
                .bind(&room.id)
                .execute(&hub.store.pool)
                .await;
        }

        ClientMessage::ReportPosition { position } => {
            let mut room = room_arc.write().await;
            if let Some(p) = room.participants.get_mut(user_id) {
                p.last_position = position;
                p.last_seen_ms = current_epoch_ms();
            }
        }

        ClientMessage::ChatMessage { content } => {
            let trimmed = content.trim();
            if trimmed.is_empty() || trimmed.len() > MAX_CHAT_LENGTH {
                return;
            }

            let msg = ChatMessagePayload {
                id: Uuid::new_v4().to_string(),
                user_id: user_id.to_string(),
                display_name: display_name.to_string(),
                content: trimmed.to_string(),
                kind: "chat".to_string(),
                created_at: current_epoch_s(),
            };

            {
                let mut room = room_arc.write().await;
                if room.messages.len() >= MAX_STORED_MESSAGES {
                    room.messages.pop_front();
                }
                room.messages.push_back(msg.clone());
                let _ = room.broadcast_tx.send(ServerMessage::NewMessage {
                    message: msg.clone(),
                });
            }

            let room_id = { room_arc.read().await.id.clone() };
            let _ = query("INSERT INTO watch_party_messages (id, party_id, user_id, content, kind, created_at) VALUES (?, ?, ?, ?, 'chat', ?)")
                .bind(&msg.id)
                .bind(&room_id)
                .bind(user_id)
                .bind(&msg.content)
                .bind(msg.created_at)
                .execute(&hub.store.pool)
                .await;
        }

        ClientMessage::Reaction { emoji } => {
            let trimmed = emoji.trim();
            if trimmed.is_empty() || trimmed.len() > 16 {
                return;
            }

            let room = room_arc.read().await;
            let _ = room.broadcast_tx.send(ServerMessage::NewReaction {
                user_id: user_id.to_string(),
                display_name: display_name.to_string(),
                emoji: trimmed.to_string(),
            });
        }

        ClientMessage::RequestControl => {
            let room = room_arc.read().await;
            if room.control_mode == ControlMode::Request {
                let _ = room.broadcast_tx.send(ServerMessage::ControlRequested {
                    user_id: user_id.to_string(),
                    display_name: display_name.to_string(),
                });
            }
        }

        ClientMessage::GrantControl { user_id: target } => {
            let mut room = room_arc.write().await;
            if room.host_user_id == user_id {
                room.active_controllers.insert(target.clone());
                let _ = room
                    .broadcast_tx
                    .send(ServerMessage::ControlGranted { user_id: target });
            }
        }

        ClientMessage::RevokeControl { user_id: target } => {
            let mut room = room_arc.write().await;
            if room.host_user_id == user_id {
                room.active_controllers.remove(&target);
                let _ = room
                    .broadcast_tx
                    .send(ServerMessage::ControlRevoked { user_id: target });
            }
        }

        ClientMessage::ChangeMedia { media_id } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            // Verify access
            let title: Option<String> =
                query_scalar("SELECT title FROM inventory_works WHERE id = ?")
                    .bind(&media_id)
                    .fetch_optional(&hub.store.pool)
                    .await
                    .unwrap_or(None);

            if let Some(m_title) = title {
                room.media_id = media_id.clone();
                room.media_title = m_title.clone();
                room.state = PartyState::Waiting;
                room.position_s = 0.0;
                room.position_updated_ms = current_epoch_ms();
                room.seq += 1;

                // Reset member readiness for new media
                let host_id = room.host_user_id.clone();
                for p in room.participants.values_mut() {
                    p.is_ready = p.user_id == host_id;
                    p.is_buffering = false;
                }

                let _ = room.broadcast_tx.send(ServerMessage::MediaChanged {
                    media_id: media_id.clone(),
                    media_title: m_title,
                    caused_by_name: display_name.to_string(),
                });

                let _ = query("UPDATE watch_parties SET media_id = ?, state = 'waiting', current_position_ms = 0, position_updated_at_ms = ? WHERE id = ?")
                    .bind(&media_id)
                    .bind(current_epoch_ms())
                    .bind(&room.id)
                    .execute(&hub.store.pool)
                    .await;
            }
        }

        ClientMessage::AddToQueue { media_id } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            let title: Option<String> =
                query_scalar("SELECT title FROM inventory_works WHERE id = ?")
                    .bind(&media_id)
                    .fetch_optional(&hub.store.pool)
                    .await
                    .unwrap_or(None);

            if let Some(m_title) = title {
                let q_id = Uuid::new_v4().to_string();
                let now = current_epoch_s();
                let pos = room.queue.len() as i64;

                let item = QueueItemPayload {
                    id: q_id.clone(),
                    media_id: media_id.clone(),
                    media_title: m_title,
                    added_by_user_id: user_id.to_string(),
                    added_by_name: display_name.to_string(),
                    position: pos,
                    created_at: now,
                };
                room.queue.push(item);

                let _ = room.broadcast_tx.send(ServerMessage::QueueUpdated {
                    queue: room.queue.clone(),
                });

                let _ = query("INSERT INTO watch_party_queue (id, party_id, media_id, added_by, position, created_at) VALUES (?, ?, ?, ?, ?, ?)")
                    .bind(&q_id)
                    .bind(&room.id)
                    .bind(&media_id)
                    .bind(user_id)
                    .bind(pos)
                    .bind(now)
                    .execute(&hub.store.pool)
                    .await;
            }
        }

        ClientMessage::RemoveFromQueue { queue_id } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            room.queue.retain(|item| item.id != queue_id);
            let _ = room.broadcast_tx.send(ServerMessage::QueueUpdated {
                queue: room.queue.clone(),
            });

            let _ = query("DELETE FROM watch_party_queue WHERE id = ? AND party_id = ?")
                .bind(&queue_id)
                .bind(&room.id)
                .execute(&hub.store.pool)
                .await;
        }

        ClientMessage::CreatePoll { question, options } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            let clean_q = question.trim();
            if clean_q.is_empty() || options.len() < 2 || options.len() > 6 {
                return;
            }

            let poll_id = Uuid::new_v4().to_string();
            let now = current_epoch_s();
            let poll_options = options
                .iter()
                .map(|opt| PollOptionPayload {
                    text: opt.trim().to_string(),
                    votes: 0,
                    voter_ids: Vec::new(),
                })
                .collect::<Vec<_>>();

            let poll_payload = PollPayload {
                id: poll_id.clone(),
                question: clean_q.to_string(),
                options: poll_options,
                created_by: user_id.to_string(),
                is_closed: false,
                created_at: now,
                total_votes: 0,
            };

            room.current_poll = Some(poll_payload.clone());
            let _ = room.broadcast_tx.send(ServerMessage::PollUpdated {
                poll: Some(poll_payload.clone()),
            });

            let opts_json = json!(options).to_string();
            let _ = query("INSERT INTO watch_party_polls (id, party_id, question, options_json, created_by, is_closed, created_at) VALUES (?, ?, ?, ?, ?, 0, ?)")
                .bind(&poll_id)
                .bind(&room.id)
                .bind(clean_q)
                .bind(opts_json)
                .bind(user_id)
                .bind(now)
                .execute(&hub.store.pool)
                .await;
        }

        ClientMessage::VotePoll {
            poll_id,
            option_index,
        } => {
            let mut room = room_arc.write().await;
            let mut updated_poll = None;
            if let Some(ref mut poll) = room.current_poll {
                if poll.id == poll_id && !poll.is_closed && option_index < poll.options.len() {
                    // Remove previous vote if any
                    for opt in &mut poll.options {
                        if let Some(idx) = opt.voter_ids.iter().position(|v| v == user_id) {
                            opt.voter_ids.remove(idx);
                            opt.votes = opt.votes.saturating_sub(1);
                        }
                    }
                    // Add new vote
                    poll.options[option_index]
                        .voter_ids
                        .push(user_id.to_string());
                    poll.options[option_index].votes += 1;
                    poll.total_votes = poll.options.iter().map(|o| o.votes).sum();
                    updated_poll = Some(poll.clone());
                }
            }

            if let Some(poll) = updated_poll {
                let _ = room
                    .broadcast_tx
                    .send(ServerMessage::PollUpdated { poll: Some(poll) });

                let _ = query(
                    "INSERT INTO watch_party_poll_votes (poll_id, user_id, option_index) VALUES (?, ?, ?) \
                     ON CONFLICT(poll_id, user_id) DO UPDATE SET option_index = excluded.option_index",
                )
                .bind(&poll_id)
                .bind(user_id)
                .bind(option_index as i64)
                .execute(&hub.store.pool)
                .await;
            }
        }

        ClientMessage::ClosePoll { poll_id } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                return;
            }
            let mut closed_poll = None;
            if let Some(ref mut poll) = room.current_poll {
                if poll.id == poll_id {
                    poll.is_closed = true;
                    closed_poll = Some(poll.clone());
                }
            }
            if let Some(poll) = closed_poll {
                let _ = room
                    .broadcast_tx
                    .send(ServerMessage::PollUpdated { poll: Some(poll) });

                let _ = query("UPDATE watch_party_polls SET is_closed = 1 WHERE id = ?")
                    .bind(&poll_id)
                    .execute(&hub.store.pool)
                    .await;
            }
        }

        ClientMessage::KickMember { user_id: target } => {
            let mut room = room_arc.write().await;
            if room.host_user_id == user_id && target != room.host_user_id {
                room.participants.remove(&target);
                room.online_user_ids.remove(&target);
                room.active_controllers.remove(&target);

                let _ = room.broadcast_tx.send(ServerMessage::Kicked {
                    user_id: target.clone(),
                });

                let _ = query("DELETE FROM watch_party_members WHERE party_id = ? AND user_id = ?")
                    .bind(&room.id)
                    .bind(&target)
                    .execute(&hub.store.pool)
                    .await;
            }
        }

        ClientMessage::TransferHost { new_host_id } => {
            let mut room = room_arc.write().await;
            if room.host_user_id == user_id && room.participants.contains_key(&new_host_id) {
                room.host_user_id = new_host_id.clone();
                let new_name = room
                    .participants
                    .get(&new_host_id)
                    .map(|p| p.display_name.clone())
                    .unwrap_or_else(|| new_host_id.clone());

                if let Some(p) = room.participants.get_mut(user_id) {
                    p.role = "participant".to_string();
                }
                if let Some(p) = room.participants.get_mut(&new_host_id) {
                    p.role = "host".to_string();
                }

                let _ = room.broadcast_tx.send(ServerMessage::HostTransferred {
                    new_host_id: new_host_id.clone(),
                    new_host_name: new_name,
                });

                let _ = query("UPDATE watch_parties SET host_user_id = ? WHERE id = ?")
                    .bind(&new_host_id)
                    .bind(&room.id)
                    .execute(&hub.store.pool)
                    .await;
            }
        }

        ClientMessage::SetSettings {
            name,
            control_mode,
            is_locked,
            wait_for_buffering,
        } => {
            let mut room = room_arc.write().await;
            if room.host_user_id != user_id {
                let _ = send_error(
                    out_tx,
                    "FORBIDDEN",
                    "Seul l'organisateur peut modifier les paramètres",
                )
                .await;
                return;
            }

            if let Some(n) = name {
                if !n.trim().is_empty() {
                    room.name = n.trim().to_string();
                }
            }
            if let Some(m) = control_mode {
                room.control_mode = ControlMode::from_str(&m);
            }
            if let Some(l) = is_locked {
                room.is_locked = l;
            }
            if let Some(w) = wait_for_buffering {
                room.wait_for_buffering = w;
            }

            let host_name: String = query_scalar(
                "SELECT COALESCE(display_name, sso_username, id) FROM users WHERE id = ?",
            )
            .bind(&room.host_user_id)
            .fetch_optional(&hub.store.pool)
            .await
            .unwrap_or(None)
            .unwrap_or_else(|| room.host_user_id.clone());

            let sync = ServerMessage::SyncState {
                party: room.to_state_payload(host_name),
                members: room.members_payload(),
                messages: room.messages.iter().cloned().collect(),
                queue: room.queue.clone(),
                poll: room.current_poll.clone(),
                server_epoch_ms: current_epoch_ms(),
                seq: room.seq,
            };
            let _ = room.broadcast_tx.send(sync);

            let _ = query(
                "UPDATE watch_parties SET name = ?, control_mode = ?, is_locked = ?, wait_for_buffering = ? WHERE id = ?",
            )
            .bind(&room.name)
            .bind(room.control_mode.as_str())
            .bind(i64::from(room.is_locked))
            .bind(i64::from(room.wait_for_buffering))
            .bind(&room.id)
            .execute(&hub.store.pool)
            .await;
        }

        ClientMessage::LeaveParty => {
            let mut room = room_arc.write().await;
            room.participants.remove(user_id);
            room.online_user_ids.remove(user_id);
            room.active_controllers.remove(user_id);

            let _ = room.broadcast_tx.send(ServerMessage::MemberLeft {
                user_id: user_id.to_string(),
                display_name: display_name.to_string(),
                reason: Some("A quitté la séance".to_string()),
            });

            let _ = query("DELETE FROM watch_party_members WHERE party_id = ? AND user_id = ?")
                .bind(&room.id)
                .bind(user_id)
                .execute(&hub.store.pool)
                .await;
        }
    }
}

async fn send_error(out_tx: &tokio::sync::mpsc::Sender<Message>, code: &str, message: &str) {
    let err = ServerMessage::Error {
        code: code.to_string(),
        message: message.to_string(),
    };
    if let Ok(json_str) = serde_json::to_string(&err) {
        let _ = out_tx.send(Message::Text(json_str.into())).await;
    }
}

// ---------------------------------------------------------------------------
// Background cleanup task for expired parties and invites
// ---------------------------------------------------------------------------

pub async fn clean_expired_parties(store: &IdentityStore) -> Result<u64, sqlx::Error> {
    let now = current_epoch_s();

    // 1. Mark parties ended if older than 24 hours without activity
    let done = query(
        "UPDATE watch_parties SET state = 'ended', ended_at = ? \
         WHERE ended_at IS NULL AND created_at < ?",
    )
    .bind(now)
    .bind(now - 86400)
    .execute(&store.pool)
    .await?;
    let ended = done.rows_affected();

    // 2. Clean expired invites older than 7 days
    let _ = query("DELETE FROM watch_party_invites WHERE expires_at < ?")
        .bind(now - 604800)
        .execute(&store.pool)
        .await;

    Ok(ended)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    #[test]
    fn test_control_mode_and_state_roundtrip() {
        assert_eq!(ControlMode::from_str("host_only"), ControlMode::HostOnly);
        assert_eq!(ControlMode::from_str("shared"), ControlMode::Shared);
        assert_eq!(ControlMode::from_str("request"), ControlMode::Request);
        assert_eq!(ControlMode::from_str("unknown"), ControlMode::HostOnly);

        assert_eq!(PartyState::from_str("waiting"), PartyState::Waiting);
        assert_eq!(PartyState::from_str("playing"), PartyState::Playing);
        assert_eq!(PartyState::from_str("paused"), PartyState::Paused);
        assert_eq!(PartyState::from_str("ended"), PartyState::Ended);
    }

    #[test]
    fn test_computed_position_when_playing() {
        let (mut room, _) = PartyRoom::new(
            "p1".to_string(),
            "Room".to_string(),
            "host".to_string(),
            "m1".to_string(),
            "Movie".to_string(),
            ControlMode::HostOnly,
            true,
            current_epoch_s(),
        );

        assert_eq!(room.computed_position(), 0.0);
        room.state = PartyState::Playing;
        room.position_s = 10.0;
        room.position_updated_ms = current_epoch_ms() - 2000; // 2 seconds ago

        let pos = room.computed_position();
        assert!((pos - 12.0).abs() < 0.2);
    }

    #[test]
    fn test_control_permissions() {
        let (mut room, _) = PartyRoom::new(
            "p1".to_string(),
            "Room".to_string(),
            "host".to_string(),
            "m1".to_string(),
            "Movie".to_string(),
            ControlMode::HostOnly,
            true,
            current_epoch_s(),
        );

        assert!(room.can_user_control("host"));
        assert!(!room.can_user_control("other"));

        room.control_mode = ControlMode::Shared;
        assert!(room.can_user_control("other"));

        room.control_mode = ControlMode::Request;
        assert!(!room.can_user_control("other"));
        room.active_controllers.insert("other".to_string());
        assert!(room.can_user_control("other"));
    }

    #[tokio::test]
    async fn test_party_hub_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("parties.sqlite");
        let store = IdentityStore::open(&db_path).await.unwrap();
        let hub = WatchPartyHub::new(store.clone());

        query(
            "INSERT INTO users (id, role, display_name) VALUES ('host-user', 'user', 'Host User')",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        let party_id = "test-party-123";
        let now = current_epoch_s();
        query(
            "INSERT INTO watch_parties (id, name, host_user_id, media_id, control_mode, wait_for_buffering, state, current_position_ms, position_updated_at_ms, created_at) \
             VALUES (?, 'Movie Night', 'host-user', 'media-456', 'host_only', 1, 'waiting', 0, ?, ?)",
        )
        .bind(party_id)
        .bind(now * 1000)
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();

        let room_arc = hub
            .get_or_load_room(party_id)
            .await
            .expect("Room should be loaded");
        {
            let room = room_arc.read().await;
            assert_eq!(room.name, "Movie Night");
            assert_eq!(room.host_user_id, "host-user");
            assert_eq!(room.state, PartyState::Waiting);
            assert!(room.can_user_control("host-user"));
            assert!(!room.can_user_control("guest-user"));
        }

        query("UPDATE watch_parties SET state = 'ended', ended_at = ? WHERE id = ?")
            .bind(now)
            .bind(party_id)
            .execute(&store.pool)
            .await
            .unwrap();

        hub.evict_room(party_id).await;
        let reloaded = hub.get_or_load_room(party_id).await;
        assert!(reloaded.is_none());
    }

    #[tokio::test]
    async fn test_clean_expired_parties() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("cleanup.sqlite");
        let store = IdentityStore::open(&db_path).await.unwrap();

        query("INSERT INTO users (id, role, display_name) VALUES ('host-1', 'user', 'Host 1'), ('user-2', 'user', 'User 2')")
            .execute(&store.pool)
            .await
            .unwrap();

        let now = current_epoch_s();
        let old_time = now - 100_000;

        query(
            "INSERT INTO watch_parties (id, name, host_user_id, media_id, control_mode, wait_for_buffering, state, current_position_ms, position_updated_at_ms, created_at) \
             VALUES ('old-party', 'Old Party', 'host-1', 'm1', 'host_only', 1, 'waiting', 0, 0, ?)",
        )
        .bind(old_time)
        .execute(&store.pool)
        .await
        .unwrap();

        let old_invite_time = now - 700_000;
        query(
            "INSERT INTO watch_party_invites (id, party_id, token, created_by, target_user_id, expires_at, created_at) \
             VALUES ('inv-1', 'old-party', 'token-123', 'host-1', 'user-2', ?, ?)",
        )
        .bind(old_invite_time)
        .bind(old_invite_time)
        .execute(&store.pool)
        .await
        .unwrap();

        let affected = clean_expired_parties(&store).await.unwrap();
        assert_eq!(affected, 1);

        let row = query("SELECT state, ended_at FROM watch_parties WHERE id = 'old-party'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let state: String = row.get("state");
        let ended_at: Option<i64> = row.get("ended_at");
        assert_eq!(state, "ended");
        assert!(ended_at.is_some());

        let invite_count: i64 =
            query("SELECT COUNT(*) as count FROM watch_party_invites WHERE id = 'inv-1'")
                .fetch_one(&store.pool)
                .await
                .unwrap()
                .get("count");
        assert_eq!(invite_count, 0);
    }

    #[tokio::test]
    async fn test_access_media_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let db_path = temp.path().join("perm.sqlite");
        let store = IdentityStore::open(&db_path).await.unwrap();

        query("INSERT INTO users (id, role, display_name) VALUES ('admin-1', 'administrator', 'Admin'), ('user-1', 'user', 'User 1')")
            .execute(&store.pool)
            .await
            .unwrap();

        query(
            "INSERT INTO media_nodes (id, name, base_url, status, shared_secret) \
             VALUES ('local', 'Local Node', 'http://127.0.0.1:8080', 'connected', 'secret') \
             ON CONFLICT(id) DO UPDATE SET status = 'connected'",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        query(
            "INSERT INTO libraries (id, name, kind, node_id, root, enabled) \
             VALUES ('lib-1', 'Films', 'films', 'local', '/media', 1)",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        query(
            "INSERT INTO inventory_works (id, library_id, identity_key, title, external_ids_json) \
             VALUES ('work-1', 'lib-1', 'work-1-key', 'Test Movie', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        // 1. Admin has access
        assert!(can_user_access_media(&store, "admin-1", &Role::Administrator, "work-1").await);

        // 2. Nonexistent media -> false even for admin
        assert!(!can_user_access_media(&store, "admin-1", &Role::Administrator, "work-404").await);

        // 3. User without permission -> false
        assert!(!can_user_access_media(&store, "user-1", &Role::User, "work-1").await);

        // 4. Grant permission -> user now has access
        query("INSERT INTO library_permissions (library_id, user_id) VALUES ('lib-1', 'user-1')")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(can_user_access_media(&store, "user-1", &Role::User, "work-1").await);

        // 5. Episode of work-1 is accessible via episode ID
        query(
            "INSERT INTO inventory_seasons (id, series_id, season_number, title) \
             VALUES ('season-1', 'work-1', 1, 'Saison 1')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        query(
            "INSERT INTO inventory_episodes (id, season_id, episode_number, work_id) \
             VALUES ('ep-101', 'season-1', 1, 'work-1')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        assert!(can_user_access_media(&store, "user-1", &Role::User, "ep-101").await);
        assert!(!can_user_access_media(&store, "user-1", &Role::User, "ep-404").await);
    }
}
