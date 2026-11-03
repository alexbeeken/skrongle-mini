#!/usr/bin/env python3
"""Backlog helpers for the backlog-sweep skill (stdlib only, calls `gh api`).

Only REST calls: cloud sessions reach GitHub through a proxy that refuses
GraphQL, which `gh issue list` and `gh pr list` use.

  issues.py candidates [--repo OWNER/NAME]
      One tab-separated line per open issue:
        number  status  labels  prs  updated  title
      updated is the day the issue last changed (a comment, a label, an
      edit), for the "old and stuck" questions of the sweep.
      status is one of
        candidate              nothing says no
        design                 a design, research or RFC issue: the title starts
                               with Design(s), Design proposal(s), Exploration,
                               Research or RFC, or the body says the outcome is an
                               artifact. Built like a candidate, but the deliverable
                               is a claude.ai artifact linked on the issue, no PR
                               (builder-brief.md, "A design issue")
        skip:designed          label "designed": the artifact is on the issue and
                               the user decides what to build from it
        skip:blocked           label "blocked"
        skip:on-hold           label "on hold"
        skip:wontfix           label "wontfix" or "invalid" or "duplicate"
        skip:needs-grooming    label "needs grooming" or "[needs grooming]" in the title
        skip:in-progress       label "in progress", or an OPEN PR claims the issue.
                               An open PR labelled "needs work" counts too: the
                               Review Desk sent it to a fix agent (pr-fix), so the
                               work goes on in that PR and must not be built twice
        check-done:<PR,...>    a MERGED PR claims the issue but it is still open
                               (checked before the label skips: a label says what
                               the issue needed, the merged PR says it got it)
      prs lists the PRs that claim the issue as open:N / merged:N, and
      needs-work:N for an open PR labelled "needs work".
      A PR claims an issue when its title has [N] or its body says
      Closes/Fixes/Resolves #N (case-insensitive, comma lists included).
      "Part of #N" is not a claim.

  issues.py order --tiers FILE [--seed N]
      FILE has one "tier number" pair per line (blank lines and # comments
      ignored). Prints one numbered line per issue: rank, tier, number.
      Issues are sorted by tier; inside a tier the order is a shuffle with
      the seed, so the same seed gives the same order. Without --seed a
      random one is drawn; either way the seed used is printed first as
      "# seed N", so the order can be reproduced.
"""
import argparse
import json
import random
import re
import subprocess
import sys


def gh_rest(repo, path):
    """Every page of a REST list endpoint, flattened into one list."""
    out = subprocess.run(["gh", "api", "--paginate", "--slurp", f"repos/{repo}/{path}"],
                         check=True, capture_output=True, text=True).stdout
    return [item for page in json.loads(out) for item in page]


def claims(pr):
    """Issue numbers a PR claims to finish: [N] in the title, or a Closes/Fixes/
    Resolves line in the body, including comma lists ("Closes #1, #2 and #3").
    GitHub itself only auto-closes the first of such a list, which is how
    PR #530 left 25 issues open."""
    nums = set(int(n) for n in re.findall(r"\[(\d+)\]", pr.get("title") or ""))
    body = pr.get("body") or ""
    for m in re.finditer(r"\b(?:closes|fixes|resolves)\s*:?\s*((?:#\d+[\s,]*(?:and\s*)?)+)", body, re.IGNORECASE):
        nums.update(int(n) for n in re.findall(r"#(\d+)", m.group(1)))
    return nums


def label_names(item):
    return {l["name"].lower() for l in item.get("labels", [])}


# "Design: …", "Designs for …", "Design proposals: …", "exploration: …",
# "Research …", "RFC: …", with or without a leading [tag].
DESIGN_TITLE = re.compile(
    r"^\s*(?:\[[^\]]*\]\s*)*(?:designs?|design proposals?|exploration|research|rfc)\b", re.IGNORECASE)
# "The outcome of this issue is an artifact pitching …"
DESIGN_BODY = re.compile(r"\b(?:outcome|output|deliverable|result)\b[^.\n]{0,80}\bartifact\b", re.IGNORECASE)


def is_design(issue):
    """An issue whose deliverable is a design, research or RFC artifact."""
    return bool(DESIGN_TITLE.search(issue.get("title") or "") or DESIGN_BODY.search(issue.get("body") or ""))


