-- Connected apps (#167): one row per connection an account gave an app or
-- an assistant, and the bearer tokens under it. Only a token's SHA-256 is
-- stored (`auth::hash_token`, as for sessions). Every grant has an expiry.
CREATE TABLE api_grants (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id    TEXT    NOT NULL,                 -- 'personal' for a token made on the profile
    client_name  TEXT    NOT NULL,                 -- the name the profile shows
    scopes       TEXT    NOT NULL,                 -- space-separated: courses:read courses:write
    courses      TEXT    NOT NULL DEFAULT '',      -- space-separated course ids; '' for all of the account's own
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    last_used_at INTEGER
);
CREATE INDEX idx_api_grants_user ON api_grants(user_id);
CREATE INDEX idx_api_grants_expires ON api_grants(expires_at);

CREATE TABLE api_tokens (
    id         INTEGER PRIMARY KEY,
    grant_id   INTEGER NOT NULL REFERENCES api_grants(id) ON DELETE CASCADE,
    kind       TEXT    NOT NULL CHECK (kind IN ('personal', 'access', 'refresh')),
    token_hash TEXT    NOT NULL UNIQUE,            -- the token's hash
    expires_at INTEGER NOT NULL
);
CREATE INDEX idx_api_tokens_grant ON api_tokens(grant_id);
CREATE INDEX idx_api_tokens_expires ON api_tokens(expires_at);
