"""Unit tests for the Review Desk. No network, no GitHub.

    cd scripts/review-desk && python3 -I -B -m unittest -v
"""

import json
import os
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import rules  # noqa: E402

GREEN = [{"__typename": "CheckRun", "name": "cargo test", "workflowName": "checks",
          "status": "COMPLETED", "conclusion": "SUCCESS", "startedAt": "2026-10-28T10:00:00Z"}]


def qa(n, verdict="merge", at="abcdef12", created="2026-10-28T12:00:00Z"):
    body = "## QA: PR #%d Title\n\n**Verdict: %s.** Fine.\n\n" % (n, verdict)
    if at:
        body += "**Checked at** `%s`.\n\n" % at
    body += "### Checklist\n1. ok\n\n### Jev\nfine\n"
    return {"body": body, "createdAt": created, "url": "https://github.com/x/y/pull/%d#c" % n}


def pr(n, created="2026-10-28T08:00:00Z", labels=(), draft=False, comments=None, ci=None,
       mergeable="MERGEABLE", head="abcdef12" + "0" * 32):
    return {
        "number": n, "title": "[1] PR %d" % n, "createdAt": created, "isDraft": draft, "state": "OPEN",
        "labels": [{"name": l} for l in labels], "comments": [qa(n)] if comments is None else comments,
        "statusCheckRollup": GREEN if ci is None else ci, "mergeable": mergeable,
        "headRefOid": head, "baseRefName": "main",
        "body": "## What this PR is for\n\n- **Shows** the unit name.\n- Second.\n",
    }


class QueueRules(unittest.TestCase):
    def test_filtering(self):
        prs = [
            pr(1),
            pr(2, draft=True),
            pr(3, labels=["mergeme"]),
            pr(4, labels=["needs work"]),
            pr(5, comments=[]),
            pr(6, comments=[{"body": "## QA: PR #66 other", "createdAt": "x"}]),
            pr(7, comments=[{"body": "nice work"}]),
        ]
        q = rules.build_queue(prs, {})
        self.assertEqual([c["number"] for c in q["cards"]], [1])
        self.assertEqual(q["counts"]["being_fixed"], 1)
        self.assertEqual(q["counts"]["waiting_for_qa"], 3)  # 5, 6 and 7; not the draft

    def test_normal_card(self):
        c = rules.card(pr(1), stale=False)
        self.assertFalse(c["red"])
        self.assertEqual(c["reasons"], [])
        self.assertEqual(c["qa"], "merge")
        self.assertEqual(c["checked_at"], "abcdef12")
        self.assertEqual(c["why"], "Shows the unit name.")
        self.assertEqual(c["checklist"], "1. ok")

    def test_red_reasons(self):
        failed = GREEN + [{"__typename": "CheckRun", "name": "e2e smoke 3", "workflowName": "smoke",
                           "status": "COMPLETED", "conclusion": "FAILURE", "startedAt": "2026-10-28T10:00:00Z"}]
        self.assertEqual(rules.card(pr(1, ci=failed), False)["reasons"], ["CI red: e2e smoke 3"])
        running = [dict(GREEN[0], status="IN_PROGRESS", conclusion="")]
        self.assertEqual(rules.card(pr(1, ci=running), False)["reasons"], ["CI still running"])
        self.assertEqual(rules.card(pr(1, comments=[qa(1, "not ready")]), False)["reasons"], ["QA says not ready"])
        self.assertEqual(rules.card(pr(1, mergeable="CONFLICTING"), False)["reasons"], ["Conflicts with main"])
        self.assertEqual(rules.card(pr(1), True)["reasons"], ["Changed since QA"])
        self.assertFalse(rules.card(pr(1, mergeable="UNKNOWN"), False)["red"])

    def test_rerun_counts_by_newest_run(self):
        old_fail = dict(GREEN[0], conclusion="FAILURE", startedAt="2026-10-28T09:00:00Z")
        self.assertEqual(rules.ci_state([old_fail] + GREEN)["state"], "green")
        self.assertEqual(rules.ci_state([])["state"], "none")

    def test_order_normal_oldest_first_then_red(self):
        prs = [
            pr(10, created="2026-10-28T09:00:00Z"),
            pr(11, created="2026-10-27T09:00:00Z", mergeable="CONFLICTING"),
            pr(12, created="2026-10-26T09:00:00Z"),
            pr(13, created="2026-10-25T09:00:00Z"),
        ]
        q = rules.build_queue(prs, {13: True})
        self.assertEqual([c["number"] for c in q["cards"]], [12, 10, 13, 11])
        self.assertEqual(q["counts"]["red"], 2)

    def test_verdict_forms(self):
        for line, want in [
            ("**Verdict: merge.** ok", "merge"),
            ("**Verdict:** merge after CI is green on `af9d0b73`.", "merge"),
            ("**Verdict: merge** (once CI is green)", "merge"),
            ("**Verdict: not ready.** CI red", "not ready"),
            ("**Verdict:** not ready", "not ready"),
            ("7. The verdict was \"Victoria\"", None),
        ]:
            self.assertEqual(rules.verdict(line), want, line)

    def test_qa_title_needs_the_number_to_end(self):
        self.assertIsNone(rules.latest_qa([{"body": "## QA: PR #99 x"}], 9))
        self.assertIsNotNone(rules.latest_qa([{"body": "## QA: PR #9 x"}], 9))

    def test_qa_page_url(self):
        body = "<!-- qa-page -->\n**QA:** [open](https://claude.ai/artifact/AbC#pr-890)\n"
        self.assertEqual(rules.qa_page_url(body), "https://claude.ai/artifact/AbC#pr-890")
        self.assertIsNone(rules.qa_page_url("[x](https://claude.ai/artifact/AbC)"))


def c(sha, *parents, date="2026-10-28T10:00:00Z"):
    return {"sha": sha, "parents": list(parents), "date": date}


