#!/usr/bin/env bash
# Start one rotation: four Orca workspaces, each a Claude session running one
# skill (/mergeme, /backlog-sweep, /pr-qa-demo, /qa), all at once. Checks
# first, then launches, then appends one line to
# ~/skrongle-workflow/log/rotate.log. It doesn't wait for the sessions.
#
#   start.sh                 # checks + launch all four
#   ONLY="qa mergeme" start.sh   # some of them: mergeme backlog pr-qa qa
#   DRY=1 start.sh           # checks + print the orca commands, launch nothing
#   BASE_BRANCH=<branch> start.sh   # start the sessions from a branch (to try skill changes before they merge)
#   FORCE=1 start.sh         # start every part even when its work check finds nothing to do
#   MODEL_PR_QA=opus start.sh   # a session on another model (MODEL_MERGEME, MODEL_BACKLOG, MODEL_PR_QA, MODEL_QA)
#   QWEN=off start.sh        # don't start the local Qwen server (it holds ~20 GB of RAM)
#
# Each session's prompt is just "/<skill>": the four skills are written for
# the rotation, so nothing is passed or skipped. The builder and QA agent
# limits come from budget.sh (every skill reads it); under 50 GB free this
# script doesn't start a full rotation.
#
# Before it starts a part, it checks that the part has work (#1314), and
# skips it with a note in the log line when it doesn't:
#   mergeme  no open PR labelled mergeme or with a "mergeme" comment
#   backlog  fewer than 2 issues that issues.py calls "candidate"
#   pr-qa    no open non-draft PR that find-prs.sh says needs QA or a demo
#   qa       the last /qa run (qa.log) ended under QA_EVERY hours ago (20)
# Each session starts on its own model (model_for below), so a /model
# switch in another session doesn't change what the factory runs on. The
# rotator (the Fable session running /rotate) picks them; all default to
# sonnet, and the sessions start opus agents for code.
set -euo pipefail
ONLY="${ONLY:-mergeme backlog pr-qa qa}"
BASE_BRANCH="${BASE_BRANCH:-main}"
DAY=$(date +%F)
LOGDIR="$HOME/skrongle-workflow/log"
note=()
die() { echo "rotate: $*" >&2; mkdir -p "$LOGDIR"
  [ -n "${DRY:-}" ] || printf '%s  stopped: %s\n' "$(date '+%Y-%m-%d %H:%M')" "$*" >> "$LOGDIR/rotate.log"; exit 1; }
want() { case " $ONLY " in *" $1 "*) return 0 ;; esac; return 1; }
drop() { ONLY=$(echo " $ONLY " | sed "s/ $1 / /"); note+=("$1 skipped ($2)"); }
SKILLS="$(cd "$(dirname "$0")/../.." && pwd)"

[ "${CLAUDE_CODE_REMOTE:-}" = true ] && die "cloud session; rotate needs the Mac (Orca, runners, ~/skrongle-demos)"
command -v orca >/dev/null || die "orca CLI not found"

# Orca knows the repo by its main checkout, not by a worktree.
common=$(git rev-parse --path-format=absolute --git-common-dir)
REPO_PATH=$(dirname "$common")

free=$(df -g / | awk 'NR==2 {print $4}')
[ "$free" -ge 50 ] || die "only ${free} GB free (need 50)"
b=$("$(dirname "$0")/budget.sh" builders)
q=$("$(dirname "$0")/budget.sh" qa)

# A combined mergeme PR still open means the last mergeme run hasn't landed;
# a second one would race it for main.
if want mergeme; then
  open=$(gh api 'repos/{owner}/{repo}/pulls?state=open&per_page=100' --paginate \
    --jq '.[] | select(.head.ref | startswith("alexbeeken/mergeme-")) | .number' | tr '\n' ' ')
  if [ -n "${open// }" ]; then
    drop mergeme "PR #${open% } still open"
  fi
fi

