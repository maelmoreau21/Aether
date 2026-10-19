CREATE TABLE backup_schedule (
    singleton BIGINT PRIMARY KEY CHECK(singleton = 1),
    enabled BIGINT NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
    interval_minutes BIGINT NOT NULL DEFAULT 1440 CHECK(interval_minutes BETWEEN 60 AND 10080),
    next_due_at BIGINT NOT NULL DEFAULT 0,
    last_started_at BIGINT,
    last_completed_at BIGINT,
    last_outcome TEXT NOT NULL DEFAULT 'never' CHECK(last_outcome IN ('never','running','succeeded','failed','interrupted'))
);
INSERT INTO backup_schedule(singleton) VALUES(1);
