-- Objects that exist in the baseline `schema.sql` but were added to it after some databases were
-- created, and that no other step creates. `schema.sql` is never replayed on an existing database,
-- so without this file an older install silently lacks them. Additive and idempotent.
CREATE TABLE IF NOT EXISTS avatar_packs (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, source TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS avatar_images (
 id TEXT PRIMARY KEY, pack_id TEXT NOT NULL REFERENCES avatar_packs(id) ON DELETE CASCADE,
 name TEXT NOT NULL, sha256 TEXT NOT NULL REFERENCES assets(sha256)
);
CREATE INDEX IF NOT EXISTS avatar_images_asset ON avatar_images(sha256);
CREATE INDEX IF NOT EXISTS avatar_images_pack ON avatar_images(pack_id);
CREATE TABLE IF NOT EXISTS avatar_policy (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 allow_custom BIGINT NOT NULL CHECK(allow_custom IN (0,1)),
 allow_gravatar BIGINT NOT NULL CHECK(allow_gravatar IN (0,1)),
 assign_on_create BIGINT NOT NULL CHECK(assign_on_create IN (0,1))
);
CREATE INDEX IF NOT EXISTS extension_audit_retention ON extension_audit(created_at,id);
