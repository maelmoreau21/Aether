CREATE TABLE IF NOT EXISTS acquisition_settings (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), auto_approve BIGINT NOT NULL DEFAULT 0 CHECK(auto_approve IN (0,1)), max_pending_per_user BIGINT NOT NULL DEFAULT 20
);
INSERT INTO acquisition_settings(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
CREATE TABLE IF NOT EXISTS acquisition_servers (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('radarr','sonarr','lidarr')),
 base_url TEXT NOT NULL, api_key TEXT NOT NULL, allowed_ips_json TEXT NOT NULL,
 library_id TEXT NOT NULL REFERENCES libraries(id), root_folder TEXT NOT NULL, quality_profile_id BIGINT NOT NULL,
 metadata_profile_id BIGINT, is_default BIGINT NOT NULL DEFAULT 0 CHECK(is_default IN (0,1)), enabled BIGINT NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)), generation TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS media_requests (
 id TEXT PRIMARY KEY, user_id TEXT REFERENCES users(id) ON DELETE SET NULL, server_id TEXT REFERENCES acquisition_servers(id),
 library_id TEXT NOT NULL REFERENCES libraries(id), kind TEXT NOT NULL, title TEXT NOT NULL, external_id TEXT NOT NULL, external_provider TEXT NOT NULL,
 input_json TEXT NOT NULL, status TEXT NOT NULL, error_code TEXT, remote_id TEXT, work_id TEXT,
 created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, imported BIGINT NOT NULL DEFAULT 0 CHECK(imported IN (0,1)),
 source_key TEXT UNIQUE, author_label TEXT, source_status TEXT
);
CREATE INDEX IF NOT EXISTS media_requests_owner ON media_requests(user_id,created_at);
CREATE INDEX IF NOT EXISTS media_requests_identity ON media_requests(server_id,kind,external_id,status);
CREATE TABLE IF NOT EXISTS acquisition_jobs (
 request_id TEXT PRIMARY KEY REFERENCES media_requests(id) ON DELETE CASCADE, available_at BIGINT NOT NULL,
 lease_token TEXT, lease_until BIGINT NOT NULL DEFAULT 0, attempts BIGINT NOT NULL DEFAULT 0, phase TEXT NOT NULL DEFAULT 'initial'
);
CREATE TABLE IF NOT EXISTS acquisition_events (
 id TEXT PRIMARY KEY, request_id TEXT NOT NULL REFERENCES media_requests(id) ON DELETE CASCADE,
 actor_id TEXT, status TEXT NOT NULL, code TEXT, created_at BIGINT NOT NULL
);
