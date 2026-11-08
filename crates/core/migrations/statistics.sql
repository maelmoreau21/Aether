-- Additive, shared SQLite/PostgreSQL schema. No media paths, IPs or credentials.
CREATE TABLE IF NOT EXISTS statistics_settings (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), enabled BIGINT NOT NULL DEFAULT 0,
 event_retention_days BIGINT NOT NULL DEFAULT 365, summary_retention_days BIGINT NOT NULL DEFAULT 0,
 first_enabled_at BIGINT, generation BIGINT NOT NULL DEFAULT 0
);
INSERT INTO statistics_settings(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
CREATE TABLE IF NOT EXISTS statistics_collection_periods (
 id TEXT PRIMARY KEY, started_at BIGINT NOT NULL, ended_at BIGINT
);
CREATE TABLE IF NOT EXISTS statistics_sessions (
 id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 owner_hash TEXT NOT NULL, generation BIGINT NOT NULL,
 media_id TEXT NOT NULL, canonical_id TEXT NOT NULL, asset_id TEXT NOT NULL,
 library_id TEXT NOT NULL, node_id TEXT NOT NULL, kind TEXT NOT NULL,
 title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, device TEXT NOT NULL,
 started_at BIGINT NOT NULL, updated_at BIGINT NOT NULL, ended_at BIGINT,
 state_json TEXT NOT NULL, seconds DOUBLE PRECISION NOT NULL DEFAULT 0,
 covered_seconds DOUBLE PRECISION NOT NULL DEFAULT 0, duration DOUBLE PRECISION NOT NULL DEFAULT 0,
 completed BIGINT NOT NULL DEFAULT 0, last_sequence BIGINT NOT NULL DEFAULT 0,
 end_reason TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS statistics_sessions_date ON statistics_sessions(started_at,id);
CREATE INDEX IF NOT EXISTS statistics_sessions_user_date ON statistics_sessions(user_id,started_at,id);
CREATE INDEX IF NOT EXISTS statistics_sessions_active ON statistics_sessions(ended_at,updated_at);
CREATE TABLE IF NOT EXISTS statistics_events (
 session_id TEXT NOT NULL REFERENCES statistics_sessions(id) ON DELETE CASCADE,
 sequence BIGINT NOT NULL, occurred_at BIGINT NOT NULL, received_at BIGINT NOT NULL,
 event_json TEXT NOT NULL, PRIMARY KEY(session_id,sequence)
);
CREATE INDEX IF NOT EXISTS statistics_events_retention ON statistics_events(received_at);
CREATE TABLE IF NOT EXISTS statistics_daily (
 session_id TEXT NOT NULL REFERENCES statistics_sessions(id) ON DELETE CASCADE,
 day BIGINT NOT NULL, seconds DOUBLE PRECISION NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,day)
);
CREATE TABLE IF NOT EXISTS statistics_dimensions (
 session_id TEXT NOT NULL REFERENCES statistics_sessions(id) ON DELETE CASCADE,
 dimension TEXT NOT NULL, label TEXT NOT NULL, seconds DOUBLE PRECISION NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,dimension,label)
);
-- Yearly recap visibility (JellyTrack wrappedVisible/period). A separate singleton
-- keeps this additive and idempotent on both engines without ALTER TABLE.
CREATE TABLE IF NOT EXISTS statistics_wrapped_settings (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), visible BIGINT NOT NULL DEFAULT 1,
 period_enabled BIGINT NOT NULL DEFAULT 0,
 start_month BIGINT NOT NULL DEFAULT 12, start_day BIGINT NOT NULL DEFAULT 1,
 end_month BIGINT NOT NULL DEFAULT 1, end_day BIGINT NOT NULL DEFAULT 31
);
INSERT INTO statistics_wrapped_settings(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
INSERT INTO system_task_schedules(id,name,description,category,interval_minutes,enabled,next_due_at)
VALUES('clean-statistics','Nettoyer les statistiques expirées','Purge bornée des événements et bilans expirés ; ferme les lecteurs sans nouvelles.','Maintenance',5,1,unixepoch()+300)
ON CONFLICT(id) DO NOTHING;

-- Personal visibility is separate from collection, disabled until explicitly allowed.
CREATE TABLE IF NOT EXISTS statistics_visibility (
 singleton BIGINT PRIMARY KEY CHECK(singleton=1), users_can_view BIGINT NOT NULL DEFAULT 0 CHECK(users_can_view IN (0,1))
);
INSERT INTO statistics_visibility(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
