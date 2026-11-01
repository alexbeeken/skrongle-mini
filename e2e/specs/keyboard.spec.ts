// Exact spec for #1231, Find it on the keys: a keyboard question shows a
// piano keyboard of one or two octaves from middle C; each key sounds when
// tapped and the last key tapped before Check is the answer, graded on the
// server by pitch class (or the exact key with "octave": true), any spelling
// of the pitch counting. In Practice, the reading check, Sprint mode, the
// Guide and the lesson editor's preview. The test course's `keyboard` lesson
// (data/test-courses/test-course/lessons/1-questions/keyboard.json) has
// tc-keyboard-01 (hear E, one octave), tc-keyboard-02 (hear B flat, answered
// by the black key A sharp, two octaves) and tc-keyboard-03 (the C above
// middle C on a staff, exact key, two octaves).
import { expect, test, type Locator, type Page } from "@playwright/test";
import { copyCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

/** The key that answers each question right, as the page posts it. */
const RIGHT: Record<string, string> = { "tc-keyboard-01": "E", "tc-keyboard-02": "^A", "tc-keyboard-03": "c" };

const PHONE = { width: 390, height: 844 };
const fitsPhone = async (page: Page) => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
const key = (kb: Locator, note: string) => kb.locator(`[data-testid="keyboard-key"][data-note="${note}"]`).first();

/** Every key is a tap target of at least 44 by 44 px. */
async function bigKeys(kb: Locator) {
  const sizes = await kb.getByTestId("keyboard-key").evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return [r.width, r.height]; }));
  for (const [w, h] of sizes) expect(Math.min(w, h)).toBeGreaterThanOrEqual(43.5) // 44 px, give or take a subpixel;
}

/** Every key of the editor's preview sits inside the preview phone (it is narrower than a phone). */
async function keysInside(phone: Locator) {
  const frame = (await phone.boundingBox())!;
  const keys = await phone.getByTestId("keyboard-key").evaluateAll((els) => els.map((e) => { const r = e.getBoundingClientRect(); return [r.left, r.right]; }));
  for (const [l, r] of keys) {
    expect(l).toBeGreaterThanOrEqual(frame.x);
    expect(r).toBeLessThanOrEqual(frame.x + frame.width);
  }
}

test.beforeEach(async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
});

test("Practice: keys sound on a tap, the last one counts, any spelling and octave unless the key is exact", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto("/practice?lesson=keyboard");
  await expect(page.locator('script[src*="keyboard.js"]')).toHaveCount(1);
  const summary = page.getByTestId("quiz-summary");
  const seen = new Set<string>();
  for (let i = 0; i < 6 && !(await summary.isVisible()); i++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    const id = (await q.getAttribute("data-qid"))!;
    const kb = q.getByTestId("keyboard");
    const fb = q.getByTestId("quiz-feedback");
    await expect(kb.getByTestId("keyboard-key")).toHaveCount(id === "tc-keyboard-01" ? 12 : 24);
    await bigKeys(kb);
    await fitsPhone(page);
    if (id === "tc-keyboard-01") {
      // Nothing tapped: Check asks for an answer first.
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "empty");
      // Hunt by ear: each key sounds; the last one tapped is the answer.
      await key(kb, "F").click();
      await expect(kb).toHaveAttribute("data-sounded", "F");
      await key(kb, "E").click();
      await expect(kb).toHaveAttribute("data-sounded", "E");
      await expect(kb).toHaveAttribute("data-chosen", "E");
      await expect(key(kb, "F")).toHaveAttribute("aria-pressed", "false");
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "right");
      await expect(key(kb, "E")).toHaveAttribute("data-state", "right");
    } else if (id === "tc-keyboard-02") {
      // B flat heard, A sharp tapped: the same key.
      await key(kb, "^A").click();
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "right");
      await expect(kb.locator('[data-state="right"]')).toHaveCount(2); // both octaves' B flat
    } else {
      // Exact key: middle C is the right note in the wrong octave.
      expect(id).toBe("tc-keyboard-03");
      await expect(q.getByTestId("quiz-abc")).toHaveCount(1);
      await key(kb, "C").click();
      await q.getByTestId("quiz-check").click();
      await expect(fb).toHaveAttribute("data-state", "wrong");
      await expect(fb).toContainText("Right note, wrong octave.");
      await expect(fb.locator("strong").first()).toHaveText("C");
      await expect(key(kb, "c")).toHaveAttribute("data-state", "right");
      await expect(key(kb, "C")).toHaveAttribute("data-state", "wrong");
      // After the answer the keys still sound but no longer change it.
      await key(kb, "D").click();
      await expect(kb).toHaveAttribute("data-chosen", "C");
    }
    seen.add(id);
    await fitsPhone(page);
    await q.getByTestId("quiz-next").click();
  }
  expect([...seen].sort()).toEqual(Object.keys(RIGHT).sort());
  await expect(summary).toHaveAttribute("data-correct", "2");
});

