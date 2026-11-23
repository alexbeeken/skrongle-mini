# The DietPi software block

DietPi does not take an upstream install script. A new title goes into
DietPi's own `dietpi/dietpi-software` as a block of bash written with its
helper functions (`Download_Install`, `Create_User`, `G_EXEC`, ...), plus a
handful of one-line edits in other files. This folder holds those pieces, ready
to paste into a fork of DietPi. Nothing here ships: `scripts/package-release.sh`
copies named files only, so this folder is not in the release tarball.
`deploy/dietpi-install.sh` stays the install for people who are not on
DietPi's catalogue.

Written against DietPi's `dev` branch as of 2026-11-22 (`dietpi-software`,
`dietpi-services`, `.meta/dietpi-survey_report`,
`.github/workflows/dietpi-software.bash`, `.github/workflows/update_urls.bash`,
`README.md`, `CHANGELOG.txt`; v10.9 was the newest changelog section).

**None of this has been run on a DietPi.** It parses (`bash -n`, wrapped in a
function) and `shellcheck -s bash` reports only SC2034 for `fallback_url`, which
`Download_Install` reads. The grep that finds the download URL was run against
this repo's real latest release and finds all four architectures and none for
riscv64. Everything else is read from DietPi's source and matched to the
blocks next to it. Points that could not be checked from the source are marked
**check on a DietPi** and collected at the end.

## Files here

| File | Goes into | Indent |
|---|---|---|
| `entry.bash` | `dietpi/dietpi-software`, `Software_Arrays_Init()` | 2 tabs |
| `install.bash` | `dietpi/dietpi-software`, the `Install_Software()` function | 2 tabs |
| `uninstall.bash` | `dietpi/dietpi-software`, the `Uninstall_Software()` function | 2 tabs |

The three companion edits that are one line each, and the `update_urls.bash`
entry, are below.

## Decisions (made by the owner)

- Software ID `skrongle`, name `Skrongle mini`, no `aSOFTWARE_ID_LEGACY`
  number (new titles get none).
- No RISC-V build: `aSOFTWARE_AVAIL_G_HW_ARCH[$software_id,11]=0`. Assets are
  `skrongle-<version>-<arch>.tar.gz`, arch `armv6l` / `armv7l` / `aarch64` /
  `x86_64` for `G_HW_ARCH` 1 / 2 / 3 / 10.
- The install block does not start, enable or wait for the service and does not
  print the owner link. `To_Install skrongle skrongle` registers the service for
  DietPi's own enable, and `dietpi-services` lists it. The owner link is a
  documented first-run command (`sudo /opt/skrongle/skrongle owner-link
  --service`, see `docs/dietpi.md`).
- Data in `/mnt/dietpi_userdata/skrongle` (owner `skrongle`, mode 0750),
  settings in `/etc/skrongle/env` (created only if missing, `root:skrongle`
  0640), program in `/opt/skrongle`, port 7576. The env file is group-readable
  so that `skrongle owner-link --env-file` can read it as the service user.
- Uninstall removes the data: `Remove_Service skrongle 1 1`, then
  `/opt/skrongle`, `/mnt/dietpi_userdata/skrongle` and `/etc/skrongle`.

## Category: Cloud & Backup

`aSOFTWARE_CATX=4`, docs `https://dietpi.com/docs/software/cloud/#skrongle-mini`.
It is the category that already holds self-hosted household web apps that run
one binary and one SQLite file: Yuvomi (family planner), HomeBox, Paperless-ngx,
Immich, Vaultwarden. The nearest alternatives fit worse: Media Systems is for
streaming servers, Social & Search for forums, feeds and chat, and Development
& Programming for toolchains. This is the maintainers' call; if they pick
another category, change `aSOFTWARE_CATX`, the docs URL, the group in
`dietpi-services` and the page the DietPi-Docs entry goes on.

## Edits in a DietPi fork, file by file

### `dietpi/dietpi-software`

1. **Software entry**, `entry.bash`. In `Software_Arrays_Init()`, after the
   `yuvomi` entry (the last one with `aSOFTWARE_CATX[$software_id]=4`, its last
   line is the `aSOFTWARE_AVAIL_G_HW_ARCH` line) and before the
   `#------------------` that opens `software_id='rclone'`. The new entry
   brings its own leading `#------------------` line.
2. **Install block**, `install.bash`. In the install section, directly after the
   whole `if To_Install yuvomi yuvomi` ... `fi` block and before
   `if To_Install solidping solidping`. Keep the blank line between blocks.
3. **Uninstall block**, `uninstall.bash`. In the uninstall section, directly
   after the whole `if To_Uninstall yuvomi` ... `fi` block, before
   `if To_Uninstall solidping`.

