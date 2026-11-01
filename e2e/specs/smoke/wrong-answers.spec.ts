// Smoke spec 8, the other half of 1–7: wrong answers. The other smoke specs
// answer everything right, so an app that took every answer as right would
// pass them. Here a new learner answers wrong on purpose, on the test course,
// in every question type, every Sprint exercise kind, the Guide, the reading check
// and every game, and each wrong answer must be marked wrong, counted wrong
// where the score is saved, and keep the lesson from passing.
//
// The scores of practice, Sprint mode, the Guide and the games are tallied in the
// browser and posted at the end, so the server's record (/api/progress,
// History) is checked against the answers given here, not only the screen.
//
// This file has Study (practice, the reading check) and Sprint mode; the Guide,
// the games, flashcards and the Tutor are in wrong-answers-games.spec.ts, so
// the two halves can run in different CI shards.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";
import { TEST_COURSE } from "../../lib/app.ts";
import { answerQuestion, passReadingCheck, playPractice, readLesson } from "../../lib/answer.ts";
import { ALL_KINDS, playSprint } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { lastHistory } from "../../lib/games.ts";
import { AnswerKey } from "../../lib/key.ts";
import { apiProgress, newAccount, progressOf, register } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);

/** Every quiz type, from src/content/quiz_type.rs; the test course has a lesson named by each slug. */
const QUESTION_TYPES: string[] = (() => {
  const src = readFileSync(fileURLToPath(new URL("../../../src/content/quiz_type.rs", import.meta.url)), "utf8");
  const types = [...src.matchAll(/QuizType::\w+ => "([a-z-]+)",/g)].map((m) => m[1]);
  if (!types.length) throw new Error("no QuizType slugs found in src/content/quiz_type.rs");
  return [...new Set(types)];
})();

/** The pass rule (docs/progress.md, src/progress.rs): seen, plus a run of 70 % over enough answers. */
const PASS_PCT = 70;
/** The Sprint exercise kinds there is an answer to (a bite is only read). */
const GRADED_KINDS = ALL_KINDS.filter((k) => k !== "bite");
const pct = (right: number, total: number) => (total ? Math.floor((right * 100) / total) : 100);

test.use({ reducedMotion: "reduce" });

const read = (page: Page, slug: string) => readLesson(page, key, slug);

test("K: every question type, answered wrong, is marked wrong and doesn't pass", async ({ page }) => {
  test.setTimeout(120_000);
  await register(page, newAccount("wrong-study"), TEST_COURSE);
  expect(QUESTION_TYPES.length).toBeGreaterThanOrEqual(5);
  for (const slug of QUESTION_TYPES) {
    await read(page, slug);
    await page.goto(`/practice?lesson=${slug}`);
    const [right, total] = await playPractice(page, key, { wrong: () => true });
    expect(total, `${slug}: questions asked`).toBeGreaterThanOrEqual(3);
    expect(right, `${slug}: answers taken as right`).toBe(0);
    const summary = page.getByTestId("quiz-summary");
    await expect(summary).toHaveAttribute("data-correct", "0");
    await expect(summary).toHaveAttribute("data-total", String(total));

    const l = await progressOf(page, slug);
    expect(l.best_pct, `${slug}: the run as saved`).toBe(0);
    expect(l.passed, `${slug}: passed after an all-wrong run`).toBe(false);
    await page.goto("/history");
    expect(await page.locator(`[data-testid="history-answer"][data-lesson-id="${slug}"][data-correct="false"]`).count(), `${slug}: History's wrong answers`).toBeGreaterThanOrEqual(total);
    await page.goto("/lessons?unit=all");
    await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${slug}"]`)).toHaveAttribute("data-passed", "false");

    // A right run afterwards passes: what kept it back was the answers.
    await page.goto(`/practice?lesson=${slug}`);
    const [r2, t2] = await playPractice(page, key);
    expect(r2, `${slug}: the right run`).toBe(t2);
    expect((await progressOf(page, slug)).passed, `${slug}: passed after a right run`).toBe(true);
  }
});

test("K: one lesson answered wrong reads as \"1 lesson needs practice\" on the course home and map", async ({ page }) => {
  await register(page, newAccount("wrong-home"), TEST_COURSE);
  const slug = "multiple-choice";
  await read(page, slug);
  await page.goto(`/practice?lesson=${slug}`);
  const [right] = await playPractice(page, key, { wrong: () => true });
  expect(right).toBe(0);
  await page.goto("/");
  const stat = page.getByTestId("home-study-stat");
  await expect(stat).toHaveAttribute("data-weak", "1");
  // The singular form (#957): "1 lesson needs practice", not "1 lesson need practice".
  await expect(stat).toContainText("1 lesson needs practice");

  // The course map's count line under the unit heading, the same slip (#990).
  await page.goto("/map");
  const counts = page.getByTestId("map-unit")
    .filter({ has: page.locator(`[data-testid="map-lesson"][data-lesson-id="${slug}"]`) })
    .getByTestId("map-unit-counts");
  await expect(counts).toHaveAttribute("data-weak", "1");
  await expect(counts).toContainText("1 needs practice");
});

