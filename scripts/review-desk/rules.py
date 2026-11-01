"""The Review Desk's rules, with no network and no server.

Which PRs show up, which card is red and why, the stale rule, where a PR's
demo is, the needs-work comment, which Orca workspaces count, which issues
the Issues mode shows and what a title may be, the Decisions mode's questions
and prompt, and the request checks. server.py does the I/O and calls these, so
the unit tests (test_review_desk.py) can check every rule on sample data.
"""

import hmac
import re
from pathlib import Path

LABEL_MERGEME = "mergeme"
LABEL_NEEDS_WORK = "needs work"

# A CheckRun conclusion or a status state that makes CI red.
_FAILED = {"FAILURE", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"}
_PENDING_STATES = {"PENDING", "EXPECTED"}

# Files the desk serves from a demo folder: pictures and videos only.
DEMO_TYPES = {
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".webp": "image/webp",
    ".mp4": "video/mp4",
    ".webm": "video/webm",
}
_VIDEO = {".mp4", ".webm"}
_SAFE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,200}$")


# ---------------------------------------------------------------- QA report

def qa_title_re(n):
    return re.compile(r"^## QA: PR #%d(\D|$)" % n)


def latest_qa(comments, n):
    """The newest '## QA: PR #N' comment, or None."""
    title = qa_title_re(n)
    found = [c for c in comments or [] if title.match(c.get("body") or "")]
    return found[-1] if found else None


def checked_at(body):
    """The commit on the report's 'Checked at' line (7-40 hex), or None."""
    m = re.findall(r"(?i)checked at\W*([0-9a-f]{7,40})\b", body or "")
    return m[-1].lower() if m else None


def verdict(body):
    """'merge', 'not ready', or None when the report has no verdict line."""
    for line in (body or "").splitlines():
        m = re.match(r"^\W*verdict\W*(.*)$", line.strip(), re.I)
        if not m:
            continue
        text = m.group(1).replace("*", "").strip(" :.").lower()
        if text.startswith("not ready") or text.startswith("not yet"):
            return "not ready"
        if text.startswith("merge"):
            return "merge"
        if "not ready" in text:
            return "not ready"
        return None
    return None


def section(body, heading):
    """The text under a markdown heading ('### Checklist'), up to the next heading."""
    lines = (body or "").splitlines()
    out, inside = [], False
    for line in lines:
        if re.match(r"^#{1,6}\s", line):
            if inside:
                break
            inside = line.strip().lower().lstrip("#").strip() == heading.lower()
            continue
        if inside:
            out.append(line)
    return "\n".join(out).strip() or None


def why_from_body(body):
    """The first bullet of the PR body's 'What this PR is for', in plain text."""
    text = section(body, "What this PR is for") or ""
    for line in text.splitlines():
        m = re.match(r"^\s*[-*]\s+(.*)", line)
        if m:
            why = m.group(1).replace("**", "").strip()
            return why if len(why) <= 240 else why[:237].rstrip() + "..."
    return ""


def qa_page_url(body):
    """The claude.ai link on or just after the '<!-- qa-page -->' line."""
    lines = (body or "").splitlines()
    for i, line in enumerate(lines):
        if "<!-- qa-page -->" in line:
            for near in lines[i:i + 3]:
                m = re.search(r"https://claude\.ai/[^\s)\"'<>]+", near)
                if m:
                    return m.group(0)
    return None


# ----------------------------------------------------------------------- CI

def ci_state(rollup):
    """{'state': green|red|running|none, 'failed': [names]} from statusCheckRollup.

    A check that ran more than once counts by its newest run only."""
    latest = {}
    for c in rollup or []:
        if c.get("__typename") == "StatusContext" or "context" in c:
            key = ("status", c.get("context") or "")
            when = c.get("startedAt") or c.get("createdAt") or ""
            st = (c.get("state") or "").upper()
            done = st not in _PENDING_STATES
            failed = st in _FAILED
            name = c.get("context") or "status"
        else:
            key = (c.get("workflowName") or "", c.get("name") or "")
            when = c.get("startedAt") or c.get("completedAt") or ""
            done = (c.get("status") or "").upper() == "COMPLETED"
            failed = done and (c.get("conclusion") or "").upper() in _FAILED
            name = c.get("name") or "check"
        if key not in latest or when >= latest[key][0]:
            latest[key] = (when, name, done, failed)
    if not latest:
        return {"state": "none", "failed": []}
    failed = sorted({v[1] for v in latest.values() if v[3]})
    if failed:
        return {"state": "red", "failed": failed}
    if any(not v[2] for v in latest.values()):
        return {"state": "running", "failed": []}
    return {"state": "green", "failed": []}


