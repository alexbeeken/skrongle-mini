-- Why a generation stopped, as a name (`ai::Failure`: key, quota, busy, ...)
-- instead of only the sentence in `error`. NULL for a stopped course from
-- before this column, and for a stop that is not an AI failure (a cancel, a
-- restart); the page then reads `error` as it always did.
ALTER TABLE courses ADD COLUMN error_kind TEXT;
