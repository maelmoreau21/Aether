//! This computer on the account's remote-control channel (`/api/v1/remote/ws`,
//! see `crates/core/src/remote.rs`): it introduces itself as a `desktop`,
//! reports what it plays, lists the other devices and obeys their commands.
//! The connection lives on the Tokio runtime and reconnects on its own.
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    Connector,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

const FIRST_RETRY: Duration = Duration::from_secs(2);
const LAST_RETRY: Duration = Duration::from_secs(30);
const PING_EVERY: Duration = Duration::from_secs(30);

/// What a device plays, as the Core relays it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playing {
    pub work_id: String,
    pub title: Option<String>,
    pub position: f64,
    pub duration: Option<f64>,
    pub paused: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteDevice {
    pub device_id: String,
    pub name: String,
    pub kind: String,
    pub playing: Option<Playing>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Play,
    Pause,
    Seek(f64),
    Stop,
    /// Open a work, at a position when it is handed over from another device.
    Open {
        work_id: String,
        position: Option<f64>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub from_name: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Devices(Vec<RemoteDevice>),
    Command(Command),
}

/// `http(s)://host/` → `ws(s)://host/api/v1/remote/ws`.
pub fn socket_url(server: &str) -> Option<String> {
    let rest = server.trim_end_matches('/');
    let url = if let Some(host) = rest.strip_prefix("https://") {
        format!("wss://{host}")
    } else {
        format!("ws://{}", rest.strip_prefix("http://")?)
    };
    Some(format!("{url}/api/v1/remote/ws"))
}

pub fn hello(device_id: &str, name: &str) -> String {
    json!({"type": "hello", "deviceId": device_id, "name": name, "kind": "desktop"}).to_string()
}

pub fn state(playing: Option<&Playing>) -> String {
    json!({"type": "state", "playing": playing}).to_string()
}

/// A command for another device of the account.
pub fn command(to: &str, action: &Action) -> String {
    let (name, position, work) = match action {
        Action::Play => ("play", None, None),
        Action::Pause => ("pause", None, None),
        Action::Seek(position) => ("seek", Some(*position), None),
        Action::Stop => ("stop", None, None),
        Action::Open { work_id, position } => ("open", *position, Some(work_id.as_str())),
    };
    let mut message = json!({"type": "command", "to": to, "action": name});
    if let Some(position) = position {
        message["position"] = json!(position);
    }
    if let Some(work) = work {
        message["workId"] = json!(work);
    }
    message.to_string()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}
fn seconds(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)?
        .as_f64()
        .filter(|s| s.is_finite() && *s >= 0.0)
}

/// A message from the Core; anything unknown or malformed is ignored.
pub fn parse(message: &str) -> Option<Incoming> {
    let value: Value = serde_json::from_str(message).ok()?;
    match value.get("type")?.as_str()? {
        "devices" => {
            let devices = value
                .get("devices")?
                .as_array()?
                .iter()
                .filter_map(|d| {
                    let playing = d.get("playing").filter(|p| p.is_object()).and_then(|p| {
                        Some(Playing {
                            work_id: text(p, "workId")?,
                            title: text(p, "title"),
                            position: seconds(p, "position").unwrap_or(0.0),
                            duration: seconds(p, "duration"),
                            paused: p.get("paused").and_then(Value::as_bool).unwrap_or(false),
                        })
                    });
                    Some(RemoteDevice {
                        device_id: text(d, "deviceId")?,
                        name: text(d, "name").unwrap_or_else(|| "Appareil".to_owned()),
                        kind: text(d, "kind").unwrap_or_else(|| "phone".to_owned()),
                        playing,
                    })
                })
                .collect();
            Some(Incoming::Devices(devices))
        }
        "command" => {
            let position = seconds(&value, "position");
            let action = match value.get("action")?.as_str()? {
                "play" => Action::Play,
                "pause" => Action::Pause,
                "stop" => Action::Stop,
                "seek" => Action::Seek(position?),
                "open" => Action::Open {
                    work_id: text(&value, "workId")?,
                    position,
                },
                _ => return None,
            };
            Some(Incoming::Command(Command {
                from_name: text(&value, "fromName").unwrap_or_default(),
                action,
            }))
        }
        _ => None,
    }
}

