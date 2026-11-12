-- Back to v2: a link carries an address again, one per (course, address).
-- Every link and its token is kept. A link whose name v2 cannot hold as it
-- is (no name, or a name another link of the course has) gets its id added,
-- so the old uniqueness holds; going up again keeps that as its name.
CREATE TABLE course_shares_v2 (
    id         INTEGER PRIMARY KEY,
    course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    email      TEXT    NOT NULL,                              -- a label for the link
    token_hash TEXT    NOT NULL UNIQUE,
    user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    opened_at  INTEGER,
    UNIQUE (course_id, email)
);
INSERT INTO course_shares_v2 (id, course_id, email, token_hash, user_id, created_at, opened_at)
    SELECT s.id, s.course_id,
           CASE
               WHEN s.label = '' THEN 'link ' || s.id
               WHEN EXISTS (SELECT 1 FROM course_shares o WHERE o.course_id = s.course_id AND o.label = s.label AND o.id < s.id)
                   THEN s.label || ' (' || s.id || ')'
               ELSE s.label
           END,
           s.token_hash, s.user_id, s.created_at, s.opened_at
    FROM course_shares s;
DROP TABLE course_shares;
ALTER TABLE course_shares_v2 RENAME TO course_shares;
CREATE INDEX idx_course_shares_user ON course_shares(user_id);
