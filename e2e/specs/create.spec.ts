// Exact specs for "Create a course" (#144): one button on /courses makes a
// draft and opens its editor with the Generate dialog. The title is renamed
// in the editor; names (course and lessons) keep the dialog on offer and go
// to the AI, any other edit takes it away. Generating is followed in the
// editor's progress modal, which closes by itself when the course is
// written. The simulated provider answers, from the fixtures.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, editorSaved, fastAutosave, generationEnds } from "../lib/app";
import { newAccount, register, removeApiKey, resetServerKeys, saveApiKey } from "../lib/learner";

test.beforeEach(async ({ page }) => {
  await fastAutosave(page);
  await devLogin(page, "keys=fake&next=/courses");
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const dialog = (page: Page) => page.getByTestId("generate");
const titles = (page: Page) => page.getByTestId("course-lessons-title");
const progress = (page: Page) => page.getByTestId("course-ai-modal");

async function renameCourse(page: Page, title: string): Promise<void> {
  await page.getByTestId("course-edit-rename").click();
  await page.getByTestId("course-edit-rename-input").fill(title);
  await page.keyboard.press("Enter");
}

test("create, close the dialog, name the course and its lessons, then generate into it", async ({ page }) => {
  await expect(page.getByTestId("courses-build")).toHaveCount(0);
  await expect(page.getByTestId("courses-generate")).toHaveCount(0);
  const id = await createCourse(page);
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?generate=1$`));
  await expect(page.getByTestId("course-edit-title")).toHaveText("Untitled course");
  await expect(page.getByTestId("generate-named")).toHaveCount(0);

  // Closed: the starter course stays, and Use AI opens the dialog again.
  await page.getByTestId("generate-close").click();
  await expect(dialog(page)).toBeHidden();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}$`));
  await expect(titles(page)).toHaveText(["Your first lesson"]);
  await page.getByTestId("course-ai-generate").click();
  await expect(dialog(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();

  // The title is renamed in place and saved straight away.
  await renameCourse(page, "Fractions for Friday");
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions for Friday");
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toHaveText("Renamed the course to “Fractions for Friday”");
  await page.reload();
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions for Friday");

  // The pencil after the title (#422) says so and renames the same way; Escape keeps the name.
  const pencil = page.getByTestId("course-edit-rename-pencil");
  await expect(pencil).toBeVisible();
  await expect(pencil).toHaveAttribute("aria-label", "Rename the course");
  await expect(pencil).toHaveAttribute("title", "Click to rename");
  await pencil.click();
  await expect(page.getByTestId("course-edit-rename-input")).toHaveValue("Fractions for Friday");
  await expect(pencil).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions for Friday");
  await expect(pencil).toBeVisible();

  // Lesson names are names too: renamed, and one added by name alone.
  await page.getByTestId("course-lessons-title").first().click();
  await page.getByTestId("course-lessons-rename").fill("Halves and quarters");
  await page.keyboard.press("Enter");
  await page.getByTestId("course-lessons-add-here").first().click();
  await page.getByTestId("course-lessons-rename").fill("Comparing fractions");
  await page.keyboard.press("Enter");
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toContainText(/1 added|1 renamed/);

  // Still a draft: the dialog is on offer and says what the AI keeps.
  await page.getByTestId("course-ai-generate").click();
  await expect(page.getByTestId("generate-named")).toHaveText("The AI keeps the title “Fractions for Friday” and your lessons “Halves and quarters”, “Comparing fractions”.");
  // The title alone is enough to ask for.
  await page.getByTestId("generate-submit").click();
  // Straight back to the same course's editor; the modal closes when it is written.
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?job=`));
  expect(await generationEnds(page)).toBe("done");
  await expect(progress(page)).toHaveCount(0);
  // Written into the same course, under the learner's title.
  await expect(page.getByTestId("course-edit-title")).toHaveText("Fractions for Friday");
  await expect(titles(page)).toHaveText(["Simplifying", "What a fraction is", "Adding fractions"]);
  await expect(page.getByTestId("course-ai-generate")).toHaveCount(0);
  await expect(dialog(page)).toHaveCount(0);
  await expect(page.getByTestId("course-undo-form")).toHaveCount(0);
});

test("a new course's card counts in the singular: 1 lesson · 1 unit, 1 Lektion · 1 Einheit (#1254)", async ({ page, context }) => {
  const id = await createCourse(page);
  await page.goto("/courses");
  const card = page.getByTestId("courses-mine").locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card).toContainText("1 lesson · 1 unit · ");
  await expect(card).not.toContainText("1 lessons");
  await context.addCookies([{ name: "lang", value: "de", url: page.url() }]);
  await page.goto("/courses");
  await expect(card).toContainText("1 Lektion · 1 Einheit · ");
  await expect(card).not.toContainText("1 Lektionen");
});

test("editing a lesson's content takes Generate away", async ({ page }) => {
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const lesson = page.getByTestId("course-lessons-row").first();
  await lesson.getByTestId("course-lessons-expand").click();
  const json = lesson.getByTestId("course-lessons-json");
  await expect(json).toHaveValue(/"summary"/);
  const v = JSON.parse(await json.inputValue());
  v.summary = "Fractions, my way.";
  await json.fill(JSON.stringify(v, null, 2));
  await editorSaved(page);
  await expect(page.getByTestId("course-undo-label")).toContainText("1 edited");
  // Use AI is drawn again without a reload: Generate is gone.
  await expect(page.getByTestId("course-ai-form")).toBeVisible();
  await expect(page.getByTestId("course-ai-generate")).toHaveCount(0);
  // ?generate=1 and a direct post both find no dialog any more.
  await page.goto(`/courses/${id}?generate=1`);
  await expect(dialog(page)).toHaveCount(0);
  const r = await page.request.post(`/courses/${id}/generate`, { form: { provider: "anthropic", wish: "Fractions" }, maxRedirects: 0 });
  expect(r.status()).toBe(303);
  expect(r.headers().location).toContain("notice=");
});

test("a refused generate form comes back open with what was typed", async ({ page }) => {
  await createCourse(page);
  await page.getByTestId("generate-wish").fill("ab");
  await page.getByTestId("generate-submit").click();
  await expect(dialog(page)).toBeVisible();
  await expect(page.getByTestId("generate-error")).toHaveText("Your prompt is too short! Please tell us what you'd like to learn or paste your notes so the AI has enough information to get to work.");
  await expect(page.getByTestId("generate-wish")).toHaveValue("ab");
});

test("the effort beside Generate opens its own dialog and the account keeps the last level (#918)", async ({ page }) => {
  await createCourse(page);
  const effort = page.getByTestId("generate-effort");
  const levels = page.getByTestId("generate-effort-dialog");
  await expect(effort).toHaveText("Effort: Balanced");
  await expect(levels).toBeHidden();
  await effort.click();
  await expect(levels).toBeVisible();
  await expect(levels.getByRole("heading")).toHaveText("How hard should the AI work?");
  await expect(page.getByTestId("generate-effort-balanced")).toBeChecked();
  await page.getByTestId("generate-effort-thorough").check();
  await page.getByTestId("generate-effort-done").click();
  // Done closes only the effort dialog; Generate's stays open with the new level named.
  await expect(levels).toBeHidden();
  await expect(dialog(page)).toBeVisible();
  await expect(effort).toHaveText("Effort: Thorough");

  await page.getByTestId("generate-wish").fill("Fractions for a Friday quiz, year 6");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");

  // The next course starts at Thorough.
  await page.goto("/courses");
  await createCourse(page);
  await expect(page.getByTestId("generate-effort")).toHaveText("Effort: Thorough");
  await expect(page.getByTestId("generate-effort-thorough")).toBeChecked();
});

test("the old pages lead to the new flow", async ({ page }) => {
  await page.goto("/courses/new");
  await expect(page).toHaveURL(/\/courses$/);
  await page.goto("/courses/generate");
  await expect(page).toHaveURL(/\/courses$/);
});

test("the dialog asks which provider only when the server has two or more keys", async ({ page }) => {
  // keys=fake saves the simulated provider's key for each of the three
  // providers: the dialog asks, Anthropic first, and says where the notes go.
  await createCourse(page);
  await expect(page.getByTestId("generate-provider")).toHaveCount(3);
  await expect(page.locator('[data-testid="generate-provider"][value="anthropic"]')).toBeChecked();
  await expect(page.getByTestId("generate-no-key")).toHaveCount(0);
  await expect(page.getByTestId("generate").getByTestId("ai-disclosure")).toContainText("Claude");
  for (const field of ["generate-api-key", "generate-save-key", "generate-model", "generate-base-url"]) {
    await expect(page.getByTestId(field), field).toHaveCount(0);
  }

  // The keys are the server's (#25): with none, another account's dialog
  // is greyed out under a line that says to ask the owner; no stand-in, even
  // in dev mode.
  const ctx = await page.context().browser()!.newContext();
  const fresh = await ctx.newPage();
  await register(fresh, newAccount("nokey"), "spanish");
  try {
    for (const p of ["anthropic", "openai", "google"]) await removeApiKey(page, p);
    await createCourse(fresh);
    await expect(fresh.getByTestId("generate-no-key")).toBeVisible();
    await expect(fresh.getByTestId("generate-no-key")).toContainText("Ask the owner of this server");
    await expect(fresh.getByTestId("generate-disabled")).toHaveCount(1);
    await expect(fresh.getByTestId("generate-provider")).toHaveCount(0);
    await expect(fresh.getByTestId("generate-provider-only")).toHaveCount(0);

    // One key, the owner's: it is used, with no choice and no key fields.
    await saveApiKey(page, "sk-ant-dev-one-key-1111");
    await createCourse(fresh);
    await expect(fresh.getByTestId("generate-provider")).toHaveCount(0);
    await expect(fresh.getByTestId("generate-provider-only")).toHaveValue("anthropic");
    await expect(fresh.getByTestId("generate-disabled")).toHaveCount(0);
    await expect(fresh.getByTestId("generate").getByTestId("ai-disclosure")).toHaveText("Your notes are sent to the AI provider using this server's key.");

    // Two keys: now it asks, and says where the notes go.
    await saveApiKey(page, "sk-dev-two-keys-2222", "openai");
    await createCourse(fresh);
    await expect(fresh.getByTestId("generate-provider")).toHaveCount(2);
    await expect(fresh.locator('[data-testid="generate-provider"][value="anthropic"]')).toBeChecked();
    await expect(fresh.getByTestId("generate").getByTestId("ai-disclosure")).toContainText("Claude");
    await fresh.locator('[data-testid="generate-provider"][value="openai"]').check();
    await expect(fresh.getByTestId("generate").getByTestId("ai-disclosure")).toContainText("OpenAI");
  } finally {
    await resetServerKeys(page);
    await ctx.close();
  }
});

test("the progress modal can't be dismissed while it runs; stopped, the course waits on the list with Resume", async ({ page }) => {
  const id = await createCourse(page);
  // [fixture:slow]: the outline takes 3 s.
  await page.getByTestId("generate-wish").fill("Fractions, slowly [fixture:slow]");
  await page.getByTestId("generate-submit").click();
  await expect(progress(page)).toHaveAttribute("data-state", "running");
  await expect(page.getByTestId("course-ai-modal-close")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(progress(page)).toBeVisible();
  // Coming back while it runs shows it again, from the course list's Watch too.
  await page.goto("/courses");
  await page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`).getByTestId("courses-watch").click();
  await expect(page).toHaveURL(new RegExp(`/courses/${id}$`));
  await expect(progress(page)).toHaveAttribute("data-state", "running");

  // Cancel stops it after the outline: no editor, the list opens the dialog to resume in.
  await page.getByTestId("course-ai-modal-cancel").click();
  expect(await generationEnds(page)).toBe("failed");
  await expect(page).toHaveURL(new RegExp(`/courses\\?generate=${id}$`));
  await expect(page.getByTestId("generate-submit")).toHaveText("Resume writing");
  await expect(page.getByTestId("generate-own")).toContainText("waits in your list");
  // Closed: the card has Resume and Delete, and no way into the editor.
  await page.getByTestId("generate-close").click();
  await expect(dialog(page)).toBeHidden();
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-card-stopped")).toBeVisible();
  await expect(card.getByTestId("courses-resume")).toBeVisible();
  await expect(card.getByTestId("courses-retry")).toHaveCount(0);
  await expect(card.getByTestId("courses-edit")).toHaveCount(0);
  await page.goto(`/courses/${id}`);
  await expect(page).toHaveURL(/\/courses$/);
});

test("a generation that fails before its outline: Retry on the card, the dialog filled in, and a retry writes the same course", async ({ page }) => {
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill("Fractions [fixture:provider-error]");
  await page.getByTestId("generate-notes").fill("Halves, quarters and eighths, with pictures of pizza.");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("failed");
  // Straight into the dialog on the list: the error, and what was asked.
  await expect(page.getByTestId("generate-error")).toContainText("didn't accept the server's API key");
  await expect(page.getByTestId("generate-error")).not.toContainText("Resume");
  await expect(page.getByTestId("generate-wish")).toHaveValue("Fractions [fixture:provider-error]");
  await expect(page.getByTestId("generate-notes")).toHaveValue("Halves, quarters and eighths, with pictures of pizza.");
  await expect(page.getByTestId("generate-submit")).toHaveText("Retry");
  await page.getByTestId("generate-close").click();

  // The card: a red ! for the error (the provider's text behind it), Retry and Delete; no Edit files, and no editor behind it.
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-card-stopped")).toHaveAttribute("data-tip", "An error occurred during generation.");
  await expect(card.getByTestId("courses-card-error")).toBeHidden();
  await card.getByTestId("courses-card-stopped").click();
  await expect(card.getByTestId("courses-card-error")).toContainText("HTTP 401");
  await card.getByTestId("courses-card-error-close").click();
  await expect(card.getByTestId("courses-card-error")).toBeHidden();
  await expect(card.getByTestId("courses-retry")).toBeVisible();
  await expect(card.getByTestId("courses-delete")).toBeVisible();
  await expect(card.getByTestId("courses-edit")).toHaveCount(0);
  await expect(card.getByTestId("courses-resume")).toHaveCount(0);
  await page.goto(`/courses/${id}`);
  await expect(page).toHaveURL(/\/courses$/);
  await page.goto(`/courses/${id}/file?path=course.json`);
  await expect(page).toHaveURL(/\/courses$/);

  // Retry: the dialog prefilled again; a working wish writes the same course.
  await card.getByTestId("courses-retry").click();
  await expect(page).toHaveURL(new RegExp(`/courses\\?generate=${id}$`));
  await expect(dialog(page)).toBeVisible();
  await expect(page.getByTestId("generate-wish")).toHaveValue("Fractions [fixture:provider-error]");
  await page.getByTestId("generate-wish").fill("Fractions for Friday");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?job=`));
  await expect(titles(page)).toHaveText(["Simplifying", "What a fraction is", "Adding fractions"]);
});

test("a stopped course can be deleted from its card", async ({ page }) => {
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill("Doomed [fixture:provider-error]");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("failed");
  await page.getByTestId("generate-close").click();
  page.once("dialog", (d) => d.accept());
  await page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`).getByTestId("courses-delete").click();
  await expect(page).toHaveURL(/\/courses\?notice=/);
  await expect(page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveCount(0);
});

test("a stopped course's red ! does not widen the course list on a phone (#675)", async ({ page }) => {
  const id = await createCourse(page);
  // Stopped after its outline, so the card carries the outline's title and the red ! after it.
  await page.getByTestId("generate-wish").fill("Fractions [fixture:slow]");
  await page.getByTestId("generate-submit").click();
  await expect(progress(page)).toHaveAttribute("data-state", "running");
  await page.getByTestId("course-ai-modal-cancel").click();
  expect(await generationEnds(page)).toBe("failed");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.getByTestId("courses-card-stopped")).toBeVisible();
  const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
  expect(sw).toBeLessThanOrEqual(cw);
});

