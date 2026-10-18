-- Migration additive 0011: Modèle relationnel des séries, saisons et épisodes.
-- Étend inventory_works pour classifier les oeuvres ('film', 'series', 'episode'),
-- crée les tables inventory_seasons et inventory_episodes,
-- et ajoute les colonnes de reconnaissance des séries dans le staging PostgreSQL.

ALTER TABLE inventory_works ADD COLUMN kind TEXT NOT NULL DEFAULT 'film';

CREATE TABLE IF NOT EXISTS inventory_seasons (
    id TEXT PRIMARY KEY,
    series_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    season_number BIGINT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    UNIQUE(series_id, season_number)
);

CREATE TABLE IF NOT EXISTS inventory_episodes (
    id TEXT PRIMARY KEY,
    season_id TEXT NOT NULL REFERENCES inventory_seasons(id) ON DELETE CASCADE,
    episode_number BIGINT NOT NULL,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    UNIQUE(season_id, episode_number),
    UNIQUE(work_id)
);

CREATE INDEX IF NOT EXISTS inventory_works_library_kind ON inventory_works(library_id, kind);
CREATE INDEX IF NOT EXISTS inventory_seasons_series ON inventory_seasons(series_id, season_number);
CREATE INDEX IF NOT EXISTS inventory_episodes_season ON inventory_episodes(season_id, episode_number);
CREATE INDEX IF NOT EXISTS inventory_episodes_work ON inventory_episodes(work_id);

ALTER TABLE scan_staging_files ADD COLUMN series_identity_key TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_title TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_year BIGINT;
ALTER TABLE scan_staging_files ADD COLUMN series_external_ids_json TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_editorial_json TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_poster_path TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_poster_size BIGINT;
ALTER TABLE scan_staging_files ADD COLUMN series_poster_modified_ns TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_poster_mime TEXT;
ALTER TABLE scan_staging_files ADD COLUMN series_poster_priority BIGINT;
ALTER TABLE scan_staging_files ADD COLUMN season_number BIGINT;
ALTER TABLE scan_staging_files ADD COLUMN episode_number BIGINT;
