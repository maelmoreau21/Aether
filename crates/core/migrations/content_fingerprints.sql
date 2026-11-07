-- Partial content fingerprints (size + sampled BLAKE3, see crates/node/src/fingerprint.rs).
-- A hint used only to recognise a file that was renamed or moved inside one library so its
-- work, manual corrections, translations and prepared caches follow it. Never an identity:
-- rows carry no authority beyond "candidate", and losing the whole table only loses the hint.
-- Additive and idempotent on SQLite and PostgreSQL; old Nodes and old rows keep working.
CREATE TABLE IF NOT EXISTS inventory_asset_fingerprints (
 asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id) ON DELETE CASCADE,
 library_id TEXT NOT NULL,
 size BIGINT NOT NULL,
 modified_ns TEXT NOT NULL,
 fingerprint TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS inventory_asset_fingerprints_lookup ON inventory_asset_fingerprints(library_id, size, fingerprint);
-- Fingerprints reported by a Node for the staged scan, consumed and deleted by its publication.
CREATE TABLE IF NOT EXISTS scan_staging_fingerprints (
 scan_id TEXT NOT NULL,
 relative_path TEXT NOT NULL,
 library_id TEXT NOT NULL,
 fingerprint TEXT NOT NULL,
 PRIMARY KEY(scan_id, relative_path)
);
