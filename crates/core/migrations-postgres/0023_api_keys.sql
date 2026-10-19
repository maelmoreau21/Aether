CREATE TABLE api_keys (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80), user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE, token_hash TEXT NOT NULL UNIQUE, created_at BIGINT NOT NULL, expires_at BIGINT NOT NULL, revoked_at BIGINT, scope TEXT NOT NULL CHECK(scope='catalog:read'));
CREATE INDEX api_keys_owner ON api_keys(user_id);
CREATE TABLE api_key_libraries (key_id TEXT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE, PRIMARY KEY(key_id,library_id));
