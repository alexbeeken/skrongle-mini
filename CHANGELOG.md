# Changelog

What changed in each release, for the person running the server. Newest
first. Each release's GitHub notes are its section here. How version
numbers work is in the README, "Versions".

## Unreleased

- Groundwork for per-account permissions: every account now has a stored
  set of permissions (Student and Teacher are the built-in presets). Every
  existing and new account gets the Teacher set, which is everything an
  account could do before, so nothing changes on screen yet. The database
  moves to schema v4 on the first start.
- Manage Users has a **Permissions** column and, on each account's row, a
  **Permissions** dialog: choose Student, Teacher or Custom (a switch for
  each of the seven permissions). An account given **Superuser** gets Manage
  Users in its menu and sees only the accounts there; invitation links, the
  AI keys, temporary passwords, two-factor off and Make owner stay the
  owner's. The permissions are stored and shown; they do not limit anything
  else yet.
- `uninstall.sh --purge` left the data behind on a DietPi install: it said
  "removed, and its data with it" while `/mnt/dietpi_userdata/skrongle` (the
  database and its key) stayed, so installing again brought the old accounts
  back. It now removes that folder too. When the settings put the database
  in a folder the script does not know, it says at the end that the database
  is not removed, instead of saying the data is gone.
- A plain `uninstall.sh` ended by telling you to run `uninstall.sh --purge`
  to delete the data, a script it had just removed with `/opt/skrongle`. It
  now ends with the `rm -rf` line for the folders it kept.
- `dietpi-install.sh` ends with the owner link again, in a short block of its
  own. On a fresh DietPi it used to end with "is up: open http://..." and no
  link, because the command that makes the link could not read the service's
  settings there. The link now has the machine's IP address, not
  `<hostname>.local`, which a fresh DietPi does not answer to; with
  `SKRONGLE_PUBLIC_URL` set it starts with that. If the link cannot be made,
  the script says why.
- Making your own account, the owner's, no longer asks you to tick "I am
  at least 14 years old". The people you invite are still asked, and
  `SKRONGLE_MIN_AGE` works as before.
Changes the database (a new column on `courses`), so the next release is a
minor one, not a patch.

- When writing a course stops because of the AI provider (a rejected key, no
  credit left, a busy provider), the explanation and next step now appear in
  your language, as do the headings of the progress window. They were English
  only. The reason is kept with the course, so it no longer depends on the
  provider's wording.
- A Skrongle mini now holds up to 50 accounts, up from 8, so a class fits.
  It takes 128 connections at once, up from 32. Nothing to change on your
  side. A full class using it at the same moment has not been measured on a
  Pi Zero 2 W yet; if yours is slow or runs out of memory with many
  learners at once, please open an issue with what `skrongle doctor` prints.
- Study mode: after you pass a lesson's two-question check, **Next lesson**
  sits beside *Recommended practice* and *Back to the lessons*.

## 0.2.1 (2026-11-09)

A sign-in fix and a lighter server. Nothing to change on your side.

- `update.sh` runs from a copy of itself, so replacing `/opt/skrongle` no
  longer disturbs the script doing the replacing. Updating 0.1.2 to 0.2.0
  ended with `Syntax error: ")" unexpected` after the service was already
  running the new version; that line was harmless. Updating 0.2.0 to this
  release may show it one last time, because the script that runs is the
  installed 0.2.0 one.
- Sign-in could be blocked for everyone: about 64 wrong sign-ins with
  made-up addresses filled a rate-limit table, and then no account that was
  not already in it could sign in for up to 15 minutes. Made-up addresses no
  longer take a place there, and the per-address tables make room for a new
  address instead of refusing it.
- Lighter on the Pi. A course in memory holds each lesson's page once, not
  three times. The first page after a start no longer reads every built-in
  script and style to fingerprint it. Saving in the course editor no longer
  holds up other people's pages while the course is checked. The bundled
  SQLite leaves out the full-text and R-tree modules nothing used.
- The database's query statistics are refreshed after a start and in the
  hourly housekeeping (`PRAGMA optimize`).
- Course generation with an Anthropic key sends the course format reference
  in a way the provider can cache, so the lessons after the first cost less
  input.

**What to do:** nothing.

**Back up:** nothing new.

