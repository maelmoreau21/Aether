-- Historical insertion dates are unknown. Never fabricate them during migration.
ALTER TABLE inventory_works ADD COLUMN added_at INTEGER;
CREATE INDEX inventory_works_added_at ON inventory_works(added_at, id);
CREATE TRIGGER inventory_works_added_at_insert AFTER INSERT ON inventory_works
WHEN NEW.added_at IS NULL BEGIN
  UPDATE inventory_works SET added_at=unixepoch() WHERE id=NEW.id;
END;