class StaleRule(unittest.TestCase):
    HEAD = "c" * 40

    def test_head_is_checked_at(self):
        self.assertFalse(rules.is_stale("abcdef12" + "0" * 32, "abcdef12", None, None))
        self.assertFalse(rules.needs_commit_list("abcdef12" + "0" * 32, "abcdef12"))

    def test_own_commit_after_qa_is_stale(self):
        own = [c("a" * 40, "m1"), c(self.HEAD, "a" * 40)]
        self.assertTrue(rules.is_stale(self.HEAD, "aaaaaaaa", None, own))

    def test_main_merge_after_qa_is_not_stale(self):
        own = [c("a" * 40, "m1"), c(self.HEAD, "a" * 40, "m2")]
        self.assertFalse(rules.is_stale(self.HEAD, "aaaaaaaa", None, own))

    def test_main_merge_then_own_commit_is_stale(self):
        own = [c("a" * 40, "m1"), c("b" * 40, "a" * 40, "m2"), c(self.HEAD, "b" * 40)]
        self.assertTrue(rules.is_stale(self.HEAD, "aaaaaaaa", None, own))

    def test_merging_another_pr_branch_is_stale(self):
        # The second parent is one of the PR's own commits, not main.
        own = [c("a" * 40, "m1"), c("d" * 40, "m1"), c(self.HEAD, "a" * 40, "d" * 40)]
        self.assertTrue(rules.is_stale(self.HEAD, "aaaaaaaa", None, own))

    def test_checked_at_gone_or_no_compare_is_stale(self):
        self.assertTrue(rules.is_stale(self.HEAD, "12345678", None, [c(self.HEAD, "m1")]))
        self.assertTrue(rules.is_stale(self.HEAD, "aaaaaaaa", None, None))

    def test_old_report_uses_dates(self):
        own = [c("a" * 40, "m1", date="2026-10-27T10:00:00Z"),
               c(self.HEAD, "a" * 40, "m2", date="2026-10-28T10:00:00Z")]
        self.assertFalse(rules.is_stale(self.HEAD, None, "2026-10-28T00:00:00Z", own))
        own.append(c("e" * 40, self.HEAD, date="2026-10-28T11:00:00Z"))
        self.assertTrue(rules.is_stale("e" * 40, None, "2026-10-28T00:00:00Z", own))


