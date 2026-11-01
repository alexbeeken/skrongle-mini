#!/usr/bin/env python3
"""Ask the engineer a question on the Review Desk's Decisions mode (#1163).

The factory skills (backlog-sweep, pr-qa-demo, mergeme, qa, pr-fix) run with
nobody watching. A question only the engineer can answer is written here as
one JSON file in ~/skrongle-workflow/decisions/ (REVIEW_DESK_HOME overrides
the folder), and the desk shows it, saves the answer, and turns the answered
questions into one prompt for an agent. The skill writes the question and
goes on with the rest of its work.

  ask.py --issue N | --pr N --question "..." --asked-by <skill>
         [--option A --option B ...] [--recommended A]
         [--context "..." | --context-file FILE] [--slug word-word] [--title "..."]
      Writes decisions/<issue|pr>-N-<slug>.json and prints "wrote <id>".
      The slug comes from --slug, else from the question's first words.
      Prints "open <id>" and writes nothing when a question with the same
      id is still open (not sent, not dismissed): the engineer hasn't
      answered it yet. Prints "dismissed <id>" and writes nothing when the
      engineer dropped that question in the last 30 days: don't ask it
      again. A sent (answered) question frees the id again.
      Exit 0 in all three cases; 2 for a bad question.

  ask.py list [N]
      Every question (on issue or PR N when given), one per line:
      id, state (open|answered|sent|dismissed), asked_at, question.
      Read it before asking about a number, so a question the engineer
      already answered or dropped isn't asked again in other words.
      Exit 2 with a usage line when N isn't a number.

Python standard library only; the file format is checked by rules.py, the
same rules the desk reads with.
"""

import argparse
import json
import os
import re
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import rules  # noqa: E402

DISMISSED_FOR_DAYS = 30
MAX_SLUG = 40


def home():
    return Path(os.environ.get("REVIEW_DESK_HOME") or (Path.home() / "skrongle-workflow")).expanduser()


def decisions_dir():
    return home() / "decisions"


def slugify(text, limit=MAX_SLUG):
    words = re.sub(r"[^a-z0-9]+", "-", (text or "").lower()).strip("-")
    out = ""
    for w in words.split("-"):
        if not w:
            continue
        if out and len(out) + 1 + len(w) > limit:
            break
        out = w if not out else out + "-" + w
    return out[:limit].strip("-")


def now_iso():
    return datetime.now(timezone.utc).astimezone().replace(microsecond=0).isoformat()


def read_all(folder):
    """(id, raw) for every readable decisions file, sorted by id."""
    out = []
    try:
        paths = sorted(folder.glob("*.json"))
    except OSError:
        return out
    for p in paths:
        if not rules.decision_id_ok(p.stem) or not p.is_file():
            continue
        try:
            out.append((p.stem, json.loads(p.read_text(encoding="utf-8"))))
        except (OSError, ValueError):
            continue
    return out


def state_of(raw):
    if raw.get("sent_at"):
        return "sent"
    if raw.get("dismissed_at"):
        return "dismissed"
    answer = raw.get("answer") if isinstance(raw.get("answer"), dict) else {}
    if answer.get("pick") or answer.get("text"):
        return "answered"
    return "open"


def parse_when(value):
    try:
        return datetime.fromisoformat(str(value))
    except (TypeError, ValueError):
        return None


def recently_dismissed(raw, now=None):
    when = parse_when(raw.get("dismissed_at"))
    if when is None:
        return False
    now = now or datetime.now(timezone.utc).astimezone()
    if when.tzinfo is None:
        when = when.replace(tzinfo=now.tzinfo)
    return now - when < timedelta(days=DISMISSED_FOR_DAYS)


