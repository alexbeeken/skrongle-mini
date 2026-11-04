-- Sharing a course is by link alone (#16): a link carries a name the owner
-- gives it (or none), not an e-mail address, and a course may have several
-- unnamed ones, so the (course, address) uniqueness goes. The rows and
-- their tokens are kept: every link that worked still works.
CREATE TABLE course_shares_v3 (
    id         INTEGER PRIMARY KEY,
    course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    label      TEXT    NOT NULL DEFAULT '',                   -- a name the owner gave the link, or ''
    token_hash TEXT    NOT NULL UNIQUE,
    user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    opened_at  INTEGER
);
INSERT INTO course_shares_v3 (id, course_id, label, token_hash, user_id, created_at, opened_at)
    SELECT id, course_id, email, token_hash, user_id, created_at, opened_at FROM course_shares;
DROP TABLE course_shares;
ALTER TABLE course_shares_v3 RENAME TO course_shares;
CREATE INDEX idx_course_shares_user ON course_shares(user_id);
-- The (course, address) uniqueness used to cover course_id; an index of its own now.
CREATE INDEX idx_course_shares_course ON course_shares(course_id);
