use super::{access::*, cleanup::*, hub::*, types::*, util::*, ws::*};
use crate::{
    database::query,
    identity::{IdentityStore, Role},
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

use sqlx::Row;

#[test]
fn websocket_contract_uses_browser_camel_case_fields() {
    for raw in [
        json!({"type":"ping", "clientEpochMs":123}),
        json!({"type":"changeMedia", "mediaId":"movie"}),
        json!({"type":"moveQueueItem", "queueId":"q", "newIndex":2}),
        json!({"type":"votePoll", "pollId":"p", "optionIndex":1}),
        json!({"type":"grantControl", "userId":"u"}),
        json!({"type":"setSettings", "controlMode":"shared", "isLocked":true, "waitForBuffering":false}),
    ] {
        let event: ClientMessage = serde_json::from_value(raw.clone()).unwrap();
        let encoded = serde_json::to_value(event).unwrap();
        for (key, value) in raw.as_object().unwrap() {
            assert_eq!(&encoded[key], value, "field {key}");
        }
    }
    let pong = serde_json::to_value(ServerMessage::Pong {
        client_epoch_ms: 123,
        server_epoch_ms: 456,
    })
    .unwrap();
    assert_eq!(
        pong,
        json!({"type":"pong", "clientEpochMs":123, "serverEpochMs":456})
    );
}

#[test]
fn reconnect_snapshot_interpolates_once_and_countdown_keeps_base_position() {
    let (mut room, _) = PartyRoom::new(
        "p".into(),
        "Room".into(),
        "h".into(),
        "m".into(),
        "Movie".into(),
        ControlMode::HostOnly,
        true,
        current_epoch_s(),
    );
    room.state = PartyState::Playing;
    room.position_s = 50.0;
    room.position_updated_ms = current_epoch_ms() - 10_000;
    assert_eq!(room.to_state_payload("Host".into()).position, 50.0);
    assert!((room.computed_position() - 60.0).abs() < 0.5);
    room.position_updated_ms = current_epoch_ms() + 3000;
    assert_eq!(room.computed_position(), 50.0);
}

#[tokio::test]
async fn two_clients_receive_authoritative_controls_and_buffering_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("controls.sqlite"))
        .await
        .unwrap();
    let hub = WatchPartyHub::new(store);
    let (mut room, mut host_rx) = PartyRoom::new(
        "p".into(),
        "Room".into(),
        "host".into(),
        "m".into(),
        "Movie".into(),
        ControlMode::HostOnly,
        true,
        current_epoch_s(),
    );
    for id in ["host", "follower"] {
        room.online_user_ids.insert(id.into());
        room.participants.insert(
            id.into(),
            LiveParticipant {
                user_id: id.into(),
                display_name: id.into(),
                role: "participant".into(),
                is_ready: true,
                is_buffering: false,
                last_position: 0.0,
                last_seen_ms: current_epoch_ms(),
                joined_at: current_epoch_s(),
            },
        );
    }
    let mut follower_rx = room.broadcast_tx.subscribe();
    let room = Arc::new(RwLock::new(room));
    let (out, mut errors) = tokio::sync::mpsc::channel(16);
    for (raw, expected) in [
        (json!({"type":"play","position":12}), "playing"),
        (json!({"type":"seek","position":45}), "playing"),
        (json!({"type":"pause","position":45}), "paused"),
    ] {
        handle_client_event(
            serde_json::from_value(raw).unwrap(),
            &room,
            "host",
            "Host",
            &out,
            &hub,
        )
        .await;
        let a = serde_json::to_value(host_rx.recv().await.unwrap()).unwrap();
        let b = serde_json::to_value(follower_rx.recv().await.unwrap()).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["state"], expected);
        assert!(a["serverEpochMs"].is_i64());
    }
    handle_client_event(
        ClientMessage::Seek { position: 99.0 },
        &room,
        "follower",
        "Follower",
        &out,
        &hub,
    )
    .await;
    assert!(errors.recv().await.is_some());
    assert_eq!(room.read().await.position_s, 45.0);
    handle_client_event(
        ClientMessage::Play {
            position: 45.0,
            countdown: None,
        },
        &room,
        "host",
        "Host",
        &out,
        &hub,
    )
    .await;
    handle_client_event(
        ClientMessage::SetBuffering {
            buffering: true,
            position: 46.0,
        },
        &room,
        "follower",
        "Follower",
        &out,
        &hub,
    )
    .await;
    assert_eq!(room.read().await.state, PartyState::Paused);
    handle_client_event(
        ClientMessage::SetBuffering {
            buffering: false,
            position: 46.0,
        },
        &room,
        "follower",
        "Follower",
        &out,
        &hub,
    )
    .await;
    assert_eq!(room.read().await.state, PartyState::Playing);
    handle_client_event(
        ClientMessage::SetBuffering {
            buffering: true,
            position: 47.0,
        },
        &room,
        "follower",
        "Follower",
        &out,
        &hub,
    )
    .await;
    handle_client_event(
        ClientMessage::Pause { position: 47.0 },
        &room,
        "host",
        "Host",
        &out,
        &hub,
    )
    .await;
    handle_client_event(
        ClientMessage::SetBuffering {
            buffering: false,
            position: 47.0,
        },
        &room,
        "follower",
        "Follower",
        &out,
        &hub,
    )
    .await;
    assert_eq!(room.read().await.state, PartyState::Paused);
}

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

    query("INSERT INTO users (id, role, display_name) VALUES ('host-user', 'user', 'Host User')")
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

