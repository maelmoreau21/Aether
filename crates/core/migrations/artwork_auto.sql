-- Automatic artwork: one row per work the background filler has already handled in
-- a given mode ('single' = one image of each enabled kind, 'pool' = multi-image pool),
-- so a library pass never asks the providers twice for the same work.
CREATE TABLE IF NOT EXISTS artwork_auto_attempts (
 work_id TEXT NOT NULL,
 mode TEXT NOT NULL,
 attempted_at BIGINT NOT NULL,
 PRIMARY KEY(work_id, mode)
);
