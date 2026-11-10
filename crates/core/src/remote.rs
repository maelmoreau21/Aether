//! Remote control between one account's devices: a phone drives the TV,
//! "resume on…" hands playback to another device at the same position.
//!
//! `GET /api/v1/remote/ws` (cookie or device token). The first message names
//! the device; afterwards each device reports what it plays and may send a
//! command to another device **of the same account**. Every connected device
//! receives the list of the account's other devices whenever it changes.
//!
//! Client → Core:
//! - `{"type":"hello","deviceId","name","kind"}` (`kind`: phone, tv, web, desktop)
//! - `{"type":"state","playing":{"workId","title","position","duration","paused"}|null}`
//! - `{"type":"command","to","action","position"?,"workId"?}`
//!   (`action`: play, pause, seek, stop, open)
//!
//! Core → client:
//! - `{"type":"devices","devices":[{"deviceId","name","kind","playing"}]}`
//! - `{"type":"command","from","fromName","action","position"?,"workId"?}`
//!
//! The registry lives in this Core's memory: devices connected to two different
//! Cores of a multi-Core deployment do not see each other.
use crate::auth::AuthState;
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

const MAX_DEVICES_PER_USER: usize = 16;
const MAX_MESSAGE_BYTES: usize = 4096;
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Commands per second a device may send: a held button, never a flood.
const COMMANDS_PER_SECOND: u32 = 10;

#[derive(Clone)]
struct RemoteState {
    auth: AuthState,
    hub: Arc<Hub>,
}

struct Device {
    /// Distinguishes a reconnection of the same device from the old socket.
    connection: u64,
    name: String,
    kind: String,
    playing: Value,
    sender: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Hub {
    users: Mutex<HashMap<String, HashMap<String, Device>>>,
    next: std::sync::atomic::AtomicU64,
}

impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<String, Device>>> {
        self.users.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Sends every device of `user` the list of its *other* devices.
    fn broadcast(&self, user: &str) {
        let users = self.lock();
        let Some(devices) = users.get(user) else {
            return;
        };
        for (id, device) in devices {
            let others: Vec<Value> = devices
                .iter()
                .filter(|(other, _)| *other != id)
                .map(|(other, d)| {
                    json!({"deviceId": other, "name": d.name, "kind": d.kind, "playing": d.playing})
                })
                .collect();
            let _ = device
                .sender
                .send(json!({"type": "devices", "devices": others}).to_string());
        }
    }
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/remote/ws", get(upgrade))
        .with_state(RemoteState {
            auth,
            hub: Arc::new(Hub::default()),
        })
}

async fn upgrade(
    State(state): State<RemoteState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    match state.auth.authenticate(&headers).await {
        Ok(identity) => ws
            .max_message_size(MAX_MESSAGE_BYTES)
            .on_upgrade(move |socket| session(socket, state, identity.user.id))
            .into_response(),
        Err(status) => crate::error(status, "authentication_required"),
    }
}

/// A printable identifier or label of bounded length, or `None`.
fn field(value: &Value, key: &str, max: usize, id: bool) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    let valid = !text.is_empty()
        && text.chars().count() <= max
        && text.chars().all(|c| {
            if id {
                c.is_ascii_alphanumeric() || c == '-' || c == '_'
            } else {
                !c.is_control()
            }
        });
    valid.then(|| text.to_owned())
}
fn seconds(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)?
        .as_f64()
        .filter(|s| s.is_finite() && (0.0..=86_400.0).contains(s))
}

/// What a device reports it plays, keeping only known, bounded fields.
fn playing(message: &Value) -> Value {
    let Some(now) = message.get("playing").filter(|p| p.is_object()) else {
        return Value::Null;
    };
    let (Some(work), Some(position)) = (field(now, "workId", 64, true), seconds(now, "position"))
    else {
        return Value::Null;
    };
    json!({
        "workId": work,
        "title": field(now, "title", 200, false),
        "position": position,
        "duration": seconds(now, "duration"),
        "paused": now.get("paused").and_then(Value::as_bool).unwrap_or(false),
    })
}

#[derive(Deserialize)]
struct Hello {
    #[serde(rename = "type")]
    kind_of_message: String,
}

