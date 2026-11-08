//! Versioned backup bundles. No archive-provided paths.
//!
//! * `MDBK0003` (current): every record is sealed with ChaCha20-Poly1305 under a per-archive key
//!   derived (HKDF-SHA256, random 32-byte salt) from an external 32-byte operator key. The record
//!   counter is the nonce, so reordering, dropping or duplicating records fails authentication, and
//!   the mandatory final record detects truncation.
//! * `MDBK0002` (legacy, read-only by default): plaintext with a SHA-256 corruption checksum, no
//!   authenticity. Still readable so existing archives stay recoverable; only written when the
//!   operator explicitly opts in to [`Protection::Unencrypted`].
//! * `MDBK0001` (legacy encrypted): not readable by this code; see `docs/BACKUP_BUNDLE.md`.
use ring::{
    aead, hkdf,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;
const MAGIC_V2: &[u8; 8] = b"MDBK0002";
const MAGIC_V3: &[u8; 8] = b"MDBK0003";
const MAGIC_LEGACY_ENCRYPTED: &[u8; 8] = b"MDBK0001";
const CHUNK: usize = 65536;
const TAG_LEN: usize = 16;
const SALT_LEN: usize = 32;
const KEY_INFO: &[u8] = b"aether media backup MDBK0003 record key";
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("backup I/O failed")]
    Io(#[from] std::io::Error),
    #[error("backup checksum or format validation failed")]
    InvalidBundle,
    #[error("backup target already exists")]
    TargetExists,
    #[error("backup configuration is invalid or exceeds 64 KiB")]
    InvalidConfig,
    #[error("backup worker failed")]
    Worker,
    #[error("an asset is missing or does not match its recorded SHA-256 or size")]
    AssetIntegrity,
    #[error(
        "this archive is encrypted (MDBK0003): set MEDIA_BACKUP_KEY_FILE (or MEDIA_BACKUP_KEY) to its 64-hex-character key"
    )]
    KeyRequired,
    #[error(
        "invalid backup key: expected 64 hexadecimal characters (32 random bytes, e.g. `openssl rand -hex 32`) that are not obviously weak, with only one of MEDIA_BACKUP_KEY and MEDIA_BACKUP_KEY_FILE set"
    )]
    InvalidKey,
    #[error("backup authentication failed: wrong key, or the archive was modified or corrupted")]
    Authentication,
    #[error(
        "no backup key configured: set MEDIA_BACKUP_KEY_FILE (recommended) or MEDIA_BACKUP_KEY, or explicitly accept unencrypted archives with MEDIA_BACKUP_ALLOW_UNENCRYPTED=true"
    )]
    ProtectionUnavailable,
    #[error(
        "this is a legacy MDBK0001 archive; it is not readable by this version. Keep it untouched and restore it with the Aether release that created it"
    )]
    LegacyEncrypted,
}

/// External 32-byte backup key. Never stored in an archive, never logged (`Debug` is redacted).
#[derive(Clone)]
pub struct BackupKey(Zeroizing<[u8; 32]>);
impl std::fmt::Debug for BackupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BackupKey(<redacted>)")
    }
}
impl BackupKey {
    /// Parse 64 hexadecimal characters. Rejects keys with fewer than 16 distinct byte values, which
    /// no 32-byte random key plausibly has (all-zero, repeated pattern, counting sequences…).
    pub fn parse(text: &str) -> Result<Self, BundleError> {
        let text = text.trim();
        if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(BundleError::InvalidKey);
        }
        let mut bytes = Zeroizing::new([0u8; 32]);
        for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks(2)) {
            let nibble = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            *slot = nibble(pair[0]) << 4 | nibble(pair[1]);
        }
        let mut seen = [false; 256];
        for byte in bytes.iter() {
            seen[*byte as usize] = true;
        }
        if seen.iter().filter(|present| **present).count() < 16 {
            return Err(BundleError::InvalidKey);
        }
        Ok(Self(bytes))
    }

    fn record_key(&self, salt: &[u8]) -> Result<aead::LessSafeKey, BundleError> {
        struct Len;
        impl hkdf::KeyType for Len {
            fn len(&self) -> usize {
                32
            }
        }
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(&self.0[..]);
        let info = [KEY_INFO];
        let okm = prk
            .expand(&info, Len)
            .map_err(|_| BundleError::InvalidKey)?;
        let mut derived = Zeroizing::new([0u8; 32]);
        okm.fill(&mut derived[..])
            .map_err(|_| BundleError::InvalidKey)?;
        let unbound = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, &derived[..])
            .map_err(|_| BundleError::InvalidKey)?;
        Ok(aead::LessSafeKey::new(unbound))
    }
}

