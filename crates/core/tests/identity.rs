use argon2::{
    Argon2,
    password_hash::{PasswordVerifier, phc::PasswordHash},
};
use media_poc::identity::{IdentityError, IdentityStore};
use sqlx::{Connection, Row};
use zeroize::Zeroizing;
const PASSWORD: &str = "synthetic test password only";

#[tokio::test]
async fn migrations_bootstrap_hash_restart_and_permanent_closure() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("identity.sqlite3");
    let store = IdentityStore::open(&path).await.unwrap();
    let id = store
        .bootstrap("local-admin", Zeroizing::new(PASSWORD.into()))
        .await
        .unwrap();
    store.close().await;
    let reopened = IdentityStore::open(&path).await.unwrap();
    assert_eq!(
        reopened.user(&id).await.unwrap().unwrap().role,
        media_poc::identity::Role::Administrator
    );
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite:{}", path.display()))
        .await
        .unwrap();
    let row = sqlx::query("SELECT u.id, u.role, c.password_hash FROM users u JOIN local_credentials c ON c.user_id=u.id").fetch_one(&mut conn).await.unwrap();
    assert_eq!(row.get::<String, _>("id"), id);
    assert_eq!(row.get::<String, _>("role"), "administrator");
    let hash: String = row.get("password_hash");
    assert!(hash.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"));
    assert!(!hash.contains(PASSWORD));
    let parsed = PasswordHash::new(&hash).unwrap();
    assert!(
        Argon2::default()
            .verify_password(PASSWORD.as_bytes(), &parsed)
            .is_ok()
    );
    assert!(
        Argon2::default()
            .verify_password(b"incorrect", &parsed)
            .is_err()
    );
    assert!(matches!(
        reopened
            .bootstrap("another", Zeroizing::new(PASSWORD.into()))
            .await,
        Err(IdentityError::AlreadyInitialized)
    ));
    assert!(
        sqlx::query("INSERT INTO users(id,role) VALUES ('bad','guest')")
            .execute(&mut conn)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO local_credentials(user_id,login,password_hash) VALUES ('missing','test','hash')").execute(&mut conn).await.is_err());
    sqlx::query("DELETE FROM local_credentials")
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users")
        .execute(&mut conn)
        .await
        .unwrap();
    assert!(matches!(
        reopened
            .bootstrap("another", Zeroizing::new(PASSWORD.into()))
            .await,
        Err(IdentityError::AlreadyInitialized)
    ));
    reopened.close().await;
}

#[tokio::test]
async fn independent_connections_compete_and_invalid_input_does_not_consume_bootstrap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("identity.sqlite3");
    let (a, b) = tokio::join!(IdentityStore::open(&path), IdentityStore::open(&path));
    let a = a.unwrap();
    let b = b.unwrap();
    assert!(matches!(
        a.bootstrap("INVALID", Zeroizing::new(PASSWORD.into()))
            .await,
        Err(IdentityError::InvalidLogin)
    ));
    assert!(matches!(
        a.bootstrap("valid", Zeroizing::new("short".into())).await,
        Err(IdentityError::InvalidPassword)
    ));
    let (one, two) = tokio::join!(
        a.bootstrap("first", Zeroizing::new(PASSWORD.into())),
        b.bootstrap("second", Zeroizing::new(PASSWORD.into()))
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert!(matches!(
        one.err().or(two.err()),
        Some(IdentityError::AlreadyInitialized)
    ));
    a.close().await;
    b.close().await;
}

#[tokio::test]
async fn missing_parent_and_corrupt_database_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    assert!(
        IdentityStore::open(&temp.path().join("absent/db.sqlite3"))
            .await
            .is_err()
    );
    let path = temp.path().join("corrupt.sqlite3");
    std::fs::write(&path, b"not a sqlite database").unwrap();
    assert!(IdentityStore::open(&path).await.is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"not a sqlite database");
}

#[test]
fn cli_rejects_noninteractive_input_and_secret_arguments() {
    for args in [vec![], vec!["forbidden-secret-argument"]] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_media-bootstrap"))
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("forbidden-secret-argument"));
    }
}

#[test]
#[ignore = "subprocess helper, exercised by competing_processes"]
fn bootstrap_process_worker() {
    let path = std::env::var_os("MEDIA_TEST_DB").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let store = IdentityStore::open(std::path::Path::new(&path))
            .await
            .unwrap();
        let result = store
            .bootstrap("process-admin", Zeroizing::new(PASSWORD.into()))
            .await;
        store.close().await;
        match result {
            Ok(_) => std::process::exit(0),
            Err(IdentityError::AlreadyInitialized) => std::process::exit(20),
            Err(_) => std::process::exit(21),
        }
    });
}

#[test]
fn competing_processes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("process.sqlite3");
    let mut children = Vec::new();
    for _ in 0..3 {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "bootstrap_process_worker"])
                .env("MEDIA_TEST_DB", &path)
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    let codes: Vec<_> = children
        .iter_mut()
        .map(|p| p.wait().unwrap().code().unwrap())
        .collect();
    assert_eq!(codes.iter().filter(|&&c| c == 0).count(), 1, "{codes:?}");
    assert_eq!(codes.iter().filter(|&&c| c == 20).count(), 2, "{codes:?}");
}

#[tokio::test]
async fn failed_insert_rolls_back_entire_bootstrap() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rollback.sqlite3");
    let store = IdentityStore::open(&path).await.unwrap();
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite:{}", path.display()))
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_credentials BEFORE INSERT ON local_credentials BEGIN SELECT RAISE(ABORT,'synthetic failure'); END").execute(&mut conn).await.unwrap();
    assert!(matches!(
        store
            .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
            .await,
        Err(IdentityError::Database(_))
    ));
    sqlx::query("DROP TRIGGER reject_credentials")
        .execute(&mut conn)
        .await
        .unwrap();
    assert!(
        store
            .bootstrap("admin", Zeroizing::new(PASSWORD.into()))
            .await
            .is_ok()
    );
    store.close().await;
}
