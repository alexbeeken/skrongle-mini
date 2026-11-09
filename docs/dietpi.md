<!-- The Skrongle section for DietPi-Docs (docs/software/), kept here to copy when it is accepted into dietpi-software; `aSOFTWARE_DOCS` will point at its anchor, #skrongle. -->

# Skrongle on DietPi

This page is written to be copied into the DietPi-Docs repository. It uses
DietPi's page style, so the notes below use `!!! note`.

## Skrongle

**Skrongle** is a learning app you run yourself, for the people at home: a
family, a class, a few friends. It teaches from courses made of JSON files:
lessons, adaptive quizzes, five-minute practice sessions, games, flashcards
and an AI tutor. Spanish and French courses ship with it. You can write
your own course, upload one, or describe one to an AI that writes it.

![Skrongle web interface](https://raw.githubusercontent.com/alexbeeken/skrongle-mini/main/static/icon-192.png "Skrongle web interface")

**Why Skrongle?** It is one binary and one SQLite file. It runs on a
Raspberry Pi Zero 2 W with room to spare, and there is a build for every
Raspberry Pi family. It needs no outside service: no e-mail, no cloud
account. The AI features work only with your own provider keys, and the rest
works without them. It is free software under the AGPL-3.0.

- Web interface: `http://<your.IP>:7576`
- Up to 8 accounts, each with up to 4 signed-in devices
- Runs as the `skrongle` system user, under systemd, with a memory cap

### Installation

```sh
dietpi-software install <id>
```

Or, in the menu: `dietpi-software` > `Search Software` > `Skrongle` > `Install`.

DietPi downloads the release build for your CPU: `aarch64`, `x86_64`,
`armv7l` or `armv6l`. It checks the file against the checksum published with
it. It then unpacks it to `/opt/skrongle` and starts the `skrongle` service.

### Access

The web interface is on port **7576**:

- URL: `http://<your.IP>:7576`

??? info "Use `http://<hostname>.local:7576` instead of the IP"
    See [Avahi-Daemon](https://dietpi.com/docs/software/system_security/#avahi-daemon) to reach it by name.

Skrongle has no e-mail, so it cannot send a sign-up link. The first account
is the owner's, and you make it with a one-time **owner link**:

```sh
sudo /opt/skrongle/skrongle owner-link --service
```

Open the link it prints in a browser. Until you do, `/register` only says
that the server has no owner yet. Afterwards `/register` stays closed to
the public: the owner invites each person from the **Manage Users** page, one
link per person.

!!! note "Keep it off the internet"
    Skrongle is built for people who know whoever runs it. Do not forward
    port 7576 from your router. For access away from home, use a VPN such as
    [Tailscale](https://dietpi.com/docs/software/remote_desktop/#tailscale) or WireGuard. Turn on two-factor
    sign-in for the owner under *Profile* > *Two-factor authentication*.

### AI features (optional)

The AI tutor and course generation use your own provider keys (Anthropic,
OpenAI, Google, DeepSeek, Mistral, xAI, OpenRouter, or an Ollama-compatible
server on your network). The owner pastes a key on the **Manage Users**
page. Without a key, everything else works and the AI features stay hidden.

### Data directory

All data is in a single folder:

- `/mnt/dietpi_userdata/skrongle`

It holds `learn.db` (accounts, progress, courses made in the app, saved AI
keys, encrypted), its `-wal` and `-shm` files, and `learn.db.key`, the key
that decrypts the saved AI keys. Back up the folder whole, with the
service stopped, or use `sqlite3`'s `.backup` for the database. The folder
must be on a local disk, not an NFS or SMB mount.

### Config file

Settings are environment variables, all optional, in:

- `/etc/skrongle/env`

One `NAME=value` per line, then `systemctl restart skrongle`. The ones most
installs touch:

| Variable | Default | Meaning |
|---|---|---|
| `SKRONGLE_BIND` | `0.0.0.0:7576` | Listen address and port |
| `SKRONGLE_PUBLIC_URL` | (none) | The address people use, for the links it shows |
| `SKRONGLE_SIGNUP` | `invite` | Who may make an account |
| `SKRONGLE_LOG` | `info` | `error`, `warn`, `info`, `debug` or `trace` |

The full list is in the
[Self-hosting reference](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md#configuration).

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
journalctl -u skrongle -f
```

To check the install, run `sudo /opt/skrongle/skrongle doctor --service`.
It prints the version, the settings in use and the free disk and memory, and
marks anything wrong with a `PROBLEM:` line. It never prints a secret.

### Update

```sh
dietpi-software reinstall <id>
```

This fetches the newest release and replaces the program in `/opt/skrongle`.
It keeps your data and settings. Read the release's section in the
[changelog](https://github.com/alexbeeken/skrongle-mini/blob/main/CHANGELOG.md)
first: it says what to back up.

!!! note "Going back to an older version"
    A newer version may upgrade the database, and an older version then
    refuses to open it. Before an update, copy the data folder. Updates made by
    Skrongle's own `update.sh` also leave a `learn.db.before-<version>` copy.

### Remove

```sh
dietpi-software uninstall <id>
```

This stops and disables the `skrongle` service and deletes `/opt/skrongle`
and the `skrongle` system user. **Your data stays** unless you purge it:
`/mnt/dietpi_userdata/skrongle` and `/etc/skrongle` are kept, so installing
again finds the accounts and progress as they were. The release also ships
`sudo /opt/skrongle/uninstall.sh --purge`, which removes the data and
`/etc/skrongle` too. That cannot be undone, so back up first.

### Official documentation

- Website and source: <https://github.com/alexbeeken/skrongle-mini>
- Documentation: [Set up Skrongle on a new Raspberry Pi](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/setup-home-wifi.md), [Self-hosting reference](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md) and [FAQ](https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting-faq.md)
- Help: [GitHub Discussions](https://github.com/alexbeeken/skrongle-mini/discussions)
- License: [AGPL-3.0](https://github.com/alexbeeken/skrongle-mini/blob/main/LICENSE)
