# Course format

A course is a folder of JSON files. The app reads them at start-up (built-in
courses under `data/courses/<id>/`) or from the database (courses built in the
app). Both use exactly this format. This page is the complete reference: an
LLM given this page can write a valid course.
See also: [JSON Schema](course.schema.json), and a walkthrough for people,
[How a course is built](course-structure).

```
<course>/
  course.json              the manifest (required)
  placement.json           optional placement test (a course with "levels")
  lessons/**/*.json        one lesson per file; folders are free-form
  pictures/**/*.svg        optional SVG pictures a lesson's text can show
```

Validation runs on every load. A mistake stops a built-in course from
starting (the message is in the log) and shows on the editor page of a
course built in the app. Every error names the file and the field.

## The hierarchy

```
course
└── units            "A1", "Chapter 1", "Module: Scales"   (manifest.units, ordered)
    └── groups       "Grammar", "Vocabulary", "Basics"      (manifest.groups, free-form ids)
        └── lessons  one JSON file each; the unit of learning
```

A lesson names its unit and group by id. Units are ordered as listed in the
manifest; lessons are ordered inside a unit by `order`, then title. Mastery,
the Map and "current unit" all follow this structure.

## course.json

```jsonc
{
  "id": "spanish",                 // lowercase a-z, 0-9, '-'. Built-in: must equal the folder name
  "title": "Spanish",
  "subject": "Spanish",            // optional, defaults to title
  "description": "One or two sentences for the course card.",
  "icon": "🇪🇸",                   // optional emoji
  "terms":    { "label": "Español", "lang": "es" },   // what the vocabulary terms are
  "meanings": { "label": "English", "lang": "en" },   // what their meanings are written in
  "units": [                       // required, at least one, in learning order
    { "id": "A1", "title": "A1", "description": "Beginner" },
    { "id": "A2", "title": "A2", "description": "Elementary" }
  ],
  "order": 1,                      // optional, built-in courses only: lowest comes first among the ready-made courses (see below)
  "levels": true,                  // optional: the units are levels a learner may start at (see "Starting level")
  "language_features": true,       // optional: the language features on or off (see below); unset = on when terms.lang differs from meanings.lang
  "music": { "note_names": "letters" },   // optional, a music course: how notes are named on screen (see "Music questions")
  "groups": [                      // optional; lessons may use group ids not listed here (shown as-is)
    { "id": "Grammar", "title": "Grammar" },
    { "id": "Vocabulary", "title": "Vocabulary" }
  ],
  "paradigm": { ... },             // optional, see "Paradigm tables"
  "chars": ["á", "é", "í", "ó", "ú", "ñ", "ü", "¿", "¡"],   // optional tap-to-insert characters for text answers
  "calendar": {                    // optional, see "Calendar"
    "days": ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
    "months": ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
    "format": "{day}, {d} de {month} de {year}"
  },
  "strings": {                     // optional
    "praise": ["¡Correcto!", "¡Muy bien!"],   // shown on a right answer (one at random)
    "almost": ["Not quite", "Casi…"]          // shown on a wrong answer
  }
}
```

`order` means something only for a built-in course (a folder under
`data/courses/`): it places the course among the ready-made courses on
**Courses** and in the landing page's list, lowest first. Built-in courses
without it come after those with one, in folder-name order. *How to Use
Skrongle* has `0`, so it comes first. A whole number only; any other value
is ignored. In a course made in the app the field is kept but does nothing.

`levels` says the units are levels of the subject (CEFR A1, A2…;
Beginner, Intermediate…) rather than chapters that build on each other.
With it, a learner who opens the course for the first time picks the level
to start at (they can change it later on the lessons page): the
lessons of the levels below are *skipped*, shown as open but never
recommended, practised, reviewed or counted as passed. Leave it out (or
`false`) when every unit needs the ones before it; a course needs at least
two units with lessons for the picker to appear. The editor's gear
(**Levels**) writes this field; with it on, the gear's **Placement test**
switch adds or takes out [placement.json](#placementjson-placement-test).

`terms` / `meanings` label the two sides of every vocabulary entry and
flashcard. For a language course the terms are the language being learnt and
the meanings the learner's language. For anything else (music theory, a
programming language, anatomy) the terms are the terminology and the meanings
their definitions; set `lang` to `""` or the page language. `lang` is a BCP 47
tag used for `lang=` attributes and spell-checking.

