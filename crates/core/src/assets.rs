//! Universal store for durable application binaries (avatars, posters, thumbnails, derived data).
//!
//! * Objects are immutable and identified by the SHA-256 of their real bytes: identical content is
//!   stored once whatever the number of users or works referencing it.
//! * SQL keeps identity, metadata and references (typed foreign keys on owner tables, listed in
//!   [`REFERENCING_COLUMNS`]); the bytes live on the local disk (`MEDIA_ASSETS_PATH`, default
//!   `<data>/assets`) behind the small [`BlobBackend`] interface, so a shared backend (e.g. S3) can be
//!   added later without touching callers. No other backend is implemented on purpose.
//! * Writes stream into `tmp/`, are hashed and fsynced, then published atomically. A crash between
//!   the file and the SQL row leaves an unreferenced file that the sweeper removes after a grace
//!   period; a row is never inserted before its file exists.
//! * Deletion is two-phase and deferred: unreferenced assets are first marked (`orphaned_at`), and
//!   removed only after a grace period if still unreferenced and not leased by a running
//!   processing step. The database foreign keys additionally refuse to delete a referenced row.
//! * The store never exposes a disk path. Callers authorize the *owner* of a reference first and
//!   only then call [`AssetStore::response_after_authorization`].
//!
//! Single-Core assumption: the in-process gate and leases serialize writers and the collector
//! inside one Core. Several Cores would need a shared backend and a database-level collector lock.
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use bytes::Bytes;
use sha2::{Digest, Sha256};
use sqlx::AnyPool;
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, RwLock},
};

/// Owner columns holding a foreign key to `assets(sha256)`. Every such column must be listed here
/// (a test compares this list with the real schema): the collector only trusts this registry.
pub const REFERENCING_COLUMNS: &[(&str, &str)] = &[
    ("extension_avatar_images", "sha256"),
    ("library_artwork", "sha256"),
    ("poster_previews", "sha256"),
    ("user_avatars", "sha256"),
    ("work_image_data", "sha256"),
    ("work_posters", "sha256"),
];

const CHUNK: usize = 64 * 1024;
const MAX_GC_BATCH: i64 = 500;
const MAX_SWEEP: usize = 2_000;

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset storage I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("asset database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("asset is empty")]
    Empty,
    #[error("asset exceeds the maximum object size")]
    TooLarge,
    #[error("asset storage quota exceeded")]
    QuotaExceeded,
    #[error("invalid asset digest")]
    InvalidDigest,
    #[error("invalid media type")]
    InvalidMime,
    #[error("asset not found")]
    NotFound,
    #[error("asset content does not match its digest or size")]
    Corrupt,
    #[error("invalid asset storage configuration: {0}")]
    Configuration(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct AssetMeta {
    pub sha256: String,
    pub size: i64,
    pub mime: String,
}

#[derive(Debug, Clone)]
pub struct AssetConfig {
    /// Largest accepted object, in bytes.
    pub max_object_bytes: u64,
    /// Total bytes of all committed assets; `None` means unlimited.
    pub quota_bytes: Option<u64>,
    /// Time an unreferenced asset survives before deletion.
    pub orphan_grace: Duration,
    /// Age after which a stray temporary/unreferenced file is swept.
    pub file_grace: Duration,
}

impl Default for AssetConfig {
    fn default() -> Self {
        Self {
            max_object_bytes: 32 * 1024 * 1024,
            quota_bytes: None,
            orphan_grace: Duration::from_secs(24 * 3600),
            file_grace: Duration::from_secs(3600),
        }
    }
}

impl AssetConfig {
    /// `MEDIA_ASSETS_MAX_OBJECT_BYTES`, `MEDIA_ASSETS_QUOTA_BYTES`, `MEDIA_ASSETS_ORPHAN_GRACE_SECS`.
    /// A set but unparsable or zero value is refused instead of silently ignored.
    pub fn from_env() -> Result<Self, AssetError> {
        fn number(name: &'static str) -> Result<Option<u64>, AssetError> {
            match std::env::var(name) {
                Err(_) => Ok(None),
                Ok(value) if value.is_empty() => Ok(None),
                Ok(value) => match value.parse::<u64>() {
                    Ok(n) if n > 0 => Ok(Some(n)),
                    _ => Err(AssetError::Configuration(name)),
                },
            }
        }
        let mut config = Self::default();
        if let Some(n) = number("MEDIA_ASSETS_MAX_OBJECT_BYTES")? {
            config.max_object_bytes = n;
        }
        config.quota_bytes = number("MEDIA_ASSETS_QUOTA_BYTES")?;
        if let Some(n) = number("MEDIA_ASSETS_ORPHAN_GRACE_SECS")? {
            config.orphan_grace = Duration::from_secs(n);
        }
        Ok(config)
    }
}

/// An object already published in a backend.
#[derive(Debug, Clone)]
pub struct BlobEntry {
    pub sha256: String,
    pub modified: SystemTime,
}

/// Minimal blocking object-store interface. Only [`LocalFs`] exists today.
pub trait BlobBackend: Send + Sync + 'static {
    /// Root directory when objects live on the local disk (used for media-root overlap checks).
    fn local_root(&self) -> Option<PathBuf> {
        None
    }
    /// Directory on the *same* file system where uploads are staged before publication.
    fn temp_dir(&self) -> PathBuf;
    /// Atomically publish a finished temporary file under its digest; idempotent.
    fn publish(&self, sha256: &str, temp: &Path) -> io::Result<()>;
    fn open(&self, sha256: &str) -> io::Result<std::fs::File>;
    fn exists(&self, sha256: &str) -> io::Result<bool>;
    fn remove(&self, sha256: &str) -> io::Result<bool>;
    /// At most `max` published objects (unordered), for orphan-file sweeping.
    fn scan(&self, max: usize) -> io::Result<Vec<BlobEntry>>;
    /// Stale staging files older than `age` removed; returns how many.
    fn sweep_temp(&self, age: Duration) -> io::Result<usize>;
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Content-addressed layout `objects/ab/cd/<sha256>` below a private root.
pub struct LocalFs {
    root: PathBuf,
}

impl LocalFs {
    /// Create the private layout. The root must not overlap `forbidden` (the media root).
    pub fn open(root: &Path, forbidden: Option<&Path>) -> Result<Self, AssetError> {
        let root = std::path::absolute(root)?;
        if let Some(media) = forbidden
            && let Ok(media) = media.canonicalize()
        {
            // Compare after creating the root so symlinks are resolved on both sides.
            create_private_dir(&root)?;
            let canonical = root.canonicalize()?;
            if canonical.starts_with(&media) || media.starts_with(&canonical) {
                return Err(AssetError::Configuration(
                    "assets directory overlaps the media root",
                ));
            }
        }
        create_private_dir(&root)?;
        create_private_dir(&root.join("objects"))?;
        create_private_dir(&root.join("tmp"))?;
        Ok(Self { root })
    }
    fn path_for(&self, sha256: &str) -> io::Result<PathBuf> {
        if !is_sha256_hex(sha256) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid digest",
            ));
        }
        Ok(self
            .root
            .join("objects")
            .join(&sha256[..2])
            .join(&sha256[2..4])
            .join(sha256))
    }
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(path) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

