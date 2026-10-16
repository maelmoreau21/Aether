CREATE TABLE media_nodes (
    id TEXT PRIMARY KEY NOT NULL CHECK (id = 'local')
) STRICT;
INSERT INTO media_nodes(id) VALUES ('local');
CREATE TABLE libraries (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
    kind TEXT NOT NULL CHECK(kind IN ('films', 'series')),
    node_id TEXT NOT NULL REFERENCES media_nodes(id),
    root TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
) STRICT;
CREATE TABLE library_permissions (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(library_id, user_id)
) STRICT;
CREATE INDEX library_permissions_user ON library_permissions(user_id, library_id);
CREATE TABLE library_initialization (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1)
) STRICT;
