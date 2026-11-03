// Exact specs for Forms mode in the lesson table's drawer (#114): a lesson
// as the lesson editor (#841, outline + one form per item), the course
// settings as a form built from docs/course.schema.json, both editing the
// same document as Edit JSON. Fixtures only.
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


/** Generate the fixture course (Simplifying, What a fraction is, Adding fractions). */
async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

const at = (scope: Page | ReturnType<Page["locator"]>, testid: string, path: string) => scope.locator(`[data-testid="${testid}"][data-path="${path}"]`);

/** Flip the JSON switch: it sits in the gear's Editor settings dialog, closed again after. */
async function flipJson(page: Page): Promise<void> {
  await page.getByTestId("course-lessons-gear").click();
  await expect(page.getByTestId("course-lessons-prefs")).toBeVisible();
  await page.getByTestId("course-lessons-json-switch").click();
  await page.getByTestId("course-lessons-prefs-close").click();
  await expect(page.getByTestId("course-lessons-prefs")).toBeHidden();
}

test("Forms mode: add a word and a question in the lesson editor, see them as JSON, and find them in the file as saved", async ({ page }) => {
  const id = await makeCourse(page, "Fractions in forms, step by step");
  await page.goto(`/courses/${id}`);
  await page.evaluate(() => localStorage.removeItem("lt-mode"));
  const titles = await page.getByTestId("course-lessons-title").allTextContents();
  const row = page.getByTestId("course-lessons-row").first();
  const path = (await row.getAttribute("data-path"))!;
  const drawer = row.getByTestId("course-lessons-drawer");
  const json = row.getByTestId("course-lessons-json");
  const form = row.getByTestId("course-form");
  const section = (sec: string) => form.locator(`[data-testid="course-outline-section"][data-sec="${sec}"]`);
  const item = (sec: string, idx: number) => form.locator(`[data-testid="course-outline-item"][data-sec="${sec}"][data-idx="${idx}"]`);
  // A section with items folds and unfolds on its row; open it only when shut.
  const open = async (sec: string) => {
    if ((await section(sec).getAttribute("aria-expanded")) !== "true") await section(sec).click();
    await expect(section(sec)).toHaveAttribute("aria-expanded", "true");
  };

  await row.getByTestId("course-lessons-expand").click();
  await expect(json).toHaveValue(/"vocabulary"/);
  const before = JSON.parse(await json.inputValue());
  // One JSON switch for the whole table, on until turned off.
  await expect(page.getByTestId("course-lessons-json-switch")).toHaveAttribute("aria-checked", "true");
  await flipJson(page);
  await expect(drawer).toHaveAttribute("data-mode", "forms");
  await expect(form).toBeVisible();
  await expect(json).toBeHidden();
  await expect(page.getByTestId("course-lessons-json-switch")).toHaveAttribute("aria-checked", "false");
  // The lesson editor (#841): an outline of the file, the lesson's own fields first; opening the form changes nothing.
  await expect(form.getByTestId("course-form-layout")).toBeVisible();
  await expect(at(form, "course-form-field", "title")).toHaveValue(before.title);
  await section("content").click();
  await expect(at(form, "course-form-field", "content")).toHaveValue(before.content);
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "saved");
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);

  // A vocabulary item, from the outline's + Word.
  await open("vocabulary");
  await expect(item("vocabulary", 1)).toBeVisible();
  await form.locator('[data-testid="course-outline-add"][data-path="vocabulary"]').click();
  await at(form, "course-form-field", "vocabulary.2.term").fill("mixed number");
  await at(form, "course-form-field", "vocabulary.2.meaning").fill("a whole number and a fraction together");
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toContainText("1 edited");
  // Reorder: up puts it second, down puts it back; the selection follows.
  await form.getByTestId("course-form-up").click();
  await expect(at(form, "course-form-field", "vocabulary.1.term")).toHaveValue("mixed number");
  await form.getByTestId("course-form-down").click();
  await expect(at(form, "course-form-field", "vocabulary.2.term")).toHaveValue("mixed number");

  // A multiple-choice question: its id is made up, its options get a right answer.
  await open("quiz");
  await form.locator('[data-testid="course-outline-add"][data-path="quiz"]').click();
  const q = "quiz.3";
  // Ids are left to the JSON: none of the lesson's own, nor the question's.
  await expect(form.locator('[data-testid="course-form-field"][data-path$="id"], [data-path="slug"], [data-path="unit"]')).toHaveCount(0);
  await expect(form.getByTestId("course-form-type").locator('[aria-pressed="true"]')).toHaveAttribute("data-value", "multiple-choice");
  await at(form, "course-form-field", `${q}.prompt`).fill("Which is a mixed number?");
  await at(form, "course-form-field", `${q}.options.0`).fill("3/4");
  await at(form, "course-form-field", `${q}.options.1`).fill("1 1/2");
  await form.getByTestId("course-form-new-option").pressSequentially("5");
  await at(form, "course-form-correct", `${q}.options.1`).check();
  // No "Correct answer" box: the radio is the answer, and retyping the right option keeps it the answer.
  await expect(at(form, "course-form-field", `${q}.correct_answer`)).toHaveCount(0);
  await at(form, "course-form-field", `${q}.options.1`).fill("2 1/2");
  await at(form, "course-form-remove", `${q}.options.2`).click();
  await expect(at(form, "course-form-item", `${q}.options.2`)).toHaveCount(0);
  // Type-specific fields follow the type: tolerance is for numeric questions.
  await expect(at(form, "course-form-field", `${q}.tolerance`)).toHaveCount(0);
  await form.getByTestId("course-form-type").locator('[data-value="numeric"]').click();
  await expect(at(form, "course-form-field", `${q}.tolerance`)).toHaveCount(1);
  await form.getByTestId("course-form-type").locator('[data-value="multiple-choice"]').click();
  await expect(at(form, "course-form-correct", `${q}.options.1`)).toBeChecked();

  // The same document as JSON.
  await flipJson(page);
  await expect(json).toBeVisible();
  await expect(form).toBeHidden();
  const edited = JSON.parse(await json.inputValue());
  expect(edited.vocabulary[2]).toEqual({ term: "mixed number", meaning: "a whole number and a fraction together" });
  expect(edited.quiz[3]).toEqual({ question_id: `${before.id}-q1`, prompt: "Which is a mixed number?", type: "multiple-choice", correct_answer: "2 1/2", options: ["3/4", "2 1/2"] });
  // A field the course format does not know (the fixture's "hint") is kept as it was.
  expect(edited.quiz[0].hint).toBe(before.quiz[0].hint);
  // A JSON edit shows in the form when switching back.
  edited.summary = "Edited as JSON";
  await json.fill(JSON.stringify(edited, null, 2));
  await flipJson(page);
  await section("lesson").click();
  await expect(at(form, "course-form-field", "summary")).toHaveValue("Edited as JSON");
  await open("vocabulary");
  await item("vocabulary", 2).click();
  await expect(at(form, "course-form-field", "vocabulary.2.term")).toHaveValue("mixed number");

  await editorSaved(page);
  await page.reload();
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  await expect(page.getByTestId("course-lessons-title")).toHaveText(titles);
  // The drawer opens in the mode last used, on the saved file.
  await row.getByTestId("course-lessons-expand").click();
  await expect(drawer).toHaveAttribute("data-mode", "forms");
  await expect(at(form, "course-form-field", "summary")).toHaveValue("Edited as JSON");
  const file = await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path)}`)).json();
  const saved = JSON.parse(file.content);
  expect(saved.vocabulary.map((v: { term: string }) => v.term)).toEqual([...before.vocabulary.map((v: { term: string }) => v.term), "mixed number"]);
  expect(saved.quiz[3].correct_answer).toBe("2 1/2");
  expect(saved.quiz[3].options).toEqual(["3/4", "2 1/2"]);
  expect(saved.quiz[0].hint).toBe(before.quiz[0].hint);
  expect(saved.summary).toBe("Edited as JSON");
});

test("the JSON switch: a drawer reopened after a flip follows it, and quick flips end where they point", async ({ page }) => {
  const id = await makeCourse(page, "Fractions switch, step by step");
  await page.goto(`/courses/${id}`);
  const sw = page.getByTestId("course-lessons-json-switch");
  const row = page.getByTestId("course-lessons-row").first();
  const drawer = row.getByTestId("course-lessons-drawer");
  await row.getByTestId("course-lessons-expand").click();
  await expect(row.getByTestId("course-lessons-json")).toHaveValue(/"title"/);
  await expect(drawer).toHaveAttribute("data-mode", "json");
  // Shut, flip, reopen: the form.
  await row.getByTestId("course-lessons-expand").click();
  await flipJson(page);
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await row.getByTestId("course-lessons-expand").click();
  await expect(drawer).toHaveAttribute("data-mode", "forms");
  await expect(row.getByTestId("course-form")).toBeVisible();
  // And back: shut, flip, reopen is JSON again.
  await row.getByTestId("course-lessons-expand").click();
  await flipJson(page);
  await row.getByTestId("course-lessons-expand").click();
  await expect(drawer).toHaveAttribute("data-mode", "json");
  await expect(row.getByTestId("course-lessons-json")).toBeVisible();
  // Two quick flips with the drawer open, on a fresh page (the form's schema
  // not loaded yet): the form that was loading must not land after JSON.
  await page.reload();
  await row.getByTestId("course-lessons-expand").click();
  await expect(row.getByTestId("course-lessons-json")).toHaveValue(/"title"/);
  await page.getByTestId("course-lessons-gear").click();
  await sw.click();
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await page.getByTestId("course-lessons-prefs-close").click();
  await page.waitForTimeout(500);
  await expect(drawer).toHaveAttribute("data-mode", "json");
  await expect(row.getByTestId("course-form")).toBeHidden();
  await flipJson(page);
  await expect(drawer).toHaveAttribute("data-mode", "forms");
});

test("a new lesson is a whole form before it is saved", async ({ page }) => {
  const id = await makeCourse(page, "Fractions new lesson form");
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-lessons-add-here").last().click();
  await page.getByTestId("course-lessons-rename").fill("Mixed numbers");
  await page.keyboard.press("Enter");
  const row = page.getByTestId("course-lessons-row").filter({ hasText: "Mixed numbers" });
  const form = row.getByTestId("course-form");
  await row.getByTestId("course-lessons-expand").click();
  // The JSON box starts from a blank lesson named after the row.
  await expect(row.getByTestId("course-lessons-json")).toHaveValue(/"title": "Mixed numbers"/);
  await flipJson(page);
  await expect(row.getByTestId("course-lessons-drawer")).toHaveAttribute("data-mode", "forms");
  // No ids, no order: the table sets them.
  await expect(form.locator('[data-path="id"], [data-path="slug"], [data-path="unit"], [data-path="order"]')).toHaveCount(0);
  await at(form, "course-form-field", "summary").fill("A whole number and a fraction.");
  await form.locator('[data-testid="course-outline-section"][data-sec="quiz"]').click();
  await form.locator('[data-testid="course-outline-add"][data-path="quiz"]').click();
  await expect(form.locator('[data-path="quiz.0.question_id"]')).toHaveCount(0);
  await at(form, "course-form-field", "quiz.0.prompt").fill("Is 2 1/2 a mixed number?");
  await form.getByTestId("course-form-type").locator('[data-value="true-false"]').click();
  await form.locator('.le-tf-btn[data-value="true"]').click();
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toContainText("1 added");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  const path = await row.getAttribute("data-path");
  const file = await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path!)}`)).json();
  const saved = JSON.parse(file.content);
  expect(saved).toMatchObject({ id: "mixed-numbers", slug: "mixed-numbers", title: "Mixed numbers", summary: "A whole number and a fraction." });
  expect(saved.quiz).toEqual([{ question_id: "mixed-numbers-q1", prompt: "Is 2 1/2 a mixed number?", type: "true-false", correct_answer: "true" }]);
});

