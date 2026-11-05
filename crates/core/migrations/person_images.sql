-- Additive, idempotent cache of cast/crew portraits fetched from metadata providers.
-- Only the SHA-256 of the provider URL is stored; bytes live in the asset store.
CREATE TABLE IF NOT EXISTS person_images (
 source_hash TEXT PRIMARY KEY,
 sha256 TEXT REFERENCES assets(sha256),
 width BIGINT,
 height BIGINT,
 error_code TEXT,
 fetched_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS person_images_asset ON person_images(sha256);
