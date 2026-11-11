# FAQ

**Do I need an API key?**
No. Courses, the four modes and every game work without one. A key (Claude,
OpenAI, Gemini, DeepSeek, Mistral, Grok or OpenRouter; or your own
Ollama-compatible server, which needs none) is only for AI tutoring, generating a course and adding
content with AI. The server's owner saves it once under *Manage Users → AI
features*, and every account uses it; it is stored encrypted and never
shown again. Don't want AI at all? The owner turns on *Hide AI features for
everyone* in the same place.

**I passed a lesson in Sprint. Why does Study mode still say "in progress" or "needs practice"?**
Passing and mastery are different. A pass is a milestone that stays. Mastery
is a score that rises with practice and fades with time. See
[Getting started](getting-started#when-a-lesson-is-passed).

**Why is a game missing from my course?**
A feature only appears when the course has the data for it. Date Dash needs a
`calendar`, the drill a `paradigm`, Listening a language course with `listening` sentences. See
[Which field switches on what](course-structure#which-field-switches-on-what).

**Why is there no *Create a course* button on my course list?**
Making a course (creating one, copying one, copying and translating one)
needs the *Create courses* permission, and your account does not have it.
Ask the person who runs your Skrongle, or whoever manages its accounts: they
turn it on under *Manage Users → Permissions*. You can still learn from the
ready-made courses and from courses shared with you, and courses you already
made stay yours to open and edit.

**Can other people see my course?**
Only the people you share it with: **Share** on the course page makes an
invitation link, which you pass on; whoever opens it first is in. They learn from it read-only, with their own progress. See
[Share a course](making-courses#share-a-course).

**My course does not load. What now?**
The error names the file and the field. See
[Fixing validation errors](making-courses#fixing-validation-errors).

**Can ChatGPT, Claude or another LLM write my course?**
Yes. Give it [`/llms.txt`](/llms.txt) or paste the
[course format](course-format.md), then add its replies on the course page.
See [Write lessons with an LLM](making-courses#write-lessons-with-an-llm-you-already-use).


**I forgot my password. How do I get back in?**
Ask whoever runs your Skrongle. On the Superuser page they set a temporary
password for your account and give it to you; you choose your own at the
next sign-in. Lost the phone with your authenticator app? They can turn
two-factor sign-in off for you there too. Skrongle sends no e-mail, so there is no reset link.
See [Accounts and AI keys](self-hosting#accounts-and-ai-keys).

**Why does my Skrongle mini say it has all the accounts it can hold?**
A Skrongle mini holds at most 8 accounts (a household plus a couple of
guests). The number is fixed, not a setting: a small known size keeps the
server light enough for a Pi Zero 2 W. Nobody new can register, by form,
invitation link or proxy sign-in, until an account is deleted. See
[Accounts and AI keys](self-hosting#accounts-and-ai-keys).

**Can an AI agent outside the app record my progress?**
Yes, through `/api/progress`. Give the agent [Progress across modes](progress).

**I'm a developer. Where are the code docs?**
[Architecture](architecture).
