ALTER TABLE libraries ADD COLUMN options_json TEXT NOT NULL DEFAULT '{}';
ALTER TABLE libraries ADD COLUMN next_metadata_refresh_at INTEGER NOT NULL DEFAULT 0;
CREATE TABLE user_media_policies (user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,allow_media_download INTEGER NOT NULL DEFAULT 1 CHECK(allow_media_download IN (0,1))) STRICT;
ALTER TABLE parental_policies ADD COLUMN blocked_tags_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE parental_policies ADD COLUMN access_schedule_json TEXT NOT NULL DEFAULT '[]';
CREATE TABLE parental_blocked_tags (user_id TEXT NOT NULL REFERENCES parental_policies(user_id) ON DELETE CASCADE,tag_json TEXT NOT NULL,PRIMARY KEY(user_id,tag_json)) STRICT;
CREATE TABLE library_artwork (library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,kind TEXT NOT NULL CHECK(kind IN ('poster','backdrop')),image_data BLOB NOT NULL,revision TEXT NOT NULL,PRIMARY KEY(library_id,kind)) STRICT;
