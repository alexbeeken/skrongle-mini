-- Back to v11: no daily work. Every plan, its activities and every student's
-- progress on them go; the class clock (settings 'class_tz') goes too.
DROP TABLE daily_done;
DROP TABLE daily_activities;
DROP TABLE daily_plans;
DELETE FROM sqlite_sequence WHERE name IN ('daily_plans', 'daily_activities');
DELETE FROM settings WHERE name = 'class_tz';
