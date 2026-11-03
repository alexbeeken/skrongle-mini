# Security policy

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue or
pull request.

Use GitHub's private reporting: on the repository page open **Security →
Report a vulnerability** (or go to `/security/advisories/new`). Only the
maintainers see the report, and we can discuss and fix it there before
anything is public.

Include what you can of:

- what is affected (a route, a page, a file in `src/`) and the version or
  commit;
- how to reproduce it, step by step, ideally against a local server
  (`./.dev-start --reset --seed`, see `docs/testing.md`);
- what an attacker gains (another learner's data, their session, a call from
  the server to an address of your choice, …).

Please do not test against the hosted site with other people's accounts, do
not run load or denial-of-service tests against it, and do not access or keep
data that is not yours. A local copy is all you need: the whole app is in this
repository.

## What to expect

- An acknowledgement within a week.
- A fix, or a reasoned answer if we think it is not a vulnerability, as soon
  as we can; serious issues come first.
- Credit in the advisory and the release notes, if you want it.

## Supported versions

Only the latest commit on `main` is supported. Self-hosters should update to
it; the app has no other release channel yet.

## Scope notes

Some risks are accepted knowingly (for example: dev mode `SKRONGLE_DEV=1` is
for a laptop only, and anyone with root on a server can read its saved API
keys). Reports about an accepted risk are still welcome when you see a way to
make it worse than described.
