//! Recent server log lines, kept in memory for the administration page.
//!
//! The Core writes its diagnostics to stderr. Every line also lands in a bounded
//! ring buffer (lost on restart, never written to disk) that administrators read
//! through `GET /api/v1/admin/logs`. Secrets carried in URLs are masked first.
use crate::{auth::AuthState, identity::Role};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const CAPACITY: usize = 2000;
const MAX_LINE: usize = 4096;
const PAGE: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warning,
    Error,
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

/// Writes the line to stderr and keeps it for the administration page.
pub fn record(line: String) {
    ::std::eprintln!("{line}");
    keep(classify(&line), &line);
}

/// Like [`record`], with an explicit level.
pub fn record_level(level: Level, line: String) {
    ::std::eprintln!("{line}");
    keep(level, &line);
}

/// Keeps a line already written elsewhere (stdout, the panic hook).
pub fn remember(level: Level, line: &str) {
    keep(level, line);
}

fn keep(level: Level, line: &str) {
    let message: String = redact(line.trim_end()).chars().take(MAX_LINE).collect();
    if message.is_empty() {
        return;
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
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
        message,
    });
}

/// Lines newer than `after`, oldest first, at most one page.
pub fn since(after: u64, minimum: Level) -> (Vec<Entry>, u64) {
    let buffer = BUFFER.lock().unwrap_or_else(|poison| poison.into_inner());
    let rank = |level: Level| level as u8;
    let mut lines: Vec<Entry> = buffer
        .lines
        .iter()
        .filter(|entry| entry.seq > after && rank(entry.level) >= rank(minimum))
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LogQuery {
    #[serde(default)]
    after: u64,
    #[serde(default)]
    level: Option<Level>,
}

pub fn router(auth: AuthState) -> Router {
    Router::new()
        .route("/api/v1/admin/logs", get(list))
        .with_state(auth)
}

async fn list(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>, StatusCode> {
    let actor = auth.authenticate(&headers).await?;
    if actor.user.role != Role::Administrator {
        return Err(StatusCode::FORBIDDEN);
    }
    let (entries, latest) = since(query.after, query.level.unwrap_or(Level::Info));
    Ok(Json(
        json!({ "entries": entries, "latest": latest, "capacity": CAPACITY }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_classified_redacted_and_bounded() {
        assert_eq!(
            classify("automation task could not persist failure"),
            Level::Error
        );
        assert_eq!(
            classify("Release download failed (x); retrying through the GitHub API"),
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

        let before = since(u64::MAX - 1, Level::Info).1;
        record_level(Level::Warning, "server_log test warning".into());
        record_level(Level::Info, "server_log test info".into());
        let (warnings, latest) = since(before, Level::Warning);
        assert!(latest >= before + 2);
        assert!(
            warnings
                .iter()
                .any(|e| e.message == "server_log test warning")
        );
        assert!(!warnings.iter().any(|e| e.message == "server_log test info"));
        for index in 0..CAPACITY + 10 {
            record_level(Level::Info, format!("fill {index}"));
        }
        assert_eq!(BUFFER.lock().unwrap().lines.len(), CAPACITY);
        assert!(since(0, Level::Info).0.len() <= PAGE);
    }
}
