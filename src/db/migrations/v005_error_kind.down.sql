-- Back to v4: courses.error_kind goes. The sentence in `error` stays, and a
-- v4 binary reads only that.
ALTER TABLE courses DROP COLUMN error_kind;
