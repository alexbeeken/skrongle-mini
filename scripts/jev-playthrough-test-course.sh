#!/usr/bin/env bash
# The test course, played through: like scripts/jev-playthrough-all.sh (Sprint,
# Study and Play mode one after the other, each with its own fresh server and
# account, progress checked after every lesson, one summary table), but the
# server offers the test course (TEST_COURSE=1, data/test-courses/) and the
# runs play it instead of Spanish. Every feature a course file can switch on
# is in it, one lesson per feature, so this walks them all as a learner.
#
#   scripts/jev-playthrough-test-course.sh                 # the whole test course in every mode
#   scripts/jev-playthrough-test-course.sh --lessons 2     # options go to every run
#   MODES="sprint play" scripts/jev-playthrough-test-course.sh     # some modes only
#
# Results: e2e/results/playthrough/<timestamp>-test-course/{sprint,study,play}/ and summary.md there.
set -uo pipefail
cd "$(dirname "$0")/.."
case " $* " in *" -h "*|*" --help "*) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac
case " $* " in *" --course "*) echo "this script plays the test course; for another course use scripts/jev-playthrough-all.sh --course <id>"; exit 2 ;; esac

export TEST_COURSE=1
OUT="e2e/results/playthrough/$(date -u +%Y-%m-%dT%H-%M-%S)-test-course"
STATUS=0 DIRS=()
for m in ${MODES:-sprint study play}; do
  echo "==> test course playthrough: $m"
  mkdir -p "$OUT/$m"
  PLAYTHROUGH_OUT="$PWD/$OUT/$m" scripts/jev-playthrough.sh "$m" --course test-course "$@"
  s=$?
  # 2 is a usage error or a missing key: the others would fail the same way.
  [ "$s" = 2 ] && exit 2
  [ "$s" != 0 ] && STATUS=1
  DIRS+=("$OUT/$m")
done

node e2e/playthrough/summary.ts "${DIRS[@]}" | tee "$OUT/summary.md"
[ -n "${GITHUB_STEP_SUMMARY:-}" ] && cat "$OUT/summary.md" >>"$GITHUB_STEP_SUMMARY"
echo "==> reports: $OUT/{$(IFS=,; echo "${MODES:-sprint,study,play}" | tr ' ' ,)}/report.md"
exit "$STATUS"