### `.github/workflows/dietpi-software.bash`

In the `case $i in` list that sets `aSERVICES`/`aTCP`, alphabetically, after
the `'shairportsync'` line and before `'snapcastclient'` (3 tabs):

```
			'skrongle') aSERVICES[$i]='skrongle' aTCP[$i]='7576';;
```

No `aDELAY`. The script waits `delay=10` seconds after `dietpi-services start`
unless an entry asks for more, and entries such as `navidrome` and `homebox`
set none. A first start under QEMU measured 1.5 to 7 s, which fits. Their
convention for a slower start is `(( $emulation )) && aDELAY[$i]=N`; it is
not needed here. The test image installs the latest release at test time, so a
release must exist for every architecture the matrix tests.

### `.github/workflows/update_urls.bash`

Needed: this script is how DietPi keeps every `fallback_url` current, and
without an entry the pinned `v0.4.0` would go stale. `aARCH_CHECK` makes it
report when a riscv64 asset first appears. Add after the
`solidping` entry, before `### URL check loop ###` (no indent):

```
software_id='skrongle'
aURL[$software_id]='https://api.github.com/repos/alexbeeken/skrongle-mini/releases/latest'
aCHECK[$software_id]='echo "$response" | grep -Po "\"browser_download_url\": *\"\K[^\"]*\/skrongle-[0-9.]*-$arch\.tar\.gz(?=\")"'
aARCH[$software_id]='armv6l armv7l aarch64 x86_64'
aARCH_CHECK[$software_id]='riscv64'
aREGEX[$software_id]='https://github.com/alexbeeken/skrongle-mini/releases/download/.*/skrongle-.*-\$arch\.tar\.gz'
```

`aREGEX` was checked against the `fallback_url` line in `install.bash`. The
loop rewrites that line inside `if To_Install skrongle` ... `fi` at two-tab
indent, which the block keeps.

### `dietpi/dietpi-services`

In the `# - Cloud/Backups` group, after `'paperless-scheduler'` (the group's
last entry), 3 tabs:

```
			'skrongle'
```

### `.meta/dietpi-survey_report`

In the name-keyed array (not the numeric legacy-ID one), alphabetically, after
`[sfpg]='Single File PHP Gallery'` and before `[snapcastclient]`, 2 tabs. It
uses the software name, as the neighbours do (`[immichml]='Immich Machine
Learning'`), so `Skrongle mini`, not `Skrongle` as the issue text had it:

```
		[skrongle]='Skrongle mini'
```

There is no numeric entry: new titles have no legacy ID.

### `README.md`

In the software list, at the end (after the `Podman` line):

```
- [Skrongle mini](https://github.com/alexbeeken/skrongle-mini)
```

### `CHANGELOG.txt`

Under `New software:` of the newest section, as its last item (v10.9 was the
top section; if it has been released, use the next one):

```
- Skrongle mini | This self-hosted learning app, with courses, quizzes, flashcards and games for a household or a class, has been added to our software catalogue with ID "skrongle". It is available for ARMv6, ARMv7, ARMv8 and x86_64 systems. Many thanks to @alexbeeken for implementing this software option: <DietPi PR URL>
```

Fill in the PR URL once the DietPi PR exists (this repo cannot name it).

### DietPi-Docs

`docs/dietpi.md` in this repo holds the entry (its header comment says how to
adapt it) and is the `aSOFTWARE_DOCS` target, section `## Skrongle mini`,
anchor `#skrongle-mini`, on the Cloud page (`software/cloud.md`). Convert it as
that file's header comment says: tabs, the Avahi snippet, a screenshot, and the
software ID.

## What the block does, and how it differs from `deploy/dietpi-install.sh`

The block follows Navidrome (flat tarball, unit, service user) and Yuvomi
(`Create_User`, `install -g ... /dev/null` for a group-readable file, the
`cat << '_EOF_' > file || exit 1` style, whole-folder replacement on reinstall).

