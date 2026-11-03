// Exact spec for a fact check through Use AI (#1238). The feedback asks for
// a check, so the plan sends every lesson back to be read for errors
// (revise-plan.json `check`, check-lesson.json): "What a fraction is" comes
// back with one fix, "Simplifying" unchanged with a Check line, "Adding
// fractions" unchanged. Review AI changes lists only the fixed lesson, with
// its why line above the diff, and the Check line under "Looked at, left
// alone". Fixtures only.
import { expect, test } from "@playwright/test";
import { aiMode, devLogin, devReset, generateCourse, generationEnds } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a fact check shows a why line under the fixed lesson and the Check lines of the rest", async ({ page }) => {
  await generateCourse(page, "Fractions to fact check");
  expect(await generationEnds(page)).toBe("done");
  const id = (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;

  await page.goto(`/courses/${id}`);
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill("Check the facts in every lesson and fix what is wrong [fixture:check]");
  await page.getByTestId("course-ai-submit").click();
  const review = page.getByTestId("course-review-modal");
  await expect(review).toHaveAttribute("data-after-job", "true", { timeout: 30_000 });

  // Only the fixed lesson is a change; the two the check left alone are not.
  const files = review.getByTestId("course-review-file");
  await expect(files).toHaveCount(1);
  await expect(files.getByTestId("course-review-file-title")).toHaveText("What a fraction is");
  // Its why line, above its diff.
  const why = files.getByTestId("course-review-why-line");
  await expect(why).toHaveCount(1);
  await expect(why).toBeVisible();
  await expect(why).toContainText('Changed "3 parts are taken" to "3 of the 4 parts are taken"');
  const whyBox = await files.getByTestId("course-review-why").boundingBox();
  const diffBox = await files.getByTestId("course-review-split").boundingBox();
  expect(whyBox!.y).toBeLessThan(diffBox!.y);

  // "Simplifying" was looked at and left alone, with what the check was unsure of.
  const alone = review.getByTestId("course-review-left-alone");
  await expect(alone).toContainText("Looked at, left alone");
  const lesson = alone.getByTestId("course-review-left-alone-lesson");
  await expect(lesson).toHaveCount(1);
  await expect(lesson).toContainText("Simplifying");
  await expect(lesson.getByTestId("course-review-check-line")).toHaveText('Check: "Multiply back to check your answer" is a habit, not a rule; left as it is.');
});
