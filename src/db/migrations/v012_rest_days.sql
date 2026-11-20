-- Rest days (#237): the weekdays an account does not study, one bit a
-- weekday (bit 0 Sunday, `earnings::weekday`), at most two set (checked
-- when saved). A rest day without activity keeps the day streak going in
-- every course. `rest_days_since` is the learner's local day number the
-- mask was last changed on: no day before it counts as a rest day, so a
-- change never brings back a streak that had already ended.
ALTER TABLE users ADD COLUMN rest_days INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN rest_days_since INTEGER NOT NULL DEFAULT 0;