test("K: the 70 % line, 3 of 5 doesn't pass and 4 of 5 does", async ({ page }) => {
  await register(page, newAccount("wrong-line"), TEST_COURSE);
  const slug = "fill-in-blank";
  await read(page, slug);

  await page.goto(`/practice?lesson=${slug}`);
  const [r1, t1] = await playPractice(page, key, { wrong: (n) => n < 2 });
  expect([r1, t1], "3 of 5 right").toEqual([3, 5]);
  let l = await progressOf(page, slug);
  expect(l.best_pct).toBe(60);
  expect(l.passed, "60 % passes").toBe(false);

  await page.goto(`/practice?lesson=${slug}`);
  const [r2, t2] = await playPractice(page, key, { wrong: (n) => n < 1 });
  expect([r2, t2], "4 of 5 right").toEqual([4, 5]);
  l = await progressOf(page, slug);
  expect(l.best_pct).toBe(80);
  expect(l.passed, "80 % passes").toBe(true);
});

test("near misses are wrong, and the answers they nearly are stay right", async ({ page }) => {
  await register(page, newAccount("wrong-near"), TEST_COURSE);
  const answer = async (question_id: string, given: string) => {
    const r = await page.request.post("/api/answer", { data: { question_id, answer: given } });
    expect(r.ok(), `POST /api/answer ${question_id}`).toBe(true);
    return (await r.json()) as { correct: boolean; note?: string | null };
  };
  const cases: [string, string, boolean][] = [
    ["tc-fill-in-blank-01", "esta", false], // the accent missing
    ["tc-fill-in-blank-01", "está", true],
    ["tc-numeric-03", "12.6", false], // outside the 0.01 tolerance
    ["tc-numeric-03", "12.575", true],
    ["tc-numeric-04", "2", false], // half of the list
    ["tc-numeric-04", "-3; 2", true], // any order
    ["tc-numeric-05", "30", false],
    ["tc-numeric-05", "29", true], // accepted_answers
    ["tc-numeric-02", "0.7", false],
    ["tc-numeric-02", "0.75", true],
    ["tc-expression-01", "x^2 + 2x", false],
    ["tc-expression-01", "(x+1)(x+1)", true],
    ["tc-true-false-01", "false", false],
    ["tc-multiple-choice-01", "dog", false],
  ];
  const off: string[] = [];
  for (const [id, given, right] of cases) if ((await answer(id, given)).correct !== right) off.push(`${id} "${given}": should be ${right ? "right" : "wrong"}`);
  expect(off, "answers graded the wrong way").toEqual([]);
  expect((await answer("tc-fill-in-blank-01", "esta")).note ?? "", "the accent near miss says why").toMatch(/accent/i);
});

test("a reading check with one wrong answer fails, the lesson stays unread, and a retry passes", async ({ page }) => {
  await register(page, newAccount("wrong-check"), TEST_COURSE);
  const slug = "multiple-choice";
  await page.goto(`/lessons/${slug}`);
  const questions = await page.getByTestId("lesson-check-question").all();
  expect(questions.length, "the check asks two questions").toBe(2);
  await answerQuestion(questions[0], key);
  await answerQuestion(questions[1], key, true);
  await page.getByTestId("lesson-check-submit").click();
  await expect(page.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "failed");
  await expect(page.getByTestId("lesson-check-feedback").nth(0)).toHaveAttribute("data-state", "right");
  await expect(page.getByTestId("lesson-check-feedback").nth(1)).toHaveAttribute("data-state", "wrong");

  // "Try two more questions" reloads the page with the check offered again.
  await page.getByTestId("lesson-check-retry").click();
  await page.waitForLoadState("domcontentloaded");
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "false");
  expect((await progressOf(page, slug)).seen, "seen after a failed check").toBe(false);
  await passReadingCheck(page, key);
  await page.reload();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "true");
});

