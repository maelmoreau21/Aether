//! The client against a real Core served on a loopback port, with a film
//! library scanned from a generated fixture: what an application does, end to end.
use aether_client::{CatalogQuery, Client, Error, ProgressUpdate};
use media_poc::{auth::AuthState, identity::IdentityStore, node::Inventory};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use zeroize::Zeroizing;

/// Made per run: the test account never has a fixed password in the source.
static PASSWORD: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("synthetic client contract {nanos:x}")
});

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/generated/p1/clip-mp4-en.mp4")
}

/// Serves a Core with one administrator and returns (temp dir, origin, media root).
async fn serve() -> (tempfile::TempDir, String, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    let film = media.join("Films/Client Film (2026)/film.mp4");
    std::fs::create_dir_all(film.parent().unwrap()).unwrap();
    std::fs::copy(fixture(), &film).unwrap();
    let episode = media.join("Series/Client Series (2026)/Season 01/Client Series S01E01.mp4");
    std::fs::create_dir_all(episode.parent().unwrap()).unwrap();
    std::fs::copy(fixture(), &episode).unwrap();
    let store = IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.clone()))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let auth = AuthState::new(store, origin.clone()).await.unwrap();
    let app = media_poc::secure_app(
        Inventory::local(media.clone()).await.unwrap(),
        address.port(),
        auth,
    )
    .await
    .unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (temp, origin, media)
}

