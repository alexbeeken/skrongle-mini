"""Unit tests for usage.py (#1366). No network; a fake ~/.claude/projects in a temp folder.

    cd scripts/review-desk && python3 -I -B -m unittest -v
"""

import json
import os
import sys
import tempfile
import unittest
import unittest.mock
from datetime import datetime, timezone
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import usage  # noqa: E402


def stamp(local):
    """A transcript timestamp (UTC) for a local "YYYY-MM-DD HH:MM"."""
    t = datetime.strptime(local, "%Y-%m-%d %H:%M").astimezone(timezone.utc)
    return t.strftime("%Y-%m-%dT%H:%M:%S.000Z")


def call(at, mid, model="claude-opus-5-5", inp=0, out=0, write=0, read=0, w1h=0):
    return {"timestamp": stamp(at), "requestId": "req_" + mid, "message": {
        "id": mid, "model": model, "role": "assistant", "content": [],
        "usage": {"input_tokens": inp, "output_tokens": out, "cache_creation_input_tokens": write,
                  "cache_read_input_tokens": read,
                  "cache_creation": {"ephemeral_1h_input_tokens": w1h, "ephemeral_5m_input_tokens": write - w1h}}}}


def tool(at, tid, command):
    return {"timestamp": stamp(at), "message": {"role": "assistant", "content": [
        {"type": "tool_use", "id": tid, "name": "Bash", "input": {"command": command}}]}}


def result(at, tid, text, error=False):
    return {"timestamp": stamp(at), "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": tid, "content": text, "is_error": error}]}}


class UsageTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        self.projects = root / "projects"
        self.home = root / "wf"
        (self.home / "log").mkdir(parents=True)
        env = {"CLAUDE_PROJECTS_DIR": str(self.projects), "REVIEW_DESK_HOME": str(self.home)}
        self.env = unittest.mock.patch.dict(os.environ, env)
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def transcript(self, workspace, lines, agent=None, meta=None):
        folder = self.projects / ("-Users-alex-orca-workspaces-spanish-learning-" + workspace)
        path = folder / "s1.jsonl"
        if agent:
            path = folder / "s1" / "subagents" / (agent + ".jsonl")
            if meta:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.with_suffix(".meta.json").write_text(json.dumps(meta))
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, "a") as f:
            for l in lines:
                f.write(json.dumps(l) + "\n")

    def log(self, name, text):
        (self.home / "log" / name).write_text(text)

    def test_tokens_add_every_kind(self):
        self.log("rotate.log", "2026-10-30 17:59  started qa-2026-10-30\n")
        self.transcript("qa-2026-10-30", [call("2026-10-30 18:00", "m1", inp=1, out=10, write=100, read=1000),
                                          call("2026-10-30 18:01", "m2", model="claude-haiku-9-9", out=5)])
        [rec] = usage.collect()
        self.assertEqual(rec["models"]["claude-opus-5-5"], {"calls": 1, "input": 1, "output": 10, "cache_write": 100,
                                                            "cache_read": 1000, "tokens": 1111})
        self.assertEqual(rec["models"]["claude-haiku-9-9"]["tokens"], 5)  # a model we don't know still counts
        self.assertEqual(rec["tokens"], 1116)
        self.assertEqual((rec["cached"], rec["uncached"]), (1000, 116))

    def test_rotation_workspace_session_and_agents(self):
        self.log("rotate.log", "2026-10-30 17:59  started backlog-2026-10-30 qa-2026-10-30 · builders 4\n")
        line = call("2026-10-30 18:00", "m1", model="claude-fable-5-1", out=100_000)
        # Claude Code writes one line per content block, each with the same usage: count it once.
        self.transcript("backlog-2026-10-30", [line, line])
        self.transcript("backlog-2026-10-30", [call("2026-10-30 18:10", "m2", out=50_000)],
                        agent="agent-a1", meta={"description": "Build issue #1 to a PR", "agentType": "general-purpose"})
        [rec] = usage.collect()
        self.assertEqual(rec["workspace"], "backlog-2026-10-30")
        self.assertEqual(rec["skill"], "backlog")
        self.assertEqual(rec["rotation"], "2026-10-30 17:59")
        self.assertEqual(rec["session"], {"tokens": 100_000, "cached": 0, "uncached": 100_000})
        self.assertEqual(rec["tokens"], 150_000)
        self.assertEqual(rec["models"]["claude-fable-5-1"]["calls"], 1)
        self.assertEqual(rec["agents"], [{"what": "Build issue #1 to a PR", "type": "general-purpose",
                                          "model": "claude-opus-5-5", "tokens": 50_000,
                                          "cached": 0, "uncached": 50_000}])
        written = [json.loads(l) for l in (self.home / "log" / "usage.jsonl").read_text().splitlines()]
        self.assertEqual(written, [rec])

    def test_outside_a_rotation(self):
        self.log("rotate.log", "2026-10-30 17:59  started qa-2026-10-30\n"
                               "2026-10-30 19:00  stopped: only 20 GB free (need 50)\n")
        self.transcript("qa-2026-10-30-2", [call("2026-10-30 21:00", "m1", out=1000)])
        self.transcript("mergeme-2026-10-30", [call("2026-10-30 18:00", "m2", out=1000)])  # not started then
        self.assertEqual([r["rotation"] for r in usage.collect()], ["", ""])

    def test_work_from_github_writes(self):
        self.log("rotate.log", "2026-10-30 17:59  started backlog-2026-10-30\n")
        at = "2026-10-30 18:00"
        lines = [call(at, "m1", out=10),
                 # gh api create: the new PR's own html_url comes first
                 tool(at, "t1", "gh api repos/{owner}/{repo}/pulls -f base=main -f head=x -f title=t --jq ."),
                 result(at, "t1", '{"html_url":"https://github.com/o/r/pull/12","body":"see https://github.com/o/r/pull/3"}'),
                 # gh issue create in a loop: every URL counts
                 tool(at, "t2", "for f in a b; do gh issue create --title x --body-file $f; done"),
                 result(at, "t2", "https://github.com/o/r/issues/20\nhttps://github.com/o/r/issues/21\n"),
                 # a QA comment through gh api
                 tool(at, "t3", "gh api repos/{owner}/{repo}/issues/7/comments -F body=@r.md --jq .html_url"),
                 result(at, "t3", "https://github.com/o/r/pull/7#issuecomment-1"),
                 # reads and failed writes don't count
                 tool(at, "t4", "gh api repos/{owner}/{repo}/pulls --jq '.[].number'"),
                 result(at, "t4", "https://github.com/o/r/pull/99"),
                 tool(at, "t5", "gh pr create --title y"),
                 result(at, "t5", "https://github.com/o/r/pull/98 already exists", error=True)]
        self.transcript("backlog-2026-10-30", lines)
        [rec] = usage.collect()
        self.assertEqual(rec["work"], {"prs_opened": [12], "issues_filed": [20, 21],
                                       "prs_commented": [7], "prs_landed": []})

    def test_mergeme_landed_from_its_log(self):
        self.log("rotate.log", "2026-10-30 17:59  started mergeme-2026-10-30\n")
        self.log("mergeme.log", "2026-10-29 16:34  PR #1261: in CI #1186 #1189\n"
                                "2026-10-30 18:05  PR #1291: in CI #1210 #1214 #1216\n")
        self.transcript("mergeme-2026-10-30", [call("2026-10-30 18:00", "m1", out=10),
                                               call("2026-10-30 18:04", "m2", out=10)])
        [rec] = usage.collect()
        self.assertEqual(rec["work"]["prs_landed"], [1210, 1214, 1216])

    def test_no_transcripts(self):
        self.assertEqual(usage.collect(), [])
        self.assertEqual((self.home / "log" / "usage.jsonl").read_text(), "")


if __name__ == "__main__":
    unittest.main()
