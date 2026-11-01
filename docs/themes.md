# Theming

A *theme* is a skin a learner wears across the whole app: colours, a font,
art, its own little icons and profile pictures, and a few strings. A
*course* can be customised for a theme by giving its lessons material in the
theme's world. This page is the complete reference for both halves: what a
theme file may contain, and everything a course may do to look and read
right inside a theme. The app ships three themes, `realm` (The Realm of
Eldmark, an original high-fantasy world; formerly `wow`), `meadowbrook` (an original cozy farm town; formerly `stardew`) and
`rustwake` (an original wasteland; formerly `fallout`). Old ids are still read
as the new ones in lesson `themes` blocks and migrated in `users.theme`; the Spanish
course carries material for all three.

Themes are offered only when the server runs with
`SKRONGLE_THEMES_ENABLED=1` (#1138); otherwise the app has no themes at all
and every course is shown as Default. Profile pictures, conversation
portraits and their backgrounds are off with them (#1153); a character
shows only its name.

**The rules that never change.** JSON files declare everything and are the
only source of truth. The app renders what they declare and nothing more.
A theme is flair:

- A course must be complete without any theme.
- A theme can only *add* to a lesson and never take teaching away.
- A lesson with nothing for a theme is shown unchanged in the theme's colours.
- Switching themes (or back to plain) changes how things look and which
  examples come first, never what is taught.

## Themes, courses and profiles

A learner can change three things, and they are kept apart on purpose:

| | What it is | Language | Depends on |
|---|---|---|---|
| **Theme** | Flair for the whole site: colours, font, art, icons, praise lines, and how your profile picture looks | the app's language (English in `theme.json`, the others in `i18n/`) | nothing |
| **Course** | What you learn. It may carry extra material for a theme so practice fits the theme's world: example sentences, exercises, praise lines | the course's own (Spanish practice in the Spanish course) | nothing; a theme only adds to it |
| **Profile** | You: your name and the picture you picked | English | nothing; never the course |

- A theme is worn across every course (or in one course, see "A course's own
  theme" below), so nothing in a theme file is in a course's language. It is
  written in English, and a built-in theme's title, description, praise
  ("Victory!") and colour and background names are app text in every
  language too (`themes.<id>` in `i18n/<code>.yaml`, see
  [Languages](i18n)), shown in the learner's app language in any course.
  Picture names ("Mogg") are names and read the same everywhere.
- Words in a course's language come from the course, never from a theme or
  the profile. To make a theme feel at home in a course, give the lessons a
  `themes.<id>` block (Part 2) and the manifest `themes.<id>.strings` for
  praise in the course's language ("¡Victoria!" in Spanish).
- The profile does not read the active course. The worn theme can change how
  your picture looks and what it is called. Switching courses changes
  neither. Spanish appears in the Spanish course's lessons, not on your
  profile.
- A test (`builtin_themes_speak_english` in `src/themes.rs`) fails when a
  built-in theme's title, description, picture names or praise look Spanish,
  and `builtin_theme_text_is_app_text` when `i18n/en.yaml` does not say what
  `theme.json` says. A change to a built-in theme's text changes both, and
  the other languages' `themes.<id>` keys.

## Part 1: the theme file

```
data/themes/<id>/
  theme.json             everything below
  pictures/**/*.svg      the art and pictures theme.json refers to
  badges/<badge>.svg     the theme's look of a shop badge (optional; see "Shop badges")
```

The app loads themes at startup and offers them to every learner in their
profile settings. A broken theme stops the app, and the error names the file
and field. Every key except `id` and `title` is optional.

| Key | What it does | Rules |
|---|---|---|
| `id` | The theme's name in URLs, in `users.theme` and in lessons' `themes` blocks | lowercase a-z, 0-9, `-`; must equal the folder name |
| `title` | Shown in the picker | non-empty |
| `description` | Shown under the picker when the theme is on | free text |
| `icon` | Emoji for the picker chip when the theme has no `art.mark` | any short text |
| `colors` | Colour tokens applied to every page, Sprint mode included | see [tokens](#colour-tokens); hex colours only |
| `dark` | The same tokens again, for browsers that prefer a dark scheme | same rules; omit for a theme with one look |
| `font` | A CSS `font-family` stack for the whole site | names, quotes, commas and hyphens only, ≤ 160 chars |
| `art.banner` | Wide picture at the top of the course home page and the Sprint mode home | `pictures/<name>.svg` of the theme, or an `https://` URL |
| `art.mark` | Small emblem on the picker chip and next to the course title | same |
| `strings.praise` | Replace a course's praise lines (a right answer) while the theme is on, unless the course has its own for the theme | non-empty strings in English; omit to keep the course's |
| `strings.almost` | Replace its "not quite" lines | same |
| `avatars` | The theme's version of each built-in profile picture | `id` one of `noor, amani, yuki, kai, arjun, sofia, mochi, coco` (the ids from before #180, `owl` … `bolt`, still name a slot); `image` a theme file or URL, drawn on a transparent background; `label` the picture's name, in English; `color` (optional) a hex colour behind the picture's tile in the picture dialog; since #263 the learner's own picture always wears a background of the palette (see [Picture backgrounds](#picture-backgrounds)) |
| `avatar_colors` | The theme's colours for the eight slots of the profile picture's palette | `slot` 1–8, each at most once; `label` the colour's name in English (the swatch's accessible name); `color` a hex colour. A slot left out keeps the default colour; the slot's pattern stays |
| `avatar_backgrounds` | Scenes of the theme's world the learner can put behind the profile picture, offered only while the theme is worn | `slot` 9–16, each at most once; `label` the scene's name in English; `image` a theme file (`pictures/backgrounds/<name>.svg`) or URL, a 64 × 64 SVG; `color` a hex colour shown under it while it loads |
| `icons` | The app's own little icons, by slot | see [icon slots](#icon-slots); 1–4 characters, no markup |
| `lesson_icons` | Icons for lessons that set none, used in turn along the Sprint mode path | same rule per entry |

### Colour tokens

`bg`, `surface`, `text`, `muted`, `line`, `accent`, `accent_dark`, `gold`,
`gold_dark`, `ok`, `ok_dark`, `ok_bg`, `bad`, `bad_bg`, `warn_bg`, `sky`,
`violet`. Each becomes the CSS custom property of the same name
(`--accent-dark` for `accent_dark`) that both stylesheets are built on. A
token left out keeps the app's default; the ones given override both the
light and the dark default, so a dark theme is simply one whose `colors` are
dark. `accent` is the button and link colour (buttons print white on it),
`gold` the Play mode and Sprint mode button colour (dark text on it), `ok`/`bad`
the right/wrong feedback, `sky` and `violet` the Sprint mode accents.

### Icon slots

| Slot | Default | Where |
|---|---|---|
| `streak` | 🔥 | Day streak on the home page and the Sprint mode home; a hot run of answers in a session |
| `xp` | ⚡ | XP counter; the Lightning round |
| `words` | 📚 | Terms collected |
| `goal` | 🎉 | Daily goal done; session complete |
| `flawless` | 🌟 | A perfect session |
| `effort` | 💪 | A rough session |
| `mastery` | 🌶️ | One unit of mastery on a path stop |
| `locked` | 🔒 | A path stop not yet reachable |
| `trophy` | 🏆 | Every lesson done |
| `review` | 🔁 | Review mix |
| `path` | 🗺️ | The Sprint mode path (bottom bar) |
| `site` | 🖥️ | Back to the full site |
| `start` | 🏁 | The current lesson when it has no icon |
| `bite` | 💡 | A bite without an emoji of its own |
| `hint` | 💡 | The hint toggle |
| `mode_sprint`, `mode_study`, `mode_play`, `mode_guide`, `mode_ai` | 📱 📖 🎲 🧭 🤖 | The modes on the home page and in the mode bars (`mode_d`, `mode_k`, `mode_g` still load as the first three; see `ICON_ALIASES`) |

### Renaming a theme

If a theme's id changes, add the old id to `THEME_ALIASES` in `src/content.rs`
(lesson blocks under the old id load as the new one), and write a migration
for `users.theme` and any ids derived from the theme (`stardew` became
`meadowbrook` in migration v14, which also rewrites answer history).

### Profile pictures

The eight built-in pictures (`src/profile.rs`) are the learner's choice
in each course (`projects.avatar`, the course's settings, #565) and never change. Every one is a person or an animal
with a name: six people from around the world and two pets. `avatars`
gives the theme's *version* of each, by id: a learner who chose Sofía
sees Nym the wood elf while Eldmark is on, Bo the sheep while Meadowbrook
is on, Blip the antenna rabbit while Rustwake is on, and Sofía again in
plain. A picture the theme gives no version of keeps its plain look.

A theme's pictures are people or animals too, never things. They include the
fantasy folk, robots, zombies and mutants of the theme's world. Each has a
short name of its own ("Mogg", not "Ogre"), simple to say. At least half of
them fit anyone, and they are not all from one country. Draw them as 64 × 64
SVGs on a **transparent background**, sized to fill a round badge; see
`data/themes/realm/pictures/avatars/` for the style. The app draws the badge
disc behind the picture in the learner's colour (below), so a picture must
not paint its own full-size disc, or the colour never shows (a soft white
glaze, like Meadowbrook's, is fine). Name them in English, because a theme is
worn across courses in any language.

### Picture backgrounds

After the picture, a learner picks the background of the disc behind it
(`projects.avatar_color`, #227, #248, per course since #565): one of eight
patterned backgrounds in slots 1–8, or one of the theme's scenes in slots
9–16. The background is the course's, not the picture's: another picture
keeps it (#263), and one never picked is slot 1.

**Slots 1–8** are a colour with a light pattern over it. The default
palette is `profile::AVATAR_COLORS`: Amber sunburst, Green waves, Rose
confetti, Teal stars, Slate stripes, Olive dots, Violet rings and Gold
zigzag (the colours are the built-in pictures' own). The patterns are
`static/avatar-bg/<pattern>.svg`, white shapes at low opacity, so they suit
any colour. `avatar_colors` gives the theme's colour for any slot, the way
`avatars` gives its version of a picture, and the pattern stays: the slot
is what is stored, so a learner who picked slot 3 wears confetti on Ember in
Eldmark, on Blossom in Meadowbrook and on Rose in plain. A slot the theme
gives no colour for keeps the default one, and a theme without
`avatar_colors` offers the default palette (a theme only adds).

**Slots 9–16** are the theme's own: `avatar_backgrounds` adds little
scenes of its world (the built-in themes have four each: Eldmark's moonlit
pines, snowy peaks, dune sunset and northern lights; Meadowbrook's crop
fields, orchard hills, fishing pond and harvest sunset; Rustwake's dust
horizon, rust canyon, desert night and pylon ridge). They come after the
eight, only while the theme is worn. A chosen scene follows the theme like
a colour does: the stored slot shows each theme's scene in that slot. While
a theme without one is worn (or plain), slot 1 shows and its swatch is
selected (`profile::shown_slot`). The app keeps the choice, so wearing the
theme again brings the scene back. Draw a scene as an original, simple,
flat 64 × 64 SVG (no raster images, a few hundred bytes to a few KB; a
test keeps each under 4 KB) and keep the middle and bottom calm. The
picture stands in front of it, so the sky and the edges are what show.
Generic scenery only, never a real game's places, maps or logos.

A picture's tile in a course's settings wears its own colour: the theme's
version's `color` while the theme is on, else the built-in picture's. A
version without `color` has the page's surface colour behind its tile.

### Shop badges

The Skrongle Cash shop (#569) sells badges that show on a course's SKC
hero. Each has plain art, `data/badges/<badge>.svg`. A built-in theme may
draw its own look of any of them as `data/themes/<id>/badges/<badge>.svg`,
named by a badge id of `data/badges/catalogue.json` (a test fails on a file
that names no badge). It is not listed in `theme.json`. The file being
there is enough. While the theme is worn, a badge it draws shows in its
look; one it does not draw shows plain, and so does every badge under a
theme you made. A badge is bought once, so taking the theme off keeps the
badge and only changes how it looks. Draw it like the plain art: an
original, flat 64 × 64 SVG in the theme's colours, no raster images and no
real game's emblems. The built-in themes each draw two (Meadowbrook and
Eldmark: Sprout and Lantern; Rustwake: Lantern and Compass).

### A complete theme file

```jsonc
{
  "id": "meadowbrook",
  "title": "Meadowbrook",
  "description": "Sunny farm colours, a barnyard of profile pictures, and sentences about crops, hens and fishing wherever a course has them.",
  "icon": "🌻",
  "colors": { "bg": "#f6efd8", "surface": "#fffaf0", "text": "#3b2a1a", "muted": "#7a6a52", "line": "#e0d3b0",
              "accent": "#4e8f3a", "accent_dark": "#37702a", "gold": "#e8b43a", "gold_dark": "#c2921a",
              "ok": "#5a9e3c", "ok_dark": "#3f7a28", "ok_bg": "#e3f2d6", "bad": "#c74a3a", "bad_bg": "#f8e1dc",
              "warn_bg": "#fbf0c8", "sky": "#6fb3e0", "violet": "#9b6fc9" },
  "dark": { "bg": "#1f1a12", "surface": "#2a2418", "text": "#f0e6cc", "muted": "#b3a689", "line": "#4a4030",
            "ok_bg": "#22361a", "bad_bg": "#3e1f1a", "warn_bg": "#3a3016" },
  "font": "'Avenir Next Rounded', 'Nunito', 'Varela Round', 'Trebuchet MS', sans-serif",
  "art": { "banner": "pictures/banner.svg", "mark": "pictures/mark.svg" },
  "strings": { "praise": ["Good harvest!", "It's blooming!"], "almost": ["Not quite, neighbour", "Close… a little more water"] },
  "avatars": [ { "id": "sofia", "label": "Bo", "image": "pictures/avatars/sheep.svg", "color": "#a58ad4" } ],
  "avatar_colors": [ { "slot": 1, "label": "Honey", "color": "#f2b24a" }, { "slot": 2, "label": "Clover", "color": "#8cc46a" } ],
  "avatar_backgrounds": [ { "slot": 9, "label": "Crop fields", "image": "pictures/backgrounds/crop-fields.svg", "color": "#8fd0f0" } ],
  "icons": { "streak": "🌞", "xp": "✨", "words": "🌾", "goal": "🧺", "mastery": "🍓", "locked": "🪵", "mode_sprint": "🎣" },
  "lesson_icons": ["🥕", "🍓", "🌽", "🎃", "🐔", "🐄"]
}
```

## Part 2: customising a course for a theme

A course never needs a theme. It may carry, in any lesson, a block under
`themes.<id>` written in the theme's world, and, in `course.json`, praise
lines for the theme in the course's language. Everything in the block is
optional, every field has the same shape as the lesson field it mirrors, and
a lesson may carry blocks for several themes side by side.

```jsonc
// inside a lesson file
"themes": {
  "meadowbrook": {
    "icon": "🌱",                 // the lesson's stop on the Sprint mode path while the theme is on
    "content": "## En la granja\n\n…",             // Markdown appended as extra section(s)
    "vocabulary": [
      { "term": "hola", "example": "¡Hola! Soy la nueva granjera." },            // a term of the lesson: a new example only
      { "term": "la granja", "meaning": "farm", "example": "Mi granja está al lado del bosque." }   // a new term: needs a meaning
    ],
    "quiz": [ ... ],              // questions; ids unique across the course, so suffix them: "<lesson id>-meadowbrook-01"
    "listening": [ ... ],         // sentences (text, meaning, note)
    "conversations": [ ... ],     // dialogues; ids unique across the course: "<lesson id>-conv-meadowbrook-<slug>"
    "bites": [ ... ],             // Sprint mode bites (title, emoji, rule, example, example_meaning)
    "sorts": [ ... ], "spots": [ ... ], "builds": [ ... ]   // Sprint mode exercises, same shapes as under sprint
  }
}
```

### What each field does while the theme is on

| Field | Merged how | Shows up in |
|---|---|---|
| `icon` | Replaces `sprint.icon` | The lesson's stop on the Sprint mode path, the Continue button |
| `content` | Appended after the lesson's Markdown as extra `## ` sections | Lesson page, Guide cards, AI tutoring; Sprint mode bites when the lesson writes none |
| `vocabulary` (existing term) | Its `example` replaces the lesson's; term and meaning untouched | Lesson page (marked with the theme's icon), Vocabulary page, flashcards, Sprint mode "this means…" feedback |
| `vocabulary` (new term) | Added after the lesson's own terms | Everywhere vocabulary is used: Word Match, flashcards, Sprint mode pick / match / listening-by-word |
| `quiz` | Added in front of the lesson's questions | Practice, the lesson quiz, Guide quick checks, AI tutoring, Sprint mode (taken first) |
| `listening` | Added in front | Listening practice, Sprint mode listening exercises (taken first) |
| `conversations` | Added in front | Sprint mode sentence builds (taken first) |
| `bites` | Added in front | Sprint mode (the first bites of a session) |
| `sorts`, `spots`, `builds` | Added in front | Sprint mode (taken first) |

### Praise in the course's language

A theme's `strings` are English, so a Spanish course worn in Eldmark would
cheer "Victory!". The course can say it in its own language instead:

```jsonc
// course.json
"themes": {
  "realm": { "strings": { "praise": ["¡Victoria!", "¡Botín épico!"], "almost": ["Casi… repite la misión"] } }
}
```

While a theme is worn, the course's `themes.<id>.strings` win, then the
theme's `strings`, then the course's own `strings`; each of `praise` and
`almost` is picked on its own. A block for a theme that is not installed is
kept and never used. Empty lines are a validation error naming `course.json`.

"In front" matters. Sprint mode shuffles each kind and takes the first few.
With a theme on, it takes the themed ones first. A lesson with one themed
sort, two themed spots and two themed builds opens a session almost entirely
in the theme's world and falls back to its own material for the rest. The
practice quiz draws by mastery across lessons, so themed questions appear
as a share of the whole.

### Validation

The app validates every block with the course whether or not anyone wears the
theme, and errors name the file and the theme (`lessons/a1/greetings.json
(theme 'meadowbrook')`). The checks are the lesson's own (a spot's wrong word
appears once, a build has 2–12 words, listening texts are distinct, a
question's `teaches` names terms of the merged lesson, conversations give
both roles a line…) plus the theme-specific ones:

- a re-examples entry may not change a term's meaning, and must give an example;
- a new term needs a meaning;
- themed question and conversation ids must be unique within the theme and
  never reuse the lessons' own;
- `icon` is a short glyph (1–4 characters, no markup);
- the block's key must look like a theme id. A block for a theme this
  installation does not have is kept and never shown.

### What a theme cannot change in a course

The lesson's own fields, its structure (units, groups, order), paradigm
tables and conjugations, and the Guide's
hand-written step order. The themed extras go into the derived places (a
hand-ordered Guide gets the theme's questions as quick checks right after
its first card, since no step of it can name them). Mastery, vocabulary and
history records are the same whichever theme is on (themed questions and
terms are more of them, with their own ids).

### Checklist for a themed lesson

A good block, in the order the learner meets it:

1. `icon`: one emoji for the stop on the path (the greetings hand becomes a
   gauntlet or a sprout).
2. `content`: one `## … en …` section, ≤ 60 words, Spanish in bold, that
   restates the lesson's one idea with the theme's things.
3. `vocabulary`: re-examples for 5–8 of the lesson's own terms, plus 2–4 new
   theme words with a meaning (la mazmorra, la cosecha…). Keep the grammar
   exactly that of the lesson.
4. `bites`: one, with an emoji, a rule ≤ 200 characters and an example.
5. `listening`: five sentences of 2–10 words, a `New word:` note where a
   theme word is new.
6. `builds`: two; `spots`: two; `sorts`: one with 2–3 buckets and 5–6 items.
7. `quiz`: two questions, ids `<lesson id>-<theme>-01`, `-02`, `teaches`
   naming merged terms.
8. `conversations`: optional, one short dialogue (6–8 lines) with an id
   `<lesson id>-conv-<theme>-<slug>`; it also feeds Sprint mode's sentence builds.

Write and finish the lesson first, add the block afterwards, and never move
teaching into it. The Spanish course's `AUTHORING.md` has the per-theme
house style (place names, races, crops) for the two built-in themes.

## Themes you make

A learner can keep themes of their own: the same `theme.json` and pictures,
checked by the same rules and stored in the app's database instead of
`data/themes/`. They are private. Nobody else can wear one, see its pictures
or find out it exists. A theme made in the app is worn under the id
`user-<n>` (its `theme.json` needs no `id`; one given is ignored), which
built-in themes may not use. The course settings list them after the built-in
ones. No page deletes one yet (#577; `POST /theme/delete` does, and the
courses that wore it go back to Default).
Each account may keep a few themes of limited total size (see
[the configuration](architecture#configuration)). The theme editor will make
and edit them in the browser. Lessons' `themes.<id>` material for them is not
available yet.

## A course's own theme

A learner can wear one theme in one course and another elsewhere (#565). The
theme is one of the course's settings (#574; see
[Features](features#themes)): picked in the wizard of the course's first
visit, then behind the gear on its card in the course list (only for a
course they opened), with the motivator, the profile picture and its
background. The choices are *Default* or any theme they may wear, their own
included; the pictures and backgrounds are drawn in the theme picked, and
the wizard previews it on itself. While that course is the active one,
every page wears its theme, the profile page too, except the course list and
the pages under `/courses` (the editor, a new course), which wear Default,
as does every page with no active course. The profile has no theme (#577).
Lessons get the material of the theme worn in the course.

On the course list, each opened course's card wears its own theme's colours,
font and banner (behind the text at 30 % opacity). This uses `data-card-theme`
and `/courses/themes.css`, which holds every theme the learner may wear,
scoped to a card; Default, which the list wears, has none. Picking a theme in
the dialog shows it on the card with a short fade. Cancel puts the saved one
back. Save posts without a reload, closes the dialog, and the card grows and
shrinks for 300 ms. With reduced motion, neither animation plays.

## Where things live

| | |
|---|---|
| Theme files | `data/themes/<id>/theme.json`, `pictures/**` |
| Themed lesson material | `themes.<id>` in any `lessons/**/*.json` |
| The learner's choice | per course: `projects.theme` ('' or NULL = Default), `projects.avatar`, `projects.avatar_color` (`users.theme` is no longer read, #577) |
| Theme pictures on the web | `/themes/<id>/pictures/…` (a theme you made: only you) |
| Shop badges | plain `data/badges/<badge>.svg` at `/badges/<badge>.svg`; a theme's look `data/themes/<id>/badges/<badge>.svg` at `/themes/<id>/badges/<badge>.svg` (`src/motivators/shop.rs`) |
| Themes you made | `user_themes`, `user_theme_files`; worn as `user-<n>` |
| The theme's stylesheet | `/theme.css?t=<id>` (the page names the theme it wears) |
| Model and merge | `src/content.rs` (`Theme`, `LessonTheme`, `apply_theme`, `Icons`), `src/themes.rs` |
