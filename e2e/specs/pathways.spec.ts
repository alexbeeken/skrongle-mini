// Exact specs for the pathways #36 left open: switching course, generating
// from notes and files, cancelling a generation, resuming a stopped one, and
// removing an API key. The simulated provider answers from the fixtures
// (data/test-fixtures/llm/, scenarios picked by a [fixture:NAME] token,
// docs/testing.md "Fixtures"), no model in the loop, so they run with the exact specs on every scripts/e2e.sh.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, generationEnds, openFromEditor } from "../lib/app";
import { newAccount, openCourse, register, saveApiKey } from "../lib/learner";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const modal = (page: Page) => page.getByTestId("course-ai-modal");
const card = (page: Page, id: string, section = "") =>
  page.locator(`${section ? `[data-testid="${section}"] ` : ""}[data-testid="courses-card"][data-course-id="${id}"]`);
const courseId = async (page: Page) => (await page.getByTestId("course-edit").getAttribute("data-course-id"))!;

/** A job as the API reports it (lessons, error and log): `jobId`, else the one the editor follows (`?job=`). */
async function job(page: Page, jobId?: string): Promise<{ lessons_done: number; error: string | null; log: string[] }> {
  const id = jobId ?? new URL(page.url()).searchParams.get("job");
  return (await page.request.get(`/api/generate/${id}`)).json();
}

/** Submit the open Generate dialog of course `id`; returns the job it started (from the redirect, which a stopped job skips past). */
async function submit(page: Page, id: string): Promise<string> {
  const [r] = await Promise.all([
    page.waitForResponse((r) => r.request().method() === "POST" && r.url().endsWith(`/courses/${id}/generate`)),
    page.getByTestId("generate-submit").click(),
  ]);
  return new URL(r.headers().location, page.url()).searchParams.get("job")!;
}

/** Create a course and generate it from `wish`. Returns the course id and the job. */
async function startGenerating(page: Page, wish: string): Promise<{ id: string; jobId: string }> {
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  const id = await createCourse(page);
  await page.getByTestId("generate-wish").fill(wish);
  return { id, jobId: await submit(page, id) };
}

test("switch course: Spanish, then a generated course, then back", async ({ page }) => {
  const { id } = await startGenerating(page, "Fractions to switch to");
  expect(await generationEnds(page)).toBe("done");

  // Still in Spanish: the header says so.
  await page.goto("/");
  const sw = page.getByTestId("nav-course-switch");
  await expect(sw).toHaveAttribute("data-course-id", "spanish");
  await expect(sw).toHaveAttribute("aria-label", "Switch course (now Spanish)");
  // It is a link to the course list, so it does not dress as a menu (#398).
  await expect(sw).toHaveText("Spanish");

  // The header link leads to the course list; open the new course there.
  await sw.click();
  await expect(page).toHaveURL(/\/courses$/);
  await expect(card(page, "spanish", "courses-projects")).toHaveAttribute("data-active", "true");
  await expect(card(page, id, "courses-mine")).toBeVisible();
  await openCourse(page, id); // its Start wizard (#587), with the settings it starts with
  await expect(sw).toHaveAttribute("data-course-id", id);
  await expect(sw).toHaveAttribute("aria-label", "Switch course (now Fractions)");
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);
  await sw.click();
  await expect(card(page, id, "courses-projects")).toHaveAttribute("data-active", "true");
  await expect(card(page, "spanish", "courses-projects")).toHaveAttribute("data-active", "false");
  await expect(page.locator('[data-testid="courses-projects"] [data-testid="courses-card"][data-active="true"]')).toHaveCount(1);

  // And back to Spanish from its project card: its lessons again.
  await card(page, "spanish", "courses-projects").getByTestId("courses-open").click();
  await expect(sw).toHaveAttribute("data-course-id", "spanish");
  await page.goto("/lessons?unit=all");
  await expect(page.locator('[data-testid="lessons-item"][data-lesson-id="ser-vs-estar"]')).toHaveCount(1);
  await sw.click();
  await expect(card(page, "spanish", "courses-projects")).toHaveAttribute("data-active", "true");
  await expect(card(page, id, "courses-projects")).toHaveAttribute("data-active", "false");
});

