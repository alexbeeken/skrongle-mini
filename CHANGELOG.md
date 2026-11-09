# Changelog

What changed in each release, for the person running the server. Newest
first. Each release's GitHub notes are its section here. How version
numbers work is in the README, "Versions".

## Unreleased

- `update.sh` runs from a copy of itself, so replacing `/opt/skrongle` no
  longer disturbs the script doing the replacing. Updating 0.1.2 to 0.2.0
  ended with `Syntax error: ")" unexpected` after the service was already
  running the new version; that line was harmless. Updating 0.2.0 to this
  release may show it one last time, because the script that runs is the
  installed 0.2.0 one.

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
