-- Libraries removed by an administrator. Media files and history are kept,
-- but the library and its folders are disabled and hidden everywhere.
CREATE TABLE IF NOT EXISTS deleted_libraries (
 library_id TEXT PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
 deleted_at BIGINT NOT NULL
);
