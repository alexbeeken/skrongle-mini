-- A reused refresh token revokes its grant (#184, RFC 9700 §4.14.2).
-- Refreshing no longer deletes the refresh token it was given: it marks it
-- used, so the token is still recognised if it comes back, and the grant
-- and every token under it are revoked then. A grant keeps only its newest
-- used refresh token; it goes with its grant, or when the grant expires.
-- NULL for every token that has not been used.
ALTER TABLE api_tokens ADD COLUMN used_at INTEGER;
