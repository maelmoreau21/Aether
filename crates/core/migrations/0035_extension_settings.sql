CREATE TABLE IF NOT EXISTS extension_settings (
    extension_id TEXT PRIMARY KEY REFERENCES extensions(id) ON DELETE CASCADE,
    settings_json TEXT NOT NULL DEFAULT '{}'
);
