# The test course

A course for automated tests only. It uses **every feature a course file can
switch on**: every quiz type, every Sprint mode exercise, every game, the Guide
(hand-ordered and derived), Listening, the paradigm drill, Date Dash and the legacy field names. Browser tests use it to
smoke-test a feature without playing through the Spanish course.

It is offered only when the server runs with `TEST_COURSE=1`
(`./.dev-start --test-course`; `scripts/e2e.sh` always sets it), never in
production (`deploy/` doesn't set it and the deploy scripts don't copy
`data/test-courses/`). Open it with `/dev/login?open=test-course`, or in
a spec with `devLogin(page, "open=" + TEST_COURSE)` from `e2e/lib/app.ts`.

## The rule

**Adding a feature that course files can use means adding it here, in the
same change, together with the browser tests that use it.** That covers a
new lesson or manifest field, a quiz type, an exercise kind, a Guide step,
a game that reads course data, or an alias. `cargo test test_course`
(`src/test_course.rs`) fails until the course covers it:

- every property and enum value of `docs/course.schema.json` (manifest and
  lesson) is used somewhere in the course;
- `lessons/0-all/everything.json` has every lesson field;
- `lessons/5-edge/legacy.json` uses every `#[serde(alias)]` of `src/content.rs`;
- every `QuizType` has its own lesson, named by its slug;
- Sprint mode produces every exercise kind (`kind()` in the test names them, and a
  new `Exercise` variant doesn't compile until it is named), and each
  feature lesson's session always contains its feature (the table below);
- every Guide step kind appears, and one lesson derives its Guide.

Some features have no field of their own, such as a behaviour of the
loader, a page or a game that reads existing data. The tests can't see those, so you
have to remember them: give the feature a lesson (or a part of one) here and
add a row to the table.

## What is where

One lesson per feature, so a spec can open a lesson and know what it will
get. "Sprint mode always has" is asserted for every shuffle by
`each_feature_lesson_always_shows_its_feature`.

| Unit | Slug | Feature | Sprint mode always has |
|---|---|---|---|
| Everything | `everything` | Every lesson field at once: intro text, table, list, blockquote, code block with a fake `## `, every quiz type but algebra tiles and measure, a conjugation inheriting its art (hablar, preterite), a conversation, listening, a hand-ordered Guide with every step kind (section, vocab, question, tip, concept), sprint icon/bites/sort/spot/build. The first lesson, so the first Guide stage and the first stop on the path | |
| Question types | `multiple-choice` | 2- and 4-option questions, one and two hints, `teaches`, auto-collected term | choice |
| | `true-false` | `true` / `false`, served as **True** / **False** | choice |
| | `fill-in-blank` | Accent near miss (`está`; `esta` gets the note), `accepted_answers` (`yo soy`), explicit `teaches`, auto-collected term, a two-word answer | word tiles |
| | `numeric` | Integer, fraction (`3/4`; `0.75` and `75%` count), `tolerance` + `unit` (`12.57 cm²`), an any-order list (`2; -3`, typed, no tiles), `accepted_answers` (`28` or `29`) | number tiles |
| | `expression` | Equivalence (`(x+1)(x+1)`), a leading `y =` ignored, `accepted_answers` (`4s` or `s^2`) | typed formula |
| | `fill-bar` | Fill the bar: any tiles that fill a 2-beat gap, no `correct_answer` (the loader picks `B2`), an overfull tile (`B4`); an `exact` one whose answer is `c2` and where `cc` gets the "particular notes" note; a 3/4 bar one beat short in G (`e2e/specs/fill-bar.spec.ts`) | fill the bar |
| | `chord-spell` | Spell the chord: `tiles` as a string and as a list, a triad (`D ^F A`; `D _G A` gets the spelling note), a minor triad, a four-note seventh chord (`e2e/specs/chord-spell.spec.ts`) | chord spell |
| | `staff-note` | Place the note: treble `G` (octave counts; `g` gets the octave note), bass middle C on a ledger line, `^F` with `any_octave` (`e2e/specs/staff-note.spec.ts`) | staff note |
| | `keyboard` | Find it on the keys (`e2e/specs/keyboard.spec.ts`): an ear question by pitch class (`play` E, any E counts, `octaves: 1` written out); a black key heard as `_B` that the key `^A` answers, on two octaves; an `octave: true` question with a staff and no `play`, where middle C gets the "wrong octave" note | keyboard |
| | `algebra-tiles` | A board of x², x and 1 tiles in Study and the Guide; `tiles` with and without `form`, two big squares (`(2x + 1)(x + 1)`), no units (`x(x + 3)`); the expanded form gets the two-sides note | nothing: Sprint mode skips it |
| | `clock` | Set the clock: a quarter to four (5-minute steps, a hint), las seis y media (15-minute steps), twelve o'clock (where the clock starts); turned around: read a clock and pick 9:15 among four times | set-clock, read-clock |
| | `measure` | A protractor over an obtuse (125°) and two acute (40°, 72°) angles, `tolerance` 2, unit `°`; 55° on the first gets the other-scale note. Study and the Guide only | bites only (Sprint mode skips the questions) |
| Sprint mode exercises | `bites` | Hand-written bites: with emoji, without (the 💡 icon), without an example; `sprint.icon` | bite |
| | `sort` | Sorts with 2, 3 and 4 buckets; bites derived from the sections; a "Quick check" section Sprint mode skips; no icon (default/`lesson_icons`) | sort |
| | `spot` | Spot the mistake, incl. a wrong word with punctuation attached (`gato.`) | spot |
| | `build` | Builds with and without distractors (2 words) | build |
| | `conjugations` | Paradigm table "Verb forms" (7 slots), two items with art, a fill-in-blank whose tiles come from the paradigm; the drill unlocks hablar and comer when this lesson passes | ladder |
| | `conversation` | Two conversations: scenario, a `note`, a line without `meaning`; one without a scenario | conversation build |
| Words and sound | `vocabulary` | Six colours: flashcards, Word Match (`/play/matching/play?lesson=vocabulary`), pick, match the pairs, the word listening exercises; an entry without an example | pick (term), match |
| | `listening` | Listening sentences: tileable, one word, 23 words (Study mode only), a grammar note, a `New word:` note, one without a meaning | a listening exercise |
| Stages | `guide-derived` | No `guide`: cards from sections, vocabulary in hands of 5 + 1, questions from the third card on; `prerequisites` | |
| Edge cases | `read-only` | No questions, no vocabulary, no group (`general`), no order (100): the *I've read it* button; a Sprint mode session of bites only | bite |
| | `legacy` | Every old field name (`level`, `category`, `markdown_content`, `journey`, `es`/`en`, `example_es`/`example_en`, `verb`/`tense`, `dmode`, …); group `Legacy`, which the manifest doesn't list | |
| | `notation` | A ```` ```abc ```` block in the lesson text, drawn as a staff on the lesson page and in the Guide (`e2e/specs/notation.spec.ts`), a plain code block that stays code, and a non-ABC and an empty `abc` block that stay as text (#1187); a "Name the note" question with `abc` on a staff above its prompt in Practice, Sprint mode and the Guide (`music.note_names` is `letters` in `course.json`); a Sprint build of the circle of fifths (`C G D A E B`), kept because the course has `music` (#1232); two "Pick the interval" questions (#1220), C then E and a stacked D and F, with interval names as options and explanations that count the half steps; a Sprint spot with `abc` (Fix the wrong note: tap the 4th note, pick F from three tiles); two "Name the chord" questions (#1222), a stacked D minor and F major, with chord names as options and explanations that name the notes; two "Name the key" questions (#1221), a two-sharp signature (with a whole note B so the staff is drawn) and a short tune in A minor, with key names as options and explanations that count the sharps or name the note the tune ends on; order 200 | bite, build, choice, spot |
| | `ear` | Music to listen to (`e2e/specs/music-sound.spec.ts`): "Hear the interval", a multiple-choice question with `play` and no `abc` (a speaker button, the notes on a staff after the answer); "Which melody plays?", with `option_kind: "abc"` (the options drawn as staves); "Which bar did you hear?", a rhythm with `K:C clef=perc` and `M:4/4` in `play` that plays as clicks of one pitch, with three one-bar staves as options; a Sprint sort of speaker tiles (sort items with `play` and no `text`); a Sprint scale, D major with the decoys F and C (`e2e/specs/scale-ladder.spec.ts`), also drawn and played on the lesson page; order 210, the last of its unit | choice, sort, scale |

Course-level features, from `course.json`: a language course (`es` →
`en`, with `language_features` set to `true` as well: Listening, Numbers by
ear, tap to hear, Sprint mode listening), `chars`,
`calendar` (Date Dash), a paradigm with a custom label and slot art (seven slots),
`strings` with one praise (`¡Correcto!`) and one almost (`Casi…`) so a spec
can match them exactly, six units (one without a description) marked as `levels`, so a
new learner picks one to start at (`e2e/specs/smoke/start-level.spec.ts`
starts at *Edge cases*), listed and unlisted groups. `placement.json` is
the placement test (#1106): two written questions for *Everything* (every
field of a written question) and `random_per_unit: 1`, so every other unit
with a multiple-choice question draws one (`e2e/specs/smoke/placement.spec.ts`).

## Writing specs against it

- The smoke specs finish the whole course as a new account:
  `e2e/specs/smoke/complete-test-course.spec.ts` (Study, Sprint, Play; it fails when Sprint
  never serves a kind of `ALL_KINDS`) and `test-course-games.spec.ts` (every
  game on `/play`, the tutor). `wrong-answers.spec.ts` answers
  the same features wrong and checks that each is marked and counted wrong
  (it fails when a quiz type or a graded Sprint kind is never answered wrong).
  A new feature needs a step in both: the right path and the wrong one.
- Answers: `new AnswerKey("test-course")` (`e2e/lib/key.ts`) gives the right
  answer to a question id or a Sprint mode exercise id, from these files;
  `correctAnswers("test-course")` maps question ids to answers. A
  true-false answer is `"true"` in the file and **True** on screen.
- Guide stages unlock in order; only `everything` is open for a new account.
  Sprint mode lessons, practice, listening and the lesson pages are not gated.

## Writing content here

Short, plain and unambiguous: one obvious right answer, correct Spanish,
no content that teaches anything beyond the feature. Keep each lesson about
its feature, so its sessions stay predictable. Add to `everything.json` too
when you add a lesson field. Lesson files are `json.dumps(indent=2,
ensure_ascii=False)`.
