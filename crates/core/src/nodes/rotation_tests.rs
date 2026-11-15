//! Two-phase secret rotation of the Media Nodes (roadmap §2.3): the new key stays pending until
//! the Node's first request signed with it, which promotes it; the old key is refused after.
use super::outbox::{
    ACK_TIMEOUT_SECONDS, acknowledge, claim_next, enqueue, maintain_node, supersede_rotation,
};
use super::registry::load_node;
use super::router;
use super::transport::{promote_pending_key, resolve_node_signing_key};
use crate::auth::AuthState;
use crate::database::{query, query_as, query_scalar};
use crate::identity::IdentityStore;
use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::Method;
use axum::http::StatusCode;
use http_body_util::BodyExt;
use media_contracts::protocol::{
    HDR_BODY_SHA256, HDR_KEY_ID, HDR_NODE_ID, HDR_NONCE, HDR_PROTOCOL, HDR_RES_NONCE,
    HDR_RES_SIGNATURE, HDR_RES_TIMESTAMP, HDR_SIGNATURE, HDR_TIMESTAMP, NodeCommandEnvelope,
    NodeCommandType, PROTOCOL_VERSION, ReplayWindow, random_nonce_hex, sha256_hex,
    sign_internal_request, unix_now, verify_internal_response,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tower::ServiceExt;

const NODE: &str = "rotating-node";
const KEY0: &str = "key-initial";
const SECRET0: &str = "synthetic-rotation-test-secret-not-a-deployment-secret";
const ORIGIN: &str = "http://127.0.0.1:3000";
/// Core id of the other Core the tests pretend owns the channel (so that the Node is reachable
/// for an emission from this Core).
const OTHER_CORE: &str = "other-core-instance";

struct Fixture {
    _dir: Option<tempfile::TempDir>,
    store: IdentityStore,
    app: Router,
    cookie: String,
    csrf: String,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = IdentityStore::open(&dir.path().join("rotation.sqlite"))
        .await
        .unwrap();
    seed(&store, OTHER_CORE).await;
    let mut f = admin_app(store).await;
    f._dir = Some(dir);
    f
}

/// The Node (current key [`KEY0`], polled recently), its channel owned by `owner`, and an
/// administrator session.
async fn seed(store: &IdentityStore, owner: &str) {
    let now = unix_now();
    query(
        "INSERT INTO media_nodes(id,name,kind,status,key_id,shared_secret,last_seen_at,connection_mode) \
            VALUES(?,'Rotating','remote','connected',?,?,?,'outbound')",
    )
    .bind(NODE)
    .bind(KEY0)
    .bind(SECRET0)
    .bind(now)
    .execute(store.pool())
    .await
    .unwrap();
    query(
        "INSERT INTO node_channel_owner(node_id,core_id,epoch,lease_until,uplink_url) VALUES(?,?,1,?,'')",
    )
    .bind(NODE)
    .bind(owner)
    .bind(now + 3600)
    .execute(store.pool())
    .await
    .unwrap();
    let token = "d".repeat(64);
    query("INSERT INTO users(id,role) VALUES('rotation-admin','administrator')")
        .execute(store.pool())
        .await
        .unwrap();
    query("INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES(?,'rotation-admin',?,?,?)")
        .bind(format!("{:x}", Sha256::digest(token.as_bytes())))
        .bind(now)
        .bind(now)
        .bind(now + 3600)
        .execute(store.pool())
        .await
        .unwrap();
}

/// A Core router on `store`, with the session [`seed`] created.
async fn admin_app(store: IdentityStore) -> Fixture {
    let token = "d".repeat(64);
    let auth = AuthState::new(store.clone(), ORIGIN.into()).await.unwrap();
    let app = router(auth, Arc::new(ReplayWindow::new()));
    Fixture {
        _dir: None,
        store,
        app,
        cookie: format!("aether_session={token}"),
        csrf: format!("{:x}", Sha256::digest(format!("csrf:{token}"))),
    }
}

/// Which Core owns the Node's channel: [`OTHER_CORE`] makes the Node reachable for an emission
/// from this Core, this Core's own id lets it deliver (`claim_next`).
async fn set_owner(store: &IdentityStore, core: &str) {
    query("UPDATE node_channel_owner SET core_id=? WHERE node_id=?")
        .bind(core)
        .bind(NODE)
        .execute(store.pool())
        .await
        .unwrap();
}

async fn admin_post(f: &Fixture, path: &str) -> (StatusCode, String) {
    let response = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header("origin", ORIGIN)
                .header("cookie", &f.cookie)
                .header("x-csrf-token", &f.csrf)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

async fn rotate(f: &Fixture) -> (StatusCode, String) {
    admin_post(f, &format!("/api/v1/admin/nodes/{NODE}/rotate-secret")).await
}

/// A heartbeat signed by the Node with `key_id`/`secret`. On success, the Core's response must be
/// signed with that same key: returns the status and the response's key id.
async fn node_call(app: &Router, key_id: &str, secret: &str) -> (StatusCode, Option<String>) {
    let path = "/internal/v1/core/heartbeat";
    let body = b"{}".to_vec();
    let now = unix_now();
    let nonce = random_nonce_hex();
    let digest = sha256_hex(&body);
    let signature = sign_internal_request(
        secret,
        "node-to-core",
        "POST",
        path,
        NODE,
        key_id,
        now,
        &nonce,
        &digest,
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header(HDR_PROTOCOL, PROTOCOL_VERSION)
                .header(HDR_NODE_ID, NODE)
                .header(HDR_KEY_ID, key_id)
                .header(HDR_TIMESTAMP, now.to_string())
                .header(HDR_NONCE, &nonce)
                .header(HDR_BODY_SHA256, digest)
                .header(HDR_SIGNATURE, signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    if !status.is_success() {
        return (status, None);
    }
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned()
    };
    let response_key = header(HDR_KEY_ID);
    verify_internal_response(
        secret,
        &nonce,
        status.as_u16(),
        NODE,
        &response_key,
        header(HDR_RES_TIMESTAMP).parse().unwrap_or(0),
        &header(HDR_RES_NONCE),
        &header(HDR_RES_SIGNATURE),
        &sha256_hex(&bytes),
        unix_now(),
    )
    .expect("the response is signed with the key of the request");
    (status, Some(response_key))
}

/// `(key_id, pending_key_id, pending_secret, pending_since)` as stored.
async fn keys(store: &IdentityStore) -> (String, Option<String>, Option<String>, Option<i64>) {
    query_as(
        "SELECT key_id,pending_key_id,pending_secret,pending_since FROM media_nodes WHERE id=?",
    )
    .bind(NODE)
    .fetch_one(store.pool())
    .await
    .unwrap()
}

async fn pending(store: &IdentityStore) -> (String, String) {
    let (_, key_id, secret, _) = keys(store).await;
    (
        key_id.expect("pending key"),
        secret.expect("pending secret"),
    )
}

/// Deliver the next command as the channel owner would (this Core takes the channel).
async fn deliver(store: &IdentityStore) -> Option<NodeCommandEnvelope> {
    set_owner(store, store.instance_id()).await;
    let command = claim_next(store, NODE, 1, true, unix_now()).await.unwrap();
    set_owner(store, OTHER_CORE).await;
    command
}

async fn command_states(store: &IdentityStore) -> Vec<(String, String)> {
    query_as("SELECT command_type,state FROM node_commands WHERE node_id=? ORDER BY seq")
        .bind(NODE)
        .fetch_all(store.pool())
        .await
        .unwrap()
}

#[tokio::test]
async fn pending_key_is_promoted_by_its_first_use_then_the_old_key_is_refused() {
    let f = fixture().await;
    let (status, body) = rotate(&f).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let view: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(view["pendingRotation"], true);
    assert_eq!(view["keyId"], KEY0, "the current key is unchanged");
    let (new_key, new_secret) = pending(&f.store).await;
    assert!(keys(&f.store).await.3.is_some(), "pending_since recorded");
    assert!(!body.contains(&new_secret) && !body.contains(&new_key) && !body.contains(SECRET0));

    // Not used yet: the old key keeps working, and the Core answers with it.
    assert_eq!(
        node_call(&f.app, KEY0, SECRET0).await,
        (StatusCode::OK, Some(KEY0.to_owned()))
    );
    let command = deliver(&f.store).await.expect("rotation delivered");
    assert_eq!(command.command_type, NodeCommandType::RotateSecret);
    assert_eq!(command.payload["newKeyId"], new_key.as_str());
    assert_eq!(command.payload["newSecret"], new_secret.as_str());

    // First request signed with the new key: promoted, answered with the new key.
    assert_eq!(
        node_call(&f.app, &new_key, &new_secret).await,
        (StatusCode::OK, Some(new_key.clone()))
    );
    assert_eq!(keys(&f.store).await, (new_key.clone(), None, None, None));
    assert_eq!(
        node_call(&f.app, KEY0, SECRET0).await.0,
        StatusCode::UNAUTHORIZED,
        "the old key is refused after the promotion"
    );
    assert_eq!(
        node_call(&f.app, &new_key, &new_secret).await.0,
        StatusCode::OK
    );
    let listed = f
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/admin/nodes")
                .header("cookie", &f.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let listed = listed.into_body().collect().await.unwrap().to_bytes();
    let listed: serde_json::Value = serde_json::from_slice(&listed).unwrap();
    let node = listed["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == NODE)
        .unwrap();
    assert!(node.get("pendingRotation").is_none(), "{node}");

    // The acknowledgement was lost: the command comes back to the queue, but its key is no
    // longer pending, so it is cancelled instead of being delivered again.
    maintain_node(&f.store, NODE, 1, unix_now() + ACK_TIMEOUT_SECONDS + 1)
        .await
        .unwrap();
    assert!(deliver(&f.store).await.is_none());
    assert_eq!(
        command_states(&f.store).await,
        vec![("rotate_secret".to_owned(), "cancelled".to_owned())]
    );
    f.store.close().await;
}

#[tokio::test]
async fn lost_rotation_command_never_disconnects_the_node() {
    let f = fixture().await;
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (new_key, new_secret) = pending(&f.store).await;
    // Delivered, but the response never reached the Node.
    let first = deliver(&f.store).await.expect("delivered");
    for _ in 0..3 {
        assert_eq!(node_call(&f.app, KEY0, SECRET0).await.0, StatusCode::OK);
    }
    assert_eq!(keys(&f.store).await.0, KEY0);
    // No acknowledgement: delivered again, with the same key.
    maintain_node(&f.store, NODE, 1, unix_now() + ACK_TIMEOUT_SECONDS + 1)
        .await
        .unwrap();
    let again = deliver(&f.store).await.expect("redelivered");
    assert_eq!(again.command_id, first.command_id);
    assert_eq!(again.payload["newKeyId"], new_key.as_str());
    // Lost again, then expired: the Node still works with its current key.
    maintain_node(&f.store, NODE, 1, unix_now() + 3600)
        .await
        .unwrap();
    assert_eq!(
        command_states(&f.store).await,
        vec![("rotate_secret".to_owned(), "expired".to_owned())]
    );
    assert_eq!(node_call(&f.app, KEY0, SECRET0).await.0, StatusCode::OK);
    assert_eq!(
        pending(&f.store).await,
        (new_key.clone(), new_secret.clone())
    );
    // Delivered at least once, the Node may hold that key (applied it, then went offline):
    // a new rotation is refused instead of replacing it, even once the command expired.
    assert_eq!(rotate(&f).await.0, StatusCode::CONFLICT);
    assert_eq!(
        pending(&f.store).await,
        (new_key.clone(), new_secret.clone())
    );
    // The Node comes back signing with the delivered key: accepted and promoted.
    assert_eq!(
        node_call(&f.app, &new_key, &new_secret).await,
        (StatusCode::OK, Some(new_key.clone()))
    );
    assert_eq!(keys(&f.store).await.0, new_key);
    // Settled by use: a later rotation is allowed again.
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (newer_key, newer_secret) = pending(&f.store).await;
    assert_eq!(
        deliver(&f.store).await.expect("newer rotation").payload["newKeyId"],
        newer_key.as_str()
    );
    assert_eq!(
        node_call(&f.app, &newer_key, &newer_secret).await,
        (StatusCode::OK, Some(newer_key.clone()))
    );
    assert_eq!(keys(&f.store).await.0, newer_key);
    f.store.close().await;
}

#[tokio::test]
async fn newer_rotation_replaces_an_undelivered_pending_one() {
    let f = fixture().await;
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (first_key, first_secret) = pending(&f.store).await;
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (second_key, second_secret) = pending(&f.store).await;
    assert_ne!(first_key, second_key);
    assert_eq!(
        command_states(&f.store).await,
        vec![
            ("rotate_secret".to_owned(), "cancelled".to_owned()),
            ("rotate_secret".to_owned(), "queued".to_owned()),
        ]
    );
    let delivered = deliver(&f.store).await.expect("newer rotation");
    assert_eq!(delivered.payload["newKeyId"], second_key.as_str());
    assert!(deliver(&f.store).await.is_none());
    assert_eq!(
        node_call(&f.app, &first_key, &first_secret).await.0,
        StatusCode::UNAUTHORIZED,
        "a replaced pending key is never accepted"
    );
    assert_eq!(
        node_call(&f.app, &second_key, &second_secret).await.0,
        StatusCode::OK
    );
    assert_eq!(keys(&f.store).await.0, second_key);
    f.store.close().await;
}

#[tokio::test]
async fn delivered_rotation_blocks_a_newer_one_until_settled() {
    let f = fixture().await;
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (key, secret) = pending(&f.store).await;
    let delivered = deliver(&f.store).await.expect("delivered");
    // The Node may already sign with this key: it stays the pending one.
    assert_eq!(rotate(&f).await.0, StatusCode::CONFLICT);
    assert!(!supersede_rotation(&f.store, NODE).await.unwrap());
    assert_eq!(pending(&f.store).await, (key.clone(), secret.clone()));
    // The Node uses it (promotion) and acknowledges it: a new rotation is accepted again.
    assert_eq!(node_call(&f.app, &key, &secret).await.0, StatusCode::OK);
    acknowledge(&f.store, NODE, &[delivered.command_id], unix_now())
        .await
        .unwrap();
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    assert_ne!(pending(&f.store).await.0, key);
    assert_eq!(
        keys(&f.store).await.0,
        key,
        "current until the next promotion"
    );
    f.store.close().await;
}

#[tokio::test]
async fn superseded_rotation_command_is_never_delivered() {
    let f = fixture().await;
    // A rotation whose key is not (or no longer) pending, e.g. queued by a racing emission
    // before a re-pairing: cancelled at delivery instead of switching the Node to a dead key.
    enqueue(
        &f.store,
        NODE,
        NodeCommandEnvelope {
            command_id: "rotate-key-stale".into(),
            command_type: NodeCommandType::RotateSecret,
            expires_at: None,
            epoch: None,
            payload: serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "nodeId": NODE,
                "newKeyId": "key-stale",
                "newSecret": "stale-secret",
                "timestamp": 0,
                "nonce": "n",
            }),
        },
    )
    .await
    .unwrap();
    assert!(deliver(&f.store).await.is_none());
    let (state, payload): (String, String) =
        query_as("SELECT state,payload_json FROM node_commands WHERE node_id=?")
            .bind(NODE)
            .fetch_one(f.store.pool())
            .await
            .unwrap();
    assert_eq!((state.as_str(), payload.as_str()), ("cancelled", "null"));
    assert_eq!(node_call(&f.app, KEY0, SECRET0).await.0, StatusCode::OK);
    f.store.close().await;
}

#[tokio::test]
async fn revocation_clears_the_pending_key() {
    let f = fixture().await;
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (key, secret) = pending(&f.store).await;
    let (status, body) = admin_post(&f, &format!("/api/v1/admin/nodes/{NODE}/revoke")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, pending_key, pending_secret, pending_since) = keys(&f.store).await;
    assert_eq!(
        (pending_key, pending_secret, pending_since),
        (None, None, None)
    );
    assert_eq!(
        node_call(&f.app, &key, &secret).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        node_call(&f.app, KEY0, SECRET0).await.0,
        StatusCode::FORBIDDEN
    );
    // The pending rotation was cancelled with everything else waiting for the Node.
    assert!(
        command_states(&f.store)
            .await
            .iter()
            .all(|(kind, state)| kind != "rotate_secret" || state == "cancelled")
    );
    assert_eq!(rotate(&f).await.0, StatusCode::CONFLICT);
    f.store.close().await;
}

#[tokio::test]
async fn concurrent_promotions_of_the_same_key_both_succeed() {
    let f = fixture().await;
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (key, secret) = pending(&f.store).await;
    // Two requests signed with the new key, verified concurrently (same or different Cores).
    let mut first = load_node(&f.store, NODE).await.unwrap().unwrap();
    let mut second = first.clone();
    let first_key = resolve_node_signing_key(&f.store, &mut first, &key)
        .await
        .unwrap_or_else(|_| panic!("pending key accepted"));
    let second_key = resolve_node_signing_key(&f.store, &mut second, &key)
        .await
        .unwrap_or_else(|_| panic!("pending key accepted"));
    assert!(first_key.pending && second_key.pending);
    assert!(
        promote_pending_key(&f.store, &mut first, &first_key)
            .await
            .is_ok()
    );
    assert!(
        promote_pending_key(&f.store, &mut second, &second_key)
            .await
            .is_ok(),
        "already promoted to the same key"
    );
    assert_eq!(
        (first.key_id.as_str(), first.shared_secret.as_str()),
        (key.as_str(), secret.as_str())
    );
    assert_eq!(second.key_id, key);
    assert!(
        resolve_node_signing_key(&f.store, &mut first, KEY0)
            .await
            .is_err()
    );
    f.store.close().await;
}

#[tokio::test]
async fn another_core_with_a_stale_cache_follows_the_promotion() {
    let f = fixture().await;
    // A second Core on the same database, whose registry cache holds the initial key.
    let other = IdentityStore::open(&f._dir.as_ref().unwrap().path().join("rotation.sqlite"))
        .await
        .unwrap();
    let other_app = router(
        AuthState::new(other.clone(), ORIGIN.into()).await.unwrap(),
        Arc::new(ReplayWindow::new()),
    );
    assert_eq!(node_call(&other_app, KEY0, SECRET0).await.0, StatusCode::OK);
    assert_eq!(rotate(&f).await.0, StatusCode::OK);
    let (key, secret) = pending(&f.store).await;
    // Promoted through this Core...
    assert_eq!(node_call(&f.app, &key, &secret).await.0, StatusCode::OK);
    // ...the other one refuses the old key and accepts the new one at once.
    assert_eq!(
        node_call(&other_app, KEY0, SECRET0).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        node_call(&other_app, &key, &secret).await,
        (StatusCode::OK, Some(key.clone()))
    );
    let cached: String = query_scalar("SELECT key_id FROM media_nodes WHERE id=?")
        .bind(NODE)
        .fetch_one(other.pool())
        .await
        .unwrap();
    assert_eq!(cached, key);
    other.close().await;
    f.store.close().await;
}

/// Two Cores on PostgreSQL: B, which does not own the Node's channel, rotates the secret; A, the
/// owner, delivers it; the first request signed with the new key (to A) promotes it, and B
/// refuses the old key at once. A rotation whose key is no longer pending is never delivered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_rotation_emitted_by_another_core_is_promoted_on_first_use() {
    let connection = std::env::var("MEDIA_TEST_POSTGRES_URL")
        .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server");
    let pool = sqlx::PgPool::connect(&connection).await.unwrap();
    let schema = format!("node_rotation_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&pool)
        .await
        .unwrap();
    let mut url = url::Url::parse(&connection).unwrap();
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let a_store = IdentityStore::open_postgres(url.as_str()).await.unwrap();
    let b_store = IdentityStore::open_postgres(url.as_str()).await.unwrap();
    seed(&a_store, a_store.instance_id()).await;
    let a = admin_app(a_store).await;
    let b = admin_app(b_store).await;

    // Emitted by B: pending only, B and A still accept the current key.
    let (status, body) = rotate(&b).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (key, secret) = pending(&a.store).await;
    assert_eq!(node_call(&b.app, KEY0, SECRET0).await.0, StatusCode::OK);
    assert_eq!(node_call(&a.app, KEY0, SECRET0).await.0, StatusCode::OK);
    // Only A, the owner, delivers it.
    assert!(
        claim_next(&b.store, NODE, 1, true, unix_now())
            .await
            .unwrap()
            .is_none()
    );
    let delivered = claim_next(&a.store, NODE, 1, true, unix_now())
        .await
        .unwrap()
        .expect("delivered by the owner");
    assert_eq!(delivered.payload["newKeyId"], key.as_str());
    // A newer rotation is refused while this one is in flight.
    assert_eq!(rotate(&b).await.0, StatusCode::CONFLICT);

    assert_eq!(
        node_call(&a.app, &key, &secret).await,
        (StatusCode::OK, Some(key.clone()))
    );
    assert_eq!(keys(&a.store).await, (key.clone(), None, None, None));
    assert_eq!(
        node_call(&b.app, KEY0, SECRET0).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(node_call(&b.app, &key, &secret).await.0, StatusCode::OK);

    // Acknowledgement lost: requeued, then cancelled at delivery (its key is no longer pending).
    maintain_node(&a.store, NODE, 1, unix_now() + ACK_TIMEOUT_SECONDS + 1)
        .await
        .unwrap();
    assert!(
        claim_next(&a.store, NODE, 1, true, unix_now())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        command_states(&a.store).await,
        vec![("rotate_secret".to_owned(), "cancelled".to_owned())]
    );
    // Revocation from B clears a new pending key.
    assert_eq!(rotate(&b).await.0, StatusCode::OK);
    assert_eq!(
        admin_post(&b, &format!("/api/v1/admin/nodes/{NODE}/revoke"))
            .await
            .0,
        StatusCode::OK
    );
    let (_, pending_key, pending_secret, pending_since) = keys(&a.store).await;
    assert_eq!(
        (pending_key, pending_secret, pending_since),
        (None, None, None)
    );
    assert_eq!(
        node_call(&a.app, &key, &secret).await.0,
        StatusCode::FORBIDDEN
    );
    a.store.close().await;
    b.store.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&pool)
        .await
        .unwrap();
}
