// Smoke spec 8, second half: wrong answers in the Guide, the games, the
// flashcards and the Tutor. Study and Sprint mode are in
// wrong-answers.spec.ts, which says what the spec is for.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";
import { TEST_COURSE } from "../../lib/app.ts";
import { playStage, readLesson } from "../../lib/answer.ts";
import { playSprint } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { lastHistory, playDates, playDrill, playFlashcards, playListening, playMatching, playNumbers, recordSpeech, watchBugs } from "../../lib/games.ts";
import { AnswerKey } from "../../lib/key.ts";
import { newAccount, progressOf, register, resetServerKeys } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);
const course = JSON.parse(readFileSync(fileURLToPath(new URL("../../../data/test-courses/test-course/course.json", import.meta.url)), "utf8"));

const pct = (right: number, total: number) => (total ? Math.floor((right * 100) / total) : 100);

test.use({ reducedMotion: "reduce" });

const read = (page: Page, slug: string) => readLesson(page, key, slug);

test("Guide: a stage of wrong quick checks isn't passed and the next stays locked; a replay passes it", async ({ page }) => {
  await register(page, newAccount("wrong-play"), TEST_COURSE);
  await page.goto("/guide");
  const first = (await page.locator('[data-testid="guide-stage"]').first().getAttribute("data-lesson-id"))!;
  const second = (await page.locator('[data-testid="guide-stage"]').nth(1).getAttribute("data-lesson-id"))!;

  await page.goto(`/guide/${first}`);
  const [right, total] = await playStage(page, key, { wrong: () => true });
  expect(total, "quick checks asked").toBeGreaterThan(0);
  expect(right, "answers taken as right").toBe(0);
  const done = page.getByTestId("stage-done");
  await expect(done).toHaveAttribute("data-correct", "0");
  await expect(done).toHaveAttribute("data-answered", String(total));
  await expect(done).toHaveAttribute("data-passed", "false");
  await expect(page.getByTestId("stage-next")).toBeHidden();
  // "Play again" restarts the stage on the server, then reloads it at its first
  // step; wait for that reload, or it can interrupt the goto below.
  const reloaded = page.waitForEvent("load");
  await page.getByTestId("stage-replay").click();
  await reloaded;
  expect((await progressOf(page, first)).passed).toBe(false);
  await page.goto("/guide");
  await expect(page.locator(`[data-testid="guide-stage"][data-lesson-id="${second}"]`)).toHaveAttribute("data-state", "locked");

  // Play it again, right this time: passed, and the next stage opens.
  await page.goto(`/guide/${first}`);
  const [r2, t2] = await playStage(page, key);
  expect(r2).toBe(t2);
  await expect(page.getByTestId("stage-done")).toHaveAttribute("data-passed", "true");
  await page.goto("/guide");
  await expect(page.locator(`[data-testid="guide-stage"][data-lesson-id="${second}"]`)).not.toHaveAttribute("data-state", "locked");

  // Reopened without "Play again" (#490): it starts at step 1, the first tap
  // goes on to step 2, and the whole run plays and counts again.
  await page.goto(`/guide/${first}`);
  await expect(page.locator('[data-testid="stage-step"]:visible')).toHaveAttribute("data-index", "0");
  const [r3, t3] = await playStage(page, key);
  expect([r3, t3]).toEqual([t2, t2]);
  await expect(page.getByTestId("stage-done")).toHaveAttribute("data-answered", String(t2));
  await expect(page.getByTestId("stage-done")).toHaveAttribute("data-passed", "true");
});

test("Word Match: wrong tries are counted and keep the lesson from passing", async ({ page }, info) => {
  const bugs = watchBugs(page, info);
  await register(page, newAccount("wrong-match"), TEST_COURSE);
  const slug = "vocabulary";
  await read(page, slug);
  const pairs = key.lesson(slug).vocabulary.length;
  const tries = Math.floor((pairs * 3) / 7) + 1; // just enough to fall under 70 %
  await bugs.go(`/play/matching/play?lesson=${slug}`);
  expect(await playMatching(page, key, { wrong: (n) => n < tries }), "wrong tries made").toBe(tries);
  await expect(page.getByTestId("matching-result")).toContainText(`${tries} wrong`);
  const l = await progressOf(page, slug);
  expect(l.best_pct, "the board as a run: every pair right, every wrong try a miss").toBe(pct(pairs, pairs + tries));
  expect(l.passed).toBe(false);
  bugs.none();
});