`language_features` switches the language features on or off: Listening,
Sprint mode's listening exercises and sentence builds, tap to hear, Numbers
by ear, and in the lesson editor the translation fields (a word's example
meaning, a conversation line's meaning) and the sections only a language
course fills. Leave it out and they are on exactly when `terms.lang` differs
from `meanings.lang`. Set it to `false` for a course that teaches terms in
the learner's own language and should not ask for translations; set it to
`true` for a course whose terms carry no `lang` but wants them anyway. The
editor's gear (**Language features**) writes this field. A lesson's
`listening`, `conversations` and `builds` stay valid either way; they are
simply not offered while the features are off, except builds in a music
course (one with `music`), which keep them.

### Paradigm tables

Optional. A paradigm is a table shape shared by the whole course: for Spanish
it is the six persons (rows, "slots") × tenses (columns, "sets"). Lessons can
then carry `conjugations`, which power the Conjugation Drill game and the
"ladder" exercise in Sprint mode. Without a paradigm those features are hidden.

```jsonc
"paradigm": {
  "label": "Conjugations",         // heading on lesson pages and in Games
  "item_label": "Verb",            // what one table describes
  "slots": [                       // 2 to 12 rows
    { "key": "yo", "label": "yo", "hint": "I",
      "art": ["   |   ", "   |   ", " \\ | / ", "  \\|/  ", "   v   "] },   // optional ASCII picture, ≤ 8 lines × 24 chars
    { "key": "tu", "label": "tú", "hint": "you" }
  ],
  "sets": [                        // 1 or more columns
    { "key": "present", "label": "Present" },
    { "key": "preterite", "label": "Preterite" }
  ]
}
```

Slot labels may list alternatives separated by ` / ` (e.g. `"él / ella / usted"`);
the drill accepts an answer typed with any of them in front ("ella habla").

### Calendar

Optional. The names of the days of the week and the months in the course's
language, for the Date Dash game (days and months against the clock). Without
it the game is hidden. `days` lists exactly seven names, **Monday first**;
`months` exactly twelve, January first; both in the usual case of the
language (lower-case for Spanish). `format` says how a full date is written,
with `{day}`, `{d}` (the day of the month), `{month}` and `{year}`
placeholders; `{day}` and `{month}` are required, the default is
`"{day}, {d} {month} {year}"`. The learner's own names for the days and
months come from the browser, in `meanings.lang`, so they are not listed.

```jsonc
"calendar": {
  "days": ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
  "months": ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
  "format": "{day}, {d} de {month} de {year}"      // -> "jueves, 9 de octubre de 2026"
}
```


## placement.json (placement test)

Optional, one per course, for a course with `"levels": true` (with fewer
than two units that have lessons there is nothing to place, and the test
is not offered). A learner who opens the course for the first time can take
it at the end of the start wizard instead of picking a level. Every question
is multiple choice and about one unit:

```jsonc
{
  "random_per_unit": 3,            // optional, 0 to 10: questions drawn at random per unit (below)
  "questions": [                   // optional: written questions
    {
      "unit": "A1",                // a unit id from course.json
      "prompt": "¿Cómo te llamas? means…",
      "options": ["What is your name?", "How are you?", "Where do you live?"],   // at least 2, none twice
      "answer": "What is your name?"   // one of the options
    }
  ]
}
```

A unit with written questions is tested with those. Every other unit draws
`random_per_unit` of its own lessons' `multiple-choice` (and `true-false`)
quiz questions at random each time the test is taken. A unit with neither is
not tested. The file needs at least one written question or a
`random_per_unit` above 0. The options are shown shuffled.

Marking: a unit answered all right can be skipped; the **first** unit with
a wrong answer is where the test suggests starting. With every answer right,
it suggests the unit after the last one tested (or the last unit). The
suggestion only picks a level in the wizard: nothing is marked passed, as
with any [starting level](features#starting-level).

## Lesson files

```jsonc
{
  "id": "a1-ser-vs-estar",          // unique in the course; referenced by prerequisites
  "slug": "ser-vs-estar",           // URL: /lessons/ser-vs-estar  (lowercase a-z, 0-9, '-')
  "title": "Ser vs. Estar",
  "unit": "A1",                     // a unit id from the manifest (required)
  "group": "Grammar",               // a group id; defaults to "general"
  "order": 20,                      // sort key inside the unit, lower first (default 100)
  "summary": "One line for lists.",
  "focus": "Choosing between ser and estar",   // the one thing the lesson teaches
  "prerequisites": ["a1-regular-ar-present"],  // lesson ids, optional
  "content": "## Heading\n\nMarkdown…",        // required; see "Content"
  "vocabulary": [ ... ],            // see below
  "quiz": [ ... ],                  // see below
  "conjugations": [ ... ],          // needs manifest.paradigm
  "conversations": [ ... ],         // dialogues; Sprint mode builds sentences from their lines
  "listening": [ ... ],             // hear it, type it (language courses)
  "guide": [ ... ],                 // optional hand-ordered Guide steps
  "sprint": { ... }                 // optional phone-session extras
}
```

### Content

`content` is Markdown. Tables, lists, bold, code blocks and blockquotes are
supported. Raw HTML is not: it is shown as text. Links may use relative,
`http(s)://` or `mailto:` URLs only; any other scheme points nowhere. Images
load only from a relative URL or a `data:image/…` URL; an image from another
host is shown as a plain link. Every `## ` heading starts a new *section*; text
before the first heading becomes an "Introduction" section. Sections are the cards of the Guide
and the "bites" of Sprint mode when none are written by hand, so keep each one
short and about one thing. Three to five sections is a good lesson.

#### Music notation

A fenced code block marked `abc` is music notation, written in
[ABC notation](https://abcnotation.com/wiki/abc:standard:v2.1) (plain text,
a few header lines and then the notes). The lesson page and the Guide draw it as
a staff where the block stands; other modes show the ABC text as a code block.

````markdown
```abc
X:1
T:C major scale
M:4/4
L:1/4
K:C
C D E F | G A B c |]
```
````

Start each block with `X:` and end the header with `K:` (the key). One tune
per block; keep it short enough to read on a phone (two to four bars). Put
what the learner should notice in the text around it, not in the ABC.
A block abcjs can't read, or one with no notes, is shown as its ABC text.
A lesson without an `abc` block looks as it always did.

### Vocabulary

The terminology of the lesson. Entries are collected by the learner when they
answer a question that teaches them, then studied as flashcards, played in
Word Match and in Sprint mode.

```jsonc
"vocabulary": [
  { "term": "hola", "meaning": "hello",
    "example": "¡Hola! ¿Qué tal?", "example_meaning": "Hello! How are you?" }   // example optional; give it a meaning
]
```

Terms must be unique within a lesson. An `example` should come with its
`example_meaning` in the meaning language: it is shown as a hint when the
learner hovers the example (on the lesson page, the flashcards and the
Vocabulary page) and after a Sprint mode vocabulary exercise.

### Quiz

```jsonc
"quiz": [
  {
    "question_id": "a1-ser-vs-estar-01",   // unique across the whole course: prefix with the lesson id
    "prompt": "Mi madre ___ médica.",
    "type": "multiple-choice",             // or "fill-in-blank", "true-false", "numeric", "expression", "fill-bar", "chord-spell", "staff-note", "algebra-tiles", "clock", "measure"
    "options": ["es", "está"],             // multiple-choice: 2+ options containing correct_answer; shuffled per quiz
    "correct_answer": "es",
    "accepted_answers": ["ella es"],       // fill-in-blank, numeric, expression, clock: other answers that count as right
    "explanation": "Occupation takes ser.",
    "hints": ["Is a profession an identity or a state?"],
    "teaches": ["ser"]                     // vocabulary terms (of this lesson) collected on a correct answer
  }
]
```

Six question types, chosen to fit the subject:

| `type` | The learner… | `correct_answer` | Checked |
|---|---|---|---|
| `multiple-choice` | taps one of `options` | one of the options | exactly |
| `true-false` | taps True or False | `"true"` or `"false"` | exactly (served as a two-option choice; no `options` needed) |
| `fill-in-blank` | types a word or phrase (Sprint mode offers word tiles) | the text | ignoring case, surrounding punctuation and extra spaces; a missing accent is a near miss |
| `numeric` | types a number (Sprint mode offers number tiles) | a number | by value, see below |
| `expression` | types a formula | an algebraic expression | by equivalence, see below |
| `fill-bar` | taps note-value `tiles` into the gap of a bar of music | optional: tiles that fill the gap | by length, see "Music questions" below |
| `chord-spell` | taps note `tiles` into slots to spell a chord | the chord's notes in ABC, separated by spaces | as a set, spelling counts; see "Music questions" below |
| `staff-note` | taps a staff to place a note | one note in ABC with its octave | by pitch and octave; see "Music questions" below |
| `keyboard` | taps a key of a piano keyboard | one ABC note | by pitch, see "Music notation" below |
| `algebra-tiles` | lays out `tiles` as a rectangle, then types its two sides | the sides multiplied, `(x + 2)(x + 3)` | by equivalence, and it must be two sides, see below |
| `clock` | drags the minute hand of a clock face; with `options`, reads a clock and taps its time | a time, `H:MM` | as the same time on the clock face, see below |
| `measure` | measures a drawn angle with a protractor and types the number (Study and the Guide only) | a number | like `numeric`, see below |

Fill-in-blank answers are checked case-insensitively, ignoring surrounding
punctuation and extra spaces. A vocabulary term equal to an accepted answer is
collected automatically; `teaches` lists any others. Write `___` in the prompt
where the answer goes: Sprint mode shows it as a slot to fill from word tiles.
A fill-in-blank has one answer and so one `___`. A sentence with two gaps is a
multiple-choice question whose options fill both (`"más … que"`).

**Maths answers.** `numeric`, `expression` and `algebra-tiles` questions are graded by value,
so the learner can write the answer any reasonable way:

```jsonc
{ "question_id": "q-frac-01", "prompt": "Write 3/4 as a decimal.", "type": "numeric", "correct_answer": "0.75" },
{ "question_id": "q-circle-01", "prompt": "Area of a circle of radius 2, to 2 decimals?", "type": "numeric",
  "correct_answer": "12.57", "tolerance": 0.01, "unit": "cm²" },
{ "question_id": "q-roots-01", "prompt": "Solve x² + x − 6 = 0.", "type": "numeric", "correct_answer": "2; -3",
  "explanation": "(x + 3)(x − 2) = 0, so x = −3 or x = 2." },
{ "question_id": "q-expand-01", "prompt": "Expand (x + 1)².", "type": "expression", "correct_answer": "x^2 + 2x + 1" }
```

- `numeric`: `3/4`, `0.75`, `1 1/2`, `1,250`, `2.5e3`, `40%`, `2^10`,
  `sqrt(2)` and `x = 4` are all read as numbers. Without `tolerance` the
  value must be exact (up to floating-point noise); with it, anything within
  that margin counts (`0.01` for "to two decimals"). `unit` is shown after the
  answer box and may be typed or not. Values separated by `;` make a list
  answered in any order (`2; -3` accepts `-3, 2`, `x = 2 or x = -3`). A wrong
  answer within 1 % gets a "check your rounding" note, the right number with
  the wrong sign a "check the sign" note.
- `expression`: an answer is right when it is equivalent to
  `correct_answer`: `2(x+1)` for `2x + 2`, `(x-1)(x+1)` for `x^2 - 1`. Write
  it with `+ - * / ^`, brackets, implicit multiplication (`2x`, `3(x+1)`,
  `xy`), `sqrt abs exp ln log sin cos tan asin acos atan`, `pi` and `e`
  (so `e` is never a variable); variables are single letters. A leading
  `y =` is ignored. The look-alikes from phones and copy-paste (`−`, `×`,
  `÷`, `²`, `³`, `√`, `π`) work too. If the question wants a particular form
  ("factor completely"), ask for it with multiple-choice instead: equivalent
  forms are all accepted.
- `algebra-tiles`: the learner gets a pile of tiles, big squares for x²,
  strips for x and small squares for 1, and drags them into a rectangle
  (with the keyboard: Tab to a tile, arrow keys move it, R turns a strip,
  Enter checks). Then they type the rectangle's two sides. `tiles` is the
  pile, written `a x^2 + b x + c` with whole numbers, a from 1 to 3, b up to
  12 and c up to 20 (no negative tiles yet); any letter can be the variable.
  `form` is `"product"`, the one form there is, and may be left out. An
  answer is right when it is two sides multiplied (`(x + 3)(x + 2)`,
  `x(x + 5)`, `(x + 2)^2`) and they multiply out to the tiles; the expanded
  `x^2 + 5x + 6` is refused with a note to write the two sides. The layout
  itself is not graded. `correct_answer` must be such a product. Sprint mode
  and the tutor skip these questions; Study and the Guide show the board.

  ```jsonc
  { "question_id": "q-tiles-01", "type": "algebra-tiles", "prompt": "Make a rectangle from x² + 5x + 6. What are its sides?",
    "tiles": "x^2 + 5x + 6", "form": "product", "correct_answer": "(x + 2)(x + 3)" }
  ```
- `measure`: the page draws an angle of `angle` degrees (more than 0, less
  than 180), turned a random way, and a see-through protractor (`tool`:
  `"protractor"`, the only tool so far). The learner drags the protractor,
  turns it by its handle or with two fingers, reads the angle and types it.
  Keys: arrows move it, Shift with ← or → turns it, Enter goes to the
  answer box. It is graded like `numeric`, so give `tolerance` (a degree or
  two) and `unit` (`°`). An answer of 180 minus the angle, read off the other
  scale, gets a note saying so. Sprint mode and the AI tutor can't draw it
  and skip it; it doesn't count towards the answers a run needs.

  ```jsonc
  { "question_id": "q-angle-01", "type": "measure", "tool": "protractor", "prompt": "How big is this angle?",
    "angle": 125, "correct_answer": "125", "tolerance": 2, "unit": "°" }
  ```
- `numeric`, `expression`, `algebra-tiles`: an answer (expected or typed) is at most 200 characters and 64
  levels deep (brackets, signs, powers, function calls, terms in a row);
  longer or deeper ones are rejected as unreadable.

**Set the clock.** A `clock` question shows a clock face at 12:00. The
learner drags the minute hand and the hour hand follows it round, so a
quarter to four has the hour hand nearly on the 4 (the hour hand can be
dragged too; arrow keys move one step, Page Up and Page Down an hour):

```jsonc
{ "question_id": "q-time-01", "prompt": "Show a quarter to four.", "type": "clock",
  "step_minutes": 5, "correct_answer": "3:45" },
{ "question_id": "q-time-02", "prompt": "Son las seis y media.", "type": "clock",
  "step_minutes": 15, "correct_answer": "6:30" },
{ "question_id": "q-time-03", "prompt": "What time is it?", "type": "clock",
  "options": ["9:15", "3:45", "2:45", "4:45"], "correct_answer": "9:15" }
```

- `correct_answer` (and any `accepted_answers`) is `H:MM` with an hour
  from 1 to 12: `3:45`, `12:00`. It is graded as the same time on the face.
- `step_minutes` (default 5) is how far one step of the minute hand goes;
  it divides 60 (1, 5, 15, 30…), and the answer must fall on a step.
- With `options` the question is turned around: the clock shows
  `correct_answer` and the learner picks it among `options` (2 or more,
  each `H:MM`). `step_minutes` does not apply.
- The prompt may be in the course's language ("Son las cuatro menos
  cuarto"); the clock itself has no words, only the numbers 1 to 12.

Write maths in prompts, content and explanations as plain text or Unicode
(`x²`, `√2`, `π`, `×`, `≤`), not LaTeX. A maths course should set both
`terms.lang` and `meanings.lang` to the learner's language (or leave them
empty) so the language games stay off.

**Music questions.** A question's `abc` is music in ABC notation (see
[Music notation](#music-notation)), drawn as a staff above its prompt in
Practice, the reading check, Sprint mode, the Guide and the editor's preview.
Any question type may carry it. "Name the note" is a multiple-choice question
with one note on the staff:

```jsonc
{ "question_id": "m1-treble-03", "type": "multiple-choice", "prompt": "Which note is this?",
  "abc": "X:1\nL:1/4\nK:C\nA",
  "options": ["F", "G", "A", "B"], "correct_answer": "A",
  "explanation": "The second space from the bottom. Count up from G on the second line: the next space is A." }
```

Write notes in the options as ABC pitches: `C` is middle C, `c` the octave
above, `^F` F sharp, `_B` B flat, `=B` B natural. In a multiple-choice
question with `abc`, such an option is shown in the course's note names,
`music.note_names` in `course.json`: `letters` (C D E, F♯, B♭; the default),
`solfege` (Do Re Mi, Fa♯, Si♭) or `german` (C D E, Fis, H for B, B for B
flat). The octave is not part of a name, so two options for the same note in
different octaves are an error. Options that aren't one note (`"minor 3rd"`,
`"G major"`) are shown as written. A wrong answer shows the `explanation`, so
say there how to find the note from a line the learner knows. A lesson
without `abc` loads no notation code. A music course's Sprint `builds` put
notes or keys in order (see [Sprint mode](#sprint-mode-optional)).

A question's `play` is music to listen to, also in ABC: a speaker button
plays it with the browser's own tones (no recordings, nothing downloaded)
as often as the learner likes, and only after a tap. Two single notes play
one after the other, then together. With `play` and no `abc` the learner
only hears it, and once the question is answered the notes appear on a
staff. "Hear the interval":

```jsonc
{ "question_id": "m2-ear-05", "type": "multiple-choice", "prompt": "Which interval did you hear?",
  "play": "X:1\nL:1/2\nK:C\nC G",
  "options": ["perfect 4th", "perfect 5th", "major 6th"], "correct_answer": "perfect 5th" }
```

`option_kind: "abc"` on a multiple-choice question with `play` or `abc`
draws each option as a staff: write just the notes, and the header lines
(`X:`, `L:`, `M:`, `K:`) of `play` (else `abc`) go in front. "Which one did
you hear?":

```jsonc
{ "question_id": "m2-ear-09", "type": "multiple-choice", "prompt": "Which melody plays?",
  "play": "X:1\nL:1/4\nK:C\nCDEC", "option_kind": "abc",
  "options": ["CDEC", "CEDC", "CDCE"], "correct_answer": "CDEC" }
```

With `clef=perc` in the `K:` line, the tune is a rhythm. Every note plays
as the same short click, so only the timing counts, and the staves show the
percussion clef and the metre. "Which bar did you hear?":

```jsonc
{ "question_id": "m2-rhythm-02", "type": "multiple-choice", "prompt": "Which bar did you hear?",
  "play": "X:1\nM:4/4\nL:1/8\nK:C clef=perc\nB2 BB B2 B2|", "option_kind": "abc",
  "options": ["B2 BB B2 B2|", "BB B2 B2 B2|", "B2 B2 BB B2|"], "correct_answer": "B2 BB B2 B2|" }
```

The speaker follows the site's audio switch (Sprint mode's speaker button).
"Can't listen now" turns sound off for an hour: Sprint mode then skips
questions that need it (`play` without `abc`, sorts of speaker tiles), and
the other modes show the notes on a staff instead, except when the options
are the staves to tell apart. A lesson without `play` loads no sound code.

"Fill the bar" is a `fill-bar` question. Its `abc` is a bar with one
invisible rest (`x2`) marking the gap, and `tiles` are note values in ABC,
one group per tile without spaces. The learner taps tiles into the gap (a
tile may be used again, Undo takes the last one out) until it is full, then
Check; the bar is drawn again after each tap. Any tiles that add up to the gap
are right, so `correct_answer` may be left out (the app then shows the fewest
tiles that fill it after a wrong answer). With `"exact": true` only
`correct_answer` and `accepted_answers` count, written as tiles separated by
spaces. A wrong length says how many beats went in.

```jsonc
{ "question_id": "m2-rhythm-01", "type": "fill-bar", "prompt": "This bar is two beats short. Fill it.",
  "abc": "X:1\nM:4/4\nL:1/4\nK:C\nB B x2|",
  "tiles": ["B2", "BB", "B/B/B", "B4"] },
{ "question_id": "m2-rhythm-02", "type": "fill-bar", "prompt": "Fill the bar with one half note.",
  "abc": "X:1\nM:4/4\nL:1/4\nK:C\nc c x2|",
  "tiles": ["cc", "c2", "c4"], "exact": true, "correct_answer": "c2" }
```

Lengths are read in `L:` units (`B2`, `B/`, `B3/2`, rests `z`, chords
`[CEG]2`, triplets `(3BBB`); ties, broken rhythm, grace notes, chord symbols
and decorations add nothing. A beat is the bottom number of `M:` (6/8 counts
eighths). The loader refuses a question whose `abc` has no gap or more than
one, a tile that isn't notes, fewer than 2 tiles, or tiles that can't fill the
gap. Fill the bar is asked in Practice, the reading check, the Guide and
Sprint mode; the AI tutor skips it.

"Pick the interval" is the same kind of question with two notes, one after
the other (`C E`) or stacked (`[CE]`), and interval names as options. Its
explanation counts the half steps:

```jsonc
{ "question_id": "m2-intervals-01", "type": "multiple-choice", "prompt": "What is the interval?",
  "abc": "X:1\nL:1/2\nK:C\nC E",
  "options": ["minor 3rd", "major 3rd", "perfect 4th"], "correct_answer": "major 3rd",
  "explanation": "C to E is four half steps." }
```

"Spell the chord" is a `chord-spell` question. `correct_answer` is the
chord's notes in ABC, three or four of them, separated by spaces, and `tiles`
are the notes to pick from, as one string separated by spaces or as a list.
The learner sees one empty slot per note of the chord and taps tiles into
them in any order (a tap on a filled slot takes the note out again); a staff
above draws the notes in the slots as one chord. Tiles are shown in the
course's note names. The answer is right when it spells the same notes in
any order; the octave doesn't count. The same sounds spelled otherwise
(`D _G A` for D major) are wrong, with a "right sound, wrong spelling" note.
`accepted_answers` may give other spellings that count.

```jsonc
{ "question_id": "m3-triads-01", "type": "chord-spell", "prompt": "Spell D major.",
  "correct_answer": "D ^F A",
  "tiles": "D E F ^F _G A B" }
```

The loader refuses a tile that isn't one note, two tiles that spell the same
note, an answer that isn't 3 or 4 different notes, an answer note that isn't
a tile, no tile beyond the chord's notes, and `abc` on the question (it draws
its own staff). Spell the chord is asked in Practice, the reading check, the
Guide and Sprint mode; the AI tutor skips it.

"Place the note" is a `staff-note` question. The learner sees an empty staff
with a `clef`, `treble` or `bass`, and taps it. The note snaps to the nearest
line or space, and they can drag it, or add a flat or a sharp, before Check.
`correct_answer` is one note in ABC with its octave, so `G` is the G on the
treble staff's second line, `c` the C above middle C and `C,` the C below it.
The answer is right when it is the same note in the same octave. With
`"any_octave": true` the note counts in any octave. The right note in the
wrong octave is marked wrong with a "wrong octave" note. Spelling counts:
`_G` is not `^F`. `accepted_answers` may give other notes that count.

```jsonc
{ "question_id": "m1-place-01", "type": "staff-note", "prompt": "Tap where G goes.",
  "clef": "treble", "correct_answer": "G" }
```

The staff reaches two ledger lines below and above, and the space past
each: on a treble staff from `G,` to `d'`, on a bass staff from `B,,,` to
`F`. The loader refuses a missing or unknown `clef`, an answer that isn't
one note (or has a double sharp or flat), an answer off the staff, and `abc`
on the question (it draws its own staff). Place the note is asked in
Practice, the reading check, the Guide and Sprint mode; the AI tutor skips
it.

"Name the chord" stacks three or four notes (`[DFA]`) and has chord names as
options. Chord names are shown as written, not in `music.note_names`, so
write them the way the course names notes. Its explanation names the notes:

```jsonc
{ "question_id": "m3-chords-01", "type": "multiple-choice", "prompt": "Name this chord.",
  "abc": "X:1\nL:1\nK:C\n[DFA]",
  "options": ["D minor", "D major", "F major"], "correct_answer": "D minor",
  "explanation": "D, F and A: a minor 3rd and a perfect 5th above D." }
```

A `keyboard` question ("Find it on the keys") shows a piano keyboard of one
octave from middle C, or two with `"octaves": 2`. Each key sounds when
tapped, so the learner can hunt by ear, and the last key tapped before Check
is the answer. `correct_answer` (and any `accepted_answers`) is one ABC
note: `C` is middle C, `^F` F sharp, `_B` B flat, `c` the C above middle C.
Any key of that note counts (any C for `C`); with `"octave": true` only that
key does, and it must be on the keyboard. Every spelling of a pitch is the
same key, so `_B` is right for the black key that is also A sharp. The
answer is shown in the course's note names. `play` and `abc` are optional:
an ear question plays a note, a reading question draws one.

```jsonc
{ "question_id": "m1-keys-01", "type": "keyboard", "prompt": "Find the note you hear.",
  "play": "X:1\nL:1/2\nK:C\nE", "correct_answer": "E" }
{ "question_id": "m1-keys-02", "type": "keyboard", "prompt": "Find this C on the keys.",
  "abc": "X:1\nL:1/2\nK:C\nc", "correct_answer": "c", "octave": true, "octaves": 2 }
```

Find it on the keys is asked in Practice, the reading check, the Guide and
Sprint mode (skipped there while sound is off when it has `play` and no
`abc`); the AI tutor skips it.

"Name the key" shows a key signature and has key names as options. Put one
note after the signature (a note that is in every key on offer, so it gives
nothing away), since a staff with no note on it is kept as ABC text (#1187). For the
harder kind, write a short tune instead: the signature gives a major key and
its relative minor, and the note the tune ends on decides between them. The
explanation counts the sharps or flats, or names that last note:

```jsonc
{ "question_id": "m4-keys-01", "type": "multiple-choice", "prompt": "Which major key has this signature?",
  "abc": "X:1\nL:1\nK:D\nB",
  "options": ["G major", "D major", "A major"], "correct_answer": "D major",
  "explanation": "Two sharps, F♯ and C♯, mean D major." }
```

Each quiz question feeds the lesson's mastery score (right
answers build it up with diminishing returns, wrong ones take 35 % off, and it
decays slowly after three days without practice).

### Conjugations (paradigm tables)

Only when the manifest has a `paradigm`. `forms` must have a value for every
slot key. An `item` + `set` pair may be taught by one lesson only. Each
distinct item needs an `art` in one lesson (other lessons teaching the same
item in another set inherit it); it is optional otherwise.

```jsonc
"conjugations": [
  { "item": "hablar", "meaning": "to speak", "set": "present",
    "art": [" .--------. ", "( ¡hola!   )", " `---. ,--' ", "      V     "],
    "forms": { "yo": "hablo", "tu": "hablas", "el": "habla",
               "nosotros": "hablamos", "vosotros": "habláis", "ellos": "hablan" } }
]
```

### Conversations

A scripted two-person exchange. In Sprint mode its lines of three to nine words
become sentence builds. `meaning` is the line in the learner's language; it
may be omitted for courses where that makes no sense (such a line is not
built).

```jsonc
"conversations": [{
  "id": "a1-greetings-conv-first-meeting",   // unique across the course
  "title": "First meeting",
  "scenario": "Ana and Carlos bump into each other on the first day of a course.",
  "role_a": "Ana", "role_b": "Carlos",
  "lines": [                                 // at least 2, both roles must speak
    { "speaker": "a", "text": "¡Buenos días!", "meaning": "Good morning!" },
    { "speaker": "b", "text": "Buenos días. ¿Qué tal?", "meaning": "Good morning. How's it going?",
      "note": "optional pointer shown under the line" }
  ]
}]
```

### Listening (language courses)

Sentences for listening practice: the browser reads one aloud in the
course's language (its own speech synthesis, in `terms.lang`), the learner
types what they heard and sees it checked word by word. Sprint mode draws its
listening exercises from the same sentences (rebuilt from tiles, with a gap
to fill, or heard and matched to its meaning). Only language courses
(`language_features`, or a `terms.lang` that differs from `meanings.lang`)
offer them; a lesson without `listening` simply does not appear there.

```jsonc
"listening": [
  { "text": "Mi madre es médica.", "meaning": "My mother is a doctor.", "note": "Occupation takes ser." },
  { "text": "Los niños están en la oficina.", "meaning": "The children are at the office.",
    "note": "New word: la oficina = the office." }
]
```

`text` is the sentence exactly as it should be typed (1 to 30 words; checked
case-insensitively, ignoring punctuation, with accent slips flagged rather
than counted wrong). `meaning` is its translation, shown after answering.
`note` is optional: a one-line pointer at the grammar, or the new word the
sentence slips in, written `New word: <word> = <meaning>.` so the app can
highlight it. Eight sentences per lesson, short to long, is a good set; most
words should come from the lesson's own vocabulary, with one new word in two
or three of them to stretch the learner. Write numbers as words, since the
learner types what they hear. Texts must be distinct within a lesson.

### Guide (optional)

The Guide plays a lesson as a sequence of small steps: concept cards, quick
checks, flashcard hands, tips. Without a `guide` array the steps are derived:
every section becomes a card, vocabulary is dealt in hands of five after the
first card, and questions are interleaved from the third card on. To order the
steps by hand, reference the lesson's own content:

```jsonc
"guide": [
  { "section": "Use ser" },                  // a section, matched by heading prefix (case-insensitive)
  { "question": "a1-ser-vs-estar-01" },      // a quiz question of this lesson, by id
  { "vocab": 4 },                            // the next four vocabulary entries as flashcards
  { "tip": "Still unsure? Ask: what it IS, or how it is right now?" },
  { "concept": { "title": "…", "markdown": "…" } }   // an extra card that exists only here
]
```

A reference to an unknown section or question is a validation error.

### Sprint mode (optional)

Sprint mode is the phone-first, five-minute session: one bite, then rule games,
quiz questions and vocabulary games, interleaved. Everything below is
optional; a lesson without a `sprint` block still plays (bites come from the
sections, the rest from the quiz, vocabulary, conjugations and conversations).

```jsonc
"sprint": {
  "icon": "🎭",                              // emoji for the lesson's stop on the path; different from every other lesson's
  "bites": [                                 // the idea in one breath; not graded
    { "title": "Ser = what it is", "emoji": "🪪", "rule": "Identity, origin, profession, time: ser.",
      "example": "Soy médica.", "example_meaning": "I am a doctor." }
  ],
  "sorts": [                                 // tap each item into the right bucket; 2–4 buckets, 3–10 items
    { "title": "Ser or estar?", "buckets": ["ser", "estar"],
      "items": [ { "text": "Soy de Madrid.", "bucket": 0 }, { "text": "Estoy cansado.", "bucket": 1 }, { "text": "Es médica.", "bucket": 0 } ],
      "why": "Origin and profession take ser; a temporary state takes estar." }
  ],
  "spots": [                                 // tap the one wrong word
    { "sentence": "Mi madre está médica.", "wrong": "está", "fix": "es", "why": "Profession takes ser." }
  ],
  "builds": [                                // rebuild the text from shuffled word tiles; 2–12 words
    { "text": "Mi madre es médica.", "meaning": "My mother is a doctor.", "distractors": ["está"] }
  ]
}
```

Rules checked by the validator: a spot's `wrong` word must appear exactly once
in its `sentence` (punctuation is ignored); a build's `text` has 2 to 12 words;
every sort item points at an existing bucket.

A sort item with `play` (ABC) and no `text` is a speaker tile: the learner
taps it to hear it, then taps its bucket. "Major or minor?":
`{ "title": "Major or minor?", "buckets": ["major", "minor"], "items": [ { "play": "[CEG]2", "bucket": 0 }, { "play": "[A,CE]2", "bucket": 1 }, { "play": "[GBd]2", "bucket": 0 } ] }`.

**A spot on a staff** ("Fix the wrong note", #1227). In a music course a spot
may hold a tune in ABC (`abc`, see [Music notation](#music-notation)) instead
of a `sentence`. One note in it is wrong: `wrong` is its number, counting the
notes from 1, and `fix` is the right note written as one ABC note (`F`, `^C`,
`_B`). Sprint mode draws the staff; the learner taps the wrong note, then picks
its fix from three tiles (the fix and two notes near it, in the course's note
names), or drags the note up or down to the right line. It counts as right
only when both the note and the fix are right. A `T:` line is drawn as the
staff's caption.

```json
"spots": [
  { "abc": "X:1\nT:C major scale\nL:1/8\nK:C\nCDEG GABc|", "wrong": 4, "fix": "F",
    "why": "A scale climbs one step at a time: after E comes F." }
]
```

The validator counts the notes the way the staff draws them: each note or
chord once; rests, bar lines, chord symbols, decorations and grace notes are
not counted. `wrong` must be a number from 1 to that count, and `fix` must be
one note that differs from the note it replaces. A spot has a `sentence` or
`abc`, not both. A staff spot is played in Sprint mode only.

**Build the scale** (`scales`, #1229) is the ladder with notes, for a music
course. `notes` is the scale's eight notes in ABC (see
[Music notation](#music-notation)), bottom to top, separated by spaces;
`decoys` are notes that are not in it. The first rung comes filled; the
learner taps the other seven from a bank of the scale's notes and the
decoys, shown in the course's note names (`music.note_names`), and each
note is drawn on a staff and played as it lands. It is graded at Check
("6 of 8 notes right"), by note name: the octave doesn't count.

```json
"scales": [ { "title": "D major", "notes": "D E ^F G A B ^c d", "decoys": "F c" } ]
```

The validator checks that `notes` has exactly eight notes and every note
and decoy is one ABC note (`F`, `^C`, `_B`, `c'`), and that no decoy has
the name of a scale note (`d` is a D, so it can't be a decoy in D major).
The lesson page shows each scale on a staff with a speaker.

Which games fit depends on the kind of course. In a language course, spots
catch a wrong word in a sentence of the language and builds rebuild its
sentences. In a music course (`music` in `course.json`), a build puts
notes or keys in order: `"text": "C G D A E B"` with `"meaning": "Each key
adds one sharp."` is the circle of fifths. Write the tiles in the course's
note names; a build's text is shown as written. In any other course, sorts
are the main game, builds are left out, and a spot only makes sense where
swapping one key term makes a
statement false ("Exiles protect the system from pain": wrong `Exiles`, fix
`Managers`), never for grammar, spelling or an everyday word.


## Field aliases

Older Spanish-specific spellings still load, so existing files keep working:
`level` → `unit`, `category` → `group`, `target_grammar_point` → `focus`,
`markdown_content` → `content`, `journey` → `guide`, `es`/`en` → `term`/`meaning`
(vocabulary), `text`/`meaning` (lines and builds), `example_es`/`example_en` →
`example`/`example_meaning` (bites and vocabulary), `verb`/`tense` → `item`/`set`, `dmode` → `sprint`. New files should use
the canonical names shown above.

## Which feature reads what

| Lesson field | Used by |
|---|---|
| `content` (split on `## `) | Lesson viewer, Guide cards, Sprint mode bites (when none are written) |
| `content` ```` ```abc ```` blocks | Music notation drawn as a staff on the lesson page and Guide cards |
| `quiz[].abc` (+ `music.note_names` in `course.json`) | A staff above the question in Practice, the reading check, Sprint mode, the Guide and the editor's preview; note options shown in the course's note names |
| `quiz[].octave`, `quiz[].octaves` (`keyboard`) | Find it on the keys in Practice, the reading check, Sprint mode, the Guide and the editor's preview |
| `quiz[].play`, `quiz[].option_kind` | A speaker button above the question in Practice, the reading check, Sprint mode and the Guide, the played notes on a staff after the answer; options drawn as staves |
| `sprint.sorts[].items[].play` | Speaker tiles in Sprint mode's Sort it |
| `quiz[].tiles`, `quiz[].exact` (`fill-bar`) | Fill the bar in Practice, the reading check, Sprint mode, the Guide and the editor's preview |
| `sprint.spots[].abc` | Sprint mode's spot the mistake on a staff: tap the wrong note, pick its fix |
| `quiz[].tiles` (`chord-spell`) | Spell the chord in Practice, the reading check, Sprint mode, the Guide and the editor's preview |
| `quiz[].clef`, `quiz[].any_octave` (`staff-note`) | Place the note in Practice, the reading check, Sprint mode, the Guide and the editor's preview |
| `sprint.scales` | Sprint mode's Build the scale ladder; the scale on a staff with a speaker on the lesson page |
| `quiz` | Practice quizzes, lesson quiz, Guide quick checks, Sprint mode, mastery scores |
| `quiz[].teaches` + `vocabulary` | Vocabulary collection and flashcards |
| `vocabulary` | Guide flashcard hands, Vocabulary page, Word Match, Sprint mode pick/match and its listening exercises (what do you hear, select the matching audio, listen and match) |
| `conjugations` | Lesson table, Conjugation Drill, Sprint mode ladders |
| `conversations` | Sprint mode sentence builds |
| `listening` | Listening practice (hear it, type it), Sprint mode listening exercises (tap / fill in what you hear, what does it mean) |
| `guide` | Hand-ordered Guide steps |
| `sprint` | Sprint mode bites and sort / spot / build exercises |
| `prerequisites`, `unit`, `order` | Lesson order, quiz weighting, Map, Guide stage order |

`placement.json` (course-level) is read by the start wizard's placement
test; its `random_per_unit` draws from the lessons' multiple-choice `quiz`.

A lesson simply does not appear in a feature whose data it lacks (Word Match
needs at least 2 vocabulary entries, the drill
needs conjugations, Listening needs `listening` sentences).

## A minimal valid course

```json
// course.json
{ "id": "knots", "title": "Knots", "units": [ { "id": "1", "title": "Basics" } ] }

// lessons/bowline.json
{
  "id": "k-bowline", "slug": "bowline", "title": "The bowline", "unit": "1",
  "content": "## What it is\n\nA fixed loop that will not slip.\n\n## Tying it\n\nRabbit out of the hole, round the tree, back down the hole.",
  "vocabulary": [ { "term": "bight", "meaning": "a U-shaped bend in the rope" }, { "term": "standing part", "meaning": "the rope that leads away from the knot" } ],
  "quiz": [ { "question_id": "k-bowline-01", "prompt": "A bowline makes a loop that…", "type": "multiple-choice",
              "options": ["does not slip", "tightens under load"], "correct_answer": "does not slip", "explanation": "It is a fixed loop." } ]
}
```
