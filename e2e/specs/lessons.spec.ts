// Exact specs for the lesson table (#105, after the Lessons panel of #31):
// move, rename, delete and add rows, edit their JSON, saved as they are made
// (#151, #190; the editor's own autosave specs are in editor-autosave.spec.ts),
// add with AI and the undo history. Fixtures only. The happy path in one go is in
// exact.spec.ts; these are the refusals, locks and other ways in.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, devReset, editorSaved, fastAutosave, generateCourse, generationEnds, aiMode } from "../lib/app";

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
const titles = (page: Page) => page.getByTestId("course-lessons-title");
const row = (page: Page, title: string) => page.getByTestId("course-lessons-row").filter({ hasText: title });

/** Generate the fixture course (Simplifying, What a fraction is, Adding fractions). */
async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

type Row = { key: string | null; unit: string; title: string; content?: string | null };

/** The table as the page shows it, ready to send back. */
async function table(page: Page, id: string): Promise<Row[]> {
  await page.goto(`/courses/${id}`);
  return page.getByTestId("course-lessons-row").evaluateAll((els) =>
    els.map((e) => ({
      key: (e as HTMLElement).dataset.lessonId || null,
      unit: (e as HTMLElement).dataset.unit!,
      title: e.querySelector("[data-testid=course-lessons-title]")!.textContent!.trim(),
    })),
  );
}

/** Save a table straight to the server: status and the error or notice. */
async function save(page: Page, id: string, lessons: Row[]) {
  const r = await page.request.post(`/courses/${id}/lessons`, { data: { lessons } });
  const body = await r.json().catch(() => ({}));
  return { status: r.status(), error: (body.error ?? "") as string, notice: (body.notice ?? "") as string };
}

/** Post a form to the course and return the redirect's status and notice. */
async function post(page: Page, id: string, action: string, form: [string, string][]) {
  const r = await page.request.post(`/courses/${id}/${action}`, {
    data: new URLSearchParams(form).toString(),
    headers: { "content-type": "application/x-www-form-urlencoded" },
    maxRedirects: 0,
  });
  const loc = r.headers()["location"] ?? "";
  return { status: r.status(), notice: decodeURIComponent(loc.split("notice=")[1]?.split("#")[0] ?? "").replace(/\+/g, " ") };
}

const lessonIds = async (page: Page, id: string) => (await table(page, id)).map((r) => r.key);

test("the server refuses a table it cannot use, and changes nothing", async ({ page }) => {
  const id = await makeCourse(page, "Fractions bad tables");
  const rows = await table(page, id);
  const before = rows.map((r) => r.key);
  for (const [why, lessons, says] of [
    ["unknown lesson", [...rows, { key: "nope", unit: "1", title: "Nope" }], "no lesson 'nope'"],
    ["twice", [...rows, rows[0]], "listed twice"],
    ["empty", [], "at least one lesson"],
    ["no name", [{ ...rows[0], title: "  " }, ...rows.slice(1)], "no name"],
    ["no unit", [{ ...rows[0], unit: "9" }, ...rows.slice(1)], "no unit '9'"],
    ["bad JSON", [{ ...rows[0], content: "{ nope" }, ...rows.slice(1)], "Not valid JSON"],
    ["same", rows, "Nothing to save"],
  ] as [string, Row[], string][]) {
    const r = await save(page, id, lessons);
    expect(r.status, why).toBe(400);
    expect(r.error, why).toContain(says);
  }
  expect(await lessonIds(page, id)).toEqual(before);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
});

test("the last lesson cannot be deleted, and Escape keeps a lesson's name", async ({ page }) => {
  const id = await makeCourse(page, "Fractions delete all");
  await page.goto(`/courses/${id}`);
  const rows = page.getByTestId("course-lessons-row");
  await rows.nth(0).getByTestId("course-lessons-delete").click();
  await rows.nth(0).getByTestId("course-lessons-delete").click();
  await expect(rows).toHaveCount(1);
  await rows.nth(0).getByTestId("course-lessons-delete").click();
  await expect(rows).toHaveCount(1);
  await expect(page.getByTestId("course-lessons-status")).toContainText("at least one lesson");
  const name = await titles(page).first().innerText();
  await titles(page).first().click();
  await page.getByTestId("course-lessons-rename").fill("Something else");
  await page.keyboard.press("Escape");
  await expect(titles(page).first()).toHaveText(name);
});

