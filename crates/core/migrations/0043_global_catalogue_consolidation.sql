-- Migration additive 0043: Consolidation globale du catalogue, distinction édition/version,
-- détection d'ambiguïtés, historique de fusions réversibles et indexation de performance.

CREATE TABLE IF NOT EXISTS inventory_edition_metadata (
    edition_id TEXT PRIMARY KEY REFERENCES inventory_editions(id) ON DELETE CASCADE,
    edition_key TEXT NOT NULL DEFAULT 'standard',
    label TEXT NOT NULL DEFAULT 'Édition locale'
) STRICT;

CREATE TABLE IF NOT EXISTS inventory_version_metadata (
    version_id TEXT PRIMARY KEY REFERENCES inventory_versions(id) ON DELETE CASCADE,
    version_key TEXT NOT NULL DEFAULT 'default',
    label TEXT NOT NULL DEFAULT 'Version locale'
) STRICT;

CREATE TABLE IF NOT EXISTS media_work_external_ids (
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (work_id, provider)
) STRICT;
CREATE INDEX IF NOT EXISTS media_work_external_ids_lookup ON media_work_external_ids(kind, provider, external_id, work_id);

CREATE TABLE IF NOT EXISTS media_match_candidates (
    id TEXT PRIMARY KEY,
    left_work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    right_work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    score INTEGER NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'confirmed', 'rejected', 'undone')),
    merge_id TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(left_work_id, right_work_id)
) STRICT;
CREATE INDEX IF NOT EXISTS media_match_candidates_status ON media_match_candidates(status, score DESC, id);

CREATE TABLE IF NOT EXISTS media_merge_history (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('auto', 'manual', 'candidate')),
    candidate_id TEXT,
    snapshot_json TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    undone_at INTEGER
) STRICT;
CREATE INDEX IF NOT EXISTS media_merge_history_group ON media_merge_history(group_id, created_at DESC);

ALTER TABLE inventory_works ADD COLUMN display_title_sort TEXT NOT NULL DEFAULT '';
ALTER TABLE inventory_works ADD COLUMN effective_year INTEGER;
ALTER TABLE inventory_works ADD COLUMN max_height INTEGER NOT NULL DEFAULT 0;
ALTER TABLE inventory_works ADD COLUMN codecs_csv TEXT NOT NULL DEFAULT ',';
ALTER TABLE inventory_works ADD COLUMN languages_csv TEXT NOT NULL DEFAULT ',';
ALTER TABLE inventory_works ADD COLUMN genres_csv TEXT NOT NULL DEFAULT '|';
ALTER TABLE inventory_works ADD COLUMN has_available_asset INTEGER NOT NULL DEFAULT 0 CHECK(has_available_asset IN (0, 1));
ALTER TABLE inventory_works ADD COLUMN season_count_cached INTEGER NOT NULL DEFAULT 0;
ALTER TABLE inventory_works ADD COLUMN episode_count_cached INTEGER NOT NULL DEFAULT 0;

UPDATE inventory_works
SET display_title_sort = lower(COALESCE(json_extract(manual_json, '$.title'), title_sort)),
    effective_year = COALESCE(CAST(json_extract(manual_json, '$.year') AS INTEGER), year);

CREATE TRIGGER IF NOT EXISTS inventory_works_summary_insert AFTER INSERT ON inventory_works
WHEN NEW.display_title_sort = '' AND NEW.title_sort != '' BEGIN
    UPDATE inventory_works
    SET display_title_sort = lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort)),
        effective_year = COALESCE(CAST(json_extract(NEW.manual_json, '$.year') AS INTEGER), NEW.year),
        genres_csv = COALESCE(
            (SELECT '|' || group_concat(lower(trim(value)), '|') || '|'
             FROM json_each(CASE
                 WHEN json_valid(COALESCE(NEW.manual_json, NEW.editorial_json))
                      AND json_type(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres') = 'array'
                 THEN json_extract(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres')
                 ELSE '[]'
             END)
             WHERE trim(value) != ''),
            '|'
        )
    WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS inventory_works_summary_update AFTER UPDATE OF title, title_sort, year, manual_json, editorial_json ON inventory_works
WHEN NEW.display_title_sort != lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort))
  OR NEW.effective_year IS NOT COALESCE(CAST(json_extract(NEW.manual_json, '$.year') AS INTEGER), NEW.year)
  OR OLD.manual_json IS NOT NEW.manual_json
  OR OLD.editorial_json IS NOT NEW.editorial_json
