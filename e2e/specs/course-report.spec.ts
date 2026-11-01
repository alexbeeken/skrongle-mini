// Exact spec for the course editor's feature usage report (#328): the round
// bar-chart button beside the gear opens a modal listing, per unit and
// lesson, the question types and lesson features the lesson uses.
import { expect, test } from "@playwright/test";
import { devLogin, devReset, editorSaved, fastAutosave, generateCourse, generationEnds } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("the report shows which features each lesson uses", async ({ page }) => {
  await generateCourse(page, "Fractions feature report");
  expect(await generationEnds(page)).toBe("done");
  const report = page.getByTestId("course-report");
  await expect(report).toBeHidden();
  await page.getByTestId("course-report-open").click();
  await expect(report).toBeVisible();

  // The first unit is open, the others folded; it counts its lessons.
  const units = report.getByTestId("course-report-unit");
  await expect(units.first()).toHaveAttribute("open", "");
  await expect(units.first().getByTestId("course-report-unit-counts")).toContainText(/\d+ lessons? · \d+ of \d+ features · \d+ of 6 question types/);

  // A lesson opens to its two lists; its text is used, listening is not.
  const lesson = units.first().getByTestId("course-report-lesson").first();
  await lesson.locator("summary").click();
  await expect(lesson.locator('[data-testid=course-report-feature][data-key="text"]')).toHaveAttribute("data-used", "true");
  await expect(lesson.locator('[data-testid=course-report-feature][data-key="listening"]')).toHaveAttribute("data-used", "false");
  expect(await lesson.locator('[data-testid=course-report-question][data-used="true"]').count()).toBeGreaterThan(0);

  // Close shuts it.
  await report.getByTestId("course-report-close").click();
  await expect(report).toBeHidden();
});

test("the report fits a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await generateCourse(page, "Fractions report phone");
  expect(await generationEnds(page)).toBe("done");
  await page.getByTestId("course-report-open").click();
  const report = page.getByTestId("course-report");
  await expect(report).toBeVisible();
  await report.getByTestId("course-report-lesson").first().locator("summary").click();
  const box = (await report.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(390);
  expect(await report.evaluate((d) => d.scrollWidth <= d.clientWidth)).toBe(true);
});

test("the report follows a save", async ({ page }) => {
  await fastAutosave(page);
  await generateCourse(page, "Fractions report after a save");
  expect(await generationEnds(page)).toBe("done");
  const row = page.getByTestId("course-lessons-row").first();
  const id = (await row.getAttribute("data-lesson-id"))!;
  const summary = page.locator(`[data-testid=course-report-lesson][data-lesson="${id}"] [data-testid=course-report-feature][data-key="summary"]`);
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"id"/);
  const lesson = JSON.parse(await json.inputValue());
  lesson.summary = "";
  await json.fill(JSON.stringify(lesson, null, 2));
  await json.blur();
  await editorSaved(page);
  await expect(summary).toHaveAttribute("data-used", "false");
  lesson.summary = "Now it has one.";
  await json.fill(JSON.stringify(lesson, null, 2));
  await json.blur();
  await editorSaved(page);
  // The page is drawn again after the save: the report says so without a reload.
  await expect(summary).toHaveAttribute("data-used", "true");
});
