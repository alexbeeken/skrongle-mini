#!/usr/bin/env bash
# Every playthrough, one after the other: Sprint mode, Study mode, Play mode, each with
# its own fresh server and account. Prints one summary table (and adds it to
# $GITHUB_STEP_SUMMARY in a GitHub Action). Exits 1 if any run failed.
#
#   scripts/jev-playthrough-all.sh                 # the whole course in every mode
#   scripts/jev-playthrough-all.sh --lessons 5     # options go to every run
#   MODES="sprint play" scripts/jev-playthrough-all.sh     # some modes only
#
# Results: e2e/results/playthrough/<timestamp>-all/{sprint,study,play}/ and summary.md there.
set -uo pipefail
cd "$(dirname "$0")/.."
case " $* " in *" -h "*|*" --help "*) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac

OUT="e2e/results/playthrough/$(date -u +%Y-%m-%dT%H-%M-%S)-all"
STATUS=0 DIRS=()
for m in ${MODES:-sprint study play}; do
  echo "==> playthrough: $m"
  mkdir -p "$OUT/$m"
  PLAYTHROUGH_OUT="$PWD/$OUT/$m" scripts/jev-playthrough.sh "$m" "$@"
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
