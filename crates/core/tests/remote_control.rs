//! Remote control over `/api/v1/remote/ws` on a real Core: an account's devices
//! see each other and what they play, relay commands, and never reach another
//! account's devices.
use aether_core::{auth::AuthState, database::query, identity::IdentityStore, node::Inventory};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use zeroize::Zeroizing;

const PASSWORD: &str = "synthetic remote control password";
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn serve() -> (tempfile::TempDir, String) {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir_all(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    let admin = store
        .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    query("INSERT INTO users(id,role) VALUES('viewer-id','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO local_credentials(user_id,login,password_hash) SELECT 'viewer-id','viewer',password_hash FROM local_credentials WHERE user_id=?")
        .bind(&admin)
        .execute(store.pool())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let auth = AuthState::new(store, origin.clone()).await.unwrap();
    let app = aether_core::secure_app(Inventory::local(media).await.unwrap(), address.port(), auth)
        .await
        .unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (temp, origin)
}

async fn token(origin: &str, login: &str) -> String {
    let body: Value = reqwest::Client::new()
        .post(format!("{origin}/api/v1/auth/login"))
        .json(&json!({"login": login, "password": PASSWORD, "tokenType": "bearer"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    body["accessToken"].as_str().unwrap().to_owned()
}

async fn connect(origin: &str, token: &str, device: &str, name: &str, kind: &str) -> Socket {
    let mut request = format!("{}/api/v1/remote/ws", origin.replace("http", "ws"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    send(
        &mut socket,
        json!({"type":"hello","deviceId":device,"name":name,"kind":kind}),
    )
    .await;
    socket
}

async fn send(socket: &mut Socket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

/// The next message of `kind` matching `test`, skipping others; `None` after 1.5 s.
async fn next(socket: &mut Socket, kind: &str, test: impl Fn(&Value) -> bool) -> Option<Value> {
    tokio::time::timeout(Duration::from_millis(1500), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == kind && test(&value) {
                    return Some(value);
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

fn lists(value: &Value, device: &str) -> bool {
    value["devices"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["deviceId"] == device)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn devices_of_one_account_see_each_other_and_relay_commands() {
    let (_temp, origin) = serve().await;
    let (admin, viewer) = (
        token(&origin, "admin").await,
        token(&origin, "viewer").await,
    );

    let mut phone = connect(&origin, &admin, "phone-1", "Pixel 9", "phone").await;
    let mut tv = connect(&origin, &admin, "tv-1", "Salon", "tv").await;
    let mut stranger = connect(&origin, &viewer, "phone-2", "Autre", "phone").await;

    let seen = next(&mut phone, "devices", |v| lists(v, "tv-1"))
        .await
        .unwrap();
    let entry = &seen["devices"][0];
    assert_eq!(
        (entry["name"].as_str(), entry["kind"].as_str()),
        (Some("Salon"), Some("tv"))
    );
    assert!(
        !lists(&seen, "phone-2"),
        "another account's device is listed"
    );
    assert!(
        next(&mut stranger, "devices", |v| lists(v, "tv-1"))
            .await
            .is_none()
    );

    // The TV reports what it plays; the phone sees it, to resume there.
    send(&mut tv, json!({"type":"state","playing":{"workId":"work-1","title":"Film","position":42.0,"duration":600.0,"paused":false}})).await;
    let playing = next(&mut phone, "devices", |v| {
        v["devices"][0]["playing"]["position"] == 42.0
    })
    .await
    .unwrap();
    assert_eq!(playing["devices"][0]["playing"]["workId"], "work-1");

    // Resume on the TV: open another work at 10 s.
    send(
        &mut phone,
        json!({"type":"command","to":"tv-1","action":"open","workId":"work-2","position":10.0}),
    )
    .await;
    let command = next(&mut tv, "command", |_| true).await.unwrap();
    assert_eq!(
        (
            command["from"].as_str(),
            command["fromName"].as_str(),
            command["action"].as_str()
        ),
        (Some("phone-1"), Some("Pixel 9"), Some("open"))
    );
    assert_eq!(
        (command["workId"].as_str(), command["position"].as_f64()),
        (Some("work-2"), Some(10.0))
    );

    // Refused: another account, an unknown action, a seek without position.
    send(
        &mut stranger,
        json!({"type":"command","to":"tv-1","action":"pause"}),
    )
    .await;
    send(
        &mut phone,
        json!({"type":"command","to":"tv-1","action":"format"}),
    )
    .await;
    send(
        &mut phone,
        json!({"type":"command","to":"tv-1","action":"seek"}),
    )
    .await;
    assert!(next(&mut tv, "command", |_| true).await.is_none());
    send(
        &mut phone,
        json!({"type":"command","to":"tv-1","action":"pause"}),
    )
    .await;
    assert_eq!(
        next(&mut tv, "command", |_| true).await.unwrap()["action"],
        "pause"
    );

    // Leaving is announced to the account's other devices.
    phone.close(None).await.unwrap();
    assert!(
        next(&mut tv, "devices", |v| !lists(v, "phone-1"))
            .await
            .is_some()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_socket_without_credentials_or_hello_gets_nothing() {
    let (_temp, origin) = serve().await;
    let request = format!("{}/api/v1/remote/ws", origin.replace("http", "ws"));
    assert!(tokio_tungstenite::connect_async(request).await.is_err());
    // A device that never introduces itself is not listed.
    let admin = token(&origin, "admin").await;
    let mut request = format!("{}/api/v1/remote/ws", origin.replace("http", "ws"))
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {admin}").parse().unwrap());
    let (mut silent, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    send(&mut silent, json!({"type":"state","playing":null})).await;
    let mut phone = connect(&origin, &admin, "phone-1", "Pixel 9", "phone").await;
    let first = next(&mut phone, "devices", |_| true).await.unwrap();
    assert_eq!(first["devices"], json!([]));
}