# -------------------------------------------------------------------- stale

def needs_commit_list(head_sha, at):
    """Whether the stale rule needs the PR's commit list (one API call)."""
    return not (at and (head_sha or "").lower().startswith(at))


def is_stale(head_sha, at, report_created_at, own):
    """True when the PR has its own new work since QA checked it.

    head_sha:  the PR head commit.
    at:        the report's 'Checked at' commit, or None for an older report.
    report_created_at: the report's time, used only when `at` is None.
    own:       the PR's commits its base branch doesn't have, oldest first,
               as [{'sha', 'parents': [sha], 'date'}], or None if GitHub
               could not compare.

    A base merge (a merge commit whose second parent is not one of the PR's
    own commits, i.e. it pulled main or the base branch in) is not new work.
    If `at` is not among the own commits (a rebase or force push), the PR is
    stale. The rule is "Stale" in .claude/skills/pr-qa-demo/references/
    manifest.md. It gets `new` (step 3 there) from one compare call instead
    of two: the own commits that are not `at` or one of its ancestors. That
    is the same set, because an own commit is never behind a base commit.
    """
    if not needs_commit_list(head_sha, at):
        return False
    if own is None:
        return True
    by_sha = {c["sha"]: c for c in own}
    base_merges = {
        c["sha"] for c in own
        if len(c.get("parents") or []) > 1 and c["parents"][1] not in by_sha
    }
    if at:
        start = next((s for s in by_sha if s.lower().startswith(at)), None)
        if start is None:
            return True
        seen, todo = set(), [start]
        while todo:
            s = todo.pop()
            if s in seen or s not in by_sha:
                continue
            seen.add(s)
            todo.extend(by_sha[s].get("parents") or [])
        new = [s for s in by_sha if s not in seen]
    else:
        new = [c["sha"] for c in own if (c.get("date") or "") > (report_created_at or "")]
    return any(s not in base_merges for s in new)


# -------------------------------------------------------------------- queue

def label_names(pr):
    return {(l.get("name") or "") for l in pr.get("labels") or []}


# ------------------------------------------------------------- design PRs
# A design PR (#1239) carries proposals in a claude.ai artifact, not a feature.
# The builder brief's convention: title "[N] Design: ...", and the body's first
# line "Design: https://claude.ai/artifact/<id>".

DESIGN_TITLE = re.compile(r"^(?:\[[0-9]+\])+\s*Design:", re.I)
DESIGN_LINE = re.compile(r"^Design:\s*(https://claude\.ai/artifact/[A-Za-z0-9_-]+)")
ARTIFACT_URL = re.compile(r"https://claude\.ai/artifact/[A-Za-z0-9_-]+")


def first_line(body):
    for line in (body or "").lstrip("\ufeff").splitlines():
        if line.strip():
            return line.strip()
    return ""


def is_design(pr):
    """Title starts with "[N] Design:" or the body's first line with "Design: <artifact link>"."""
    return bool(DESIGN_TITLE.match(pr.get("title") or "")) or bool(DESIGN_LINE.match(first_line(pr.get("body"))))


def design_link(body):
    """The artifact link: the one on the body's first line, else the first one in the body."""
    m = DESIGN_LINE.match(first_line(body))
    if m:
        return m.group(1)
    m = ARTIFACT_URL.search(body or "")
    return m.group(0) if m else ""


def design_prompt(n, title, link):
    """What "Make issues" hands to an agent."""
    return (
        "Read the design PR #%d%s and its artifact (%s), the claude.ai page with the mockups."
        " File one implementation issue per piece the user picked (listed below; if nothing is listed,"
        " ask on the PR which pieces to build), each linking the PR and the artifact. Then close the PR"
        " with a comment that lists the new issues.\n"
        % (n, (" (" + title + ")") if title else "", link or "no artifact link in the PR body")
    )


