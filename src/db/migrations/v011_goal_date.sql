-- A date to aim for (#235): one optional day per learner and course,
-- 'YYYY-MM-DD' as <input type="date"> sends it, '' for none. Only the
-- learner's own course home reads it (src/goal_date.rs).
ALTER TABLE projects ADD COLUMN goal_date TEXT NOT NULL DEFAULT '';
