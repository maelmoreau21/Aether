//! Real Chrome + AV1/Opus playback and optional statistics, on a disposable local Core.
use aether_core::{database::query, identity::IdentityStore};
use std::{
    path::Path,
    process::Stdio,
    time::{Duration, UNIX_EPOCH},
};
use zeroize::Zeroizing;

#[tokio::test]
#[ignore = "requires generated representative fixtures, built Web, FFmpeg and Chrome"]
async fn browser_statistics_video_music_and_restart() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let films = temp.path().join("films");
    let music = temp.path().join("music");
    std::fs::create_dir(&films).unwrap();
    std::fs::create_dir(&music).unwrap();
    let db = temp.path().join("statistics.sqlite");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "statistics-admin",
            Zeroizing::new("disposable statistics browser password".into()),
        )
        .await
        .unwrap();
    query("UPDATE extensions SET enabled=0")
        .execute(store.pool())
        .await
        .unwrap();
    for (id, kind, directory) in [
        ("statistics-films", "films", &films),
        ("statistics-music", "music", &music),
    ] {
        query("INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES(?,?,?,'local',?,1)")
            .bind(id)
            .bind(id)
            .bind(kind)
            .bind(directory.to_string_lossy().as_ref())
            .execute(store.pool())
            .await
            .unwrap();
    }
    query("INSERT INTO inventory_works(id,library_id,identity_key,title,title_sort,kind,external_ids_json,has_available_asset) VALUES('statistics-film','statistics-films','statistics-film','Statistics Film','statistics film','film','{}',1)").execute(store.pool()).await.unwrap();
    query(
        "INSERT INTO inventory_editions(id,work_id) VALUES('statistics-edition','statistics-film')",
    )
    .execute(store.pool())
    .await
    .unwrap();
    for extension in ["mp4", "mkv"] {
        let file = films.join(format!("statistics.{extension}"));
        std::fs::copy(
            root.join(format!(
                "tests/fixtures/generated/representative/representative.{extension}"
            )),
            &file,
        )
        .unwrap();
        let probe = tokio::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&file)
            .output()
            .await
            .unwrap();
        assert!(probe.status.success());
        let probe = String::from_utf8(probe.stdout).unwrap();
        let metadata = std::fs::metadata(&file).unwrap();
        let id = format!("statistics-{extension}");
        query("INSERT INTO inventory_versions(id,edition_id) VALUES(?,'statistics-edition')")
            .bind(&id)
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO inventory_sources(id,version_id,library_id,root_snapshot) VALUES(?,?,'statistics-films',?)").bind(&id).bind(&id).bind(films.to_string_lossy().as_ref()).execute(store.pool()).await.unwrap();
        query("INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES(?,?,'statistics-films',?,?,?,?,?,1)")
            .bind(&id).bind(&id).bind(format!("statistics.{extension}")).bind(metadata.len() as i64).bind(metadata.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos().to_string())
            .bind(if extension=="mp4"{"video/mp4"}else{"video/x-matroska"}).bind(probe).execute(store.pool()).await.unwrap();
    }
    let song = music.join("statistics.opus");
    assert!(
        tokio::process::Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(films.join("statistics.mp4"))
            .args(["-map", "0:a:0", "-vn", "-c:a", "copy"])
            .arg(&song)
            .status()
            .await
            .unwrap()
            .success()
    );
    let metadata = std::fs::metadata(&song).unwrap();
    query(
        "INSERT INTO music_tracks(id,library_id,root_snapshot,relative_path,size,modified_ns,\
            title,artist,album,album_artist,duration,codec,mime,scan_id,available) \
            VALUES('statistics-song','statistics-music',?,'statistics.opus',?,?,'Statistics Song',\
            'Statistics Artist','Statistics Album','Statistics Artist',12.0,'opus','audio/ogg',\
            'fixture',1)",
    )
    .bind(music.to_string_lossy().as_ref())
    .bind(metadata.len() as i64)
    .bind(
        metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string(),
    )
    .execute(store.pool())
    .await
    .unwrap();
    store.close().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    for restart in [false, true] {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_aether-core"))
            .current_dir(&root)
            .env("MEDIA_DB_PATH", &db)
            .env("MEDIA_DB_ENGINE", "sqlite")
            .env_remove("MEDIA_DB_URL")
            .env_remove("MEDIA_PUBLIC_URL")
            .env("MEDIA_PORT", port.to_string())
            .env("MEDIA_LIBRARY_ROOT", &films)
            .env("TEMP", temp.path())
            .env("TMP", temp.path())
            .env("TMPDIR", temp.path())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                assert!(child.try_wait().unwrap().is_none(), "Core exited");
                if tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        let status = tokio::process::Command::new("node")
            .arg("scripts/browser-statistics.cjs")
            .current_dir(&root)
            .env("MEDIA_TEST_ORIGIN", format!("http://127.0.0.1:{port}"))
            .env("MEDIA_TEST_RESTART", if restart { "1" } else { "0" })
            .status()
            .await
            .unwrap();
        child.kill().await.unwrap();
        assert!(
            status.success(),
            "statistics browser flow failed (restart={restart})"
        );
    }
}
