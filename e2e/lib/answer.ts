// Answer Study and Play questions from the answer key (key.ts). A question on the
// lesson page (reading check), in Practice and in a Guide stage carries its
// id in `data-qid`, and either radio options, a text box or a clock to set.
import { expect, type Locator, type Page } from "@playwright/test";
import { setClock } from "./clock.ts";
import type { AnswerKey } from "./key.ts";
import { pressKey } from "./keyboard.ts";
import { progressOf } from "./learner.ts";

/** Fill in the right answer to the question `q` (an element with data-qid), or a wrong one. It doesn't submit. */
export async function answerQuestion(q: Locator, key: AnswerKey, wrong = false): Promise<string> {
  const id = await q.getAttribute("data-qid");
  if (!id) throw new Error("answerQuestion: the element has no data-qid");
  const bar = q.getByTestId("fill-bar");
  if (await bar.count()) {
    const want = key.question(id).correct_answer;
    return fillTheBar(bar, want ? want.split(/\s+/) : null, wrong);
  }
  const chord = q.getByTestId("chord-spell");
  if (await chord.count()) return spellChord(chord, key.question(id).correct_answer.split(/\s+/), wrong);
  const staff = q.getByTestId("staff-note");
  if (await staff.count()) {
    const want = key.question(id).correct_answer;
    return placeNote(staff, wrong ? otherNote(want) : want);
  }
  const keyboard = q.getByTestId("keyboard");
  if (await keyboard.count()) {
    const k = key.question(id);
    return pressKey(keyboard, [k.correct_answer, ...k.accepted_answers], k.octave, wrong);
  }
  const radios = q.locator("input[type=radio]");
  if (await radios.count()) {
    const values = await radios.evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value));
    const answer = wrong ? key.wrong(id, values) : key.answer(id, values);
    const i = values.indexOf(answer);
    if (i < 0) throw new Error(`question ${id}: the answer "${answer}" is not among the options ${JSON.stringify(values)}`);
    await radios.nth(i).check();
    return answer;
  }
  const answer = wrong ? key.wrong(id) : key.answer(id);
  if (await q.locator("[data-clock] [role=slider]").count()) await setClock(q.locator("[data-clock]"), answer);
  else await q.locator("input.fill").fill(answer);
  return answer;
}

/**
 * Fill the bar (#1230) in `box`: tap `tiles` in order, or with none given the
 * largest tile that still fits until the gap is full. Wrong: overfill it, the
 * smallest tile until the largest no longer fits, then the largest. Each tile
 * says its beats (`data-beats`), the bar what is left (`data-left`). Returns
 * the answer given, the tiles separated by spaces. It doesn't submit.
 */
export async function fillTheBar(box: Locator, tiles: string[] | null, wrong = false): Promise<string> {
  const buttons = box.getByTestId("fill-bar-tile");
  await expect(buttons.first()).toHaveAttribute("data-beats", /\d/);
  const all = await buttons.evaluateAll((els) => els.map((e) => ({ tile: e.getAttribute("data-tile") ?? "", beats: Number(e.getAttribute("data-beats")) })));
  const left = async () => Number(await box.getAttribute("data-left"));
  const given: string[] = [];
  const press = async (i: number) => {
    if (i < 0) throw new Error(`fill the bar: no such tile among ${JSON.stringify(all.map((x) => x.tile))}`);
    await buttons.nth(i).click();
    given.push(all[i].tile);
  };
  const bySize = all.map((_, i) => i).sort((a, b) => all[a].beats - all[b].beats);
  if (wrong) {
    const small = bySize[0], big = bySize[bySize.length - 1];
    while ((await left()) >= all[big].beats && (await left()) > 0) await press(small);
    if ((await left()) <= 0) throw new Error(`fill the bar: the tiles ${JSON.stringify(all)} can't overfill it`);
    await press(big);
  } else if (tiles) {
    for (const t of tiles) await press(all.findIndex((x) => x.tile === t));
  } else {
    for (let r = await left(); r > 0; r = await left()) await press(bySize.filter((i) => all[i].beats <= r + 1e-9).pop() ?? -1);
  }
  await expect(box).toHaveAttribute("data-full", "true");
  return given.join(" ");
}

