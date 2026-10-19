CREATE TABLE work_feedback (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK(favorite IN (0,1)),
    rating INTEGER CHECK(rating BETWEEN 1 AND 5),
    PRIMARY KEY(user_id,work_id)
);
ALTER TABLE inventory_images ADD COLUMN role TEXT NOT NULL DEFAULT 'poster';
ALTER TABLE scan_staging_files ADD COLUMN artwork_json TEXT NOT NULL DEFAULT '{}';