def card(pr, stale):
    """One PR's card: what the page shows and whether it is red, and why."""
    n = pr["number"]
    report = latest_qa(pr.get("comments"), n) or {}
    body = report.get("body") or ""
    v = verdict(body)
    ci = ci_state(pr.get("statusCheckRollup"))
    design = is_design(pr)
    reasons = []
    if ci["state"] == "red":
        reasons.append("CI red: " + ", ".join(ci["failed"]))
    elif ci["state"] == "running":
        reasons.append("CI still running")
    elif ci["state"] == "none":
        reasons.append("CI has not run")
    if design and not report:
        pass  # a design PR needs no QA report: the artifact is what you review
    elif v == "not ready":
        reasons.append("QA says not ready")
    elif v is None:
        reasons.append("QA report has no verdict")
    if pr.get("mergeable") == "CONFLICTING":
        reasons.append("Conflicts with main")
    if stale:
        reasons.append("Changed since QA")
    return {
        "number": n,
        "title": pr.get("title") or "",
        "url": pr.get("url") or "",
        "created_at": pr.get("createdAt") or "",
        "head": pr.get("headRefOid") or "",
        "base": pr.get("baseRefName") or "",
        "why": why_from_body(pr.get("body")),
        "qa": v or "unknown",
        "ci": ci["state"],
        "ci_failed": ci["failed"],
        "mergeable": pr.get("mergeable") or "UNKNOWN",
        "checked_at": checked_at(body),
        "stale": bool(stale),
        "red": bool(reasons),
        "reasons": reasons,
        "qa_report": body,
        "qa_url": report.get("url") or "",
        "checklist": section(body, "Checklist"),
        "qa_page_url": qa_page_url(pr.get("body")),
        "design": design,
        "artifact": design_link(pr.get("body")) if design else "",
        "design_prompt": design_prompt(n, pr.get("title") or "", design_link(pr.get("body"))) if design else "",
    }


def in_queue(pr):
    """Open (the list only has open PRs), not a draft, QA'd, not labelled.
    A design PR shows without a QA report."""
    if pr.get("isDraft") or pr.get("state", "OPEN") != "OPEN":
        return False
    if label_names(pr) & {LABEL_MERGEME, LABEL_NEEDS_WORK}:
        return False
    return is_design(pr) or latest_qa(pr.get("comments"), pr["number"]) is not None


def build_queue(prs, stale_by_pr):
    """Cards in desk order and the header counts.

    Normal cards first, oldest first, then red cards, oldest first."""
    cards = [card(p, stale_by_pr.get(p["number"], False)) for p in prs if in_queue(p)]
    cards.sort(key=lambda c: (c["red"], c["created_at"], c["number"]))
    open_prs = [p for p in prs if p.get("state", "OPEN") == "OPEN"]
    being_fixed = sum(1 for p in open_prs if LABEL_NEEDS_WORK in label_names(p))
    waiting = sum(
        1 for p in open_prs
        if not p.get("isDraft")
        and not (label_names(p) & {LABEL_MERGEME, LABEL_NEEDS_WORK})
        and not is_design(p)
        and latest_qa(p.get("comments"), p["number"]) is None
    )
    return {
        "cards": cards,
        "counts": {
            "queue": len(cards),
            "red": sum(1 for c in cards if c["red"]),
            "being_fixed": being_fixed,
            "waiting_for_qa": waiting,
        },
    }


# --------------------------------------------------------------------- demo

def safe_name(name):
    return bool(_SAFE_NAME.match(name or "")) and ".." not in name


def demo_file(demos, n, name):
    """The file to serve for /api/demo/<n>/<name>, or None.

    It is either inside ~/skrongle-demos/pr-<n>/ or, for older demos, a file
    in ~/skrongle-demos itself with 'pr-<n>-' in its name. Only pictures and
    videos, never a path outside those folders."""
    if not isinstance(n, int) or n <= 0 or not safe_name(name):
        return None
    if Path(name).suffix.lower() not in DEMO_TYPES:
        return None
    root = Path(demos).expanduser().resolve()
    for base, ok in ((root / ("pr-%d" % n), True), (root, ("pr-%d-" % n) in name)):
        if not ok:
            continue
        try:
            path = (base / name).resolve()
            base = base.resolve()
        except OSError:
            continue
        if path.parent == base and path.is_file():
            return path
    return None


