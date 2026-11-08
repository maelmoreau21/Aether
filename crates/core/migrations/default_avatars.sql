-- Separate from the pack: removing a default collection is a durable choice.
CREATE TABLE IF NOT EXISTS avatar_gallery_initialization (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1)
);
-- Each built-in collection shipped with the server is offered once, on new and existing
-- installations alike; removing it afterwards is just as durable.
CREATE TABLE IF NOT EXISTS avatar_builtin_seeds (
    pack_id TEXT PRIMARY KEY NOT NULL
);
