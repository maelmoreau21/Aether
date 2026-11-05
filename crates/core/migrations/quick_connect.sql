CREATE TABLE IF NOT EXISTS device_connections (
    code_hash TEXT PRIMARY KEY,
    requester_hash TEXT NOT NULL UNIQUE,
    device_label TEXT NOT NULL,
    approver_session_hash TEXT REFERENCES sessions(token_hash) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS device_connections_expiry ON device_connections(expires_at);
