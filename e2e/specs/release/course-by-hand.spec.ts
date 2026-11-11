// A course made without AI (#40): create it, name it, add a lesson in the
// editor, let it autosave, and open it as a learner. No key is needed.
import { createCourse, editorSaved, fastAutosave } from "../../lib/app.ts";
import { expect, newLearner, test } from "../../lib/release.ts";

test("a course made by hand: add a lesson in the editor, it autosaves, and opens as a course", async ({ browser }) => {
  const { page } = await newLearner(browser, "by-hand");
  await fastAutosave(page);
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();

  // Rename the course, add a lesson by name.
  await page.getByTestId("course-edit-title").click();
  await page.getByTestId("course-edit-rename-input").fill("Tea by hand");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("course-edit-title")).toHaveText("Tea by hand");
  await page.getByTestId("course-lessons-add-here").first().click();
  await page.getByTestId("course-lessons-rename").fill("Brewing tea");
  await page.keyboard.press("Enter");
  await editorSaved(page);

  // Write into its JSON; it saves as it is typed, with no Save button.
  const row = page.locator('[data-testid="course-lessons-row"]', { hasText: "Brewing tea" });
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"id": "brewing-tea"/);
  const lesson = JSON.parse(await json.inputValue());
  lesson.summary = "Water, leaves, patience.";
  await json.fill(JSON.stringify(lesson, null, 2));
  await editorSaved(page);
  await page.reload();
  await expect(page.getByTestId("course-lessons-title").filter({ hasText: "Brewing tea" })).toHaveCount(1);

  // Open it as a learner: it is on the list, and its lessons are the learner's.
  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toContainText("Tea by hand");
  await page.request.post(`/courses/open/${id}`, { maxRedirects: 0 });
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item").filter({ hasText: "Brewing tea" })).toHaveCount(1);
  expect((await page.goto("/lessons/brewing-tea"))?.status()).toBe(200);
  await expect(page.getByTestId("lesson")).toBeVisible();
});
