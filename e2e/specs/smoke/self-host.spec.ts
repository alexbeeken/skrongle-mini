// "Host it yourself" (#59): a visitor finds the guide from the docs, reads
// all eight steps on a phone without sideways scrolling, and can reach the
// technical version. No account, no key, no network.
//
import { expect, test } from "../../lib/fixtures.ts";


const STEPS = ["why", "need", "setup", "install", "yours", "away", "more"];

test("the self-hosting guide is linked from the docs and readable on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 740 });
  await page.goto("/docs");
  await page.getByTestId("docs-link").filter({ hasText: "Host it yourself" }).click();
  await expect(page).toHaveURL(/\/self-host$/);

  const steps = page.getByTestId("selfhost-step");
  await expect(steps).toHaveCount(STEPS.length);
  for (const [i, step] of STEPS.entries()) await expect(steps.nth(i)).toHaveAttribute("data-step", step);

  // Each thing to buy links to an official reseller or a Raspberry Pi reseller, never Amazon.
  const buy = page.getByTestId("selfhost-buy");
  await expect(buy).toHaveCount(4);
  for (const href of await buy.evaluateAll((as) => as.map((a) => (a as HTMLAnchorElement).href))) expect(href).not.toMatch(/amazon|amzn/);

  // Every command has its copy button, and the page itself never scrolls sideways
  // (long commands scroll inside their own block).
  const commands = page.getByTestId("selfhost-command");
  expect(await commands.count()).toBeGreaterThan(0);
  await expect(page.getByTestId("selfhost-copy")).toHaveCount(await commands.count());
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "horizontal overflow in px").toBeLessThanOrEqual(0);

  // Over plain http the clipboard is off limits, so Copy selects the command instead.
  await page.getByTestId("selfhost-copy").first().click();
  await expect(page.getByTestId("selfhost-copy").first()).toHaveText(/Copied|Selected/);

  await page.getByTestId("selfhost-technical").click();
  await expect(page).toHaveURL(/\/docs\/self-hosting$/);
  await expect(page.locator("h1")).toHaveText("Self-hosting");
});

test("the guide is only linked from the docs, not the landing page or the footer", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator('a[href$="/self-host"]')).toHaveCount(0);
});

test("the self-hosting FAQ is linked from the technical guide and points at the diagnostics", async ({ page }) => {
  await page.goto("/docs/self-hosting");
  await page.locator('[data-testid="docs-link"][data-slug="self-hosting-faq"]').first().click();
  await expect(page).toHaveURL(/\/docs\/self-hosting-faq$/);
  await expect(page.locator("h1")).toHaveText("Self-hosting FAQ");
  await expect(page.getByTestId("docs-body")).toContainText("skrongle doctor");

  await page.getByTestId("docs-link").filter({ hasText: "Diagnostics" }).click();
  await expect(page).toHaveURL(/\/docs\/self-hosting#diagnostics-skrongle-doctor$/);
  await expect(page.locator("#diagnostics-skrongle-doctor")).toBeInViewport();
});
