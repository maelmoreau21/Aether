CREATE TABLE user_groups (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE CHECK(length(name) BETWEEN 1 AND 80));
CREATE TABLE user_group_members (group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, PRIMARY KEY(group_id,user_id));
CREATE INDEX user_group_members_user ON user_group_members(user_id);
CREATE TABLE user_group_libraries (group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE, PRIMARY KEY(group_id,library_id));
CREATE VIEW effective_library_permissions AS SELECT library_id,user_id FROM library_permissions UNION SELECT l.library_id,m.user_id FROM user_group_libraries l JOIN user_group_members m ON m.group_id=l.group_id;