/**
 * Spell the chord (#1228) in `box`: tap the tiles of `notes` (ABC) into the
 * slots, or, wrong, as many tiles as there are slots, those not in the chord
 * first. Returns the answer given, the notes separated by spaces. It doesn't submit.
 */
export async function spellChord(box: Locator, notes: string[], wrong = false): Promise<string> {
  const tiles = box.getByTestId("chord-spell-tile");
  await expect(tiles.first()).toBeVisible();
  const all = await tiles.evaluateAll((els) => els.map((e) => e.getAttribute("data-tile") ?? ""));
  // A note's spelling without its octave, as src/notation/chord_spell.rs compares them.
  const spell = (n: string) => n.replace(/[,']/g, "").toUpperCase();
  const inChord = (t: string) => notes.some((n) => spell(n) === spell(t));
  const pick = wrong ? [...all.filter((t) => !inChord(t)), ...all.filter(inChord)].slice(0, notes.length) : notes.map((n) => all.find((t) => spell(t) === spell(n)) ?? n);
  for (const t of pick) await box.locator(`[data-testid="chord-spell-tile"][data-tile="${t}"]`).click();
  await expect(box).toHaveAttribute("data-full", "true");
  return pick.join(" ");
}

// Place the note (#1226), as static/staff-note.js draws it: a viewBox 180
// wide and 118 tall, the note at x 112, step s (lines and spaces counted
// from the bottom line) at y 14 + (13 - s) * 5; one step per letter, middle C
// = 28, the bottom line E4 (treble) or G2 (bass).
const LETTERS = "CDEFGAB";
function noteStep(abc: string): { step: number; acc: string } {
  const m = /^(\^|_)?([A-Ga-g])([,']*)$/.exec(abc.trim());
  if (!m) throw new Error(`"${abc}" is not one ABC note`);
  let oct = m[2] === m[2].toLowerCase() ? 5 : 4;
  for (const c of m[3]) oct += c === "'" ? 1 : -1;
  return { step: oct * 7 + LETTERS.indexOf(m[2].toUpperCase()), acc: m[1] ?? "" };
}
function noteAbc(step: number, acc = ""): string {
  const oct = Math.floor(step / 7), l = LETTERS[step - oct * 7];
  return acc + (oct >= 5 ? l.toLowerCase() + "'".repeat(oct - 5) : l + ",".repeat(4 - oct));
}
/** A wrong note for `abc`: the one a step higher. */
export function otherNote(abc: string): string {
  const { step, acc } = noteStep(abc);
  return noteAbc(step + 1, acc);
}

/**
 * Place `abc` (one ABC note with its octave) on the staff-note question in
 * `box`: a tap on its line or space, then its accidental. Returns the answer
 * placed. It doesn't submit.
 */
export async function placeNote(box: Locator, abc: string): Promise<string> {
  const svg = box.getByTestId("staff-note-staff");
  const { step, acc } = noteStep(abc);
  const s = step - ((await svg.getAttribute("data-clef")) === "bass" ? 18 : 30);
  const r = await svg.boundingBox();
  if (!r) throw new Error("placeNote: the staff is not on screen");
  await svg.click({ position: { x: (r.width * 112) / 180, y: ((14 + (13 - s) * 5) * r.height) / 118 } });
  for (const [a, id] of [["_", "staff-note-flat"], ["^", "staff-note-sharp"]]) {
    const b = box.getByTestId(id);
    if (((await b.getAttribute("aria-pressed")) === "true") !== (acc === a)) await b.click();
  }
  const placed = noteAbc(step, acc);
  await expect(box).toHaveAttribute("data-answer", placed);
  return placed;
}

/** Which answers to give wrong: the 0-based index of each answer in the round. */
export interface Wrong {
  wrong?: (n: number) => boolean;
}

/**
 * After a deliberately wrong answer to `q`: the feedback `fb` says wrong, and a
 * picked option is marked wrong. A wrong answer taken as right is the bug.
 */
async function expectMarkedWrong(q: Locator, fb: Locator) {
  const id = await q.getAttribute("data-qid");
  await expect(fb, `question ${id}: a wrong answer is marked wrong`).toHaveAttribute("data-state", "wrong");
  if (await q.locator(".option input[type=radio]").count())
    await expect(q.locator('.option[data-state="wrong"]'), `question ${id}: the option picked is marked wrong`).toHaveCount(1);
}

/** Read `slug` (pass its reading check), so that only a run stands between it and passing. */
export async function readLesson(page: Page, key: AnswerKey, slug: string): Promise<void> {
  await page.goto(`/lessons/${slug}`);
  await passReadingCheck(page, key);
  expect((await progressOf(page, slug)).seen, `${slug} is seen`).toBe(true);
}

/** Pass the reading check of the lesson page open in `page` (or press "I've read it" when it has none). */
export async function passReadingCheck(page: Page, key: AnswerKey): Promise<void> {
  const check = page.getByTestId("lesson-check");
  if (!(await check.count())) {
    // No questions to check reading with: the lesson says so with "I've read it" (gone once it is seen).
    const read = page.getByTestId("lesson-read");
    if (await read.count()) {
      await read.first().click();
      await page.waitForLoadState("domcontentloaded");
    }
    return;
  }
  if ((await check.getAttribute("data-done")) === "true") return;
  const questions = page.getByTestId("lesson-check-question");
  for (const q of await questions.all()) await answerQuestion(q, key);
  await page.getByTestId("lesson-check-submit").click();
  await expect(page.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "passed");
}

/** Play the Practice quiz open in `page` to its summary, every answer right but those `o.wrong` picks. Returns [right, total]. */
export async function playPractice(page: Page, key: AnswerKey, o: Wrong = {}): Promise<[number, number]> {
  const summary = page.getByTestId("quiz-summary");
  let right = 0, total = 0;
  for (let i = 0; i < 50 && !(await summary.isVisible()); i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    await expect(q).toHaveCount(1);
    const wrong = o.wrong?.(total) ?? false;
    await answerQuestion(q, key, wrong);
    await q.getByTestId("quiz-check").click();
    const fb = q.getByTestId("quiz-feedback");
    await expect(fb).toHaveAttribute("data-state", /right|wrong/);
    if (wrong) await expectMarkedWrong(q, fb);
    total++;
    if ((await fb.getAttribute("data-state")) === "right") right++;
    await q.getByTestId("quiz-next").click();
  }
  await expect(summary).toBeVisible();
  return [right, total];
}

/** Play the Guide stage open in `page` to its end, every quick check right but those `o.wrong` picks. Returns [right, total]. */
export async function playStage(page: Page, key: AnswerKey, o: Wrong = {}): Promise<[number, number]> {
  const done = page.getByTestId("stage-done");
  let right = 0, total = 0;
  for (let i = 0; i < 100 && !(await done.isVisible()); i++) {
    await expect(page.locator('[data-testid="stage-step"]:visible')).toHaveCount(1);
    // Hold on to this step by its index: the page moves on once the server has the step.
    const index = await page.locator('[data-testid="stage-step"]:visible').getAttribute("data-index");
    const step = page.locator(`[data-testid="stage-step"][data-index="${index}"]`);
    if ((await step.getAttribute("data-kind")) !== "question") {
      // Turn the flashcards over, as a learner would, then go on.
      for (const card of await step.getByTestId("stage-flashcard").all()) await card.click();
      await step.getByTestId("stage-continue").click();
    } else {
      const wrong = o.wrong?.(total) ?? false;
      await answerQuestion(step, key, wrong);
      await step.getByTestId("stage-check").click();
      const fb = step.getByTestId("stage-feedback");
      await expect(fb).toHaveAttribute("data-state", /right|wrong/);
      if (wrong) await expectMarkedWrong(step, fb);
      total++;
      if ((await fb.getAttribute("data-state")) === "right") right++;
      await step.getByTestId("stage-next-step").click();
    }
    await expect(step).toBeHidden();
  }
  await expect(done).toBeVisible();
  return [right, total];
}
