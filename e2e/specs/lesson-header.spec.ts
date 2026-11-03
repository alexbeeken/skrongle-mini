// Exact spec for the quiet lesson header (#999): on a phone the lesson text
// starts high on the first screen, with one main button, the other ways in
// under "More ways" and the progress details behind an (i).
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";

test.beforeEach(async ({ page }) => {
  await devLogin(page);
});

test("on a phone the lesson text starts under 420 px down a 390x844 screen: one main button, the rest under More ways, the details behind an (i) (#999)", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const slug = "regular-ar-present-tense";
  await page.goto(`/lessons/${slug}`);
  // On main (before #999) the text started 682 px down; the quiet header puts it at 374 px (400 once a Read pill wraps the status to its own line; 420 leaves room).
  const top = await page.getByTestId("lesson-content").evaluate((e) => e.getBoundingClientRect().top);
  expect(top).toBeLessThan(420);

  // One main button; the other ways in sit in the menu, closed until asked.
  const actions = page.getByTestId("lesson-actions");
  await expect(actions.locator(":scope > .btn, :scope > form .btn")).toHaveCount(1);
  await expect(page.getByTestId("lesson-tutor")).toBeHidden();
  await page.getByTestId("lesson-more-toggle").click();
  const menu = page.getByTestId("lesson-more");
  await expect(menu.getByTestId("lesson-listen")).toHaveAttribute("href", `/listening/play?lesson=${slug}`);
  await expect(menu.getByTestId("lesson-tutor")).toHaveAttribute("href", `/tutor?lesson=${slug}`);
  await expect(menu.getByTestId("lesson-guide")).toHaveAttribute("href", `/guide/${slug}`);
  await expect(menu.getByTestId("lesson-sprint")).toHaveAttribute("href", `/sprint/play?lesson=${slug}`);
  // The open menu stays on the screen: no sideways scroll on a phone.
  const box = await menu.locator(".menu").boundingBox();
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.getByTestId("lesson-more-toggle").click();

  // Mastery details and the pass rule open from the (i), not on the page.
  await expect(page.getByTestId("lesson-pass-rule")).toBeHidden();
  await page.getByTestId("lesson-info").click();
  const info = page.getByTestId("lesson-info-dialog");
  await expect(info).toBeVisible();
  await expect(page.getByTestId("lesson-pass-rule")).not.toBeEmpty();
  await page.getByTestId("lesson-info-close").click();
  await expect(info).toBeHidden();
  // The footer's line of links is gone; the footer keeps its button and the same (i).
  await expect(page.locator('.lesson-footer a[href*="/guide/"], .lesson-footer a[href*="/sprint/"]')).toHaveCount(0);
  await expect(page.getByTestId("lesson-pass-info")).toBeVisible();
});
