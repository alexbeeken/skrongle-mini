-- Back to v7: api_tokens.used_at goes. Used refresh tokens are deleted
-- first, because a v7 binary does not look at used_at and would take one
-- for a working refresh token. Every working token stays.
DELETE FROM api_tokens WHERE used_at IS NOT NULL;
ALTER TABLE api_tokens DROP COLUMN used_at;
