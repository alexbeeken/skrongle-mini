-- Back to v3: no permissions and no presets. What each account could do
-- (users.perms), its preset and every preset the owner made are not kept;
-- going up again gives every account the Teacher set, as v4 did the first time.
DROP INDEX idx_users_preset;
ALTER TABLE users DROP COLUMN preset_id;
ALTER TABLE users DROP COLUMN perms;
DROP TABLE permission_presets;
