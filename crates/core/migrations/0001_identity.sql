CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('administrator', 'user')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    disabled_at INTEGER
) STRICT;
CREATE TABLE local_credentials (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    login TEXT NOT NULL UNIQUE CHECK(length(login) BETWEEN 3 AND 64 AND login NOT GLOB '*[^a-z0-9._-]*'),
    password_hash TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
-- Permanent completion marker: deleting a user must never reopen bootstrap.
CREATE TABLE bootstrap_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    completed_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
