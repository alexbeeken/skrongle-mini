# Self-hosting

The technical version of the [Host it yourself](/self-host) guide. The
guide walks a first-timer through a Raspberry Pi Zero 2 W; this page has the
details behind each step, for any 64-bit Linux machine. Stuck? The
[self-hosting FAQ](self-hosting-faq) answers the first questions people ask.

Self-hosting is free. Help is best-effort, in
[GitHub Discussions](https://github.com/alexbeeken/pooki/discussions); the
hosted site is the supported option.

## Skrongle mini

The release you install is **Skrongle mini**: the same binary as the hosted
site, with `skrongle.service` set for a handful of people who know you and
no outside services. What it leaves out:

- **No e-mail.** Links that would be mailed are shown in
  the page ([Without e-mail](#without-e-mail)).
- **No plans or payments.** Everyone has everything.
- **Sign-up by invitation.** The first account is yours, the owner's;
  everyone else needs a link you make (`SKRONGLE_SIGNUP=invite`,
  [Accounts and AI keys](#accounts-and-ai-keys)).
- **Your own Terms.** `SKRONGLE_TERMS=/etc/skrongle/terms.md`
  ([Your own Terms](#your-own-terms)).
- **Background work in process.** The mail and text outboxes and the
  background jobs are rows in SQLite, run by threads in the server: no
  Redis or queue service.
- **SQLite only.** One database file in `/var/lib/skrongle`, no database
  server.

`SKRONGLE_MINI=1` is what makes a server a mini (`skrongle.service` sets
it). The startup log line and `/healthz` then say `skrongle mini <version>`
(`skrongle <version>` on the hosted site).

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

## What a release contains

Each [GitHub release](https://github.com/alexbeeken/pooki/releases) has one
tarball per architecture, named `skrongle-<version>-<arch>.tar.gz`, where
`<arch>` is what `uname -m` prints:

| File | For |
|---|---|
| `skrongle-<version>-aarch64.tar.gz` | 64-bit ARM: Raspberry Pi Zero 2 W, 3, 4, 5 on the 64-bit OS |
| `skrongle-<version>-x86_64.tar.gz` | PCs, mini PCs, most VPSs |

Each comes with a `.sha256` file. A tarball unpacks (flat, no top folder) to:

| Path | What it is |
|---|---|
| `skrongle` | The server: one binary. Templates, CSS, scripts and these docs are compiled in. |
| `data/courses/`<!-- themes -->, `data/themes/`<!-- /themes --> | The built-in courses<!-- themes --> and themes<!-- /themes -->, read from disk when the server starts (`SKRONGLE_DATA_DIR`). |
| `skrongle.service` | The systemd unit below (`deploy/skrongle.service` in the repo). |
| `terms.example.md` | A template for your own Terms of Service (`deploy/terms.example.md`); see [Your own Terms](#your-own-terms). |
| `update.sh` | Updates the install in place: see [Updating](#updating). |
| `VERSION` | The version, one line. |

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

Pick the version from the [releases page](https://github.com/alexbeeken/pooki/releases)
(the commands below use `0.2.0`; `uname -m` picks the architecture):

```sh
V=0.2.0
curl -LO https://github.com/alexbeeken/pooki/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz
curl -LO https://github.com/alexbeeken/pooki/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz.sha256
sha256sum -c skrongle-$V-$(uname -m).tar.gz.sha256
sudo mkdir -p /opt/skrongle
sudo tar -xzf skrongle-$V-$(uname -m).tar.gz -C /opt/skrongle
sudo mkdir -p /etc/skrongle
sudo cp -n /opt/skrongle/terms.example.md /etc/skrongle/terms.md
sudo cp /opt/skrongle/skrongle.service /etc/systemd/system/
sudo systemctl enable --now skrongle
```

Then open `http://<hostname>.local:8080`, or `http://<ip>:8080` where `<ip>`
is the first address `hostname -I` prints.

To try it without systemd, run it from its folder:
`cd /opt/skrongle && SKRONGLE_DB=$HOME/learn.db ./skrongle`.

## Diagnostics: `skrongle doctor`

`skrongle doctor` prints one screen about the install, to read yourself or
paste into a bug report. It shows the version, OS and architecture, the
device model, the listen address and whether the port is free, the data
folder and how many courses<!-- themes --> and themes<!-- /themes --> it holds, the database path, size
and schema (migration) level, and whether the database folder is writable.
It also shows whether a secret key and subscriptions are
configured, how many AI keys learners have saved, and free disk and memory.
Secrets show as yes/no only, never their values. It only reads. It opens
the database read-only and creates nothing.

It reads the same `SKRONGLE_*` settings as the server. To see what the systemd
service sees, give it the unit's settings and its env file:

```sh
sudo env $(systemctl show skrongle -p Environment --value) /opt/skrongle/skrongle doctor --env-file /etc/skrongle/env
```

A line that starts with `PROBLEM:` says what to fix.

## The systemd unit

`deploy/skrongle.service` holds the Skrongle mini defaults:

- runs `/opt/skrongle/skrongle` as a user systemd creates for it
  (`DynamicUser=yes`), so there is no `useradd` step;
- keeps the database in `/var/lib/skrongle` (`StateDirectory=skrongle`);
- listens on port 8080 on every interface;
- restarts it if it stops (`Restart=on-failure`);
- caps memory at 64 MB (`MemoryMax`) and sandboxes the process
  (`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, …);
- shows your Terms from `/etc/skrongle/terms.md` (`SKRONGLE_TERMS`; it will
  not start without that file, see [Your own Terms](#your-own-terms));
- lets people sign up only with an invitation from you
  (`SKRONGLE_SIGNUP=invite`, see [Accounts and AI keys](#accounts-and-ai-keys));
- makes the server a Skrongle mini (`SKRONGLE_MINI=1`), which turns plans,
  payments and e-mail off (see
  [Accounts and AI keys](#accounts-and-ai-keys));
- reads optional settings from `/etc/skrongle/env`.

Useful commands:

```sh
systemctl status skrongle          # running? since when?
journalctl -u skrongle -f          # the log, live
sudo systemctl restart skrongle    # after changing /etc/skrongle/env
```

## Configuration

Every setting is an environment variable. Put extra ones in
`/etc/skrongle/env`, one `NAME=value` per line, and restart. Every name
starts `SKRONGLE_`; see [Renamed settings](#renamed-settings) if yours start `LEARN_`.

"mini" marks what `skrongle.service` sets for Skrongle mini.

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_MINI` | unset (mini: `1`) | `1`: this server is a Skrongle mini, the self-hosted version. It is what makes the server a mini: without it the server is the hosted site, which refuses to start without e-mail, Google and Stripe. In a mini there are no plans or payments (every account has every feature and the server's AI key is shared, see [Accounts and AI keys](#accounts-and-ai-keys)), and the e-mail, text, Google, Stripe and S3 settings are ignored. Read once at startup. |
| `SKRONGLE_BIND` | `0.0.0.0:8080` | Address and port to listen on; a bare port (`8081`) means every interface. |
| `SKRONGLE_SIGNUP` | `open` (mini: `invite`) | Who may make an account: `open`, `invite` or `closed`; see [Accounts and AI keys](#accounts-and-ai-keys). |
| `SKRONGLE_PROXY_AUTH_HEADER`, `SKRONGLE_PROXY_AUTH_FROM` | — | Let the proxy in front sign people in; see [Signing in at the proxy](#signing-in-at-the-proxy). Both or neither. |
| `SKRONGLE_DATA_DIR` | `data` | Folder with `courses/`<!-- themes --> and `themes/`<!-- /themes -->. |
| `SKRONGLE_DB` | `learn.db` | SQLite database path. The secret key is created next to it as `<db>.key`. |
| `SKRONGLE_SECRET_KEY` | (from `<db>.key`) | 64 hex characters; encrypts saved AI keys. Set it to keep the secret out of the data folder. |
| `SKRONGLE_NEW_SECRET_KEY` | off | `1` only when the secret key is lost for good: see [Backup and restore](#backup-and-restore). |
| `SKRONGLE_OPENAI_BASE_URL` | `https://api.openai.com` | Point the OpenAI provider at any OpenAI-compatible server, e.g. Ollama on another machine. |
| `SKRONGLE_ALLOW_PRIVATE_AI_URLS` | off | `1` lets learners type a base URL on your own network (e.g. `http://192.168.1.20:11434`) into the AI forms. Off, the server refuses to call private, loopback and link-local addresses for them, and ports other than 443 and 80. |
| `SKRONGLE_AI_JOB_BUDGET_SECS` | `3600` | How long one AI generation job may run in all. Raise it for a slow self-hosted model. |
| `SKRONGLE_MAX_CONNECTIONS` | `256` | Open connections the server allows; more clients wait until one closes. Keep it well under the unit's `LimitNOFILE` (1024). |
| `SKRONGLE_LOG` | `info` | `error`, `warn`, `info`, `debug` or `trace`. |
| `SKRONGLE_SECURE_COOKIES` | off | `1` when the site is served over HTTPS (behind a TLS proxy). |
| `SKRONGLE_PUBLIC_URL` | — | The address people use, e.g. `https://learn.example.org`. |
| `SKRONGLE_TERMS` | `builtin` (mini: `/etc/skrongle/terms.md`) | Your Terms of Service: the path of a Markdown file (the unit sets `/etc/skrongle/terms.md`), or `builtin` for the hosted service's terms. See [Your own Terms](#your-own-terms). |
| `SKRONGLE_LEGAL_EMAIL` | — | With `SKRONGLE_TERMS=builtin` only: your copyright / legal contact, shown on `/terms` in place of `[CONTACT EMAIL]` and `[SUPPORT EMAIL]`. |
| `SKRONGLE_MIN_AGE` | `14` | The minimum age the registration checkbox states. |
| `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` | `30` | An AI course whose generation failed is deleted, with the notes it was given, after this many days without a resume or edit. `0` keeps them. |
| `SKRONGLE_COURSES_PER_USER` | `50` (no limit in dev mode) | Courses one account may make. "Create a course" says so when an account has them all. |
| `SKRONGLE_COURSE_BYTES` | `8388608` (8 MB) | Total size of one course's files (the Spanish course is 3.7 MB). A save, upload or lesson-table change past it is refused and stores nothing. |
| `SKRONGLE_COURSE_BYTES_PER_USER` | `33554432` (32 MB) | Total size of the files of all the courses one account owns. Undo copies are not counted. |

[Architecture](architecture) lists the rest (subscriptions, developer
settings).

### Renamed settings

Settings used to start `LEARN_` (and before that `SPANISH_`). They now
start `SKRONGLE_`: `LEARN_DB` is `SKRONGLE_DB`, `LEARN_MAIL_FROM` is
`SKRONGLE_MAIL_FROM`, and so on. The rest of the name is the same.

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

## Your own Terms

Everyone who makes an account on your server accepts its Terms of Service,
at sign-up and again whenever they change. The text is yours: you run the
server, so the hosted service's terms do not fit. The release ships a short
plain-language template, `terms.example.md`. Copy it and put your name in:

```sh
sudo mkdir -p /etc/skrongle
sudo cp -n /opt/skrongle/terms.example.md /etc/skrongle/terms.md
sudo nano /etc/skrongle/terms.md     # fill in every [BRACKET]
sudo systemctl restart skrongle
```

`/terms` then shows that file as it is. Its SHA-256 is the version, so after
any edit (and a restart) every account is asked to accept again at its next
visit, on a plain page with the accept box. There is no summary of what
changed and no e-mail: the hosted site's notices and 30-day e-mail of a
change come from its changelog, which your file does not have. If the file
is missing, unreadable or empty the server refuses to start, and the log
names the path. `SKRONGLE_TERMS=builtin` shows the hosted service's terms
instead. The age box ("I am at least 14") stays either way
(`SKRONGLE_MIN_AGE`).

## Without e-mail

A Skrongle mini sends no e-mail (#633): the log says so
once at start (`e-mail: not configured; features that need it are off`),
and every feature that would send a message
hides itself instead. The e-mail setting of the hosted site
(`SKRONGLE_MAIL_PROVIDERS`) is ignored in a
mini, and so is Stripe; the log
names any that are set.

| Feature | Without e-mail |
|---|---|
| Confirming an address | No link is sent. The e-mail field shows an (i), not the red !, and nothing asks to send it again. |
| Two-factor sign-in | The e-mail method is not offered. |
| Sharing a course | After inviting, the Share dialog shows each address's link with a Copy button, once (links are stored only hashed); pass it on yourself. |
| Download my data | No mail: when the copy is ready, the Data section's **Download my data** button gets it (the button is there with mail too). |

The authenticator app needs no e-mail, so two-factor sign-in still works. An
account whose second step was e-mail (a database moved over from
elsewhere) cannot sign in: the login page says so. Turn two-factor sign-in
off for that account (see [If you lock yourself out](#if-you-lock-yourself-out)).

Dev mode's mailbox and phone are off in a mini too, so `./.dev-start --mini`
shows what a real mini shows.

## Accounts and AI keys

`SKRONGLE_SIGNUP` decides who may make an account. The systemd unit sets it
to `invite`:

1. Start the server. While it has no account, it writes a one-time
   **owner link** to its log. Print it:

   ```sh
   sudo journalctl -u skrongle -g "Owner link" -o cat | tail -n 1
   ```

   Open that link and register: the account is yours, the owner's. It is
   a superuser, so its account menu (the picture, top right) has a
   **Superuser** page. Without the
   link, `/register` only says the server has no owner yet, so someone
   else on your network can't make the first account before you. Every
   restart makes a new link (the old one stops working) until the owner's
   account exists. The link starts with `SKRONGLE_PUBLIC_URL` when set, else
   `http://<hostname>.local:<port>`; if that address doesn't open, keep the
   part from `/register` and put the address you use in front.
2. For everyone else, open **Superuser** and click
   **Make an invitation link**. Copy the link (it is shown only then) and
   send it to one person. It works once, for 7 days; **Revoke** takes back
   one nobody has used yet.
3. The person opens the link and registers with a password. Without a link, `/register` only says that sign-ups are
   closed and to ask whoever runs the server.

The same page lists the open invitations and the accounts (e-mail address,
when it was made, last sign-in; 25 to a page).

A Skrongle mini holds at most **8 accounts**: a household plus a couple of
guests. This is a fixed number, not a setting, in every sign-up mode. Past
it the register form, invitation links and the proxy sign-in's account
creation all say "This Skrongle has all the accounts it can hold", and the
Superuser page shows "8 of 8 accounts" and hides the invitation form. The
Pi Zero 2 W has little memory, and one known size is what lets the rest of
the server (sessions, rate limits, connections) bound what it keeps. The
other values of the setting:

| `SKRONGLE_SIGNUP` | Who may register |
|---|---|
| `open` (unset) | Anyone who can reach the server. The hosted site runs this way. |
| `invite` | The first account (the owner, a superuser, through the owner link in the log), then only through an invitation link. |
| `closed` | The first account (the owner, a superuser, through the owner link in the log), then nobody. |

Any other value stops the server at
startup with a message naming it; the log says which mode is on. Existing
accounts always sign in. A server that already has
accounts when you turn `invite` on gets no new owner: make one a superuser
with `make-owner` (below).

Someone forgot their password, or lost the phone their two-factor codes go
to? On the **Superuser** page, each account has **Temporary
password** (a new password, shown to you once, that they must change at
their next sign-in) and **Turn off two-factor**. No message is sent: give
them the password yourself.

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
| `make-owner <email>` | Makes the account a superuser: its account menu gets the **Superuser** page. |
| `help` | Lists the commands. |

An address no account has changes nothing and says so. Any other first
argument prints the list and exits with status 2. Built from the repo, the
binary is `target/release/spanish-learning` (`./spanish-learning
reset-password …`); a release names it `skrongle`.

Keep the server on your own network or behind Tailscale (below) anyway.
The AI features (tutor, course generator) use a key each learner saves
under *Profile settings → AI API keys* (Anthropic, OpenAI or Google). The
secret key encrypts saved keys, so a database without its `.key` file can't
use them.

Your server has no plans or payments: a Skrongle mini (`SKRONGLE_MINI=1`)
has none, so every account has every feature and the profile
shows no subscription. To let everyone use the tutor and the course
generator without a key of their own, set one key for the whole server in
`/etc/skrongle/env`:

```sh
SKRONGLE_AI_KEY=sk-ant-…          # shared by every account on this server
SKRONGLE_AI_DAILY_CAP=2.00        # dollars a day across everyone (default 10.00)
SKRONGLE_AI_RATE_PER_MINUTE=12    # requests one account may start a minute
```

Everyone shares a server key. There is no monthly allowance per account,
you pay the provider's bill, and the daily cap is the only brake. Past it,
AI on the server's key stops until midnight UTC and the log says `ALERT`;
learners' own keys keep working. Every call is recorded in the
`ai_usage` table of the database.

## Reaching it from outside

Don't forward a port on the router. Install [Tailscale](https://tailscale.com)
on the server (`curl -fsSL https://tailscale.com/install.sh | sh`, then
`sudo tailscale up`) and on each phone or laptop; the server is then at
`http://<hostname>:8080` from any of them.

For a public site, put a TLS proxy (nginx, Caddy) in front, set
`SKRONGLE_BIND=127.0.0.1:8080`, `SKRONGLE_SECURE_COOKIES=1` and `SKRONGLE_PUBLIC_URL`.
`deploy/nginx-spanish.conf` is a working nginx example.

Anything that faces the internet should sit behind such a proxy, even
without HTTPS. The app has only coarse protection. It closes a connection
that takes more than 15 seconds to send its headers or stalls 30 seconds in
its body, takes at most `SKRONGLE_MAX_CONNECTIONS` connections, and refuses
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
  with no password, its address already confirmed; it accepts the Terms on
  its first page. `invite` and `closed` show the "sign-ups are closed" page
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
e-mail addresses). `SKRONGLE_BIND=127.0.0.1:8080`,
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
    proxy_pass http://127.0.0.1:8080;
}
```

**Tailscale Serve.** Serve puts the signed-in tailnet user's login in
`Tailscale-User-Login` (and drops one the browser sent). It is an e-mail
address for most identity providers; a GitHub login looks like
`name@github`, which is not one, and gets no account. With
`SKRONGLE_BIND=127.0.0.1:8080`, `SKRONGLE_PROXY_AUTH_HEADER=Tailscale-User-Login`
and `SKRONGLE_PROXY_AUTH_FROM=127.0.0.1`:

```sh
sudo tailscale serve --bg 8080
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
checks its checksum, stops the service, copies the database aside as
`learn.db.before-<version>`, replaces the binary and `data/`, and starts the
service again. Give it a version to install that one instead
(`sudo /opt/skrongle/update.sh 0.2.0`), which is also how to go back.
Database migrations run when the new server starts. If the release notes
say the unit changed, copy `skrongle.service` to `/etc/systemd/system/`
again and run `sudo systemctl daemon-reload` (the script tells you when the
two differ).

By hand, the same thing:

```sh
V=0.2.0
curl -LO https://github.com/alexbeeken/pooki/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz
curl -LO https://github.com/alexbeeken/pooki/releases/download/v$V/skrongle-$V-$(uname -m).tar.gz.sha256
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

(`learn.db.exports/` beside them holds "Download my data" files for a day at
most; it need not be backed up.) A deleted account stays in your backups
and in the journal until they expire. The hosted site promises its users
31 days at most; on your server that limit is yours to set and keep. Keep the two together, and somewhere other
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

## Releases

For maintainers. A release is a tag that matches the `version` in
`Cargo.toml` (bump it, merge, then tag main; the workflow refuses a tag
that doesn't match). Pushing `v<version>` runs
`.github/workflows/release.yml`, which builds both architectures with
`scripts/build-pi.sh` (cargo-zigbuild, glibc 2.31), packs each with
`scripts/package-release.sh` and attaches the tarballs and checksums to the
GitHub release of that tag (created with generated notes if it doesn't
exist yet). The binary reports the tag's version (`skrongle --version`).

```sh
git tag v0.2.0 origin/main && git push origin v0.2.0
```

*Actions → release → Run workflow* builds the same tarballs as workflow
artifacts without making a release. To check the layout locally:
`scripts/build-pi.sh aarch64 && scripts/package-release.sh 0.0.0 aarch64`.

## Troubleshooting

| Symptom | Check |
|---|---|
| `ssh: Could not resolve hostname skrongle.local` | The Pi isn't on the Wi-Fi yet (wait a few minutes after the first boot), or the Wi-Fi details in Imager were wrong. Flash the card again. |
| The page doesn't load from a phone | Use the IP address instead of `.local`; make sure the phone is on the same Wi-Fi; `systemctl status skrongle`. |
| `port 8080 is already in use` in the log | Something else uses port 8080: set `SKRONGLE_BIND=0.0.0.0:8081` in `/etc/skrongle/env`. |
| `cannot write the database` in the log | The folder isn't writable by the service: with the unit above it is `/var/lib/skrongle`; if you changed `SKRONGLE_DB`, the new folder must belong to the service user. |
| `the secret key is missing` in the log | See [Backup and restore](#backup-and-restore): restore `learn.db.key`. |
| Service keeps restarting | `journalctl -u skrongle -n 50`. A course validation error names the file and field. |
| Locked out of your own account | [If you lock yourself out](#if-you-lock-yourself-out): `reset-password`, `disable-2fa`. |
| Anything else | Run [`skrongle doctor`](#diagnostics-skrongle-doctor) and read the [FAQ](self-hosting-faq). |
