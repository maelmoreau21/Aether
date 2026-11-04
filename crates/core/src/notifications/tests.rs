use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
    response::IntoResponse,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU16, Ordering},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tower::ServiceExt;

#[test]
fn configuration_rejects_ssrf_injection_and_redacts_secrets() {
    let config = Config {
        destination: "https://notify.example/topic".into(),
        secret: Some("fixture-secret-never-returned".into()),
        ..Default::default()
    };
    assert!(transport::validate("ntfy", &config).is_ok());
    assert!(
        !config
            .redacted()
            .to_string()
            .contains("fixture-secret-never-returned")
    );
    for destination in [
        "http://notify.example/topic",
        "https://u:p@notify.example/topic",
        "https://notify.example:444/topic",
        "https://notify.example/topic?token=secret",
        "https://notify.example/topic#fragment",
    ] {
        assert!(
            transport::validate(
                "ntfy",
                &Config {
                    destination: destination.into(),
                    ..config.clone()
                }
            )
            .is_err()
        );
    }
    let email = Config {
        smtp_host: "smtp.example".into(),
        smtp_port: 465,
        sender: "sender@example.org".into(),
        recipient: "receiver@example.org".into(),
        ..Default::default()
    };
    assert!(transport::validate("email", &email).is_ok());
    assert!(
        transport::validate(
            "email",
            &Config {
                sender: "sender@example.org\r\nBcc:other@example.org".into(),
                ..email.clone()
            }
        )
        .is_err()
    );
    assert!(
        transport::validate(
            "email",
            &Config {
                smtp_port: 25,
                ..email
            }
        )
        .is_err()
    );
    assert!(serde_json::from_value::<Config>(json!({"localTest":true})).is_err());
}

