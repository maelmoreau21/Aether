CREATE TABLE media_nodes_new (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) BETWEEN 1 AND 128),
    name TEXT NOT NULL DEFAULT 'Local Media Node' CHECK(length(name) BETWEEN 1 AND 100),
    kind TEXT NOT NULL DEFAULT 'local' CHECK(kind IN ('local', 'remote')),
    base_url TEXT,
    public_url TEXT,
    status TEXT NOT NULL DEFAULT 'connected' CHECK(status IN ('connected', 'disconnected', 'revoked')),
    key_id TEXT NOT NULL DEFAULT 'local',
    shared_secret TEXT NOT NULL DEFAULT '',
    roots_json TEXT NOT NULL DEFAULT '[]',
    paired_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_seen_at INTEGER,
    revoked_at INTEGER
) STRICT;

INSERT INTO media_nodes_new(
    id, name, kind, base_url, public_url, status, key_id, shared_secret, roots_json, paired_at, last_seen_at, revoked_at
)
SELECT
    id,
    CASE WHEN id = 'local' THEN 'Node local (Standalone)' ELSE id END,
    CASE WHEN id = 'local' THEN 'local' ELSE 'remote' END,
    NULL,
    NULL,
    'connected',
    'local',
    '',
    '[]',
    unixepoch(),
    unixepoch(),
    NULL
FROM media_nodes;

DROP TABLE media_nodes;
ALTER TABLE media_nodes_new RENAME TO media_nodes;
