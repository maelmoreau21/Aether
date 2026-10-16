use media_poc::identity::IdentityStore;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;

async fn http(port: u16, method: &str, path: &str, extra: &str, body: &str) -> String {
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n{body}",
        body.len()
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), socket.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8(response).unwrap()
}

async fn start(root: &Path, db: &Path, port: u16) -> tokio::process::Child {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_media-poc"))
        .current_dir(root)
        .env("MEDIA_DB_PATH", db)
        .env("MEDIA_PORT", port.to_string())
        .env("MEDIA_FIXTURE_DIR", root.join("tests/fixtures/generated"))
        .env_remove("MEDIA_PUBLIC_URL")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
async fn production_process_restart_keeps_session_and_logout_revokes_it() {
    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("restart.sqlite3");
    let store = IdentityStore::open(&db).await.unwrap();
    store
        .bootstrap(
            "restart-admin",
            Zeroizing::new("synthetic restart test password".into()),
        )
        .await
        .unwrap();
    store.close().await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut child = start(&root, &db, port).await;
    let response = http(
        port,
        "POST",
        "/api/v1/auth/login",
        &format!("Origin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\n"),
        r#"{"login":"restart-admin","password":"synthetic restart test password"}"#,
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 200"));
    let cookie = response
        .lines()
        .find_map(|line| line.strip_prefix("set-cookie: "))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let identity: serde_json::Value =
        serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    child.kill().await.unwrap();
    let mut child = start(&root, &db, port).await;
    let headers = format!("Cookie: {cookie}\r\n");
    assert!(
        http(port, "GET", "/api/v1/auth/me", &headers, "")
            .await
            .starts_with("HTTP/1.1 200")
    );
    let csrf = identity["csrfToken"].as_str().unwrap();
    let headers =
        format!("Cookie: {cookie}\r\nOrigin: http://127.0.0.1:{port}\r\nx-csrf-token: {csrf}\r\n");
    assert!(
        http(port, "POST", "/api/v1/auth/logout", &headers, "")
            .await
            .starts_with("HTTP/1.1 204")
    );
    child.kill().await.unwrap();
    let mut child = start(&root, &db, port).await;
    assert!(
        http(
            port,
            "GET",
            "/api/v1/auth/me",
            &format!("Cookie: {cookie}\r\n"),
            ""
        )
        .await
        .starts_with("HTTP/1.1 401")
    );
    child.kill().await.unwrap();
}
