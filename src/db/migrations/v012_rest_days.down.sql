-- Back to the version before rest days: the account's rest days are
-- forgotten. A streak kept going over one stays as the ledger has it
-- (`earnings`); from then on any day without activity ends it.
ALTER TABLE users DROP COLUMN rest_days_since;
ALTER TABLE users DROP COLUMN rest_days;