def build(args):
    """The file's contents from the command line, or (None, why)."""
    kind = "issue" if args.issue else "pr"
    number = args.issue or args.pr
    raw = {kind: number, "question": (args.question or "").strip()}
    if args.title:
        raw["title"] = args.title.strip()
    options = [o.strip() for o in args.option or [] if o and o.strip()]
    if options:
        raw["options"] = options[:rules.MAX_OPTIONS]
    if args.recommended:
        raw["recommended"] = args.recommended.strip()
    context = args.context
    if args.context_file:
        try:
            context = Path(args.context_file).read_text(encoding="utf-8")
        except OSError as e:
            return None, "could not read --context-file: %s" % e
    if context and context.strip():
        raw["context"] = context.strip()
    raw["asked_by"] = args.asked_by.strip()
    raw["asked_at"] = now_iso()
    slug = slugify(args.slug or args.question)
    if not slug:
        return None, "the question needs some letters or digits for its id"
    did = "%s-%d-%s" % (kind, number, slug)
    if rules.decision_card(did, raw) is None:
        return None, "the desk would not show this question (an empty question, or a bad id)"
    if args.recommended and options and args.recommended.strip() not in options:
        return None, "--recommended must be one of the --option values"
    return (did, raw), None


def ask(args, out=None):
    out = out or sys.stdout
    built, why = build(args)
    if why:
        print("ask.py: " + why, file=sys.stderr)
        return 2
    did, raw = built
    folder = decisions_dir()
    try:
        folder.mkdir(parents=True, exist_ok=True)
    except OSError as e:
        print("ask.py: cannot make %s: %s" % (folder, e), file=sys.stderr)
        return 1
    path = folder / (did + ".json")
    if path.exists():
        try:
            old = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            old = {}
        st = state_of(old)
        if st in ("open", "answered"):
            print("open %s" % did, file=out)
            return 0
        if st == "dismissed" and recently_dismissed(old):
            print("dismissed %s" % did, file=out)
            return 0
        # Sent, or dismissed long ago: keep the record under a dated name.
        stamp = re.sub(r"[^0-9]", "", str(old.get("sent_at") or old.get("dismissed_at") or ""))[:12] or "old"
        path.rename(folder / ("%s-%s.json" % (did, stamp)))
    tmp = path.with_name("." + path.name + ".tmp")
    tmp.write_text(json.dumps(raw, indent=2) + "\n", encoding="utf-8")
    os.replace(tmp, path)
    print("wrote %s" % did, file=out)
    return 0


def list_questions(number, out=None):
    out = out or sys.stdout
    for did, raw in read_all(decisions_dir()):
        card = rules.decision_card(did, raw)
        if card is None:
            continue
        if number and card["number"] != number:
            continue
        q = card["question"].replace("\n", " ")
        print("%s\t%s\t%s\t%s" % (did, state_of(raw), card["asked_at"] or "-", q), file=out)
    return 0


def parser():
    p = argparse.ArgumentParser(description="Write a question for the Review Desk's Decisions mode.")
    sub = p.add_subparsers(dest="cmd")
    a = sub.add_parser("ask", help="write one question (the default)")
    add_ask_args(a)
    l = sub.add_parser("list", help="list the questions, optionally for one number")
    l.add_argument("number", nargs="?", type=int)
    return p


def add_ask_args(a):
    g = a.add_mutually_exclusive_group(required=True)
    g.add_argument("--issue", type=int)
    g.add_argument("--pr", type=int)
    a.add_argument("--question", required=True)
    a.add_argument("--asked-by", required=True, help="the skill or session asking, e.g. backlog-sweep")
    a.add_argument("--option", action="append")
    a.add_argument("--recommended")
    a.add_argument("--context")
    a.add_argument("--context-file")
    a.add_argument("--slug")
    a.add_argument("--title")


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] == "list":
        if len(argv) > 2 or (len(argv) == 2 and not re.fullmatch(r"[1-9][0-9]*", argv[1])):
            print("usage: ask.py list [N]  (N is an issue or PR number)", file=sys.stderr)
            return 2
        return list_questions(int(argv[1]) if len(argv) == 2 else None)
    if not argv or argv[0] != "ask":
        argv = ["ask"] + argv
    args = parser().parse_args(argv)
    if (args.issue or 0) <= 0 and (args.pr or 0) <= 0:
        print("ask.py: --issue or --pr needs a positive number", file=sys.stderr)
        return 2
    return ask(args)


if __name__ == "__main__":
    sys.exit(main())