test("the games: a wrong answer is marked wrong and History counts it", async ({ page }, info) => {
  test.setTimeout(180_000);
  await recordSpeech(page);
  const bugs = watchBugs(page, info);
  await register(page, newAccount("wrong-games"), TEST_COURSE);
  const firstWrong = { wrong: (n: number) => n === 0 };
  const expectOneMiss = async (game: string, played: [number, number] | null) => {
    const h = await lastHistory(page, game);
    expect(h.total, `${game}: answers recorded`).toBeGreaterThan(1);
    expect(h.correct, `${game}: the one wrong answer counted`).toBe(h.total - 1);
    if (played) expect([h.correct, h.total], `${game}: History agrees with the round`).toEqual(played);
  };

  await bugs.go("/listening/play?lesson=listening");
  await expectOneMiss("Listening", await playListening(page, key, firstWrong));

  await page.request.post("/api/progress/pass", { data: { lesson: "conjugations", answered: 5, correct: 5, evidence: "wrong-answers spec: unlocks the drill" } });
  await bugs.go("/play/conjugation");
  await page.getByTestId("drill-setup-start").click();
  await expectOneMiss(`${course.paradigm.item_label} Drill`, await playDrill(page, key, firstWrong));

  await bugs.go("/play/numbers");
  await expectOneMiss("Numbers by ear", await playNumbers(page, firstWrong));

  await bugs.go("/play/dates");
  await playDates(page, course.calendar, "days", 4, firstWrong);
  await expectOneMiss("Date Dash", null);

  bugs.none();
});

test("flashcards: a card marked not known comes back in the missed deck", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await register(page, newAccount("wrong-cards"), TEST_COURSE);
  // Words to study: collect them in a Sprint session.
  await page.goto("/sprint/play?lesson=vocabulary&reduce-motion=1");
  await playSprint(page, key);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/vocabulary/study");
  const n = await playFlashcards(page, { wrong: (i) => i === 0 });
  expect(n).toBeGreaterThan(1);
  await page.getByTestId("flashcards-missed").click();
  await expect(page.getByTestId("flashcards"), "the missed deck is the one card").toHaveAttribute("data-total", "1");
});

test("Tutor: a wrong answer gets a wrong verdict", async ({ page }) => {
  await register(page, newAccount("wrong-tutor"), TEST_COURSE);
  // The server's keys serve every account (#25): the simulated provider's, answered from the fixtures.
  await resetServerKeys(page);

  await page.goto("/tutor?lesson=everything");
  const ex = page.locator('[data-testid="tutor-exercise"]:not([data-state=done])');
  await expect(ex).toHaveCount(1, { timeout: 15_000 });
  // A wrong fill-in typed into the chat goes to the model to judge, and the
  // fixture model never grades, so skip to an exercise the app grades
  // itself. Which comes first depends on the seeded draws before this test.
  let qid = (await ex.getAttribute("data-qid"))!;
  for (let i = 0; key.question(qid).type === "fill-in-blank"; i++) {
    expect(i, "the lesson has an exercise other than a fill-in").toBeLessThan(10);
    await ex.getByTestId("tutor-skip").click();
    await expect(ex).not.toHaveAttribute("data-qid", qid, { timeout: 15_000 });
    qid = (await ex.getAttribute("data-qid"))!;
  }
  const chips = ex.getByTestId("tutor-choice");
  const options = await chips.allInnerTexts();
  const wrong = key.wrong(qid, options);
  if (options.length) await chips.nth(options.indexOf(wrong)).click();
  else {
    await page.getByTestId("tutor-input").fill(wrong);
    await page.getByTestId("tutor-send").click();
  }
  await expect(page.locator(`[data-testid="tutor-verdict"][data-qid="${qid}"]`)).toHaveAttribute("data-state", "wrong");
});
