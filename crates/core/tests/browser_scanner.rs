//! Disposable browser scan/playback evidence, including a real Core process restart.
use media_poc::identity::IdentityStore;
use std::{path::Path, process::Stdio, time::Duration};
use zeroize::Zeroizing;

async fn start(root: &Path, db: &Path, library: &Path, port: u16) -> tokio::process::Child {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-poc"))
        .current_dir(root)
        .env("MEDIA_DB_PATH", db)
        .env("MEDIA_DB_ENGINE", "sqlite")
        .env_remove("MEDIA_DB_URL")
        // A forced process restart must not leave preparation files in the host temp directory.
        .env("TMPDIR", db.parent().unwrap())
        .env("TEMP", db.parent().unwrap())
        .env("TMP", db.parent().unwrap())
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
    let long = std::env::var("MEDIA_TEST_LONG").as_deref() == Ok("1");
    fn digest(path: &Path) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut file = std::fs::File::open(path).unwrap();
        let mut hash = Sha256::new();
        let mut bytes = [0; 65536];
        loop {
            let n = file.read(&mut bytes).unwrap();
            if n == 0 {
                break;
            }
            hash.update(&bytes[..n]);
        }
        hash.finalize().to_vec()
    }
    let mut originals = Vec::new();
    for extension in ["mp4", "mkv"] {
        let input = root.join(format!(
            "tests/fixtures/generated/representative/representative.{extension}"
        ));
        let output = film.join(format!("Scanner Film (2026) {extension}.{extension}"));
        if long {
            let status = tokio::process::Command::new("ffmpeg")
                .args(["-v", "error", "-nostdin", "-stream_loop", "-1", "-i"])
                .arg(&input)
                .args(["-t", "5400", "-map", "0", "-c", "copy"])
                .arg(&output)
                .status()
                .await
                .unwrap();
            assert!(status.success());
        } else {
            std::fs::copy(&input, &output).unwrap();
        }
        originals.push((output.clone(), digest(&output)));
    }
    for container in ["mp4", "mkv"] {
        std::fs::write(film.join(format!("Scanner Film (2026) {container}.en.ass")), "[Script Info]\nScriptType: v4.00+\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:12.00,Default,,0,0,0,,{\\i1}ASS Hello, world{\\i0}\\NSecond line").unwrap();
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
    let task_backlog = std::env::var("MEDIA_TEST_TASK_BACKLOG").as_deref() == Ok("1");
    if task_backlog {
        media_poc::libraries::initialize_local(&store)
            .await
            .unwrap();
        let empty = library.join("background-task-queue");
        std::fs::create_dir_all(&empty).unwrap();
        let empty = std::fs::canonicalize(empty).unwrap();
        media_poc::database::query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('browser-task-backlog','Browser task backlog','films','local',?,1)").bind(empty.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
        let timestamp = media_contracts::protocol::unix_now();
        let mut tx = store.pool().begin().await.unwrap();
        for index in 0..10_000 {
            let id = format!("browser-backlog-{index:05}");
            media_poc::database::query("INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,created_at,updated_at,available_at) VALUES(?,'browser-task-backlog','browser-test',?,'local',?,?,?)")
                .bind(&id).bind(&id).bind(timestamp).bind(timestamp).bind(timestamp).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
    }
    store.close().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut completed_before_restart = 0_i64;
    for restart in [false, true] {
        let mut child = start(&root, &db, &library, port).await;
        let mut cmd = tokio::process::Command::new("node");
        cmd.arg("scripts/browser-scanner.cjs")
            .env("MEDIA_TEST_ORIGIN", format!("http://127.0.0.1:{port}"))
            .env("MEDIA_TEST_RESTART", if restart { "1" } else { "0" })
            .current_dir(&root);
        if let Ok(dir) = std::env::var("MEDIA_REPORT_DIR") {
            cmd.env("MEDIA_REPORT_DIR", dir);
        }
        let result = cmd.status().await.unwrap();
        if task_backlog && result.success() {
            let check = IdentityStore::open(&db).await.unwrap();
            let completed=tokio::time::timeout(Duration::from_secs(75),async {
                loop {
                    let completed:i64=media_poc::database::query_scalar("SELECT COUNT(*) FROM durable_tasks WHERE owner='browser-test' AND status='completed'").fetch_one(check.pool()).await.unwrap();
                    if completed>completed_before_restart {break completed;}
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }).await.expect("background queue must make real progress across restart");
            let total: i64 = media_poc::database::query_scalar(
                "SELECT COUNT(*) FROM durable_tasks WHERE owner='browser-test'",
            )
            .fetch_one(check.pool())
            .await
            .unwrap();
            assert_eq!(total, 10_000, "no task may be silently lost");
            let evidence = serde_json::json!({"restart":restart,"fixtureBacklog":total,"completed":completed,"completedBeforeRestart":completed_before_restart,"browserFlowPassed":true,"scope":"real browser image and PCM assertions from browser-scanner.cjs; backlog consists of empty-library scans, not 10000 media probes"});
            println!("BROWSER_TASK_BACKLOG {evidence}");
            if let Ok(directory) = std::env::var("MEDIA_REPORT_DIR") {
                let directory = std::path::PathBuf::from(directory);
                let directory = if directory.is_absolute() {
                    directory
                } else {
                    root.join(directory)
                };
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(
                    directory.join(if restart {
                        "task-backlog-restart.json"
                    } else {
                        "task-backlog.json"
                    }),
                    serde_json::to_vec_pretty(&evidence).unwrap(),
                )
                .unwrap();
            }
            completed_before_restart = completed;
            check.close().await;
        }
        child.kill().await.unwrap();
        assert!(
            result.success(),
            "scanner browser scenario failed (restart={restart})"
        );
    }
    for (path, original) in originals {
        assert_eq!(digest(&path), original);
    }
}
