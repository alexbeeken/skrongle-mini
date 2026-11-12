# Getting started

Skrongle teaches from courses. A course is a set of lessons, and every mode
of the app practises the same lessons.

## First steps

1. Register with your e-mail address. There are no usernames. Registering
   signs you in.
2. On *What would you like to learn?* (`/courses`) pick a ready-made course
   (Spanish, French and *How to Use Skrongle*, a tutorial for the app
   itself, ship with it), or *Create a course* of your own, by hand
   or [with AI](making-courses). (No *Create a course*? See the [FAQ](faq).)
3. The first time you open a course, a short wizard sets it up: the
   motivator and, last, where to start (for a course whose units are
   levels, like Spanish's A1, A2…; a course with a placement test lets you
   take it to find out). Starting further in skips the earlier lessons
   without passing them. Change any of it later from the gear on the
   course's card on `/courses`.
4. The course's home page leads with today's goal and *Continue*, then the
   modes. Pick one.

Progress is per course. Switching course switches the home page, mastery,
vocabulary and history with it.

## The modes

| Mode | Where | What you do |
|---|---|---|
| **Sprint** | `/sprint` | Five-minute phone sessions of short exercises along a path of lessons. The daily goal ring, day streak, a review mix and a 60-second lightning round. |
| **Study** | `/lessons` | Read each lesson, pass its two-question reading check, then practise. Lessons, Practice, Listening, Map, Vocabulary, History, Mistakes. |
| **Play** | `/play` | Games: Word Match, the conjugation drill, Numbers by ear, Date Dash, Flashcards. |
| **Guide** | `/guide` | Each lesson as a stage of small steps: cards, quick checks, flashcards, tips. Stars. |
| **AI tutoring** | `/tutor` | A one-to-one lesson over chat. Needs an API key (Claude, OpenAI or Gemini) on the server, which the owner saves on *Manage Users*. |

A game or exercise only appears when the course has the material for it. A
course without a calendar has no Date Dash, a maths course has no listening.
Language courses also let you tap a word to hear it.

Every mode fills the same daily goal, counted in effort points: reading, a
game or a quiz answer all move it. See
[The daily goal](progress#the-daily-goal).

## When a lesson is passed

A lesson is **passed** once both are true, in any order and any mode:

1. **Seen**: you read it to the end in Study mode (and passed its reading
   check), or played it as a Sprint session or a Guide stage, or a tutor
   taught it.
2. **Shown**: in one sitting you answered at least 3 of its exercises (all of
   them if it has fewer) and got 70 % or more right.

A pass in one mode counts in every mode: it completes the lesson's stop on the
Sprint path and its Guide stage, unlocks the next, and puts a ✓ on the lesson
in Study mode. A pass stays even after a bad round later.

**Mastery** is separate: a 0–100 score per lesson that rises with right
answers, falls with wrong ones and fades after a few days without practice.
Practice uses it to pick your weak lessons. 80 or more is "mastered".

Full rule: [Progress across modes](progress). Every feature in detail:
[Features](features).

## Your profile

The badge in the top-right corner opens the menu: profile, switch course,
documentation, log out. It shows a gear. On the profile you
set your name and the app's language. A course's
motivator belongs to the course: the gear on its card on
`/courses`.
Changes save as you make them; there is no Save button. The AI API keys and
*Hide AI features for everyone* are the server's, on the owner's *Manage
Users* page: one set of keys serves every account.
