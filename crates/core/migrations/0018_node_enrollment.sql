-- Ephemeral, single-use enrollment tokens for Media Node self-registration.
CREATE TABLE node_enrollment_tokens (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL,
    used_at INTEGER,
    used_by_node_id TEXT
) STRICT;

ALTER TABLE media_nodes ADD COLUMN connection_mode TEXT NOT NULL DEFAULT 'outbound';