test("a stopped run keeps its Resume, and has no editor until it is finished", async ({ page }) => {
  await generateCourse(page, "Fractions stopped [fixture:validation-fails]");
  expect(await generationEnds(page)).toBe("failed");
  const id = new URL(page.url()).searchParams.get("generate")!;
  await expect(page.getByTestId("generate-submit")).toHaveText("Resume writing");
  // Half a course is never shown: not in an editor, not as files.
  await page.goto(`/courses/${id}`);
  await expect(page).toHaveURL(/\/courses$/);
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-resume")).toBeVisible();
  await expect(card.getByTestId("courses-edit")).toHaveCount(0);
});

test("while an AI lesson is being written, every way to change the course is refused", async ({ page }) => {
  const id = await makeCourse(page, "Fractions locked, step by step");
  const rows = await table(page, id);
  await page.getByTestId("course-ai-text").fill("Comparing [fixture:slow]");
  await page.getByTestId("course-ai-submit").click();
  await expect(modal(page)).toHaveAttribute("data-adding", "true");
  await expect(modal(page)).toHaveAttribute("data-state", "running");
  await expect(page.getByTestId("course-ai-modal-title")).toHaveText("Generating…");
  // Coming back while it runs shows the modal again (no ?job= this time).
  await page.goto(`/courses/${id}`);
  await expect(modal(page)).toHaveAttribute("data-state", "running");
  const r = await save(page, id, rows.slice(1));
  expect(r.status).toBe(400);
  expect(r.error).toContain("being revised");
  for (const [action, form] of [
    ["ai", [["text", "Another one please"], ["mode", "add"]]],
    ["undo", []],
  ] as [string, [string, string][]][]) {
    const p = await post(page, id, action, form);
    expect(p.status, action).toBe(303);
    expect(p.notice, action).toBe("revising"); // a notice key (#385)
  }
  // The modal follows the job to its end, then the review of what it changed opens.
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  // None of the refused writes got in: three lessons and the new one.
  expect(await lessonIds(page, id)).toHaveLength(4);
});

test("three saves at once leave a course that loads", async ({ page }) => {
  const id = await makeCourse(page, "Fractions race, step by step");
  const rows = await table(page, id);
  const rs = await Promise.all(rows.map((r) => save(page, id, rows.filter((x) => x !== r))));
  expect(rs.filter((r) => r.status === 200).length).toBeGreaterThanOrEqual(1);
  const left = await lessonIds(page, id);
  expect(left.length).toBeGreaterThanOrEqual(1);
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
});

test("a folded unit hides its lessons and still takes new ones", async ({ page }) => {
  const id = await makeCourse(page, "Fractions folders, step by step");
  await page.goto(`/courses/${id}`);
  const basics = page.getByTestId("course-lessons-unit").first();
  // A unit row is its fold arrow and title: no folder emoji or other icon (#215).
  const marker = await basics.getByTestId("course-lessons-unit-title").evaluate((e) => getComputedStyle(e, "::before").content);
  expect(["none", "normal", ""]).toContain(marker);
  await expect(basics.getByTestId("course-lessons-unit-title")).not.toContainText("📁");
  await basics.getByTestId("course-lessons-fold").click();
  await expect(row(page, "Simplifying")).toBeHidden();
  await expect(basics.getByTestId("course-lessons-fold")).toHaveAttribute("aria-expanded", "false");
  // + on a folded unit opens it and puts the lesson at its end.
  await basics.getByTestId("course-lessons-add-here").click();
  await page.keyboard.press("Enter");
  await expect(row(page, "Simplifying")).toBeVisible();
  await expect(titles(page)).toHaveText(["Simplifying", "What a fraction is", "New lesson", "Adding fractions"]);
  await expect(page.getByTestId("course-lessons-count")).toHaveText(["3", "1"]);
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toHaveText("Saved the lessons (1 added)");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
});

