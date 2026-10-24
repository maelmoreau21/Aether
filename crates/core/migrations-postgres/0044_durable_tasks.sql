CREATE TABLE durable_tasks (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    owner TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES media_nodes(id),
    category TEXT NOT NULL DEFAULT 'scan' CHECK(category IN ('scan','analyze','prepare','image')),
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
    UNIQUE(owner,idempotency_key)
);
CREATE INDEX durable_tasks_dispatch ON durable_tasks(node_id,status,priority DESC,created_at,id);
CREATE INDEX durable_tasks_expiry ON durable_tasks(status,lease_until);
CREATE INDEX durable_tasks_library ON durable_tasks(library_id,status);
CREATE INDEX durable_tasks_recent ON durable_tasks(created_at DESC,id);
CREATE INDEX durable_tasks_pending ON durable_tasks(status) WHERE status IN ('queued','running','paused');
CREATE INDEX durable_tasks_leased ON durable_tasks(lease_until,id) WHERE lease_until IS NOT NULL;
CREATE INDEX durable_tasks_leased_node ON durable_tasks(node_id) WHERE lease_until IS NOT NULL;
CREATE INDEX durable_tasks_leased_library ON durable_tasks(library_id) WHERE lease_until IS NOT NULL;
CREATE INDEX durable_tasks_ready ON durable_tasks(status,available_at,node_id) WHERE status='queued';
CREATE TABLE active_scan_runs (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    scan_id TEXT NOT NULL,
    node_id TEXT NOT NULL REFERENCES media_nodes(id)
);
ALTER TABLE scheduled_scans ADD COLUMN last_request_key TEXT NOT NULL DEFAULT '';
CREATE TABLE scan_batch_receipts (
    scan_id TEXT NOT NULL,
    sequence BIGINT NOT NULL CHECK(sequence>=0),
    PRIMARY KEY(scan_id,sequence)
);
