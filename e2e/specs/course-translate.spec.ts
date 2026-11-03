// Copy and translate a course (#630): the "Copy and translate to…" icon left
// of the copy icon in a course's settings dialog behind the card's gear (not
// in the Start wizard of a course not opened yet) opens its own dialog (the language, the AI, Start; what gets
// translated behind its (i)). The app copies the course and an AI job
// translates the copy file by file, followed in the editor's progress
// modal like a generation. The simulated provider answers from
// data/test-fixtures/llm/translate-course.json, which translates the
// starter course of "Create a course" into Spanish.
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, generationEnds } from "../lib/app";
import { openCourse } from "../lib/learner";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

const card = (page: Page, id: string) => page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`);

/** Opens the course's settings dialog behind the card's gear, opening the course first if it has none yet
 * (its Start wizard has no editor, copy or translate icons). */
async function openSettings(page: Page, id: string) {
  if (!(await card(page, id).getByTestId("courses-settings").count())) {
    await openCourse(page, id);
    await page.goto("/courses");
  }
  const c = card(page, id);
  await c.getByTestId("courses-settings").click();
  const dialog = c.getByTestId("courses-settings-dialog");
  await expect(dialog).toBeVisible();
  return dialog;
}

async function lessonFile(page: Page, course: string): Promise<string> {
  const r = await page.request.get(`/api/courses/${course}/file?path=lessons/first-lesson.json`);
  expect(r.status()).toBe(200);
  return ((await r.json()) as { content: string }).content;
}

test("Copy and translate to Español makes a translated copy; the original stays as it was", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/courses");
  const original = await createCourse(page);
  await page.getByTestId("generate-close").click();
  await page.goto("/courses");

  // Not in the Start wizard of the course, which has not been opened yet.
  await card(page, original).getByTestId("courses-open").click();
  const wizard = card(page, original).locator('[data-testid="courses-settings-dialog"][data-start]');
  await expect(wizard).toBeVisible();
  await expect(wizard.getByTestId("courses-translate")).toHaveCount(0);
  await wizard.getByTestId("courses-settings-close").click();

  // The dialog, from the icon in the course's settings behind the gear: what gets translated is behind the (i),
  // a card as wide as the dialog's title, not the dot; the course's own language is offered like any other.
  const settings = await openSettings(page, original);
  const icon = settings.getByTestId("courses-translate");
  await expect(icon).toHaveAttribute("data-tip", "Copy and translate to…");
  await icon.click();
  const dialog = card(page, original).getByTestId("courses-translate-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("courses-translate-info-text")).toBeHidden();
  await dialog.getByTestId("courses-translate-info").locator("summary").click();
  await expect(dialog.getByTestId("courses-translate-info-text")).toContainText("The language the course teaches stays as it is");
  const tip = await dialog.getByTestId("courses-translate-info-text").boundingBox();
  const heading = await dialog.locator("h2").boundingBox();
  expect(tip!.width).toBeGreaterThan(heading!.width * 0.9);
  // Closed again, so the card does not cover the form.
  await dialog.getByTestId("courses-translate-info").locator("summary").click();
  await dialog.getByTestId("courses-translate-lang").selectOption("es");
  await dialog.getByTestId("courses-translate-start").click();

  // The editor follows the job to its end, then shows the copy.
  const done = await generationEnds(page);
  expect(done).toBe("done");
  const id = new URL(page.url()).pathname.split("/").pop() ?? "";
  expect(id).not.toBe(original);
  await expect(page.getByTestId("course-edit-title")).toHaveText("Untitled course (Español)");
  await expect(page.getByTestId("course-edit")).toHaveAttribute("data-loads", "true");
  await expect(page.getByTestId("course-lessons-title").first()).toContainText("Tu primera lección");

  // The learner's side is in Spanish, the ids are as they were; the original is untouched.
  const copy = JSON.parse(await lessonFile(page, id));
  expect(copy.id).toBe("u1-first-lesson");
  expect(copy.quiz[0].prompt).toBe("¿Qué campo guarda lo que significa un término?");
  expect(copy.quiz[0].options).toEqual(["significado", "ejemplo", "slug"]);
  expect(copy.quiz[0].correct_answer).toBe("significado");
  expect(JSON.parse(await lessonFile(page, original)).title).toBe("Your first lesson");

  // Both on the list.
  await page.goto("/courses");
  await expect(card(page, id).locator(".course-body h3")).toContainText("Untitled course (Español)");
  await expect(card(page, original).locator(".course-body h3")).toContainText("Untitled course");
  await expect(card(page, original).locator(".course-body h3")).not.toContainText("Español");
});

test("the dialog does not offer the language a course is already explained in", async ({ page }) => {
  await devLogin(page, "keys=fake&next=/courses");
  await openSettings(page, "spanish");
  const languages = card(page, "spanish").getByTestId("courses-translate-lang");
  await expect(languages.locator('option[value="en"]')).toHaveCount(0);
  await expect(languages.locator('option[value="es"]')).toHaveCount(1);
  await expect(languages.locator('option[value="fr"]')).toHaveCount(1);
  // Closing the dialog starts nothing.
  await card(page, "spanish").getByTestId("courses-translate").click();
  await card(page, "spanish").getByTestId("courses-translate-close").click();
  await expect(card(page, "spanish").getByTestId("courses-translate-dialog")).toBeHidden();
  await expect(page).toHaveURL(/\/courses$/);
});
