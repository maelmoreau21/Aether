//! Server log: recent lines in memory for the live viewer, and daily files on disk.
//!
//! The Core writes its diagnostics to stderr. Every line at or above the configured
//! level also lands in a bounded ring buffer (lost on restart) and, once
//! [`init_files`] has run, in `aether-core-YYYY-MM-DD.log` (UTC day) inside the log
//! directory. Old files are pruned after the retention period. Secrets carried in
//! URLs are masked before a line is kept anywhere. Administrators read the buffer
//! through `GET /api/v1/admin/logs`, list and read the files through
//! `/api/v1/admin/logs/files`, and change the level and retention through
//! `/api/v1/admin/logs/settings` (persisted in the server settings).
use crate::{auth::AuthState, identity::Role};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::Write,
    path::{Path as DiskPath, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const CAPACITY: usize = 2000;
const MAX_LINE: usize = 4096;
const PAGE: usize = 500;
/// The viewer reads at most the end of a file; the full file is a download.
const VIEW_BYTES: u64 = 1024 * 1024;
const FILE_PREFIX: &str = "aether-core-";
const FILE_SUFFIX: &str = ".log";
pub const DEFAULT_RETENTION_DAYS: u32 = 14;
pub const MAX_RETENTION_DAYS: u32 = 365;

/// Ordered from the most to the least verbose: a level keeps itself and everything above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    #[default]
    Info,
    Warning,
    Error,
}

impl Level {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Debug,
            1 => Self::Info,
            2 => Self::Warning,
            _ => Self::Error,
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub seq: u64,
    /// Unix time in milliseconds.
    pub at: u64,
    pub level: Level,
    pub message: String,
}

struct Buffer {
    next: u64,
    lines: VecDeque<Entry>,
}

static BUFFER: Mutex<Buffer> = Mutex::new(Buffer {
    next: 1,
    lines: VecDeque::new(),
});
static MIN_LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);
static RETENTION_DAYS: AtomicU32 = AtomicU32::new(DEFAULT_RETENTION_DAYS);

