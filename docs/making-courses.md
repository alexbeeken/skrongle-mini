# Building and generating courses

Every course of your own starts from one button, *Courses → Create a
course* (`/courses`). Your courses are private until you share them (below).
On the course list each of yours has *Open* (to learn from it) and
*Edit files* (this page).

It makes a starter course called *Untitled course*, with one example lesson.
Its title, description, unit and lesson names are in the app language you
use. It opens its page with the **Generate the course** dialog open. Fill it in
to have AI write the course, or close it to write the course yourself.

## Build one by hand

1. Close the Generate dialog. Click the title at the top of the page to
   rename the course.
2. The course page (`/courses/<id>`) shows its lessons as a table. The first
   row is the course settings (`course.json`).
   - **+** on a unit adds a lesson there; name it on the spot.
   - Click a name to rename it; drag a row by its grip (or focus it and use
     ↑ / ↓) to reorder it; ✕ deletes it.
   - ⧉ duplicates a lesson: the copy, called "&lt;title&gt; (copy)", lands right
     below it with new ids, ready to edit.
   - ✦ (with an AI key) edits one lesson with AI: say what
     should change ("more worked examples, simpler words") and only that
     lesson is rewritten. Undo puts it back. For changes across many lessons,
     use **Edit** in the Use AI box instead.
   - **▸** opens a row's JSON in place. On a new lesson, paste a whole lesson
     file there. The gear on the **Course settings** row opens **Editor
     settings**; turn its **JSON** switch off and the same file opens as a
     form instead.
   - **Paste a word list** (language courses) turns a list you already have,
     one `word — meaning` per line (or a comma, `=` or tab between them),
     into a new lesson with those words. It shows what it read first, and the
     lines it couldn't, before you add it. No AI key needed.
3. There is no Save button: every change is saved as you make it (typing,
   three seconds after the last keystroke). The circle by the course title
   is green ✓ when everything is saved, yellow while saving, and a red ! with
   the reason under the title when something is not. The whole course is
   checked on each save; if it would not load, nothing changes.

The **Undo** and **Redo** icons under the table take back the last stretch
of editing (or the last AI change) and put it back, up to ten steps.
**Download all files**, at the bottom of the page, exports the course as one
JSON file.

The format: [How a course is built](course-structure), and every field in the
[course format](course-format).

## Write lessons with an LLM you already use

Paste the [course format](course-format.md) (raw Markdown) into the chat, then
ask for one lesson at a time:

> Here is the course.json of my course (paste it). Write the lesson file for
> "Ordering coffee" in unit "1", group "phrases", following the format
> exactly. Reply with JSON only.

On the course page: **+** on the unit, name the lesson, **▸**, paste the
reply. It saves itself. If it does not validate, paste the error back to the
model. An LLM that can fetch pages can start from [`/llms.txt`](/llms.txt)
instead.

Scripts can send up to 100 files at once while signed in:
`POST /api/courses/<id>/files` with `{"files": [{"path": "lessons/coffee.json",
"content": "…"}]}`. The reply lists what was saved and the course's
validation error, if any.

## Generate one with AI

The **Generate the course** dialog opens on a new course, and stays on offer
(under *Use AI*) while you have only changed names (the title, lesson titles
and lessons added by name). The AI keeps those names and plans the named lessons
first. It needs an API key (Claude, OpenAI or Gemini) on the server, which
the owner saves on *Manage Users*; without one the dialog is greyed out and
says so.

1. Say what you want to learn in a sentence or two.
2. Optionally add your material: notes, homework, a syllabus (pasted, or text
   files dropped in; up to 60 000 characters). The course teaches exactly that.
3. Pick the kind of course (or let the AI decide).
4. Pick how much to write: the **Full course** (the AI decides how many
   lessons, up to 15), a **Preview** (one unit with one full lesson), or an
   **Outline** (every unit and lesson, each a bulleted list of what it will
   teach plus one example question). The (i) beside the choice explains them.

A progress modal shows the phase and a progress bar, with *Cancel*. On the
course list the course is greyed out as *generating*, with a *Watch* link.
The model writes an outline, then one lesson at a time; the app validates each one and, if it is wrong, sends it back with the error, up to twice. When it is ready
the modal goes and the course's lessons are there.

- A run that stops (error, cancel, restart) keeps what was written. Its card
  on the course list has a red ! that opens why. **Resume** writes only the missing lessons
  (or **Retry** when not even the outline was saved).
- To start a generated course over, delete it and create a new one.

## Change a course with AI

**Use AI**, under the lesson table, uses the server's first API key (it
names the provider only when there are two or more). Pick what the
AI may do with *Add*, *Edit* or *Add and edit*; the (i) beside them says what
each does. The switch *Outline only* makes the lessons it writes outlines
instead of full lessons (not with *Edit*, which always writes full lessons).

- *Add* (the default): one new lesson, written and placed by the AI. Nothing
  else in the course may change, or nothing is saved.
- *Edit*: rewrites the lessons already there ("fix the typo in lesson 2",
  "simpler words"). No lesson is added, removed or moved, or nothing is saved.
- *Add and edit* (any finished course of yours): say what should change
  ("more word problems", "drop the history lesson"). It may also add, reorder
  or remove lessons. The course keeps working as it was until the whole new
  version validates.

To have the facts checked, say so: "Check the facts in every lesson and fix
what is wrong". The AI reads every lesson and changes only what is wrong.
For each fix it says what it changed and why, and it lists what it was
unsure of and left alone as *Check:* lines.

The (i) beside the box, *What can the AI do?*, lists example requests. Click one to add it to the box, then change it to fit your
course. Most need *Allow changes to existing content* on.

- Check: "Check the facts in every lesson and fix what is wrong."
- Add: "Add a lesson on ...", "Add three multiple-choice questions to lesson
  4.", "Add a Sprint sort exercise to every lesson that has none." A language
  course also gets "Add listening sentences to every lesson that has
  vocabulary but no listening." and "Add a conversation to lesson 2."
- Change: "Use simpler words in the explanations."
- Remove: "Drop the history lesson."

For the course in another language, use *Copy and translate to…* in the
course settings instead; it makes a translated copy.

When the job ends, **Review AI changes** opens: every file the AI added,
changed or removed, with a side-by-side diff. A fact check's reasons sit
above each fixed lesson's diff, and lessons it left alone with *Check:* lines
are under *Looked at, left alone*. *Review AI changes* under Use AI
opens it again later. **Undo** under the table brings the old version back.

While the server's owner hides the AI features (*Manage Users → AI features*), none of this is shown.

## Share a course

**Share** on the course page: type e-mail addresses, *Send invitations*. Each
person gets a link; the course then sits on their list (under *Shared with
you* until opened). They learn from it with their own progress; only you can
change it, and your edits reach them at once. The dialog lists every address
with *invited* or *opened*; *Remove* stops sharing with one.

## Fixing validation errors

Every error names the file and the field, for example:

```
lessons/coffee.json: unit 'B' is not in the manifest (units: A1, A2)
```

- On the lesson table, the save is refused and the red ! by the title says
  why; fix the JSON in the row (▸) and it is saved again.
- A course that does not load shows its files instead of the table, with the
  error on top: open the file named, fix it, *Save and validate*.
- Common causes: a `unit` not listed in `course.json`, a `question_id` used
  twice in the course, a multiple-choice `correct_answer` that is not one of its
  `options`, a `slug` with capitals or spaces.

The rules behind each error are in the [course format](course-format).
