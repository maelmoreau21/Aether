-- Migration additive 0009: Staging PostgreSQL pour l'ingestion progressive du scanner.
-- Permet des transactions courtes par lots bornés avec backpressure sans verrouiller
-- le catalogue en cours de scan et garantit une publication strictement atomique.
CREATE TABLE IF NOT EXISTS scan_staging_files (
    scan_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    size BIGINT NOT NULL,
    modified_ns TEXT NOT NULL,
    mime TEXT NOT NULL,
    title TEXT NOT NULL,
    year BIGINT,
    identity_key TEXT NOT NULL,
    external_ids_json TEXT NOT NULL,
    probe_json TEXT,
    reused BIGINT NOT NULL,
    poster_path TEXT,
    poster_size BIGINT,
    poster_modified_ns TEXT,
    poster_mime TEXT,
    poster_priority BIGINT,
    editorial_outcome TEXT NOT NULL,
    editorial_json TEXT,
    editorial_path TEXT,
    editorial_priority BIGINT,
    PRIMARY KEY (scan_id, relative_path)
);

CREATE TABLE IF NOT EXISTS scan_staging_issues (
    scan_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    message TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS scan_staging_files_scan_reused ON scan_staging_files(scan_id, reused);
CREATE INDEX IF NOT EXISTS scan_staging_files_scan_library ON scan_staging_files(scan_id, library_id);
CREATE INDEX IF NOT EXISTS inventory_images_library_available ON inventory_images(library_id, available);
