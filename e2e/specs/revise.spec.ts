// Exact specs for changing a finished AI course with feedback, and undoing it
// (#27). Fixtures only: the plan is picked by a [fixture:NAME] token in the
// feedback (data/test-fixtures/llm/revise-plan.json). The course page
// follows each revision in its modal (course-ai-modal). The happy path in one
// go is in exact.spec.ts; these are the other journeys through the same pages.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, editorSaved, fastAutosave, generateCourse, generationEnds, aiMode } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const modal = (page: Page) => page.getByTestId("course-ai-modal");
const startedJob = /\/courses\/[^/?]+\?job=([0-9a-f]{16})$/;

/** Generate the fixture course (3 lessons) and stay on its editor. Returns its id. */
async function makeCourse(page: Page, wish: string): Promise<string> {
  await generateCourse(page, wish);
  expect(await generationEnds(page)).toBe("done");
  return (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
}

/** Ask for a revision from the course page and wait for the job in its modal. */
async function revise(page: Page, id: string, feedback: string, { wait = true } = {}) {
  await page.goto(`/courses/${id}`);
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill(feedback);
  await page.getByTestId("course-ai-submit").click();
  // A fixture reply can be done before the page loads: then the review opens at once.
  if (wait) await expect(page.locator('[data-testid="course-review-modal"], [data-testid="course-ai-modal"][data-state="failed"]')).toBeVisible({ timeout: 30_000 });
  else await expect(modal(page)).toHaveAttribute("data-revision", "true");
}

/** The titles the course shows under Lessons (open it first). */
async function lessonTitles(page: Page, id: string): Promise<string[]> {
  await page.request.post(`/courses/open/${id}`);
  await page.goto("/lessons?unit=all");
  return (await page.getByTestId("lessons-item").allInnerTexts()).map((t) => t.trim());
}

test("revise a course twice in a row, and undo goes back one step", async ({ page }) => {
  const id = await makeCourse(page, "Fractions twice, step by step");
  await revise(page, id, "More worked examples, and add word problems.");
  await expect(page.getByTestId("course-review-modal")).toBeVisible();
  expect(await lessonTitles(page, id)).toHaveLength(4);

  // The second revision starts from the revised course, not the original: the
  // 'remove' plan keeps two lessons, so the added one and "Adding" both go.
  await revise(page, id, "Drop everything past the basics [fixture:remove]");
  await expect(page.getByTestId("course-review-modal")).toBeVisible();
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  await page.getByTestId("course-review-close").click();
  await expect(page.getByTestId("course-undo-label")).toHaveText("Changed with feedback: “Drop everything past the basics [fixture:remove]”");
  expect(await lessonTitles(page, id)).toHaveLength(2);

  // Undo restores the course as it was before the *last* change only.
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  expect(await lessonTitles(page, id)).toHaveLength(4);
  // The first revision is still there to undo (the history keeps ten steps).
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-undo-label")).toContainText("More worked examples");
  await expect(page.getByTestId("course-undo-form")).toHaveAttribute("data-earlier", "0");
});

test("undo and redo icons right after a revision, from the course page the review closes onto (#231)", async ({ page }) => {
  const id = await makeCourse(page, "Fractions undone here");
  const undo = page.getByTestId("course-undo");
  const redo = page.getByTestId("course-redo");
  const wish = "Drop everything past the basics [fixture:remove]";
  // A fresh course: both icons there, both greyed out, no "Last change" line.
  await page.goto(`/courses/${id}`);
  await expect(undo).toBeDisabled();
  await expect(redo).toBeDisabled();
  await expect(page.getByTestId("course-edit")).not.toContainText(/Last change|can be undone/);
  const before = await lessonTitles(page, id);
  await revise(page, id, wish);
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  await page.getByTestId("course-review-close").click();
  await expect(modal(page)).toHaveCount(0);
  await expect(undo).toBeEnabled();
  await expect(redo).toBeDisabled();
  // Hovering says what it does; its accessible name says which change.
  await undo.hover();
  expect(await undo.evaluate((e) => getComputedStyle(e, "::after").content)).toBe('"Undo"');
  await expect(undo).toHaveAccessibleName(`Undo: Changed with feedback: “${wish}”`);
  // Undo asks nothing (Redo takes it back) and puts the course back, in place: no reload,
  // and a drawer that was open is open again on the lesson that comes back.
  const firstRow = page.getByTestId("course-lessons-row").first();
  await firstRow.getByTestId("course-lessons-expand").click();
  await expect(firstRow.getByTestId("course-lessons-json")).toHaveValue(/"title"/);
  await page.evaluate(() => { (window as any).__samePage = true; });
  await undo.click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  await expect(firstRow.getByTestId("course-lessons-drawer")).toBeVisible();
  await expect(firstRow.getByTestId("course-lessons-json")).toHaveValue(/"title"/);
  await expect(page).not.toHaveURL(/notice=/);
  expect(await page.evaluate(() => (window as any).__samePage)).toBe(true);
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(before.length);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  await expect(undo).toBeDisabled();
  await expect(redo).toBeEnabled();
  await expect(page.getByTestId("course-redo-label")).toHaveText(`Changed with feedback: “${wish}”`);
  expect(await lessonTitles(page, id)).toEqual(before);
  // Redo puts the revision back, and Undo can take it away again.
  await page.goto(`/courses/${id}`);
  await redo.click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Redone");
  await expect(undo).toBeEnabled();
  await expect(redo).toBeDisabled();
  expect(await lessonTitles(page, id)).toHaveLength(2);
  await page.goto(`/courses/${id}`);
  await undo.click();
  await expect(redo).toBeEnabled();
  expect(await lessonTitles(page, id)).toEqual(before);
  // A new edit after an undo leaves nothing to redo; the icons update without a reload.
  await fastAutosave(page);
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-lessons-title").first().click();
  await page.getByTestId("course-lessons-rename").fill("Renamed after the undo");
  await page.keyboard.press("Enter");
  await editorSaved(page);
  await expect(redo).toBeDisabled();
  await expect(undo).toBeEnabled();
  await expect(page.getByTestId("course-undo-label")).toContainText("1 renamed");
});

test("Undo and Redo: rows the step adds fade in, rows it takes away fade out, the rest stay still; none under reduced motion (#251)", async ({ page }) => {
  // A long transition (window.UNDO_TEST_MS in courses.js), so the spec sees it run.
  await page.addInitScript(() => { (window as unknown as { UNDO_TEST_MS: number }).UNDO_TEST_MS = 1500; });
  const id = await makeCourse(page, "Fractions, fading in and out");
  const list = page.getByTestId("course-lessons-list");
  const rows = page.getByTestId("course-lessons-row");
  const leaving = list.locator("[data-leaving]");
  await page.goto(`/courses/${id}`);
  const before = await rows.count();
  await revise(page, id, "Drop everything past the basics [fixture:remove]");
  await page.goto(`/courses/${id}`);
  const after = await rows.count();
  expect(after).toBeLessThan(before);
  // Where the first row sits in the table (a notice above the table may push the whole table down).
  const firstBox = () => rows.first().evaluate((e) => {
    const r = e.getBoundingClientRect(), t = e.parentElement!.getBoundingClientRect();
    return { x: r.x - t.x, y: r.y - t.y, width: r.width };
  });
  const first = await firstBox();
  // Undo brings rows back: they fade in, the first row (kept) does not move or animate.
  await page.getByTestId("course-undo").click();
  await expect(list).toHaveAttribute("data-stepping", "");
  await expect(rows).toHaveCount(before);
  expect(await rows.first().evaluate((e) => e.getAnimations().length)).toBe(0);
  expect(await rows.last().evaluate((e) => e.getAnimations().length)).toBe(1);
  expect(await firstBox()).toEqual(first);
  await expect(list).not.toHaveAttribute("data-stepping");
  // Redo takes them away: they stay a moment, out of the table's rows, where they were, and fade.
  await page.getByTestId("course-redo").click();
  await expect(leaving).toHaveCount(before - after);
  await expect(rows).toHaveCount(after);
  const ghost = await leaving.first().evaluate((e) => {
    const r = e.getBoundingClientRect(), t = e.parentElement!.getBoundingClientRect();
    return { x: r.x - t.x, width: r.width };
  });
  expect(ghost.x).toBe(first.x);
  expect(ghost.width).toBe(first.width);
  await expect(page.getByTestId("course-lessons-position")).toHaveText(Array.from({ length: after }, (_, i) => String(i + 1)));
  await expect(leaving).toHaveCount(0);
  await expect(list).not.toHaveAttribute("data-stepping");
  // Reduced motion: the rows swap outright.
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.getByTestId("course-undo").click();
  await expect(rows).toHaveCount(before);
  await expect(list).not.toHaveAttribute("data-stepping");
  expect(await list.evaluate((e) => e.getAnimations({ subtree: true }).length)).toBe(0);
});

test("a generated course is changed with Only add or Allow overall changes, not generated again", async ({ page }) => {
  const id = await makeCourse(page, "Fractions generated once");
  await page.goto(`/courses/${id}?generate=1`);
  await expect(page.getByTestId("course-ai-form")).toBeVisible();
  await expect(page.getByTestId("course-ai-generate")).toHaveCount(0);
  await expect(page.getByTestId("generate")).toHaveCount(0);
});

test("a course with content of its own sends you to the Use AI box, not to the old Only add buttons (#1245)", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/courses");
  const id = await createCourse(page);
  await page.goto(`/courses/${id}?notice=has-content`);
  const notice = page.getByTestId("course-edit-notice");
  await expect(notice).toHaveText("This course has content of its own now, so the AI won't generate it again. Ask for what you want in the Use AI box instead.");
  await expect(notice).not.toContainText("Only add");
});

