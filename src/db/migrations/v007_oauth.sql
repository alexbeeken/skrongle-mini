-- OAuth sign-in for connected apps (#169): the apps that registered
-- themselves (RFC 7591) and the one-time authorization codes. A code is
-- stored by its SHA-256 only (`auth::hash_token`), as tokens are. Allow on
-- the consent page makes the grant (`api_grants`) and the code that points
-- at it; the token endpoint trades the code for the grant's first tokens.
CREATE TABLE oauth_clients (
    id            INTEGER PRIMARY KEY,
    client_id     TEXT    NOT NULL UNIQUE,         -- random, handed to the app
    name          TEXT    NOT NULL,                -- the name the app gave, at most 60 characters
    redirect_uris TEXT    NOT NULL,                -- newline-separated, at most 5
    created_at    INTEGER NOT NULL,
    last_grant_at INTEGER                          -- the last Allow; NULL until the first
);
CREATE INDEX idx_oauth_clients_created ON oauth_clients(created_at);

CREATE TABLE oauth_codes (
    id             INTEGER PRIMARY KEY,
    code_hash      TEXT    NOT NULL UNIQUE,        -- the code's hash
    client_id      TEXT    NOT NULL,
    user_id        INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    grant_id       INTEGER NOT NULL REFERENCES api_grants(id) ON DELETE CASCADE,
    redirect_uri   TEXT    NOT NULL,
    resource       TEXT    NOT NULL,
    code_challenge TEXT    NOT NULL,               -- PKCE S256
    expires_at     INTEGER NOT NULL,
    used_at        INTEGER                         -- set by the first exchange; a second one revokes the grant
);
CREATE INDEX idx_oauth_codes_user ON oauth_codes(user_id);
CREATE INDEX idx_oauth_codes_grant ON oauth_codes(grant_id);
CREATE INDEX idx_oauth_codes_expires ON oauth_codes(expires_at);
