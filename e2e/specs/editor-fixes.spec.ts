// Exact specs for four small course-editor fixes found by QA: the settings
// drawer of a refused save says it was not saved (#272, in
// editor-errors.spec.ts), the file page's path pattern compiles and refuses a
// bad path (#273), "Course settings" and "+ Add unit" do not collide on a
// phone (#274, #673), and closing a rename box by clicking elsewhere leaves focus
// where the click went (#275). Fixtures only.
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

async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

test("the file page's path box checks the path and logs no console error (#273)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions path pattern");
  await page.goto(`/courses/${id}`);
  const path = (await page.getByTestId("course-lessons-row").first().getAttribute("data-path"))!;
  const errors: string[] = [];
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`/courses/${id}/file?path=${encodeURIComponent(path)}`);
  const box = page.getByTestId("course-file-path");
  await expect(box).toHaveValue(path);
  expect(await box.evaluate((el: HTMLInputElement) => el.validity.valid)).toBe(true);
  // A path outside lessons/ and pictures/ is refused by the browser: the form is not sent.
  await box.fill("lessons/../notes.txt");
  expect(await box.evaluate((el: HTMLInputElement) => el.validity.patternMismatch)).toBe(true);
  let posted = false;
  page.on("request", (r) => { if (r.method() === "POST") posted = true; });
  await page.getByTestId("course-file-save").click();
  await expect(page).toHaveURL(/\/file\?path=/);
  expect(posted).toBe(false);
  await box.fill("lessons/notes/more.json");
  expect(await box.evaluate((el: HTMLInputElement) => el.validity.valid)).toBe(true);
  expect(errors).toEqual([]);
});

test("on a phone, Course settings and + Add unit do not overlap (#274)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions phone settings row");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/courses/${id}`);
  const settings = page.getByTestId("course-lessons-settings");
  const add = page.getByTestId("course-lessons-add-unit");
  await expect(add).toBeVisible();
  await expectApart(page, settings, add);
  // A wider font (the system's fallback) wraps the label; it
  // still may not run under the button.
  await page.addStyleTag({ content: "html { font-family: monospace; font-size: 19px; }" });
  await expectApart(page, settings, add);
  // A bigger font still: the label stays one line and beside its arrow;
  // the two drop under the icons together, and the button below them (#673).
  await page.addStyleTag({ content: "html { font-size: 22px; }" });
  await expectApart(page, settings, add);
  const gear = (await settings.getByTestId("course-lessons-gear").boundingBox())!;
  const arrow = (await settings.getByTestId("course-lessons-expand").boundingBox())!;
  const name = await settings.locator(".lt-name").evaluate((el) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    const rects = Array.from(r.getClientRects());
    return { lines: rects.length, top: Math.min(...rects.map((q) => q.top)), bottom: Math.max(...rects.map((q) => q.bottom)) };
  });
  expect(name.lines, "the label is one line").toBe(1);
  const middle = arrow.y + arrow.height / 2;
  expect(middle, `arrow ${JSON.stringify(arrow)} beside label ${JSON.stringify(name)}`).toBeGreaterThanOrEqual(name.top - 4);
  expect(middle).toBeLessThanOrEqual(name.bottom + 4);
  expect(arrow.y, "the arrow and label sit under the icons").toBeGreaterThanOrEqual(gear.y + gear.height - 1);
  const button = (await add.boundingBox())!;
  expect(button.y).toBeGreaterThanOrEqual(Math.max(arrow.y + arrow.height, name.bottom) - 1);
});

async function expectApart(page: Page, settings: ReturnType<Page["getByTestId"]>, add: ReturnType<Page["getByTestId"]>) {
  // The words themselves, not the label's box: a box narrower than its
  // longest word lets "settings" spill out under the button.
  const a = await settings.locator(".lt-name").evaluate((el) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    const rects = Array.from(r.getClientRects());
    const x = Math.min(...rects.map((q) => q.left)), y = Math.min(...rects.map((q) => q.top));
    return { x, y, width: Math.max(...rects.map((q) => q.right)) - x, height: Math.max(...rects.map((q) => q.bottom)) - y };
  });
  const b = (await add.boundingBox())!;
  const overlap = a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
  expect(overlap, `title ${JSON.stringify(a)} and button ${JSON.stringify(b)}`).toBe(false);
  expect(b.x + b.width).toBeLessThanOrEqual(390);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
}

test("closing a rename box by clicking elsewhere leaves focus where the click went (#275)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions rename focus");
  await page.goto(`/courses/${id}`);
  const row = page.getByTestId("course-lessons-row").first();
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"id"/);
  const before = await json.inputValue();

  // The course title's rename box, closed by a click into the lesson's JSON.
  await page.getByTestId("course-edit-rename").click();
  const input = page.getByTestId("course-edit-rename-input");
  await input.fill("Fractions, renamed by a click away");
  await json.click();
  await expect(input).toHaveCount(0);
  await expect(page.getByTestId("course-edit-rename")).toHaveText("Fractions, renamed by a click away");
  await expect(json).toBeFocused();
  await page.keyboard.type(" ");
  await expect(json).not.toHaveValue(before);

  // A lesson's rename box, the same way.
  const other = page.getByTestId("course-lessons-row").nth(1);
  await other.getByTestId("course-lessons-title").click();
  await page.getByTestId("course-lessons-rename").fill("Renamed by a click away");
  await json.click();
  await expect(page.getByTestId("course-lessons-rename")).toHaveCount(0);
  await expect(json).toBeFocused();

  // Enter still hands focus back to the title.
  await page.getByTestId("course-edit-rename").click();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("course-edit-rename")).toBeFocused();
});
