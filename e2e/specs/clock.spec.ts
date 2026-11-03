// Exact spec for #1340, Set the clock: a `clock` question shows a clock face.
// Dragging the minute hand round takes the hour hand with it, the keys move
// it a step or an hour, Enter checks, and the server grades the time. Turned
// around, the clock shows a time to pick. Practice, the Guide and Sprint mode
// all show it (the test course's "clock" lesson), and the tutor draws a clock to read.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { answerQuestion, playStage } from "../lib/answer";
import { devLogin, TEST_COURSE } from "../lib/app";
import { AnswerKey } from "../lib/key";
import { saveApiKey } from "../lib/learner";
import { playSprintRight } from "../lib/sprint-play";

const key = new AnswerKey(TEST_COURSE);
const PHONE = { width: 390, height: 844 };

/** The point at `deg` degrees clockwise from 12 and `frac` of the radius out, on the clock in `box`. */
async function at(box: Locator, deg: number, frac: number) {
  const r = (await box.locator("svg").boundingBox())!;
  const rad = (deg * Math.PI) / 180, R = (r.width / 2) * (100 / 110);
  return { x: r.x + r.width / 2 + Math.sin(rad) * R * frac, y: r.y + r.height / 2 - Math.cos(rad) * R * frac };
}

/** Press at `from` degrees and drag round to `to` (more than 360 goes round again), `frac` of the radius out. */
async function drag(page: Page, box: Locator, from: number, to: number, frac: number) {
  let p = await at(box, from, frac);
  await page.mouse.move(p.x, p.y);
  await page.mouse.down();
  for (let d = from; d <= to; d += 10) {
    p = await at(box, d, frac);
    await page.mouse.move(p.x, p.y);
  }
  p = await at(box, to, frac);
  await page.mouse.move(p.x, p.y);
  await page.mouse.up();
}

test("Practice: drag the minute hand, use the keys, read a clock", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/practice?lesson=clock");
  const seen = new Set<string>();
  for (let i = 0; i < 4; i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    await expect(q).toHaveCount(1);
    const id = (await q.getAttribute("data-qid"))!;
    seen.add(id);
    const box = q.getByTestId("clock");
    const face = box.locator("svg");
    const fb = q.getByTestId("quiz-feedback");
    if (id === "tc-clock-01") {
      // A quarter to four: the minute hand three times round and on to the 9.
      await expect(face).toHaveAttribute("role", "slider");
      await expect(face).toHaveAttribute("aria-valuetext", "12:00");
      await drag(page, box, 0, 3 * 360 + 270, 0.75);
      await expect(face).toHaveAttribute("aria-valuetext", "3:45");
      // The hour hand followed: nearly on the 4.
      await expect(box.getByTestId("clock-hour-hand")).toHaveAttribute("transform", "rotate(112.5)");
      await expect(box.getByTestId("clock-minute-hand")).toHaveAttribute("transform", "rotate(270)");
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "right");
    } else if (id === "tc-clock-02") {
      // Las seis y media, in 15-minute steps, with the keys; Enter checks.
      await expect(face).toHaveAttribute("data-step", "15");
      await face.focus();
      await face.press("ArrowUp");
      await expect(face).toHaveAttribute("aria-valuetext", "12:15");
      await face.press("ArrowDown");
      for (let k = 0; k < 6; k++) await face.press("PageUp");
      await face.press("ArrowUp");
      await face.press("ArrowUp");
      await expect(face).toHaveAttribute("aria-valuetext", "6:30");
      await face.press("Enter");
      await expect(fb).toHaveAttribute("data-state", "right");
    } else if (id === "tc-clock-03") {
      // Twelve o'clock, answered wrong: the hour hand dragged to the 1.
      await drag(page, box, 0, 30, 0.3);
      await expect(face).toHaveAttribute("aria-valuetext", "1:00");
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "wrong");
      await expect(fb).toContainText("12:00");
      // A graded clock no longer moves.
      await face.press("ArrowUp");
      await expect(face).toHaveAttribute("aria-valuetext", "1:00");
    } else {
      // Turned around: the clock shows 9:15, four times to pick from.
      await expect(face).toHaveAttribute("role", "img");
      await expect(box.getByTestId("clock-hour-hand")).toHaveAttribute("transform", "rotate(277.5)");
      await expect(box.getByTestId("clock-minute-hand")).toHaveAttribute("transform", "rotate(90)");
      await expect(q.getByTestId("quiz-option")).toHaveCount(4);
      await answerQuestion(q, key);
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "right");
    }
    await q.getByTestId("quiz-next").click();
  }
  expect([...seen].sort()).toEqual(["tc-clock-01", "tc-clock-02", "tc-clock-03", "tc-clock-04"]);
  await expect(page.getByTestId("quiz-summary")).toHaveAttribute("data-correct", "3");
  // The clock fits the phone.
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
});

test("the Guide and Sprint mode show the clock too", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/guide/clock");
  const [right, total] = await playStage(page, key);
  expect([right, total]).toEqual([4, 4]);

  await page.goto("/sprint/play?lesson=clock&reduce-motion=1");
  const run = await playSprintRight(page, key);
  expect(run.wrong).toEqual([]);
  expect(run.kinds.has("set-clock") && run.kinds.has("read-clock"), [...run.kinds].join(", ")).toBe(true);
});

test("the tutor draws a clock to read, with its times to tap", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  // A key (any: the dev server answers model calls from data/test-fixtures/llm/).
  await page.goto("/profile");
  await saveApiKey(page, "sk-ant-dev-test-fixtures");
  await page.goto("/tutor?lesson=clock");
  for (let i = 0; i < 4; i++) {
    const ex = page.locator('[data-testid="tutor-exercise"]:not([data-state=done])');
    await expect(ex).toHaveCount(1, { timeout: 15_000 });
    const qid = (await ex.getAttribute("data-qid"))!;
    if (qid === "tc-clock-04") {
      // Turned around: the clock shows 9:15 and its four times are chips.
      const face = ex.getByTestId("clock").locator("svg");
      await expect(face).toHaveAttribute("role", "img");
      await expect(ex.getByTestId("clock-hour-hand")).toHaveAttribute("transform", "rotate(277.5)");
      await expect(ex.getByTestId("tutor-choice")).toHaveCount(4);
      await ex.getByTestId("tutor-choice").filter({ hasText: /^9:15$/ }).click();
      await expect(page.locator(`[data-testid="tutor-verdict"][data-qid="${qid}"]`)).toHaveAttribute("data-state", "right");
      return;
    }
    // A clock to set is typed in the chat as H:MM.
    await expect(ex.getByTestId("tutor-choice")).toHaveCount(0);
    await page.getByTestId("tutor-input").fill(key.answer(qid));
    await page.getByTestId("tutor-send").click();
    await expect(page.locator(`[data-testid="tutor-verdict"][data-qid="${qid}"]`)).toHaveAttribute("data-state", "right");
  }
  throw new Error("the read-the-clock question never came up");
});
