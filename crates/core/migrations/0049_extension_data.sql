-- Private extension state; uninstall does not implicitly destroy durable data.
CREATE TABLE extension_data_namespaces (
 extension_id TEXT PRIMARY KEY,
 publisher_id TEXT NOT NULL,
 publisher_key TEXT NOT NULL,
 entries BIGINT NOT NULL DEFAULT 0 CHECK(entries >= 0),
 bytes BIGINT NOT NULL DEFAULT 0 CHECK(bytes >= 0)
);
CREATE TABLE extension_data (
 extension_id TEXT NOT NULL REFERENCES extension_data_namespaces(extension_id),
 scope TEXT NOT NULL,
 scope_id TEXT NOT NULL,
 key TEXT NOT NULL,
 value TEXT NOT NULL,
 version TEXT NOT NULL,
 expires_at BIGINT,
 durability TEXT NOT NULL CHECK(durability IN ('cache','durable')),
 value_bytes BIGINT NOT NULL CHECK(value_bytes >= 0),
 sha256 TEXT NOT NULL,
 PRIMARY KEY(extension_id,scope,scope_id,key)
);
CREATE INDEX extension_data_expiry ON extension_data(extension_id,expires_at);

