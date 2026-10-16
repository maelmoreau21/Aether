CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE login_throttle (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    window_started INTEGER NOT NULL,
    attempts INTEGER NOT NULL
) STRICT;
