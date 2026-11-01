#!/usr/bin/env python3
"""Skrongle Review Desk: a local page that walks through the PRs waiting for
review, one at a time (#1121, #1126; a design PR opens in design mode, #1239), in its Issues mode through the issues
waiting for grooming (#1139), and in its Decisions mode through the questions
that block work, which it turns into one prompt for an agent (#1163).

Python standard library only. Start it with `scripts/review-desk/review`.

Data and settings live in ~/skrongle-workflow (REVIEW_DESK_HOME overrides it),
never in the repo:
  config.json        created with defaults on the first run (the repo comes from
                     the workspace the desk was started from)
  log/actions.log    every action, and every command it ran
  log/mergeme.log    written by the mergeme skill; the header shows its last line
  log/usage.jsonl    Claude tokens per rotation workspace, rewritten by usage.py on every
                     load of the Usage view (#1366)
  decisions/*.json   one question each, written by the factory skills; the desk
                     saves the answer in the same file (format:
                     .claude/skills/backlog-sweep/references/decisions.md)

REVIEW_DESK_DRY_RUN=1 logs and prints every command that changes GitHub or
Orca instead of running it. Commands that only read still run.

Endpoints (all /api/* need the key, as ?k= or the X-Review-Key header):
  GET  /                         the page (static/index.html)
  GET  /api/queue[?refresh=1]    cards in desk order, header counts, mergeme.log line
  GET  /api/demo/<pr>/<file>     a demo picture or video (Range requests work)
  GET  /api/orca                 Orca reachable?, fix-pr-* and mergeme-* workspaces,
                                 the fix-agent limit, PRs labelled mergeme
  GET  /api/issues[?refresh=1]   open issues tagged [qa] or [needs grooming] (title
                                 or label), oldest first
  GET  /api/decisions            open questions (not sent, not dismissed), oldest first
  GET  /api/usage                Claude tokens per rotation workspace (usage.py), oldest first
  POST /api/action               {"action": <name in ACTIONS>, "pr": <int>, ...}
                                 ("run-mergeme" takes no "pr"; "issue-edit" takes
                                 "issue" and a new "title" and/or "body";
                                 "pr-close" takes "pr" and "confirmed": true;
                                 "issue-close" and "issue-reopen" take "issue"; the
                                 decision-* actions take "id" or "ids")
"""

import argparse
import json
import os
import re
import secrets
import shlex
import subprocess
import sys
import threading
import time
import webbrowser
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, unquote, urlsplit

sys.dont_write_bytecode = True  # no __pycache__ in the repo
sys.path.insert(0, str(Path(__file__).resolve().parent))
import rules  # noqa: E402
import usage  # noqa: E402

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent
DRY_RUN = os.environ.get("REVIEW_DESK_DRY_RUN", "") not in ("", "0")

ISSUE_FIELDS = "number,title,url,state,labels,createdAt,author,body"
PR_FIELDS = ",".join([
    "number", "title", "url", "isDraft", "state", "labels", "createdAt", "headRefOid",
    "baseRefName", "mergeable", "statusCheckRollup", "comments", "body",
])