test("feedback that is too short is refused on the form and kept", async ({ page }) => {
  const id = await makeCourse(page, "Fractions short feedback");
  await page.goto(`/courses/${id}`);
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill("ab");
  await page.getByTestId("course-ai-submit").click();
  // The course page again (rendered in place), form open, mistake named, nothing started.
  await expect(page.getByTestId("course-edit-notice")).toHaveCount(0);
  await expect(page.getByTestId("course-ai-error")).toHaveText("Say what should change, in at least 20 characters.");
  await expect(aiMode(page, "change")).toBeChecked();
  await expect(page.getByTestId("course-ai-text")).toHaveValue("ab");
  expect(await lessonTitles(page, id)).toHaveLength(3);
});

test("a plan that removes a lesson drops it, keeps the others' progress, and undo brings it back", async ({ page }) => {
  const id = await makeCourse(page, "Fractions to trim, step by step");
  await page.request.post(`/courses/open/${id}`);
  for (const lesson of ["what-is-a-fraction", "adding"]) {
    const r = await page.request.post("/api/progress/seen", { data: { lesson } });
    expect(r.ok(), lesson).toBeTruthy();
  }
  await page.goto(`/courses/${id}`);

  await revise(page, id, "Drop the adding lesson [fixture:remove]");
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  expect(await lessonTitles(page, id)).toHaveLength(2);
  const kept = await page.request.get("/api/progress/lessons/what-is-a-fraction");
  expect(JSON.stringify(await kept.json())).toMatch(/"seen":\s*true/);

  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  expect(await lessonTitles(page, id)).toHaveLength(3);
});

