# Themes for learners and authors

A theme is a skin for the whole app while you are in a course. It never
changes what is taught.

## Wearing a theme

Each course has its own theme. Pick it in the wizard the first time you open
the course, then change it any time from the gear on the course's card on
`/courses`. *Default* takes it off. The course list, and any page with no
course open, wears Default.

The app ships three:

- **The Realm of Eldmark**: an original high-fantasy world, gold on night.
- **Meadowbrook**: a cozy original farm town.
- **Rustwake**: an original retro-futurist wasteland, terminal green on dark.

With a theme on:

- Every page takes its colours and font; its banner sits on the course home
  and the Sprint home.
- Its own icons replace the app's (streak, XP, the modes, the path).
- The course's picture shows in the theme's version when the theme draws
  one; take the theme off and the plain picture is back.
- Right-answer praise comes from the theme, or from the course's own lines
  for that theme.
- Where a lesson has material for the theme, its themed examples, exercises
  and questions come first. The Spanish course has some for all three.
- Badges from the Skrongle Cash shop show in the theme's look where the
  theme draws one, plain where it does not. Taking the theme off never
  takes a badge away. Theme authors: [Shop badges](themes#shop-badges).

Themed material appears in the built-in courses only for now. A course you
built or generated is shown in the theme's colours with its content
unchanged.

## Writing themed material (authors)

A lesson may carry a `themes.<id>` block: extra material in the theme's world,
written with the same shapes as the lesson's own fields. The rules:

1. **The lesson comes first.** Write it complete without any theme, then add
   themed material on top.
2. **A theme only adds.** Themed bites, sentences, exercises, questions and
   conversations go *in front of* the lesson's own; nothing of the lesson is
   removed.
3. **Examples may change, meanings may not.** A themed `vocabulary` entry for
   a term the lesson already has swaps in a new example only; a new term must
   give its meaning.
4. **Ids stay unique across the course.** Suffix themed question and
   conversation ids, e.g. `a1-greetings-realm-01`.
5. **The course's language comes from the course.** A theme file is in
   English (the built-in themes also speak the app's other languages);
   praise in the course's language goes in `course.json`
   `themes.<id>.strings`.
6. **No block, no change.** A lesson without a block for a theme is shown as
   it is. You never need to cover every lesson.

From the Spanish greetings lesson, under `themes.realm`:

```json
{
  "icon": "🧤",
  "vocabulary": [
    { "term": "hola", "example": "¡Hola! Soy un mago de Silvermere.", "example_meaning": "Hello! I'm a mage from Silvermere." }
  ]
}
```

The lesson's stop on the Sprint path shows 🧤 instead of 👋 while Eldmark is
on, and *hola* gets a fantasy example; its meaning stays "hello".

Every field a theme block takes:
[Themed material](course-format#themed-material-optional). Theme files
themselves (colours, icons, pictures): [Theming](themes).