type Captures = Arc<Mutex<Vec<(HeaderMap, String)>>>;
async fn receiver() -> (
    String,
    Captures,
    Arc<AtomicU16>,
    tokio::task::JoinHandle<()>,
) {
    let captures: Captures = Default::default();
    let status = Arc::new(AtomicU16::new(200));
    let c = captures.clone();
    let s = status.clone();
    let app = Router::new().fallback(move |request: Request<Body>| {
        let c = c.clone();
        let s = s.clone();
        async move {
            let headers = request.headers().clone();
            let body = to_bytes(request.into_body(), 16384).await.unwrap();
            c.lock()
                .unwrap()
                .push((headers, String::from_utf8(body.to_vec()).unwrap()));
            (
                StatusCode::from_u16(s.load(Ordering::Relaxed)).unwrap(),
                [("location", "https://should-never-be-contacted.invalid/")],
                "fixture",
            )
                .into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/topic", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, captures, status, handle)
}
fn event() -> Event {
    Event {
        id: "event-fixture-stable".into(),
        kind: "scan.completed".into(),
        task_id: "opaque-task".into(),
        library_id: Some("allowed-library".into()),
        occurred_at: now(),
    }
}

#[tokio::test]
async fn http_adapters_deliver_locally_sign_webhook_and_refuse_redirects() {
    let (destination, captures, status, server) = receiver().await;
    for kind in ["ntfy", "gotify", "webhook"] {
        let config = Config {
            destination: destination.clone(),
            secret: Some("fixture-secret-at-least-32-characters".into()),
            local_test: true,
            ..Default::default()
        };
        transport::deliver(kind, &config, &event()).await.unwrap();
        let (headers, body) = captures.lock().unwrap().last().cloned().unwrap();
        assert_eq!(headers["x-media-event-id"], "event-fixture-stable");
        if kind == "ntfy" {
            assert!(
                headers["authorization"]
                    .to_str()
                    .unwrap()
                    .starts_with("Bearer ")
            );
            assert!(body.contains("Scan termine"));
        }
        if kind == "gotify" {
            assert_eq!(headers["x-gotify-key"], config.secret.as_deref().unwrap());
            assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["priority"], 5);
        }
        if kind == "webhook" {
            assert_eq!(
                headers["x-media-signature"],
                format!(
                    "sha256={}",
                    transport::signature(
                        config.secret.as_deref().unwrap(),
                        headers["x-media-timestamp"].to_str().unwrap(),
                        &event().id,
                        body.as_bytes()
                    )
                )
            );
            assert_eq!(
                serde_json::from_str::<Value>(&body).unwrap()["libraryId"],
                "allowed-library"
            );
            assert!(!body.contains("fixture-secret"));
        }
    }
    status.store(302, Ordering::Relaxed);
    assert_eq!(
        transport::deliver(
            "ntfy",
            &Config {
                destination,
                local_test: true,
                ..Default::default()
            },
            &event()
        )
        .await
        .unwrap_err(),
        "notification_destination_denied"
    );
    assert_eq!(captures.lock().unwrap().len(), 4);
    assert_eq!(
        transport::deliver(
            "ntfy",
            &Config {
                destination: "https://127.0.0.1/topic".into(),
                ..Default::default()
            },
            &event()
        )
        .await
        .unwrap_err(),
        "notification_destination_denied"
    );
    server.abort();
}

#[tokio::test]
async fn smtp_sends_only_to_a_local_receiver_in_test_mode() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        reader
            .get_mut()
            .write_all(b"220 localhost ESMTP\r\n")
            .await
            .unwrap();
        let mut data = String::new();
        let mut receiving = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let reply = if receiving {
                if line == ".\r\n" {
                    receiving = false;
                    "250 accepted\r\n"
                } else {
                    data.push_str(&line);
                    continue;
                }
            } else if line.starts_with("EHLO") {
                "250-localhost\r\n250 8BITMIME\r\n"
            } else if line.starts_with("DATA") {
                receiving = true;
                "354 end with dot\r\n"
            } else if line.starts_with("QUIT") {
                reader.get_mut().write_all(b"221 bye\r\n").await.unwrap();
                break;
            } else {
                "250 ok\r\n"
            };
            reader.get_mut().write_all(reply.as_bytes()).await.unwrap();
        }
        data
    });
    transport::deliver(
        "email",
        &Config {
            smtp_host: "127.0.0.1".into(),
            smtp_port: port,
            sender: "from@example.org".into(),
            recipient: "to@example.org".into(),
            local_test: true,
            ..Default::default()
        },
        &event(),
    )
    .await
    .unwrap();
    let body = tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap();
    assert!(body.contains("event-fixture-stable"));
    assert!(body.contains("Scan termine"));
    assert!(body.contains("Message-ID:"));
}

async fn fixture(store: &IdentityStore, user: &str) {
    query("INSERT INTO users(id,role) VALUES(?,'administrator')")
        .bind(user)
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO notification_preferences(user_id,enabled,scans,tasks) VALUES(?,1,1,1)")
        .bind(user)
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('allowed-library','Private','films','local','/read-only',1)").execute(store.pool()).await.unwrap();
}
async fn terminal_task(store: &IdentityStore, id: &str, at: i64) {
    query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,status,created_at,updated_at,available_at,finished_at) VALUES(?,'allowed-library','test',?,'local','scan','completed',?,?,?,?)")
        .bind(id).bind(id).bind(at).bind(at).bind(at).bind(at).execute(store.pool()).await.unwrap();
}

