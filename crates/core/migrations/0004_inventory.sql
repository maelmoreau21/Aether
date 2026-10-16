CREATE TABLE inventory_works (id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id), identity_key TEXT NOT NULL, title TEXT NOT NULL, year INTEGER, external_ids_json TEXT NOT NULL, UNIQUE(library_id,identity_key)) STRICT;
CREATE TABLE inventory_editions (id TEXT PRIMARY KEY, work_id TEXT NOT NULL REFERENCES inventory_works(id)) STRICT;
CREATE TABLE inventory_versions (id TEXT PRIMARY KEY, edition_id TEXT NOT NULL REFERENCES inventory_editions(id)) STRICT;
CREATE TABLE inventory_sources (root_snapshot TEXT NOT NULL, id TEXT PRIMARY KEY, version_id TEXT NOT NULL REFERENCES inventory_versions(id), library_id TEXT NOT NULL REFERENCES libraries(id)) STRICT;
CREATE TABLE inventory_assets (id TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES inventory_sources(id), library_id TEXT NOT NULL REFERENCES libraries(id), relative_path TEXT NOT NULL, size INTEGER NOT NULL, modified_ns TEXT NOT NULL, mime TEXT NOT NULL, probe_json TEXT NOT NULL, available INTEGER NOT NULL CHECK(available IN (0,1)), UNIQUE(library_id,relative_path)) STRICT;
CREATE TABLE inventory_tracks (id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES inventory_assets(id), stream_index INTEGER NOT NULL, metadata_json TEXT NOT NULL, UNIQUE(asset_id,stream_index)) STRICT;
CREATE INDEX inventory_sources_library ON inventory_sources(library_id);
CREATE TABLE scan_jobs (library_id TEXT PRIMARY KEY REFERENCES libraries(id), status TEXT NOT NULL, discovered INTEGER NOT NULL DEFAULT 0, probed INTEGER NOT NULL DEFAULT 0, reused INTEGER NOT NULL DEFAULT 0, errors_json TEXT NOT NULL DEFAULT '[]') STRICT;

