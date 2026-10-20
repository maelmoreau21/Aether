use media_poc::backup_bundle::{BundleError, create, extract};
use std::collections::BTreeMap;
const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
#[tokio::test]
async fn round_trip_both_database_engines_and_encrypted_config() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let data: Vec<u8> = (0..196731).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source, &data).unwrap();
    for engine in ["sqlite", "postgres"] {
        let target = dir.path().join(format!("{engine}.media-backup"));
        let config = BTreeMap::from([
            ("MEDIA_MODE".into(), "standalone".into()),
            (
                "MEDIA_OIDC_CLIENT_SECRET".into(),
                "private-client-secret".into(),
            ),
            ("MEDIA_BACKUP_KEY".into(), KEY.into()),
        ]);
        let size = create(&source, config, &target, KEY, engine).await.unwrap();
        assert!(size > data.len() as u64);
        let encrypted = std::fs::read(&target).unwrap();
        assert!(!encrypted.windows(21).any(|w| w == b"private-client-secret"));
        let output = dir.path().join(format!("{engine}-restored"));
        let result = extract(&target, &output, KEY).await.unwrap();
        assert_eq!(result.engine, engine);
        assert_eq!(result.bytes, data.len() as u64);
        assert_eq!(
            std::fs::read(output.join(result.database_file)).unwrap(),
            data
        );
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("config.json")).unwrap()).unwrap();
        assert_eq!(config["MEDIA_OIDC_CLIENT_SECRET"], "private-client-secret");
        assert!(config.get("MEDIA_BACKUP_KEY").is_none());
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
async fn authentication_rejects_wrong_key_truncation_tamper_reorder_and_trailing_data() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, vec![42; 150000]).unwrap();
    let archive = dir.path().join("good");
    create(&source, BTreeMap::new(), &archive, KEY, "sqlite")
        .await
        .unwrap();
    let good = std::fs::read(&archive).unwrap();
    let output = dir.path().join("wrong-key");
    assert!(extract(&archive, &output, &"aa".repeat(32)).await.is_err());
    assert!(!output.exists());
    let mut variants = vec![good[..39].to_vec(), good[..good.len() - 1].to_vec()];
    let mut tampered = good.clone();
    tampered[70] ^= 1;
    variants.push(tampered);
    let mut trailing = good.clone();
    trailing.push(0);
    variants.push(trailing);
    // Reordering authenticated records must fail even though each payload/tag is unchanged.
    let record_size = |bytes: &[u8], start: usize| {
        5 + u32::from_be_bytes(bytes[start + 1..start + 5].try_into().unwrap()) as usize + 16
    };
    let manifest_size = record_size(&good, 40);
    let first = 40 + manifest_size;
    let second = first + record_size(&good, first);
    let end = second + record_size(&good, second);
    let mut reordered = good[..first].to_vec();
    reordered.extend_from_slice(&good[second..end]);
    reordered.extend_from_slice(&good[first..second]);
    reordered.extend_from_slice(&good[end..]);
    variants.push(reordered);
    for (i, bytes) in variants.iter().enumerate() {
        let archive = dir.path().join(format!("bad-{i}"));
        std::fs::write(&archive, bytes).unwrap();
        let output = dir.path().join(format!("output-{i}"));
        assert!(extract(&archive, &output, KEY).await.is_err());
        assert!(!output.exists());
    }
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with(".media-backup-"))
            .count(),
        0
    );
}
#[tokio::test]
async fn existing_targets_invalid_keys_and_oversized_configuration_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database bytes").unwrap();
    let archive = dir.path().join("archive");
    create(&source, BTreeMap::new(), &archive, KEY, "postgres")
        .await
        .unwrap();
    let original = std::fs::read(&archive).unwrap();
    assert!(matches!(
        create(&source, BTreeMap::new(), &archive, KEY, "postgres").await,
        Err(BundleError::TargetExists)
    ));
    assert_eq!(std::fs::read(&archive).unwrap(), original);
    let output = dir.path().join("existing");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(output.join("sentinel"), b"keep").unwrap();
    assert!(matches!(
        extract(&archive, &output, KEY).await,
        Err(BundleError::TargetExists)
    ));
    assert_eq!(std::fs::read(output.join("sentinel")).unwrap(), b"keep");
    assert!(matches!(
        create(
            &source,
            BTreeMap::new(),
            &dir.path().join("invalid"),
            "abc",
            "sqlite"
        )
        .await,
        Err(BundleError::InvalidKey)
    ));
    let config = BTreeMap::from([("MEDIA_LARGE".into(), "x".repeat(16385))]);
    assert!(matches!(
        create(&source, config, &dir.path().join("large"), KEY, "sqlite").await,
        Err(BundleError::InvalidConfig)
    ));
    // Equal plaintext and root key yield independently salted ciphertext.
    let other = dir.path().join("other");
    create(&source, BTreeMap::new(), &other, KEY, "postgres")
        .await
        .unwrap();
    assert_ne!(&original[8..40], &std::fs::read(other).unwrap()[8..40]);
}
#[cfg(unix)]
#[tokio::test]
async fn extraction_refuses_symlink_parent() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database").unwrap();
    let archive = dir.path().join("archive");
    create(&source, BTreeMap::new(), &archive, KEY, "sqlite")
        .await
        .unwrap();
    let linked = dir.path().join("linked");
    std::os::unix::fs::symlink(dir.path(), &linked).unwrap();
    assert!(
        extract(&archive, &linked.join("output"), KEY)
            .await
            .is_err()
    );
    assert!(!dir.path().join("output").exists());
}
#[tokio::test]
async fn configured_internal_ca_is_authenticated_and_staged_at_fixed_path() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"database").unwrap();
    let ca = dir.path().join("operator-ca.pem");
    let certificate =
        b"-----BEGIN CERTIFICATE-----\nTEST CERTIFICATE BYTES\n-----END CERTIFICATE-----\n";
    std::fs::write(&ca, certificate).unwrap();
    let archive = dir.path().join("backup");
    let config = BTreeMap::from([("MEDIA_INTERNAL_CA_FILE".into(), ca.to_str().unwrap().into())]);
    create(&source, config, &archive, KEY, "sqlite")
        .await
        .unwrap();
    let output = dir.path().join("output");
    extract(&archive, &output, KEY).await.unwrap();
    assert_eq!(
        std::fs::read(output.join("internal-ca.pem")).unwrap(),
        certificate
    );
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("config.json")).unwrap()).unwrap();
    assert_eq!(config["MEDIA_INTERNAL_CA_FILE"], ca.to_str().unwrap());
    let mut bytes = std::fs::read(&archive).unwrap();
    let manifest_len = u32::from_be_bytes(bytes[41..45].try_into().unwrap()) as usize;
    let ca_start = 40 + 5 + manifest_len + 16;
    assert_eq!(bytes[ca_start], 4);
    bytes[ca_start + 5] ^= 1;
    let bad = dir.path().join("bad");
    std::fs::write(&bad, bytes).unwrap();
    let rejected = dir.path().join("rejected");
    assert!(extract(&bad, &rejected, KEY).await.is_err());
    assert!(!rejected.exists());
    let missing = BTreeMap::from([(
        "MEDIA_INTERNAL_CA_FILE".into(),
        dir.path().join("absent.pem").to_str().unwrap().into(),
    )]);
    assert!(
        create(&source, missing, &dir.path().join("missing"), KEY, "sqlite")
            .await
            .is_err()
    );
}
#[tokio::test]
async fn postgres_tls_attachments_round_trip_all_supported_url_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"PGDMP test bytes").unwrap();
    let root = dir.path().join("ca.pem");
    let cert = dir.path().join("client.pem");
    let key = dir.path().join("client-key.pem");
    std::fs::write(&root, b"root certificate").unwrap();
    std::fs::write(&cert, b"client certificate").unwrap();
    std::fs::write(&key, b"private key secret").unwrap();
    for (i, aliases) in [
        ["sslrootcert", "sslcert", "sslkey"],
        ["ssl-root-cert", "ssl-cert", "ssl-key"],
        ["ssl-ca", "sslcert", "sslkey"],
    ]
    .into_iter()
    .enumerate()
    {
        let mut url = url::Url::parse("postgres://user:secret@localhost/database").unwrap();
        for (name, path) in aliases.into_iter().zip([&root, &cert, &key]) {
            url.query_pairs_mut()
                .append_pair(name, path.to_str().unwrap());
        }
        let original = url.to_string();
        let archive = dir.path().join(format!("tls-{i}"));
        create(
            &source,
            BTreeMap::from([("MEDIA_DB_URL".into(), original.clone())]),
            &archive,
            KEY,
            "postgres",
        )
        .await
        .unwrap();
        let output = dir.path().join(format!("tls-output-{i}"));
        extract(&archive, &output, KEY).await.unwrap();
        for (name, expected) in [
            ("postgres-ca.pem", b"root certificate".as_slice()),
            ("postgres-cert.pem", b"client certificate".as_slice()),
            ("postgres-key.pem", b"private key secret".as_slice()),
        ] {
            assert_eq!(std::fs::read(output.join(name)).unwrap(), expected);
        }
        let configuration: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("config.json")).unwrap()).unwrap();
        assert_eq!(configuration["MEDIA_DB_URL"], original);
        let encrypted = std::fs::read(archive).unwrap();
        assert!(!encrypted.windows(18).any(|w| w == b"private key secret"));
    }
    let mut url = url::Url::parse("postgres://localhost/db").unwrap();
    url.query_pairs_mut()
        .append_pair("sslkey", dir.path().join("missing.pem").to_str().unwrap());
    assert!(
        create(
            &source,
            BTreeMap::from([("MEDIA_DB_URL".into(), url.to_string())]),
            &dir.path().join("missing-tls"),
            KEY,
            "postgres"
        )
        .await
        .is_err()
    );
}
#[tokio::test]
async fn attachment_size_and_conflicting_aliases_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("db");
    std::fs::write(&source, b"db").unwrap();
    let large = dir.path().join("large.pem");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(4 * 1024 * 1024 + 1)
        .unwrap();
    let config = BTreeMap::from([(
        "MEDIA_INTERNAL_CA_FILE".into(),
        large.to_str().unwrap().into(),
    )]);
    assert!(matches!(
        create(
            &source,
            config,
            &dir.path().join("too-large"),
            KEY,
            "sqlite"
        )
        .await,
        Err(BundleError::InvalidConfig)
    ));
    let mut url = url::Url::parse("postgres://localhost/db").unwrap();
    url.query_pairs_mut()
        .append_pair("sslkey", "one.pem")
        .append_pair("ssl-key", "two.pem");
    assert!(matches!(
        create(
            &source,
            BTreeMap::from([("MEDIA_DB_URL".into(), url.to_string())]),
            &dir.path().join("conflict"),
            KEY,
            "postgres"
        )
        .await,
        Err(BundleError::InvalidConfig)
    ));
}
#[tokio::test]
async fn empty_database_is_rejected_and_backup_key_file_setting_excluded() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("empty");
    std::fs::write(&source, []).unwrap();
    assert!(matches!(
        create(
            &source,
            BTreeMap::new(),
            &dir.path().join("empty-archive"),
            KEY,
            "sqlite"
        )
        .await,
        Err(BundleError::InvalidBundle)
    ));
    std::fs::write(&source, b"not empty").unwrap();
    let archive = dir.path().join("archive");
    create(
        &source,
        BTreeMap::from([("MEDIA_BACKUP_KEY_FILE".into(), "secret-location".into())]),
        &archive,
        KEY,
        "sqlite",
    )
    .await
    .unwrap();
    let output = dir.path().join("output");
    extract(&archive, &output, KEY).await.unwrap();
    let configuration: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("config.json")).unwrap()).unwrap();
    assert!(configuration.get("MEDIA_BACKUP_KEY_FILE").is_none());
}
