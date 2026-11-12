-- Any earlier refresh token of a grant revokes it when it comes back, not
-- only the last one used (#192, RFC 9700 §4.14.2). Every refresh token of a
-- grant ends with the grant's family, 128 random bits; the grant keeps only
-- the family's SHA-256 (`auth::hash_token`), so a copy of the database still
-- holds no working token and cannot make one. A refresh deletes the token it
-- was given: used rows (v8) are no longer kept, so they and `used_at` go.
-- A used row must go before the column does: without `used_at` this binary
-- would take it for a working refresh token. NULL for a personal grant, and
-- for an OAuth grant whose refresh token predates families until it
-- refreshes once.
DELETE FROM api_tokens WHERE used_at IS NOT NULL;
ALTER TABLE api_tokens DROP COLUMN used_at;
ALTER TABLE api_grants ADD COLUMN refresh_family_hash TEXT;
CREATE UNIQUE INDEX idx_api_grants_refresh_family ON api_grants(refresh_family_hash);
