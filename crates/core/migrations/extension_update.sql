CREATE TABLE IF NOT EXISTS extension_update_revisions (
 extension_id TEXT PRIMARY KEY REFERENCES extensions(id) ON DELETE CASCADE,
 package_json TEXT NOT NULL, settings_json TEXT NOT NULL,
 enabled BIGINT NOT NULL, all_libraries BIGINT NOT NULL,
 target_sha256 TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS extension_update_data AS SELECT * FROM extension_data WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_data_id ON extension_update_data(extension_id);
CREATE TABLE IF NOT EXISTS extension_update_objects AS SELECT * FROM extension_objects WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_objects_id ON extension_update_objects(extension_id);
CREATE TABLE IF NOT EXISTS extension_update_chunks AS SELECT * FROM extension_object_chunks WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_chunks_id ON extension_update_chunks(object_id);
CREATE TABLE IF NOT EXISTS extension_update_namespaces AS SELECT * FROM extension_data_namespaces WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_namespaces_id ON extension_update_namespaces(extension_id);
CREATE TABLE IF NOT EXISTS extension_update_grants AS SELECT * FROM extension_grants WHERE 1=0;
CREATE INDEX IF NOT EXISTS extension_update_grants_id ON extension_update_grants(extension_id);

