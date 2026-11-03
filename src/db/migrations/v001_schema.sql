-- v1: the schema. Times are Unix seconds; `course_id` is a course's id
-- (a built-in course's or a row of `courses`). docs/architecture.md
-- "SQLite schema" describes each table.

-- ---------- accounts ----------

CREATE TABLE users (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,   -- never reused
    email                TEXT,
    password_hash        TEXT    NOT NULL,                    -- '' for an account made by a trusted proxy
    created_at           INTEGER NOT NULL,
    email_verified_at    INTEGER,
    first_name           TEXT    NOT NULL DEFAULT '',
    last_name            TEXT    NOT NULL DEFAULT '',
    age_confirmed        INTEGER NOT NULL DEFAULT 0,
    owner                INTEGER NOT NULL DEFAULT 0,          -- runs the server: Manage Users
    must_change_password INTEGER NOT NULL DEFAULT 0,
    two_factor           TEXT    NOT NULL DEFAULT '',         -- 'totp' (on) | '' (off); anything else reads as off
    active_course        TEXT,
    lang                 TEXT    NOT NULL DEFAULT '',         -- '' follows the browser
    review_quiet         INTEGER NOT NULL DEFAULT 0,
    ai_effort            TEXT    NOT NULL DEFAULT 'medium',   -- ai::Effort::id of the last "Generate the course"
    delete_at            INTEGER                              -- deletion scheduled for then
);
CREATE INDEX idx_users_email_lower ON users(lower(email));
CREATE INDEX idx_users_active_course ON users(active_course) WHERE active_course IS NOT NULL;
CREATE INDEX idx_users_delete_at ON users(delete_at) WHERE delete_at IS NOT NULL;

CREATE TABLE sessions (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_token TEXT    NOT NULL UNIQUE,                    -- the token's hash
    created_at    INTEGER NOT NULL,
    expires_at    INTEGER NOT NULL
);
CREATE INDEX idx_sessions_expires ON sessions(expires_at);
CREATE INDEX idx_sessions_user ON sessions(user_id);

CREATE TABLE totp_keys (
    user_id      INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret       BLOB    NOT NULL,                            -- sealed (secrets::seal_totp)
    created_at   INTEGER NOT NULL,
    confirmed_at INTEGER,
    last_step    INTEGER
);
CREATE INDEX idx_totp_keys_pending ON totp_keys(created_at) WHERE confirmed_at IS NULL;

CREATE TABLE two_factor_challenges (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT    NOT NULL UNIQUE,
    attempts   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX idx_two_factor_challenges_user ON two_factor_challenges(user_id);

CREATE TABLE signup_invites (
    id         INTEGER PRIMARY KEY,
    token_hash TEXT    NOT NULL UNIQUE,
    created_by INTEGER REFERENCES users(id) ON DELETE CASCADE,   -- NULL: the first owner's link
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at    INTEGER,
    used_by    INTEGER REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX idx_signup_invites_created_by ON signup_invites(created_by);
CREATE INDEX idx_signup_invites_used_by ON signup_invites(used_by);

-- ---------- the server ----------

CREATE TABLE settings (
    name  TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE server_api_keys (
    provider   TEXT    PRIMARY KEY,                           -- secrets::Provider::id
    ciphertext BLOB    NOT NULL,
    set_at     INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE jobs (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL,
    payload_json TEXT    NOT NULL DEFAULT '{}',
    state        TEXT    NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'done', 'failed')),
    priority     INTEGER NOT NULL DEFAULT 0,
    run_at       INTEGER NOT NULL,
    attempts     INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 1,
    locked_by    TEXT,
    locked_until INTEGER,
    last_error   TEXT,
    created_at   INTEGER NOT NULL,
    finished_at  INTEGER
);
CREATE INDEX idx_jobs_due ON jobs(state, priority, run_at, id) WHERE state IN ('queued', 'running');
CREATE INDEX idx_jobs_finished ON jobs(state, finished_at) WHERE state IN ('done', 'failed');

-- ---------- courses people build ----------

CREATE TABLE courses (
    id         TEXT    PRIMARY KEY,
    owner_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title      TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    error      TEXT,                                          -- last validation error; NULL when the course loads
    state      TEXT    NOT NULL DEFAULT 'ready',              -- generating | revising | ready | failed
    source     TEXT    NOT NULL DEFAULT 'hand',               -- hand | ai | translate
    job_id     TEXT,
    brief      TEXT    NOT NULL DEFAULT '',
    outline    TEXT,
    notes      TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX idx_courses_owner_id ON courses(owner_id, id);
CREATE INDEX idx_courses_state_updated ON courses(state, updated_at);
CREATE INDEX idx_courses_with_notes ON courses(state) WHERE notes <> '';

CREATE TABLE course_files (
    course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    path       TEXT    NOT NULL,                              -- 'course.json' or 'lessons/<name>.json'
    content    TEXT    NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (course_id, path)
) WITHOUT ROWID;

CREATE TABLE course_undo (
    course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    title      TEXT    NOT NULL,
    outline    TEXT,
    label      TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    session    TEXT,
    started_at INTEGER,
    PRIMARY KEY (course_id, seq)
) WITHOUT ROWID;

CREATE TABLE course_undo_files (
    course_id TEXT    NOT NULL,
    seq       INTEGER NOT NULL,
    path      TEXT    NOT NULL,
    content   TEXT    NOT NULL,
    PRIMARY KEY (course_id, seq, path),
    FOREIGN KEY (course_id, seq) REFERENCES course_undo(course_id, seq) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE TABLE course_ai_runs (
    course_id  TEXT    PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
    label      TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    excluded   TEXT    NOT NULL DEFAULT '',
    edits      TEXT    NOT NULL DEFAULT '',
    why        TEXT    NOT NULL DEFAULT ''                    -- JSON ai::LessonNote list
) WITHOUT ROWID;

CREATE TABLE course_ai_run_files (
    course_id TEXT NOT NULL REFERENCES course_ai_runs(course_id) ON DELETE CASCADE,
    side      TEXT NOT NULL,                                  -- 'before' | 'after'
    path      TEXT NOT NULL,
    content   TEXT NOT NULL,
    PRIMARY KEY (course_id, side, path)
) WITHOUT ROWID;

CREATE TABLE course_shares (
    id         INTEGER PRIMARY KEY,
    course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    email      TEXT    NOT NULL,                              -- a label for the link
    token_hash TEXT    NOT NULL UNIQUE,
    user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    opened_at  INTEGER,
    UNIQUE (course_id, email)
);
CREATE INDEX idx_course_shares_user ON course_shares(user_id);

-- ---------- a learner's progress, per course ----------

CREATE TABLE projects (
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id      TEXT    NOT NULL,
    created_at     INTEGER NOT NULL,
    last_opened_at INTEGER NOT NULL,
    start_unit     TEXT    NOT NULL DEFAULT '',
    motivator      TEXT    NOT NULL DEFAULT '',
    set_up         INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, course_id)
) WITHOUT ROWID;
CREATE INDEX idx_projects_course ON projects(course_id);

CREATE TABLE user_progress (
    user_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id         TEXT    NOT NULL,
    lesson_id         TEXT    NOT NULL,
    mastery_score     REAL    NOT NULL DEFAULT 0,             -- 0..100
    times_tested      INTEGER NOT NULL DEFAULT 0,
    times_correct     INTEGER NOT NULL DEFAULT 0,
    streak            INTEGER NOT NULL DEFAULT 0,             -- consecutive correct answers
    last_practiced_at INTEGER,
    read_at           INTEGER,
    PRIMARY KEY (user_id, course_id, lesson_id)
) WITHOUT ROWID;

CREATE TABLE lesson_passes (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id  TEXT    NOT NULL,
    lesson_id  TEXT    NOT NULL,
    seen_at    INTEGER,
    seen_via   TEXT,
    best_pct   INTEGER NOT NULL DEFAULT -1,                   -- best qualifying run, -1 none
    best_via   TEXT,
    passed_at  INTEGER,
    passed_via TEXT,
    runs       INTEGER NOT NULL DEFAULT 0,
    evidence   TEXT    NOT NULL DEFAULT '',                   -- an agent's note on how the learner showed it
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, lesson_id)
) WITHOUT ROWID;

CREATE TABLE quiz_history (
    id              INTEGER PRIMARY KEY,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id       TEXT    NOT NULL DEFAULT 'spanish',
    lesson_id       TEXT    NOT NULL,
    question_id     TEXT    NOT NULL,
    selected_answer TEXT    NOT NULL,
    was_correct     INTEGER NOT NULL,
    answered_at     INTEGER NOT NULL
);
CREATE INDEX idx_history_user_course_time ON quiz_history(user_id, course_id, answered_at DESC);
CREATE INDEX idx_history_user_time ON quiz_history(user_id, answered_at DESC);
CREATE INDEX idx_history_answered ON quiz_history(answered_at);

CREATE TABLE journey_progress (
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id    TEXT    NOT NULL,
    lesson_id    TEXT    NOT NULL,
    step         INTEGER NOT NULL DEFAULT 0,                  -- next step to play in the current run
    completed    INTEGER NOT NULL DEFAULT 0,                  -- how many times the stage was finished
    best_stars   INTEGER NOT NULL DEFAULT 0,                  -- 0..3
    run_correct  INTEGER NOT NULL DEFAULT 0,
    run_answered INTEGER NOT NULL DEFAULT 0,
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, lesson_id)
) WITHOUT ROWID;

CREATE TABLE user_vocab (
    user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id        TEXT    NOT NULL,
    lesson_id        TEXT    NOT NULL,
    word             TEXT    NOT NULL,
    learned_at       INTEGER NOT NULL,
    times_seen       INTEGER NOT NULL DEFAULT 0,              -- flashcard reviews
    times_known      INTEGER NOT NULL DEFAULT 0,
    last_reviewed_at INTEGER,
    due_at           INTEGER,                                 -- start of the day it is due; NULL: no schedule yet
    gap_days         INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, course_id, lesson_id, word)
) WITHOUT ROWID;
CREATE INDEX idx_user_vocab_due ON user_vocab(user_id, course_id, due_at);

CREATE TABLE lesson_notes (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id  TEXT    NOT NULL,
    lesson_id  TEXT    NOT NULL,
    body       TEXT    NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, lesson_id)
);

