use super::{types::*, util::*};
use crate::{
    database::{query_as, query_scalar},
    identity::IdentityStore,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};
use tokio::sync::{RwLock, broadcast};

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
    pub(super) connection_counts: HashMap<String, usize>,
    pub messages: VecDeque<ChatMessagePayload>,
    pub queue: Vec<QueueItemPayload>,
    pub current_poll: Option<PollPayload>,
    pub seq: u64,
    pub paused_for_buffering: bool,
    /// Last moment someone was connected (or the room was created/loaded).
    /// A room nobody has been connected to for a while is abandoned.
    pub last_online_ms: i64,
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
            connection_counts: HashMap::new(),
            messages: VecDeque::with_capacity(MAX_STORED_MESSAGES),
            queue: Vec::new(),
            current_poll: None,
            seq: 1,
            paused_for_buffering: false,
            last_online_ms: current_epoch_ms(),
            broadcast_tx,
        };
        (room, rx)
    }

    /// Calculate the current live playhead position based on server time.
    pub fn computed_position(&self) -> f64 {
        if self.state == PartyState::Playing {
            let elapsed = (current_epoch_ms() - self.position_updated_ms).max(0) as f64 / 1000.0;
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

    pub(super) fn resume_after_buffering(&mut self, user_id: &str, display_name: &str) -> bool {
        if self.state != PartyState::Paused
            || !self.paused_for_buffering
            || self
                .participants
                .values()
                .any(|p| self.online_user_ids.contains(&p.user_id) && p.is_buffering)
        {
            return false;
        }
        self.paused_for_buffering = false;
        self.state = PartyState::Playing;
        self.position_updated_ms = current_epoch_ms();
        self.seq += 1;
        let _ = self.broadcast_tx.send(ServerMessage::PlaybackUpdate {
            state: "playing".into(),
            position: self.position_s,
            server_epoch_ms: self.position_updated_ms,
            caused_by_user_id: user_id.into(),
            caused_by_name: display_name.into(),
            seq: self.seq,
        });
        true
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
            position: self.position_s,
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

/// After `user_id` left `room`: hands the host role to someone still connected, or ends
/// the party when nobody is connected any more. Returns true when the party ended (the
/// caller then evicts the room).
pub(super) async fn after_member_left(
    room: &mut PartyRoom,
    user_id: &str,
    store: &IdentityStore,
) -> bool {
    if room.online_user_ids.is_empty() {
        room.state = PartyState::Ended;
        let _ = room.broadcast_tx.send(ServerMessage::PartyEnded {
            reason: "Tout le monde a quitté la séance.".to_string(),
        });
        let _ = crate::database::query(
            "UPDATE watch_parties SET state = 'ended', ended_at = ? WHERE id = ? AND ended_at IS NULL",
        )
        .bind(current_epoch_s())
        .bind(&room.id)
        .execute(&store.pool)
        .await;
        return true;
    }
    if room.host_user_id != user_id {
        return false;
    }
    let mut online: Vec<&String> = room.online_user_ids.iter().collect();
    online.sort();
    let Some(next_host_id) = online.first().map(|id| (*id).clone()) else {
        return false;
    };
    let next_name = room
        .participants
        .get(&next_host_id)
        .map(|p| p.display_name.clone())
        .unwrap_or_else(|| next_host_id.clone());
    room.host_user_id = next_host_id.clone();
    if let Some(p) = room.participants.get_mut(&next_host_id) {
        p.role = "host".to_string();
    }
    let _ = crate::database::query("UPDATE watch_parties SET host_user_id = ? WHERE id = ?")
        .bind(&next_host_id)
        .bind(&room.id)
        .execute(&store.pool)
        .await;
    let _ = crate::database::query(
        "UPDATE watch_party_members SET role = 'host' WHERE party_id = ? AND user_id = ?",
    )
    .bind(&room.id)
    .bind(&next_host_id)
    .execute(&store.pool)
    .await;
    let _ = room.broadcast_tx.send(ServerMessage::HostTransferred {
        new_host_id: next_host_id,
        new_host_name: next_name,
    });
    false
}

#[derive(Clone)]
pub struct WatchPartyHub {
    pub store: IdentityStore,
    rooms: Arc<RwLock<HashMap<String, Arc<RwLock<PartyRoom>>>>>,
}

/// Rooms of the server's hub, read by the expiry pass to spare parties someone is watching.
static LIVE_ROOMS: std::sync::OnceLock<Arc<RwLock<HashMap<String, Arc<RwLock<PartyRoom>>>>>> =
    std::sync::OnceLock::new();

/// Drops ended parties from memory so they cannot be joined again, and tells any
/// tab still connected that the party is over.
pub async fn forget_rooms(ids: &[String]) {
    let Some(rooms) = LIVE_ROOMS.get() else {
        return;
    };
    let removed: Vec<Arc<RwLock<PartyRoom>>> = {
        let mut rooms = rooms.write().await;
        ids.iter().filter_map(|id| rooms.remove(id)).collect()
    };
    for room in removed {
        let mut room = room.write().await;
        room.state = PartyState::Ended;
        let _ = room.broadcast_tx.send(ServerMessage::PartyEnded {
            reason: "La séance a pris fin faute d'activité.".to_string(),
        });
    }
}

/// When this process's hub started (epoch seconds), 0 before. A party loaded from the
/// database after a restart is not judged abandoned before its members could reconnect.
static HUB_STARTED_S: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

pub fn hub_started_s() -> i64 {
    HUB_STARTED_S.get().copied().unwrap_or(0)
}

/// Presence of a room held in memory.
#[derive(Debug, Clone, Copy)]
pub struct RoomPresence {
    pub online: usize,
    pub last_online_ms: i64,
}

/// Presence of every room held in memory, by party id.
pub async fn room_presence() -> HashMap<String, RoomPresence> {
    let Some(rooms) = LIVE_ROOMS.get() else {
        return HashMap::new();
    };
    let rooms: Vec<(String, Arc<RwLock<PartyRoom>>)> = rooms
        .read()
        .await
        .iter()
        .map(|(id, room)| (id.clone(), room.clone()))
        .collect();
    let mut presence = HashMap::with_capacity(rooms.len());
    for (id, room) in rooms {
        let room = room.read().await;
        let online = room.online_user_ids.len();
        presence.insert(
            id,
            RoomPresence {
                online,
                last_online_ms: if online > 0 {
                    current_epoch_ms()
                } else {
                    room.last_online_ms
                },
            },
        );
    }
    presence
}

impl WatchPartyHub {
    pub fn new(store: IdentityStore) -> Self {
        // One room map per process: every hub (the server builds one; tests may build
        // several) shares it, so presence and expiry always see the rooms in use.
        let rooms = LIVE_ROOMS
            .get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
            .clone();
        let _ = HUB_STARTED_S.set(current_epoch_s());
        Self { store, rooms }
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
