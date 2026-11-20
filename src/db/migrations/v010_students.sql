-- Student accounts (mini #217, src/students.rs): an account a teacher makes,
-- which signs in with a username and a password and has no e-mail address
-- (users.email stays NULL). users.username is the sign-in name, stored lower
-- case and unique across the server; NULL for a normal account.
-- users.teacher_id is the account that holds the student: deleting the
-- teacher deletes its students, with everything in them. NULL for a normal
-- account.
ALTER TABLE users ADD COLUMN username TEXT;
ALTER TABLE users ADD COLUMN teacher_id INTEGER REFERENCES users(id) ON DELETE CASCADE;
CREATE UNIQUE INDEX idx_users_username ON users(username);
-- A teacher's students, and the cascade when a teacher is deleted, are
-- found through this, not a scan of users.
CREATE INDEX idx_users_teacher ON users(teacher_id);
-- The eighth permission, Hold students (bit 128, src/perms.rs): the built-in
-- Teacher preset gains it. Accounts given Teacher before keep the set they
-- have; an owner may do everything whatever is stored.
UPDATE permission_presets SET perms = perms | 128 WHERE id = 2 AND builtin = 1;
