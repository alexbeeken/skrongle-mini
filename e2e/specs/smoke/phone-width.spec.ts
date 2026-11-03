// Phone width (#201): no main page scrolls sideways at 390 px, as the seeded
// dev user. Exact: no model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { createCourse, devLogin } from "../../lib/app.ts";
import { apiProgress } from "../../lib/learner.ts";

test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });

/** The page is no wider than the screen. (With isMobile the layout viewport grows to the content, so compare with 390, not innerWidth.) */
async function expectNoSidewaysScroll(page: import("@playwright/test").Page, path: string) {
  await page.goto(path);
  await page.waitForLoadState("load");
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width, `${path} is ${width} px wide`).toBe(390);
}

test("the main pages and a lesson in each mode fit a 390 px phone", async ({ page }) => {
  test.setTimeout(120_000);
  await devLogin(page);
  const [first] = await apiProgress(page);
  for (const path of [
    "/sprint?reduce-motion=1",
    `/sprint/play?lesson=${first.slug}&reduce-motion=1`,
    "/lessons",
    `/lessons/${first.slug}`,
    `/practice?lesson=${first.slug}`,
    "/map",
    "/guide",
    `/guide/${first.slug}`,
    "/play",
    "/courses",
    "/profile",
  ]) {
    await expectNoSidewaysScroll(page, path);
  }
});

test("the course editor fits a 390 px phone", async ({ page }) => {
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  const id = await createCourse(page);
  await expectNoSidewaysScroll(page, `/courses/${id}`);
});
