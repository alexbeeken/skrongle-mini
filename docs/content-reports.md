# Content reports and copyright notices

What to do when someone reports a course. This is for the people who run the
hosted site. Skrongle mini has no report form: its copyright page shows the
contact address, and its owner handles any complaint by hand.

## Where reports come from

- **Report this course.** A learner opens the circle on a course someone
  shared with them (on `/courses`) and sends a report: harmful, copyright or
  other, with a text and their address.
- **The copyright form.** Anyone, signed in or not, can send a notice from
  `/legal/copyright` (linked from `/terms`): name, address, the work, where
  the copy is, what is copied, and the good-faith statement.

Each one is stored (`content_reports`) and e-mailed to `SKRONGLE_LEGAL_EMAIL`,
or to every superuser when that is unset. Open reports are listed on
`/superuser` under **Reports**, oldest first.

## What to do, and by when

1. **Read it within two working days.** Open the course (ask the owner to
   share it with you if you can't see it) and check the claim.
2. **Copyright notice:** check that it has a name, an address, the work,
   where the copy is and the ticked statement. If something is missing,
   reply to the claimant and ask for it; nothing else happens until it
   arrives.
3. **Forward to owner.** For a complete copyright notice, press *Forward to
   owner* on the same day. The Terms promise the owner a copy, and the copy
   carries the claimant's name and address. For a harmful or other report,
   forward it when the owner can fix it; that copy never carries the
   reporter's address.
4. **Remove it** when the claim holds up: clearly copied material, or
   content that is harmful under the Terms. For a copyright notice, do this
   within five working days of a complete notice. Ask the owner to delete
   the course, or delete it from the database.
5. **Resolve** the report when nothing more is to be done (taken down,
   rejected, or the owner fixed it). It leaves the open list; the row stays.

## Counter-notices

An owner who thinks a copyright notice is wrong replies to the forwarded
message. A counter-notice needs their name, address, the course, a
statement that they believe in good faith the material was removed by
mistake, and their consent to the courts named in the Terms.

- Send a copy of the counter-notice to the claimant.
- Put the course back after 10 to 14 working days unless the claimant tells
  you they have gone to court.
- Keep the notice, the counter-notice and your replies.

## Repeat infringers

An account with three copyright notices that held up is closed (the Terms
say so). Search the reports by owner before you resolve a notice.

## Records

Rows in `content_reports` are not deleted when a course is, including one deleted from the Courses table. A reporter who
deletes their account takes their reports with them; copyright notices sent
signed out stay. Mail copies are in the mail provider's log.