/// A session names a film, an episode or a series; the player needs one of its files.
#[tokio::test]
async fn a_party_title_resolves_to_its_first_playable_file() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("resolve.sqlite"))
        .await
        .unwrap();
    for sql in [
        "INSERT INTO media_nodes (id, name, base_url, status, shared_secret) VALUES ('local', 'Node', 'http://127.0.0.1:8080', 'connected', 'sec') ON CONFLICT(id) DO UPDATE SET status='connected'",
        "INSERT INTO libraries (id, name, kind, node_id, root, enabled) VALUES ('lib-1', 'Mixte', 'series', 'local', '/media', 1)",
        "INSERT INTO inventory_works (id, library_id, identity_key, title, external_ids_json, kind) VALUES \
         ('film', 'lib-1', 'k-film', 'Film', '{}', 'film'), ('show', 'lib-1', 'k-show', 'Série', '{}', 'series'), \
         ('sp', 'lib-1', 'k-sp', 'Spécial', '{}', 'episode'), ('e2', 'lib-1', 'k-e2', 'S1E2', '{}', 'episode'), \
         ('e1', 'lib-1', 'k-e1', 'S1E1', '{}', 'episode'), ('empty', 'lib-1', 'k-empty', 'Vide', '{}', 'film')",
        "INSERT INTO inventory_seasons (id, series_id, season_number) VALUES ('s0', 'show', 0), ('s1', 'show', 1)",
        "INSERT INTO inventory_episodes (id, season_id, episode_number, work_id) VALUES ('ep-sp', 's0', 1, 'sp'), ('ep-2', 's1', 2, 'e2'), ('ep-1', 's1', 1, 'e1')",
    ] {
        query(sql).execute(&store.pool).await.unwrap();
    }
    for work in ["film", "sp", "e2", "e1"] {
        for sql in [
            "INSERT INTO inventory_editions (id, work_id) VALUES ('ed-' || ?, ?)",
            "INSERT INTO inventory_versions (id, edition_id) VALUES ('v-' || ?, 'ed-' || ?)",
            "INSERT INTO inventory_sources (root_snapshot, id, version_id, library_id) VALUES ('/media', 'src-' || ?, 'v-' || ?, 'lib-1')",
            "INSERT INTO inventory_assets (id, source_id, library_id, relative_path, size, modified_ns, mime, probe_json, available) VALUES ('a-' || ?, 'src-' || ?, 'lib-1', 'f.mkv-' || ?, 1, '0', 'video/x-matroska', '{}', 1)",
        ] {
            let mut statement = query(sql).bind(work).bind(work);
            if sql.contains("inventory_assets") {
                statement = statement.bind(work);
            }
            statement.execute(&store.pool).await.unwrap();
        }
    }

    let resolve = |id: &'static str| {
        let store = store.clone();
        async move {
            crate::inventory::playable_asset_for_work(&store, id)
                .await
                .unwrap()
        }
    };
    assert_eq!(resolve("film").await.as_deref(), Some("a-film"));
    assert_eq!(resolve("e2").await.as_deref(), Some("a-e2"));
    assert_eq!(resolve("ep-2").await.as_deref(), Some("a-e2"));
    // A series starts at its first regular episode, specials last.
    assert_eq!(resolve("show").await.as_deref(), Some("a-e1"));
    assert_eq!(resolve("empty").await, None);
    assert_eq!(resolve("missing").await, None);
}