/// How a new archive is protected.
#[derive(Clone, Debug)]
pub enum Protection {
    /// `MDBK0003`: encrypted and authenticated.
    Encrypted(BackupKey),
    /// Legacy `MDBK0002`: secrets in clear text. Only reachable through an explicit opt-in.
    Unencrypted,
}

/// Decide how to protect new archives from `MEDIA_BACKUP_KEY_FILE` / `MEDIA_BACKUP_KEY` /
/// `MEDIA_BACKUP_ALLOW_UNENCRYPTED` as returned by `lookup`. Fails closed: with no key and no
/// explicit opt-in, no archive is produced. The key file may be a symlink (Docker/Kubernetes
/// secrets are) but must be a small regular file.
pub fn protection_from_settings(
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Protection, BundleError> {
    let non_empty = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
    match (
        non_empty("MEDIA_BACKUP_KEY"),
        non_empty("MEDIA_BACKUP_KEY_FILE"),
    ) {
        (Some(_), Some(_)) => Err(BundleError::InvalidKey),
        (Some(text), None) => Ok(Protection::Encrypted(BackupKey::parse(&text)?)),
        (None, Some(path)) => Ok(Protection::Encrypted(read_key_file(Path::new(&path))?)),
        (None, None) => {
            let allowed = non_empty("MEDIA_BACKUP_ALLOW_UNENCRYPTED").is_some_and(|v| {
                matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            });
            if allowed {
                Ok(Protection::Unencrypted)
            } else {
                Err(BundleError::ProtectionUnavailable)
            }
        }
    }
}

/// The key needed to read archives (`None` is fine for legacy `MDBK0002`).
pub fn key_from_settings(
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<BackupKey>, BundleError> {
    match protection_from_settings(lookup) {
        Ok(Protection::Encrypted(key)) => Ok(Some(key)),
        Ok(Protection::Unencrypted) | Err(BundleError::ProtectionUnavailable) => Ok(None),
        Err(error) => Err(error),
    }
}

fn read_key_file(path: &Path) -> Result<BackupKey, BundleError> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(BundleError::InvalidKey);
    }
    let mut text = Zeroizing::new(String::new());
    file.take(1024)
        .read_to_string(&mut text)
        .map_err(|_| BundleError::InvalidKey)?;
    BackupKey::parse(&text)
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
/// Per-archive record protection state, shared by the writer and the reader.
enum Sealer {
    /// `MDBK0002`: running SHA-256 over header and frames.
    Plain(Sha256),
    /// `MDBK0003`: AEAD key, authenticated header (`MAGIC || salt`) and next record number.
    Sealed(Box<SealedState>),
}
struct SealedState {
    key: aead::LessSafeKey,
    context: Vec<u8>,
    counter: u64,
}
impl Sealer {
    fn plain() -> Self {
        let mut checksum = Sha256::new();
        checksum.update(MAGIC_V2);
        Self::Plain(checksum)
    }
    fn sealed(key: &BackupKey, salt: &[u8; SALT_LEN]) -> Result<Self, BundleError> {
        let mut context = MAGIC_V3.to_vec();
        context.extend_from_slice(salt);
        Ok(Self::Sealed(Box::new(SealedState {
            key: key.record_key(salt)?,
            context,
            counter: 0,
        })))
    }
    /// Nonce = record counter; the key is unique per archive (random salt), so nonces never repeat.
    fn next_nonce_and_aad(
        context: &[u8],
        counter: &mut u64,
        kind: u8,
        plain_len: usize,
    ) -> Result<(aead::Nonce, Vec<u8>), BundleError> {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        *counter = counter.checked_add(1).ok_or(BundleError::InvalidBundle)?;
        let mut aad = context.to_vec();
        aad.push(kind);
        aad.extend_from_slice(
            &u32::try_from(plain_len)
                .map_err(|_| BundleError::InvalidBundle)?
                .to_be_bytes(),
        );
        Ok((aead::Nonce::assume_unique_for_key(nonce), aad))
    }
    fn seal(&mut self, out: &mut File, kind: u8, plain: &[u8]) -> Result<(), BundleError> {
        if plain.len() > CHUNK {
            return Err(BundleError::InvalidBundle);
        }
        let len = u32::try_from(plain.len()).map_err(|_| BundleError::InvalidBundle)?;
        match self {
            Self::Plain(checksum) => {
                let mut frame = [0u8; 5];
                frame[0] = kind;
                frame[1..].copy_from_slice(&len.to_be_bytes());
                out.write_all(&frame)?;
                out.write_all(plain)?;
                checksum.update(frame);
                checksum.update(plain);
            }
            Self::Sealed(state) => {
                let SealedState {
                    key,
                    context,
                    counter,
                } = &mut **state;
                let (nonce, aad) = Self::next_nonce_and_aad(context, counter, kind, plain.len())?;
                let mut buffer = Zeroizing::new(Vec::with_capacity(plain.len() + TAG_LEN));
                buffer.extend_from_slice(plain);
                key.seal_in_place_append_tag(nonce, aead::Aad::from(aad), &mut *buffer)
                    .map_err(|_| BundleError::InvalidBundle)?;
                let sealed_len =
                    u32::try_from(buffer.len()).map_err(|_| BundleError::InvalidBundle)?;
                let mut frame = [0u8; 5];
                frame[0] = kind;
                frame[1..].copy_from_slice(&sealed_len.to_be_bytes());
                out.write_all(&frame)?;
                out.write_all(&buffer)?;
            }
        }
        Ok(())
    }
    /// Bytes that follow the final record: the SHA-256 for `MDBK0002`, nothing for `MDBK0003`.
    fn trailer(self) -> Option<[u8; 32]> {
        match self {
            Self::Plain(checksum) => Some(checksum.finalize().into()),
            Self::Sealed(_) => None,
        }
    }
    /// Read and (for `MDBK0003`) decrypt and authenticate one record.
    fn open(&mut self, input: &mut File) -> Result<(u8, Zeroizing<Vec<u8>>), BundleError> {
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
        match self {
            Self::Plain(checksum) => {
                if len > CHUNK || !matches!(kind, 1..=9) {
                    return Err(BundleError::InvalidBundle);
                }
                let mut data = Zeroizing::new(vec![0; len]);
                input
                    .read_exact(&mut data)
                    .map_err(|_| BundleError::InvalidBundle)?;
                checksum.update(frame);
                checksum.update(&*data);
                Ok((kind, data))
            }
            Self::Sealed(state) => {
                let SealedState {
                    key,
                    context,
                    counter,
                } = &mut **state;
                if !(TAG_LEN..=CHUNK + TAG_LEN).contains(&len) || !matches!(kind, 1..=9) {
                    return Err(BundleError::InvalidBundle);
                }
                let mut data = Zeroizing::new(vec![0; len]);
                input
                    .read_exact(&mut data)
                    .map_err(|_| BundleError::InvalidBundle)?;
                let (nonce, aad) = Self::next_nonce_and_aad(context, counter, kind, len - TAG_LEN)?;
                let plain_len = key
                    .open_in_place(nonce, aead::Aad::from(aad), &mut data)
                    .map_err(|_| BundleError::Authentication)?
                    .len();
                data.truncate(plain_len);
                Ok((kind, data))
            }
        }
    }
}
fn record(out: &mut File, sealer: &mut Sealer, kind: u8, plain: &[u8]) -> Result<(), BundleError> {
    sealer.seal(out, kind, plain)
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
/// Package an already-consistent database snapshot, never the active database file.
pub async fn create(
    database_archive: &Path,
    config: BTreeMap<String, String>,
    target: &Path,
    engine: &str,
    protection: Protection,
) -> Result<u64, BundleError> {
    create_with_assets(database_archive, config, target, engine, None, protection).await
}
/// Like [`create`], additionally embedding every asset of `assets`. Each object is re-hashed while
/// it is read: a missing, truncated or altered object aborts the backup instead of producing an
/// archive that claims to be complete.
pub async fn create_with_assets(
    database_archive: &Path,
    config: BTreeMap<String, String>,
    target: &Path,
    engine: &str,
    assets: Option<AssetSource>,
    protection: Protection,
) -> Result<u64, BundleError> {
    let database_archive = database_archive.to_owned();
    let target = target.to_owned();
    let engine = engine.to_owned();
    tokio::task::spawn_blocking(move || {
        create_sync(
            &database_archive,
            config,
            &target,
            &engine,
            assets,
            protection,
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
    engine: &str,
    assets: Option<AssetSource>,
    protection: Protection,
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
    let mut checksum = match &protection {
        Protection::Unencrypted => {
            output.write_all(MAGIC_V2)?;
            Sealer::plain()
        }
        Protection::Encrypted(key) => {
            let mut salt = [0u8; SALT_LEN];
            SystemRandom::new()
                .fill(&mut salt)
                .map_err(|_| BundleError::Worker)?;
            output.write_all(MAGIC_V3)?;
            output.write_all(&salt)?;
            Sealer::sealed(key, &salt)?
        }
    };
    record(&mut output, &mut checksum, 1, &manifest)?;
    let mut source = File::open(database)?;
    let mut buffer = Zeroizing::new(vec![0; CHUNK]);
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
                &mut checksum,
                attachment_kind(name).ok_or(BundleError::InvalidConfig)?,
                &buffer[..n],
            )?;
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
            record(&mut output, &mut checksum, 8, &header_json)?;
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
                record(&mut output, &mut checksum, 9, &buffer[..n])?;
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
        record(&mut output, &mut checksum, 2, &buffer[..n])?;
    }
    if database_bytes == 0 {
        return Err(BundleError::InvalidBundle);
    }
    record(&mut output, &mut checksum, 3, &[])?;
    if let Some(digest) = checksum.trailer() {
        output.write_all(&digest)?;
    }
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
/// Authenticate (`MDBK0003`, needs `key`) or checksum (legacy `MDBK0002`) the archive and validate
/// its record structure in a private staging directory, then publish that directory without
/// replacing anything. The source archive is only read, never modified or removed.
pub async fn extract(
    source: &Path,
    target_dir: &Path,
    key: Option<&BackupKey>,
) -> Result<BundleSummary, BundleError> {
    let source = source.to_owned();
    let target = target_dir.to_owned();
    let key = key.cloned();
    tokio::task::spawn_blocking(move || extract_sync(&source, &target, key.as_ref()))
        .await
        .map_err(|_| BundleError::Worker)?
}
fn extract_sync(
    source: &Path,
    target: &Path,
    key: Option<&BackupKey>,
) -> Result<BundleSummary, BundleError> {
    // Identify the format before creating anything, so a missing key or an unsupported
    // version leaves no staging directory behind.
    let mut input = File::open(source)?;
    let mut header = [0; 8];
    input
        .read_exact(&mut header)
        .map_err(|_| BundleError::InvalidBundle)?;
    let mut checksum = if &header == MAGIC_V2 {
        Sealer::plain()
    } else if &header == MAGIC_V3 {
        let key = key.ok_or(BundleError::KeyRequired)?;
        let mut salt = [0u8; SALT_LEN];
        input
            .read_exact(&mut salt)
            .map_err(|_| BundleError::InvalidBundle)?;
        Sealer::sealed(key, &salt)?
    } else if &header == MAGIC_LEGACY_ENCRYPTED {
        return Err(BundleError::LegacyEncrypted);
    } else {
        return Err(BundleError::InvalidBundle);
    };
    let temp = fresh_temp(target)?;
    private_dir(&temp)?;
    let mut guard = Cleanup(temp.clone(), true);
    let mut first = true;
    let mut database: Option<File> = None;
    let mut result: Option<BundleSummary> = None;
    let mut attachments = BTreeMap::<u8, (File, u64, u64)>::new();
    let mut database_started = false;
    let mut expected_assets = (0u64, 0u64);
    let mut current_asset: Option<CurrentAsset> = None;
    let mut asset_index: Vec<AssetHeader> = Vec::new();
    let mut asset_total = 0u64;
    loop {
        let (kind, data) = checksum.open(&mut input)?;
        let len = data.len();
        if first != (kind == 1)
            || (matches!(kind, 2 | 4..=9) && len == 0)
            || (kind == 3 && len != 0)
        {
            return Err(BundleError::InvalidBundle);
        }
        let plain = &data[..];
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
                sha2::Digest::update(hasher, plain);
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
                if let Some(actual) = checksum.trailer() {
                    let mut expected = [0; 32];
                    input
                        .read_exact(&mut expected)
                        .map_err(|_| BundleError::InvalidBundle)?;
                    if actual != expected {
                        return Err(BundleError::InvalidBundle);
                    }
                }
                let mut trailing = [0; 1];
                if input.read(&mut trailing)? != 0 {
                    return Err(BundleError::InvalidBundle);
                }
                break;
            }
            _ => return Err(BundleError::InvalidBundle),
        }
        first = false;
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

    const TEST_KEY: &str = "3f9a1c7e5b2d4086a1f3c5e7092b4d6f8a0c2e4619375b5d7f1a3c5e7b9d0f12";
    const OTHER_KEY: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90112233445566778899aabbccddeeff01";

    fn key(text: &str) -> BackupKey {
        BackupKey::parse(text).unwrap()
    }

    #[tokio::test]
    async fn unencrypted_opt_in_round_trip_and_rejects_legacy_encrypted_magic() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        fs::write(&db, b"sqlite-bytes").unwrap();
        let archive = dir.path().join("new.media-backup");
        create(
            &db,
            BTreeMap::new(),
            &archive,
            "sqlite",
            Protection::Unencrypted,
        )
        .await
        .unwrap();
        assert_eq!(&fs::read(&archive).unwrap()[..8], MAGIC_V2);
        let output = dir.path().join("output");
        let summary = extract(&archive, &output, None).await.unwrap();
        assert_eq!(summary.bytes, 12);
        assert_eq!(
            fs::read(output.join("database.sqlite3")).unwrap(),
            b"sqlite-bytes"
        );
        let mut old = fs::read(&archive).unwrap();
        old[..8].copy_from_slice(b"MDBK0001");
        let old_path = dir.path().join("old.media-backup");
        fs::write(&old_path, old).unwrap();
        let old_output = dir.path().join("old-output");
        assert!(matches!(
            extract(&old_path, &old_output, None).await,
            Err(BundleError::LegacyEncrypted)
        ));
        assert!(!old_output.exists());
    }

    #[tokio::test]
    async fn encrypted_bundle_hides_content_and_needs_the_exact_key() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        let payload: Vec<u8> = (0..200_000).map(|i| (i % 253) as u8).collect();
        fs::write(&db, &payload).unwrap();
        let archive = dir.path().join("enc.media-backup");
        let config = BTreeMap::from([("MEDIA_SECRET".into(), "synthetic-secret-marker".into())]);
        create(
            &db,
            config,
            &archive,
            "postgres",
            Protection::Encrypted(key(TEST_KEY)),
        )
        .await
        .unwrap();
        let bytes = fs::read(&archive).unwrap();
        assert_eq!(&bytes[..8], MAGIC_V3);
        assert!(
            !bytes
                .windows(b"synthetic-secret-marker".len())
                .any(|w| w == b"synthetic-secret-marker")
        );
        assert!(!bytes.windows(64).any(|w| w == &payload[1000..1064]));
        let no_key = dir.path().join("no-key");
        assert!(matches!(
            extract(&archive, &no_key, None).await,
            Err(BundleError::KeyRequired)
        ));
        let wrong = dir.path().join("wrong");
        assert!(matches!(
            extract(&archive, &wrong, Some(&key(OTHER_KEY))).await,
            Err(BundleError::Authentication)
        ));
        assert!(!no_key.exists() && !wrong.exists());
        let output = dir.path().join("ok");
        let summary = extract(&archive, &output, Some(&key(TEST_KEY)))
            .await
            .unwrap();
        assert_eq!(summary.engine, "postgres");
        assert_eq!(fs::read(output.join("database.dump")).unwrap(), payload);
        let restored: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("config.json")).unwrap()).unwrap();
        assert_eq!(restored["MEDIA_SECRET"], "synthetic-secret-marker");
    }

    #[tokio::test]
    async fn encrypted_bundle_rejects_every_single_byte_modification_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        fs::write(&db, vec![7u8; 150_000]).unwrap();
        let archive = dir.path().join("enc");
        create(
            &db,
            BTreeMap::new(),
            &archive,
            "sqlite",
            Protection::Encrypted(key(TEST_KEY)),
        )
        .await
        .unwrap();
        let good = fs::read(&archive).unwrap();
        let mut variants: Vec<Vec<u8>> = vec![good[..7].to_vec(), good[..good.len() - 1].to_vec()];
        // Whole-record truncation: drop the final (kind 3) record, which is 5 + 16 bytes.
        variants.push(good[..good.len() - 21].to_vec());
        let mut trailing = good.clone();
        trailing.push(0);
        variants.push(trailing);
        // Flip one byte in the salt, each frame header region, the middle and the last tag.
        for index in [
            8,
            40,
            45,
            60,
            good.len() / 2,
            good.len() - 22,
            good.len() - 1,
        ] {
            let mut changed = good.clone();
            changed[index] ^= 1;
            variants.push(changed);
        }
        // Scramble eight ciphertext bytes of the first record after the manifest.
        let mut swapped = good.clone();
        let first = 8 + SALT_LEN;
        let manifest_len = u32::from_be_bytes(swapped[first + 1..first + 5].try_into().unwrap());
        let second = first + 5 + manifest_len as usize;
        swapped[second + 5..second + 5 + 8].reverse();
        variants.push(swapped);
        for (index, bytes) in variants.into_iter().enumerate() {
            let damaged = dir.path().join(format!("damaged-{index}"));
            fs::write(&damaged, bytes).unwrap();
            let output = dir.path().join(format!("output-{index}"));
            assert!(
                extract(&damaged, &output, Some(&key(TEST_KEY)))
                    .await
                    .is_err(),
                "variant {index} was accepted"
            );
            assert!(!output.exists());
            assert_eq!(
                fs::read_dir(dir.path())
                    .unwrap()
                    .filter(|e| e
                        .as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".media-backup-"))
                    .count(),
                0,
                "staging leftovers after variant {index}"
            );
        }
        // The pristine archive remains readable and byte-identical afterwards.
        assert_eq!(fs::read(&archive).unwrap(), good);
        extract(&archive, &dir.path().join("final"), Some(&key(TEST_KEY)))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn two_archives_with_the_same_key_and_content_differ() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        fs::write(&db, b"same bytes").unwrap();
        let mut seen = Vec::new();
        for name in ["a", "b"] {
            let archive = dir.path().join(name);
            create(
                &db,
                BTreeMap::new(),
                &archive,
                "sqlite",
                Protection::Encrypted(key(TEST_KEY)),
            )
            .await
            .unwrap();
            seen.push(fs::read(archive).unwrap());
        }
        assert_ne!(seen[0][8..40], seen[1][8..40]);
        assert_ne!(seen[0], seen[1]);
    }

    #[test]
    fn key_parsing_rejects_weak_malformed_and_ambiguous_settings() {
        let bad_keys: Vec<String> = vec![
            String::new(),
            "short".into(),
            "0".repeat(64),
            "11".repeat(32),
            "ab".repeat(32),
            "0123456789abcdef".repeat(4),
            format!("{}z", "a".repeat(63)),
            "a".repeat(65),
        ];
        for bad in &bad_keys {
            assert!(
                matches!(BackupKey::parse(bad), Err(BundleError::InvalidKey)),
                "{bad}"
            );
        }
        assert!(BackupKey::parse(&format!("  {TEST_KEY}\n")).is_ok());
        assert_eq!(format!("{:?}", key(TEST_KEY)), "BackupKey(<redacted>)");
        let map = |entries: &[(&'static str, String)]| {
            let entries: BTreeMap<String, String> = entries
                .iter()
                .map(|(k, v)| ((*k).into(), v.clone()))
                .collect();
            move |name: &str| entries.get(name).cloned()
        };
        // Fail closed without a key; explicit opt-in is required for plaintext.
        assert!(matches!(
            protection_from_settings(&map(&[])),
            Err(BundleError::ProtectionUnavailable)
        ));
        assert!(matches!(
            protection_from_settings(&map(&[("MEDIA_BACKUP_ALLOW_UNENCRYPTED", "true".into())])),
            Ok(Protection::Unencrypted)
        ));
        assert!(matches!(
            protection_from_settings(&map(&[("MEDIA_BACKUP_ALLOW_UNENCRYPTED", "false".into())])),
            Err(BundleError::ProtectionUnavailable)
        ));
        // A configured key always wins over the opt-in.
        assert!(matches!(
            protection_from_settings(&map(&[
                ("MEDIA_BACKUP_KEY", TEST_KEY.into()),
                ("MEDIA_BACKUP_ALLOW_UNENCRYPTED", "true".into())
            ])),
            Ok(Protection::Encrypted(_))
        ));
        // Both sources at once, or a weak key, never silently downgrade.
        assert!(matches!(
            protection_from_settings(&map(&[
                ("MEDIA_BACKUP_KEY", TEST_KEY.into()),
                ("MEDIA_BACKUP_KEY_FILE", "/x".into())
            ])),
            Err(BundleError::InvalidKey)
        ));
        assert!(matches!(
            protection_from_settings(&map(&[
                ("MEDIA_BACKUP_KEY", "11".repeat(32)),
                ("MEDIA_BACKUP_ALLOW_UNENCRYPTED", "true".into())
            ])),
            Err(BundleError::InvalidKey)
        ));
    }

    #[test]
    fn key_file_is_read_trimmed_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("backup.key");
        fs::write(&file, format!("{TEST_KEY}\n")).unwrap();
        let path = file.to_string_lossy().into_owned();
        let lookup = |name: &str| (name == "MEDIA_BACKUP_KEY_FILE").then(|| path.clone());
        assert!(matches!(
            protection_from_settings(&lookup),
            Ok(Protection::Encrypted(_))
        ));
        fs::write(&file, vec![b'a'; 4096]).unwrap();
        assert!(matches!(
            protection_from_settings(&lookup),
            Err(BundleError::InvalidKey)
        ));
        assert!(
            protection_from_settings(&|name: &str| (name == "MEDIA_BACKUP_KEY_FILE")
                .then(|| dir.path().to_string_lossy().into_owned()))
            .is_err()
        );
    }
}
