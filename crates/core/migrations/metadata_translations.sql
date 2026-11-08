-- Additive, idempotent store of translated texts for works (films, series, episodes) and
-- seasons. Invariant facts (year, runtime, identifiers, original language) stay on
-- inventory_works. One row per (entity, locale, source): a manual correction, an NFO and a
-- provider translation coexist and are ranked at read time, so a refresh never overwrites
-- a hand-written value. Works with no row here keep their legacy columns and JSON.
CREATE TABLE IF NOT EXISTS work_translations (
 work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
 locale TEXT NOT NULL,
 language TEXT NOT NULL,
 source TEXT NOT NULL,
 priority BIGINT NOT NULL,
 title TEXT,
 sort_title TEXT,
 overview TEXT,
 tagline TEXT,
 provider_locale TEXT,
 updated_at BIGINT NOT NULL,
 PRIMARY KEY(work_id, locale, source)
);
CREATE INDEX IF NOT EXISTS work_translations_language ON work_translations(work_id, language);
CREATE TABLE IF NOT EXISTS season_translations (
 season_id TEXT NOT NULL REFERENCES inventory_seasons(id) ON DELETE CASCADE,
 locale TEXT NOT NULL,
 language TEXT NOT NULL,
 source TEXT NOT NULL,
 priority BIGINT NOT NULL,
 title TEXT,
 sort_title TEXT,
 overview TEXT,
 tagline TEXT,
 provider_locale TEXT,
 updated_at BIGINT NOT NULL,
 PRIMARY KEY(season_id, locale, source)
);
CREATE INDEX IF NOT EXISTS season_translations_language ON season_translations(season_id, language);

-- Searching by any translated or original title without duplicating works: one row per distinct
-- folded title, rebuilt for a work whenever its translations or metadata change (see
-- translations/search.rs) and filled once for existing databases by a journaled Rust step.
CREATE TABLE IF NOT EXISTS work_search_titles (
 work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
 title_search TEXT NOT NULL,
 PRIMARY KEY(work_id, title_search)
);
-- Lets the catalogue ask "does anyone have a translation in this language?" without a table scan.
CREATE INDEX IF NOT EXISTS work_translations_by_language ON work_translations(language, work_id);
