-- Administrator triage of media reports: priority proposed by the Signalements
-- extension at creation (reports.classify), adjustable by an administrator.
-- Suggested checks are advisory codes; nothing here schedules a repair.
CREATE TABLE IF NOT EXISTS media_report_triage (
 report_id TEXT PRIMARY KEY REFERENCES media_reports(id) ON DELETE CASCADE,
 priority TEXT NOT NULL CHECK(priority IN ('high','normal','low')),
 code TEXT, checks_json TEXT NOT NULL DEFAULT '[]', updated_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS media_reports_queue ON media_reports(status,updated_at);
CREATE INDEX IF NOT EXISTS media_requests_queue ON media_requests(status,created_at);
