// Practice (#405): no option looks chosen before you pick one, and Check with
// no answer says so in the page's own feedback area, not in the browser's
// validation bubble. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { TEST_COURSE } from "../../lib/app.ts";
import { answerQuestion } from "../../lib/answer.ts";
import { AnswerKey } from "../../lib/key.ts";
import { newAccount, register } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);

test("Check with no answer asks for one inline, on a choice and on a typed question", async ({ page }) => {
  await register(page, newAccount("practice-empty"), TEST_COURSE);
  for (const [slug, ask] of [["multiple-choice", "Pick an answer first."], ["fill-in-blank", "Type an answer first."]]) {
    await page.goto(`/practice?lesson=${slug}`);
    const q = page.locator('[data-testid="quiz-question"]:visible');
    await expect(q).toHaveCount(1);
    // The form validates itself: no native bubble.
    await expect(q).toHaveAttribute("novalidate", "");
    if (slug === "multiple-choice") {
      // Nothing is checked, and no radio has focus (a focused radio looks chosen).
      await expect(q.locator("input[type=radio]:checked")).toHaveCount(0);
      expect(await page.evaluate(() => (document.activeElement as HTMLInputElement | null)?.type)).not.toBe("radio");
    }
    const fb = q.getByTestId("quiz-feedback");
    await q.getByTestId("quiz-check").click();
    await expect(fb).toBeVisible();
    await expect(fb).toHaveAttribute("data-state", "empty");
    await expect(fb).toHaveText(ask);
    // Nothing was sent: Check is still there, no Next.
    await expect(q.getByTestId("quiz-check")).toBeEnabled();
    await expect(q.getByTestId("quiz-next")).toHaveCount(0);
    // Enter while the line shows doesn't skip the question either.
    const qid = await q.getAttribute("data-qid");
    await (slug === "fill-in-blank" ? q.locator("input.fill") : q.getByTestId("quiz-check")).focus();
    await page.keyboard.press("Enter");
    await expect(fb).toHaveAttribute("data-state", "empty");
    await expect(page.locator('[data-testid="quiz-question"]:visible')).toHaveAttribute("data-qid", qid!);

    // Answering clears the line, and Check then grades as usual.
    await answerQuestion(q, key);
    await expect(fb).toBeHidden();
    await q.getByTestId("quiz-check").click();
    await expect(fb).toHaveAttribute("data-state", "right");
  }
});
