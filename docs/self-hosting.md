# Self-hosting reference

**New here? Don't start on this page.** Follow
[Set up Skrongle on a new Raspberry Pi](setup-home-wifi), top to bottom: it
goes from the box to a private install with your account, two-factor sign-in
and a backup. Then [Away from home with Tailscale](setup-tailscale) if you
want it outside the house.

This page is the reference behind those steps, for any 64-bit Linux
machine: every setting, the systemd unit, a proxy in front, recovery
commands, uninstalling and troubleshooting. Stuck? The
[self-hosting FAQ](self-hosting-faq) answers the first questions people ask.

Self-hosting is free. Help is best-effort, in
[GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions).

## Skrongle mini

The release you install is **Skrongle mini**: one binary, with
`skrongle.service` set for a handful of people who know you and
no outside services. How it is set up:

- **No e-mail.** Nothing is mailed; links are shown in the page
  ([No e-mail](#no-e-mail)).
- **Sign-up by invitation.** The first account is yours, the owner's;
  everyone else needs a link you make (`SKRONGLE_SIGNUP=invite`,
  [Accounts and AI keys](#accounts-and-ai-keys)).
- **Background work in process.** The background jobs are rows in SQLite, run by threads in the server: no
  Redis or queue service.
- **SQLite only.** One database file in `/var/lib/skrongle`, no database
  server.

The startup log line and `/healthz` say `skrongle mini <version>`.

### Security

You secure the login, as with any self-hosted app:

- A strong password for every account.
- Two-factor sign-in with an authenticator app (*Profile → Two-factor
  authentication*). It needs no e-mail.
- The server reachable only over Tailscale or a VPN
  ([Reaching it from outside](#reaching-it-from-outside)). **Never forward
  a port** on the router.

What the app covers by itself: argon2id password hashes; sign-in limits (30
attempts per address per 10 minutes, 10 wrong passwords per account per 15
minutes) and registration limits; slow or oversized requests cut off; a
connection cap. What it doesn't: HTTPS (it speaks plain HTTP; Tailscale
encrypts the link), a lockout or an alert after many guesses, attackers
guessing from many addresses at once, and updates (install new releases
yourself, [Updating](#updating)).

- Already run a sign-in proxy (Authelia, Tailscale Serve)? Let it sign
  people in: [Signing in at the proxy](#signing-in-at-the-proxy).
- Locked yourself out? The binary resets a password or two-factor from the
  shell: [If you lock yourself out](#if-you-lock-yourself-out).
- Thinking of turning on *Let apps connect*? [Security: connected
  apps](security-apps) lists what can go wrong with tokens, the MCP server
  and OAuth sign-in, what stops each, and the risks left.

## What a release contains

Each [GitHub release](https://github.com/alexbeeken/skrongle-mini/releases) has one
tarball per architecture, named `skrongle-<version>-<arch>.tar.gz`, where
`<arch>` is what `uname -m` prints:

| File | For |
|---|---|
| `skrongle-<version>-aarch64.tar.gz` | 64-bit ARM: Raspberry Pi Zero 2 W, 3, 4, 5 on the 64-bit OS |
| `skrongle-<version>-x86_64.tar.gz` | PCs, mini PCs, most VPSs |
| `skrongle-<version>-armv7l.tar.gz` | 32-bit ARM: Raspberry Pi 2 and newer on the 32-bit OS |
| `skrongle-<version>-armv6l.tar.gz` | Raspberry Pi 1, Zero and Zero W (ARMv6, one core, 512 MB). Sign-in is slow on these |

Each tarball comes with a `.sha256` file, named
`skrongle-<version>-<arch>.tar.gz.sha256`. A tarball unpacks (flat, no top folder) to:

| Path | What it is |
|---|---|
| `skrongle` | The server: one binary. Templates, CSS, scripts and these docs are compiled in. |
| `data/courses/` | The built-in courses, read from disk when the server starts (`SKRONGLE_DATA_DIR`). |
| `skrongle.service` | The systemd unit below (`deploy/skrongle.service` in the repo). |
| `update.sh` | Updates the install in place: see [Updating](#updating). |
| `uninstall.sh` | Removes the install, keeping the data unless `--purge`: see [Uninstalling](#uninstalling). |
| `VERSION` | The version, one line. |

This layout is stable. A later release may add files; none of these five
paths is removed or renamed without a major version. Release assets are
never deleted or renamed either (see the README).

Everything a learner creates (accounts, progress, courses made in the app)
is in the SQLite database, not in these files, so unpacking a new release
over the old one keeps it.

The `.sha256` file next to each tarball is its SHA-256 sum. The install
command below downloads it and checks the tarball against it (`sha256sum -c`
prints `OK`); stop if it doesn't. `update.sh` does the same check.

## Requirements

- 64-bit Linux with glibc 2.31 or newer: Raspberry Pi OS Bookworm or later,
  Debian 11+, Ubuntu 20.04+. On a Pi, use the **64-bit** OS (`aarch64`).
- No other services: there is no separate database, web server or runtime.
- About 64 MB of memory for the service (the unit's hard limit).

## Install

The first line finds the newest release on the
[releases page](https://github.com/alexbeeken/skrongle-mini/releases)
(or set `V=` to the one you want); `uname -m` picks the architecture:

```sh
V=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/alexbeeken/skrongle-mini/releases/latest); V=${V##*/v}; echo "installing $V"
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz.sha256
sha256sum -c skrongle-$V-$(uname -m).tar.gz.sha256
sudo mkdir -p /opt/skrongle
sudo tar -xzf skrongle-$V-$(uname -m).tar.gz -C /opt/skrongle
sudo mkdir -p /etc/skrongle
sudo cp /opt/skrongle/skrongle.service /etc/systemd/system/
sudo systemctl enable --now skrongle
```

Then:

1. `sudo /opt/skrongle/skrongle doctor --service` should show no `PROBLEM:`
   line ([Diagnostics](#diagnostics-skrongle-doctor)).
2. `http://<hostname>.local:7576` (or `http://<ip>:7576`, where `<ip>` is the
   first address `hostname -I` prints) opens. It has no account yet.
3. **Make your account with the owner link**:
   `sudo /opt/skrongle/skrongle owner-link --service`. Until
   you do, `/register` only says the server has no owner yet
   ([Accounts and AI keys](#accounts-and-ai-keys)).
4. Turn on two-factor sign-in (*Profile → Two-factor authentication*), keep
   it off the internet ([Reaching it from outside](#reaching-it-from-outside))
   and make a backup ([Backup and restore](#backup-and-restore)).

To try it without systemd, run it from its folder:
`cd /opt/skrongle && SKRONGLE_DB=$HOME/learn.db ./skrongle`.

### Installing on DietPi

`deploy/dietpi-install.sh` does the whole install in one run, the way
`dietpi-software` does it: it downloads the latest release for this machine
and checks its checksum, unpacks it to `/opt/skrongle`, creates the
`skrongle` user, keeps the data in `/mnt/dietpi_userdata/skrongle`, writes
the [static-user unit](#a-static-user-data-outside-varlib) and starts it.
Its last lines are the owner link, with the machine's IP address unless
`SKRONGLE_PUBLIC_URL` is set. Run it again to update (it keeps your env file
and unit; the database is not copied); `--uninstall` and
`--uninstall --purge` remove it. The steps are listed at the top of the file.

## Diagnostics: `skrongle doctor`

`skrongle doctor` prints one screen about the install, to read yourself or
paste into a bug report. It shows the version, OS and architecture, the
device model, the listen address and whether the port is free, the data
folder and how many courses it holds, the database path, size
and schema (migration) level, and whether the database folder is writable.
It also shows whether a secret key is
configured, how many AI keys learners have saved, whether apps may connect
(with the number of grants and registered apps, and a warning when they may
connect over plain HTTP on an address that is not a home network's), and
free disk and memory.
Secrets show as yes/no only, never their values. It only reads. It opens
the database read-only and creates nothing.

It reads the same `SKRONGLE_*` settings as the server. To see what the systemd
service sees, run it with `--service`: it asks systemd for the `skrongle`
unit's settings and reads its env file, as the service does (the shell's own
`SKRONGLE_*` variables are left out):

```sh
sudo /opt/skrongle/skrongle doctor --service
```

Without systemd, `skrongle doctor --env-file FILE` reads the settings from a
file on top of the environment.

A line that starts with `PROBLEM:` says what to fix.

## The systemd unit

`deploy/skrongle.service` holds the Skrongle mini defaults:

- runs `/opt/skrongle/skrongle` as a user systemd creates for it
  (`DynamicUser=yes`), so there is no `useradd` step;
- keeps the database in `/var/lib/skrongle` (`StateDirectory=skrongle`);
- listens on port 7576 on every interface;
- restarts it if it stops (`Restart=on-failure`);
- caps memory at 64 MB (`MemoryMax`) and sandboxes the process
  (`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, …);
- lets people sign up only with an invitation from you
  (`SKRONGLE_SIGNUP=invite`, see [Accounts and AI keys](#accounts-and-ai-keys));
- runs it with no plans or payments (see
  [Accounts and AI keys](#accounts-and-ai-keys));
- reads optional settings from `/etc/skrongle/env`.

Useful commands:

```sh
systemctl status skrongle          # running? since when?
journalctl -u skrongle -f          # the log, live
sudo systemctl restart skrongle    # after changing /etc/skrongle/env
```

### A static user, data outside `/var/lib`

Some systems (DietPi keeps user data under `/mnt/dietpi_userdata`) want a
fixed `skrongle` user and a data folder you choose. Changes from
`deploy/skrongle.service`:

- `User=skrongle` and `Group=skrongle` replace `DynamicUser=yes`;
- no `StateDirectory`: you make the data folder and own it;
- `SKRONGLE_DB` points into that folder, and `ReadWritePaths=` names it so
  `ProtectSystem=strict` (everything else read-only) still applies;
- the rest of the sandbox stays.

```sh
sudo useradd --system --no-create-home --shell /usr/sbin/nologin skrongle
sudo install -d -o skrongle -g skrongle -m 0750 /mnt/dietpi_userdata/skrongle
```

The data folder must be on a local disk (SD card, SSD, USB drive), not an
NFS or SMB mount: SQLite's locking and its `-wal`/`-shm` files do not work
safely over a network share. `learn.db`, `learn.db-wal`, `learn.db-shm` and
the secret key `learn.db.key` (created with mode 0600) all live in that
folder, so back it up whole. `/opt/skrongle` stays root-owned and read-only
to the service. If the folder is not writable the server stops with a
message naming the folder and the user it runs as.

```ini
[Service]
Type=simple
User=skrongle
Group=skrongle
WorkingDirectory=/opt/skrongle
ExecStart=/opt/skrongle/skrongle
Restart=on-failure
# 78: the server refused the database (a newer release upgraded it, or it
# predates the 1.0 schema; src/db.rs NEWER_DB_EXIT). Restarting cannot help.
RestartPreventExitStatus=78
RestartSec=3

Environment=SKRONGLE_BIND=0.0.0.0:7576
Environment=SKRONGLE_DATA_DIR=/opt/skrongle/data
Environment=SKRONGLE_DB=/mnt/dietpi_userdata/skrongle/learn.db
Environment=SKRONGLE_LOG=info
Environment=SKRONGLE_SIGNUP=invite
EnvironmentFile=-/etc/skrongle/env
Environment=MALLOC_ARENA_MAX=2
Environment=MALLOC_TRIM_THRESHOLD_=65536

MemoryMax=64M
MemoryHigh=48M
TasksMax=32
LimitNOFILE=256
CPUWeight=80
Nice=2

NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/mnt/dietpi_userdata/skrongle
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6
RestrictNamespaces=true
LockPersonality=true
SystemCallArchitectures=native
```

Keep `[Unit]` and `[Install]` as in `deploy/skrongle.service`. If the folder
is on a mount that comes up late, add `RequiresMountsFor=/mnt/dietpi_userdata/skrongle`
under `[Unit]`. `skrongle doctor --service` reads this unit and reports the
database and data paths (`sudo /opt/skrongle/skrongle doctor --service`).
Its "runs as" and "db writable" lines are about whoever runs the command, so
with `sudo` they are root's. `sudo -u skrongle` makes them the service's, but
only on a system with D-Bus: a fresh DietPi has none, and there `systemctl`
answers root alone, so `--service` fails for any other user. CI starts the binary as
a non-root user with the database in a temp folder and checks `/healthz`,
the owner link and the key file's mode (`scripts/non-root-start.sh`).

## Configuration

Every setting is an environment variable. Put extra ones in
`/etc/skrongle/env`, one `NAME=value` per line, and restart.
[`deploy/env.example`](https://github.com/alexbeeken/skrongle-mini/blob/main/deploy/env.example)
lists the ones you might set, commented out. Every name
starts `SKRONGLE_`; see [Renamed settings](#renamed-settings) if yours start `LEARN_`.

Where a default says "unit", `skrongle.service` sets it.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:7576` | Address and port to listen on; a bare port (`8081`) means every interface. |
| `SKRONGLE_SIGNUP` | `open` (unit: `invite`) | Who may make an account: `open`, `invite` or `closed`; see [Accounts and AI keys](#accounts-and-ai-keys). |
| `SKRONGLE_PROXY_AUTH_HEADER`, `SKRONGLE_PROXY_AUTH_FROM` | — | Let the proxy in front sign people in; see [Signing in at the proxy](#signing-in-at-the-proxy). Both or neither. |
| `SKRONGLE_DATA_DIR` | `data` | Folder with `courses/`. |
| `SKRONGLE_DB` | `learn.db` | SQLite database path. The secret key is created next to it as `<db>.key`. |
| `SKRONGLE_SECRET_KEY` | (from `<db>.key`) | 64 hex characters; encrypts saved AI keys. Set it to keep the secret out of the data folder. |
| `SKRONGLE_NEW_SECRET_KEY` | off | `1` only when the secret key is lost for good: see [Backup and restore](#backup-and-restore). |
| `SKRONGLE_OPENAI_BASE_URL` | `https://api.openai.com` | Point the OpenAI provider at any OpenAI-compatible server, e.g. Ollama on another machine. |
| `SKRONGLE_ALLOW_PRIVATE_AI_URLS` | off | `1` lets learners type a base URL on your own network (e.g. `http://192.168.1.20:11434`) into the AI forms. Off, the server refuses to call private, loopback and link-local addresses for them, and ports other than 443 and 80. |
| `SKRONGLE_AI_JOB_BUDGET_SECS` | `3600` | How long one AI generation job may run in all. Raise it for a slow self-hosted model. |
| `SKRONGLE_LOG` | `info` | One level: `error`, `warn`, `info`, `debug` or `trace`. Per-module filters like `skrongle=debug` are not supported; an unrecognised value logs one warning and runs at `info`. |
| `SKRONGLE_SECURE_COOKIES` | off | `1` when the site is served over HTTPS (behind a TLS proxy). |
| `SKRONGLE_PUBLIC_URL` | — | The address people use, e.g. `https://learn.example.org`. The owner link and invitation links start with it. Apps that connect from the cloud (OAuth) need it: without it their sign-in routes answer 404. |
| `SKRONGLE_BASE_PATH` | *(empty)* | A URL prefix when a proxy serves the app under a path, e.g. `/learn`. |
| `SKRONGLE_MIN_AGE` | `14` | The minimum age the registration checkbox states. The owner's own account is not asked. |
| `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` | `30` | An AI course whose generation failed is deleted, with the notes it was given, after this many days without a resume or edit. `0` keeps them. |
| `SKRONGLE_RETAIN_HISTORY_DAYS` | `730` | Answers and finished game rounds older than this many days are deleted, at start and hourly. `0` keeps them. |
| `SKRONGLE_KEEP_NOTES` | off | `1` or `true` keeps an AI course's notes after it is ready, so a later revision sees the original material. Off, they are removed once it is ready. |
| `SKRONGLE_COURSES_PER_USER` | `50` | Courses one account may make. "Create a course" says so when an account has them all. |
| `SKRONGLE_COURSE_BYTES` | `8388608` (8 MB) | Total size of one course's files (the Spanish course is 3.7 MB). A save, upload or lesson-table change past it is refused and stores nothing. |
| `SKRONGLE_COURSE_BYTES_PER_USER` | `33554432` (32 MB) | Total size of the files of all the courses one account owns. Undo copies are not counted. |

[Architecture](architecture) lists the rest (developer
settings).

### Renamed settings

Settings used to start `LEARN_` (and before that `SPANISH_`). They now
start `SKRONGLE_`: `LEARN_DB` is `SKRONGLE_DB`, `LEARN_SIGNUP` is
`SKRONGLE_SIGNUP`, and so on. The rest of the name is the same.

Three settings are the exception: `SKRONGLE_SIGNUP`, `SKRONGLE_PROXY_AUTH_HEADER`
and `SKRONGLE_PROXY_AUTH_FROM` are read under the new name only. An old
`LEARN_` or `SPANISH_` line for one of them does nothing, though the startup
warning still lists it.

This release still reads the old names, so an upgrade keeps working. At
startup the log has one warning listing each old name it found and its new
name. If both spellings are set, the `SKRONGLE_` one wins and the warning
says so. The next release stops reading the old names, so rename them now:

```sh
sudo sed -i 's/^LEARN_/SKRONGLE_/; s/^SPANISH_/SKRONGLE_/' /etc/skrongle/env
sudo sed -i 's/=LEARN_/=SKRONGLE_/' /etc/systemd/system/skrongle.service
sudo systemctl daemon-reload && sudo systemctl restart skrongle
```

Or copy the new `skrongle.service` from the release over yours, if you
have not edited it.

## No e-mail

Skrongle mini sends no e-mail and has no setting for it (#633, #10). An
e-mail address is only what an account signs in with.

| Feature | How it works |
|---|---|
| Registration | Registering signs the account in at once; nothing confirms the address. |
| A forgotten password | No reset link: you give the account a temporary password (below). |
| Two-factor sign-in | The authenticator app. |
| Sharing a course | The Share dialog makes an invitation link and shows it with a Copy button, once (links are stored only hashed); pass it on yourself. |

## Accounts and AI keys

`SKRONGLE_SIGNUP` decides who may make an account. The systemd unit sets it
to `invite`:

1. Start the server. While it has no account, it makes a one-time
   **owner link** and writes it to its log too. Print one any time:

   ```sh
   sudo /opt/skrongle/skrongle owner-link --service
   ```

   Links are stored hashed, so an earlier one cannot be read back: every
   run (and `--new`, the same thing) makes a fresh link and the old one,
   the log's included, stops working. Once an account exists it prints no
   link and says so. Under `SKRONGLE_SIGNUP=open` there is no owner link:
   the first account to register is the owner. Without systemd, use
   `--env-file FILE` in place of `--service`.

   Open that link and register: the account is yours, the owner's. Its account menu (the picture, top right) has a
   **Manage Users** page. Without the
   link, `/register` only says the server has no owner yet, so someone
   else on your network can't make the first account before you. Every
   restart makes a new link (the old one stops working) until the owner's
   account exists. The link starts with `SKRONGLE_PUBLIC_URL` when set, else
   `http://<hostname>.local:<port>`; if that address doesn't open, keep the
   part from `/register` and put the address you use in front.
2. For everyone else, open **Manage Users** and click
   **Make an invitation link**. Copy the link (it is shown only then) and
   send it to one person. It works once, for 7 days; **Revoke** takes back
   one nobody has used yet.
3. The person opens the link and registers with a password. Without a link, `/register` only says that sign-ups are
   closed and to ask whoever runs the server.

The same page lists the open invitations and the accounts (e-mail address,
when it was made, last sign-in; 25 to a page).

A Skrongle mini holds at most **50 accounts**: a class, or a household with
room to spare. This is a fixed number, not a setting, in every sign-up mode. Past
it the register form, invitation links and the proxy sign-in's account
creation all say "This Skrongle has all the accounts it can hold", and the
Manage Users page shows "50 of 50 accounts" and hides the invitation form. The
Pi Zero 2 W has little memory, and one known size is what lets the rest of
the server (sessions, rate limits, connections) bound what it keeps. The
other values of the setting:

| `SKRONGLE_SIGNUP` | Who may register |
|---|---|
| `open` (unset) | Anyone who can reach the server. The first account made is the owner's. |
| `invite` | The first account (the owner's, through the owner link in the log), then only through an invitation link. |
| `closed` | The first account (the owner's, through the owner link in the log), then nobody. |

Any other value stops the server at
startup with a message naming it; the log says which mode is on. Existing
accounts always sign in. A server that already has
accounts when you turn `invite` on gets no new owner: make one an owner
with `make-owner` (below), or with **Make owner** on the Manage Users page.

Someone forgot their password, or lost the phone their two-factor codes go
to? On the **Manage Users** page, each account has **Temporary
password** (a new password, shown to you once, that they must change at
their next sign-in), **Turn off two-factor** and **Make owner** (another
person who may do all this). No message is sent: give
them the password yourself.

Each account's row also has **Permissions**: choose **Student** (may do
nothing extra), **Teacher** (everything but Superuser; what a new account
gets) or **Custom**, which shows a switch for each of the seven permissions.
An account you give **Superuser** gets **Manage Users** in its menu and sees
the accounts there, to set other people's permissions; it never sees the
invitation links, the AI keys or the three actions above, and it cannot
change an owner's permissions or its own. Owners always have every
permission. An account without a permission is not shown that feature and
is refused if it asks for it anyway.

For a whole class, tick the box at the start of each account's row (or the
one in the table's heading for all of them), choose a preset in the bar that
appears above the table and click **Apply to selected**. After you confirm,
every ticked account gets that preset; the page says how many were changed.
Owners and your own account have no box and are never changed this way.

### If you lock yourself out

The binary has commands that change the database directly, for when you
can't sign in yourself. They work with the server running or stopped. Run
them as root, with the database the server uses:

```sh
sudo SKRONGLE_DB=/var/lib/skrongle/learn.db /opt/skrongle/skrongle reset-password you@example.org
sudo SKRONGLE_DB=/var/lib/skrongle/learn.db /opt/skrongle/skrongle disable-2fa you@example.org
sudo SKRONGLE_DB=/var/lib/skrongle/learn.db /opt/skrongle/skrongle make-owner you@example.org
```

| Command | What it does |
|---|---|
| `reset-password <email>` | Prints a temporary password and signs the account out everywhere. Sign in with it; the app then asks for a new one before anything else. |
| `disable-2fa <email>` | Turns two-factor sign-in off: the password alone signs in. Turn it on again in the profile. |
| `make-owner <email>` | Makes the account an owner: its account menu gets the **Manage Users** page. |
| `help` | Lists the commands. |

An address no account has changes nothing and says so. Any other first
argument prints the list and exits with status 2. Built from the repo, the
binary is `target/release/spanish-learning` (`./spanish-learning
reset-password …`); a release names it `skrongle`.

Keep the server on your own network or behind Tailscale (below) anyway.
The AI features (tutor, course generator) use the server's keys, which
you, the owner, save under *Manage Users → AI features* (Anthropic, OpenAI,
Google, DeepSeek, Mistral, xAI or OpenRouter); every account uses them,
billed to you, and nobody else sees them. To use a model you run yourself,
give the *Ollama* row your server's base URL (`http://192.168.1.20:11434`,
any OpenAI-compatible server) and the model it runs (`llama3.1`); it needs
no key, and a Pi Zero 2 W is far too small to run one itself. The same section has *Hide AI features for everyone*, which takes
the AI features away from every account. With it off, each account has the AI
its permissions give it: *Use the AI tutor* and *Use AI in courses*, set per
account in the *Permissions* dialog on the same page (an account that does not
see the tutor lacks the first one). The secret key encrypts saved
keys, so a database without its `.key` file can't use them.

Below it is *Let apps connect*, off until you turn it on. On, an account
that may create courses can make a token on its profile and give it to an
AI assistant or another app, which can then work on that account's
courses; you see every connection there and can revoke any of them. Off,
no app can connect and every token stops working; turning it on again
brings them back. It needs none of the AI keys above
([features](features#connected-apps)). The card also counts the apps that
registered and the tokens handed out in the last day, so a flood or a
stolen token shows; what can go wrong and what stops it is on
[Security: connected apps](security-apps).

With `SKRONGLE_PUBLIC_URL` set, an app in a vendor's cloud (claude.ai,
ChatGPT) can also sign in with OAuth instead of being given a token: the
person sees a page naming the app and clicks **Allow**. For a cloud app
the server needs a public HTTPS address; on a home network give the app a
token instead ([api](api#oauth)).

### Connected apps behind a path

When Skrongle is at a path such as `https://example.org/skrongle`
(`SKRONGLE_BASE_PATH=/skrongle`), its OAuth issuer is
`https://example.org/skrongle`. The MCP authorization spec (draft of
2026, "Authorization Server Discovery") has a client try, for an issuer
with a path, in this order:

1. `https://example.org/.well-known/oauth-authorization-server/skrongle`
2. `https://example.org/.well-known/openid-configuration/skrongle`
3. `https://example.org/skrongle/.well-known/openid-configuration`

and find the MCP server's own document from the `resource_metadata` in
`/mcp`'s 401 (Skrongle names
`https://example.org/skrongle/.well-known/oauth-protected-resource/mcp`),
else at `https://example.org/.well-known/oauth-protected-resource/skrongle/mcp`
and then `https://example.org/.well-known/oauth-protected-resource`.

Skrongle answers everything under its path: the third address above, and
`/skrongle/.well-known/oauth-authorization-server` and
`/skrongle/.well-known/oauth-protected-resource/mcp`. A client that follows
the spec finds it with no change to the proxy. For clients that only try
the addresses at the site's root, pass those two on in nginx:

```nginx
location = /.well-known/oauth-authorization-server/skrongle { proxy_pass http://127.0.0.1:7576/skrongle/.well-known/oauth-authorization-server; }
location = /.well-known/oauth-protected-resource/skrongle/mcp { proxy_pass http://127.0.0.1:7576/skrongle/.well-known/oauth-protected-resource/mcp; }
```

(Caddy: `rewrite /.well-known/oauth-authorization-server/skrongle /skrongle/.well-known/oauth-authorization-server`
and the same for the second, before the `reverse_proxy`.) At the site's
root, with no base path, nothing is needed.

## Reaching it from outside

Don't forward a port on the router. Install [Tailscale](https://tailscale.com)
on the server (`curl -fsSL https://tailscale.com/install.sh | sh`, then
`sudo tailscale up`) and on each phone or laptop; the server is then at
`http://<hostname>:7576` from any of them.

For a public site, put a TLS proxy (nginx, Caddy) in front, set
`SKRONGLE_BIND=127.0.0.1:7576`, `SKRONGLE_SECURE_COOKIES=1` and `SKRONGLE_PUBLIC_URL`.

Anything that faces the internet should sit behind such a proxy, even
without HTTPS. The app has only coarse protection. It closes a connection
that takes more than 15 seconds to send its headers or stalls 30 seconds in
its body, takes at most 128 connections (more clients wait until one closes), and refuses
bodies over 64 KB outside the course editor. A proxy buffers slow clients
and can limit them per address.

### Signing in at the proxy

If a proxy in front already knows who people are (Authelia, Authentik,
Tailscale, Cloudflare Access), Skrongle can take its word and skip its own
password form. Set both variables:

| Variable | Meaning |
|---|---|
| `SKRONGLE_PROXY_AUTH_HEADER` | The header the proxy puts the person's **e-mail address** in, e.g. `Remote-Email` or `Tailscale-User-Login`. |
| `SKRONGLE_PROXY_AUTH_FROM` | The proxy's own addresses, comma-separated, each an address or a range: `127.0.0.1`, `10.0.0.5, fd00::/8`. |

One without the other stops the server at startup. The log says which
header and addresses are trusted.

What happens then:

- A request that comes **straight from one of those addresses** (the
  connection itself, not `X-Forwarded-For`) and carries the header is
  signed in as the account with that e-mail address (any capitals). There
  is no two-factor step: the proxy did that. If the browser was signed in
  as someone else, that session ends.
- No account with that address: `SKRONGLE_SIGNUP` decides. `open` makes one
  with no password, its address already confirmed. `invite` and `closed` show the "sign-ups are closed" page
  (on an empty server the first one is still let in, as the owner).
  Invitation links don't help here, since `/register` has no form. When the
  proxy only lets your own people through, set `SKRONGLE_SIGNUP=open`: the
  proxy already decides who gets in.
- `/login` and `/register` only say that the proxy signs you in. `/logout`
  ends the session and says the next page through the proxy signs you in
  again.
- The header from **any other address** is ignored and logged (once an hour
  per address). So the proxy must be the only way in: bind Skrongle to
  `127.0.0.1` (or firewall it) so nobody reaches it around the proxy, and
  make the proxy **overwrite** the header, never pass on one the browser
  sent.

**Run the proxy on the same machine, or reach Skrongle over a VPN.** The
address check proves only which machine a connection came from. If the
proxy and Skrongle talk over a plain network such as home Wi-Fi, a LAN or
the internet, anyone on that network can pretend to be the proxy and sign in
as any account. Put the proxy on the Skrongle machine and connect it to
`127.0.0.1`, where the traffic never leaves the machine. If the proxy runs
elsewhere, connect it to Skrongle over an encrypted VPN such as Tailscale or
WireGuard, and set `SKRONGLE_PROXY_AUTH_FROM` to the proxy's VPN address. We
don't recommend any other setup.

**Authelia with nginx** (`auth_request`). Authelia's `Remote-User` is a user
name; use `Remote-Email` (or `Remote-User` only if your user names are
e-mail addresses). `SKRONGLE_BIND=127.0.0.1:7576`,
`SKRONGLE_PROXY_AUTH_HEADER=Remote-Email`, `SKRONGLE_PROXY_AUTH_FROM=127.0.0.1`:

```nginx
location /internal/authelia/authz {
    internal;
    proxy_pass http://127.0.0.1:9091/api/authz/auth-request;
    proxy_set_header X-Original-Method $request_method;
    proxy_set_header X-Original-URL $scheme://$http_host$request_uri;
    proxy_set_header X-Forwarded-For $remote_addr;
    proxy_set_header Content-Length "";
    proxy_pass_request_body off;
}

location / {
    auth_request /internal/authelia/authz;
    auth_request_set $email $upstream_http_remote_email;
    auth_request_set $redirection_url $upstream_http_location;
    error_page 401 =302 $redirection_url;
    # Always set: replaces any Remote-Email the browser sent.
    proxy_set_header Remote-Email $email;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_pass http://127.0.0.1:7576;
}
```

**Tailscale Serve.** Serve puts the signed-in tailnet user's login in
`Tailscale-User-Login` (and drops one the browser sent). It is an e-mail
address for most identity providers; a GitHub login looks like
`name@github`, which is not one, and gets no account. With
`SKRONGLE_BIND=127.0.0.1:7576`, `SKRONGLE_PROXY_AUTH_HEADER=Tailscale-User-Login`
and `SKRONGLE_PROXY_AUTH_FROM=127.0.0.1`:

```sh
sudo tailscale serve --bg 7576
```

The server is then at `https://<hostname>.<tailnet>.ts.net`, and everyone on
the tailnet is signed in as themselves. Requests from tagged devices carry
no user and get the usual "the proxy signs you in" page. Trusting
`127.0.0.1` trusts every program on the server too; that is fine on a box
that only runs Skrongle.

A client that keeps no cookies (a script, `curl`) gets a new session on every
request, so an account keeps only its newest 4 sessions (the oldest ends when
a fifth starts); sessions expire after 30 days and are cleaned up hourly.

## Updating

One line:

```sh
sudo /opt/skrongle/update.sh
```

It finds the latest release, downloads the tarball for this machine and
checks its checksum, stops the service, replaces the binary and `data/`, and
starts the service again. It does not copy the database. Give it a version
to install that one instead (`sudo /opt/skrongle/update.sh 0.2.0`), which is
also how to go back: when the older version has an older database schema,
the installed binary first takes the schema back to it
(`skrongle migrate`; [what that keeps](packaging#upgrade)).
Read the release's section in
[CHANGELOG.md](https://github.com/alexbeeken/skrongle-mini/blob/main/CHANGELOG.md)
first: it says what to do and what to back up. The version numbers' rules
are in the README, "Versions".
Database migrations run when the new server starts. A database from a
build before the 1.0 schema is not upgraded: the server stops with one line
naming the file ("this database predates the 1.0 schema; start from a fresh
one"); move it aside and start with an empty database. That exit status is
78, the same one a database a newer build upgraded gets (the message says to
deploy that version or restore a backup). The unit does not restart on 78,
since a restart cannot help. If the release notes say the unit changed, copy
`skrongle.service` to `/etc/systemd/system/` again and run
`sudo systemctl daemon-reload` (the script tells you when the two differ).

The default port is now 7576, not 8080 (8080 is taken on many home servers).
An existing install stays on 8080: `update.sh` doesn't replace the unit in
`/etc/systemd/system`, and the old unit sets 8080. Copying the new
`skrongle.service` there moves the service to 7576. To keep 8080 instead,
add `SKRONGLE_BIND=0.0.0.0:8080` to `/etc/skrongle/env` first. If you do
move, also change the port in `SKRONGLE_PUBLIC_URL` in `/etc/skrongle/env`
(the setup guide wrote it with `:8080`, and invitation links start with it),
and in anything else that points at 8080: a bookmark,
`tailscale serve --bg 8080` (run `sudo tailscale serve reset`, then
`sudo tailscale serve --bg 7576`), a proxy.

By hand, the same thing:

```sh
V=0.2.0
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz
curl -LO https://github.com/alexbeeken/skrongle-mini/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz.sha256
sha256sum -c skrongle-$V-$(uname -m).tar.gz.sha256
sudo systemctl stop skrongle
sudo rm -rf /opt/skrongle/data
sudo tar -xzf skrongle-$V-$(uname -m).tar.gz -C /opt/skrongle
sudo systemctl start skrongle
```

## Backup and restore

The whole state is two files in `/var/lib/skrongle`:

| File | What it is |
|---|---|
| `learn.db` | The SQLite database: accounts, progress, courses made in the app. (`learn.db-wal` and `learn.db-shm` next to it belong to it while the server runs.) |
| `learn.db.key` | The secret key that encrypts the AI keys learners saved. Without it those keys can't be read. If you set `SKRONGLE_SECRET_KEY` instead, keep a copy of that value. |

A deleted account stays in your backups
and in the journal until they expire. How long that is on your
server is yours to set and keep. Keep the two together, and somewhere other
than the SD card. Stop the service while copying so the database is
consistent. The backup holds every account and the key that decrypts stored
API keys, so it is created readable by you only (`umask 077`; an old copy is
removed first, since umask only applies to new files):

```sh
sudo systemctl stop skrongle
(umask 077 && rm -f skrongle-backup.tar.gz && sudo tar -czf - -C /var/lib/skrongle . > skrongle-backup.tar.gz)
sudo systemctl start skrongle
```

To back up without stopping, let SQLite write a consistent copy (needs the
`sqlite3` package), then add the key:

```sh
sudo sqlite3 /var/lib/skrongle/learn.db ".backup /root/learn.db"
sudo cp /var/lib/skrongle/learn.db.key /root/
```

To restore, on the same machine or a fresh install:

```sh
sudo systemctl stop skrongle
sudo rm -f /var/lib/skrongle/learn.db-wal /var/lib/skrongle/learn.db-shm
sudo tar -xzf skrongle-backup-2026-10-17.tar.gz -C /var/lib/skrongle
sudo systemctl start skrongle
```

(`/var/lib/skrongle` belongs to the user systemd creates for the service;
systemd fixes the ownership of the restored files when it starts it.)

If the database has saved AI keys but its key is missing, or the key there
cannot read them (a `SKRONGLE_SECRET_KEY` from another install, a
`learn.db.key` from a different backup), the server won't start and its log
says so; `skrongle doctor` shows it as a PROBLEM on the `secret key` line.
Restore `learn.db.key` from the same backup as the database. If the key is
lost for good, add `SKRONGLE_NEW_SECRET_KEY=1` to
`/etc/skrongle/env`, restart once, then remove the line: a new key is made,
the saved AI keys are deleted and learners enter theirs again. An
authenticator app set up for two-factor sign-in was sealed with the old key
too: it is turned off, and those learners sign in with their password and
set it up again. Everything else is unaffected.

## Uninstalling

The release has a script that removes the install, run as root:

```sh
sudo /opt/skrongle/uninstall.sh           # keeps the data
sudo /opt/skrongle/uninstall.sh --purge   # removes the data too
```

Without `--purge` it removes the service and the program, and keeps the data
(below):

- stops and disables the `skrongle` unit, and deletes its unit file and
  drop-ins in `/etc/systemd/system/`;
- deletes `/opt/skrongle` (the program, `update.sh` and the built-in courses);
- deletes the `skrongle` system user, but only if your install made one (a
  DietPi-style install may have; the release unit does not).

It does not remove the packages you installed, Tailscale or a proxy you set
up. Running it again is safe: what is already gone is skipped.

The same by hand:

```sh
sudo systemctl disable --now skrongle
sudo rm /etc/systemd/system/skrongle.service
sudo systemctl daemon-reload
sudo rm -rf /opt/skrongle
getent passwd skrongle && sudo userdel skrongle   # only if the user is there
```

### The data

The database folder is your data, so the script keeps it unless you pass
`--purge`. It is `/var/lib/skrongle`. The service's user is created by
systemd, so the real folder is `/var/lib/private/skrongle`, with
`/var/lib/skrongle` linked to it; the script handles both. An install made
by `dietpi-install.sh` keeps it in `/mnt/dietpi_userdata/skrongle`, which the
script handles too. The folder holds:

- `learn.db`: the accounts, progress, courses made in the app, and the AI
  keys people saved (encrypted);
- `learn.db.key`: the key that decrypts those AI keys;
- `learn.db.before-<version>`: copies an `update.sh` up to 0.3.0 made before
  each update. Later ones make none; delete these when you no longer want them.

Removing that folder is permanent: nothing brings the accounts back after
it. [Back it up](#backup-and-restore) first. Without `--purge`, installing
again with the same unit finds the accounts and progress as they were.

`/etc/skrongle` is kept too without `--purge`, because its env file can hold
`SKRONGLE_SECRET_KEY`, which the kept database needs to read the saved AI
keys. `--purge` removes that folder as well. To remove the data by hand,
after the steps above: `sudo rm -rf /var/lib/skrongle /var/lib/private/skrongle /etc/skrongle`
(on DietPi, `/mnt/dietpi_userdata/skrongle` in place of the first two).

To use Skrongle again later, follow [Install](#install).

## Troubleshooting

| Symptom | Check |
|---|---|
| `ssh: Could not resolve hostname skrongle.local` | The Pi isn't on the Wi-Fi yet (wait a few minutes after the first boot), or the Wi-Fi details in Imager were wrong. Flash the card again. |
| The page doesn't load from a phone | Use the IP address instead of `.local`; make sure the phone is on the same Wi-Fi; `systemctl status skrongle`. |
| `port 7576 is already in use` in the log | Something else uses port 7576: set `SKRONGLE_BIND=0.0.0.0:8081` in `/etc/skrongle/env`. |
| `cannot write the database` in the log | The folder isn't writable by the service: with the unit above it is `/var/lib/skrongle`; if you changed `SKRONGLE_DB`, the new folder must belong to the service user. |
| `the secret key is missing` in the log | See [Backup and restore](#backup-and-restore): restore `learn.db.key`. |
| Service keeps restarting | `journalctl -u skrongle -n 50`. A course validation error names the file and field. |
| Locked out of your own account | [If you lock yourself out](#if-you-lock-yourself-out): `reset-password`, `disable-2fa`. |
| Anything else | Run [`skrongle doctor`](#diagnostics-skrongle-doctor) and read the [FAQ](self-hosting-faq). |
