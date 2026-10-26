//! Versioned, chunk-authenticated encrypted backup bundles. No archive-provided paths.
use ring::{
    aead, hkdf,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;
const MAGIC: &[u8; 8] = b"MDBK0001";
const CHUNK: usize = 65536;
/// Publicly known key used by pre-hardening betas. It is never accepted for creating or restoring
/// archives unless the operator explicitly calls [`extract_legacy_public_key`].
const LEGACY_PUBLIC_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const INSTALLATION_KEY_FILE: &str = "backup.key";
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("backup I/O failed")]
    Io(#[from] std::io::Error),
    #[error(
        "backup key must be exactly 64 hexadecimal characters and must not be a known weak value"
    )]
    InvalidKey,
    #[error("no backup key configured: set MEDIA_BACKUP_KEY or MEDIA_BACKUP_KEY_FILE")]
    MissingKey,
    #[error("backup authentication or format validation failed")]
    InvalidBundle,
    #[error("backup target already exists")]
    TargetExists,
    #[error("backup configuration is invalid or exceeds 64 KiB")]
    InvalidConfig,
    #[error("backup worker failed")]
    Worker,
    #[error("an asset is missing or does not match its recorded SHA-256 or size")]
    AssetIntegrity,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    engine: String,
    config: BTreeMap<String, String>,
    #[serde(default)]
    attachments: BTreeMap<String, u64>,
    /// Number of content-addressed assets carried by the archive (0 for database-only archives).
    #[serde(default)]
    assets: u64,
    #[serde(default)]
    asset_bytes: u64,
}
/// One durable binary object to embed. `(sha256, size, mime)` come from the SQL `assets` table.
pub struct AssetSource {
    pub items: Vec<(String, u64, String)>,
    pub open: AssetOpener,
}
/// Opens one stored object by digest (blocking).
pub type AssetOpener = Box<dyn Fn(&str) -> std::io::Result<File> + Send + 'static>;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetHeader {
    sha256: String,
    size: u64,
    mime: String,
}
fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Debug, Serialize)]
pub struct BundleSummary {
    pub engine: String,
    pub database_file: String,
    pub bytes: u64,
    /// Verified assets written to `<output>/assets/<sha256>` (listed in `assets.json`).
    pub assets: u64,
}
struct Cleanup(PathBuf, bool);
impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.1 {
            if self.0.is_dir() {
                let _ = fs::remove_dir_all(&self.0);
            } else {
                let _ = fs::remove_file(&self.0);
            }
        }
    }
}
fn decode_key_inner(value: &str, allow_legacy: bool) -> Result<Zeroizing<[u8; 32]>, BundleError> {
    if value.is_empty() {
        return Err(BundleError::MissingKey);
    }
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(BundleError::InvalidKey);
    }
    if !allow_legacy && value == LEGACY_PUBLIC_KEY {
        return Err(BundleError::InvalidKey);
    }
    let mut key = Zeroizing::new([0; 32]);
    for (i, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| BundleError::InvalidKey)?;
        key[i] = u8::from_str_radix(text, 16).map_err(|_| BundleError::InvalidKey)?;
    }
    Ok(key)
}
fn decode_key(value: &str) -> Result<Zeroizing<[u8; 32]>, BundleError> {
    decode_key_inner(value, false)
}
/// Validate a backup key without exposing it. Empty, malformed and the legacy public key are refused.
pub fn validate_key(value: &str) -> Result<(), BundleError> {
    decode_key(value).map(|_| ())
}
/// Resolve the backup key with a strict order and no fixed fallback:
/// 1. `explicit` (`MEDIA_BACKUP_KEY`) — a set but invalid value is an error, never ignored;
/// 2. `key_file` (`MEDIA_BACKUP_KEY_FILE`) — must be a regular file containing 64 hex digits;
/// 3. the installation key `<data_dir>/secrets/backup.key`, generated from the OS CSPRNG when
///    `generate` is true and the file does not exist yet (created atomically, mode 0600 on Unix).
///
/// The key is never written inside an archive.
pub fn resolve_key(
    explicit: Option<&str>,
    key_file: Option<&Path>,
    data_dir: Option<&Path>,
    generate: bool,
) -> Result<Zeroizing<String>, BundleError> {
    if let Some(value) = explicit.filter(|v| !v.is_empty()) {
        validate_key(value.trim())?;
        return Ok(Zeroizing::new(value.trim().to_owned()));
    }
    if let Some(path) = key_file {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file() || meta.len() > 256 {
            return Err(BundleError::InvalidKey);
        }
        let value = Zeroizing::new(fs::read_to_string(path)?);
        validate_key(value.trim())?;
        return Ok(Zeroizing::new(value.trim().to_owned()));
    }
    let dir = data_dir.ok_or(BundleError::MissingKey)?.join("secrets");
    let path = dir.join(INSTALLATION_KEY_FILE);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() <= 256 => {
            let value = Zeroizing::new(fs::read_to_string(&path)?);
            validate_key(value.trim())?;
            return Ok(Zeroizing::new(value.trim().to_owned()));
        }
        Ok(_) => return Err(BundleError::InvalidKey),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if !generate {
        return Err(BundleError::MissingKey);
    }
    let _ = private_dir(&dir);
    let mut bytes = Zeroizing::new([0u8; 32]);
    SystemRandom::new()
        .fill(bytes.as_mut())
        .map_err(|_| BundleError::InvalidKey)?;
    let hex = Zeroizing::new(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
    validate_key(&hex)?;
    match private_file(&path) {
        Ok(mut file) => {
            file.write_all(hex.as_bytes())?;
            file.sync_all()?;
            Ok(hex)
        }
        // Another process created it first: use that key, never overwrite.
        Err(BundleError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            resolve_key(None, None, data_dir, false)
        }
        Err(e) => Err(e),
    }
}
struct KeyLength;
impl hkdf::KeyType for KeyLength {
    fn len(&self) -> usize {
        32
    }
}
fn cipher(root: &[u8], salt: &[u8]) -> Result<aead::LessSafeKey, BundleError> {
    let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, salt);
    let prk = salt.extract(root);
    let info = [b"media-backup-v1".as_slice()];
    let okm = prk
        .expand(&info, KeyLength)
        .map_err(|_| BundleError::InvalidBundle)?;
    let mut sub = Zeroizing::new([0; 32]);
    okm.fill(sub.as_mut())
        .map_err(|_| BundleError::InvalidBundle)?;
    Ok(aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, sub.as_ref())
            .map_err(|_| BundleError::InvalidBundle)?,
    ))
}
fn nonce(counter: u64) -> aead::Nonce {
    let mut bytes = [0; 12];
    bytes[4..].copy_from_slice(&counter.to_be_bytes());
    aead::Nonce::assume_unique_for_key(bytes)
}
fn aad(header: &[u8], counter: u64, kind: u8, len: u32) -> Vec<u8> {
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&counter.to_be_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&len.to_be_bytes());
    bytes
}
fn record(
    out: &mut File,
    key: &aead::LessSafeKey,
    header: &[u8],
    counter: u64,
    kind: u8,
    plain: &[u8],
) -> Result<(), BundleError> {
    let len = u32::try_from(plain.len()).map_err(|_| BundleError::InvalidBundle)?;
    let mut data = Zeroizing::new(plain.to_vec());
    key.seal_in_place_append_tag(
        nonce(counter),
        aead::Aad::from(aad(header, counter, kind, len)),
        &mut *data,
    )
    .map_err(|_| BundleError::InvalidBundle)?;
    out.write_all(&[kind])?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(&data)?;
    Ok(())
}
fn private_file(path: &Path) -> Result<File, BundleError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
fn private_dir(path: &Path) -> Result<(), BundleError> {
    #[allow(unused_mut)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
fn validate_parent(target: &Path) -> Result<(), BundleError> {
    let absolute = if target.is_absolute() {
        target.to_owned()
    } else {
        std::env::current_dir()?.join(target)
    };
    if absolute
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(BundleError::InvalidConfig);
    }
    let parent = absolute.parent().ok_or(BundleError::InvalidConfig)?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(BundleError::InvalidConfig);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(BundleError::InvalidConfig);
            }
        }
    }
    Ok(())
}
fn fresh_temp(target: &Path) -> Result<PathBuf, BundleError> {
    validate_parent(target)?;
    let parent = target.parent().ok_or(BundleError::InvalidConfig)?;
    if fs::symlink_metadata(target).is_ok() {
        return Err(BundleError::TargetExists);
    }
    Ok(parent.join(format!(".media-backup-{}", uuid::Uuid::new_v4())))
}
/// Encrypt an already-consistent database snapshot, never the active database file.
pub async fn create(
    database_archive: &Path,
    config: BTreeMap<String, String>,
    target: &Path,
    key_hex: &str,
    engine: &str,
) -> Result<u64, BundleError> {
    create_with_assets(database_archive, config, target, key_hex, engine, None).await
}
/// Like [`create`], additionally embedding every asset of `assets`. Each object is re-hashed while
/// it is read: a missing, truncated or altered object aborts the backup instead of producing an
/// archive that claims to be complete.
pub async fn create_with_assets(
    database_archive: &Path,
    config: BTreeMap<String, String>,
    target: &Path,
    key_hex: &str,
    engine: &str,
    assets: Option<AssetSource>,
) -> Result<u64, BundleError> {
    let database_archive = database_archive.to_owned();
    let target = target.to_owned();
    let key = decode_key(key_hex)?;
    let engine = engine.to_owned();
    tokio::task::spawn_blocking(move || {
        create_sync(
            &database_archive,
            config,
            &target,
            &key[..],
            &engine,
            assets,
        )
    })
    .await
    .map_err(|_| BundleError::Worker)?
}
fn attachment_kind(name: &str) -> Option<u8> {
    match name {
        "internal-ca.pem" => Some(4),
        "postgres-ca.pem" => Some(5),
        "postgres-cert.pem" => Some(6),
        "postgres-key.pem" => Some(7),
        _ => None,
    }
}
fn create_sync(
    database: &Path,
    mut config: BTreeMap<String, String>,
    target: &Path,
    root: &[u8],
    engine: &str,
    assets: Option<AssetSource>,
) -> Result<u64, BundleError> {
    if !matches!(engine, "sqlite" | "postgres") {
        return Err(BundleError::InvalidConfig);
    }
    config.remove("MEDIA_BACKUP_KEY");
    config.remove("MEDIA_BACKUP_KEY_FILE");
    if fs::metadata(database)?.len() == 0 {
        return Err(BundleError::InvalidBundle);
    }
    if config.iter().any(|(k, v)| k.len() > 256 || v.len() > 16384) {
        return Err(BundleError::InvalidConfig);
    }
    let mut paths = BTreeMap::<String, String>::new();
    if let Some(path) = config
        .get("MEDIA_INTERNAL_CA_FILE")
        .filter(|v| !v.is_empty())
    {
        paths.insert("internal-ca.pem".into(), path.clone());
    }
    if let Some(value) = config.get("MEDIA_DB_URL").filter(|v| !v.is_empty()) {
        let url = url::Url::parse(value).map_err(|_| BundleError::InvalidConfig)?;
        for (key, value) in url.query_pairs() {
            let name = match key.as_ref() {
                "sslrootcert" | "ssl-root-cert" | "ssl-ca" => "postgres-ca.pem",
                "sslcert" | "ssl-cert" => "postgres-cert.pem",
                "sslkey" | "ssl-key" => "postgres-key.pem",
                _ => continue,
            };
            if value.is_empty() || paths.get(name).is_some_and(|old| old != value.as_ref()) {
                return Err(BundleError::InvalidConfig);
            }
            paths.insert(name.into(), value.into_owned());
        }
    }
    let mut files = BTreeMap::<String, (File, u64)>::new();
    for (name, path) in paths {
        let path = Path::new(&path);
        validate_parent(path)?;
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 4 * 1024 * 1024 {
            return Err(BundleError::InvalidConfig);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(BundleError::InvalidConfig);
            }
        }
        files.insert(name, (File::open(path)?, meta.len()));
    }
    let attachments = files
        .iter()
        .map(|(name, (_, bytes))| (name.clone(), *bytes))
        .collect();
    let manifest = Zeroizing::new(
        serde_json::to_vec(&Manifest {
            engine: engine.into(),
            config,
            attachments,
            assets: assets.as_ref().map_or(0, |a| a.items.len() as u64),
            asset_bytes: assets
                .as_ref()
                .map_or(0, |a| a.items.iter().map(|i| i.1).sum()),
        })
        .map_err(|_| BundleError::InvalidConfig)?,
    );
    if manifest.len() > CHUNK {
        return Err(BundleError::InvalidConfig);
    }
    let temp = fresh_temp(target)?;
    let mut guard = Cleanup(temp.clone(), true);
    let mut output = private_file(&temp)?;
    let mut header = [0; 40];
    header[..8].copy_from_slice(MAGIC);
    SystemRandom::new()
        .fill(&mut header[8..])
        .map_err(|_| BundleError::InvalidBundle)?;
    let key = cipher(root, &header[8..])?;
    output.write_all(&header)?;
    record(&mut output, &key, &header, 0, 1, &manifest)?;
    let mut source = File::open(database)?;
    let mut buffer = Zeroizing::new(vec![0; CHUNK]);
    let mut counter = 1u64;
    for (name, (file, expected)) in &mut files {
        let mut copied = 0u64;
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            copied += n as u64;
            if copied > *expected {
                return Err(BundleError::InvalidConfig);
            }
            record(
                &mut output,
                &key,
                &header,
                counter,
                attachment_kind(name).ok_or(BundleError::InvalidConfig)?,
                &buffer[..n],
            )?;
            counter = counter.checked_add(1).ok_or(BundleError::InvalidBundle)?;
        }
        if copied != *expected {
            return Err(BundleError::InvalidConfig);
        }
    }
    if let Some(assets) = assets {
        for (sha256, size, mime) in &assets.items {
            if !crate::assets::is_sha256_hex(sha256) || *size == 0 {
                return Err(BundleError::AssetIntegrity);
            }
            let header_json = serde_json::to_vec(&AssetHeader {
                sha256: sha256.clone(),
                size: *size,
                mime: mime.clone(),
            })
            .map_err(|_| BundleError::InvalidConfig)?;
            record(&mut output, &key, &header, counter, 8, &header_json)?;
            counter = counter.checked_add(1).ok_or(BundleError::InvalidBundle)?;
            let mut file = (assets.open)(sha256).map_err(|_| BundleError::AssetIntegrity)?;
            let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
            let mut copied = 0u64;
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                copied += n as u64;
                if copied > *size {
                    return Err(BundleError::AssetIntegrity);
                }
                sha2::Digest::update(&mut hasher, &buffer[..n]);
                record(&mut output, &key, &header, counter, 9, &buffer[..n])?;
                counter = counter.checked_add(1).ok_or(BundleError::InvalidBundle)?;
            }
            if copied != *size || hex_digest(sha2::Digest::finalize(hasher)) != *sha256 {
                return Err(BundleError::AssetIntegrity);
            }
        }
    }
    let mut database_bytes = 0u64;
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        database_bytes = database_bytes
            .checked_add(n as u64)
            .ok_or(BundleError::InvalidBundle)?;
        record(&mut output, &key, &header, counter, 2, &buffer[..n])?;
        counter = counter.checked_add(1).ok_or(BundleError::InvalidBundle)?;
    }
    if database_bytes == 0 {
        return Err(BundleError::InvalidBundle);
    }
    record(&mut output, &key, &header, counter, 3, &[])?;
    output.sync_all()?;
    let bytes = output.metadata()?.len();
    drop(output);
    fs::hard_link(&temp, target).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            BundleError::TargetExists
        } else {
            e.into()
        }
    })?;
    fs::remove_file(&temp)?;
    sync_directory(target.parent().ok_or(BundleError::InvalidConfig)?)?;
    guard.1 = false;
    Ok(bytes)
}
/// Explicit, loud recovery path for archives written by pre-hardening betas with the publicly known
/// all-zero key. Such archives never had confidentiality; callers must warn the operator.
pub async fn extract_legacy_public_key(
    source: &Path,
    target_dir: &Path,
) -> Result<BundleSummary, BundleError> {
    let source = source.to_owned();
    let target = target_dir.to_owned();
    let key = decode_key_inner(LEGACY_PUBLIC_KEY, true)?;
    tokio::task::spawn_blocking(move || extract_sync(&source, &target, &key[..]))
        .await
        .map_err(|_| BundleError::Worker)?
}
/// Authenticate all records in a private sibling staging directory, then publish atomically.
pub async fn extract(
    source: &Path,
    target_dir: &Path,
    key_hex: &str,
) -> Result<BundleSummary, BundleError> {
    let source = source.to_owned();
    let target = target_dir.to_owned();
    let key = decode_key(key_hex)?;
    tokio::task::spawn_blocking(move || extract_sync(&source, &target, &key[..]))
        .await
        .map_err(|_| BundleError::Worker)?
}
fn extract_sync(source: &Path, target: &Path, root: &[u8]) -> Result<BundleSummary, BundleError> {
    let temp = fresh_temp(target)?;
    private_dir(&temp)?;
    let mut guard = Cleanup(temp.clone(), true);
    let mut input = File::open(source)?;
    let mut header = [0; 40];
    input
        .read_exact(&mut header)
        .map_err(|_| BundleError::InvalidBundle)?;
    if &header[..8] != MAGIC {
        return Err(BundleError::InvalidBundle);
    }
    let key = cipher(root, &header[8..])?;
    let mut counter = 0u64;
    let mut database: Option<File> = None;
    let mut result: Option<BundleSummary> = None;
    let mut attachments = BTreeMap::<u8, (File, u64, u64)>::new();
    let mut database_started = false;
    let mut expected_assets = (0u64, 0u64);
    let mut current_asset: Option<CurrentAsset> = None;
    let mut asset_index: Vec<AssetHeader> = Vec::new();
    let mut asset_total = 0u64;
    loop {
        let mut frame = [0; 5];
        input
            .read_exact(&mut frame)
            .map_err(|_| BundleError::InvalidBundle)?;
        let kind = frame[0];
        let len = u32::from_be_bytes(
            frame[1..]
                .try_into()
                .map_err(|_| BundleError::InvalidBundle)?,
        ) as usize;
        if len > CHUNK
            || !matches!(kind, 1..=9)
            || (counter == 0) != (kind == 1)
            || (matches!(kind, 2 | 4..=9) && len == 0)
            || (kind == 3 && len != 0)
        {
            return Err(BundleError::InvalidBundle);
        }
        let mut data = Zeroizing::new(vec![0; len + aead::AES_256_GCM.tag_len()]);
        input
            .read_exact(&mut data)
            .map_err(|_| BundleError::InvalidBundle)?;
        let plain = key
            .open_in_place(
                nonce(counter),
                aead::Aad::from(aad(&header, counter, kind, len as u32)),
                &mut data,
            )
            .map_err(|_| BundleError::InvalidBundle)?;
        match kind {
            1 => {
                let manifest: Manifest =
                    serde_json::from_slice(plain).map_err(|_| BundleError::InvalidBundle)?;
                let file = match manifest.engine.as_str() {
                    "sqlite" => "database.sqlite3",
                    "postgres" => "database.dump",
                    _ => return Err(BundleError::InvalidBundle),
                };
                if manifest.config.contains_key("MEDIA_BACKUP_KEY")
                    || manifest.config.contains_key("MEDIA_BACKUP_KEY_FILE")
                {
                    return Err(BundleError::InvalidBundle);
                }
                for (name, bytes) in manifest.attachments {
                    let kind = attachment_kind(&name).ok_or(BundleError::InvalidBundle)?;
                    if bytes > 4 * 1024 * 1024 {
                        return Err(BundleError::InvalidBundle);
                    }
                    attachments.insert(kind, (private_file(&temp.join(name))?, bytes, 0));
                }
                let mut config = private_file(&temp.join("config.json"))?;
                serde_json::to_writer(&mut config, &manifest.config)
                    .map_err(|_| BundleError::InvalidBundle)?;
                config.sync_all()?;
                database = Some(private_file(&temp.join(file))?);
                result = Some(BundleSummary {
                    engine: manifest.engine,
                    database_file: file.into(),
                    bytes: 0,
                    assets: 0,
                });
                expected_assets = (manifest.assets, manifest.asset_bytes);
                if manifest.assets > 0 {
                    private_dir(&temp.join("assets"))?;
                }
            }
            8 => {
                if database_started || result.is_none() {
                    return Err(BundleError::InvalidBundle);
                }
                finish_asset(&mut current_asset, &mut asset_index, &mut asset_total)?;
                let h: AssetHeader =
                    serde_json::from_slice(plain).map_err(|_| BundleError::InvalidBundle)?;
                if !crate::assets::is_sha256_hex(&h.sha256)
                    || h.size == 0
                    || h.mime.len() > 100
                    || asset_index.iter().any(|a| a.sha256 == h.sha256)
                {
                    return Err(BundleError::InvalidBundle);
                }
                let file = private_file(&temp.join("assets").join(&h.sha256))?;
                current_asset = Some((h, file, <sha2::Sha256 as sha2::Digest>::new(), 0));
            }
            9 => {
                if database_started {
                    return Err(BundleError::InvalidBundle);
                }
                let (h, file, hasher, copied) =
                    current_asset.as_mut().ok_or(BundleError::InvalidBundle)?;
                *copied += len as u64;
                if *copied > h.size {
                    return Err(BundleError::InvalidBundle);
                }
                sha2::Digest::update(hasher, &*plain);
                file.write_all(plain)?;
            }
            2 => {
                finish_asset(&mut current_asset, &mut asset_index, &mut asset_total)?;
                database_started = true;
                database
                    .as_mut()
                    .ok_or(BundleError::InvalidBundle)?
                    .write_all(plain)?;
                let summary = result.as_mut().ok_or(BundleError::InvalidBundle)?;
                summary.bytes = summary
                    .bytes
                    .checked_add(len as u64)
                    .ok_or(BundleError::InvalidBundle)?;
            }
            4..=7 => {
                if database_started {
                    return Err(BundleError::InvalidBundle);
                }
                let (file, expected, copied) = attachments
                    .get_mut(&kind)
                    .ok_or(BundleError::InvalidBundle)?;
                *copied = copied
                    .checked_add(len as u64)
                    .ok_or(BundleError::InvalidBundle)?;
                if *copied > *expected {
                    return Err(BundleError::InvalidBundle);
                }
                file.write_all(plain)?;
            }
            3 => {
                finish_asset(&mut current_asset, &mut asset_index, &mut asset_total)?;
                if expected_assets != (asset_index.len() as u64, asset_total) {
                    return Err(BundleError::InvalidBundle);
                }
                if attachments
                    .values()
                    .any(|(_, expected, copied)| expected != copied)
                {
                    return Err(BundleError::InvalidBundle);
                }
                if result.as_ref().is_none_or(|summary| summary.bytes == 0) {
                    return Err(BundleError::InvalidBundle);
                }
                let mut trailing = [0; 1];
                if input.read(&mut trailing)? != 0 {
                    return Err(BundleError::InvalidBundle);
                }
                break;
            }
            _ => return Err(BundleError::InvalidBundle),
        }
        counter = counter.checked_add(1).ok_or(BundleError::InvalidBundle)?;
    }
    database
        .as_mut()
        .ok_or(BundleError::InvalidBundle)?
        .sync_all()?;
    drop(database);
    for (_, (file, _, _)) in attachments {
        file.sync_all()?;
    }
    if !asset_index.is_empty() {
        let mut list = private_file(&temp.join("assets.json"))?;
        serde_json::to_writer(&mut list, &asset_index).map_err(|_| BundleError::InvalidBundle)?;
        list.sync_all()?;
        sync_directory(&temp.join("assets"))?;
    }
    if let Some(summary) = result.as_mut() {
        summary.assets = asset_index.len() as u64;
    }
    sync_directory(&temp)?;
    publish_dir(&temp, target)?;
    sync_directory(target.parent().ok_or(BundleError::InvalidConfig)?)?;
    guard.1 = false;
    result.ok_or(BundleError::InvalidBundle)
}
type CurrentAsset = (AssetHeader, File, sha2::Sha256, u64);
/// Close the asset being extracted: size and SHA-256 must both match what the header promised.
fn finish_asset(
    current: &mut Option<CurrentAsset>,
    index: &mut Vec<AssetHeader>,
    total: &mut u64,
) -> Result<(), BundleError> {
    if let Some((header, file, hasher, copied)) = current.take() {
        if copied != header.size || hex_digest(sha2::Digest::finalize(hasher)) != header.sha256 {
            return Err(BundleError::InvalidBundle);
        }
        file.sync_all()?;
        *total = total
            .checked_add(copied)
            .ok_or(BundleError::InvalidBundle)?;
        index.push(header);
    }
    Ok(())
}
fn sync_directory(path: &Path) -> Result<(), BundleError> {
    #[cfg(unix)]
    {
        File::open(if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        })?
        .sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn publish_dir(source: &Path, target: &Path) -> Result<(), BundleError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let source =
        CString::new(source.as_os_str().as_bytes()).map_err(|_| BundleError::InvalidConfig)?;
    let target =
        CString::new(target.as_os_str().as_bytes()).map_err(|_| BundleError::InvalidConfig)?;
    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const std::ffi::c_char,
            newdirfd: i32,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    } // RENAME_NOREPLACE ensures even a concurrently-created empty target survives.
    // SAFETY: NUL-terminated owned strings remain alive for this call; AT_FDCWD resolves both paths.
    let result = unsafe { renameat2(-100, source.as_ptr(), -100, target.as_ptr(), 1) };
    if result == 0 {
        Ok(())
    } else {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            Err(BundleError::TargetExists)
        } else {
            Err(e.into())
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn publish_dir(source: &Path, target: &Path) -> Result<(), BundleError> {
    if fs::symlink_metadata(target).is_ok() {
        return Err(BundleError::TargetExists);
    }
    #[cfg(not(windows))]
    {
        return Err(BundleError::InvalidConfig);
    }
    #[cfg(windows)]
    {
        fs::rename(source, target).map_err(|e| {
            if fs::symlink_metadata(target).is_ok() {
                BundleError::TargetExists
            } else {
                e.into()
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_zero_key_and_malformed_keys_are_refused() {
        assert!(matches!(
            validate_key(LEGACY_PUBLIC_KEY),
            Err(BundleError::InvalidKey)
        ));
        assert!(matches!(validate_key(""), Err(BundleError::MissingKey)));
        assert!(validate_key(&"g".repeat(64)).is_err());
        assert!(validate_key(&"a".repeat(63)).is_err());
        assert!(validate_key(&"ab".repeat(32)).is_ok());
        // An explicitly configured but invalid key is an error, never a silent fallback.
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_key(Some("bad"), None, Some(dir.path()), true).is_err());
        assert!(resolve_key(Some(LEGACY_PUBLIC_KEY), None, Some(dir.path()), true).is_err());
        assert!(!dir.path().join("secrets").exists());
    }

    #[test]
    fn installation_key_is_generated_once_persisted_and_never_in_archive() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            resolve_key(None, None, Some(dir.path()), false),
            Err(BundleError::MissingKey)
        ));
        assert!(matches!(
            resolve_key(None, None, None, true),
            Err(BundleError::MissingKey)
        ));
        let first = resolve_key(None, None, Some(dir.path()), true).unwrap();
        let second = resolve_key(None, None, Some(dir.path()), true).unwrap();
        assert_eq!(*first, *second);
        assert_eq!(first.len(), 64);
        assert_ne!(first.as_str(), LEGACY_PUBLIC_KEY);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("secrets/backup.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
        // A fresh installation gets a different key.
        let other = tempfile::tempdir().unwrap();
        let third = resolve_key(None, None, Some(other.path()), true).unwrap();
        assert_ne!(*first, *third);
        // Key file takes precedence over the installation key and must be well formed.
        let file = dir.path().join("explicit.key");
        fs::write(&file, format!("{}\n", "cd".repeat(32))).unwrap();
        let from_file = resolve_key(None, Some(&file), Some(dir.path()), true).unwrap();
        assert_eq!(from_file.as_str(), "cd".repeat(32));
        fs::write(&file, "short").unwrap();
        assert!(resolve_key(None, Some(&file), Some(dir.path()), true).is_err());
    }

    #[tokio::test]
    async fn legacy_archive_opens_only_through_explicit_legacy_path() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        fs::write(&db, b"sqlite-bytes").unwrap();
        let archive = dir.path().join("legacy.media-backup");
        // Simulate an archive written by an old beta with the public key.
        let key = decode_key_inner(LEGACY_PUBLIC_KEY, true).unwrap();
        create_sync(&db, BTreeMap::new(), &archive, &key[..], "sqlite", None).unwrap();
        let denied = extract(&archive, &dir.path().join("out-a"), LEGACY_PUBLIC_KEY).await;
        assert!(matches!(denied, Err(BundleError::InvalidKey)));
        let good_key = "ab".repeat(32);
        assert!(
            extract(&archive, &dir.path().join("out-b"), &good_key)
                .await
                .is_err()
        );
        let summary = extract_legacy_public_key(&archive, &dir.path().join("out-c"))
            .await
            .unwrap();
        assert_eq!(summary.bytes, 12);
    }
    #[test]
    fn authenticated_empty_database_and_key_file_manifest_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        for (index, config) in [
            BTreeMap::new(),
            BTreeMap::from([(
                "MEDIA_BACKUP_KEY_FILE".to_owned(),
                "secret-location".to_owned(),
            )]),
        ]
        .into_iter()
        .enumerate()
        {
            let archive = directory.path().join(format!("empty-{index}"));
            let mut file = private_file(&archive).unwrap();
            let mut header = [0; 40];
            header[..8].copy_from_slice(MAGIC);
            header[8..].fill(index as u8 + 1);
            let key = cipher(&[7; 32], &header[8..]).unwrap();
            file.write_all(&header).unwrap();
            let manifest = serde_json::to_vec(&Manifest {
                engine: "sqlite".into(),
                config,
                attachments: BTreeMap::new(),
                assets: 0,
                asset_bytes: 0,
            })
            .unwrap();
            record(&mut file, &key, &header, 0, 1, &manifest).unwrap();
            record(&mut file, &key, &header, 1, 3, &[]).unwrap();
            drop(file);
            let output = directory.path().join(format!("output-{index}"));
            assert!(extract_sync(&archive, &output, &[7; 32]).is_err());
            assert!(!output.exists());
        }
    }
}