CREATE TABLE lesson_touches (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT    NOT NULL,
    lesson_id TEXT    NOT NULL,
    mode      TEXT    NOT NULL,                               -- activity::Mode::key
    at        INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, lesson_id)
);

CREATE TABLE activity_log (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT    NOT NULL,
    kind      TEXT    NOT NULL,                               -- activity::Kind::key
    n         INTEGER NOT NULL,
    at        INTEGER NOT NULL
);
CREATE INDEX idx_activity_log_user ON activity_log(user_id, course_id, at);
CREATE INDEX idx_activity_log_at ON activity_log(at);

CREATE TABLE earnings (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id  TEXT    NOT NULL,
    points     REAL    NOT NULL DEFAULT 0,
    goals      INTEGER NOT NULL DEFAULT 0,
    day        INTEGER,
    day_points REAL    NOT NULL DEFAULT 0,
    streak     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, course_id)
);

CREATE TABLE motivator_purchases (
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id  TEXT    NOT NULL,
    item       TEXT    NOT NULL,
    price      INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, course_id, item)
);

-- ---------- games ----------

CREATE TABLE game_scores (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT    NOT NULL DEFAULT 'spanish',
    game      TEXT    NOT NULL,                               -- e.g. 'matching'
    board     TEXT    NOT NULL,                               -- 'all' or a lesson id
    score_ms  INTEGER NOT NULL,                               -- lower is better
    wrong     INTEGER NOT NULL DEFAULT 0,
    played_at INTEGER NOT NULL
);
CREATE INDEX idx_game_scores_course_board ON game_scores(course_id, game, board, score_ms, played_at);
CREATE INDEX idx_game_scores_user_board ON game_scores(user_id, course_id, game, board, score_ms);

CREATE TABLE game_activity (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id   TEXT    NOT NULL DEFAULT 'spanish',
    game        TEXT    NOT NULL,
    board       TEXT    NOT NULL,                             -- lesson id / 'all' / tense key
    label       TEXT    NOT NULL,                             -- human description, e.g. 'Present · 7 verbs'
    duration_ms INTEGER NOT NULL,
    total       INTEGER NOT NULL,
    correct     INTEGER NOT NULL,
    score_ms    INTEGER,                                      -- Word Match time incl. penalties
    played_at   INTEGER NOT NULL
);
CREATE INDEX idx_game_activity_user_course ON game_activity(user_id, course_id, played_at DESC);
CREATE INDEX idx_game_activity_game ON game_activity(user_id, course_id, game, played_at DESC);
CREATE INDEX idx_game_activity_board ON game_activity(user_id, course_id, game, board, played_at DESC);
CREATE INDEX idx_game_activity_played ON game_activity(played_at);

CREATE TABLE game_rounds (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    course_id TEXT    NOT NULL,
    game      TEXT    NOT NULL,
    rounds    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, course_id, game)
) WITHOUT ROWID;