struct Sink {
    directory: PathBuf,
    day: String,
    file: Option<std::fs::File>,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// The level below which lines are dropped (stderr, viewer and files alike).
pub fn min_level() -> Level {
    Level::from_u8(MIN_LEVEL.load(Ordering::Relaxed))
}

pub fn enabled(level: Level) -> bool {
    level >= min_level()
}

/// Applies the persisted level and retention, then prunes files past the retention.
pub fn configure(level: Level, retention_days: u32) {
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
    RETENTION_DAYS.store(
        retention_days.clamp(1, MAX_RETENTION_DAYS),
        Ordering::Relaxed,
    );
    if let Some(directory) = directory() {
        prune(&directory);
    }
}

pub fn retention_days() -> u32 {
    RETENTION_DAYS.load(Ordering::Relaxed)
}

/// `MEDIA_LOG_DIR`, else `logs` beside the SQLite database, else `/data/logs`, else `./logs`.
pub fn default_directory() -> PathBuf {
    if let Some(dir) = std::env::var_os("MEDIA_LOG_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(parent) = std::env::var_os("MEDIA_DB_PATH")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .and_then(|path| path.parent().map(DiskPath::to_path_buf))
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        return parent.join("logs");
    }
    if DiskPath::new("/data").is_dir() {
        return PathBuf::from("/data/logs");
    }
    PathBuf::from("logs")
}

/// Starts writing kept lines to daily files in `directory`.
pub fn init_files(directory: PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(&directory)?;
    // Probe write access now so a read-only volume is reported once, at startup.
    let probe = directory.join(".write-test");
    std::fs::write(&probe, b"")?;
    let _ = std::fs::remove_file(probe);
    prune(&directory);
    *SINK.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(Sink {
        directory,
        day: String::new(),
        file: None,
    });
    Ok(())
}

/// The log directory, once files are enabled.
pub fn directory() -> Option<PathBuf> {
    SINK.lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .as_ref()
        .map(|sink| sink.directory.clone())
}

/// Writes the line to stderr and keeps it, when its guessed level is enabled.
pub fn record(line: String) {
    record_level(classify(&line), line);
}

/// Like [`record`], with an explicit level.
pub fn record_level(level: Level, line: String) {
    if !enabled(level) {
        return;
    }
    ::std::eprintln!("{line}");
    keep(level, &line);
}

/// Keeps a line already written elsewhere (stdout, the panic hook).
pub fn remember(level: Level, line: &str) {
    if enabled(level) {
        keep(level, line);
    }
}

fn keep(level: Level, line: &str) {
    let message: String = redact(line.trim_end()).chars().take(MAX_LINE).collect();
    if message.is_empty() {
        return;
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    {
        let mut buffer = BUFFER.lock().unwrap_or_else(|poison| poison.into_inner());
        let seq = buffer.next;
        buffer.next += 1;
        if buffer.lines.len() == CAPACITY {
            buffer.lines.pop_front();
        }
        buffer.lines.push_back(Entry {
            seq,
            at,
            level,
            message: message.clone(),
        });
    }
    write_file(at, level, &message);
}

fn write_file(at: u64, level: Level, message: &str) {
    let mut guard = SINK.lock().unwrap_or_else(|poison| poison.into_inner());
    let Some(sink) = guard.as_mut() else {
        return;
    };
    let (day, stamp) = timestamp(at);
    if sink.day != day || sink.file.is_none() {
        sink.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(sink.directory.join(file_name(&day)))
            .ok();
        let rolled = !sink.day.is_empty() && sink.day != day;
        sink.day = day;
        if rolled {
            prune(&sink.directory);
        }
    }
    if let Some(file) = sink.file.as_mut()
        && writeln!(file, "{stamp} {:<5} {message}", level.tag()).is_err()
    {
        // Reopen on the next line (rotated, deleted or full disk).
        sink.file = None;
    }
}

fn file_name(day: &str) -> String {
    format!("{FILE_PREFIX}{day}{FILE_SUFFIX}")
}

/// UTC day (`YYYY-MM-DD`) and RFC 3339 timestamp with milliseconds.
fn timestamp(at_ms: u64) -> (String, String) {
    let moment = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at_ms) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let day = format!(
        "{:04}-{:02}-{:02}",
        moment.year(),
        moment.month() as u8,
        moment.day()
    );
    let stamp = format!(
        "{day}T{:02}:{:02}:{:02}.{:03}Z",
        moment.hour(),
        moment.minute(),
        moment.second(),
        moment.millisecond()
    );
    (day, stamp)
}

/// Only names this module writes are ever listed, read or deleted.
fn valid_name(name: &str) -> bool {
    let Some(day) = name
        .strip_prefix(FILE_PREFIX)
        .and_then(|rest| rest.strip_suffix(FILE_SUFFIX))
    else {
        return false;
    };
    day.len() == 10
        && day.char_indices().all(|(index, c)| {
            if index == 4 || index == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

fn day_of(name: &str) -> &str {
    &name[FILE_PREFIX.len()..FILE_PREFIX.len() + 10]
}

fn prune(directory: &DiskPath) {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    // Today counts as one day: a retention of 1 keeps only today's file.
    let (cutoff, _) =
        timestamp(now_ms.saturating_sub(u64::from(retention_days()).saturating_sub(1) * 86_400_000));
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if valid_name(&name) && day_of(&name) < cutoff.as_str() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    pub name: String,
    pub size: u64,
    /// Unix seconds.
    pub modified_at: u64,
}

pub fn files() -> Vec<LogFile> {
    let Some(directory) = directory() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<LogFile> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = entry.metadata().ok()?;
            (valid_name(&name) && metadata.is_file()).then(|| LogFile {
                name,
                size: metadata.len(),
                modified_at: metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs()),
            })
        })
        .collect();
    files.sort_by(|a, b| b.name.cmp(&a.name));
    files
}

/// Lines newer than `after`, oldest first, at most one page.
pub fn since(after: u64, minimum: Level) -> (Vec<Entry>, u64) {
    let buffer = BUFFER.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut lines: Vec<Entry> = buffer
        .lines
        .iter()
        .filter(|entry| entry.seq > after && entry.level >= minimum)
        .cloned()
        .collect();
    if lines.len() > PAGE {
        lines.drain(..lines.len() - PAGE);
    }
    (lines, buffer.next - 1)
}

fn classify(line: &str) -> Level {
    let lower = line.to_ascii_lowercase();
    if ["panic", "error", "failed", "could not", "cannot", "unable"]
        .iter()
        .any(|word| lower.contains(word))
    {
        Level::Error
    } else if ["warn", "retry", "retrying", "skipped", "refused", "denied"]
        .iter()
        .any(|word| lower.contains(word))
    {
        Level::Warning
    } else {
        Level::Info
    }
}

/// Masks the values of credential-like `name=value` pairs (query strings, DSNs).
fn redact(line: &str) -> String {
    const NAMES: [&str; 8] = [
        "api_key", "apikey", "token", "password", "secret", "key", "jwt", "sig",
    ];
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(index) = rest.find('=') {
        let (before, after) = rest.split_at(index);
        let name_start = before
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .map_or(0, |i| i + 1);
        let name = before[name_start..].to_ascii_lowercase();
        out.push_str(before);
        out.push('=');
        let value = &after[1..];
        let end = value
            .find(|c: char| c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ')' | ','))
            .unwrap_or(value.len());
        if !value[..end].is_empty() && NAMES.iter().any(|n| name.ends_with(n)) {
            out.push_str("***");
        } else {
            out.push_str(&value[..end]);
        }
        rest = &value[end..];
    }
    out.push_str(rest);
    // `scheme://user:password@host` credentials.
    if let Some(scheme) = out.find("://")
        && let Some(at) = out[scheme + 3..].find('@').map(|i| i + scheme + 3)
        && let Some(colon) = out[scheme + 3..at].find(':').map(|i| i + scheme + 3)
        && !out[scheme + 3..at].contains(char::is_whitespace)
    {
        out.replace_range(colon + 1..at, "***");
    }
    out
}

