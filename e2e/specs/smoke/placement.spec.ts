// The placement test (#1106): a course with levels and a placement.json
// offers it in the level step, now the last step of the Start wizard. The
// test course writes two questions for its first unit and draws one from
// every other unit's multiple-choice questions. Answering the first unit
// right and everything else wrong suggests the second unit: its card is
// picked, the report card shows green for the first unit and red for the
// rest, and Get Started! starts the course there. Each test registers its
// own account; no key, no network.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { AnswerKey } from "../../lib/key.ts";
import { courseDir, TEST_COURSE } from "../../lib/app.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { login, newAccount, openCourse, signUp } from "../../lib/learner.ts";
import type { Locator, Page } from "@playwright/test";

const key = new AnswerKey(TEST_COURSE);
const written: { unit: string; prompt: string; options: string[]; answer: string }[] = JSON.parse(readFileSync(join(courseDir(TEST_COURSE), "placement.json"), "utf8")).questions;

function wizard(page: Page) {
  return page.locator(`[data-testid="courses-settings-dialog"][data-course-id="${TEST_COURSE}"][data-start]`);
}

/** Slide the wizard to its level step, the last one. */
async function toLevels(dlg: Locator) {
  while ((await dlg.locator('.cs-step.here[data-step="level"]').count()) === 0) await dlg.getByTestId("courses-settings-next").click();
}

/** Answer every question: the written ones (the first unit) right, the drawn ones wrong. Returns how many were asked. */
async function sit(dlg: Locator): Promise<number> {
  const quiz = dlg.getByTestId("placement-quiz");
  const result = dlg.getByTestId("placement-result");
  let asked = 0;
  while (!(await result.isVisible())) {
    await expect(quiz).toBeVisible();
    asked += 1;
    await expect(dlg.getByTestId("placement-count")).toContainText(`Question ${asked} of `);
    const id = (await quiz.getAttribute("data-question-id"))!;
    const options = await dlg.getByTestId("placement-option").allTextContents();
    const pick = id.startsWith("w") ? written[Number(id.slice(1)) - 1].answer : key.wrong(id.slice(2), options);
    await dlg.getByTestId("placement-option").filter({ hasText: new RegExp(`^${pick.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`) }).click();
    // The next question, or the result after the last.
    await expect.poll(async () => (await result.isVisible()) || (await quiz.getAttribute("data-question-id")) !== id).toBe(true);
  }
  return asked;
}

test("the placement test suggests the first unit with a miss, shows a report card, and the course starts there", async ({ page }) => {
  const acct = newAccount("placement");
  await signUp(page, acct);
  await openCourse(page, TEST_COURSE, { wizard: true });
  const dlg = wizard(page);
  // The level step is the last one, and offers the test beside the cards.
  await toLevels(dlg);
  await expect(dlg.getByTestId("courses-settings-next")).toBeHidden();
  await expect(dlg.getByTestId("courses-settings-level")).toHaveCount(6);
  await dlg.getByTestId("placement-start").click();
  // While it runs the cards make way for one question at a time.
  await expect(dlg.getByTestId("courses-settings-level").first()).toBeHidden();
  // Get Started! waits for the result.
  await expect(dlg.getByTestId("courses-settings-save")).toBeHidden();
  const asked = await sit(dlg);
  expect(asked).toBeGreaterThan(written.length);

  await expect(dlg.getByTestId("placement-suggest")).toHaveText("Your result: start at Question types");
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit="questions"] input')).toBeChecked();
  // The report card: one row a tested unit, green when all were right, red when none were.
  const card = dlg.getByTestId("placement-card");
  await expect(card).toBeHidden();
  await dlg.getByTestId("placement-card-toggle").click();
  await expect(card).toBeVisible();
  const rows = card.getByTestId("placement-card-row");
  await expect(rows.first()).toHaveAttribute("data-unit", "all");
  await expect(rows.first()).toHaveAttribute("data-status", "all");
  await expect(rows.first()).toContainText(`${written.length} / ${written.length}`);
  await expect(rows.nth(1)).toHaveAttribute("data-status", "none");
  const [green, red] = await Promise.all([rows.first().locator("td").first(), rows.nth(1).locator("td").first()].map((c) => c.evaluate((el) => getComputedStyle(el).backgroundColor)));
  expect(green).not.toBe(red);
  await dlg.getByTestId("placement-card-toggle").click();
  await expect(card).toBeHidden();

  // The suggestion only picks a card: another may still be chosen, and the test taken again.
  await dlg.locator('[data-testid="courses-settings-level"][data-unit="exercises"]').click();
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit="exercises"] input')).toBeChecked();
  await dlg.getByTestId("placement-retake").click();
  await expect(dlg.getByTestId("placement-quiz")).toBeVisible();
  await sit(dlg);
  await expect(dlg.locator('[data-testid="courses-settings-level"][data-unit="questions"] input')).toBeChecked();

  // Get Started! saves the suggested level.
  await dlg.getByTestId("courses-settings-save").click();
  await expect(page).toHaveURL(/\/$/);
  const progress = await (await page.request.get("/api/progress")).json();
  expect(progress.start_unit).toBe("Question types"); // the unit's title
});

test("the marking is the server's: answers come back with each unit's tally and no question carries its answer", async ({ page }) => {
  const acct = newAccount("placement-api");
  await signUp(page, acct);
  const sitting = await (await page.request.get(`/courses/${TEST_COURSE}/placement`)).json();
  expect(sitting.questions.length).toBeGreaterThan(written.length);
  for (const q of sitting.questions) expect(Object.keys(q).sort()).toEqual(["id", "options", "prompt", "unit"]);
  // Everything right: the unit after the last one tested.
  const answers = sitting.questions.map((q: { id: string; options: string[] }) => ({
    id: q.id,
    answer: q.id.startsWith("w") ? written[Number(q.id.slice(1)) - 1].answer : key.answer(q.id.slice(2), q.options),
  }));
  const r = await page.request.post(`/courses/${TEST_COURSE}/placement`, { data: { answers } });
  expect(r.ok()).toBe(true);
  const out = await r.json();
  expect(out.units.every((u: { status: string }) => u.status === "all")).toBe(true);
  // Spanish has levels but no placement test: nothing to take.
  expect((await page.request.get("/courses/spanish/placement")).status()).toBe(404);
});
