-- Core maintenance uses the same durable queue; local is the existing executor identity.
ALTER TABLE durable_tasks ALTER COLUMN library_id DROP NOT NULL;
ALTER TABLE durable_tasks DROP CONSTRAINT durable_tasks_category_check;
ALTER TABLE durable_tasks ADD CONSTRAINT durable_tasks_category_check CHECK(category IN ('scan','analyze','prepare','image','system'));
ALTER TABLE durable_tasks ADD CONSTRAINT durable_tasks_scope_check CHECK((category='system' AND library_id IS NULL AND node_id='local') OR (category!='system' AND library_id IS NOT NULL));
CREATE INDEX durable_tasks_system_ready ON durable_tasks(priority DESC,created_at,id) WHERE category='system' AND status='queued';
ALTER TABLE system_task_schedules DROP CONSTRAINT system_task_schedules_last_outcome_check;
ALTER TABLE system_task_schedules ADD CONSTRAINT system_task_schedules_last_outcome_check CHECK(last_outcome IN ('never','queued','running','succeeded','failed','interrupted'));

-- One-time recovery of pre-migration in-memory system executions.
UPDATE system_task_schedules SET last_outcome='interrupted' WHERE last_outcome='running' AND NOT EXISTS(SELECT 1 FROM durable_tasks t WHERE t.category='system' AND t.target_id=system_task_schedules.id AND t.status IN ('queued','running','paused'));
