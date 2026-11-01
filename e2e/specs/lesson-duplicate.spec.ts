// Duplicate a lesson (#454): the ⧉ on a lesson row of the course editor
// copies it as "<title> (copy)" right below, with a fresh id, and saves it
// with the table (POST /api/courses/{id}/lessons). Against a copy of the
// test course, whose "Everything at once" lesson has questions,
// conversations, a guide, themes and conjugations, so the copy has every
// id that must stay unique in the course.
import { expect, test, type Page } from "@playwright/test";
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

const ORIGINAL = "Everything at once";
const COPY = `${ORIGINAL} (copy)`;

function row(page: Page, title: string) {
  return page.getByTestId("course-lessons-row").filter({ has: page.getByTestId("course-lessons-title").getByText(title, { exact: true }) });
}

async function file(page: Page, course: string, path: string) {
  return JSON.parse(((await (await page.request.get(`/api/courses/${course}/file?path=${encodeURIComponent(path)}`)).json()) as { content: string }).content);
}

test("duplicate a lesson: the copy sits below the original, with new ids, and the course still loads", async ({ page }) => {
  const id = await copyCourse(page, TEST_COURSE);
  const titles = page.getByTestId("course-lessons-title");
  const before = (await titles.allTextContents()).map((t) => t.trim());
  const at = before.indexOf(ORIGINAL);
  expect(at).toBeGreaterThanOrEqual(0);
  const original = row(page, ORIGINAL);
  const originalId = (await original.getAttribute("data-lesson-id"))!;
  const originalFile = await file(page, id, (await original.getAttribute("data-path"))!);

  await original.getByTestId("course-lessons-duplicate").click();
  await expect(titles.nth(at + 1)).toHaveText(COPY);
  await editorSaved(page);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  const copy = row(page, COPY);
  await expect(copy).toHaveAttribute("data-lesson-id", `${originalId}-copy`);
  await expect(copy).toHaveAttribute("data-unit", (await original.getAttribute("data-unit"))!);

  // The saved copy: the original's lesson under new ids; the original is as it was.
  const saved = await file(page, id, (await copy.getAttribute("data-path"))!);
  expect(saved.title).toBe(COPY);
  expect(saved.id).toBe(`${originalId}-copy`);
  expect(saved.content).toBe(originalFile.content);
  expect(saved.vocabulary).toEqual(originalFile.vocabulary);
  expect(saved.quiz.length).toBe(originalFile.quiz.length);
  for (const [i, q] of saved.quiz.entries()) expect(q.question_id).toBe(`${originalFile.quiz[i].question_id}-copy`);
  for (const [i, c] of (saved.conversations ?? []).entries()) expect(c.id).toBe(`${originalFile.conversations[i].id}-copy`);
  expect(saved.conjugations ?? []).toEqual([]);
  const after = await file(page, id, (await original.getAttribute("data-path"))!);
  expect(after.title).toBe(ORIGINAL);
  expect(after.quiz).toEqual(originalFile.quiz);

  // It stays put after a reload, and a second copy takes the next free id.
  await page.goto(`/courses/${id}`);
  await expect(titles.nth(at)).toHaveText(ORIGINAL);
  await expect(titles.nth(at + 1)).toHaveText(COPY);
  await row(page, ORIGINAL).getByTestId("course-lessons-duplicate").click();
  await expect(titles.nth(at + 1)).toHaveText(COPY);
  await expect(titles.nth(at + 2)).toHaveText(COPY);
  await editorSaved(page);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  await expect(page.getByTestId("course-lessons-row").nth(at + 1)).toHaveAttribute("data-lesson-id", `${originalId}-copy-2`);
  await expect(titles).toHaveCount(before.length + 2);
});
