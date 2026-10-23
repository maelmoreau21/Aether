ALTER TABLE extensions ADD COLUMN all_libraries INTEGER NOT NULL DEFAULT 0 CHECK(all_libraries IN (0,1));
UPDATE extensions SET all_libraries=1 WHERE NOT EXISTS (SELECT 1 FROM extension_grants g WHERE g.extension_id=extensions.id);
CREATE TABLE extension_avatar_packs (
 id TEXT PRIMARY KEY, extension_id TEXT NOT NULL REFERENCES extensions(id) ON DELETE CASCADE,
 name TEXT NOT NULL, source TEXT NOT NULL
);
CREATE TABLE extension_avatar_images (
 id TEXT PRIMARY KEY, pack_id TEXT NOT NULL REFERENCES extension_avatar_packs(id) ON DELETE CASCADE,
 name TEXT NOT NULL, image_data BYTEA NOT NULL
);