# Work checks: a part with nothing to do isn't started. FORCE=1 skips them.
if [ -z "${FORCE:-}" ]; then
  if want mergeme; then
    n=$(gh api -X GET search/issues -f q='repo:{owner}/{repo} is:pr is:open label:mergeme' --jq .total_count 2>/dev/null || echo 1)
    if [ "$n" = 0 ]; then
      n=$(gh api -X GET search/issues -f q='repo:{owner}/{repo} is:pr is:open commenter:alexbeeken mergeme in:comments' --jq .total_count 2>/dev/null || echo 1)
    fi
    [ "$n" = 0 ] && drop mergeme "no mergeme PRs"
  fi
  if want backlog; then
    n=$(python3 "$SKILLS/backlog-sweep/scripts/issues.py" candidates 2>/dev/null | awk -F'\t' '$2=="candidate"' | wc -l | tr -d ' ')
    [ "${n:-0}" -lt 2 ] && drop backlog "${n:-0} buildable issues"
  fi
  if want pr-qa; then
    echo "rotate: checking which PRs need QA (about 3 minutes)" >&2
    n=$("$SKILLS/pr-qa-demo/scripts/find-prs.sh" 2>/dev/null \
      | awk -F'\t' '$9!="true" && ($8=="both" || $8=="qa" || $8=="demo" || $8=="stale")' | wc -l | tr -d ' ')
    [ "${n:-0}" -eq 0 ] && drop pr-qa "no PR needs QA"
  fi
  if want qa; then
    last=$(tail -1 "$LOGDIR/qa.log" 2>/dev/null | cut -c1-16)
    if [ -n "$last" ] && then_s=$(date -j -f '%Y-%m-%d %H:%M' "$last" +%s 2>/dev/null); then
      age=$(( ($(date +%s) - then_s) / 3600 ))
      [ "$age" -lt "${QA_EVERY:-20}" ] && drop qa "last run ${age} h ago"
    fi
  fi
fi

if want pr-qa || want qa; then
  key=$(zsh -ic 'echo ${#TYPESAFE_API_KEY}' 2>/dev/null | tail -1)
  if [ "${key:-0}" -eq 0 ] && ! grep -qs '^TYPESAFE_API_KEY=.' "$REPO_PATH/.env" "$REPO_PATH/e2e/.env"; then
    die "no TYPESAFE_API_KEY (Jev); pr-qa-demo and qa need it"
  fi
fi

if [ -z "${DRY:-}" ]; then
  if ! orca status --json 2>/dev/null | grep -q '"reachable": *true'; then
    orca open --json >/dev/null 2>&1 || true
    orca status --json 2>/dev/null | grep -q '"reachable": *true' || die "Orca isn't running and orca open failed"
  fi
fi

# Qwen does the factory's mechanical text work (factory-rules.md, "Qwen
# first"). Start it now so the first call doesn't wait for the model to
# load. The rotation runs without it when it's off or won't start.
QWEN_CTL="$HOME/ai/qwen-server.sh"
qwen=off
if [ "${QWEN:-on}" != off ] && [ -x "$QWEN_CTL" ]; then
  if [ -n "${DRY:-}" ]; then
    qwen=$("$QWEN_CTL" status >/dev/null 2>&1 && echo on || echo "would start")
  elif "$QWEN_CTL" status >/dev/null 2>&1 || "$QWEN_CTL" start >/dev/null 2>&1; then
    qwen=on
  else
    note+=("qwen didn't start (~/ai/qwen-server.log)")
  fi
fi

# The model each session runs on. The rotator is the only Fable; it sets
# MODEL_* when a run needs more. Code is written by the sessions' opus
# agents (../references/factory-rules.md).
model_for() {
  case "$1" in
    mergeme) echo "${MODEL_MERGEME:-sonnet}" ;;
    backlog) echo "${MODEL_BACKLOG:-sonnet}" ;;
    pr-qa)   echo "${MODEL_PR_QA:-sonnet}" ;;
    qa)      echo "${MODEL_QA:-sonnet}" ;;
  esac
}
prompt_for() {
  case "$1" in
    mergeme) echo "/mergeme" ;;
    backlog) echo "/backlog-sweep" ;;
    pr-qa)   echo "/pr-qa-demo" ;;
    qa)      echo "/qa" ;;
  esac
}
started=()
for kind in mergeme backlog pr-qa qa; do
  want "$kind" || continue
  name="$kind-$DAY"
  cmd=(orca worktree create --name "$name" --repo "path:$REPO_PATH" --base-branch "$BASE_BRANCH"
       --agent claude --model "$(model_for "$kind")" --prompt "$(prompt_for "$kind")" --no-parent --json)
  if [ -n "${DRY:-}" ]; then printf '%q ' "${cmd[@]}"; echo; started+=("$name"); continue; fi
  if out=$("${cmd[@]}" 2>&1); then
    started+=("$name"); echo "started $name on $(model_for "$kind")"
  else
    # "could not confirm the launch outcome" has meant the agent started anyway.
    echo "$out" >&2; note+=("$kind: orca said no, check the workspace list")
  fi
done

t=$("$(dirname "$0")/budget.sh" total)
line="started ${started[*]:-nothing} · qwen $qwen · builders $b · QA agents $q · $t agents per run · ${free} GB free"
for n in "${note[@]+"${note[@]}"}"; do line="$line · $n"; done
echo "$line"
if [ -z "${DRY:-}" ]; then
  mkdir -p "$LOGDIR"; printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M')" "$line" >> "$LOGDIR/rotate.log"
fi
