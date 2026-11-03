// Exact spec for #1226, Place the note: a staff-note question shows an empty
// staff with its clef. A tap puts the note on the nearest line or space, a
// drag moves it, the flat and sharp buttons add an accidental, and the arrow
// keys and Enter do the same from the keyboard. Check grades the note on the
// server by pitch and octave ("Right note, wrong octave" for the same note an
// octave off) unless the question says any_octave; a wrong answer draws the
// right note beside it. In Practice, the reading check, Sprint mode, the Guide
// and the lesson editor's preview. The test course's `staff-note` lesson
// (data/test-courses/test-course/lessons/1-questions/staff-note.json) has
// tc-staff-note-01 (treble G), tc-staff-note-02 (middle C on the bass staff,
// one ledger line) and tc-staff-note-03 (F sharp, any octave). The test
// course names notes in letters.
import { expect, test, type Locator, type Page } from "@playwright/test";
import { otherNote, placeNote } from "../lib/answer";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

const ANSWER: Record<string, string> = { "tc-staff-note-01": "G", "tc-staff-note-02": "C", "tc-staff-note-03": "^F" };
const CLEF: Record<string, string> = { "tc-staff-note-01": "treble", "tc-staff-note-02": "bass", "tc-staff-note-03": "treble" };
const OCTAVE = "Almost! Right note, wrong octave.";

const PHONE = { width: 390, height: 844 };
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
const staff = (box: Locator) => box.getByTestId("staff-note-staff");
/** The point of step `s` (from the bottom line) on the staff, as static/staff-note.js draws it. */
async function at(box: Locator, s: number) {
  const r = (await staff(box).boundingBox())!;
  return { x: r.x + (r.width * 112) / 180, y: r.y + ((14 + (13 - s) * 5) * r.height) / 118 };
}

test.beforeEach(async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
});

test("Practice: tap, drag and the keyboard place the note; the octave counts unless any octave does", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto("/practice?lesson=staff-note");
  const summary = page.getByTestId("quiz-summary");
  const seen = new Set<string>();
  for (let i = 0; i < 8 && !(await summary.isVisible()); i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = (await q.getAttribute("data-qid"))!;
    const box = q.getByTestId("staff-note");
    const fb = q.getByTestId("quiz-feedback");
    await expect(staff(box)).toBeVisible();
    await expect(staff(box)).toHaveAttribute("data-clef", CLEF[id]);
    // The whole staff is the tap target, well over 44 px tall.
    expect((await staff(box).boundingBox())!.height).toBeGreaterThanOrEqual(44);
    await expect(box).toHaveAttribute("data-answer", "");
    // Nothing placed yet: Check asks for an answer first.
    await q.getByTestId("quiz-check").click();
    await expect(fb).toHaveAttribute("data-state", "empty");
    if (id === "tc-staff-note-01") {
      // A tap a little off the G line snaps to it, so the note sits on the line.
      const g = await at(box, 2);
      await page.mouse.click(g.x, g.y + 1.5);
      await expect(box).toHaveAttribute("data-answer", "G");
      await expect(box.getByTestId("staff-note-head-ledger")).toHaveCount(0);
      // The G an octave up (the space above the top line) is the right note in the wrong octave.
      await placeNote(box, "g");
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "wrong");
      await expect(fb).toContainText(OCTAVE);
      // The right note is drawn beside it and named; the staff takes no more taps.
      await expect(box.getByTestId("staff-note-right")).toHaveCount(1);
      await expect(fb.getByTestId("staff-note-answer")).toHaveText("G");
      await page.mouse.click(g.x, g.y);
      await expect(box).toHaveAttribute("data-answer", "g");
    } else if (id === "tc-staff-note-02") {
      // The keyboard: the arrows move a dotted note from the middle line, Enter places it.
      await staff(box).focus();
      await expect(box.getByTestId("staff-note-ghost")).toBeVisible();
      for (let k = 0; k < 6; k++) await page.keyboard.press("ArrowUp");
      await expect(staff(box)).toHaveAttribute("aria-valuetext", "Ledger line 1 above");
      await expect(box).toHaveAttribute("data-answer", "");
      await page.keyboard.press("Enter");
      await expect(box).toHaveAttribute("data-answer", "C");
      // Middle C on a bass staff sits on one ledger line above it.
      await expect(box.getByTestId("staff-note-head-ledger")).toHaveCount(1);
      // Enter on the placed note checks it.
      await page.keyboard.press("Enter");
      await expect(fb).toHaveAttribute("data-state", "right");
    } else {
      // A drag: down on the top line (high F), let go in the bottom space (low F), then a sharp.
      const top = await at(box, 8), low = await at(box, 1);
      await page.mouse.move(top.x, top.y);
      await page.mouse.down();
      await expect(box).toHaveAttribute("data-answer", "f");
      await page.mouse.move(low.x, (top.y + low.y) / 2, { steps: 4 });
      await page.mouse.move(low.x, low.y, { steps: 4 });
      await page.mouse.up();
      await expect(box).toHaveAttribute("data-answer", "F");
      await box.getByTestId("staff-note-sharp").click();
      await expect(box.getByTestId("staff-note-sharp")).toHaveAttribute("aria-pressed", "true");
      await expect(box).toHaveAttribute("data-answer", "^F");
      // Any octave counts here: the low F sharp is as right as the high one.
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "right");
      await expect(box).toHaveAttribute("data-state", "right");
    }
    seen.add(id);
    await fitsPhone(page);
    await q.getByTestId("quiz-next").click();
  }
  expect([...seen].sort()).toEqual(Object.keys(ANSWER).sort());
});

