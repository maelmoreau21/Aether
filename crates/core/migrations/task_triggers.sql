-- Jellyfin-style task triggers shared by system tasks, library scans and the backup.
-- Additive and idempotent: legacy interval rows are converted once per task, and the legacy
-- `next_due_at` / `next_run_at` columns remain the computed "earliest next run" cache.
CREATE TABLE IF NOT EXISTS task_triggers (
 id TEXT PRIMARY KEY,
 task_kind TEXT NOT NULL CHECK(task_kind IN ('system','scan','backup')),
 task_id TEXT NOT NULL,
 trigger_type TEXT NOT NULL CHECK(trigger_type IN ('daily','weekly','interval','startup')),
 time_of_day_minutes BIGINT CHECK(time_of_day_minutes IS NULL OR time_of_day_minutes BETWEEN 0 AND 1439),
 day_of_week BIGINT CHECK(day_of_week IS NULL OR day_of_week BETWEEN 0 AND 6),
 interval_minutes BIGINT CHECK(interval_minutes IS NULL OR interval_minutes BETWEEN 1 AND 10080),
 max_runtime_seconds BIGINT CHECK(max_runtime_seconds IS NULL OR max_runtime_seconds BETWEEN 60 AND 604800),
 enabled BIGINT NOT NULL DEFAULT 1 CHECK(enabled IN (0,1)),
 next_fire_at BIGINT,
 created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS task_triggers_task ON task_triggers(task_kind,task_id);
-- One row per task once its legacy interval has been converted (never converted twice).
CREATE TABLE IF NOT EXISTS task_trigger_migrations (
 task_kind TEXT NOT NULL,
 task_id TEXT NOT NULL,
 migrated_at BIGINT NOT NULL,
 PRIMARY KEY(task_kind,task_id)
);
INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at)
SELECT 'legacy-system-' || s.id,'system',s.id,'interval',s.interval_minutes,1,s.next_due_at,unixepoch()
FROM system_task_schedules s
WHERE s.enabled=1 AND NOT EXISTS(SELECT 1 FROM task_trigger_migrations m WHERE m.task_kind='system' AND m.task_id=s.id)
ON CONFLICT(id) DO NOTHING;
INSERT INTO task_trigger_migrations(task_kind,task_id,migrated_at)
SELECT 'system',s.id,unixepoch() FROM system_task_schedules s WHERE s.id<>''
ON CONFLICT(task_kind,task_id) DO NOTHING;
INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at)
SELECT 'legacy-scan-' || s.library_id,'scan',s.library_id,'interval',s.interval_minutes,1,s.next_run_at,unixepoch()
FROM scheduled_scans s
WHERE s.enabled=1 AND NOT EXISTS(SELECT 1 FROM task_trigger_migrations m WHERE m.task_kind='scan' AND m.task_id=s.library_id)
ON CONFLICT(id) DO NOTHING;
INSERT INTO task_trigger_migrations(task_kind,task_id,migrated_at)
SELECT 'scan',s.library_id,unixepoch() FROM scheduled_scans s WHERE s.library_id<>''
ON CONFLICT(task_kind,task_id) DO NOTHING;
INSERT INTO task_triggers(id,task_kind,task_id,trigger_type,interval_minutes,enabled,next_fire_at,created_at)
SELECT 'legacy-backup','backup','backup','interval',b.interval_minutes,1,b.next_due_at,unixepoch()
FROM backup_schedule b
WHERE b.enabled=1 AND NOT EXISTS(SELECT 1 FROM task_trigger_migrations m WHERE m.task_kind='backup' AND m.task_id='backup')
ON CONFLICT(id) DO NOTHING;
INSERT INTO task_trigger_migrations(task_kind,task_id,migrated_at)
SELECT 'backup','backup',unixepoch() FROM backup_schedule b WHERE b.singleton=1
ON CONFLICT(task_kind,task_id) DO NOTHING;
