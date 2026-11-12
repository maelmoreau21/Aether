-- Rows every database starts with. Runs once after the schema, in this order.

INSERT INTO statistics_settings(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
INSERT INTO statistics_wrapped_settings(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
INSERT INTO system_task_schedules(id,name,description,category,interval_minutes,enabled,next_due_at)
VALUES('clean-statistics','Nettoyer les statistiques expirées','Purge bornée des événements et bilans expirés ; ferme les lecteurs sans nouvelles.','Maintenance',5,1,unixepoch()+300)
ON CONFLICT(id) DO NOTHING;
INSERT INTO statistics_visibility(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
INSERT INTO acquisition_settings(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING;
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
INSERT INTO request_default_policy(singleton,policy_json) VALUES(1,'{}') ON CONFLICT(singleton) DO NOTHING;
