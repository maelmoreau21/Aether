CREATE TABLE scheduled_scans (
    library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
    interval_minutes BIGINT NOT NULL CHECK(interval_minutes BETWEEN 5 AND 10080),
    enabled BIGINT NOT NULL CHECK(enabled IN (0,1)),
    next_run_at BIGINT NOT NULL,
    last_run_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never'
);
CREATE INDEX scheduled_scans_due ON scheduled_scans(enabled,next_run_at);
