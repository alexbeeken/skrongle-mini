# Self-hosting FAQ

The first questions people ask when they run their own copy. The details
are in [Self-hosting](self-hosting); the step-by-step Pi guide is
[Set up on home Wi-Fi](setup-home-wifi).

**Before you ask anywhere, run the diagnostics.** `skrongle doctor` prints
the version, OS, database, schema level, which settings are configured and
free disk and memory, and marks anything wrong with `PROBLEM:`. It never
prints a secret. With the systemd unit:

```sh
sudo env $(systemctl show skrongle -p Environment --value) /opt/skrongle/skrongle doctor --env-file /etc/skrongle/env
```

What each line means: [Diagnostics](self-hosting#diagnostics-skrongle-doctor).

## Getting help

**Is self-hosting free?** Yes. Help is best-effort.

**Where do I ask?** In [GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions),
under self-hosting help. Other self-hosters read it too, and an answer there
helps the next person.

**I think it's a bug.** Open an issue with the *Self-hosting bug* form
(version, device, OS, the `skrongle doctor` output and the last lines of
`journalctl -u skrongle -n 50`). Read the log for anything private before
you paste it.

## Hardware and OS

**Which machines does it run on?** Any 64-bit Linux with glibc 2.31 or
newer: a Raspberry Pi Zero 2 W, 3, 4 or 5 with the 64-bit Raspberry Pi OS
(Bullseye or later), Debian 11+, Ubuntu 20.04+, on ARM (`aarch64`) or a PC
(`x86_64`). `uname -m` tells you which release file to download.

**What about the 32-bit Raspberry Pi OS?** There is no release for it
(`uname -m` prints `armv7l`). Install the 64-bit OS, or build it yourself
with `scripts/build-pi.sh armv7`.

**macOS, Windows, Docker?** No release for them. On a Mac you can build it
from source (`cargo build --release`) to try it; Docker isn't needed, since
the app is one binary with no other services.

**How much memory and disk does it need?** About 15–25 MB of memory while
running (the unit caps it at 64 MB), about 10 MB for the program and its
courses, and a database that grows by a few MB with use. A 16 GB SD card is
plenty.

## Installing and running

**The page doesn't load.** Is it running? `systemctl status skrongle`. Then
try `http://<ip>:8080` from the same Wi-Fi, where `<ip>` is the first
address `hostname -I` prints; `.local` names don't work on every network or
phone.

**The log says the port is already in use.** Another program (or a second
copy) has port 8080. Find it with `sudo ss -ltnp 'sport = :8080'`, or move
Skrongle: put `SKRONGLE_BIND=0.0.0.0:8081` in `/etc/skrongle/env` and
`sudo systemctl restart skrongle`.

**The log says it can't write the database.** The database folder must be
writable by the service. The unit keeps it in `/var/lib/skrongle`, which
systemd creates; if you changed `SKRONGLE_DB`, give that folder to the service
user, or go back to the default.

**The log says the secret key is missing.** The database has AI keys that
learners saved, encrypted with `learn.db.key`, and that file isn't next to
the database (typically after restoring or moving only `learn.db`). Copy
`learn.db.key` back from the same backup. If it is gone for good, see
[Backup and restore](self-hosting#backup-and-restore) for
`SKRONGLE_NEW_SECRET_KEY=1`.

**The log says the secret key is wrong.** The key is there but cannot read
the saved AI keys: `SKRONGLE_SECRET_KEY` belongs to another install, or
`learn.db.key` comes from a different backup than `learn.db`. Use the pair
from the same backup (unset `SKRONGLE_SECRET_KEY` to use the file). If the
right key is gone for good, the same `SKRONGLE_NEW_SECRET_KEY=1` applies.

**No courses show up.** `skrongle doctor` shows the data folder and how
many courses it found. The server reads them from `SKRONGLE_DATA_DIR`
(`/opt/skrongle/data` in the unit): unpack the whole tarball, not only the
binary.

**How do I change a setting?** Put `NAME=value` lines in `/etc/skrongle/env`
(create it), then `sudo systemctl restart skrongle`. Every setting is listed
under [Configuration](self-hosting#configuration).

## Keeping it running

**How do I update?** `sudo /opt/skrongle/update.sh`. It keeps a copy of
the database from before the update. See [Updating](self-hosting#updating).

**Will an update lose my learners' progress?** No. Progress, accounts and
courses made in the app are in the database, which an update doesn't
replace; the new version migrates it when it starts (a database from
before the 1.0 schema is not upgraded: the server says so and stops). The update script
still copies it aside first.

**How do I back up?** Copy `learn.db` and `learn.db.key` from
`/var/lib/skrongle`, together, while the service is stopped. Commands for
backing up and restoring: [Backup and restore](self-hosting#backup-and-restore).

**How do I move to a new machine?** Install the same version there, stop
both services, copy `learn.db` and `learn.db.key` into `/var/lib/skrongle`
on the new one, start it.

**How do I go back to an older version?** `sudo /opt/skrongle/update.sh
<old version>`. If the newer version changed the database, restore the
`learn.db.before-<version>` copy the update made.

## Access and AI

**Can people outside my home use it?** Don't forward a port on your router.
Use Tailscale, or put a TLS proxy in front for a public site: see
[Reaching it from outside](self-hosting#reaching-it-from-outside).

**Who can sign up?** Anyone who can reach the server. Keep it on your own
network, or behind Tailscale.

**Do I need an AI key?** No. Lessons, quizzes, games and the built-in
courses work without one. The tutor and the course generator use a key
each learner saves in their profile (Anthropic, OpenAI, Google, or an
OpenAI-compatible server such as Ollama via `SKRONGLE_OPENAI_BASE_URL`).

**Are saved AI keys safe?** They are encrypted with the secret key, which
lives in a separate file (`learn.db.key`), so a copy of the database alone
can't reveal them. Anyone with root on the machine can read both.
