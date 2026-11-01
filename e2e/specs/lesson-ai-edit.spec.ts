// Exact specs for Edit with AI on a lesson's row (#1242): the row's ✦ opens a
// small dialog, its request rewrites that one lesson in one model call
// (data/test-fixtures/llm/edit-lesson.json, picked by a [fixture:NAME] token
// in the request), and the editor follows the job in its modal like any Use
// AI run. Only that lesson changes; a reply that renames it is refused.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, devReset, fastAutosave, generateCourse, generationEnds } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await fastAutosave(page);
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const modal = (page: Page) => page.getByTestId("course-ai-modal");
const row = (page: Page, title: string) => page.getByTestId("course-lessons-row").filter({ has: page.getByTestId("course-lessons-title").getByText(title, { exact: true }) });

/** Generate the fixture course (3 lessons) and stay on its editor. Returns its id. */
async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

/** The course's files as the server has them, path → content. */
async function files(page: Page, id: string): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  for (const path of ["course.json", "lessons/what-is-a-fraction.json", "lessons/simplifying.json", "lessons/adding.json"]) {
    const r = await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`);
    expect(r.ok(), path).toBe(true);
    out[path] = (await r.json()).content;
  }
  return out;
}

/** Open the row's Edit with AI, ask for `request` and send it. */
async function editWithAi(page: Page, title: string, request: string) {
  await row(page, title).getByTestId("course-lessons-ai-edit").click();
  const dialog = page.getByTestId("course-lessons-ai-dialog");
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("course-lessons-ai-title")).toHaveText(`Edit “${title}” with AI`);
  await expect(page.getByTestId("course-lessons-ai-count")).toHaveText("Up to 4,000 characters. Only this lesson changes.");
  await expect(page.getByTestId("course-lessons-ai-provider")).toBeVisible();
  await page.getByTestId("course-lessons-ai-text").fill(request);
  await page.getByTestId("course-lessons-ai-submit").click();
}

test("Edit with AI on a row rewrites that lesson only, and Undo takes it back", async ({ page }) => {
  const id = await makeCourse(page, "Fractions, one lesson edited");
  // Every saved row has the icon, beside Copy and Delete.
  await expect(page.getByTestId("course-lessons-ai-edit")).toHaveCount(3);
  await expect(row(page, "Simplifying").getByTestId("course-lessons-ai-edit")).toHaveAttribute("aria-label", "Edit Simplifying with AI");
  const before = await files(page, id);

  // Cancel closes the dialog and sends nothing.
  await row(page, "Simplifying").getByTestId("course-lessons-ai-edit").click();
  await page.getByTestId("course-lessons-ai-cancel").click();
  await expect(page.getByTestId("course-lessons-ai-dialog")).toBeHidden();

  await editWithAi(page, "Simplifying", "More worked examples, please [fixture:slow]");
  await expect(modal(page)).toHaveAttribute("data-editing", "true");
  await expect(page.getByTestId("course-ai-modal-what")).toHaveText("Editing “Simplifying”…");
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId("course-review-label")).toContainText("Edited with feedback");
  await page.getByTestId("course-review-close").click();
  await expect(page.getByTestId("course-undo-label")).toHaveText("Edited with feedback: “More worked examples, please [fixture:slow]”");

  const after = await files(page, id);
  for (const path of Object.keys(before)) {
    if (path === "lessons/simplifying.json") {
      expect(after[path]).toContain("edited from your request");
      expect(JSON.parse(after[path])).toMatchObject({ id: "u1-simplify", slug: "simplifying", unit: "1" });
    } else expect(after[path], path).toBe(before[path]);
  }

  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  expect(await files(page, id)).toEqual(before);
});

test("a reply that renames the lesson is refused and the course stays as it was", async ({ page }) => {
  const id = await makeCourse(page, "Fractions, an edit refused");
  const before = await files(page, id);
  await editWithAi(page, "What a fraction is", "Rename it all [fixture:renames]");
  await expect(modal(page)).toHaveAttribute("data-state", "failed", { timeout: 30_000 });
  await expect(page.getByTestId("course-ai-modal-title")).toHaveText("Editing stopped");
  await expect(page.getByTestId("course-ai-modal-error")).toContainText("it changed the lesson's id");
  await expect(page.getByTestId("course-ai-modal-error")).toContainText("The course is unchanged.");
  await page.getByTestId("course-ai-modal-close").click();
  await expect(modal(page)).toHaveCount(0);
  expect(await files(page, id)).toEqual(before);
});

test("a refused request opens the dialog again with why", async ({ page }) => {
  await makeCourse(page, "Fractions, a short request");
  await row(page, "Adding fractions").getByTestId("course-lessons-ai-edit").click();
  // Too short: the server refuses it before any job starts (the box's own minimum is bypassed here).
  await page.getByTestId("course-lessons-ai-text").evaluate((t: HTMLTextAreaElement) => { t.removeAttribute("required"); t.value = "short"; });
  await page.getByTestId("course-lessons-ai-submit").click();
  await expect(page.getByTestId("course-lessons-ai-dialog")).toBeVisible();
  await expect(page.getByTestId("course-lessons-ai-error")).toBeVisible();
  await expect(page.getByTestId("course-lessons-ai-title")).toHaveText("Edit “Adding fractions” with AI");
  await expect(page.getByTestId("course-lessons-ai-text")).toHaveValue("short");
  await expect(page.getByTestId("course-ai-error")).toHaveCount(0);
});

test("a lesson gone since the page loaded: the dialog opens again and says so", async ({ page }) => {
  await makeCourse(page, "Fractions, a lesson gone");
  await row(page, "Adding fractions").getByTestId("course-lessons-ai-edit").click();
  // As if another tab deleted the lesson: the form names a file the course no longer has.
  await page.getByTestId("course-lessons-ai-lesson").evaluate((i: HTMLInputElement) => { i.value = "lessons/gone.json"; });
  await page.getByTestId("course-lessons-ai-text").fill("More worked examples, please");
  await page.getByTestId("course-lessons-ai-submit").click();
  await expect(page.getByTestId("course-lessons-ai-dialog")).toBeVisible();
  await expect(page.getByTestId("course-lessons-ai-error")).toHaveText("That lesson is no longer in the course. Reload the page and try again.");
  await expect(page.getByTestId("course-lessons-ai-title")).toHaveText("Edit with AI");
});

test("a lesson copied in the editor gets its ✦ once it is saved", async ({ page }) => {
  await makeCourse(page, "Fractions, a copied lesson");
  await row(page, "Adding fractions").getByTestId("course-lessons-duplicate").click();
  const copy = row(page, "Adding fractions (copy)");
  await expect(copy).not.toHaveAttribute("data-lesson-id", "");
  await expect(copy.getByTestId("course-lessons-ai-edit")).toBeVisible();
});
