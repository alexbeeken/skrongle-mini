# Incident response

What to do when personal data on the hosted site may have leaked: someone
got into the server, the database or a backup left our hands, a provider
key turned up somewhere public, or a learner reports seeing another
learner's data. The privacy notice promises a breach notice, and the law
gives deadlines, so this page is written to be followed under stress, in
order.

This is not legal advice. The deadlines and regulator duties below are
what we understand today; the open questions for the lawyer are at the
end.

This file sits in `ops/`, not `docs/`, on purpose: everything in `docs/`
is built into the server and readable by anyone at `/docs`. Keep it here.

Skrongle mini has no e-mail, SMS, Google or payments. On a mini the
operator follows the same steps for their own server and tells their few
users directly; the e-mail steps and provider rotations do not apply.

## Who decides

The owner (Alex) decides, alone, and writes each decision and its time
in the breach register as it is made. If the owner cannot be reached
within the first hour, whoever has ssh access to the box does the first
hour's steps (they only contain and preserve; none of them tells anyone)
and keeps trying the owner.

## The deadlines

The clock starts when we have reason to believe a breach happened, not
when we are sure of the details.

| Who | What | When |
|---|---|---|
| Privacy Commissioner of Canada (PIPEDA) | a report, if there is a real risk of significant harm | as soon as feasible |
| Affected people (PIPEDA) | a notice, same test | as soon as feasible, usually with the report |
| Commission d'accès à l'information (Quebec, Law 25) | a notice, for Quebec residents, if there is a risk of serious injury | promptly |
| The EU and UK regulators (GDPR, UK GDPR) | a report, unless the breach is unlikely to be a risk to people | within 72 hours |
| Affected people (GDPR, UK GDPR) | a notice, when the risk is high | without undue delay |
| The breach register | an entry for every breach, harmful or not | at once; keep it 24 months (Quebec may need longer, ask) |

A report can be sent before every fact is known and completed later. Do
not hold a report back to finish the investigation.

## The first hour

Contain first, then preserve. Nothing here notifies anyone.

1. Start the breach register entry (below): the time, who noticed, what
   they saw. Keep adding to it as you go.