impl BlobBackend for LocalFs {
    fn local_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
    fn temp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
    fn publish(&self, sha256: &str, temp: &Path) -> io::Result<()> {
        let target = self.path_for(sha256)?;
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("invalid object path"))?;
        create_private_dir(parent)?;
        if std::fs::symlink_metadata(&target).is_ok() {
            // Identical bytes already published; the staged copy is redundant.
            return std::fs::remove_file(temp);
        }
        std::fs::rename(temp, &target)?;
        sync_dir(parent);
        Ok(())
    }
    fn open(&self, sha256: &str) -> io::Result<std::fs::File> {
        let path = self.path_for(sha256)?;
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a regular file",
            ));
        }
        std::fs::File::open(path)
    }
    fn exists(&self, sha256: &str) -> io::Result<bool> {
        match std::fs::symlink_metadata(self.path_for(sha256)?) {
            Ok(meta) => Ok(meta.is_file()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn remove(&self, sha256: &str) -> io::Result<bool> {
        match std::fs::remove_file(self.path_for(sha256)?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn scan(&self, max: usize) -> io::Result<Vec<BlobEntry>> {
        let mut found = Vec::new();
        for first in std::fs::read_dir(self.root.join("objects"))? {
            let first = first?;
            if !first.file_type()?.is_dir() {
                continue;
            }
            for second in std::fs::read_dir(first.path())? {
                let second = second?;
                if !second.file_type()?.is_dir() {
                    continue;
                }
                for file in std::fs::read_dir(second.path())? {
                    let file = file?;
                    let name = file.file_name().to_string_lossy().into_owned();
                    let meta = file.metadata()?;
                    if meta.is_file() && is_sha256_hex(&name) {
                        found.push(BlobEntry {
                            sha256: name,
                            modified: meta.modified()?,
                        });
                        if found.len() >= max {
                            return Ok(found);
                        }
                    }
                }
            }
        }
        Ok(found)
    }
    fn sweep_temp(&self, age: Duration) -> io::Result<usize> {
        let mut removed = 0;
        for entry in std::fs::read_dir(self.temp_dir())? {
            let entry = entry?;
            let meta = entry.metadata()?;
            let stale = meta
                .modified()
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|elapsed| elapsed >= age);
            if meta.is_file() && stale {
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Failure of one collector step; the collector continues with the remaining items.
#[derive(Debug, Clone)]
pub struct GcFailure {
    pub kind: &'static str,
    pub scope: String,
}

#[derive(Debug, Default)]
pub struct GcReport {
    pub marked: u64,
    pub cleared: u64,
    pub deleted_assets: u64,
    pub deleted_bytes: u64,
    pub skipped_leased: u64,
    pub swept_files: u64,
    pub swept_temp: u64,
    pub failures: Vec<GcFailure>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub checked: u64,
    pub missing: Vec<String>,
    pub corrupt: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct RestoreReport {
    pub imported: u64,
    pub already_present: u64,
    pub audit: VerifyReport,
}

impl RestoreReport {
    pub fn is_complete(&self) -> bool {
        self.audit.missing.is_empty() && self.audit.corrupt.is_empty()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AssetStats {
    pub objects: i64,
    pub bytes: i64,
    pub orphaned: i64,
}

/// Prevents the collector from deleting an asset while a processing step or response uses it.
pub struct Lease {
    store: Arc<LeaseTable>,
    sha256: String,
}

type LeaseTable = StdMutex<HashMap<String, usize>>;

impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut table) = self.store.lock()
            && let Some(count) = table.get_mut(&self.sha256)
        {
            *count -= 1;
            if *count == 0 {
                table.remove(&self.sha256);
            }
        }
    }
}

pub struct AssetStore {
    pool: AnyPool,
    backend: Arc<dyn BlobBackend>,
    config: AssetConfig,
    refs: &'static [(&'static str, &'static str)],
    /// Writers hold it shared; the collector holds it exclusively.
    gate: RwLock<()>,
    /// Serializes quota check + publish + row insertion (short critical section).
    commit: Mutex<()>,
    leases: Arc<LeaseTable>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn valid_mime(mime: &str) -> bool {
    let Some((kind, subtype)) = mime.split_once('/') else {
        return false;
    };
    mime.len() <= 100
        && !kind.is_empty()
        && !subtype.is_empty()
        && mime
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"/.+-".contains(&b))
}

impl AssetStore {
    pub fn new(pool: AnyPool, backend: Arc<dyn BlobBackend>, config: AssetConfig) -> Self {
        Self::with_refs(pool, backend, config, REFERENCING_COLUMNS)
    }

    /// Same as [`Self::new`] with an explicit reference registry (used by tests).
    pub fn with_refs(
        pool: AnyPool,
        backend: Arc<dyn BlobBackend>,
        config: AssetConfig,
        refs: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            pool,
            backend,
            config,
            refs,
            gate: RwLock::new(()),
            commit: Mutex::new(()),
            leases: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Refuse a local asset directory that is, contains or lies inside the media root.
    pub fn ensure_outside(&self, media_root: &Path) -> Result<(), AssetError> {
        let (Some(root), Ok(media)) = (self.backend.local_root(), media_root.canonicalize()) else {
            return Ok(());
        };
        let root = root.canonicalize()?;
        if root.starts_with(&media) || media.starts_with(&root) {
            return Err(AssetError::Configuration(
                "assets directory overlaps the media root",
            ));
        }
        Ok(())
    }

    pub fn config(&self) -> &AssetConfig {
        &self.config
    }

    /// SQL predicate (over table `assets`) true when no owner references the row.
    fn unreferenced(&self) -> String {
        if self.refs.is_empty() {
            return "(1=1)".into();
        }
        let parts: Vec<String> = self
            .refs
            .iter()
            .map(|(table, column)| {
                format!("NOT EXISTS (SELECT 1 FROM {table} WHERE {column} = assets.sha256)")
            })
            .collect();
        format!("({})", parts.join(" AND "))
    }

    pub async fn put_bytes(&self, bytes: &[u8], mime: &str) -> Result<AssetMeta, AssetError> {
        self.put_stream(bytes, mime).await
    }

    /// Stream `reader` into the store with bounded memory (one 64 KiB buffer).
    pub async fn put_stream(
        &self,
        mut reader: impl AsyncRead + Unpin,
        mime: &str,
    ) -> Result<AssetMeta, AssetError> {
        if !valid_mime(mime) {
            return Err(AssetError::InvalidMime);
        }
        let _shared = self.gate.read().await;
        let temp = self
            .backend
            .temp_dir()
            .join(format!("{}.part", uuid::Uuid::new_v4().simple()));
        let result = self.put_inner(&mut reader, mime, &temp).await;
        // Whatever happened, a staging file never outlives the call (the sweeper covers crashes).
        let _ = tokio::fs::remove_file(&temp).await;
        result
    }

    async fn put_inner(
        &self,
        reader: &mut (impl AsyncRead + Unpin),
        mime: &str,
        temp: &Path,
    ) -> Result<AssetMeta, AssetError> {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temp)
            .await?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let n = reader.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            size += n as u64;
            if size > self.config.max_object_bytes {
                return Err(AssetError::TooLarge);
            }
            hasher.update(&buffer[..n]);
            file.write_all(&buffer[..n]).await?;
        }
        if size == 0 {
            return Err(AssetError::Empty);
        }
        file.sync_all().await?;
        drop(file);
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        let _commit = self.commit.lock().await;
        let existing: Option<AssetMeta> =
            crate::database::query_as("SELECT sha256, size, mime FROM assets WHERE sha256=?")
                .bind(&digest)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(meta) = existing {
            // Heal a lost file from the freshly verified bytes, otherwise just deduplicate.
            if !self.blocking_exists(&digest).await? {
                self.blocking_publish(&digest, temp).await?;
            }
            // A re-upload rescues the asset from a pending deletion mark.
            crate::database::query("UPDATE assets SET orphaned_at=NULL WHERE sha256=?")
                .bind(&digest)
                .execute(&self.pool)
                .await?;
            return Ok(meta);
        }
        if let Some(quota) = self.config.quota_bytes {
            let used: i64 =
                crate::database::query_scalar("SELECT COALESCE(SUM(size),0) FROM assets")
                    .fetch_one(&self.pool)
                    .await?;
            if (used as u64).saturating_add(size) > quota {
                return Err(AssetError::QuotaExceeded);
            }
        }
        // File first, row second: a crash in between leaves only a sweepable stray file.
        self.blocking_publish(&digest, temp).await?;
        crate::database::query(
            "INSERT INTO assets(sha256,size,mime,created_at) VALUES (?,?,?,?) ON CONFLICT(sha256) DO NOTHING",
        )
        .bind(&digest)
        .bind(size as i64)
        .bind(mime)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(AssetMeta {
            sha256: digest,
            size: size as i64,
            mime: mime.to_owned(),
        })
    }

    async fn blocking_publish(&self, sha: &str, temp: &Path) -> Result<(), AssetError> {
        let (backend, sha, temp) = (self.backend.clone(), sha.to_owned(), temp.to_owned());
        tokio::task::spawn_blocking(move || backend.publish(&sha, &temp))
            .await
            .map_err(|_| io::Error::other("storage worker failed"))??;
        Ok(())
    }

    async fn blocking_exists(&self, sha: &str) -> Result<bool, AssetError> {
        let (backend, sha) = (self.backend.clone(), sha.to_owned());
        Ok(tokio::task::spawn_blocking(move || backend.exists(&sha))
            .await
            .map_err(|_| io::Error::other("storage worker failed"))??)
    }

    pub async fn meta(&self, sha256: &str) -> Result<AssetMeta, AssetError> {
        if !is_sha256_hex(sha256) {
            return Err(AssetError::InvalidDigest);
        }
        crate::database::query_as("SELECT sha256, size, mime FROM assets WHERE sha256=?")
            .bind(sha256)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(AssetError::NotFound)
    }

    /// Hold an asset against collection for as long as the returned guard lives.
    pub fn lease(&self, sha256: &str) -> Result<Lease, AssetError> {
        if !is_sha256_hex(sha256) {
            return Err(AssetError::InvalidDigest);
        }
        let mut table = self
            .leases
            .lock()
            .map_err(|_| io::Error::other("lease table poisoned"))?;
        *table.entry(sha256.to_owned()).or_insert(0) += 1;
        Ok(Lease {
            store: self.leases.clone(),
            sha256: sha256.to_owned(),
        })
    }

    fn is_leased(&self, sha256: &str) -> bool {
        self.leases
            .lock()
            .map(|t| t.contains_key(sha256))
            .unwrap_or(true)
    }

    /// Open for streaming. Verifies the stored length (cheap) but not the digest (see `verify`).
    pub async fn open(&self, sha256: &str) -> Result<(AssetMeta, tokio::fs::File), AssetError> {
        let meta = self.meta(sha256).await?;
        let (backend, sha) = (self.backend.clone(), meta.sha256.clone());
        let file = tokio::task::spawn_blocking(move || backend.open(&sha))
            .await
            .map_err(|_| io::Error::other("storage worker failed"))?;
        let file = match file {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(AssetError::Corrupt),
            Err(e) => return Err(e.into()),
        };
        if file.metadata()?.len() != meta.size as u64 {
            return Err(AssetError::Corrupt);
        }
        Ok((meta, tokio::fs::File::from_std(file)))
    }

    /// Whole-object read for small assets only (bounded by `max_object_bytes`).
    pub async fn read_all(&self, sha256: &str) -> Result<(AssetMeta, Vec<u8>), AssetError> {
        let (meta, mut file) = self.open(sha256).await?;
        let mut out = Vec::with_capacity(meta.size as usize);
        file.read_to_end(&mut out).await?;
        Ok((meta, out))
    }

    /// Serve an asset **after the caller verified the user may read the owner of a reference to
    /// it**. Strong `ETag` = digest (immutable object), private revalidation-free cache, streamed
    /// body in 64 KiB chunks holding a lease. A changed image has a new digest, hence a new ETag
    /// and a new URL where the owner exposes it: invalidation is exact.
    pub async fn response_after_authorization(
        &self,
        sha256: &str,
        request: &HeaderMap,
    ) -> Result<Response, AssetError> {
        let (meta, file) = self.open(sha256).await?;
        let etag = format!("\"{}\"", meta.sha256);
        let mut builder = Response::builder()
            .header(header::ETAG, &etag)
            .header(
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            )
            .header(header::VARY, "Cookie")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
        let matches = request
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .map(|part| part.trim().trim_start_matches("W/"))
                    .any(|part| part == etag || part == "*")
            });
        if matches {
            return builder
                .status(StatusCode::NOT_MODIFIED)
                .body(Body::empty())
                .map_err(|_| AssetError::Corrupt);
        }
        let content_type =
            HeaderValue::from_str(&meta.mime).map_err(|_| AssetError::InvalidMime)?;
        builder = builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, meta.size);
        let lease = self.lease(&meta.sha256)?;
        let stream = futures_util::stream::unfold(
            (file, lease, meta.size as u64),
            |(mut file, lease, remaining)| async move {
                if remaining == 0 {
                    return None;
                }
                let mut buffer = vec![0u8; CHUNK.min(remaining as usize)];
                match file.read(&mut buffer).await {
                    Ok(0) => Some((
                        Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                        (file, lease, 0),
                    )),
                    Ok(n) => {
                        buffer.truncate(n);
                        Some((Ok(Bytes::from(buffer)), (file, lease, remaining - n as u64)))
                    }
                    Err(e) => Some((Err(e), (file, lease, 0))),
                }
            },
        );
        builder
            .body(Body::from_stream(stream))
            .map_err(|_| AssetError::Corrupt)
    }

    /// Recompute the digest of one object (streaming).
    pub async fn verify(&self, sha256: &str) -> Result<(), AssetError> {
        let (meta, mut file) = self.open(sha256).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; CHUNK];
        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest == meta.sha256 {
            Ok(())
        } else {
            Err(AssetError::Corrupt)
        }
    }

    /// Verify up to `limit` committed assets, in digest order after `after` (for resumable audits).
    pub async fn verify_batch(
        &self,
        after: &str,
        limit: i64,
    ) -> Result<(VerifyReport, Option<String>), AssetError> {
        let rows: Vec<(String,)> = crate::database::query_as(
            "SELECT sha256 FROM assets WHERE sha256>? ORDER BY sha256 LIMIT ?",
        )
        .bind(after)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut report = VerifyReport::default();
        let last = rows.last().map(|r| r.0.clone());
        for (sha,) in rows {
            report.checked += 1;
            match self.verify(&sha).await {
                Ok(()) => {}
                Err(AssetError::Corrupt) => {
                    if self.blocking_exists(&sha).await? {
                        report.corrupt.push(sha)
                    } else {
                        report.missing.push(sha)
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok((report, last))
    }

    /// Everything a complete backup must embed: all committed rows plus a way to open each object.
    /// Rows come from SQL (the source of truth); bytes are re-hashed by the backup writer.
    pub async fn export_source(&self) -> Result<crate::backup_bundle::AssetSource, AssetError> {
        let rows: Vec<AssetMeta> =
            crate::database::query_as("SELECT sha256, size, mime FROM assets ORDER BY sha256")
                .fetch_all(&self.pool)
                .await?;
        let backend = self.backend.clone();
        Ok(crate::backup_bundle::AssetSource {
            items: rows
                .into_iter()
                .map(|r| (r.sha256, r.size.max(0) as u64, r.mime))
                .collect(),
            open: Box::new(move |sha| backend.open(sha)),
        })
    }

    /// Put one object from an extracted backup back into the store. The bytes are hashed again and
    /// must match both `sha256` and `size`; SQL rows are not touched (they come from the restored
    /// database). Idempotent: an existing object is left as is.
    pub async fn restore_object(
        &self,
        sha256: &str,
        size: u64,
        from: &Path,
    ) -> Result<bool, AssetError> {
        if !is_sha256_hex(sha256) {
            return Err(AssetError::InvalidDigest);
        }
        if self.blocking_exists(sha256).await? {
            return Ok(false);
        }
        let _shared = self.gate.read().await;
        let temp = self
            .backend
            .temp_dir()
            .join(format!("{}.part", uuid::Uuid::new_v4().simple()));
        let result: Result<(), AssetError> = async {
            let mut source = tokio::fs::File::open(from).await?;
            let mut target = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .await?;
            let mut hasher = Sha256::new();
            let mut copied = 0u64;
            let mut buffer = vec![0u8; CHUNK];
            loop {
                let n = source.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                copied += n as u64;
                hasher.update(&buffer[..n]);
                target.write_all(&buffer[..n]).await?;
            }
            target.sync_all().await?;
            let digest: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if copied != size || digest != sha256 {
                return Err(AssetError::Corrupt);
            }
            self.blocking_publish(sha256, &temp).await
        }
        .await;
        let _ = tokio::fs::remove_file(&temp).await;
        result.map(|()| true)
    }

    /// Import every object listed in `<dir>/assets.json` (written by backup extraction), then audit
    /// the whole store against SQL. The restore is only complete when the returned report has
    /// `is_complete()`: rows without bytes, or altered bytes, are never reported as success.
    pub async fn restore_extracted(&self, dir: &Path) -> Result<RestoreReport, AssetError> {
        #[derive(serde::Deserialize)]
        struct Entry {
            sha256: String,
            size: u64,
        }
        let list = dir.join("assets.json");
        let entries: Vec<Entry> = match tokio::fs::read(&list).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                AssetError::Io(io::Error::new(io::ErrorKind::InvalidData, "assets.json"))
            })?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let (mut imported, mut already_present) = (0u64, 0u64);
        for entry in &entries {
            let path = dir.join("assets").join(&entry.sha256);
            if self
                .restore_object(&entry.sha256, entry.size, &path)
                .await?
            {
                imported += 1;
            } else {
                already_present += 1;
            }
        }
        Ok(RestoreReport {
            imported,
            already_present,
            audit: self.verify_all().await?,
        })
    }

    /// Audit every committed row against the store (streaming, bounded memory). A restore is only
    /// successful when this report has neither missing nor corrupt objects.
    pub async fn verify_all(&self) -> Result<VerifyReport, AssetError> {
        let mut total = VerifyReport::default();
        let mut after = String::new();
        loop {
            let (report, last) = self.verify_batch(&after, 500).await?;
            total.checked += report.checked;
            total.missing.extend(report.missing);
            total.corrupt.extend(report.corrupt);
            match last {
                Some(last) => after = last,
                None => return Ok(total),
            }
        }
    }

    pub async fn stats(&self) -> Result<AssetStats, AssetError> {
        let (objects, bytes, orphaned): (i64, i64, i64) = crate::database::query_as(
            "SELECT COUNT(*), COALESCE(SUM(size),0), COALESCE(SUM(CASE WHEN orphaned_at IS NULL THEN 0 ELSE 1 END),0) FROM assets",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(AssetStats {
            objects,
            bytes,
            orphaned,
        })
    }

    /// Mark, then delete after the grace period, unreferenced assets; sweep stray files.
    /// Idempotent and resumable: every step re-checks its precondition in SQL.
    pub async fn collect(&self) -> Result<GcReport, AssetError> {
        self.collect_at(now()).await
    }

    pub async fn collect_at(&self, at: i64) -> Result<GcReport, AssetError> {
        let _exclusive = self.gate.write().await;
        let mut report = GcReport::default();
        let unreferenced = self.unreferenced();

        let marked = crate::database::query(sqlx::AssertSqlSafe(format!(
            "UPDATE assets SET orphaned_at=? WHERE orphaned_at IS NULL AND {unreferenced}"
        )))
        .bind(at)
        .execute(&self.pool)
        .await?;
        report.marked = marked.rows_affected();
        let cleared = crate::database::query(sqlx::AssertSqlSafe(format!(
            "UPDATE assets SET orphaned_at=NULL WHERE orphaned_at IS NOT NULL AND NOT {unreferenced}"
        )))
        .execute(&self.pool)
        .await?;
        report.cleared = cleared.rows_affected();

        let cutoff = at - self.config.orphan_grace.as_secs() as i64;
        let candidates: Vec<(String, i64)> = crate::database::query_as(
            "SELECT sha256, size FROM assets WHERE orphaned_at IS NOT NULL AND orphaned_at<=? ORDER BY orphaned_at LIMIT ?",
        )
        .bind(cutoff)
        .bind(MAX_GC_BATCH)
        .fetch_all(&self.pool)
        .await?;
        for (sha, size) in candidates {
            if self.is_leased(&sha) {
                report.skipped_leased += 1;
                continue;
            }
            // The foreign keys also refuse this delete for a referenced row.
            let deleted = crate::database::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM assets WHERE sha256=? AND orphaned_at IS NOT NULL AND {unreferenced}"
            )))
            .bind(&sha)
            .execute(&self.pool)
            .await;
            match deleted {
                Ok(done) if done.rows_affected() == 1 => {
                    report.deleted_assets += 1;
                    report.deleted_bytes += size as u64;
                    let (backend, file) = (self.backend.clone(), sha.clone());
                    match tokio::task::spawn_blocking(move || backend.remove(&file)).await {
                        Ok(Ok(_)) => {}
                        // The row is gone; the stray file is swept on the next run.
                        _ => report.failures.push(GcFailure {
                            kind: "disk",
                            scope: "asset-file".into(),
                        }),
                    }
                }
                Ok(_) => {}
                Err(_) => report.failures.push(GcFailure {
                    kind: "database",
                    scope: "assets".into(),
                }),
            }
        }

        // Stray files: published but never committed (crash) or whose row was deleted.
        let backend = self.backend.clone();
        let grace = self.config.file_grace;
        let scanned = tokio::task::spawn_blocking(move || {
            let temp = backend.sweep_temp(grace);
            let objects = backend.scan(MAX_SWEEP);
            (temp, objects)
        })
        .await
        .map_err(|_| io::Error::other("storage worker failed"))?;
        match scanned.0 {
            Ok(n) => report.swept_temp = n as u64,
            Err(_) => report.failures.push(GcFailure {
                kind: "disk",
                scope: "asset-temp".into(),
            }),
        }
        match scanned.1 {
            Ok(objects) => {
                for entry in objects {
                    let old_enough = entry
                        .modified
                        .elapsed()
                        .is_ok_and(|age| age >= self.config.file_grace);
                    if !old_enough || self.is_leased(&entry.sha256) {
                        continue;
                    }
                    let known: Option<(String,)> =
                        crate::database::query_as("SELECT sha256 FROM assets WHERE sha256=?")
                            .bind(&entry.sha256)
                            .fetch_optional(&self.pool)
                            .await?;
                    if known.is_none() {
                        let (backend, file) = (self.backend.clone(), entry.sha256.clone());
                        match tokio::task::spawn_blocking(move || backend.remove(&file)).await {
                            Ok(Ok(true)) => report.swept_files += 1,
                            Ok(Ok(false)) => {}
                            _ => report.failures.push(GcFailure {
                                kind: "disk",
                                scope: "asset-file".into(),
                            }),
                        }
                    }
                }
            }
            Err(_) => report.failures.push(GcFailure {
                kind: "disk",
                scope: "asset-scan".into(),
            }),
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::IdentityStore;

    const TEST_REFS: &[(&str, &str)] = &[("asset_test_refs", "sha256")];

    struct Fixture {
        _dir: tempfile::TempDir,
        identity: IdentityStore,
        store: Arc<AssetStore>,
        backend: Arc<LocalFs>,
    }

    async fn fixture(config: AssetConfig) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let identity = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        crate::database::query(
            "CREATE TABLE asset_test_refs(id TEXT PRIMARY KEY, sha256 TEXT NOT NULL REFERENCES assets(sha256))",
        )
        .execute(identity.pool())
        .await
        .unwrap();
        let backend = Arc::new(LocalFs::open(&dir.path().join("blobs"), None).unwrap());
        let store = Arc::new(AssetStore::with_refs(
            identity.pool().clone(),
            backend.clone(),
            config,
            TEST_REFS,
        ));
        Fixture {
            _dir: dir,
            identity,
            store,
            backend,
        }
    }

    fn quick() -> AssetConfig {
        AssetConfig {
            max_object_bytes: 1024 * 1024,
            quota_bytes: None,
            orphan_grace: Duration::from_secs(100),
            file_grace: Duration::ZERO,
        }
    }

    async fn reference(f: &Fixture, id: &str, sha: &str) {
        crate::database::query("INSERT INTO asset_test_refs(id,sha256) VALUES (?,?)")
            .bind(id)
            .bind(sha)
            .execute(f.identity.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn identical_bytes_are_stored_once_and_survive_until_unreferenced() {
        let f = fixture(quick()).await;
        let a = f
            .store
            .put_bytes(b"avatar-bytes", "image/png")
            .await
            .unwrap();
        let b = f
            .store
            .put_bytes(b"avatar-bytes", "image/png")
            .await
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.sha256.len(), 64);
        assert_eq!(f.store.stats().await.unwrap().objects, 1);
        assert_eq!(f.backend.scan(10).unwrap().len(), 1);
        reference(&f, "user-1", &a.sha256).await;
        reference(&f, "user-2", &a.sha256).await;
        let later = now() + 10_000;
        // Referenced by two owners: never collected, however late.
        let report = f.store.collect_at(later).await.unwrap();
        assert_eq!((report.marked, report.deleted_assets), (0, 0));
        // One reference left: still kept.
        crate::database::query("DELETE FROM asset_test_refs WHERE id='user-1'")
            .execute(f.identity.pool())
            .await
            .unwrap();
        assert_eq!(f.store.collect_at(later).await.unwrap().deleted_assets, 0);
        // Last reference gone: phase 1 only marks, deletion is deferred by the grace period.
        crate::database::query("DELETE FROM asset_test_refs")
            .execute(f.identity.pool())
            .await
            .unwrap();
        let report = f.store.collect_at(later).await.unwrap();
        assert_eq!((report.marked, report.deleted_assets), (1, 0));
        assert!(f.store.open(&a.sha256).await.is_ok());
        // A new reference before the deadline rescues it.
        reference(&f, "user-3", &a.sha256).await;
        let report = f.store.collect_at(later + 500).await.unwrap();
        assert_eq!((report.cleared, report.deleted_assets), (1, 0));
        // Unreferenced again, then past the grace period: row and file are deleted.
        crate::database::query("DELETE FROM asset_test_refs")
            .execute(f.identity.pool())
            .await
            .unwrap();
        f.store.collect_at(later + 600).await.unwrap();
        let report = f.store.collect_at(later + 800).await.unwrap();
        assert_eq!(report.deleted_assets, 1);
        assert!(report.failures.is_empty());
        assert!(!f.backend.exists(&a.sha256).unwrap());
        assert!(matches!(
            f.store.meta(&a.sha256).await,
            Err(AssetError::NotFound)
        ));
    }

    #[tokio::test]
    async fn foreign_key_refuses_deleting_a_referenced_asset() {
        let f = fixture(quick()).await;
        let a = f.store.put_bytes(b"poster", "image/webp").await.unwrap();
        reference(&f, "work-1", &a.sha256).await;
        let result = crate::database::query("DELETE FROM assets WHERE sha256=?")
            .bind(&a.sha256)
            .execute(f.identity.pool())
            .await;
        assert!(result.is_err(), "database must protect referenced assets");
    }

    #[tokio::test]
    async fn limits_validation_and_invalid_digests_are_enforced() {
        let mut config = quick();
        config.max_object_bytes = 10;
        config.quota_bytes = Some(15);
        let f = fixture(config).await;
        assert!(matches!(
            f.store.put_bytes(&[1; 11], "image/png").await,
            Err(AssetError::TooLarge)
        ));
        assert!(matches!(
            f.store.put_bytes(b"", "image/png").await,
            Err(AssetError::Empty)
        ));
        for mime in ["png", "IMAGE/PNG", "image/png; x=1", "a/b\r\nx: y", ""] {
            assert!(matches!(
                f.store.put_bytes(b"x", mime).await,
                Err(AssetError::InvalidMime)
            ));
        }
        f.store.put_bytes(&[1; 10], "image/png").await.unwrap();
        assert!(matches!(
            f.store.put_bytes(&[2; 10], "image/png").await,
            Err(AssetError::QuotaExceeded)
        ));
        // Deduplicated content costs no quota.
        f.store.put_bytes(&[1; 10], "image/png").await.unwrap();
        // Nothing but the one published object remains, and no staging file.
        assert_eq!(f.backend.scan(10).unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(f.backend.temp_dir()).unwrap().count(), 0);
        for bad in ["../../etc/passwd", "AB", &"A".repeat(64), &"g".repeat(64)] {
            assert!(matches!(
                f.store.meta(bad).await,
                Err(AssetError::InvalidDigest)
            ));
            assert!(f.backend.open(bad).is_err());
        }
    }

    #[tokio::test]
    async fn crash_leftovers_are_swept_and_never_break_a_later_put() {
        let f = fixture(quick()).await;
        // Crash after publication but before the SQL row: file without row.
        let temp = f.backend.temp_dir().join("crashed.part");
        std::fs::write(&temp, b"orphan-bytes").unwrap();
        let sha: String = Sha256::digest(b"orphan-bytes")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        f.backend.publish(&sha, &temp).unwrap();
        // Crash during staging: stale temporary file.
        std::fs::write(f.backend.temp_dir().join("half.part"), b"par").unwrap();
        // A later identical upload simply adopts the file.
        let meta = f
            .store
            .put_bytes(b"orphan-bytes", "image/png")
            .await
            .unwrap();
        assert_eq!(meta.sha256, sha);
        f.store.verify(&sha).await.unwrap();
        // The unrelated stray file is swept; the committed object is untouched.
        let stray = f.backend.temp_dir().join("again.part");
        std::fs::write(&stray, b"x").unwrap();
        let other: String = Sha256::digest(b"stray")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        std::fs::write(f.backend.temp_dir().join("s"), b"stray").unwrap();
        f.backend
            .publish(&other, &f.backend.temp_dir().join("s"))
            .unwrap();
        let report = f.store.collect_at(now()).await.unwrap();
        assert_eq!(report.swept_files, 1);
        assert!(report.swept_temp >= 2);
        assert!(!f.backend.exists(&other).unwrap());
        assert!(f.backend.exists(&sha).unwrap());
    }

    #[tokio::test]
    async fn leased_assets_are_not_collected() {
        let f = fixture(quick()).await;
        let a = f.store.put_bytes(b"in-use", "image/png").await.unwrap();
        let lease = f.store.lease(&a.sha256).unwrap();
        let t = now() + 10_000;
        f.store.collect_at(t).await.unwrap();
        let report = f.store.collect_at(t + 1000).await.unwrap();
        assert_eq!((report.skipped_leased, report.deleted_assets), (1, 0));
        assert!(f.store.open(&a.sha256).await.is_ok());
        drop(lease);
        assert_eq!(
            f.store.collect_at(t + 2000).await.unwrap().deleted_assets,
            1
        );
    }

    #[tokio::test]
    async fn corruption_and_loss_are_detected_not_served() {
        let f = fixture(quick()).await;
        let good = f.store.put_bytes(b"good-bytes", "image/png").await.unwrap();
        let bad = f.store.put_bytes(b"to-corrupt", "image/png").await.unwrap();
        let gone = f.store.put_bytes(b"to-delete!", "image/png").await.unwrap();
        // Same length, different content: only a digest check can see it.
        let file_path = f
            ._dir
            .path()
            .join("blobs/objects")
            .join(&bad.sha256[..2])
            .join(&bad.sha256[2..4])
            .join(&bad.sha256);
        std::fs::write(&file_path, b"corrupted!").unwrap();
        f.backend.remove(&gone.sha256).unwrap();
        let (report, last) = f.store.verify_batch("", 10).await.unwrap();
        assert_eq!(report.checked, 3);
        assert_eq!(report.corrupt, vec![bad.sha256.clone()]);
        assert_eq!(report.missing, vec![gone.sha256.clone()]);
        assert!(last.is_some());
        assert!(f.store.verify(&good.sha256).await.is_ok());
        assert!(matches!(
            f.store.open(&gone.sha256).await,
            Err(AssetError::Corrupt)
        ));
        // Re-uploading the correct bytes heals a lost file.
        f.store.put_bytes(b"to-delete!", "image/png").await.unwrap();
        assert!(f.store.verify(&gone.sha256).await.is_ok());
    }

    #[tokio::test]
    async fn http_response_is_private_immutable_and_revalidates_with_etag() {
        let f = fixture(quick()).await;
        let body = vec![7u8; 200_000];
        let a = f.store.put_bytes(&body, "image/png").await.unwrap();
        let response = f
            .store
            .response_after_authorization(&a.sha256, &HeaderMap::new())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let etag = response.headers()[header::ETAG]
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(etag, format!("\"{}\"", a.sha256));
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        let cache = response.headers()[header::CACHE_CONTROL].to_str().unwrap();
        assert!(cache.starts_with("private") && cache.contains("immutable"));
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(bytes.as_ref(), body.as_slice());
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_str(&etag).unwrap());
        let response = f
            .store
            .response_after_authorization(&a.sha256, &headers)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert!(response.headers().get(header::ETAG).is_some());
    }

    #[tokio::test]
    async fn concurrent_uploads_of_the_same_content_converge() {
        let f = fixture(quick()).await;
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let store = f.store.clone();
            tasks.push(tokio::spawn(async move {
                store.put_bytes(&[9; 50_000], "image/png").await
            }));
        }
        let mut digests = std::collections::BTreeSet::new();
        for task in tasks {
            digests.insert(task.await.unwrap().unwrap().sha256);
        }
        assert_eq!(digests.len(), 1);
        assert_eq!(f.store.stats().await.unwrap().objects, 1);
        assert_eq!(f.backend.scan(10).unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(f.backend.temp_dir()).unwrap().count(), 0);
    }

    /// Every foreign key to `assets` must be registered, otherwise the collector would consider
    /// the asset unreferenced; the database would block the delete, but the registry must be exact.
    #[tokio::test]
    async fn registry_matches_every_foreign_key_to_assets() {
        let dir = tempfile::tempdir().unwrap();
        let identity = IdentityStore::open(&dir.path().join("core.sqlite3"))
            .await
            .unwrap();
        let rows: Vec<(String, String)> = crate::database::query_as(
            "SELECT m.name, f.\"from\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f WHERE m.type='table' AND f.\"table\"='assets'",
        )
        .fetch_all(identity.pool())
        .await
        .unwrap();
        let mut actual: Vec<(String, String)> = rows;
        actual.sort();
        let mut registered: Vec<(String, String)> = REFERENCING_COLUMNS
            .iter()
            .map(|(t, c)| (t.to_string(), c.to_string()))
            .collect();
        registered.sort();
        assert_eq!(actual, registered);
    }

    #[test]
    fn local_root_overlapping_the_media_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let backend = Arc::new(LocalFs::open(&media.join("assets"), None).unwrap());
        assert_eq!(backend.local_root().unwrap(), media.join("assets"));
        let own = LocalFs::open(&dir.path().join("own"), Some(&media)).unwrap();
        assert!(own.local_root().is_some());
        assert!(matches!(
            LocalFs::open(&media.join("inside"), Some(&media)),
            Err(AssetError::Configuration(_))
        ));
    }

    const BACKUP_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[tokio::test]
    async fn backup_carries_assets_and_restore_is_verified_on_a_fresh_store() {
        let f = fixture(quick()).await;
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
        let a = f.store.put_bytes(&big, "image/png").await.unwrap();
        let b = f
            .store
            .put_bytes(b"tiny avatar", "image/webp")
            .await
            .unwrap();
        let db = f._dir.path().join("snapshot.db");
        std::fs::write(&db, b"not-a-real-database-but-non-empty").unwrap();
        let archive = f._dir.path().join("full.media-backup");
        let source = f.store.export_source().await.unwrap();
        assert_eq!(source.items.len(), 2);
        crate::backup_bundle::create_with_assets(
            &db,
            Default::default(),
            &archive,
            BACKUP_KEY,
            "sqlite",
            Some(source),
        )
        .await
        .unwrap();
        let out = f._dir.path().join("extracted");
        let summary = crate::backup_bundle::extract(&archive, &out, BACKUP_KEY)
            .await
            .unwrap();
        assert_eq!(summary.assets, 2);

        // Fresh installation: SQL rows come back with the restored database, bytes via restore_object.
        let g = fixture(quick()).await;
        for meta in [&a, &b] {
            crate::database::query(
                "INSERT INTO assets(sha256,size,mime,created_at) VALUES (?,?,?,0)",
            )
            .bind(&meta.sha256)
            .bind(meta.size)
            .bind(&meta.mime)
            .execute(g.identity.pool())
            .await
            .unwrap();
        }
        let before = g.store.verify_all().await.unwrap();
        assert_eq!(
            before.missing.len(),
            2,
            "rows without bytes must not look restored"
        );
        let restored = g.store.restore_extracted(&out).await.unwrap();
        assert_eq!((restored.imported, restored.already_present), (2, 0));
        assert!(restored.is_complete());
        let again = g.store.restore_extracted(&out).await.unwrap();
        assert_eq!(
            (again.imported, again.already_present),
            (0, 2),
            "idempotent"
        );
        let after = restored.audit;
        assert_eq!(
            (after.checked, after.missing.len(), after.corrupt.len()),
            (2, 0, 0)
        );
        assert_eq!(g.store.read_all(&a.sha256).await.unwrap().1, big);

        // A wrong object is refused and never published.
        let wrong = out.join("assets").join(&b.sha256);
        let h = fixture(quick()).await;
        assert!(matches!(
            h.store
                .restore_object(&a.sha256, a.size as u64, &wrong)
                .await,
            Err(AssetError::Corrupt)
        ));
        assert!(!h.backend.exists(&a.sha256).unwrap());
    }

    #[tokio::test]
    async fn backup_refuses_a_missing_or_altered_asset() {
        let f = fixture(quick()).await;
        let a = f
            .store
            .put_bytes(b"first object bytes", "image/png")
            .await
            .unwrap();
        let db = f._dir.path().join("snapshot.db");
        std::fs::write(&db, b"non-empty").unwrap();
        // Altered in place (same size, other bytes).
        let path = f
            ._dir
            .path()
            .join("blobs/objects")
            .join(&a.sha256[..2])
            .join(&a.sha256[2..4])
            .join(&a.sha256);
        std::fs::write(&path, b"FIRST OBJECT BYTES").unwrap();
        let target = f._dir.path().join("bad.media-backup");
        let result = crate::backup_bundle::create_with_assets(
            &db,
            Default::default(),
            &target,
            BACKUP_KEY,
            "sqlite",
            Some(f.store.export_source().await.unwrap()),
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::backup_bundle::BundleError::AssetIntegrity)
        ));
        assert!(
            !target.exists(),
            "no archive may be published after a failure"
        );
        // Missing file.
        std::fs::remove_file(&path).unwrap();
        let result = crate::backup_bundle::create_with_assets(
            &db,
            Default::default(),
            &target,
            BACKUP_KEY,
            "sqlite",
            Some(f.store.export_source().await.unwrap()),
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::backup_bundle::BundleError::AssetIntegrity)
        ));
    }
}
