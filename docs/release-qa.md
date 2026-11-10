# Release QA: the hardware checklist

For maintainers. The emulated matrix (`scripts/release-qa.sh`) is the
per-architecture check; this list is the hardware gate, run on a Pi Zero 2 W
before a release is announced (the decisions on #47).

Tick a box only when you saw what it says. If you did not, leave it
unticked and write down why in [Record it](#record-it). `<version>` is the
release you are checking, without the `v`; `<previous>` is the release
before it.

## Before you start

- [ ] You have a Pi Zero 2 W (or any 64-bit Pi) with Skrongle already installed from the [self-hosting guide](self-hosting#install), and at least one account on it besides the owner's, or a spare invitation to make one.
- [ ] The release's tag is pushed and its GitHub release has the tarballs (`skrongle-<version>-aarch64.tar.gz` and its `.sha256`).
- [ ] The automated run is green: `scripts/release-qa.sh <version>` on your laptop ends with `ok` on both rows and exits 0.
- [ ] You have a phone on the same network as the Pi, and about 30 minutes.
- [ ] You know `<previous>`: the version `cat /opt/skrongle/VERSION` prints now.

## Update the install

- [ ] On the Pi, run `sudo /opt/skrongle/update.sh <version>`. It prints `downloading skrongle-<version>-aarch64.tar.gz`, then `stopping skrongle (<previous> -> <version>)`, then `database copied to /var/lib/skrongle/learn.db.before-<version>`.
- [ ] The output ends with `Skrongle <version> is running`. It does not end with `did not start`.
- [ ] `cat /opt/skrongle/VERSION` prints `<version>`.
- [ ] `sudo /opt/skrongle/skrongle doctor --service` shows the version and no line that starts with `PROBLEM:`.
- [ ] If the script printed a `note:` that `skrongle.service` differs from the one in `/etc/systemd/system`, the release notes say to copy it. If they do not, that is a finding.

## First day as a learner

- [ ] On the phone, open `http://<hostname>.local:7576` (or `http://<ip>:7576`). The sign-in page opens.
- [ ] Sign in with an existing learner account. You land on the home page with the mode cards.
- [ ] Open `/lessons`, then a lesson. The lesson text shows and a **Practise this lesson** link is at the bottom.
- [ ] Click **Practise this lesson** and answer the questions. At the end you see your score.
- [ ] Back on the home page, click **Start recommended practice** and finish one set. It ends with a summary and no error page.
- [ ] On the home page, click **Start a session** under Sprint and play one session to its end. The session summary shows.
- [ ] Open `/lessons` again. The lesson you worked on shows its new progress.
- [ ] Open the home page again. The numbers on the Practice card (answers this week) went up.
- [ ] Nothing took more than a few seconds to open. This is a feel, not a measurement: note it if a page felt stuck.

## Owner tasks

- [ ] Sign in as the owner on a laptop. The account menu (the picture, top right) has **Manage Users**.
- [ ] In **Manage Users**, click **Make an invitation link**. A link shows once, with a **Copy** button.
- [ ] Open that link on the phone in a private tab. It shows the register page, not an error. Register a test learner and sign in as them.
- [ ] Back in **Manage Users**, the new learner is listed and the link is gone from the open invitations.
- [ ] Open a course you can edit and click **Share**. The dialog titled "Share this course" opens.
- [ ] In the dialog, click **Make an invitation link** and copy the link.
- [ ] Open that link in another private tab and register or sign in. The course opens for that learner and its status in the dialog changes from invited to opened.

## Roll back and forward

- [ ] Run `sudo /opt/skrongle/update.sh <previous>`. The output ends with `Skrongle <previous> is running`.
- [ ] Sign in as the learner and as the test learner. Both accounts work and the progress from the sections above is still on `/lessons`.
- [ ] Run `sudo /opt/skrongle/update.sh <version>`. The output ends with `Skrongle <version> is running`.
- [ ] Sign in again. The accounts, the progress and the shared course are all still there.
- [ ] `sudo /opt/skrongle/skrongle doctor --service` shows no line that starts with `PROBLEM:`.

## Uninstall on a spare card only

Optional. Never on the install people use: it removes the program, and
`--purge` removes the accounts.

- [ ] On a spare card with a copy of the install, run `sudo /opt/skrongle/uninstall.sh`. It prints `stopped the skrongle service`, `removed /opt/skrongle`, and ends with `Skrongle is removed. Its data is kept, so installing it again brings the accounts back.`
- [ ] `ls /var/lib/skrongle` still shows `learn.db`.
- [ ] Install again with the steps in [Install](self-hosting#install). Sign in with an account from before. It works.

## Record it

- [ ] Comment on the release issue with: the board (for example "Pi Zero 2 W, 512 MB"), the OS and its version (`cat /etc/os-release`), `<previous>` and `<version>`, and which sections you walked.
- [ ] List every box that was not as expected, with what you saw instead, and every box you skipped and why.
- [ ] Say in one line whether you would announce the release.