2. Sign everyone out. A session is a random token stored hashed in the
   `sessions` table; there is no signing secret to rotate, so deleting
   the rows ends every session at once. On `/superuser`, Breach tools →
   **Sign accounts out**, Every account, a reason (the register entry),
   type the word it asks for. It also drops sign-ins waiting for a
   second-step code and unused password-reset links, and keeps your own
   session. For only some accounts, choose "Only these addresses" and
   paste them. If the site is down or you can't sign in, do it on the box:

   ```sh
   ssh -i ~/.ssh/learnmap ec2-user@<ip>
   command -v sqlite3 || sudo dnf install -y sqlite
   sudo -u learnmap sqlite3 /var/lib/learnmap/learn.db \
     "DELETE FROM sessions; DELETE FROM two_factor_challenges; DELETE FROM password_resets;"
   ```

   For one account, Superuser → Suspend does the
   same for that account and keeps it out
   ([the Superuser section](../docs/features.md#profile)).
3. Rotate every provider secret, newest leak first. Make a new key in
   each provider's dashboard, store it, revoke the old one there, then
   restart the app so it reads the new values:

   ```sh
   scripts/aws-env.sh set SKRONGLE_STRIPE_SECRET_KEY        # then _WEBHOOK_SECRET
   scripts/aws-env.sh set SKRONGLE_GOOGLE_CLIENT_SECRET
   scripts/aws-env.sh set SKRONGLE_MAIL_<NAME>_API_KEY      # each provider in SKRONGLE_MAIL_PROVIDERS
   scripts/aws-env.sh set SKRONGLE_SMS_<NAME>_...           # each provider in SKRONGLE_SMS_PROVIDERS
   scripts/aws-env.sh set SKRONGLE_AI_KEY
   scripts/aws-env.sh set TYPESAFE_API_KEY
   scripts/deploy-aws.sh --skip-build                       # restarts with the new values
   ```

   Also replace the deploy user's access key (IAM → Users → the deploy
   user → Security credentials) and, if the Mac may be the way in, the
   ssh key. Names of every setting are in
   [configuration](../docs/architecture.md#configuration).
4. If the database left our hands together with the secret key
   (`SKRONGLE_SECRET_KEY` or `<db>.key`), the learners' saved AI keys and
   authenticator-app secrets are readable. There is no tool that re-seals
   them under a new key. Delete them instead, and say so in the notice so
   learners revoke those AI keys at their provider and set their
   authenticator app up again:

   ```sh
   sudo -u learnmap sqlite3 /var/lib/learnmap/learn.db \
     "DELETE FROM user_api_keys; DELETE FROM totp_keys; DELETE FROM two_factor_confirmations; UPDATE users SET two_factor = '' WHERE two_factor = 'totp';"
   ```

   Then set a new `SKRONGLE_SECRET_KEY` (64 hex characters) and restart
   once with `SKRONGLE_NEW_SECRET_KEY=1`. If you skipped the SQL above,
   that start does it for you: it deletes the saved AI keys the new key
   can't read and turns off the authenticator apps it can't read (the log
   says how many of each).
5. Preserve what happened. Take a snapshot of the instance's volume
   before changing anything else on it (EC2 → Volumes → the volume →
   Create snapshot, or `aws ec2 create-snapshot --volume-id <id>
   --description "incident <date>"`). Copy the logs off the box:

   ```sh
   ssh -i ~/.ssh/learnmap ec2-user@<ip> \
     "sudo journalctl -u learnmap -u caddy -u learnmap-check --since '-14 days' -o short-iso" > incident-<date>-journal.txt
   ```

   The routine backups from #964 are not a forensic copy; the snapshot is.
6. Export the AWS account's activity. **Needs #973.** With CloudTrail
   writing a trail to S3, copy that bucket's objects for the incident's
   days. Until #973 is done there is no trail, only CloudTrail's built-in
   90-day event history: CloudTrail → Event history → Download events (or
   `aws cloudtrail lookup-events --start-time <when>`), which covers
   management events such as new users, keys and instances, not data
   reads.
7. Look for a way in that is still open. **Needs #973** for the account
   side: the account is still operated as root with old keys, so check
   IAM → Users and each user's access keys, and the root user's keys,
   for anything you did not make, and delete it. On the box:
   `last`, `sudo cat /home/ec2-user/.ssh/authorized_keys`, and the
   security group's inbound rules.

## Within 24 hours: is there a real risk of significant harm?

PIPEDA's test, which Quebec and the GDPR resemble: how sensitive the
information is, and how likely it is to be misused. Write the answer and
the reasons in the register, whichever way it goes.

What each table holds is in the [data inventory](../docs/data-inventory.md). As a
guide for this app:

- High risk: e-mail addresses together with names or phone numbers,
  readable saved AI keys (someone can spend the learner's money),
  authenticator secrets, payment references, tutor transcripts and notes
  (learners type personal things into them). Notify.
- Lower risk: password hashes alone (argon2id; a strong password stays
  safe, a weak one may not, so ask learners to change a password they
  use elsewhere), course content, lesson progress. Usually record only,
  unless combined with the above.
- Not a breach of personal data: a course text or lesson leaking with no
  account attached. Record it anyway if unsure.

Signing everyone out (step 2) is enough after a leaked database: the
passwords themselves are not in it. A forced password reset for every
account is not a tool yet (see "Tools still missing").

## Telling the regulators

Check each link still works when you need it; forms move.

- Canada: the Office of the Privacy Commissioner's breach report form,
  <https://www.priv.gc.ca/en/report-a-concern/report-a-privacy-breach-at-your-organization/>.
- Quebec: the Commission d'accès à l'information's confidentiality
  incident notice, <https://www.cai.gouv.qc.ca/>, for affected Quebec
  residents.
- UK: the ICO's breach report, <https://ico.org.uk/for-organisations/report-a-breach/>,
  or 0303 123 1113.
- EU: we have no establishment in the EU, so there is no single lead
  regulator. Report to the data protection authority of each EU country
  where affected learners live. Whether we need an EU or UK
  representative is a lawyer question.

Our own contacts for these regulators (account numbers, a named person,
past case numbers) go in the private ops notes, not in this repo.

## Telling the learners

A notice goes by e-mail to every affected account, in plain words, as
soon as the risk test says so. The hosted site's mail outbox caps every
purpose at `SKRONGLE_MAIL_PER_HOUR` (default 3,000 an hour), so a large
list takes hours; start early.

On `/superuser`, Breach tools → **E-mail a notice**: every account with
a confirmed address, or only the addresses you paste. Write the subject
and the text (the template below, with the blanks filled; the button
sends the same text to everyone, so write "Hello," without a name), a
reason, and type the word it asks for. The notice on the page says how
many were queued. Mails past the hourly limit wait for a later hour; if
the page says some accounts still wait, press Send again later with the
same text, and nobody gets it twice. Write in the register when it went
and to how many.

The notice says what PIPEDA's regulations and the GDPR ask for. Template:

```text
Subject: Skrongle: a security incident affecting your account

Hello,

On <date> we found that <what happened, in one or two sentences>. It
happened between <start> and <end>.

What was involved: <the kinds of information, e.g. your e-mail address
and the notes you typed into course generation>. <What was not, e.g.
your password itself is not stored, only a scrambled form of it.>

What we have done: <signed every account out, replaced our keys, removed
saved AI keys, reported it to the Privacy Commissioner of Canada, ...>.

What you can do: <sign in again; if you used your Skrongle password
anywhere else, change it there; if you saved an AI key in Skrongle,
revoke it at your AI provider and make a new one; set up your
authenticator app again>.

Questions: reply to this e-mail or write to <SKRONGLE_LEGAL_EMAIL>.

<Name>, Skrongle
```

Write it in English. For a learner whose `lang` is another app language,
add a translation below the English text if there is time to check it;
do not hold the notice for it.

## The breach register

Every breach goes in, including the ones that did not need a report.
It lives in the private ops notes, not in `docs/` or anywhere in this
repo, and each entry is kept for at least 24 months. One entry:

- the date and time we learned of it, and who noticed
- what happened and when, as far as known
- what personal information was involved, and roughly how many accounts
- the risk decision (real risk of significant harm or not) and why
- who was told and when: each regulator, the learners, providers
- what we changed afterwards so it does not happen again

## Afterwards

Within a week, write down what let it happen and open an issue for each
fix. Check this page against what actually happened and correct it.

## Before it happens

`skrongle doctor` on the box has a `legal e-mail set` line. It must say
`yes`: the notice tells learners to write to `SKRONGLE_LEGAL_EMAIL`.

## Tools still missing

- A forced password reset for every account. Not planned: signing out is
  enough after a leaked database (see the risk section), since the
  passwords are not in it.

## Questions for the lawyer

- Which regulators apply to a Vancouver sole proprietor with users in
  the EU, UK and Quebec?
- Do we need an EU or UK representative?
- What must the register contain, and is 24 months enough for Quebec?