test("Add unit: named in place, saved in the settings, it takes lessons, an empty one goes at once and one with lessons asks first (#232)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions, with a unit added");
  await page.goto(`/courses/${id}`);
  const units = page.getByTestId("course-lessons-unit");
  const unitTitles = page.getByTestId("course-lessons-unit-title");
  const [first, second] = await unitTitles.allInnerTexts();
  // Every unit has a ✕ (one holding lessons asks first, below).
  await expect(page.getByTestId("course-lessons-unit-delete").filter({ visible: true })).toHaveCount(2);
  // Escape drops the new unit; nothing is saved.
  await page.getByTestId("course-lessons-add-unit").click();
  await expect(page.getByTestId("course-lessons-unit-rename")).toHaveValue("New unit");
  await page.keyboard.press("Escape");
  await expect(units).toHaveCount(2);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  // Named in place: an empty unit at the end, saved with the course settings.
  await page.getByTestId("course-lessons-add-unit").click();
  await page.getByTestId("course-lessons-unit-rename").fill("Word problems");
  await page.keyboard.press("Enter");
  await expect(unitTitles).toHaveText([first, second, "Word problems"]);
  const added = units.nth(2);
  await expect(added).toHaveAttribute("data-unit", "3");
  await expect(added.getByTestId("course-lessons-count")).toHaveText("0");
  await expect(added.getByTestId("course-lessons-unit-delete")).toBeVisible();
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toHaveText("Saved the course settings");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  // Where they sit: Undo and Redo under the table; + Add unit on the settings row, right of its gear.
  const listBox = (await page.getByTestId("course-lessons-list").boundingBox())!;
  const historyBox = (await page.getByTestId("course-history").boundingBox())!;
  expect(historyBox.y).toBeGreaterThanOrEqual(listBox.y + listBox.height - 1);
  const addBox = (await page.getByTestId("course-lessons-add-unit").boundingBox())!;
  const gearBox = (await page.getByTestId("course-lessons-gear").boundingBox())!;
  expect(gearBox.x + gearBox.width).toBeLessThanOrEqual(addBox.x);
  expect(Math.abs(addBox.y - gearBox.y)).toBeLessThan(addBox.height);
  await page.reload();
  await expect(unitTitles).toHaveText([first, second, "Word problems"]);
  const manifest = JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=course.json`)).json()).content);
  expect(manifest.units.at(-1)).toEqual({ id: "3", title: "Word problems" });
  // Its + adds a lesson there. Deleting a unit that holds lessons asks first: Cancel keeps both.
  await units.nth(2).getByTestId("course-lessons-add-here").click();
  await page.getByTestId("course-lessons-rename").fill("Pizza slices");
  await page.keyboard.press("Enter");
  await expect(row(page, "Pizza slices")).toHaveAttribute("data-unit", "3");
  await editorSaved(page);
  const dialog = page.getByTestId("course-lessons-unit-dialog");
  await units.nth(2).getByTestId("course-lessons-unit-delete").click();
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Warning: this will delete all lessons in the unit. Are you sure?");
  await page.getByTestId("course-lessons-unit-cancel").click();
  await expect(dialog).toBeHidden();
  await expect(unitTitles).toHaveText([first, second, "Word problems"]);
  await expect(row(page, "Pizza slices")).toHaveCount(1);
  // Confirm takes the unit out of the settings and its lesson out of the course, in one save.
  await units.nth(2).getByTestId("course-lessons-unit-delete").click();
  await page.getByTestId("course-lessons-unit-confirm").click();
  await expect(dialog).toBeHidden();
  await expect(unitTitles).toHaveText([first, second]);
  await expect(row(page, "Pizza slices")).toHaveCount(0);
  await editorSaved(page);
  await page.reload();
  await expect(unitTitles).toHaveText([first, second]);
  await expect(row(page, "Pizza slices")).toHaveCount(0);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  const after = JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=course.json`)).json()).content);
  expect(after.units.map((u: { id: string }) => u.id)).toEqual(["1", "2"]);
  // The learner's lesson list never shows an empty unit: add one, open the course, look.
  await page.getByTestId("course-lessons-add-unit").click();
  await page.keyboard.press("Enter");
  await expect(unitTitles).toHaveText([first, second, "New unit"]);
  await editorSaved(page);
  await page.request.post(`/courses/open/${id}`);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);
  await expect(page.getByText("New unit")).toHaveCount(0);
});

