<!--
The Skrongle entry for DietPi-Docs, kept here until DietPi takes the title.
`aSOFTWARE_DOCS` will point at its anchor, #skrongle.

To put it into DietPi-Docs, copy everything from "## Skrongle" to the end into
the category page the maintainers choose, then make four changes so it matches
its neighbours there:

1. Each `###` section becomes a tab: `=== "Access to the web interface"`, with
   the section's text indented four spaces. The section names are already
   DietPi's tab names.
2. Replace the comment under the URL with DietPi's Avahi snippet:
   `--8<---------- "snippet-includes/AvahiDaemon-WebInterface-access_infoblock.md"`
3. Add a screenshot of the course home under the description, as
   `../assets/images/dietpi-software-<category>-skrongle.png`, with the
   `{: width="…" height="…" loading="lazy"}` attributes the other entries use.
4. Replace `<id>` with the software ID DietPi assigns.
-->

# Skrongle on DietPi

This page holds two things: the facts a DietPi maintainer needs to judge the
request, and the entry for the DietPi documentation, which starts at
[Skrongle](#skrongle).

## For DietPi maintainers

| | |
|---|---|
| What it is | A learning web app for a household or a class: lessons, quizzes, short practice sessions, games and flashcards. Spanish and French courses are included, and people can write or upload their own |
| Install | One release tarball per architecture, checked against the SHA-256 published beside it and unpacked to `/opt/skrongle`. [`deploy/dietpi-install.sh`](https://github.com/alexbeeken/skrongle-mini/blob/main/deploy/dietpi-install.sh) is the whole install, in twelve commented steps. Running it again updates |
| Dependencies | `curl`, for the download. It needs no web server, database server or language runtime; SQLite is compiled in |
| Architectures | ARMv6, ARMv7, ARMv8 and x86_64. There is no RISC-V build. On ARMv6 boards (Raspberry Pi 1, Zero, Zero W) sign-in is slow |
| Port | 7576, TCP |
| Service | `skrongle.service`, running as the system user `skrongle`, limited to 64 MB of memory (`MemoryMax`) and sandboxed by systemd |
| Data | `/mnt/dietpi_userdata/skrongle` |
| Settings | `/etc/skrongle/env`, optional |
| Size on a Raspberry Pi Zero 2 W | A binary of about 15 MB. About 16 MB of memory idle and about 21 MB in use |
| Outside services | None needed. The AI features are optional and use provider keys the owner supplies |
| Uninstall | Removes the program, the unit and the user. Data and settings stay unless purged |
| Licence | AGPL-3.0 |

What a release may change for a packager, and what it will not, is in the
[packaging contract](packaging). To try the install on a DietPi system today:

```sh
curl -fsSLO https://raw.githubusercontent.com/alexbeeken/skrongle-mini/main/deploy/dietpi-install.sh
sudo sh dietpi-install.sh
```

`sudo sh dietpi-install.sh --uninstall` removes it again and keeps the data.

## Skrongle

A learning app for a household or a class, with a web interface. It teaches
from courses written as JSON files: lessons, adaptive quizzes, five-minute
practice sessions, games and flashcards. Spanish and
French courses are included. It is one program and one SQLite file, so no web
server or database server is required.

It holds up to 50 accounts, each signed in on up to 4 devices.

### Access to the web interface

The web interface is accessible via TCP port **7576**:

- URL: `http://<your.IP>:7576`

<!-- DietPi's Avahi snippet goes here: reaching it as http://<hostname>.local:7576 -->

### First run setup

Skrongle sends no e-mail, so it cannot send a sign-up link. The first account
is the owner's, and you make it with a one-time owner link:

```sh
sudo /opt/skrongle/skrongle owner-link --service
```

Open the link it prints in a browser. Until you do, `/register` only says
that the server has no owner yet. After that the owner invites each person
from the *Manage Users* page, one link per person, and `/register` stays
closed to everyone else.

### External access

!!! attention "Keep it off the internet"
    Skrongle is built for people who know whoever runs it. Do not forward
    port 7576 from your router. For access away from home, use a VPN such as
    [Tailscale](https://dietpi.com/docs/software/vpn/#tailscale) or
    [WireGuard](https://dietpi.com/docs/software/vpn/#wireguard), and turn on
    two-factor sign-in for the owner under *Profile* > *Two-factor
    authentication*.

### AI features

The AI tutor and course generation are optional. They use your own provider
key: Anthropic, OpenAI, Google, DeepSeek, Mistral, xAI, OpenRouter, or an
Ollama-compatible server on your network. The owner pastes a key on the
*Manage Users* page. Without a key the AI features stay hidden and everything
else works.

### Directories

- Program: `/opt/skrongle`
- Data: `/mnt/dietpi_userdata/skrongle`

The data directory holds `learn.db` (accounts, progress, courses made in the
app, and the saved AI keys, encrypted), its `-wal` and `-shm` files, and
`learn.db.key`, the key that decrypts the saved AI keys. Back up the whole
directory with the service stopped, or use `sqlite3`'s `.backup` for the
database. It must be on a local disk; SQLite does not work safely on an NFS
or SMB mount.

### Configuration

Settings are environment variables in `/etc/skrongle/env`, one `NAME=value`
per line. All are optional. Restart the service after a change:

```sh
sudo systemctl restart skrongle
```

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:7576` | Listen address and port |
| `SKRONGLE_PUBLIC_URL` | (none) | The address people use, for the links it shows |
| `SKRONGLE_SIGNUP` | `invite` | Who may make an account |
| `SKRONGLE_LOG` | `info` | `error`, `warn`, `info`, `debug` or `trace` |

The [self-hosting reference](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md#configuration)
lists the rest.

### Service control

```sh
systemctl status skrongle
sudo systemctl start skrongle
sudo systemctl stop skrongle
sudo systemctl restart skrongle
```

### View logs

```sh
journalctl -u skrongle
```

To check the install:

```sh
sudo /opt/skrongle/skrongle doctor --service
```

It prints the version, the settings in use and the free disk and memory, and
marks anything wrong with a `PROBLEM:` line. It never prints a secret.

### Update

You can update Skrongle by reinstalling it. Your data and settings are
preserved by this:

```sh
dietpi-software reinstall <id>
```

Read the release's section in the
[changelog](https://github.com/alexbeeken/skrongle-mini/blob/main/CHANGELOG.md)
first.

!!! note "Going back to an older version"
    A newer version may upgrade the database, and an older version then
    refuses to open it. Copy the data directory before an update if you may
    want to go back.

### Uninstall

```sh
dietpi-software uninstall <id>
```

This stops the service and removes `/opt/skrongle`, the unit and the
`skrongle` user. `/mnt/dietpi_userdata/skrongle` and `/etc/skrongle` are
kept, so a later install finds the accounts and progress as they were.
Skrongle's own uninstaller removes those as well. Back up first, because it
cannot be undone:

```sh
sudo /opt/skrongle/uninstall.sh --purge
```

***

Official documentation: <https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md>  
Source code: <https://github.com/alexbeeken/skrongle-mini>  
License: [AGPL-3.0](https://github.com/alexbeeken/skrongle-mini/blob/main/LICENSE)
