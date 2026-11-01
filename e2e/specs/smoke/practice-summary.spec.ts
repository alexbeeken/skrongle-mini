// Practice summary (#408): the change in mastery is said in words and only
// when there is one (no bare "(0)"), and a round too short to count
// says what it would count for. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { TEST_COURSE } from "../../lib/app.ts";
import { playPractice } from "../../lib/answer.ts";
import { AnswerKey } from "../../lib/key.ts";
import { newAccount, register } from "../../lib/learner.ts";

const key = new AnswerKey(TEST_COURSE);
const slug = "multiple-choice";

test("the mastery change is shown as (+N), and left out when nothing changed", async ({ page }) => {
  await register(page, newAccount("practice-summary"), TEST_COURSE);

  // All wrong from 0 %: mastery doesn't move, so no change is shown.
  await page.goto(`/practice?lesson=${slug}`);
  await playPractice(page, key, { wrong: () => true });
  let row = page.locator(`[data-testid="quiz-summary-lesson"][data-lesson-id="${slug}"]`);
  await expect(row).toContainText("mastery now 0%");
  await expect(row.locator(".delta")).toHaveCount(0);
  await expect(row).not.toContainText(/\(\d+\)/);

  // All right: mastery goes up, and the row says by how much.
  await page.goto(`/practice?lesson=${slug}`);
  await playPractice(page, key);
  row = page.locator(`[data-testid="quiz-summary-lesson"][data-lesson-id="${slug}"]`);
  const delta = row.locator(".delta.up");
  await expect(delta).toHaveCount(1);
  const d = Number(await delta.getAttribute("data-delta"));
  expect(d).toBeGreaterThan(0);
  await expect(delta).toHaveText(`(+${d})`);
  await expect(row).toContainText(`% (+${d})`);
});

test("a round too short to count says what it needs the answers for", async ({ page }) => {
  await register(page, newAccount("practice-short"), TEST_COURSE);
  // The server's verdict on the round, as for a lesson with only one question in a mixed round.
  await page.route("**/api/practice/finish", (route) =>
    route.fulfill({ json: { runs: [{ passed: false, passed_now: false, seen: true, run_counted: false, run_pct: 100, required_answers: 3 }] } }));
  await page.goto(`/practice?lesson=${slug}`);
  await playPractice(page, key);
  const row = page.locator(`[data-testid="quiz-summary-lesson"][data-lesson-id="${slug}"]`);
  await expect(row.locator(".pass-line")).toHaveText("· a round needs 3 answers on this lesson to count towards passing it");
});