test("a stopped course's red ! tip stays on a phone screen while it shows (#864)", async ({ page }) => {
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill("Fractions [fixture:slow]");
  await page.getByTestId("generate-submit").click();
  await expect(progress(page)).toHaveAttribute("data-state", "running");
  await page.getByTestId("course-ai-modal-cancel").click();
  expect(await generationEnds(page)).toBe("failed");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/courses");
  const bang = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`).getByTestId("courses-card-stopped");
  await bang.hover();
  const tip = await bang.evaluate((el) => {
    const s = getComputedStyle(el, "::after");
    return { display: s.display, content: s.content };
  });
  expect(tip.display).toBe("block");
  expect(tip.content).toContain("An error occurred during generation.");
  const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
  expect(sw).toBeLessThanOrEqual(cw);
});

test("the old status page leads to the editor", async ({ page }) => {
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill("Fractions via the old page");
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");
  const job = new URL(page.url()).searchParams.get("job");
  await page.goto(`/courses/generate/${job}`);
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?job=${job}$`));
  await expect(progress(page)).toHaveCount(0);
});

test("the dialog shows the lesson count, the time and the cost before generating (#443)", async ({ page }) => {
  await createCourse(page);
  const estimate = page.getByTestId("generate-estimate");
  const anthropic = page.locator('[data-testid="generate-provider"][value="anthropic"]');
  if (await anthropic.count()) await anthropic.check();
  // As the server drew it: a Full course counts 6 lessons, on the Anthropic key's model.
  await expect(estimate).toHaveText(/^6 lessons · about 5 minutes · roughly CA\$\d+\.\d\d$/);
  const first = await estimate.textContent();
  const dollars = async () => parseFloat(((await estimate.textContent()) ?? "").split("CA$")[1]);
  const six = await dollars();

  // A Preview is one lesson: less time and less money; back to Full, the script's sum is the server's.
  await page.locator('[data-testid="generate-depth"][value="preview"]').check();
  await expect(estimate).toHaveText(/^1 lesson · about 2 minutes · roughly CA\$\d+\.\d\d$/);
  expect(await dollars()).toBeLessThan(six);
  await page.locator('[data-testid="generate-depth"][value="full"]').check();
  await expect(estimate).toHaveText(first!);

  // Material goes with every request, so it costs more.
  await page.getByTestId("generate-notes").fill("Fractions: halves, quarters and eighths. ".repeat(400));
  await expect(async () => expect(await dollars()).toBeGreaterThan(six)).toPass();

  // A model with no price of its own: only the lessons and the time.
  const openai = page.locator('[data-testid="generate-provider"][value="openai"]');
  if (await openai.count()) {
    await openai.check();
    await expect(estimate).toHaveText("6 lessons · about 5 minutes");
  }

  // The (i) says how it is worked out; OK goes back to the dialog.
  await page.getByTestId("generate-estimate-info").click();
  await expect(page.getByTestId("generate-estimate-dialog")).toBeVisible();
  await expect(page.getByTestId("generate-estimate-dialog")).toContainText("not a promise");
  await expect(page.getByTestId("generate-estimate-dialog")).not.toContainText("{hosted}");
  await page.getByTestId("generate-estimate-close").click();
  await expect(page.getByTestId("generate-estimate-dialog")).toBeHidden();
  await expect(page.getByTestId("generate")).toBeVisible();
});

// #1255: the starter names are the app's words, so they come in the author's
// app language; the course still counts as a draft the AI can write into.
test("in German, a new course starts with German names and description", async ({ page }) => {
  await page.context().addCookies([{ name: "lang", value: "de", url: page.url() }]);
  const id = await createCourse(page);
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expect(page.getByTestId("course-edit-title")).toHaveText("Kurs ohne Titel");
  await page.getByTestId("generate-close").click();
  await expect(dialog(page)).toBeHidden();
  await expect(page.getByTestId("course-lessons-unit-title")).toHaveText(["Einheit 1"]);
  await expect(titles(page)).toHaveText(["Deine erste Lektion"]);
  await expect(page.getByTestId("course-ai-generate")).toBeVisible();

  await page.goto("/courses");
  const card = page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);
  await expect(card.locator(".course-body h3")).toContainText("Kurs ohne Titel");
  await expect(card.locator(".course-body")).toContainText("Beschreib den Kurs in ein oder zwei Sätzen.");
});
