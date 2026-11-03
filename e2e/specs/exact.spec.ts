// Exact specs: fixtures plus testid assertions, no model in the loop. These
// run on every `scripts/e2e.sh`, with no network and no key.
import { expect, test } from "@playwright/test";
import { aiMode, correctAnswers, createCourse, devLogin, devReset, editorSaved, fastAutosave, playSprintSession, generateCourse, generationEnds, openFromEditor } from "../lib/app";
import { login, logout, newAccount, register, removeApiKey, resetServerKeys, saveApiKey } from "../lib/learner";
import { readdirSync, readFileSync } from "node:fs";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

test("generate a course from the fixtures and open it", async ({ page }) => {
  await generateCourse(page, "Fractions for Friday");

  // The editor's progress modal follows the job and closes when it is done.
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(3);
  // Open is an icon (#229): a labelled button, no text, and the course opens in a new tab.
  const open = page.getByTestId("course-edit-open");
  await expect(open).toHaveAttribute("aria-label", "Open this course, in a new tab");
  await expect(open).toHaveText("");
  const tab = await openFromEditor(page);
  await expect(tab.getByTestId("nav-course-switch")).toHaveAttribute("data-course-id", (await page.getByTestId("course-edit").getAttribute("data-course-id"))!);
  await expect(page.getByTestId("course-edit")).toBeVisible();
  await tab.close();

  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);
  await expect(page.getByTestId("lessons-item").filter({ hasText: /fraction/i })).not.toHaveCount(0);
});

test("revise a generated course with feedback, then undo it", async ({ page }) => {
  await generateCourse(page, "Fractions to revise, step by step");
  expect(await generationEnds(page)).toBe("done");
  const id = (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;

  // "Use AI" on the finished course: Add is the default of three radios (#1241).
  await expect(page.getByTestId("course-ai").getByRole("heading")).toHaveText("Use AI");
  await expect(page.getByTestId("course-ai-mode")).toHaveCount(3);
  await expect(aiMode(page, "add")).toBeChecked();
  await expect(aiMode(page, "add")).toHaveAttribute("type", "radio");
  // The info tip explains all three.
  await expect(page.getByTestId("course-ai-mode-info-text")).toBeHidden();
  await page.getByTestId("course-ai-mode-info").locator("summary").click();
  await expect(page.getByTestId("course-ai-mode-info-text")).toContainText("Undo puts the old version back");
  // The dev user's keys are three (one per provider), so the first is named.
  await expect(page.getByTestId("course-ai-provider")).toContainText("Claude");
  await expect(page.getByTestId("course-ai").getByTestId("ai-disclosure")).toContainText("Your notes are sent to Claude");
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill("More worked examples, and add word problems.");
  await page.getByTestId("course-ai-submit").click();

  // No status page: the course page follows the revision in a modal.
  const modal = page.getByTestId("course-ai-modal");
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId("course-review-label")).toContainText("Changed with feedback");
  // The review opens by itself; closing it shows the new version in the lesson table.
  await page.getByTestId("course-review-close").click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}#lessons$`));
  await expect(modal).toHaveCount(0);
  await expect(page.getByTestId("course-lessons-title").filter({ hasText: "Word problems" })).toHaveCount(1);
  await expect(page.getByTestId("course-undo-label")).toHaveText("Changed with feedback: “More worked examples, and add word problems.”");
  await page.request.post(`/courses/open/${id}`);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(4);
  await expect(page.getByTestId("lessons-item").filter({ hasText: "Word problems" })).toHaveCount(1);

  // Undo, from the course page.
  await page.goto(`/courses/${id}`);
  await page.getByTestId("course-undo").click();
  await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);

  // Feedback the AI finds nothing to do with changes nothing, and says so.
  await page.goto(`/courses/${id}`);
  await aiMode(page, "change").check();
  await page.getByTestId("course-ai-text").fill("Looks good [fixture:nothing]");
  await page.getByTestId("course-ai-submit").click();
  await expect(modal).toHaveAttribute("data-state", "failed", { timeout: 30_000 });
  await expect(page.getByTestId("course-ai-modal-title")).toHaveText("Revision stopped");
  await expect(page.getByTestId("course-ai-modal-error")).toContainText("Nothing changed");
  await expect(page.getByTestId("course-review-modal")).toHaveCount(0);
  await page.getByTestId("course-ai-modal-close").click();
  await expect(modal).toHaveCount(0);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);
});