test("D: every exercise kind, answered wrong, is marked wrong, and no lesson passes on wrong answers", async ({ page }) => {
  test.setTimeout(300_000);
  await page.setViewportSize({ width: 390, height: 844 });
  await register(page, newAccount("wrong-sprint"), TEST_COURSE);
  const wrongKinds = new Set<string>();
  const takenRight: string[] = [];
  const badTally: string[] = [];
  const passedWrongly: string[] = [];

  const play = async (url: string, what: string) => {
    await page.goto(url);
    const run = await playSprint(page, key, { wrong: () => true });
    run.wrongKinds.forEach((k) => wrongKinds.add(k));
    takenRight.push(...run.takenRight.map((x) => `${what}: ${x}`));
    const done = page.getByTestId("sprint-play-done");
    await expect(done, `${what}: the session is saved`).toHaveAttribute("data-saved", "true");
    // The score sent is the answers the app showed: right ones only where nothing could be got wrong.
    const sent = [Number(await done.getAttribute("data-correct")), Number(await done.getAttribute("data-answered"))];
    if (sent[0] !== run.right || sent[1] !== run.answered) badTally.push(`${what}: sent ${sent[0]}/${sent[1]}, the sheets said ${run.right}/${run.answered}`);
    return run;
  };

  for (const l of await apiProgress(page)) {
    const run = await play(`/sprint/play?lesson=${l.slug}&reduce-motion=1`, l.slug);
    if (l.slug === "sort" || l.slug === "conjugations") {
      // History reads a sort or a ladder as a score (#425), not "slip(s)" or a count in the answer's place.
      await page.goto("/history");
      const rows = page.locator(`[data-testid="history-answer"][data-lesson-id="${l.slug}"]`);
      // A sort judges each tap (#1148): answered wrong, it ends all sorted after a wrong tap, and says so first.
      if (l.slug === "sort") await expect(rows.and(page.locator('[data-correct="false"]')).filter({ hasText: /\d+ wrong taps?, then all \d+ sorted correctly/ }).first()).toBeVisible();
      if (l.slug === "conjugations") await expect(rows.filter({ hasText: /\d+ of \d+ forms right/ }).first()).toBeVisible();
      // A ladder marked ✗ never reads "3 of 3 forms right" (#640): its words agree with its mark.
      for (const text of await rows.and(page.locator('[data-correct="false"]')).filter({ hasText: /Ladder:/ }).allInnerTexts()) {
        const m = /(\d+) of (\d+) forms right/.exec(text);
        expect(m && m[1] !== m[2], `${l.slug}: a wrong ladder row reads "${text}"`).toBe(true);
      }
      // A sort or ladder has no single right answer to show: its result cell is a bare mark, not "✗ (8 items)".
      const marks = await rows.filter({ hasText: /Sort it:|Ladder:/ }).locator("td:last-child").allInnerTexts();
      expect(marks.length, `${l.slug}: sort/ladder rows`).toBeGreaterThan(0);
      for (const cell of marks) expect(cell.trim(), `${l.slug}: result cell`).toMatch(/^[✓✗]$/);
      await expect(rows.filter({ hasText: /slip|rungs|\(\d+ items\)|\(all forms\)/ })).toHaveCount(0);
    }
    if (!run.answered || pct(run.right, run.answered) >= PASS_PCT || !(await progressOf(page, l.slug)).passed) continue;
    // By design (docs/progress.md): a lesson with no quiz questions passes once seen, whatever its Sprint exercises scored.
    if (key.lesson(l.slug).quiz.length) passedWrongly.push(`${l.slug} (${run.right}/${run.answered})`);
  }
  // Some kinds come only in the review mix and lightning (a term's meaning, at random): a few rounds more while one is missing.
  // Which kinds a seeded session draws depends on every draw the shard's server made
  // before, so a kind can hide for a few rounds; eight rounds have always shown them all.
  for (let n = 0; n < 8 && GRADED_KINDS.some((k) => !wrongKinds.has(k)); n++) {
    await play(`/sprint/play?mode=review&reduce-motion=1`, `review ${n + 1}`);
  }

  expect(takenRight, "wrong answers the app marked right").toEqual([]);
  expect(badTally, "scores sent that differ from the answers").toEqual([]);
  expect(passedWrongly, "lessons passed on a run under 70 %").toEqual([]);
  expect(GRADED_KINDS.filter((k) => !wrongKinds.has(k)), "D exercise kinds never answered wrong").toEqual([]);
});

test("D: the session's score is the answers given, on the screen, in History and in progress", async ({ page }) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 390, height: 844 });
  await register(page, newAccount("wrong-dmix"), TEST_COURSE);
  const slug = "fill-in-blank";
  await page.goto(`/sprint/play?lesson=${slug}&reduce-motion=1`);
  const run = await playSprint(page, key, { wrong: (n) => n % 2 === 1 });
  expect(run.takenRight, "wrong answers the app marked right").toEqual([]);
  expect(run.wrong, "right answers the app marked wrong").toEqual([]);
  expect(run.answered).toBeGreaterThanOrEqual(3);
  expect(run.right).toBeLessThan(run.answered);
  const done = page.getByTestId("sprint-play-done");
  await expect(done).toHaveAttribute("data-saved", "true");
  await expect(done).toHaveAttribute("data-correct", String(run.right));
  await expect(done).toHaveAttribute("data-answered", String(run.answered));
  expect(await lastHistory(page, "Sprint mode"), "History's Sprint mode row").toEqual({ correct: run.right, total: run.answered });
  const l = await progressOf(page, slug);
  expect(l.best_pct, "the run as saved").toBe(pct(run.right, run.answered));
  expect(l.passed).toBe(pct(run.right, run.answered) >= PASS_PCT);
});
