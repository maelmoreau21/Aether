CREATE TABLE IF NOT EXISTS playback_grants (
    token_hash TEXT PRIMARY KEY,
    session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    asset_id TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS playback_grants_session ON playback_grants(session_hash);
CREATE INDEX IF NOT EXISTS playback_grants_expiry ON playback_grants(expires_at);