/// Administration with the device token: no cookie, CSRF or Origin needed.
async fn admin(
    origin: &str,
    client: &Client,
    method: reqwest::Method,
    path: &str,
    body: Value,
) -> Value {
    let token = client.token().unwrap().access;
    let mut request = reqwest::Client::new()
        .request(method, format!("{origin}{path}"))
        .bearer_auth(token);
    if !body.is_null() {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    assert!(
        response.status().is_success(),
        "{path}: {}",
        response.status()
    );
    response.json().await.unwrap_or(Value::Null)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_application_signs_in_pairs_a_tv_browses_plays_and_resumes() {
    if !fixture().exists() {
        eprintln!("skipping: generated fixtures missing");
        return;
    }
    let (_temp, origin, media) = serve().await;
    let phone = Client::new(&origin, "Pixel 9").unwrap();
    assert!(matches!(phone.me().await, Err(Error::SignedOut)));
    assert!(
        phone
            .login("admin", &format!("{}-wrong", *PASSWORD))
            .await
            .unwrap_err()
            .is_unauthorized()
    );
    let user = phone.login("admin", &PASSWORD).await.unwrap();
    assert_eq!(
        (user.login.as_str(), user.role.as_str()),
        ("admin", "administrator")
    );
    assert_eq!(phone.me().await.unwrap(), user);

    // Film and series libraries, created and scanned with the same token.
    for (kind, folder) in [("films", "Films"), ("series", "Series")] {
        let library = admin(&origin, &phone, reqwest::Method::POST, "/api/v1/admin/libraries",
            json!({"name":folder,"kind":kind,"nodeId":"local","root":media.join(folder).to_string_lossy(),"enabled":true,"userIds":[]})).await;
        let scan = format!(
            "/api/v1/admin/libraries/{}/scan",
            library["id"].as_str().unwrap()
        );
        admin(&origin, &phone, reqwest::Method::POST, &scan, Value::Null).await;
        tokio::time::timeout(Duration::from_secs(45), async {
            while admin(&origin, &phone, reqwest::Method::GET, &scan, Value::Null).await["status"]
                != "completed"
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
    }

    // A series opens on its seasons; an episode plays like a film.
    let series_query = CatalogQuery {
        kind: Some("series".into()),
        limit: 20,
        ..Default::default()
    };
    let series_page = phone.catalog(&series_query).await.unwrap();
    let series = series_page
        .items
        .iter()
        .find(|w| w.title == "Client Series")
        .unwrap_or_else(|| panic!("{series_page:?}"));
    let detail = phone.work(&series.id).await.unwrap();
    assert_eq!(detail.work.id, series.id);
    let episode = &detail.seasons[0].episodes[0];
    assert_eq!(
        (detail.seasons[0].number, episode.number),
        (Some(1), Some(1))
    );
    assert!(phone.media(&episode.id).await.unwrap().available);

    // Browse, then play through an HLS URL that needs no credential.
    let query = CatalogQuery {
        kind: Some("film".into()),
        limit: 20,
        ..Default::default()
    };
    let page = phone.catalog(&query).await.unwrap();
    let film = page
        .items
        .iter()
        .find(|w| w.title == "Client Film")
        .unwrap_or_else(|| panic!("{page:?}"));
    assert_eq!(film.year, Some(2026));
    let searched = phone
        .catalog(&CatalogQuery {
            search: Some("nothing like it".into()),
            limit: 5,
            ..query.clone()
        })
        .await
        .unwrap();
    assert!(searched.items.is_empty());
    assert!(phone.work(&film.id).await.unwrap().seasons.is_empty());
    let asset = phone.media(&film.id).await.unwrap();
    assert!(asset.available);
    let playback = phone.playback_url(&asset.id).await.unwrap();
    assert!(playback.url.starts_with(&origin) && playback.url.ends_with("/master.m3u8"));
    let master = reqwest::get(&playback.url).await.unwrap();
    assert_eq!(master.status(), 200);
    assert!(master.text().await.unwrap().starts_with("#EXTM3U"));

    // Resume: a second device writing in between does not lose the position.
    let start = phone.progress(&film.id).await.unwrap();
    assert_eq!(start.revision, 0);
    let update = |position, expected_revision| ProgressUpdate {
        position,
        duration: 600.0,
        watched: false,
        expected_revision,
    };
    let first = phone
        .save_progress(&film.id, &update(30.0, 0))
        .await
        .unwrap();
    assert_eq!((first.position, first.revision), (30.0, 1));
    let stale = phone
        .save_progress(&film.id, &update(42.0, 0))
        .await
        .unwrap();
    assert_eq!((stale.position, stale.revision), (42.0, 2));
    let recent = phone.recent_progress().await.unwrap();
    let entry = recent.iter().find(|p| p.work_id == film.id).unwrap();
    assert_eq!(
        (entry.title.as_str(), entry.position),
        ("Client Film", 42.0)
    );

    // Direct play of the original with the token, ranges included.
    let original = reqwest::Client::new()
        .get(phone.content_url(&asset.id).unwrap())
        .bearer_auth(phone.token().unwrap().access)
        .header("range", "bytes=0-15")
        .send()
        .await
        .unwrap();
    assert_eq!(original.status(), 206);
    assert_eq!(original.bytes().await.unwrap().len(), 16);

    // A TV pairs by QR code, approved from the phone.
    let tv = Client::new(&origin, "Salon TV").unwrap();
    let pairing = tv.start_pairing().await.unwrap();
    assert_eq!(
        pairing.qr_modules.len(),
        pairing.qr_width * pairing.qr_width
    );
    assert!(pairing.url.contains(&pairing.code));
    assert_eq!(tv.poll_pairing(&pairing).await.unwrap(), None);
    phone.approve_pairing(&pairing.code).await.unwrap();
    assert_eq!(tv.poll_pairing(&pairing).await.unwrap(), Some(user.clone()));
    assert!(matches!(
        tv.poll_pairing(&pairing).await,
        Err(Error::PairingExpired)
    ));
    assert_eq!(tv.progress(&film.id).await.unwrap().position, 42.0);

    // Home screen: what to continue, and each library on its own.
    let resume = tv.continue_watching().await.unwrap();
    assert_eq!(resume[0].work_id, film.id);
    assert_eq!(
        (resume[0].position, resume[0].title.as_str()),
        (42.0, "Client Film")
    );
    let libraries = tv.libraries().await.unwrap();
    assert!(!libraries.is_empty(), "{libraries:?}");
    let mut found = false;
    for library in &libraries {
        let page = tv
            .catalog(&CatalogQuery {
                library: Some(library.id.clone()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        found |= page.items.iter().any(|w| w.id == film.id);
    }
    assert!(found, "the film is listed in one of {libraries:?}");

    // Rotation, then sign-out: the old and the forgotten tokens stop working.
    let before = tv.token().unwrap();
    assert!(!tv.refresh_if_due(3600).await.unwrap());
    tv.refresh().await.unwrap();
    assert_ne!(tv.token().unwrap(), before);
    let saved = tv.token().unwrap();
    tv.logout().await.unwrap();
    assert!(tv.token().is_none());
    let restored = Client::new(&origin, "Salon TV").unwrap().with_token(saved);
    assert!(restored.me().await.unwrap_err().is_unauthorized());
    assert_eq!(
        phone.me().await.unwrap(),
        user,
        "the phone keeps its own session"
    );
}

/// An address that answers like any other web server is refused, and a closed
/// port is a network error: the sign-in screen tells them apart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_address_that_is_not_an_aether_server_is_refused() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let page = axum::Router::new()
        .fallback(|| async { axum::response::Html("<!doctype html><title>Not Aether</title>") });
    tokio::spawn(async move { axum::serve(listener, page).await.unwrap() });
    let client = Client::new(&format!("http://{address}"), "Salon").unwrap();
    assert!(matches!(client.discover().await, Err(Error::NotAether)));

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let client = Client::new(&format!("http://127.0.0.1:{port}"), "Salon").unwrap();
    assert!(matches!(client.discover().await, Err(Error::Network(_))));
}

/// The sign-in screen first checks that the address is an Aether server and
/// reads how it lets people sign in (needs no media).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovery_reads_a_real_core_before_any_sign_in() {
    let temp = tempfile::tempdir().unwrap();
    let media = temp.path().join("media");
    std::fs::create_dir_all(&media).unwrap();
    let store = IdentityStore::open(&temp.path().join("core.sqlite3"))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = format!("http://{address}");
    let auth = AuthState::new(store.clone(), origin.clone()).await.unwrap();
    let app = media_poc::secure_app(Inventory::local(media).await.unwrap(), address.port(), auth)
        .await
        .unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = Client::new(&origin, "Salon").unwrap();

    let fresh = client.discover().await.unwrap();
    assert_eq!(fresh.name, "Aether");
    assert!(fresh.sign_in.local_login_enabled && !fresh.sign_in.enabled);
    assert!(fresh.needs_bootstrap, "no administrator yet");

    store
        .bootstrap("admin", Zeroizing::new(PASSWORD.clone()))
        .await
        .unwrap();
    assert!(!client.discover().await.unwrap().needs_bootstrap);
}
