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
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("backup I/O failed")]
    Io(#[from] std::io::Error),
    #[error("backup key must be exactly 64 hexadecimal characters")]
    InvalidKey,
    #[error("backup authentication or format validation failed")]
    InvalidBundle,
    #[error("backup target already exists")]
    TargetExists,
    #[error("backup configuration is invalid or exceeds 64 KiB")]
    InvalidConfig,
    #[error("backup worker failed")]
    Worker,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    engine: String,
    config: BTreeMap<String, String>,
    #[serde(default)]
    attachments: BTreeMap<String, u64>,
}
#[derive(Debug, Serialize)]
pub struct BundleSummary {
    pub engine: String,
    pub database_file: String,
    pub bytes: u64,
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
fn decode_key(value: &str) -> Result<Zeroizing<[u8; 32]>, BundleError> {
    if value.len() != 64 {
        return Err(BundleError::InvalidKey);
    }
    let mut key = Zeroizing::new([0; 32]);
    for (i, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair).map_err(|_| BundleError::InvalidKey)?;
        key[i] = u8::from_str_radix(text, 16).map_err(|_| BundleError::InvalidKey)?;
    }
    Ok(key)
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
    let database_archive = database_archive.to_owned();
    let target = target.to_owned();
    let key = decode_key(key_hex)?;
    let engine = engine.to_owned();
    tokio::task::spawn_blocking(move || {
        create_sync(&database_archive, config, &target, &key[..], &engine)
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
            || !matches!(kind, 1..=7)
            || (counter == 0) != (kind == 1)
            || (matches!(kind, 2 | 4..=7) && len == 0)
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
                });
            }
            2 => {
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
    sync_directory(&temp)?;
    publish_dir(&temp, target)?;
    sync_directory(target.parent().ok_or(BundleError::InvalidConfig)?)?;
    guard.1 = false;
    result.ok_or(BundleError::InvalidBundle)
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