BEGIN
    UPDATE inventory_works
    SET display_title_sort = lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort)),
        effective_year = COALESCE(CAST(json_extract(NEW.manual_json, '$.year') AS INTEGER), NEW.year),
        genres_csv = COALESCE(
            (SELECT '|' || group_concat(lower(trim(value)), '|') || '|'
             FROM json_each(CASE
                 WHEN json_valid(COALESCE(NEW.manual_json, NEW.editorial_json))
                      AND json_type(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres') = 'array'
                 THEN json_extract(COALESCE(NEW.manual_json, NEW.editorial_json), '$.genres')
                 ELSE '[]'
             END)
             WHERE trim(value) != ''),
            '|'
        )
    WHERE id = NEW.id;
END;

CREATE TRIGGER IF NOT EXISTS inventory_assets_avail_insert AFTER INSERT ON inventory_assets
WHEN NEW.available = 1 BEGIN
    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id = NEW.source_id
    ) AND has_available_asset = 0;

    UPDATE inventory_works SET has_available_asset = 1
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id = NEW.source_id
    ) AND has_available_asset = 0;
END;

CREATE TRIGGER IF NOT EXISTS inventory_episodes_avail_insert AFTER INSERT ON inventory_episodes BEGIN
    UPDATE inventory_works SET has_available_asset = 1
    WHERE id = (SELECT series_id FROM inventory_seasons WHERE id = NEW.season_id)
      AND has_available_asset = 0
      AND EXISTS (SELECT 1 FROM inventory_works ew WHERE ew.id = NEW.work_id AND ew.has_available_asset = 1);
END;

CREATE TRIGGER IF NOT EXISTS inventory_assets_avail_update AFTER UPDATE OF available, source_id ON inventory_assets BEGIN
    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_editions e
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE e.work_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id IN (OLD.source_id, NEW.source_id)
    );

    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_seasons se
        JOIN inventory_episodes ep ON ep.season_id = se.id
        JOIN inventory_editions e ON e.work_id = ep.work_id
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE se.series_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id IN (OLD.source_id, NEW.source_id)
    );
END;

CREATE TRIGGER IF NOT EXISTS inventory_assets_avail_delete AFTER DELETE ON inventory_assets BEGIN
    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_editions e
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE e.work_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT e.work_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE s.id = OLD.source_id
    );

    UPDATE inventory_works
    SET has_available_asset = CASE WHEN EXISTS(
        SELECT 1 FROM inventory_seasons se
        JOIN inventory_episodes ep ON ep.season_id = se.id
        JOIN inventory_editions e ON e.work_id = ep.work_id
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE se.series_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    WHERE id IN (
        SELECT se.series_id FROM inventory_sources s
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        JOIN inventory_episodes ep ON ep.work_id = e.work_id
        JOIN inventory_seasons se ON se.id = ep.season_id
        WHERE s.id = OLD.source_id
    );
END;

CREATE TRIGGER IF NOT EXISTS inventory_tracks_summary_insert AFTER INSERT ON inventory_tracks BEGIN
    UPDATE inventory_works
    SET max_height = CASE
            WHEN json_extract(NEW.metadata_json, '$.codec_type') = 'video'
                 AND COALESCE(CAST(json_extract(NEW.metadata_json, '$.height') AS INTEGER), 0) > max_height
            THEN CAST(json_extract(NEW.metadata_json, '$.height') AS INTEGER)
            ELSE max_height
        END,
        codecs_csv = CASE
            WHEN json_extract(NEW.metadata_json, '$.codec_name') IS NOT NULL
                 AND instr(codecs_csv, ',' || lower(json_extract(NEW.metadata_json, '$.codec_name')) || ',') = 0
            THEN codecs_csv || lower(json_extract(NEW.metadata_json, '$.codec_name')) || ','
            ELSE codecs_csv
        END,
        languages_csv = CASE
            WHEN json_extract(NEW.metadata_json, '$.tags.language') IS NOT NULL
                 AND instr(languages_csv, ',' || lower(json_extract(NEW.metadata_json, '$.tags.language')) || ',') = 0
            THEN languages_csv || lower(json_extract(NEW.metadata_json, '$.tags.language')) || ','
            ELSE languages_csv
        END
    WHERE id IN (
        SELECT e.work_id FROM inventory_assets a
        JOIN inventory_sources s ON s.id = a.source_id
        JOIN inventory_versions v ON v.id = s.version_id
        JOIN inventory_editions e ON e.id = v.edition_id
        WHERE a.id = NEW.asset_id
    );
END;

UPDATE inventory_works
SET has_available_asset = CASE
    WHEN kind = 'series' THEN CASE WHEN EXISTS(
        SELECT 1 FROM inventory_seasons se
        JOIN inventory_episodes ep ON ep.season_id = se.id
        JOIN inventory_editions e ON e.work_id = ep.work_id
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE se.series_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
    ELSE CASE WHEN EXISTS(
        SELECT 1 FROM inventory_editions e
        JOIN inventory_versions v ON v.edition_id = e.id
        JOIN inventory_sources s ON s.version_id = v.id
        JOIN inventory_assets a ON a.source_id = s.id
        WHERE e.work_id = inventory_works.id AND a.available = 1
    ) THEN 1 ELSE 0 END
END;

CREATE INDEX IF NOT EXISTS inventory_works_global_sort ON inventory_works(kind, display_title_sort, id);
CREATE INDEX IF NOT EXISTS inventory_works_lib_display_sort ON inventory_works(library_id, kind, display_title_sort, id);
CREATE INDEX IF NOT EXISTS inventory_works_lib_avail_sort ON inventory_works(library_id, kind, has_available_asset, display_title_sort, id);
CREATE INDEX IF NOT EXISTS inventory_works_global_avail_sort ON inventory_works(kind, has_available_asset, display_title_sort, id);
CREATE INDEX IF NOT EXISTS inventory_works_avail_sort ON inventory_works(has_available_asset, display_title_sort, id, library_id, kind);
CREATE INDEX IF NOT EXISTS inventory_works_global_year_sort ON inventory_works(kind, effective_year, display_title_sort, id);
CREATE INDEX IF NOT EXISTS inventory_works_global_added_sort ON inventory_works(kind, added_at, display_title_sort, id);
CREATE INDEX IF NOT EXISTS inventory_works_fts_sort ON inventory_works(id, library_id, kind, has_available_asset, display_title_sort, effective_year, max_height, codecs_csv, languages_csv, genres_csv);
CREATE INDEX IF NOT EXISTS inventory_works_filter_sort ON inventory_works(kind, has_available_asset, effective_year, max_height, display_title_sort, id, library_id, codecs_csv, languages_csv, genres_csv);
CREATE INDEX IF NOT EXISTS inventory_sources_library_root ON inventory_sources(library_id, root_snapshot);

CREATE VIRTUAL TABLE IF NOT EXISTS inventory_works_fts USING fts5(
    work_id UNINDEXED,
    search_text,
    tokenize = 'trigram'
);

CREATE TRIGGER IF NOT EXISTS inventory_works_fts_insert AFTER INSERT ON inventory_works BEGIN
    INSERT INTO inventory_works_fts(work_id, search_text)
    VALUES (NEW.id, COALESCE(NULLIF(NEW.display_title_sort, ''), NEW.title_sort));
END;

CREATE TRIGGER IF NOT EXISTS inventory_works_fts_update AFTER UPDATE OF title_sort, display_title_sort, manual_json ON inventory_works
WHEN COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort) != COALESCE(NULLIF(NEW.display_title_sort, ''), lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort)))
BEGIN
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) >= 3 AND search_text MATCH '"' || replace(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort), '"', '""') || '"' AND work_id = OLD.id;
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) < 3 AND work_id = OLD.id;
    INSERT INTO inventory_works_fts(work_id, search_text)
    VALUES (NEW.id, COALESCE(NULLIF(NEW.display_title_sort, ''), lower(COALESCE(json_extract(NEW.manual_json, '$.title'), NEW.title_sort))));
