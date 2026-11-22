-- Daily work (mini #221, src/daily.rs, docs/features.md "Daily work"): a
-- teacher plans what the class does in one of its courses on one day, and
-- each student's progress through it is kept per activity.
--
-- A plan: one course, one day (days since 1 January 1970 on the class clock,
-- settings `class_tz`). assigned_at is NULL while it is a draft. Plans older
-- than caps::DAILY_KEEP_DAYS are deleted by the hourly housekeeping.
CREATE TABLE daily_plans (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,                       -- never reused
    course_id   TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    day         INTEGER NOT NULL,
    assigned_at INTEGER,
    updated_at  INTEGER NOT NULL,
    UNIQUE (course_id, day)
);
CREATE INDEX idx_daily_plans_day ON daily_plans(day);
-- An activity of a plan, in order (position 0, 1, 2 … with no gaps, at most
-- daily::MAX_ACTIVITIES). kind: 'study' (one lesson), 'review' or 'play'
-- (one or more); lessons and games are JSON arrays of ids; rounds: a play's
-- times through, 0 for no limit. Its id is a student's stop and never reused,
-- so a round or a game kept against a stop never meets a later activity.
CREATE TABLE daily_activities (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,                          -- never reused
    plan_id  INTEGER NOT NULL REFERENCES daily_plans(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    kind     TEXT    NOT NULL CHECK (kind IN ('study', 'review', 'play')),
    lessons  TEXT    NOT NULL DEFAULT '[]',
    games    TEXT    NOT NULL DEFAULT '[]',
    rounds   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_daily_activities_plan ON daily_activities(plan_id, position);
-- A student's progress on one activity: a row once started. mark records the
-- work the kind asks for (daily::STUDIED: the lesson read through the stop;
-- daily::REVIEWED: its Review round finished), plays a play's times through,
-- done_at when it was finished.
CREATE TABLE daily_done (
    activity_id INTEGER NOT NULL REFERENCES daily_activities(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    started_at  INTEGER NOT NULL,
    mark        INTEGER NOT NULL DEFAULT 0,
    plays       INTEGER NOT NULL DEFAULT 0,
    done_at     INTEGER,
    PRIMARY KEY (activity_id, user_id)
) WITHOUT ROWID;
CREATE INDEX idx_daily_done_user ON daily_done(user_id);
