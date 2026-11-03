// Exact spec for #1228, Spell the chord: a chord-spell question shows a staff,
// one empty slot per note of the chord and note tiles in the course's note
// names. A tap puts a tile in the first empty slot and the staff draws the
// notes as one chord; a tap on a filled slot takes it out; once every slot is
// full Check grades the notes on the server as a set (order free, spelling
// strict, D G♭ A for D major is "right sound, wrong spelling"). In Practice,
// the reading check, Sprint mode, the Guide and the lesson editor's preview.
// The test course's `chord-spell` lesson
// (data/test-courses/test-course/lessons/1-questions/chord-spell.json) has
// tc-chord-spell-01 (D major, tiles as a string), tc-chord-spell-02 (A minor,
// tiles as a list) and tc-chord-spell-03 (G7, four notes). The test course
// names notes in letters.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { spellChord } from "../lib/answer";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

/** Each question's chord, in ABC, and a right answer in another order. */
const CHORD: Record<string, string[]> = { "tc-chord-spell-01": ["D", "^F", "A"], "tc-chord-spell-02": ["A", "C", "E"], "tc-chord-spell-03": ["G", "B", "D", "F"] };
const OTHER_ORDER: Record<string, string[]> = { "tc-chord-spell-01": ["A", "D", "^F"], "tc-chord-spell-02": ["E", "A", "C"], "tc-chord-spell-03": ["F", "D", "B", "G"] };
const NEAR_MISS = "Almost! Right sound, wrong spelling. Check the note names.";

const PHONE = { width: 390, height: 844 };
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
const tile = (box: Locator, abc: string) => box.locator(`[data-testid="chord-spell-tile"][data-tile="${abc}"]`);
const slots = (box: Locator) => box.getByTestId("chord-spell-slot");
const staff = (box: Locator) => box.getByTestId("chord-spell-staff");
const chordAbc = (notes: string) => `X:1\nL:1/1\nK:C\n${notes}|`;

test.beforeEach(async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
});

test("Practice: tap tiles into the slots, take one out, hear about a wrong spelling, and spell the others in any order", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto("/practice?lesson=chord-spell");
  const summary = page.getByTestId("quiz-summary");
  const seen = new Set<string>();
  for (let i = 0; i < 8 && !(await summary.isVisible()); i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = (await q.getAttribute("data-qid"))!;
    const box = q.getByTestId("chord-spell");
    // One empty slot per note, an empty staff drawn, the tiles in letters.
    await expect(slots(box)).toHaveCount(CHORD[id].length);
    await expect(slots(box).first()).toHaveText("");
    await expect(staff(box)).toHaveAttribute("data-abc", chordAbc("x"));
    await expect(staff(box).locator("svg")).toBeVisible();
    if (id === "tc-chord-spell-01") {
      await expect(tile(box, "^F")).toHaveText("F♯");
      await expect(tile(box, "_G")).toHaveText("G♭");
      // Two notes in: in the slots and on the staff as one chord.
      await tile(box, "D").click();
      await tile(box, "_G").click();
      await expect(slots(box).nth(0)).toHaveText("D");
      await expect(slots(box).nth(1)).toHaveText("G♭");
      await expect(staff(box)).toHaveAttribute("data-abc", chordAbc("[D_G]"));
      await expect(tile(box, "_G")).toBeDisabled();
      // A tap on a filled slot takes its note out again.
      await slots(box).nth(1).click();
      await expect(slots(box).nth(1)).toHaveText("");
      await expect(tile(box, "_G")).toBeEnabled();
      await expect(staff(box)).toHaveAttribute("data-abc", chordAbc("[D]"));
      // Not full yet: Check asks for an answer first.
      await q.getByTestId("quiz-check").click();
      await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "empty");
      // G flat sounds like F sharp but is spelled wrong for D major: a near miss.
      await tile(box, "_G").click();
      await tile(box, "A").click();
      await expect(box).toHaveAttribute("data-full", "true");
      await expect(tile(box, "B")).toBeDisabled();
      await q.getByTestId("quiz-check").click();
      const fb = q.getByTestId("quiz-feedback");
      await expect(fb).toHaveAttribute("data-state", "wrong");
      await expect(fb).toContainText(NEAR_MISS);
      // The right answer in the course's note names, and the slots locked.
      await expect(fb.getByTestId("chord-spell-answer")).toHaveText("D F♯ A");
      await expect(slots(box).nth(0)).toBeDisabled();
    } else {
      await spellChord(box, OTHER_ORDER[id]);
      await expect(staff(box)).toHaveAttribute("data-abc", chordAbc(`[${OTHER_ORDER[id].join("")}]`));
      await q.getByTestId("quiz-check").click();
      await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
    }
    seen.add(id);
    await fitsPhone(page);
    await q.getByTestId("quiz-next").click();
  }
  expect([...seen].sort()).toEqual(Object.keys(CHORD).sort());
});