async fn session(socket: WebSocket, state: RemoteState, user: String) {
    let (mut sink, mut stream) = socket.split();
    // The device introduces itself first; anything else closes the socket.
    let hello = match tokio::time::timeout(HELLO_TIMEOUT, stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text).ok(),
        _ => None,
    };
    let Some(hello) = hello.filter(|h| {
        serde_json::from_value::<Hello>(h.clone()).is_ok_and(|h| h.kind_of_message == "hello")
    }) else {
        return;
    };
    let (Some(device), Some(name)) = (
        field(&hello, "deviceId", 64, true),
        field(&hello, "name", 64, false),
    ) else {
        return;
    };
    let kind = field(&hello, "kind", 16, true)
        .filter(|k| matches!(k.as_str(), "phone" | "tv" | "web" | "desktop"))
        .unwrap_or_else(|| "phone".to_owned());

    let (sender, mut outbox) = mpsc::unbounded_channel::<String>();
    let connection = state
        .hub
        .next
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut users = state.hub.lock();
        let devices = users.entry(user.clone()).or_default();
        if !devices.contains_key(&device) && devices.len() >= MAX_DEVICES_PER_USER {
            return;
        }
        // A reconnection replaces the previous socket of the same device.
        devices.insert(
            device.clone(),
            Device {
                connection,
                name: name.clone(),
                kind,
                playing: Value::Null,
                sender,
            },
        );
    }
    state.hub.broadcast(&user);

    let writer = tokio::spawn(async move {
        while let Some(text) = outbox.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let mut window = tokio::time::Instant::now();
    let mut commands = 0u32;
    while let Some(Ok(message)) = stream.next().await {
        let Message::Text(text) = message else {
            if matches!(message, Message::Close(_)) {
                break;
            }
            continue;
        };
        let Ok(message) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match message.get("type").and_then(Value::as_str) {
            Some("state") => {
                let now = playing(&message);
                let changed = {
                    let mut users = state.hub.lock();
                    match users.get_mut(&user).and_then(|d| d.get_mut(&device)) {
                        Some(entry) if entry.connection == connection && entry.playing != now => {
                            entry.playing = now;
                            true
                        }
                        _ => false,
                    }
                };
                if changed {
                    state.hub.broadcast(&user);
                }
            }
            Some("command") => {
                if window.elapsed() >= Duration::from_secs(1) {
                    window = tokio::time::Instant::now();
                    commands = 0;
                }
                commands += 1;
                if commands > COMMANDS_PER_SECOND {
                    continue;
                }
                let Some(target) = field(&message, "to", 64, true) else {
                    continue;
                };
                let Some(action) = field(&message, "action", 8, true)
                    .filter(|a| matches!(a.as_str(), "play" | "pause" | "seek" | "stop" | "open"))
                else {
                    continue;
                };
                let position = seconds(&message, "position");
                let work = field(&message, "workId", 64, true);
                // seek needs a position; open needs a work.
                if (action == "seek" && position.is_none()) || (action == "open" && work.is_none())
                {
                    continue;
                }
                let users = state.hub.lock();
                // Only the same account's devices are reachable.
                if let Some(to) = users.get(&user).and_then(|d| d.get(&target)) {
                    let _ = to.sender.send(
                        json!({"type": "command", "from": device, "fromName": name, "action": action,
                               "position": position, "workId": work})
                        .to_string(),
                    );
                }
            }
            _ => {}
        }
    }

    writer.abort();
    let left = {
        let mut users = state.hub.lock();
        let mut left = false;
        if let Some(devices) = users.get_mut(&user) {
            if devices
                .get(&device)
                .is_some_and(|d| d.connection == connection)
            {
                devices.remove(&device);
                left = true;
            }
            if devices.is_empty() {
                users.remove(&user);
            }
        }
        left
    };
    if left {
        state.hub.broadcast(&user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_playback_is_validated_and_bounded() {
        let ok = json!({"playing":{"workId":"w-1","title":"Film","position":12.5,"duration":600.0,"paused":true,"extra":"x"}});
        assert_eq!(
            playing(&ok),
            json!({"workId":"w-1","title":"Film","position":12.5,"duration":600.0,"paused":true})
        );
        for bad in [
            json!({"playing":null}),
            json!({"playing":{"workId":"a/b","position":1.0}}),
            json!({"playing":{"workId":"w","position":-1.0}}),
            json!({"playing":{"workId":"w","position":1e12}}),
        ] {
            assert_eq!(playing(&bad), Value::Null, "{bad}");
        }
        assert_eq!(field(&json!({"n":"Salon\nTV"}), "n", 64, false), None);
        assert_eq!(field(&json!({"n":"x".repeat(65)}), "n", 64, false), None);
        assert_eq!(
            field(&json!({"n":"  Salon  "}), "n", 64, false).as_deref(),
            Some("Salon")
        );
    }
}