test("generate from notes and uploaded files, with no wish", async ({ page }) => {
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await createCourse(page);
  const notes = page.getByTestId("generate-notes");
  const count = page.locator("#gen-notes-count");

  // Too little to go on: refused on the form, what was typed is kept.
  await notes.fill("fractions");
  await page.getByTestId("generate-submit").click();
  await expect(page.getByTestId("generate-error")).toHaveText("Your prompt is too short! Please tell us what you'd like to learn or paste your notes so the AI has enough information to get to work.");
  await expect(notes).toHaveValue("fractions");

  const typed = "Week 3 homework: simplify 6/8 and 10/15.";
  await notes.fill(typed);
  await expect(count).toHaveText(`${typed.length} / 60,000 characters`);

  // Text files are read into the box after what was typed, each under its name.
  const homework = "1. Add 1/4 + 2/4.\n2. Which is bigger, 3/5 or 4/7?\n";
  const syllabus = "# Unit 2\n\nEquivalent fractions and common denominators.\n";
  await page.getByTestId("generate-files").setInputFiles([
    { name: "homework.txt", mimeType: "text/plain", buffer: Buffer.from(homework) },
    { name: "syllabus.md", mimeType: "text/markdown", buffer: Buffer.from(syllabus) },
  ]);
  const expected = `${typed}\n\n## homework.txt\n\n${homework.trim()}\n\n## syllabus.md\n\n${syllabus.trim()}`;
  await expect(notes).toHaveValue(expected);
  await expect(count).toHaveText(`${expected.length} / 60,000 characters`);

  // A file that isn't text is turned away and the box stays as it was.
  await page.getByTestId("generate-files").setInputFiles({ name: "scan.txt", mimeType: "text/plain", buffer: Buffer.from([0x25, 0x50, 0x00, 0x01, 0x02, 0x03]) });
  await expect(count).toHaveText(/does not look like a text file/);
  await expect(notes).toHaveValue(expected);

  // Notes alone are enough to start; the job says it builds from them.
  await page.getByTestId("generate-submit").click();
  expect(await generationEnds(page)).toBe("done");
  expect((await job(page)).log).toContain(`Building from your material (${expected.length} characters).`);
  const id = await courseId(page);
  const tab = await openFromEditor(page);
  await expect(tab.getByTestId("nav-course-switch")).toHaveAttribute("data-course-id", id);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);
});

test("cancel a slow generation, then resume it to a ready course", async ({ page }) => {
  // [fixture:slow]: the outline takes 3 s, time enough to press Cancel while it is asked for.
  const { id, jobId } = await startGenerating(page, "Fractions to cancel [fixture:slow]");
  await expect(modal(page)).toHaveAttribute("data-state", "running");
  await expect(modal(page)).toHaveAttribute("data-revision", "false");
  const cancel = page.getByTestId("course-ai-modal-cancel");
  await expect(cancel).toHaveText("Cancel");
  await cancel.click();
  // Back on the editor, whose modal follows the job until it stops.
  await expect(page).toHaveURL(new RegExp(`/courses/${id}\\?notice=.*&job=`));
  await expect(page.getByTestId("course-edit-notice")).toContainText("Cancelling");

  // The job stops after the request in flight: the outline is kept, no lesson is
  // written, and the course list opens the dialog to resume in.
  expect(await generationEnds(page)).toBe("failed");
  await expect(page.getByTestId("generate-error")).toContainText("Cancelled. The lessons written so far are kept");
  const stopped = await job(page, jobId);
  expect(stopped.lessons_done).toBe(0);
  expect(stopped.log.some((l) => l.startsWith("Cancel requested"))).toBe(true);
  await page.goto("/courses");
  await expect(card(page, id)).toHaveAttribute("data-course-status", "failed");
  await expect(card(page, id).getByTestId("courses-resume")).toBeVisible();
  await expect(card(page, id).getByTestId("courses-open")).toHaveCount(0);

  // Resume: the same wish, read-only; the resumed job writes all three lessons.
  await card(page, id).getByTestId("courses-resume").click();
  await expect(page.getByTestId("generate-wish-readonly")).toHaveValue("Fractions to cancel [fixture:slow]");
  await expect(page.getByTestId("generate-wish")).toHaveCount(0);
  await expect(page.getByTestId("generate-submit")).toHaveText("Resume writing");
  await submit(page, id);
  expect(await generationEnds(page)).toBe("done");
  expect(await courseId(page)).toBe(id);
  const resumed = await job(page);
  expect([resumed.error, resumed.lessons_done]).toEqual([null, 3]);
  expect(resumed.log.some((l) => /^Resuming .*: 0 of 3 lessons already written\.$/.test(l))).toBe(true);
  const tab = await openFromEditor(page);
  await expect(tab.getByTestId("nav-course-switch")).toHaveAttribute("data-course-id", id);
  await page.goto("/lessons?unit=all");
  await expect(page.getByTestId("lessons-item")).toHaveCount(3);
});

