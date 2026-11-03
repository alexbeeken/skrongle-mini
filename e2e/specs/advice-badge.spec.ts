// Exact spec for the course editor's advice badge (#1235): a lesson whose
// question has no explanation gets a yellow ! on its row, read from the
// saved file even with its drawer shut; the badge opens "Things to check",
// whose row opens that question's explanation field in Forms mode; writing
// the explanation takes the badge away. Against a copy of the test course.
import { expect, test } from "@playwright/test";
import { copyCourse, devLogin, devReset, editorSaved, fastAutosave, TEST_COURSE } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await fastAutosave(page);
  await devLogin(page, `open=${TEST_COURSE}`);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("a question without an explanation puts a yellow ! on its row, and the list opens its field", async ({ page }) => {
  await copyCourse(page, TEST_COURSE);
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Multiple choice" }).first();
  const badge = row.getByTestId("course-lessons-advice");
  // Every question of this lesson has its explanation: no badge.
  await expect(row.getByTestId("course-lessons-expand")).toBeVisible();
  await expect(badge).toBeHidden();

  // The second question loses its explanation (in JSON); the row's badge says so as it is typed.
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"quiz"/);
  const lesson = JSON.parse(await json.inputValue());
  delete lesson.quiz[1].explanation;
  await json.fill(JSON.stringify(lesson, null, 2));
  await json.blur();
  await editorSaved(page);
  await expect(badge).toBeVisible();
  await expect(badge).toHaveAttribute("aria-label", "1 thing to check");

  // Drawn again with every drawer shut: the badge comes from the saved file.
  await page.goto(page.url());
  await expect(badge).toBeVisible();
  await expect(row.getByTestId("course-lessons-drawer")).toBeHidden();
  // The lessons bar has none.
  await expect(page.locator(".lt-bar [data-testid=course-lessons-advice]")).toHaveCount(0);

  // The badge opens the list, at this lesson.
  await badge.click();
  const dialog = page.getByTestId("course-advice-dialog");
  await expect(dialog).toBeVisible();
  const lessonId = (await row.getAttribute("data-lesson-id"))!;
  const group = dialog.locator(`[data-testid=course-advice-lesson][data-lesson-id="${lessonId}"]`);
  await expect(group).toContainText("Multiple choice");
  const item = group.locator('[data-testid=course-advice-item][data-path="quiz.1.explanation"]');
  await expect(item).toContainText("Quiz 2");
  await expect(item).toContainText("No explanation yet");
  // Its yellow ! sits at the start of its own row, not pinned to the dialog's edge.
  const mark = (await item.locator(".field-check.advice").boundingBox())!;
  const box = (await item.boundingBox())!;
  expect(mark.x).toBeLessThan(box.x + 30);
  expect(mark.y).toBeGreaterThanOrEqual(box.y - 1);
  expect(mark.y + mark.height).toBeLessThanOrEqual(box.y + box.height + 1);

  // Its row opens the lesson in Forms mode with the explanation field focused.
  await item.click();
  await expect(dialog).toBeHidden();
  await expect(row.getByTestId("course-lessons-drawer")).toHaveAttribute("data-mode", "forms");
  const field = row.locator('[data-testid=course-form-field][data-path="quiz.1.explanation"]');
  await expect(field).toBeFocused();
  // The amber line under it shows one check.
  await expect(row.locator('.le-check[data-check="quiz.1.explanation"]')).toHaveAttribute("data-level", "warn");

  // Writing the explanation takes the badge away, and the save keeps it away.
  await field.fill("Each option is a colour; only one matches.");
  await expect(badge).toBeHidden();
  await field.blur();
  await editorSaved(page);
  await page.goto(page.url());
  await page.waitForLoadState("networkidle");
  await expect(badge).toBeHidden();
});
