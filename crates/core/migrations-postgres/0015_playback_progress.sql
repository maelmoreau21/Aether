CREATE TABLE playback_progress (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    position_seconds DOUBLE PRECISION NOT NULL CHECK(position_seconds >= 0),
    duration_seconds DOUBLE PRECISION NOT NULL CHECK(duration_seconds > 0),
    watched INTEGER NOT NULL CHECK(watched IN (0,1)),
    revision BIGINT NOT NULL CHECK(revision > 0),
    updated_at BIGINT NOT NULL,
    PRIMARY KEY(user_id,work_id)
);
CREATE INDEX playback_progress_recent ON playback_progress(user_id,updated_at DESC);