#[tokio::test]
async fn test_queue_reordering_and_playing_from_queue() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("queue_test.sqlite");
    let store = IdentityStore::open(&db_path).await.unwrap();
    let hub = WatchPartyHub::new(store.clone());

    query("INSERT INTO users (id, role, display_name) VALUES ('host-1', 'user', 'Host')")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO media_nodes (id, name, base_url, status, shared_secret) VALUES ('local', 'Node', 'http://127.0.0.1:8080', 'connected', 'sec') ON CONFLICT(id) DO UPDATE SET status='connected'")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO libraries (id, name, kind, node_id, root, enabled) VALUES ('lib-1', 'Films', 'films', 'local', '/media', 1)")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO library_permissions (library_id, user_id) VALUES ('lib-1', 'host-1')")
        .execute(&store.pool)
        .await
        .unwrap();
    query("INSERT INTO inventory_works (id, library_id, identity_key, title, external_ids_json) VALUES ('w1', 'lib-1', 'k1', 'Film 1', '{}'), ('w2', 'lib-1', 'k2', 'Film 2', '{}'), ('w3', 'lib-1', 'k3', 'Film 3', '{}')")
        .execute(&store.pool)
        .await
        .unwrap();

    let now = current_epoch_s();
    query("INSERT INTO watch_parties (id, name, host_user_id, media_id, control_mode, wait_for_buffering, state, current_position_ms, position_updated_at_ms, created_at) VALUES ('party-q', 'Queue Test Room', 'host-1', 'w1', 'host_only', 1, 'waiting', 0, ?, ?)")
        .bind(now * 1000)
        .bind(now)
        .execute(&store.pool)
        .await
        .unwrap();

    let room_arc = hub
        .get_or_load_room("party-q")
        .await
        .expect("Room should be loaded");

    let (out_tx, mut _out_rx) = tokio::sync::mpsc::channel(16);

    // 1. Add w2 and w3 to queue
    handle_client_event(
        ClientMessage::AddToQueue {
            media_id: "w2".into(),
        },
        &room_arc,
        "host-1",
        "Host",
        &out_tx,
        &hub,
    )
    .await;

    handle_client_event(
        ClientMessage::AddToQueue {
            media_id: "w3".into(),
        },
        &room_arc,
        "host-1",
        "Host",
        &out_tx,
        &hub,
    )
    .await;

    {
        let room = room_arc.read().await;
        assert_eq!(room.queue.len(), 2);
        assert_eq!(room.queue[0].media_id, "w2");
        assert_eq!(room.queue[1].media_id, "w3");
    }

    // 2. Move w3 to the top (index 0)
    let q_id_w3 = { room_arc.read().await.queue[1].id.clone() };
    handle_client_event(
        ClientMessage::MoveQueueItem {
            queue_id: q_id_w3.clone(),
            new_index: 0,
        },
        &room_arc,
        "host-1",
        "Host",
        &out_tx,
        &hub,
    )
    .await;

    {
        let room = room_arc.read().await;
        assert_eq!(room.queue[0].media_id, "w3");
        assert_eq!(room.queue[1].media_id, "w2");
    }

    // 3. Play w3 from queue
    handle_client_event(
        ClientMessage::PlayQueueItem { queue_id: q_id_w3 },
        &room_arc,
        "host-1",
        "Host",
        &out_tx,
        &hub,
    )
    .await;

    {
        let room = room_arc.read().await;
        assert_eq!(room.media_id, "w3");
        assert_eq!(room.media_title, "Film 3");
        assert_eq!(
            room.queue.len(),
            1,
            "Played item must be removed from queue"
        );
        assert_eq!(room.queue[0].media_id, "w2");
    }
}
