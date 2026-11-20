// Exact spec for #236, Paper mode: a lesson or a unit printed as a worksheet.
// More ways → Print opens the sheet; printed, it is black on white even in a
// dark theme, the navigation and the tools are left out, each question keeps
// together and the answer key starts a new page. A lesson whose questions all
// need the screen (the test course's clock lesson) has no Print.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, TEST_COURSE } from "../lib/app";

/** The number of pages of a PDF Chromium made (its page objects). */
function pdfPages(pdf: Buffer): number {
  return (pdf.toString("latin1").match(/\/Type\s*\/Page[^s]/g) ?? []).length;
}

async function printed(page: Page) {
  await page.emulateMedia({ media: "print", colorScheme: "dark" });
}

test("Print a lesson: the sheet, its key on a new page, black on white", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons/everything");
  await page.getByTestId("lesson-more-toggle").click();
  await page.getByTestId("lesson-print").click();
  await expect(page).toHaveURL(/\/lessons\/everything\/print$/);

  const sheet = page.getByTestId("paper");
  await expect(sheet.getByTestId("paper-lesson")).toHaveCount(1);
  await expect(sheet.getByTestId("paper-vocabulary")).toBeVisible();
  await expect(sheet.getByTestId("paper-name-date")).toBeVisible();
  const questions = sheet.getByTestId("paper-question");
  const count = await questions.count();
  expect(count).toBeGreaterThan(0);
  await expect(sheet.getByTestId("paper-answer")).toHaveCount(count);
  await expect(page.getByTestId("paper-not-recorded")).toBeVisible();
  await expect(page.getByTestId("paper-print")).toBeVisible();

  // The same order on a second print.
  const order = await questions.evaluateAll((qs) => qs.map((q) => q.getAttribute("data-question-id")));
  await page.reload();
  expect(await questions.evaluateAll((qs) => qs.map((q) => q.getAttribute("data-question-id")))).toEqual(order);

  // Printed, in a dark theme.
  await printed(page);
  await expect(page.locator(".topbar")).toBeHidden();
  await expect(page.getByTestId("paper-tools")).toBeHidden();
  await expect(page.getByTestId("paper-print")).toBeHidden();
  await expect(page.getByTestId("paper-not-recorded")).toBeHidden();
  const look = await page.evaluate(() => {
    const css = (sel: string) => getComputedStyle(document.querySelector(sel)!);
    return {
      body: css("body").backgroundColor,
      sheet: css('[data-testid="paper"]').backgroundColor,
      text: css('[data-testid="paper-question"]').color,
      key: css('[data-testid="paper-key"]').breakBefore,
      question: css('[data-testid="paper-question"]').breakInside,
    };
  });
  expect(look).toEqual({ body: "rgb(255, 255, 255)", sheet: "rgb(255, 255, 255)", text: "rgb(0, 0, 0)", key: "page", question: "avoid" });

  // A4 and Letter both lay out, the key on a page of its own.
  for (const format of ["A4", "Letter"] as const) {
    const pdf = await page.pdf({ format });
    expect(pdfPages(pdf), format).toBeGreaterThanOrEqual(2);
  }
});

test("Print the unit: every lesson, one key at the end; no Print where nothing fits paper", async ({ page }) => {
  await devLogin(page, "open=" + TEST_COURSE);
  await page.goto("/lessons?unit=questions");
  await page.getByTestId("lessons-print-unit").click();
  await expect(page).toHaveURL(/\/lessons\/print\?unit=questions$/);
  const lessons = page.getByTestId("paper-lesson");
  expect(await lessons.count()).toBeGreaterThan(1);
  await expect(page.getByTestId("paper-key")).toHaveCount(1);
  await expect(page.locator('[data-testid="paper-lesson"][data-lesson-id="clock"] [data-testid="paper-question"]')).toHaveCount(0);
  await expect(page.getByTestId("paper-question")).toHaveCount(await page.getByTestId("paper-answer").count());

  // The clock lesson: every question needs the screen, so no Print.
  await page.goto("/lessons/clock");
  await page.getByTestId("lesson-more-toggle").click();
  await expect(page.getByTestId("lesson-sprint")).toBeVisible();
  await expect(page.getByTestId("lesson-print")).toHaveCount(0);
});
