-- The Skrongle Cash motivator and its badge shop are gone: drop what the
-- shop sold, and let a course that wore Skrongle Cash wear XP instead.
DROP TABLE motivator_purchases;
UPDATE projects SET motivator = 'xp' WHERE motivator = 'skc';
