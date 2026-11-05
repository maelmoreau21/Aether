CREATE TABLE IF NOT EXISTS media_reports (
 id TEXT PRIMARY KEY, user_id TEXT REFERENCES users(id) ON DELETE SET NULL, library_id TEXT REFERENCES libraries(id) ON DELETE SET NULL,
 work_id TEXT, track_id TEXT, asset_id TEXT, category TEXT NOT NULL, description TEXT NOT NULL,
 status TEXT NOT NULL, position_seconds DOUBLE PRECISION, end_seconds DOUBLE PRECISION, context_json TEXT NOT NULL DEFAULT '{}',
 created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, source_key TEXT UNIQUE, author_label TEXT
);
CREATE INDEX IF NOT EXISTS media_reports_owner ON media_reports(user_id,updated_at);
CREATE TABLE IF NOT EXISTS media_report_comments (
 id TEXT PRIMARY KEY, report_id TEXT NOT NULL REFERENCES media_reports(id) ON DELETE CASCADE,
 user_id TEXT REFERENCES users(id) ON DELETE SET NULL, author_label TEXT, message TEXT NOT NULL, created_at BIGINT NOT NULL, source_key TEXT UNIQUE
);
CREATE TABLE IF NOT EXISTS media_report_events (
 id TEXT PRIMARY KEY, report_id TEXT NOT NULL REFERENCES media_reports(id) ON DELETE CASCADE,
 user_id TEXT, kind TEXT NOT NULL, status TEXT NOT NULL, created_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS application_imports (
 id TEXT PRIMARY KEY, source_url TEXT NOT NULL, connection_json TEXT NOT NULL, status TEXT NOT NULL,
 phase TEXT NOT NULL, page_offset BIGINT NOT NULL DEFAULT 0, processed BIGINT NOT NULL DEFAULT 0,
 errors BIGINT NOT NULL DEFAULT 0, mappings_json TEXT NOT NULL DEFAULT '{}', preview_json TEXT NOT NULL,
 lease_token TEXT, lease_until BIGINT NOT NULL DEFAULT 0, updated_at BIGINT NOT NULL, last_error TEXT
);
