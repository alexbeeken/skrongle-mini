# Changelog

What changed in each release, for the person running the server. Newest
first. Each release's GitHub notes are its section here. How version
numbers work is in the README, "Versions".

## Unreleased

- New: `sudo /opt/skrongle/skrongle doctor --service` reads the settings of
  the installed unit and checks them. `doctor --env-file FILE` still works.
- The setup guide is one path now, [Set up Skrongle on a new Raspberry Pi](docs/setup-home-wifi.md).
  The reference and the FAQ are reorganised to match.
- The unit file and `deploy/env.example` changed in their comments only.

**What to do:** nothing. An installed unit and `/etc/skrongle/env` keep working.

**Back up:** nothing new. See "Backup and restore" in `docs/self-hosting.md`.

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
