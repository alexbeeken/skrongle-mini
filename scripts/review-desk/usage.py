#!/usr/bin/env python3
"""AI usage per rotation (#1366): what each /rotate workspace spent, and on what.

Reads the Claude Code transcripts of the four rotation workspaces
(~/.claude/projects/*-{mergeme,backlog,pr-qa,qa}-<date>[-N]/, the session and
every subagent it started), adds up the tokens per model as the API reported
them, counts the work the workspace did on GitHub, and rewrites
~/skrongle-workflow/log/usage.jsonl with one line per workspace:

  {"workspace": "backlog-2026-10-30", "skill": "backlog",
   "rotation": "2026-10-30 17:59",          # the rotate.log line it belongs to,
                                            # or "" when it wasn't started by /rotate
   "start": "2026-10-30T18:00:12", "end": "...",   # local time
   "tokens": 55093200,                     # every token the API handled
   "cached": 52000000, "uncached": 3093200, # cache reads / input + cache writes + output
   "session": {"tokens": 9800000, "cached": 9100000, "uncached": 700000},  # the main session only
   "models": {"claude-opus-5-5": {"calls": 812, "input": 1200, "output": 90000,
              "cache_write": 3000000, "cache_read": 52000000, "tokens": 55091200}, ...},
   "agents": [{"what": "Build issue #1237 to a PR", "type": "general-purpose",
               "model": "claude-opus-5-5", "tokens": 6100000, "cached": 5800000,
               "uncached": 300000}, ...],   # biggest first
   "work": {"prs_opened": [1301], "issues_filed": [], "prs_commented": [],
            "prs_landed": []}}

"tokens" is input + cache write + cache read + output: every token the API
handled. "cached" is the cache reads: the conversation so far, re-read on each
step at a tenth of the input price. "uncached" is the rest: new input, cache
writes (new context saved for the next step) and output. Jev (TypeSafe) calls and the local Qwen model
are not Claude tokens and are not counted here.

  usage.py            rewrite the log, print one line per rotation
  usage.py --json     print the records instead

Standard library only; the desk imports collect() and calls it on every load
of its Usage view (a full scan of a few GB of transcripts takes about a second).
"""

import json
import os
import re
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

SKILLS = ("mergeme", "backlog", "pr-qa", "qa")
WORKSPACE = re.compile(r"-((mergeme|backlog|pr-qa|qa)-(\d{4}-\d{2}-\d{2})(?:-\d+)?)$")

# Where a rotate.log line and its sessions' first messages may sit apart: the
# line is written after the launches, and Orca can take a while to start one.
ROTATION_BEFORE = timedelta(minutes=10)
ROTATION_AFTER = timedelta(minutes=30)

CREATED = re.compile(r"github\.com/[^/\s]+/[^/\s]+/(issues|pull)/(\d+)")
LANDED = re.compile(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2})\s+PR #\d+: (?:landed|in CI)((?: #\d+)+)")


def projects_dir():
    return Path(os.environ.get("CLAUDE_PROJECTS_DIR") or "~/.claude/projects").expanduser()


def workflow_home():
    return Path(os.environ.get("REVIEW_DESK_HOME") or "~/skrongle-workflow").expanduser()


def parse_time(stamp):
    """A transcript timestamp (UTC, ISO) as a naive local datetime."""
    try:
        t = datetime.fromisoformat(stamp.replace("Z", "+00:00"))
    except (AttributeError, ValueError):
        return None
    if t.tzinfo is None:
        t = t.replace(tzinfo=timezone.utc)
    return t.astimezone().replace(tzinfo=None)


class Tally:
    def __init__(self):
        self.models = {}
        self.tokens = 0
        self.cached = 0
        self.seen = set()

    def add(self, line):
        m = line.get("message") or {}
        u = m.get("usage")
        model = m.get("model") or ""
        if not u or not model or model.startswith("<"):
            return
        # One API call is written once per content block, each with the same usage.
        key = (m.get("id"), line.get("requestId"))
        if key in self.seen:
            return
        self.seen.add(key)
        row = self.models.setdefault(model, {"calls": 0, "input": 0, "output": 0, "cache_write": 0,
                                             "cache_read": 0, "tokens": 0})
        row["calls"] += 1
        row["input"] += u.get("input_tokens") or 0
        row["output"] += u.get("output_tokens") or 0
        row["cache_write"] += u.get("cache_creation_input_tokens") or 0
        row["cache_read"] += u.get("cache_read_input_tokens") or 0
        n = sum(u.get(k) or 0 for k in ("input_tokens", "output_tokens", "cache_creation_input_tokens",
                                        "cache_read_input_tokens"))
        row["tokens"] += n
        self.tokens += n
        self.cached += u.get("cache_read_input_tokens") or 0

    def split(self):
        return {"tokens": self.tokens, "cached": self.cached, "uncached": self.tokens - self.cached}

    def main_model(self):
        return max(self.models, key=lambda k: self.models[k]["tokens"], default="")


