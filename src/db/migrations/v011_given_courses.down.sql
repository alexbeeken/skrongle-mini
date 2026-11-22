-- Back to v10: no given courses. A v10 binary would show a given course as
-- an opened link in its Share dialog, so the given rows go: the students
-- lose those courses (their progress rows stay). Links are kept.
DELETE FROM course_shares WHERE given = 1;
DROP INDEX idx_course_shares_given;
ALTER TABLE course_shares DROP COLUMN given;
