CREATE TABLE extension_publishers (
    id TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    active BIGINT NOT NULL DEFAULT 1 CHECK(active IN (0,1))
);
CREATE TABLE extensions (
    id TEXT PRIMARY KEY,
    publisher_id TEXT NOT NULL REFERENCES extension_publishers(id),
    package_json TEXT NOT NULL,
    enabled BIGINT NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
    generation TEXT NOT NULL
);
CREATE TABLE extension_grants (
    extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    PRIMARY KEY(extension_id,library_id)
);
CREATE TABLE extension_runs (
    id TEXT PRIMARY KEY,
    extension_id TEXT NOT NULL,
    library_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    started_at BIGINT NOT NULL,
    duration_ms BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    result_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX extension_runs_recent ON extension_runs(started_at);
CREATE TABLE extension_audit (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL,
    extension_id TEXT NOT NULL,
    action TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
