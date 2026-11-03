// Practice with nothing to practise (#682): a learner who has read no lesson
// gets "Read next" and no question counter (it used to read "1 / 0"). A round
// with questions still shows the counter. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { TEST_COURSE } from "../../lib/app.ts";
import { newAccount, register } from "../../lib/learner.ts";

test("a fresh learner's practice shows Read next and no counter", async ({ page }) => {
  await register(page, newAccount("practice-none"), "spanish");
  await page.goto("/practice");
  await expect(page.getByTestId("quiz-read-next")).toBeVisible();
  // By class, not testid: main had no testid on the counter.
  await expect(page.locator(".quiz-progress")).toHaveCount(0);
  await expect(page.locator("#q-bar")).toHaveCount(0);
});

test("a round with questions still shows the counter", async ({ page }) => {
  await register(page, newAccount("practice-some"), TEST_COURSE);
  await page.goto("/practice?lesson=multiple-choice");
  await expect(page.locator('[data-testid="quiz-question"]:visible')).toHaveCount(1);
  await expect(page.getByTestId("quiz-counter")).toHaveText(/^1 \/ [1-9]\d*$/);
});
