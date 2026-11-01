// Exact specs for what the course editor says when an edit is refused
// (#228): every problem says its kind (not JSON / against the course rules /
// not saved), where it is (the lesson by its title, the field, the line)
// and how to fix it, and the drawer and the line under the title say the
// same sentence. Fixtures only.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, devReset, editorSaved, fastAutosave, generateCourse, generationEnds } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await fastAutosave(page);
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const circle = (page: Page) => page.getByTestId("course-edit-save-status");
const why = (page: Page) => page.getByTestId("course-edit-save-error");
const rows = (page: Page) => page.getByTestId("course-lessons-row");

async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

/** The drawer's status and the line under the title say the same sentence. */
async function saysBoth(page: Page, status: ReturnType<Page["getByTestId"]>, text: string | RegExp) {
  await expect(circle(page)).toHaveAttribute("data-state", "error");
  await expect(status).toContainText(text);
  await expect(why(page)).toContainText(text);
  expect(await why(page).innerText()).toBe(await status.innerText());
}

test("a parse error, an unknown prerequisite and a duplicate id each say what, where and how to fix it", async ({ page }) => {
  const id = await makeCourse(page, "Fractions error messages");
  await page.goto(`/courses/${id}`);
  const row = rows(page).first();
  const other = rows(page).nth(1);
  const title = (await row.getByTestId("course-lessons-title").innerText()).trim();
  const otherId = (await other.getAttribute("data-lesson-id"))!;
  const otherTitle = (await other.getByTestId("course-lessons-title").innerText()).trim();
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  const status = row.getByTestId("course-lessons-json-status");
  await expect(json).toHaveValue(/"id"/);
  const lesson = JSON.parse(await json.inputValue());
  await expect(status).toHaveText(/^Valid JSON/);

  // Not JSON: the page finds it, with the line and column, and sends none of it.
  await json.fill(`{\n  "id": "${lesson.id}",\n}`);
  await saysBoth(
    page,
    status,
    `Not valid JSON in “${title}”, line 3, column 1: There is a comma right before } or ], which JSON does not allow. To fix it: look just before that point`,
  );
  await expect(row).toHaveAttribute("data-problem", "json");
  await expect(json).toHaveAttribute("aria-invalid", "");
  await expect(row.getByTestId("course-lessons-json-goto")).toHaveText("Go to line 3");
  // The row is tinted like a removed line in Review AI changes (--bad-bg),
  // and so is the line in the box: the tint sits over line 3 of the textarea.
  const badBg = await page.evaluate(() => {
    const d = document.createElement("div");
    d.style.color = "var(--bad-bg)";
    document.body.append(d);
    const v = getComputedStyle(d).color;
    d.remove();
    return v;
  });
  expect(await row.locator(".lt-line").evaluate((el) => getComputedStyle(el).backgroundColor)).toBe(badBg);
  const highlight = row.getByTestId("course-lessons-json-highlight");
  await expect(highlight).toBeVisible();
  const lineHeight = await json.evaluate((t) => parseFloat(getComputedStyle(t).lineHeight));
  const hlBox = (await highlight.boundingBox())!;
  const jsonBox = (await json.boundingBox())!;
  expect(Math.abs(hlBox.height - lineHeight)).toBeLessThan(1);
  expect(hlBox.y - jsonBox.y).toBeGreaterThan(2 * lineHeight);
  expect(hlBox.y - jsonBox.y).toBeLessThan(3 * lineHeight + 12);
  // Line numbers (off until the gear's switch turns them on) number the box and mark line 3.
  const lines = row.getByTestId("course-lessons-lines");
  await expect(lines).toBeHidden();
  await page.getByTestId("course-lessons-gear").click();
  await expect(page.getByTestId("course-lessons-lines-switch")).toHaveAttribute("aria-checked", "false");
  await page.getByTestId("course-lessons-lines-switch").click();
  await expect(page.getByTestId("course-lessons-lines-switch")).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect(lines).toBeVisible();
  await expect(lines).toHaveText(/^1\s*2\s*3\s*$/);
  await expect(row.getByTestId("course-lessons-bad-line")).toHaveText("3");
  await row.getByTestId("course-lessons-json-goto").click();
  await expect(json).toBeFocused();
  expect(await json.evaluate((t: HTMLTextAreaElement) => t.value.slice(t.selectionStart, t.selectionEnd))).toBe("}");

  // Valid JSON, but a prerequisite that is no lesson's id: against the rules,
  // the field named, a guess and the ids to choose from. Never "Valid JSON".
  const nearMiss = otherId.slice(0, -1);
  await json.fill(JSON.stringify({ ...lesson, prerequisites: [nearMiss] }, null, 2));
  await saysBoth(page, status, `Against the course rules in “${title}”, field “prerequisites”, line `);
  await expect(status).toContainText(`“${nearMiss}” is not the id of a lesson in this course. To fix it: a prerequisite must be the "id" of another lesson in this course, or remove it from the list. Did you mean “${otherId}”? The lesson ids are: `);
  await expect(status).not.toContainText("Valid JSON");
  await expect(row).toHaveAttribute("data-problem", "rules");
  // Nothing close to it: no guess, but the ids still.
  await json.fill(JSON.stringify({ ...lesson, prerequisites: ["uasdf asf ..asdf ads1"] }, null, 2));
  await saysBoth(page, status, "“uasdf asf ..asdf ads1” is not the id of a lesson in this course.");
  await expect(status).not.toContainText("Did you mean");
  await expect(status).toContainText(`“${otherId}”`);

  // Another lesson's id: both lessons named, the field, and a free id to use.
  const dup = JSON.stringify({ ...lesson, id: otherId }, null, 2);
  const idLine = dup.split("\n").findIndex((l) => l.startsWith('  "id":')) + 1;
  await json.fill(dup);
  await saysBoth(page, status, `Against the course rules in “${title}”, field “id”, line ${idLine}: “${title}” and “${otherTitle}” both use the id “${otherId}”. To fix it: give “${title}” another "id"`);
  await expect(row).toHaveAttribute("data-problem", "rules");
  await expect(other).not.toHaveAttribute("data-problem");

  // Fixed: saved, and every sign of the problem goes. The numbers follow the text.
  const fixed = JSON.stringify({ ...lesson, summary: "Fixed." }, null, 2);
  await json.fill(fixed);
  await editorSaved(page);
  await expect(why(page)).toBeHidden();
  await expect(row).not.toHaveAttribute("data-problem");
  await expect(status).toHaveText(/^Valid JSON/);
  await expect(row.getByTestId("course-lessons-json-goto")).toBeHidden();
  await expect(highlight).toBeHidden();
  await expect(row.getByTestId("course-lessons-bad-line")).toHaveCount(0);
  await expect(lines).toContainText(String(fixed.split("\n").length));
  // Off again, and remembered across a reload.
  await page.getByTestId("course-lessons-gear").click();
  await page.getByTestId("course-lessons-lines-switch").click();
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect(lines).toBeHidden();
  await page.reload();
  await rows(page).first().getByTestId("course-lessons-expand").click();
  await expect(rows(page).first().getByTestId("course-lessons-json")).toHaveValue(/"id"/);
  await expect(rows(page).first().getByTestId("course-lessons-lines")).toBeHidden();
});

