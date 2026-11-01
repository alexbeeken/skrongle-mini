// Exact specs for the course editor saving as it is edited (#151, #190): no
// Save button, the circle by the course title (green saved, yellow saving,
// red not saved), a lesson's JSON or form saved 3 s after the last
// keystroke (or when its drawer loses focus, or the tab is hidden), what
// does not validate kept on the page but not stored, a page that is behind
// another tab told to reload, and nothing saved while an AI job locks the
// editor. Most specs shorten the waits with fastAutosave(); the 3 s itself
// is checked on Playwright's clock. Fixtures only.
import { expect, test, type Page, type Request } from "@playwright/test";
import { devLogin, devReset, editorSaved, fastAutosave, generateCourse, generationEnds } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const circle = (page: Page) => page.getByTestId("course-edit-save-status");
const why = (page: Page) => page.getByTestId("course-edit-save-error");
const firstRow = (page: Page) => page.getByTestId("course-lessons-row").first();

/** Generate the fixture course (Simplifying, What a fraction is, Adding fractions). */
async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

/** The saves the page sends, from now on. */
function saves(page: Page): Request[] {
  const seen: Request[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST" && /\/api\/courses\/[^/]+\/lessons$/.test(r.url())) seen.push(r);
  });
  return seen;
}

/** A lesson file as stored. */
async function stored(page: Page, id: string, path: string): Promise<Record<string, unknown>> {
  const file = await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json();
  return JSON.parse(file.content);
}

/** Open the first lesson's JSON drawer; returns the box, its path and the parsed lesson. */
async function openJson(page: Page) {
  const row = firstRow(page);
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"id"/);
  return { json, path: (await row.getAttribute("data-path"))!, lesson: JSON.parse(await json.inputValue()) };
}

test("typing in a lesson's JSON saves 3 s after the last keystroke, or when the drawer loses focus", async ({ page }) => {
  const id = await makeCourse(page, "Fractions three seconds");
  await page.clock.install();
  await page.goto(`/courses/${id}`);
  const sent = saves(page);
  await expect(page.getByTestId("course-lessons-save")).toHaveCount(0);
  await expect(circle(page)).toHaveAttribute("data-state", "saved");
  // Nothing edited and nothing wrong: no circle yet (#249).
  await expect(circle(page)).toBeHidden();
  const { json, path, lesson } = await openJson(page);

  lesson.summary = "Saved by itself.";
  await json.fill(JSON.stringify(lesson, null, 2));
  // Waiting: yellow, and nothing sent for two seconds.
  await expect(circle(page)).toHaveAttribute("data-state", "saving");
  await expect(circle(page)).toBeVisible();
  await page.clock.runFor(2_000);
  expect(sent).toHaveLength(0);
  // A keystroke starts the wait again.
  await json.press("End");
  await json.press("Space");
  await page.clock.runFor(2_000);
  expect(sent).toHaveLength(0);
  await page.clock.runFor(1_500);
  await expect.poll(() => sent.length).toBe(1);
  // Answered, then green 200–500 ms later (on the page's clock).
  await sent[0].response();
  await page.clock.runFor(600);
  await expect(circle(page)).toHaveAttribute("data-state", "saved");
  await expect(circle(page)).toHaveAttribute("aria-label", "All changes saved");
  expect((await stored(page, id, path)).summary).toBe("Saved by itself.");
  await expect(page.getByTestId("course-undo-label")).toHaveText("Saved the lessons (1 edited)");

  // Leaving the drawer does not wait.
  lesson.summary = "Saved on blur.";
  await json.fill(JSON.stringify(lesson, null, 2));
  await page.getByTestId("course-edit-title").click({ position: { x: 1, y: 1 } });
  await expect.poll(() => sent.length).toBe(2);
  // Answered, then green 200–500 ms later (on the page's clock).
  await sent[1].response();
  await page.clock.runFor(600);
  await expect(circle(page)).toHaveAttribute("data-state", "saved");
  expect((await stored(page, id, path)).summary).toBe("Saved on blur.");
  // One page, one Undo step.
  await expect(page.getByTestId("course-undo-form")).toHaveAttribute("data-earlier", "0");
});