async fn content_contract(store: &IdentityStore) {
    let at = now();
    query("INSERT INTO users(id,role) VALUES('content-reader','user')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('allowed-library','content-reader')").execute(store.pool()).await.unwrap();
    query("INSERT INTO parental_policies(user_id,maximum_age,allow_unrated) VALUES('content-reader',12,1)").execute(store.pool()).await.unwrap();
    query("INSERT INTO notification_preferences(user_id,enabled,scans,tasks) VALUES('content-reader',1,0,0)").execute(store.pool()).await.unwrap();
    query("INSERT INTO notification_content_preferences(user_id,enabled,since_at) VALUES('content-reader',1,?)").bind(at-10).execute(store.pool()).await.unwrap();
    assert!(
        load_preferences(store, "content-reader")
            .await
            .unwrap()
            .content
    );
    let (destination, captures, _, server) = receiver().await;
    transport::test_destinations()
        .lock()
        .unwrap()
        .insert(destination.clone());
    persist_channel(
        store,
        "content-channel",
        ChannelInput {
            name: "Content fixture".into(),
            recipient_id: "content-reader".into(),
            kind: "webhook".into(),
            enabled: true,
            config: Config {
                destination,
                secret: Some("fixture-secret-at-least-32-characters".into()),
                ..Default::default()
            },
        },
        false,
    )
    .await
    .unwrap();
    query("UPDATE notification_channels SET created_at=? WHERE id='content-channel'")
        .bind(at)
        .execute(store.pool())
        .await
        .unwrap();
    for (id, added, age) in [
        ("old-content", at - 20, 0),
        ("new-content", at, 0),
        ("adult-content", at, 21),
    ] {
        query("INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json,kind,added_at,minimum_age,has_available_asset) VALUES(?,'allowed-library',?,'PRIVATE TITLE','{}','film',?,?,1)")
            .bind(id).bind(id).bind(added).bind(age).execute(store.pool()).await.unwrap();
    }
    collect(store, at).await.unwrap();
    collect(store, at).await.unwrap();
    let events: Vec<Event> =
        query_as("SELECT * FROM notification_events WHERE kind='content.added' ORDER BY task_id")
            .fetch_all(store.pool())
            .await
            .unwrap();
    assert_eq!(
        events.len(),
        2,
        "old catalogue omitted; repeated reconciliation deduplicated"
    );
    let channel: Channel =
        query_as("SELECT * FROM notification_channels WHERE id='content-channel'")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(
        !recipient_allowed(store, &channel, &events[0])
            .await
            .unwrap(),
        "parental restriction"
    );
    assert!(
        recipient_allowed(store, &channel, &events[1])
            .await
            .unwrap()
    );
    for _ in 0..2 {
        query("UPDATE notification_channels SET next_allowed_at=0 WHERE id='content-channel'")
            .execute(store.pool())
            .await
            .unwrap();
        run_once(store, at).await.unwrap();
    }
    let received = captures.lock().unwrap().clone();
    assert_eq!(received.len(), 1);
    let body: Value = serde_json::from_str(&received[0].1).unwrap();
    assert_eq!(body["workId"], "new-content");
    assert!(body.get("taskId").is_none());
    assert!(!body.to_string().contains("PRIVATE TITLE"));
    query("DELETE FROM library_permissions WHERE user_id='content-reader'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        !recipient_allowed(store, &channel, &events[1])
            .await
            .unwrap(),
        "live rights revoked"
    );
    query("INSERT INTO library_permissions(library_id,user_id) VALUES('allowed-library','content-reader')").execute(store.pool()).await.unwrap();
    query("UPDATE inventory_works SET has_available_asset=0 WHERE id='new-content'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        !recipient_allowed(store, &channel, &events[1])
            .await
            .unwrap(),
        "removed content"
    );
    query("UPDATE inventory_works SET has_available_asset=1 WHERE id='new-content'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE notification_content_preferences SET enabled=0 WHERE user_id='content-reader'")
        .execute(store.pool())
        .await
        .unwrap();
    assert!(
        !recipient_allowed(store, &channel, &events[1])
            .await
            .unwrap(),
        "live opt-out"
    );
    server.abort();
}

