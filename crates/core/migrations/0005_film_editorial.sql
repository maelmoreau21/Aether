-- Editorial metadata is separate from the technical probe and stable media graph.
ALTER TABLE inventory_works ADD COLUMN editorial_json TEXT NOT NULL DEFAULT '{}';
CREATE TABLE inventory_editorial (
    asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id),
    fallback_json TEXT NOT NULL,
    nfo_json TEXT,
    nfo_path TEXT,
    nfo_priority INTEGER
) STRICT;
