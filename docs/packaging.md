# Packaging Skrongle

For people who package Skrongle mini for a distribution or an image (DietPi,
a Debian package, an appliance image). It lists what the program reads, writes
and returns, so you do not need the source. The install steps for one person
on one box are in [Self-hosting](self-hosting); this page is the contract.

Skrongle is one binary (templates, CSS, scripts and these docs are compiled
in), a folder of built-in courses, and one SQLite database. It needs no
e-mail, no other service and no database server.

## The recommended shape

| Decision | Value |
|---|---|
| System user | a static `skrongle` user and group, no login shell, no home |
| Program | `/opt/skrongle` (root-owned, read-only to the service) |
| Data folder | `/mnt/dietpi_userdata/skrongle` on DietPi, `/var/lib/skrongle` on plain Debian |
| Settings | `/etc/skrongle/env`, mode 0600, owned by root |
| Unit | `/etc/systemd/system/skrongle.service` with `RestartPreventExitStatus=78` |
| Port | 7576 |

`deploy/dietpi-install.sh` is the worked example of this shape. The release
unit `deploy/skrongle.service` uses `DynamicUser=yes` and
`StateDirectory=skrongle` instead; the static-user variant is in
[Self-hosting](self-hosting), "A static user". Whichever you
use, keep `RestartPreventExitStatus=78` (see [Exit codes](#exit-codes)).

## Files

| What | Where | Notes |
|---|---|---|
| Binary | `/opt/skrongle/skrongle` | One file. `update.sh` keeps the last one as `skrongle.previous`. |
| Built-in courses | `/opt/skrongle/data/courses/` | Read at start. `SKRONGLE_DATA_DIR` points at the folder that holds `courses/`. Replaced on every update. |
| Database | `<data folder>/learn.db` | SQLite, WAL mode: `learn.db-wal` and `learn.db-shm` appear beside it. |
| Secret key | `<data folder>/learn.db.key` | Created on first start, mode 0600, next to the database (`<SKRONGLE_DB>.key`). It decrypts the AI keys saved in the database. Back both up together. |
| Copies from updates | `<data folder>/learn.db.before-<version>` | Made by `update.sh`. |
| Settings | `/etc/skrongle/env` | `NAME=value` lines, read by systemd (`EnvironmentFile=-`). It may hold `SKRONGLE_SECRET_KEY`. |
| Unit | `/etc/systemd/system/skrongle.service` | |
| Release helpers | `/opt/skrongle/update.sh`, `uninstall.sh`, `VERSION` | From the tarball. |

The data folder must be on a local disk. SQLite's locking does not work safely
over NFS or SMB. The service user must be able to write the folder; if not,
the server stops with a message naming the folder and the user. If the folder
is on a mount that comes up late, add `RequiresMountsFor=<data folder>` under
`[Unit]`.

## Environment variables

Every setting is an environment variable named `SKRONGLE_<NAME>`. An empty
value counts as unset. The two older prefixes from the first releases (listed
under "Renamed settings" in `docs/self-hosting.md`) are still read for one more
release: the server logs one warning listing each old name and its new one,
and the `SKRONGLE_` spelling wins when both are set. New packages should write
only `SKRONGLE_`.

Two exceptions: `SKRONGLE_SIGNUP` and `SKRONGLE_PROXY_AUTH_HEADER` /
`SKRONGLE_PROXY_AUTH_FROM` are read under that spelling only.

"Unit" in the default column means the release unit sets it.

| Variable | Default | What it does |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:7576` | `address:port` to listen on. A bare port (`8081`) means every interface. A value that does not parse stops the server (exit 1). Use `127.0.0.1:7576` behind a proxy on the same machine. |
| `SKRONGLE_DATA_DIR` | `data` (unit: `/opt/skrongle/data`) | Folder that holds `courses/`. With none found, the server starts and warns. |
| `SKRONGLE_DB` | `learn.db` (unit: `/var/lib/skrongle/learn.db`) | The SQLite file. The folder must exist and be writable. |
| `SKRONGLE_SECRET_KEY` | read from `<db>.key` | 64 hex characters. Encrypts saved AI keys. Set it to keep the key out of the data folder. |
| `SKRONGLE_NEW_SECRET_KEY` | off | `1` or `true` only when the key is lost for good: makes a new one and deletes the saved keys it cannot read. Without it, the server refuses to start when saved keys exist but the key is missing or opens none of them. Remove it after one start. |
| `SKRONGLE_SIGNUP` | `open` (unit: `invite`) | Who may make an account: `open`, `invite` or `closed` (case does not matter). Anything else stops the server. See [First run](#first-run). |
| `SKRONGLE_PUBLIC_URL` | unset | The address people open, such as `http://skrongle.local:7576`. The owner link and invitation links start with it. Unset, links use the host of the request. |
| `SKRONGLE_SECURE_COOKIES` | off | `1` or `true` when the site is served over HTTPS. The server warns at start if it listens on plain HTTP beyond loopback, or if the public URL is `https://` and this is off. It does not refuse to start. |
| `SKRONGLE_BASE_PATH` | empty | URL prefix when a proxy serves the app under a path, such as `/learn`. |
| `SKRONGLE_LOG` | `info` | One level: `error`, `warn`, `info`, `debug` or `trace`. Anything else logs one warning and runs at `info`. |
| `SKRONGLE_MIN_AGE` | `14` | Age named by the register page's checkbox. A value that is not a whole number stops the server. The owner's own account is not asked. |
| `SKRONGLE_PROXY_AUTH_HEADER`, `SKRONGLE_PROXY_AUTH_FROM` | unset | Sign people in by a reverse proxy: the header carrying the person's e-mail address, and the proxy's addresses (IPs or CIDRs, comma-separated). Both or neither; one alone stops the server. Bind to `127.0.0.1` so nobody reaches the app around the proxy. |
| `SKRONGLE_COURSES_PER_USER` | `50` | Courses one account may make. |
| `SKRONGLE_COURSE_BYTES` | `8388608` (8 MB) | Size of one made course's files. |
| `SKRONGLE_COURSE_BYTES_PER_USER` | `33554432` (32 MB) | Size of all the made courses of one account. |
| `SKRONGLE_RETAIN_HISTORY_DAYS` | `730` | Answers and game rounds older than this are deleted, at start and hourly. `0` keeps them while the account exists. |
| `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` | `30` | A failed AI course is deleted after this many days untouched. `0` keeps them. |
| `SKRONGLE_KEEP_NOTES` | off | `1` or `true` keeps the notes of an AI course after it is ready. |
| `SKRONGLE_AI_JOB_BUDGET_SECS` | `3600` | How long one AI generation job may run in all. |
| `SKRONGLE_AI_TIMEOUT_SECS` | 900 for generation, 120 for a chat turn | How long one model request may take. |
| `SKRONGLE_ALLOW_PRIVATE_AI_URLS` | off | `1` or `true` lets a base URL typed into an AI form point at a private or loopback address and any port. Off, only public addresses on ports 443 and 80 are allowed. |
| `SKRONGLE_OPENAI_BASE_URL` | `https://api.openai.com` | Where the OpenAI provider sends requests: any OpenAI-compatible server. Never checked against the private-address rule. |
| `SKRONGLE_ANTHROPIC_BASE_URL`, `SKRONGLE_GOOGLE_BASE_URL`, `SKRONGLE_DEEPSEEK_BASE_URL`, `SKRONGLE_MISTRAL_BASE_URL`, `SKRONGLE_XAI_BASE_URL`, `SKRONGLE_OPENROUTER_BASE_URL` | the provider's public API | Same for the other providers. Ollama has no variable: its URL is saved on the Manage Users page. |
| `SKRONGLE_AI_PRICES` | built-in table | `model=in/out[/cached],…` in dollars per million tokens, for the cost estimate. A malformed value stops the server. |

Variables for development and tests (`SKRONGLE_DEV`, `SKRONGLE_LLM_*`,
`SKRONGLE_RNG_SEED`, `SKRONGLE_SQL_TRACE`, `SKRONGLE_TEST_COURSE`) only work in
a build with the `dev` feature. A release binary ignores them. `SKRONGLE_VERSION`
is read when the binary is built, not when it runs.

The unit also sets `MALLOC_ARENA_MAX=2` and `MALLOC_TRIM_THRESHOLD_=65536` to
keep memory flat, and limits the service to 64 MB (`MemoryMax`), 32 tasks and
256 open files. Keep them on a small board.

To see the settings the service really ends up with, run
`sudo /opt/skrongle/skrongle doctor --service`. It prints
diagnostics with no secrets and exits 0.

## Port

The default is **7576**. 8080, the usual choice, is already taken on many home
servers (proxies, other dashboards), so a fresh install would fail to bind.
The server binds first; a taken port stops it with a message naming the port
and how to pick another (exit 1).

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Clean shutdown after SIGTERM or Ctrl-C, or a command that finished (`--version`, `doctor`, `owner-link`, ...). |
| 1 | Fatal start-up error: a setting that does not parse, port taken, data folder not writable, secret key problem, courses folder unreadable, or a failed command. The reason is one line on stderr or in the log. |
| 2 | Bad command line: an unknown argument, or an account command (`reset-password`, ...) that could not run. |
| 78 | The database is newer than this binary (a newer release upgraded it), or is from before the 1.0 schema. The server stops before it writes anything. |

Restarting cannot fix 78, so the unit must say `RestartPreventExitStatus=78`.
Otherwise systemd restarts it every few seconds forever. The log line is:

```
fatal: this database was upgraded by a newer Skrongle (schema v5; this binary knows up to v4); deploy that version or restore a backup
```

The older case reads `fatal: this database predates the 1.0 schema; start from a fresh one`.
For a script, `skrongle --schema-version` prints the schema number this
binary migrates to, and `skrongle --version` prints `skrongle <version>`.

## Health check

`GET /healthz` returns `200` with a plain-text body: `ok skrongle mini 0.2.0`
(the word `ok`, then the edition and the version). It needs no sign-in and
does not touch the database; it says the process is up and serving. Match on
the status or on a leading `ok`, not on the whole line.

```sh
curl -fsS http://127.0.0.1:7576/healthz
```

## First run

1. Start the service. On an empty database, with `SKRONGLE_SIGNUP` set to
   `invite` or `closed`, the log has one warning line:
   `no account yet. Owner link, to make your own account (the owner's): <link>`.
   Whoever opens the link first makes the owner's account. Only someone who can
   read the log can claim the server.
2. The link starts with `SKRONGLE_PUBLIC_URL`, or with the bind address when
   that is unset. Set the public URL first if the address people use differs.
3. The server stores only a hash of the link, so a lost link cannot be read
   back. Print a new one (the earlier one stops working), while no account exists:

   ```sh
   sudo /opt/skrongle/skrongle owner-link --service
   ```

   `--service` reads the installed unit and its env file; `--env-file FILE`
   reads a file alone. Use one or the other. The command needs the database to
   exist, so start the server once first. After the owner exists it says so and
   exits 0.
4. Other accounts: the owner makes invitation links on the Manage Users page.
   An owner who is locked out gets back in with
   `skrongle reset-password <email>`; `skrongle make-owner <email>` and
   `skrongle disable-2fa <email>` also exist. They open `SKRONGLE_DB` and work
   with the service running or stopped.

| `SKRONGLE_SIGNUP` | Who can make an account |
|---|---|
| `open` | Anyone who reaches the server. The first account becomes the owner. No owner link is logged. |
| `invite` | Only through an invitation link an owner makes. The first account is the owner's, through the owner link. |
| `closed` | Nobody but that first account. |

The release unit sets `invite`. The default in the program, when the variable
is unset, is `open`: a package that does not set it makes an open server.

## Log

The server writes plain lines to stderr (the journal under systemd):
level, then message, with no target. Lines worth alerting on:

| Line starts with or contains | Level | What it means |
|---|---|---|
| `fatal:` | error | The server is stopping. The text says why. Exit 78 means the database is newer; any other fatal exits 1. |
| `cannot listen on` | fatal | Port taken, no permission, or no such address. |
| `cannot write the database` | fatal | The data folder is missing or not writable by the service user. |
| `settings with an old name, read for this release only` | warn | A setting under one of the two old prefixes is still in use. Rename it (`docs/self-hosting.md`, "Renamed settings"). |
| `no account yet. Owner link` | warn | First run, waiting for the owner. Expected once. Alert if it appears on an install that had accounts (the database path changed). |
| `serving plain HTTP on` | warn | Listening beyond loopback without HTTPS. Expected on a trusted home network. |
| `SKRONGLE_PUBLIC_URL is https:// but SKRONGLE_SECURE_COOKIES is not 1` | warn | Cookies are not marked Secure. |
| `trusts a very wide range` | warn | `SKRONGLE_PROXY_AUTH_FROM` is too wide: anyone in it can sign in as anyone. |
| `no built-in courses under` | warn | `SKRONGLE_DATA_DIR` is wrong; only made courses are offered. |
| `course generation(s) were interrupted by the restart` | warn | A restart cut AI jobs short. Their owners can resume them. |
| `checkpoint at shutdown failed` | warn | The database may not be folded into one file after the stop. |
| `listening on http://` | info | Start-up finished. `shut down cleanly` is the matching stop. |

## Upgrade

Releases are tarballs, so an upgrade is a binary swap:

1. Download `skrongle-<version>-<arch>.tar.gz` and its `.sha256`; check it.
2. Stop the service.
3. Copy the database aside: `learn.db` to `learn.db.before-<version>` (and
   `learn.db-wal` if it is not empty, as `learn.db.before-<version>-wal`).
4. Replace `/opt/skrongle/skrongle`, and delete and replace `/opt/skrongle/data/`.
   Never touch the data folder.
5. Start the service. The new binary migrates the database as it starts.

`/opt/skrongle/update.sh [version]` does all of this and reports whether the
service came up. A clean stop folds the write-ahead log into `learn.db`, so one
file is a complete copy. `update.sh` does not replace the unit; it prints a note
when the shipped `skrongle.service` differs from the installed one.

Migrations only go forward. The database records its schema number
(`PRAGMA user_version`, one per migration). A binary with a smaller number than
the database refuses it: exit 78 before any write. The version number does not
matter, only the schema. So a downgrade after a release that changed the schema
needs the `learn.db.before-<version>` copy put back first. Version rules:
a patch release never changes the unit, the variables or the database layout;
a minor release may add settings and tables; a major release may remove them.
Read the release's section in `CHANGELOG.md` before you upgrade.

## Release assets

Each release (tag `v<version>`) has, for every architecture, a tarball and its
checksum:

```
skrongle-<version>-<arch>.tar.gz
skrongle-<version>-<arch>.tar.gz.sha256
```

at `https://github.com/alexbeeken/skrongle-mini/releases/download/v<version>/`.
`<version>` has no `v`. `<arch>` is what `uname -m` prints:

| `<arch>` | For |
|---|---|
| `aarch64` | 64-bit ARM: Raspberry Pi Zero 2 W, 3, 4, 5 on a 64-bit OS |
| `x86_64` | PCs, mini PCs, most servers |
| `armv7l` | 32-bit ARM: Raspberry Pi 2 and newer on a 32-bit OS |
| `armv6l` | Raspberry Pi 1, Zero and Zero W (one slow core) |

The `.sha256` file is the output of `sha256sum` on the tarball, so
`sha256sum -c skrongle-<version>-<arch>.tar.gz.sha256` run beside it prints
`OK`. The binaries link glibc 2.31, so they run on Debian 11, Ubuntu 20.04,
Raspberry Pi OS Bullseye and anything newer.

The tarball unpacks flat, with no top folder:

| Path | What it is |
|---|---|
| `skrongle` | The server |
| `data/courses/` | Built-in courses |
| `skrongle.service` | The release unit |
| `update.sh` | In-place updater |
| `uninstall.sh` | Remover |
| `VERSION` | The version, one line |

These paths are not removed or renamed without a major version; a release may
add files. Release assets are never deleted or renamed. A broken release gets
a new patch version instead, so a URL a package once pointed at keeps working.

## Uninstall

`sudo /opt/skrongle/uninstall.sh` removes the program and the service and keeps
the data. `--purge` removes the data too, permanently. Running it again is safe.

| Item | Default | With `--purge` |
|---|---|---|
| Service (stopped, disabled), `/etc/systemd/system/skrongle.service` and `skrongle.service.d` | removed | removed |
| `/opt/skrongle` (binary, courses, scripts) | removed | removed |
| `skrongle` system user and group, if a static one exists | removed | removed |
| `/var/lib/skrongle`, `/var/lib/private/skrongle`, and on DietPi `/mnt/dietpi_userdata/skrongle`: database, `learn.db.key`, `learn.db.before-*` | kept | removed |
| `/etc/skrongle` (the env file may hold `SKRONGLE_SECRET_KEY`) | kept | removed |

The script handles only these paths. A package that puts the data folder
anywhere else must remove or keep that folder in its own remove step. The
script prints a note when `SKRONGLE_DB`, in the unit or in `/etc/skrongle/env`,
points outside them, and with `--purge` it then ends by saying that the
database is not removed. It does not remove
packages you installed, or a proxy or Tailscale you set up.