def classify(issues, open_prs, merged_prs):
    """The rows of `candidates`, from the REST shapes of open issues, open PRs
    and merged PRs. No network, so it can be tested on sample data."""
    by_issue = {}
    for state, prs in (("open", open_prs), ("merged", merged_prs)):
        for pr in prs:
            # An open PR the Review Desk sent back for fixing is still open
            # work. Mark it so the report can say why the issue was skipped.
            kind = "needs-work" if state == "open" and "needs work" in label_names(pr) else state
            for n in claims(pr):
                by_issue.setdefault(n, []).append((kind, pr["number"]))

    rows = []
    for it in sorted(issues, key=lambda i: i["number"]):
        n = it["number"]
        labels = [l["name"] for l in it.get("labels", [])]
        lower = {l.lower() for l in labels}
        title = it["title"]
        refs = by_issue.get(n, [])
        needs_work_refs = [p for s, p in refs if s == "needs-work"]
        open_refs = [p for s, p in refs if s == "open"] + needs_work_refs
        merged_refs = [p for s, p in refs if s == "merged"]

        # A merged PR that claims the issue outranks every label: the label
        # says what the issue needed, the merged PR says it got it. An open PR
        # still outranks that (the work continues, e.g. docs updated twice),
        # and so does one labelled "needs work" (a fix agent is on it).
        if "in progress" in lower or open_refs:
            status = "skip:in-progress"
        elif merged_refs:
            status = "check-done:" + ",".join(str(p) for p in sorted(merged_refs))
        elif "blocked" in lower:
            status = "skip:blocked"
        elif "on hold" in lower:
            status = "skip:on-hold"
        elif lower & {"wontfix", "invalid", "duplicate"}:
            status = "skip:wontfix"
        elif "needs grooming" in lower or "[needs grooming]" in title.lower():
            status = "skip:needs-grooming"
        elif "designed" in lower:
            status = "skip:designed"
        elif is_design(it):
            status = "design"
        else:
            status = "candidate"

        prs = " ".join([f"needs-work:{p}" if p in needs_work_refs else f"open:{p}" for p in sorted(open_refs)]
                       + [f"merged:{p}" for p in sorted(merged_refs)])
        rows.append((n, status, ",".join(labels), prs, (it.get("updated_at") or "")[:10], title))
    return rows


def candidates(repo):
    repo = repo or "{owner}/{repo}"  # gh fills these in from the current checkout
    # The issues endpoint lists PRs too; they carry a "pull_request" key.
    issues = [i for i in gh_rest(repo, "issues?state=open&per_page=100") if "pull_request" not in i]
    open_prs = gh_rest(repo, "pulls?state=open&per_page=100")
    merged_prs = [p for p in gh_rest(repo, "pulls?state=closed&per_page=100") if p.get("merged_at")]
    rows = classify(issues, open_prs, merged_prs)

    print("number\tstatus\tlabels\tprs\tupdated\ttitle")
    for r in rows:
        print("\t".join(str(c) for c in r))


def order(tiers_path, seed):
    tiers = {}
    with open(tiers_path) as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            parts = line.split()
            if len(parts) != 2:
                sys.exit(f"bad line in {tiers_path}: {line!r} (want: tier number)")
            tier, num = int(parts[0]), int(parts[1].lstrip("#"))
            if num in tiers:
                sys.exit(f"issue #{num} listed twice in {tiers_path}")
            tiers[num] = tier

    if seed is None:
        seed = random.SystemRandom().randrange(1, 10**9)
    rng = random.Random(seed)
    print(f"# seed {seed}")
    rank = 0
    for tier in sorted(set(tiers.values())):
        nums = sorted(n for n, t in tiers.items() if t == tier)
        rng.shuffle(nums)
        for n in nums:
            rank += 1
            print(f"{rank}\ttier {tier}\t#{n}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("candidates")
    c.add_argument("--repo", default=None)
    o = sub.add_parser("order")
    o.add_argument("--tiers", required=True)
    o.add_argument("--seed", type=int, default=None)
    a = ap.parse_args()
    if a.cmd == "candidates":
        candidates(a.repo)
    else:
        order(a.tiers, a.seed)


if __name__ == "__main__":
    main()