test("a note a step off is wrong with no octave note", async ({ page }) => {
  await page.goto("/practice?lesson=staff-note");
  for (let i = 0; i < 3; i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = (await q.getAttribute("data-qid"))!;
    await placeNote(q.getByTestId("staff-note"), otherNote(ANSWER[id]));
    await q.getByTestId("quiz-check").click();
    const fb = q.getByTestId("quiz-feedback");
    await expect(fb).toHaveAttribute("data-state", "wrong");
    await expect(fb).not.toContainText(OCTAVE);
    await q.getByTestId("quiz-next").click();
  }
});

test("the reading check, Sprint mode and the Guide take the note", async ({ page }) => {
  await page.setViewportSize(PHONE);
  // The reading check: every note placed right passes it.
  await page.goto("/lessons/staff-note");
  for (const q of await page.getByTestId("lesson-check-question").all()) {
    await placeNote(q.getByTestId("staff-note"), ANSWER[(await q.getAttribute("data-qid"))!]);
  }
  await page.getByTestId("lesson-check-submit").click();
  await expect(page.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "passed");

  // Sprint mode: past the bites; the first note a step off, the second right.
  await page.goto("/sprint/play?lesson=staff-note");
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  const result = page.getByTestId("sprint-play-result");
  for (let n = 0; n < 2; n++) {
    for (let i = 0; i < 12 && (await stage.getAttribute("data-kind")) !== "staff-note"; i++) await check.click();
    await expect(stage).toHaveAttribute("data-kind", "staff-note");
    const id = (await stage.getAttribute("data-exercise-id"))!;
    const box = stage.getByTestId("staff-note");
    await expect(check).toBeDisabled();
    await expect(staff(box)).toHaveAttribute("data-clef", CLEF[id]);
    if (n === 0) {
      await placeNote(box, otherNote(ANSWER[id]));
      await check.click();
      await expect(result).toHaveAttribute("data-state", "wrong");
      await expect(box.getByTestId("staff-note-right")).toHaveCount(1);
    } else {
      await placeNote(box, ANSWER[id]);
      await check.click();
      await expect(result).toHaveAttribute("data-state", "right");
    }
    await fitsPhone(page);
    await page.getByTestId("sprint-play-next").click();
  }

  // The Guide: through the cards to the first quick check, placed right.
  await page.goto("/guide/staff-note");
  const step = page.locator('[data-testid="stage-step"][data-kind="question"]').first();
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await expect(card).toBeVisible();
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await expect(step).toBeVisible();
  await placeNote(step.getByTestId("staff-note"), ANSWER[(await step.getAttribute("data-qid"))!]);
  await step.getByTestId("stage-check").click();
  await expect(step.getByTestId("stage-feedback")).toHaveAttribute("data-state", "right");
});

test("a lesson without a staff-note question loads no staff-note script", async ({ page }) => {
  for (const path of ["/practice?lesson=multiple-choice", "/lessons/multiple-choice", "/guide/multiple-choice", "/sprint/play?lesson=multiple-choice"]) {
    await page.goto(path);
    await expect(page.locator("main, .sprint-play-main").first()).toBeVisible();
    await expect(page.locator('script[src*="staff-note"]'), path).toHaveCount(0);
    await expect(page.getByTestId("staff-note")).toHaveCount(0);
  }
});

test("the lesson editor has the note, clef and any-octave boxes, and its preview plays the question", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Place the note" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
  await expect(form.locator('[data-testid="course-form-type"] [data-value="staff-note"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.locator('[data-testid="course-form-field"][data-path="quiz.0.correct_answer"]')).toHaveValue("G");
  await expect(form.locator('[data-testid="course-form-field"][data-path="quiz.0.clef"] [data-value="treble"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.getByTestId("course-form-any-octave")).toHaveAttribute("aria-pressed", "false");
  // The preview: the treble staff; G placed, and Try it says right.
  const phone = form.getByTestId("course-preview-phone");
  const box = phone.getByTestId("course-preview-staff-note");
  await expect(staff(box)).toHaveAttribute("data-clef", "treble");
  await placeNote(box, "G");
  await phone.getByTestId("course-preview-check").click();
  await expect(phone.getByTestId("course-preview-feedback")).toHaveAttribute("data-state", "right");
  await devReset(page);
});