test("course settings are the first row: pinned, kept, and their units reshape the table as they are typed", async ({ page }) => {
  const id = await makeCourse(page, "Fractions settings, step by step");
  await page.goto(`/courses/${id}`);
  const settings = page.getByTestId("course-lessons-settings");
  const units = page.getByTestId("course-lessons-unit-title");
  await expect(page.getByTestId("course-lessons-list").locator("> li").first()).toHaveAttribute("data-testid", "course-lessons-settings");
  await expect(settings.getByTestId("course-lessons-grip")).toHaveCount(0);
  await expect(settings.getByTestId("course-lessons-delete")).toHaveCount(0);
  // Up from the first lesson stays under the first folder, below the settings.
  await page.getByTestId("course-lessons-grip").first().focus();
  await page.keyboard.press("ArrowUp");
  await expect(titles(page)).toHaveText(["Simplifying", "What a fraction is", "Adding fractions"]);
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "saved");
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);

  await settings.getByTestId("course-lessons-expand").click();
  const json = settings.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"units"/);
  const manifest = JSON.parse(await json.inputValue());
  const [first, second] = manifest.units.map((u: { title: string }) => u.title);
  // A new unit and a new course title show up while typing.
  manifest.title = "Fractions, revised";
  manifest.units[0].title = "The basics";
  manifest.units.push({ id: "3", title: "Practice" });
  await json.fill(JSON.stringify(manifest, null, 2));
  await expect(units).toHaveText(["The basics", second, "Practice"]);
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions, revised");
  // Units in a new order take their lessons along.
  manifest.units = [manifest.units[1], manifest.units[0], manifest.units[2]];
  await json.fill(JSON.stringify(manifest, null, 2));
  await expect(units).toHaveText([second, "The basics", "Practice"]);
  await expect(titles(page)).toHaveText(["Adding fractions", "Simplifying", "What a fraction is"]);
  await expect(page.getByTestId("course-lessons-position")).toHaveText(["1", "2", "3"]);
  // A unit taken out while it holds lessons stays, marked, and the save is refused, saying why.
  const without = { ...manifest, units: manifest.units.filter((u: { id: string }) => u.id !== "2") };
  await json.fill(JSON.stringify(without, null, 2));
  await expect(page.locator(".lt-unit.missing")).toHaveCount(1);
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "error");
  await expect(page.getByTestId("course-edit-save-error")).toContainText("no unit '2'");
  // The marked unit sits last; up moves its lesson into Practice, and the empty unit goes away.
  await expect(units).toHaveText(["The basics", "Practice", second]);
  await row(page, "Adding fractions").getByTestId("course-lessons-grip").focus();
  await page.keyboard.press("ArrowUp");
  await expect(row(page, "Adding fractions")).toHaveAttribute("data-unit", "3");
  await json.fill(JSON.stringify(without, null, 2));
  await expect(units).toHaveText(["The basics", "Practice"]);
  await editorSaved(page);
  await expect(page.getByTestId("course-edit-save-error")).toBeHidden();
  await expect(page.getByTestId("course-undo-label")).toContainText(/Saved the course settings|settings edited/);
  await page.reload();
  await expect(units).toHaveText(["The basics", "Practice"]);
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions, revised");
  await expect(row(page, "Adding fractions")).toHaveAttribute("data-unit", "3");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  expect(first).not.toBe("The basics");
  // The drawer is the way in: no Edit link while scripts run.
  await expect(settings.getByTestId("course-edit-manifest")).toHaveCount(0);
  await expect(page.getByTestId("course-lessons-edit")).toHaveCount(0);
});

test("add-lesson refusals: short brief, and a reply that cannot be used leaves the course alone", async ({ page }) => {
  const id = await makeCourse(page, "Fractions add refusals");
  const before = await lessonIds(page, id);
  await page.getByTestId("course-ai-text").fill("ab");
  // `required` + no minlength: the server must be the one saying no.
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-ai-error")).toContainText("Say what the new lesson should teach");
  await expect(page.getByTestId("course-ai-text")).toHaveValue("ab");
  for (const scenario of ["taken", "invalid-json"]) {
    await page.goto(`/courses/${id}`);
    await page.getByTestId("course-ai-text").fill(`Anything [fixture:${scenario}]`);
    await page.getByTestId("course-ai-submit").click();
    await expect(modal(page)).toHaveAttribute("data-state", "failed", { timeout: 30_000 });
    await expect(page.getByTestId("course-ai-modal-title")).toHaveText("Adding stopped");
    await expect(page.getByTestId("course-ai-modal-error")).toContainText("The course is unchanged");
    await page.keyboard.press("Escape");
    await expect(modal(page)).toHaveCount(0);
    expect(await lessonIds(page, id), scenario).toEqual(before);
    await expect(page.getByTestId("course-undo-form"), scenario).toHaveCount(0);
  }
});

