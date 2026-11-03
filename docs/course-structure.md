# How a course is built

A course is a folder of JSON files. This page walks through one with examples
from the built-in Spanish course. Every field and rule is in the
[course format](course-format), the reference the AI generator also follows.

## The folder

```
spanish/
  course.json                                  the manifest: title, units, languages
  placement.json                               optional: a placement test (a course with levels)
  lessons/a1/greetings-and-introductions.json  one lesson per file
  lessons/a1/…                                 folders under lessons/ are up to you
```

A built-in course lives in `data/courses/<id>/` and its folder name is its
id. A course made in the app has the same files, stored in the database.

## Units, groups, lessons

```
course
└── units     A1, A2 …              in the order course.json lists them
    └── groups   Grammar, Vocabulary   labels for lists; free-form
        └── lessons  one file each      ordered by "order", then title
```

## course.json

The Spanish manifest, shortened:

```json
{
  "id": "spanish",
  "title": "Spanish",
  "icon": "🇪🇸",
  "terms":    { "label": "Español", "lang": "es" },
  "meanings": { "label": "English", "lang": "en" },
  "units": [
    { "id": "A1", "title": "A1", "description": "Beginner" },
    { "id": "A2", "title": "A2", "description": "Elementary" }
  ],
  "groups": [ { "id": "Grammar", "title": "Grammar" }, { "id": "Vocabulary", "title": "Vocabulary" } ],
  "chars": ["á", "é", "í", "ó", "ú", "ñ", "ü", "¿", "¡"],
  "strings": { "praise": ["¡Correcto!", "¡Muy bien!"], "almost": ["Not quite", "Casi…"] }
}
```

- `terms` is what you learn, `meanings` what it is explained in. When their
  `lang` differs it is a **language course**, and listening, Numbers by ear and
  tap to hear switch on. For maths or history, set both to your language.
  The editor's gear has a **Language features** switch that overrides this
  (it writes `language_features` here), for a course that teaches terms in
  your own language and should not ask for translations.
- `chars` are the buttons under a text answer for letters a keyboard lacks.
- Spanish also has a `paradigm` (the conjugation table: six persons × tenses)
  and a `calendar` (days and months, for Date Dash). Leave them out and those
  games are hidden.

## A lesson

`lessons/a1/greetings-and-introductions.json`, shortened:

```json
{
  "id": "a1-greetings",
  "slug": "greetings-and-introductions",
  "title": "Greetings and introductions",
  "unit": "A1",
  "group": "Vocabulary",
  "order": 1,
  "summary": "Say hello, goodbye, and introduce yourself.",
  "content": "## Saying hello\n\n- **Hola** — hi / hello (any time of day)\n…",
  "vocabulary": [
    { "term": "hola", "meaning": "hello", "example": "¡Hola! ¿Qué tal?", "example_meaning": "Hi! How's it going?" }
  ],
  "quiz": [
    {
      "question_id": "a1-greetings-01",
      "prompt": "It is 9 in the morning. You greet your neighbour with:",
      "type": "multiple-choice",
      "options": ["Buenos días", "Buenas noches", "Buenas tardes"],
      "correct_answer": "Buenos días",
      "explanation": "'Buenos días' is used in the morning, roughly until lunchtime.",
      "teaches": ["buenos días"]
    }
  ],
  "listening": [ { "text": "Buenos días, ¿qué tal?", "meaning": "Good morning, how's it going?" } ],
  "sprint": {
    "icon": "👋",
    "spots": [ { "sentence": "Buenos tardes, señora García.", "wrong": "Buenos", "fix": "Buenas",
                 "why": "Tarde is feminine, so it's buenas tardes." } ]
  }
}
```

Only `id`, `slug`, `title`, `unit` and `content` are required. `content` is
Markdown; each `## ` heading starts a section, and sections become the Guide's
cards and Sprint's bites. Three to five short sections make a good lesson.

## Which field switches on what

Add a field and the lesson appears in the features that read it; leave it out
and the lesson simply is not there. The complete table is
[Which feature reads what](course-format#which-feature-reads-what).

- **Lesson page, Guide cards, Sprint bites**: `content`.
- **Practice, reading check, Guide checks, Sprint questions, mastery**:
  `quiz`. Five types: multiple-choice, true-false, fill-in-blank, numeric
  and expression (maths answers checked by value).
- **Flashcards, Word Match, Sprint word games**: `vocabulary`.
- **Listening and Sprint's listening exercises** (language courses): `listening`.
- **Sprint sentence builds**: `conversations`, or `sprint.builds`.
- **Sprint sort / spot-the-mistake / bites by hand**: `sprint`.
- **Conjugation drill, Sprint ladders**: `conjugations` (needs `paradigm`).
- **A Guide stage in your own order**: `guide`.

## Smallest valid course

Two files are enough; see [A minimal valid course](course-format#a-minimal-valid-course).
To make one in the app, see [Building and generating courses](making-courses).
