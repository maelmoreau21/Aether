-- Independent PostgreSQL Unicode backfill marker. Rust applies the same
-- title.to_lowercase() normalization as SQLite for stable cross-engine ordering.
CREATE TABLE IF NOT EXISTS inventory_works_unicode_migration (
    completed_at TEXT NOT NULL
);