test("Forms mode edits the course settings too, and fits a phone", async ({ page }) => {
  const id = await makeCourse(page, "Fractions settings form");
  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto(`/courses/${id}`);
  const settings = page.getByTestId("course-lessons-settings");
  const form = settings.getByTestId("course-form");
  await settings.getByTestId("course-lessons-expand").click();
  await expect(settings.getByTestId("course-lessons-json")).toHaveValue(/"units"/);
  // The switch turns every open drawer into a form, the lessons' too.
  const lesson = page.getByTestId("course-lessons-row").first();
  await lesson.getByTestId("course-lessons-expand").click();
  await expect(lesson.getByTestId("course-lessons-json")).toHaveValue(/"title"/);
  await flipJson(page);
  await expect(lesson.getByTestId("course-lessons-drawer")).toHaveAttribute("data-mode", "forms");
  await expect(settings.getByTestId("course-lessons-drawer")).toHaveAttribute("data-mode", "forms");
  await lesson.getByTestId("course-lessons-expand").click();
  // A unit added in the form shows as a folder at once, like typing JSON does.
  const units = page.getByTestId("course-lessons-unit-title");
  const before = await units.allTextContents();
  await at(form, "course-form-section", "units").locator("> summary").click();
  await at(form, "course-form-add", "units").click();
  const n = before.length;
  await at(form, "course-form-field", `units.${n}.title`).fill("Practice");
  await expect(units).toHaveText([...before, "Practice"]);
  await at(form, "course-form-field", "title").fill("Fractions by form");
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions by form");
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toContainText(/Saved the course settings|settings edited/);
  await page.reload();
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions by form");
  await expect(units).toHaveText([...before, "Practice"]);
  const file = await (await page.request.get(`/api/courses/${id}/file?path=course.json`)).json();
  // Its id is made up, not asked for.
  const saved = JSON.parse(file.content).units;
  expect(saved.at(-1)).toEqual({ id: expect.stringMatching(/^unit-\d+$/), title: "Practice" });
  expect(new Set(saved.map((u: { id: string }) => u.id)).size).toBe(saved.length);
});
