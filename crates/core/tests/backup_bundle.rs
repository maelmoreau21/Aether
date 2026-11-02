use media_poc::backup_bundle::{BundleError, create, extract};
use std::collections::BTreeMap;

#[tokio::test]
async fn round_trip_both_engines_without_key() {
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
        ]);
        let size = create(&source, config, &archive, engine).await.unwrap();
        assert!(size > data.len() as u64);
        let bytes = std::fs::read(&archive).unwrap();
        assert_eq!(&bytes[..8], b"MDBK0002");
        // No encryption: operators must keep the entire archive private.
        assert!(
            bytes
                .windows(23)
                .any(|part| part == b"synthetic-private-value")
        );
        let output = dir.path().join(format!("{engine}-restored"));
        let result = extract(&archive, &output).await.unwrap();
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
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
async fn checksum_and_structure_reject_corruption_truncation_and_trailing_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    std::fs::write(&source, vec![42; 150000]).unwrap();
    let archive = dir.path().join("good");
    create(&source, BTreeMap::new(), &archive, "sqlite")
        .await
        .unwrap();
    let good = std::fs::read(&archive).unwrap();
    let mut variants = vec![good[..7].to_vec(), good[..good.len() - 1].to_vec()];
    let mut changed = good.clone();
    changed[50] ^= 1;
    variants.push(changed);
    let mut trailing = good.clone();
    trailing.push(0);
    variants.push(trailing);
    let mut legacy = good.clone();
    legacy[..8].copy_from_slice(b"MDBK0001");
    variants.push(legacy);
    for (index, bytes) in variants.into_iter().enumerate() {
        let damaged = dir.path().join(format!("damaged-{index}"));
        std::fs::write(&damaged, bytes).unwrap();
        let output = dir.path().join(format!("output-{index}"));
        assert!(extract(&damaged, &output).await.is_err());
        assert!(!output.exists());
    }
}

#[tokio::test]
async fn existing_targets_and_oversized_configuration_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database bytes").unwrap();
    let archive = dir.path().join("archive");
    create(&source, BTreeMap::new(), &archive, "sqlite")
        .await
        .unwrap();
    let original = std::fs::read(&archive).unwrap();
    assert!(matches!(
        create(&source, BTreeMap::new(), &archive, "sqlite").await,
        Err(BundleError::TargetExists)
    ));
    assert_eq!(std::fs::read(&archive).unwrap(), original);
    let output = dir.path().join("existing");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("sentinel"), b"keep").unwrap();
    assert!(matches!(
        extract(&archive, &output).await,
        Err(BundleError::TargetExists)
    ));
    assert_eq!(std::fs::read(output.join("sentinel")).unwrap(), b"keep");
    let config = BTreeMap::from([("MEDIA_LARGE".into(), "x".repeat(16385))]);
    assert!(matches!(
        create(&source, config, &dir.path().join("large"), "sqlite").await,
        Err(BundleError::InvalidConfig)
    ));
}

#[tokio::test]
async fn referenced_certificate_round_trips_and_is_covered_by_checksum() {
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
    create(&source, config, &archive, "sqlite").await.unwrap();
    let output = dir.path().join("output");
    extract(&archive, &output).await.unwrap();
    assert_eq!(
        std::fs::read(output.join("internal-ca.pem")).unwrap(),
        b"synthetic certificate bytes"
    );
    let mut bytes = std::fs::read(&archive).unwrap();
    let index = bytes
        .windows(b"synthetic certificate bytes".len())
        .position(|part| part == b"synthetic certificate bytes")
        .unwrap();
    bytes[index] ^= 1;
    let damaged = dir.path().join("damaged");
    std::fs::write(&damaged, bytes).unwrap();
    assert!(
        extract(&damaged, &dir.path().join("rejected"))
            .await
            .is_err()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn extraction_refuses_symlink_parent() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database").unwrap();
    let archive = dir.path().join("archive");
    create(&source, BTreeMap::new(), &archive, "sqlite")
        .await
        .unwrap();
    let linked = dir.path().join("linked");
    std::os::unix::fs::symlink(dir.path(), &linked).unwrap();
    assert!(extract(&archive, &linked.join("output")).await.is_err());
    assert!(!dir.path().join("output").exists());
}