enum Outgoing {
    State(Option<Playing>),
    Command(String),
}

/// Handle on the connection; dropping it closes the socket for good.
pub struct RemoteLink {
    outbox: mpsc::UnboundedSender<Outgoing>,
}

impl RemoteLink {
    /// Connects in the background. `token` is read at every (re)connection so
    /// that a refreshed token is used. Messages arrive on the returned receiver.
    pub fn start(
        runtime: &tokio::runtime::Handle,
        server: &str,
        device_id: String,
        name: String,
        token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    ) -> Option<(Self, futures::channel::mpsc::UnboundedReceiver<Incoming>)> {
        let url = socket_url(server)?;
        let (outbox, inbox) = mpsc::unbounded_channel();
        let (events, receiver) = futures::channel::mpsc::unbounded();
        runtime.spawn(run(url, device_id, name, token, inbox, events));
        Some((Self { outbox }, receiver))
    }
    pub fn report(&self, playing: Option<Playing>) {
        let _ = self.outbox.send(Outgoing::State(playing));
    }
    pub fn send(&self, to: &str, action: &Action) {
        let _ = self.outbox.send(Outgoing::Command(command(to, action)));
    }
}

async fn run(
    url: String,
    device_id: String,
    name: String,
    token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    mut inbox: mpsc::UnboundedReceiver<Outgoing>,
    events: futures::channel::mpsc::UnboundedSender<Incoming>,
) {
    let mut playing: Option<Playing> = None;
    let mut retry = FIRST_RETRY;
    loop {
        if let Some(access) = token() {
            match session(
                &url,
                &access,
                &device_id,
                &name,
                &mut playing,
                &mut inbox,
                &events,
            )
            .await
            {
                Ended::Closed => return,
                Ended::Connected => retry = FIRST_RETRY,
                Ended::Failed => {}
            }
        }
        // The device list is stale while disconnected.
        if events
            .unbounded_send(Incoming::Devices(Vec::new()))
            .is_err()
        {
            return;
        }
        // Keep the latest state while waiting, and stop when the link is dropped.
        let wait = tokio::time::sleep(retry);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                message = inbox.recv() => match message {
                    Some(Outgoing::State(now)) => playing = now,
                    Some(Outgoing::Command(_)) => {}
                    None => return,
                },
            }
        }
        retry = (retry * 2).min(LAST_RETRY);
    }
}

enum Ended {
    /// The application dropped the link.
    Closed,
    /// The socket worked for a while, then closed.
    Connected,
    Failed,
}

