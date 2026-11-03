//! The schema migrations (moved out of db.rs, #1041): one SQL string per
//! version, applied in order by `db::migrate`. Append a new one at the end.

/// Schema migrations, applied in order. `PRAGMA user_version` tracks which have run.
pub(crate) const MIGRATIONS: &[&str] = &[
    // v1: initial schema
    "
    CREATE TABLE users (
        id            INTEGER PRIMARY KEY,
        username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
        password_hash TEXT    NOT NULL,
        created_at    INTEGER NOT NULL
    );

    CREATE TABLE sessions (
        id            INTEGER PRIMARY KEY,
        user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        session_token TEXT    NOT NULL UNIQUE,
        created_at    INTEGER NOT NULL,
        expires_at    INTEGER NOT NULL
    );
    CREATE INDEX idx_sessions_expires ON sessions(expires_at);
    CREATE INDEX idx_sessions_user    ON sessions(user_id);

    CREATE TABLE user_progress (
        user_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        lesson_id         TEXT    NOT NULL,
        mastery_score     REAL    NOT NULL DEFAULT 0,   -- 0..100
        times_tested      INTEGER NOT NULL DEFAULT 0,
        times_correct     INTEGER NOT NULL DEFAULT 0,
        streak            INTEGER NOT NULL DEFAULT 0,   -- consecutive correct answers
        last_practiced_at INTEGER,
        PRIMARY KEY (user_id, lesson_id)
    ) WITHOUT ROWID;

    CREATE TABLE quiz_history (
        id              INTEGER PRIMARY KEY,
        user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        lesson_id       TEXT    NOT NULL,
        question_id     TEXT    NOT NULL,
        selected_answer TEXT    NOT NULL,
        was_correct     INTEGER NOT NULL,
        answered_at     INTEGER NOT NULL
    );
    CREATE INDEX idx_history_user_time     ON quiz_history(user_id, answered_at DESC);
    CREATE INDEX idx_history_user_question ON quiz_history(user_id, question_id, answered_at DESC);
    ",
    // v2: guided journey progress
    "
    CREATE TABLE journey_progress (
        user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        lesson_id    TEXT    NOT NULL,
        step         INTEGER NOT NULL DEFAULT 0,  -- next step to play in the current run
        completed    INTEGER NOT NULL DEFAULT 0,  -- how many times the stage was finished
        best_stars   INTEGER NOT NULL DEFAULT 0,  -- 0..3
        xp           INTEGER NOT NULL DEFAULT 0,  -- total XP earned in this stage
        run_correct  INTEGER NOT NULL DEFAULT 0,  -- questions right in the current run
        run_answered INTEGER NOT NULL DEFAULT 0,
        updated_at   INTEGER NOT NULL,
        PRIMARY KEY (user_id, lesson_id)
    ) WITHOUT ROWID;
    ",
    // v3: vocabulary collected from passed quiz questions
    "
    CREATE TABLE user_vocab (
        user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        lesson_id        TEXT    NOT NULL,
        word             TEXT    NOT NULL,   -- the lesson vocabulary entry's Spanish text
        learned_at       INTEGER NOT NULL,
        times_seen       INTEGER NOT NULL DEFAULT 0,  -- flashcard reviews
        times_known      INTEGER NOT NULL DEFAULT 0,
        last_reviewed_at INTEGER,
        PRIMARY KEY (user_id, lesson_id, word)
    ) WITHOUT ROWID;
    ",
    // v4: game leaderboards
    "
    CREATE TABLE game_scores (
        id        INTEGER PRIMARY KEY,
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        game      TEXT    NOT NULL,   -- e.g. 'matching'
        board     TEXT    NOT NULL,   -- 'all' or a lesson id
        score_ms  INTEGER NOT NULL,   -- lower is better
        wrong     INTEGER NOT NULL DEFAULT 0,
        played_at INTEGER NOT NULL
    );
    CREATE INDEX idx_game_scores_board ON game_scores(game, board, score_ms);
    CREATE INDEX idx_game_scores_user  ON game_scores(user_id, game, board);
    ",
    // v5: every finished game round, for dashboard/history stats
    "
    CREATE TABLE game_activity (
        id          INTEGER PRIMARY KEY,
        user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        game        TEXT    NOT NULL,   -- 'matching' | 'conjugation'
        board       TEXT    NOT NULL,   -- lesson id / 'all' / tense key
        label       TEXT    NOT NULL,   -- human description, e.g. 'Present · 7 verbs'
        duration_ms INTEGER NOT NULL,
        total       INTEGER NOT NULL,   -- pairs matched / forms asked
        correct     INTEGER NOT NULL,   -- pairs (always = total) / forms right
        score_ms    INTEGER,            -- Word Match time incl. penalties
        played_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_game_activity_user_time ON game_activity(user_id, played_at DESC);
    ",
    // v6: XP earned per Sprint mode session
    "
    ALTER TABLE game_activity ADD COLUMN xp INTEGER NOT NULL DEFAULT 0;
    ",
    // v7: Google sign-in. Accounts created through Google have an empty password_hash
    // (which never verifies), so they can only sign in with Google.
    "
    ALTER TABLE users ADD COLUMN google_sub TEXT;
    ALTER TABLE users ADD COLUMN email TEXT;
    CREATE UNIQUE INDEX idx_users_google_sub ON users(google_sub);
    ",
    // v8: many courses. Progress is scoped by course_id; courses people build in
    // the app live in `courses` + `course_files`; `projects` records which courses
    // a user has opened and `users.active_course` the one they are in.
    "
    ALTER TABLE users ADD COLUMN active_course TEXT;

    CREATE TABLE courses (
        id         TEXT    PRIMARY KEY,
        owner_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        title      TEXT    NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        error      TEXT                -- last validation error; NULL when the course loads
    );
    CREATE INDEX idx_courses_owner ON courses(owner_id);

    CREATE TABLE course_files (
        course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
        path       TEXT    NOT NULL,   -- 'course.json' or 'lessons/<name>.json'
        content    TEXT    NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (course_id, path)
    ) WITHOUT ROWID;

    CREATE TABLE projects (
        user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id      TEXT    NOT NULL,
        created_at     INTEGER NOT NULL,
        last_opened_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, course_id)
    ) WITHOUT ROWID;

    CREATE TABLE user_progress_v8 (
        user_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id         TEXT    NOT NULL,
        lesson_id         TEXT    NOT NULL,
        mastery_score     REAL    NOT NULL DEFAULT 0,
        times_tested      INTEGER NOT NULL DEFAULT 0,
        times_correct     INTEGER NOT NULL DEFAULT 0,
        streak            INTEGER NOT NULL DEFAULT 0,
        last_practiced_at INTEGER,
        PRIMARY KEY (user_id, course_id, lesson_id)
    ) WITHOUT ROWID;
    INSERT INTO user_progress_v8 SELECT user_id, 'spanish', lesson_id, mastery_score, times_tested, times_correct, streak, last_practiced_at FROM user_progress;
    DROP TABLE user_progress;
    ALTER TABLE user_progress_v8 RENAME TO user_progress;

    CREATE TABLE journey_progress_v8 (
        user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id    TEXT    NOT NULL,
        lesson_id    TEXT    NOT NULL,
        step         INTEGER NOT NULL DEFAULT 0,
        completed    INTEGER NOT NULL DEFAULT 0,
        best_stars   INTEGER NOT NULL DEFAULT 0,
        xp           INTEGER NOT NULL DEFAULT 0,
        run_correct  INTEGER NOT NULL DEFAULT 0,
        run_answered INTEGER NOT NULL DEFAULT 0,
        updated_at   INTEGER NOT NULL,
        PRIMARY KEY (user_id, course_id, lesson_id)
    ) WITHOUT ROWID;
    INSERT INTO journey_progress_v8 SELECT user_id, 'spanish', lesson_id, step, completed, best_stars, xp, run_correct, run_answered, updated_at FROM journey_progress;
    DROP TABLE journey_progress;
    ALTER TABLE journey_progress_v8 RENAME TO journey_progress;

    CREATE TABLE user_vocab_v8 (
        user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id        TEXT    NOT NULL,
        lesson_id        TEXT    NOT NULL,
        word             TEXT    NOT NULL,
        learned_at       INTEGER NOT NULL,
        times_seen       INTEGER NOT NULL DEFAULT 0,
        times_known      INTEGER NOT NULL DEFAULT 0,
        last_reviewed_at INTEGER,
        PRIMARY KEY (user_id, course_id, lesson_id, word)
    ) WITHOUT ROWID;
    INSERT INTO user_vocab_v8 SELECT user_id, 'spanish', lesson_id, word, learned_at, times_seen, times_known, last_reviewed_at FROM user_vocab;
    DROP TABLE user_vocab;
    ALTER TABLE user_vocab_v8 RENAME TO user_vocab;

    ALTER TABLE quiz_history  ADD COLUMN course_id TEXT NOT NULL DEFAULT 'spanish';
    ALTER TABLE game_scores   ADD COLUMN course_id TEXT NOT NULL DEFAULT 'spanish';
    ALTER TABLE game_activity ADD COLUMN course_id TEXT NOT NULL DEFAULT 'spanish';
    CREATE INDEX idx_history_user_course_time ON quiz_history(user_id, course_id, answered_at DESC);
    CREATE INDEX idx_game_scores_course_board ON game_scores(course_id, game, board, score_ms);
    CREATE INDEX idx_game_activity_user_course ON game_activity(user_id, course_id, played_at DESC);

    -- Everyone who has done anything so far was learning Spanish.
    UPDATE users SET active_course = 'spanish' WHERE id IN (
        SELECT user_id FROM user_progress UNION SELECT user_id FROM journey_progress UNION SELECT user_id FROM game_activity);
    INSERT OR IGNORE INTO projects (user_id, course_id, created_at, last_opened_at)
        SELECT id, 'spanish', created_at, created_at FROM users WHERE active_course = 'spanish';
    ",
    // v9: user profiles. `avatar` names one of the built-in icons (src/profile.rs);
    // `email` (added in v7 for Google accounts) is now asked for at registration
    // and, once set, never changes.
    "
    ALTER TABLE users ADD COLUMN first_name TEXT NOT NULL DEFAULT '';
    ALTER TABLE users ADD COLUMN last_name  TEXT NOT NULL DEFAULT '';
    ALTER TABLE users ADD COLUMN avatar     TEXT NOT NULL DEFAULT 'owl';
    CREATE INDEX idx_users_email ON users(email);
    ",
    // v10: learners' AI API keys, encrypted at rest (src/secrets.rs). The blob is
    // `version || nonce || ChaCha20-Poly1305 ciphertext`, bound to (user_id, provider).
    "
    CREATE TABLE user_api_keys (
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        provider   TEXT    NOT NULL,   -- 'anthropic' | 'openai' | 'google'
        ciphertext BLOB    NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, provider)
    ) WITHOUT ROWID;
    ",
    // v11: the theme a learner wears (an id from data/themes/, or '' for
    // none). The profile picture stays `avatar`; a theme shows its own
    // version of that picture while it is worn.
    "
    ALTER TABLE users ADD COLUMN theme TEXT NOT NULL DEFAULT '';
    ",
    // v12: when the learner finished reading a lesson (passed its two-question
    // check). Recommended practice only draws from read lessons. Lessons that
    // already had quiz answers before this existed count as read.
    "
    ALTER TABLE user_progress ADD COLUMN read_at INTEGER;
    UPDATE user_progress SET read_at = COALESCE(last_practiced_at, strftime('%s', 'now'))
        WHERE times_tested > 0;
    ",
    // v13: one idea of "passed" for every mode (src/progress.rs): a lesson is
    // passed once seen and shown (a run of >= 70 %). Earlier Guide / Sprint mode
    // completions carry over: seen, and passed where they earned 2+ stars
    // (>= 70 % right). Lessons read before (v12's read_at) count as seen;
    // from now on seen and read are one thing (progress::mark_seen sets both).
    "
    CREATE TABLE lesson_passes (
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id  TEXT    NOT NULL,
        lesson_id  TEXT    NOT NULL,
        seen_at    INTEGER,
        seen_via   TEXT,
        best_pct   INTEGER NOT NULL DEFAULT -1,   -- best qualifying run, -1 none
        best_via   TEXT,
        passed_at  INTEGER,
        passed_via TEXT,                          -- 'k' | 'd' | 'g' | 'tutor' | 'agent' | 'legacy'
        runs       INTEGER NOT NULL DEFAULT 0,
        evidence   TEXT    NOT NULL DEFAULT '',   -- an agent's note on how the learner showed it
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, course_id, lesson_id)
    ) WITHOUT ROWID;
    INSERT INTO lesson_passes
        (user_id, course_id, lesson_id, seen_at, seen_via, best_pct, best_via, passed_at, passed_via, runs, evidence, updated_at)
    SELECT user_id, course_id, lesson_id, updated_at, 'legacy',
           CASE WHEN best_stars >= 3 THEN 90 WHEN best_stars = 2 THEN 70 ELSE 0 END, 'legacy',
           CASE WHEN best_stars >= 2 THEN updated_at END, CASE WHEN best_stars >= 2 THEN 'legacy' END,
           completed, '', updated_at
    FROM journey_progress WHERE completed > 0;
    INSERT OR IGNORE INTO lesson_passes (user_id, course_id, lesson_id, seen_at, seen_via, updated_at)
    SELECT user_id, course_id, lesson_id, read_at, 'legacy', read_at FROM user_progress WHERE read_at IS NOT NULL;
    ",
    // v14: user-made courses have a lifecycle. An AI-generated course exists
    // (greyed out in the course list) from the moment it is asked for:
    // state 'generating' until the job finishes, then 'ready' or 'failed'.
    // `outline` keeps the planned lessons so a failed run can resume;
    // `brief` is what the learner asked for, `notes` the material they gave. Every user course is private to
    // its owner; 'public' is reserved for publishing.
    "
    ALTER TABLE courses ADD COLUMN state      TEXT NOT NULL DEFAULT 'ready';   -- 'generating' | 'ready' | 'failed'
    ALTER TABLE courses ADD COLUMN source     TEXT NOT NULL DEFAULT 'hand';    -- 'hand' | 'ai'
    ALTER TABLE courses ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'; -- 'private' | 'public' (not offered yet)
    ALTER TABLE courses ADD COLUMN job_id     TEXT;
    ALTER TABLE courses ADD COLUMN brief      TEXT NOT NULL DEFAULT '';
    ALTER TABLE courses ADD COLUMN outline    TEXT;
    ALTER TABLE courses ADD COLUMN notes      TEXT NOT NULL DEFAULT '';    -- the learner's material, for a resumed run
    ",
    // v15: paid subscriptions (src/billing.rs). `subscriptions` mirrors the
    // learner's Stripe subscription as the webhooks last described it;
    // `plan_grants` is premium given without paying (by hand, or the dev toggle).
    "
    CREATE TABLE subscriptions (
        user_id              INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        customer_id          TEXT    NOT NULL UNIQUE,      -- Stripe cus_…
        subscription_id      TEXT    NOT NULL,             -- Stripe sub_…
        status               TEXT    NOT NULL,             -- Stripe's: active | trialing | past_due | canceled | unpaid | incomplete | …
        price_id             TEXT    NOT NULL DEFAULT '',
        current_period_end   INTEGER,
        cancel_at_period_end INTEGER NOT NULL DEFAULT 0,
        event_at             INTEGER NOT NULL,             -- `created` of the newest event applied; older ones are skipped
        updated_at           INTEGER NOT NULL
    );
    CREATE TABLE plan_grants (
        user_id    INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        granted_at INTEGER NOT NULL,
        expires_at INTEGER,                                -- NULL: no end
        note       TEXT    NOT NULL DEFAULT ''
    );
    ",
    // v16: the "stardew" theme became "meadowbrook" (original art and world);
    // learners keep their choice, and their answer history keeps its question ids.
    "
    UPDATE users SET theme = 'meadowbrook' WHERE theme = 'stardew';
    UPDATE quiz_history SET question_id = replace(question_id, '-stardew-', '-meadowbrook-')
        WHERE question_id LIKE '%-stardew-%';
    ",
    // v17: the `fallout` theme became `rustwake` (an original wasteland):
    // learners keep wearing it, and their history of its themed questions
    // (ids like `a1-tener-fallout-01`) follows the new ids.
    "
    UPDATE users SET theme = 'rustwake' WHERE theme = 'fallout';
    UPDATE quiz_history SET question_id = REPLACE(question_id, '-fallout-', '-rustwake-')
        WHERE question_id LIKE '%-fallout-%';
    ",
    // v18: the `wow` theme became `realm` (an original world); keep what
    // learners wear and the themed question ids in their history.
    "
    UPDATE users SET theme = 'realm' WHERE theme = 'wow';
    UPDATE quiz_history SET question_id = REPLACE(question_id, '-wow-', '-realm-')
        WHERE question_id LIKE '%-wow-%';
    ",
    // v19: revising an AI course from the learner's feedback. While a
    // revision runs the course is 'revising' (it still opens; its files cannot
    // change). The new files replace the old ones in one go, and the old set
    // is kept here so the last revision can be undone. One backup per course.
    "
    CREATE TABLE course_backups (
        course_id  TEXT    PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
        title      TEXT    NOT NULL,
        outline    TEXT,
        feedback   TEXT    NOT NULL,   -- what the revision was asked to change
        created_at INTEGER NOT NULL
    );
    CREATE TABLE course_backup_files (
        course_id  TEXT    NOT NULL REFERENCES course_backups(course_id) ON DELETE CASCADE,
        path       TEXT    NOT NULL,
        content    TEXT    NOT NULL,
        PRIMARY KEY (course_id, path)
    ) WITHOUT ROWID;
    ",
    // v20: themes learners make (src/themes.rs): the files of a built-in
    // theme (`theme.json`, `pictures/*.svg`), private to their owner. Worn
    // as `user-<id>` in `users.theme`.
    "
    CREATE TABLE user_themes (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,   -- never reused: ids end up in caches and lessons
        owner_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        title      TEXT    NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX idx_user_themes_owner ON user_themes(owner_id);
    CREATE TABLE user_theme_files (
        theme_id INTEGER NOT NULL REFERENCES user_themes(id) ON DELETE CASCADE,
        path     TEXT    NOT NULL,
        content  TEXT    NOT NULL,
        PRIMARY KEY (theme_id, path)
    ) WITHOUT ROWID;
    ",
    // v21: an undo history instead of one backup. Every change to a course as
    // a whole (feedback, a lesson added or removed, a new order) keeps the
    // files from before it as a step; Undo takes the newest step off. Ten
    // steps per course at most. `label` says what the change was.
    "
    CREATE TABLE course_undo (
        course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
        seq        INTEGER NOT NULL,
        title      TEXT    NOT NULL,
        outline    TEXT,
        label      TEXT    NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (course_id, seq)
    ) WITHOUT ROWID;
    CREATE TABLE course_undo_files (
        course_id  TEXT    NOT NULL,
        seq        INTEGER NOT NULL,
        path       TEXT    NOT NULL,
        content    TEXT    NOT NULL,
        PRIMARY KEY (course_id, seq, path),
        FOREIGN KEY (course_id, seq) REFERENCES course_undo(course_id, seq) ON DELETE CASCADE
    ) WITHOUT ROWID;
    INSERT INTO course_undo (course_id, seq, title, outline, label, created_at)
        SELECT course_id, 1, title, outline, 'Changed with feedback: “' || feedback || '”', created_at FROM course_backups;
    INSERT INTO course_undo_files (course_id, seq, path, content) SELECT course_id, 1, path, content FROM course_backup_files;
    DROP TABLE course_backup_files;
    DROP TABLE course_backups;
    ",
    // v22: the "I am N or older" confirmation at sign-up (src/privacy.rs,
    // `AgeGate`); a boolean, never a birth date. 0 for accounts from before it.
    "
    ALTER TABLE users ADD COLUMN age_confirmed INTEGER NOT NULL DEFAULT 0;
    ",
    // v23: Terms of Service acceptances (src/terms.rs), one row per version a
    // learner accepted, so the history is kept. No IP address is stored.
    "
    CREATE TABLE terms_acceptances (
        user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        version     TEXT    NOT NULL,
        accepted_at INTEGER NOT NULL,
        UNIQUE (user_id, version)
    );
    ",
    // v24: developer accounts (no UI sets `developer`: an UPDATE by hand, or
    // `./.dev-start --seed`). They got the profile's Dev section (gone with
    // #298), whose `mock_ai` option answered every model call of theirs from
    // the LLM fixtures (dropped in v40).
    "
    ALTER TABLE users ADD COLUMN developer INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE users ADD COLUMN mock_ai INTEGER NOT NULL DEFAULT 0;
    ",
    // v25: the e-mail outbox (src/mail.rs). A row is written before any
    // provider is called and the `mailer` thread sends it; `provider` is the
    // one that took it. Sent bodies are emptied (except in dev mode) and
    // finished rows deleted after 30 days. `user_id` NULL: about no account.
    "
    CREATE TABLE mail_outbox (
        id              INTEGER PRIMARY KEY,
        user_id         INTEGER REFERENCES users(id) ON DELETE CASCADE,
        to_addr         TEXT    NOT NULL,
        subject         TEXT    NOT NULL,
        body_text       TEXT    NOT NULL,
        body_html       TEXT    NOT NULL DEFAULT '',
        purpose         TEXT    NOT NULL,
        state           TEXT    NOT NULL DEFAULT 'queued',  -- queued | sent | failed
        attempts        INTEGER NOT NULL DEFAULT 0,
        next_attempt_at INTEGER NOT NULL,
        created_at      INTEGER NOT NULL,
        sent_at         INTEGER,
        provider        TEXT    NOT NULL DEFAULT '',
        last_error      TEXT    NOT NULL DEFAULT ''
    );
    CREATE INDEX idx_mail_outbox_due ON mail_outbox(state, next_attempt_at);
    CREATE INDEX idx_mail_outbox_user ON mail_outbox(user_id);
    ",
    // v26: no more usernames; an account is its e-mail address. SQLite cannot
    // drop a UNIQUE column, so `users` is rebuilt (with foreign keys off, see
    // `REBUILDS`, or the DROP would cascade into every table).
    "
    CREATE TABLE users_new (
        id            INTEGER PRIMARY KEY,
        password_hash TEXT    NOT NULL,
        created_at    INTEGER NOT NULL,
        google_sub    TEXT,
        email         TEXT,
        active_course TEXT,
        first_name    TEXT    NOT NULL DEFAULT '',
        last_name     TEXT    NOT NULL DEFAULT '',
        avatar        TEXT    NOT NULL DEFAULT 'owl',
        theme         TEXT    NOT NULL DEFAULT '',
        age_confirmed INTEGER NOT NULL DEFAULT 0,
        developer     INTEGER NOT NULL DEFAULT 0,
        mock_ai       INTEGER NOT NULL DEFAULT 0
    );
    INSERT INTO users_new (id, password_hash, created_at, google_sub, email, active_course, first_name, last_name, avatar, theme, age_confirmed, developer, mock_ai)
        SELECT id, password_hash, created_at, google_sub, email, active_course, first_name, last_name, avatar, theme, age_confirmed, developer, mock_ai FROM users;
    DROP TABLE users;
    ALTER TABLE users_new RENAME TO users;
    CREATE UNIQUE INDEX idx_users_google_sub ON users(google_sub);
    CREATE INDEX idx_users_email ON users(email);
    ",
    // v27: "Hide all AI features" (#147). With `ai_off` 1 the account
    // sees none of the features that call an AI provider (the tutor, course
    // generation, AI revisions, theme creation) and their routes refuse.
    "
    ALTER TABLE users ADD COLUMN ai_off INTEGER NOT NULL DEFAULT 0;
    ",
    // v28: the latest AI run on a course, for "Review AI changes" (#163): the
    // files it changed as they were before (`side` 'before') and as the AI
    // wrote them ('after'). A file missing on one side was added or removed.
    // One run per course; the next one replaces it.
    "
    CREATE TABLE course_ai_runs (
        course_id  TEXT    PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
        label      TEXT    NOT NULL,
        created_at INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE course_ai_run_files (
        course_id  TEXT    NOT NULL REFERENCES course_ai_runs(course_id) ON DELETE CASCADE,
        side       TEXT    NOT NULL,   -- 'before' | 'after'
        path       TEXT    NOT NULL,
        content    TEXT    NOT NULL,
        PRIMARY KEY (course_id, side, path)
    ) WITHOUT ROWID;
    ",
    // v29: e-mail verification (src/verify.rs). `email_verified_at` is when
    // the current address was confirmed (NULL: not yet; existing accounts
    // start unconfirmed). A link's token is stored only as its SHA-256;
    // rows older than 7 days are deleted by the hourly housekeeping.
    "
    ALTER TABLE users ADD COLUMN email_verified_at INTEGER;
    CREATE TABLE email_verifications (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        email      TEXT    NOT NULL,
        token_hash TEXT    NOT NULL UNIQUE,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        used_at    INTEGER
    );
    CREATE INDEX idx_email_verifications_user ON email_verifications(user_id);
    ",
    // v30: superusers (#220). `superuser` 1 shows the profile's Superuser
    // section, whose actions reach any account. No page sets it: an UPDATE
    // by hand, or the dev user in dev mode (`dev::ensure_user`).
    "
    ALTER TABLE users ADD COLUMN superuser INTEGER NOT NULL DEFAULT 0;
    ",
    // v31: eight named profile pictures (people and animals) replace the
    // ten icons (#180). Each old id becomes the picture that took its slot
    // (profile::ALIASES); the column's historic DEFAULT 'owl' stays and is
    // read as the default picture, and new accounts store today's id.
    "
    UPDATE users SET avatar = CASE avatar
        WHEN 'owl' THEN 'noor' WHEN 'fox' THEN 'kai' WHEN 'cat' THEN 'mochi' WHEN 'bear' THEN 'coco'
        WHEN 'rabbit' THEN 'sofia' WHEN 'robot' THEN 'arjun' WHEN 'rocket' THEN 'yuki' WHEN 'star' THEN 'amani'
        WHEN 'leaf' THEN 'sofia' WHEN 'bolt' THEN 'mochi' ELSE avatar END;
    ",
    // v32: the editor saves as you edit (#151, #190). The saves of one
    // editor page (`session`, made up by the page) share one Undo step for
    // up to ten minutes from `started_at`; `created_at` is its latest save.
    // Older steps have neither and are never added to.
    "
    ALTER TABLE course_undo ADD COLUMN session TEXT;
    ALTER TABLE course_undo ADD COLUMN started_at INTEGER;
    ",
    // v33: the level a learner starts a course at (#175): a unit id of a
    // course whose units are `levels`, '' for the beginning. Lessons of the
    // units below it are skipped: never offered, not passed.
    "
    ALTER TABLE projects ADD COLUMN start_unit TEXT NOT NULL DEFAULT '';
    ",
    // v34: a colour behind the profile picture (#227): a slot 1-8 of the
    // palette worn (profile::palette, a theme's `avatar_colors`), 0 for the
    // picture's own colour, which is how every account looked before.
    "
    ALTER TABLE users ADD COLUMN avatar_color INTEGER NOT NULL DEFAULT 0;
    ",
    // v35: no more "the picture's own colour" (#263): the colour is the
    // account's, so an account that picked none gets the slot its picture
    // wore (slot n is AVATARS[n - 1]'s colour) and keeps it when the picture
    // changes. 0 is left only on new accounts: profile::DEFAULT_COLOR.
    "
    UPDATE users SET avatar_color = CASE avatar
        WHEN 'noor' THEN 1 WHEN 'amani' THEN 2 WHEN 'yuki' THEN 3 WHEN 'kai' THEN 4
        WHEN 'arjun' THEN 5 WHEN 'sofia' THEN 6 WHEN 'mochi' THEN 7 WHEN 'coco' THEN 8
        ELSE 1 END
    WHERE avatar_color = 0;
    ",
    // v36 (#74): `sessions.session_token` holds the SHA-256 of the cookie's
    // token (`auth::hash_token`), not the token. SQLite has no SHA-256, so the
    // plain-text sessions are ended: everyone signs in once more. Every
    // account that accepted the Terms ticked the box that also states the
    // minimum age, so those are marked as age-confirmed.
    "
    DELETE FROM sessions;
    UPDATE users SET age_confirmed = 1 WHERE id IN (SELECT user_id FROM terms_acceptances);
    ",
    // v37 (#149): the text message (SMS) outbox (src/sms.rs), built like
    // `mail_outbox`. The per-account and per-number limits count its rows,
    // hence the two (…, created_at) indexes. Sent bodies are emptied (except in
    // dev mode) and finished rows deleted after 30 days.
    "
    CREATE TABLE sms_outbox (
        id              INTEGER PRIMARY KEY,
        user_id         INTEGER REFERENCES users(id) ON DELETE CASCADE,
        to_number       TEXT    NOT NULL,   -- E.164, e.g. +447700900123
        body_text       TEXT    NOT NULL,
        purpose         TEXT    NOT NULL,
        state           TEXT    NOT NULL DEFAULT 'queued',  -- queued | sent | failed
        attempts        INTEGER NOT NULL DEFAULT 0,
        next_attempt_at INTEGER NOT NULL,
        created_at      INTEGER NOT NULL,
        sent_at         INTEGER,
        provider        TEXT    NOT NULL DEFAULT '',
        last_error      TEXT    NOT NULL DEFAULT ''
    );
    CREATE INDEX idx_sms_outbox_due ON sms_outbox(state, next_attempt_at);
    CREATE INDEX idx_sms_outbox_user ON sms_outbox(user_id, created_at);
    CREATE INDEX idx_sms_outbox_number ON sms_outbox(to_number, created_at);
    ",
    // v38: the review keeps what the learner left out of the AI's changes
    // (#189): `excluded` is JSON (`review::Exclusions`), '' for nothing. And
    // `review_quiet` (#187): the review does not open by itself when an AI
    // job finishes; the "Review changes" button still opens it.
    "
    ALTER TABLE course_ai_runs ADD COLUMN excluded TEXT NOT NULL DEFAULT '';
    ALTER TABLE users ADD COLUMN review_quiet INTEGER NOT NULL DEFAULT 0;
    ",
    // v39 (#292): two-factor sign-in. The profile's phone number (E.164,
    // '' for none) and when it was confirmed by a texted code; the one
    // second step in use (`two_factor`: '' off, `sms`, `email` or `totp`).
    // `totp_keys`: the account's authenticator app, its secret sealed with
    // the AI API keys' vault (ChaCha20-Poly1305 under the server's master
    // secret, bound to the user: `secrets::seal_totp`; it must be readable
    // to work codes out, so it can't be hashed), unconfirmed while
    // enrolling, and the last time step accepted so a code works once.
    // `phone_verifications`, `two_factor_challenges` and
    // `two_factor_confirmations` (a Google-only account's code for turning
    // the second step off) hold texted or e-mailed codes as SHA-256 only
    // (with a random salt; a sign-in code also with the token in the
    // browser's cookie, which is itself stored only as its SHA-256), for
    // 10 minutes and 5 tries.
    "
    ALTER TABLE users ADD COLUMN phone TEXT NOT NULL DEFAULT '';
    ALTER TABLE users ADD COLUMN phone_verified_at INTEGER;
    ALTER TABLE users ADD COLUMN two_factor TEXT NOT NULL DEFAULT '';
    CREATE TABLE totp_keys (
        user_id      INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        secret       BLOB    NOT NULL,
        created_at   INTEGER NOT NULL,
        confirmed_at INTEGER,
        last_step    INTEGER
    );
    CREATE TABLE phone_verifications (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        phone      TEXT    NOT NULL,
        code_hash  TEXT    NOT NULL,
        salt       TEXT    NOT NULL,
        attempts   INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    );
    CREATE INDEX idx_phone_verifications_user ON phone_verifications(user_id);
    CREATE TABLE two_factor_challenges (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        token_hash TEXT    NOT NULL UNIQUE,
        method     TEXT    NOT NULL,
        code_hash  TEXT    NOT NULL DEFAULT '',
        salt       TEXT    NOT NULL DEFAULT '',
        attempts   INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    );
    CREATE INDEX idx_two_factor_challenges_user ON two_factor_challenges(user_id);
    CREATE TABLE two_factor_confirmations (
        user_id    INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        method     TEXT    NOT NULL,
        code_hash  TEXT    NOT NULL,
        salt       TEXT    NOT NULL,
        attempts   INTEGER NOT NULL DEFAULT 0,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL
    );
    ",
    // v40 (#298): the profile's "Mock AI responses" option is gone; in dev
    // mode the simulated provider answers through the seeded keys instead.
    "
    ALTER TABLE users DROP COLUMN mock_ai;
    ",
    // v41: the review's edits of the AI's lines (#287): `edits` is JSON
    // (`review::Edits`: path → row key → the learner's text), '' for none.
    "
    ALTER TABLE course_ai_runs ADD COLUMN edits TEXT NOT NULL DEFAULT '';
    ",
    // v42 (#462): the language the app speaks to this account in
    // (`i18n/<code>.yaml`); '' until chosen, then the request's cookie or
    // browser language decides.
    "
    ALTER TABLE users ADD COLUMN lang TEXT NOT NULL DEFAULT '';
    ",
    // v43 (#451): a course shared with other learners by e-mail address
    // (src/share.rs). One row per address invited: the link's token is kept
    // as its SHA-256 only; `user_id` is the account that opened the link
    // (NULL until then, or when that account is deleted).
    "
    CREATE TABLE course_shares (
        id         INTEGER PRIMARY KEY,
        course_id  TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
        email      TEXT    NOT NULL,
        token_hash TEXT    NOT NULL UNIQUE,
        user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,
        created_at INTEGER NOT NULL,
        opened_at  INTEGER,
        UNIQUE (course_id, email)
    );
    CREATE INDEX idx_course_shares_user ON course_shares(user_id);
    CREATE INDEX idx_course_shares_email ON course_shares(email);
    ",
    // v44 (#430): "Download my data" as a background job (exports.rs).
    // `state`: queued, building, ready (the link was mailed), downloaded,
    // expired or failed. `token_hash` is the SHA-256 of the mailed link's
    // token ('' once used or expired); `file` the name in the exports
    // folder ('' once deleted); `link_base` where the link points (the
    // request's site, as for verification links).
    "
    CREATE TABLE data_exports (
        id            INTEGER PRIMARY KEY,
        user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        state         TEXT    NOT NULL DEFAULT 'queued',
        token_hash    TEXT    NOT NULL DEFAULT '',
        file          TEXT    NOT NULL DEFAULT '',
        link_base     TEXT    NOT NULL DEFAULT '',
        bytes         INTEGER NOT NULL DEFAULT 0,
        error         TEXT    NOT NULL DEFAULT '',
        created_at    INTEGER NOT NULL,
        ready_at      INTEGER,
        expires_at    INTEGER,
        downloaded_at INTEGER
    );
    CREATE INDEX idx_data_exports_user ON data_exports(user_id);
    CREATE INDEX idx_data_exports_state ON data_exports(state);
    ",
    // v45 (#364, #394): account ids are never reused. Without AUTOINCREMENT
    // SQLite gives a new row max(id)+1, so deleting the newest account and
    // registering again handed the deleted account's id (and the Stripe
    // events still naming it) to someone else. `users` is rebuilt (foreign
    // keys off, see `REBUILDS`) with every column and index kept, plus:
    // `billing_ref`, a random per-account reference Stripe Checkout carries
    // in the subscription's metadata instead of the row id, and
    // `stripe_customer`, the one Stripe customer every Checkout of the
    // account shares (created before the first one).
    "
    CREATE TABLE users_new (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        password_hash     TEXT    NOT NULL,
        created_at        INTEGER NOT NULL,
        google_sub        TEXT,
        email             TEXT,
        active_course     TEXT,
        first_name        TEXT    NOT NULL DEFAULT '',
        last_name         TEXT    NOT NULL DEFAULT '',
        avatar            TEXT    NOT NULL DEFAULT 'owl',
        theme             TEXT    NOT NULL DEFAULT '',
        age_confirmed     INTEGER NOT NULL DEFAULT 0,
        developer         INTEGER NOT NULL DEFAULT 0,
        ai_off            INTEGER NOT NULL DEFAULT 0,
        email_verified_at INTEGER,
        superuser         INTEGER NOT NULL DEFAULT 0,
        avatar_color      INTEGER NOT NULL DEFAULT 0,
        review_quiet      INTEGER NOT NULL DEFAULT 0,
        phone             TEXT    NOT NULL DEFAULT '',
        phone_verified_at INTEGER,
        two_factor        TEXT    NOT NULL DEFAULT '',
        lang              TEXT    NOT NULL DEFAULT '',
        billing_ref       TEXT    NOT NULL DEFAULT (lower(hex(randomblob(16)))),
        stripe_customer   TEXT
    );
    INSERT INTO users_new (id, password_hash, created_at, google_sub, email, active_course, first_name, last_name, avatar, theme,
                           age_confirmed, developer, ai_off, email_verified_at, superuser, avatar_color, review_quiet, phone,
                           phone_verified_at, two_factor, lang, stripe_customer)
        SELECT u.id, u.password_hash, u.created_at, u.google_sub, u.email, u.active_course, u.first_name, u.last_name, u.avatar, u.theme,
               u.age_confirmed, u.developer, u.ai_off, u.email_verified_at, u.superuser, u.avatar_color, u.review_quiet, u.phone,
               u.phone_verified_at, u.two_factor, u.lang, (SELECT s.customer_id FROM subscriptions s WHERE s.user_id = u.id)
        FROM users u;
    DROP TABLE users;
    ALTER TABLE users_new RENAME TO users;
    CREATE UNIQUE INDEX idx_users_google_sub ON users(google_sub);
    CREATE INDEX idx_users_email ON users(email);
    CREATE UNIQUE INDEX idx_users_billing_ref ON users(billing_ref);
    CREATE INDEX idx_users_stripe_customer ON users(stripe_customer);
    ",
    // v46 (#362)(#363): the SMS limits count a ledger that outlives the
    // account. `sms_sends` has one row per text queued: the account (set
    // NULL when it is deleted, so the row stays and still counts), the
    // number as a SHA-256 (`sms::number_key`; the limits only need to match
    // it), the allowance it drew on (`signin` or `other`, so verification
    // texts cannot use up a number's sign-in codes) and when. Rows go after
    // two days; the limits look back one. And a phone number can be
    // verified on one account only: where two accounts had verified the
    // same number, the later one's verification (and its text-message
    // second step, which needs it) is undone before the unique index.
    "
    CREATE TABLE sms_sends (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,
        number_key TEXT    NOT NULL,
        kind       TEXT    NOT NULL,   -- signin | other
        created_at INTEGER NOT NULL
    );
    CREATE INDEX idx_sms_sends_number ON sms_sends(number_key, kind, created_at);
    CREATE INDEX idx_sms_sends_user ON sms_sends(user_id, created_at);
    CREATE INDEX idx_sms_sends_kind ON sms_sends(kind, created_at);
    UPDATE users SET phone_verified_at = NULL, two_factor = CASE WHEN two_factor = 'sms' THEN '' ELSE two_factor END
     WHERE phone_verified_at IS NOT NULL AND phone <> '' AND EXISTS (
        SELECT 1 FROM users o WHERE o.phone = users.phone AND o.id <> users.id AND o.phone_verified_at IS NOT NULL
           AND (o.phone_verified_at < users.phone_verified_at OR (o.phone_verified_at = users.phone_verified_at AND o.id < users.id)));
    CREATE UNIQUE INDEX idx_users_phone_verified ON users(phone) WHERE phone_verified_at IS NOT NULL AND phone <> '';
    ",
    // v47 (#558): activity from every mode in one place (`activity.rs`).
    // `activity_log` has a row per thing done (a kind and how many: three
    // answers, a round of 13 pairs), which the daily goal weighs into effort
    // points; `lesson_touches` keeps, per lesson, when and in which mode it
    // was last opened or worked on, so every mode resumes at the same lesson.
    "
    CREATE TABLE activity_log (
        id        INTEGER PRIMARY KEY,
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id TEXT    NOT NULL,
        kind      TEXT    NOT NULL,   -- activity::Kind::key
        n         INTEGER NOT NULL,
        at        INTEGER NOT NULL
    );
    CREATE INDEX idx_activity_log_user ON activity_log(user_id, course_id, at);
    CREATE INDEX idx_activity_log_at ON activity_log(at);
    CREATE TABLE lesson_touches (
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id TEXT    NOT NULL,
        lesson_id TEXT    NOT NULL,
        mode      TEXT    NOT NULL,   -- activity::Mode::key
        at        INTEGER NOT NULL,
        PRIMARY KEY (user_id, course_id, lesson_id)
    );
    ",
    // v48 (#559): the motivator worn (`motivators.rs`), '' for the default.
    "ALTER TABLE users ADD COLUMN motivator TEXT NOT NULL DEFAULT '';",
    // v49 (#565): the theme a learner wears in one course, over the
    // profile's (`users.theme`): NULL for the profile's, '' for Default, else
    // a theme id. The course picker always wears the profile's.
    "ALTER TABLE projects ADD COLUMN theme TEXT;",
    // v50 (#565): the profile picture and its background are per course,
    // picked in the course's settings: each course starts with the account's
    // (`users.avatar`, `users.avatar_color`), which the profile no longer edits.
    "
    ALTER TABLE projects ADD COLUMN avatar TEXT NOT NULL DEFAULT '';
    ALTER TABLE projects ADD COLUMN avatar_color INTEGER NOT NULL DEFAULT 0;
    UPDATE projects SET avatar = (SELECT u.avatar FROM users u WHERE u.id = projects.user_id),
                        avatar_color = (SELECT u.avatar_color FROM users u WHERE u.id = projects.user_id);
    ",
    // v51 (#574): the motivator is per course too, starting as the account's
    // (`users.motivator`, which the profile no longer edits), and `set_up`
    // is 0 until the learner leaves the settings wizard a course shows on
    // its first visit. Courses already opened have been started.
    "
    ALTER TABLE projects ADD COLUMN motivator TEXT NOT NULL DEFAULT '';
    ALTER TABLE projects ADD COLUMN set_up INTEGER NOT NULL DEFAULT 0;
    UPDATE projects SET motivator = (SELECT u.motivator FROM users u WHERE u.id = projects.user_id), set_up = 1;
    ",
    // v52 (#577): the profile has no theme any more, so a course is never on
    // "Same as profile": those wear Default. `users.theme` is no longer read.
    "UPDATE projects SET theme = '' WHERE theme IS NULL;",
    // v53 (#299, #56): AI metering (src/metering.rs). One `ai_usage` row per model call,
    // hosted (the server's key: costed, charged to the monthly allowance and
    // then to credits) or own (the learner's key: cost 0). `users.ai_credits`
    // is the prepaid balance; `ai_credit_purchases` the Stripe payments that
    // filled it, so a retried webhook adds nothing and a refund takes back
    // its share. Amounts are micro-dollars.
    "
    CREATE TABLE ai_usage (
        id            INTEGER PRIMARY KEY,
        user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id     TEXT    NOT NULL DEFAULT '',
        job_id        TEXT    NOT NULL DEFAULT '',
        purpose       TEXT    NOT NULL,              -- ai::Call purpose: tutor-turn, generate-lesson, …
        provider      TEXT    NOT NULL,              -- anthropic | openai | google
        model         TEXT    NOT NULL,
        input_tokens  INTEGER NOT NULL DEFAULT 0,    -- not counting cached ones
        output_tokens INTEGER NOT NULL DEFAULT 0,
        cached_tokens INTEGER NOT NULL DEFAULT 0,
        cost_micros   INTEGER NOT NULL DEFAULT 0,    -- from the price table; 0 for own keys
        credit_micros INTEGER NOT NULL DEFAULT 0,    -- the part of the cost paid from credits
        key_source    TEXT    NOT NULL,              -- 'hosted' | 'own'
        created_at    INTEGER NOT NULL
    );
    CREATE INDEX idx_ai_usage_user ON ai_usage(user_id, created_at);
    CREATE INDEX idx_ai_usage_day ON ai_usage(key_source, created_at);
    ALTER TABLE users ADD COLUMN ai_credits INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE ai_credit_purchases (
        session_id      TEXT    PRIMARY KEY,         -- Stripe Checkout cs_…
        user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        payment_intent  TEXT    NOT NULL DEFAULT '', -- pi_…, how a refund finds the purchase
        price_id        TEXT    NOT NULL DEFAULT '',
        amount_cents    INTEGER NOT NULL DEFAULT 0,
        credit_micros   INTEGER NOT NULL,
        refunded_micros INTEGER NOT NULL DEFAULT 0,
        created_at      INTEGER NOT NULL
    );
    CREATE INDEX idx_ai_credit_purchases_pi ON ai_credit_purchases(payment_intent);
    ",
    // v54 (#315): plans bought on the profile (billing::subscribe). Plus or
    // Pro, renewing month after month until cancelled; cancelled, the plan
    // is kept to `renews_at` and the learner is Free after. Nothing is
    // charged yet (a payment provider is a separate task). Each cancellation
    // keeps the reason and feedback the learner gave. `users.own_keys_fallback`:
    // on Pro, "use my keys when my Skrongle AI limit is hit".
    "
    CREATE TABLE plan_subscriptions (
        user_id              INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
        plan                 TEXT    NOT NULL,              -- 'plus' | 'pro'
        started_at           INTEGER NOT NULL,
        renews_at            INTEGER NOT NULL,              -- the end of the paid month on record
        cancel_at_period_end INTEGER NOT NULL DEFAULT 0,    -- 1: ends at renews_at instead of renewing
        updated_at           INTEGER NOT NULL
    );
    CREATE TABLE plan_cancellations (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        plan       TEXT    NOT NULL,
        reason     TEXT    NOT NULL DEFAULT '',             -- billing::CANCEL_REASONS id, or ''
        feedback   TEXT    NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL
    );
    CREATE INDEX idx_plan_cancellations_user ON plan_cancellations(user_id, created_at);
    ALTER TABLE users ADD COLUMN own_keys_fallback INTEGER NOT NULL DEFAULT 0;
    ",
    // v55 (#314): which plan a Stripe subscription pays for ('plus' or
    // 'pro'), from Checkout's `subscription_data[metadata][plan]`. Every
    // subscription before this was Pro.
    "
    ALTER TABLE subscriptions ADD COLUMN plan TEXT NOT NULL DEFAULT 'pro';
    ",
    // v56 (#589): what a learner has earned in a course so far, kept as one
    // row (`earnings.rs`) instead of being added up from the whole
    // `activity_log` on every visit: the effort points ever logged, the days
    // whose daily goal was reached, the local day last active (`day`, its
    // number: (at + tz) / 86400; NULL before any activity), the points logged
    // that day and the day streak ending on it. `activity::record` keeps it
    // up to date; a learner from before this gets their row filled in from
    // their history once, on first use.
    "
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
    ",
    // v57 (#602, #603): a cross-learner count read one
    // course's last eight days. These make both reads index ranges over that
    // course and window, instead of a scan of every pass ever made and of
    // every course's week of activity.
    "
    CREATE INDEX idx_lesson_passes_course_passed ON lesson_passes(course_id, passed_at);
    CREATE INDEX idx_activity_log_course_at ON activity_log(course_id, at);
    ",
    // v58 (#579, #607-#613): the outboxes' caps and purges read through
    // indexes instead of scanning every row. `mail_outbox.scheduled_at`: the
    // window whose hourly budget a message counts against (`mail::enqueue`);
    // a message over its purpose's ceiling waits for a later window instead
    // of being dropped, so this is when it was first due, not when it was
    // queued. The `_unblanked` partial indexes hold only rows that still
    // carry a body (queued ones, and settled ones `blank_settled` has yet to
    // empty); their WHERE must stay word for word the one in
    // `mail::blank_settled` / `sms::blank_settled`, or the planner skips them.
    "
    ALTER TABLE mail_outbox ADD COLUMN scheduled_at INTEGER NOT NULL DEFAULT 0;
    UPDATE mail_outbox SET scheduled_at = created_at;
    CREATE INDEX idx_mail_outbox_addr ON mail_outbox(lower(to_addr), purpose, created_at);
    CREATE INDEX idx_mail_outbox_scheduled ON mail_outbox(purpose, scheduled_at);
    CREATE INDEX idx_mail_outbox_created ON mail_outbox(created_at);
    CREATE INDEX idx_mail_outbox_unblanked ON mail_outbox(state)
        WHERE body_text <> '' OR body_html <> '' OR (purpose IN ('login-code', 'two-factor-confirm', 'email-verify', 'data-export') AND subject <> '');
    CREATE INDEX idx_sms_outbox_created ON sms_outbox(created_at);
    CREATE INDEX idx_sms_outbox_unblanked ON sms_outbox(state) WHERE body_text <> '';
    CREATE INDEX idx_totp_keys_pending ON totp_keys(created_at) WHERE confirmed_at IS NULL;
    ",
    // v59 (#625, #626): indexes for the data export. It reads a learner's
    // rows of each table a page at a time in key order (`privacy::page`);
    // these let each page start at its key in the learner's own rows
    // instead of sorting them all (or, for `ai_credit_purchases`, every
    // learner's). `courses(owner_id, id)` replaces `(owner_id)`.
    // `data_exports`: the hourly purge deletes by `created_at`, and the
    // purge and the startup recovery read only the rows that still name a
    // file on disk.
    "
    CREATE INDEX idx_history_user_id ON quiz_history(user_id, id);
    CREATE INDEX idx_game_scores_user_id ON game_scores(user_id, id);
    CREATE INDEX idx_game_activity_user_id ON game_activity(user_id, id);
    CREATE INDEX idx_activity_log_user_id ON activity_log(user_id, id);
    CREATE INDEX idx_plan_cancellations_user_id ON plan_cancellations(user_id, id);
    CREATE INDEX idx_ai_usage_user_id ON ai_usage(user_id, id);
    CREATE INDEX idx_ai_credit_purchases_user ON ai_credit_purchases(user_id, session_id);
    CREATE INDEX idx_terms_acceptances_user ON terms_acceptances(user_id);
    CREATE INDEX idx_sms_outbox_user_id ON sms_outbox(user_id, id);
    CREATE INDEX idx_sms_sends_user_id ON sms_sends(user_id, id);
    DROP INDEX idx_courses_owner;
    CREATE INDEX idx_courses_owner_id ON courses(owner_id, id);
    CREATE INDEX idx_data_exports_created ON data_exports(created_at);
    CREATE INDEX idx_data_exports_files ON data_exports(file, state, expires_at, created_at) WHERE file <> '';
    ",
    // v60 (#614–#620): the housekeeping sweeps and deletions find their rows
    // through an index instead of reading whole tables. History retention
    // deletes by time (`quiz_history.answered_at`, `game_activity.played_at`);
    // the course sweeps look up a state (failed and old, or interrupted at
    // startup), and the notes sweep only sees courses that still hold notes;
    // deleting a course or an account clears `projects` and the learners
    // who had it open or wore its owner's theme (`ai_credit_purchases` is
    // covered by v59's `(user_id, session_id)`).
    "
    CREATE INDEX idx_history_answered ON quiz_history(answered_at);
    CREATE INDEX idx_game_activity_played ON game_activity(played_at);
    CREATE INDEX idx_courses_state_updated ON courses(state, updated_at);
    CREATE INDEX idx_courses_with_notes ON courses(state) WHERE notes <> '';
    CREATE INDEX idx_projects_course ON projects(course_id);
    CREATE INDEX idx_users_active_course ON users(active_course) WHERE active_course IS NOT NULL;
    CREATE INDEX idx_users_theme ON users(theme) WHERE theme <> '';
    ",
    // v61 (#634): invitation links for `SKRONGLE_SIGNUP=invite`
    // (`signup.rs`): made by a superuser, one-time, 7 days. No `created_by`:
    // the owner's link, made at startup while there is no account. The
    // link's token is stored as its SHA-256 only. `used_by` keeps the row
    // (as used) when the invited account is deleted.
    "
    CREATE TABLE signup_invites (
        id         INTEGER PRIMARY KEY,
        token_hash TEXT    NOT NULL UNIQUE,
        created_by INTEGER REFERENCES users(id) ON DELETE CASCADE,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        used_at    INTEGER,
        used_by    INTEGER REFERENCES users(id) ON DELETE SET NULL
    );
    CREATE INDEX idx_signup_invites_created_by ON signup_invites(created_by);
    CREATE INDEX idx_signup_invites_used_by ON signup_invites(used_by);
    ",
    // v62 (#635): a temporary password set by the owner (the Superuser
    // section, or `reset-password` on the command line, `owner.rs`): the
    // learner must choose a new one before anything else (`CurrentUser`).
    "
    ALTER TABLE users ADD COLUMN must_change_password INTEGER NOT NULL DEFAULT 0;
    ",
    // v63 (#604, #605, #606, #622): accounts are found by address
    // case-insensitively, always as `lower(email) = lower(?)`, which this
    // index serves (the plain one on `email` cannot). The accounts in a
    // course, for taking a share away, are found by `active_course` through
    // v60's `idx_users_active_course`.
    "
    CREATE INDEX idx_users_email_lower ON users(lower(email));
    ",
    // v64 (#592-#601): game queries that stay small however long a learner's
    // history and however many learners. A learner's best on a board is one
    // index lookup (`idx_game_scores_user_board`); the board's top scores read
    // in order from `idx_game_scores_course_board`, now with `played_at` for
    // the tie-break. The two older score indexes are covered by these.
    // A game's recent rounds read from `idx_game_activity_game` (and per
    // board from `idx_game_activity_board`) instead of walking every round of
    // the course; `game_rounds` counts each game's rounds so a page does not
    // count them (`games::record_activity_at` moves it; filled in here once).
    "
    DROP INDEX IF EXISTS idx_game_scores_user;
    DROP INDEX IF EXISTS idx_game_scores_board;
    DROP INDEX IF EXISTS idx_game_scores_course_board;
    CREATE INDEX idx_game_scores_user_board ON game_scores(user_id, course_id, game, board, score_ms);
    CREATE INDEX idx_game_scores_course_board ON game_scores(course_id, game, board, score_ms, played_at);
    CREATE INDEX idx_game_activity_game ON game_activity(user_id, course_id, game, played_at DESC);
    CREATE INDEX idx_game_activity_board ON game_activity(user_id, course_id, game, board, played_at DESC);
    CREATE TABLE game_rounds (
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id TEXT    NOT NULL,
        game      TEXT    NOT NULL,
        rounds    INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (user_id, course_id, game)
    ) WITHOUT ROWID;
    INSERT INTO game_rounds (user_id, course_id, game, rounds)
        SELECT user_id, course_id, game, COUNT(*) FROM game_activity GROUP BY user_id, course_id, game;
    ",
    // v65 (#623, #624): running totals of `ai_usage`, so the meter reads one
    // row instead of adding up rows on every model call. `ai_usage_months`:
    // per learner and calendar month (UTC; `month` is its first second,
    // `metering::month_start`) the hosted spend, the part paid from credits
    // and the hosted and own-key call counts. `ai_spend_days`: per UTC day
    // (`day` is its first second, `metering::day_start`) the hosted spend of
    // everyone, for the daily cap. Triggers on `ai_usage` move both, so every
    // path that inserts or deletes usage rows (recording a call, deleting an
    // account, a dev reset) keeps them equal to the old sums; `ai_usage`
    // rows are never updated. A new month or day starts with no row (zero).
    "
    CREATE TABLE ai_usage_months (
        user_id       INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        month         INTEGER NOT NULL,
        cost_micros   INTEGER NOT NULL DEFAULT 0,
        credit_micros INTEGER NOT NULL DEFAULT 0,
        hosted_calls  INTEGER NOT NULL DEFAULT 0,
        own_calls     INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (user_id, month)
    ) WITHOUT ROWID;
    CREATE TABLE ai_spend_days (
        day         INTEGER PRIMARY KEY,
        cost_micros INTEGER NOT NULL DEFAULT 0
    );
    INSERT INTO ai_usage_months (user_id, month, cost_micros, credit_micros, hosted_calls, own_calls)
        SELECT user_id, CAST(strftime('%s', created_at, 'unixepoch', 'start of month') AS INTEGER),
               COALESCE(SUM(CASE WHEN key_source = 'hosted' THEN cost_micros END), 0),
               COALESCE(SUM(CASE WHEN key_source = 'hosted' THEN credit_micros END), 0),
               COUNT(CASE WHEN key_source = 'hosted' THEN 1 END),
               COUNT(CASE WHEN key_source = 'own' THEN 1 END)
        FROM ai_usage GROUP BY 1, 2;
    INSERT INTO ai_spend_days (day, cost_micros)
        SELECT created_at - created_at % 86400, SUM(cost_micros) FROM ai_usage WHERE key_source = 'hosted' GROUP BY 1;
    CREATE TRIGGER ai_usage_totals_add AFTER INSERT ON ai_usage BEGIN
        INSERT INTO ai_usage_months (user_id, month, cost_micros, credit_micros, hosted_calls, own_calls)
            VALUES (NEW.user_id, CAST(strftime('%s', NEW.created_at, 'unixepoch', 'start of month') AS INTEGER),
                    CASE WHEN NEW.key_source = 'hosted' THEN NEW.cost_micros ELSE 0 END,
                    CASE WHEN NEW.key_source = 'hosted' THEN NEW.credit_micros ELSE 0 END,
                    NEW.key_source = 'hosted', NEW.key_source = 'own')
            ON CONFLICT (user_id, month) DO UPDATE SET
                cost_micros = cost_micros + excluded.cost_micros,
                credit_micros = credit_micros + excluded.credit_micros,
                hosted_calls = hosted_calls + excluded.hosted_calls,
                own_calls = own_calls + excluded.own_calls;
        INSERT INTO ai_spend_days (day, cost_micros)
            SELECT NEW.created_at - NEW.created_at % 86400, NEW.cost_micros WHERE NEW.key_source = 'hosted'
            ON CONFLICT (day) DO UPDATE SET cost_micros = cost_micros + excluded.cost_micros;
    END;
    CREATE TRIGGER ai_usage_totals_remove AFTER DELETE ON ai_usage BEGIN
        UPDATE ai_usage_months SET
                cost_micros = cost_micros - CASE WHEN OLD.key_source = 'hosted' THEN OLD.cost_micros ELSE 0 END,
                credit_micros = credit_micros - CASE WHEN OLD.key_source = 'hosted' THEN OLD.credit_micros ELSE 0 END,
                hosted_calls = hosted_calls - (OLD.key_source = 'hosted'),
                own_calls = own_calls - (OLD.key_source = 'own')
            WHERE user_id = OLD.user_id AND month = CAST(strftime('%s', OLD.created_at, 'unixepoch', 'start of month') AS INTEGER);
        UPDATE ai_spend_days SET cost_micros = cost_micros - OLD.cost_micros
            WHERE OLD.key_source = 'hosted' AND day = OLD.created_at - OLD.created_at % 86400;
    END;
    ",
    // v66 (#379): password reset (src/reset.rs). A link's token is stored
    // only as its SHA-256; it works once, for an hour, while the account
    // still has the address it was sent to. Rows older than 7 days are
    // deleted by the hourly housekeeping.
    "
    CREATE TABLE password_resets (
        id         INTEGER PRIMARY KEY,
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        email      TEXT    NOT NULL,
        token_hash TEXT    NOT NULL UNIQUE,
        created_at INTEGER NOT NULL,
        expires_at INTEGER NOT NULL,
        used_at    INTEGER
    );
    CREATE INDEX idx_password_resets_user ON password_resets(user_id);
    ",
    // v67 (#966): who was e-mailed about which material change of the
    // built-in terms (src/terms.rs, `announce_upcoming`), so pressing Send
    // on /superuser again never mails anyone twice.
    "
    CREATE TABLE terms_announcements (
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        version   TEXT    NOT NULL,
        mailed_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, version)
    );
    ",
    // v68 (#968): content reports and copyright notices (src/content_reports.rs).
    // `user_id` is the reporter (NULL for a notice sent signed out); the
    // course is named by id without a foreign key, so the record outlives a
    // course taken down or deleted.
    "
    CREATE TABLE content_reports (
        id           INTEGER PRIMARY KEY,
        user_id      INTEGER REFERENCES users(id) ON DELETE CASCADE,
        course_id    TEXT,
        course_title TEXT    NOT NULL DEFAULT '',
        location     TEXT    NOT NULL DEFAULT '',
        reason       TEXT    NOT NULL,
        email        TEXT    NOT NULL,
        name         TEXT    NOT NULL DEFAULT '',
        work         TEXT    NOT NULL DEFAULT '',
        details      TEXT    NOT NULL,
        good_faith   INTEGER NOT NULL DEFAULT 0,
        created_at   INTEGER NOT NULL,
        state        TEXT    NOT NULL DEFAULT 'open',
        forwarded_at INTEGER,
        resolved_at  INTEGER
    );
    CREATE INDEX idx_content_reports_state ON content_reports(state, created_at);
    CREATE INDEX idx_content_reports_user ON content_reports(user_id);
    ",
    // v69 (#969): the account's number opted out of texts (a STOP reply):
    // set when a provider refuses a text to it for that reason, cleared when
    // a text to it goes through or the number changes. Sign-in codes go by
    // e-mail meanwhile, and the challenge remembers it did (`sent_by`).
    "
    ALTER TABLE users ADD COLUMN phone_opted_out_at INTEGER;
    ALTER TABLE two_factor_challenges ADD COLUMN sent_by TEXT NOT NULL DEFAULT '';
    ",
    // v70 (#967): superuser moderation (src/moderation.rs). A made course
    // taken down stops opening for everyone until it is put back; a
    // suspended account cannot sign in and its courses are not shared out;
    // a blocked address or range gets a 403 on every request.
    "
    ALTER TABLE courses ADD COLUMN taken_down_at INTEGER;
    ALTER TABLE users ADD COLUMN suspended_at INTEGER;
    CREATE TABLE blocked_ips (
        id         INTEGER PRIMARY KEY,
        cidr       TEXT    NOT NULL UNIQUE,
        created_at INTEGER NOT NULL
    );
    ",
    // v71 (#569): badges bought in the Skrongle Cash shop
    // (motivators/shop.rs), one row per badge per course. The price paid is
    // kept, so a later price change never moves anyone's balance.
    "
    CREATE TABLE motivator_purchases (
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id  TEXT    NOT NULL,
        item       TEXT    NOT NULL,
        price      INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, course_id, item)
    );
    ",
    // v72 (#872): background jobs (src/jobs.rs). One row per job: `kind`
    // names what runs, `run_at` when it may; the runner thread takes a due
    // row by setting `state = 'running'` and a lease (`locked_by`,
    // `locked_until`), and a running row whose lease ran out is taken again.
    // A failure goes back to `queued` with a later `run_at` until
    // `max_attempts`, then `failed` with `last_error`. The hourly
    // housekeeping is the first user: one row per purge.
    "
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
    ",
    // v73 (#920): the plans were renamed. What was Plus ($1, the cheap tier)
    // is Basic (CA$3); what was Pro ($15) is Plus. One statement per table so
    // no row is mapped twice. `subscriptions.plan` keeps its historical
    // DEFAULT 'pro', which `billing::Plan::parse` still reads as Plus.
    "
    UPDATE plan_subscriptions  SET plan = CASE plan WHEN 'pro' THEN 'plus' WHEN 'plus' THEN 'basic' ELSE plan END;
    UPDATE plan_cancellations  SET plan = CASE plan WHEN 'pro' THEN 'plus' WHEN 'plus' THEN 'basic' ELSE plan END;
    UPDATE subscriptions       SET plan = CASE plan WHEN 'pro' THEN 'plus' WHEN 'plus' THEN 'basic' ELSE plan END;
    ",    // v74 (#1033): the content flag of a made course, one row per course
    // (src/course_flags.rs). `state` 'clear' | 'flagged' | 'overridden'; a
    // flagged course doesn't start or share. `source` is the check that set
    // it: 'terms' (the listed-term check) or 'ai' (the AI scan, #1036/#1037).
    // `matches_json` is what triggered it: [{term, path, field, lesson,
    // text}]. `reason` is a short text for the superuser (the AI report).
    // `content_hash` is the SHA-256 of what was checked, `checked_at` when.
    // The override columns are a superuser's decision (#1034), read by the
    // AI's second stage (#1037).
    "
    CREATE TABLE course_flags (
        course_id     TEXT    PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
        state         TEXT    NOT NULL DEFAULT 'clear' CHECK (state IN ('clear', 'flagged', 'overridden')),
        source        TEXT    NOT NULL DEFAULT 'terms' CHECK (source IN ('terms', 'ai')),
        matches_json  TEXT    NOT NULL DEFAULT '[]',
        reason        TEXT    NOT NULL DEFAULT '',
        content_hash  TEXT    NOT NULL DEFAULT '',
        checked_at    INTEGER NOT NULL,
        override_note TEXT,
        overridden_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
        overridden_at INTEGER
    );
    CREATE INDEX idx_course_flags_state ON course_flags(state) WHERE state <> 'clear';
    CREATE INDEX idx_course_flags_overridden_by ON course_flags(overridden_by) WHERE overridden_by IS NOT NULL;
    ",    // v75 (#1036): the AI content check's stage 1 (src/content_check.rs),
    // one row per part of a made course: a lesson file, or `path#2`… for
    // the later parts of a long one. `hash` is the SHA-256 of what Jev was
    // sent; a part with the same hash and model is not sent again. The two
    // scores are Jev's probabilities (0..1); `cost_usd` what the call cost.
    // `review` is NULL below the threshold, 'queued' for stage 2 (#1037).
    "
    CREATE TABLE content_scores (
        course_id       TEXT    NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
        chunk           TEXT    NOT NULL,
        lesson          TEXT    NOT NULL DEFAULT '',
        hash            TEXT    NOT NULL,
        terms_score     REAL    NOT NULL,
        self_harm_score REAL    NOT NULL,
        model           TEXT    NOT NULL,
        input_tokens    INTEGER NOT NULL DEFAULT 0,
        cost_usd        REAL    NOT NULL DEFAULT 0,
        review          TEXT,
        scanned_at      INTEGER NOT NULL,
        PRIMARY KEY (course_id, chunk)
    );
    CREATE INDEX idx_content_scores_review ON content_scores(review) WHERE review IS NOT NULL;
    ",
    // v76 (#1037): stage 2 of the AI content check (src/content_check/review.rs) records its calls in
    // ai_usage under the course owner, purpose 'content-review', on the server's key: the day's spend
    // counts them, the owner's month doesn't (these undo what ai_usage_totals_* do there, in either order).
    "
    CREATE TRIGGER ai_usage_review_add AFTER INSERT ON ai_usage WHEN NEW.purpose = 'content-review' BEGIN
        INSERT INTO ai_usage_months (user_id, month, cost_micros, credit_micros, hosted_calls, own_calls)
            VALUES (NEW.user_id, CAST(strftime('%s', NEW.created_at, 'unixepoch', 'start of month') AS INTEGER),
                    -NEW.cost_micros, -NEW.credit_micros, -(NEW.key_source = 'hosted'), -(NEW.key_source = 'own'))
            ON CONFLICT (user_id, month) DO UPDATE SET cost_micros = cost_micros + excluded.cost_micros, credit_micros = credit_micros
                + excluded.credit_micros, hosted_calls = hosted_calls + excluded.hosted_calls, own_calls = own_calls + excluded.own_calls;
    END;
    CREATE TRIGGER ai_usage_review_remove AFTER DELETE ON ai_usage WHEN OLD.purpose = 'content-review' BEGIN
        UPDATE ai_usage_months SET cost_micros = cost_micros + OLD.cost_micros, credit_micros = credit_micros + OLD.credit_micros,
            hosted_calls = hosted_calls + (OLD.key_source = 'hosted'), own_calls = own_calls + (OLD.key_source = 'own')
        WHERE user_id = OLD.user_id AND month = CAST(strftime('%s', OLD.created_at, 'unixepoch', 'start of month') AS INTEGER);
    END;
    ",
    // v77 (#1038): when the content check last read a made course's files (src/content_check/nightly.rs); edited since: due tonight.
    "CREATE TABLE content_checks (course_id TEXT PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE, checked_at INTEGER NOT NULL);",
    // v78 (#1039): the rules version each content check ran under (TERMS_VERSION, AI_CHECK_VERSION; docs/content-check-versions.md).
    "ALTER TABLE course_flags ADD COLUMN terms_version INTEGER NOT NULL DEFAULT 1; ALTER TABLE course_flags ADD COLUMN ai_version INTEGER; ALTER TABLE content_scores ADD COLUMN ai_version INTEGER NOT NULL DEFAULT 1;",
    // v79 (#1041): when `skrongle content-check` last checked in (src/content_check/worker.rs); one row.
    "CREATE TABLE content_check_worker (id INTEGER PRIMARY KEY CHECK (id = 1), name TEXT NOT NULL, seen_at INTEGER NOT NULL);",
    // v80 (#1046): one AI check score per part and category (src/content_check.rs CATEGORIES) instead of two columns; the
    // two old scores move over as the categories 'terms' and 'self_harm', and `high` names the categories at or over 0.5.
    "
    CREATE TABLE content_category_scores (
        course_id TEXT NOT NULL,
        chunk     TEXT NOT NULL,
        category  TEXT NOT NULL,
        score     REAL NOT NULL,
        PRIMARY KEY (course_id, chunk, category),
        FOREIGN KEY (course_id, chunk) REFERENCES content_scores(course_id, chunk) ON DELETE CASCADE
    ) WITHOUT ROWID;
    INSERT INTO content_category_scores (course_id, chunk, category, score)
        SELECT course_id, chunk, 'terms', terms_score FROM content_scores UNION ALL SELECT course_id, chunk, 'self_harm', self_harm_score FROM content_scores;
    ALTER TABLE content_scores ADD COLUMN high TEXT NOT NULL DEFAULT '';
    UPDATE content_scores SET high = trim(CASE WHEN terms_score >= 0.5 THEN 'terms' ELSE '' END || CASE WHEN self_harm_score >= 0.5 THEN ',self_harm' ELSE '' END, ',');
    ALTER TABLE content_scores DROP COLUMN terms_score;
    ALTER TABLE content_scores DROP COLUMN self_harm_score;
    ",
    // v81 (#1035): when a superuser last e-mailed a course's owner why it
    // was flagged or taken down (by hand, from the course table), so nobody
    // sends it twice by accident. NULL: never.
    "ALTER TABLE courses ADD COLUMN owner_emailed_at INTEGER;",
    // v82 (#974): optional consents (src/consent.rs), one row per time a
    // learner gave one, closed by `withdrawn_at` when they take it back.
    // AI features are on unless hidden at sign-up, so everyone with them on
    // gave that consent when the account was made.
    "
    CREATE TABLE consents (
        id           INTEGER PRIMARY KEY,
        user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        purpose      TEXT    NOT NULL,
        granted_at   INTEGER NOT NULL,
        withdrawn_at INTEGER
    );
    CREATE INDEX idx_consents_user ON consents(user_id);
    INSERT INTO consents (user_id, purpose, granted_at) SELECT id, 'ai', created_at FROM users WHERE ai_off = 0;
    ",
    // v83 (#963): Stripe is the only way to pay. Plans "bought" with the
    // pretend card form (v54) were never paid for, so they go with it.
    // `plan_cancellations` stays: it keeps why Stripe subscribers left.
    "
    DROP TABLE plan_subscriptions;
    ",
    // v84 (#873): the mail and text outboxes are sent by the job runner
    // (src/jobs.rs). It leases a due row before sending it (`locked_by`,
    // `locked_until`, like `jobs`), so a row being sent is never handed out
    // twice; a lease that ran out (its server stopped mid-send) is taken
    // again. The row stays `queued` while leased.
    "
    ALTER TABLE mail_outbox ADD COLUMN locked_by TEXT;
    ALTER TABLE mail_outbox ADD COLUMN locked_until INTEGER;
    ALTER TABLE sms_outbox ADD COLUMN locked_by TEXT;
    ALTER TABLE sms_outbox ADD COLUMN locked_until INTEGER;
    ",
    // v85 (#981): the log of what a superuser did about a privacy request
    // (src/privacy_requests.rs): an account's data sent, an address
    // forgotten, an account deleted, with the reason typed. It names no
    // learner, only the superuser, whose id goes NULL when they are deleted.
    "
    CREATE TABLE privacy_actions (
        id           INTEGER PRIMARY KEY,
        superuser_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
        action       TEXT    NOT NULL,
        reason       TEXT    NOT NULL,
        created_at   INTEGER NOT NULL
    );
    CREATE INDEX idx_privacy_actions_superuser ON privacy_actions(superuser_id);
    ",
    // v86 (#934): the waitlist for a slow release (src/waitlist.rs). One row per address (lower case);
    // confirm_hash is the SHA-256 of the confirmation link's token until it is used. An e-mailed invitation
    // is a signup_invites row tied to the address (email); an account made through one is a beta account.
    // user_id: the account the address signed up as (deleting it deletes the row).
    // release_settings holds the waitlist switch (one row, set on Superuser → Release).
    "
    CREATE TABLE waitlist (
        id           INTEGER PRIMARY KEY,
        email        TEXT    NOT NULL UNIQUE,
        joined_at    INTEGER NOT NULL,
        confirm_hash TEXT,
        confirmed_at INTEGER,
        invite_id    INTEGER REFERENCES signup_invites(id) ON DELETE SET NULL,
        invited_at   INTEGER,
        invites_sent INTEGER NOT NULL DEFAULT 0,
        signed_up_at INTEGER,
        user_id      INTEGER REFERENCES users(id) ON DELETE CASCADE
    );
    CREATE INDEX idx_waitlist_user ON waitlist(user_id) WHERE user_id IS NOT NULL;
    CREATE UNIQUE INDEX idx_waitlist_confirm ON waitlist(confirm_hash) WHERE confirm_hash IS NOT NULL;
    CREATE INDEX idx_waitlist_invite ON waitlist(invite_id) WHERE invite_id IS NOT NULL;
    CREATE INDEX idx_waitlist_unconfirmed ON waitlist(joined_at) WHERE confirmed_at IS NULL;
    ALTER TABLE signup_invites ADD COLUMN email TEXT;
    ALTER TABLE users ADD COLUMN beta INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE release_settings (id INTEGER PRIMARY KEY CHECK (id = 1), waitlist INTEGER NOT NULL DEFAULT 0);
    ",
    // v87 (#931): when a new account got its one-time Skrongle AI credit
    // (`SKRONGLE_AI_ALLOWANCE_FREE`, `Meter::grant_signup_credit`). NULL: not
    // yet. Accounts that already exist are marked done: only new ones get it.
    "
    ALTER TABLE users ADD COLUMN signup_credit_at INTEGER;
    UPDATE users SET signup_credit_at = 0;
    ",
    // v88 (#970): when an account the learner asked to delete is deleted
    // (src/account_deletion.rs). NULL: not scheduled. Until then the
    // account can only cancel it; the hourly sweep deletes it once due.
    "
    ALTER TABLE users ADD COLUMN delete_at INTEGER;
    CREATE INDEX idx_users_delete_at ON users(delete_at) WHERE delete_at IS NOT NULL;
    ",
    // v89 (#505): a cross-learner streak count read one course's learners whose
    // streak ended in the last few days. The
    // earnings key is (user_id, course_id), so this makes that read an index
    // range over the course and its recent days instead of a scan.
    "
    CREATE INDEX idx_earnings_course_day ON earnings(course_id, day);
    ",
    // v90 (#501): a cross-learner "N learners worked on this lesson in the last hour"
    // count read one course's recent lesson_touches as an index range.
    "
    CREATE INDEX idx_lesson_touches_course_at ON lesson_touches(course_id, at);
    ",
    // v91 (#1068): who has had which breach notice (src/breach.rs). `notice` is
    // the SHA-256 of the notice's subject and body, so pressing Send again with
    // the same text mails nobody twice.
    "
    CREATE TABLE breach_notices (
        user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        notice    TEXT    NOT NULL,
        mailed_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, notice)
    );
    ",
    // v92 (#918): the effort "Generate the course" last ran at (`ai::Effort::id`:
    // low = Fast, medium = Balanced, high = Thorough); the next one starts there.
    "ALTER TABLE users ADD COLUMN ai_effort TEXT NOT NULL DEFAULT 'medium';",
    // v93 (#336): a learner's own note on a lesson ("My note" on the lesson page, src/notes.rs).
    // Plain text, at most notes::MAX_CHARS characters; an emptied note deletes its row.
    "
    CREATE TABLE lesson_notes (
        user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        course_id  TEXT    NOT NULL,
        lesson_id  TEXT    NOT NULL,
        body       TEXT    NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (user_id, course_id, lesson_id)
    );
    ",
    // v94 (#1238): the why and check lines of the latest AI run's lessons, as
    // JSON (`ai::LessonNote`, one per lesson that has any); empty for none.
    "ALTER TABLE course_ai_runs ADD COLUMN why TEXT NOT NULL DEFAULT '';",
    // v95 (#1360): a review schedule for collected words (src/vocab.rs).
    // `due_at` is the start of the day a word is due (NULL: no schedule yet),
    // `gap_days` its rung on the ladder. Words already collected get a rung
    // from how often they were known (0: 1 day, 1: 3, 2: 7, more: 16), due
    // that long after their last review; the ones that come out overdue are
    // spread over today and the six days after it.
    "
    ALTER TABLE user_vocab ADD COLUMN due_at INTEGER;
    ALTER TABLE user_vocab ADD COLUMN gap_days INTEGER NOT NULL DEFAULT 0;
    UPDATE user_vocab SET gap_days = CASE times_known WHEN 0 THEN 1 WHEN 1 THEN 3 WHEN 2 THEN 7 ELSE 16 END;
    UPDATE user_vocab SET due_at = (COALESCE(last_reviewed_at, learned_at) / 86400 + gap_days) * 86400;
    UPDATE user_vocab SET due_at = (CAST(strftime('%s', 'now') AS INTEGER) / 86400 + (learned_at + unicode(word)) % 7) * 86400
        WHERE due_at < CAST(strftime('%s', 'now') AS INTEGER) / 86400 * 86400;
    CREATE INDEX idx_user_vocab_due ON user_vocab(user_id, course_id, due_at);
    ",
    // v96 (#45): the twelve `housekeeping.*` job kinds became one
    // `housekeeping`; drop the old rows so they don't sit in /dev/jobs.
    "DELETE FROM jobs WHERE kind LIKE 'housekeeping.%';",
    // v97 (#40): the cross-learner motivator is gone (a crowd of 5 never forms on a
    // mini); anyone wearing it wears the default again (`motivators::DEFAULT`, ''), and
    // the two indexes only it read are dropped.
    "
    UPDATE users SET motivator = '' WHERE motivator = 'social';
    UPDATE projects SET motivator = '' WHERE motivator = 'social';
    DROP INDEX idx_lesson_touches_course_at;
    DROP INDEX idx_earnings_course_day;
    ",
];

/// Migrations (1-based versions) that rebuild a table other tables point at:
/// they run with foreign keys off and are checked before they commit.
pub(crate) const REBUILDS: &[i64] = &[26, 45];