test("Use AI: Only add appends a lesson each time; Allow overall changes can replace the whole course", async ({ page }) => {
  const id = await makeCourse(page, "Fractions add and replace, step by step");
  await page.goto(`/courses/${id}`);
  const ai = page.getByTestId("course-ai");
  await expect(ai).toHaveAttribute("data-has-key", "true");
  await expect(ai.getByTestId("ai-disclosure")).toBeVisible();
  const before = await lessonIds(page, id);
  const original = await titles(page).allInnerTexts();
  // Only add: add-lesson.json's default fits any course, after the last lesson, under a new id each time.
  for (const n of [1, 2]) {
    await page.getByTestId("course-ai-text").fill("A lesson on comparing fractions");
    await page.getByTestId("course-ai-submit").click();
    await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
    await page.getByTestId("course-review-close").click();
    await expect(titles(page)).toHaveText([...original, ...Array(n).fill("Comparing fractions")]);
  }
  expect(await lessonIds(page, id)).toHaveLength(before.length + 2);
  // Allow overall changes, [fixture:replace-all]: every lesson and unit replaced (revise-plan.json).
  await page.goto(`/courses/${id}`);
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill("Change it all, top to bottom [fixture:replace-all]");
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  await page.getByTestId("course-review-close").click();
  await expect(titles(page)).toHaveText(["New lesson 1: the course was replaced", "New lesson 2: new units too", "New lesson 3: undo puts it back"]);
  await expect(page.getByTestId("course-lessons-unit-title")).toHaveText(["Unit A (everything replaced)", "Unit B (everything replaced)"]);
  await expect(page.getByTestId("course-edit-title")).toHaveText("Your course, rewritten overall");
});

test("a hand-made course can be changed overall too", async ({ page }) => {
  await devLogin(page, "keys=dev&next=/courses");
  const made = await page.request.post("/courses/new", { form: { title: "Hand-made overall" } });
  const id = new URL(made.url()).pathname.split("/").pop()!;
  await page.goto(`/courses/${id}`);
  await expect(aiMode(page, "add")).toBeChecked();
  await expect(aiMode(page, "change")).toBeEnabled();
  await aiMode(page, "change").check();
  // [fixture:replace-all] fits any course: it replaces everything.
  await page.getByTestId("course-ai-text").fill("Make it a real course [fixture:replace-all]");
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  await page.getByTestId("course-review-close").click();
  await expect(titles(page)).toHaveText(["New lesson 1: the course was replaced", "New lesson 2: new units too", "New lesson 3: undo puts it back"]);
});

