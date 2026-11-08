use super::{access::*, error::*, hub::*, types::*, util::*};
use crate::{
    auth::{AuthState, Authenticated},
    database::{query, query_scalar},
};
use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Real-Time WebSocket Handler
// ---------------------------------------------------------------------------

pub(super) async fn ws_handler(
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
        // Joining is a separate authenticated REST action with invitation and
        // lock checks. A socket must not bypass it or restore a kicked member.
        if room.state == PartyState::Ended || !room.participants.contains_key(&actor.user.id) {
            return Err(StatusCode::FORBIDDEN);
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
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, room_arc, actor, hub)))
}

pub(super) async fn handle_socket(
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
        *room.connection_counts.entry(user_id.clone()).or_default() += 1;
        room.last_online_ms = current_epoch_ms();

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
        if let Some(member) = room
            .members_payload()
            .into_iter()
            .find(|p| p.user_id == user_id)
        {
            let _ = room
                .broadcast_tx
                .send(ServerMessage::MemberJoined { member });
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
        loop {
            let msg = match rx.recv().await {
                Ok(msg) => msg,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Trigger a reconnect and fresh snapshot instead of leaving
                    // a connected client permanently missing playback events.
                    let _ = out_tx_clone.send(Message::Close(None)).await;
                    break;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            };
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
        let remaining = room
            .connection_counts
            .get_mut(&user_id)
            .map(|count| {
                *count = count.saturating_sub(1);
                *count
            })
            .unwrap_or(0);
        if remaining == 0 {
            room.connection_counts.remove(&user_id);
            room.online_user_ids.remove(&user_id);
            if room.resume_after_buffering(&user_id, &display_name) {
                let _ = query("UPDATE watch_parties SET state = 'playing', current_position_ms = ?, position_updated_at_ms = ? WHERE id = ?")
                    .bind((room.position_s * 1000.0) as i64).bind(room.position_updated_ms).bind(&room.id)
                    .execute(&hub.store.pool).await;
            }
        }
        if let Some(p) = room.participants.get_mut(&user_id) {
            p.last_seen_ms = current_epoch_ms();
        }
        room.last_online_ms = current_epoch_ms();

        if remaining == 0 {
            // Persisted so presence survives a restart of the server.
            let _ = query(
                "UPDATE watch_party_members SET last_seen_at = ? WHERE party_id = ? AND user_id = ?",
            )
            .bind(current_epoch_s())
            .bind(&room.id)
            .bind(&user_id)
            .execute(&hub.store.pool)
            .await;
            let _ = room.broadcast_tx.send(ServerMessage::MemberLeft {
                user_id: user_id.clone(),
                display_name: display_name.clone(),
                reason: Some("Déconnecté".to_string()),
            });
        }
    }

    bcast_task.abort();
    send_task.abort();
}

/// Zero-leak: a title is only announced, queued or played when the actor and
/// every current member may access it (library, media type and parental rules).
async fn media_allowed_for_room(
    hub: &WatchPartyHub,
    room_arc: &Arc<RwLock<PartyRoom>>,
    actor: &str,
    media_id: &str,
) -> bool {
    let mut users: Vec<String> = room_arc
        .read()
        .await
        .participants
        .values()
        .map(|p| p.user_id.clone())
        .collect();
    users.push(actor.to_owned());
    users.sort();
    users.dedup();
    for user in users {
        let role: Option<crate::identity::Role> =
            query_scalar("SELECT role FROM users WHERE id = ?")
                .bind(&user)
                .fetch_optional(&hub.store.pool)
                .await
                .unwrap_or(None);
        let Some(role) = role else { return false };
        if !can_user_access_media(&hub.store, &user, &role, media_id).await {
            return false;
        }
    }
    true
}

pub(super) async fn handle_client_event(
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
                room.paused_for_buffering = true;
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

            // Resume only a pause caused by buffering, after every connected
            // participant has recovered. A manual pause cancels this intent.
            if !buffering && room.resume_after_buffering(user_id, display_name) {
                let _ = query("UPDATE watch_parties SET state = 'playing', current_position_ms = ?, position_updated_at_ms = ? WHERE id = ?")
                    .bind((room.position_s * 1000.0) as i64).bind(room.position_updated_ms).bind(&room.id)
                    .execute(&hub.store.pool).await;
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

            room.paused_for_buffering = false;
            let countdown_s = countdown.unwrap_or(0).min(30);
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
                .bind(room.position_updated_ms)
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

            room.paused_for_buffering = false;
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
            if !media_allowed_for_room(hub, room_arc, user_id, &media_id).await {
                let _ = send_error(
                    out_tx,
                    "MEDIA_FORBIDDEN",
                    "Média non autorisé pour ce groupe",
                )
                .await;
                return;
            }
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
                room.paused_for_buffering = false;
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
            if !media_allowed_for_room(hub, room_arc, user_id, &media_id).await {
                let _ = send_error(
                    out_tx,
                    "MEDIA_FORBIDDEN",
                    "Média non autorisé pour ce groupe",
                )
                .await;
                return;
            }
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

        ClientMessage::MoveQueueItem {
            queue_id,
            new_index,
        } => {
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            if let Some(pos) = room.queue.iter().position(|item| item.id == queue_id) {
                let item = room.queue.remove(pos);
                let target_idx = new_index.min(room.queue.len());
                room.queue.insert(target_idx, item);

                let room_id = room.id.clone();
                for (idx, q_item) in room.queue.iter_mut().enumerate() {
                    q_item.position = idx as i64;
                    let _ = query(
                        "UPDATE watch_party_queue SET position = ? WHERE id = ? AND party_id = ?",
                    )
                    .bind(idx as i64)
                    .bind(&q_item.id)
                    .bind(&room_id)
                    .execute(&hub.store.pool)
                    .await;
                }

                let _ = room.broadcast_tx.send(ServerMessage::QueueUpdated {
                    queue: room.queue.clone(),
                });
            }
        }

        ClientMessage::PlayQueueItem { queue_id } => {
            // Rights may have changed since the item was queued.
            let queued = room_arc
                .read()
                .await
                .queue
                .iter()
                .find(|item| item.id == queue_id)
                .map(|item| item.media_id.clone());
            if let Some(media_id) = queued
                && !media_allowed_for_room(hub, room_arc, user_id, &media_id).await
            {
                let _ = send_error(
                    out_tx,
                    "MEDIA_FORBIDDEN",
                    "Média non autorisé pour ce groupe",
                )
                .await;
                return;
            }
            let mut room = room_arc.write().await;
            if !room.can_user_control(user_id) {
                let _ = send_error(out_tx, "FORBIDDEN", "Contrôles non autorisés").await;
                return;
            }

            if let Some(pos) = room.queue.iter().position(|item| item.id == queue_id) {
                let item = room.queue.remove(pos);
                let media_id = item.media_id.clone();
                let media_title = item.media_title.clone();

                room.media_id = media_id.clone();
                room.media_title = media_title.clone();
                room.state = PartyState::Waiting;
                room.paused_for_buffering = false;
                room.position_s = 0.0;
                room.position_updated_ms = current_epoch_ms();
                room.seq += 1;

                let host_id = room.host_user_id.clone();
                for p in room.participants.values_mut() {
                    p.is_ready = p.user_id == host_id;
                    p.is_buffering = false;
                }

                let _ = room.broadcast_tx.send(ServerMessage::MediaChanged {
                    media_id: media_id.clone(),
                    media_title,
                    caused_by_name: display_name.to_string(),
                });

                let room_id = room.id.clone();
                for (idx, q_item) in room.queue.iter_mut().enumerate() {
                    q_item.position = idx as i64;
                    let _ = query(
                        "UPDATE watch_party_queue SET position = ? WHERE id = ? AND party_id = ?",
                    )
                    .bind(idx as i64)
                    .bind(&q_item.id)
                    .bind(&room_id)
                    .execute(&hub.store.pool)
                    .await;
                }

                let _ = room.broadcast_tx.send(ServerMessage::QueueUpdated {
                    queue: room.queue.clone(),
                });

                let _ = query("UPDATE watch_parties SET media_id = ?, state = 'waiting', current_position_ms = 0, position_updated_at_ms = ? WHERE id = ?")
                    .bind(&media_id)
                    .bind(current_epoch_ms())
                    .bind(&room.id)
                    .execute(&hub.store.pool)
                    .await;

                let _ = query("DELETE FROM watch_party_queue WHERE id = ? AND party_id = ?")
                    .bind(&queue_id)
                    .bind(&room.id)
                    .execute(&hub.store.pool)
                    .await;
            }
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

            // The host role passes to someone still connected; nobody left ends the party.
            if after_member_left(&mut room, user_id, &hub.store).await {
                let id = room.id.clone();
                drop(room);
                hub.evict_room(&id).await;
            }
        }
    }
}

pub(super) async fn send_error(
    out_tx: &tokio::sync::mpsc::Sender<Message>,
    code: &str,
    message: &str,
) {
    let err = ServerMessage::Error {
        code: code.to_string(),
        message: message.to_string(),
    };
    if let Ok(json_str) = serde_json::to_string(&err) {
        let _ = out_tx.send(Message::Text(json_str.into())).await;
    }
}
