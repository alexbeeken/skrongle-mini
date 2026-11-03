// The reward line (#568): the end of a lesson or session, in every mode, has
// one short line from the motivator worn (src/motivators.rs), in the same
// element everywhere. Statistics and metrics, the default, says what the
// run did for the lesson, today's effort against the daily goal and the
// streak ("✓ Passed · 14 / 20 today · 🔥 1").
import type { Locator } from "@playwright/test";
import { AnswerKey } from "../../lib/key.ts";
import { passReadingCheck, playPractice, playStage } from "../../lib/answer.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, newAccount, register } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);

/** The one reward line inside `where`, worn by the default motivator. */
async function rewardIn(where: Locator): Promise<Locator> {
  const line = where.getByTestId("motivator-reward");
  await expect(line).toHaveCount(1);
  await expect(line).toHaveAttribute("data-motivator", "stats");
  await expect(line).toContainText(/\d+ \/ 20 today|Daily goal met/);
  return line;
}

test("every mode's end screen shows the worn motivator's reward line", async ({ page }) => {
  await register(page, newAccount("reward"), COURSE);
  const lessons = (await apiProgress(page)).filter((l) => l.required_answers >= 2);
  const [a, b] = lessons;

  // Study mode: the reading check earns effort points, so it gets a line.
  await page.goto(`/lessons/${a.slug}`);
  await passReadingCheck(page, key);
  await rewardIn(page.getByTestId("lesson-check-result"));

  // Sprint mode: a lesson session all right passes the lesson.
  await page.goto(`/sprint/play?lesson=${a.slug}&reduce-motion=1`);
  await playSprintRight(page, key);
  const done = page.getByTestId("sprint-play-done");
  await expect(done).toHaveAttribute("data-saved", "true");
  const sprintLine = await rewardIn(done);
  await expect(sprintLine).toContainText(/✓ Passed|Unit finished/);

  // Study mode again: a practice round on another lesson, all right.
  await page.goto(`/practice?lesson=${b.slug}`);
  await playPractice(page, key);
  await rewardIn(page.getByTestId("quiz-summary"));

  // The Guide: replaying the passed lesson's stage (a stage further on is locked).
  await page.goto(`/guide/${a.slug}`);
  await playStage(page, key);
  await rewardIn(page.getByTestId("stage-done"));
});
