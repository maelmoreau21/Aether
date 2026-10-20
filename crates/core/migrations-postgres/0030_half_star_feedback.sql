ALTER TABLE work_feedback DROP CONSTRAINT work_feedback_rating_check;
ALTER TABLE work_feedback ALTER COLUMN rating TYPE DOUBLE PRECISION USING rating::double precision;
ALTER TABLE work_feedback ADD CONSTRAINT work_feedback_rating_check CHECK(rating BETWEEN 0.5 AND 5 AND rating * 2 = floor(rating * 2));
