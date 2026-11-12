# Changelog

What changed in each release, for the person running the server. Newest
first. Each release's GitHub notes are its section here. How version
numbers work is in the README, "Versions".

## Unreleased

- The **Create courses** permission now counts: an account without it (a
  Student, for example) has no *Create a course*, *Copy* or *Copy and
  translate* on its course list, and those actions are refused. It still
  learns from built-in and shared courses, and courses it already made stay
  its own to open and edit.
- The **Share courses** permission now counts: an account without it cannot
  make invitation links for its courses. Links it already made keep working
  and it can still remove them; opening someone else's link is open to
  every account.
- The **Skip levels** permission now counts. An account without it starts
  every course at the beginning: it is not asked *Where do you want to
  start?*, has no level row behind the gear on a course's card and cannot
  take the placement test. A level chosen earlier is kept; it just cannot be
  moved.
- AI is now per account. **Use the AI tutor** decides who sees and may open
  the tutor; **Use AI in courses** decides who may generate a course, use
  *Use AI* in the editor and *Copy and translate*. An account without one
  is not shown that feature, and its pages answer "Your account is not
  allowed to do this". *Hide AI features for everyone* still turns AI off
  for every account, whatever their permissions.
- Manage Users: tick several accounts (or all of them with the box in the
  table's heading), choose a preset and click **Apply to selected** to give
  it to all of them in one step, after a confirmation. The page says how
  many accounts were changed; owners and your own account are never changed
  this way.
- Manage Users has a **Progress** link on each account's row: the courses
  the account studies, how many lessons it has passed, when it last studied
  and its streak, for you and for superusers. An account with the
  **Privacy** permission (a Teacher has it, a Student does not) shows
  *Private* instead, has no progress page, and a superuser does not see its
  last sign-in. Owners are always private. Each account's profile says, in
  *Who sees what*, whether its progress is visible.
- In the Permissions dialog, a **Custom** set can be given a name (**Save as
  a preset**): it is then offered for every account, next to Student and
  Teacher. A server keeps up to 20 saved presets; they are listed under the
  accounts on Manage Users, each with **Delete**. Deleting one changes
  nobody's permissions: the accounts that had it show as Custom.
- Updating no longer copies the whole database. `update.sh` and
  `dietpi-install.sh` made a `learn.db.before-<version>` copy on every run;
  they make none now.
- Every database migration can be taken back. `update.sh <older version>`
  does it for you: the installed binary takes the database's layout back to
  the older version's before the binary is swapped (`skrongle migrate
  <schema-version>` is the command behind it). What only the newer version
  stored is dropped on the way back.
- A fresh `dietpi-install.sh` install no longer prints
  `stopping skrongle (none -> <version>)`, and a second run says
  `installing <version> again`.
- **Connected apps** (first step of letting an AI assistant make courses):
  a new switch on Manage Users, **Let apps connect**, off until you turn it
  on. With it on, an account that may create courses gets a *Connected
  apps* section on its profile, where it can **Make a token** (it asks for
  the password, the token is shown once and works for 30 days, 90 days or
  a year) and **Revoke** one. You see every account's connections on
  Manage Users and can revoke any of them. Changing a password, setting a
  temporary password and turning off an account's two-factor sign-in
  disconnect its apps. Only a token's fingerprint (SHA-256) is stored. The
  database gets two new tables (`api_grants`, `api_tokens`); going back to
  an older version drops them.

**What to do:** nothing to update. The update script no longer leaves a copy
of the database behind, so if you relied on those as backups, make your own
before an update (`dietpi-backup`, or copy `learn.db` and `learn.db.key`
with the service stopped). The `learn.db.before-*` files from earlier
updates are yours to delete. Going back from this version to 0.3.0 works
with `update.sh 0.3.0`; going back from 0.3.0 to an older one still needs
the copy that update made.

## 0.3.0 (2026-11-11)

Room for a class, a tutorial course, and fixes for the DietPi installer.

**This release changes the database** (schema v3 to v5) on its first start.
`update.sh` copies it aside first as `learn.db.before-0.3.0`. Version 0.2.1
cannot open the database afterwards, so going back means putting that copy
back. Nothing else to change on your side.

- Groundwork for per-account permissions: every account now has a stored
  set of permissions (Student and Teacher are the built-in presets). Every
  existing and new account gets the Teacher set, which is everything an
  account could do before, so nothing changes on screen yet.
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
- A new ready-made course, *How to Use Skrongle*: 19 short lessons on the
  app itself (the ways to study, making and sharing courses, AI keys and
  the AI features, your profile, two-factor sign-in and the owner's page).
  It is in English.

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
