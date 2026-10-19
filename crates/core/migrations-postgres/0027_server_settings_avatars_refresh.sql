CREATE TABLE server_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    settings_json TEXT NOT NULL
);
CREATE TABLE user_avatars (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    image_data BYTEA NOT NULL
);
ALTER TABLE scan_jobs ADD COLUMN refresh_mode TEXT NOT NULL DEFAULT 'incremental'
    CHECK(refresh_mode IN ('incremental','new_only','missing_metadata','full'));
