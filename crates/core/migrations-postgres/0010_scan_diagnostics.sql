-- Migration additive 0010: Compteurs totaux d'anomalies de scan pour diagnostics administrateur.
ALTER TABLE scan_jobs ADD COLUMN errors_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE scan_jobs ADD COLUMN warnings_count BIGINT NOT NULL DEFAULT 0;
