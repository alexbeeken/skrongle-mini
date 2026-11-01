# Privacy requests by e-mail

What to do when someone writes in about their personal data instead of
using Profile → Your data. Account holders can download and delete on
their own. This page is for everyone else. That means a learner who is locked out of
their account, an agent acting for someone, a parent, or a person with no
account whose address is in a course share invitation.

This is not legal advice. The deadlines below are what we understand
today. The open questions for the lawyer are at the end.

This is the hosted site only. Skrongle mini has no e-mail, and its owner
has the database and answers their few users directly. The superuser
actions this page uses are not on a mini (they are 404 there).

## Who decides

The owner (Alex) answers every request and decides every appeal. With
one person running the site there is nobody else to hand an appeal to.
The notice says so, and the appeal reply says it too. The person can
still go to their regulator.

## The inbox

Requests come to the privacy contact the notice names. That is the
address in `SKRONGLE_LEGAL_EMAIL`, which is also the contact for
copyright notices and suspended accounts. The plan is
`privacy@skrongle.com`, an alias at the mail provider that forwards to
the owner's mailbox, so the notice does not change when a personal
address does (`TODO.md`, "Legal"). A request that reaches another address
of ours counts from the day it arrived there. Forward it to the inbox.

## The deadlines

Each clock starts on the day the request arrives, not on the day the
identity check finishes. Write the date in the request log at once.

| Law | Who it covers | Answer within | Can it be extended? |
|---|---|---|---|
| PIPEDA (Canada) | everyone in Canada | 30 days | once, by up to 30 more days, if we tell the person before the first 30 run out and say why |
| GDPR, UK GDPR | people in the EU and the UK | one month | by two more months for a complex request or many of them, if we tell the person within the first month and say why |
| CCPA (California) and the other US state laws | people in those states | 45 days; confirm we got it within 10 business days | once, by 45 more days, if we tell the person within the first 45 |

When you can't tell where the person lives, use the shortest clock, 30
days. Aim to answer within a week anyway. Most requests are one
superuser action.

## 1. Log it

Open an entry in the request log (below) before you do anything else:
the date it arrived, the address it came from, what it asks for. Reply
the same day to say we have it, when to expect an answer, and what we
need to confirm who they are. That reply also covers the CCPA's 10-day
confirmation.

## 2. Check who is asking

Ask for as little as you can. Never ask for an ID document, a password
or a payment card.

- **An account holder** (the address has an account): the request must
  come from the account's own address. If it came from another address,
  write to the account's address and ask the person to reply from there.
  Everything we send about the account goes to that address only.
- **Someone with no account**: write to the address the request is about
  and ask them to reply from it. A reply proves they read that mailbox,
  which is all we hold about them.
- **An authorized agent** (a person or company acting for someone):
  ask for the person's signed permission naming the agent and what they
  may ask for. Then confirm with the person themselves, by the step above
  for their address, that they sent the agent. Without both, refuse in
  writing and say what is missing. Send any data to the person's own
  address, never to the agent.
- **A parent about a child's account**: ask the parent to name the
  account's address, then write to that address. If the account belongs
  to a child under the minimum age in the Terms, deleting it is the
  answer whatever the check shows. Use the reason "a child's data".

