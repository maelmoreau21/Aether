use serde::{Deserialize, Serialize};

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
    /// Members connected right now.
    #[serde(default)]
    pub online_count: usize,
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
    MoveQueueItem {
        queue_id: String,
        new_index: usize,
    },
    PlayQueueItem {
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
