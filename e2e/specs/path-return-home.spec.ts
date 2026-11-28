// The learner path (docs/features-learner-path.md, "Back to the course home"):
// an activity started from the course home ends there. The end of a Study, a
// Review and a Play has one main button, "Back to {course}", which opens `/`,
// where the finished activity is under "Done today".
import { AnswerKey } from "../lib/key.ts";
import { playStage } from "../lib/answer.ts";
import { playMatching } from "../lib/games.ts";
import { playSprintRight } from "../lib/sprint-play.ts";
import { expect, test } from "../lib/fixtures.ts";
import { devLogin, devReset } from "../lib/app.ts";

const key = new AnswerKey("spanish");

test.beforeEach(async ({ page }) => {
  await devReset(page);
});

test("Study, Review and Play each end with Back to Spanish, and home lists them done", async ({ page }) => {
  await devLogin(page, "open=spanish");
  await page.goto("/");
  const next = page.getByTestId("path-next");
  await expect(next).toHaveAttribute("data-kind", "study");
  const lesson = (await next.getAttribute("data-lesson"))!;
  const done = (kind: string) => page.locator(`[data-testid="path-done"][data-lesson="${lesson}"][data-kind="${kind}"]`);

  // Study: the stage's way out goes home, and so does its end.
  await page.getByTestId("path-start").click();
  await expect(page.getByTestId("stage")).toBeVisible();
  await expect(page.getByTestId("stage-leave-home")).toHaveAttribute("href", /\/$/);
  await playStage(page, key);
  const back = page.getByTestId("activity-back-home");
  await expect(back).toHaveText("Back to Spanish");
  await expect(page.getByTestId("stage-done").locator(".btn:not(.ghost)")).toHaveCount(1);
  await back.click();
  await expect(page).toHaveURL(/\/$/);
  await expect(done("study")).toBeVisible();
  await expect(next).toHaveAttribute("data-kind", "review");
  await expect(next).toHaveAttribute("data-lesson", lesson);

  // Review: quitting and finishing both go home.
  const review = (await page.getByTestId("path-start").getAttribute("href"))!;
  await page.goto(`${review}&reduce-motion=1`);
  await expect(page.getByTestId("sprint-play-exit")).toHaveAttribute("href", /\/$/);
  await playSprintRight(page, key);
  await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");
  await expect(page.getByTestId("sprint-play-done-home")).toHaveCount(0);
  await page.getByTestId("activity-back-home").click();
  await expect(page).toHaveURL(/\/$/);
  await expect(done("review")).toContainText("Back in 1 day");

  // Play: the lesson's Word Match board.
  await expect(next).toHaveAttribute("data-kind", "play");
  await expect(next).toHaveAttribute("data-lesson", lesson);
  await page.getByTestId("path-start").click();
  await playMatching(page, key);
  await expect(page.getByTestId("matching-again")).toHaveClass(/ghost/);
  await page.getByTestId("activity-back-home").click();
  await expect(page).toHaveURL(/\/$/);
  await expect(done("play")).toBeVisible();
});
