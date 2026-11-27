-- The learner path's record of each attempt at a lesson's Study, Review or
-- Play (`path::record`), and the activities a learner hid from their list.
-- Seeded once from earlier progress: a lesson whose Study was walked to the
-- end, or that is passed, is a Study tried; a passed lesson is also a Review
-- tried and mastered (streak 4 = `path::master_steps`), last tried now.
CREATE TABLE path_activity (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT    NOT NULL,
    lesson_id TEXT    NOT NULL,
    kind      TEXT    NOT NULL CHECK (kind IN ('study', 'review', 'play')),
    first_at  INTEGER NOT NULL,
    last_at   INTEGER NOT NULL,
    last_pct  INTEGER NOT NULL,
    streak    INTEGER NOT NULL DEFAULT 0,                    -- in steps: a clean attempt is 2, a pass 1
    attempts  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (user_id, course_id, lesson_id, kind)
) WITHOUT ROWID;
CREATE TABLE path_hidden (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT    NOT NULL,
    lesson_id TEXT    NOT NULL,
    kind      TEXT    NOT NULL CHECK (kind IN ('study', 'review', 'play')),
    hidden_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, lesson_id, kind)
) WITHOUT ROWID;
INSERT INTO path_activity (user_id, course_id, lesson_id, kind, first_at, last_at, last_pct, streak, attempts)
    SELECT user_id, course_id, lesson_id, 'study', passed_at, passed_at, CASE WHEN best_pct >= 0 THEN best_pct ELSE 100 END, 0, 1
    FROM lesson_passes WHERE passed_at IS NOT NULL;
INSERT OR IGNORE INTO path_activity (user_id, course_id, lesson_id, kind, first_at, last_at, last_pct, streak, attempts)
    SELECT user_id, course_id, lesson_id, 'study', updated_at, updated_at, 100, 0, 1
    FROM journey_progress WHERE completed > 0;
INSERT INTO path_activity (user_id, course_id, lesson_id, kind, first_at, last_at, last_pct, streak, attempts)
    SELECT user_id, course_id, lesson_id, 'review', passed_at, CAST(strftime('%s', 'now') AS INTEGER),
           CASE WHEN best_pct >= 0 THEN best_pct ELSE 100 END, 4, MAX(runs, 1)
    FROM lesson_passes WHERE passed_at IS NOT NULL;
