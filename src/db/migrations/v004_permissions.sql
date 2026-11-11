-- Permissions (#138): each account has a set (users.perms, one bit a
-- permission, src/perms.rs) and the preset it was given (users.preset_id,
-- NULL = a custom set). Presets: Student and Teacher are built in.
CREATE TABLE permission_presets (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL COLLATE NOCASE UNIQUE,   -- built-in: 'student', 'teacher' (the screen translates them)
    builtin    INTEGER NOT NULL DEFAULT 0,
    perms      INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
INSERT INTO permission_presets (id, name, builtin, perms, created_at) VALUES (1, 'student', 1, 0, 0), (2, 'teacher', 1, 63, 0);
-- Every account that exists keeps what it can do today: the Teacher set.
ALTER TABLE users ADD COLUMN perms INTEGER NOT NULL DEFAULT 63;
ALTER TABLE users ADD COLUMN preset_id INTEGER REFERENCES permission_presets(id) ON DELETE SET NULL;
UPDATE users SET preset_id = 2;
-- Deleting a preset sets its accounts' preset_id NULL: found through this, not a scan of users.
CREATE INDEX idx_users_preset ON users(preset_id);
