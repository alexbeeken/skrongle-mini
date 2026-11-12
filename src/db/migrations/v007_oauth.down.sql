-- Back to v6: no OAuth. Registered apps and pending codes go; grants an app
-- got through OAuth stay until they expire or are revoked, but the app can
-- no longer refresh its tokens without OAuth and has to connect again.
DROP TABLE oauth_codes;
DROP TABLE oauth_clients;
