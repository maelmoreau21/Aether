ALTER TABLE personal_lists ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private' CHECK(visibility IN ('private','shared','public'));
ALTER TABLE personal_list_items ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
UPDATE personal_list_items SET position=(SELECT COUNT(*) FROM personal_list_items other WHERE other.list_id=personal_list_items.list_id AND (other.added_at<personal_list_items.added_at OR (other.added_at=personal_list_items.added_at AND other.work_id<personal_list_items.work_id)));
CREATE TABLE playlist_music_items(list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,track_id TEXT NOT NULL REFERENCES music_tracks(id) ON DELETE CASCADE,position INTEGER NOT NULL,PRIMARY KEY(list_id,track_id)) STRICT;
CREATE TABLE playlist_shares(list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,PRIMARY KEY(list_id,user_id)) STRICT;
CREATE INDEX playlist_shares_user ON playlist_shares(user_id,list_id);
CREATE TABLE playlist_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1),allow_public INTEGER NOT NULL DEFAULT 0 CHECK(allow_public IN (0,1)),allow_sharing INTEGER NOT NULL DEFAULT 0 CHECK(allow_sharing IN (0,1))) STRICT;
INSERT INTO playlist_policy(singleton) VALUES(1);
ALTER TABLE music_tracks ADD COLUMN lyrics TEXT NOT NULL DEFAULT '';
