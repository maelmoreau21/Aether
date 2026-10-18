ALTER TABLE scan_staging_files ADD COLUMN subtitles_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scan_staging_files ADD COLUMN audios_json TEXT NOT NULL DEFAULT '[]';
CREATE TABLE inventory_audio (
    asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id) ON DELETE CASCADE,
    audios_json TEXT NOT NULL
);
CREATE TABLE inventory_subtitles (
    asset_id TEXT PRIMARY KEY REFERENCES inventory_assets(id) ON DELETE CASCADE,
    subtitles_json TEXT NOT NULL
);