#[tokio::test]
async fn content_additions_are_opt_in_deduplicated_and_permission_filtered() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("content.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    fixture(&store, "content-admin").await;
    // Simulate an existing installation from before the additive content subscription table.
    query("DROP TABLE notification_content_preferences")
        .execute(store.pool())
        .await
        .unwrap();
    store.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    content_contract(&store).await;
    store.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    assert!(
        !load_preferences(&store, "content-reader")
            .await
            .unwrap()
            .content
    );
    assert_eq!(
        query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM notification_events WHERE kind='content.added'"
        )
        .fetch_one(store.pool())
        .await
        .unwrap(),
        2
    );
}
#[tokio::test]
async fn queue_is_durable_deduplicated_permission_filtered_and_retries_with_backoff() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("notifications.sqlite");
    let store = IdentityStore::open(&path).await.unwrap();
    fixture(&store, "recipient").await;
    let (destination, captures, status, server) = receiver().await;
    transport::test_destinations()
        .lock()
        .unwrap()
        .insert(destination.clone());
    persist_channel(
        &store,
        "channel",
        ChannelInput {
            name: "Fixture".into(),
            recipient_id: "recipient".into(),
            kind: "webhook".into(),
            enabled: true,
            config: Config {
                destination,
                secret: Some("fixture-secret-at-least-32-characters".into()),
                ..Default::default()
            },
        },
        false,
    )
    .await
    .unwrap();
    let at = now();
    terminal_task(&store, "task-one", at).await;
    collect(&store, at).await.unwrap();
    collect(&store, at).await.unwrap();
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM notification_deliveries")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        1
    );
    status.store(503, Ordering::Relaxed);
    assert!(run_once(&store, at).await.unwrap());
    let retry: (String, i64, i64) =
        query_as("SELECT status,attempt,available_at FROM notification_deliveries")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(retry.0, "pending");
    assert_eq!(retry.1, 1);
    assert!(retry.2 >= at + 60);
    assert!(!run_once(&store, at).await.unwrap());
    store.close().await;
    let store = IdentityStore::open(&path).await.unwrap();
    query("UPDATE notification_deliveries SET available_at=0")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE notification_channels SET next_allowed_at=0")
        .execute(store.pool())
        .await
        .unwrap();
    status.store(200, Ordering::Relaxed);
    run_once(&store, now()).await.unwrap();
    assert_eq!(
        query_scalar::<_, String>("SELECT status FROM notification_deliveries")
            .fetch_one(store.pool())
            .await
            .unwrap(),
        "sent"
    );
    let received = captures.lock().unwrap().clone();
    assert_eq!(received.len(), 2);
    assert_eq!(
        received[0].0["x-media-event-id"],
        received[1].0["x-media-event-id"]
    );
    terminal_task(&store, "task-two", now()).await;
    query("UPDATE users SET role='user' WHERE id='recipient'")
        .execute(store.pool())
        .await
        .unwrap();
    query("UPDATE notification_channels SET next_allowed_at=0")
        .execute(store.pool())
        .await
        .unwrap();
    run_once(&store, now()).await.unwrap();
    assert_eq!(query_scalar::<_,String>("SELECT status FROM notification_deliveries d JOIN notification_events e ON e.id=d.event_id WHERE e.task_id='task-two'").fetch_one(store.pool()).await.unwrap(),"suppressed");
    assert_eq!(captures.lock().unwrap().len(), 2);
    server.abort();
}

