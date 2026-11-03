// Exact spec for the course lints (#445): the top of the editor's feature
// usage report lists things to check, cheap checks with no AI, and the
// report's button counts them. A prerequisite placed after the lesson that
// needs it is one; the spec makes one, sees it, and clears it again. The
// second test sees the same finding in the lesson editor (#1199): an amber
// ! in the corner of the prerequisites after the save, whose modal says it,
// gone after the fix; the ! takes no room, so the form never moves.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, devReset, editorSaved, fastAutosave, generateCourse, generationEnds } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

/** Rewrite the prerequisites of the lesson in `row` through its JSON drawer, and wait for the save. */
async function setPrerequisites(page: Page, rowId: string, prerequisites: string[]): Promise<void> {
  const row = page.locator(`[data-testid=course-lessons-row][data-lesson-id="${rowId}"]`);
  const json = row.getByTestId("course-lessons-json");
  if (!(await json.isVisible())) await row.getByTestId("course-lessons-expand").click();
  await expect(json).toHaveValue(/"id"/);
  const lesson = JSON.parse(await json.inputValue());
  lesson.prerequisites = prerequisites;
  await json.fill(JSON.stringify(lesson, null, 2));
  await json.blur();
  await editorSaved(page);
}

test("a prerequisite after its lesson shows up as a thing to check", async ({ page }) => {
  await fastAutosave(page);
  await generateCourse(page, "Fractions lint check");
  expect(await generationEnds(page)).toBe("done");
  const rows = page.getByTestId("course-lessons-row");
  expect(await rows.count()).toBeGreaterThanOrEqual(3);
  // The fixture's first lesson already lists the second as a prerequisite,
  // so the second and third are used: the row is new, not already there.
  const second = (await rows.nth(1).getAttribute("data-lesson-id"))!;
  const third = (await rows.nth(2).getAttribute("data-lesson-id"))!;
  const lint = page.locator(`[data-testid=course-lint][data-lesson="${second}"][data-kind="prerequisite-later"]`);
  await expect(lint).toHaveCount(0);

  // The second lesson needs the third: a thing to check, counted on the button.
  await setPrerequisites(page, second, [third]);
  await expect(page.getByTestId("course-report-lint-count")).toBeVisible();
  await page.getByTestId("course-report-open").click();
  const report = page.getByTestId("course-report");
  await expect(report).toBeVisible();
  await expect(lint).toHaveCount(1);
  await expect(lint).toContainText("comes after it");

  // The (i) says what these checks are; Close goes back to the report.
  await report.getByTestId("course-lints-info").click();
  const info = page.getByTestId("course-lints-info-dialog");
  await expect(info).toBeVisible();
  await expect(info).toContainText("None of them stops a save");
  await info.getByTestId("course-lints-info-close").click();
  await expect(info).toBeHidden();
  await report.getByTestId("course-report-close").click();
  await expect(report).toBeHidden();

  // Without the prerequisite the row is gone after the save, without a reload.
  await setPrerequisites(page, second, []);
  await expect(lint).toHaveCount(0);
});

test("the lesson editor shows a lint as an amber ! on its field after the save, opens it in a modal, and drops it after the fix", async ({ page }) => {
  await fastAutosave(page);
  await page.addInitScript(() => localStorage.setItem("lt-mode", "forms"));
  await generateCourse(page, "Fractions lint advice");
  expect(await generationEnds(page)).toBe("done");
  const rows = page.getByTestId("course-lessons-row");
  expect(await rows.count()).toBeGreaterThanOrEqual(3);
  const second = rows.nth(1);
  const third = (await rows.nth(2).getAttribute("data-lesson-id"))!;
  await second.getByTestId("course-lessons-expand").click();
  const form = second.getByTestId("course-form");
  await expect(form.getByTestId("course-form-layout")).toBeVisible();
  // The lesson's own fields are open first; nothing amber on the prerequisites yet.
  const item = form.getByTestId("course-item");
  await expect(item).toHaveAttribute("data-sec", "lesson");
  const mark = item.locator('[data-testid="course-form-lint"][data-path="prerequisites"]');
  await expect(mark).toHaveCount(0);
  // What has no field of its own (the fixture's 3-question quiz) is the ! on the form's title.
  const dialog = page.getByTestId("course-form-lint-dialog");
  const advice = item.getByTestId("course-form-advice");
  await expect(advice).toHaveAttribute("data-kinds", /few-questions/);
  await advice.click();
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("questions");
  await dialog.getByTestId("course-form-lint-close").click();
  await expect(dialog).toBeHidden();

  // The second lesson needs the third: saved, the report is drawn again and the ! appears, with nothing moved.
  const hint = item.locator(".le-ids");
  await form.getByTestId("course-form-add-prereq").selectOption(third);
  await editorSaved(page);
  await expect(mark).toBeVisible();
  await expect(mark).toHaveAttribute("data-kinds", "prerequisite-later");
  await expect(advice).not.toHaveAttribute("data-kinds", /prerequisite-later/);
  // The ! takes no room: without it the form below sits exactly where it does with it.
  const withMark = (await hint.boundingBox())!.y;
  await mark.evaluate((b) => { (b as HTMLElement).style.display = "none"; });
  expect((await hint.boundingBox())!.y).toBe(withMark);
  await mark.evaluate((b) => { (b as HTMLElement).style.display = ""; });
  await mark.click();
  await expect(dialog).toContainText("comes after it");
  await dialog.getByTestId("course-form-lint-close").click();

  // Drop the prerequisite: after the save the ! is gone again.
  await form.locator('[data-testid="course-form-remove"][data-path="prerequisites.0"]').click();
  await editorSaved(page);
  await expect(mark).toHaveCount(0);
});