END;

CREATE TRIGGER IF NOT EXISTS inventory_works_fts_delete AFTER DELETE ON inventory_works BEGIN
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) >= 3 AND search_text MATCH '"' || replace(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort), '"', '""') || '"' AND work_id = OLD.id;
    DELETE FROM inventory_works_fts WHERE length(COALESCE(NULLIF(OLD.display_title_sort, ''), OLD.title_sort)) < 3 AND work_id = OLD.id;
END;

INSERT INTO inventory_works_fts(work_id, search_text)
SELECT id, COALESCE(NULLIF(display_title_sort, ''), title_sort) FROM inventory_works;

CREATE TABLE IF NOT EXISTS inventory_cache_generation (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    gen INTEGER NOT NULL DEFAULT 1
);
INSERT OR IGNORE INTO inventory_cache_generation(id, gen) VALUES (1, 1);

CREATE TRIGGER IF NOT EXISTS cache_gen_works_ins AFTER INSERT ON inventory_works BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_works_upd AFTER UPDATE ON inventory_works BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_works_del AFTER DELETE ON inventory_works BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_assets_upd AFTER UPDATE OF available, source_id ON inventory_assets BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_libs_upd AFTER INSERT ON libraries BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_libs_chg AFTER UPDATE ON libraries BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_groups_ins AFTER INSERT ON media_group_members BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_groups_del AFTER DELETE ON media_group_members BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_parental_ins AFTER INSERT ON parental_policies BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_parental_upd AFTER UPDATE ON parental_policies BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_parental_del AFTER DELETE ON parental_policies BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_perms_ins AFTER INSERT ON library_permissions BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_perms_del AFTER DELETE ON library_permissions BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_images_ins AFTER INSERT ON inventory_images BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_images_upd AFTER UPDATE ON inventory_images BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_posters_ins AFTER INSERT ON work_posters BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_posters_upd AFTER UPDATE ON work_posters BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS cache_gen_posters_del AFTER DELETE ON work_posters BEGIN
    UPDATE inventory_cache_generation SET gen = gen + 1 WHERE id = 1;
END;