test("resume a run that stopped on a lesson: the resumed job keeps what was written", async ({ page }) => {
  // [fixture:validation-fails]: lesson 1 is saved, lesson 2's reply never validates.
  const { id, jobId } = await startGenerating(page, "Fractions to resume [fixture:validation-fails]");
  expect(await generationEnds(page)).toBe("failed");
  const first = await job(page, jobId);
  expect(first.lessons_done).toBe(1);
  expect(first.error).toMatch(/lesson 2 did not validate/);
  const lesson1 = async () => (await (await page.request.get(`/api/courses/${id}/file?path=lessons/what-is-a-fraction.json`)).json()).content as string;
  const written = await lesson1();
  expect(written).toContain('"slug": "what-is-a-fraction"');

  // Straight into the dialog on the list, to resume in.
  await expect(page.getByTestId("generate")).toBeVisible();
  await expect(page.getByTestId("generate-wish-readonly")).toHaveValue("Fractions to resume [fixture:validation-fails]");
  const again = await submit(page, id);
  expect(again).not.toBe(jobId);
  expect(await generationEnds(page)).toBe("failed");
  // A resumed job: it starts from the saved outline and the lesson already written,
  // asks only for the missing ones, and lesson 1 stays byte for byte. The fixture's
  // lesson 2 never validates, so it stops there again, still resumable.
  const resumed = await job(page, again);
  expect(resumed.log.some((l) => /^Resuming .*: 1 of 3 lessons already written\.$/.test(l))).toBe(true);
  expect(resumed.log.some((l) => l.startsWith("Outline ready"))).toBe(false);
  expect(resumed.lessons_done).toBe(1);
  expect(resumed.error).toMatch(/lesson 2 did not validate/);
  await expect(page.getByTestId("generate-submit")).toHaveText("Resume writing");
  expect(await lesson1()).toBe(written);
  // The half-written course stays out of sight: no editor, Resume on its card.
  await page.goto(`/courses/${id}`);
  await expect(page).toHaveURL(/\/courses$/);
  await expect(card(page, id).getByTestId("courses-resume")).toBeVisible();
});

test("saving into a stopped run keeps it resumable, and Resume keeps the edit (#81)", async ({ page }) => {
  // [fixture:validation-fails]: lesson 1 is saved, lesson 2's reply never validates.
  const { id } = await startGenerating(page, "Fractions edited while stopped [fixture:validation-fails]");
  expect(await generationEnds(page)).toBe("failed");
  const reason = await page.getByTestId("generate-error").innerText();
  const path = "lessons/what-is-a-fraction.json";
  const lesson1 = async () => JSON.parse((await (await page.request.get(`/api/courses/${id}/file?path=${path}`)).json()).content);
  const lesson = await lesson1();
  // Every way a save can still reach a stopped run (a stale editor tab, the
  // file form, the upload API, Undo) used to make it "ready" and drop Resume.
  lesson.title = "Edited while stopped";
  const form = await page.request.post(`/courses/${id}/file`, { form: { path, content: JSON.stringify(lesson, null, 2) }, maxRedirects: 0 });
  expect(form.status()).toBe(303);
  lesson.summary = "Uploaded while stopped.";
  const up = await (await page.request.post(`/api/courses/${id}/files`, { data: { files: [{ path, content: JSON.stringify(lesson, null, 2) }] } })).json();
  expect(up.saved).toEqual([path]);
  expect(up.error).toBeNull();
  await page.request.post(`/courses/${id}/publish`, { maxRedirects: 0 });
  await page.goto("/courses");
  await expect(card(page, id).getByTestId("courses-resume")).toBeVisible();
  // The card: a red ! with the reason behind it, and nothing else about the error.
  await expect(card(page, id).getByTestId("courses-card-stopped")).toHaveAttribute("data-tip", "An error occurred during generation.");
  await card(page, id).getByTestId("courses-card-stopped").click();
  await expect(card(page, id).getByTestId("courses-card-error")).toBeVisible();
  await card(page, id).getByTestId("courses-card-error-close").click();
  // Still stopped for the same reason, so Resume opens the resume dialog, not a blank form.
  await card(page, id).getByTestId("courses-resume").click();
  await expect(page.getByTestId("generate-submit")).toHaveText("Resume writing");
  await expect(page.getByTestId("generate-wish-readonly")).toHaveValue("Fractions edited while stopped [fixture:validation-fails]");
  await expect(page.getByTestId("generate-error")).toHaveText(reason);
  // Resume starts from the edited lesson and keeps it.
  const again = await submit(page, id);
  expect(await generationEnds(page)).toBe("failed");
  expect((await job(page, again)).log.some((l) => /^Resuming .*: 1 of 3 lessons already written\.$/.test(l))).toBe(true);
  expect(await lesson1()).toMatchObject({ title: "Edited while stopped", summary: "Uploaded while stopped." });
  await page.goto("/courses");
  await expect(card(page, id).getByTestId("courses-resume")).toBeVisible();
});

