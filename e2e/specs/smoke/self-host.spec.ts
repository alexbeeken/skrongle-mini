// "Host it yourself" (#59): a visitor finds the guide from the docs, reads
// every step on a phone without sideways scrolling, and can reach the
// technical version. No account, no key, no network.
//
import { expect, test } from "../../lib/fixtures.ts";


const STEPS = ["why", "need", "setup", "yours", "away", "more"];

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

  // The page itself never scrolls sideways.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow, "horizontal overflow in px").toBeLessThanOrEqual(0);

  // The technical link is for experts and AI, and goes to the reference on GitHub.
  await expect(page.getByTestId("selfhost-technical")).toHaveAttribute(
    "href",
    "https://github.com/alexbeeken/skrongle-mini/blob/main/docs/self-hosting.md",
  );

  // Start the guide leads to the step-by-step guide, and it renders.
  await page.getByTestId("selfhost-start").click();
  await expect(page).toHaveURL(/\/docs\/setup-home-wifi$/);
  await expect(page.locator("h1")).toHaveText("Set up Skrongle on a new Raspberry Pi (home Wi-Fi only)");

  // Away from home links to the Tailscale guide.
  await page.goto("/self-host");
  await page.locator("#away a").first().click();
  await expect(page).toHaveURL(/\/docs\/setup-tailscale$/);
  await expect(page.locator("h1")).toHaveText("Away from home with Tailscale");
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