test("a wrong set that isn't the same sounds gets no spelling note", async ({ page }) => {
  await page.goto("/practice?lesson=chord-spell");
  for (let i = 0; i < 3; i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = (await q.getAttribute("data-qid"))!;
    await spellChord(q.getByTestId("chord-spell"), CHORD[id], true);
    await q.getByTestId("quiz-check").click();
    const fb = q.getByTestId("quiz-feedback");
    await expect(fb).toHaveAttribute("data-state", "wrong");
    await expect(fb).not.toContainText(NEAR_MISS);
    await q.getByTestId("quiz-next").click();
  }
});

test("the reading check, Sprint mode and the Guide take the chord in any order", async ({ page }) => {
  await page.setViewportSize(PHONE);
  // The reading check: every question spelled right passes it.
  await page.goto("/lessons/chord-spell");
  for (const q of await page.getByTestId("lesson-check-question").all()) {
    await spellChord(q.getByTestId("chord-spell"), OTHER_ORDER[(await q.getAttribute("data-qid"))!]);
  }
  await page.getByTestId("lesson-check-submit").click();
  await expect(page.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "passed");

  // Sprint mode: past the bites; D major spelled with G flat is the near miss, the others right.
  await page.goto("/sprint/play?lesson=chord-spell");
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  const result = page.getByTestId("sprint-play-result");
  for (let n = 0; n < 2; n++) {
    for (let i = 0; i < 12 && (await stage.getAttribute("data-kind")) !== "chord-spell"; i++) await check.click();
    await expect(stage).toHaveAttribute("data-kind", "chord-spell");
    const id = (await stage.getAttribute("data-exercise-id"))!;
    const box = stage.getByTestId("chord-spell");
    await expect(check).toBeDisabled();
    await expect(staff(box).locator("svg")).toBeVisible();
    if (id === "tc-chord-spell-01") {
      await spellChord(box, ["_G", "A", "D"]);
      await check.click();
      await expect(result).toHaveAttribute("data-state", "wrong");
      await expect(result).toContainText(NEAR_MISS);
      await expect(result.getByTestId("chord-spell-answer")).toHaveText("D F♯ A");
    } else {
      await spellChord(box, OTHER_ORDER[id]);
      await check.click();
      await expect(result).toHaveAttribute("data-state", "right");
    }
    await fitsPhone(page);
    await page.getByTestId("sprint-play-next").click();
  }

  // The Guide: through the cards to the first quick check, spelled right.
  await page.goto("/guide/chord-spell");
  const step = page.locator('[data-testid="stage-step"][data-kind="question"]').first();
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await expect(card).toBeVisible();
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await expect(step).toBeVisible();
  const box = step.getByTestId("chord-spell");
  await expect(staff(box).locator("svg")).toBeVisible();
  await spellChord(box, OTHER_ORDER[(await step.getAttribute("data-qid"))!]);
  await step.getByTestId("stage-check").click();
  await expect(step.getByTestId("stage-feedback")).toHaveAttribute("data-state", "right");
});

test("a lesson without a chord-spell question loads no chord-spell script", async ({ page }) => {
  for (const path of ["/practice?lesson=multiple-choice", "/lessons/multiple-choice", "/guide/multiple-choice", "/sprint/play?lesson=multiple-choice"]) {
    await page.goto(path);
    await expect(page.locator("main, .sprint-play-main").first()).toBeVisible();
    await expect(page.locator('script[src*="chord-spell"]'), path).toHaveCount(0);
    await expect(page.getByTestId("chord-spell")).toHaveCount(0);
  }
});

test("the lesson editor has the answer and tiles boxes, and its preview plays the question", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Spell the chord" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
  await expect(form.locator('[data-testid="course-form-type"] [data-value="chord-spell"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.locator('[data-testid="course-form-field"][data-path="quiz.0.correct_answer"]')).toHaveValue("D ^F A");
  await expect(form.locator('[data-testid="course-form-field"][data-path="quiz.0.tiles"]')).toHaveValue("D E F ^F _G A B");
  // The preview: three slots and the tiles; the chord in another order, and Try it says right.
  const phone = form.getByTestId("course-preview-phone");
  const box = phone.getByTestId("course-preview-chord-spell");
  await expect(slots(box)).toHaveCount(3);
  for (const n of ["A", "^F", "D"]) await tile(box, n).click();
  await phone.getByTestId("course-preview-check").click();
  await expect(phone.getByTestId("course-preview-feedback")).toHaveAttribute("data-state", "right");
  await devReset(page);
});
