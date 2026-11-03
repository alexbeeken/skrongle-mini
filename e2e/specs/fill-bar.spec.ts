// Exact spec for #1230, Fill the bar: a fill-bar question shows a bar with a
// gap, the beats still to fill and tiles of note values. A tap puts a tile in
// the gap and the bar is drawn again; Undo takes it out; once the gap is full
// Check grades it on the server by length (or, with "exact", against the
// answer). In Practice, the reading check, Sprint mode, the Guide and the
// lesson editor's preview. The test course's `fill-bar` lesson
// (data/test-courses/test-course/lessons/1-questions/fill-bar.json) has two:
// tc-fill-bar-01 (any tiles that fill 2 beats; B4 is too long) and
// tc-fill-bar-02 (exact: only c2, though cc fills the gap too) and
// tc-fill-bar-03 (a 3/4 bar one beat short).
import { expect, test, type Locator, type Page } from "@playwright/test";
import { fillTheBar } from "../lib/answer";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

/** A right answer to each question of the lesson. */
const RIGHT: Record<string, string[]> = { "tc-fill-bar-01": ["B/B/B"], "tc-fill-bar-02": ["c2"], "tc-fill-bar-03": ["d/d/"] };

const PHONE = { width: 390, height: 844 };
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
const notes = (bar: Locator) => bar.locator('[data-testid="fill-bar-staff"] svg .abcjs-note');
const tile = (bar: Locator, t: string) => bar.locator(`[data-testid="fill-bar-tile"][data-tile="${t}"]`);

test.beforeEach(async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
});

test("Practice: tap tiles into the gap, undo one, overfill it and hear how many beats went in", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto("/practice?lesson=fill-bar");
  const summary = page.getByTestId("quiz-summary");
  const seen = new Set<string>();
  for (let i = 0; i < 8 && !(await summary.isVisible()); i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = (await q.getAttribute("data-qid"))!;
    const bar = q.getByTestId("fill-bar");
    const left = bar.getByTestId("fill-bar-left");
    // The bar is drawn with its notes and an invisible gap; the tiles are drawn as notes too.
    await expect(notes(bar)).toHaveCount(id === "tc-fill-bar-03" ? 1 : 2);
    await expect(left).toHaveText(id === "tc-fill-bar-03" ? "1 beat to fill" : "2 beats to fill");
    await expect(bar.locator('[data-testid="fill-bar-tile"] svg').first()).toBeVisible();
    await expect(bar.getByTestId("fill-bar-undo")).toBeDisabled();
    if (id === "tc-fill-bar-01") {
      // One beat in: drawn in the bar, one to go. Undo takes it out again.
      await tile(bar, "B").click();
      await expect(notes(bar)).toHaveCount(3);
      await expect(left).toHaveText("1 beat to fill");
      await bar.getByTestId("fill-bar-undo").click();
      await expect(notes(bar)).toHaveCount(2);
      await expect(left).toHaveText("2 beats to fill");
      // Nothing in the gap: Check asks for an answer first.
      await q.getByTestId("quiz-check").click();
      await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "empty");
      // A whole note is two beats too many; once full the tiles are locked.
      await tile(bar, "B4").click();
      await expect(left).toHaveText("The bar is full.");
      await expect(tile(bar, "B2")).toBeDisabled();
      await q.getByTestId("quiz-check").click();
      const fb = q.getByTestId("quiz-feedback");
      await expect(fb).toHaveAttribute("data-state", "wrong");
      await expect(fb).toContainText("Beats you put in: 4. Beats the gap holds: 2.");
      // The right answer (the loader's pick: the fewest tiles, B2) drawn in the bar.
      await expect(fb.getByTestId("fill-bar-answer").locator("svg .abcjs-note")).toHaveCount(3);
      await expect(bar.getByTestId("fill-bar-undo")).toBeDisabled();
    } else if (id === "tc-fill-bar-03") {
      // Two eighth notes make the one beat.
      await fillTheBar(bar, RIGHT[id]);
      await q.getByTestId("quiz-check").click();
      await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
    } else {
      // Exact: two quarter notes fill the gap but aren't the half note asked for.
      expect(id).toBe("tc-fill-bar-02");
      await tile(bar, "cc").click();
      await q.getByTestId("quiz-check").click();
      const fb = q.getByTestId("quiz-feedback");
      await expect(fb).toHaveAttribute("data-state", "wrong");
      await expect(fb).toContainText("Those notes fill the gap, but this question wants particular ones.");
    }
    seen.add(id);
    await fitsPhone(page);
    await q.getByTestId("quiz-next").click();
  }
  expect([...seen].sort()).toEqual(["tc-fill-bar-01", "tc-fill-bar-02", "tc-fill-bar-03"]);
});

