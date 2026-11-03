// Drivers for the games and side pages of spec 7 (issue #48), answering
// from the course files (key.ts) or, for what is only spoken (Numbers by
// ear), from what the fake voice was asked to say. They read the state
// attributes listed in docs/testing.md, never class names.
import { expect, type Page, type TestInfo } from "@playwright/test";
import { Findings } from "./findings.ts";
import type { AnswerKey } from "./key.ts";

/** Which answers a driver gives wrong: the 0-based index of each answer in the round. */
export interface Wrong {
  wrong?: (n: number) => boolean;
}

const lc = (s: string) => s.trim().toLowerCase();
/** Tap the pill (a label around a visually hidden radio) for `value` in the fieldset `testid`. */
const pill = (page: Page, testid: string, value: string) => page.locator(`[data-testid="${testid}"] label:has(input[value="${value}"])`).click();

// ---------- bug signals ----------

/** Exact bug signals on `page` (HTTP and console errors, overflow): call `go` to open a page, `none()` at the end. */
export function watchBugs(page: Page, info: TestInfo) {
  const findings = new Findings("games", info.outputPath());
  findings.watch(page);
  const seen = new Set<string>();
  return {
    findings,
    async go(path: string) {
      const r = await page.goto(path);
      expect(r?.status(), `HTTP status of ${path}`).toBeLessThan(400);
      const key = new URL(page.url()).pathname;
      if (!seen.has(key)) {
        seen.add(key);
        await findings.checkPage(page);
      }
    },
    none() {
      const bugs = findings.list.filter((f) => f.kind === "bug").map((f) => `${f.what} (${f.url})`);
      expect(bugs, "bugs seen (HTTP errors, console errors, overflow)").toEqual([]);
    },
  };
}

/** Remember everything the (fake) voice is asked to say, in window.__spoken. Call before the first goto. */
export async function recordSpeech(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { __spoken: string[]; speechSynthesis: { speak: (u: { text: string }) => void } };
    w.__spoken = [];
    const synth = w.speechSynthesis;
    const speak = synth.speak.bind(synth);
    synth.speak = (u) => {
      w.__spoken.push(u.text);
      speak(u);
    };
  });
}
const spoken = (page: Page) => page.evaluate(() => (window as unknown as { __spoken: string[] }).__spoken.slice());
/** What the voice says next, after `heard` things were said (speech starts on a timer). */
async function nextSpoken(page: Page, heard: number): Promise<string> {
  await expect.poll(async () => (await spoken(page)).length, { message: "something is spoken" }).toBeGreaterThan(heard);
  return (await spoken(page)).at(-1)!;
}

/** The latest History row of `game` ("Date Dash", …): what the server recorded. */
export async function lastHistory(page: Page, game: string): Promise<{ correct: number; total: number }> {
  await page.goto("/history");
  const row = page.locator(`[data-testid="history-game"][data-game="${game}"]`).first();
  await expect(row, `History lists ${game}`).toBeVisible();
  return { correct: Number(await row.getAttribute("data-correct")), total: Number(await row.getAttribute("data-total")) };
}

/** History lists a game row for each of `games` (their names as shown: "Word Match", "Date Dash", …). */
export async function expectHistory(page: Page, games: string[]) {
  await page.goto("/history");
  const shown = await page.getByTestId("history-game").evaluateAll((els) => els.map((e) => e.getAttribute("data-game")));
  for (const g of games) expect(shown, `History lists ${g}`).toContain(g);
}

// ---------- Study mode Listening ----------

/** Play the listening round open in `page` (after Start) to its summary, wrong where `o.wrong` says. Returns [right, total]. */
export async function playListening(page: Page, key: AnswerKey, o: Wrong = {}): Promise<[number, number]> {
  const card = page.getByTestId("listen-card"), fb = page.getByTestId("listen-feedback"), done = page.getByTestId("listen-done");
  await page.getByTestId("listen-start").click();
  let right = 0, total = 0;
  for (let i = 0; i < 60 && !(await done.isVisible()); i++) {
    await expect(fb).toBeHidden();
    const lesson = (await card.getAttribute("data-lesson"))!, index = Number(await card.getAttribute("data-index"));
    const wrong = o.wrong?.(total) ?? false;
    await page.getByTestId("listen-answer").fill(wrong ? "zzz" : key.lesson(lesson).listening[index].text);
    await page.getByTestId("listen-check").click();
    await expect(fb).toBeVisible(); // this answer's verdict, not the last one's (kept while hidden)
    await expect(fb).toHaveAttribute("data-state", wrong ? "wrong" : /right|wrong/);
    total++;
    if ((await fb.getAttribute("data-state")) === "right") right++;
    // Next at once, rather than waiting for the auto-advance.
    if (await card.isVisible()) await page.getByTestId("listen-check").click().catch(() => {});
  }
  await expect(done).toBeVisible();
  return [right, total];
}

