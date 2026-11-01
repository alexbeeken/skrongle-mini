# Rolling back a bad release

For the hosted site on AWS. Skrongle mini (the Pi) has no rollback yet.
The reference, with what the deploy keeps and logs, is
[AWS setup, Rolling back](../docs/aws-setup.md#rolling-back).

1. See what is live and what came before it:

   ```sh
   scripts/deploy-aws.sh --status          # the last five releases, with schema versions and backups
   ```

2. Roll back:

   ```sh
   scripts/deploy-aws.sh --rollback
   ```

   If the last release did not change the database, this swaps the
   previous release back in and nothing is lost. If it did, the command
   stops, names the backup taken just before the release and says how long
   ago it was.

3. Only when losing what learners did since that backup is better than
   keeping the bad release:

   ```sh
   scripts/deploy-aws.sh --rollback --restore
   ```

   The database it replaces stays on the instance as
   `/var/lib/learnmap/learn.db.before-restore-<time>`. Delete it once you
   are sure (it holds personal data).

4. Sign in and check your account. Fix forward with a new commit; the next
   deploy keeps this release as the previous one again.

If the service shows failed with "this database was upgraded by a newer
Skrongle", an older binary was started on a newer database. Deploy the
newer commit again, or use step 3.
