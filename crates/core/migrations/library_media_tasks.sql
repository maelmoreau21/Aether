-- Per-library media sub-tasks of the scheduler (previews, intros and credits, images).
-- One row per library and sub-task: when its last run was requested and by what (an
-- administrator or a scan). The work itself stays in `durable_tasks` and
-- `artwork_collection_jobs`; this only dates the run the scheduler reports.
CREATE TABLE IF NOT EXISTS library_media_task_runs (
 library_id TEXT NOT NULL,
 task TEXT NOT NULL CHECK(task IN ('trickplay','intros','artwork')),
 started_at BIGINT NOT NULL,
 origin TEXT NOT NULL DEFAULT 'manual' CHECK(origin IN ('manual','scan')),
 PRIMARY KEY(library_id, task)
);
