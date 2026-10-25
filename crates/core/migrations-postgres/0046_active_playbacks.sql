CREATE TABLE active_playbacks (
 id TEXT PRIMARY KEY,
 session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
 work_id TEXT NOT NULL,
 position_seconds DOUBLE PRECISION NOT NULL,
 duration_seconds DOUBLE PRECISION NOT NULL,
 paused INTEGER NOT NULL CHECK(paused IN (0,1)),
 updated_at BIGINT NOT NULL
);
CREATE INDEX active_playbacks_session ON active_playbacks(session_hash);
CREATE INDEX active_playbacks_updated ON active_playbacks(updated_at);
