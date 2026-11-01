# Pricing Skrongle AI

What a learner pays for the AI Skrongle provides on its own key (*Skrongle
AI*, [features](features#skrongle-ai-allowances-and-credits)), and why the
server never loses money on it. Every number here is a default in
`src/metering.rs`. You can change it in the environment
(`deploy/env.example`).

Skrongle mini has no plans. With `SKRONGLE_MINI=1` (what
`deploy/skrongle.service` sets) there are no prices or payments: every
account has every feature, and the server's AI key, when the owner sets
`SKRONGLE_AI_KEY`, is shared by every account with no monthly allowance. The
owner pays the provider's bill; `SKRONGLE_AI_DAILY_CAP` and
`SKRONGLE_AI_RATE_PER_MINUTE` bound it, and `ai_usage` still records every
call. Everything below is about the hosted site, where plans are on.

## The numbers

| | Price | Skrongle AI included |
|---|---|---|
| Free | $0 | none: Free brings its own keys, or buys credit packs |
| Basic | CA$3 a month (`billing::Plan`) | none: like Free, Basic brings its own keys or buys credit packs (#931) |
| Plus | CA$15 a month (`billing::Plan`; a Stripe subscription or a grant is Plus too) | $1.50 of list-price AI a month (`SKRONGLE_AI_ALLOWANCE_PLUS=1.50`) |
| Sign-up credit | free, once | `SKRONGLE_AI_ALLOWANCE_FREE` (default $0) of AI as credits, given when a new account first confirms its address; not monthly (#931) |
| Credit pack | $5 once (a one-off Stripe price), sold on every plan | $3.00 of AI, spent after Plus's allowance, never expires (`SKRONGLE_AI_CREDIT_PACKS=price_…:3.00:$5`) |

Guards: $10 of Skrongle AI a day across everyone (`SKRONGLE_AI_DAILY_CAP`), 12
Skrongle AI requests a minute per account (`SKRONGLE_AI_RATE_PER_MINUTE`).
Superusers see the day's spend against the cap on `/superuser`; learners
never do.

## The models and their list prices

Anthropic's list prices, checked 2026-10-19 (dollars per million tokens,
standard global routing):

| Model | Used for | Input | Output | Cache read | Cache write (5 min) |
|---|---|---|---|---|---|
| `claude-haiku-5-5` | the tutor (`SKRONGLE_AI_MODEL_TUTOR`) | 0.10 | 0.50 | 0.01 | 0.125 |
| `claude-sonnet-5-5` | writing and revising courses (`SKRONGLE_AI_MODEL_COURSES`) | 2.00 | 10.00 | 0.10 | 2.50 |
| `claude-opus-5-5` | the default for learners' own Anthropic keys | 4.00 | 20.00 | 0.20 | 5.00 |
| anything else (`*`) | an unlisted model | 10.00 | 50.00 | 1.00 | |

The meter charges each call at exactly these prices from the usage the
provider reports, so a dollar of allowance is a dollar of Anthropic's bill.
Where the bill could be higher, the meter rounds up:

- **Cache writes** cost 1.25 input tokens; the meter counts them so.
- **Unknown models** (another provider, a new model) are charged at the
  dearest rate until `SKRONGLE_AI_PRICES` lists them.
- **Haiku 5.5 over 100,000 prompt tokens** costs five times as much. A tutor
  prompt is the system prompt plus the last 24 entries of at most 1,000
  characters each, about 10,000 tokens at most, so it never gets there. An
  operator who moves course writing to Haiku should price it at the higher
  rate (`claude-haiku-5-5=0.50/2.50/0.05`), since course material can be long.
- **US-only inference** (1.1 times) is never requested.

## What $1.50 buys

From the meter's own estimates (`ai::job_tokens`, `tutor::REPLY_MAX_TOKENS`):

- **A tutor turn** on Haiku: about 4,000 tokens in and 250 out, $0.0005; at
  the most (10,000 in, 700 out) $0.0014. $1.50 is 1,000 to 3,000 turns.
- **A lesson** on Sonnet: about 10,000 tokens in (the course format reference
  plus the request) and 6,000 out, $0.08; the outline $0.05. A 3-lesson
  course is about $0.29, a 6-lesson one $0.53. $1.50 is about five
  3-lesson courses. Real lessons usually come in under the estimate.

## The margin

Per paid learner per month, if they use every cent of their allowance:

| | |
|---|---|
| Subscription | $5.00 |
| Stripe's fee (2.9 % + $0.30, a US card) | − $0.45 |
| Skrongle AI at list price, at most | − $1.50 |
| A job's last request may run past the allowance (one lesson at most: 16,000 output tokens on Sonnet) | − $0.18 |
| **Left, at worst** | **$2.87 (57 % of the price)** |

A credit pack: $5.00 − $0.45 − $3.00 − $0.18 = **$1.37 (27 %)** at worst.
Free learners cost nothing: they are never offered Skrongle AI. Learners on
their own keys cost nothing either (their calls are recorded at $0).

The server checks before every call, so a turn or a job can only start
while the learner has allowance (a job's estimate is held up front). The
only overshoot is a job's last request, above. The daily cap bounds a bug.

## Why one key and our own meter

Anthropic's Admin API was checked (2026-10-19) for a key or a workspace per
learner:

- **Workspaces:** 100 per organisation by default (archived ones don't
  count). That is not enough for one per learner, and each would need its own spend
  limit, which you set in the Console, not through the API.
- **API keys:** the Admin API lists and updates keys but does not create
  them. You make keys in the Console (Claude Code also mints them in its own
  workspace). The app could not issue one per learner.
- **Usage and cost reports:** usage shows up "typically within 5 minutes";
  the cost report is daily buckets only; polling once a minute is the
  sustained limit. That is too late to stop a learner before a call, which is when
  the allowance has to be checked.
- Prompt caches are per workspace, so splitting learners would also lose
  the cache.

So the server keeps one key (`SKRONGLE_AI_KEY`) and meters each learner in
`ai_usage` (`src/metering.rs`), checking before every call. The check reads
running totals, not the rows: `ai_usage_months` (per learner and UTC
calendar month) and `ai_spend_days` (everyone, per UTC day), which triggers
on `ai_usage` keep equal to the sums (#623, #624).

## Reconciling with Anthropic

Once a month (not built yet; `TODO(reconcile)` in `src/metering.rs`):

1. Put the server's key in a workspace of its own, and make an Admin API key.
2. `GET /v1/organizations/cost_report?starting_at=<1st>&ending_at=<1st of next>&group_by[]=workspace_id`
   with the Admin key: that workspace's cost for the month.
3. Compare it with `SELECT SUM(cost_micros) FROM ai_usage WHERE key_source = 'hosted' AND created_at >= <1st>`.

A meter that reads lower than the bill means the price table is out of date
or a model is missing from it. Fix `SKRONGLE_AI_PRICES` before the next month.