Write in the log how you checked ("replied from the account's address on
<date>"). If the person never replies, close the entry after the deadline
with "not verified" and send a last note saying why nothing was done.

## 3. Do what they asked

All of these are on `/superuser` (the Privacy requests
card, [features](features)). Each one asks for a reason, and the reason
goes in the privacy log on the page. That log names no learner, so write
the request log's entry number as the reason, for example
`privacy request PR-2026-004`.

- **A copy of their data** (access, portability): Send an account its
  data. It starts the same export Profile → Your data makes, and the
  download link goes to the account's own address. The link opens only
  while signed in to that account, works once, for 24 hours, and there is
  one export a day. A locked-out learner needs to get back in first. Set
  a Temporary password and send it to the account's address, in its own
  message.
- **Deleting their account** (erasure): Delete account, on the account's
  row. It deletes everything the learner's own deletion deletes. It is
  refused while a Stripe subscription is still billing. Cancel that first
  in the Stripe dashboard, then delete.
- **Forgetting an address with no account**: Forget an address. It
  deletes every course share invitation to that address and every mail to
  it still waiting to be sent. An address that has an account is refused.
  Delete the account instead.
- **Correcting something**: the learner edits their profile. For a
  locked-out learner, a Temporary password first. Anything a learner can't
  edit (the address on an account they can't reach, say) is a database
  change on the box. Write what you changed in the log.
- **Objecting to a use of their data**: the AI features can be turned off
  in Profile → Privacy and consent. For anything else, tell them what we
  do with it and why (the notice, sections 3 to 5). If the only way to
  stop is to stop holding the data, offer deletion.
- **Asking what we hold about them**: the [data inventory](data-inventory)
  lists every table. For a person with no account, a search of the
  database for their address is the answer. Send them the list of places
  it appears, not the rows.

Then answer by e-mail: what you did, when, and for an export, that the
link is in a separate message to the account's address.

## 4. Saying no, or not all of it

Some requests can't be met in full: the person can't be verified, the
data is someone else's (a course another learner made), or the law lets
us keep something (copyright notices sent to us stay as a record). Say
no in writing, give the reason in plain words, and say how to appeal.
Every refusal, full or partial, ends with the appeal step.

## 5. Appeals

A person who disagrees with an answer can appeal by replying to it. The
reply that refuses tells them so, and tells them they have 60 days to do
it.

1. Log the appeal against the original entry, with the date.
2. Look at the request again from the start, as if it were new. Decide
   within 45 days of the appeal, the shortest window the US state laws
   that have appeals allow (some allow 60).
3. Answer in writing: the decision and the reason for it.
4. If the answer is still no, say how to complain to their regulator
   (below), with the link.

## 6. Complaints (UK, and everyone else)

A complaint is someone unhappy with how we handled their data at all, not
only with an answer to a request. UK law asks us to handle it in these
steps, and we do the same for everyone:

1. Acknowledge it within 30 days of receiving it. Sooner is better, the
   same day, with the log entry.
2. Look into it: what happened, what the data shows, what we did.
3. Tell them the outcome and what we changed, without undue delay.
4. Tell them they can complain to their regulator if they are not
   satisfied.

The regulators:

- UK: the ICO, <https://ico.org.uk/make-a-complaint/>, 0303 123 1113.
- Canada: the Office of the Privacy Commissioner,
  <https://www.priv.gc.ca/en/report-a-concern/>. Quebec residents can also
  go to the Commission d'accès à l'information,
  <https://www.cai.gouv.qc.ca/>.
- EU: the data protection authority of their country.
- California: the California Privacy Protection Agency,
  <https://cppa.ca.gov/webapplications/complaint>.

If the complaint is that their data leaked, follow the breach runbook
(#979) instead.

## The request log

Every request goes in, including the ones we couldn't verify and the ones
that needed no action. A regulator on audit asks to see it, and the CCPA
expects it kept for 24 months. Keep each entry at least that long.

It lives in the private ops notes, not in `docs/` or anywhere in this
repo, and not in the app's database. It is about people who may have no
account, in their own words. One entry:

- an entry number (`PR-<year>-<n>`), the same one written as the reason
  on `/superuser`
- the date it arrived and the address it came from
- what kind: access, deletion, correction, objection, forget an address,
  complaint, or several
- who it is about, and whether they have an account
- how we verified them, and when; for an agent, the permission we saw
- the deadline, and any extension with the date we told them
- what we did and when, and the date we answered
- the outcome: done, partly done (and why), refused (and why), or not
  verified
- an appeal: the date, the decision, the date we answered

The privacy log on `/superuser` (`privacy_actions`) is the app's half:
which superuser did which action when, with the entry number as its
reason. The two together show what was asked and what was done.

## Questions for the lawyer

- Do the 30 days (PIPEDA), one month (GDPR, UK GDPR) and 45 days (CCPA)
  all apply to us, given where we are and who signs up?
- Is a reply from the address enough proof of identity for a person with
  no account, and for an account holder?
- Do we need a written appeal process for the US states, and which
  window applies?
- How long should the request log be kept outside the US rules?
