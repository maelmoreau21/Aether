ALTER TABLE inventory_works ADD COLUMN manual_json TEXT;
ALTER TABLE inventory_works ADD COLUMN minimum_age BIGINT CHECK(minimum_age BETWEEN 0 AND 21);
CREATE TABLE parental_policies (user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,maximum_age BIGINT NOT NULL CHECK(maximum_age BETWEEN 0 AND 21),allow_unrated BIGINT NOT NULL CHECK(allow_unrated IN (0,1)));
CREATE TABLE work_posters (work_id TEXT PRIMARY KEY REFERENCES inventory_works(id) ON DELETE CASCADE,image_data BYTEA NOT NULL);
CREATE TABLE personal_lists (id TEXT PRIMARY KEY,user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 100));
CREATE TABLE personal_list_items (list_id TEXT NOT NULL REFERENCES personal_lists(id) ON DELETE CASCADE,work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,added_at BIGINT NOT NULL DEFAULT(EXTRACT(EPOCH FROM NOW())::BIGINT),PRIMARY KEY(list_id,work_id));
CREATE TABLE media_groups (id TEXT PRIMARY KEY,label TEXT NOT NULL);
CREATE TABLE media_group_members (work_id TEXT PRIMARY KEY REFERENCES inventory_works(id) ON DELETE CASCADE,group_id TEXT NOT NULL REFERENCES media_groups(id) ON DELETE CASCADE);
