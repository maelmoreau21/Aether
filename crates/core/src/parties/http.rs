use super::{access::*, error::*, hub::*, types::*, util::*, ws::*};
use crate::{
    auth::AuthState,
    database::{begin_write, query, query_as, query_scalar},
    identity::Role,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

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
pub(super) struct CreatePartyInput {
    pub name: Option<String>,
    pub media_id: String,
    pub control_mode: Option<String>,
    pub wait_for_buffering: Option<bool>,
    /// Public (the default): anyone who may see the title finds and joins it.
    /// Private: invited users only.
    pub is_public: Option<bool>,
    pub invite_user_ids: Option<Vec<String>>,
    pub invite_usernames: Option<Vec<String>>,
}

pub(super) async fn create_party(
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
            (SELECT w.title FROM inventory_episodes ep \
             JOIN inventory_works w ON w.id = ep.work_id WHERE ep.id = ?), \
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
    let is_locked = !input.is_public.unwrap_or(true);

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
         VALUES (?, ?, ?, ?, ?, 'waiting', 0, ?, ?, ?, ?)",
    )
    .bind(&party_id)
    .bind(&party_name)
    .bind(&actor.user.id)
    .bind(media_id)
    .bind(control_mode.as_str())
    .bind(now_ms)
    .bind(i64::from(is_locked))
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
        "isLocked": is_locked,
        "state": "waiting",
        "createdAt": now
    })))
}

pub(super) async fn list_parties(
    State((auth, _)): State<(AuthState, WatchPartyHub)>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    // A party nobody is connected to any more is over: never list it as in progress.
    let _ = super::cleanup::end_abandoned_parties(&auth.store).await;
    let presence = room_presence().await;
    let now = current_epoch_s();

    // Parties where user is member, host, or invited to, plus the open (unlocked)
    // parties someone is watching right now: anyone who may see the title may join them.
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
                OR p.is_locked = 0 \
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
        let online_count = presence.get(&id).map_or(0, |room| room.online);
        let own = host_id == actor.user.id || is_member != 0;
        // Someone else's open party is only offered while it is being watched.
        let invited = || async {
            let count: i64 = query_scalar(
                "SELECT COUNT(*) FROM watch_party_invites WHERE party_id = ? AND (target_user_id = ? OR target_user_id IS NULL) AND expires_at > ? AND revoked_at IS NULL",
            )
            .bind(&id)
            .bind(&actor.user.id)
            .bind(now)
            .fetch_one(&auth.store.pool)
            .await
            .unwrap_or(0);
            count > 0
        };
        let invited_or_own = own || is_locked != 0 || invited().await;
        if !invited_or_own && online_count == 0 {
            continue;
        }
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
                online_count,
                created_at: created,
                is_host: host_id == actor.user.id,
                is_member: is_member != 0,
            });
        }
    }

    Ok(Json(json!({ "parties": parties })))
}

pub(super) async fn get_party(
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

pub(super) async fn join_party(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, PartyApiError> {
    let actor = auth.authenticate(&headers).await?;
    auth.require_csrf(&headers, &actor)?;

    let room_arc = hub.get_or_load_room(&id).await.ok_or_else(|| {
        PartyApiError(
            StatusCode::NOT_FOUND,
            "Cette séance est terminée ou n'existe plus.".to_string(),
        )
    })?;
    let mut room = room_arc.write().await;

    if room.state == PartyState::Ended {
        return Err(StatusCode::GONE.into());
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
        return Err(PartyApiError(
            StatusCode::FORBIDDEN,
            "Votre compte n'a pas accès à ce titre (bibliothèque ou contrôle parental)."
                .to_string(),
        ));
    }

    // A private (locked) session admits its members and the users it invited.
    let invited = || async {
        let count: i64 = query_scalar(
            "SELECT COUNT(*) FROM watch_party_invites WHERE party_id = ? AND target_user_id = ? AND expires_at > ? AND revoked_at IS NULL",
        )
        .bind(&id)
        .bind(&actor.user.id)
        .bind(current_epoch_s())
        .fetch_one(&auth.store.pool)
        .await
        .unwrap_or(0);
        count > 0
    };
    if room.is_locked && !room.participants.contains_key(&actor.user.id) && !invited().await {
        return Err(PartyApiError(
            StatusCode::FORBIDDEN,
            "La séance est verrouillée : demandez une invitation à l'hôte.".to_string(),
        ));
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
    .map_err(|_| PartyApiError::from(StatusCode::INTERNAL_SERVER_ERROR))?;

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

pub(super) async fn leave_party(
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

        // The host role passes to someone still connected; nobody left ends the party.
        if after_member_left(&mut room, &actor.user.id, &auth.store).await {
            drop(room);
            hub.evict_room(&id).await;
        }
    }

    let _ = query("DELETE FROM watch_party_members WHERE party_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&actor.user.id)
        .execute(&auth.store.pool)
        .await;

    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn end_party(
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
pub(super) struct UpdateSettingsInput {
    pub name: Option<String>,
    pub control_mode: Option<String>,
    pub is_locked: Option<bool>,
    pub wait_for_buffering: Option<bool>,
}

pub(super) async fn update_party_settings(
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
pub(super) struct CreateInviteInput {
    pub target_user_id: Option<String>,
    pub expires_in_seconds: Option<i64>,
}

pub(super) async fn create_party_invite(
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

pub(super) async fn list_party_invites(
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

pub(super) async fn revoke_party_invite(
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

pub(super) async fn list_user_invites(
    State((auth, _)): State<(AuthState, WatchPartyHub)>,
    headers: HeaderMap,
) -> Result<Json<Value>, Error> {
    let actor = auth.authenticate(&headers).await?;
    let _ = super::cleanup::end_abandoned_parties(&auth.store).await;
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

pub(super) async fn accept_invite(
    State((auth, hub)): State<(AuthState, WatchPartyHub)>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, PartyApiError> {
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
        return Err(StatusCode::GONE.into());
    }

    if let Some(target) = target_user_id {
        if target != actor.user.id {
            return Err(StatusCode::FORBIDDEN.into());
        }
    }

    let room_arc = hub
        .get_or_load_room(&party_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let mut room = room_arc.write().await;

    if room.state == PartyState::Ended {
        return Err(StatusCode::GONE.into());
    }

    if !can_user_access_media(
        &auth.store,
        &actor.user.id,
        &actor.user.role,
        &room.media_id,
    )
    .await
    {
        return Err(StatusCode::FORBIDDEN.into());
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
pub(super) struct EligibleQuery {
    pub media_id: Option<String>,
    pub q: Option<String>,
}

pub(super) async fn eligible_users(
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