| Our script | In the block |
|---|---|
| SHA-256 check of the tarball | **Left out.** `Download_Install` downloads and unpacks with no checksum, and no comparable block verifies one: they trust TLS to GitHub. A check would be a new helper in `Download_Install`, which is the maintainers' to add. |
| `skrongle.previous` (the last binary kept) | **Left out.** The comparable blocks (File Browser, HomeBox, Yuvomi) remove the old program folder and move the new one in. DietPi installs the latest and has no "go back" command. |
| Schema take-back for an older version | **Left out**, as decided: DietPi always installs the latest, so it never installs an older version. A newer database that an older binary refuses exits 78 and the unit does not restart. |
| Checks the new binary runs (`--version`) before touching anything | **Left out.** `Download_Install` has no hook for it; the architecture comes from `G_HW_ARCH`. |
| Stops the service, restarts it, waits for `/healthz` | **Left out**, as decided: DietPi stops and starts services itself. |
| Prints the owner link | **Left out**, as decided: documented first-run command. |
| Keeps an edited unit unless `--unit` | **Replaced on every install**, as Navidrome's and Yuvomi's blocks do. Edits belong in a drop-in under `/etc/systemd/system/skrongle.service.d`, which `Remove_Service` removes. Settings belong in `/etc/skrongle/env`, which is kept. |
| `rm -rf /opt/skrongle/data`, then copy (step 8) | **The whole `/opt/skrongle` is removed**, then the new folder is moved in, so the built-in courses are replaced, not merged. The data folder and the env file are elsewhere and are not touched. |
| `useradd ... --shell /usr/sbin/nologin` | `Create_User -d /mnt/dietpi_userdata/skrongle skrongle`, which makes a system user and a group of the same name with the nologin shell, or fixes an existing user. |
| `env.example` or a built-in env text | The tarball has no `env.example` (`scripts/package-release.sh` does not copy it), so the block writes the env file itself: commented examples only, nothing set. |
| `uninstall.sh` and `update.sh` from the tarball | They land in `/opt/skrongle` but the DietPi uninstall does not use them. |

The unit is the one in `deploy/dietpi-install.sh` with three changes: the
description ends in `(DietPi)`, `After=` also names `remote-fs.target` as
DietPi units do, and the `$OPT`, `$DATA`, `$CONF` and `$PORT` values are
written out because the heredoc is quoted. `RestartPreventExitStatus=78`, the
memory limits and the sandboxing are as they were; the prose comments in the
unit are dropped. The upstream link is the one comment line before the heredoc.

## To check on a DietPi

Nothing here was run on DietPi. Look at these first:

1. **A flat tarball with a target.** `Download_Install` unpacks with
   `tar xf "$file" --no-same-owner --one-top-level=skrongle`. GNU tar documents
   that as "extract all files into DIR", with no stripping, and Navidrome's
   flat tarball is installed the same way, so `skrongle/skrongle` and
   `skrongle/data/courses/...` are expected in the working directory. Check:
   after the block, `ls /opt/skrongle` shows `skrongle`, `data`,
   `skrongle.service`, `update.sh`, `uninstall.sh`, `VERSION`, with no extra
   top folder (our tarball entries start with `./`).
2. **Relative target and `$G_WORKING_DIR`.** The block passes the relative target
   `skrongle`, as Yuvomi does, and moves `skrongle` into `/opt` afterwards.
   Check that nothing is left behind in the working directory.
3. **Reinstall keeps the data and the env file.** Run
   `dietpi-software reinstall skrongle` on a running install with an account
   made. Check: the account is still there, `/etc/skrongle/env` is unchanged
   (edit it first), `/opt/skrongle/data` has only the new release's courses
   (put a stray file in it first), and the service runs the new binary.
4. **Services during a reinstall.** The block replaces `/opt/skrongle` while the
   old service may still be running. Check that `dietpi-software` stops services
   before installs and restarts them after (it calls `dietpi-services restart`
   near the end), so the old process does not keep the deleted binary.
5. **Directory and file modes.** Check that `/opt/skrongle` and the binary are
   readable and executable by the `skrongle` user (the folder is created by
   `tar` under the install's umask; the block does not chmod it), and that
   `/etc/skrongle/env` is `root:skrongle` 0640 and `/mnt/dietpi_userdata/skrongle`
   is `skrongle:skrongle` 0750.
6. **Group handling.** `Create_User` without `-g` makes a group `skrongle`;
   `chown skrongle:skrongle` and `install -g skrongle` rely on it. `Remove_Service
   skrongle 1 1` removes the user and then the group if still present. Check
   `getent passwd skrongle` and `getent group skrongle` are empty after an
   uninstall.
7. **First start and the owner link.** After install and boot,
   `systemctl is-active skrongle` is `active`, port 7576 listens, and
   `sudo /opt/skrongle/skrongle owner-link --service` prints a link on a fresh
   DietPi (no D-Bus).
8. **Uninstall.** `dietpi-software uninstall skrongle` leaves no
   `/opt/skrongle`, `/mnt/dietpi_userdata/skrongle`, `/etc/skrongle`, unit or
   user.
9. **`update_urls.bash`.** Run it (or its `skrongle` entry) once: it must find
   the regex in the block and the four architectures, and none for riscv64.
10. **Linters.** Their `shellcheck` run may flag `fallback_url` (SC2034); the
    other blocks have the same pattern, so it should be excluded already.
11. **Category, description and changelog placement** are for the maintainers
    to settle.
