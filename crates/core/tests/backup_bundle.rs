use media_poc::backup_bundle::{BackupKey, BundleError, Protection, create, extract};
use std::collections::BTreeMap;

const KEY: &str = "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12";

fn key() -> BackupKey {
    BackupKey::parse(KEY).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|part| part == needle)
}

#[tokio::test]
async fn encrypted_round_trip_both_engines_hides_secrets_and_requires_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let data: Vec<u8> = (0..196731).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &data).unwrap();
    for engine in ["sqlite", "postgres"] {
        let archive = dir.path().join(format!("{engine}.media-backup"));
        let config = BTreeMap::from([
            ("MEDIA_MODE".into(), "standalone".into()),
            (
                "MEDIA_OIDC_CLIENT_SECRET".into(),
                "synthetic-private-value".into(),
            ),
            // A key accidentally present in the captured configuration must never be archived.
            ("MEDIA_BACKUP_KEY".into(), KEY.into()),
        ]);
        let size = create(
            &source,
            config,
            &archive,
            engine,
            Protection::Encrypted(key()),
        )
        .await
        .unwrap();
        assert!(size > data.len() as u64);
        let bytes = std::fs::read(&archive).unwrap();
        assert_eq!(&bytes[..8], b"MDBK0003");
        assert!(!contains(&bytes, b"synthetic-private-value"));
        assert!(!contains(&bytes, b"standalone"));
        assert!(!contains(&bytes, KEY.as_bytes()));
        assert!(!contains(&bytes, &data[5000..5064]));
        assert!(matches!(
            extract(&archive, &dir.path().join("no-key"), None).await,
            Err(BundleError::KeyRequired)
        ));
        let output = dir.path().join(format!("{engine}-restored"));
        let result = extract(&archive, &output, Some(&key())).await.unwrap();
        assert_eq!(result.engine, engine);
        assert_eq!(result.bytes, data.len() as u64);
        assert_eq!(
            std::fs::read(output.join(result.database_file)).unwrap(),
            data
        );
        let restored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("config.json")).unwrap()).unwrap();
        assert_eq!(
            restored["MEDIA_OIDC_CLIENT_SECRET"],
            "synthetic-private-value"
        );
        assert!(restored.get("MEDIA_BACKUP_KEY").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(output.join("config.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[tokio::test]
async fn legacy_unencrypted_archives_remain_readable_without_conversion_or_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let data: Vec<u8> = (0..196731).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &data).unwrap();
    let archive = dir.path().join("legacy.media-backup");
    let config = BTreeMap::from([("MEDIA_OIDC_CLIENT_SECRET".into(), "legacy-secret".into())]);
    create(&source, config, &archive, "sqlite", Protection::Unencrypted)
        .await
        .unwrap();
    let original = std::fs::read(&archive).unwrap();
    assert_eq!(&original[..8], b"MDBK0002");
    // Documented hazard of the legacy format: the secret is visible in the file.
    assert!(contains(&original, b"legacy-secret"));
    // Readable without a key, and a configured key does not get in the way.
    for (index, key) in [None, Some(key())].into_iter().enumerate() {
        let output = dir.path().join(format!("legacy-out-{index}"));
        let summary = extract(&archive, &output, key.as_ref()).await.unwrap();
        assert_eq!(summary.bytes, data.len() as u64);
        assert_eq!(
            std::fs::read(output.join("database.sqlite3")).unwrap(),
            data
        );
    }
    // Reading never modifies, converts or removes the source archive.
    assert_eq!(std::fs::read(&archive).unwrap(), original);
}

#[tokio::test]
async fn corruption_truncation_trailing_bytes_and_bad_magic_are_rejected_in_both_formats() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::write(&source, vec![42; 150000]).unwrap();
    for (label, protection) in [
        ("v3", Protection::Encrypted(key())),
        ("v2", Protection::Unencrypted),
    ] {
        let archive = dir.path().join(format!("good-{label}"));
        create(&source, BTreeMap::new(), &archive, "sqlite", protection)
            .await
            .unwrap();
        let good = std::fs::read(&archive).unwrap();
        let mut variants = vec![good[..7].to_vec(), good[..good.len() - 1].to_vec()];
        let mut changed = good.clone();
        changed[50] ^= 1;
        variants.push(changed);
        let mut last = good.clone();
        let tail = last.len() - 1;
        last[tail] ^= 1;
        variants.push(last);
        let mut trailing = good.clone();
        trailing.push(0);
        variants.push(trailing);
        let mut unknown = good.clone();
        unknown[..8].copy_from_slice(b"MDBK0099");
        variants.push(unknown);
        variants.push(Vec::new());
        for (index, bytes) in variants.into_iter().enumerate() {
            let damaged = dir.path().join(format!("damaged-{label}-{index}"));
            std::fs::write(&damaged, bytes).unwrap();
            let output = dir.path().join(format!("output-{label}-{index}"));
            assert!(
                extract(&damaged, &output, Some(&key())).await.is_err(),
                "{label} variant {index}"
            );
            assert!(!output.exists());
        }
        // No staging directory is left behind by any failure.
        assert!(
            !std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .any(|entry| entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".media-backup-"))
        );
    }
}