// ---------- Word Match ----------

/**
 * Play the Word Match board open in `page`: every pair matched from the course's vocabulary,
 * after a wrong try first for the `o.wrong` picks (n counts the pairs). Returns the number of wrong tries.
 */
export async function playMatching(page: Page, key: AnswerKey, o: Wrong = {}): Promise<number> {
  const pairs = new Map<string, Set<string>>();
  const add = (a: string, b: string) => (pairs.get(lc(a)) ?? pairs.set(lc(a), new Set()).get(lc(a))!).add(lc(b));
  for (const l of key.lessons) for (const v of key.lesson(l.id).vocabulary) { add(v.term, v.meaning); add(v.meaning, v.term); }
  await page.getByTestId("matching-start").click();
  const result = page.getByTestId("matching-result");
  const started = Date.now();
  let wrong = 0, matched = 0, owed = 0;
  for (let i = 0; i < 200 && !(await result.isVisible()); i++) {
    const left = page.locator('[data-testid="matching-col-left"] .tile:not([data-state=matched])');
    const right = page.locator('[data-testid="matching-col-right"] .tile:not([data-state=matched]):not([data-state=wrong])');
    if (!(await left.count())) { await page.waitForTimeout(100); continue; } // the next round is coming
    // Pinned by its id: once matched, "the first unmatched tile" is another one.
    const l = page.locator(`[data-testid="matching-col-left"] .tile[data-id="${await left.first().getAttribute("data-id")}"]`);
    const text = (await l.innerText()).trim();
    const options = await right.allInnerTexts();
    const j = options.findIndex((t) => pairs.get(lc(text))?.has(lc(t)));
    if (j < 0) throw new Error(`Word Match: no partner for "${text}" among ${JSON.stringify(options)}`);
    // The server refuses a game faster than 300 ms a pair (games::MIN_MS_PER_PAIR), so keep a human pace.
    if (o.wrong?.(matched)) owed++;
    if (owed && options.length > 1) {
      owed--;
      // A wrong try: the tile, then a partner that isn't its. Both flash wrong, then unlock.
      await l.click();
      await right.nth(j === 0 ? 1 : 0).click();
      await expect(l, `Word Match: "${text}" with the wrong partner is marked wrong`).toHaveAttribute("data-state", "wrong");
      await expect(l).not.toHaveAttribute("data-state", "wrong");
      wrong++;
    }
    const due = started + ++matched * 320 - Date.now();
    if (due > 0) await page.waitForTimeout(due);
    await l.click();
    await right.nth(j).click();
    if ((await l.getAttribute("data-state", { timeout: 1_000 }).catch(() => "")) === "wrong") wrong++;
  }
  await expect(result).toBeVisible();
  return wrong;
}

// ---------- the paradigm drill ----------

export async function playDrill(page: Page, key: AnswerKey, o: Wrong = {}): Promise<[number, number]> {
  const drill = page.getByTestId("drill"), card = page.getByTestId("drill-card"), fb = page.getByTestId("drill-feedback"), done = page.getByTestId("drill-done");
  const set = (await drill.getAttribute("data-set"))!;
  let right = 0, total = 0;
  await expect(card).toBeVisible();
  for (let i = 0; i < 150 && !(await done.isVisible()); i++) {
    await expect(fb).toBeHidden();
    const item = (await card.getAttribute("data-item"))!, slot = (await card.getAttribute("data-slot"))!;
    const k = key.lessons.flatMap((l) => l.conjugations).find((c) => c.item === item && c.set === set);
    if (!k) throw new Error(`drill: ${item} (${set}) is in no lesson`);
    const wrong = o.wrong?.(total) ?? false;
    await page.getByTestId("drill-answer").fill(wrong ? "zzz" : k.forms[slot]);
    await page.getByTestId("drill-check").click();
    await expect(fb).toBeVisible(); // this answer's verdict, not the last one's (kept while hidden)
    await expect(fb).toHaveAttribute("data-state", wrong ? "wrong" : /right|wrong/);
    total++;
    if ((await fb.getAttribute("data-state")) === "right") right++;
    await page.getByTestId("drill-check").click(); // Next
  }
  await expect(done).toBeVisible();
  return [right, total];
}

// ---------- Numbers by ear ----------

