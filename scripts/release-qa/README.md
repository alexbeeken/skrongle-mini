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
| `upgrade-check.sh <tarball>` | The upgrade path (#43): the tarball started on the database each older release wrote (`fixtures/`), checking the migration ran, both accounts sign in, progress, the shared course and the unused invitation survived, a second start changes nothing, the older release's binary refuses the migrated database (exit 78, file unchanged), and going back works: the tarball's `skrongle migrate` takes it down to the fixture's schema and the older binary starts on it with both accounts and the progress. `KEEP_DB=<path>.db` on `package-smoke.sh` writes a new fixture (`fixtures/README.md`). |
| `compose.yaml` | One service per architecture (`skrongle-aarch64` on port 8081, `skrongle-x86_64` on 8082; `skrongle-armv7l` on 8083 and `skrongle-armv6l` on 8084 behind the `arm32` profile until a release ships them, #21), plus `qa-<arch>` services that run `package-smoke.sh`, a `fetch` one-shot that downloads a release, and a `binfmt` one-shot for QEMU's handlers. |
| `matrix.sh <version \| dir> [arch ...]` | Runs `qa-<arch>` (package-smoke.sh, then upgrade-check.sh; `SKIP_UPGRADE=1` leaves the second out) for each architecture and prints one table. Logs in `results/release-qa/<version>/`. |
| `vm.sh <arch> <command>`, `lima/` | A full Debian 12 VM with systemd per architecture under Lima (#42): `up`, `install`, `update`, `e2e`, `measure`, `uninstall`, `down`, `shell`. See "VMs" below. |
| `e2e-run.sh` | What the `e2e` profile's Playwright container runs (below). |
| `sim-build.sh <arch>` | Builds the AI simulator the `simulator` profile mounts (below). |
| `Dockerfile`, `entrypoint.sh`, `fetch.sh` | The image (Debian 11 + curl), what it does with the tarball (`serve`, `serve-e2e`, `smoke` or `upgrade`, after checking that the container's `uname -m` is the architecture it is named for), and the downloader. |

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

## The browser specs in the rig (`e2e` profile)

The Playwright `release` projects (`e2e/specs/release`, #40; `docs/testing.md`,
"Release specs") against the tarball's own server, in the rig (#41):

```sh
E2E=1 scripts/release-qa/matrix.sh dist x86_64     # a third column, e2e, after verdict and upgrade
# or by hand, for one architecture (aarch64 or x86_64):
cd scripts/release-qa
SKRONGLE_VERSION=0.2.1 docker compose --profile e2e run --rm --build e2e-x86_64
docker compose --profile e2e rm -fs e2e-server-x86_64
```

`e2e-server-<arch>` unpacks the tarball into the same Debian 11 image as the
other services and runs it on a fresh database (a tmpfs), with its log on a
volume. `e2e-<arch>` is Playwright's own image (`mcr.microsoft.com/playwright`,
the version `e2e/package.json` pins, so keep the tag in `compose.yaml` equal),
which copies `e2e/`, `data/` and `src/test_course.rs` from the repo, reads the
owner link from the server's log and runs `--project release`. Only the server
is the architecture under test; Chromium and Node run on the host's
architecture, so the specs run at full speed whatever the server is. The
HTML report and traces of a run are in `results/release-qa/e2e/`; the log is
`results/release-qa/<version>/<arch>.e2e.log`. The 32-bit ARM servers have no
`e2e` service: nothing they run differs for a browser, and the slow part is
already `package-smoke.sh`. Off by default: without `E2E=1`, `matrix.sh` does
what it did before. A run spends the owner link, which is why the server's
database is fresh every time.

The `release-qa` workflow runs the same specs in its x86_64 cell, natively on
the runner against the unpacked tarball, not through this profile
(`docs/testing.md`, "Release QA").

### The AI spec in the rig (`simulator` profile)

`e2e/specs/release/ai.spec.ts` needs an AI simulator: a release binary has no
`/dev` routes. `SIM=1` starts one beside the tarball's server, so that spec
runs instead of skipping itself:

```sh
E2E=1 SIM=1 scripts/release-qa/matrix.sh dist aarch64   # the e2e cell reads "ok (AI spec ran)"
# or by hand:
scripts/release-qa/sim-build.sh aarch64      # prints target/aarch64-unknown-linux-gnu/release-qa-sim/spanish-learning (compose's default SIM_BIN_AARCH64)
cd scripts/release-qa
E2E_SIM=1 SKRONGLE_VERSION=0.3.0 docker compose --profile e2e --profile simulator run --rm --build e2e-aarch64
docker compose --profile e2e --profile simulator rm -fs simulator-aarch64 e2e-server-aarch64
```

`simulator-<arch>` is a dev-feature build of this checkout for that
architecture (not in the tarball: built on the host by `sim-build.sh`,
cargo-zigbuild with the dev feature and without debug assertions, so the
pages and translations are embedded as in a release, not read from this
checkout's path; or `SIM_BIN_AARCH64` / `SIM_BIN_X86_64`), run in the same
Debian 11 image as the server, in dev mode with the fixtures from the repo's
`data/`, as `scripts/release-e2e.sh` runs its simulator. Dev mode answers
only requests made to `localhost` or `127.0.0.1`, so it cannot be reached by
its compose name: the simulator and the Playwright container join the
server's network namespace (`network_mode: service:e2e-server-<arch>`), and
on that shared loopback the release server reaches it at
`127.0.0.1:8090/dev/ai/anthropic` and `/dev/ai/openai/v1`
(`SKRONGLE_ANTHROPIC_BASE_URL` and `SKRONGLE_OPENAI_BASE_URL`, the
operator's settings; `ai-formats.spec.ts` needs the second) and the spec reads its call log there (`E2E_AI_SIM_URL`).
`E2E_SIM=1` sets both; without it they are empty and the `e2e` profile runs
as before. With `SIM=1`, `matrix.sh` marks the cell `FAIL` if the AI spec did
not run. aarch64 and x86_64 only, as for the `e2e` profile (the Playwright
image has no 32-bit ARM build).

The specs also run against any server you start yourself:
`E2E_BASE_URL=http://localhost:8081
E2E_OWNER_LINK='<owner link from docker compose logs>' npx --prefix e2e playwright test --project release`
on a fresh volume (`docker compose down -v`).

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
