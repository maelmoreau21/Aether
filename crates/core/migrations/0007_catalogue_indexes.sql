-- Catalogue performance: Unicode sort/search column and relational graph indexes.
ALTER TABLE inventory_works ADD COLUMN title_sort TEXT NOT NULL DEFAULT '';
UPDATE inventory_works SET title_sort = lower(title);

CREATE INDEX inventory_works_library_sort ON inventory_works(library_id, title_sort, id);
CREATE INDEX inventory_works_title_sort ON inventory_works(title_sort, id);

CREATE INDEX inventory_editions_work ON inventory_editions(work_id);
CREATE INDEX inventory_versions_edition ON inventory_versions(edition_id);
CREATE INDEX inventory_sources_version ON inventory_sources(version_id);
CREATE INDEX inventory_assets_source_available ON inventory_assets(source_id, available);