# The only commands the desk runs. Each is an argv list (never a shell) with
# {placeholders} filled from PARAMS-checked values. "writes" commands change
# GitHub or Orca: a dry run logs and prints them instead.
COMMANDS = {
    "git-origin": {"argv": ["git", "-C", "{path}", "remote", "get-url", "origin"], "writes": False},
    "pr-list": {"argv": ["gh", "pr", "list", "--repo", "{repo}", "--state", "open",
                         "--limit", "300", "--json", PR_FIELDS], "writes": False},
    "pr-state": {"argv": ["gh", "pr", "view", "{pr}", "--repo", "{repo}", "--json", "state,labels"],
                 "writes": False},
    "pr-commits": {"argv": ["gh", "api", "--paginate", "repos/{repo}/compare/{base}...{head}?per_page=100",
                            "--jq", ".commits[] | [.sha, (.parents | map(.sha) | join(\",\")),"
                                    " .commit.committer.date] | @tsv"], "writes": False},
    "label-add": {"argv": ["gh", "pr", "edit", "{pr}", "--repo", "{repo}", "--add-label", "{label}"],
                  "writes": True},
    "label-remove": {"argv": ["gh", "pr", "edit", "{pr}", "--repo", "{repo}", "--remove-label", "{label}"],
                     "writes": True},
    "label-list": {"argv": ["gh", "label", "list", "--repo", "{repo}", "--search", "{label}",
                            "--limit", "100", "--json", "name"], "writes": False},
    "label-create": {"argv": ["gh", "label", "create", "{label}", "--repo", "{repo}", "--color", "D93F0B",
                              "--description", "A fix agent is working on this PR (Review Desk)"],
                     "writes": True},
    # The needs-work text goes in on stdin, never in argv.
    "pr-comment": {"argv": ["gh", "pr", "comment", "{pr}", "--repo", "{repo}", "--body-file", "-"],
                   "writes": True},
    "pr-close": {"argv": ["gh", "api", "-X", "PATCH", "repos/{repo}/pulls/{pr}", "-f", "state=closed"],
                 "writes": True},
    "issue-list": {"argv": ["gh", "issue", "list", "--repo", "{repo}", "--state", "open",
                            "--limit", "1000", "--json", ISSUE_FIELDS], "writes": False},
    "issue-state": {"argv": ["gh", "issue", "view", "{issue}", "--repo", "{repo}", "--json", "state,title,body"],
                    "writes": False},
    # One argument (--title=...), so a title that starts with "-" is never read as
    # a flag. The description goes in on stdin, never in argv.
    "issue-edit": {"argv": ["gh", "issue", "edit", "{issue}", "--repo", "{repo}", "--title={title}",
                            "--body-file", "-"], "writes": True},
    "issue-close": {"argv": ["gh", "issue", "close", "{issue}", "--repo", "{repo}"], "writes": True},
    "issue-reopen": {"argv": ["gh", "issue", "reopen", "{issue}", "--repo", "{repo}"], "writes": True},
    "mergeme-prs": {"argv": ["gh", "pr", "list", "--repo", "{repo}", "--state", "open", "--label", "mergeme",
                             "--limit", "300", "--json", "number"], "writes": False},
    "git-common-dir": {"argv": ["git", "-C", "{path}", "rev-parse", "--path-format=absolute", "--git-common-dir"],
                       "writes": False},
    "orca-status": {"argv": ["orca", "status", "--json"], "writes": False},
    "orca-open": {"argv": ["orca", "open", "--json"], "writes": True},
    "orca-list": {"argv": ["orca", "worktree", "list", "--repo", "path:{path}", "--limit", "1000", "--json"],
                  "writes": False},
    "orca-create-fix": {"argv": ["orca", "worktree", "create", "--name", "{workspace}", "--repo", "path:{path}",
                                 "--pr", "{pr}", "--agent", "claude", "--prompt", "/pr-fix {pr}",
                                 "--model", "{model}", "--effort", "{effort}", "--json"], "writes": True},
    "orca-create-mergeme": {"argv": ["orca", "worktree", "create", "--name", "{workspace}", "--repo", "path:{path}",
                                     "--base-branch", "main", "--agent", "claude", "--prompt", "/mergeme",
                                     "--json"], "writes": True},
}

# What each placeholder may hold. Anything else is refused before a command runs.
PARAMS = {
    "pr": lambda v: rules.parse_pr(v) is not None,
    "issue": lambda v: rules.parse_pr(v) is not None,
    "title": lambda v: rules.clean_title(v) == v,
    "repo": lambda v: bool(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", str(v))),
    "base": lambda v: bool(re.fullmatch(r"[A-Za-z0-9_./-]{1,200}", str(v))) and ".." not in str(v),
    "head": lambda v: bool(re.fullmatch(r"[0-9a-f]{40}", str(v))),
    "label": lambda v: v in (rules.LABEL_MERGEME, rules.LABEL_NEEDS_WORK),
    "path": lambda v: Path(str(v)).is_absolute(),
    "workspace": lambda v: any(re.fullmatch(p, str(v)) for p in (rules.FIX_NAME, rules.MERGEME_NAME)),
    "model": lambda v: v in ("opus", "sonnet", "haiku", "fable")
                       or bool(re.fullmatch(r"claude-[a-z0-9][a-z0-9.-]{0,60}", str(v))),
    "effort": lambda v: v in ("low", "medium", "high", "xhigh", "max"),
}


class CommandError(Exception):
    def __init__(self, message, out=""):
        super().__init__(message)
        self.out = out  # stdout of a failed command (orca puts its JSON error there)


# ------------------------------------------------------------ config + logs

def home():
    return Path(os.environ.get("REVIEW_DESK_HOME") or "~/skrongle-workflow").expanduser()


def log_dir():
    d = home() / "log"
    d.mkdir(parents=True, exist_ok=True)
    return d


def log_action(line):
    stamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    with open(log_dir() / "actions.log", "a", encoding="utf-8") as f:
        f.write("%s  %s\n" % (stamp, line))


def repo_from_url(url):
    m = re.search(r"github\.com[:/]([^/\s]+/[^/\s]+?)(?:\.git)?/?$", url.strip())
    return m.group(1) if m else ""


def load_config():
    """config.json, created with defaults on the first run. Missing keys get defaults.

    repo_path and repo are not saved: they come from the workspace the desk was
    started from, on every run, unless config.json sets them."""
    path = home() / "config.json"
    defaults = {
        "demos": "~/skrongle-demos",
        "fix_agent_model": "opus",
        "fix_agent_effort": "high",
        "fix_agent_limit": 3,
    }
    cfg = {}
    if path.exists():
        cfg = json.loads(path.read_text(encoding="utf-8"))
    saved = {**defaults, **cfg}
    if saved != cfg:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(saved, indent=2) + "\n", encoding="utf-8")
    merged = {"repo_path": str(REPO_ROOT), "repo": "", **saved}
    if not merged["repo"]:
        try:
            merged["repo"] = repo_from_url(run("git-origin", path=merged["repo_path"]))
        except CommandError:
            pass
    return merged


def decisions_dir():
    return home() / "decisions"