#[tokio::test]
async fn legacy_encrypted_magic_is_reported_distinctly_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("old-key-archive");
    let mut bytes = b"MDBK0001".to_vec();
    bytes.extend_from_slice(&[9; 200]);
    std::fs::write(&archive, &bytes).unwrap();
    let output = dir.path().join("out");
    assert!(matches!(
        extract(&archive, &output, Some(&key())).await,
        Err(BundleError::LegacyEncrypted)
    ));
    assert!(!output.exists());
    assert_eq!(std::fs::read(&archive).unwrap(), bytes);
}

#[tokio::test]
async fn existing_targets_and_oversized_configuration_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database bytes").unwrap();
    let archive = dir.path().join("archive");
    let protection = || Protection::Encrypted(key());
    create(&source, BTreeMap::new(), &archive, "sqlite", protection())
        .await
        .unwrap();
    let original = std::fs::read(&archive).unwrap();
    assert!(matches!(
        create(&source, BTreeMap::new(), &archive, "sqlite", protection()).await,
        Err(BundleError::TargetExists)
    ));
    assert_eq!(std::fs::read(&archive).unwrap(), original);
    let output = dir.path().join("existing");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("sentinel"), b"keep").unwrap();
    assert!(matches!(
        extract(&archive, &output, Some(&key())).await,
        Err(BundleError::TargetExists)
    ));
    assert_eq!(std::fs::read(output.join("sentinel")).unwrap(), b"keep");
    let config = BTreeMap::from([("MEDIA_LARGE".into(), "x".repeat(16385))]);
    assert!(matches!(
        create(
            &source,
            config,
            &dir.path().join("large"),
            "sqlite",
            protection()
        )
        .await,
        Err(BundleError::InvalidConfig)
    ));
    // A failed creation leaves neither a target nor a temporary file.
    assert!(!dir.path().join("large").exists());
    let leftovers = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".media-backup-")
        })
        .count();
    assert_eq!(leftovers, 0);
    // An empty database snapshot is refused rather than archived.
    let empty = dir.path().join("empty");
    std::fs::write(&empty, b"").unwrap();
    assert!(matches!(
        create(
            &empty,
            BTreeMap::new(),
            &dir.path().join("never"),
            "sqlite",
            protection()
        )
        .await,
        Err(BundleError::InvalidBundle)
    ));
    assert!(!dir.path().join("never").exists());
}

#[tokio::test]
async fn referenced_certificate_round_trips_encrypted_and_is_authenticated() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database bytes").unwrap();
    let certificate = dir.path().join("operator-ca.pem");
    std::fs::write(&certificate, b"synthetic certificate bytes").unwrap();
    let archive = dir.path().join("archive");
    let config = BTreeMap::from([(
        "MEDIA_INTERNAL_CA_FILE".into(),
        certificate.to_string_lossy().into_owned(),
    )]);
    create(
        &source,
        config,
        &archive,
        "sqlite",
        Protection::Encrypted(key()),
    )
    .await
    .unwrap();
    let bytes = std::fs::read(&archive).unwrap();
    assert!(!contains(&bytes, b"synthetic certificate bytes"));
    let output = dir.path().join("output");
    extract(&archive, &output, Some(&key())).await.unwrap();
    assert_eq!(
        std::fs::read(output.join("internal-ca.pem")).unwrap(),
        b"synthetic certificate bytes"
    );
    // Flipping any byte of the sealed certificate record is detected (not just by a checksum
    // an attacker could recompute).
    for index in (60..bytes.len()).step_by(7) {
        let mut damaged = bytes.clone();
        damaged[index] ^= 1;
        let path = dir.path().join("damaged");
        std::fs::write(&path, damaged).unwrap();
        assert!(
            extract(
                &path,
                &dir.path().join(format!("rejected-{index}")),
                Some(&key())
            )
            .await
            .is_err(),
            "byte {index}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn extraction_and_creation_refuse_symlink_parents_and_symlinked_certificates() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database").unwrap();
    let archive = dir.path().join("archive");
    create(
        &source,
        BTreeMap::new(),
        &archive,
        "sqlite",
        Protection::Encrypted(key()),
    )
    .await
    .unwrap();
    let linked = dir.path().join("linked");
    std::os::unix::fs::symlink(dir.path(), &linked).unwrap();
    assert!(
        extract(&archive, &linked.join("output"), Some(&key()))
            .await
            .is_err()
    );
    assert!(!dir.path().join("output").exists());
    assert!(
        create(
            &source,
            BTreeMap::new(),
            &linked.join("second"),
            "sqlite",
            Protection::Encrypted(key())
        )
        .await
        .is_err()
    );
    assert!(!dir.path().join("second").exists());
    // A certificate path that is a symlink (e.g. to /etc/shadow) is never embedded.
    let secret = dir.path().join("not-a-certificate");
    std::fs::write(&secret, b"private").unwrap();
    let link = dir.path().join("ca.pem");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    let config = BTreeMap::from([(
        "MEDIA_INTERNAL_CA_FILE".into(),
        link.to_string_lossy().into_owned(),
    )]);
    assert!(matches!(
        create(
            &source,
            config,
            &dir.path().join("third"),
            "sqlite",
            Protection::Encrypted(key())
        )
        .await,
        Err(BundleError::InvalidConfig)
    ));
    assert!(!dir.path().join("third").exists());
}
