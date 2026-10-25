ALTER TABLE work_image_entries ADD COLUMN language TEXT;
ALTER TABLE work_image_entries ADD COLUMN provider TEXT;
ALTER TABLE work_image_entries ADD COLUMN fingerprint TEXT;
ALTER TABLE work_image_entries ADD COLUMN source_url_hash TEXT;
CREATE INDEX work_image_provider_source ON work_image_entries(work_id,provider,source_url_hash);
CREATE TABLE artwork_image_sources (work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,source_url_hash TEXT NOT NULL,image_id TEXT NOT NULL REFERENCES work_image_entries(id) ON DELETE CASCADE,PRIMARY KEY(work_id,source_url_hash));
