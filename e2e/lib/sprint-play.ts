// Play a Sprint mode session from the answer key (key.ts): the stage says which
// exercise is up (`data-kind`, `data-exercise-id`), the key says what is
// right, this taps it, or, for the exercises `wrong` picks, taps something
// wrong. Unlike playSprintSession in app.ts, an answer the app grades
// otherwise than meant is a failure of the app (or of the key), so it is
// reported, not tapped around.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, type Locator, type Page } from "@playwright/test";
import { fillTheBar } from "./answer.ts";
import { spellChord } from "./answer.ts";
import { otherNote, placeNote } from "./answer.ts";
import { pressKey } from "./keyboard.ts";
import { setClock } from "./clock.ts";
import { bare, WRONG_TYPED, type AnswerKey } from "./key.ts";

/** Every exercise kind Sprint mode has, from src/test_course.rs (ALL_KINDS), so a new kind can't be missed. */
export const ALL_KINDS: string[] = (() => {
  const src = readFileSync(fileURLToPath(new URL("../../src/test_course.rs", import.meta.url)), "utf8");
  const list = /const ALL_KINDS: \[&str; \d+\] = \[([^\]]*)\]/.exec(src);
  if (!list) throw new Error("ALL_KINDS not found in src/test_course.rs");
  return [...list[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
})();

export interface SprintRun {
  /**
   * Exercise kinds met, named as `kind()` in src/test_course.rs names them
   * (`word-tiles`, `number-tiles`, `typed-math`, `pick-term`, `conversation-build`, …),
   * plus `type-instead` when "Type it instead" was used.
   */
  kinds: Set<string>;
  answered: number;
  /** Answers the app marked right. */
  right: number;
  wrong: string[]; // "<kind> <id>: <verdict>" for each answer meant right that the app marked wrong
  /** Kinds answered wrong on purpose (`SprintOptions.wrong`) and marked wrong. */
  wrongKinds: Set<string>;
  /** "<kind> <id>" for each answer meant wrong that the app marked right: the false positives. */
  takenRight: string[];
}

export interface SprintOptions {
  /** Answer the first tiles exercise that offers it by "Type it instead". */
  typeOnce?: boolean;
  maxSteps?: number;
  /** Stop after this many answers instead of at the done screen (lightning: the clock ends it). */
  answers?: number;
  /**
   * Answer wrong where this says so: `n` counts the graded exercises so far, `kind`
   * is the fine kind (SprintRun.kinds). Sort and match go wrong by one slip on the way, a ladder by
   * one form in the wrong place; a build of one word with no other tile can't be got wrong and is answered right.
   */
  wrong?: (n: number, kind: string) => boolean;
}

const lc = (s: string) => s.trim().toLowerCase();

/** The index of the first of `els` whose text (or data-value) is `want`: exact first, then ignoring case. */
async function find(els: Locator, want: string, attr?: string): Promise<number> {
  const texts = await els.evaluateAll((list, a) => list.map((e) => (a ? e.getAttribute(a) : null) ?? (e as HTMLElement).innerText), attr);
  const clean = texts.map((t) => (t ?? "").replace(/^\d+\s*/, "").trim()); // options start with their key number
  let i = clean.findIndex((t, j) => t === want || texts[j] === want);
  if (i < 0) i = clean.findIndex((t) => lc(t) === lc(want));
  return i;
}

async function tap(els: Locator, want: string, what: string, attr?: string) {
  const i = await find(els, want, attr);
  if (i < 0) throw new Error(`Sprint mode: no ${what} "${want}" among ${JSON.stringify(await els.allInnerTexts())}`);
  await els.nth(i).click();
}

/** Tap the first of `els` that is not `want` (by text or `attr`). False when there is none. */
async function tapOther(els: Locator, want: string, attr?: string): Promise<boolean> {
  const texts = await els.evaluateAll((list, a) => list.map((e) => (a ? e.getAttribute(a) : null) ?? (e as HTMLElement).innerText), attr);
  const i = texts.findIndex((t) => lc(bare((t ?? "").replace(/^\d+\s*/, ""))) !== lc(bare(want)) && lc(t ?? "") !== lc(want));
  if (i < 0) return false;
  await els.nth(i).click();
  return true;
}

/** The exercise's kind as src/test_course.rs names it: the stage's kind, told apart further by its id and tiles. */
async function fineKind(stage: Locator, kind: string, id: string, key: AnswerKey): Promise<string> {
  if (kind === "tiles") {
    // A word gap is word-tiles even when too few tiles were found and it is typed (issue #678).
    if (key.question(id).type === "fill-in-blank") return "word-tiles";
    return (await stage.locator(".sprint-bank .sprint-tile").count()) ? "number-tiles" : "typed-math";
  }
  if (kind === "clock") return key.question(id).options.length ? "read-clock" : "set-clock";
  if (kind === "pick") return id.startsWith("pick:s:") ? "pick-term" : "pick-meaning";
  if (kind === "build" && id.startsWith("cbuild:")) return "conversation-build";
  if (kind === "hear-pick" && id.startsWith("hmean:")) return "hear-meaning";
  return kind;
}

/** Play the session open in `page` to its done screen, answering every exercise right. */
export function playSprintRight(page: Page, key: AnswerKey, o: SprintOptions = {}): Promise<SprintRun> {
  return playSprint(page, key, o);
}

/** Play the session open in `page` to its done screen: right, except where `o.wrong` says wrong. */
export async function playSprint(page: Page, key: AnswerKey, o: SprintOptions = {}): Promise<SprintRun> {
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  const next = page.getByTestId("sprint-play-next");
  const result = page.getByTestId("sprint-play-result");
  const done = page.getByTestId("sprint-play-done");
  const run: SprintRun = { kinds: new Set(), answered: 0, right: 0, wrong: [], wrongKinds: new Set(), takenRight: [] };
  let typed = !o.typeOnce;
  let last = "";
  let meant: { where: string; kind: string; wrong: boolean } | null = null; // the answer on the sheet

  for (let step = 0; step < (o.maxSteps ?? 200); step++) {
    if (await done.isVisible()) return run;
    if (o.answers !== undefined && run.answered >= o.answers) return run;
    if (await result.isVisible()) {
      const state = await result.getAttribute("data-state");
      if (state === "right") run.right++;
      if (meant?.wrong) {
        if (state === "right") run.takenRight.push(meant.where);
        else run.wrongKinds.add(meant.kind);
      } else if (state === "wrong") {
        run.wrong.push(`${await stage.getAttribute("data-kind")} ${await stage.getAttribute("data-exercise-id")}: ${await page.getByTestId("sprint-play-verdict").innerText()}`);
      }
      meant = null;
      await next.click();
      continue;
    }
    const kind = (await stage.getAttribute("data-kind")) ?? "";
    const id = (await stage.getAttribute("data-exercise-id")) ?? "";
    const pos = (await stage.getAttribute("data-pos")) ?? "";
    if (!kind || `${pos}:${id}` === last) {
      // Nothing new yet (a request or an animation in flight).
      await page.waitForTimeout(100);
      continue;
    }
    last = `${pos}:${id}`;
    const fine = await fineKind(stage, kind, id, key);
    run.kinds.add(fine);
    const a = key.sprint(kind, id);
    const where = `${kind} ${id}`;
    let wrong = a.kind !== "none" && (o.wrong?.(run.answered, fine) ?? false);
    try {
      switch (a.kind) {
        case "none":
          await check.click();
          continue; // a bite moves on by itself
        case "option":
          if (wrong) wrong = await tapOther(stage.locator(".sprint-opt, .sprint-hear-opt"), a.value, "data-value");
          if (!wrong) await tap(stage.locator(".sprint-opt, .sprint-hear-opt"), a.value, "option", "data-value");
          await check.click();
          break;
        case "clock":
          await setClock(stage.getByTestId("clock"), wrong ? key.wrong(id) : a.value);
          await check.click();
          break;
        case "tile": {
          const tiles = stage.locator(".sprint-bank .sprint-tile:visible");
          const typeLink = stage.locator(".sprint-link:not(.sprint-cant)");
          if (!wrong && !typed && (await typeLink.count())) {
            typed = true;
            run.kinds.add("type-instead");
            await typeLink.click();
          }
          if (fine === "word-tiles") {
            // A gap's tiles are words: no true/false answer among them (issue #410).
            const q = key.question(id);
            const lesson = key.lesson(q.lesson);
            const words = new Set(lesson.quiz.filter((x) => x.type === "fill-in-blank").map((x) => lc(x.correct_answer)));
            const tf = new Set(lesson.quiz.filter((x) => x.type === "true-false").map((x) => lc(x.correct_answer)).filter((x) => !words.has(x)));
            const odd = (await tiles.allInnerTexts()).filter((t) => tf.has(lc(t)));
            if (odd.length) throw new Error(`true/false answers among the gap tiles: ${JSON.stringify(odd)}`);
          }
          if (await tiles.count()) {
            if (wrong) wrong = await tapOther(tiles, a.value);
            if (!wrong) await tap(tiles, a.value, "tile");
          } else {
            const typedWrong = id.includes(":") ? WRONG_TYPED["fill-in-blank"] : (WRONG_TYPED[key.question(id).type] ?? WRONG_TYPED["fill-in-blank"]);
            await stage.locator("input.sprint-input").fill(wrong ? typedWrong : a.value);
          }
          await check.click();
          break;
        }
        case "words": {
          const bank = stage.locator(".sprint-bank .sprint-tile");
          if (fine === "hear-build") {
            // Tap what you hear: no tile keeps the sentence's punctuation (issue #413).
            const marked = (await bank.allInnerTexts()).filter((t) => bare(t.trim()) !== t.trim());
            if (marked.length) throw new Error(`punctuation on the word tiles: ${JSON.stringify(marked)}`);
          }
          const reversed = a.words.slice().reverse();
          if (wrong && reversed.map(lc).join(" ") !== a.words.map(lc).join(" ")) {
            for (const w of reversed) await tap(bank, w, "word tile");
          } else if (wrong) {
            wrong = await tapOther(bank, a.words[0]); // one word: a tile that isn't it
          }
          if (!wrong) for (const w of a.words) await tap(bank, w, "word tile");
          await check.click();
          break;
        }
        case "bar":
          await fillTheBar(stage.getByTestId("fill-bar"), a.tiles, wrong);
          await check.click();
          break;
        case "chord":
          await spellChord(stage.getByTestId("chord-spell"), a.notes, wrong);
          await check.click();
          break;
        case "staff":
          await placeNote(stage.getByTestId("staff-note"), wrong ? otherNote(a.abc) : a.abc);
          await check.click();
          break;
        case "key":
          await pressKey(stage.getByTestId("keyboard"), a.answers, a.exact, wrong);
          await check.click();
          break;
        case "spot": {
          const i = (await stage.locator(".sprint-word").allInnerTexts()).findIndex((t) => (lc(bare(t)) === lc(a.wrong)) !== wrong);
          if (i < 0) throw new Error(`no word "${a.wrong}" in the sentence`);
          await stage.locator(".sprint-word").nth(i).click();
          await check.click();
          break;
        }
        case "note": {
          // A spot on a staff (#1227): tap the note (wrong: the one after it), then the fix tile.
          const notes = stage.getByTestId("wrong-note-note");
          await notes.nth(wrong ? (a.index + 1) % (await notes.count()) : a.index).click({ force: true }); // staff lines cross the note; abcjs finds the nearest
          await tap(stage.getByTestId("wrong-note-tile"), a.fix, "fix tile", "data-value");
          await check.click();
          break;
        }
        case "sort":
          // One item at a time, each tap judged at once (#1148); the sort checks itself once the pile is empty.
          // Wrong: one tap on a wrong bucket first (the word shakes and stays on the pile).
          for (let k = 0; k < a.bucketOf.size; k++) {
            const card = stage.locator(".sprint-sort-item");
            const speaker = card.locator("[data-play]");
            const text = (await speaker.count()) ? (await speaker.getAttribute("data-play"))! : (await card.innerText()).trim();
            const b = a.bucketOf.get(text);
            if (b === undefined) throw new Error(`sort item "${text}" is not in the key`);
            if (wrong && k === 0) await stage.locator(".sprint-bucket").nth(b === 0 ? 1 : 0).click();
            await stage.locator(".sprint-bucket").nth(b).click();
          }
          break;
        case "ladder": {
          // Each tap fills the glowing rung; Check once every rung is filled (#640).
          // Wrong: the first rung gets another form (swapped with the rung it belongs on).
          const bank = stage.locator(".sprint-bank .sprint-tile:not([data-state=used])");
          const order = a.forms.slice();
          if (wrong) {
            const j = order.findIndex((f) => lc(f) !== lc(order[0]));
            if (j < 0) wrong = false; // every form the same: nothing can go on a wrong rung
            else [order[0], order[j]] = [order[j], order[0]];
          }
          for (const f of order) await tap(bank, f, "form");
          await check.click();
          break;
        }
        case "pairs":
        case "same": {
          const left = stage.locator(".sprint-match .col").first().locator(".sprint-tile");
          const right = stage.locator(".sprint-match .col").nth(1).locator(".sprint-tile:not([data-state=done])");
          const n = await left.count();
          for (let k = 0; k < n; k++) {
            const t = left.nth(k);
            const term = a.kind === "same" ? (await t.getAttribute("data-value"))! : (await t.innerText()).trim();
            const want = a.kind === "same" ? term : a.meaningOf.get(term);
            if (want === undefined) throw new Error(`match: "${term}" is not in the key`);
            if (wrong && k === 0) {
              // One mix-up first: the term, then a partner that isn't its.
              await t.click();
              wrong = await tapOther(right, want, a.kind === "same" ? "data-value" : undefined);
            }
            await t.click();
            await tap(right, want, "match tile");
          }
          break;
        }
      }
    } catch (e) {
      throw new Error(`Sprint mode, ${where}: ${(e as Error).message}`);
    }
    run.answered++;
    meant = { where, kind: fine, wrong };
    await expect(result, `the answer sheet after ${where}`).toBeVisible();
  }
  throw new Error(`Sprint mode session did not end in ${o.maxSteps ?? 200} steps (last: ${last})`);
}
