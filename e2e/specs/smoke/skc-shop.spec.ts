// The Skrongle Cash shop (#569): badges bought with a course's SKC, shown
// on that course's SKC hero only (src/motivators/shop.rs). The balance is
// earned less spent; a badge is bought once.
import type { Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { AnswerKey } from "../../lib/key.ts";
import { playSprintRight } from "../../lib/sprint-play.ts";
import { expect, test } from "../../lib/fixtures.ts";
import { apiProgress, newAccount, openCourse, pickMotivator, register, wearMotivator } from "../../lib/learner.ts";

const COURSE = "spanish";
const key = new AnswerKey(COURSE);
const catalogue: { id: string; price: number }[] = JSON.parse(readFileSync(new URL("../../../data/badges/catalogue.json", import.meta.url), "utf8"));
const cheap = catalogue[0];
const dear = catalogue[catalogue.length - 1];

/** The SKC balance on the course home page. */
async function balance(page: Page): Promise<number> {
  await page.goto("/");
  await expect(page.getByTestId("home-today")).toHaveAttribute("data-motivator", "skc");
  return Number(await page.getByTestId("skc-balance").getAttribute("data-skc"));
}

/** The badge on the hero's shelf, after loading the home page. */
async function heroBadge(page: Page, id: string) {
  await page.goto("/");
  return page.getByTestId("skc-shelf").locator(`[data-testid="skc-badge"][data-badge="${id}"]`);
}

test("the shop sells a badge once and keeps it across motivators", async ({ page }) => {
  await register(page, newAccount("shop"), COURSE);
  await pickMotivator(page, COURSE, "skc");

  // The (i) explains the shop; the hero's Shop button opens it.
  await page.goto("/");
  await page.getByTestId("skc-info").click();
  await expect(page.getByTestId("skc-info-dialog").getByTestId("skc-info-shop")).toContainText("shop");
  await page.getByTestId("skc-info-close").click();
  await expect(page.getByTestId("skc-shelf")).toHaveCount(0);
  await page.getByTestId("skc-shop").click();
  await expect(page).toHaveURL(/\/shop$/);
  await expect(page.getByTestId("shop-badge")).toHaveCount(catalogue.length);
  await expect(page.getByTestId("shop-shelf-empty")).toBeVisible();

  // Earn SKC by passing lessons in Sprint until the cheapest badge is affordable.
  const lessons = (await apiProgress(page)).filter((l) => l.required_answers >= 2);
  let have = 0;
  for (const l of lessons) {
    await page.goto(`/sprint/play?lesson=${l.slug}&reduce-motion=1`);
    await playSprintRight(page, key);
    await expect(page.getByTestId("sprint-play-done")).toHaveAttribute("data-saved", "true");
    have = await balance(page);
    if (have >= cheap.price) break;
  }
  expect(have).toBeGreaterThanOrEqual(cheap.price);
  expect(have).toBeLessThan(dear.price);

  // Too dear: refused in a modal, nothing charged.
  await page.goto("/shop");
  await expect(page.getByTestId("shop-balance")).toHaveAttribute("data-skc", String(have));
  await page.locator(`[data-testid="shop-badge"][data-badge="${dear.id}"]`).getByTestId("shop-buy").click();
  const refusal = page.getByTestId("shop-refusal");
  await expect(refusal).toBeVisible();
  await expect(refusal).toContainText(`${dear.price - have} more SKC`);
  await page.getByTestId("shop-refusal-close").click();
  await expect(refusal).toBeHidden();
  expect(await balance(page)).toBe(have);

  // Bought: the balance drops by the price; the badge is on the shelf and the hero.
  await page.goto("/shop");
  const card = page.locator(`[data-testid="shop-badge"][data-badge="${cheap.id}"]`);
  await card.getByTestId("shop-buy").click();
  await expect(card).toHaveAttribute("data-owned", "true");
  await expect(card.getByTestId("shop-buy")).toHaveCount(0);
  await expect(page.getByTestId("shop-shelf").locator(`[data-badge="${cheap.id}"]`)).toBeVisible();
  await expect(page.getByTestId("shop-balance")).toHaveAttribute("data-skc", String(have - cheap.price));
  expect(await balance(page)).toBe(have - cheap.price);
  await expect(await heroBadge(page, cheap.id)).toBeVisible();

  // Twice: refused, nothing charged.
  const again = await page.request.post("/api/shop/buy", { form: { item: cheap.id }, maxRedirects: 0 });
  expect(again.headers()["location"]).toContain("refused=owned");
  expect(await balance(page)).toBe(have - cheap.price);

  // Stats, then SKC again: the badge and the balance are as they were.
  await wearMotivator(page, COURSE, "stats");
  await page.goto("/");
  await expect(page.getByTestId("home-today")).not.toHaveAttribute("data-motivator", "skc");
  expect((await page.request.get("/shop", { maxRedirects: 0 })).status(), "no shop without SKC").toBe(303);
  await wearMotivator(page, COURSE, "skc");
  expect(await balance(page)).toBe(have - cheap.price);
  await expect(await heroBadge(page, cheap.id)).toBeVisible();

  // Another course wearing SKC: its own balance, no badge.
  await openCourse(page, "french");
  await wearMotivator(page, "french", "skc");
  expect(await balance(page)).toBe(0);
  await expect(page.getByTestId("skc-shelf")).toHaveCount(0);
});

// Without JavaScript (#926) the refusal is drawn open, so the page still
// says why the buy failed; with script it is the modal tested above.
test("without JavaScript a refused buy says why", async ({ page, browser }) => {
  await register(page, newAccount("shop-nojs"), COURSE);
  await pickMotivator(page, COURSE, "skc");
  const ctx = await browser.newContext({ javaScriptEnabled: false, storageState: await page.context().storageState() });
  const plain = await ctx.newPage();
  await plain.goto("/shop");
  const have = Number(await plain.getByTestId("shop-balance").getAttribute("data-skc"));
  expect(have).toBeLessThan(dear.price);
  await plain.locator(`[data-testid="shop-badge"][data-badge="${dear.id}"]`).getByTestId("shop-buy").click();
  await expect(plain).toHaveURL(/\/shop\?refused=short/);
  const refusal = plain.getByTestId("shop-refusal");
  await expect(refusal).toBeVisible();
  await expect(refusal).toContainText(`${dear.price - have} more SKC`);
  // In the page's flow above the heading, not drawn over it.
  const box = (await refusal.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual((await plain.locator("h1").first().boundingBox())!.y);
  await ctx.close();
});