test("a hidden tab sends what waits at once", async ({ page }) => {
  await fastAutosave(page, 60_000);
  const id = await makeCourse(page, "Fractions hidden tab");
  await page.goto(`/courses/${id}`);
  const sent = saves(page);
  const { json, path, lesson } = await openJson(page);
  lesson.summary = "Saved as the tab went away.";
  await json.fill(JSON.stringify(lesson, null, 2));
  await expect(circle(page)).toHaveAttribute("data-state", "saving");
  await page.evaluate(() => {
    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await expect.poll(() => sent.length).toBe(1);
  await editorSaved(page);
  expect((await stored(page, id, path)).summary).toBe("Saved as the tab went away.");
});

test("what does not validate turns the circle red, says why, and is not stored but stays on the page", async ({ page }) => {
  await fastAutosave(page);
  const id = await makeCourse(page, "Fractions invalid, step by step");
  await page.goto(`/courses/${id}`);
  const { json, path, lesson } = await openJson(page);
  const before = await stored(page, id, path);

  // Not JSON: the page knows, sends none of it.
  await json.fill("{ half a lesson");
  await expect(circle(page)).toHaveAttribute("data-state", "error");
  await expect(circle(page)).toHaveAttribute("aria-label", "Changes not saved");
  await expect(why(page)).toContainText("Not valid JSON in “");
  await page.waitForTimeout(500);
  expect(await stored(page, id, path)).toEqual(before);
  await expect(json).toHaveValue("{ half a lesson");

  // JSON the course rules refuse (an id another lesson has): the server says so, stores nothing.
  const other = await page.getByTestId("course-lessons-row").nth(1).getAttribute("data-lesson-id");
  await json.fill(JSON.stringify({ ...lesson, id: other }, null, 2));
  await expect(circle(page)).toHaveAttribute("data-state", "error");
  await expect(why(page)).toContainText("both use the id");
  expect(await stored(page, id, path)).toEqual(before);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  // Leaving asks first, saying why.
  await page.locator(".crumbs a").first().click();
  await expect(page.getByTestId("autosave-leave-dialog")).toBeVisible();
  await expect(page.getByTestId("autosave-leave-dialog")).toContainText("both use the id");
  await page.getByTestId("autosave-stay").click();

  // Fixed: green, and stored.
  await json.fill(JSON.stringify({ ...lesson, summary: "Fixed." }, null, 2));
  await editorSaved(page);
  await expect(why(page)).toBeHidden();
  expect((await stored(page, id, path)).summary).toBe("Fixed.");
});

test("a save the server fails is tried again, and a page behind another tab is told to reload", async ({ page, context }) => {
  await fastAutosave(page);
  const id = await makeCourse(page, "Fractions in two tabs, step by step");
  await page.goto(`/courses/${id}`);

  // The server fails once: red while it waits, then saved on the retry.
  let failed = false;
  await page.route(/\/api\/courses\/[^/]+\/lessons$/, (route) => {
    if (failed) return route.continue();
    failed = true;
    return route.fulfill({ status: 500, body: "boom" });
  });
  await page.getByTestId("course-lessons-title").first().click();
  await page.getByTestId("course-lessons-rename").fill("Simplifying, again");
  await page.keyboard.press("Enter");
  await expect(circle(page)).toHaveAttribute("data-state", "error");
  await expect(why(page)).toContainText("Trying again");
  await editorSaved(page, 15_000);
  await expect(why(page)).toBeHidden();
  await page.unroute(/\/api\/courses\/[^/]+\/lessons$/);

  // A second tab saves; the first, which never saw that, is refused and keeps its edit.
  const other = await context.newPage();
  await other.goto(`/courses/${id}`);
  await other.getByTestId("course-lessons-title").last().click();
  await other.getByTestId("course-lessons-rename").fill("Adding, from tab two");
  await other.keyboard.press("Enter");
  await editorSaved(other);
  await page.getByTestId("course-lessons-title").nth(1).click();
  await page.getByTestId("course-lessons-rename").fill("From the tab behind");
  await page.keyboard.press("Enter");
  await expect(circle(page)).toHaveAttribute("data-state", "error");
  await expect(why(page)).toContainText("Reload the page");
  await expect(page.getByTestId("course-lessons-title").nth(1)).toHaveText("From the tab behind");
  // Not tried again: still refused a while later, and the second tab's edit stands.
  await page.waitForTimeout(800);
  await expect(circle(page)).toHaveAttribute("data-state", "error");
  await other.reload();
  await expect(other.getByTestId("course-lessons-title")).toHaveText(["Simplifying, again", "What a fraction is", "Adding, from tab two"]);
  // Reloading is asked about first: the edit here is not saved.
  let asked = "";
  page.once("dialog", (d) => ((asked = d.type()), d.accept()));
  await page.reload();
  expect(asked).toBe("beforeunload");
  await expect(page.getByTestId("course-lessons-title")).toHaveText(["Simplifying, again", "What a fraction is", "Adding, from tab two"]);
});

test("an AI job locks the editor: what was typed is saved first, then nothing is saved until it ends", async ({ page }) => {
  // Long enough that the edit is still waiting when Use AI is pressed.
  await fastAutosave(page, 20_000);
  const id = await makeCourse(page, "Fractions locked editor");
  await page.goto(`/courses/${id}`);
  const sent = saves(page);
  const { json, path, lesson } = await openJson(page);
  lesson.summary = "Typed just before Use AI.";
  await json.fill(JSON.stringify(lesson, null, 2));
  await expect(circle(page)).toHaveAttribute("data-state", "saving");
  await page.getByTestId("course-ai-text").fill("Comparing [fixture:slow]");
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-ai-modal")).toHaveAttribute("data-state", "running");
  await expect(page.getByTestId("course-edit-locked")).toHaveJSProperty("inert", true);
  expect(sent).toHaveLength(1);
  expect((await stored(page, id, path)).summary).toBe("Typed just before Use AI.");

  // Under the modal, an edit (forced in by a script: the workspace is inert) is not sent.
  const poke = () =>
    page.evaluate(() => {
      const ta = document.querySelector<HTMLTextAreaElement>("[data-testid=course-lessons-json]")!;
      ta.value = ta.value.replace("Typed just before Use AI.", "Typed while locked.");
      ta.dispatchEvent(new Event("input", { bubbles: true }));
    });
  await poke();
  await page.waitForTimeout(600);
  expect(sent).toHaveLength(1);
  // The page drawn while the job runs has no saver at all.
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-edit-locked")).toBeVisible();
  const again = saves(page);
  await expect(circle(page)).toBeHidden();
  await page.evaluate(() => {
    document.querySelector<HTMLElement>("[data-testid=course-lessons-expand]")!.click();
  });
  await poke();
  await page.waitForTimeout(600);
  expect(again).toHaveLength(0);

  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  expect((await stored(page, id, path)).summary).toBe("Typed just before Use AI.");
});