/// At the debug level, one line per HTTP request (path only: query strings may carry secrets).
pub async fn trace_requests(request: Request, next: Next) -> Response {
    if !enabled(Level::Debug) {
        return next.run(request).await;
    }
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();
    let response = next.run(request).await;
    // The viewer polls every few seconds: logging its own requests would drown everything else.
    if !path.starts_with("/api/v1/admin/logs") {
        record_level(
            Level::Debug,
            format!(
                "HTTP {method} {path} -> {} ({} ms)",
                response.status().as_u16(),
                started.elapsed().as_millis()
            ),
        );
    }
    response
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LogQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    level: Option<Level>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SettingsInput {
    level: Level,
    retention_days: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileQuery {
    #[serde(default)]
    download: bool,
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/logs", get(list))
        .route(
            "/api/v1/admin/logs/settings",
            get(read_settings).put(write_settings),
        )
        .route("/api/v1/admin/logs/files", get(list_files))
        .route("/api/v1/admin/logs/files/{name}", get(read_file))
        .layer(DefaultBodyLimit::max(1024))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        ))
        .with_state(auth)
}

async fn administrator(
    auth: &AuthState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<(), StatusCode> {
    let actor = auth.authenticate(headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    if mutation {
        auth.require_csrf(headers, &actor)?;
    }
    Ok(())
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    let (entries, latest) = since(query.after, query.level.unwrap_or(Level::Debug));
    Ok(Json(
        json!({ "entries": entries, "latest": latest, "capacity": CAPACITY }),
    ))
}

fn settings_json() -> Value {
    json!({
        "level": min_level(),
        "retentionDays": retention_days(),
        "maxRetentionDays": MAX_RETENTION_DAYS,
        "directory": directory().map(|d| d.to_string_lossy().into_owned()),
        "filesEnabled": directory().is_some(),
    })
}

async fn read_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    Ok(Json(settings_json()))
}

async fn write_settings(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<SettingsInput>,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, true).await?;
    if !(1..=MAX_RETENTION_DAYS).contains(&input.retention_days) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    crate::server_settings::save_logging(&auth.store, input.level, input.retention_days).await?;
    let previous = min_level();
    configure(input.level, input.retention_days);
    if previous != input.level {
        // Written at warning level so the change itself is always recorded.
        record_level(
            Level::Warning,
            format!(
                "Log level changed from {} to {}",
                previous.tag(),
                input.level.tag()
            ),
        );
    }
    Ok(Json(settings_json()))
}

async fn list_files(
    State(auth): State<AuthState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    administrator(&auth, &headers, false).await?;
    let files = tokio::task::spawn_blocking(files)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "files": files,
        "directory": directory().map(|d| d.to_string_lossy().into_owned()),
    })))
}

