#!/usr/bin/env python3
"""Where a month of Claude Code usage went, from the local session logs (#1314).

    scripts/claude-usage.py                 # this month
    scripts/claude-usage.py 2026-10         # one month
    scripts/claude-usage.py 2026-10 --top 30

Reads ~/.claude/projects/*/<session>.jsonl and each session's
subagents/*.jsonl. Counts each API message once (by message id) and weights
its tokens by what they cost relative to plain input: input 1, cache write
1.25, cache read 0.1, output 5. The numbers are shares of that weighted
total, not dollars, and they don't price models differently.

Prints the share by model, main session vs subagents, by the factory skill
that started the session, the top sessions, and the factory commands with
an `rm` that waited over 5 minutes for their result (a removal held for a
person's approval; see .claude/skills/rotate/references/factory-rules.md).
Compare a month with the one before by running both.
"""
import collections
import datetime as dt
import glob
import json
import os
import re
import sys

ROOT = os.path.expanduser("~/.claude/projects")
SKILLS = ["rotate", "backlog-sweep", "mergeme", "pr-qa-demo", "qa", "pr-fix"]
CMD = re.compile(r"<command-name>/?([\w:\-]+)</command-name>")
RM = re.compile(r"(^|[;&|\s(])rm\s")


def weight(u):
    return (u.get("input_tokens", 0) + 1.25 * u.get("cache_creation_input_tokens", 0)
            + 0.1 * u.get("cache_read_input_tokens", 0) + 5 * u.get("output_tokens", 0))


def when(ts):
    return dt.datetime.fromisoformat(ts.replace("Z", "+00:00"))


def text_of(content):
    if isinstance(content, str):
        return content
    return " ".join(b.get("text", "") for b in content or [] if isinstance(b, dict) and b.get("type") == "text")


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    month = args[0] if args else dt.date.today().strftime("%Y-%m")
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 15

    by_model, by_side, by_session = collections.Counter(), collections.Counter(), collections.Counter()
    sub_of, label = collections.Counter(), {}
    waits = []
    for f in glob.glob(f"{ROOT}/*/*.jsonl") + glob.glob(f"{ROOT}/*/*/subagents/*.jsonl"):
        sub = "/subagents/" in f
        parts = f[len(ROOT) + 1:].split("/")
        key = (parts[0], parts[1] if sub else parts[1][:-6])
        seen, pending = set(), {}
        for line in open(f, errors="ignore"):
            try:
                r = json.loads(line)
            except ValueError:
                continue
            msg, ts = r.get("message") or {}, r.get("timestamp", "")
            if r.get("type") == "user" and not sub and key not in label:
                for name in CMD.findall(text_of(msg.get("content"))):
                    if name.split(":")[-1] in SKILLS:
                        label[key] = "/" + name.split(":")[-1]
            content = msg.get("content")
            if isinstance(content, list):
                for b in content:
                    if not isinstance(b, dict):
                        continue
                    if b.get("type") == "tool_use" and b.get("name") == "Bash":
                        pending[b["id"]] = (b.get("input", {}).get("command", ""), ts)
                    elif b.get("type") == "tool_result" and b.get("tool_use_id") in pending:
                        cmd, t0 = pending.pop(b["tool_use_id"])
                        if RM.search(cmd) and t0[:7] == month and ts:
                            secs = (when(ts) - when(t0)).total_seconds()
                            if secs > 300:
                                waits.append((secs, key[0], cmd))
            u = msg.get("usage")
            if r.get("type") != "assistant" or not u or not ts.startswith(month) or msg.get("id") in seen:
                continue
            seen.add(msg.get("id"))
            w = weight(u)
            by_model[msg.get("model")] += w
            by_side["subagents" if sub else "main sessions"] += w
            by_session[key] += w
            if sub:
                sub_of[key] += w

    total = sum(by_session.values())
    if not total:
        print(f"no usage found for {month}")
        return
    pct = lambda v: f"{100 * v / total:5.1f}%"
    print(f"Claude Code usage, {month}: {len(by_session)} sessions, {total / 1e6:.0f}M weighted tokens\n")
    print("By model")
    for k, v in by_model.most_common():
        print(f"  {pct(v)}  {k}")
    print("\nWho made the calls")
    for k, v in by_side.most_common():
        print(f"  {pct(v)}  {k}")

    skills, count = collections.Counter(), collections.Counter()
    for k, v in by_session.items():
        skills[label.get(k, "other")] += v
        count[label.get(k, "other")] += 1
    print("\nBy the factory skill that started the session")
    for k, v in skills.most_common():
        print(f"  {pct(v)}  {count[k]:4d} sessions  {v / count[k] / 1e6:5.1f}M each  {k}")

    print(f"\nTop {top} sessions")
    for k, v in by_session.most_common(top):
        print(f"  {pct(v)}  {v / 1e6:5.1f}M  subagents {100 * sub_of[k] / v:3.0f}%  {label.get(k, '-'):15s} {k[0][-50:]}")
    big = sum(v for _, v in by_session.most_common(10))
    print(f"  the top 10 are {pct(big).strip()} of the month")

    waits.sort(reverse=True)
    print(f"\nCommands with an rm that waited over 5 minutes: {len(waits)}")
    for secs, proj, cmd in waits[:top]:
        one_line = re.sub(r"\s+", " ", cmd)[:110]
        print(f"  {secs / 3600:5.1f} h  {proj[-40:]}  {one_line}")


if __name__ == "__main__":
    main()
