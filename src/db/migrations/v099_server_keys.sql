-- v99 (#25): the AI API keys and the hide-AI switch are server-wide and
-- the owner's (src/secrets.rs, src/settings.rs, the Manage Users page).
-- `server_api_keys` starts with the keys of the first owner who saved any,
-- sealed as they were: `sealed_for` names the user id their associated
-- data was bound to (NULL: a key saved for the server). The per-user switch
-- (`users.ai_off`) and its consent rows go out of use: the switch is a
-- `settings` row now, AI shown until the owner hides it. `user_api_keys`
-- and `users.ai_off` stay, unread, until the schema squash drops them.
CREATE TABLE server_api_keys (
    provider   TEXT    PRIMARY KEY,
    ciphertext BLOB    NOT NULL,
    set_at     INTEGER NOT NULL,
    sealed_for INTEGER
) WITHOUT ROWID;
INSERT INTO server_api_keys (provider, ciphertext, set_at, sealed_for)
    SELECT provider, ciphertext, updated_at, user_id FROM user_api_keys
    WHERE user_id = (SELECT MIN(k.user_id) FROM user_api_keys k JOIN users u ON u.id = k.user_id WHERE u.owner = 1);
CREATE TABLE settings (
    name  TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
DELETE FROM consents WHERE purpose = 'ai';