test("Use AI's Add / Edit / Add and edit choice: the box follows it, and an Edit run rewrites lessons in place (#1241)", async ({ page }) => {
  await generateCourse(page, "Fractions to edit, lesson by lesson");
  expect(await generationEnds(page)).toBe("done");
  const id = (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
  const text = page.getByTestId("course-ai-text");
  const count = page.getByTestId("course-ai-text-count");
  const outline = page.getByTestId("course-ai-outline");
  await expect(page.getByTestId("course-ai-mode-choice")).toBeVisible();
  await expect(page.getByTestId("course-ai-mode-info-text")).toBeHidden();
  await page.getByTestId("course-ai-mode-info").locator("summary").click();
  const info = page.getByTestId("course-ai-mode-info-text");
  await expect(info).toContainText("Writes one new lesson");
  await expect(info).toContainText("No lesson is added, removed or moved");
  await expect(info).toContainText("Undo puts the old version back");
  await expect(page.getByTestId("course-ai-mode-finished-only")).toHaveCount(0);
  await page.getByTestId("course-ai-mode-info").locator("summary").click();

  // Add: the short limit; Edit: the long one, its own label, Outline only off; Add and edit: long again.
  await expect(page.getByTestId("course-ai-text-label")).toHaveText("What should the AI add?");
  await expect(text).toHaveAttribute("maxlength", "1000");
  await expect(count).toContainText("1,000");
  await expect(outline).toBeEnabled();
  await aiMode(page, "edit").check();
  await expect(page.getByTestId("course-ai-text-label")).toHaveText("What should the AI change?");
  await expect(text).toHaveAttribute("maxlength", "4000");
  await expect(text).toHaveAttribute("placeholder", /fix the typo/);
  await expect(count).toContainText("4,000");
  await expect(outline).toBeDisabled();
  await aiMode(page, "change").check();
  await expect(page.getByTestId("course-ai-text-label")).toHaveText("What should the AI add or change?");
  await expect(text).toHaveAttribute("maxlength", "4000");
  await expect(outline).toBeEnabled();
  await aiMode(page, "add").check();
  await expect(text).toHaveAttribute("maxlength", "1000");
  await expect(count).toContainText("1,000");

  // The server draws the same (the refused form is its page): Edit stays chosen, the text kept.
  await aiMode(page, "edit").check();
  await text.fill("ab");
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-ai-error")).toBeVisible();
  await expect(aiMode(page, "edit")).toBeChecked();
  await expect(text).toHaveValue("ab");
  await expect(page.getByTestId("course-ai-text-label")).toHaveText("What should the AI change?");
  await expect(text).toHaveAttribute("maxlength", "4000");
  await expect(outline).toBeDisabled();

  // An Edit run: the modal says Editing, then the review; the lesson is rewritten in its place.
  await page.goto(`/courses/${id}`);
  await aiMode(page, "edit").check();
  await text.fill("More worked examples, simpler words [fixture:slow]");
  await page.getByTestId("course-ai-submit").click();
  await expect(page.getByTestId("course-ai-modal-what")).toContainText("Editing");
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await page.getByTestId("course-review-close").click();
  await expect(page.getByTestId("course-lessons-title")).toHaveCount(3);
  await expect(page.getByTestId("course-lessons-title").filter({ hasText: "Simplifying fractions" })).toHaveCount(1);

  // An Edit that would add a lesson is refused, and the course stays as it was.
  await page.goto(`/courses/${id}`);
  await aiMode(page, "edit").check();
  await text.fill("Add a lesson on word problems [fixture:adds]");
  await page.getByTestId("course-ai-submit").click();
  const modal = page.getByTestId("course-ai-modal");
  await expect(modal).toHaveAttribute("data-state", "failed", { timeout: 30_000 });
  await expect(page.getByTestId("course-ai-modal-error")).toContainText("The course is unchanged");
  await page.getByTestId("course-ai-modal-close").click();
  await expect(page.getByTestId("course-lessons-title")).toHaveCount(3);

  // At phone width the choice has its own row, its (i) beside it, and nothing scrolls sideways.
  await page.setViewportSize({ width: 390, height: 800 });
  await page.goto(`/courses/${id}`);
  const choice = (await page.getByTestId("course-ai-mode-choice").boundingBox())!;
  const tip = (await page.getByTestId("course-ai-mode-info").boundingBox())!;
  const head = (await page.getByTestId("course-ai").getByRole("heading").boundingBox())!;
  expect(choice.y).toBeGreaterThanOrEqual(head.y + head.height - 1);
  expect(tip.y).toBeLessThan(choice.y + choice.height);
  expect(tip.x + tip.width).toBeLessThanOrEqual(390);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});

test("the lesson table: move, rename, delete, add, edit JSON, each saved as it is made, then undo each page's edits", async ({ page }) => {
  await fastAutosave(page);
  await generateCourse(page, "Fractions to rearrange");
  expect(await generationEnds(page)).toBe("done");
  const id = (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
  await page.goto(`/courses/${id}`);
  const titles = page.getByTestId("course-lessons-title");
  const undoLabel = page.getByTestId("course-undo-label");
  const original = ["Simplifying", "What a fraction is", "Adding fractions"];
  await expect(titles).toHaveText(original);
  // Two units, so two folders; no file names anywhere in the table's rows
  // (the Placement test (i) dialog names placement.json, #1106, and sits outside them).
  await expect(page.getByTestId("course-lessons-unit")).toHaveCount(2);
  await expect(page.getByTestId("course-lessons-list")).not.toContainText(".json");
  await expect(page.getByTestId("course-edit-upload")).toHaveCount(0);
  // No Save button: the circle by the title says how the edits stand.
  await expect(page.getByTestId("course-lessons-save")).toHaveCount(0);
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "saved");

  // The arrow keys on a grip move a row, and the move is saved a moment later.
  const row = (title: string) => page.getByTestId("course-lessons-row").filter({ hasText: title });
  await row("What a fraction is").getByTestId("course-lessons-grip").focus();
  await page.keyboard.press("ArrowUp");
  await expect(titles).toHaveText(["What a fraction is", "Simplifying", "Adding fractions"]);
  await expect(page.getByTestId("course-lessons-position")).toHaveText(["1", "2", "3"]);
  await editorSaved(page);
  await expect(undoLabel).toHaveText("Rearranged the lessons");
  // Each visit to the page is one Undo step.
  await page.reload();
  // Undo and Redo sit in the row under the table, where courses.js redraws them after a save.
  await expect(page.locator("#course-history-row").getByTestId("course-history")).toHaveCount(1);
  await expect(titles).toHaveText(["What a fraction is", "Simplifying", "Adding fractions"]);

  // Up from the first lesson of a unit goes into the unit above; click a name to rename it.
  await row("Adding fractions").getByTestId("course-lessons-grip").focus();
  await page.keyboard.press("ArrowUp");
  await expect(row("Adding fractions")).toHaveAttribute("data-unit", "1");
  await expect(page.getByTestId("course-lessons-count")).toHaveText(["3", "0"]);
  await editorSaved(page);
  await row("Simplifying").getByTestId("course-lessons-title").click();
  await page.getByTestId("course-lessons-rename").fill("Simplifying fractions");
  await page.keyboard.press("Enter");
  await editorSaved(page);
  await expect(undoLabel).toContainText("1 renamed");
  await expect(titles).toHaveText(["What a fraction is", "Simplifying fractions", "Adding fractions"]);

  // Drag a lesson onto a folder: it goes to the end of that unit.
  const grip = row("Adding fractions").getByTestId("course-lessons-grip");
  const folder = page.getByTestId("course-lessons-unit").nth(1);
  const from = (await grip.boundingBox())!;
  const to = (await folder.boundingBox())!;
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2);
  await page.mouse.down();
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, { steps: 8 });
  await expect(folder).toHaveClass(/drop-into/);
  await page.mouse.up();
  await expect(row("Adding fractions")).toHaveAttribute("data-unit", "2");
  await editorSaved(page);
  // Three saves on one page, one Undo step: back where it was, but for the rename.
  await expect(page.getByTestId("course-undo-form")).toHaveAttribute("data-earlier", "1");
  await page.reload();
  await expect(row("Adding fractions")).toHaveAttribute("data-unit", "2");
  await expect(titles).toHaveText(["What a fraction is", "Simplifying fractions", "Adding fractions"]);

  // Delete one, add one: the new lesson is saved once it has its name.
  await row("Simplifying fractions").getByTestId("course-lessons-delete").click();
  await page.getByTestId("course-lessons-add-here").first().click();
  await page.getByTestId("course-lessons-rename").fill("Equivalent fractions");
  await page.keyboard.press("Enter");
  await expect(titles).toHaveText(["What a fraction is", "Equivalent fractions", "Adding fractions"]);
  await editorSaved(page);
  await expect(undoLabel).toContainText("1 added, 1 removed");
  await expect(row("Equivalent fractions")).toHaveAttribute("data-lesson-id", "equivalent-fractions");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  await page.reload();
  await expect(titles).toHaveText(["What a fraction is", "Equivalent fractions", "Adding fractions"]);

  // ▸ opens the lesson's JSON; an edit there is saved as it is typed, and its title renames the row.
  await row("Equivalent fractions").getByTestId("course-lessons-expand").click();
  const json = row("Equivalent fractions").getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"id": "equivalent-fractions"/);
  const lesson = JSON.parse(await json.inputValue());
  lesson.title = "Equal fractions";
  lesson.summary = "Two fractions, one amount.";
  await json.fill(JSON.stringify(lesson, null, 2));
  await expect(row("Equal fractions").getByTestId("course-lessons-json-status")).toContainText("Valid JSON");
  await editorSaved(page);
  await expect(undoLabel).toHaveText("Saved the lessons (1 edited)");
  await expect(titles).toHaveText(["What a fraction is", "Equal fractions", "Adding fractions"]);

  // Broken JSON is not sent: the circle turns red and says which lesson, the text stays to be fixed.
  await row("Equal fractions").getByTestId("course-lessons-json").fill("{ nope");
  await expect(page.getByTestId("course-edit-save-status")).toHaveAttribute("data-state", "error");
  await expect(page.getByTestId("course-edit-save-error")).toContainText("Not valid JSON in “Equal fractions”");
  await expect(row("Equal fractions").getByTestId("course-lessons-json")).toHaveValue("{ nope");
  // Leaving discards it, once the browser has asked.
  let asked = "";
  page.once("dialog", (d) => ((asked = d.type()), d.accept()));
  await page.goto(`/courses/${id}`);
  expect(asked).toBe("beforeunload");
  await expect(titles).toHaveText(["What a fraction is", "Equal fractions", "Adding fractions"]);
  await expect(undoLabel).toHaveText("Saved the lessons (1 edited)");

  // A new lesson from the last folder's +.
  await page.getByTestId("course-lessons-add-here").last().click();
  await page.getByTestId("course-lessons-rename").fill("Mixed numbers");
  await page.keyboard.press("Enter");
  await editorSaved(page);
  await expect(undoLabel).toHaveText("Saved the lessons (1 added)");
  await expect(titles).toHaveText(["What a fraction is", "Equal fractions", "Adding fractions", "Mixed numbers"]);

  // Use AI, Only add, under the table ([fixture:middle]: after "What a fraction is").
  await page.getByTestId("course-ai-text").fill("Comparing two fractions [fixture:middle]");
  await page.getByTestId("course-ai-submit").click();
  const modal = page.getByTestId("course-ai-modal");
  await expect(page.getByTestId("course-review-modal")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId("course-review-label")).toContainText("Added with AI");
  await page.getByTestId("course-review-close").click();
  await expect(modal).toHaveCount(0);
  await expect(titles).toHaveText(["What a fraction is", "Comparing fractions", "Equal fractions", "Adding fractions", "Mixed numbers"]);
  await expect(undoLabel).toHaveText("Added with AI: “Comparing fractions”");

  // Undo walks back one page of edits at a time, to the course as it was made.
  await page.goto(`/courses/${id}`);
  for (const [label, after] of [
    ["Added with AI", ["What a fraction is", "Equal fractions", "Adding fractions", "Mixed numbers"]],
    ["1 added", ["What a fraction is", "Equal fractions", "Adding fractions"]],
    ["1 edited", ["What a fraction is", "Equivalent fractions", "Adding fractions"]],
    ["1 added, 1 removed", ["What a fraction is", "Simplifying fractions", "Adding fractions"]],
    ["1 renamed", ["What a fraction is", "Simplifying", "Adding fractions"]],
    ["Rearranged", original],
  ] as const) {
    await expect(undoLabel).toContainText(label);
    await page.getByTestId("course-undo").click();
    await expect(page.getByTestId("course-edit-notice")).toContainText("Undone");
    await expect(titles).toHaveText([...after]);
  }
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
});

