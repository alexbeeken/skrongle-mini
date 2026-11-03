// Hand edits to a generated course's files and revising with feedback (#27,
// followed in the course page's modal) share one set of course files. These specs cover where they meet: the course page while a
// revision runs, a hand edit meeting a revision, and Undo meeting a hand
// edit. Fixtures only.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, devReset, generateCourse, generationEnds, aiMode } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const modal = (page: Page) => page.getByTestId("course-ai-modal");

async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

async function revise(page: Page, id: string, feedback: string, { wait = true } = {}) {
  await page.goto(`/courses/${id}`);
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill(feedback);
  await page.getByTestId("course-ai-submit").click();
  // A fixture reply can be done before the page loads: then the review opens at once.
  if (wait) await expect(page.locator('[data-testid="course-review-modal"], [data-testid="course-ai-modal"][data-state="failed"]')).toBeVisible({ timeout: 30_000 });
  else await expect(modal(page)).toHaveAttribute("data-revision", "true");
}

/** The stored JSON of one file, read through the owner-only API. */
async function stored(page: Page, id: string, path: string): Promise<string> {
  const r = await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`);
  return (await r.json()).content;
}

/** Save one file through the owner-only API; returns the course's load error, if any. */
async function save(page: Page, id: string, path: string, content: string): Promise<string | null> {
  const r = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content }] } });
  expect(r.ok()).toBe(true);
  return (await r.json()).error;
}

async function editTitle(page: Page, id: string, path: string, title: string) {
  const json = JSON.parse(await stored(page, id, path));
  json.title = title;
  expect(await save(page, id, path, JSON.stringify(json, null, 2))).toBeNull();
}

test("while a revision runs the modal covers the locked workspace; closing the review shows the new lessons", async ({ page }) => {
  const id = await makeCourse(page, "Fractions panel during revision");
  await revise(page, id, "Take your time [fixture:slow]", { wait: false });
  await expect(modal(page)).toHaveAttribute("data-state", "running");
  // Under the modal: the usual workspace, inert, so nothing can be saved from it.
  await expect(page.getByTestId("course-edit-locked")).toHaveJSProperty("inert", true);
  await expect(page.getByTestId("course-edit-locked").getByTestId("course-lessons")).toBeVisible();
  // The refusal is the server's, not only the page's.
  const save = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path: "course.json", content: "{}" }] } });
  expect(save.status()).toBeGreaterThanOrEqual(400);

  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await page.getByTestId("course-review-close").click();
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(4);
  await expect(page.locator('[data-testid="course-lessons-row"][data-path="lessons/word-problems.json"]')).toHaveCount(1);
});

test("a lesson edited by hand survives a revision that keeps it", async ({ page }) => {
  const id = await makeCourse(page, "Fractions hand edit then revise");
  const path = "lessons/what-is-a-fraction.json";
  await editTitle(page, id, path, "Edited by hand");
  const edited = await stored(page, id, path);
  expect(edited).toContain("Edited by hand");

  // The 'remove' plan keeps this lesson, so the revision must leave the hand edit alone.
  await revise(page, id, "Drop the adding lesson [fixture:remove]");
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  // Untouched but for `order`, which a plan that reorders lessons sets.
  const { order, ...rest } = JSON.parse(await stored(page, id, path));
  expect(typeof order).toBe("number");
  expect(rest).toEqual(JSON.parse(edited));
});

test("undo puts back the files from before the revision and drops later hand edits", async ({ page }) => {
  const id = await makeCourse(page, "Fractions undo drops edits");
  const path = "lessons/what-is-a-fraction.json";
  const original = await stored(page, id, path);
  await revise(page, id, "Drop the adding lesson [fixture:remove]");
  // Close the review, edit by hand in the file editor, then undo the revision.
  await page.getByTestId("course-review-close").click();
  await page.goto(`/courses/${id}/file?path=${encodeURIComponent(path)}`);
  const box = page.getByTestId("course-file-content");
  const json = JSON.parse(await box.inputValue());
  json.title = "Edited after the revision";
  await box.fill(JSON.stringify(json, null, 2));
  await page.getByTestId("course-file-save").click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?notice=`));
  expect(await stored(page, id, path)).toContain("Edited after the revision");

  // Undo asks nothing: Redo takes it back (#231).
  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  expect(await stored(page, id, path)).toBe(original);
});

test("a hand edit that breaks the course rules does not block a revision of a course that loads again", async ({ page }) => {
  const id = await makeCourse(page, "Fractions broken then revised");
  // Break a lesson by hand: the course no longer loads, so it cannot be revised.
  const good = await stored(page, id, "lessons/adding.json");
  expect(await save(page, id, "lessons/adding.json", "{}")).not.toBeNull();
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-ai-mode-choice")).toHaveCount(0);

  // Repair it; the form comes back.
  const fix = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path: "lessons/adding.json", content: good }] } });
  expect((await fix.json()).error).toBeNull();
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-ai-mode-choice")).toHaveCount(1);
});
