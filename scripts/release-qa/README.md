# Release QA: every architecture's tarball, on one laptop

A test rig for the release tarballs (`skrongle-<version>-<arch>.tar.gz`),
not a way to run Skrongle. Self-hosters install the tarball under systemd
(`docs/self-hosting.md`); this folder runs the same tarball in a stock
Debian container of each architecture we ship, so a release can be checked,
and poked at in a browser, without owning the hardware. The plan it is part
of is issue #47; this folder is #39 and #41.

What is here:

| File | What it is |
|---|---|
| `package-smoke.sh <tarball>` | Checks one tarball on the Linux machine it runs on: layout, checksum, the binary (version, `doctor`, what it links), then a first day over HTTP with curl: the owner link from the log, the owner, an invitation and a learner, the built-in course's pages, a copied course shared by link, a clean stop and a second start on the same database. POSIX sh + curl. No dev routes: it drives the binary self-hosters run. |
| `upgrade-check.sh <tarball>` | The upgrade path (#43): the tarball started on the database each older release wrote (`fixtures/`), checking the migration ran, both accounts sign in, progress, the shared course and the unused invitation survived, a second start changes nothing, and the older release's binary refuses the migrated database (exit 78, file unchanged). `KEEP_DB=<path>.db` on `package-smoke.sh` writes a new fixture (`fixtures/README.md`). |
| `compose.yaml` | One service per architecture (`skrongle-aarch64` on port 8081, `skrongle-x86_64` on 8082; `skrongle-armv7l` on 8083 and `skrongle-armv6l` on 8084 behind the `arm32` profile until a release ships them, #21), plus `qa-<arch>` services that run `package-smoke.sh`, a `fetch` one-shot that downloads a release, and a `binfmt` one-shot for QEMU's handlers. |
| `matrix.sh <version \| dir> [arch ...]` | Runs `qa-<arch>` (package-smoke.sh, then upgrade-check.sh; `SKIP_UPGRADE=1` leaves the second out) for each architecture and prints one table. Logs in `results/release-qa/<version>/`. |
| `vm.sh <arch> <command>`, `lima/` | A full Debian 12 VM with systemd per architecture under Lima (#42): `up`, `install`, `update`, `e2e`, `measure`, `uninstall`, `down`, `shell`. See "VMs" below. |
| `Dockerfile`, `entrypoint.sh`, `fetch.sh` | The image (Debian 11 + curl), what it does with the tarball (`serve` or `smoke`, after checking that the container's `uname -m` is the architecture it is named for), and the downloader. |

## Check a release

```sh
scripts/release-qa/matrix.sh 0.1.2            # a published release: fetched into dist/ when missing
scripts/release-qa/matrix.sh dist             # a candidate you built (scripts/build-pi.sh + scripts/package-release.sh)
scripts/release-qa/matrix.sh 0.1.2 aarch64    # one architecture
```

Needs Docker with Compose. The first run builds the images (a minute or
two; Debian 11's packages come from `archive.debian.org`). It ends like this:

```
release 0.1.2
architecture verdict
aarch64    ok
x86_64     ok
armv7l     skipped (no tarball for armv7l in …/dist)
armv6l     skipped (no tarball for armv6l in …/dist)
```

A `FAIL` row prints the failing lines; the whole log is in
`results/release-qa/<version>/<arch>.log`. The exit status is non-zero on any
`FAIL`, so a script can gate on it.

## Look around in a browser

```sh
cd scripts/release-qa
SKRONGLE_VERSION=0.1.2 docker compose --profile fetch run --rm fetch   # once per release
SKRONGLE_VERSION=0.1.2 docker compose up -d
docker compose logs skrongle-aarch64 | grep "Owner link"
```

Open the link the log prints (it already carries the host port, `http://localhost:8081/register?invite=…`)
to make the owner's account on that server, then use it as a learner would:
Manage Users for invitation links, the Share dialog for course links. Each
architecture is its own server with its own database (a named volume), so
state survives `docker compose down`; `docker compose down -v` starts over.
`SKRONGLE_VERSION=0.1.3 docker compose up -d` swaps the tarball under the
same database, which is the update path without `update.sh`.

To run a tarball you built instead of a release, put it in `dist/` (where
`scripts/package-release.sh` writes) with its `.sha256`, or point `TARBALLS`
at its directory.

## The 32-bit ARM architectures

Docker runs another architecture's containers through QEMU user-mode
emulation. An Apple Silicon Mac runs x86_64 through Rosetta out of the box;
for `linux/arm/v7` and `linux/arm/v6` (and for x86_64 on a Linux laptop),
install the handlers once:

```sh
docker compose --profile setup run --rm binfmt
```

Then `docker compose --profile arm32 up -d` adds the two 32-bit servers, and
`matrix.sh` checks them when the release has their tarballs. They are slow:
a page can take seconds, the first start minutes. The health checks allow
for it.

Until a release ships those tarballs (#21), check a build of your own:
`scripts/build-pi.sh armv7` builds the armv7 binary, and `cargo zigbuild
--target arm-unknown-linux-gnueabihf.2.31` the armv6 one; pack each like
`scripts/package-release.sh` does, named `skrongle-<version>-armv7l.tar.gz`
and `-armv6l`, into a folder of their own, and run `matrix.sh <that folder>`.
The version in the name must be the one the binary prints (`Cargo.toml`), or
the version check fails, as it should. Both cells pass this way under
QEMU.

## What this does and does not cover

A container runs the binary and the app: the architecture's instruction set,
32-bit integer widths, what the binary links against (Debian 11, glibc 2.31,
is the oldest we support), the data files, SQLite, the pages, the links the
app hands out. It does not run systemd, so the unit's sandboxing,
`DynamicUser`, `StateDirectory`, `update.sh` and rollback are not covered
here; that is the VM layer below (#42). Timings under emulation mean nothing, so
nothing here measures time (the decisions on #47). A green cell is good
evidence a release works on that architecture, not proof; the hardware
checklist (#45) stays.

The `dietpi-install` workflow covers the DietPi install block in containers (`scripts/dietpi-install-check.sh`, #31); #70 covers a real board.

`package-smoke.sh` also runs on its own, on any Linux box or VM with curl:
`scripts/release-qa/package-smoke.sh skrongle-0.1.2-aarch64.tar.gz`. `KEEP=1`
leaves its server running and prints the URL.

The browser specs for a release server (`e2e/specs/release`, Playwright project
`release`, #40) run against any of these servers: `E2E_BASE_URL=http://localhost:8081
E2E_OWNER_LINK='<owner link from docker compose logs>' npx --prefix e2e playwright test --project release`
on a fresh volume (`docker compose down -v`). See `docs/testing.md`, "Release specs".

## VMs

The containers above do not run systemd. `vm.sh` does, in a Lima VM (Debian 12,
1 CPU, 512 MiB, named `skrongle-qa-<arch>`; only that name is ever touched), and
runs the commands of `docs/self-hosting.md` in it: Install (the doc's code
block, read from the file), `update.sh` (a candidate served from this machine
through `SKRONGLE_RELEASE_BASE`, so the download and the SHA-256 check are the
real ones), going back to the published release, `uninstall.sh` plain and with
`--purge`. `docs/testing.md`, "Release QA", "VMs" has the commands and what
each checks.

```sh
scripts/build-pi.sh aarch64 && scripts/package-release.sh 0.3.0-qa aarch64   # the candidate, in dist/
scripts/release-qa/vm.sh aarch64 up
scripts/release-qa/vm.sh aarch64 install
scripts/release-qa/vm.sh aarch64 update
scripts/release-qa/vm.sh aarch64 e2e        # the release Playwright specs against http://127.0.0.1:18576
scripts/release-qa/vm.sh aarch64 measure    # results/release-qa/<version>/aarch64-vm.txt
scripts/release-qa/vm.sh aarch64 uninstall
scripts/release-qa/vm.sh aarch64 down
```

`vm.sh aarch64 shell` opens a shell in it; the app is also at
`http://127.0.0.1:18576` (x86_64: 18577) while it runs. aarch64 uses Apple's
Virtualization framework (`vmType: vz`), so no QEMU. x86_64 needs QEMU
(`brew install qemu`) and has been written but not run. armv7l: no Debian
cloud image Lima can boot, so 32-bit ARM stays with the containers.
