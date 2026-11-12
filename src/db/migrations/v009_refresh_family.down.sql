-- Back to v8: api_grants.refresh_family_hash goes and api_tokens.used_at
-- comes back, NULL everywhere. v9 keeps no used refresh token (a refresh
-- deletes the one it was given), so the only refresh token a v8 binary
-- finds for a grant is its newest, which works there as it should; the
-- earlier ones are not stored and stay refused. A v8 binary reads a refresh
-- token by its hash only, so the family at the end of a v9 token is never
-- looked at.
DROP INDEX idx_api_grants_refresh_family;
ALTER TABLE api_grants DROP COLUMN refresh_family_hash;
ALTER TABLE api_tokens ADD COLUMN used_at INTEGER;