test("a problem in another lesson opens that lesson's drawer", async ({ page }) => {
  const id = await makeCourse(page, "Fractions error drawer");
  await page.goto(`/courses/${id}`);
  const settings = page.getByTestId("course-lessons-settings");
  await settings.getByTestId("course-lessons-expand").click();
  const json = settings.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"units"/);
  const manifest = JSON.parse(await json.inputValue());
  const last = manifest.units[manifest.units.length - 1].id as string;
  const inLast = page.locator(`[data-testid="course-lessons-row"][data-unit="${last}"]`).first();
  const name = (await inLast.getByTestId("course-lessons-title").innerText()).trim();
  await expect(inLast.getByTestId("course-lessons-drawer")).toBeHidden();

  // The unit goes while it still holds a lesson: that lesson's drawer opens and says so.
  await json.fill(JSON.stringify({ ...manifest, units: manifest.units.filter((u: { id: string }) => u.id !== last) }, null, 2));
  await expect(inLast).toHaveAttribute("data-problem", "rules");
  await expect(inLast.getByTestId("course-lessons-drawer")).toBeVisible();
  await saysBoth(page, inLast.getByTestId("course-lessons-json-status"), `Against the course rules in “${name}”, field “unit”: It sits in unit '${last}', and there is no unit '${last}' in the course settings. To fix it: drag it into another unit`);
  // The settings drawer, where the edit was typed, was not saved either (#272):
  // it says so in red and names the lesson, never "Valid JSON".
  const own = settings.getByTestId("course-lessons-json-status");
  await expect(own).toHaveText(`Not saved: the change was refused because of “${name}”; see why there.`);
  await expect(own).toHaveClass(/bad-text/);
  await expect(own).not.toContainText("Valid JSON");
});

