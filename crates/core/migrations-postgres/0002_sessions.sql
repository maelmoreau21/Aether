CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    last_seen_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE login_throttle (
    singleton BIGINT PRIMARY KEY CHECK(singleton = 1),
    window_started BIGINT NOT NULL,
    attempts BIGINT NOT NULL
);