async fn call(
    app: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> (StatusCode, Value, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("origin", "http://127.0.0.1:3000")
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .to_string();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

#[tokio::test]
async fn http_configuration_requires_admin_csrf_and_never_returns_secrets() {
    let temp = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&temp.path().join("api.sqlite"))
        .await
        .unwrap();
    let user = store
        .bootstrap(
            "notification-admin",
            zeroize::Zeroizing::new("synthetic notification password".into()),
        )
        .await
        .unwrap();
    let auth = AuthState::new(store.clone(), "http://127.0.0.1:3000".into())
        .await
        .unwrap();
    let app = router(auth.clone()).merge(crate::auth::router(auth));
    let (_, login, cookie) = call(
        &app,
        "/api/v1/auth/login",
        "POST",
        "",
        "",
        json!({"login":"notification-admin","password":"synthetic notification password"}),
    )
    .await;
    let csrf = login["csrfToken"].as_str().unwrap();
    let input = json!({"name":"Private channel","recipientId":user,"kind":"webhook","enabled":false,"config":{"destination":"https://notify.example/webhook","secret":"fixture-secret-at-least-32-characters"}});
    assert_eq!(
        call(
            &app,
            "/api/v1/admin/notifications/channels",
            "POST",
            &cookie,
            "",
            input.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/api/v1/admin/notifications/channels",
            "POST",
            &cookie,
            csrf,
            input
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, channels, _) = call(
        &app,
        "/api/v1/admin/notifications/channels",
        "GET",
        &cookie,
        "",
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(channels["items"][0]["config"]["hasSecret"], true);
    assert!(!channels.to_string().contains("fixture-secret"));
    assert_eq!(
        call(
            &app,
            "/api/v1/account/notifications",
            "GET",
            &cookie,
            "",
            Value::Null
        )
        .await
        .1["enabled"],
        false
    );
    query("UPDATE users SET role='user' WHERE id=?")
        .bind(&user)
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "/api/v1/admin/notifications/channels",
            "GET",
            &cookie,
            "",
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "/api/v1/account/notifications",
            "PUT",
            &cookie,
            csrf,
            json!({"enabled":true,"scans":true,"tasks":true,"content":true})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &app,
            "/api/v1/account/notifications",
            "GET",
            &cookie,
            "",
            Value::Null
        )
        .await
        .1["tasks"],
        false
    );
    assert!(load_preferences(&store, &user).await.unwrap().content);
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_notification_claim_is_fenced_and_queue_survives_reopen() {
    let url = zeroize::Zeroizing::new(
        std::env::var("MEDIA_TEST_POSTGRES_URL").expect("set disposable PostgreSQL test URL"),
    );
    let first = IdentityStore::open_postgres(&url).await.unwrap();
    fixture(&first, "pg-notification-recipient").await;
    let second = IdentityStore::open_postgres(&url).await.unwrap();
    let (destination, captures, _, server) = receiver().await;
    transport::test_destinations()
        .lock()
        .unwrap()
        .insert(destination.clone());
    persist_channel(
        &first,
        "pg-channel",
        ChannelInput {
            name: "PG fixture".into(),
            recipient_id: "pg-notification-recipient".into(),
            kind: "webhook".into(),
            enabled: true,
            config: Config {
                destination,
                secret: Some("fixture-secret-at-least-32-characters".into()),
                ..Default::default()
            },
        },
        false,
    )
    .await
    .unwrap();
    let at = now();
    terminal_task(&first, "pg-task", at).await;
    collect(&first, at).await.unwrap();
    let (a, b) = tokio::join!(claim(&first, at), claim(&second, at));
    assert_eq!(
        usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()),
        1
    );
    query("UPDATE notification_deliveries SET lease_until=0 WHERE channel_id='pg-channel'")
        .execute(first.pool())
        .await
        .unwrap();
    first.close().await;
    second.close().await;
    let reopened = IdentityStore::open_postgres(&url).await.unwrap();
    assert!(claim(&reopened, now()).await.unwrap().is_none());
    query("UPDATE notification_deliveries SET available_at=0 WHERE channel_id='pg-channel'")
        .execute(reopened.pool())
        .await
        .unwrap();
    query("UPDATE notification_channels SET next_allowed_at=0 WHERE id='pg-channel'")
        .execute(reopened.pool())
        .await
        .unwrap();
    run_once(&reopened, now()).await.unwrap();
    assert_eq!(
        query_scalar::<_, String>(
            "SELECT status FROM notification_deliveries WHERE channel_id='pg-channel'"
        )
        .fetch_one(reopened.pool())
        .await
        .unwrap(),
        "sent"
    );
    assert_eq!(captures.lock().unwrap().len(), 1);
    assert_eq!(
        query_scalar::<_, i64>("SELECT COUNT(*) FROM notification_events WHERE task_id='pg-task'")
            .fetch_one(reopened.pool())
            .await
            .unwrap(),
        1
    );
    content_contract(&reopened).await;
    reopened.close().await;
    server.abort();
}
