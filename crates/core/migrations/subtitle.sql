-- Additive, idempotent schema: no catalogue rebuild or media writes.
CREATE TABLE IF NOT EXISTS subtitle_searches (
 id TEXT PRIMARY KEY, actor_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
 provider_id TEXT NOT NULL, generation TEXT NOT NULL, fingerprint TEXT NOT NULL,
 result_json TEXT NOT NULL, expires_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS subtitle_search_expiry ON subtitle_searches(expires_at);
CREATE TABLE IF NOT EXISTS subtitle_downloads (
 id TEXT PRIMARY KEY, asset_id TEXT NOT NULL REFERENCES inventory_assets(id) ON DELETE CASCADE,
 provider_id TEXT NOT NULL, file_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
 track_json TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','completed','failed')),
 error_code TEXT, created_at BIGINT NOT NULL, content TEXT,
 UNIQUE(asset_id,provider_id,file_id)
);
CREATE INDEX IF NOT EXISTS subtitle_download_asset ON subtitle_downloads(asset_id,status);
CREATE TABLE IF NOT EXISTS subtitle_provider_limits (
 provider_id TEXT PRIMARY KEY, next_allowed_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS subtitle_download_limits (
 provider_id TEXT PRIMARY KEY, remaining BIGINT, reset_time_utc TEXT,
 next_allowed_at BIGINT NOT NULL
);
