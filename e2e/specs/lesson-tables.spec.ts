// Exact spec for #465: on a phone, a wide table on a lesson page (the
// paradigm table, a Markdown table in the lesson text, the vocabulary) scrolls
// inside its own box; the page itself never gets wider than the screen.
import { expect, test, type Page } from "@playwright/test";
import { devLogin, TEST_COURSE } from "../lib/app";

const PHONE = { width: 390, height: 844 };

async function fitsThePhone(page: Page, slug: string): Promise<void> {
  await page.goto(`/lessons/${slug}`);
  await expect(page.getByTestId("lesson-content")).toBeVisible();
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width, `/lessons/${slug} is ${width}px wide`).toBeLessThanOrEqual(PHONE.width);
}

test("the test course's lessons with tables fit a phone", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page, "open=" + TEST_COURSE);
  for (const slug of ["everything", "conjugations", "legacy"]) await fitsThePhone(page, slug);
  // The tables sit in their scroll boxes.
  await page.goto("/lessons/everything");
  await expect(page.getByTestId("lesson-content").getByTestId("table-scroll").first()).toBeVisible();
  await expect(page.getByTestId("lesson-vocabulary-scroll")).toBeVisible();
  await expect(page.getByTestId("lesson-conjugations-scroll")).toBeVisible();
});

test("Spanish paradigm tables scroll in their box, not the page", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await devLogin(page);
  for (const slug of ["ser-vs-estar", "proofreading", "regular-preterite-er-ir", "imperfect", "present-subjunctive-forms"]) {
    await fitsThePhone(page, slug);
  }
  // The paradigm table is wider than the phone, so its box scrolls.
  const box = page.getByTestId("lesson-conjugations-scroll");
  expect(await box.evaluate((e) => e.scrollWidth > e.clientWidth)).toBe(true);
});
