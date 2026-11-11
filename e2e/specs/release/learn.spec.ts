// A learner's first lesson on a release server (#40): open the built-in Spanish
// course, pass the first lesson's reading check, run a Practice set and a Sprint
// session, and see the lesson list and the home page agree it is passed once
// (docs/progress.md). Answers are read from data/courses, as exact.spec.ts does.
import { passReadingCheck, playPractice } from "../../lib/answer.ts";
import { AnswerKey } from "../../lib/key.ts";
import { apiProgress, expectProgressAgrees, openCourse } from "../../lib/learner.ts";
import { expect, newLearner, test } from "../../lib/release.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";

test("a learner reads, practises and sprints the first Spanish lesson, and every page agrees", async ({ browser }) => {
  test.setTimeout(180_000);
  const key = new AnswerKey("spanish");
  const { page } = await newLearner(browser, "learn");
  await openCourse(page, "spanish");
  const first = (await apiProgress(page))[0].slug;

  // Nothing passed yet.
  await page.goto("/");
  await expect(page.getByTestId("home-map-progress")).toHaveAttribute("data-passed", "0");

  // Reading check.
  await page.goto(`/lessons/${first}`);
  await passReadingCheck(page, key);
  await page.reload();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "true");

  // Practice.
  await page.goto(`/practice?lesson=${first}`);
  const [right, total] = await playPractice(page, key);
  expect(right, "every practice answer right").toBe(total);

  // Sprint.
  await page.goto(`/sprint/play?lesson=${first}&reduce-motion=1`);
  const run = await playSprintRight(page, key);
  expect(run.wrong, "answers the app marked wrong").toEqual([]);
  await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");

  // One lesson passed, the same on every page.
  const lessons = await apiProgress(page);
  expect(lessons.filter((l) => l.passed).map((l) => l.slug)).toEqual([first]);
  await expectProgressAgrees(page);
  await page.goto("/lessons?unit=all");
  await expect(page.locator(`[data-testid="lessons-item"][data-lesson-id="${first}"]`)).toHaveAttribute("data-passed", "true");
  await expect(page.locator('[data-testid="lessons-item"][data-passed="true"]')).toHaveCount(1);
  await page.goto("/");
  await expect(page.getByTestId("home-map-progress")).toHaveAttribute("data-passed", "1");
});