test("the file page words a parse error as the editor does", async ({ page }) => {
  const id = await makeCourse(page, "Fractions error file page");
  await page.goto(`/courses/${id}`);
  const path = (await rows(page).first().getAttribute("data-path"))!;
  await page.goto(`/courses/${id}/file?path=${encodeURIComponent(path)}`);
  const sentence = `Not valid JSON in ${path}, line 3, column 1: There is a comma right before } or ], which JSON does not allow. To fix it: look just before that point`;
  await page.getByTestId("course-file-content").fill(`{\n  "id": "x",\n}`);
  await expect(page.getByTestId("course-file-status")).toContainText(sentence);
  await page.getByTestId("course-file-save").click();
  await expect(page.getByTestId("course-file-error")).toContainText(sentence);
});

test("Undo takes back changes that cannot be saved, after saving what can (#288)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions undo unsaved");
  await page.goto(`/courses/${id}`);
  const row = rows(page).first();
  const other = rows(page).nth(1);
  const undo = page.getByTestId("course-undo");
  const path = (await row.getAttribute("data-path"))!;
  const fileOf = async () => (await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json()).content as string;
  await row.getByTestId("course-lessons-expand").click();
  const json = row.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"id"/);
  const lesson = JSON.parse(await json.inputValue());
  const otherTitle = (await other.getByTestId("course-lessons-title").innerText()).trim();

  // A rename that can be saved, then JSON that cannot: Undo is on for it.
  await other.getByTestId("course-lessons-title").click();
  await page.getByTestId("course-lessons-rename").fill("Renamed before undo");
  await page.keyboard.press("Enter");
  await json.fill(`{\n  "id": "${lesson.id}",\n}`);
  await expect(row).toHaveAttribute("data-problem", "json");
  await expect(undo).toBeEnabled();
  await expect(undo).toHaveAttribute("data-discard", "1");
  await expect(undo).toHaveAttribute("aria-label", "Undo your unsaved changes");
  // The rename saves meanwhile; it leaves the broken JSON in its box.
  const otherPath = (await other.getAttribute("data-path"))!;
  await expect
    .poll(async () => JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(otherPath)}`)).json()).content).title)
    .toBe("Renamed before undo");
  await page.waitForTimeout(300);
  await expect(json).toHaveValue(`{\n  "id": "${lesson.id}",\n}`);
  await expect(row).toHaveAttribute("data-problem", "json");

  // Undo: the rename is saved, the broken JSON goes, the drawer shows the file as saved.
  await undo.click();
  await expect(page.getByTestId("course-lessons-status")).toHaveText("Your unsaved changes were undone.");
  await expect(row).not.toHaveAttribute("data-problem");
  await expect(why(page)).toBeHidden();
  await expect(circle(page)).not.toHaveAttribute("data-state", "error");
  await expect(undo).not.toHaveAttribute("data-discard");
  await expect(json).toHaveValue(/"id"/);
  expect(await json.inputValue()).toBe(await fileOf());
  expect(JSON.parse(await json.inputValue()).id).toBe(lesson.id);
  await expect(other.getByTestId("course-lessons-title")).toHaveText("Renamed before undo");

  // A save the course rules refuse (another lesson's id): Undo takes that back too.
  const otherId = (await other.getAttribute("data-lesson-id"))!;
  await json.fill(JSON.stringify({ ...JSON.parse(await json.inputValue()), id: otherId }, null, 2));
  await expect(row).toHaveAttribute("data-problem", "rules");
  await expect(undo).toHaveAttribute("data-discard", "1");
  await undo.click();
  await expect(row).not.toHaveAttribute("data-problem");
  await expect(json).toHaveValue(/"id"/);
  expect(JSON.parse(await json.inputValue()).id).toBe(lesson.id);
  await expect(circle(page)).not.toHaveAttribute("data-state", "error");
  expect(JSON.parse(await fileOf()).id).toBe(lesson.id);

  // Nothing unsaved left: Undo undoes saved steps again (the rename).
  await expect(undo).not.toHaveAttribute("data-discard");
  await undo.click();
  await expect(other.getByTestId("course-lessons-title")).toHaveText(otherTitle);
});
