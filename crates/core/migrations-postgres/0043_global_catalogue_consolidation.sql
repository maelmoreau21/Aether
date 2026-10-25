-- Migration additive 0043 (PostgreSQL): Consolidation globale du catalogue, distinction édition/version,
-- détection d'ambiguïtés, historique de fusions réversibles et indexation de performance.

CREATE TABLE IF NOT EXISTS inventory_edition_metadata (
    edition_id TEXT PRIMARY KEY REFERENCES inventory_editions(id) ON DELETE CASCADE,
    edition_key TEXT NOT NULL DEFAULT 'standard',
    label TEXT NOT NULL DEFAULT 'Édition locale'
);

CREATE TABLE IF NOT EXISTS inventory_version_metadata (
    version_id TEXT PRIMARY KEY REFERENCES inventory_versions(id) ON DELETE CASCADE,
    version_key TEXT NOT NULL DEFAULT 'default',
    label TEXT NOT NULL DEFAULT 'Version locale'
);

CREATE TABLE IF NOT EXISTS media_work_external_ids (
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    provider TEXT NOT NULL,
    external_id TEXT NOT NULL,
    PRIMARY KEY (work_id, provider)
);
CREATE INDEX IF NOT EXISTS media_work_external_ids_lookup ON media_work_external_ids(kind, provider, external_id, work_id);

CREATE TABLE IF NOT EXISTS media_match_candidates (
    id TEXT PRIMARY KEY,
    left_work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    right_work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    score BIGINT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'confirmed', 'rejected', 'undone')),
    merge_id TEXT,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    updated_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    UNIQUE(left_work_id, right_work_id)
);
CREATE INDEX IF NOT EXISTS media_match_candidates_status ON media_match_candidates(status, score DESC, id);

CREATE TABLE IF NOT EXISTS media_merge_history (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('auto', 'manual', 'candidate')),
    candidate_id TEXT,
    snapshot_json TEXT NOT NULL,
    created_at BIGINT NOT NULL DEFAULT (EXTRACT(EPOCH FROM NOW())::BIGINT),
    undone_at BIGINT
);
CREATE INDEX IF NOT EXISTS media_merge_history_group ON media_merge_history(group_id, created_at DESC);

ALTER TABLE inventory_works ADD COLUMN display_title_sort TEXT NOT NULL DEFAULT '';
ALTER TABLE inventory_works ADD COLUMN effective_year BIGINT;
ALTER TABLE inventory_works ADD COLUMN max_height BIGINT NOT NULL DEFAULT 0;
ALTER TABLE inventory_works ADD COLUMN codecs_csv TEXT NOT NULL DEFAULT ',';
ALTER TABLE inventory_works ADD COLUMN languages_csv TEXT NOT NULL DEFAULT ',';
ALTER TABLE inventory_works ADD COLUMN genres_csv TEXT NOT NULL DEFAULT '|';
ALTER TABLE inventory_works ADD COLUMN has_available_asset BIGINT NOT NULL DEFAULT 0 CHECK(has_available_asset IN (0, 1));
ALTER TABLE inventory_works ADD COLUMN season_count_cached BIGINT NOT NULL DEFAULT 0;
ALTER TABLE inventory_works ADD COLUMN episode_count_cached BIGINT NOT NULL DEFAULT 0;

UPDATE inventory_works
SET display_title_sort = title_sort,
    effective_year = year,
    has_available_asset = CASE
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
CREATE INDEX IF NOT EXISTS inventory_sources_library_root ON inventory_sources(library_id, root_snapshot);

CREATE TABLE IF NOT EXISTS inventory_cache_generation (
    id BIGINT PRIMARY KEY CHECK (id = 1),
    gen BIGINT NOT NULL DEFAULT 1
);
INSERT INTO inventory_cache_generation(id, gen) VALUES (1, 1) ON CONFLICT (id) DO NOTHING;