class DemoFiles(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "demos"
        (self.root / "pr-5").mkdir(parents=True)
        (self.root / "pr-5" / "01-after.png").write_bytes(b"png")
        (self.root / "pr-5" / "notes.txt").write_text("x")
        (self.root / "issue-1-pr-5-old.mp4").write_bytes(b"mp4")
        (self.root / "issue-1-pr-6-other.png").write_bytes(b"png")
        (Path(self.tmp.name) / "secret.png").write_bytes(b"no")
        os.symlink(Path(self.tmp.name) / "secret.png", self.root / "pr-5" / "link.png")

    def tearDown(self):
        self.tmp.cleanup()

    def test_serves_only_inside_the_folders(self):
        self.assertIsNotNone(rules.demo_file(self.root, 5, "01-after.png"))
        self.assertIsNotNone(rules.demo_file(self.root, 5, "issue-1-pr-5-old.mp4"))
        for bad in ["../secret.png", "..", "pr-5/01-after.png", "%2e%2e/secret.png", ".hidden.png",
                    "notes.txt", "link.png", "issue-1-pr-6-other.png", "", "/etc/passwd"]:
            self.assertIsNone(rules.demo_file(self.root, 5, bad), bad)
        self.assertIsNone(rules.demo_file(self.root, -5, "01-after.png"))
        self.assertIsNone(rules.demo_file(self.root, "5", "01-after.png"))

    def test_find_demo_fallbacks(self):
        man = {"pr": 5, "why": "W", "items": [{"caption": "c", "before": "../x.png", "after": "01-after.png"}],
               "video": "demo.mp4"}
        d = rules.find_demo(self.root, 5, man, [], None)
        self.assertEqual(d["kind"], "manifest")
        self.assertIsNone(d["items"][0]["before"])
        self.assertEqual(d["items"][0]["after"], "/api/demo/5/01-after.png")
        d = rules.find_demo(self.root, 5, None, ["issue-1-pr-5-old.mp4", "issue-1-pr-55-x.png"], None)
        self.assertEqual((d["kind"], len(d["videos"]), d["items"]), ("legacy", 1, []))
        self.assertEqual(rules.find_demo(self.root, 5, None, [], "https://claude.ai/a")["kind"], "qa-page")
        self.assertEqual(rules.find_demo(self.root, 5, None, [], None)["kind"], "none")

    def test_range(self):
        self.assertIsNone(rules.parse_range(None, 100))
        self.assertEqual(rules.parse_range("bytes=0-", 100), (0, 99))
        self.assertEqual(rules.parse_range("bytes=10-19", 100), (10, 19))
        self.assertEqual(rules.parse_range("bytes=90-500", 100), (90, 99))
        self.assertEqual(rules.parse_range("bytes=-10", 100), (90, 99))
        for bad in ["bytes=100-", "bytes=5-2", "bytes=-", "items=0-1", "bytes=0-1,5-6"]:
            self.assertEqual(rules.parse_range(bad, 100), "bad", bad)


class RequestChecks(unittest.TestCase):
    def test_key(self):
        self.assertTrue(rules.key_ok("abc", "abc"))
        for given in ["", None, "abd", "abc "]:
            self.assertFalse(rules.key_ok(given, "abc"), given)
        self.assertFalse(rules.key_ok("", ""))

    def test_host_and_origin(self):
        self.assertTrue(rules.host_ok("127.0.0.1:8123", 8123))
        self.assertTrue(rules.host_ok("localhost:8123", 8123))
        for host in [None, "evil.example:8123", "127.0.0.1:9999", "127.0.0.1"]:
            self.assertFalse(rules.host_ok(host, 8123), host)
        self.assertTrue(rules.origin_ok(None, 8123))
        self.assertFalse(rules.origin_ok("https://evil.example", 8123))

    def test_pr_numbers(self):
        self.assertEqual(rules.parse_pr(1103), 1103)
        self.assertEqual(rules.parse_pr("1103"), 1103)
        for bad in ["1103; rm -rf", "-1", "0", "01", 0, -3, True, 1.5, None, "1e3", " 12"]:
            self.assertIsNone(rules.parse_pr(bad), bad)


class Commands(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["REVIEW_DESK_HOME"] = self.tmp.name
        import server
        self.server = server

    def tearDown(self):
        self.tmp.cleanup()

    def test_argv_is_filled_and_checked(self):
        argv = self.server.build_argv("label-add", {"pr": 12, "repo": "a/b", "label": "mergeme"})
        self.assertEqual(argv, ["gh", "pr", "edit", "12", "--repo", "a/b", "--add-label", "mergeme"])
        for params in [{"pr": "12;x", "repo": "a/b", "label": "mergeme"},
                       {"pr": 12, "repo": "a/b", "label": "bug"},
                       {"pr": 12, "repo": "a b/c", "label": "mergeme"},
                       {"pr": 12, "label": "mergeme"}]:
            with self.assertRaises(self.server.CommandError):
                self.server.build_argv("label-add", params)

    def test_dry_run_logs_instead_of_running(self):
        self.server.DRY_RUN = True
        try:
            out = self.server.run("label-add", pr=12, repo="a/b", label="mergeme")
        finally:
            self.server.DRY_RUN = False
        self.assertEqual(out, "")
        log = (Path(self.tmp.name) / "log" / "actions.log").read_text()
        self.assertIn("DRY RUN, not run: gh pr edit 12 --repo a/b --add-label mergeme", log)

    def test_config_created_with_defaults(self):
        cfg_path = Path(self.tmp.name) / "config.json"
        cfg_path.write_text(json.dumps({"repo": "a/b"}))
        cfg = self.server.load_config()
        self.assertEqual(cfg["repo"], "a/b")
        self.assertEqual(cfg["fix_agent_limit"], 3)
        self.assertEqual(json.loads(cfg_path.read_text())["demos"], "~/skrongle-demos")

    def test_repo_path_is_the_workspace_it_runs_from(self):
        cfg_path = Path(self.tmp.name) / "config.json"
        cfg_path.write_text(json.dumps({"repo": "a/b"}))
        cfg = self.server.load_config()
        self.assertEqual(cfg["repo_path"], str(self.server.REPO_ROOT))
        self.assertNotIn("repo_path", json.loads(cfg_path.read_text()))


class NeedsWork(unittest.TestCase):
    def test_comment_is_the_shared_format(self):
        self.assertEqual(rules.needs_work_comment("Wrap the unit name."),
                         "## Needs work\n\nWrap the unit name.\n\n<!-- review-desk:needs-work -->\n")

    def test_text_is_cleaned(self):
        self.assertEqual(rules.clean_fix_text("  a b\r\nc <!-- review-desk:needs-work --> "), "a b\nc")
        self.assertIsNone(rules.clean_fix_text(None))
        self.assertIsNone(rules.clean_fix_text(["a b c"]))

    def test_three_words(self):
        self.assertTrue(rules.enough_words("fix the wrap"))
        for bad in ["", "fix", "fix it", "  fix \n it  ", None, "x " * 5000]:
            self.assertFalse(rules.enough_words(bad), bad)

    def test_fix_limit(self):
        self.assertFalse(rules.at_fix_limit(2, 3))
        self.assertTrue(rules.at_fix_limit(3, 3))
        self.assertTrue(rules.at_fix_limit(4, 3))
        self.assertFalse(rules.at_fix_limit(0, "x"))

    def test_workspaces_counted(self):
        base = "/Users/a/orca/workspaces/skrongle-mini/"
        rows = [
            {"path": base + "fix-pr-1103", "branch": "refs/heads/alexbeeken/fix-pr-1103",
             "displayName": "[1100] Some PR title"},
            {"path": base + "1104-other", "branch": "refs/heads/x", "displayName": "Fix pr 1104"},
            {"path": base + "fix-pr-1105-2", "branch": "", "displayName": ""},
            {"path": base + "fix-pr-1106", "isArchived": True},
            {"path": base + "mergeme-2026-10-28", "branch": "refs/heads/main", "displayName": "Mergeme 2026 10 28"},
            {"path": base + "tarpon", "branch": "refs/heads/alexbeeken/mergeme-batch-2026-10",
             "displayName": "Mergeme batch 2026 10"},
            {"path": base + "cockle", "branch": "refs/heads/alexbeeken/mergeme-land-labelled-prs"},
            {"path": base + "fix-pr-later", "displayName": "fix pr later"},
            "not a row",
        ]
        ws = rules.workspaces(rows)
        self.assertEqual(ws["fix"], ["fix-pr-1103", "1104-other", "fix-pr-1105-2"])
        self.assertEqual(ws["mergeme"], ["mergeme-2026-10-28"])


class OrcaCommands(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["REVIEW_DESK_HOME"] = self.tmp.name
        import server
        self.server = server

    def tearDown(self):
        self.tmp.cleanup()

    def test_fix_workspace_command(self):
        argv = self.server.build_argv("orca-create-fix", {"workspace": "fix-pr-12", "path": "/r", "pr": 12,
                                                          "model": "opus", "effort": "high"})
        self.assertEqual(argv, ["orca", "worktree", "create", "--name", "fix-pr-12", "--repo", "path:/r",
                                "--pr", "12", "--agent", "claude", "--prompt", "/pr-fix 12",
                                "--model", "opus", "--effort", "high", "--json"])

    def test_mergeme_workspace_command(self):
        argv = self.server.build_argv("orca-create-mergeme", {"workspace": "mergeme-2026-10-28", "path": "/r"})
        self.assertEqual(argv[:9], ["orca", "worktree", "create", "--name", "mergeme-2026-10-28",
                                    "--repo", "path:/r", "--base-branch", "main"])
        self.assertEqual(argv[-5:], ["--agent", "claude", "--prompt", "/mergeme", "--json"])

    def test_new_params_are_checked(self):
        good = {"workspace": "fix-pr-12", "path": "/r", "pr": 12, "model": "opus", "effort": "high"}
        self.server.build_argv("orca-create-fix", dict(good, model="claude-opus-5-5"))
        for key, bad in [("workspace", "fix-pr-12; rm"), ("workspace", "fix-pr-012"), ("workspace", "mergeme-today"),
                         ("workspace", "../fix-pr-1"), ("model", "opus --dangerously"), ("model", ""),
                         ("model", None), ("effort", "extreme"), ("path", "relative/path")]:
            with self.assertRaises(self.server.CommandError, msg="%s=%r" % (key, bad)):
                self.server.build_argv("orca-create-fix", dict(good, **{key: bad}))
        with self.assertRaises(self.server.CommandError):
            self.server.build_argv("orca-create-mergeme", {"workspace": "fix-pr-1x", "path": "/r"})

    def test_comment_goes_on_stdin_not_argv(self):
        argv = self.server.build_argv("pr-comment", {"pr": 12, "repo": "a/b"})
        self.assertEqual(argv[-2:], ["--body-file", "-"])
        self.server.DRY_RUN = True
        try:
            self.server.run("pr-comment", pr=12, repo="a/b", stdin=rules.needs_work_comment("fix the wrap"))
        finally:
            self.server.DRY_RUN = False
        log = (Path(self.tmp.name) / "log" / "actions.log").read_text()
        self.assertIn("gh pr comment 12 --repo a/b --body-file -", log)
        self.assertIn("    fix the wrap", log)

    def test_needs_work_refuses_short_text_before_any_command(self):
        desk = self.server.Desk({"repo": "a/b", "repo_path": "/r"})
        r = self.server.act_needs_work(desk, 12, {"text": "fix it"})
        self.assertFalse(r["ok"])
        self.assertIn("at least 3 words", r["message"])
        self.assertFalse((Path(self.tmp.name) / "log" / "actions.log").exists())

    def test_run_mergeme_needs_a_yes(self):
        desk = self.server.Desk({"repo": "a/b", "repo_path": "/r"})
        self.assertFalse(self.server.act_run_mergeme(desk, None, {})["ok"])


def issue(n, title, labels=(), created="2026-10-20T08:00:00Z", state="OPEN"):
    return {"number": n, "title": title, "url": "https://github.com/x/y/issues/%d" % n, "state": state,
            "labels": [{"name": l} for l in labels], "createdAt": created,
            "author": {"login": "alexbeeken"}, "body": "What is wrong."}


class IssuesMode(unittest.TestCase):
    def test_which_issues_show(self):
        issues = [
            issue(1, "[qa] Level cards say 1 lessons", created="2026-10-21T00:00:00Z"),
            issue(2, "[needs grooming] Placement quiz", created="2026-10-19T00:00:00Z"),
            issue(3, "[QA][Needs Grooming] Both, any case"),
            issue(4, "Plain title", labels=["needs grooming"]),
            issue(5, "Plain title, no label", labels=["bug"]),
            issue(6, "[qa] closed one", state="CLOSED"),
            issue(7, "qa without brackets, needs grooming in words"),
        ]
        q = rules.build_issue_queue(issues)
        self.assertEqual([c["number"] for c in q["cards"]], [2, 3, 4, 1])  # oldest first
        self.assertEqual(q["counts"]["queue"], 4)
        c = q["cards"][1]
        self.assertEqual(c["tags"], ["[QA]", "[Needs Grooming]"])
        self.assertEqual(c["author"], "alexbeeken")
        self.assertEqual(q["cards"][2]["labels"], ["needs grooming"])

    def test_strip_tags(self):
        self.assertEqual(rules.strip_tags("[qa] [needs grooming] Level cards  say"), "Level cards say")
        self.assertEqual(rules.strip_tags("Sprint [QA] mode"), "Sprint mode")
        self.assertEqual(rules.strip_tags("No tags"), "No tags")

    def test_clean_title(self):
        self.assertEqual(rules.clean_title("  Level cards say 1 lessons "), "Level cards say 1 lessons")
        for bad in ["", "   ", None, 12, "two\nlines", "tab\there", "x" * 257]:
            self.assertIsNone(rules.clean_title(bad), repr(bad))

    def test_clean_body(self):
        self.assertEqual(rules.clean_body("Steps:\r\n- open\r\n\t- see\n\n"), "Steps:\n- open\n\t- see")
        self.assertEqual(rules.clean_body("   "), "")  # clears the description
        self.assertEqual(rules.clean_body("x" * 65536), "x" * 65536)
        for bad in [None, 12, ["a"], "bell\x07here", "x" * 65537, "del\x7f"]:
            self.assertIsNone(rules.clean_body(bad), repr(bad))


class IssueCommands(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["REVIEW_DESK_HOME"] = self.tmp.name
        import server
        self.server = server

    def tearDown(self):
        self.tmp.cleanup()

    def test_title_is_one_argument_and_body_goes_on_stdin(self):
        argv = self.server.build_argv("issue-edit", {"issue": 12, "repo": "a/b", "title": "-x Level cards"})
        self.assertEqual(argv, ["gh", "issue", "edit", "12", "--repo", "a/b", "--title=-x Level cards",
                                "--body-file", "-"])
        argv = self.server.build_argv("issue-edit", {"issue": 12, "repo": "a/b", "title": "Uses {repo} text"})
        self.assertEqual(argv[-3], "--title=Uses {repo} text")
        for params in [{"issue": "12;x", "repo": "a/b", "title": "ok"},
                       {"issue": 12, "repo": "a/b", "title": " padded "},
                       {"issue": 12, "repo": "a/b", "title": "two\nlines"},
                       {"issue": 12, "repo": "a/b"}]:
            with self.assertRaises(self.server.CommandError):
                self.server.build_argv("issue-edit", params)
        self.server.DRY_RUN = True
        try:
            self.server.run("issue-edit", issue=12, repo="a/b", title="Level cards", stdin="Steps:\n- open the page")
        finally:
            self.server.DRY_RUN = False
        log = (Path(self.tmp.name) / "log" / "actions.log").read_text()
        self.assertIn("gh issue edit 12 --repo a/b '--title=Level cards' --body-file -", log)
        self.assertIn("    - open the page", log)

    def test_bad_title_or_body_refused_before_any_command(self):
        desk = self.server.Desk({"repo": "a/b", "repo_path": "/r"})
        for body in [{"title": "  "}, {"body": "bell\x07"}, {"title": "Fine", "body": "x" * 65537}, {}]:
            r = self.server.act_issue_edit(desk, 12, body)
            self.assertFalse(r["ok"], body)
        self.assertFalse((Path(self.tmp.name) / "log" / "actions.log").exists())

    def test_lists_expire_and_a_closed_issue_leaves_at_once(self):
        desk = self.server.Desk({"repo": "a/b", "repo_path": "/r"})
        calls = []

        def fake_run(name, stdin=None, timeout=60, **params):
            calls.append(name)
            if name == "issue-list":
                return json.dumps([issue(1, "[qa] One"), issue(2, "[qa] Two")])
            if name == "issue-state":
                return json.dumps({"state": "OPEN", "title": "[qa] One", "body": ""})
            return ""
        real = self.server.run
        self.server.run = fake_run
        try:
            self.assertEqual([c["number"] for c in desk.load_issues()["cards"]], [1, 2])
            desk.load_issues()
            self.assertEqual(calls.count("issue-list"), 1)  # fresh: served from the cache
            self.assertTrue(self.server.act_issue_close(desk, 1, {})["ok"])
            self.assertEqual([c["number"] for c in desk.load_issues()["cards"]], [2])  # gone without a fetch
            self.assertEqual(calls.count("issue-list"), 1)
            desk.issues_at -= self.server.LIST_TTL + 1
            desk.load_issues()
            self.assertEqual(calls.count("issue-list"), 2)  # stale: fetched again
        finally:
            self.server.run = real

    def test_close_and_reopen(self):
        desk = self.server.Desk({"repo": "a/b", "repo_path": "/r"})
        ran = []
        state = {"state": "OPEN"}

        def fake_run(name, stdin=None, timeout=60, **params):
            ran.append(name)
            return json.dumps(dict(state, title="[qa] Old title", body="")) if name == "issue-state" else ""
        real = self.server.run
        self.server.run = fake_run
        try:
            self.assertTrue(self.server.act_issue_close(desk, 12, {})["ok"])
            self.assertEqual(ran, ["issue-state", "issue-close"])
            self.assertTrue(self.server.act_issue_reopen(desk, 12, {}).get("unchanged"))  # still open: nothing to do
            state["state"] = "CLOSED"
            r = self.server.act_issue_close(desk, 12, {})
            self.assertFalse(r["ok"]); self.assertTrue(r["skipped"])
            ran.clear()
            self.assertTrue(self.server.act_issue_reopen(desk, 12, {})["ok"])
            self.assertEqual(ran, ["issue-state", "issue-reopen"])
        finally:
            self.server.run = real
        self.assertEqual(self.server.build_argv("issue-close", {"issue": 12, "repo": "a/b"}),
                         ["gh", "issue", "close", "12", "--repo", "a/b"])
        self.assertEqual(self.server.build_argv("issue-reopen", {"issue": 12, "repo": "a/b"}),
                         ["gh", "issue", "reopen", "12", "--repo", "a/b"])

    def test_edit_sends_only_what_changed_and_returns_the_old_values(self):
        desk = self.server.Desk({"repo": "a/b", "repo_path": "/r"})
        ran = []

        def fake_run(name, stdin=None, timeout=60, **params):
            ran.append((name, stdin, params))
            if name == "issue-state":
                return json.dumps({"state": "OPEN", "title": "[qa] Old title", "body": "Old text.\r\n"})
            return ""
        real = self.server.run
        self.server.run = fake_run
        try:
            r = self.server.act_issue_edit(desk, 12, {"body": "New text."})
            self.assertTrue(r["ok"])
            self.assertEqual(r["changed"], ["description"])
            self.assertEqual((r["old_title"], r["old_body"]), ("[qa] Old title", "Old text.\r\n"))
            self.assertEqual(ran[-1], ("issue-edit", "New text.", {"issue": 12, "repo": "a/b", "title": "[qa] Old title"}))
            r = self.server.act_issue_edit(desk, 12, {"title": "Old title", "body": "Old text."})
            self.assertEqual(r["changed"], ["title"])  # a trailing newline is not a change
            r = self.server.act_issue_edit(desk, 12, {"title": "Old title"})
            self.assertEqual(r["changed"], ["title"])  # a title-only save leaves the description alone
            self.assertEqual(ran[-1][1], "Old text.")
            r = self.server.act_issue_edit(desk, 12, {"title": "Old title", "body": "Old text. More."})
            self.assertEqual(r["changed"], ["title", "description"])
            self.assertIn("title and description", r["message"])
            r = self.server.act_issue_edit(desk, 12, {"title": "[qa] Old title", "body": "Old text.\n\n"})
            self.assertTrue(r.get("unchanged"))
            ran.clear()
            self.server.run = lambda name, stdin=None, timeout=60, **p: json.dumps({"state": "CLOSED"})
            r = self.server.act_issue_edit(desk, 12, {"title": "Anything"})
            self.assertTrue(r["skipped"])
        finally:
            self.server.run = real


def question(n=1163, kind="issue", **more):
    return {kind: n, "question": "Where do the questions live?", "title": "Decision mode",
            "asked_by": "backlog-sweep", "asked_at": "2026-10-29T08:00:00Z", **more}


class DecisionsMode(unittest.TestCase):
    def test_which_files_are_questions(self):
        files = [
            ("issue-1163-a", question()),
            ("pr-1160-b", question(1160, "pr", asked_at="2026-10-28T08:00:00Z")),
            ("no-number", {"question": "Why?"}),
            ("both-numbers", question(pr=5)),
            ("no-question", {"issue": 4}),
            ("sent", question(sent_at="2026-10-29T09:00:00Z")),
            ("dropped", question(dismissed_at="2026-10-29T09:00:00Z")),
            ("../escape", question()),
            ("list", ["not", "a", "question"]),
        ]
        q = rules.build_decision_queue(files)
        self.assertEqual([c["id"] for c in q["cards"]], ["pr-1160-b", "issue-1163-a"])  # oldest first
        self.assertEqual(q["cards"][0]["kind"], "pr")
        self.assertEqual(q["counts"], {"open": 2, "answered": 0})

    def test_saved_answer_is_read_back(self):
        raw = question(options=["One file per question", "One shared file"],
                       answer={"pick": "One shared file", "text": "Keep it simple."})
        c = rules.decision_card("x", raw)
        self.assertTrue(c["answered"])
        self.assertEqual((c["pick"], c["text"]), ("One shared file", "Keep it simple."))
        raw["answer"]["pick"] = "Not an option"
        self.assertEqual(rules.decision_card("x", raw)["pick"], "")

    def test_clean_answer(self):
        opts = ["A", "B"]
        self.assertEqual(rules.clean_answer("B", "  why  ", opts), {"pick": "B", "text": "why"})
        self.assertEqual(rules.clean_answer("C", "", opts), {"pick": "", "text": ""})
        self.assertEqual(rules.clean_answer(None, "two\nlines", opts)["text"], "two\nlines")
        self.assertIsNone(rules.clean_answer("A", "bell\x07", opts))
        self.assertIsNone(rules.clean_answer("A", "x" * (rules.MAX_ANSWER_CHARS + 1), opts))

    def test_byline_shows_the_date_once(self):
        # #1206: ask.py used to write "backlog-sweep 2026-10-29" and the chip added the date again.
        self.assertEqual(rules.decision_card("x", question())["byline"], "backlog-sweep, 2026-10-29")
        old = question(asked_by="backlog-sweep 2026-10-29")
        self.assertEqual(rules.decision_card("x", old)["byline"], "backlog-sweep 2026-10-29")
        self.assertEqual(rules.decision_card("x", question(asked_at=""))["byline"], "backlog-sweep")
        self.assertEqual(rules.decision_card("x", question(asked_by=""))["byline"], "2026-10-29")

    def test_prompt(self):
        cards = [
            rules.decision_card("a", question(answer={"text": "One file each."})),
            rules.decision_card("b", question(1160, "pr", title="", options=["Keep", "Close"],
                                              answer={"pick": "Close", "text": ""})),
            rules.decision_card("c", question(7)),  # not answered: left out
        ]
        p = rules.decision_prompt(cards)
        self.assertTrue(p.startswith(rules.DECISION_PREAMBLE))
        self.assertIn("Open the issue or PR and act on the answer now.", p)
        self.assertNotIn("read the question there", p)
        self.assertIn("## 1. Issue #1163: Decision mode\n\nQuestion: Where do the questions live?\n"
                      "Answer: One file each.", p)
        self.assertIn("## 2. PR #1160\n\nQuestion: Where do the questions live?\nPicked: Close", p)
        self.assertNotIn("#7", p)


class DecisionFiles(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["REVIEW_DESK_HOME"] = self.tmp.name
        import server
        self.server = server
        self.dir = Path(self.tmp.name) / "decisions"
        self.dir.mkdir()
        (self.dir / "issue-1163-a.json").write_text(json.dumps(question(options=["A", "B"])))
        (self.dir / "broken.json").write_text("{not json")
        self.desk = server.Desk({"repo": "a/b", "repo_path": "/r"})

    def tearDown(self):
        self.tmp.cleanup()

    def saved(self):
        return json.loads((self.dir / "issue-1163-a.json").read_text())

    def test_list_skips_broken_files(self):
        q = self.server.load_decisions(self.desk)
        self.assertEqual([c["id"] for c in q["cards"]], ["issue-1163-a"])
        self.assertEqual(q["repo"], "a/b")

    def test_answer_is_saved_in_the_file(self):
        r = self.server.act_decision_answer(self.desk, None, {"id": "issue-1163-a", "pick": "B", "text": " Go. "})
        self.assertTrue(r["ok"])
        self.assertTrue(r["card"]["answered"])
        saved = self.saved()
        self.assertEqual((saved["answer"]["pick"], saved["answer"]["text"]), ("B", "Go."))
        self.assertEqual(saved["question"], "Where do the questions live?")  # the rest is kept
        r = self.server.act_decision_answer(self.desk, None, {"id": "issue-1163-a", "pick": "", "text": ""})
        self.assertFalse(r["card"]["answered"])
        self.assertNotIn("answer", self.saved())

    def test_bad_answer_and_bad_id_change_nothing(self):
        before = (self.dir / "issue-1163-a.json").read_text()
        r = self.server.act_decision_answer(self.desk, None, {"id": "issue-1163-a", "text": "x" * 5000})
        self.assertFalse(r["ok"])
        self.assertIn("at most", r["message"])
        r = self.server.act_decision_answer(self.desk, None, {"id": "issue-1163-a", "text": "bell\x07"})
        self.assertFalse(r["ok"])
        self.assertNotIn("at most", r["message"])  # a control character isn't a length problem
        for did in ("../issue-1163-a", "missing", 12, None):
            self.assertFalse(self.server.act_decision_answer(self.desk, None, {"id": did, "text": "ok"})["ok"])
        self.assertEqual((self.dir / "issue-1163-a.json").read_text(), before)

    def test_prompt_then_sent_hides_them(self):
        self.assertFalse(self.server.act_decision_prompt(self.desk, None, {})["ok"])  # nothing answered
        self.server.act_decision_answer(self.desk, None, {"id": "issue-1163-a", "text": "One file each."})
        r = self.server.act_decision_prompt(self.desk, None, {})
        self.assertEqual(r["ids"], ["issue-1163-a"])
        self.assertIn("Issue #1163", r["prompt"])
        r = self.server.act_decision_sent(self.desk, None, {"ids": r["ids"]})
        self.assertEqual(r["closed"], ["issue-1163-a"])
        self.assertIn("sent_at", self.saved())
        self.assertEqual(self.server.load_decisions(self.desk)["cards"], [])
        # A sent question can't be answered or sent again.
        self.assertFalse(self.server.act_decision_answer(self.desk, None, {"id": "issue-1163-a", "text": "x"})["ok"])
        self.assertEqual(self.server.act_decision_sent(self.desk, None, {"ids": ["issue-1163-a"]})["closed"], [])
        self.assertFalse(self.server.act_decision_sent(self.desk, None, {"ids": ["../x"]})["ok"])

    def test_dismiss(self):
        r = self.server.act_decision_dismiss(self.desk, None, {"id": "issue-1163-a"})
        self.assertEqual(r["closed"], ["issue-1163-a"])
        self.assertIn("dismissed_at", self.saved())
        self.assertEqual(self.server.load_decisions(self.desk)["counts"]["open"], 0)

    def test_no_folder_is_no_questions(self):
        for f in self.dir.iterdir():
            f.unlink()
        self.dir.rmdir()
        self.assertEqual(self.server.load_decisions(self.desk)["cards"], [])

    def test_actions_need_no_pr(self):
        for name in ("decision-answer", "decision-prompt", "decision-sent", "decision-dismiss"):
            self.assertIn(name, self.server.ACTIONS)
            self.assertIn(name, self.server.NO_PR)


class AskScript(unittest.TestCase):
    """ask.py: the factory skills' way to put a question on the desk."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["REVIEW_DESK_HOME"] = self.tmp.name
        import ask
        self.ask = ask
        self.dir = Path(self.tmp.name) / "decisions"

    def tearDown(self):
        self.tmp.cleanup()

    def run_ask(self, *argv):
        import io
        out = io.StringIO()
        with unittest.mock.patch("sys.stdout", out):
            code = self.ask.main(list(argv))
        return code, out.getvalue().strip()

    def files(self):
        return sorted(p.name for p in self.dir.glob("*.json"))

    def test_writes_a_file_the_desk_shows(self):
        code, out = self.run_ask("--issue", "72", "--question", "Close #72 now?", "--asked-by", "backlog-sweep",
                                 "--option", "Close it", "--option", "Keep it", "--recommended", "Keep it",
                                 "--title", "Terms", "--context", "  why  ")
        self.assertEqual((code, out), (0, "wrote issue-72-close-72-now"))
        raw = json.loads((self.dir / "issue-72-close-72-now.json").read_text())
        self.assertEqual(raw["issue"], 72)
        self.assertEqual(raw["options"], ["Close it", "Keep it"])
        self.assertEqual(raw["context"], "why")
        self.assertEqual(raw["asked_by"], "backlog-sweep")
        self.assertEqual(rules.decision_card("issue-72-close-72-now", raw)["byline"],
                         "backlog-sweep, " + raw["asked_at"][:10])  # the date once (#1206)
        card = rules.decision_card("issue-72-close-72-now", raw)
        self.assertEqual((card["kind"], card["number"], card["recommended"]), ("issue", 72, "Keep it"))
        q = rules.build_decision_queue([("issue-72-close-72-now", raw)])
        self.assertEqual(q["counts"]["open"], 1)

    def test_slug_and_pr(self):
        code, out = self.run_ask("--pr", "1180", "--question", "Keep the abc block?", "--slug", "notation field",
                                 "--asked-by", "pr-qa-demo")
        self.assertEqual((code, out), (0, "wrote pr-1180-notation-field"))
        self.assertEqual(self.files(), ["pr-1180-notation-field.json"])

    def test_open_question_is_not_asked_twice(self):
        self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "a")
        code, out = self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "b")
        self.assertEqual((code, out), (0, "open issue-7-build-it"))
        self.assertEqual(json.loads((self.dir / "issue-7-build-it.json").read_text())["asked_by"], "a")
        # Answered but not yet sent: still the engineer's turn.
        raw = json.loads((self.dir / "issue-7-build-it.json").read_text())
        raw["answer"] = {"pick": "", "text": "yes"}
        (self.dir / "issue-7-build-it.json").write_text(json.dumps(raw))
        self.assertEqual(self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "b")[1], "open issue-7-build-it")

    def test_dismissed_recently_is_not_asked_again_but_sent_is(self):
        self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "a")
        path = self.dir / "issue-7-build-it.json"
        raw = json.loads(path.read_text())
        raw["dismissed_at"] = self.ask.now_iso()
        path.write_text(json.dumps(raw))
        self.assertEqual(self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "a")[1], "dismissed issue-7-build-it")
        raw.pop("dismissed_at")
        raw["sent_at"] = "2026-10-01T10:00:00-07:00"
        path.write_text(json.dumps(raw))
        self.assertEqual(self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "a")[1], "wrote issue-7-build-it")
        self.assertEqual(self.files(), ["issue-7-build-it-202610011000.json", "issue-7-build-it.json"])

    def test_bad_input(self):
        for argv in (["--issue", "7", "--question", "???", "--asked-by", "a"],
                     ["--issue", "7", "--question", "Build it?", "--asked-by", "a", "--option", "A", "--recommended", "B"],
                     ["--issue", "0", "--question", "Build it?", "--asked-by", "a"]):
            with unittest.mock.patch("sys.stderr", __import__("io").StringIO()):
                self.assertEqual(self.ask.main(argv), 2, argv)
        self.assertEqual(self.files(), [])
        # list takes one issue or PR number: a usage line and exit 2, no traceback (#1206).
        for argv in (["list", "abc"], ["list", "-3"], ["list", "7", "8"]):
            err = __import__("io").StringIO()
            with unittest.mock.patch("sys.stderr", err):
                self.assertEqual(self.ask.main(argv), 2, argv)
            self.assertTrue(err.getvalue().startswith("usage: ask.py list [N]"), argv)

    def test_list(self):
        self.run_ask("--issue", "7", "--question", "Build it?", "--asked-by", "a")
        self.run_ask("--pr", "9", "--question", "Land it?", "--asked-by", "a")
        code, out = self.run_ask("list", "7")
        self.assertEqual(code, 0)
        self.assertEqual(len(out.splitlines()), 1)
        self.assertTrue(out.startswith("issue-7-build-it	open	"))
        self.assertEqual(len(self.run_ask("list")[1].splitlines()), 2)


ART = "https://claude.ai/artifact/7ZJU2FJiHHdUw1XhGM6Ysr"


class DesignMode(unittest.TestCase):
    """#1239: a design PR opens in design mode with its artifact."""

    def design(self, n=9, title="[1239] Design: Desk layouts", body="## What this PR is for\n\n- Mockups.\n",
               comments=()):
        p = pr(n, comments=list(comments))
        p["title"], p["body"] = title, body
        return p

    def test_detected_from_the_title(self):
        self.assertTrue(rules.is_design(self.design()))
        self.assertTrue(rules.is_design(self.design(title="[12][13] design: two")))
        self.assertTrue(rules.is_design(self.design(title="[12]Design: tight")))
        for title in ["[12] Designs for X", "Design: no number", "[12] Fix the Design: page", "[x] Design: y"]:
            self.assertFalse(rules.is_design(self.design(title=title)), title)

    def test_detected_from_the_first_line_of_the_body(self):
        p = self.design(title="[5] Desk pictures", body="\n Design: %s\n\n## What this PR is for\n" % ART)
        self.assertTrue(rules.is_design(p))
        self.assertEqual(rules.design_link(p["body"]), ART)
        # Only the first line counts; a link further down doesn't make it a design PR.
        later = self.design(title="[5] Desk pictures", body="## What\n\nDesign: %s\n" % ART)
        self.assertFalse(rules.is_design(later))
        self.assertFalse(rules.is_design(self.design(title="[5] x", body="Design: https://example.com/a")))
        self.assertFalse(rules.is_design(self.design(title="[5] x", body=None)))

    def test_link_falls_back_to_the_first_artifact_in_the_body(self):
        body = "Mockups are at [the page](%s) and %s/other" % (ART, ART)
        self.assertEqual(rules.design_link(body), ART)
        self.assertEqual(rules.design_link("no link"), "")

    def test_card_needs_no_qa_report(self):
        q = rules.build_queue([self.design(), pr(2)], {})
        self.assertEqual([c["number"] for c in q["cards"]], [2, 9])
        self.assertEqual(q["counts"]["waiting_for_qa"], 0)
        c = rules.card(self.design(body="Design: %s\n" % ART), False)
        self.assertTrue(c["design"])
        self.assertFalse(c["red"], c["reasons"])
        self.assertEqual(c["artifact"], ART)
        self.assertIn("#9", c["design_prompt"])
        self.assertIn(ART, c["design_prompt"])
        self.assertIn("close the PR", c["design_prompt"])
        # A QA report that says not ready still counts.
        self.assertEqual(rules.card(self.design(comments=[qa(9, "not ready")]), False)["reasons"], ["QA says not ready"])
        plain = rules.card(pr(2), False)
        self.assertFalse(plain["design"])
        self.assertEqual(plain["artifact"], "")


class ClosePr(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["REVIEW_DESK_HOME"] = self.tmp.name
        import server
        self.server = server
        self.desk = server.Desk({"repo": "a/b", "repo_path": "/r"})
        self.calls = []
        self.state = "OPEN"

        class Done:
            def __init__(self, out):
                self.returncode, self.stdout, self.stderr = 0, out, ""

        def fake(argv, **kw):
            self.calls.append(argv)
            if argv[:3] == ["gh", "pr", "view"]:
                return Done(json.dumps({"state": self.state, "labels": []}))
            return Done("{}")
        self.real = server.subprocess.run
        server.subprocess.run = fake

    def tearDown(self):
        self.server.subprocess.run = self.real
        self.tmp.cleanup()

    def test_close_calls_gh_api_patch(self):
        r = self.server.act_pr_close(self.desk, 42, {"confirmed": True})
        self.assertTrue(r["ok"], r)
        self.assertEqual(self.calls[-1], ["gh", "api", "-X", "PATCH", "repos/a/b/pulls/42", "-f", "state=closed"])
        self.assertIn("pr-close #42", (Path(self.tmp.name) / "log" / "actions.log").read_text())

    def test_close_needs_a_yes_and_an_open_pr(self):
        r = self.server.act_pr_close(self.desk, 42, {})
        self.assertFalse(r["ok"])
        self.assertEqual(self.calls, [])
        self.state = "MERGED"
        r = self.server.act_pr_close(self.desk, 42, {"confirmed": True})
        self.assertFalse(r["ok"]); self.assertTrue(r["skipped"])
        self.assertEqual(len(self.calls), 1)  # only the state check
        self.assertIn("pr-close", self.server.ACTIONS)
        self.assertNotIn("pr-close", self.server.NO_PR)
        with self.assertRaises(self.server.CommandError):
            self.server.build_argv("pr-close", {"pr": "42/../x", "repo": "a/b"})

    def test_dry_run_closes_nothing(self):
        self.server.DRY_RUN = True
        try:
            self.assertTrue(self.server.act_pr_close(self.desk, 42, {"confirmed": True})["ok"])
        finally:
            self.server.DRY_RUN = False
        self.assertEqual([c[:3] for c in self.calls], [["gh", "pr", "view"]])


if __name__ == "__main__":
    unittest.main()
