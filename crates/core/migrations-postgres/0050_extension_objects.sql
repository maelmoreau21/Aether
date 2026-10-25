-- Shared PostgreSQL payloads require no per-Core object directory or SQL in guests.
CREATE TABLE extension_objects (
    id TEXT PRIMARY KEY,
    extension_id TEXT NOT NULL,
    scope_kind TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    size BIGINT NOT NULL CHECK(size BETWEEN 0 AND 16777216),
    received BIGINT NOT NULL DEFAULT 0 CHECK(received >= 0 AND received <= size),
    sha256 TEXT NOT NULL,
    durability TEXT NOT NULL CHECK(durability IN ('cache','durable')),
    expires_at BIGINT,
    upload_expires_at BIGINT NOT NULL,
    complete BIGINT NOT NULL DEFAULT 0 CHECK(complete IN (0,1))
);
CREATE INDEX extension_objects_owner ON extension_objects(extension_id,scope_kind,scope_id);
CREATE TABLE extension_object_chunks (
    object_id TEXT NOT NULL REFERENCES extension_objects(id) ON DELETE CASCADE,
    offset_bytes BIGINT NOT NULL CHECK(offset_bytes >= 0),
    payload BYTEA NOT NULL CHECK(octet_length(payload) <= 16384),
    sha256 TEXT NOT NULL,
    PRIMARY KEY(object_id,offset_bytes)
);
