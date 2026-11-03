CREATE TABLE IF NOT EXISTS recommendation_preferences (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 enabled BIGINT NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
 exclude_watched BIGINT NOT NULL DEFAULT 1 CHECK(exclude_watched IN (0,1)),
 history_days BIGINT NOT NULL DEFAULT 180 CHECK(history_days BETWEEN 1 AND 3650)
);
