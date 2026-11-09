## What this PR is for

- 

## How to QA it

<!--
Walkable on a release build: no /dev/login, only what a self-hoster sees.
Example of a good first box and a good step:

- [ ] Run it: `./.dev-start --reset --seed --background`, then open `/dev/login`; or on a release build, `cd scripts/release-qa && docker compose up -d`, then open the owner link from `docker compose logs skrongle-aarch64` and make the first account.
- [ ] On a course you made, click Share: the "Share this course" dialog opens; click "Make an invitation link" and a link appears, marked "not opened yet".
-->

- [ ] Run it: `./.dev-start --reset --seed --background`, then open `/dev/login`; or on a release build, `scripts/release-qa/matrix.sh dist` to check a tarball you built, or `cd scripts/release-qa && docker compose up -d`, then the owner link from `docker compose logs` to make the first account.
- [ ] 

Closes #
