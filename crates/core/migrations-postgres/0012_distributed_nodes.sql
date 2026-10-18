-- Alter in place: PostgreSQL foreign keys remain attached to this table.
ALTER TABLE media_nodes DROP CONSTRAINT media_nodes_id_check;
ALTER TABLE media_nodes ADD CONSTRAINT media_nodes_id_check CHECK(length(id) BETWEEN 1 AND 128);
ALTER TABLE media_nodes ADD COLUMN name TEXT NOT NULL DEFAULT 'Local Media Node' CHECK(length(name) BETWEEN 1 AND 100);
ALTER TABLE media_nodes ADD COLUMN kind TEXT NOT NULL DEFAULT 'local' CHECK(kind IN ('local', 'remote'));
ALTER TABLE media_nodes ADD COLUMN base_url TEXT;
ALTER TABLE media_nodes ADD COLUMN public_url TEXT;
ALTER TABLE media_nodes ADD COLUMN status TEXT NOT NULL DEFAULT 'connected' CHECK(status IN ('connected', 'disconnected', 'revoked'));
ALTER TABLE media_nodes ADD COLUMN key_id TEXT NOT NULL DEFAULT 'local';
ALTER TABLE media_nodes ADD COLUMN shared_secret TEXT NOT NULL DEFAULT '';
ALTER TABLE media_nodes ADD COLUMN roots_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE media_nodes ADD COLUMN paired_at BIGINT NOT NULL DEFAULT (floor(extract(epoch FROM statement_timestamp()))::bigint);
ALTER TABLE media_nodes ADD COLUMN last_seen_at BIGINT;
ALTER TABLE media_nodes ADD COLUMN revoked_at BIGINT;
UPDATE media_nodes SET name = 'Node local (Standalone)', last_seen_at = floor(extract(epoch FROM statement_timestamp()))::bigint WHERE id = 'local';