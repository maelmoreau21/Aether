CREATE TABLE work_feedback_half_stars (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id TEXT NOT NULL REFERENCES inventory_works(id) ON DELETE CASCADE,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK(favorite IN (0,1)),
    rating REAL CHECK(rating BETWEEN 0.5 AND 5 AND rating * 2 = CAST(rating * 2 AS INTEGER)),
    PRIMARY KEY(user_id,work_id)
);
INSERT INTO work_feedback_half_stars SELECT user_id,work_id,favorite,CAST(rating AS REAL) FROM work_feedback;
DROP TABLE work_feedback;
ALTER TABLE work_feedback_half_stars RENAME TO work_feedback;
