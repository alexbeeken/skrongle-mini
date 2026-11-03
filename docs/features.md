# Features

Every feature reads the same course files; nothing needs its own content. A
lesson does not appear in a feature whose data it lacks. All progress
is per course: switching course switches the home page, mastery, vocabulary,
games and history with it. Progress is also shared across the modes: a
lesson passed in Sprint mode on the bus is passed in Study mode at home (see
[Progress across modes](#progress-across-modes)).

## The modes

Once a course is open, the home page leads with today's goal (#558): Sprint
mode's ring, large, filled by the effort points earned today in any mode
(see [Progress across modes](progress), "The daily goal"; what earns what is
behind its (i)), a line about the day, chips for the day streak, lessons
passed of the total, terms collected and the skipped lessons when there are
any, and *Continue: <lesson>*, which opens the lesson every mode resumes at
in the mode last used ("Where you left off"). Under it, in the same card, the
unit of that lesson ("Unit n of m" and a bar of its lessons passed; the map
itself is the Study nav's *Map*). Then come four modes plus AI tutoring. Each
mode card shows its name, a one-line tag, one stat and a big button. It has
no list of links (#1263); each mode's own page and bar carry those. What the
mode is lives behind the card's (i), in a modal. The top bar
only carries the course switcher and the account links.

- **Sprint mode** (`/sprint`): phone-first five-minute sessions with the daily
  goal's ring and a streak. Its page offers the path, the review mix and the
  lightning round; its home card names the next stop (the lesson to resume at).
- **Study mode** (`/study` opens its Lessons tab): read the lessons and test
  yourself. Lessons, Practice, Listening, Map, Vocabulary, History, Mistakes.

In a language course, every word that matters can be tapped to hear it (see
[Tap to hear](#tap-to-hear)).
- **Play mode** (`/play`): games and one-off exercises. Word Match, the
  paradigm drill, Numbers by ear, Date Dash and Flashcards (the
  Play mode bar also links the Guide). "Play" is its one name: the home card,
  the mode bar, the page title and the game pages' breadcrumb (`Play › Word
  Match`).
- **Guide** (`/guide`): guided lessons, one stage at a time (see the Guide row
  below). Its home card shows stages complete of those not skipped (from
  the same rule as `/guide`); *Start the Guide* /
  *Continue the Guide* opens the current stage, *See your stages* the list
  once every stage is done. Its icon is the `mode_guide` slot (🧭). "Guide"
  is its one name (home card, page heading, Play mode bar); a *stage* is one
  lesson inside it. The list at `/guide` has one section per unit:
  open and passed stages as cards, and the unit's locked stages folded into
  one "N locked stages" row under a single lock that opens to a plain list
  (#1213).
- **AI tutoring** (`/tutor`): a one-to-one lesson over chat with a tutor that
  runs on the learner's own API key. Same exercises and progress as Practice,
  but the tutor teaches first and sets them when you are ready. Its prompt
  says the learner may be as young as the Terms' minimum age
  (`SKRONGLE_MIN_AGE`): it stays on the course, declines sexual, self-harm,
  violent, hateful or illegal turns and goes back to the lesson, and never
  asks for personal details (`tutor::safety_rules`, #919).

The modes' old paths (`/d`, `/d/…`, `/k`, `/g`, `/games`, `/games/…`) redirect
to the new ones with their query strings, so old bookmarks still work.

| Feature | Route | What it is | Reads |
|---|---|---|---|
| Courses | `/courses`, `/courses/{id}/settings` | "What would you like to learn?": your projects (each shows a thin progress bar under its title, pale gold for lessons passed and full gold for lessons mastered, and under it two small chips with the same two numbers and the unit count; when you last opened it sits beside *Continue*. A chip turns from grey towards gold as more of the course is done, shows its name on hover, and opens a modal with the stat when clicked. The modal fetches what is next as it opens (`GET /courses/{id}/stat/{metric}`): for lessons passed, the next lesson to pass, the last one passed and each unit's count; for lessons mastered, up to three lessons to practise next, fading ones first. Each lesson named opens the course at it, and an (i) says what the stat counts. Each card has a gear for its settings: level and motivator), your own courses (private; greyed out while being generated), ready-made courses, Create a course | manifests |
| Generate with AI | the editor's *Generate the course* dialog (`POST /courses/{id}/generate`) and progress modal | Notes or homework in, a whole private course out, written with the learner's own API key; see [Your own courses](#your-own-courses) | everything (it writes it) |
| Starting level | The course's settings: the level step at the end of its start wizard, then the gear on `/courses` | A course whose units are `levels` asks a learner who opens it for the first time which level to start at; the lessons below it are skipped; the level can be changed later without losing progress (see [Starting level](#starting-level)) | `levels`, units |
| Home | `/` | Today's goal as a ring with the streak, lessons passed and terms collected, *Continue* where you left off, the current unit's progress, a "Words due" card on days collected words are due (#1360: the count, three of the words, an (i) with the ladder, and *Review N words*, which opens `/vocabulary/study?due=1`), then the four modes (Sprint, Study, Play, Guide) and AI tutoring, with one stat and one button each and an (i) explaining each | everything |
| Landing | `/` (no session) | The sales page for visitors: what the app is, a twenty-second sample of each mode to play in place (Sprint, Study, Play and Guide tabs, each in Language, Maths and Music, each subject with its own kinds of question: listening and a typed word with accent near misses; shading a fraction, a number line, plotting a point, a formula graded by equivalence, graphs as options and pairs; a staff to read and to place a note on, a keyboard, intervals and chords played by a small synth. Several preview question types still in issues and PRs (#1218–#1234, #1330–#1359). `static/landing.js` holds the samples, `static/landing-kit.js` the drawing and sound; #1365), a lesson file next to the features it switches on, the three ways to get a course, the built-in courses with their unit and lesson counts, "What can I do with it?" (use cases), register / log in | manifests |
| Lessons | `/lessons`, `/lessons/{slug}` | One unit at a time (#837): a tab per unit with its passed count and bar, 12 lessons a page with page links (`?unit=<id>&page=<n>`; without `?page=` the page holding the next lesson to do, without `?unit=` the unit holding it; `?unit=all` lists every unit on one page). A filter box searches every unit (every word typed must appear in a lesson's title, group or summary; a search that finds nothing says so; without script it is `?q=`); under the box, *Look up "…" in the words* opens `/lookup?q=` (#348): the words of the whole course whose term or meaning has a word starting with each word typed (accents and case ignored), and the forms of its paradigm tables (a form names its item, set and slots, and a form two verbs share is listed for both), at most 20, exact terms first; each with its meaning, the lessons that teach it (linked), *Collected* with the flashcard tally when you have it, and a 🔊 in a language course; a quiet header (#999): the breadcrumb with a small mastery ring, the status pills and an (i) on one line, then the title, the summary and one main button (I've read it, quiz me until the lesson is seen, then Practise; a lesson without `quiz` questions has no mastery ring, no Practise and no Test yourself, and its button just says I've read it, #1197) beside a "More ways" menu (Practise before the lesson is seen, Listen and type when the lesson has listening, Tutor when AI is on, the Guide, a Sprint session). The (i) opens the mastery details, the focus and question counts and the pass rule; the footer has its button and the same (i); the lesson text (read as a page), vocabulary table (hover a term for its meaning, an example for its translation), paradigm tables, conversations (#989: each one as two characters talking, the role's name over its line, A on the left and B on the right; each line is a button, and tapping it reads the line aloud in the course's language (when the audio switch is on and there is a voice) and shows its meaning under it, which a second tap hides again (#1264); without JavaScript the meanings show from the start), a "My note" box (#336: the learner's own private note on the lesson, plain text up to 500 characters, saved as they type, in *Download my data*; its (i) says so), and the two-question reading check that marks the lesson as read | `content`, `vocabulary` (`example_meaning` for the hover hint), `conjugations`, `conversations`, `quiz` |
| Quizzes | `/practice`, `/practice?lesson=` | Adaptive micro-quizzes: 5–10 questions from the lessons you have read, weighted towards weak topics; or one lesson on request (a lesson with no questions, reached by its URL, says so and links back to the lesson and its Sprint session) | `quiz` |
| Listening | `/listening`, `/listening/play?lesson=` | Hear a sentence, type it, see it checked word by word; one lesson or a mixed round | `listening` |
| Guide | `/guide`, `/guide/{slug}` | The short-attention-span version: each lesson as a stage of small steps (cards, quick checks, flashcards, tips); stars | `content`, `quiz`, `vocabulary`, `guide` |
| Sprint mode | `/sprint`, `/sprint/play` | Phone-first five-minute sessions of short exercises: bites, sort / spot / build / ladder / scale / pick / match exercises (a sort judges each tap at once, a ladder or a scale is graded only at Check), five listening exercises (tap what you hear, fill in what you hear, what do you hear, select the matching audio, listen and match), an audio switch, streaks, daily goal, lightning round | `sprint`, `quiz`, `vocabulary`, `conjugations`, `conversations`, `listening` |
| AI tutoring | `/tutor`, `/tutor?lesson=` | A one-to-one tutor over chat: teaches the lesson, checks you follow, sets the adaptive quiz's questions when it judges you ready, and reacts to the app's grading; you answer in the chat (typed or a tapped choice) and can ask anything in between; needs an API key from the profile; at most two tutor replies run at once on the server, and a third learner gets the busy message and tries again | `content`, `vocabulary`, `quiz` |
| Play | `/play` (also `/g`) | The Play mode hub: Word Match (timed pairs, shared leaderboard), the paradigm drill (type the form for a picture), Numbers by ear (hear a number, type the digits), Date Dash, plus links to flashcards and the Guide | `vocabulary`, `conjugations`, `terms.lang`, `calendar` |
| Map | `/map` | Every lesson of every unit in order with your mastery; how far you are from mastering the unit; one "you are here" on the whole map, at the lesson every mode resumes at ([where you left off](progress)) | units, mastery |
| Vocabulary | `/vocabulary`, `/vocabulary/study` | Terms collected by answering questions, studied as flashcards with per-term review stats. Each word has a review schedule (#1360, `src/vocab.rs`): a new word is due tomorrow; *Knew it* on a due word moves it up the ladder of 1, 3, 7, 16, 35, 75, then 150 days; *Didn't know* drops it back to 1 day. A review before the word is due counts in the stats but doesn't move it (a miss still drops it). Only flashcards and Sprint's word exercises move it; Sprint's review mix deals due words first. "Due today" is by the end of the learner's day. `?due=1` deals the due words, longest overdue first, at most 20; a missed card comes back once at the end, and the summary says how many come back tomorrow and when the next words are due. Passes don't change; each card gives 0.3 effort points | `vocabulary`, `quiz[].teaches` |
| History | `/history` | Every answer and every finished game | mastery, activity |
| Mistakes | `/mistakes` (the *Mistakes* tab of the Study mode bar, #1263) | Quiz questions whose latest answer was wrong, newest first. Each row has the question, your last wrong answer and the right one. A question leaves the list once you answer it right. Reads the last 500 answers and shows up to 50 (#338). A lesson without a quiz never appears | `quiz` |
| Motivators | `POST /courses/{id}/settings` (`motivator`) | How progress is dressed up, per course: draws the top of the course home page and the reward line at the end of every lesson or session (`src/motivators.rs`); Statistics and metrics by default | none (app-level) |
| Progress API | `/api/progress…` | Lesson passes and answers reported by AI agents that teach the learner outside the app (see [Progress across modes](progress)) | everything |
| Docs | `/docs`, `/llms.txt` | This documentation, served from the binary: "Using Skrongle" (learners, authors, LLMs) and "Working on Skrongle" (developers) in the sidebar, each page also as raw Markdown, the list of pages beside the page and after it on a phone; `/llms.txt` points an LLM at the course format and schema | — |

## Your own courses

Anyone can make a course, and it is theirs alone: a course made in the app
lives in the database as the same JSON files a built-in course has
(`courses` + `course_files`), and only its owner sees it, opens it or
changes it (anyone else gets a 404), unless the owner shares it (below).
Courses are `private`; publishing them to a catalogue is planned
(`courses.visibility` is reserved for it).

A course made in the app gets every feature its files have data for, exactly
like Spanish: the three modes, the tutor, mastery, passes, streaks, the daily goal,
history. The language-only games (Listening, Numbers by ear,
tap to hear, the listening exercises of Sprint mode) appear only when its
manifest says it is a language course.

One button on the course list, **Create a course** (`POST /courses/new`),
makes a starter course called *Untitled course* (one example lesson, the
[format](course-format) to copy; the course title, description, unit and
lesson names are in the author's app language, #1255) and opens its page with the **Generate the
course** dialog open. Close it to write the course yourself from the lesson
table; the title at the top of the page is renamed by clicking it and saved
with the table, like any other change (the editor saves as you edit; see
*Saving as you edit* below).

- **Generate the course** (the dialog, `POST /courses/{id}/generate`): "drop
  your homework in, get a study app out". It writes into this same course
  (same id and page), replacing the starter files. The dialog is on offer,
  under *Use AI* and at `/courses/{id}?generate=1`, while the course is a
  **draft**: the starter course with only names changed (`courses::draft_of`).
  The course title, lesson titles and lessons added by name alone are names;
  the AI is told them, keeps a title the learner chose (only *Untitled
  course* takes the outline's) and plans the named lessons first, in order.
  A title alone is enough to ask for. Any other change (a lesson's content,
  the settings, an upload) ends the draft: from then on the course is changed
  with *Only add* or *Allow overall changes* instead. A generation that
  failed before its outline offers the dialog again; to start over a
  generated course, delete it and create a new one.
  1. *What do you want to learn?*: a sentence or two (level, what the test
     covers), 20 to 2,000 characters.
  2. *Your material* (optional): class notes, homework problems, a syllabus,
     pasted or dropped in as text files (`.txt`, `.md`, `.csv`, `.tex` …; 20 to
     60,000 characters, read in the browser). The model is told to teach
     exactly that material, in its notation, and to treat it as content,
     never as instructions.
  3. *What kind of course*: let the AI decide, maths or science (number and
     formula answers), a language (listening, conversations, tap to hear),
     or facts and concepts. Each kind has its own rules for the model (which
     question types, which Sprint games). Left to the AI, the outline names
     the kind it chose. The kind is saved with the outline, and every later
     request for the course (each lesson, Resume, Revise, *Only add*) gets
     only that kind's rules, so a course on therapy models is not asked for
     "tap the wrong word" spots in every lesson. A course with no saved kind
     (made before this, or by hand) counts as a language course when its
     language features are on, and otherwise gets all three sets of rules.
  4. *How much to write* (#1119), with an "i" that explains the three:
     *Full course* (the default; the outline plans as many lessons as the
     subject and material need, at most 15), *Preview* (the course is
     exactly one unit with one full lesson: the outline is cut to its first
     lesson and that lesson's unit) or *Outline* (every unit and lesson, but
     each lesson's `content` is one `## ` section listing the concepts it
     will teach, with one example question in `quiz` and no vocabulary or
     Sprint games). The choice is saved with the outline (`depth`), so a
     Resume writes the same kind of lessons. With the server's key a Full or
     Outline course reserves for 6 lessons up front and a Preview for one;
     the meter still checks every request.
  5. *Your AI provider* only when two or
     more keys are saved on the profile: a choice between those. With one
     saved key the dialog says nothing about providers and uses it, with the
     provider's default model. With no key at all the dialog opens greyed
     out (every field and *Generate* disabled) under a line linking to the
     profile's API keys, so nobody writes a prompt only to be stopped at
     the end. Keys are added on the profile only; the server still takes a
     pasted `api_key` (with `model` and `base_url`) from API clients. A
     `base_url` needs a pasted key: a saved key only goes to its provider's
     own address. That URL must use port 443 or 80 and resolve to public
     addresses only, unless the operator sets
     `SKRONGLE_ALLOW_PRIVATE_AI_URLS=1` (dev mode allows it too); redirects
     are never followed.
     Under *How much to write*, an estimate: "6 lessons · about 5 minutes ·
     roughly CA$1.48" (#443). It counts an outline request and one request
     per lesson (the depth's expected count: 6, or 1 for a Preview), each carrying the format reference and the material
     (`ai::job_tokens`), at about 45 seconds a
     lesson (`job_minutes`, `src/routes/courses/generate.rs`). An own key is priced at its provider's
     default model from the price table (`src/prices.rs`, `SKRONGLE_AI_PRICES`),
     turned from US into Canadian dollars at a fixed 1.40
     (`CAD_CENTS_PER_USD`);
     a model the table does not list (it would get the high `*` rate) shows
     only the lessons and the time. `static/gen-estimate.js`
     works it out again as the count, the material or the provider changes;
     the (i) beside it says how it is worked out and that it is rough.
  6. *Advanced: edit the instructions*. The system prompt every request
     carries is shown and can be edited, behind a warning: instructions that
     fight the format waste API credits on retries and make courses that do
     not load, and pasted instructions can make the model write things you
     did not ask for. The rules for the kind of course, the course format
     reference and the outline are always added after them; *Reset* puts the
     default back.
  6. *Effort* (#918), the button beside *Generate* (`generate-effort`),
     opens a small dialog (`generate-effort-dialog`) with three levels:
     *Fast*, *Balanced* (the default) and *Thorough*. Every provider gets the
     same model at every level, with `output_config.effort` (Anthropic) or
     `reasoning_effort` (OpenAI, Gemini) set to `low`, `medium` or `high`
     (`ai::Effort`, `src/ai/budget.rs`). Thorough lets each request write
     twice as much (`max_tokens` 32,000). The level posted
     is saved on the account (`users.ai_effort`) and is where the dialog
     starts next time. It applies to *Generate the course* and its Resume
     only; the tutor, *Use AI* and translations send no effort.

  Under the two boxes a **live count** reads "45 / 2,000 characters"
  (trimmed, counted as the server counts): red with "· at least 20" below
  `ai::MIN_PROMPT` (the material, being optional, may also stay empty), red
  with "· at most …" above the maximum (`maxlength` stops typing there),
  normal text in between. The form needs one of the two filled in (or a
  course title), and each one that is filled in within its limits. The
  browser checks that before posting, with the server's words
  (`generate_in` refuses the same): a refusal opens in place, without a
  reload. The error eases open at the top of the dialog, which stays put
  (it is held at the top of the screen, not centred) and scrolls up to it
  smoothly; the box to fix turns red and keeps the focus, and the error
  closes again once it is fixed. Other refusals (no key, too many jobs)
  come back from the server with the dialog open and the error at its top.

  On submit the course is handed to the job at once and the page comes back
  (`/courses/{id}?job=<job>`) with a **progress modal**: the phase and a
  progress bar, followed live, and *Cancel*; it cannot be closed while the
  job runs, and it is back whenever the course's page is opened until the
  job ends. When the course is ready the modal goes away, leaving the
  course's page with its lessons. On the course list the course is
  **greyed out as "generating"**, with the same phase and bar and a *Watch*
  link to its page. The job runs on its own OS thread: one request for
  the outline (the manifest and the lesson plan, saved straight away), then
  one per lesson, each validated with the normal loader and sent back to the
  model with the validator's message up to twice. Each lesson request also
  lists the terms and section headings of the lessons written before it
  (about 2,000 characters at most), a map of the course (each unit with its
  lessons in order, this one marked), and the whole lesson just before and
  just after it (the plan for one not written yet), so a later lesson builds
  on them and uses the same words instead of teaching them again. Until it
  finishes the course cannot be opened or edited. *Cancel* stops after the
  request in flight. A run that stops (an error, a cancel, a server restart) keeps its
  outline and every lesson written, but a half-written course is never shown:
  a course whose first generation stopped has **no editor** (its pages
  redirect to the course list). Its card shows a **red !** by the title
  ("An error occurred during generation." on hover) that opens a modal with
  the provider's or the validator's own text and nothing else; the plain
  explanation with a next step (`ai::plain_failure`) is in the generate
  dialog. A provider account whose credit or usage limit is used up
  (Anthropic's 400 "usage limits" / "credit balance", OpenAI's 429
  `insufficient_quota`) reads "Your AI provider account has run out of
  credit or reached its usage limit. Check your plan and billing with the
  provider, then try again." (#297; before, "turned the request down" or
  "busy"). The card also has *Delete*, and one button that opens the dialog on
  the course list (`/courses?generate=<id>`): **Resume** when the outline is
  saved (it writes only the missing lessons), else **Retry** (start over,
  with the first run's wish and notes filled in). A save that still reaches a
  stopped run with its outline (a stale editor tab, the file form, the upload
  API, Undo) leaves it stopped, with its reason and Resume, even when the
  files then load (`courses::revalidate`); Resume keeps the files that load,
  edits included. Only a run that stopped before its outline becomes a course
  when its files are fixed by hand. When the run stops while
  its progress modal is open, the page goes straight to that dialog, with
  the reason at its top.
  The key is used for the job and dropped with it; it is never stored with
  the course. One job per learner, two in all (a slot is free once the
  job's thread has ended), at most 10 started per hour and 40 per day per
  learner, and an hour per job (`SKRONGLE_AI_JOB_BUDGET_SECS`). What the AI
  writes keeps to the editor's caps: at most 15 lessons in an outline, each
  file at most 1 MiB, at most 400 files, and a model reply over 1 MiB is
  not read. Once the course is ready
  its notes are deleted within the hour (`SKRONGLE_KEEP_NOTES=1` keeps them for
  later revisions); a stopped course keeps them so it can resume, and is
  deleted after `SKRONGLE_FAILED_COURSE_RETENTION_DAYS` (30) days without a
  resume or edit. A line under the notes box asks not to paste sensitive
  personal data (Use AI's box carries it too).
- **Use AI** (add content with AI). One box on a course's own page
  (`POST /courses/{id}/ai`, `text` + `mode`), for any course of your own
  that loads and is free. Like a generation it has no status page, and the
  page does not change: the form is sent in the background and the job's
  **modal** comes up over the course page, which stays as it is but inert
  (the lesson table, Use AI and the rest can't be used) until the job ends
  (without JavaScript the post lands on `/courses/{id}?job=<job>`, the same
  page). Coming back while it runs draws the same workspace, inert, under
  the modal; the modal's *Cancel* is the only one. *Generating…* with the phase and a
  progress bar while it runs (the page polls `/api/generate/{job}`), and a
  *Cancel* that stops it after the request in flight; when the job ends the
  page reloads: a job that changed the course opens **Review AI changes**
  (below) on what it changed, with the new lessons in the lesson table and
  Undo under it once it is closed (with *Don't show this automatically next
  time* ticked, the page says the AI has finished instead); a job that stopped shows the error in
  plain words (a cancel, a reply that could not be used, a refused Only
  add) with *Close*. The course page shows the modal again whenever the
  course is being revised, so coming back to it finds the job still running.
  The old status page's address (`/courses/generate/{job}`) leads to
  the course page too, for any job. It uses the learner's first
  saved provider key (profile, *AI API keys*); it never asks for a key or a
  model, and names it only when two or more keys are saved. Without a saved key it shows only a red line linking to the profile,
  and the server refuses the same way. A three-way choice, *Add* / *Edit* /
  *Add and edit* (#1241, radios `mode` = `add`, `edit`, `change`), top right
  of the Use AI box, picks the mode; one "i" beside it explains all three in
  a line each. At phone width the choice takes its own full-width row under
  the heading. A switch, *Outline only* (with its own "i"), makes the lessons
  the job writes, the added one or the rewritten and new ones, outlines like
  the Generate dialog's *Outline*; it is greyed out while *Edit* is chosen,
  since an edit always writes full lessons. The text takes 20 to 1,000
  characters for *Add* and up to 4,000 for *Edit* and *Add and edit*; the
  box's label ("What should the AI add?", "…change?", "…add or change?"),
  placeholder and live count follow the choice, the count red outside those
  limits (as on the Generate dialog), and the server refuses the same
  lengths. A refused request comes back with the same choice and text:
  - *Add* (the default, *Only add*): one new lesson, written and placed by the AI
    (see *the lesson table* below). The result is checked in code, not
    trusted to the prompt (`arrange::only_adds`): every existing file stays,
    `course.json` byte for byte and each lesson as it was except its `order`;
    anything else and nothing is saved.
  - *Edit* (*Edit lessons*, `mode=edit`, #1240): rewrite lessons already in
    the course from feedback, adding, removing and moving none. Any course
    that loads, made with AI or by hand, finished or not. Below, with the
    feedback revision.
  - *Add and edit* (*Allow overall changes*, `mode=change`; any finished
    course, made with AI, by hand or uploaded): a revision from feedback,
    below. On a course that cannot be revised it stays in place, greyed out,
    with the reason in the "i".
- **Change it with feedback** (*Allow overall changes*). The learner says
  what should change or what to focus on ("more word problems", "simpler
  English", "drop the history lesson"). One request plans the revision against the
  course as it is: each lesson is kept, rewritten (same id and slug, so
  progress stays with it) or new, and a lesson left out is removed. Then one
  request per lesson to write, with the current file and the feedback. Kept
  lessons stay byte for byte (except that a prerequisite pointing at a removed
  lesson is dropped, and `order` is set when the plan puts lessons in a new
  order). While it runs the course is `revising`: it still opens and works as
  it was, but its files can't be changed. Nothing is saved until the whole new
  course validates; then the files are swapped in at once, so a failure, a
  cancel or a restart leaves the course exactly as it was. The previous files
  are kept for **Undo** (see below).
  **Edit lessons** (`mode=edit`) works like this revision with stricter rules.
  Its plan (`edit-plan`) may only keep or rewrite each lesson, every lesson
  once and in the order it is in, and leaves `course.json` alone; a plan that
  adds, drops or reorders is refused and asked for again once. A rewrite keeps
  its id, slug, unit, group and prerequisites and may take a new title. The
  result is checked in code (`arrange::only_edits`): the same files, each
  lesson keeping its unit, `order` and place, `course.json` and every other
  file byte for byte, and at least one lesson changed. Anything else and
  nothing is saved ("the edit was refused because it added or removed a
  lesson (…). The course is unchanged."). It always writes full lessons:
  *Outline only* is ignored, since an outline would throw away a lesson's
  content. Its text takes 20 to 4,000 characters, as a change does. The modal reads
  *Editing "…"…* and "2 of 3 lessons rewritten", *Editing stopped* when it
  fails, and the Undo row and Review AI changes say *Edited with feedback:
  "…"*.
  **A fact check** (#1238): when the feedback asks for one ("Check the facts
  in every lesson and fix what is wrong"), either plan marks every lesson
  `check`. One `check-lesson` request per lesson reads it for errors and
  sends it back unchanged unless a fact is wrong; an unchanged lesson stays
  byte for byte and is not written. A checked or rewritten lesson comes back
  with *why* lines, one per fix ("Changed 'La Paz is the capital of Bolivia'
  to 'Sucre is the constitutional capital; La Paz is the seat of
  government'"), and *Check* lines for what the model was unsure of and left
  alone. Both are kept with the run (`course_ai_runs.why`) and shown in
  Review AI changes. A check that finds nothing wrong stops with "The check
  found nothing to fix" and changes nothing.
  **Edit with AI on one lesson** (#1242): the ✦ on a lesson's row in the
  lesson table opens a small dialog, *Edit "&lt;title&gt;" with AI*, with one
  request box ("Up to 4,000 characters. Only this lesson changes."),
  Submit and Cancel, and the same provider note as the Use AI box. It posts
  to the same route with `lesson=<the lesson's file>` (an edit, whatever
  `mode` says) and runs as a job the editor's modal follows (`?job=`). One
  request (`edit-lesson`, no plan) gets that lesson's file, `course.json`
  and the request, and replies with the whole lesson. The result is checked
  in code (`arrange::only_edits_one`): that file is the only one that
  changed, with the same id, slug, unit, `order` and place; anything else is
  asked for again (three tries in all), then refused with the course
  unchanged ("the edit was refused because it changed the lesson's id
  …"). The modal reads *Editing "<lesson title>"…* and *Editing stopped*;
  Undo and Review AI changes say *Edited with feedback: "…"*, as for a
  course-wide edit. A refused request
  (too short, the lesson gone) opens the dialog again with the
  text and the reason. The icon is only there when the Use AI box has a key
  to use (a saved key), and not on a row that is not saved
  yet. Edits across many lessons still go through *Edit* in the Use AI box.
  *Download all files* (`/courses/{id}/export.json`) is one JSON
  object: `"version": "1.0"` first, then every file by its path.
- **The lesson table.** Every course of your own (made by hand or with AI)
  that loads shows its lessons as a table on its page (`/courses/{id}`):
  number, name and delete, no file names (without JavaScript, an *Edit*
  link per row opens its file instead). The first row is the
  course settings (`course.json`): it can't be moved or deleted, and its
  JSON opens in the row. While its JSON is valid,
  typing in it reshapes the table: the course title, units added, renamed or
  put in a new order (each taking its lessons along); a unit taken out that
  still holds lessons stays, marked, until they move. Units are header rows
  (even just one; a fold arrow and the title, no icon) with their lessons
  indented under them; a unit row folds, and its **+** adds a lesson to that
  unit. A lesson row's ✦ edits that one lesson with AI (#1242, above). A lesson row's ⧉ duplicates it (#454): the lesson as it stands in
  the table (its JSON as typed, if open) as "&lt;title&gt; (copy)", right below,
  saved with the table like a new lesson. The copy's `id` and `slug` end in
  `-copy` (`-copy-2` and so on when taken), and so do its question and
  conversation ids; the guide's question references
  follow. Its `conjugations` are left out, since an item and set belong to
  one lesson; `prerequisites` stay. **+ Add unit**, on the Course settings row right of its gear,
  adds a unit at the end, named on the
  spot ("New unit"; Escape drops it): it goes into the settings' `units`
  (id: the next number when every unit id is a number, else a slug of the
  name) and saves like a settings edit. **Paste a word list**, beside the
  table's heading (language courses only; it follows the Language features
  switch), takes lines like `word — meaning`, `word, meaning`, `word = meaning`
  or tab-separated columns from a spreadsheet, shows the pairs it read under
  the course's `terms` / `meanings` labels and lists every line it could not
  read (no separator, a repeated word, over 500 words) with its number. *Add
  the lesson* puts a new row at the end of the last unit with the words as its
  `vocabulary` and a table of them as its `content`, saved like any new row
  (`src/word_list.rs`, `POST /api/courses/{id}/words`, which stores nothing).
  No model is involved. An empty unit is a header with no
  rows, which learners never see. Every unit but the last has a ✕: an empty
  one goes at once; one holding lessons asks first ("Warning: this will
  delete all lessons in the unit. Are you sure?", Cancel / Confirm) and takes
  its lessons with it, in one save that Undo takes back. A unit holding every
  lesson of the course cannot go. Drag a row by its grip
  (mouse or finger; or focus the grip and use ↑ / ↓) to reorder it; dropped
  on a unit row it goes to the end of that unit. Click a name to rename it;
  ▸ opens the lesson's JSON in the row to edit there, or the same file as a
  form (**Forms mode**, when the **JSON** switch in Editor settings is off; see below); a unit's **+** adds a
  blank lesson (id, slug, title, unit, group and one card of `content`)
  named on the spot; its drawer works at once, in either mode, before any
  save (the save fills in the unit and group, and an id or slug left empty). Every save sends the whole
  table, settings included (`POST /api/courses/{id}/lessons`): renames and JSON edits land
  in their files, deleted lessons go (with their ids in other lessons'
  `prerequisites`), `order` and `unit` are rewritten along the list, and the
  course must load or nothing changes. A learner who has started the course is
  asked before a delete (it is saved at once). The last lesson can't be deleted. A
  course that does not load (or is being generated) shows its files
  instead, to fix by hand. No model is called. *Only add* in **Use AI**
  (under the table) takes a short brief: one request (`add-lesson`)
  writes the lesson and says which lesson it comes after, choosing the unit
  itself; it is validated with the whole course (up to three tries) and
  joins it only if it all loads, as a job like a revision (the course keeps
  working; the course page's modal follows it).
- **Editor settings.** The gear on the course settings row opens a dialog
  of three switches. **JSON** (below) and **Line numbers** are remembered
  in the browser; Line numbers (off at first, shown only with JSON on)
  numbers the lines of every JSON box in a gutter that scrolls with it, as
  vim's `:set number` does. **Language features** (shown only with JSON
  off) is a fact about the course, not the browser: it writes
  `language_features` into `course.json` and saves it with the table. On,
  the lesson editor shows every section and field; off, it hides what only
  a language course fills, so a course that teaches terms in the learner's
  own language is not asked for translations: the Listening and
  Conversations sections and **+ Build** while they are empty (a section
  with items stays; a music course, `music` in `course.json`, keeps
  **+ Build**), a word's and a bite's example meaning, a conversation
  line's meaning (kept when it has text), and the check that an example
  wants a meaning. The same field turns Listening, the Sprint listening
  exercises and sentence builds, tap to hear and Numbers by ear on or off
  for learners (`Manifest::is_language`). Unset, it follows the `lang`
  codes: on when `terms.lang` differs from `meanings.lang`. An (i) by the
  switch opens the explanation.
- **Forms mode.** The **JSON** switch (in the gear's dialog) sets every
  drawer, open or opened later, to JSON (on) or a form (off); the choice is
  remembered in the browser. A lesson's form is the **lesson editor**
  (#841, `static/lesson-editor.js`), three columns in the drawer:
  - an **outline** of the file on the left (Lesson, Text, Vocabulary, Quiz,
    Listening, Sprint, Conversations, Conjugations when the course has a
    paradigm, Guide; Listening and Conversations only with the
    language features on or items in them), each with its count; a section opens
    to its items, each with a kind badge (`A·B·C`, `___`, `T/F`, `123`,
    `ƒ(x)`; `BITE`, `SORT`, `SPOT`, `BUILD`, `ICON`) and a ⚠ on an item the
    course rules would refuse; **+ Question**, **+ Word**, … at the bottom of
    each section, and *Edit as JSON* at the end (the JSON switch);
  - a **form for the one item picked**, written by hand per kind and grouped
    *Ask / Answer / After answering* (or the kind's own groups: *Scene /
    Lines*, *Verb / Forms / Art*, *Buckets and items*…), showing only that
    kind's fields. The right answer is picked, never retyped: a radio beside
    each option of a multiple-choice question (a dashed *Another option…*
    box grows the list from the first letter typed; retyping the right
    option keeps it the answer), **True / False** buttons, the wrong word of
    a spot by a click on the sentence's words, a sort's items in bucket
    columns (dragged, or ◂ ▸). The type switch in a question's heading
    reshapes the form (options with text in them stay in the file for a
    switch back; tolerance and unit go when a question stops being
    numeric). **Checks** appear as you type as a mark in the field's
    corner that opens them in a modal and takes no room, so nothing below
    moves (#1320): a green ✓ for a rule met (*3 options, shuffled for each
    quiz*, *"está" appears once*), an amber ! for advice (no explanation
    yet), a red ! for what the course rules
    would refuse on save (the same rules as `ContentStore::from_files`, in
    `itemChecks`), which the outline flags too; a save the server refuses
    opens the first item the rules refuse (or stays on the one open, when
    the rules refuse it). A numeric or expression answer is read by the
    server's own checker as it is typed (`POST
    /api/courses/{id}/answer-key`). The forms keep the rules where they can
    (#1200): a list's ✕ stops at the rules' least (2 options, 2 lines, 2
    buckets, 3 sort items) and *+ Item* at 10, a word the sentence has twice
    cannot be a spot's wrong word, a new conjugation starts in the
    paradigm's first set, an optional box of only spaces is left out, and
    what names an item follows it: a word renamed or deleted in the
    questions that teach it, a deleted
    question's steps in the hand-ordered Guide, the deleted option that was
    the answer (none is picked then). What one lesson's form cannot see, a
    conjugation or id another lesson also uses, is left to the save's
    refusal, which names both lessons. A field's mark lists its checks, worst
    first. **Advice badge** (#1235,
    `static/advice.js`): a lesson row in the table whose items have amber
    advice gets a yellow ! after its title (its tip says how many things
    there are to check), read from every lesson's JSON, not only the open
    one, and drawn again as the JSON is edited or saved. Any badge opens one
    *Things to check* modal that lists the advice of the whole course,
    grouped by lesson and scrolled to the lesson clicked; a row opens that
    lesson's drawer in Forms mode with the item picked and its field
    focused. Red checks are not listed. The lessons bar has no badge, and
    nothing shows outside the editor. Ids are never shown (the
    lesson's id, slug, unit and order, a question's or conversation's id:
    edit them as JSON; a new item's id is made up, `<lesson>-q1` for a
    question, `<lesson>-conv-1` for a conversation). ↑ ↓ in the footer move
    the item, *Duplicate* copies it (with a new id), *Delete* asks first.
    The Guide section lists the hand-ordered steps (or offers *Order the
    steps by hand* from the derived ones, and *Back to derived steps*); a
    section or question step is picked from this lesson's own headings and
    questions. The Lesson section has the title, summary, focus, group and
    *Read first* (other lessons of the course, by title); Text is the
    Markdown with the cards it makes listed under it;
  - a **live preview** on the right (`static/lesson-preview.js`): the item on
    a phone-sized screen as the learner sees it, in **Study**, **Sprint** or
    **Guide** (the toggle offers the modes that show the kind; another
    section opens in the mode that uses its kind most), drawn again as the
    item is typed and playable: a quiz question's *Check* grades the answer
    with the course's own checker (`POST /api/courses/{id}/preview-answer`,
    the question as it stands, nothing recorded), a Sprint sort, spot or
    build by the session's rules in the browser, a listening sentence read
    aloud by the browser and checked word by word. Fill-in tiles are the
    answer among words of the lesson; a conversation shows its first line of
    3–9 words as a Sprint build; a conjugation table shows in Study and as
    the Sprint ladder.
  Below about 720 px of drawer width the columns become **Outline / Edit /
  Preview** tabs (picking an item in the outline goes to its form). The
  course editor's page is wider than the rest of the app (1240 px) to make
  room. The course settings
  use the **schema form** (`static/schema-form.js`), built in the browser
  from `docs/course.schema.json` (fetched from `/docs/course.schema.json`),
  so a field added to the schema appears without code: text boxes (Markdown
  fields as text areas), numbers, checkboxes for booleans, selects for
  enums; lists with **+ Add**, ↑ / ↓ and ✕; nested objects, maps by key
  as folding sections, labels from the field names and hints
  from the schema's descriptions; ids are not shown (a unit's id is made up,
  `unit-2`). Both modes edit one document: each change in the form is
  written back into the JSON box, so switching keeps the changes and both
  save the same way. Fields the editor does not know are kept untouched.
  Opening the form changes nothing; the first edit rewrites the file with
  two-space indentation. The drawer opens in the mode used last (kept in
  the browser). A new lesson is edited as a form once saved.
- **Saving as you edit** (#151, #190). The editor has no Save button: it
  uses the profile's `static/autosave.js` (see [Profile](#profile)). A move,
  rename, delete or a new lesson (once named) is saved a second after it is
  made; typing in a lesson's or the settings' JSON, or in their form, three
  seconds after the last keystroke, or as soon as focus leaves that drawer,
  or the tab is hidden or closed. The circle by the course title sums it
  up: green ✓ saved, yellow while waiting or saving, red ! when something is
  not saved, with the reason in red under the title. JSON that does not
  parse is not sent (the rest of the table still is) and stays in its box
  to be fixed; a table the course rules refuse (a unit that is gone, an id
  taken, a course that would not load) is refused whole by the server
  (`422`) and nothing changes. A failed save is tried again every few
  seconds; a page that is behind (the course changed in another tab, or an
  AI job holds it) is refused (`409`) and asks you to reload, keeping its
  edits on screen until you do. Leaving with anything unsaved asks first.
  **Use AI** and **Generate** save what is waiting before they start, and
  while a job runs the locked editor saves nothing. One page's saves make
  one **Undo** step (a new one after ten minutes), so Undo takes back a
  stretch of editing, not a keystroke; the Undo and Redo icons and a draft's
  Generate dialog are kept up to date without a reload. Without JavaScript
  the table's *Edit* links open the file editor, whose form still has its
  **Save and validate** button.
- **Error messages** (#228). Whatever refuses an edit (the page's own JSON
  check as you type, the lesson table's rules in `src/arrange.rs`, or the
  course rules of `ContentStore::from_files`), it comes out as one problem
  (`src/problems.rs`): its kind in plain words (*Not valid JSON*, *Against
  the course rules*, *Not saved*), where it is (the lesson by its title or
  the course settings, the field, the line, and the column for a parse
  error), what is wrong and how to fix it (for a reference, what it must
  name, the valid values and a "did you mean" for a near miss; for a
  duplicate id, both lessons and a free id). The server sends it as
  `422 {"errors": {"table": text}, "problems": [{kind, file, row,
  lesson_id, lesson_title, field, line, column, message, hint, text}]}`.
  The drawer's status and the line under the title say the same sentence;
  the lesson's drawer opens (its folder unfolds), the row is tinted red
  (the colour of a removed line in *Review AI changes*) and so is the
  problem's line in the JSON box, with its number marked when line numbers
  are on; in Forms mode the lesson editor opens the section the problem
  names and outlines the field, and *Go to line N* (or a
  click on the line under the title) selects that line in the JSON.
  While such a problem stands, **Undo** (enabled even with no saved step)
  takes back the changes that cannot be saved (#288): what is valid and
  waiting is saved first, then the table and its drawers go back to the
  course as saved. After that, Undo undoes saved steps again. The file page
  and a course that does not load (its card, the editor's banner) word it
  the same way.
- **Feature usage report** (#328). A round bar-chart button beside the gear
  on the Course settings row opens a modal, *Feature usage*, that shows how
  much of the [course format](course-format) the course uses (handy for
  seeing what the AI made of it). Units are folding sections (the first one
  open) whose header counts its lessons and the features and question types
  at least one of them uses ("2 lessons · 7 of 22 features · 3 of 6 question
  types"); each lesson folds open to two lists, *Question types* (the quiz
  types of its `quiz`: multiple choice, fill in the blank, true or false,
  numeric, expression, algebra tiles, set the clock) and *Lesson features* (lesson text, `##` cards, tables
  in the text, summary, focus, prerequisites, vocabulary, example sentences,
  question hints, answer explanations, `teaches`, paradigm tables,
  conversations, listening, a hand-ordered Guide, the Sprint icon,
  bites, sorts, spots, builds), each with a green check when
  the lesson uses it and a grey dash when not. It is computed on the server
  from the loaded course (`src/report.rs`, `feature_usage`), drawn into the
  editor page with the lesson table (`templates/course_report.html`, no extra
  request) and drawn again after each save. Folding works without
  JavaScript; without it the button is a link that shows the report in place.
- **Things to check** (#445, #1199). The top of the *Feature usage* modal lists
  course lints: quick checks with no AI, made on the server each time the
  editor page is drawn (`src/lint.rs`). A yellow count on the report button says
  how many there are; the (i) beside the heading says what they are. None of
  them blocks a save; the validator still refuses broken files. Each row names
  the lesson and what to look at:
  - a prerequisite that comes after the lesson (or is the lesson itself);
  - a quiz of 1 to 3 questions, where one wrong answer puts a run under the
    70 % pass mark ([progress](progress));
  - a multiple choice question whose answer stands in its prompt, unless the
    prompt names another option too ("ser or estar?");
  - a fill in the blank whose answer (three letters or more) stands as a
    whole word in its sentence; an instruction before a colon ("Complete
    with du, de la or des:") is not the sentence;
  - a fill in the blank answer of several words with no `accepted_answers`
    (a one-word answer is the common case and is left alone);
  - a lesson whose Sprint session would be bites only (no quiz, vocabulary,
    listening, conversations, paradigm tables, sorts, spots or builds);
  - a language course lesson whose text reads like another language than
    the course explains in (`meanings.lang`), guessed from function words
    among English, Spanish, French, German, Portuguese, Italian and Chinese
    (`src/lint/language.rs`); a text in a language it doesn't know gets no
    finding;
  - a vocabulary term taught in a third lesson (or later), naming the lessons
    that taught it first.
  The same findings show in the lesson editor as an amber ! in the corner
  of the field they name (the prerequisites, a question's prompt or accepted
  answers, a word, the text), or of the form's title for the rest; it opens
  the messages in a modal. It is drawn again after every save like the
  report, and takes no room, so nothing on the page moves. A generation job runs
  the same checks when its course is complete and logs one line per finding
  (its log says how many); the repeated-term lines are the dedupe report of
  #446. An AI review of each lesson is planned on top of these (#445).
- **Undo and Redo.** Each of these changes to a course as a whole (feedback, one
  visit's edits to the lesson table, a lesson added with AI) keeps the files from before
  it as one step, ten per course at most. Two icons under the table (circular
  arrows, left and right; hovering says *Undo* / *Redo*, and each is greyed out
  when it has nothing to do) take back the newest step, or put back the change the
  last Undo took away. Neither asks first, since the other one reverses it; the
  notice after says what it was ("Removed “Simplifying”"). Both act in place:
  the table takes its new rows, the title, icons, notice and Use AI are drawn
  again, open drawers and folded units stay as they were, and the page does
  not reload (without JavaScript the forms post and the page reloads). The
  table changes over a fifth of a second: rows that stay keep still (one
  that changes place slides up or down to it), rows the step adds fade in,
  rows it takes away fade out where they were, and the table's edge follows;
  nothing moves sideways and nothing dims, and with reduced motion the rows
  swap outright. The change itself is
  the button's accessible name. Any new change (a save, an AI job) clears what
  could be redone, as in a text editor. Edits made by hand since a change are
  undone with it (and Redo brings them back).
- **Review AI changes.** Opens by itself when an Only add or Allow overall
  changes job finishes, unless the learner ticked *Don't show this
  automatically next time* in it (saved on the account at once,
  `users.review_quiet`; unticking it there turns it back on). Once the AI has
  generated, revised or added to a course, the *Review changes* button next
  to **Use AI**'s *Submit* opens it again (`/courses/{id}?review=1`).
  - It reads like the lesson table: one sentence on what the run did ("The
    AI added 1 lesson and changed 2."), then *Course settings* and the
    lessons it touched in course order, numbered as the course page numbers
    them (removed ones last), by title, never by file name. Each has a
    light green, amber or red *New*, *Changed* or *Removed* tag and what the
    AI did in words ("Rewrote the reading; added 2 quiz questions.").
  - When the AI said why it changed a lesson (a fact check or a rewrite,
    #1238), the lesson's why lines and *Check:* lines sit above its diff.
    Lessons a fact check read and left alone are not changes; those with
    *Check:* lines are listed under the lessons as *Looked at, left alone*.
  - A lesson opens with its ▸ (*Open all* / *Close all*; a run of one
    lesson starts open) to its changes: what was there on the left (red
    where it went), what the AI wrote on the right (green), the part of a
    replaced line that differs marked, long unchanged stretches folded. A
    lesson the AI added (or removed) as a whole is one column. Lesson JSON
    reads as labelled text ("Question  What is ½ of 4?", nested items
    indented, brackets as thin rows); *Show the files as JSON* shows the
    lines as they are in the file. Labels are the course format's keys in
    plain words (#282, `review.rs` `LABELS`): `prompt` is *Question*,
    `content` *Reading*, `correct_answer` *Answer*, `options` *Choices*,
    `slug` *Web address*, `question_id` and `id` *Id*, and so on; any other
    key in words (`example_meaning` → *Example meaning*). Lines that only
    identify or file a lesson (Id, Web address, Group, Comes after, Place in
    its unit) are dimmed unless they changed.
  - **Leaving things out.** A red − appears beside a changed line on hover
    (always, faintly, on touch screens): clicking it leaves that change out,
    the AI's line struck through and the old one plain again; dragging from
    it up or down leaves out every changed line it passes over as one group
    (unchanged lines are skipped); while dragging only the band shows, then
    the group's one − on its first row. Every − is one size, the thin
    bracket rows' too. A line that opens a list or an item
    ("Quiz", a question) takes the lines up to its closing bracket with it.
    A yellow −, always shown, marks what is left out and brings back its
    whole group. The − beside a lesson's name leaves the whole lesson out
    (for a removed one: keeps it) and folds it; brought back, it opens again
    with the lines left out inside it still left out.
  - **Editing in place** (#287), like in a text editor. Any line on the
    right of a lesson's JSON (the AI's, or an unchanged one shown around a
    change) can be rewritten as text: click it (or Enter, when it has the
    focus), and the raw line opens in a box (`"title": "Halves",`), key,
    value, brackets and all. Enter is a new line, so one line can become
    several (a new key, another list item); ⌘/Ctrl+Enter or leaving the box
    keeps it, Escape drops it, and an emptied box changes nothing (the −
    leaves a line out). The lesson must still read as JSON once the edits
    are in: commas before a bracket or between items are mended, anything
    else is refused at Accept by lesson name ("Your edits to “…” leave it
    unreadable"), the review staying open with the edit to fix. Removed
    lessons are not editable.
  - **An edited line is always in.** It turns blue (band, text and a bar on
    its left), is read like any other line (its key as a label), loses its
    −, and a drag skips it. Editing a line that was left out takes it back
    in. While a lesson has an edited line its − is disabled, its title
    saying why, and so is the − of a line that opens a block (a list, a
    question) holding one. *Show the files as JSON* shows the line as Accept
    will write it, and the footer counts "1 line edited". The server keeps
    the same rule: an edited line, and its lesson, are written even if the
    form also says to leave them out.
  - **Undo / Redo of edits.** The first edit brings an Undo / Redo pair into
    the modal's footer (the editor's icons). Each kept edit is one step; Undo
    puts the text before it back (back to the AI's text, the line is green
    again and has its − back), Redo applies it again. Ctrl/⌘+Z and
    Ctrl/⌘+Shift+Z or Ctrl+Y do the same while no field has the focus. The
    history lives in the page until Accept; after Accept the course's own
    Undo takes the whole review back in one step. A reopened review shows
    the accepted edits blue and still editable, with an empty history:
    typing the line back as the AI wrote it makes it green again.
  - **Accept** (*Accept the rest* while something is left out) writes the
    course as the AI left it minus what is left out, with your edits, as one
    Undo step ("Left out part of the AI's changes", "Edited the AI's
    changes", or both); with nothing left out or edited it changes nothing.
    What was left out and edited is kept with the run, so the review shows it
    again and can bring it back. A lesson changed by hand since the run (other than by
    an earlier Accept) is refused by name; changes the lesson table makes
    (its `order`, written into every lesson on any table save, its unit, a
    rename, the prerequisites of a deleted lesson) don't count, #280: they
    are kept and the review's choice goes in around them, unless it changes
    the same field. Also refused: a lesson that would no
    longer read as JSON or a course that would no longer load; the review
    stays open with the boxes as they were. Close leaves everything as it
    is (asking first when something was left out or edited but not accepted).
  - It shows what the AI wrote at the time, not later hand edits, and only
    the latest run (the next one replaces it). Without JavaScript it is a
    page: every − is a checkbox of the Accept form, each line its own group.
  - **After Undo** of the run (#281: every file it changed is as it was
    before, and the review had not left it all out), the review still shows
    what the AI did, read-only: an *Undone* tag by the title with an (i)
    ("Redo on the course page puts them back"), no −, no editable lines and
    no Accept (an Accept sent anyway is refused). After Redo it works again.

**Exercises beyond languages.** Quiz questions come in six types
(`multiple-choice`, `true-false`, `fill-in-blank`, `numeric`, `expression`,
`algebra-tiles`;
`measure`;
see [the course format](course-format#quiz)), so a maths course can ask for a
number or a formula and grade it by value: `3/4` for `0.75`, `x = 2 or x = -3`
for the roots `2; -3`, `2(x+1)` for `2x + 2`. Numbers can have a tolerance and
a unit. Every mode serves them: Practice, the lesson's reading check and the
Guide show an answer box with the unit and without the accent bar; Sprint mode
offers a number as four tiles (the answer and the usual slips: off by one,
the wrong sign, a power of ten, doubled or halved) with *Type it instead*, and
asks for a formula as a typed answer; the tutor counts a typed number or
formula as an attempt and is told the app checks by value. The checker is
`src/mathcheck.rs`, a small parser and evaluator with no dependencies:
equivalence is decided by evaluating both expressions at fixed sample points.

**Algebra tiles** (#1355, `src/algebra_tiles.rs`, `static/algebra-tiles.js`).
An `algebra-tiles` question shows a board above the answer box in Practice,
the lesson's reading check and the Guide: a pile of tiles (big squares for
x², strips for x, small squares for 1) read from the question's `tiles`.
The learner drags them into a rectangle (they snap to a grid when dropped;
*Turn* turns a strip, *Start again* puts them back), or uses the keyboard:
Tab to a tile, the arrow keys move it one step, R turns a strip, Enter
checks. Then they type the two sides. The server grades the typed answer
only: it must be two sides multiplied, each with the variable, and multiply
out to the tiles (an expanded answer gets the note "Write it as its two
sides multiplied"). The page never holds the right layout or answer. The
board's script and styles load only on pages with such a question. Sprint
mode and the tutor skip these questions, and a lesson without them never
shows a board.

**Measure the angle** (`measure`, #1352, `src/measure.rs`). Practice, the
reading check and the Guide draw the question's `angle` and a see-through
protractor with two scales (`static/measure.js`, drawn with JSXGraph from
`static/vendor/`, loaded only on a page with such a question). The learner
drags it, turns it by its handle or with two fingers (arrow keys move it,
Shift with ← or → turns it), reads the angle and types the number; the server
grades it like `numeric`, and 180 minus the angle gets "You read the other
scale". The drawing turns the angle a random way each time. Sprint mode and
the AI tutor ask in text, so they leave these questions out
(`QuizType::needs_widget`), and a run doesn't need them to count
(`progress::required_answers`).

### Sharing a course

**Share** on the course page (`src/share.rs`, `src/routes/share.rs`) opens a
dialog: type e-mail addresses (one per line or separated by commas, up to
20 at a time and 50 per course) and *Send invitations*. Every new address
gets a message, "*Owner* shared a course with you on Skrongle: *title*",
with a link (`/courses/shared/<token>`); the dialog lists every address with
*invited* or *opened* and a *Remove* button. The owner's own address is
refused, and an address already on the list gets no second message.
On a server without mail (#633, no `SKRONGLE_MAIL_PROVIDERS`) the dialog says
so, its button reads *Make invitation links*, and nothing is mailed: the
page that comes back shows each new address's link (`course-share-link`)
with *Copy* (`course-share-link-copy`), once, since only the token's hash is
stored. The share rows are made the same way; the owner passes the links on.

Who may open a shared course: the account that opened its link (the first
account to open a link keeps it, so a forwarded link lets nobody else in;
the owner opening their own link, from an inbox say, takes nothing and lands
on the Share dialog), and any account whose **confirmed** e-mail address is the invited one, link
or no link, so a learner who already has an account with that address finds
the course on their list as soon as it is shared. An unconfirmed address does
not count: anyone can register with someone else's address.

For the recipient the course is **read only**: it is on their course list
(under *Shared with you* until opened, then under *Your projects*) with a
small filled circle after the title, which opens a note saying whose course
it is; they open it and learn from it with their own progress, passes and
history, like any course, but there is no *Edit files*: the editor, the
files, the export and the AI forms stay the owner's (a 404 for anyone else).
The owner's later changes reach them at once (it is the same course, not a
copy).

Opening the link **signed out** leads to the login page, which says *It looks
like you don't have an account with us! Please login or signup to continue
viewing your course: title* above the form (the register page says the same,
and the two link to each other with the token). Logging in or registering
there goes on to the link, which puts the course on the list and opens it,
also through the two-factor code step: a short-lived `share_next`
cookie carries the token there (#1016). *Cancel* on the code step goes back
to the login page for the same link.

**Removing** an address takes the course off that learner's list and out of
their session (their progress rows stay for the day it is shared again).
Deleting the course deletes its shares. A link that no longer matches a
share (the address was removed, or invited again with a new link) leads to
the course list with *This link no longer works. Ask the course's owner to
share the course with you again.* (`?notice=share-gone`, #1082). Signed out,
the login page says the same and signing in goes on to that list.

### Copying a course

The copy icon at the top right of a course's settings dialog, beside the
X (`course_copy.html`; `POST /courses/{id}/copy`, #629), makes a new
private course of the learner's own with every file of the original
(`course.json`, the lessons, the pictures), called *Copy of &lt;title&gt;*
(cut to 80 characters), and opens it in the editor. Its hover text says
*Create a copy of this course.* The dialog is the one behind the card's gear
of a course the learner has opened; the Start wizard of a course not opened
yet has no copy, translate or pencil icon, and the card itself has no Copy
button. On a course of
the learner's own a pencil icon sits left of the copy icon (*Edit this
course's files.*) and opens the course editor; the card has no *Edit files*
button either. Built-in
courses and the learner's own can be copied; a course shared with them
cannot (it is the owner's, so its dialog has no icon), and one still
being generated or stopped half way cannot either (*A course can be
copied once it has been written.*). Only the
manifest changes: its `id` becomes the copy's and its `title` the new
name; its `subject` and everything else stay. The original is not touched,
and nothing of the learner's progress goes with the copy (carrying it over
is #347). Keeping a copy in step with its original is up to the learner.

A copy is a course like any other: it counts against the course limits
(`Limits` in `src/courses.rs`: courses per account, a course's bytes, an
account's bytes), checked in the same `Db::call` that stores it. Over a
limit nothing is made and the course list says which limit (`?notice=`
`course-count-<n>`, `copy-too-big-<bytes>` or `copy-no-room-<bytes>`).

### Copying and translating a course

**Copy and translate to…**, an icon left of the copy icon in the same
settings dialogs (not with AI off), opens a dialog of its own: the language to translate into, the AI
to use (when there is more than one: the saved keys, as
Generate offers them) and **Start**; what gets translated is behind its
(i). The languages are the app's own (`i18n/*.yaml`, each in its own
name), less the one the course is already explained in (`meanings.lang`).
Start (`POST /courses/{id}/translate`, #630) makes a copy as Copy does,
called *&lt;title&gt; (&lt;language&gt;)*, e.g. *Spanish (Français)*, and
hands it to an AI job that translates it in place, one request per file
(the manifest first, then each lesson): the editor follows it in the same
progress modal as a generation, the course list greys its card out with
Cancel, and the copy cannot be opened until it is done. The original is
never changed.

What is translated is the learner's side: explanations, meanings,
instructions, quiz questions and answers, titles, the manifest's labels and
units. A language course keeps what it teaches as it is (its `terms`,
examples, conjugations, listening sentences, conversation lines, sort and
spot sentences stay, and the model is told to leave that language alone
inside mixed texts too); a course that teaches no language is translated
entirely. The model never rewrites a file: the app collects its texts
(`src/translate.rs`), each distinct one once, sends them numbered and puts
each translation back wherever that text was, so ids, references and the
structure stay, and a `correct_answer` that is one of the `options` stays
one of them. A guide step's `section` follows its translated heading. The
manifest's `meanings.lang` becomes the new language. Each file is then
validated with the rest of the course before it is saved, like any lesson
(a reply that cannot be used or does not load is asked for again, twice).

The job is a generation in every other way (`src/ai.rs`): its own OS
thread, one job per learner, one AI job at a time on the whole server (a second Generate is refused, no waiting line, #43), the hourly and daily AI
job quotas, on the learner's own key. Its calls carry the purpose `translate-course` (the file's path
as the key). The copy's `courses.source` is `translate` and its notes hold
the job's plan (the language, and the files done so far), so a stopped
translation keeps what it translated: its card has the red ! with the
reason and **Resume translation**, which opens the same dialog without the
language and translates the files not done yet. Keeping the copy in step
with its original is up to the learner.

## A course's settings

**A course's settings** (#565, #574, #587): the **level** to start at (a
course with levels, see [Starting level](#starting-level)) and the
**motivator** worn in the course
(see [Motivators](#motivators)), stored in
`projects.motivator` (`templates/course_settings.html`,
`static/course-settings.js`, `POST /courses/{id}/settings`, where a field
left out keeps its value). There is no character art: the account badge in
the top bar is the gear, and conversation lines show only the speaker's name.

- **First start: a wizard.** Clicking a course's **Start learning** (or
  *Open*) button on the course list for the first time opens the settings
  as a guided dialog over the list, one step a page, with an arrow on
  either side to slide between them and dots for where you are:
  *Choose a motivator* (with an (i) saying what it
  is and that it can be changed later), then *Where do you want to start?* (a course with levels
  only, last since #1106, with its placement test when it has one; one slim
  row a level, the lesson count on the right).
  **Get Started!** sits under every step and opens the course with those
  settings (`POST /courses/open/{id}` with `wizard=1`); a step not reached
  keeps its default (#896).
  Closing the wizard opens nothing: the course stays on the list, and Start
  shows the wizard again.
- **Opened another way** (a share link, the editor's open icon, no script),
  the same wizard opens over the course's home page on its first visit.
  There **Get Started!** saves and draws the home page again, and closing
  it keeps the settings the course started with. Either way the course is
  set up (`projects.set_up`, v51) and the wizard never opens again; courses
  opened before #574 count as set up.
- **After that: one sheet** (#576, `templates/course_settings.html`, a
  separate dialog from the wizard). The gear on an opened course's card in
  the course list opens every setting in view at once: a header with the title, then a row a setting with its label on the
  left: the level as a joined ladder of its titles with a note on the one
  chosen (its description, "From the beginning" for the first, its lesson
  count), the motivator as chips, then
  **Save**. An (i) by the title says every setting can be swapped any
  time without resetting anything. Closing it without Save keeps what was
  saved.

Every course starts with the account's motivator (`users.motivator`, which
nothing edits any more).


## Progress across modes

A lesson is **passed** once it has been *seen* (read to the end in Study mode,
or taught by a Sprint mode session, a Guide stage or a tutor) and one sitting of
its exercises scored **70 %** or more over at least 3 answers (all of them
for a lesson with fewer quiz questions). Every mode reports to the same
record, `src/progress.rs`, so passing anywhere counts everywhere:

- **Study mode**: passing the lesson page's reading check marks it seen (a
  lesson without quiz questions has an *I've read it* button instead). A practice round reports a run for
  each lesson it asked about, and the summary shows which lessons passed. A
  listening round on one lesson is a run too. The lessons list, the
  listening list, the lesson page and the map show a ✓ for passed lessons.
  The lists' mastery pill (its tooltip says *Mastery*) appears only once the
  lesson has answers, so a lesson reads "✓ Passed · ✓ Read" or just
  "Unread", never "Passed · Not started". The home page counts them.
- **Sprint mode**: a lesson session is a seen and a run together. A stop on the
  path is complete once its lesson is passed in any mode, and the next stop
  unlocks then. A session under 70 % leaves the stop open.
- **Play mode**: a Guide stage works like a Sprint mode session and shares the same
  complete/locked state. Word Match on one lesson's terms is a run.
- **AI tutoring**: once a session has set enough of a lesson's exercises,
  their score is a run (skips count as misses). The tutor's prompt carries
  the rule, the chat shows "✓ Lesson passed", and the tutor is told so it
  can congratulate the learner.
- **AI agents** outside the app report seen lessons, graded answers and
  passes through `/api/progress`, under the same rule. The rule, the
  agents' brief and the API are on [their own page](progress).

Mastery (below) is separate: it decays and keeps steering practice, while a
pass is a milestone that stays.

## Motivators

A motivator decides how a learner's progress is dressed up: what keeps them
coming back. A motivator owns two places: the
top of the course home page (its **hero**) and one short **reward line** at
the end of a lesson or session in every mode. The learner picks one per
course in [the course's settings](#a-courses-settings) (the wizard of its first visit,
then the gear on its card; the (i) says what each one does), stored in
`projects.motivator` ('' for the default; #574). A new course starts with
`users.motivator`, the account-wide choice from before #574, which nothing
edits any more. Motivators are app-level code, not course data: a course
file cannot switch one on.

| Motivator | id | What the home page leads with | Reward line |
|---|---|---|---|
| Statistics and metrics (default) | `stats` | The daily goal's ring, the streak, lessons passed of the course, terms collected, *Continue: <lesson>* and the current unit's bar | "✓ Passed · 14 / 20 today · 🔥 6" (unit finished or daily goal met in place of the first parts when they happen) |
| XP (#561) | `xp` | A character level badge ("Lv 7"), the bar to the next level ("340 / 700 XP"), *Continue: <lesson>* and today's daily goal counted in XP ("today: 120 / 200 XP": effort points ×10, the same goal as everyone's); its (i) has the effort table in XP and the bonuses | "+45 XP", plus "Level up! Lv 8" when the run crossed one or more levels |
| Skrongle Cash | `skc` | The course's SKC balance with SKC's own glyph, today's SKC against the daily goal in SKC ("today: 18 / 20 SKC", the effort-point goal at SKC rates), *Continue: <lesson>*; a *Shop* button and the shelf of badges bought (#569); its (i) lists the rates and explains the shop in one line | "+27 SKC · +10 SKC daily goal · +5 SKC lesson passed" (the run's SKC, then each bonus that landed) |

**The reward line** (#568) is the same element on every end screen:
`<p class="motivator-reward" data-testid="motivator-reward"
data-motivator="<id>">`, drawn by the server on the request's task (so it
is translated) and inserted by the page. A run that earned nothing (nothing
logged, passed or reached) shows no line, never "+0". Where each mode gets
it:

| End screen | Endpoint | How the run's start is known |
|---|---|---|
| Sprint mode session | `POST /api/sprint/finish` → `reward` | `duration_ms` |
| Study practice round | `POST /api/practice/finish` → `{ runs, reward }` | `duration_ms` |
| Study reading check | `POST /api/lesson-check` → `reward` | the request |
| Study "I've read it" | `POST /lessons/{slug}/read` redirects to `/lessons/{slug}?read=<time>`; the page shows the line (for 10 minutes) | `read` |
| Study listening round | `POST /api/listening/finish` → `reward` | the request |
| Guide stage | `POST /api/guide/step` → `reward` when `finished` | `duration_ms` (time on the stage page) |
| Play games (Word Match, drill, numbers, dates) | `POST /api/games/*/finish` → `reward` | the request, or `duration_ms` for the drill |
| AI tutor | the answer entry (`reward`) that passed a lesson or used up the exercises | since the last line, or the session's start |

What a run earned is a `motivators::RunOutcome`: the effort points it
logged (`points`), whether it passed a lesson (`passed`), took today's
effort to the daily goal (`goal_met`) or passed a unit's last lesson
(`unit_finished`), the same as an `earnings::Tally` (`earned`), today's
effort (`today`) and the streak. `motivators::reward(conn, user, course,
tz, since, lesson)` measures it from the shared tables (everything in
`[since, now]`) and asks the worn motivator for its line.

**Earnings** (`src/earnings.rs`). A motivator that counts in units of its
own (XP, a currency) prices what was earned; nothing is priced in advance:
`earnings::between(conn, user, course, tz, from, to)` counts, in a window,
the effort points logged (`activity_log`), lessons passed
(`lesson_passes.passed_at`; passes from before #558 count too), days whose
daily goal was reached (the moment a day's points reached the goal fell in
the window) and units finished (every lesson of the unit passed, the last
in the window). It reads the log for that window only (from the start of
its first day), so a run's tally (`RunOutcome::earned`) and
`earnings::today` cost a day of rows at most. `Rates::earned(&tally)`
prices a tally: `XP_RATES` (10 per effort point, +50 per pass, +100 per
daily goal, +250 per unit) and `SKC_RATES` (1 / 5 / 10 / 25) are tunable
constants; changing one re-prices everything already earned. The counting
itself is the pure `earnings::tally(&facts, from, to, goal)`, unit-tested
apart from the database.

The course total (`earnings::total`, what XP and SKC lead with) is never
added up from the whole log (#589): what grows without bound lives in one
`earnings` row per learner and course, the `earnings::Ledger`: the effort
points ever logged, the days whose goal was reached, the local day last
active, that day's points and the day streak ending on it.
`activity::record` moves it with every activity logged (the learner's time
zone, `CurrentUser::tz` from the `tz` cookie, decides which day the effort
lands on), and the streak every mode shows (`sprint::day_streak`) is read
from it. A learner with activity from before the row existed gets it added
up from their history once, on first use, and kept from then on. Passes
and units finished are counted from `lesson_passes`, one row per lesson of
the course at most. The history's retention (`SKRONGLE_RETAIN_HISTORY_DAYS`)
therefore never takes XP or SKC away.

**XP** (`src/motivators/xp.rs`) is per course and prices nothing in
advance: a learner's XP is `XP_RATES.earned(&earnings::total(..))`, so XP
earned while another motivator was worn is there on switching. Its character level
(not a course's CEFR starting level) follows `level_of(xp)`: level 1 at 0
XP, and going from level n to n + 1 costs `LEVEL_STEP · n` XP (100 · n;
level n starts at 50 · n · (n − 1)). Changing `LEVEL_STEP` or `XP_RATES`
re-levels everyone. A run's "+N XP" is `XP_RATES.earned(&run.earned)`;
"Level up!" names the level the course total is at after the run when it is
above the level of the total less the run.

**Skrongle Cash** (`src/motivators/skc.rs`, #562) is play money, one
balance per course, priced at `earnings::SKC_RATES` from
`earnings::total`: it keeps counting while another motivator is worn.
The balance shown is earned less spent (`motivators::skc::balance`).

The shop (`src/motivators/shop.rs`, `src/routes/shop.rs`, #569) sells
badges. A badge only changes looks. Nothing bought touches passes, the
streak, effort points or lessons. A badge belongs to one course. It is
bought with that course's SKC and shows only on that course's SKC hero,
never on the profile, the course cards or the top bar. `/shop` (course-scoped, through
`Project`) is reached only from the hero's *Shop* button and sends a course
that does not wear SKC home; it lists the catalogue with prices and *Buy*,
and the learner's shelf. `POST /api/shop/buy` (`item`) checks the price
and the balance on the server; a badge already owned or one the balance
cannot pay is refused and nothing is charged. The shop then opens with a
modal that says why (400 when the course does not wear SKC). The catalogue is
`data/badges/catalogue.json` (`id`, `price`; six badges, 50 to 1500 SKC),
each with plain art `data/badges/<id>.svg` and a title
`motivator.skc.badge.<id>` in every `i18n/<code>.yaml`. A badge is bought
once and kept in `motivator_purchases` (user, course, item, the price
paid), which a switch of motivator never clears. No refunds, no consumables.
Its glyph is an arcade ticket with a spark (an original shape, not a
currency sign), drawn in `currentColor` so it follows every
colour scheme: `motivators::skc::symbol()` draws it and
`motivators::skc::amount(n)` an amount with it; use them wherever an SKC
amount shows. "Skrongle Cash" and "SKC" are i18n keys
(`motivator.skc.title`, `.short`), left untranslated in every language.

**What a motivator may not do.** The measures underneath are the same
whichever is worn: lesson passes (`progress.rs`), the daily goal's effort
points and the resume point (`activity.rs`), mastery and vocabulary. A
motivator only reads them. One that counts something of its own (XP,
coins) works it out from those shared tables, so it keeps counting while
another motivator is worn; anything it stores itself (coins spent, items
bought) goes in its own tables, keyed by user and course, and switching
never clears it. Switching therefore loses nothing, and switching back finds
everything earned in the meantime.

**Adding one** (`src/motivators.rs`): a type implementing `Motivator` (id,
icon, `load(conn, user, course, tz)` for anything it keeps of its own or works out with `earnings`, and `reward` for its line (`RunOutcome::tz` for `earnings` windows),
both run inside `Db::call`), `Hero` (renders the home page's top from
`motivators::Progress`, the shared measures) and `Reward` (renders the
line's inner HTML from the `RunOutcome` it was given, escaping anything not
its own markup), both on the request's task so their text is translated;
counting in units of its own goes through `earnings` with a `Rates` of its
own; an entry in `motivators::ALL`; its hero template under
`templates/motivators/` (the section keeps `data-testid="home-today"` and
sets `data-motivator`); `motivator.<id>.title`, `.about` and its line's
texts (`motivator.<id>.reward.*`) in every `i18n/<code>.yaml`; a row in the
table above and an e2e spec that reads its line
(`e2e/specs/smoke/motivator-reward.spec.ts` shows how). The id is stored,
so never rename one; an id the app no longer has falls back to the
default.

## Starting level

A course whose manifest says `"levels": true` (its units are levels such as
CEFR A1, A2…, not chapters that build on each other) lets a learner who
already knows some of it start further in. The last step of the
course's start wizard (#587, #1106, [a course's settings](#a-courses-settings)) asks *Where do you want to start?*: one card per unit
that has lessons, the first one ("from the beginning") chosen. The whole
card is the choice (a visually hidden radio inside it): the chosen card is
outlined in the accent colour, grows slightly and shows a check mark
(no motion under `prefers-reduced-motion`). The choice is
stored per project (`projects.start_unit`, '' for the beginning) and can be
changed at any time (see [Changing level](#changing-level)). A
course without `levels`, or with fewer than two units that have lessons,
never shows the level step. The dev login (`/dev/login?open=…`) opens a
course without asking.

Starting at A2 does not pretend A1 was learnt. Nothing is passed and no
score is given: the learner starts with zero points. The A1 lessons are
**skipped** (`progress::is_skipped`): still open to read, but

- recommended practice, the review mix, the lightning round, mixed
  listening rounds and the AI tutor's picks never draw from them, and "read
  next" points at the first A2 lesson;
- the current unit is counted from A2, so the map and the home page say
  A2 and the map's "you are here" starts there;
- the Sprint mode path and the Guide mark them *skipped* (not locked, never
  the current stop) and open the first A2 stop;
- lists show a "Skipped" status instead of "Not started", and the totals
  ("3 of 18 lessons passed") leave them out, with "21 skipped" beside them;
- `/api/progress` gives each lesson a `skipped` flag and the course's
  `start_unit`, so an agent does not teach them unasked.

A skipped lesson the learner studies anyway (reads it, plays it, answers its
questions) is theirs from then on, like any other lesson. Paradigm drill
items unlock by passes, so the skipped lessons' items stay locked until
those lessons are passed.

### Placement test

A course with levels may have a placement test (#1106, `placement.json`,
see [the course format](course-format#placementjson-placement-test)). Its
level step then also offers **Take the placement test**
(`templates/placement_test.html`, `static/placement.js`): the level cards
make way for one multiple-choice question at a time ("Question 2 of 9",
`GET /courses/{id}/placement`, no answers sent), a level's written
questions or ones drawn from its lessons. The last pick posts the answers
(`POST /courses/{id}/placement`, `placement::mark`), and the step shows
*Your result: start at A2* above the cards with that card chosen, so **Get
Started!** saves it as any chosen level is. The learner may still pick
another. **Show report card** opens a table of each tested level's right
answers ("2 / 3"), green when all were right, yellow for some, red for
none. **Take it again** draws a new sitting. Nothing is stored until the
wizard is saved; the gear on `/courses` changes the level later as usual.

The rule: a level answered all right can be skipped; the first level with
a wrong answer is the suggested start. With every answer right it is the
level after the last one tested.

In the course editor, the gear on the **Course settings** row shows two
boxes alike: first JSON, Line numbers and Language features, then, last, a
**Levels** switch (`course.json` `levels`); with it on, a
**Placement test** switch below it, off until turned on, adds
`placement.json` (a blank test drawing 3 questions per unit) and shows a
**Placement test** row right under Course settings, whose drawer edits the
file as JSON. Turning either switch off takes the file out with the next
save (Undo brings it back). Both rows are there in JSON and Forms mode:
they are about the course, not its language.

### Changing level

The level is not only a starting point (#333). The course's settings dialog
on `/courses` (the gear) shows the levels first, the current one chosen
(`courses-settings-level`); saving another level changes it
(`POST /courses/{id}/settings`, `start_unit`). That is the only place it
changes (#587: no `/start` page, no picker on the lessons page). Only the course has to have levels (above).

Changing level moves the learner's position and nothing else:

- passes, mastery, read checks, history, XP and streaks stay as they are;
  nothing is marked passed and nothing is deleted;
- lessons below the new level that were never touched become skipped (as
  above); ones the learner already studied stay theirs, passed or not, and
  keep appearing in practice;
- every mode continues from the first open lesson **of the new level**:
  saving a level touches its first lesson (`activity::touch`, in the mode
  last used), so it is the lesson every mode resumes at
  (`activity::resume`: Sprint mode's Continue, the Guide's current stop, the
  map's "you are here"), and a begun-but-unpassed lesson below it does not
  lock the path (`guide::stages`). When that lesson is passed, the next open
  one after it is; only when everything from the new level on is complete
  does the position go back to a begun lesson below it;
- moving down again (to the beginning) brings the skipped lessons back as
  "Not started", with whatever progress they had.

## Mastery (the engine under Quizzes and the Map)

Per (user, course, lesson): a `mastery_score` from 0 to 100.

- **Correct:** `score += (100 - score) * (0.20 + 0.04 * min(streak, 5))`.
  Diminishing returns, boosted by the current streak; six right in a row take
  a fresh lesson past 80 ("mastered").
- **Wrong:** `score *= 0.65`, streak resets.
- **Decay:** the *effective* score shown and used for selection is
  `score * max(0.4, 1 - 0.015 * (days_since_practice - 3))`, so a lesson not
  touched for a month drifts back towards "needs practice".
- **Status:** ≥ 80 mastered · 50–79 in progress · < 50 needs practice · untested.
  Under 50 counts as "needs practice" only after a wrong answer or once the
  score has faded; a few answers, all right, leave the lesson in progress.
- **Current unit:** the first unit whose average effective mastery is below
  80 or that still has untested lessons, counting from the learner's
  [starting level](#starting-level).

**The learning cycle** is read → check → practise. A lesson page ends with a
reading check: two of its quiz questions (one if it only has one), chosen at
random with recently-correct ones avoided, answered together (`POST
/api/lesson-check`). Each answer is graded and recorded exactly like a
practice answer, so mastery and history move. Both right marks the lesson as
*read* (`user_progress.read_at`; a miss explains and offers two fresh
questions) and shows a "✓ Read" pill on the lesson and in the lesson list.
Reading is not mastery: two right answers leave the lesson around 36%, in
progress, until practice takes it past 80. Lessons practised before
the check existed count as read. A lesson taught in another mode (a Sprint mode
session, a Guide stage, the AI tutor, an agent) counts as read too: read
and [seen](#progress-across-modes) are one thing.

Under its footer a lesson page links the
previous and the next lesson in course order (unit, then `order`, then title, as the lesson list shows them),
passing over the levels below the learner's
[starting level](#starting-level); the first lesson has no previous link and
the last no next one.

**Quiz selection:** *recommended* practice (`/practice` with no lesson) only
draws from read lessons; when none are read it points to the next unread
lesson of the current unit instead. Practising a single lesson, Sprint mode, the
Guide, Listening and the AI tutor are not gated. Each candidate lesson gets
weight `(100 - effective) + 8`, `+15` if never tested, ×0.25 if one unit ahead
of the current unit (×0.05 beyond), ×0.35 if its prerequisites are below 40.
Questions are drawn by weighted sampling without replacement; anything
answered correctly in the last 24 h is only reused once fresh questions run
out.

Sprint mode's rule exercises (sort, spot, build, ladder) count as quiz answers for
the lesson; its vocabulary exercises (pick, match) collect and review terms.

## Music notation

A lesson can show music on a staff (#1176). Its Markdown holds a fenced block
marked `abc` with the music in ABC notation
([course format](course-format#music-notation)); the lesson page and the
Guide's cards draw each block as an SVG staff in its place, in the text
colour, scaled to the width of the screen. The drawing is done in the browser
by abcjs (`static/vendor/abcjs-basic-min.js`, MIT, about 500 KB), which is
compiled into the binary like every static file, so Skrongle mini draws it
offline. A page loads it only when its lesson has an `abc` block
(`Lesson::has_notation`), and `static/notation.js` swaps each block for its
drawing. If abcjs can't draw a block, warns about it or finds no notes in
it, its ABC text stays as a code block (#1187). Sprint
mode, the editor's preview and the other modes show the ABC text.

A quiz question may carry music too (#1218): its `abc` field is drawn as a
staff above the prompt in Practice, the lesson's reading check, the Guide's
quick checks, Sprint mode and the lesson editor's preview. The first music
question is "Name the note": one note on the staff and the note names as
options. "Pick the interval" (#1220) puts two notes on the staff, one after
the other or stacked, with interval names as options; it needs no code of its
own. "Name the chord" (#1222) stacks a chord (`[DFA]`) with chord names as
options; no code of its own either. "Name the key" (#1221) shows a key
signature, or a short tune that is in a major key or its relative minor, with
key names as options; no code either. In a multiple-choice question with `abc`, an option written as an
ABC note (`A`, `^F`, `_B`) is shown in the course's note names
(`music.note_names` in `course.json`: letters, solfege or german), and the
answer is compared in those names too (`notation::name_notes`, at load). The
server pages load the notation scripts only when a question on them has
`abc`; the editor's preview loads them the first time it shows one. The
editor's question form offers a **Music** box in a course with a `music`
block, or once a question has `abc`.

Music can be heard too (#1219). A question's `play` is ABC text that a
speaker button above the prompt plays, in Practice, the reading check, the
Guide's quick checks, Sprint mode and the editor's preview.
`static/music-sound.js` reads the ABC (key signature, note lengths, `Q:`
tempo, chords, rests, accidentals within a bar) and plays each note as a
Web Audio oscillator tone: no samples and no network, so it works offline
and on Skrongle mini. Two single notes, an interval, play one after the
other and then together. Sound starts only on a tap, and the first tap
wakes the audio context. If the audio clock has not moved a second after a
tap (an iPhone on silent, a blocked context), the button shows "No sound?"
and a line under it says why. The speaker follows the site's audio switch
(the one Sprint mode's speaker button and its listening exercises use), and
"Can't listen now" under it turns sound off for an hour: Sprint mode then
skips the exercises that need sound (a question with `play` and no `abc`, a
sort of speaker tiles), and the other modes draw the tune on a staff to read
instead, except when the options are the staves. A muted speaker turns sound
back on when tapped. With `play` and no `abc`, the played notes are drawn on
a staff once the question is answered, so the ear and the eye meet.

`option_kind: "abc"` on a multiple-choice question draws each option as a
staff ("Which one did you hear?"): its notes after the header lines of
`play` (else `abc`), `QuizItem::option_staves`. With `clef=perc` in the
`K:` line ("Which bar did you hear?", #1225), the tune is a rhythm. Every
note plays as the same short click, and the staves show the percussion clef
and the metre. After a wrong answer the
feedback line shows the right option as a staff too, not its ABC text
(#1308: `window.answerStaff` in `static/notation.js`; Sprint mode draws
the option's tune on its result sheet). In Sprint mode's Sort it, a
sort item with `play` and no `text` is a speaker tile: the card is a speaker,
and a placed tile is a small replay button (#1223 builds "Major or minor?"
on it). The editor's question form offers a **Sound** box and an "Options
are: Text | Staves" switch, and a sort item a **Sound (ABC)** box, in a music
course or once an item has them. Pages load the sound engine only when
something on them has `play` (`Lesson::has_sound`).

Builds appear in language courses and in music courses (#1232). A music
course's Sprint build puts notes or keys in order, like a sentence build.
The circle of fifths is six shuffled key tiles (C G D A E B), tapped in
order of sharps and graded exactly. With the language features off, the lesson
editor still offers **+ Build** in a course with a `music` block.

**Fill the bar** (#1230) is a quiz type of its own, `fill-bar`
([course format](course-format#quiz)): a bar with a gap (an
invisible rest in its `abc`), the beats still to fill, and tiles of note
values, each drawn as a small staff. A tap puts a tile in the gap and the bar
is drawn again with abcjs, the rest of the gap still blank; Undo takes the
last tile out. Once the gap is full the learner checks, and the server grades
the tiles through `/api/answer` by their summed length
(`notation::fill_bar`), or, with `"exact": true`, against the answer. A wrong
length says how many beats went in, and a wrong answer shows the right one
drawn in the bar. It is asked in Practice, the reading check, the Guide and
Sprint mode (`static/fill-bar.js` builds it in each); the AI tutor leaves it
out, since a chat can't tap tiles. The editor offers the type in a course
with a `music` block, with boxes for the tiles and a switch for what counts,
and the feature usage report lists it only in such a course.

A Sprint spot may be a staff too, "Fix the wrong note" (#1227): a spot with
`abc` instead of a `sentence`, one note wrong (`wrong` counts the notes from 1)
and its `fix`. The learner taps the note (abcjs's click listener says which),
then picks the fix from three tiles: the fix and two notes a step or two away,
in the course's note names. Dragging the note up or down on the staff picks
the tile of the note it lands on. Both parts must be right; with the note
found and the fix wrong, the sheet says so. The server counts the notes
(`notation::abc_notes`) and grades it (`src/sprint/spot.rs`); the page code is
`static/wrong-note.js`. It is in Sprint mode only; the editor's spot form
switches to a Music box, a note number and the fix once a spot has `abc`, and
its preview draws the staff and says the answer.

**Spell the chord** (#1228) is a quiz type of its own, `chord-spell`
([course format](course-format#quiz)): the prompt names a chord, and the
learner taps note tiles into one empty slot per note of the chord, in any
order. A tap on a filled slot takes its note out. The staff above draws the
notes in the slots as one chord (`drawStaff` from `static/notation.js`) as
they fill. Tiles are named in the course's note names. Once every slot is
full the learner checks, and the server grades the notes through
`/api/answer` as a set (`notation::chord_spell`): order and octave don't
count, spelling does. The same sounds spelled otherwise (`D G♭ A` for D
major) are a near miss, marked wrong with "right sound, wrong spelling", as a
missing accent is for a word. It is asked in Practice, the reading check, the
Guide and Sprint mode (`static/chord-spell.js` builds it in each); the AI
tutor leaves it out, since a chat can't tap tiles. The editor offers the type
in a course with a `music` block, with boxes for the answer and the tiles;
its preview shows the tiles in ABC. The feature usage report lists it only in
such a course.

**Place the note** (#1226) is a quiz type of its own, `staff-note`
([course format](course-format#quiz)): an empty staff with a treble or bass
clef and a prompt naming a note. abcjs draws but takes no input, so
`static/staff-note.js` draws this staff itself as an SVG. The whole staff is
one tap target. A tap puts the note on the nearest line or space, with
ledger lines when it sits above or below the staff, and a drag moves it.
Flat and sharp buttons add an accidental. With the keyboard, the up and down
arrows move a dotted note and Enter places it; Enter again checks. The
server grades the note through `/api/answer` (`notation::staff_note`) by
pitch, octave included, unless the question says `any_octave`; the right
note in the wrong octave is marked wrong with "Right note, wrong octave". A
wrong answer draws the right note in green beside the learner's, and names
it in the course's note names. It is asked in Practice, the reading check,
the Guide and Sprint mode, each loading `static/staff-note.js` only when a
question on the page is one; the AI tutor leaves it out, since a chat can't
tap a staff. The editor offers the type in a course with a `music` block,
with a box for the note, the clef and an "Any octave counts" switch; its
preview plays the question. The feature usage report lists it only in such a
course.

**Find it on the keys** (#1231) is a quiz type of its own, `keyboard`
([course format](course-format#quiz)): a piano keyboard of one or two
octaves from middle C, built from buttons by `static/keyboard.js` (with
`static/keyboard.css`), every key at least 44 px wide so it is easy to tap;
two octaves wrap onto two rows on a phone, the higher one on top. Each tap
sounds the key through music-sound.js (following the audio switch), and the
last key tapped before Check is the answer. The page posts the key as an ABC
note with sharps and the server grades it through `/api/answer`
(`notation::keyboard`): by pitch class, so any C is right for `C`, or with
`"octave": true` only that key, where the right note an octave off says
"Right note, wrong octave." Every spelling of a pitch is the same key, so
an ear question whose answer is `_B` takes the black key that is also A
sharp. The loader keeps the answers as ABC notes and shows `correct_answer`
in the course's note names; the answer's `key` field tells the page which
key to light up after the answer. A speaker (`play`) and a staff (`abc`)
above the prompt work as on any question. It is asked in Practice, the
reading check, the Guide (Play) and Sprint mode, which skips it while sound
is off when it has `play` and no `abc`; the AI tutor leaves it out, since a
chat has no keys. Pages load keyboard.js only when a lesson has such a
question (`Lesson::has_keyboard`). The editor offers the type in a course
with a `music` block, with the key, a "One octave | Two octaves" switch and a
"What counts" switch, and its preview plays it; the feature usage report
lists it only in a music course. `PianoKeys.mount()` is reusable: Read at
tempo (#1234) will build on it.

**Build the scale** (#1229) is the Sprint ladder with notes, from a lesson's
`sprint.scales` (a `title`, eight ABC `notes` and `decoys`). The rungs are
the degrees 1 to 8 and the first comes filled; the bank holds the other
seven notes and the decoys, shuffled, in the course's note names. A tap puts
a note on the glowing rung, the staff above draws the notes on the rungs so
far, and the note plays (when sound is on). It is graded at Check, like the
ladder: the sheet says "6 of 8 notes right" and each wrong rung shows its
note. History reads the run the same way. In Study, the lesson page shows
each scale on a staff with a speaker under a Scales heading; the ladder
itself is in Sprint mode only, as the conjugation ladder is. The server
builds and grades it (`src/sprint/scale.rs`), `static/sprint.js` runs the
ladder and `static/scale-ladder.js` draws and plays the notes. The editor's
Sprint section has **+ Scale** in a music course (title, notes, decoys,
checked as you type), and its preview draws and plays the scale.

## Set the clock

A `clock` question (#1340, [course format](course-format#quiz)) shows a
clock face at 12:00 under the prompt. The learner drags the minute hand and
the hour hand follows it, so a quarter to four has the hour hand nearly on
the 4; the hour hand can be dragged too. With the keyboard, the arrow keys
move the minute hand one step (`step_minutes`, 5 by default), Page Up and
Page Down move an hour, and Enter checks. The clock sends the time it shows
as `H:MM` to `/api/answer`, and the server grades it (`src/clock.rs`); the
page never holds the answer. Turned around, with `options`, the clock shows
a time and the learner picks it among the options. Practice, the reading
check, the Guide and Sprint mode (a `clock` exercise) all show it, with the
same widget (`static/clock.js`). Play has no question games, so it has no
clock. The clock has no words on it, only the numbers 1 to 12, so it suits
any course; the prompt carries the language ("Son las seis y media").

## Tap to hear

On every page of a language course (`language_features` in `course.json`,
or unset and `terms.lang` differs from `meanings.lang`), the words in the course's language can be tapped and are
read aloud by the browser's own speech synthesis, with the voice chosen in
the games' voice pickers (or the best installed one): the terms and examples
in a lesson's vocabulary table and in the paradigm table, the **bold** key
words, *italic* examples and table cells of the lesson text (a cell like
"rojo — red" says only the part before the dash), the terms on the
Vocabulary page, the flashcards (a 🔊 button beside the term, in the Guide
and in Vocabulary study), the correct answer in quiz feedback, and the
sentences on the Listening summary. `static/say.js` does it for any element
with a `data-say` attribute or matching its list; there is no audio to host.
It is silent while the audio switch (below) is off. Hovering a tappable word
shows its translation where the page has one (a vocabulary term's meaning, an
example's `example_meaning`), else a "Tap to hear it" hint. A phone has no
hover, so tapping a word also shows its meaning in a small bubble under it for
a few seconds, until the next tap or a scroll. It shows even with the audio
switch off. The meaning comes from the cell's own translation, else the
meaning column of its vocabulary row. A **bold** word in the lesson text uses
the same term in the page's vocabulary table (the whole term, any case). A
word with no known meaning only speaks. Nothing that
speaks ever reads out emoji, arrows or other pictographs (`speech.js` strips
them before every utterance), and a nested 🔊 button is not part of its
card's text. *Italics* in lesson text are taken to be the course's language,
so an English aside must not be italicised.

## Listening (hear it, type it)

Dictation, in Study mode. Each lesson carries `listening`
sentences; the browser's own speech synthesis reads one in the course's
language (`terms.lang`, any installed voice) and the learner types it.
*Replay* repeats it; *Slower* says it again in two-word pieces with a pause
between them, and the speed setting offers the same in short phrases or
word by word, because browsers barely honour low speech rates for system
voices (on macOS, Chrome's slowest rate is only about half as fast again). The server checks it word by
word (`POST /api/listening/check`): the sentence comes back with every word
marked right, accent slip, wrong (what was typed instead is shown struck
through), missing or extra; case and punctuation are ignored; a sentence
counts as right only when every word is exact. Then the translation and the
lesson's `note`, which is either a grammar pointer or the new word the
sentence slipped in ("✦ New word: la oficina = the office"). *Reveal* shows
the sentence and counts as a miss.

A lesson round is all of that lesson's sentences in a random order; the
*mixed round* draws from every lesson with listening material, weighted by
mastery exactly like the practice quiz (weak and untested lessons first, the
next unit peeks in), never repeating a sentence. Finished rounds go to the
activity log (`listening`, board = lesson id or `all`) and show in
History; mastery scores are not touched, since typing a
sentence is not the same as answering the lesson's questions. A round on
one lesson does count as a run towards passing it. Only language
courses have it, and only lessons with sentences appear.

## Guide

Each lesson is a stage of steps: concept cards (one `## ` section each),
quick checks (quiz questions, graded through the normal answer API so mastery
moves too), flashcard hands and tips. Finishing a stage unlocks the next, earns
stars (3 for ≥ 90 % right, 2 for ≥ 70 %, else 1) and XP (5 per card, 8 per
flashcard hand, 10 per correct answer, 2 per wrong one). Finishing a stage
marks the lesson seen and reports its quick checks as a run, so 70 % or more
passes the lesson. A stage shows as complete, and the next one unlocks,
once the lesson is passed in any mode.

## Sprint mode

A session is ~16 exercises: a bite, then rule games, quiz questions,
vocabulary and listening interleaved. Fill-in-blank questions become word
tiles (the right answer plus tempting wrong ones: the accent-less spelling,
other forms of the same item, other fill-in-blank answers from the lesson,
then words from its listening sentences and vocabulary terms), with "type it
instead" as the alternative. A gap with fewer than three tiles that differ
by more than accents gets none: the learner types it. The home screen shows a daily ring (20 answers), the
day streak, XP and the path of lessons grouped by unit. *Review mix* draws
from the adaptive quiz over the lessons the learner has read (weak spots
first); when those hold fewer than a round (8 questions), the rest come from
the next unread lessons on the path, each tagged "A taste of what's next ·
<lesson>" (`sprint.session.taste_tag`), and its rule games, words and
listening come from read lessons only. *Lightning* is 60 seconds of one-tap
questions.

**Sort it** judges each tap at once (#1148). The right bucket takes the
word on top of the pile, shown in green; a wrong bucket flashes red, the word
shakes and stays on top of the pile, and that counts as a wrong tap. The
sort is checked by itself once the pile is empty: right with no wrong taps,
wrong with any, and then the sheet says "All sorted, but with 2 wrong taps".
History reads it the same way: "Sorted 6 of 6 correctly" for a clean run,
"2 wrong taps, then all 6 sorted correctly" (✗) when there were taps.

**The ladder** (a lesson's `conjugations` with the course's paradigm) is
graded only at Check; nothing shakes or is judged on the way. Every tap on a
form puts it on the glowing rung, the first empty one, right or wrong; tap a
filled rung to send its form back to the bank. Check opens once every rung
is filled. Each rung then shows right (green) or wrong (red and struck
through, with the right form beside it). The sheet gives the score ("1 of 3
forms right"), with the key when anything is wrong; the ladder is right when
every form is in place. History reads the run as that score. Ladder rows
stored before grading moved to Check (#640) also count the wrong taps, and
put them first when the end state was all right ("1 wrong tap, then all 3
forms right"), so the words never contradict the row's ✗. **Build the
scale** is the same ladder with notes (see [Music notation](#music-notation)).

**Audio.** In a language course only (as for Tap to hear; elsewhere Sprint
is silent, with no speaker buttons and no audio switch), the browser's
speech synthesis reads words aloud, as on the full site: every tile, option or word in the course's language is read out when
tapped (word tiles, multiple-choice options, the forms of a ladder, the words
of a spot-the-mistake sentence, the term tiles of *match the pairs*), a term
shown in the course's language (the "this means…" exercise, a bite's
example) is read out and can be tapped to hear it again, the collected words
and the correct answer on the result sheet have a speaker, and a sentence
(build, tap what you hear) is read out once it is checked. Options in
the learner's language and the written side of *listen and match* stay
silent. The 🔊 button in
the top bar is the audio switch, off, nothing is read and
the listening exercises are skipped; every listening exercise also has
*Can't listen now*, which switches audio off for an hour. The switch is
shared with the full site (`speech:audio` in `localStorage`). The smaller
🎵 button is for the sound effects (off by default). Listening exercises
are also skipped when the browser has no voice for the language.

**Listening exercises** (language courses only), four per lesson session
and two in a review mix, of their own. Every one starts with a big
speaker (auto-played) and a 🐢 button that says it slowly, in two-word
pieces. Sentence ones come from the lesson's `listening` sentences and count
towards mastery like a rule game; word ones come from its `vocabulary` and
count as flashcard reviews.

- **Tap what you hear**: the sentence from word tiles, plus three words from
  the lesson's other sentences as decoys (sentences of 2–10 words). Typing a
  sentence from the audio alone proved too hard for a phone session; that
  stays in Study mode's Listening practice.
- **Fill in what you hear**: the sentence is shown with one word missing (a
  vocabulary term if the sentence has one, else a word of four letters or
  more); pick it from up to four tiles: the word, its accent-less spelling
  and words of a similar length from the lesson's other sentences.
- **What do you hear?**: a term is spoken; pick it among written options
  (its accent-less spelling and look-alike terms of the lesson). The
  variant **What does it mean?** speaks a sentence and asks for its meaning
  among the meanings of the lesson's other sentences.
- **Select the matching audio**: a meaning is shown; three speakers each say
  a term, pick the one that says it.
- **Listen and match**: five speakers on one side, the written terms on the
  other; a matched speaker turns into its word.

Finished listening exercises appear in History like the other Sprint mode
exercises (`hbuild:<lesson>:<n>`, `hgap:<lesson>:<n>:<word>`, `hmean:…`).

## Games

**Word Match** deals a lesson's vocabulary (or 25 random terms from the whole
course) in rounds of five pairs. The clock runs on the server between start
and finish; every wrong pair costs a second; top 10 per board is shared by all
users of the course.

**The drill** (Conjugation Drill for Spanish) shows a picture for the item and
a picture for the slot; type the form. A slot shows its ASCII `art`. Items unlock when their lesson is
passed in any mode, or mastered in Practice. The setup page (`/play/conjugation`)
shows one set at a time (#837): a tab per set with its unlocked count, the
first set with something unlocked open by default (`?set=<key>` picks one),
and the lessons that unlock the rest 8 a page (`&page=<n>`). Only courses with a
`paradigm` in their manifest have it.

**Numbers by ear** (`/play/numbers`) says a random number and you type its
digits. The browser's own speech synthesis does the talking, in the manifest's
`terms.lang`, so there is no audio to host and any installed voice for that
language can be picked (Spain or Mexico for Spanish, say); speed is
adjustable. Ranges: 0–10, 0–20, 0–100, 0–1 000, 0–10 000, 0–1 000 000, years
1900–2030, or a custom pair. Wide ranges mix magnitudes so short numbers still
come up. *Replay* (R) repeats the number, *Reveal* (Q) shows it and counts as a
miss; `12k` and `1.5m` are accepted for 12 000 and 1 500 000, as are thousand
separators. A round is 10, 20, 50 or endless numbers; a wrong answer resets the
streak. Finished rounds go to the activity log with the range and the best
streak; there is no leaderboard. Only language courses have it: those whose
`terms.lang` is set and differs from `meanings.lang`.

**Date Dash** (`/play/dates`) drills the days of the week and the months
until they are automatic: a 30, 60 or 120-second sprint (or 20 prompts) of
one-tap questions. The names in the course's language come from the
manifest's `calendar`; the learner's own names come from the browser (Intl,
in `meanings.lang`), so the game is not tied to any language pair. Prompts
are mixed: see a day or month, tap it in the course's language; see it in the
course's language, tap the meaning; "the day after *martes*" and "the month
before *enero*" for the sequence; a calendar page (weekday, day, month, year)
to turn into a date, the weekday first, then the month, with the manifest's
`format` filling in as you go (the first full date of every round is today's);
and, when the browser has a voice for the language, prompts spoken aloud,
including a whole date to find among four calendar pages whose decoys differ
in weekday, month or year. Drills: days, months, both, full dates, everything.
Answering is one tap or one key (1–9, 0, -, =); a right answer moves on after
a third of a second, a wrong one shows the right tile and the translation for
just over a second. Points: 100 plus up to 100 for speed (full bonus under
0.4 s, nothing after 3.4 s), times a combo multiplier that steps up every four
right answers in a row (×2, ×3, ×4) and resets on a miss. Each day and month
carries a weight: a miss multiplies it by 2.5, a slow right answer by 1.3, a
quick one by 0.6, and the next prompt is drawn by weight, so the round keeps
returning to what is not yet automatic. The summary lists what was missed and
the slowest items, with a speaker button when a voice exists. Finished rounds
go to the activity log with the drill, the length and the points; the hub and
the setup page show the personal best. Only courses with a `calendar` have it.

## Profile

The gear in the top-right corner opens the account menu: profile settings,
switch course, documentation, log out. `/profile` is independent of the active
course and is where per-user settings live; new customisation options for a
learner belong on this page. A list of links on the left (above the cards on
a phone) jumps to each section; a new section gets an `id` and a link there,
named like the section's heading. The link of the section in view is marked
(`aria-current="location"`, bold with an accent bar; `static/profile.js`),
after a jump and while scrolling.

**Language.** The Personal information card has the language the app is
shown in (the same picker the login and register pages have); it saves at
once and the page reloads in the new language. See [i18n](i18n).

**Saving as you edit.** The names, the AI API keys
and the Dev options have no Save button: each change posts to
`POST /api/profile` (only the fields that changed; all or nothing, a refused
field comes back as `422 {"errors": {field: message}}`). Names save one
second after the last keystroke, or when the field loses focus; an API key
three seconds after the last keystroke, or at once when more than five
characters arrive together (a paste); a checkbox
saves at once. A field whose change is saved shows a small green check (on
the chosen chip or picture for a choice) until it is edited again. The
circle at the right of the heading sums up the page: green ✓ saved, yellow
while saving, red ! when something is not saved. A field that
does not validate turns red with its message under it, at once and without
being sent; a lost connection says so in red at the top of the page and
retries every few seconds and when the browser comes back online. While
anything is unsaved, a link or form on the page asks first in a dialog that
says why, and reloading or closing the tab gets the browser's own prompt.
`static/autosave.js` does all of this for any page; `static/profile.js`
wires up the fields (the course editor uses it too: see *Saving as you
edit* under [Your own courses](#your-own-courses)). The password dialog, *Remove key* and account deletion
keep their explicit buttons. The page needs JavaScript.

- **Personal information**: first and last name (optional, up to 60
  characters). The site greets by first name and the menu shows "First Last"
  ("Learner" until a name is set); leaderboards show the first name and the
  last initial ("Ana L."). An (i) button by the "Personal information" heading opens a
  dialog that says so, with the learner's own "Ana L." (updated as they type),
  and that the address is private.
- **E-mail address**: what the account signs in with (there are no
  usernames). Asked for at registration and
  fixed from then on. The profile shows it read-only ("No e-mail on file"
  for an account from before addresses were asked for).
- **Account**: member since, how the account signs in, and for password accounts a
  *Change password* button that opens a dialog (current password, the new
  one twice). The browser keeps it open until the two new ones match and are
  8 characters or more; a refusal from the server (wrong current password)
  comes back with the dialog open and the reason in it. Saved, the dialog
  closes and the page says "Password changed." (`static/profile.js` opens
  and closes both dialogs.)
  *Sign out everywhere else*
  (`profile-sign-out-others`, `POST /profile/sessions/end-others`, #375,
  explained behind its (i)) ends every other session; changing the
  password or two-factor sign-in does the same.
- **Privacy and consent** (#974, `profile-consent`): one row per consent
  with its date, explained behind an (i). The Terms version accepted and
  when, with links to the Terms and the privacy policy (see
  [Terms of Service](#terms-of-service)); the age confirmation (the date of
  the first Terms acceptance, read-only: taking it back means deleting the
  account); e-mail, service messages only (hosted only;
  an optional e-mail such as reminders gets a switch here, off by default);
  and *Do Not Show Me AI Features*, "Shown since …" or "Hidden since …"
  with the profile's only AI switch (below; on hides them, `POST /profile/ai`
  returns here) and a line when an own API key is on file. Optional consents are
  rows in `consents` (`src/consent.rs`): given at `granted_at`, taken back
  at `withdrawn_at`, a new row each time one is given again; they are in
  *Download my data* and go with the account.
- **Proof that it is the owner** (`prove_owner` in `src/routes/profile.rs`):
  every change a stolen session must not make asks for it: the password
  (checked like `/login`: 10 per account per 15 minutes, a wrong one
  counting against sign-in, refused while sign-in is blocked, #378); an
  account without one (signed in through a proxy) gives a code from its
  second step, or with no second step nothing (#376). It guards the
  password change, deleting the account and every two-factor change.
- **Do Not Show Me AI Features**: an On/Off toggle pill in the Privacy and
  consent card (#147, #974, `users.ai_off`, off for every account unless the
  "Hide all AI features" pill on the registration form was turned on: `templates/ai_switch.html`, a
  checkbox `no_ai` dressed as the pill, its info dot a hover tooltip).
  The card's (i) says what it covers. Switching it on posts
  `POST /profile/ai` (`off=1`) at once, or first opens a confirm dialog
  when an AI API key is on file, because hiding the features deletes every
  saved key. On, the account is shown none of those
  features: no AI tutoring card on the home page or tutor links on a
  lesson, no *Generate with AI* card, *Resume* or *Watch* on the course
  picker, no *Use AI* form or resume button on the course editor, and no
  *AI API keys* section here (the keys were deleted). Their
  routes refuse too: `CurrentUser::require_ai` sends a page request to this
  section with a notice, and `require_ai_api` answers a JSON route with 403.
  A tutor conversation under way is dropped. Switching back off needs no
  confirmation (`off=0`). What is left never contacts an AI company.
- **AI API keys**: one key each for Claude (Anthropic), OpenAI and Google
  (Gemini), used by AI tutoring, *Generate the course* and *Use AI* (the
  card says so in one line; how keys are stored is behind its (i)). A key is checked for the
  provider's prefix (so a key pasted into the wrong box is caught), encrypted
  at rest (`src/secrets.rs`, see [architecture](architecture#security-notes))
  and never shown again; the page shows its last four characters, and a key
  can be replaced or removed at any time. Each provider is a row folded
  shut (name and status); its triangle opens the box to paste a key into
  and *Remove key*. Features call
  `secrets::get(conn, &state.vault, user_id, provider)` to use one.
- **Your data**: *Download my data* (#430): *Prepare my data*
  (`POST /profile/export`) queues an export; the `export` job on the
  background runner (`src/exports.rs`, `src/jobs.rs`) writes every row stored for the account, by table, with
  the course files, to a file a page at a time, and e-mails a one-time link
  (`GET /profile/export/download?token=…`, 24 hours, signed in as that
  account only; the file is deleted once sent). Once it is ready the Data
  section also shows *Download my data* (`profile-export-download`,
  `POST /profile/export/download`: the same file, once, for the account
  signed in). The finished file stays on the server's disk, next to the
  database, and the download streams it. On a server without mail (#633) that button is the only way:
  no link is mailed and the (i) says so. One export in flight and one
  a day per account; the (i) next to the heading says where it stands. The
  file has the columns `privacy::EXPORTED` lists: never the password hash,
  session tokens, keys (only which providers have one), codes, links or the
  text of mails. And
  *Delete account* (`POST /profile/delete`): the password, or for an
  account without one its e-mail address typed out and, with a second
  step, a code from it (`profile-delete-code`, *Send code*
  `profile-delete-send` for an e-mailed one; #376). It does not delete at once (#970,
  `src/account_deletion.rs`): it sets `users.delete_at` 14 days out, ends every session
  of the account and, where the server sends mail, mails the date
  (`account-deletion`). The login page says the date (`login-notice`).
  Signing in again works as usual, but every page then leads to
  `/account/deletion` (`account-deletion`, date `account-deletion-date`,
  *Cancel deletion* `account-deletion-cancel`, *Log out*
  `account-deletion-logout`; API routes answer 403), and the account's
  shared courses are hidden from their recipients meanwhile. The hourly
  `housekeeping` job then removes the account and everything it
  owns. What is kept and for how long:
  [data inventory](data-inventory); the privacy notice is at `/privacy`
  ([privacy](privacy), public, with a "Last updated" line at the top).
- **Manage Users page** (`/manage`, owner accounts only, #220): actions
  that reach any account, for people running the app; the account menu
  links to it. An account is an owner when `users.owner` is 1. With `SKRONGLE_SIGNUP` `invite` or `closed` (#634)
  the first account made on a server with no accounts becomes one (the
  owner, made only through the owner link from the log; one query in the
  registration transaction);
  otherwise an owner sets it with **Make owner** on the page, or
  `./spanish-learning make-owner <email>` does (#635; see
  `docs/self-hosting.md`, "If you lock yourself out"). Migration 98 (#16)
  set it for every account that was a superuser or a developer before.
  In dev mode the
  dev user (`dev@example.com`) always is one. For anyone else the menu has
  no link, and the page and its forms answer 404 (`src/routes/manage.rs`).
  - **Accounts** (#634, `src/signup.rs`): the (i) says which sign-up mode
    the server is in (`SKRONGLE_SIGNUP`: `open`, the default and the hosted
    site's; `invite`; `closed`), how invitations work and how many accounts
    there are. In `invite` mode,
    **Make an invitation link** (`POST /manage/invites`, 404
    in other modes) shows `<SKRONGLE_PUBLIC_URL or the request's host>/register?invite=<token>`
    once, with **Copy**; the token is 32 random bytes, hex, stored only as
    its SHA-256 in `signup_invites`, and works once, for 7 days. The open
    invitations are listed (made, expires) with **Revoke**
    (`POST /manage/invites/revoke`). Below, every account, newest
    first, all on one page (the server holds at most 8): e-mail address, created, last sign-in
    (the newest session on record; sessions last 30 days), and the
    actions per row, each opening a dialog with the address, what it does
    and a confirm button. Nothing is sent to the account; each writes one
    `info` log line (which owner, which account id, what; never the
    password). Three are the owner's (#635, #16, `src/owner.rs`):
    - **Temporary password** (`POST /manage/temp-password`,
      `email=`): a generated 14-character password (letters and digits
      without look-alikes) is set, and shown once in a dialog with
      **Copy** (`Cache-Control: no-store`). Every session of the account
      ends (an owner resetting their own keeps the current one), and
      `users.must_change_password` is set: after signing in with it, the
      `CurrentUser` extractor sends the account to
      `/password/change?next=…` before any other page (`/api/*` answers
      403), like the Terms gate. That page asks for the new password twice
      (not the old one), ends other sessions, clears the flag and goes on
      to `next`. Choosing a password anywhere clears the flag
      (`profile::set_password_hash`).
    - **Turn off two-factor** (`POST /manage/two-factor-off`):
      `users.two_factor` is cleared, and the account's `totp_keys`,
      waiting sign-ins (`two_factor_challenges`) and change confirmations
      (`two_factor_confirmations`) are deleted.
    - **Make owner** (`POST /manage/make-owner`, #16; only on rows that
      are not owners yet): `users.owner` is set, so the account sees
      Manage Users in its menu and may take every action here. The page
      does not take it back.
    An address no account has comes back as `No account has the address …`.
    The same three actions exist as commands on the binary for an owner
    who can't sign in: `reset-password`, `disable-2fa`, `make-owner`
    (`docs/self-hosting.md`, "If you lock yourself out").
  - **Sign-up** under `invite` and `closed`: while the server has no
    account, every start makes a new **owner link** (`signup::owner_link`:
    a `signup_invites` row with no `created_by`, one year, replacing the
    last) and logs it as a warning, `Owner link, …: <url>/register?invite=<token>`
    (`SKRONGLE_PUBLIC_URL`, else `http://<hostname>.local:<port>`, or the
    loopback address the server listens on). So whoever reaches the server
    first can't claim it: only someone who reads the log can. `/register`
    without it is the closed page saying the server has no owner yet;
    with it, the form with a line saying this account is the owner's
    Once an account exists, unused owner
    links are deleted at startup and refused meanwhile. After that, `/register` without a usable link is a short page
    ("Sign-ups are closed", ask whoever runs the server; no address, no
    form) and `POST /register` answers 403 with it; a used, expired or
    unknown link says which. `/register?invite=<token>` shows the form,
    which carries the token. Existing accounts always sign
    in. Under `closed` an invitation never works. Rate limits are as for
    any registration.
  - **Reset email verification** (`POST /manage/reset-email-verification`,
    the third row action): the account is marked unconfirmed again and sent
    a new confirmation link, exactly as at registration; links sent before
    stop working. The notice says so (`No account has the address …` when
    none does) and, with the dev mailbox, links to the new message.
    The log records which owner reset which account. Signing in through
    `/dev/login` confirms the dev user again.
  - **Coming change to the terms** (#966, hosted only): while a material
    entry of `docs/terms-changes.json` has a later `effective` date and
    some account with an address has not had its e-mail, the page shows
    "The terms have changed." and a card with **Send the announcement**.
    Its confirm dialog says how many accounts it goes to;
    `POST /manage/terms-announce` queues them (`terms::announce_upcoming`)
    and the notice says how many were queued. Once everyone has it, the
    notice and the card go away.

Each AI form says where the learner's words go (`data-testid="ai-disclosure"`):
"Your notes are sent to your AI provider using your key." on *Generate* and
*Use AI*, naming the provider ("… sent to Claude (Anthropic) …") only when
two or more keys are saved, and "Your messages are sent to Claude
(Anthropic) …" in the tutor.

## Terms of Service

`/terms` (public; `/terms.md` for the Markdown) shows `docs/terms.md`, the
Terms and Conditions of the hosted site (first published 2026-10-25, from a
Termly export). `/docs/terms` redirects there; the docs nav leaves it out.
The text names no address itself. `[SUPPORT EMAIL]` (help, plans, SMS)
becomes `SKRONGLE_SUPPORT_EMAIL` and `[CONTACT EMAIL]` (permissions,
copyright, harmful content) becomes `SKRONGLE_LEGAL_EMAIL`, each as a
`mailto:` link. Every e-mail the server sends has `Reply-To:` the support
address (#1328).

- **Accepting.** Registration has two required boxes, "I am at least
  [age] years old" (`age`; `SKRONGLE_MIN_AGE`, default 14)
  and "I agree to the Terms and Privacy Policy" (`accept`;
  `templates/terms_check.html`, each sentence with a rounded check box on
  its right in the AI pill's style); the server refuses the form without
  both, naming the missing one. Each acceptance is a row in `terms_acceptances`
  (user, version, time; no IP address), so the history is kept.
- **The changelog.** Every version of the built-in text is an entry in
  `docs/terms-changes.json` (#966): `version`, `announced` and `effective`
  dates (`YYYY-MM-DD`, UTC), `material` (it affects learners' rights or a
  paid plan) and a `summary` of a few plain sentences, in English like the
  terms. `terms::version()` is the newest entry in effect today, and the
  "Last updated" line at the top of `docs/terms.md` is the newest entry's
  date. Changing the text means adding an entry in the same PR: the
  `terms-lint` CI job (`scripts/terms-lint.sh`) fails a PR that edits one
  without the other, and `cargo test terms` checks the entries (dates in
  order, a summary, 30 days between `announced` and `effective` for a
  material change, the "Last updated" line). A material change to the
  privacy notice (`docs/privacy.md`, `/privacy`) is logged the same way
  (#978): one entry whose summary starts "The privacy policy changed: …",
  so learners get the same notice and, for a material one, the same
  30-day e-mail. The lint watches `docs/privacy.md` too, and the newest
  privacy entry dates that page's "Last updated" line. A minor privacy
  change (a correction, a new provider of the same kind) only moves that
  line. In Skrongle mini the sign-in notice is the whole notification (no
  mail).
- **New versions.** An account that accepted an earlier version sees a
  notice over the first page it opens (`templates/terms_notice.html`,
  `data-testid="terms-gate"`, `data-tier`): the summaries of every change
  since, and a link to `/terms`.
  - *Minor change*: **Continue** (`POST /terms/dismiss`) closes it and
    records acceptance of the current version; it can stay open while the
    learner reads, and nothing is blocked. Escape hides it for that page only.
  - *Material change* (or a version from before the changelog): the accept
    box and button of the gate, no way to close it, and a line pointing to
    log out or close the account from the profile. It is on every page but
    the profile; `/api/*` gets `403` and any other form (except deleting the
    account) is sent to `/terms/accept` until it is accepted.
  - *30 days ahead*: a material entry with a later `effective` date is
    e-mailed once to every account with an address (purpose `terms-change`:
    the date, the summary, a link to `/terms`) when an owner presses
    **Send the announcement** on `/manage`; nothing sends it on its own.
    `terms_announcements` records who got it, so pressing again only mails
    accounts made since (or held back by the hourly mail ceiling). It only takes effect, and the notice only appears,
    on its date. Not in Skrongle mini (no e-mail).

  An account that never accepted any version (one from before the terms)
  is sent to `/terms/accept`, and back to the page it asked for afterwards;
  `/api/*` gets `403`. Pages that work logged out (`MaybeUser`: the landing
  page, docs, `/terms`, login) show no notice and are not gated.
  `/dev/login?terms=old` (material) and `?terms=minor` leave the dev user
  one version behind, for the browser tests.
- **Your own text.** A server with its own text (`SKRONGLE_TERMS=<file>`,
  Skrongle mini) has no changelog: it uses the file's SHA-256 as the
  version, so any edit sends every account to `/terms/accept` (no summary,
  no e-mail).
- **Links.** The footer on every page, the landing page, login and register
  link Terms and Privacy (`/privacy`, its own page); the profile shows the
  version accepted and when, and its "read" link opens the terms in a
  dialog over the profile (the link still goes to `/terms` in a new tab
  or without JavaScript).

## Registration and password reset

`/register` (e-mail address, password, the [Terms](#terms-of-service)
boxes, the "Hide all AI features" pill) never says whether an address has
an account (#379, `src/reset.rs`):

- **The answer** to a valid form is always the same page, "Check your mail"
  (`register-sent`, with the address typed and a *Sign in* button,
  `register-sent-login`). Registering does not sign in: the learner signs
  in on `/login` with the password they chose (confirmed or not). The
  password is hashed (argon2) whatever happens next, so the answer takes as
  long either way. Form mistakes (a missing box, a short password, a
  malformed address) still come back on the form.
- **A new address**: the account is made and the
  [confirmation link](#e-mail-verification) is sent.
- **An address that has an account**: nothing is made or changed. The
  address is sent "You already have a Skrongle account" (outbox purpose
  `register-taken`, at most 3 per address per hour) with a link to sign in
  and one to `/reset`.
- **An unconfirmed account old enough**: a password account whose address
  was never confirmed, made `SKRONGLE_UNCONFIRMED_DAYS` (default 7; `0` never)
  days or more ago, gives the
  address up: registering it again deletes that account and everything in
  it (`privacy::delete_account`, as *Delete account* does) and makes the new
  one. Nobody can hold an address they cannot read.
- **Not in Skrongle mini.** All of this hides behind a mail, so without a
  mail provider (`caps().mail`, #633) `/register` works as before: it says
  when an address has an account, signs the new account in at once and
  replaces no account. `/reset` is a 404 there and the login page has no
  *Forgot your password?* link; the owner sets a temporary password
  instead (the Manage Users page, under [Profile](#profile)).

**Password reset** (`/reset`, linked from the login page as *Forgot your
password?*, `login-reset-link`):

- **Asking** (`reset-email`, `reset-submit`): the page always says "If
  that address has an account, a link was sent" (`reset-sent`). An account
  gets a message (outbox purpose
  `password-reset`) with `<SKRONGLE_PUBLIC_URL or the request's host>/reset?token=…`.
  At most 3 links per address and 10 requests per network per hour; past
  the address's limit nothing is sent and the page says the same.
- **The link**: 32 random bytes, hex; only the SHA-256 is stored, in
  `password_resets`. It works once, for an hour, and only while the account
  still has the address it was sent to; a new link voids the unused older
  ones. It opens a form for the new password (`reset-password`,
  `reset-save`) naming the account (`reset-account`); a used, expired or
  unknown link gets `reset-invalid` with *Send a new link*.
- **Saving** sets the password, confirms the address (the link proved it is
  read), ends every session of the account (this browser's too) and shows
  "Your password is changed" (`reset-done`). It signs nobody in, so
  [two-factor sign-in](#two-factor-sign-in) still applies at the next login.
  New passwords tried with links: 10 per network per 10 minutes.
- **Dev mode**: both messages are in the dev mailbox (`/dev/inbox`) and the
  reset link is logged at `info`. Rows are deleted 7 days after they were
  made (hourly housekeeping).

## E-mail verification

A link proves the learner owns the address the account signs in with
(`src/verify.rs`). Only [two-factor sign-in](#two-factor-sign-in) is gated
on it (e-mail codes need a confirmed address); login, registration,
learning and the rest of the profile work the same confirmed or not
(`CurrentUser::email_verified`), except that an account never confirmed
gives its address up to a new registration after `SKRONGLE_UNCONFIRMED_DAYS`
(see [Registration and password reset](#registration-and-password-reset)).

- **When a link is sent** (outbox purpose `email-verify`, through
  `Mailer::queue`): right after registering with e-mail and password; when a
  owner resets the account's verification (the Manage Users page, it starts
  unconfirmed again); and on *Send the e-mail again*
  (`POST /profile/verify-email/resend`, 5 per account per hour, past that it
  silently sends nothing).
- **The link** is `<SKRONGLE_PUBLIC_URL or the request's host>/verify-email?token=…`:
  32 random bytes, hex; only the SHA-256 is stored, in
  `email_verifications`. It works once, for 24 hours, and only while the
  account still has the address it was sent to; a new link voids the unused
  older ones. Opening it (signed in or not, in any browser) sets
  `users.email_verified_at` and shows "Your e-mail address is confirmed";
  it signs nobody in. A used, expired or unknown link gets a neutral page,
  with *Send a new link* when signed in.
- **Where it shows**: inside the profile's address field, a green check
  (`email-verified`, "Verified" on hover) or a red ! (`email-unverified`)
  that opens a dialog, "Email not yet verified. Check your inbox for the
  link.", with *Resend* (`email-verify-resend`). No other page mentions it.
- **Without mail** (#633, no `SKRONGLE_MAIL_PROVIDERS` outside dev mode): no
  link is issued or queued (`verify::send` logs and returns), the address
  field shows an (i) (`email-no-mail-info`) whose dialog says the server
  cannot send mail, in place of the red ! and *Resend*; *Send the e-mail
  again* and the owner's reset are not offered.
- **Dev mode**: the message, link included, is on `/dev/mail` (a page in a
  browser, JSON for scripts) and the link is logged at `info`; the dev user
  is confirmed. Rows are deleted 7 days after they were made (hourly
  housekeeping); existing accounts start unconfirmed.

## Two-factor sign-in

After the password an account can be asked for
a code too (#292, `src/two_factor.rs`). One method at a time
(`users.two_factor`), chosen on the profile's **Two-factor authentication**
card (`profile-two-factor[data-method]`, chips `profile-two-factor-method[data-method]`
for Off, E-mail and Authenticator app; picking one posts
`POST /profile/two-factor` and the page comes back saying what is on).
Choosing another method replaces the one in use.

**Only what the server can send** (#633). Without a mail provider the
E-mail chip is left out; the authenticator app needs none. Posting such a method anyway is refused (422) with a
message naming the setting. An account already on a method whose channel
the server has lost cannot sign in: after the password the
login page comes back (403) saying so and naming `SKRONGLE_MAIL_PROVIDERS`; no code is made.

**Every change asks who it is** (#374). Choosing Off, another method or,
with it off, any method (or the app's *Remove*) opens *Confirm it's you*
(`two-factor-confirm-dialog`, saying the change in
`two-factor-confirm-what`): the account's password
(`two-factor-confirm-password`, *Confirm* `two-factor-confirm-submit`),
checked on the server like `/login` (10 tries per account per 15 minutes;
wrong ones count against the sign-in limit). A missing or wrong password is
refused (403) with the dialog open again and `two-factor-confirm-error`.
An account without a password (signed in through a proxy) types a code from its current
method instead (`two-factor-confirm-code`): the authenticator's, or one
e-mailed by *Send code* (`two-factor-confirm-send`,
`POST /profile/two-factor/code`, purpose `two-factor-confirm`, 10 minutes,
five tries, 5 sends per 15 minutes). One with it off has nothing to type:
*Confirm* makes the change. Once made, every other session of the account ends and the
account's address is e-mailed (purpose `two-factor-changed`), so an
owner whose session someone else used hears of it. Recovery codes are not
offered yet. Without JavaScript the chips post with *Save* and the page
comes back with the dialog shown as a form.

- **E-mail**: a 6-digit code e-mailed to the account's address (outbox
  purpose `login-code`). Not offered until the address is
  [confirmed](#e-mail-verification): the chip is disabled, with a red !
  (`profile-two-factor-email-unavailable`) whose dialog has *Resend*.
- **Authenticator app** (Google Authenticator and the like; RFC 6238 TOTP,
  SHA-1, 6 digits, 30 s, `src/totp.rs`): picking it starts the set-up, a
  dialog (`two-factor-totp-dialog`) with a QR code of the
  `otpauth://totp/Skrongle:<address>?…` link for the phone's app to scan
  (`two-factor-totp-qr`, inline SVG drawn by `totp::qr_svg` with the
  `qrcode` crate, no image library; #304; the link itself is its `data-uri`),
  how to scan it behind an (i) by the dialog's title (`profile-2fa-totp-qr-info`,
  its modal `profile-2fa-totp-qr-info-dialog`; the dialog has no other text),
  the setup key in groups of four on one row, for typing it by hand
  (`two-factor-totp-secret`), and, in dev mode, *Dev: add it to the dev
  authenticator* (`two-factor-totp-dev`, the "scan" into [the dev authenticator](testing#two-factor-codes-the-dev-authenticator)).
  The app's first code (`two-factor-totp-code`, *Turn on*) makes it the
  method. One app per account; *Remove* (`profile-two-factor-totp-remove`)
  forgets it and turns two-factor sign-in off (so it asks for the password
  like Off). The app's secret is stored encrypted, like the AI API keys. Leaving it for another
  method removes it too: its secret is not kept unused.
- **Signing in**: once the password is right, an account with a
  method gets no session yet but a `twofa` cookie (10 minutes) naming a
  challenge, and `/login/code` (`login-code[data-method]`) asks for the
  code (`login-code-input`, `login-code-submit`), saying where it went
  (`login-code-sent-to`: `a•••@example.org`, or the app).
  *Send a new code* (`login-code-resend`, 5 per account per 15 minutes) and
  *Cancel* (`login-code-cancel`). The session is made only after the right
  code. In dev mode a small window floats at the bottom (`login-code-dev`)
  framing [the dev device the code went to](testing#two-factor-sign-in-the-dev-device):
  this sign-in's mail or authenticator entry, each with a *Use*
  button that types the code.
- **The rules**: an e-mailed code works for 10 minutes and five
  tries (`login-code-error` says how many are left; the fifth wrong one ends
  the sign-in); a new code voids the old one but keeps the tries already
  used (five per sign-in, not per code). Codes are stored only as
  SHA-256 with a random salt and the cookie's token, itself stored only as
  its SHA-256. An authenticator code is taken one step (30 s) either side of
  now, and each step only once, so a code seen over a shoulder cannot be
  replayed. Wrong codes count towards the sign-in limits like wrong
  passwords. There is no "remember this device": every sign-in asks.
- `/dev/login` skips the second step (it is the dev shortcut); the real
  `/login` never does.

## Install as an app

A phone or desktop browser can add Skrongle to the home screen ("Install
app" in Chrome, "Add to Home Screen" in Safari). It then opens full-screen
at the home page, with the Skrongle icon. Every page links
`/manifest.webmanifest` (`web_manifest` in `src/routes/mod.rs`), which
carries the base path in `start_url`, `scope` and the icon paths
(`static/icon-192.png`, `static/icon-512.png`). The same in the hosted site
and Skrongle mini; a browser only offers it on an HTTPS page or localhost.

There is no service worker and nothing is cached for offline use. Sprint
grades each answer on the server, so studying offline is a separate piece
of work (#345).

## Adding features

Every feature is a Rust module (`src/<feature>.rs` for the engine,
`src/routes/<feature>.rs` for the handlers, `templates/<feature>.html`,
optionally `static/<feature>.js`). Handlers take the `Project` extractor,
which carries the signed-in user and the active course (`Arc<ContentStore>`),
and persist with a `course_id`. New lesson fields go in `src/content.rs`
(with validation) and in [the course format](course-format). Keep the two
in step; the format page is what LLMs are given to write courses. A feature
that course files can switch on must also be added to the test course
(`data/test-courses/test-course/`, see [testing](testing#the-test-course))
and to the browser tests that use it, in the same change. Every piece of
text the feature shows (templates, handler messages, what its script draws)
goes into `i18n/en.yaml` and every other `i18n/<code>.yaml` as a key, never
into the markup or code: see [i18n](i18n).