async fn read_file(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Response, StatusCode> {
    administrator(&auth, &headers, false).await?;
    if !valid_name(&name) {
        return Err(StatusCode::NOT_FOUND);
    }
    let directory = directory().ok_or(StatusCode::NOT_FOUND)?;
    let mut file = tokio::fs::File::open(directory.join(&name))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let size = file
        .metadata()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .len();
    let text_plain = HeaderValue::from_static("text/plain; charset=utf-8");
    if query.download {
        let stream = futures_util::stream::unfold(Some(file), |state| async move {
            let mut file = state?;
            let mut chunk = vec![0u8; 64 * 1024];
            match file.read(&mut chunk).await {
                Ok(0) => None,
                Ok(read) => {
                    chunk.truncate(read);
                    Some((Ok(bytes::Bytes::from(chunk)), Some(file)))
                }
                Err(error) => Some((Err::<bytes::Bytes, _>(error), None)),
            }
        });
        let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok((
            [
                (header::CONTENT_TYPE, text_plain),
                (header::CONTENT_DISPOSITION, disposition),
            ],
            Body::from_stream(stream),
        )
            .into_response());
    }
    let truncated = size > VIEW_BYTES;
    if truncated {
        file.seek(std::io::SeekFrom::Start(size - VIEW_BYTES))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let mut bytes = Vec::with_capacity(size.min(VIEW_BYTES) as usize);
    file.take(VIEW_BYTES)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        // Drop the partial first line.
        if let Some(newline) = text.find('\n') {
            text.drain(..=newline);
        }
    }
    Ok((
        [
            (header::CONTENT_TYPE, text_plain),
            (
                header::HeaderName::from_static("x-log-truncated"),
                HeaderValue::from_static(if truncated { "1" } else { "0" }),
            ),
        ],
        text,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_classified_redacted_bounded_filtered_and_filed() {
        assert_eq!(
            classify("automation task could not persist failure"),
            Level::Error
        );
        assert_eq!(
            classify("Release download failed (x): repository_unavailable"),
            Level::Error
        );
        assert_eq!(classify("retrying later"), Level::Warning);
        assert_eq!(
            classify("aether-core listening on http://x/app/"),
            Level::Info
        );
        assert_eq!(
            redact("GET https://www.omdbapi.com/?apikey=abc123&t=Film failed"),
            "GET https://www.omdbapi.com/?apikey=***&t=Film failed"
        );
        assert_eq!(
            redact("postgres://media:hunter2@db:5432/media?sslmode=disable"),
            "postgres://media:***@db:5432/media?sslmode=disable"
        );
        assert_eq!(redact("a=b c=d"), "a=b c=d");

        assert!(valid_name("aether-core-2026-11-08.log"));
        for name in [
            "aether-core-2026-11-8.log",
            "../aether-core-2026-11-08.log",
            "aether-core-2026-11-08.log.bak",
            "aether-core-2026x11-08.log",
            "other.log",
        ] {
            assert!(!valid_name(name), "{name}");
        }
        assert_eq!(
            timestamp(1_762_600_000_123),
            (
                "2025-11-08".to_owned(),
                "2025-11-08T11:06:40.123Z".to_owned()
            )
        );

        // Files: written for kept lines, old days pruned by retention.
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("aether-core-2000-01-01.log");
        std::fs::write(&old, "old\n").unwrap();
        std::fs::write(temp.path().join("unrelated.log"), "x\n").unwrap();
        init_files(temp.path().to_path_buf()).unwrap();
        assert!(!old.exists());
        assert!(temp.path().join("unrelated.log").exists());

        // Levels: below the minimum, nothing is kept.
        configure(Level::Info, DEFAULT_RETENTION_DAYS);
        let before = since(u64::MAX - 1, Level::Debug).1;
        record_level(Level::Debug, "server_log test debug hidden".into());
        record_level(
            Level::Warning,
            "server_log test warning token=s3cret".into(),
        );
        record_level(Level::Info, "server_log test info".into());
        let (warnings, latest) = since(before, Level::Warning);
        assert!(latest >= before + 2);
        assert!(
            warnings
                .iter()
                .any(|e| e.message == "server_log test warning token=***")
        );
        assert!(!warnings.iter().any(|e| e.message == "server_log test info"));
        let all = since(before, Level::Debug).0;
        assert!(!all.iter().any(|e| e.message.contains("debug hidden")));
        configure(Level::Debug, DEFAULT_RETENTION_DAYS);
        record_level(Level::Debug, "server_log test debug shown".into());
        assert!(
            since(before, Level::Debug)
                .0
                .iter()
                .any(|e| e.message == "server_log test debug shown")
        );
        configure(Level::Error, DEFAULT_RETENTION_DAYS);
        assert!(!enabled(Level::Warning));
        configure(Level::Info, DEFAULT_RETENTION_DAYS);

        let listed = files();
        assert_eq!(listed.len(), 1);
        let written = std::fs::read_to_string(temp.path().join(&listed[0].name)).unwrap();
        assert!(written.contains(" WARN  server_log test warning token=***"));
        assert!(written.contains(" DEBUG server_log test debug shown"));
        assert!(!written.contains("s3cret"));
        *SINK.lock().unwrap() = None;

        for index in 0..CAPACITY + 10 {
            record_level(Level::Info, format!("fill {index}"));
        }
        assert_eq!(BUFFER.lock().unwrap().lines.len(), CAPACITY);
        assert!(since(0, Level::Info).0.len() <= PAGE);
    }
}
