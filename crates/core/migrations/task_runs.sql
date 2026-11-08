-- Fences for durable non-scan tasks (analysis, preparation, images, trickplay).
-- They are tracked per run, never through the library scan status, so a
-- post-scan preview cannot rewrite a completed scan summary or fence it out.
CREATE TABLE IF NOT EXISTS active_task_runs (
 run_id TEXT PRIMARY KEY NOT NULL,
 library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
 node_id TEXT NOT NULL,
 status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','completed','error')),
 error TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS active_task_runs_library ON active_task_runs(library_id);
