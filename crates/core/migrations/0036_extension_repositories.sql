CREATE TABLE extension_repositories (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    manifest_json TEXT NOT NULL,
    refreshed_at INTEGER NOT NULL
) STRICT;
