-- Back to v9: the learner path's two tables go, and with them each
-- activity's review streak and the list of hidden activities, which v9 has
-- no place for. Passes, Study progress and every answer stay where v9 reads
-- them (lesson_passes, journey_progress, quiz_history).
DROP TABLE path_hidden;
DROP TABLE path_activity;