def last_mergeme_line():
    path = home() / "log" / "mergeme.log"
    try:
        lines = [l for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
    except OSError:
        return None
    return lines[-1] if lines else None


# ----------------------------------------------------------------- commands

def build_argv(name, params):
    spec = COMMANDS[name]
    for key, value in params.items():
        check = PARAMS.get(key)
        if check is None or not check(value):
            raise CommandError("refused %s: bad %s" % (name, key))

    def fill(arg):
        def sub(m):
            if m.group(1) not in params:
                raise CommandError("refused %s: no %s" % (name, m.group(1)))
            return str(params[m.group(1)])
        return re.sub(r"\{(%s)\}" % "|".join(PARAMS), sub, arg)

    return [fill(a) for a in spec["argv"]]


def run(name, stdin=None, timeout=60, **params):
    """Run one command from COMMANDS. Returns stdout. A dry run skips writes."""
    argv = build_argv(name, params)
    shown = shlex.join(argv)
    if COMMANDS[name]["writes"]:
        if DRY_RUN:
            fed = "\n  with on stdin:\n" + "".join("    " + l + "\n" for l in stdin.splitlines()) if stdin else ""
            log_action("DRY RUN, not run: " + shown + fed.rstrip("\n"))
            print("DRY RUN, not run: " + shown + fed.rstrip("\n"), flush=True)
            return ""
        log_action("run: " + shown)
    try:
        p = subprocess.run(argv, input=stdin, capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as e:
        raise CommandError("%s failed: %s" % (argv[0], e))
    if p.returncode != 0:
        err = (p.stderr or p.stdout).strip().splitlines()
        if COMMANDS[name]["writes"]:
            log_action("failed: %s: %s" % (shown, err[-1] if err else p.returncode))
        raise CommandError(err[-1] if err else "%s exited %d" % (argv[0], p.returncode), p.stdout)
    return p.stdout


# ------------------------------------------------------------------- queue

# A cached PR or issue list older than this is fetched again on the next plain
# request. The lists used to live until the reload button: issues closed by a
# mergeme batch kept showing in the Issues mode for the rest of the day.
LIST_TTL = 300


class Desk:
    def __init__(self, cfg):
        self.cfg = cfg
        self.lock = threading.Lock()
        self.queue = None
        self.queue_at = 0.0
        self.issues = None
        self.issues_at = 0.0
        self.issues_lock = threading.Lock()
        self.decisions_lock = threading.Lock()
        self.stale_cache = {}
        self.orca_repo = None

    def own_commits(self, pr):
        """The PR's commits its base doesn't have, or None if GitHub can't say."""
        try:
            out = run("pr-commits", repo=self.cfg["repo"], base=pr["baseRefName"], head=pr["headRefOid"])
        except CommandError:
            return None
        commits = []
        for line in out.splitlines():
            parts = line.split("\t")
            if len(parts) == 3:
                commits.append({"sha": parts[0], "parents": [p for p in parts[1].split(",") if p],
                                "date": parts[2]})
        return commits

    def stale_for(self, pr):
        report = rules.latest_qa(pr.get("comments"), pr["number"]) or {}
        at = rules.checked_at(report.get("body"))
        head = pr.get("headRefOid") or ""
        key = (pr["number"], head, at or report.get("createdAt"))
        if key in self.stale_cache:
            return self.stale_cache[key]
        own = self.own_commits(pr) if rules.needs_commit_list(head, at) else []
        stale = rules.is_stale(head, at, report.get("createdAt"), own)
        self.stale_cache[key] = stale
        return stale

    def load(self, refresh=False):
        with self.lock:
            if self.queue is not None and not refresh and time.time() - self.queue_at < LIST_TTL:
                return self.queue
            started = time.time()
            prs = json.loads(run("pr-list", repo=self.cfg["repo"], timeout=120) or "[]")
            todo = [p for p in prs if rules.in_queue(p)]
            # Only PRs whose head moved since QA cost a call; run them side by side.
            with ThreadPoolExecutor(max_workers=8) as pool:
                stale = dict(zip([p["number"] for p in todo], pool.map(self.stale_for, todo)))
            q = rules.build_queue(prs, stale)
            demos = Path(self.cfg["demos"]).expanduser()
            try:
                names = [p.name for p in demos.iterdir() if p.is_file()]
            except OSError:
                names = []
            for c in q["cards"]:
                c["demo"] = rules.find_demo(demos, c["number"], read_manifest(demos, c["number"]),
                                            names, c["qa_page_url"])
                if c["demo"]["why"]:
                    c["why"] = c["demo"]["why"]
            q.update({
                "repo": self.cfg["repo"],
                "dry_run": DRY_RUN,
                "mergeme_log": last_mergeme_line(),
                "loaded_at": datetime.now().strftime("%H:%M"),
                "took_ms": int((time.time() - started) * 1000),
            })
            self.queue = q
            self.queue_at = time.time()
            return q


    def load_issues(self, refresh=False):
        with self.issues_lock:
            if self.issues is not None and not refresh and time.time() - self.issues_at < LIST_TTL:
                return self.issues
            started = time.time()
            issues = json.loads(run("issue-list", repo=self.cfg["repo"], timeout=120) or "[]")
            q = rules.build_issue_queue(issues)
            q.update({
                "repo": self.cfg["repo"],
                "dry_run": DRY_RUN,
                "loaded_at": datetime.now().strftime("%H:%M"),
                "took_ms": int((time.time() - started) * 1000),
            })
            self.issues = q
            self.issues_at = time.time()
            return q

    def drop_issue(self, n):
        """Take a closed issue out of the cached list, so it doesn't come back
        on the next load before the list expires."""
        with self.issues_lock:
            if self.issues is None:
                return
            cards = [c for c in self.issues["cards"] if c["number"] != n]
            self.issues = dict(self.issues, cards=cards, counts={"queue": len(cards)})


# --------------------------------------------------------------- decisions

def read_decision_files():
    """(id, parsed JSON) for each decisions/*.json. A file that isn't JSON is left out."""
    try:
        paths = sorted(decisions_dir().glob("*.json"))
    except OSError:
        return []
    out = []
    for path in paths:
        if not rules.decision_id_ok(path.stem) or not path.is_file():
            continue
        try:
            out.append((path.stem, json.loads(path.read_text(encoding="utf-8"))))
        except (OSError, ValueError):
            continue
    return out


def load_decisions(desk):
    q = rules.build_decision_queue(read_decision_files())
    q.update({"repo": desk.cfg["repo"], "dry_run": DRY_RUN, "folder": str(decisions_dir()),
              "loaded_at": datetime.now().strftime("%H:%M")})
    return q


def change_decision(desk, did, change):
    """Read one question's file, let change(raw) edit it, write it back in one step.
    Returns the card after the change, or None if there is no such open question."""
    if not rules.decision_id_ok(did):
        return None
    path = decisions_dir() / (did + ".json")
    with desk.decisions_lock:
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None
        card = rules.decision_card(did, raw)
        if card is None or card["closed"]:
            return None
        if change(raw, card) is False:
            return card
        tmp = path.with_name("." + path.name + ".tmp")
        tmp.write_text(json.dumps(raw, indent=2) + "\n", encoding="utf-8")
        os.replace(tmp, path)
        return rules.decision_card(did, raw)


def now_stamp():
    return datetime.now().astimezone().isoformat(timespec="seconds")


def read_manifest(demos, n):
    path = demos / ("pr-%d" % n) / "manifest.json"
    try:
        m = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    if not isinstance(m, dict) or m.get("pr") not in (n, None):
        return None
    return m


# -------------------------------------------------------------------- orca

def command_to_copy(name, params):
    """The command as a person would paste it in a terminal (no --json)."""
    return shlex.join(a for a in build_argv(name, params) if a != "--json")


def orca_json(name, timeout=60, **params):
    """Run an orca --json command. Returns its "result", or {} in a dry run.
    Orca's own error message wins over the exit code."""
    try:
        out = run(name, timeout=timeout, **params)
    except CommandError as e:
        out = e.out
        try:
            msg = (json.loads(out).get("error") or {}).get("message")
        except (ValueError, AttributeError):
            msg = None
        raise CommandError(msg or str(e))
    if not out.strip():
        return {}
    try:
        data = json.loads(out)
    except ValueError:
        raise CommandError("orca gave an answer that isn't JSON")
    if not data.get("ok", True):
        raise CommandError((data.get("error") or {}).get("message") or "orca said no")
    return data.get("result") or {}


def orca_reachable():
    try:
        return bool(orca_json("orca-status").get("runtime", {}).get("reachable"))
    except CommandError:
        return False


def ensure_orca():
    """True when Orca answers. Runs `orca open` first if it doesn't."""
    if orca_reachable():
        return True
    try:
        run("orca-open", timeout=120)
    except CommandError as e:
        log_action("orca open failed: %s" % e)
        return False
    return orca_reachable()


def orca_repo(desk):
    """The repo's main checkout. Orca knows a repo by that path, not a worktree's."""
    if not desk.orca_repo:
        path = desk.cfg["repo_path"]
        try:
            common = Path(run("git-common-dir", path=path).strip())
            if common.name == ".git":
                path = str(common.parent)
        except CommandError:
            pass
        desk.orca_repo = path
    return desk.orca_repo


def open_workspaces(desk):
    rows = orca_json("orca-list", path=orca_repo(desk)).get("worktrees") or []
    return rules.workspaces(rows)


def fix_limit(desk):
    try:
        return int(desk.cfg.get("fix_agent_limit", 3))
    except (TypeError, ValueError):
        return 3


def orca_info(desk):
    """What the page needs before W and Run mergeme. Never raises."""
    info = {"reachable": orca_reachable(), "fix": [], "mergeme": [], "fix_limit": fix_limit(desk),
            "mergeme_prs": [], "error": None}
    if info["reachable"]:
        try:
            info.update(open_workspaces(desk))
        except CommandError as e:
            info["error"] = "Could not list the Orca workspaces: %s" % e
    try:
        info["mergeme_prs"] = sorted(p["number"] for p in json.loads(run("mergeme-prs", repo=desk.cfg["repo"]) or "[]"))
    except (CommandError, ValueError, KeyError, TypeError) as e:
        info["error"] = info["error"] or "Could not count the mergeme PRs: %s" % e
    return info


# ----------------------------------------------------------------- actions
# Each action takes (desk, pr, body) and returns {"ok", "message", ...}.

def still_open(desk, pr):
    """None if the PR is open, else the reason to skip it."""
    try:
        info = json.loads(run("pr-state", pr=pr, repo=desk.cfg["repo"]))
    except (CommandError, ValueError) as e:
        return "could not check #%d: %s" % (pr, e)
    state = (info.get("state") or "").upper()
    if state != "OPEN":
        return "#%d is %s now" % (pr, state.lower() or "gone")
    return None


def act_mergeme(desk, pr, body):
    why_not = still_open(desk, pr)
    if why_not:
        log_action("mergeme #%d skipped: %s" % (pr, why_not))
        return {"ok": False, "skipped": True, "message": "Skipped: %s." % why_not}
    run("label-add", pr=pr, repo=desk.cfg["repo"], label=rules.LABEL_MERGEME)
    log_action("mergeme #%d%s" % (pr, " (red card, confirmed)" if body.get("confirmed_red") else ""))
    return {"ok": True, "message": "#%d labelled mergeme." % pr}


def act_pr_close(desk, pr, body):
    """Close the PR on GitHub (#1239). The page asks first; this needs its yes."""
    if not body.get("confirmed"):
        return {"ok": False, "message": "Closing a PR needs a yes on the page first."}
    why_not = still_open(desk, pr)
    if why_not:
        log_action("pr-close #%d skipped: %s" % (pr, why_not))
        return {"ok": False, "skipped": True, "message": "Skipped: %s." % why_not}
    run("pr-close", pr=pr, repo=desk.cfg["repo"])
    log_action("pr-close #%d" % pr)
    return {"ok": True, "message": "Closed #%d." % pr}


def act_skip(desk, pr, body):
    log_action("skip #%d" % pr)
    return {"ok": True, "message": "Skipped #%d." % pr}


def act_undo(desk, pr, body):
    what = body.get("of")
    if what == "mergeme":
        run("label-remove", pr=pr, repo=desk.cfg["repo"], label=rules.LABEL_MERGEME)
    elif what != "skip":
        return {"ok": False, "message": "Nothing to undo."}
    log_action("undo %s #%d" % (what, pr))
    return {"ok": True, "message": "Undone. #%d is back." % pr}


class StepFailed(Exception):
    pass


def steps(names):
    """Run numbered steps; a failure says which step stopped, in plain words."""
    total = len(names)

    def step(i, fn):
        try:
            return fn()
        except CommandError as e:
            done = " The steps before it are done." if i > 1 else " Nothing changed."
            raise StepFailed("Stopped at step %d of %d (%s): %s.%s" % (i, total, names[i - 1], e, done))
    return step


def act_needs_work(desk, pr, body):
    text = rules.clean_fix_text(body.get("text"))
    if not rules.enough_words(text):
        return {"ok": False, "message": "Write at least %d words for the agent (and under %d characters)."
                % (rules.MIN_FIX_WORDS, rules.MAX_FIX_CHARS)}
    why_not = still_open(desk, pr)
    if why_not:
        log_action("needs-work #%d skipped: %s" % (pr, why_not))
        return {"ok": False, "skipped": True, "message": "Skipped: %s." % why_not}

    repo, name = desk.cfg["repo"], "fix-pr-%d" % pr
    create = dict(workspace=name, path=orca_repo(desk), pr=pr,
                  model=desk.cfg.get("fix_agent_model"), effort=desk.cfg.get("fix_agent_effort"))
    try:
        command = command_to_copy("orca-create-fix", create)
    except CommandError as e:
        return {"ok": False, "message": "config.json: %s (check fix_agent_model and fix_agent_effort)." % e}

    # Orca first, so a full desk or a taken name stops before GitHub changes.
    orca_up = ensure_orca()
    if orca_up:
        try:
            fix = open_workspaces(desk)["fix"]
        except CommandError as e:
            return {"ok": False, "message": "Could not list the Orca workspaces: %s. Nothing changed." % e}
        if any(re.fullmatch(re.escape(name) + r"(-[0-9]+)?", f) for f in fix):
            return {"ok": False, "message": "%s is already open in Orca. Close it there first. Nothing changed." % name}
        if rules.at_fix_limit(len(fix), fix_limit(desk)) and not body.get("confirmed_limit"):
            return {"ok": False, "needs_confirm": "limit", "running": len(fix), "limit": fix_limit(desk),
                    "message": "%d fix agents are running (the limit is %d). Nothing changed."
                    % (len(fix), fix_limit(desk))}

    step = steps(["create the needs work label", "add the needs work label", "post the comment",
                  "start the fix agent in Orca"])
    try:
        def make_label():
            have = json.loads(run("label-list", repo=repo, label=rules.LABEL_NEEDS_WORK) or "[]")
            if not any(l.get("name") == rules.LABEL_NEEDS_WORK for l in have):
                run("label-create", repo=repo, label=rules.LABEL_NEEDS_WORK)
        try:
            step(1, make_label)
        except ValueError:
            raise StepFailed("Stopped at step 1 of 4 (create the needs work label): gh gave an answer"
                             " that isn't JSON. Nothing changed.")
        step(2, lambda: run("label-add", pr=pr, repo=repo, label=rules.LABEL_NEEDS_WORK))
        step(3, lambda: run("pr-comment", pr=pr, repo=repo, stdin=rules.needs_work_comment(text)))
        quote = " ".join(text.split())
        log_action("needs-work #%d: label and comment done (%s)" % (pr, quote[:200]))
        if not orca_up:
            log_action("needs-work #%d: Orca not reachable, gave the command to copy" % pr)
            return {"ok": False, "partial": True, "command": command,
                    "message": "#%d has the label and the comment, but Orca isn't running and `orca open`"
                               " failed. Start the fix agent by hand:" % pr}
        result = step(4, lambda: orca_json("orca-create-fix", timeout=300, **create))
    except StepFailed as e:
        log_action("needs-work #%d failed: %s" % (pr, e))
        return {"ok": False, "message": "#%d: %s" % (pr, e), "command": command}
    where = (result.get("worktree") or {}).get("path") or result.get("path") or name
    log_action("needs-work #%d: started %s (%s, %s effort)" % (pr, name, create["model"], create["effort"]))
    return {"ok": True, "workspace": name, "path": where,
            "message": "#%d: fix agent started in %s. It can't be undone here: close that workspace in Orca"
                       " to stop it." % (pr, name)}


def act_run_mergeme(desk, pr, body):
    if not body.get("confirmed"):
        return {"ok": False, "message": "Run mergeme needs a yes on the page first."}
    name = "mergeme-%s" % datetime.now().strftime("%Y-%m-%d")
    create = dict(workspace=name, path=orca_repo(desk))
    command = command_to_copy("orca-create-mergeme", create)
    if not ensure_orca():
        log_action("run-mergeme: Orca not reachable, gave the command to copy")
        return {"ok": False, "command": command,
                "message": "Orca isn't running and `orca open` failed. Start mergeme by hand:"}
    try:
        busy = open_workspaces(desk)["mergeme"]
        if busy:
            return {"ok": False, "message": "%s is still open in Orca. Close it there first." % busy[0]}
        orca_json("orca-create-mergeme", timeout=300, **create)
    except CommandError as e:
        log_action("run-mergeme failed: %s" % e)
        return {"ok": False, "message": "Could not start mergeme: %s." % e, "command": command}
    log_action("run-mergeme: started %s" % name)
    return {"ok": True, "workspace": name, "message": "Mergeme started in the %s workspace in Orca." % name}


def act_issue_edit(desk, n, body):
    """Set an issue's title and/or description, typed by hand on the page (also
    how U undoes it). A field left out keeps what the issue has now."""
    title = body.get("title")
    if title is not None:
        title = rules.clean_title(title)
        if title is None:
            return {"ok": False, "message": "A title is one line of 1 to %d characters." % rules.MAX_TITLE_CHARS}
    text = body.get("body")
    if text is not None:
        text = rules.clean_body(text)
        if text is None:
            return {"ok": False, "message": "A description is at most %d characters, with no control characters."
                                            % rules.MAX_BODY_CHARS}
    if title is None and text is None:
        return {"ok": False, "message": "Nothing to save."}
    try:
        info = json.loads(run("issue-state", issue=n, repo=desk.cfg["repo"]))
    except (CommandError, ValueError) as e:
        return {"ok": False, "message": "Could not check issue #%d: %s" % (n, e)}
    state = (info.get("state") or "").upper()
    if state != "OPEN":
        log_action("issue-edit #%d skipped: %s" % (n, state.lower() or "gone"))
        return {"ok": False, "skipped": True, "message": "Skipped: issue #%d is %s now." % (n, state.lower() or "gone")}
    old_title, old_body = info.get("title") or "", info.get("body") or ""
    # Line ends and trailing blank lines don't count as a change (GitHub keeps a final newline).
    old_clean = rules.clean_body(old_body)
    if old_clean is None:  # a description the desk could not have written; keep it as it is
        old_clean = old_body
    if title is None:
        title = old_title
    if text is None:
        text = old_clean
    changed = [w for w, a, b in (("title", old_title, title), ("description", old_clean, text)) if a != b]
    if not changed:
        return {"ok": True, "unchanged": True, "old_title": old_title, "old_body": old_body,
                "message": "Issue #%d already says that." % n}
    run("issue-edit", issue=n, repo=desk.cfg["repo"], title=title, stdin=text)
    if old_title != title:
        log_action("issue-edit #%d title: %r -> %r" % (n, old_title, title))
    if "description" in changed:
        log_action("issue-edit #%d description: %d -> %d characters" % (n, len(old_body), len(text)))
    what = " and ".join(changed)
    return {"ok": True, "old_title": old_title, "old_body": old_body, "title": title, "body": text,
            "changed": changed, "message": "Issue #%d: saved the %s." % (n, what)}


def issue_state(desk, n):
    """(state, info) of an issue, or (None, message) when gh could not say."""
    try:
        info = json.loads(run("issue-state", issue=n, repo=desk.cfg["repo"]))
    except (CommandError, ValueError) as e:
        return None, "Could not check issue #%d: %s" % (n, e)
    return (info.get("state") or "").upper(), info


def act_issue_close(desk, n, body):
    """Close an issue from the page. U reopens it (issue-reopen) for a few seconds."""
    state, info = issue_state(desk, n)
    if state is None:
        return {"ok": False, "message": info}
    if state != "OPEN":
        log_action("issue-close #%d skipped: %s" % (n, state.lower() or "gone"))
        return {"ok": False, "skipped": True, "message": "Skipped: issue #%d is %s already." % (n, state.lower() or "gone")}
    run("issue-close", issue=n, repo=desk.cfg["repo"])
    desk.drop_issue(n)
    log_action("issue-close #%d: %r" % (n, info.get("title") or ""))
    return {"ok": True, "message": "Closed issue #%d." % n}


def act_issue_reopen(desk, n, body):
    """Undo a close from the page."""
    state, info = issue_state(desk, n)
    if state is None:
        return {"ok": False, "message": info}
    if state == "OPEN":
        return {"ok": True, "unchanged": True, "message": "Issue #%d is open." % n}
    run("issue-reopen", issue=n, repo=desk.cfg["repo"])
    log_action("issue-reopen #%d" % n)
    return {"ok": True, "message": "Issue #%d is open again." % n}


def act_decision_answer(desk, _, body):
    """Save (or clear) the answer to one question. The question stays open."""
    did = body.get("id")
    saved = {}

    def change(raw, card):
        answer = rules.clean_answer(body.get("pick"), body.get("text"), card["options"])
        if answer is None:
            saved["bad"] = True
            return False
        if answer["pick"] or answer["text"]:
            raw["answer"] = dict(answer, answered_at=now_stamp())
        else:
            raw.pop("answer", None)
    card = change_decision(desk, did, change)
    if card is None:
        return {"ok": False, "message": "That question is gone or already sent. Reload."}
    if saved.get("bad"):
        text = body.get("text")
        if isinstance(text, str) and len(text.strip()) > rules.MAX_ANSWER_CHARS:
            return {"ok": False, "message": "An answer is at most %d characters." % rules.MAX_ANSWER_CHARS}
        return {"ok": False, "message": "The answer has a character that can't be saved. Take it out and try again."}
    log_action("decision %s: %s" % (did, "answered" if card["answered"] else "answer cleared"))
    return {"ok": True, "card": card}


def act_decision_prompt(desk, _, body):
    q = rules.build_decision_queue(read_decision_files())
    done = [c for c in q["cards"] if c["answered"]]
    if not done:
        return {"ok": False, "message": "Answer at least one question first."}
    return {"ok": True, "prompt": rules.decision_prompt(done), "ids": [c["id"] for c in done]}


def act_decision_close(desk, body, field):
    ids = body.get("ids")
    if not isinstance(ids, list) or not ids or not all(rules.decision_id_ok(i) for i in ids):
        return {"ok": False, "message": "No questions named."}
    closed = []
    for did in ids:
        def change(raw, card):
            raw[field] = now_stamp()
        if change_decision(desk, did, change) is not None:
            closed.append(did)
    log_action("decisions %s: %s" % ("sent" if field == "sent_at" else "dismissed", ", ".join(closed) or "none"))
    return {"ok": True, "closed": closed}


def act_decision_sent(desk, _, body):
    """Mark the questions in a copied prompt as sent, so they don't show again."""
    r = act_decision_close(desk, body, "sent_at")
    if r["ok"]:
        n = len(r["closed"])
        r["message"] = "%d question%s marked sent. They won't show again." % (n, "" if n == 1 else "s")
    return r


def act_decision_dismiss(desk, _, body):
    """Drop a question that no longer needs an answer."""
    did = body.get("id")
    r = act_decision_close(desk, {"ids": [did]}, "dismissed_at")
    if r["ok"]:
        r["message"] = "Dropped %s." % did if r["closed"] else "That question is gone or already sent."
    return r


ACTIONS = {
    "mergeme": act_mergeme,
    "skip": act_skip,
    "pr-close": act_pr_close,
    "undo": act_undo,
    "needs-work": act_needs_work,
    "run-mergeme": act_run_mergeme,
    "issue-edit": act_issue_edit,
    "issue-close": act_issue_close,
    "issue-reopen": act_issue_reopen,
    "decision-answer": act_decision_answer,
    "decision-prompt": act_decision_prompt,
    "decision-sent": act_decision_sent,
    "decision-dismiss": act_decision_dismiss,
}
# Actions for the whole batch or the decisions, not one PR.
NO_PR = {"run-mergeme", "decision-answer", "decision-prompt", "decision-sent", "decision-dismiss"}
ON_ISSUE = {"issue-edit", "issue-close", "issue-reopen"}  # actions that take "issue" instead of "pr"


# --------------------------------------------------------------------- HTTP

class Handler(BaseHTTPRequestHandler):
    server_version = "ReviewDesk"
    desk = None
    key = ""
    port = 0

    def log_message(self, fmt, *args):  # keep the terminal quiet
        pass

    def send_json(self, code, obj):
        data = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def guard(self, api):
        """Host check on everything; key check on /api. True when allowed."""
        if not rules.host_ok(self.headers.get("Host"), self.port):
            self.send_json(403, {"error": "wrong host"})
            return False
        if api:
            q = parse_qs(urlsplit(self.path).query)
            given = self.headers.get("X-Review-Key") or (q.get("k") or [""])[0]
            if not rules.key_ok(given, self.key):
                self.send_json(403, {"error": "missing or wrong key"})
                return False
        return True

    def do_GET(self):
        path = urlsplit(self.path).path
        if not self.guard(path.startswith("/api/")):
            return
        if path in ("/", "/index.html"):
            data = (HERE / "static" / "index.html").read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("Referrer-Policy", "no-referrer")
            self.end_headers()
            self.wfile.write(data)
        elif path == "/api/queue":
            refresh = "refresh" in parse_qs(urlsplit(self.path).query)
            try:
                self.send_json(200, self.desk.load(refresh=refresh))
            except (CommandError, ValueError) as e:
                self.send_json(502, {"error": "Could not load the PRs: %s" % e})
        elif path.startswith("/api/demo/"):
            self.send_demo(path)
        elif path == "/api/issues":
            refresh = "refresh" in parse_qs(urlsplit(self.path).query)
            try:
                self.send_json(200, self.desk.load_issues(refresh=refresh))
            except (CommandError, ValueError) as e:
                self.send_json(502, {"error": "Could not load the issues: %s" % e})
        elif path == "/api/decisions":
            self.send_json(200, load_decisions(self.desk))
        elif path == "/api/orca":
            self.send_json(200, orca_info(self.desk))
        elif path == "/api/usage":
            try:
                records = usage.collect(write=True)
            except OSError as e:
                return self.send_json(502, {"error": "Could not read the usage: %s" % e})
            self.send_json(200, {"records": records, "log": str(home() / "log" / "usage.jsonl")})
        else:
            self.send_json(404, {"error": "not found"})

    def send_demo(self, path):
        parts = path.split("/")
        if len(parts) != 5:
            return self.send_json(404, {"error": "not found"})
        n = rules.parse_pr(parts[3])
        file = rules.demo_file(Path(self.desk.cfg["demos"]).expanduser(), n, unquote(parts[4])) if n else None
        if file is None:
            return self.send_json(404, {"error": "not found"})
        size = file.stat().st_size
        rng = rules.parse_range(self.headers.get("Range"), size)
        if rng == "bad":
            self.send_response(416)
            self.send_header("Content-Range", "bytes */%d" % size)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        start, end = rng if rng else (0, size - 1)
        self.send_response(206 if rng else 200)
        self.send_header("Content-Type", rules.DEMO_TYPES[file.suffix.lower()])
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(max(0, end - start + 1)))
        if rng:
            self.send_header("Content-Range", "bytes %d-%d/%d" % (start, end, size))
        self.end_headers()
        try:
            with open(file, "rb") as f:
                f.seek(start)
                left = end - start + 1
                while left > 0:
                    chunk = f.read(min(256 * 1024, left))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    left -= len(chunk)
        except (BrokenPipeError, ConnectionResetError):
            pass  # the video player moved on to another range

    def do_POST(self):
        path = urlsplit(self.path).path
        if not self.guard(True):
            return
        if not rules.origin_ok(self.headers.get("Origin"), self.port):
            return self.send_json(403, {"error": "wrong origin"})
        if path != "/api/action":
            return self.send_json(404, {"error": "not found"})
        try:
            length = int(self.headers.get("Content-Length") or 0)
            body = json.loads(self.rfile.read(min(length, 64 * 1024)) or b"{}")
        except ValueError:
            return self.send_json(400, {"error": "bad JSON"})
        if not isinstance(body, dict):
            return self.send_json(400, {"error": "bad JSON"})
        name = body.get("action")
        action = ACTIONS.get(name)
        if action is None:
            return self.send_json(400, {"error": "unknown action"})
        field = "issue" if name in ON_ISSUE else "pr"
        pr = None if name in NO_PR else rules.parse_pr(body.get(field))
        if pr is None and name not in NO_PR:
            return self.send_json(400, {"error": "%s must be a number" % field})
        try:
            result = action(self.desk, pr, body)
        except CommandError as e:
            log_action("%s%s failed: %s" % (name, " #%d" % pr if pr else "", e))
            result = {"ok": False, "message": "Failed: %s" % e}
        result["dry_run"] = DRY_RUN
        self.send_json(200, result)


def main():
    ap = argparse.ArgumentParser(description="Skrongle Review Desk")
    ap.add_argument("--port", type=int, default=0, help="port (default: a free one)")
    ap.add_argument("--no-open", action="store_true", help="don't open the browser")
    args = ap.parse_args()

    cfg = load_config()
    if not cfg["repo"]:
        sys.exit("No GitHub repo in %s: set \"repo\" (owner/name)." % (home() / "config.json"))
    log_dir()
    Handler.desk = Desk(cfg)
    Handler.key = secrets.token_urlsafe(24)
    httpd = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    Handler.port = httpd.server_address[1]
    url = "http://127.0.0.1:%d/?k=%s" % (Handler.port, Handler.key)
    print("Review Desk for %s%s" % (cfg["repo"], "  (DRY RUN: nothing on GitHub changes)" if DRY_RUN else ""))
    print("Open: " + url, flush=True)
    log_action("desk started on port %d%s" % (Handler.port, " (dry run)" if DRY_RUN else ""))
    if not args.no_open:
        webbrowser.open(url)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
