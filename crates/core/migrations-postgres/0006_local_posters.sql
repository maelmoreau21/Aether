-- Local artwork is separate from the video graph. Paths never leave the backend.
CREATE TABLE inventory_images (
    id TEXT PRIMARY KEY,
    work_id TEXT NOT NULL REFERENCES inventory_works(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    root_snapshot TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    size BIGINT NOT NULL,
    modified_ns TEXT NOT NULL,
    mime TEXT NOT NULL,
    priority BIGINT NOT NULL,
    available BIGINT NOT NULL DEFAULT 1 CHECK(available IN (0,1)),
    UNIQUE(work_id,root_snapshot,relative_path)
);
CREATE INDEX inventory_images_work ON inventory_images(work_id,available,priority,relative_path);
