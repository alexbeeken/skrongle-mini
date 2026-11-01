#!/usr/bin/env bash
# Measure how long each kind of activity takes, to tune the daily goal's
# effort points (src/activity.rs, Kind::points). Read-only.
#
#   scripts/effort-calibrate.sh                 # ./dev.db
#   DB=/path/to/a-copy-of-prod.db scripts/effort-calibrate.sh
#
# For each kind it prints the median seconds one item takes and the points
# that would give, with a typed quiz answer as 1 point:
#   - quiz answers: the gap between one answer and the next in the same
#     course, when it is under two minutes (a sitting, not a break);
#   - game rounds, listening rounds and Sprint mode sessions: a round's
#     length over its items.
# Reading, flashcards and Guide cards leave no timings, so their points stay
# a judgement. Run it on a copy of a real database; a dev database has few rows.
set -euo pipefail
cd "$(dirname "$0")/.."
DB="${DB:-./dev.db}"
[ -f "$DB" ] || { echo "no database at $DB"; exit 1; }
command -v sqlite3 >/dev/null || { echo "sqlite3 not found"; exit 1; }

sqlite3 -readonly -header -column "$DB" "
WITH gaps AS (
    SELECT answered_at - LAG(answered_at) OVER (PARTITION BY user_id, course_id ORDER BY answered_at, id) AS secs
    FROM quiz_history
), answer AS (
    SELECT 'answer' AS kind, CAST(secs AS REAL) AS secs FROM gaps WHERE secs BETWEEN 1 AND 120
), rounds AS (
    SELECT CASE game WHEN 'sprint' THEN 'exercise (sprint)' WHEN 'listening' THEN 'listening' ELSE 'game: ' || game END AS kind,
           duration_ms / 1000.0 / total AS secs
    FROM game_activity WHERE total > 0 AND duration_ms > 0
), timed AS (
    SELECT * FROM answer UNION ALL SELECT * FROM rounds
), ranked AS (
    SELECT kind, secs, ROW_NUMBER() OVER (PARTITION BY kind ORDER BY secs) AS i, COUNT(*) OVER (PARTITION BY kind) AS n FROM timed
), medians AS (
    SELECT kind, n, AVG(secs) AS median FROM ranked WHERE i IN ((n + 1) / 2, (n + 2) / 2) GROUP BY kind, n
)
SELECT kind, n AS samples, ROUND(median, 1) AS median_secs,
       ROUND(median / (SELECT median FROM medians WHERE kind = 'answer'), 2) AS points_if_answer_is_1
FROM medians ORDER BY kind;
"
