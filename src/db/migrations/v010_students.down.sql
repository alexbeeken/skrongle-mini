-- Back to v9: no student accounts and no Hold students permission. A v9
-- binary has no place for a student (no username, no address to sign in
-- with), so every student account goes, with its progress and sessions
-- (every user table cascades from users). The Hold students bit is cleared
-- from every account and preset; going up again gives it back to the
-- built-in Teacher preset only, as v10 did the first time.
DELETE FROM users WHERE teacher_id IS NOT NULL;
DROP INDEX idx_users_teacher;
DROP INDEX idx_users_username;
ALTER TABLE users DROP COLUMN teacher_id;
ALTER TABLE users DROP COLUMN username;
UPDATE permission_presets SET perms = perms & 127;
UPDATE users SET perms = perms & 127;