def find_demo(demos, n, manifest, legacy_names, page_url):
    """What the card's Screenshots and Video tabs show.

    manifest:      the parsed pr-<n>/manifest.json, or None.
    legacy_names:  file names in the demos folder (for the 'pr-<n>-' fallback).
    page_url:      the QA page link from the PR body, or None.
    Returns {'kind': manifest|legacy|qa-page|none, 'items', 'videos',
             'page_url', 'why'}. URLs are relative to the page and still
    need the key added by the page."""
    def url(name):
        return "/api/demo/%d/%s" % (n, name)

    if isinstance(manifest, dict):
        items = []
        for it in manifest.get("items") or []:
            if not isinstance(it, dict):
                continue
            after, before = it.get("after"), it.get("before")
            items.append({
                "caption": str(it.get("caption") or ""),
                "before": url(before) if isinstance(before, str) and safe_name(before) else None,
                "after": url(after) if isinstance(after, str) and safe_name(after) else None,
            })
        video = manifest.get("video")
        videos = [{"name": video, "url": url(video)}] if isinstance(video, str) and safe_name(video) else []
        return {
            "kind": "manifest",
            "items": items,
            "videos": videos,
            "page_url": manifest.get("qa_page_url") or page_url,
            "why": str(manifest.get("why") or ""),
        }
    tag = "pr-%d-" % n
    mine = sorted(x for x in legacy_names or [] if tag in x and safe_name(x)
                  and Path(x).suffix.lower() in DEMO_TYPES)
    if mine:
        items = [{"caption": _caption(x, tag), "before": None, "after": url(x)}
                 for x in mine if Path(x).suffix.lower() not in _VIDEO]
        videos = [{"name": x, "url": url(x)} for x in mine if Path(x).suffix.lower() in _VIDEO]
        return {"kind": "legacy", "items": items, "videos": videos, "page_url": page_url, "why": ""}
    if page_url:
        return {"kind": "qa-page", "items": [], "videos": [], "page_url": page_url, "why": ""}
    return {"kind": "none", "items": [], "videos": [], "page_url": None, "why": ""}


def _caption(name, tag):
    """'issue-848-pr-890-tutor-cancel.png' -> 'tutor cancel'."""
    stem = Path(name).stem
    rest = stem.split(tag, 1)[1] if tag in stem else stem
    return rest.replace("before-after-", "").replace("-", " ").strip() or stem


# ------------------------------------------------- needs work + workspaces

NEEDS_WORK_MARK = "<!-- review-desk:needs-work -->"
MIN_FIX_WORDS = 3
MAX_FIX_CHARS = 8000
FIX_NAME = re.compile(r"fix-pr-[1-9][0-9]{0,6}")
MERGEME_NAME = re.compile(r"mergeme-[0-9]{4}-[0-9]{2}-[0-9]{2}")


def clean_fix_text(text):
    """The user's needs-work text, trimmed, with the desk's own marker taken
    out so pr-fix never sees two. None when it isn't text."""
    if not isinstance(text, str):
        return None
    text = text.replace("\r\n", "\n").replace("\r", "\n").replace(NEEDS_WORK_MARK, "")
    return text.strip()


def enough_words(text):
    """Start fix agent needs at least 3 words, and not a whole essay."""
    return isinstance(text, str) and len(text.split()) >= MIN_FIX_WORDS and len(text) <= MAX_FIX_CHARS


def needs_work_comment(text):
    """The comment pr-fix reads (the shared format, exactly)."""
    return "## Needs work\n\n%s\n\n%s\n" % (text, NEEDS_WORK_MARK)


def workspace_kind(row):
    """'fix', 'mergeme' or None for one row of `orca worktree list --json`.

    Orca may show a linked PR's title as the name, so the folder name and the
    branch count too. Orca adds -2, -3 when a name is taken."""
    if not isinstance(row, dict) or row.get("isArchived"):
        return None
    names = [
        str(row.get("path") or "").rstrip("/").rsplit("/", 1)[-1],
        str(row.get("branch") or "").rsplit("/", 1)[-1],
        re.sub(r"[\s_]+", "-", str(row.get("displayName") or "").strip().lower()),
    ]
    for kind, pattern in (("mergeme", MERGEME_NAME), ("fix", FIX_NAME)):
        if any(re.fullmatch(pattern.pattern + r"(-[0-9]+)?", name) for name in names):
            return kind
    return None


def workspaces(rows):
    """{"fix": [names], "mergeme": [names]} from `orca worktree list --json` rows."""
    out = {"fix": [], "mergeme": []}
    for row in rows or []:
        kind = workspace_kind(row)
        if kind:
            out[kind].append(str(row.get("path") or "").rstrip("/").rsplit("/", 1)[-1] or row.get("displayName"))
    return out


def at_fix_limit(running, limit):
    """True when starting one more fix agent goes past the limit."""
    try:
        return int(running) >= int(limit)
    except (TypeError, ValueError):
        return False


