-- Keep physical scan inventories stable while grouping folders into one library.
CREATE TABLE library_folders (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    parent_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
    CHECK(library_id != parent_id)
);
CREATE INDEX library_folders_parent ON library_folders(parent_id, active);
DROP VIEW effective_library_permissions;
CREATE VIEW effective_library_permissions AS
WITH direct_grants AS (
    SELECT library_id,user_id FROM library_permissions
    UNION SELECT l.library_id,m.user_id FROM user_group_libraries l JOIN user_group_members m ON m.group_id=l.group_id
    UNION SELECT l.library_id,u.id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id CROSS JOIN users u WHERE g.is_default=TRUE
)
SELECT d.library_id,d.user_id FROM direct_grants d WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=d.library_id)
UNION SELECT f.library_id,d.user_id FROM direct_grants d JOIN library_folders f ON f.parent_id=d.library_id AND f.active=1;
