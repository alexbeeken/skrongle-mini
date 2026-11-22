-- Courses given to students (mini #220, src/students.rs `give`): a teacher
-- gives one of its own courses to one of its students by account. A given
-- course is a course_shares row with the student already in it (user_id and
-- opened_at set at once) and given = 1, so everything a shared course does
-- (read only, the student's own progress) works for it unchanged. Its token
-- is random and never shown: no link leads to it. The Share dialog lists
-- only given = 0 rows, and only they count toward a course's 50 links.
ALTER TABLE course_shares ADD COLUMN given INTEGER NOT NULL DEFAULT 0;
-- A course is given to a student once.
CREATE UNIQUE INDEX idx_course_shares_given ON course_shares(course_id, user_id) WHERE given = 1;