test("the reading check, Sprint mode and the Guide take the key tapped", async ({ page }) => {
  await page.setViewportSize(PHONE);
  // The reading check: every question answered on its keys passes it.
  await page.goto("/lessons/keyboard");
  for (const q of await page.getByTestId("lesson-check-question").all()) {
    const kb = q.getByTestId("keyboard");
    await key(kb, RIGHT[(await q.getAttribute("data-qid"))!]).click();
  }
  await page.getByTestId("lesson-check-submit").click();
  await expect(page.getByTestId("lesson-check-result")).toHaveAttribute("data-state", "passed");

  // Sprint mode: Check waits for a key; every keyboard question taken right.
  await page.goto("/sprint/play?lesson=keyboard");
  await expect(page.locator('script[src*="keyboard.js"]')).toHaveCount(1);
  const stage = page.getByTestId("sprint-play-stage");
  const check = page.getByTestId("sprint-play-check");
  const result = page.getByTestId("sprint-play-result");
  const done = page.getByTestId("sprint-play-done");
  const asked = new Set<string>();
  for (let i = 0; i < 20 && !(await done.isVisible()); i++) {
    if ((await stage.getAttribute("data-kind")) !== "keyboard") {
      await check.click();
      continue;
    }
    const id = (await stage.getAttribute("data-exercise-id"))!;
    const kb = stage.getByTestId("keyboard");
    await expect(check).toBeDisabled();
    await bigKeys(kb);
    await key(kb, RIGHT[id]).click();
    await expect(check).toBeEnabled();
    await check.click();
    await expect(result).toHaveAttribute("data-state", "right");
    await expect(kb.locator('[data-state="right"]').first()).toBeVisible();
    await fitsPhone(page);
    asked.add(id);
    await page.getByTestId("sprint-play-next").click();
    await expect(result).toBeHidden();
  }
  expect(asked.size).toBeGreaterThan(0);

  // The Guide (Play): through the cards to the first quick check, answered on the keys.
  await page.goto("/guide/keyboard");
  const step = page.locator('[data-testid="stage-step"][data-kind="question"]').first();
  const index = Number(await step.getAttribute("data-index"));
  for (let i = 0; i < index; i++) {
    const card = page.locator(`[data-testid="stage-step"][data-index="${i}"]`);
    await expect(card).toBeVisible();
    await card.getByTestId("stage-continue").click();
    await expect(card).toBeHidden();
  }
  await expect(step).toBeVisible();
  await key(step.getByTestId("keyboard"), RIGHT[(await step.getAttribute("data-qid"))!]).click();
  await step.getByTestId("stage-check").click();
  await expect(step.getByTestId("stage-feedback")).toHaveAttribute("data-state", "right");
});

test("a lesson without a keyboard question loads no keyboard script", async ({ page }) => {
  for (const path of ["/practice?lesson=multiple-choice", "/lessons/multiple-choice", "/guide/multiple-choice", "/sprint/play?lesson=multiple-choice"]) {
    await page.goto(path);
    await expect(page.locator("main, .sprint-play-main").first()).toBeVisible();
    await expect(page.locator('script[src*="keyboard.js"]'), path).toHaveCount(0);
    await expect(page.getByTestId("keyboard")).toHaveCount(0);
  }
});

test("the lesson editor has the key, octaves and what-counts switches, and its preview plays the question", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Find it on the keys" }).first();
  await row.getByTestId("course-lessons-expand").click();
  const form = row.getByTestId("course-form");
  const quiz = form.locator('[data-testid="course-outline-section"][data-sec="quiz"]');
  if ((await quiz.getAttribute("aria-expanded")) !== "true") await quiz.click();
  await form.locator('[data-testid="course-outline-item"][data-sec="quiz"][data-idx="0"]').click();
  await expect(form.locator('[data-testid="course-form-type"] [data-value="keyboard"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.locator('[data-testid="course-form-octaves"] [data-value="1"]')).toHaveAttribute("aria-pressed", "true");
  await expect(form.locator('[data-testid="course-form-octave"] [data-value="any"]')).toHaveAttribute("aria-pressed", "true");
  // The preview: one octave of keys; E is right, and lights up.
  const phone = form.getByTestId("course-preview-phone");
  const kb = phone.getByTestId("course-preview-keyboard");
  await expect(kb.getByTestId("keyboard-key")).toHaveCount(12);
  await keysInside(phone);
  await key(kb, "E").click();
  await phone.getByTestId("course-preview-check").click();
  await expect(phone.getByTestId("course-preview-feedback")).toHaveAttribute("data-state", "right");
  await expect(key(kb, "E")).toHaveAttribute("data-state", "right");
  // Two octaves: the preview redraws with 24 keys.
  await form.locator('[data-testid="course-form-octaves"] [data-value="2"]').click();
  await expect(phone.getByTestId("course-preview-keyboard").getByTestId("keyboard-key")).toHaveCount(24);
  await keysInside(phone);
  await devReset(page);
});