def read_transcript(path, tally, work, times):
    """Add one .jsonl file's calls to tally, its GitHub work to work, its times to times."""
    asks = {}  # tool_use id -> what kind of work its command does
    try:
        f = open(path, "rb")
    except OSError:
        return
    with f:
        for raw in f:
            has_usage = b'"usage"' in raw
            has_tool = b'"tool_use"' in raw or b'"tool_result"' in raw
            if not (has_usage or has_tool or b'"timestamp"' in raw[:400]):
                continue
            try:
                line = json.loads(raw)
            except ValueError:
                continue
            if not isinstance(line, dict):
                continue
            t = parse_time(line.get("timestamp"))
            if t:
                times.append(t)
            if has_usage:
                tally.add(line)
            if has_tool:
                note_work(line, asks, work)


# The GitHub writes that count as work. A write through `gh api` needs a field
# (-f/-F/--field/--input) or POST; the same path without one is a read.
API_WRITE = r"(?=[^;&|\n]*(?:\s-[fF]\s|--field|--raw-field|--input|POST))"
OPEN_PR = re.compile(r"\bgh\s+pr\s+create\b|\bgh\s+api\b" + API_WRITE + r"[^;&|\n]*repos/[^/\s]+/[^/\s]+/pulls(?![/\w])")
FILE_ISSUE = re.compile(r"\bgh\s+issue\s+create\b|\bgh\s+api\b" + API_WRITE
                        + r"[^;&|\n]*repos/[^/\s]+/[^/\s]+/issues(?![/\w])")
COMMENT = re.compile(r"\bgh\s+pr\s+comment\s+(\d+)|\bgh\s+api\b" + API_WRITE
                     + r"[^;&|\n]*repos/[^/\s]+/[^/\s]+/issues/(\d+)/comments\b")


def note_work(line, asks, work):
    """Pair each work command with its result: a create counts once GitHub answers with
    the new URL; a comment counts when its command didn't fail."""
    content = (line.get("message") or {}).get("content")
    if not isinstance(content, list):
        return
    for block in content:
        if not isinstance(block, dict):
            continue
        if block.get("type") == "tool_use":
            cmd = (block.get("input") or {}).get("command")
            if not isinstance(cmd, str):
                continue
            # gh pr/issue create print only the new URLs (a loop prints several); a
            # `gh api` create prints the new item's JSON, whose own html_url comes first.
            kinds = []
            if OPEN_PR.search(cmd):
                kinds.append(("prs_opened", bool(re.search(r"\bgh\s+pr\s+create\b", cmd))))
            if FILE_ISSUE.search(cmd):
                kinds.append(("issues_filed", bool(re.search(r"\bgh\s+issue\s+create\b", cmd))))
            prs = {int(a or b) for a, b in COMMENT.findall(cmd)}
            if kinds or prs:
                asks[block.get("id")] = (kinds, prs)
        elif block.get("type") == "tool_result" and block.get("tool_use_id") in asks:
            kinds, prs = asks.pop(block.get("tool_use_id"))
            if block.get("is_error"):
                continue
            work["prs_commented"] |= prs
            text = block.get("content")
            if isinstance(text, list):
                text = " ".join(x.get("text", "") for x in text if isinstance(x, dict))
            found = CREATED.findall(str(text or ""))
            for kind, every in kinds:
                want = "pull" if kind == "prs_opened" else "issues"
                nums = [int(n) for what, n in found if what == want]
                work[kind] |= set(nums if every else nums[:1])


