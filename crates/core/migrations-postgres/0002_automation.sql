-- Transactional event outbox and orchestration metadata; durable_tasks is the only work queue.
CREATE TABLE automation_events (
 id TEXT PRIMARY KEY, event_key TEXT NOT NULL UNIQUE, kind TEXT NOT NULL,
 library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 asset_id TEXT NOT NULL, fingerprint TEXT NOT NULL, payload_json TEXT NOT NULL,
 created_at BIGINT NOT NULL, dispatched_at BIGINT, cursor_extension TEXT NOT NULL DEFAULT ''
);
CREATE INDEX automation_events_pending ON automation_events(created_at,id) WHERE dispatched_at IS NULL;
CREATE TABLE automation_observations (
 asset_id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 fingerprint TEXT NOT NULL, revision BIGINT NOT NULL
);
CREATE TABLE automation_policies (
 extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
 library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 mode TEXT NOT NULL DEFAULT 'automatic' CHECK(mode IN ('automatic','manual','disabled','scheduled')),
 priority BIGINT NOT NULL DEFAULT 100 CHECK(priority BETWEEN 0 AND 1000),
 operation TEXT NOT NULL DEFAULT 'metadata.propose', config_revision TEXT NOT NULL DEFAULT '',
 algorithm_version TEXT NOT NULL DEFAULT '1', interval_seconds BIGINT NOT NULL DEFAULT 86400 CHECK(interval_seconds>=60),
 next_run_at BIGINT NOT NULL DEFAULT 0, max_concurrent BIGINT NOT NULL DEFAULT 1 CHECK(max_concurrent BETWEEN 1 AND 16),
 PRIMARY KEY(extension_id,library_id)
);
CREATE TABLE automation_task_links (
 task_id TEXT PRIMARY KEY REFERENCES durable_tasks(id) ON DELETE CASCADE,
 event_id TEXT NOT NULL REFERENCES automation_events(id), extension_id TEXT NOT NULL,
 operation TEXT NOT NULL, fingerprint TEXT NOT NULL, generation TEXT NOT NULL, provider TEXT NOT NULL,
 UNIQUE(extension_id,fingerprint)
);
CREATE INDEX automation_tasks_extension ON automation_task_links(extension_id,task_id);
CREATE INDEX automation_tasks_provider ON automation_task_links(provider,task_id);
CREATE TABLE automation_dependencies (
 task_id TEXT NOT NULL REFERENCES durable_tasks(id) ON DELETE CASCADE,
 prerequisite_id TEXT NOT NULL REFERENCES durable_tasks(id) ON DELETE CASCADE,
 PRIMARY KEY(task_id,prerequisite_id), CHECK(task_id<>prerequisite_id)
);
CREATE TABLE automation_results (
 extension_id TEXT NOT NULL, asset_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
 task_id TEXT NOT NULL REFERENCES durable_tasks(id), result_json TEXT NOT NULL, updated_at BIGINT NOT NULL,
 PRIMARY KEY(extension_id,asset_id)
);
CREATE TABLE automation_provider_limits (
 provider TEXT PRIMARY KEY, next_allowed_at BIGINT NOT NULL DEFAULT 0,
 min_interval_seconds BIGINT NOT NULL DEFAULT 1 CHECK(min_interval_seconds>=0),
 max_concurrent BIGINT NOT NULL DEFAULT 2 CHECK(max_concurrent BETWEEN 1 AND 16)
);
CREATE TABLE automation_sweeps (
 id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 extension_id TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL, cursor_asset TEXT NOT NULL DEFAULT '',
 created_at BIGINT NOT NULL, completed_at BIGINT
);
CREATE INDEX automation_sweeps_pending ON automation_sweeps(created_at,id) WHERE completed_at IS NULL;

-- Avoid the global status/time index causing a full 100k-row candidate scan
-- for EACH Node. Ordered per-Node dispatch already has its own covering index.
DROP INDEX durable_tasks_ready;
CREATE INDEX durable_tasks_ready ON durable_tasks(node_id,available_at) WHERE status='queued' AND lease_until IS NULL;
