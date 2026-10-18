ALTER TABLE inventory_works ADD COLUMN added_at BIGINT;
ALTER TABLE inventory_works ALTER COLUMN added_at SET DEFAULT (EXTRACT(EPOCH FROM CURRENT_TIMESTAMP)::BIGINT);
CREATE INDEX inventory_works_added_at ON inventory_works(added_at, id);
