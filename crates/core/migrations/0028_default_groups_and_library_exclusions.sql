ALTER TABLE user_groups ADD COLUMN is_default INTEGER NOT NULL DEFAULT 0;
ALTER TABLE libraries ADD COLUMN excluded_patterns TEXT NOT NULL DEFAULT '';
DROP VIEW IF EXISTS effective_library_permissions;
CREATE VIEW effective_library_permissions AS
    SELECT library_id, user_id FROM library_permissions
    UNION
    SELECT l.library_id, m.user_id FROM user_group_libraries l JOIN user_group_members m ON m.group_id=l.group_id
    UNION
    SELECT l.library_id, u.id AS user_id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id CROSS JOIN users u WHERE g.is_default=1;