def rotations():
    """(local datetime, "YYYY-MM-DD HH:MM", kinds started) for each rotate.log start line."""
    out = []
    try:
        lines = (workflow_home() / "log" / "rotate.log").read_text(encoding="utf-8").splitlines()
    except OSError:
        return out
    for l in lines:
        m = re.match(r"(\d{4}-\d{2}-\d{2} \d{2}:\d{2})\s+started (.*?)(?: ·|$)", l)
        if not m or m.group(2).strip() == "nothing":
            continue
        kinds = {re.sub(r"-\d{4}-\d{2}-\d{2}.*$", "", n) for n in m.group(2).split()}
        out.append((datetime.strptime(m.group(1), "%Y-%m-%d %H:%M"), m.group(1), kinds))
    return out


def rotation_for(skill, start, starts):
    best = ""
    for when, label, kinds in starts:
        if skill in kinds and when - ROTATION_BEFORE <= start <= when + ROTATION_AFTER:
            best = label
    return best


def landed_prs(start, end):
    """PR numbers a mergeme run put in its combined PR, from mergeme.log lines in its window."""
    found = set()
    try:
        lines = (workflow_home() / "log" / "mergeme.log").read_text(encoding="utf-8").splitlines()
    except OSError:
        return found
    for l in lines:
        m = LANDED.match(l)
        if not m:
            continue
        when = datetime.strptime(m.group(1), "%Y-%m-%d %H:%M")
        if start - timedelta(minutes=1) <= when <= end + timedelta(minutes=5):
            found |= {int(n) for n in re.findall(r"\d+", m.group(2))}
    return found


def agent_info(path):
    try:
        meta = json.loads(path.with_suffix(".meta.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        meta = {}
    return str(meta.get("description") or path.stem), str(meta.get("agentType") or "")


def workspace_record(folder, name, skill, starts):
    total, session = Tally(), Tally()
    work = {"prs_opened": set(), "issues_filed": set(), "prs_commented": set(), "prs_landed": set()}
    times, agents = [], []
    for main in sorted(folder.glob("*.jsonl")):
        read_transcript(main, session, work, times)
    for sub in sorted(folder.glob("*/subagents/*.jsonl")):
        t = Tally()
        read_transcript(sub, t, work, times)
        if not t.models:
            continue
        what, kind = agent_info(sub)
        agents.append({"what": what, "type": kind, "model": t.main_model(), **t.split()})
        merge(total, t)
    merge(total, session)
    if not total.models or not times:
        return None
    start, end = min(times), max(times)
    if skill == "mergeme":
        work["prs_landed"] = landed_prs(start, end)
    agents.sort(key=lambda a: -a["tokens"])
    return {
        "workspace": name,
        "skill": skill,
        "rotation": rotation_for(skill, start, starts),
        "start": start.isoformat(timespec="seconds"),
        "end": end.isoformat(timespec="seconds"),
        **total.split(),
        "session": session.split(),
        "models": dict(sorted(total.models.items())),
        "agents": agents,
        "work": {k: sorted(v) for k, v in work.items()},
    }


def merge(into, t):
    into.tokens += t.tokens
    into.cached += t.cached
    for model, row in t.models.items():
        dst = into.models.setdefault(model, {k: 0 for k in row})
        for k, v in row.items():
            dst[k] += v


def collect(write=True):
    """Every rotation workspace's record, oldest first; rewrites usage.jsonl when write."""
    starts = rotations()
    records = []
    root = projects_dir()
    try:
        folders = sorted(p for p in root.iterdir() if p.is_dir())
    except OSError:
        folders = []
    for folder in folders:
        m = WORKSPACE.search(folder.name)
        if not m or "scratchpad" in folder.name:
            continue
        rec = workspace_record(folder, m.group(1), m.group(2), starts)
        if rec:
            records.append(rec)
    records.sort(key=lambda r: r["start"])
    if write:
        path = workflow_home() / "log" / "usage.jsonl"
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".jsonl.tmp")
        tmp.write_text("".join(json.dumps(r) + "\n" for r in records), encoding="utf-8")
        tmp.replace(path)
    return records


def main(argv):
    records = collect(write=True)
    if "--json" in argv:
        for r in records:
            print(json.dumps(r))
        return
    by = {}
    for r in records:
        by.setdefault(r["rotation"] or "outside a rotation", []).append(r)
    for label, rs in by.items():
        parts = " · ".join("%s %.1fM" % (r["skill"], r["tokens"] / 1e6) for r in rs)
        print("%-20s %8.1fM tokens  %s" % (label, sum(r["tokens"] for r in rs) / 1e6, parts))
    print("wrote %s" % (workflow_home() / "log" / "usage.jsonl"))


if __name__ == "__main__":
    main(sys.argv[1:])