## 0.2.0 (2026-11-09)

A release for every Raspberry Pi, and a safer install.

- **32-bit Raspberry Pi builds.** Two new tarballs beside `aarch64` and
  `x86_64`: `armv7l` (Pi 2 and newer on the 32-bit OS) and `armv6l` (Pi 1,
  Zero and Zero W). The install commands pick the right one from `uname -m`.
  The ARMv6 boards have one slow core: expect sign-in to take a while there.
- **Default port 7576**, not 8080, for new installs: 8080 is taken on many
  home servers. An existing install keeps 8080 (below).
- **The service no longer restarts forever** on a database that a newer
  build has upgraded. The server exits with status 78 and the unit leaves it
  stopped, with one line in the log naming the file.
- **`skrongle owner-link`** prints the first-run owner link on demand, and
  **`skrongle doctor --service`** reads the installed unit's settings and
  checks them (`doctor --env-file FILE` still works). Both take `--service`
  to read the unit that is installed.
- **`uninstall.sh`** ships in the tarball: it removes the service and the
  program and keeps the data unless you pass `--purge`. The guide has an
  Uninstalling section.
- **One setup guide**: [Set up Skrongle on a new Raspberry Pi](docs/setup-home-wifi.md)
  is the single beginner path; the self-hosting reference lists what a
  release contains, its asset names, and a unit variant for a static system
  user.
- **Faster pages on small boards**: the course view is built once per course,
  lesson vocabulary is normalised once at load, and group titles come from
  an index. Nothing changes on screen.
- For maintainers: this changelog is the release notes, a version policy in
  the README, `scripts/release-check.sh`, clippy at zero warnings in CI, and
  a release-QA rig (`scripts/release-qa`) that checks every architecture's
  tarball in a container of that architecture.

This is a minor release because the systemd unit changes (the port and the
restart rule). The database layout does not change: schema v3, as in 0.1.2.

**What to do:** update as usual: `sudo /opt/skrongle/update.sh`. Your
install stays on port 8080, because `update.sh` does not replace the unit
in `/etc/systemd/system` and the old unit says 8080. To take the new unit
(port 7576 and the restart rule): copy `/opt/skrongle/skrongle.service` to
`/etc/systemd/system/`, run `sudo systemctl daemon-reload && sudo systemctl
restart skrongle`, then change the port in `SKRONGLE_PUBLIC_URL` in
`/etc/skrongle/env` and in anything else that points at 8080 (a bookmark,
`tailscale serve`, a proxy). To take the new unit and keep 8080, add
`SKRONGLE_BIND=0.0.0.0:8080` to `/etc/skrongle/env` first. Details under
"Updating" in `docs/self-hosting.md`.

**Back up:** nothing new. `update.sh` copies the database to
`learn.db.before-0.2.0` first, as always.

## 0.1.2 (2026-11-04)

- Sharing a course is by invitation link, not by e-mail address. The Share
  dialog makes a link with an optional name, so you can tell links apart.
  The list shows each link's name, whether it was opened and by whom, and
  removes a link when you remove it.
- The database layout changes (schema v3). Links shared by e-mail keep
  working, and their address becomes their name.
- This is a patch release that changes the database layout. The version
  policy in the README does not allow that for a patch, so it is a one-off.

**What to do:** update as usual. `update.sh` copies the database to
`learn.db.before-0.1.2` first.

**Back up:** keep `learn.db.before-0.1.2`. Version 0.1.1 refuses a database
this release has upgraded, so going back needs that copy. Stop the service,
copy it over `learn.db` (and delete `learn.db-wal` and `learn.db-shm` beside
it), then run `sudo /opt/skrongle/update.sh 0.1.1`.

## 0.1.1 (2026-11-04)

The first release. It ships the server for 64-bit Raspberry Pi
(`aarch64`) and 64-bit PC Linux (`x86_64`) as one tarball, with the systemd
unit and `update.sh`.

- Skrongle Cash and its shop are removed (schema v2).
- A database from a build before the 1.0 schema is not upgraded. The server
  stops and says so; start from a fresh one.

**What to do:** a new install follows the setup guide.

**Back up:** `learn.db` and `learn.db.key` in `/var/lib/skrongle`, together.
