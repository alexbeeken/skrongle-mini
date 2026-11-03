// The mistakes list (#338): a quiz question answered wrong shows on
// /mistakes with your answer and the right one, until you answer it right.
import { expect, test } from "@playwright/test";
import { answerQuestion } from "../lib/answer.ts";
import { AnswerKey } from "../lib/key.ts";
import { newAccount, register } from "../lib/learner.ts";

const key = new AnswerKey("spanish");

test("a wrong quiz answer shows on the mistakes list, and a right one clears it (#338)", async ({ page }) => {
  await register(page, newAccount("mistakes"), "spanish");

  await page.goto("/mistakes");
  await expect(page.getByTestId("mistakes-empty")).toBeVisible();
  await expect(page.getByTestId("mistake")).toHaveCount(0);

  // Answer one Practice question wrong.
  await page.goto("/practice?lesson=adjectives-agreement-position");
  const q = page.locator('[data-testid="quiz-question"]:visible');
  await expect(q).toHaveCount(1);
  const id = (await q.getAttribute("data-qid"))!;
  const given = await answerQuestion(q, key, true);
  await q.getByTestId("quiz-check").click();
  await expect(q.getByTestId("quiz-feedback")).toHaveAttribute("data-state", "wrong");

  // The list, from the Study mode bar's Mistakes tab (#1263).
  await page.getByTestId("nav-study-mistakes").click();
  await expect(page).toHaveURL(/\/mistakes$/);
  await expect(page.getByTestId("nav-study-mistakes")).toHaveAttribute("aria-current", "page");
  const row = page.locator(`[data-testid="mistake"][data-question-id="${id}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-lesson-id", "adjectives-agreement-position");
  await expect(row.getByTestId("mistake-prompt")).toHaveText(key.question(id).prompt);
  await expect(row.getByTestId("mistake-yours")).toHaveText(given);
  await expect(row.getByTestId("mistake-right")).toHaveText(key.question(id).correct_answer);

  // The (i) explains the list.
  await page.getByTestId("mistakes-info").click();
  await expect(page.getByTestId("mistakes-info-dialog")).toBeVisible();
  await page.getByTestId("mistakes-info-close").click();
  await expect(page.getByTestId("mistakes-info-dialog")).toBeHidden();

  // Answered right since: off the list.
  const r = await page.request.post("/api/answer", { data: { question_id: id, answer: key.question(id).correct_answer } });
  expect(r.ok()).toBe(true);
  await page.goto("/mistakes");
  await expect(page.getByTestId("mistake")).toHaveCount(0);
  await expect(page.getByTestId("mistakes-empty")).toBeVisible();
});
