// Exact spec for Use AI's "What can the AI do?" (#1236): the (i) by the box
// opens a dialog of example requests; a click adds one to the box (a second
// one goes on a new line) and closes the dialog. The listening and
// conversation examples are only there for a language course. No model call.
import { expect, test, type Page } from "@playwright/test";
import { aiMode, copyCourse, createCourse, devLogin, devReset, TEST_COURSE } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page, `keys=fake&open=${TEST_COURSE}&next=/courses`);
});
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

async function openExamples(page: Page) {
  await page.getByTestId("course-ai-examples-info").click();
  await expect(page.getByTestId("course-ai-examples")).toBeVisible();
}

test("a click on an example adds it to the box; a second goes on a new line", async ({ page }) => {
  await copyCourse(page, TEST_COURSE);
  const box = page.getByTestId("course-ai-text");
  await expect(page.getByTestId("course-ai-text-label")).toHaveText("What should the AI add?");
  await expect(box).toHaveValue("");
  await openExamples(page);
  await expect(page.getByTestId("course-ai-examples-intro")).toContainText("Add and edit");
  await expect(page.getByTestId("course-ai-examples-translate")).toContainText("Copy and translate to");
  // A language course has the listening and conversation examples.
  await expect(page.getByTestId("course-ai-example-add-listening")).toBeVisible();
  await expect(page.getByTestId("course-ai-example-add-conversation")).toBeVisible();

  await page.getByTestId("course-ai-example-add-questions").click();
  await expect(page.getByTestId("course-ai-examples")).toBeHidden();
  await expect(box).toHaveValue("Add three multiple-choice questions to lesson 4.");
  await expect(box).toBeFocused();

  await openExamples(page);
  await page.getByTestId("course-ai-example-simpler").click();
  await expect(box).toHaveValue("Add three multiple-choice questions to lesson 4.\nUse simpler words in the explanations.");
  // The card does not touch the switch.
  await expect(aiMode(page, "change")).not.toBeChecked();
  // The cursor is at the end of the box.
  expect(await box.evaluate((el: HTMLTextAreaElement) => el.selectionStart === el.value.length)).toBe(true);
});

test("a course that teaches no language has no listening or conversation example", async ({ page }) => {
  await createCourse(page);
  await page.getByTestId("generate-close").click();
  await openExamples(page);
  await expect(page.getByTestId("course-ai-example-add-sort")).toBeVisible();
  await expect(page.getByTestId("course-ai-example-add-listening")).toHaveCount(0);
  await expect(page.getByTestId("course-ai-example-add-conversation")).toHaveCount(0);
  await page.getByTestId("course-ai-examples-close").click();
  await expect(page.getByTestId("course-ai-examples")).toBeHidden();
});

test("at phone width the cards stack and nothing scrolls sideways", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await copyCourse(page, TEST_COURSE);
  await openExamples(page);
  const a = (await page.getByTestId("course-ai-example-check-facts").boundingBox())!;
  const b = (await page.getByTestId("course-ai-example-add-lesson").boundingBox())!;
  expect(b.y).toBeGreaterThan(a.y + a.height - 1);
  const dialog = page.getByTestId("course-ai-examples");
  expect(await dialog.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
});
