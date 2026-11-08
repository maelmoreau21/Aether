-- SQLite only, applied once (versioned step 0001_asset_availability_insert).
-- Same trigger as schema.sql: `+has_available_asset` keeps the planner on the primary key.
-- Without it, inventory_works_avail_sort was chosen and every inserted asset visited every
-- unavailable work, so publishing slowed down with offline libraries and removed files.
DROP TRIGGER IF EXISTS inventory_assets_avail_insert;
CREATE TRIGGER inventory_assets_avail_insert AFTER INSERT ON inventory_assets
WHEN NEW.available = 1 BEGIN
    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id = NEW.source_id
    ) AND +has_available_asset = 0;

    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id = NEW.source_id
    ) AND +has_available_asset = 0;
END;