# ------------------------------------------------------------ issues mode

LABEL_NEEDS_GROOMING = "needs grooming"
GROOMING_TAG = re.compile(r"\[(?:qa|needs grooming)\]", re.I)
MAX_TITLE_CHARS = 256
MAX_BODY_CHARS = 65536  # GitHub's limit for an issue body


def title_tags(title):
    """The grooming tags in an issue title, as written: ['[qa]', '[needs grooming]']."""
    return GROOMING_TAG.findall(title or "")


def strip_tags(title):
    """The title without its grooming tags, spaces tidied."""
    return " ".join(GROOMING_TAG.sub(" ", title or "").split())


def in_issue_queue(issue):
    """Open, and a [qa] or [needs grooming] tag in the title or the needs grooming label."""
    if (issue.get("state") or "OPEN").upper() != "OPEN":
        return False
    return bool(title_tags(issue.get("title"))) or LABEL_NEEDS_GROOMING in label_names(issue)


def clean_title(title):
    """A new issue title from a request: one line, trimmed, 1-256 characters. None otherwise."""
    if not isinstance(title, str):
        return None
    title = title.strip()
    if not title or len(title) > MAX_TITLE_CHARS or any(ord(ch) < 32 or ord(ch) == 127 for ch in title):
        return None
    return title


def clean_body(body):
    """A new issue description from a request: line ends unified, end trimmed, at
    most 65,536 characters, no control characters but newline and tab. Empty is
    allowed (it clears the description). None when it can't be saved."""
    if not isinstance(body, str):
        return None
    body = body.replace("\r\n", "\n").replace("\r", "\n").rstrip()
    if len(body) > MAX_BODY_CHARS or any((ord(ch) < 32 and ch not in "\n\t") or ord(ch) == 127 for ch in body):
        return None
    return body


def issue_card(issue):
    author = issue.get("author") if isinstance(issue.get("author"), dict) else {}
    return {
        "number": issue["number"],
        "title": issue.get("title") or "",
        "url": issue.get("url") or "",
        "created_at": issue.get("createdAt") or "",
        "author": author.get("login") or "",
        "labels": sorted(label_names(issue) - {""}),
        "tags": title_tags(issue.get("title")),
        "body": issue.get("body") or "",
    }


def build_issue_queue(issues):
    """Issue cards for the Issues mode, oldest first."""
    cards = [issue_card(i) for i in issues or [] if isinstance(i, dict) and in_issue_queue(i)]
    cards.sort(key=lambda c: (c["created_at"], c["number"]))
    return {"cards": cards, "counts": {"queue": len(cards)}}


# --------------------------------------------------------- decisions mode
# One JSON file per question in ~/skrongle-workflow/decisions/, written by the
# factory skills with ask.py (format: .claude/skills/backlog-sweep/references/decisions.md).

DECISION_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]{0,119}")
MAX_ANSWER_CHARS = 4000
MAX_OPTIONS = 8

DECISION_PREAMBLE = (
    "These are decisions the engineer made on questions that were blocking work in the"
    " Skrongle repo. Each one names the issue or PR it unblocks. Take them one at a time,"
    " in this order. Open the issue or PR and act on the answer now. An answer"
    " that means build or change something on an issue: build it as one PR"
    " the way .claude/skills/backlog-sweep/references/builder-brief.md says. On a PR: push"
    " the change to that PR's branch the way .claude/skills/pr-fix/SKILL.md does, and run"
    " /pr-qa-demo for it. An answer that means close: close the issue or PR with a comment"
    " that quotes the decision. An answer that means wait, groom or park: post the decision"
    " as a comment and set the label it implies (blocked, on hold, needs grooming). End each"
    " item with a comment on the issue or PR saying what was done. If an answer still"
    " leaves something open, ask again with scripts/review-desk/ask.py instead of guessing."
)


def decision_id_ok(value):
    return isinstance(value, str) and bool(DECISION_ID.fullmatch(value))


def _text(value, limit=MAX_ANSWER_CHARS):
    return value.strip()[:limit] if isinstance(value, str) else ""


def decision_byline(asked_by, asked_at):
    """The chip on a card: who asked and the day, the day once even when asked_by ends in it."""
    day = asked_at[:10]
    if day and asked_by.endswith(day):
        day = ""
    return ", ".join(x for x in (asked_by, day) if x)


