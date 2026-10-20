ALTER TABLE libraries DROP CONSTRAINT libraries_kind_check;
ALTER TABLE libraries ADD CONSTRAINT libraries_kind_check CHECK(kind IN ('films','series','music'));
CREATE TABLE music_tracks (
 id TEXT PRIMARY KEY NOT NULL, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 root_snapshot TEXT NOT NULL, relative_path TEXT NOT NULL, size BIGINT NOT NULL, modified_ns TEXT NOT NULL,
 title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, album_artist TEXT NOT NULL,
 track_number BIGINT, disc_number BIGINT, duration DOUBLE PRECISION, codec TEXT NOT NULL, mime TEXT NOT NULL,
 scan_id TEXT NOT NULL, available BIGINT NOT NULL DEFAULT 1,
 UNIQUE(library_id,root_snapshot,relative_path)
);
CREATE INDEX music_tracks_library ON music_tracks(library_id,available);
CREATE TABLE music_scan_runs(library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,scan_id TEXT NOT NULL);
