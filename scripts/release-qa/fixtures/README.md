# Upgrade fixtures (#43)

One small database per published release, written by that release's own
binary. `upgrade-check.sh` starts a newer release on each and checks that
the upgrade works, and then that going back works: the newer release's
`skrongle migrate` takes the database down to the fixture's own schema (the
`SCHEMA=` line of its `.env`), and that release's binary starts on it with
both accounts and the recorded progress intact (`../README.md`,
`docs/testing.md` "Releases").

**All of it is test data.** The accounts use made-up addresses on
`example.org`, and the passwords below are throwaway; nothing here protects a
real install. The `learn-<version>.db.key` files are the keys those
databases were made with, kept only so the databases open. Never use one
anywhere else.

| File | What it is |
|---|---|
| `learn-<version>.db` | The database, after a clean stop (the write-ahead log is folded in). |
| `learn-<version>.db.key` | Its key file, test data only. |
| `learn-<version>.env` | What is in it: schema, accounts, the password, the course, the share link token (empty when the release had no sharing), the invitation token nobody used, the two lessons. |
| `learn-<version>.progress.txt` | The learner's progress as `/api/progress` reported it when the fixture was made: `lesson-id seen passed best_pct`. |

Accounts (the same in every fixture):

| Who | Address | Password |
|---|---|---|
| Owner | `owner@example.org` | `a-long-enough-password` |
| Learner | `learner@example.org` | `a-long-enough-password` |

Each holds: the owner and one invited learner; the learner has passed one
lesson (`a1-greetings`, 5 of 5) and seen another (`a1-subject-pronouns`);
the owner's copy of the Spanish course, shared by link and opened by the
learner (0.1.1 had no sharing, so its copy is not shared); a second
invitation link that nobody used.

Each database is about 2.6 MB because the copied course is stored in it
(about 0.5 MB compressed).

## Adding the fixture for a release

After the release is published, from the repository root:

```sh
mkdir -p dist && gh release download v0.3.0 -p 'skrongle-0.3.0-aarch64.tar.gz*' -D dist
docker compose -f scripts/release-qa/compose.yaml --profile qa build qa-aarch64
docker run --rm -e KEEP_DB=/fixtures/learn-0.3.0.db \
  -v "$PWD/dist:/tarballs:ro" -v "$PWD/scripts/release-qa/fixtures:/fixtures" \
  -v "$PWD/scripts/release-qa/package-smoke.sh:/ps.sh:ro" \
  --entrypoint sh skrongle-release-qa:aarch64 /ps.sh /tarballs/skrongle-0.3.0-aarch64.tar.gz
```

Any architecture's container makes the same file. The run may print `FAIL`
lines for checks the newer `package-smoke.sh` makes of an older package; the
fixture is written regardless. Check that `learn-0.3.0.env` lists a share
token and that `git status` shows the four new files.