test("the editor opened between the outline and lesson 1 still follows the job to its end", async ({ page }) => {
  // [fixture:slow-lessons]: the outline comes at once, its first lesson takes 3 s. The
  // table then has units but no lesson row, which once broke courses.js, so the modal never polled.
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const { jobId } = await startGenerating(page, "Fractions, slowly [fixture:slow-lessons]");
  await expect.poll(async () => (await job(page, jobId)).log.some((l) => l.startsWith("Outline ready"))).toBe(true);
  await page.reload();
  await expect(page.getByTestId("course-lessons-unit")).toHaveCount(2);
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(0);
  await expect(modal(page)).toHaveAttribute("data-state", "running");
  expect(await generationEnds(page)).toBe("done");
  await expect(page.getByTestId("course-lessons-row")).toHaveCount(3);
  expect(errors).toEqual([]);
});

test("remove an API key: the profile says no key, and generating no longer offers it", async ({ page }) => {
  // A fresh account, so the dev user's keys (keys=fake) stay for the other specs.
  await register(page, newAccount("keys"), "spanish");
  await page.goto("/profile");
  const row = page.locator('[data-testid="profile-api-key"][data-provider="anthropic"]');
  await expect(row).toHaveAttribute("data-key-set", "false");
  await expect(row.getByTestId("profile-api-key-remove")).toHaveCount(0);
  await saveApiKey(page, "sk-ant-test-remove-1234");
  await expect(row).toContainText("1234");
  await expect(row.getByTestId("profile-api-key-input")).toHaveValue(""); // never shown again
  // With one key saved the dialog uses it, with no choice shown and no key asked for.
  await createCourse(page);
  await expect(page.getByTestId("generate-provider-only")).toHaveValue("anthropic");
  await expect(page.getByTestId("generate-provider")).toHaveCount(0);
  await expect(page.getByTestId("generate-api-key")).toHaveCount(0);

  await page.goto("/profile");
  await row.getByTestId("profile-api-key-toggle").click();
  await row.getByTestId("profile-api-key-remove").click();
  await expect(page.getByTestId("profile-notice")).toHaveText("✓ Claude (Anthropic) key removed.");
  await expect(row).toHaveAttribute("data-key-set", "false");
  await expect(row.getByTestId("profile-api-key-remove")).toHaveCount(0);
  await expect(row).not.toContainText("1234");
  await expect(row.getByTestId("profile-api-key-status")).toHaveText("No key yet");
  await page.reload();
  await expect(row).toHaveAttribute("data-key-set", "false");
  // No key left: the dialog is greyed out under a link to the profile (no stand-in, even in dev mode).
  await createCourse(page);
  await expect(page.getByTestId("generate-no-key")).toBeVisible();
  await expect(page.getByTestId("generate-provider-only")).toHaveCount(0);
  await expect(page.getByTestId("generate-api-key")).toHaveCount(0);
});