test("a rewritten lesson keeps its progress", async ({ page }) => {
  const id = await makeCourse(page, "Fractions to rewrite");
  await page.request.post(`/courses/open/${id}`);
  expect((await page.request.post("/api/progress/seen", { data: { lesson: "simplifying" } })).ok()).toBeTruthy();
  await revise(page, id, "More worked examples, and add word problems.");
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  const after = await page.request.get("/api/progress/lessons/simplifying");
  expect(JSON.stringify(await after.json())).toMatch(/"seen":\s*true/);
});

test("a plan the AI cannot give leaves the course alone and offers neither undo nor resume", async ({ page }) => {
  const id = await makeCourse(page, "Fractions bad plan, step by step");
  await revise(page, id, "Change something [fixture:invalid-json]");
  await expect(modal(page)).toHaveAttribute("data-state", "failed");
  await expect(page.getByTestId("course-ai-modal-title")).toHaveText("Revision stopped");
  await expect(page.getByTestId("course-ai-modal-error")).toBeVisible();
  await expect(page.getByTestId("course-review-modal")).toHaveCount(0);
  await page.getByTestId("course-ai-modal-close").click();
  await expect(modal(page)).toHaveCount(0);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  // The failed revision does not lock the course: it can be revised again.
  await expect(aiMode(page, "change")).toBeEnabled();
  expect(await lessonTitles(page, id)).toHaveLength(3);
});