test("the download is every file by path, with a format version", async ({ page }) => {
  const id = await makeCourse(page, "Fractions download, step by step");
  await page.goto(`/courses/${id}`);
  const href = await page.getByTestId("course-edit-export").getAttribute("href");
  const body = await (await page.request.get(href!)).text();
  expect(body).toMatch(/^\{\s*"version": "1\.0",/);
  const all = JSON.parse(body);
  expect(all["course.json"].id).toBeTruthy();
  expect(Object.keys(all).filter((k) => k.startsWith("lessons/")).length).toBeGreaterThan(0);
});

test("Delete this course is a red button that warns first", async ({ page }) => {
  const id = await makeCourse(page, "Fractions to delete, step by step");
  await page.goto(`/courses/${id}`);
  const del = page.getByTestId("course-edit-delete");
  // Outlined in red with a trash icon, filled only on hover (#419).
  await expect(del).toHaveCSS("color", "rgb(192, 57, 43)");
  await expect(del).toHaveCSS("border-top-color", "rgb(192, 57, 43)");
  let warning = "";
  page.once("dialog", (d) => { warning = d.message(); d.dismiss(); });
  await del.click();
  expect(warning).toContain("cannot be undone");
  await expect(page.getByTestId("course-edit")).toBeVisible();
  page.once("dialog", (d) => d.accept());
  await del.click();
  await expect(page).toHaveURL(/\/courses(\?|$)/);
  expect((await page.request.get(`/courses/${id}`)).status()).toBe(404);
});

test("without JavaScript the table still lists the lessons, and Edit opens one", async ({ browser }) => {
  const ctx = await browser.newContext({ javaScriptEnabled: false });
  const page = await ctx.newPage();
  await generateCourse(page, "Fractions no js, step by step");
  // No scripts, so nothing polls: the progress modal is gone once a reload finds the job done.
  await page.waitForURL(/\?job=/);
  const id = (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
  await expect.poll(async () => { await page.reload(); return page.getByTestId("course-ai-modal").count(); }, { timeout: 40_000 }).toBe(0);
  await page.goto(`/courses/${id}`);
  await expect(titles(page)).toHaveText(["Simplifying", "What a fraction is", "Adding fractions"]);
  await expect(page.getByTestId("course-lessons-grip").first()).toBeHidden();
  // No circle either: without scripts nothing saves by itself; Edit's form has a Save button.
  await expect(page.getByTestId("course-edit-save-status")).toBeHidden();
  await row(page, "What a fraction is").getByTestId("course-lessons-edit").click();
  await expect(page.getByTestId("course-file-content")).toHaveValue(/What a fraction is/);
  const lesson = JSON.parse(await page.getByTestId("course-file-content").inputValue());
  lesson.summary = "Saved without scripts.";
  await page.getByTestId("course-file-content").fill(JSON.stringify(lesson, null, 2));
  await page.getByTestId("course-file-save").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("The course loads");
  const path = await row(page, "What a fraction is").getAttribute("data-path");
  const file = await (await page.request.get(`/api/courses/${id}/file?path=${encodeURIComponent(path!)}`)).json();
  expect(JSON.parse(file.content).summary).toBe("Saved without scripts.");
  await ctx.close();
});

test("the lesson table fits a phone", async ({ page }) => {
  const id = await makeCourse(page, "Fractions phone, step by step");
  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-lessons")).toBeVisible();
  await page.getByTestId("course-lessons-expand").first().click();
  await expect(page.getByTestId("course-lessons-json").first()).toHaveValue(/"id"/);
  const right = await page.getByTestId("course-lessons").evaluate((e) => Math.max(...[e, ...Array.from(e.querySelectorAll("*"))].filter((x) => x.getClientRects().length && !x.closest("textarea")).map((x) => x.getBoundingClientRect().right)) - innerWidth);
  expect(right).toBeLessThanOrEqual(0);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test("a course with progress warns before a delete, and the table is for the owner's own courses only", async ({ page }) => {
  const id = await makeCourse(page, "Fractions warning, step by step");
  await page.request.post(`/courses/open/${id}`);
  await page.request.post("/api/progress/seen", { data: { lesson: (await lessonIds(page, id))[0] } });
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-lessons")).toHaveAttribute("data-started", "true");
  // A delete saves at once, so it asks first; No keeps the lesson.
  let asked = "";
  page.once("dialog", (d) => ((asked = d.message()), d.dismiss()));
  await page.getByTestId("course-lessons-delete").first().click();
  expect(asked).toContain("Your progress in it goes too");
  await expect(titles(page)).toHaveCount(3);
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "saved");
  page.once("dialog", (d) => d.accept());
  await page.getByTestId("course-lessons-delete").first().click();
  await expect(titles(page)).toHaveCount(2);
  await editorSaved(page);
  expect(await lessonIds(page, id)).toHaveLength(2);
  // Built-in courses have no editor page, so no table.
  const r = await page.request.get("/courses/spanish", { maxRedirects: 0 });
  expect([303, 404]).toContain(r.status());
});

test("a lesson page links the lessons before and after it in course order, below the lesson only, and neither past the ends (#402, #999)", async ({ page }) => {
  await page.goto("/lessons?unit=all");
  const order = await page.getByTestId("lessons-item").evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset.lessonId!));
  expect(order.length).toBeGreaterThan(2);
  const bottom = (id: string) => page.getByTestId("lesson-pager-bottom").getByTestId(id);

  await page.goto(`/lessons/${order[0]}`);
  await expect(page.getByTestId("lesson-pager-top")).toHaveCount(0);
  await expect(page.getByTestId("lesson-prev")).toHaveCount(0);
  await expect(bottom("lesson-next")).toHaveAttribute("data-lesson-id", order[1]);
  await bottom("lesson-next").click();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-lesson-id", order[1]);
  await expect(bottom("lesson-prev")).toHaveAttribute("data-lesson-id", order[0]);
  await expect(bottom("lesson-next")).toHaveAttribute("data-lesson-id", order[2]);
  await bottom("lesson-prev").click();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-lesson-id", order[0]);

  const last = order[order.length - 1];
  await page.goto(`/lessons/${last}`);
  await expect(page.getByTestId("lesson-next")).toHaveCount(0);
  await expect(bottom("lesson-prev")).toHaveAttribute("data-lesson-id", order[order.length - 2]);
});