async fn session(
    url: &str,
    access: &str,
    device_id: &str,
    name: &str,
    playing: &mut Option<Playing>,
    inbox: &mut mpsc::UnboundedReceiver<Outgoing>,
    events: &futures::channel::mpsc::UnboundedSender<Incoming>,
) -> Ended {
    let Ok(mut request) = url.into_client_request() else {
        return Ended::Failed;
    };
    let Ok(bearer) = HeaderValue::from_str(&format!("Bearer {access}")) else {
        return Ended::Failed;
    };
    request.headers_mut().insert("authorization", bearer);
    let connector = Connector::Rustls(Arc::new(crate::net::tls()));
    let Ok((socket, _)) =
        tokio_tungstenite::connect_async_tls_with_config(request, None, true, Some(connector))
            .await
    else {
        return Ended::Failed;
    };
    let (mut sink, mut stream) = socket.split();
    let greeting = [hello(device_id, name), state(playing.as_ref())];
    for text in greeting {
        if sink.send(Message::text(text)).await.is_err() {
            return Ended::Failed;
        }
    }
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        tokio::select! {
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if let Some(incoming) = parse(&text) && events.unbounded_send(incoming).is_err() {
                        return Ended::Closed;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Ended::Connected,
                Some(Ok(_)) => {}
            },
            outgoing = inbox.recv() => {
                let text = match outgoing {
                    Some(Outgoing::State(now)) => {
                        *playing = now;
                        state(playing.as_ref())
                    }
                    Some(Outgoing::Command(text)) => text,
                    None => {
                        let _ = sink.send(Message::Close(None)).await;
                        return Ended::Closed;
                    }
                };
                if sink.send(Message::text(text)).await.is_err() {
                    return Ended::Connected;
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return Ended::Connected;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_outgoing_messages_follow_the_core_protocol() {
        assert_eq!(
            socket_url("https://media.example/").as_deref(),
            Some("wss://media.example/api/v1/remote/ws")
        );
        assert_eq!(
            socket_url("http://192.168.1.2:8080").as_deref(),
            Some("ws://192.168.1.2:8080/api/v1/remote/ws")
        );
        assert_eq!(socket_url("ftp://x"), None);
        let hello: Value = serde_json::from_str(&hello("d-1", "Bureau")).unwrap();
        assert_eq!(
            hello,
            json!({"type":"hello","deviceId":"d-1","name":"Bureau","kind":"desktop"})
        );
        let playing = Playing {
            work_id: "w".into(),
            title: Some("Film".into()),
            position: 12.5,
            duration: Some(600.0),
            paused: true,
        };
        let now: Value = serde_json::from_str(&state(Some(&playing))).unwrap();
        assert_eq!(
            now,
            json!({"type":"state","playing":{"workId":"w","title":"Film","position":12.5,"duration":600.0,"paused":true}})
        );
        let idle: Value = serde_json::from_str(&state(None)).unwrap();
        assert_eq!(idle, json!({"type":"state","playing":null}));
        let open: Value = serde_json::from_str(&command(
            "tv",
            &Action::Open {
                work_id: "w".into(),
                position: Some(30.0),
            },
        ))
        .unwrap();
        assert_eq!(
            open,
            json!({"type":"command","to":"tv","action":"open","position":30.0,"workId":"w"})
        );
        let pause: Value = serde_json::from_str(&command("tv", &Action::Pause)).unwrap();
        assert_eq!(pause, json!({"type":"command","to":"tv","action":"pause"}));
    }

    #[test]
    fn incoming_messages_are_validated() {
        let devices = parse(
            r#"{"type":"devices","devices":[
                {"deviceId":"p","name":"Pixel","kind":"phone","playing":null},
                {"deviceId":"t","name":"Salon","kind":"tv","playing":{"workId":"w","title":null,"position":5,"duration":null,"paused":false}},
                {"name":"no id"}]}"#,
        );
        let Some(Incoming::Devices(devices)) = devices else {
            panic!("{devices:?}");
        };
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].playing, None);
        let playing = devices[1].playing.as_ref().unwrap();
        assert_eq!((playing.work_id.as_str(), playing.position), ("w", 5.0));
        assert_eq!((playing.title.as_ref(), playing.duration), (None, None));

        let seek = parse(
            r#"{"type":"command","from":"p","fromName":"Pixel","action":"seek","position":90}"#,
        );
        assert_eq!(
            seek,
            Some(Incoming::Command(Command {
                from_name: "Pixel".into(),
                action: Action::Seek(90.0)
            }))
        );
        let open = parse(
            r#"{"type":"command","fromName":"Pixel","action":"open","workId":"w","position":null}"#,
        );
        assert_eq!(
            open,
            Some(Incoming::Command(Command {
                from_name: "Pixel".into(),
                action: Action::Open {
                    work_id: "w".into(),
                    position: None
                }
            }))
        );
        for bad in [
            r#"{"type":"command","action":"seek"}"#,
            r#"{"type":"command","action":"open"}"#,
            r#"{"type":"command","action":"reboot"}"#,
            r#"{"type":"command","action":"seek","position":-3}"#,
            r#"{"type":"other"}"#,
            "not json",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
