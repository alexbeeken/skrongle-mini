-- Back to v1: the shop's table is there again, empty. What was bought before
-- v2 was dropped by v2 and cannot come back, and a course that wore Skrongle
-- Cash keeps wearing XP.
CREATE TABLE motivator_purchases (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id  TEXT    NOT NULL,
    item       TEXT    NOT NULL,
    price      INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, item)
);
