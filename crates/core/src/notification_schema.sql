-- Additive notification outbox; contains no filesystem/media paths.
CREATE TABLE IF NOT EXISTS notification_channels (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, recipient_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('email','ntfy','gotify','webhook')), enabled BIGINT NOT NULL,
 config_json TEXT NOT NULL, generation TEXT NOT NULL, created_at BIGINT NOT NULL,
 next_allowed_at BIGINT NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS notification_preferences (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 enabled BIGINT NOT NULL DEFAULT 0, scans BIGINT NOT NULL DEFAULT 0, tasks BIGINT NOT NULL DEFAULT 0
);
-- Separate additive subscription preserves databases created before content events.
CREATE TABLE IF NOT EXISTS notification_content_preferences (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 enabled BIGINT NOT NULL DEFAULT 0, since_at BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS application_notification_preferences (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('requests','reports')), enabled BIGINT NOT NULL DEFAULT 0,
 since_at BIGINT NOT NULL, PRIMARY KEY(user_id,kind)
);
CREATE TABLE IF NOT EXISTS notification_events (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL, task_id TEXT NOT NULL, library_id TEXT,
 occurred_at BIGINT NOT NULL, source_attempt BIGINT NOT NULL, source_status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS notification_event_retention ON notification_events(occurred_at);
CREATE TABLE IF NOT EXISTS notification_deliveries (
 id TEXT PRIMARY KEY, event_id TEXT NOT NULL REFERENCES notification_events(id) ON DELETE CASCADE,
 channel_id TEXT NOT NULL REFERENCES notification_channels(id) ON DELETE CASCADE,
 generation TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','sending','sent','failed','suppressed')),
 attempt BIGINT NOT NULL DEFAULT 0, available_at BIGINT NOT NULL, lease_until BIGINT,
 lease_token TEXT, error_code TEXT, completed_at BIGINT, UNIQUE(event_id,channel_id)
);
CREATE INDEX IF NOT EXISTS notification_delivery_due ON notification_deliveries(status,available_at);
