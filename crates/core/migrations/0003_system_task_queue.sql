-- Extend the existing queue to Core maintenance, preserving every referencing row.

CREATE TABLE migration_system_durable_tasks AS SELECT * FROM durable_tasks;

CREATE TABLE migration_system_automation_task_links AS SELECT * FROM automation_task_links;

CREATE TABLE migration_system_automation_dependencies AS SELECT * FROM automation_dependencies;

CREATE TABLE migration_system_automation_results AS SELECT * FROM automation_results;

DROP TABLE automation_task_links;

DROP TABLE automation_dependencies;

DROP TABLE automation_results;

DROP TABLE durable_tasks;

CREATE TABLE durable_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT REFERENCES libraries(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES media_nodes(id),
    category TEXT NOT NULL DEFAULT 'scan' CHECK(category IN ('scan','analyze','prepare','image','system')),
    target_id TEXT NOT NULL DEFAULT '',
    component TEXT NOT NULL DEFAULT '',
    width BIGINT NOT NULL DEFAULT 640 CHECK(width IN (320,640,1280)),
    result_json TEXT NOT NULL DEFAULT '{}',
    refresh_mode TEXT NOT NULL DEFAULT 'incremental' CHECK(refresh_mode IN ('incremental','new_only','missing_metadata','full')),
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','running','paused','completed','cancelled','error')),
    priority BIGINT NOT NULL DEFAULT 100 CHECK(priority BETWEEN 0 AND 1000),
    attempt BIGINT NOT NULL DEFAULT 0 CHECK(attempt>=0),
    failure_count BIGINT NOT NULL DEFAULT 0 CHECK(failure_count>=0),
    max_attempts BIGINT NOT NULL DEFAULT 3 CHECK(max_attempts BETWEEN 1 AND 1000000),
    progress BIGINT NOT NULL DEFAULT 0 CHECK(progress BETWEEN 0 AND 100),
    processed_items BIGINT NOT NULL DEFAULT 0 CHECK(processed_items>=0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    started_at BIGINT,
    finished_at BIGINT,
    lease_until BIGINT,
    available_at BIGINT NOT NULL,
    error TEXT NOT NULL DEFAULT '',
    CHECK((category='system' AND library_id IS NULL AND node_id='local') OR (category!='system' AND library_id IS NOT NULL)),
    UNIQUE(owner,idempotency_key)
);

INSERT INTO durable_tasks SELECT * FROM migration_system_durable_tasks;

CREATE INDEX durable_tasks_dispatch ON durable_tasks(node_id,status,priority DESC,created_at,id);

CREATE INDEX durable_tasks_expiry ON durable_tasks(status,lease_until);

CREATE INDEX durable_tasks_library ON durable_tasks(library_id,status);

CREATE INDEX durable_tasks_recent ON durable_tasks(created_at DESC,id);

CREATE INDEX durable_tasks_pending ON durable_tasks(status) WHERE status IN ('queued','running','paused');

CREATE INDEX durable_tasks_leased ON durable_tasks(lease_until,id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_leased_node ON durable_tasks(node_id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_leased_library ON durable_tasks(library_id) WHERE lease_until IS NOT NULL;

CREATE INDEX durable_tasks_ready ON durable_tasks(node_id,available_at) WHERE status='queued' AND lease_until IS NULL;

CREATE INDEX durable_tasks_system_ready ON durable_tasks(priority DESC,created_at,id) WHERE category='system' AND status='queued';

CREATE TABLE automation_task_links (
 task_id TEXT PRIMARY KEY REFERENCES durable_tasks(id) ON DELETE CASCADE,
 event_id TEXT NOT NULL REFERENCES automation_events(id), extension_id TEXT NOT NULL,
 operation TEXT NOT NULL, fingerprint TEXT NOT NULL, generation TEXT NOT NULL, provider TEXT NOT NULL,
 UNIQUE(extension_id,fingerprint)
);

INSERT INTO automation_task_links SELECT * FROM migration_system_automation_task_links;

CREATE TABLE automation_dependencies (
 task_id TEXT NOT NULL REFERENCES durable_tasks(id) ON DELETE CASCADE,
 prerequisite_id TEXT NOT NULL REFERENCES durable_tasks(id) ON DELETE CASCADE,
 PRIMARY KEY(task_id,prerequisite_id), CHECK(task_id<>prerequisite_id)
);

INSERT INTO automation_dependencies SELECT * FROM migration_system_automation_dependencies;

CREATE TABLE automation_results (
 extension_id TEXT NOT NULL, asset_id TEXT NOT NULL, fingerprint TEXT NOT NULL,
 task_id TEXT NOT NULL REFERENCES durable_tasks(id), result_json TEXT NOT NULL, updated_at BIGINT NOT NULL,
 PRIMARY KEY(extension_id,asset_id)
);

INSERT INTO automation_results SELECT * FROM migration_system_automation_results;

CREATE INDEX automation_tasks_extension ON automation_task_links(extension_id,task_id);

CREATE INDEX automation_tasks_provider ON automation_task_links(provider,task_id);

DROP TABLE migration_system_automation_task_links;

DROP TABLE migration_system_automation_dependencies;

DROP TABLE migration_system_automation_results;

DROP TABLE migration_system_durable_tasks;

CREATE TABLE migration_system_schedules AS SELECT * FROM system_task_schedules;
DROP TABLE system_task_schedules;
CREATE TABLE system_task_schedules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    interval_minutes BIGINT NOT NULL CHECK(interval_minutes BETWEEN 5 AND 10080),
    enabled BIGINT NOT NULL CHECK(enabled IN (0,1)),
    next_due_at BIGINT NOT NULL,
    last_started_at BIGINT,
    last_completed_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never' CHECK(last_outcome IN ('never','queued','running','succeeded','failed','interrupted'))
, last_error_kind TEXT, last_error_scope TEXT, last_message TEXT);
INSERT INTO system_task_schedules SELECT * FROM migration_system_schedules;
DROP TABLE migration_system_schedules;

-- One-time recovery of pre-migration in-memory system executions.
UPDATE system_task_schedules SET last_outcome='interrupted' WHERE last_outcome='running' AND NOT EXISTS(SELECT 1 FROM durable_tasks t WHERE t.category='system' AND t.target_id=system_task_schedules.id AND t.status IN ('queued','running','paused'));
