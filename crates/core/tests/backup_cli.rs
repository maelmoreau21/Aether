//! `media-backup --full` and `media-recover` as operators run them: key from the environment,
//! fail closed without one, legacy plaintext only on explicit opt-in.
use aether_core::identity::IdentityStore;
use std::{path::Path, process::Output};

const KEY: &str = "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12";

fn run(bin: &str, args: &[&Path], root: &Path, env: &[(&str, &str)]) -> Output {
    let mut command = std::process::Command::new(bin);
    command
        .args(args)
        .current_dir(root)
        .env_remove("MEDIA_BACKUP_KEY")
        .env_remove("MEDIA_BACKUP_KEY_FILE")
        .env_remove("MEDIA_BACKUP_ALLOW_UNENCRYPTED")
        .env("MEDIA_DB_ENGINE", "sqlite")
        .env("MEDIA_DB_PATH", root.join("core.sqlite3"))
        .env("MEDIA_LIBRARY_ROOT", root.join("media"))
        .env("MEDIA_SYNTHETIC_SECRET", "cli-secret-marker");
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[tokio::test]
async fn full_backup_cli_is_encrypted_fail_closed_and_recoverable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join("media")).unwrap();
    IdentityStore::open(&root.join("core.sqlite3"))
        .await
        .unwrap()
        .close()
        .await;
    let backup = env!("CARGO_BIN_EXE_media-backup");
    let recover = env!("CARGO_BIN_EXE_media-recover");

    // No key: refused before anything is written, nothing is left behind.
    let refused = root.join("refused.media-backup");
    let output = run(backup, &[Path::new("--full"), &refused], root, &[]);
    assert!(!output.status.success());
    assert!(text(&output).contains("no backup key configured"));
    assert!(!refused.exists());
    let leftovers = std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".media-"))
        .count();
    assert_eq!(leftovers, 0);

    // A weak key is refused as well.
    let weak = "11".repeat(32);
    let output = run(
        backup,
        &[Path::new("--full"), &refused],
        root,
        &[("MEDIA_BACKUP_KEY", &weak)],
    );
    assert!(!output.status.success());
    assert!(!text(&output).contains(&weak));
    assert!(!refused.exists());

    // Key from a file: encrypted archive, no secret in stdout/stderr or in the file.
    let key_file = root.join("backup.key");
    std::fs::write(&key_file, format!("{KEY}\n")).unwrap();
    let key_path = key_file.to_string_lossy().into_owned();
    let archive = root.join("full.media-backup");
    let output = run(
        backup,
        &[Path::new("--full"), &archive],
        root,
        &[("MEDIA_BACKUP_KEY_FILE", &key_path)],
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(!text(&output).contains(KEY));
    let bytes = std::fs::read(&archive).unwrap();
    assert_eq!(&bytes[..8], b"MDBK0003");
    assert!(
        !bytes
            .windows(b"cli-secret-marker".len())
            .any(|part| part == b"cli-secret-marker")
    );

    // Recovery needs the key; failures leave no output and never touch the archive.
    let before = std::fs::read(&archive).unwrap();
    for (name, env) in [
        ("no-key", vec![]),
        (
            "wrong-key",
            vec![(
                "MEDIA_BACKUP_KEY",
                "a1b2c3d4e5f60718293a4b5c6d7e8f90112233445566778899aabbccddeeff01",
            )],
        ),
    ] {
        let target = root.join(name);
        let output = run(recover, &[&archive, &target], root, &env);
        assert!(!output.status.success(), "{name}");
        assert!(!target.exists(), "{name}");
        assert!(!text(&output).contains(KEY));
    }
    let recovered = root.join("recovered");
    let output = run(
        recover,
        &[&archive, &recovered],
        root,
        &[("MEDIA_BACKUP_KEY_FILE", &key_path)],
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(recovered.join("database.sqlite3").is_file());
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(recovered.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["MEDIA_SYNTHETIC_SECRET"], "cli-secret-marker");
    assert!(config.get("MEDIA_BACKUP_KEY_FILE").is_none());
    assert_eq!(std::fs::read(&archive).unwrap(), before);

    // Explicit opt-in still produces the legacy format, which recovers without any key.
    let legacy = root.join("legacy.media-backup");
    let output = run(
        backup,
        &[Path::new("--full"), &legacy],
        root,
        &[("MEDIA_BACKUP_ALLOW_UNENCRYPTED", "true")],
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("WARNING"));
    assert_eq!(&std::fs::read(&legacy).unwrap()[..8], b"MDBK0002");
    let legacy_out = root.join("legacy-recovered");
    let output = run(recover, &[&legacy, &legacy_out], root, &[]);
    assert!(output.status.success(), "{}", text(&output));
    assert!(legacy_out.join("database.sqlite3").is_file());
}
