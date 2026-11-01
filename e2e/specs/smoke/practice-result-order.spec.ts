// Practice (#406): after Check the result comes first and the action it leads
// to after it (result box, then Next), and the Hint toggle is gone once the
// answer is graded. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { TEST_COURSE } from "../../lib/app.ts";
import { answerQuestion } from "../../lib/answer.ts";
import { AnswerKey } from "../../lib/key.ts";
import { newAccount, register } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);

test("the result box sits above Next, and Hint goes once the answer is graded", async ({ page }) => {
  await register(page, newAccount("practice-order"), TEST_COURSE);
  await page.goto("/practice?lesson=multiple-choice");
  const summary = page.getByTestId("quiz-summary");
  let withHints = 0;
  for (let n = 0; n < 20 && !(await summary.isVisible()); n++) {
    const q = page.locator('[data-testid="quiz-question"]:visible');
    await expect(q).toHaveCount(1);
    const hints = q.locator("details.hints");
    const hasHints = (await hints.count()) > 0;
    if (hasHints) {
      withHints++;
      await expect(hints).toBeVisible();
    }
    // Wrong on purpose: the longest result box (verdict, correct answer, explanation).
    await answerQuestion(q, key, true);
    await q.getByTestId("quiz-check").click();
    const fb = q.getByTestId("quiz-feedback");
    await expect(fb).toHaveAttribute("data-state", "wrong");
    const next = q.getByTestId("quiz-next");
    const [fbBox, nextBox] = [await fb.boundingBox(), await next.boundingBox()];
    expect(fbBox!.y + fbBox!.height, "the result box ends above Next").toBeLessThanOrEqual(nextBox!.y);
    if (hasHints) await expect(hints).toBeHidden();
    await next.click();
  }
  await expect(summary).toBeVisible();
  expect(withHints, "questions with a hint in the round").toBeGreaterThan(0);
});
