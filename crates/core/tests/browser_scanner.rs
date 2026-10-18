//! Disposable browser scan/playback evidence, including a real Core process restart.
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
async fn browser_scanner_persists_and_plays_after_core_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let library = temp.path().join("films");
    let film = library.join("Scanner Film (2026) [tmdb-123]");
    std::fs::create_dir_all(&film).unwrap();
    let mut originals = Vec::new();
    for extension in ["mp4", "mkv"] {
        let input = root.join(format!(
            "tests/fixtures/generated/p1/clip-{extension}-fr.{extension}"
        ));
        let output = film.join(format!("Scanner Film (2026) {extension}.{extension}"));
        let bytes = std::fs::read(input).unwrap();
        std::fs::write(&output, &bytes).unwrap();
        originals.push((output, bytes));
    }
    for container in ["mp4", "mkv"] {
        std::fs::write(
            film.join(format!("Scanner Film (2026) {container}.fr.forced.srt")),
            "1\n00:00:00,000 --> 00:00:12,000\nSous-titre externe français forcé",
        )
        .unwrap();
        std::fs::write(
            film.join(format!("Scanner Film (2026) {container}.en.vtt")),
            "WEBVTT\n\n00:00:00.000 --> 00:00:12.000\nExternal WebVTT subtitle",
        )
        .unwrap();
    }
    let audio = tokio::process::Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(film.join("Scanner Film (2026) mp4.mp4"))
        .args(["-vn", "-c:a", "copy"])
        .arg(film.join("Scanner Film (2026) mp4.en.opus"))
        .status()
        .await
        .unwrap();
    assert!(audio.success());
    std::fs::copy(
        film.join("Scanner Film (2026) mp4.en.opus"),
        film.join("Scanner Film (2026) mkv.en.ogg"),
    )
    .unwrap();
    let db = temp.path().join("scanner.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "scanner-admin",
            Zeroizing::new("disposable scanner browser password".into()),
        )
        .await
        .unwrap();
    store.close().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    for restart in [false, true] {
        let mut child = start(&root, &db, &library, port).await;
        let result = tokio::process::Command::new("node")
            .arg("scripts/browser-scanner.cjs")
            .env("MEDIA_TEST_ORIGIN", format!("http://127.0.0.1:{port}"))
            .env("MEDIA_TEST_RESTART", if restart { "1" } else { "0" })
            .current_dir(&root)
            .status()
            .await
            .unwrap();
        child.kill().await.unwrap();
        assert!(
            result.success(),
            "scanner browser scenario failed (restart={restart})"
        );
    }
    for (path, original) in originals {
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}