test("while a revision runs the course still opens but its files are locked; cancel releases it", async ({ page }) => {
  const id = await makeCourse(page, "Fractions slow revision");
  await revise(page, id, "Take your time [fixture:slow]", { wait: false });
  const editorUrl = page.url();
  await expect(modal(page)).toHaveAttribute("data-state", "running");

  // The modal's own polling must keep describing a revision as one (it once said "written").
  await expect(modal(page)).not.toHaveAttribute("data-phase", "Starting", { timeout: 15_000 });
  await expect(page.getByTestId("course-ai-modal-count")).toContainText("rewritten or added");
  await expect(page.getByTestId("course-ai-modal-cancel")).toHaveText("Cancel");

  // The course list says so, and still offers Open.
  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-card-revising")).toHaveText("revising");
  await expect(card.getByTestId("courses-open")).toBeVisible();

  // Still usable for learning.
  expect(await lessonTitles(page, id)).toHaveLength(3);

  // But not changeable: save, delete, undo and a second revision are all refused.
  await page.goto(`/courses/${id}`);
  await expect(modal(page)).toHaveAttribute("data-state", "running");
  await expect(page.getByTestId("course-edit-locked")).toHaveJSProperty("inert", true);
  for (const action of ["file", "delete", "undo"]) {
    const r = await page.request.post(`/courses/${id}/${action}`, { form: { path: "course.json", content: "{}" }, maxRedirects: 0 });
    expect(r.status(), action).toBe(303);
    expect(r.headers()["location"], action).toContain("notice=revising");
  }
  const save = await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path: "course.json", content: "{}" }] } });
  expect(save.status()).toBeGreaterThanOrEqual(400);
  const again = await page.request.post(`/courses/${id}/ai`, { form: { text: "A second one at once", mode: "change" }, maxRedirects: 0 });
  expect(again.headers()["location"] ?? "").not.toMatch(startedJob);

  // Cancel from the modal: it stops, says the course is unchanged, and unlocks it.
  await page.goto(editorUrl);
  await expect(modal(page)).toHaveAttribute("data-revision", "true");
  const poll = (await modal(page).getAttribute("data-poll-url"))!;
  await page.getByTestId("course-ai-modal-cancel").click();
  await expect(modal(page)).toHaveAttribute("data-state", "failed", { timeout: 30_000 });
  await expect(page.getByTestId("course-ai-modal-error")).toHaveText("Cancelled. The course is unchanged.");
  expect((await (await page.request.get(poll)).json()).error).toContain("Cancelled. The course is unchanged.");
  await page.getByTestId("course-ai-modal-close").click();
  await expect(modal(page)).toHaveCount(0);
  await expect(aiMode(page, "change")).toBeEnabled();
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  expect(await lessonTitles(page, id)).toHaveLength(3);
});

test("two revisions asked for at the same moment start only one job", async ({ page }) => {
  const id = await makeCourse(page, "Fractions double click");
  const post = () =>
    page.request.post(`/courses/${id}/ai`, { form: { text: "Take your time [fixture:slow]", mode: "change" }, maxRedirects: 0 });
  const replies = await Promise.all([post(), post(), post()]);
  const started = replies.filter((r) => startedJob.test(r.headers()["location"] ?? ""));
  expect(started).toHaveLength(1);
  // The course ends up with one backup of the original: undoing the one revision restores 3 lessons.
  const job = startedJob.exec(started[0].headers()["location"])![1];
  await expect.poll(async () => (await (await page.request.get(`/api/generate/${job}`)).json()).done, { timeout: 30_000 }).toBe(true);
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  expect(await lessonTitles(page, id)).toHaveLength(3);
});
