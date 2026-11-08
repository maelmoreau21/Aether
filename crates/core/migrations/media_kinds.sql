-- Media types denied to a user or to every member of a group (films, series,
-- music). The most restrictive rule wins; library grants stay unchanged.
CREATE TABLE IF NOT EXISTS user_media_kind_denials (
 user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('films','series','music')),
 PRIMARY KEY(user_id,kind)
);
CREATE TABLE IF NOT EXISTS group_media_kind_denials (
 group_id TEXT NOT NULL REFERENCES user_groups(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('films','series','music')),
 PRIMARY KEY(group_id,kind)
);
