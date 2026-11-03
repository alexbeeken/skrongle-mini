#!/usr/bin/env bash
# Fake or wipe learning progress in the LOCAL dev database, for testing.
#
#   scripts/dev-progress.sh status                      # what the dev user has done
#   scripts/dev-progress.sh complete all                # mark every lesson's Guide stage complete
#   scripts/dev-progress.sh complete a1-ser-vs-estar …  # mark specific lesson ids complete
#   scripts/dev-progress.sh reset                       # wipe ALL progress (keeps the account)
#
# Env: DB=./dev.db (database file), USER_EMAIL=<e-mail> (default: the only/first user),
#      COURSE=spanish (course id; lessons are read from data/courses/$COURSE/lessons).
# Completed stages unlock their items in the drill and show as done in the Guide.
set -euo pipefail
cd "$(dirname "$0")/.."
DB="${DB:-./dev.db}"
COURSE="${COURSE:-spanish}"
[ -f "$DB" ] || { echo "no database at $DB (start the app once with ./.dev-start)"; exit 1; }
command -v sqlite3 >/dev/null || { echo "sqlite3 not found"; exit 1; }

q() { sqlite3 "$DB" "$@"; }
if [ -n "${USER_EMAIL:-}" ]; then
  UID_=$(q "select id from users where lower(email) = lower('$USER_EMAIL');")
else
  UID_=$(q "select id from users order by id limit 1;")
fi
[ -n "$UID_" ] || { echo "no such user"; exit 1; }
UNAME=$(q "select coalesce(email, 'user ' || id) from users where id = $UID_;")

lesson_ids() { python3 -c 'import json,glob,sys; [print(json.load(open(f))["id"]) for f in sorted(glob.glob("data/courses/" + sys.argv[1] + "/lessons/**/*.json", recursive=True))]' "$COURSE"; }

case "${1:-status}" in
  status)
    echo "user: $UNAME (id $UID_) · db: $DB · course: $COURSE"
    echo "guide stages:"
    q -column -header "select lesson_id, step, completed, best_stars from journey_progress where user_id = $UID_ and course_id = '$COURSE';"
    echo "mastery:"
    q -column -header "select lesson_id, round(mastery_score) as mastery, times_tested, times_correct from user_progress where user_id = $UID_ and course_id = '$COURSE';"
    echo "answers: $(q "select count(*) from quiz_history where user_id = $UID_ and course_id = '$COURSE';") · terms: $(q "select count(*) from user_vocab where user_id = $UID_ and course_id = '$COURSE';") · game scores: $(q "select count(*) from game_scores where user_id = $UID_ and course_id = '$COURSE';")"
    ;;
  complete)
    shift
    [ $# -gt 0 ] || { echo "usage: complete all | <lesson-id>…"; exit 1; }
    if [ "$1" = "all" ]; then set -- $(lesson_ids); fi
    for id in "$@"; do
      lesson_ids | grep -qx "$id" || { echo "unknown lesson id: $id"; exit 1; }
      q "insert into journey_progress (user_id, course_id, lesson_id, step, completed, best_stars, run_correct, run_answered, updated_at)
         values ($UID_, '$COURSE', '$id', 0, 1, 3, 0, 0, strftime('%s','now'))
         on conflict(user_id, course_id, lesson_id) do update set completed = max(completed, 1), best_stars = max(best_stars, 3), step = 0, run_correct = 0, run_answered = 0, updated_at = excluded.updated_at;"
      echo "completed: $id"
    done
    ;;
  reset)
    for t in journey_progress user_progress quiz_history user_vocab game_scores game_activity; do
      n=$(q "select count(*) from $t where user_id = $UID_ and course_id = '$COURSE';")
      q "delete from $t where user_id = $UID_ and course_id = '$COURSE';"
      echo "cleared $t ($n rows)"
    done
    echo "progress in $COURSE reset for $UNAME; the account and login are untouched"
    ;;
  *) echo "unknown command: $1 (status | complete | reset)"; exit 1 ;;
esac