def decision_card(did, raw):
    """A question from one decisions file, or None if the file is not one.

    Needs a question and one issue or PR number. Options are optional."""
    if not decision_id_ok(did) or not isinstance(raw, dict):
        return None
    question = _text(raw.get("question"))
    issue, pr = parse_pr(raw.get("issue")), parse_pr(raw.get("pr"))
    if not question or (issue is None) == (pr is None):
        return None
    options = [o.strip() for o in raw.get("options") or [] if isinstance(o, str) and o.strip()][:MAX_OPTIONS]
    answer = raw.get("answer") if isinstance(raw.get("answer"), dict) else {}
    pick = answer.get("pick") if answer.get("pick") in options else ""
    text = _text(answer.get("text"))
    asked_by, asked_at = _text(raw.get("asked_by"), 200), _text(raw.get("asked_at"), 40)
    return {
        "id": did,
        "kind": "issue" if issue else "pr",
        "number": issue or pr,
        "title": _text(raw.get("title"), 256),
        "question": question,
        "options": options,
        "recommended": _text(raw.get("recommended"), 1000),
        "context": _text(raw.get("context")),
        "asked_by": asked_by,
        "asked_at": asked_at,
        "byline": decision_byline(asked_by, asked_at),
        "pick": pick,
        "text": text,
        "answered": bool(pick or text),
        "closed": bool(raw.get("sent_at") or raw.get("dismissed_at")),
    }


def build_decision_queue(files):
    """Open questions (not sent, not dismissed) from (id, parsed JSON) pairs, oldest first."""
    cards = [c for c in (decision_card(did, raw) for did, raw in files) if c and not c["closed"]]
    cards.sort(key=lambda c: (c["asked_at"], c["id"]))
    return {"cards": cards, "counts": {"open": len(cards),
                                       "answered": sum(1 for c in cards if c["answered"])}}


def clean_answer(pick, text, options):
    """The answer to save, {"pick", "text"}, or None for text that is too long or has control
    characters. A pick that isn't one of the options is dropped. Both empty clears the answer."""
    pick = pick if isinstance(pick, str) and pick in (options or []) else ""
    text = text.strip() if isinstance(text, str) else ""
    if len(text) > MAX_ANSWER_CHARS or any(ord(ch) < 32 and ch not in "\n\t" for ch in text):
        return None
    return {"pick": pick, "text": text}


def decision_prompt(cards):
    """One prompt for an agent: the preamble, then each answered question in order."""
    parts = [DECISION_PREAMBLE]
    for n, c in enumerate([c for c in cards if c["answered"]], 1):
        what = "Issue" if c["kind"] == "issue" else "PR"
        head = "## %d. %s #%d" % (n, what, c["number"]) + (": " + c["title"] if c["title"] else "")
        lines = [head, "", "Question: " + c["question"]]
        if c["pick"]:
            lines.append("Picked: " + c["pick"])
        if c["text"]:
            lines.append("Answer: " + c["text"])
        parts.append("\n".join(lines))
    return "\n\n".join(parts) + "\n"


# ---------------------------------------------------------- request checks

def host_ok(host, port):
    """The Host header names this server, so a DNS-rebinding page is refused."""
    return host in ("127.0.0.1:%d" % port, "localhost:%d" % port)


def key_ok(given, key):
    return bool(given) and bool(key) and hmac.compare_digest(str(given), str(key))


def origin_ok(origin, port):
    """A POST from a browser carries Origin. It must be this page's own."""
    return origin is None or origin in ("http://127.0.0.1:%d" % port, "http://localhost:%d" % port)


def parse_pr(value):
    """A PR number from a request, or None. Plain digits only, no signs."""
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value if 0 < value < 10 ** 7 else None
    if isinstance(value, str) and re.fullmatch(r"[1-9][0-9]{0,6}", value):
        return int(value)
    return None


def parse_range(header, size):
    """(start, end) inclusive for a 'bytes=' Range header, None for the whole
    file, or 'bad' when it can't be served (416)."""
    if not header:
        return None
    m = re.fullmatch(r"bytes=(\d*)-(\d*)", header.strip())
    if not m or (m.group(1) == "" and m.group(2) == ""):
        return "bad"
    if m.group(1) == "":
        length = int(m.group(2))
        if length == 0 or size == 0:
            return "bad"
        return (max(0, size - length), size - 1)
    start = int(m.group(1))
    end = int(m.group(2)) if m.group(2) else size - 1
    if start >= size or end < start:
        return "bad"
    return (start, min(end, size - 1))
