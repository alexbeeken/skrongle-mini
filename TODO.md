# TODO (human)

Work that needs a person: real hardware, lawyers and accountants, accounts
and settings outside the repo, and decisions that are the owner's to make.
Everything else lives in GitHub issues and is meant to be machine-actionable.
An item here that unblocks an issue says so; tick it, then say so in the issue.

## Company and money

- [ ] Read the employment agreement for IP assignment and moonlighting
      clauses; get clearance if needed. Blocks everything below. (from #73)
- [ ] Meet an accountant: entity type and state of formation, sales tax /
      VAT on subscriptions (Stripe Tax or not, US-only first?), estimated
      tax, deductible costs, bookkeeping. Briefing and question list: #73,
      "OUTPUT 1". (from #73)
- [ ] Form the entity, get an EIN, open a business bank account, set up
      bookkeeping. (from #73)
- [ ] Sign an IP assignment (code, content, name, domains) from the founder
      to the company. (from #73)
- [ ] Connect Stripe under the company; decide Stripe test mode for e2e.
      Unblocks the credits part of #56.

## Legal

- [ ] Lawyer review of the Terms of Service draft (#72, "OUTPUT 1"), in the
      same session as privacy (#74) and AI-output ownership (#75).
- [ ] Get written answers from the lawyer on: anything too close to Duolingo
      or Kwiziq; whether a patent search is worthwhile; comparative marketing
      wording; trademark search/registration for "Skrongle"; the third-party
      franchise themes (wow, stardew, fallout). (from #72)
- [ ] Fill the ToS placeholders: company name, jurisdiction, contact and
      postal address, minimum age, refund policy, liability cap, arbitration.
      Unblocks the `/terms` page in #72. (from #72)
- [x] ~~Decide the privacy facts~~: the privacy notice is published at
      `/privacy` (#975): minimum age 14 (#976), logs and backups 31 days
      (#972), answers two years, failed courses 30 days. Still open for the
      lawyer review (#74): hosting region (the notice says the United
      States and Canada), controller status for hosted AI, DPO / EU-UK
      representative, UK ICO fee.
- [ ] Decide the AI-output position: ownership wording, the licence we take,
      the input warranty, the takedown process and DMCA agent registration.
      Recommended position: #75, "OUTPUT 1". Unblocks the in-app wording in
      #75. (from #75)

- [x] Add a `privacy@skrongle.com` alias at the mail provider, forwarding
      to the owner's mailbox, and set `SKRONGLE_LEGAL_EMAIL` to it before
      the privacy notice names a contact, so a later change of personal
      address does not mean re-notifying everyone (#978). The process is
      in `docs/privacy-requests.md`. (from #981)

## Product decisions

- [ ] AI metering numbers: defaults picked in #299 (docs/pricing.md: $5 a
      month with $1.50 of Skrongle AI, Free none, a $5 pack for $3.00,
      Haiku 5.5 tutoring, Sonnet 5.5 courses, dollars shown). Confirm or
      change them (all config); still open: a stronger paid model, and
      "sessions" instead of dollars. (from #56, #299)
- [ ] DietPi installer approach: catalogue, our own `install.sh`, a
      first-boot hook, an SD image, or a mix; where the script lives and how
      it is signed. Options in #96. (from #96)
- [ ] Publish the existing git history or a fresh one when open-sourcing,
      after the secrets scan in #57. (from #57)
- [ ] Link the `/self-host` guide from the landing page and footer, or keep
      it only under `/docs`. If public, flip
      `e2e/specs/smoke/self-host.spec.ts`. (from #101)
- [ ] Offer a paid "set it up for me" call, if demand shows up. (from #58)

## GitHub and outside services

- [ ] Make the repo public and pick a LICENSE. Unblocks #95, #96 and the
      download link in #101. (from #95)
- [ ] Turn on GitHub Discussions with a "Self-hosting help" category.
      Unblocks the help link in #101 and #58. (from #58)
- [ ] Open the DietPi software request saying we will maintain the
      installer, then open the DietPi PR and the DietPi-Docs PR once the
      blockers in #95 are done. (from #95)

## Real hardware

- [ ] Run every step of the self-host guide on a real Pi Zero 2 W from a
      fresh SD card. Record each step's time, the download size, power use and
      how many learners one Pi serves; fill the hidden `MEASURE` comments in
      `templates/self_host.html`. (from #101)
- [ ] Test the DietPi installer on a fresh DietPi image on a Pi Zero 2 W.
      (from #95, #96)