test("a course someone makes stays private to them", async ({ page, browser }) => {
  await generateCourse(page, "Private fractions, step by step");
  expect(await generationEnds(page)).toBe("done");
  const id = (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;
  const job = new URL(page.url()).searchParams.get("job")!;
  expect(id).not.toBe("");
  await page.goto("/courses");
  await expect(page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveCount(1);

  // Another learner, in their own browser session.
  const other = await browser.newContext();
  const them = await other.newPage();
  const name = `other${Date.now().toString(36)}`;
  await them.goto("/register");
  await them.getByTestId("register-email").fill(`${name}@example.com`);
  await them.getByTestId("register-password").fill("correct horse battery");
  await them.getByTestId("register-age").check();
  await them.getByTestId("register-submit").click();
  // Registering signs the new account in.
  await expect(them.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", `${name}@example.com`);

  // Not listed for them; the platform's Spanish course is.
  await them.goto("/courses");
  await expect(them.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveCount(0);
  await expect(them.locator('[data-testid="courses-card"][data-course-id="spanish"]')).toHaveCount(1);

  // Every way in answers as if the course did not exist.
  for (const url of [`/courses/${id}`, `/courses/${id}/file?path=course.json`, `/courses/${id}/export.json`, `/courses/generate/${job}`]) {
    expect((await them.request.get(url)).status(), url).toBe(404);
  }
  expect((await them.request.post(`/courses/open/${id}`, { maxRedirects: 0 })).status()).toBe(404);
  expect((await them.request.get(`/api/generate/${job}`)).status()).toBe(401);
  for (const url of [`/api/courses/${id}/files`, `/api/courses/${id}/file?path=course.json`]) {
    const r = await them.request.get(url);
    expect(r.status(), url).toBe(400);
    expect(await r.text()).not.toContain("Private fractions, step by step");
  }
  const write = await them.request.post(`/api/courses/${id}/files`, { data: { files: [{ path: "course.json", content: "{}" }] } });
  expect(write.status()).toBe(400);
  for (const action of ["file", "file/delete", "delete", "publish", "cancel", "revise", "undo", "lessons/add"]) {
    const r = await them.request.post(`/courses/${id}/${action}`, { form: { path: "course.json", content: "{}" }, maxRedirects: 0 });
    expect(r.status(), action).toBe(404);
  }
  const table = await them.request.post(`/courses/${id}/lessons`, { data: { lessons: [] } });
  expect(table.status()).toBe(400);
  expect(await table.text()).not.toContain("Private fractions, step by step");
  await other.close();

  // And the owner still has it, untouched.
  await page.goto(`/courses/${id}`);
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(3);
});

// #38: a failed generation says what went wrong in plain words in the
// dialog; its card shows only a red !, with the provider's own text behind it.
test("a failed generation explains itself in the dialog; its card keeps the raw detail behind a red !", async ({ page }) => {
  const cases = [
    { fixture: "provider-error", plain: /didn't accept the server's API key.*Manage Users/, detail: /HTTP 401/ },
    { fixture: "timeout", plain: /didn't answer in time/, detail: /timed out/ },
    { fixture: "invalid-json", plain: /reply couldn't be used/, detail: /no JSON object/ },
  ];
  await devLogin(page, "keys=fake&next=/courses");
  for (const c of cases) {
    const id = await createCourse(page);
    await page.getByTestId("generate-wish").fill(`Fractions [fixture:${c.fixture}]`);
    await page.getByTestId("generate-submit").click();
    // Stopped before its outline: the dialog on the list, with the error to retry from.
    expect(await generationEnds(page)).toBe("failed");
    await expect(page.getByTestId("generate-error")).toHaveText(c.plain);
    await expect(page.getByTestId("generate-error")).not.toContainText("HTTP");
    await expect(page.getByTestId("generate-submit")).toHaveText("Retry");

    await page.goto("/courses");
    const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
    await expect(card).toHaveAttribute("data-course-status", "failed");
    await expect(card.getByTestId("courses-card-stopped")).toHaveAttribute("data-tip", "An error occurred during generation.");
    await expect(card.getByTestId("courses-card-error")).toBeHidden();
    await card.getByTestId("courses-card-stopped").click();
    await expect(card.getByTestId("courses-card-error")).toBeVisible();
    await expect(card.getByTestId("courses-card-error")).toContainText(c.detail);
    await expect(card.getByTestId("courses-card-error")).not.toContainText("Writing your course stopped");
    await card.getByTestId("courses-card-error-close").click();
  }
});

test("a key the provider does not know is refused by the provider, not by the app", async ({ page }) => {
  // The server's only key one the simulated provider has never issued (the
  // owner's, on Manage Users, #25): the app sends it like any key, and the
  // provider's own 401 comes back (#298). The end puts the seeded keys back.
  await devLogin(page, "keys=fake&next=/manage");
  try {
    for (const p of ["openai", "google"]) await removeApiKey(page, p);
    await saveApiKey(page, "sk-ant-api03-not-a-dev-key-9999");
    await createCourse(page);
    await expect(page.getByTestId("generate-provider-only")).toHaveValue("anthropic");
    await expect(page.getByTestId("generate").getByTestId("ai-disclosure")).toBeVisible();
    await page.getByTestId("generate-wish").fill("Volcanoes and how they erupt");
    await page.getByTestId("generate-submit").click();
    expect(await generationEnds(page)).toBe("failed");
    await expect(page.getByTestId("generate-error")).toHaveText(/didn't accept the server's API key/);
  } finally {
    await resetServerKeys(page);
  }
});

test("the course list fills a desktop row and stacks on a phone (#428)", async ({ page }) => {
  // A new learner with one course open: earlier specs leave the dev user many courses of its own.
  await page.context().clearCookies();
  await register(page, newAccount("courses-grid"), "spanish");
  const tops = async () => {
    await page.goto("/courses");
    const cards = [page.getByTestId("courses-projects"), page.getByTestId("courses-builtin"), page.getByTestId("courses-make-section")];
    return Promise.all(cards.map(async (c) => Math.round((await c.boundingBox())!.y)));
  };
  await page.setViewportSize({ width: 1280, height: 900 });
  // (Where Make your own lands depends on how many ready-made cards there are: here the test course is one too.)
  const [projects, builtin] = await tops();
  expect(builtin, "ready-made courses beside your projects").toBe(projects);
  await page.setViewportSize({ width: 390, height: 844 });
  const [p2, b2, m2] = await tops();
  expect(b2).toBeGreaterThan(p2);
  expect(m2).toBeGreaterThan(b2);
});

test("filter the lessons list, and say when nothing matches", async ({ page }) => {
  await page.goto("/lessons");
  const filter = page.getByTestId("lessons-filter");
  const status = page.getByTestId("lessons-filter-status");
  const shown = page.locator('[data-testid="lessons-item"]:visible');
  await filter.fill("ser estar two verbs");
  await expect(shown).toHaveCount(1);
  await expect(shown.first()).toHaveAttribute("data-lesson-id", "ser-vs-estar");
  await expect(status).toHaveAttribute("data-matches", "1");
  await filter.fill("numbers over 100");
  await expect(shown).toHaveCount(0);
  await expect(status).toHaveText(/No lesson matches/);
  await filter.fill("");
  await expect(shown).not.toHaveCount(0);
  await expect(status).toHaveText("");
});

test("look a word up from the lessons filter: its meaning and the lesson that teaches it (#348)", async ({ page }) => {
  await page.goto("/lessons");
  const link = page.getByTestId("lessons-lookup");
  await expect(link).toBeHidden();
  await page.getByTestId("lessons-filter").fill("mesa");
  await expect(link).toBeVisible();
  await link.click();
  await expect(page).toHaveURL(/\/lookup\?q=mesa/);
  const hit = page.locator('[data-testid="lookup-hit"][data-term="la mesa"]');
  await expect(hit).toHaveCount(1);
  await expect(hit).toContainText("table");
  // A Spanish course reads its words aloud.
  await expect(hit.getByTestId("lookup-say")).toHaveAttribute("data-say", "la mesa");
  const lesson = hit.locator('[data-testid="lookup-lesson"][data-lesson-id="nouns-gender-articles-plurals"]');
  await expect(lesson).toHaveCount(1);
  await lesson.click();
  await expect(page).toHaveURL(/\/lessons\/nouns-gender-articles-plurals$/);
  // A conjugated form, accents and case ignored, names the verbs it belongs to.
  await page.goto("/lookup?q=FUIMOS");
  const forms = page.locator('[data-testid="lookup-hit"][data-term="fuimos"]').getByTestId("lookup-form-of");
  await expect(forms.filter({ hasText: /\bir ·/ })).not.toHaveCount(0);
  await expect(forms.filter({ hasText: /\bser ·/ })).not.toHaveCount(0);
  await page.goto("/lookup?q=zzzqx");
  await expect(page.getByTestId("lookup-none")).toBeVisible();
});

test("lessons show one unit at a time, with a tab per unit and page links for a long unit (#837)", async ({ page }) => {
  await page.goto("/lessons");
  const tabs = page.getByTestId("lessons-units-tab");
  expect(await tabs.count()).toBeGreaterThan(1);
  await expect(page.locator('[data-testid="lessons-units-tab"][aria-current="page"]')).toHaveCount(1);
  await expect(page.getByTestId("lessons-unit")).toHaveCount(1);
  // Another tab opens that unit: its tab is the current one, and its lessons replace the first unit's.
  const second = tabs.nth(1);
  const key = (await second.getAttribute("data-key"))!;
  await second.click();
  await expect(page).toHaveURL(/[?&]unit=/);
  await expect(page.locator(`[data-testid="lessons-units-tab"][data-key="${key}"]`)).toHaveAttribute("aria-current", "page");
  await expect(page.locator(`#${key}`)).toHaveCount(1);
  // A unit longer than a page splits into pages of 12.
  const [, total] = (await tabs.first().getByText(/^\d+\/\d+$/).innerText()).split("/").map(Number);
  await page.goto((await tabs.first().getAttribute("href"))! + "&page=1");
  const items = page.getByTestId("lessons-item");
  await expect(items).toHaveCount(Math.min(total, 12));
  if (total > 12) {
    const pager = page.getByTestId("pager");
    await expect(pager).toHaveAttribute("data-page", "1");
    await page.getByTestId("pager-next").click();
    await expect(page).toHaveURL(/page=2/);
    await expect(pager).toHaveAttribute("data-page", "2");
    await expect(items).toHaveCount(Math.min(total - 12, 12));
  }
  // The search covers every unit, and clearing it puts the page back.
  await page.goto((await tabs.nth(1).getAttribute("href"))!);
  const before = await items.count();
  await page.getByTestId("lessons-filter").fill("ser estar two verbs");
  await expect(page.locator('[data-testid="lessons-item"]:visible')).toHaveCount(1);
  await expect(page.locator('[data-testid="lessons-item"]:visible')).toHaveAttribute("data-lesson-id", "ser-vs-estar");
  await page.getByTestId("lessons-filter").fill("");
  await expect(items).toHaveCount(before);
  // Without script the same search is a ?q= link.
  await page.goto("/lessons?q=ser+estar+two+verbs");
  await expect(page.getByTestId("lessons-item")).toHaveCount(1);
});

test("the verb drill setup shows one set at a time, and its locked lessons a page at a time (#837)", async ({ page }) => {
  await page.goto("/play/conjugation");
  const tabs = page.getByTestId("drill-setup-sets-tab");
  expect(await tabs.count()).toBeGreaterThan(1);
  await expect(page.locator("fieldset.tense-group")).toHaveCount(1);
  const last = tabs.last();
  const key = (await last.getAttribute("data-key"))!;
  await last.click();
  await expect(page).toHaveURL(new RegExp(`set=${key}`));
  await expect(last).toHaveAttribute("aria-current", "page");
  await expect(page.getByTestId("drill-setup-set")).toHaveValue(key);
  // The set with the most lessons: at most 8 locked lessons a page.
  await page.goto("/play/conjugation?set=present");
  const groups = page.getByTestId("drill-setup-locked-group");
  expect(await groups.count()).toBeLessThanOrEqual(8);
  if (await page.getByTestId("pager").count()) {
    const first = await groups.first().getAttribute("data-lesson-id");
    await page.getByTestId("pager-next").click();
    await expect(page.getByTestId("drill-setup-set")).toHaveValue("present");
    await expect(groups.first()).not.toHaveAttribute("data-lesson-id", first!);
  }
});

test("pass a lesson's reading check", async ({ page }) => {
  const answers = correctAnswers();
  await page.goto("/lessons");
  // A lesson the seed has not marked read yet.
  const item = page.locator('[data-testid="lessons-item"][data-read="false"]').first();
  const slug = await item.getAttribute("data-lesson-id");
  await page.goto(`/lessons/${slug}`);
  await expect(page.getByTestId("lesson-check")).toHaveAttribute("data-done", "false");

  for (const q of await page.getByTestId("lesson-check-question").all()) {
    const answer = answers.get((await q.getAttribute("data-qid"))!);
    expect(answer, "every check question is in the lesson files").toBeDefined();
    const fill = q.getByTestId("lesson-check-answer");
    if (await fill.count()) await fill.fill(answer!);
    else await q.locator(`[data-testid="lesson-check-option"][value="${answer!.replace(/"/g, '\\"')}"]`).check();
  }
  await page.getByTestId("lesson-check-submit").click();

  await expect(page.getByTestId("lesson-check-result")).toHaveClass(/\bok\b/);
  await page.reload();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "true");
});

test("a note written on a lesson is there when the lesson opens again (#336)", async ({ page }) => {
  const lesson = "/lessons/greetings-and-introductions";
  const saved = () => page.waitForResponse((r) => r.url().endsWith("/api/lessons/greetings-and-introductions/note") && r.ok());
  await page.goto(lesson);
  const box = page.getByTestId("lesson-note-text");
  // The box sits below the lesson text, away from the header.
  const text = (await page.getByTestId("lesson-content").boundingBox())!;
  expect((await box.boundingBox())!.y).toBeGreaterThan(text.y + text.height - 1);
  const note = `ser = DOCTOR, estar = PLACE ${Date.now()}`;
  let save = saved();
  await box.fill(note);
  await save;
  await expect(page.getByTestId("lesson-note-status")).toHaveAttribute("data-state", "saved");
  await page.goto(lesson);
  await expect(page.getByTestId("lesson-note-text")).toHaveValue(note);
  // The (i) says who sees it.
  await page.getByTestId("lesson-note-info").click();
  await expect(page.getByTestId("lesson-note-info-dialog")).toBeVisible();
  await page.getByTestId("lesson-note-info-close").click();
  // Emptied, it stays empty.
  save = saved();
  await page.getByTestId("lesson-note-text").fill("");
  await save;
  await page.goto(lesson);
  await expect(page.getByTestId("lesson-note-text")).toHaveValue("");
});

test("a lesson's tappable words look tappable and take the keyboard (#403)", async ({ page }) => {
  await page.goto("/lessons/greetings-and-introductions");
  const term = page.getByTestId("lesson-vocab-item").first().locator("td").first();
  await expect(term).toHaveClass(/\bcan-say\b/);
  await expect(term).toHaveAttribute("tabindex", "0");
  expect(await term.evaluate((el) => getComputedStyle(el).textDecorationStyle)).toBe("dotted");
  await term.focus();
  expect(await term.evaluate((el) => getComputedStyle(el).textDecorationStyle)).toBe("solid");
  // The meaning column is not the course's language: no underline there.
  const meaning = page.getByTestId("lesson-vocab-item").first().locator("td").nth(1);
  await expect(meaning).not.toHaveClass(/\bcan-say\b/);
});

test.describe("tap a word on a phone (#859)", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("tapping a word shows its meaning in a bubble that fits the screen", async ({ page }) => {
    await page.goto("/lessons/greetings-and-introductions");
    const bubble = page.getByTestId("say-meaning");
    const fits = () => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
    // A vocabulary term: the bubble reads the row's meaning.
    const row = page.getByTestId("lesson-vocab-item").first();
    const meaning = ((await row.locator("td").nth(1).textContent()) || "").trim();
    expect(meaning).not.toBe("");
    await row.locator("td").first().tap();
    await expect(bubble).toBeVisible();
    await expect(bubble).toHaveText(meaning);
    expect(await fits()).toBe(true);
    const box = (await bubble.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(390);
    // A bold word in the lesson text: looked up in the vocabulary table.
    const hola = page.getByTestId("lesson-content").locator("strong.can-say", { hasText: /^Hola$/ }).first();
    await hola.scrollIntoViewIfNeeded();
    await hola.tap();
    await expect(bubble).toBeVisible();
    await expect(bubble).toHaveText("hello");
    expect(await fits()).toBe(true);
    // A tap anywhere else puts it away.
    await page.locator("h1").first().tap();
    await expect(bubble).toBeHidden();
  });
});

test("finish a Sprint mode session", async ({ page }) => {
  test.setTimeout(180_000);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/sprint?reduce-motion=1");
  await page.getByTestId("sprint-continue").first().click();
  await playSprintSession(page);
  await expect(page.getByTestId("sprint-play-done")).toBeVisible();
});

test("Sprint home fits a phone in every app language (#670)", async ({ page }) => {
  // German's "Wiederholungsmix" once pushed the Review / Lightning row 21 px past the edge.
  const langs = readdirSync(new URL("../../i18n/", import.meta.url))
    .filter((f) => f.endsWith(".yaml"))
    .map((f) => f.replace(/\.yaml$/, ""));
  expect(langs).toContain("de");
  await page.setViewportSize({ width: 390, height: 844 });
  try {
    for (const lang of langs) {
      const r = await page.request.post("/api/profile", { data: { lang } });
      expect(r.ok(), lang).toBe(true);
      await page.goto("/sprint");
      await expect(page.locator("html")).toHaveAttribute("lang", lang);
      await expect(page.getByTestId("sprint-review")).toBeVisible();
      expect(await page.evaluate(() => document.documentElement.scrollWidth), lang).toBeLessThanOrEqual(390);
    }
  } finally {
    await page.request.post("/api/profile", { data: { lang: "en" } });
  }
});

test("on a phone the History table scrolls in its box, not the page (#424)", async ({ page }) => {
  // A wrong answer with a long text makes the five-column table wider than the phone.
  const r = await page.request.post("/api/answer", { data: { question_id: "a1-adj-01", answer: "una respuesta equivocada bastante larga" } });
  expect(r.ok()).toBe(true);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/history");
  await expect(page.getByTestId("history-answer").first()).toBeVisible();
  const box = page.getByTestId("history-answers-scroll");
  expect(await box.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});

test("on a phone the Vocabulary term table scrolls in its box, not the page (#933)", async ({ page }) => {
  // Right answers in Greetings and introductions collect its words.
  const lesson = JSON.parse(readFileSync(new URL("../../data/courses/spanish/lessons/a1/greetings-and-introductions.json", import.meta.url), "utf8"));
  for (const q of lesson.quiz) {
    const r = await page.request.post("/api/answer", { data: { question_id: q.question_id, answer: q.correct_answer } });
    expect(r.ok()).toBe(true);
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/vocabulary");
  const greetings = page.locator('[data-testid="vocabulary-lesson"][data-lesson-id="greetings-and-introductions"]');
  await greetings.locator("summary").click();
  await expect(greetings.getByTestId("vocabulary-term").first()).toBeVisible();
  const box = greetings.getByTestId("vocabulary-terms-scroll");
  expect(await box.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});

test("on a phone the Study and Play mode bars take two rows and scroll sideways (#423)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  for (const [path, nav, current, other] of [
    ["/mistakes", "nav-study", "nav-study-mistakes", "nav-study-to-sprint"],
    ["/guide", "nav-play", "nav-play-stages", "nav-play-to-study"],
  ]) {
    await page.goto(path);
    const bar = page.getByTestId(nav);
    const rowHeight = (await page.getByTestId(current).boundingBox())!.height;
    expect((await bar.boundingBox())!.height).toBeLessThan(2.6 * rowHeight);
    // The Switch links stay, and the current tab (the last one) is scrolled into view.
    await expect(page.getByTestId(other)).toBeInViewport();
    await expect(page.getByTestId(current)).toBeInViewport({ ratio: 0.9 });
    // The bar fits the screen (the page below may be wider, e.g. /history before #424).
    expect(await bar.evaluate((el) => el.scrollWidth)).toBeLessThanOrEqual(390);
  }
});

test("the modes' old paths redirect, keeping the query string", async ({ page }) => {
  for (const [from, to] of [
    ["/d?reduce-motion=1", "/sprint?reduce-motion=1"],
    ["/d/play?mode=review", "/sprint/play?mode=review"],
    ["/k", "/lessons"],
    ["/study?x=1", "/lessons?x=1"],
    ["/g", "/play"],
    ["/games", "/play"],
    ["/games/matching", "/play/matching"],
  ]) {
    const res = await page.request.get(from, { maxRedirects: 0 });
    expect(res.status(), from).toBe(308);
    const to_ = new URL(res.headers()["location"], "http://x");
    expect(to_.pathname + to_.search, from).toBe(to);
  }
  const res = await page.request.post("/api/d/finish", { maxRedirects: 0 });
  expect(res.status()).toBe(308);
  expect(res.headers()["location"]).toMatch(/\/api\/sprint\/finish$/);
  await page.goto("/k");
  await expect(page.getByTestId("nav-study")).toBeVisible();
});

test("the mode bars link to Study's Lessons, not to the /study redirect", async ({ page }) => {
  for (const path of ["/", "/lessons", "/play", "/guide", "/sprint"]) {
    await page.goto(path);
    const hrefs = await page.locator("a[href]").evaluateAll((as) => as.map((a) => new URL((a as HTMLAnchorElement).href).pathname));
    expect(hrefs.filter((h) => h === "/study" || h === "/k"), path).toEqual([]);
  }
  await page.goto("/play");
  await expect(page.getByTestId("nav-play-to-study")).toHaveAttribute("href", /\/lessons$/);
});

test("the profile has no Dev section for anyone; the seeded keys answer generate and the tutor", async ({ page, browser }) => {
  // A plain learner: no Dev section, and posting its old form does nothing.
  const other = await browser.newContext();
  const them = await other.newPage();
  const name = `plain${Date.now().toString(36)}`;
  await them.goto("/register");
  await them.getByTestId("register-email").fill(`${name}@example.com`);
  await them.getByTestId("register-password").fill("correct horse battery");
  await them.getByTestId("register-age").check();
  await them.getByTestId("register-submit").click();
  // Registering signs the new account in.
  await expect(them.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", `${name}@example.com`);
  await them.goto("/profile");
  await expect(them.getByTestId("profile-two-factor")).toBeVisible();
  await expect(them.locator('[data-testid^="profile-api-key"]')).toHaveCount(0);
  await expect(them.getByTestId("profile-dev")).toHaveCount(0);
  await expect(them.locator('[data-testid="profile-nav-link"][data-section="dev"]')).toHaveCount(0);
  expect((await them.request.post("/profile/dev", { form: { mock_ai: "1" }, maxRedirects: 0 })).status()).toBe(404);
  expect((await them.request.post("/dev/stale-save", { maxRedirects: 0 })).status()).toBe(404);
  await other.close();

  // The dev user (a developer account) has no Dev section either; every nav link has its section.
  await devLogin(page, "keys=dev&next=/profile");
  await expect(page.getByTestId("profile-dev")).toHaveCount(0);
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="dev"]')).toHaveCount(0);
  for (const link of await page.getByTestId("profile-nav-link").all()) {
    await expect(page.locator(`#${await link.getAttribute("data-section")}`)).toHaveCount(1);
  }
  // The server's keys (keys=dev saves them), one per provider, are the simulated provider's.
  await page.goto("/manage#ai");
  for (const provider of ["anthropic", "openai", "google"]) {
    await expect(page.locator(`[data-testid="manage-api-key"][data-provider="${provider}"]`), provider).toHaveAttribute("data-key-set", "true");
  }

  // Generate with Google's key: the simulator answers in OpenAI's format, from the fixtures.
  await createCourse(page);
  await page.locator('[data-testid="generate-provider"][value="google"]').check();
  await page.getByTestId("generate-wish").fill("Volcanoes, through the simulator");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");

  // The tutor talks too, from the tutor-turn fixture.
  await devLogin(page);
  await page.goto("/tutor");
  await expect(page.getByTestId("tutor-chat")).toBeVisible();
  await expect(page.getByTestId("tutor-thread")).toContainText("practise this lesson together");
});

test("the profile's e-mail field: a Verified check, and an (i) dialog with the name others see", async ({ page }) => {
  await page.goto("/profile"); // the dev user's address is confirmed
  const check = page.getByTestId("email-verified");
  await expect(page.getByTestId("profile-email")).toHaveAttribute("readonly", "");
  await expect(check).toBeVisible();
  await expect(check).toHaveAttribute("aria-label", "Verified");
  await expect(page.getByTestId("profile-form")).not.toContainText("cannot be changed");
  await expect(page.getByTestId("profile-form")).not.toContainText("first name and initial");
  const first = page.getByTestId("profile-first-name"), last = page.getByTestId("profile-last-name");
  const [firstWas, lastWas] = [await first.inputValue(), await last.inputValue()];

  const note = page.getByTestId("profile-privacy-note");
  await expect(note).toBeHidden();
  await page.getByTestId("profile-privacy-info").click();
  await expect(note).toBeVisible();
  await expect(note).toContainText("Your e-mail address is private to you.");
  await page.getByTestId("profile-privacy-close").click();
  await expect(note).toBeHidden();
  // It follows the name fields as they are typed.
  await first.fill("Ana");
  await last.fill("lópez");
  await expect(page.getByTestId("profile-public-name")).toHaveText("Ana L.");
  await last.fill("");
  await expect(page.getByTestId("profile-public-name")).toHaveText("Ana");
  await first.fill("");
  await expect(page.getByTestId("profile-public-name")).toHaveText("Learner");
  await page.getByTestId("profile-privacy-info").click();
  await expect(note).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(note).toBeHidden();

  // Autosave kept those edits: put the dev user's name back.
  await first.fill(firstWas);
  await last.fill(lastWas);
  await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
  await page.reload();
  await expect(page.getByTestId("profile-public-name")).toHaveText(lastWas ? `${firstWas} ${lastWas[0].toUpperCase()}.` : firstWas || "Learner");
});

test("Start asks for a course's settings before it opens, then keeps them behind its gear; the password changes in a dialog", async ({ browser }) => {
  // A fresh account, so the password is known and the dev user keeps its own.
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  const name = `pic${Date.now().toString(36)}`;
  await page.goto("/register");
  await page.getByTestId("register-email").fill(`${name}@example.com`);
  await page.getByTestId("register-password").fill("correct horse battery");
  await page.getByTestId("register-age").check();
  await page.getByTestId("register-submit").click();
  // Registering signs the new account in.
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", `${name}@example.com`);
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", `${name}@example.com`);
  // The corner shows a gear (#577), and the profile has no picture to edit (#565).
  const corner = page.getByTestId("nav-account-toggle");
  await page.goto("/profile");
  await expect(corner.getByTestId("avatar-blank")).toHaveCount(1);
  await expect(corner.locator('[data-testid="avatar-blank"] svg circle')).toHaveCount(1);
  await expect(page.getByTestId("profile-avatar-edit")).toHaveCount(0);

  // Start on a course not opened yet opens its settings as a wizard over the
  // course list (#574, #587): a step a page, arrows on either side, two
  // dots, and "Get Started!" under every step (#896).
  await page.goto("/courses");
  const card = page.locator('[data-testid="courses-card"][data-course-id="spanish"]');
  const dialog = page.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
  const prev = dialog.getByTestId("courses-settings-prev");
  const next = dialog.getByTestId("courses-settings-next");
  const save = dialog.getByTestId("courses-settings-save");
  await card.getByTestId("courses-open").click();
  await expect(dialog).toBeVisible();
  await expect(page).toHaveURL(/\/courses$/);
  await expect(dialog).toHaveAttribute("data-wizard", "");
  await expect(dialog).toHaveAttribute("data-step", "0");
  // Closing it opens nothing: still the course list, Spanish not started.
  await dialog.getByTestId("courses-settings-close").click();
  await expect(dialog).toBeHidden();
  await expect(page).toHaveURL(/\/courses$/);
  await page.reload();
  await expect(page.getByTestId("courses-builtin").locator('[data-testid="courses-card"][data-course-id="spanish"]')).toHaveCount(1);
  await card.getByTestId("courses-open").click();
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveAttribute("data-step", "0");
  await expect(prev).toBeHidden();
  await expect(dialog.locator(".cs-dots span")).toHaveCount(2);
  await expect(dialog.getByTestId("courses-settings-step")).toHaveCount(2);
  await expect(save).toBeVisible();
  // Step one: the motivator, with an (i) on what a motivator is.
  await expect(dialog.locator('[data-testid="courses-settings-motivator"][data-motivator-id="stats"] input')).toBeChecked();
  await dialog.getByTestId("courses-settings-motivator-info").click();
  await expect(dialog.getByTestId("courses-settings-motivator-dialog")).toBeVisible();
  await dialog.getByTestId("courses-settings-motivator-close").click();
  await expect(dialog.getByTestId("courses-settings-motivator-dialog")).toBeHidden();
  await expect(dialog).toBeVisible();
  await dialog.locator('[data-testid="courses-settings-motivator"][data-motivator-id="xp"]').click();
  // Step two, the last (#1106): the level (Spanish has levels), the beginning chosen, with an (i).
  await next.click();
  await expect(dialog).toHaveAttribute("data-step", "1");
  await expect(prev).toBeVisible();
  await expect(dialog.locator('[data-testid="courses-settings-level"][data-unit=""] input')).toBeChecked();
  await dialog.getByTestId("courses-settings-level-info").click();
  await expect(dialog.getByTestId("courses-settings-level-dialog")).toBeVisible();
  await dialog.getByTestId("courses-settings-level-close").click();
  await expect(dialog).toBeVisible();
  await expect(next).toBeHidden();
  // "Get Started!": the course opens in these settings.
  await expect(save).toBeVisible();
  await save.click();
  await expect(page).toHaveURL(/\/$/);
  await expect(dialog).toHaveCount(0);
  await expect(page.locator('[data-testid="home-today"]')).toHaveAttribute("data-motivator", "xp");
  await expect(corner.getByTestId("avatar-blank")).toHaveCount(1); // the gear, inside a course too
  await page.reload();
  await expect(dialog).toHaveCount(0); // it asks once

  // Behind the card's gear the settings are one sheet (#576): a row a
  // setting, no steps, Save, and an (i) saying they can be swapped any time.
  await page.goto("/courses");
  await expect(corner.getByTestId("avatar-blank")).toHaveCount(1);
  await page.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings").click();
  await expect(dialog).toBeVisible();
  await expect(prev).toHaveCount(0);
  await expect(dialog.getByTestId("courses-settings-step")).toHaveCount(0);
  await expect(dialog.getByTestId("courses-settings-row")).toHaveCount(2); // the level and the motivator
  // The level is a ladder with a note on the one chosen.
  await expect(dialog.getByTestId("courses-settings-level-note")).toContainText("From the beginning");
  await dialog.locator('[data-testid="courses-settings-level"][data-unit="B1"]').click();
  await expect(dialog.getByTestId("courses-settings-level-note")).not.toContainText("From the beginning");
  await expect(dialog.getByTestId("courses-settings-level-note")).toContainText("18 lessons");
  await dialog.getByTestId("courses-settings-about").click();
  await expect(dialog.getByTestId("courses-settings-about-dialog")).toBeVisible();
  await dialog.getByTestId("courses-settings-about-close").click();
  await expect(dialog.locator('[data-testid="courses-settings-motivator"][data-motivator-id="xp"] input')).toBeChecked();
  // Closing without Save keeps what was saved: the level.
  await dialog.getByTestId("courses-settings-close").click();
  await expect(dialog).toBeHidden();
  await expect(dialog.locator('[data-testid="courses-settings-level"][data-unit=""] input')).toBeChecked();
  // Save: the motivator changes in place, nothing else is reset.
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings").click();
  await dialog.locator('[data-testid="courses-settings-motivator"][data-motivator-id="stats"]').click();
  await save.click();
  await expect(dialog).toBeHidden();
  await page.goto("/");
  await expect(page.locator('[data-testid="home-today"]')).toHaveAttribute("data-motivator", "stats");
  await page.goto("/profile");
  await expect(page.getByTestId("profile-avatar-shown")).toHaveCount(0); // the profile shows no picture (#577)

  // Password: mismatched or short, the form is not sent and the dialog stays.
  const pw = page.getByTestId("profile-password-dialog");
  await expect(pw).toBeHidden();
  await page.getByTestId("profile-password-open").click();
  await expect(pw.getByTestId("profile-password-rule")).toHaveText("8 to 128 characters.");
  await page.getByTestId("profile-password-current").fill("not the password");
  await page.getByTestId("profile-password-new").fill("a new password");
  await page.getByTestId("profile-password-confirm").fill("a new passw0rd");
  await page.getByTestId("profile-password-submit").click();
  await expect(pw).toBeVisible();
  await expect(page).toHaveURL(/\/profile$/);

  // The wrong current password comes back with the dialog open and the error in it.
  await page.getByTestId("profile-password-confirm").fill("a new password");
  await page.getByTestId("profile-password-submit").click();
  await expect(pw.getByTestId("profile-password-error")).toHaveText(/current password is not right/);
  await expect(pw).toBeVisible();

  // Right: the dialog closes and the page says so.
  await page.getByTestId("profile-password-current").fill("correct horse battery");
  await page.getByTestId("profile-password-new").fill("a new password");
  await page.getByTestId("profile-password-confirm").fill("a new password");
  await page.getByTestId("profile-password-submit").click();
  await expect(page.getByTestId("profile-notice")).toHaveText(/Password changed/);
  await expect(pw).toBeHidden();
  await ctx.close();
});

test("a page that does not exist is a 404 inside the normal layout, with ways back (#397)", async ({ page, browser }) => {
  const res = await page.goto("/lessons/no-such-lesson");
  expect(res!.status()).toBe(404);
  await expect(page.getByTestId("error-page")).toHaveAttribute("data-status", "404");
  // The top bar: the course and the account menu, as on every other page.
  await expect(page.getByTestId("nav-course-switch")).toBeVisible();
  await expect(page.getByTestId("nav-account-toggle")).toBeVisible();
  await expect(page.getByTestId("error-lessons")).toBeVisible();
  await page.getByTestId("error-courses").click();
  await expect(page).toHaveURL(/\/courses$/);
  // Signed out: the same page, with the signed-out top bar and a way home.
  const anon = await browser.newPage();
  expect((await anon.goto(new URL("/no-such-page", page.url()).href))!.status()).toBe(404);
  await expect(anon.getByTestId("nav-login")).toBeVisible();
  await expect(anon.getByTestId("error-home")).toBeVisible();
  await expect(anon.getByTestId("error-courses")).toHaveCount(0);
  await anon.close();
});

test("a form-only URL opened by address is a 405 inside the normal layout, with a way home (#1253)", async ({ page, browser }) => {
  const formOnly = "This page only works from its button. Go back and use the button on the page.";
  // Signed in: the error page with the learner's top bar. A GET does not sign out.
  for (const path of ["/logout"]) {
    const res = await page.goto(path);
    expect(res!.status()).toBe(405);
    expect(res!.headers()["allow"]).toBe("POST");
    await expect(page.getByTestId("error-page")).toHaveAttribute("data-status", "405");
    await expect(page.getByTestId("error-message")).toHaveText(formOnly);
    await expect(page.getByTestId("nav-account-toggle")).toBeVisible();
    await expect(page.getByTestId("error-home")).toBeVisible();
  }
  // Signed out: the same page, with the signed-out top bar.
  const anon = await browser.newPage();
  for (const path of ["/logout"]) {
    const res = await anon.goto(new URL(path, page.url()).href);
    expect(res!.status()).toBe(405);
    await expect(anon.getByTestId("error-message")).toHaveText(formOnly);
    await expect(anon.getByTestId("nav-login")).toBeVisible();
    await expect(anon.getByTestId("error-home")).toBeVisible();
  }
  await anon.close();
  // The real POST still works: the Log out button signs out.
  await logout(page);
  await expect(page.getByTestId("nav-login")).toBeVisible();
});

test("the site installs as an app: every layout links a manifest that opens at the home page (#345)", async ({ page, browser }) => {
  // No sign-in needed: the login page and the manifest are public.
  const anon = await browser.newPage();
  await anon.goto("/login");
  const href = (await anon.locator('link[rel="manifest"]').getAttribute("href"))!;
  expect(href).toMatch(/\/manifest\.webmanifest$/);
  await expect(anon.locator('link[rel="apple-touch-icon"]')).toHaveCount(1);
  await expect(anon.locator('meta[name="apple-mobile-web-app-capable"]')).toHaveAttribute("content", "yes");
  const res = await anon.request.get(href);
  expect(res.status()).toBe(200);
  expect(res.headers()["content-type"]).toContain("application/manifest+json");
  const m = await res.json();
  const base = href.replace(/\/manifest\.webmanifest$/, "");
  expect(m.name).toBe("Skrongle");
  expect(m.short_name).toBe("Skrongle");
  expect(m.display).toBe("standalone");
  expect(m.start_url).toBe(`${base}/`);
  expect(m.scope).toBe(`${base}/`);
  expect(m.theme_color).toMatch(/^#[0-9a-f]{6}$/);
  expect(m.background_color).toMatch(/^#[0-9a-f]{6}$/);
  expect(m.icons.map((i: { sizes: string }) => i.sizes).sort()).toEqual(["192x192", "512x512"]);
  for (const icon of m.icons) {
    const img = await anon.request.get(icon.src);
    expect(img.status()).toBe(200);
    expect(img.headers()["content-type"]).toBe("image/png");
  }
  // Nothing is cached for offline use: no service worker is registered.
  expect(await anon.evaluate(async () => (await navigator.serviceWorker?.getRegistrations())?.length ?? 0)).toBe(0);
  await anon.close();

  // The signed-in layouts link it too: the course home and Sprint mode.
  await page.goto("/");
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute("href", href);
  await page.goto("/sprint");
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute("href", href);
});

// #1154/#1197: a lesson with no quiz (the test course's sort lesson) offers no quiz,
// no Practise or Test yourself, and no mastery pill. Needs the test course on.
test("#1154 a lesson without questions offers no practice", async ({ page }) => {
  await devLogin(page, "open=test-course&next=/lessons/sort");
  await expect(page.getByTestId("lesson-mastery")).toHaveCount(0);
  await expect(page.getByTestId("lesson-test")).toHaveCount(0);
  await expect(page.getByTestId("lesson-read").first()).not.toContainText(/quiz/i);
  await page.getByTestId("lesson-read").first().click();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-read", "true");
  await expect(page.getByTestId("lesson-practise")).toHaveCount(0);
  await expect(page.getByTestId("lesson-test")).toHaveCount(0);
  await expect(page.getByTestId("lesson-mastery")).toHaveCount(0);

  // Reached by URL, the practice page says the lesson has no questions and
  // leads back to it.
  await page.goto("/practice?lesson=sort");
  await expect(page.getByTestId("quiz-empty")).toBeVisible();
  await expect(page.getByTestId("quiz-sprint")).toHaveAttribute("href", /\/sprint\/play\?lesson=sort$/);
  await page.getByTestId("quiz-back-to-lesson").click();
  await expect(page.getByTestId("lesson")).toHaveAttribute("data-lesson-id", "sort");
});

// #1253: a GET to a form-only URL (/logout, …) gets the error page
// in the normal layout with ways back, like a 404 does (#397), not a blank body.
test.fixme("#1253 a form-only URL opened directly shows the error page, not a blank one", async ({ page }) => {
  await devLogin(page, "open=spanish");
  for (const path of ["/logout"]) {
    const r = await page.goto(path);
    expect(r?.status()).toBe(405);
    await expect(page.getByTestId("error-page")).toBeVisible();
    await expect(page.getByTestId("error-page")).toHaveAttribute("data-status", "405");
    await expect(page.getByRole("link", { name: /home/i })).toBeVisible();
  }
});

// #1254: the course card's counts pick the plural by the number. A course made
// with Create a course has one lesson and one unit.
test.fixme("#1254 the course card says 1 lesson · 1 unit, not 1 lessons · 1 units", async ({ page }) => {
  await devLogin(page, "open=spanish");
  await page.goto("/courses");
  await page.getByTestId("courses-create").first().click();
  await page.keyboard.press("Escape");
  await page.goto("/courses");
  const mine = page.getByTestId("courses-mine").first();
  await expect(mine).toContainText("1 lesson · 1 unit");
  await expect(mine).not.toContainText("1 lessons");
});

// #1213: a new learner's Guide list on a phone folds the locked stages into
// one quiet list per unit, with one lock each and no padlock emoji, and the
// page stays a few screens long instead of a hundred.
test("the Guide list folds locked stages under one lock per unit (#1213)", async ({ browser }) => {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const page = await ctx.newPage();
  await register(page, newAccount("guide-locks"), "spanish");
  await page.goto("/guide");
  const units = page.getByTestId("guide-unit");
  expect(await units.count()).toBeGreaterThan(1);
  const withLocked = await units.evaluateAll((els) => els.filter((e) => e.querySelector('[data-state="locked"]')).length);
  expect(withLocked).toBeGreaterThan(0);
  await expect(page.getByTestId("guide-lock")).toHaveCount(withLocked);
  await expect(page.getByTestId("guide-locked")).toHaveCount(withLocked);
  expect(await page.locator("main").innerText()).not.toContain("🔒");
  // The next stage is open and comes first.
  const first = page.getByTestId("guide-stage").first();
  await expect(first).toHaveAttribute("data-current", "true");
  await expect(first).not.toHaveAttribute("data-state", "locked");
  const height = await page.evaluate(() => document.documentElement.scrollHeight);
  expect(height, "the Guide list at 390 px").toBeLessThan(6000);
  // Opening a unit's locked list shows its rows and the unlock rule once.
  const locked = page.getByTestId("guide-locked").first();
  await locked.locator("summary").click();
  await expect(locked.locator('[data-testid="guide-stage"][data-state="locked"]').first()).toBeVisible();
  await ctx.close();
});

// #1306: the lessons list and Listen and type link to a lesson's quiz only
// when it has quiz questions (the test course's sort and listening lessons have none).
test("#1306 no Practise link for a lesson without quiz questions", async ({ page }) => {
  await devLogin(page, "open=test-course&next=/lessons?unit=all");
  const row = (slug: string) => page.locator(`[data-testid="lessons-item"][data-lesson-id="${slug}"]`);
  await expect(row("sort")).toBeVisible();
  await expect(row("sort").getByTestId("lessons-item-practise")).toHaveCount(0);
  await expect(row("multiple-choice").getByTestId("lessons-item-practise")).toHaveCount(1);
  // The round-complete link is in the page from the start, hidden until the end.
  await page.goto("/listening/play?lesson=listening");
  await expect(page.getByTestId("listen-done")).toBeAttached();
  await expect(page.getByTestId("listen-quiz-lesson")).toHaveCount(0);
  await page.goto("/listening/play?lesson=everything");
  await expect(page.getByTestId("listen-quiz-lesson")).toHaveCount(1);
});
