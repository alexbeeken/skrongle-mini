// Practice (#407): a round can be played from the keyboard. 1–9 pick the n-th
// option, Enter checks the picked answer and then goes on; digits typed into a
// text box stay in the text box. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { TEST_COURSE } from "../../lib/app.ts";
import { AnswerKey } from "../../lib/key.ts";
import { newAccount, register } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);

test("number keys pick an option and Enter checks, then advances", async ({ page }) => {
  await register(page, newAccount("practice-keys"), TEST_COURSE);
  await page.goto("/practice?lesson=multiple-choice");
  const summary = page.getByTestId("quiz-summary");
  let asked = 0;
  for (let n = 0; n < 20 && !(await summary.isVisible()); n++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    await expect(q).toHaveCount(1);
    const id = (await q.getAttribute("data-qid"))!;
    const radios = q.getByTestId("quiz-option");
    const values = await radios.evaluateAll((els) => els.map((e) => (e as HTMLInputElement).value));
    expect(values.length, `${id}: options`).toBeGreaterThan(1);
    // A digit with no option behind it does nothing.
    await page.keyboard.press(String(Math.min(values.length + 1, 9)));
    if (values.length < 9) await expect(q.locator("input[type=radio]:checked")).toHaveCount(0);
    // Each digit picks its option, the last one pressed wins.
    for (let i = 0; i < values.length; i++) {
      await page.keyboard.press(String(i + 1));
      await expect(radios.nth(i)).toBeChecked();
    }
    const i = values.indexOf(key.answer(id, values));
    await page.keyboard.press(String(i + 1));
    await expect(radios.nth(i)).toBeChecked();
    await page.keyboard.press("Enter");
    const fb = q.getByTestId("quiz-feedback");
    await expect(fb).toHaveAttribute("data-state", "right");
    // Once graded the digits change nothing.
    await page.keyboard.press(String(i === 0 ? 2 : 1));
    await expect(radios.nth(i)).toBeChecked();
    asked++;
    await page.keyboard.press("Enter");
    await expect(page.locator(`[data-testid="quiz-question"][data-qid="${id}"]`)).toBeHidden();
  }
  await expect(summary).toBeVisible();
  await expect(summary).toHaveAttribute("data-correct", String(asked));
});

test("digits typed into a fill-in answer stay in the text box", async ({ page }) => {
  await register(page, newAccount("practice-keys-fill"), TEST_COURSE);
  await page.goto("/practice?lesson=numeric");
  const q = page.locator('[data-testid="quiz-question"]:visible');
  const box = q.getByTestId("quiz-answer");
  await expect(box).toBeFocused();
  await page.keyboard.type("12");
  await expect(box).toHaveValue("12");
  // Enter in the box checks the answer, as before.
  await box.fill(key.answer((await q.getAttribute("data-qid"))!));
  await page.keyboard.press("Enter");
  await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "right");
});
