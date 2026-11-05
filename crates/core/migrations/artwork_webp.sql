-- Additive metadata for application, legacy and inventoried local artwork.
-- image_id deliberately has no FK: local images and legacy posters have distinct owners.
CREATE TABLE IF NOT EXISTS library_artwork_previews (
    library_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    width BIGINT NOT NULL,
    sha256 TEXT NOT NULL REFERENCES assets(sha256),
    PRIMARY KEY(library_id,kind,width),
    FOREIGN KEY(library_id,kind) REFERENCES library_artwork(library_id,kind) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS library_artwork_previews_asset ON library_artwork_previews(sha256);
CREATE TABLE IF NOT EXISTS artwork_annotations (
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    image_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    pinned BIGINT NOT NULL DEFAULT 0 CHECK(pinned IN (0,1)),
    source_sha256 TEXT,
    encoding TEXT,
    release_evidence TEXT,
    PRIMARY KEY(work_id,image_id)
);
CREATE INDEX IF NOT EXISTS artwork_source_digest ON artwork_annotations(source_sha256,kind,encoding);
CREATE UNIQUE INDEX IF NOT EXISTS artwork_one_pin ON artwork_annotations(work_id,kind) WHERE pinned=1;
CREATE TABLE IF NOT EXISTS image_migration_attempts (
    sha256 TEXT PRIMARY KEY,
    attempts BIGINT NOT NULL DEFAULT 0,
    retry_at BIGINT NOT NULL DEFAULT 0,
    outcome TEXT NOT NULL DEFAULT 'pending'
);
CREATE TABLE IF NOT EXISTS artwork_collection_jobs (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    owner TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    parameters TEXT NOT NULL,
    cursor_work TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','completed','paused','cancelled')),
    lease_token TEXT NOT NULL DEFAULT '',
    lease_until BIGINT NOT NULL DEFAULT 0,
    next_run_at BIGINT NOT NULL DEFAULT 0,
    processed BIGINT NOT NULL DEFAULT 0,
    downloaded BIGINT NOT NULL DEFAULT 0,
    duplicates BIGINT NOT NULL DEFAULT 0,
    skipped BIGINT NOT NULL DEFAULT 0,
    errors BIGINT NOT NULL DEFAULT 0,
    work_attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS artwork_one_collection ON artwork_collection_jobs(library_id) WHERE status IN ('queued','running','paused');
