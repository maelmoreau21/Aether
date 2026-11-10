-- "Who's watching?" profiles: each profile is a full user without credentials,
-- attached to the account that created it. The owner may have its own row
-- (user_id = owner_id) only to hold a PIN. Profiles inherit the owner's
-- library access through effective_library_permissions (role_groups.sql).
CREATE TABLE IF NOT EXISTS account_profiles (
 user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 pin_hash TEXT,
 created_at BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS account_profiles_owner ON account_profiles(owner_id);