export async function playNumbers(page: Page, o: Wrong = {}): Promise<[number, number]> {
  const fb = page.getByTestId("numbers-feedback"), done = page.getByTestId("numbers-done");
  await page.getByTestId("numbers-count").selectOption("10");
  let heard = (await spoken(page)).length;
  await page.getByTestId("numbers-start").click();
  let right = 0, total = 0;
  for (let i = 0; i < 30 && !(await done.isVisible()); i++) {
    await expect(fb).toBeHidden();
    const n = await nextSpoken(page, heard);
    heard = (await spoken(page)).length;
    const wrong = o.wrong?.(total) ?? false;
    await page.getByTestId("numbers-answer").fill(wrong ? String(Number(n) + 1) : n);
    await page.getByTestId("numbers-check").click();
    await expect(fb).toBeVisible(); // this answer's verdict, not the last one's (kept while hidden)
    await expect(fb).toHaveAttribute("data-state", wrong ? "wrong" : /right|wrong/);
    total++;
    if ((await fb.getAttribute("data-state")) === "right") right++;
    if (!(await done.isVisible())) await page.getByTestId("numbers-check").click().catch(() => {}); // Next
  }
  await expect(done).toBeVisible();
  return [right, total];
}

// ---------- Date Dash ----------

const EN_DAYS = Array.from({ length: 7 }, (_, i) => new Date(Date.UTC(2024, 0, 1 + i)).toLocaleDateString("en", { weekday: "long", timeZone: "UTC" }));
const EN_MONTHS = Array.from({ length: 12 }, (_, m) => new Date(Date.UTC(2024, m, 15)).toLocaleDateString("en", { month: "long", timeZone: "UTC" }));

/** One Date Dash round in drill `mode`: `prompts` answers, right but where `o.wrong` says, then Stop. */
export async function playDates(page: Page, calendar: { days: string[]; months: string[] }, mode: string, prompts: number, o: Wrong = {}): Promise<void> {
  await pill(page, "dates-mode", mode);
  await pill(page, "dates-length", "0");
  await page.getByTestId("dates-start").click();
  const tiles = page.getByTestId("dates-tiles"), prompt = page.getByTestId("dates-prompt");
  for (let n = 0; n < prompts; n++) {
    await expect(tiles.locator(".dash-tile")).not.toHaveCount(0);
    await expect(tiles.locator(".dash-tile[data-state]")).toHaveCount(0);
    const which = (list: string[], word: string) => list.findIndex((w) => lc(w) === lc(word));
    let answer: number;
    const frame = prompt.locator(".dash-frame");
    if (await frame.count()) {
      // Build the date: the weekday first, then the month.
      const [y, m, d] = (await prompt.locator(".cal-page").getAttribute("data-date"))!.split("-").map(Number);
      answer = (await frame.locator(".blank.day").count()) ? (new Date(Date.UTC(y, m - 1, d)).getUTCDay() + 6) % 7 : m - 1;
    } else {
      const word = prompt.locator(".dash-word");
      const lang = await word.getAttribute("lang");
      if (await word.locator(".muted").count()) {
        // "the day after <term>"
        const term = (await word.locator("span[lang]").innerText()).trim();
        const after = /after/.test(await word.locator(".muted").innerText());
        const list = which(calendar.days, term) >= 0 ? calendar.days : calendar.months;
        answer = (which(list, term) + (after ? 1 : list.length - 1)) % list.length;
      } else {
        const text = (await word.innerText()).trim();
        if (lang === "en") answer = which(EN_DAYS, text) >= 0 ? which(EN_DAYS, text) : which(EN_MONTHS, text);
        else answer = which(calendar.days, text) >= 0 ? which(calendar.days, text) : which(calendar.months, text);
      }
    }
    if (answer < 0) throw new Error(`Date Dash: can't read the prompt ${JSON.stringify(await prompt.innerText())}`);
    const wrong = o.wrong?.(n) ?? false;
    const tile = wrong ? tiles.locator(`.dash-tile:not([data-i="${answer}"])`).first() : tiles.locator(`.dash-tile[data-i="${answer}"]`);
    await tile.click();
    await expect(tile, `Date Dash (${mode}) prompt ${n + 1}`).toHaveAttribute("data-state", wrong ? "wrong" : "right");
  }
  await page.getByTestId("dates-stop").click();
  await expect(page.getByTestId("dates-done")).toBeVisible();
}

// ---------- flashcards ----------

/** Turn over every flashcard on /vocabulary/study and mark it known, or not where `o.wrong` says. Returns how many. */
export async function playFlashcards(page: Page, o: Wrong = {}): Promise<number> {
  const done = page.getByTestId("flashcards-done");
  let n = 0;
  for (let i = 0; i < 100 && !(await done.isVisible()); i++) {
    await page.locator('[data-testid="flashcards-card"]:visible').click();
    await page.getByTestId(o.wrong?.(n) ? "flashcards-no" : "flashcards-yes").click();
    n++;
  }
  await expect(done).toBeVisible();
  return n;
}
