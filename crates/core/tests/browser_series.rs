//! Disposable browser series scan/playback evidence.
use media_poc::identity::IdentityStore;
use std::{path::Path, process::Stdio, time::Duration};
use zeroize::Zeroizing;

async fn start(root: &Path, db: &Path, library: &Path, port: u16) -> tokio::process::Child {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-poc"))
        .current_dir(root)
        .env("MEDIA_DB_PATH", db)
        .env("MEDIA_LIBRARY_ROOT", library)
        .env("MEDIA_PORT", port.to_string())
        .env_remove("MEDIA_PUBLIC_URL")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "Core exited before readiness"
            );
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    child
}

#[tokio::test]
#[ignore = "requires built Web assets, generated synthetic fixtures and Chrome/Playwright"]
async fn browser_series_flow_and_playback() {
    let temp = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let library = temp.path().join("series");
    let series = library.join("Browser Series (2026)");
    let s01 = series.join("Season 01");
    let s02 = series.join("Season 02");
    std::fs::create_dir_all(&s01).unwrap();
    std::fs::create_dir_all(&s02).unwrap();

    let sample_mkv = root.join("tests/fixtures/generated/representative/representative.mkv");
    let sample_mp4 = root.join("tests/fixtures/generated/p1/clip-mp4-fr.mp4");

    // Season 1: S01E01 (2 versions: MP4 + MKV) and S01E02
    std::fs::copy(&sample_mp4, s01.join("Browser Series - S01E01.mp4")).unwrap();
    std::fs::copy(&sample_mkv, s01.join("Browser Series - S01E01 - 1080p.mkv")).unwrap();
    std::fs::write(
        s01.join("Browser Series - S01E01.nfo"),
        r#"<episodedetails><title>Episode One</title><plot>First episode.</plot></episodedetails>"#,
    )
    .unwrap();

    std::fs::copy(&sample_mp4, s01.join("Browser Series - S01E02.mp4")).unwrap();

    // Season 2: S02E01
    std::fs::copy(&sample_mp4, s02.join("Browser Series - S02E01.mp4")).unwrap();
    std::fs::write(
        s02.join("Browser Series - S02E01.nfo"),
        r#"<episodedetails><title>Season Two Episode</title><plot>Second season.</plot></episodedetails>"#,
    )
    .unwrap();

    // Poster
    std::fs::write(
        series.join("poster.png"),
        vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 3,
            8, 2, 0, 0, 0, 54, 136, 73, 214, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 136, 48,
            234, 7, 34, 6, 20, 10, 0, 67, 242, 6, 151, 143, 92, 131, 231, 0, 0, 0, 0, 73, 69, 78,
            68, 174, 66, 96, 130,
        ],
    )
    .unwrap();

    let db = temp.path().join("series.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "series-admin",
            Zeroizing::new("disposable series browser password".into()),
        )
        .await
        .unwrap();

    // Create a second distinct account (Role::User) with a real Argon2id password hash
    let user_b_id = uuid::Uuid::new_v4().to_string();
    let user_b_hash = tokio::task::spawn_blocking(|| {
        use argon2::{Algorithm, Argon2, Params, Version, password_hash::PasswordHasher};
        let params = Params::new(65536, 3, 1, Some(32)).unwrap();
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(b"disposable series user-b password")
            .unwrap()
            .to_string()
    })
    .await
    .unwrap();
    sqlx::query("INSERT INTO users(id, role) VALUES (?, 'user')")
        .bind(&user_b_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO local_credentials(user_id, login, password_hash) VALUES (?, ?, ?)")
        .bind(&user_b_id)
        .bind("series-user-b")
        .bind(&user_b_hash)
        .execute(store.pool())
        .await
        .unwrap();

    store.close().await;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut child = start(&root, &db, &library, port).await;
    let mut cmd = tokio::process::Command::new("node");
    cmd.arg("scripts/browser-series.cjs")
        .env("MEDIA_TEST_ORIGIN", format!("http://127.0.0.1:{port}"))
        .current_dir(&root);
    if let Ok(dir) = std::env::var("MEDIA_REPORT_DIR") {
        cmd.env("MEDIA_REPORT_DIR", dir);
    }
    let result = cmd.status().await.unwrap();
    child.kill().await.unwrap();
    assert!(result.success(), "series browser scenario failed");
}
