CREATE TABLE IF NOT EXISTS avatar_requirements (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 required INTEGER NOT NULL CHECK(required IN (0,1))
);
