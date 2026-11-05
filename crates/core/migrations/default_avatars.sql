-- Separate from the pack: removing a default collection is a durable choice.
CREATE TABLE IF NOT EXISTS avatar_gallery_initialization (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1)
);
