CREATE TABLE libraries_next (
 id TEXT PRIMARY KEY NOT NULL,
 name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100),
 kind TEXT NOT NULL CHECK(kind IN ('films','series','music')),
 node_id TEXT NOT NULL REFERENCES media_nodes(id), root TEXT NOT NULL,
 enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), excluded_patterns TEXT NOT NULL DEFAULT ''
) STRICT;
INSERT INTO libraries_next SELECT id,name,kind,node_id,root,enabled,excluded_patterns FROM libraries;
DROP TABLE libraries;
ALTER TABLE libraries_next RENAME TO libraries;
CREATE TABLE music_tracks (
 id TEXT PRIMARY KEY NOT NULL, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 root_snapshot TEXT NOT NULL, relative_path TEXT NOT NULL, size INTEGER NOT NULL, modified_ns TEXT NOT NULL,
 title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, album_artist TEXT NOT NULL,
 track_number INTEGER, disc_number INTEGER, duration REAL, codec TEXT NOT NULL, mime TEXT NOT NULL,
 scan_id TEXT NOT NULL, available INTEGER NOT NULL DEFAULT 1,
 UNIQUE(library_id,root_snapshot,relative_path)
) STRICT;
CREATE INDEX music_tracks_library ON music_tracks(library_id,available);
CREATE TABLE music_scan_runs(library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,scan_id TEXT NOT NULL) STRICT;
