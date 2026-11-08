//! Real TCP/HTTP/WebSocket evidence with isolated accounts and SQLite.
use futures_util::{SinkExt, StreamExt};
use media_poc::{
    admin_users,
    auth::{self, AuthState},
    database::query,
    identity::IdentityStore,
    parties,
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use zeroize::Zeroizing;

const PASSWORD: &str = "temporary syncplay fixture password";
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn login(http: &Client, origin: &str, name: &str) -> (String, String) {
    let response = http
        .post(format!("{origin}/api/v1/auth/login"))
        .header("origin", origin)
        .json(&json!({"login":name,"password":PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body: Value = response.json().await.unwrap();
    (cookie, body["csrfToken"].as_str().unwrap().to_owned())
}

async fn connect(origin: &str, path: &str, cookie: &str) -> Socket {
    let url = format!("{}{path}", origin.replacen("http:", "ws:", 1));
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    connect_async(request).await.unwrap().0
}

async fn receive(socket: &mut Socket, kind: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["type"] == kind {
                    return value;
                }
            }
        }
    })
    .await
    .expect("expected WebSocket event within 5 seconds")
}

async fn send(socket: &mut Socket, value: Value) {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_and_follower_sync_over_real_websockets_and_reconnect() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("syncplay.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap("host", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    query("INSERT INTO media_nodes (id,name,base_url,status,shared_secret) VALUES ('local','Local','http://127.0.0.1','connected','fixture') ON CONFLICT(id) DO UPDATE SET status='connected'")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO libraries (id,name,kind,node_id,root,enabled) VALUES ('films','Films','films','local','/fixture',1)")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_works (id,library_id,identity_key,title,external_ids_json) VALUES ('movie','films','fixture','Movie','{}')")
        .execute(store.pool()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let app = auth::router(auth.clone())
        .merge(admin_users::router(auth.clone()))
        .merge(parties::router(auth));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Abort the server on assertion failures too.
    struct ServerGuard(tokio::task::JoinHandle<()>);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _guard = ServerGuard(server);
    let http = Client::new();
    let (host_cookie, host_csrf) = login(&http, &origin, "host").await;
    let created = http
        .post(format!("{origin}/api/v1/admin/users"))
        .header("origin", &origin)
        .header("cookie", &host_cookie)
        .header("x-csrf-token", &host_csrf)
        .json(&json!({"login":"follower","password":PASSWORD,"role":"user","libraryIds":["films"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let (follower_cookie, follower_csrf) = login(&http, &origin, "follower").await;
    let created = http
        .post(format!("{origin}/api/v1/parties"))
        .header("origin", &origin)
        .header("cookie", &host_cookie)
        .header("x-csrf-token", &host_csrf)
        .json(&json!({"name":"Together","mediaId":"movie","controlMode":"host_only"}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value = created.json().await.unwrap();
    let party_id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/parties/{party_id}/ws");
    let joined = http
        .post(format!("{origin}/api/v1/parties/{party_id}/join"))
        .header("origin", &origin)
        .header("cookie", &follower_cookie)
        .header("x-csrf-token", &follower_csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(joined.status(), StatusCode::OK);
    let mut host = connect(&origin, &path, &host_cookie).await;
    let mut follower = connect(&origin, &path, &follower_cookie).await;
    assert_eq!(
        receive(&mut host, "syncState").await["party"]["state"],
        "waiting"
    );
    assert_eq!(
        receive(&mut follower, "syncState").await["party"]["state"],
        "waiting"
    );
    send(&mut follower, json!({"type":"ping","clientEpochMs":12345})).await;
    let pong = receive(&mut follower, "pong").await;
    assert_eq!(pong["clientEpochMs"], 12345);
    assert!(pong["serverEpochMs"].is_i64());
    for (kind, position, state) in [
        ("play", 10, "playing"),
        ("seek", 40, "playing"),
        ("pause", 45, "paused"),
    ] {
        send(&mut host, json!({"type":kind,"position":position})).await;
        let a = receive(&mut host, "playbackUpdate").await;
        let b = receive(&mut follower, "playbackUpdate").await;
        assert_eq!(a, b);
        assert_eq!(b["position"], position as f64);
        assert_eq!(b["state"], state);
        assert!(b["serverEpochMs"].is_i64());
    }
    send(&mut follower, json!({"type":"seek","position":90})).await;
    assert_eq!(receive(&mut follower, "error").await["code"], "FORBIDDEN");
    send(
        &mut host,
        json!({"type":"play","position":45,"countdown":2}),
    )
    .await;
    let countdown = receive(&mut follower, "countdown").await;
    assert!(countdown["targetServerEpochMs"].is_i64());
    // Reconnection receives the same future start anchor, not an already
    // extrapolated position paired with an old timestamp.
    follower.close(None).await.unwrap();
    let mut reconnected = connect(&origin, &path, &follower_cookie).await;
    let snapshot = receive(&mut reconnected, "syncState").await;
    assert_eq!(snapshot["party"]["position"], 45.0);
    assert_eq!(
        snapshot["party"]["positionUpdatedServerMs"],
        countdown["targetServerEpochMs"]
    );
    send(
        &mut reconnected,
        json!({"type":"setBuffering","buffering":true,"position":45}),
    )
    .await;
    assert_eq!(
        receive(&mut host, "playbackUpdate").await["state"],
        "paused"
    );
    send(
        &mut reconnected,
        json!({"type":"setBuffering","buffering":false,"position":45}),
    )
    .await;
    assert_eq!(
        receive(&mut host, "playbackUpdate").await["state"],
        "playing"
    );
    send(
        &mut reconnected,
        json!({"type":"setBuffering","buffering":true,"position":46}),
    )
    .await;
    assert_eq!(
        receive(&mut host, "playbackUpdate").await["state"],
        "paused"
    );
    reconnected.close(None).await.unwrap();
    assert_eq!(
        receive(&mut host, "playbackUpdate").await["state"],
        "playing"
    );
    host.close(None).await.unwrap();
}

async fn post(
    http: &Client,
    origin: &str,
    path: &str,
    cookie: &str,
    csrf: &str,
) -> reqwest::Response {
    http.post(format!("{origin}{path}"))
        .header("origin", origin)
        .header("cookie", cookie)
        .header("x-csrf-token", csrf)
        .send()
        .await
        .unwrap()
}

async fn listed(http: &Client, origin: &str, cookie: &str) -> Vec<Value> {
    let response = http
        .get(format!("{origin}/api/v1/parties"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    body["parties"].as_array().unwrap().clone()
}

/// Another account finds an open party while it is watched, joins it without an
/// invitation, and the party ends as soon as nobody is left in it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_account_finds_and_joins_an_open_party_that_ends_when_everyone_leaves() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("open-party.sqlite"))
        .await
        .unwrap();
    store
        .bootstrap("host", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    query("INSERT INTO media_nodes (id,name,base_url,status,shared_secret) VALUES ('local','Local','http://127.0.0.1','connected','fixture') ON CONFLICT(id) DO UPDATE SET status='connected'")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO libraries (id,name,kind,node_id,root,enabled) VALUES ('films','Films','films','local','/fixture',1)")
        .execute(store.pool()).await.unwrap();
    query("INSERT INTO inventory_works (id,library_id,identity_key,title,external_ids_json) VALUES ('movie','films','fixture','Movie','{}')")
        .execute(store.pool()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let app = auth::router(auth.clone())
        .merge(admin_users::router(auth.clone()))
        .merge(parties::router(auth));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    struct ServerGuard(tokio::task::JoinHandle<()>);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _guard = ServerGuard(server);
    let http = Client::new();
    let (host_cookie, host_csrf) = login(&http, &origin, "host").await;
    let created = http
        .post(format!("{origin}/api/v1/admin/users"))
        .header("origin", &origin)
        .header("cookie", &host_cookie)
        .header("x-csrf-token", &host_csrf)
        .json(&json!({"login":"guest","password":PASSWORD,"role":"user","libraryIds":["films"]}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let (guest_cookie, guest_csrf) = login(&http, &origin, "guest").await;

    let created = http
        .post(format!("{origin}/api/v1/parties"))
        .header("origin", &origin)
        .header("cookie", &host_cookie)
        .header("x-csrf-token", &host_csrf)
        .json(&json!({"name":"Open","mediaId":"movie","controlMode":"host_only"}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value = created.json().await.unwrap();
    let party_id = created["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/parties/{party_id}/ws");

    // Nobody is watching yet: the guest is not offered someone else's empty party.
    assert!(listed(&http, &origin, &guest_cookie).await.is_empty());
    let mut host = connect(&origin, &path, &host_cookie).await;
    receive(&mut host, "syncState").await;
    let offered = listed(&http, &origin, &guest_cookie).await;
    assert_eq!(offered.len(), 1, "{offered:?}");
    assert_eq!(offered[0]["id"], party_id.as_str());
    assert_eq!(offered[0]["onlineCount"], 1);
    assert_eq!(offered[0]["isMember"], false);

    let joined = post(
        &http,
        &origin,
        &format!("/api/v1/parties/{party_id}/join"),
        &guest_cookie,
        &guest_csrf,
    )
    .await;
    assert_eq!(joined.status(), StatusCode::OK);
    let joined: Value = joined.json().await.unwrap();
    let guest_id = joined["member"]["userId"].clone();
    let mut guest = connect(&origin, &path, &guest_cookie).await;
    receive(&mut guest, "syncState").await;

    // The host leaves: the role passes to the guest, who is still connected.
    send(&mut host, json!({"type":"leaveParty"})).await;
    assert_eq!(
        receive(&mut guest, "hostTransferred").await["newHostId"],
        guest_id
    );
    host.close(None).await.unwrap();

    // The last member leaves: the party ends at once instead of staying "in progress".
    send(&mut guest, json!({"type":"leaveParty"})).await;
    receive(&mut guest, "partyEnded").await;
    guest.close(None).await.unwrap();
    assert!(listed(&http, &origin, &host_cookie).await.is_empty());
    assert!(listed(&http, &origin, &guest_cookie).await.is_empty());
    let rejoined = post(
        &http,
        &origin,
        &format!("/api/v1/parties/{party_id}/join"),
        &guest_cookie,
        &guest_csrf,
    )
    .await;
    assert_eq!(rejoined.status(), StatusCode::NOT_FOUND);
    let body: Value = rejoined.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("terminée"),
        "{body}"
    );
}
