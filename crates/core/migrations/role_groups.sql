-- Role membership is derived from users, including accounts provisioned through
-- bootstrap, local administration and OIDC. Group membership never changes roles.
DROP VIEW IF EXISTS effective_library_permissions;
DROP VIEW IF EXISTS effective_user_group_members;
CREATE VIEW effective_user_group_members AS
SELECT group_id,user_id FROM user_group_members
WHERE group_id NOT IN ('builtin-role-user','builtin-role-administrator')
UNION SELECT 'builtin-role-user',id FROM users WHERE role='user'
UNION SELECT 'builtin-role-administrator',id FROM users WHERE role='administrator';

CREATE VIEW effective_library_permissions AS
WITH direct_grants AS (
    SELECT library_id,user_id FROM library_permissions
    UNION SELECT l.library_id,m.user_id FROM user_group_libraries l JOIN effective_user_group_members m ON m.group_id=l.group_id
    UNION SELECT l.library_id,u.id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id CROSS JOIN users u WHERE g.is_default=1
)
SELECT d.library_id,d.user_id FROM direct_grants d WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=d.library_id)
UNION SELECT f.library_id,d.user_id FROM direct_grants d JOIN library_folders f ON f.parent_id=d.library_id AND f.active=1;
