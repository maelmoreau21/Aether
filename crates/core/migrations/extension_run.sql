CREATE TABLE IF NOT EXISTS extension_run_owners (
 run_id TEXT PRIMARY KEY REFERENCES extension_runs(id) ON DELETE CASCADE,
 owner_id TEXT NOT NULL, fence_token TEXT NOT NULL,
 lease_until BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS extension_run_owner_expiry ON extension_run_owners(lease_until);
