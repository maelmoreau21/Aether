-- Derived objects: views and performance indexes, never data.
-- Applied after the schema on a new database AND on every start of an existing one
-- (initialize_fresh_schema), so a performance fix reaches databases already in service.
-- Every statement must be idempotent and valid on both SQLite and PostgreSQL.

DROP VIEW IF EXISTS effective_library_permissions;
DROP VIEW IF EXISTS effective_user_group_members;

CREATE VIEW effective_user_group_members AS
SELECT group_id,user_id FROM user_group_members
WHERE group_id NOT IN ('builtin-role-user','builtin-role-administrator')
UNION SELECT 'builtin-role-user',id FROM users WHERE role='user'
UNION SELECT 'builtin-role-administrator',id FROM users WHERE role='administrator';
-- Every user-facing query reads this view: denied media types never leak
-- through catalogue, search, details, playback, music, lists or SyncPlay.
-- NOT MATERIALIZED: callers always filter on one user_id; without it PostgreSQL (and
-- SQLite) build the grants of every account before filtering, on every request.
CREATE VIEW effective_library_permissions AS
WITH account_grants AS NOT MATERIALIZED (
    SELECT library_id,user_id FROM library_permissions
    UNION SELECT l.library_id,m.user_id FROM user_group_libraries l JOIN effective_user_group_members m ON m.group_id=l.group_id
    UNION SELECT l.library_id,u.id FROM user_group_libraries l JOIN user_groups g ON g.id=l.group_id CROSS JOIN users u WHERE g.is_default=1
), direct_grants AS NOT MATERIALIZED (
    SELECT library_id,user_id FROM account_grants
    -- A profile sees what its owner sees; an administrator's access is implicit, so
    -- its profiles get every library administrators have not been excluded from.
    UNION SELECT a.library_id,p.user_id FROM account_profiles p JOIN account_grants a ON a.user_id=p.owner_id WHERE p.user_id<>p.owner_id
    UNION SELECT lib.id,p.user_id FROM account_profiles p JOIN users o ON o.id=p.owner_id AND o.role='administrator' CROSS JOIN libraries lib
      WHERE p.user_id<>p.owner_id AND COALESCE(lib.options_json,'') NOT LIKE '%adminAccess__false%'
), granted AS NOT MATERIALIZED (
    SELECT d.library_id,d.user_id FROM direct_grants d WHERE NOT EXISTS(SELECT 1 FROM library_folders f WHERE f.library_id=d.library_id)
    UNION SELECT f.library_id,d.user_id FROM direct_grants d JOIN library_folders f ON f.parent_id=d.library_id AND f.active=1
)
SELECT g.library_id,g.user_id FROM granted g JOIN libraries lib ON lib.id=g.library_id
WHERE NOT EXISTS(SELECT 1 FROM user_media_kind_denials x WHERE x.user_id=g.user_id AND x.kind=lib.kind)
AND NOT EXISTS(SELECT 1 FROM group_media_kind_denials x JOIN effective_user_group_members m ON m.group_id=x.group_id WHERE m.user_id=g.user_id AND x.kind=lib.kind)
AND NOT EXISTS(SELECT 1 FROM group_media_kind_denials x JOIN user_groups dg ON dg.id=x.group_id WHERE dg.is_default=1 AND x.kind=lib.kind);

-- Catalogue ordering indexes for fast index-only scans on `added-desc` and `year-desc`.
CREATE INDEX IF NOT EXISTS inventory_works_avail_added_desc
    ON inventory_works(kind, has_available_asset, added_at DESC, display_title_sort ASC, id ASC);
CREATE INDEX IF NOT EXISTS inventory_works_avail_year_desc
    ON inventory_works(kind, has_available_asset, effective_year DESC, display_title_sort ASC, id ASC);
