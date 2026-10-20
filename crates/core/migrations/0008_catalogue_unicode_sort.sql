-- Migration additive 0008: Marqueur de la mise à niveau Unicode de title_sort.
-- La fonction interne SQLite lower() étant restreinte à ASCII A-Z, la mise à niveau
-- de normalisation Unicode (title.to_lowercase() en Rust) pour les œuvres antérieures
-- à la migration 0007 est assurée par une procédure de rattrapage par lots bornés.
CREATE TABLE IF NOT EXISTS inventory_works_unicode_migration (
    completed_at TEXT NOT NULL
);