test("on a wide page the bar has no blank gap below it and the answer stays its own width", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/practice?lesson=fill-bar");
  // abcjs's padding-bottom percentage comes from the parent's width: the staff
  // box must be as tall as its drawing, not a third of the card's width.
  const gap = async (box: Locator) => box.evaluate((e) => e.getBoundingClientRect().height - e.querySelector("svg")!.getBoundingClientRect().height);
  for (let i = 0; i < 3; i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const bar = q.getByTestId("fill-bar");
    await expect(notes(bar).first()).toBeVisible();
    expect(await gap(bar.getByTestId("fill-bar-staff"))).toBeLessThan(8);
    if ((await q.getAttribute("data-qid")) === "tc-fill-bar-01") {
      await tile(bar, "B4").click();
      await q.getByTestId("quiz-check").click();
      const ans = q.getByTestId("quiz-feedback").getByTestId("fill-bar-answer");
      await expect(ans.locator("svg .abcjs-note")).toHaveCount(3);
      expect((await ans.boundingBox())!.width).toBeLessThanOrEqual(14 * 16 + 1);
      expect(await gap(ans)).toBeLessThan(8);
      return;
    }
    await fillTheBar(bar, null);
    await q.getByTestId("quiz-check").click();
    await q.getByTestId("quiz-next").click();
  }
  throw new Error("tc-fill-bar-01 never came up");
});

test("the reading check, Sprint mode and the Guide take any tiles that fill the gap", async ({ page }) => {
  await page.setViewportSize(PHONE);
  // The reading check: both questions filled right pass it.
  await page.goto("/lessons/fill-bar");
  for (const q of await page.getByTestId("lesson-check-question").all()) {
    const id = await q.getAttribute("data-qid");
    await fillTheBar(q.getByTestId("fill-bar"), RIGHT[id!]);
  }
  await page.getByTestId("lesson-check-submit").click();
  await expect(page.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "passed");

  // Sprint mode: past the bites; the first is right with two quarter notes, the exact one wrong with them.
  await page.goto("/sprint/play?lesson=fill-bar");
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  const result = page.getByTestId("sprint-play-result");
  for (let n = 0; n < 2; n++) {
    for (let i = 0; i < 12 && (await stage.getAttribute("data-kind")) !== "fill-bar"; i++) await check.click();
    await expect(stage).toHaveAttribute("data-kind", "fill-bar");
    const id = await stage.getAttribute("data-exercise-id");
    const bar = stage.getByTestId("fill-bar");
    await expect(check).toBeDisabled();
    // Sprint has no .btn: Undo is still drawn as an outlined button, not the browser's grey one.
    expect(await bar.getByTestId("fill-bar-undo").evaluate((e) => getComputedStyle(e).borderTopLeftRadius)).not.toMatch(/^[0-4](\.\d+)?px$/);
    if (id === "tc-fill-bar-02") await tile(bar, "cc").click();
    else if (id === "tc-fill-bar-03") await fillTheBar(bar, RIGHT[id]);
    else {
      await tile(bar, "B").click();
      await expect(check).toBeDisabled();
      await tile(bar, "B").click();
    }
    await check.click();
    await expect(result).toHaveAttribute("data-state", id === "tc-fill-bar-02" ? "wrong" : "right");
    if (id === "tc-fill-bar-02") await expect(result.getByTestId("fill-bar-answer").locator("svg .abcjs-note")).toHaveCount(3);
    await fitsPhone(page);
    await page.getByTestId("sprint-play-next").click();
  }

  // The Guide: through the cards to the first quick check, filled right.
  await page.goto("/guide/fill-bar");
  const step = page.locator('[data-testid="stage-step"][data-kind="question"]').first();
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await expect(card).toBeVisible();
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await expect(step).toBeVisible();
  await expect(notes(step.getByTestId("fill-bar")).first()).toBeVisible();
  await fillTheBar(step.getByTestId("fill-bar"), RIGHT[(await step.getAttribute("data-qid"))!]);
  await step.getByTestId("stage-check").click();
  await expect(step.getByTestId("stage-feedback")).toHaveAttribute("data-state", "right");
});

test("a lesson without a fill-bar question loads no fill-bar script", async ({ page }) => {
  for (const path of ["/practice?lesson=multiple-choice", "/lessons/multiple-choice", "/guide/multiple-choice", "/sprint/play?lesson=multiple-choice"]) {
    await page.goto(path);
    await expect(page.locator("main, .sprint-play-main").first()).toBeVisible();
    await expect(page.locator('script[src*="fill-bar"]'), path).toHaveCount(0);
    await expect(page.getByTestId("fill-bar")).toHaveCount(0);
  }
});

test("the lesson editor has the tiles and the what-counts switch, and its preview plays the question", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Fill the bar" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
  await expect(form.locator('[data-testid="course-form-type"] [data-value="fill-bar"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.locator('[data-testid="course-form-list"][data-path="quiz.0.tiles"] [data-testid="course-form-item"]')).toHaveCount(5);
  await expect(form.locator('[data-testid="course-form-exact"] [data-value="any"]')).toHaveAttribute("aria-pressed", "true");
  // The preview: the bar and its tiles; two quarter notes fill it, and Try it says right.
  const phone = form.getByTestId("course-preview-phone");
  const bar = phone.getByTestId("course-preview-fill-bar");
  await expect(notes(bar)).toHaveCount(2);
  await tile(bar, "BB").click();
  await phone.getByTestId("course-preview-check").click();
  await expect(phone.getByTestId("course-preview-feedback")).toHaveAttribute("data-state", "right");
  await devReset(page);
});
