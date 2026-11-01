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
4. **Take it down** when the claim holds up: clearly copied material, or
   content that is harmful under the Terms. For a copyright notice, do this
   within five working days of a complete notice. Find the course in the
   **Courses** table on `/superuser` (search by its id or the owner's
   address), open it and press *Take down*.
5. **Resolve** the report when nothing more is to be done (taken down,
   rejected, or the owner fixed it). It leaves the open list; the row stays.

## Flagged courses

A made course with a listed word (`data/moderation/terms.txt`), or one the
AI check found a problem in, is **flagged**: it doesn't start and can't be
shared, and its owner's card says which words matched. Nobody reports it;
it shows up first in the **Courses** table on `/superuser`.

1. **Look at it within two working days.** Filter the table to *Flagged*
   and open the course. The dialog shows each match with its lesson, file
   and field and the text around it. For a course the AI check flagged
   (*Flagged by: AI check*), it shows the AI's report (a few sentences on
   what is wrong) and the exact text it quoted, under the rule it breaks
   (one of the AI check's categories, such as Hate speech or Self-harm),
   and the parts Jev scored high, with the categories they scored high on
   and the review's word on each.
2. **The check was wrong** (a word that means something ordinary in the
   language taught, a quote in a lesson about the word itself): press
   *Override the flag* and write a short note saying why. The course starts
   and shares again. The next check keeps the override for the same words;
   a new word flags it again. For an AI flag, write the note for the AI
   too: the review reads it the next time the course changes, and follows
   it unless the new content goes beyond it (its report then says why the
   note doesn't cover it). *Clear the override* takes it back and checks
   the course again as it is.
3. **The check was right**: leave it flagged. The owner can fix it, and the
   flag clears on its own when they save (an AI flag when they save and
   press Share, which checks the course again). For content that breaks the Terms
   badly, *Take down* (the owner sees why, and the shares stay for *Put
   back*) or *Delete the course* (the course, its files and its shares go;
   not undone). For an account that keeps doing it, *Suspend the owner*.
4. **Tell the owner** when the card's red ! is not enough: a flag on a
   course they shared, a take-down, or a flag that has stayed for a few
   days. Press *E-mail the owner* in the course's dialog. The form comes
   filled in their language with the reason and how to fix it; change what
   you like and press *Send*. Never add who reported the course. The row
   then says "owner e-mailed" with the date, so check it before you send
   another. Nothing sends this e-mail on its own.

Every action is logged as which superuser did what to which course, never
the content or the note.

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
