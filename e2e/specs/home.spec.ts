// The course home page (#326, #558): today's goal as a ring with its chips
// and Continue, the unit row, the four modes (Sprint, Study, Play, Guide) and
// AI tutoring, each mode's explanation behind an (i), and the phone layout.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";

test("the home page has a map row and the Guide as a mode of its own", async ({ page }) => {
  await devLogin(page);
  await page.goto("/");

  const modes = page.getByTestId("home-mode");
  await expect(modes).toHaveCount(5);
  await expect(modes.evaluateAll((els) => els.map((e) => e.getAttribute("data-mode")))).resolves.toEqual(["sprint", "study", "play", "guide", "ai"]);

  // Hero: today's goal as a ring, the streak, lessons passed and terms as chips, and Continue.
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-goal", "20");
  await expect(page.getByTestId("home-stat-passed")).toContainText("lessons passed");
  await expect(page.getByTestId("home-stat-streak")).toContainText("day streak");
  await expect(page.getByTestId("home-continue")).toHaveAttribute("href", /\/(lessons|guide|sprint)\//);

  // The unit row names the current unit and how far through it you are.
  const map = page.getByTestId("home-map");
  await expect(map.getByTestId("home-map-unit")).toContainText(/Unit \d+ of \d+/);
  const progress = map.getByTestId("home-map-progress");
  const passed = Number(await progress.getAttribute("data-passed"));
  const total = Number(await progress.getAttribute("data-total"));
  expect(total).toBeGreaterThan(0);
  expect(passed).toBeLessThanOrEqual(total);
  // No "Open the learning map" link under the count (#1262).
  await expect(page.getByTestId("home-map-open")).toHaveCount(0);

  // The Guide card: stages complete, and its button opens the current stage.
  await page.goto("/");
  const guide = page.locator('[data-testid="home-mode"][data-mode="guide"]');
  const stat = guide.getByTestId("home-guide-stat");
  await expect(stat).toContainText("stages complete");
  await guide.getByTestId("home-start-guide").click();
  await expect(page).toHaveURL(/\/guide\/[^/]+$/);
  await page.goto("/guide");
  // The home page counts the same stages as the Guide.
  const completeOnGuide = await page.locator('[data-testid="guide-stage"][data-state="complete"]').count();
  await page.goto("/");
  await expect(stat).toHaveAttribute("data-completed", String(completeOnGuide));

  // The cards carry no list of links (#1263): heading, stat and one button each.
  await expect(page.locator(".mode-links")).toHaveCount(0);
  await expect(page.getByTestId("home-study-mistakes")).toHaveCount(0);
});

test("each mode's explanation sits behind its (i)", async ({ page }) => {
  await devLogin(page);
  await page.goto("/");
  for (const mode of ["goal", "sprint", "study", "play", "guide", "ai"]) {
    const dialog = page.getByTestId(`home-info-${mode}-dialog`);
    await expect(dialog).toBeHidden();
    await page.getByTestId(`home-info-${mode}`).click();
    await expect(dialog).toBeVisible();
    await dialog.getByTestId("home-info-close").click();
    await expect(dialog).toBeHidden();
  }
});

test("the home page fits a phone", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await devLogin(page);
  await page.goto("/");
  await expect(page.getByTestId("home-map")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});

test.describe("without JavaScript", () => {
  test.use({ javaScriptEnabled: false });
  test("the Guide card is a plain link", async ({ page }) => {
    await page.goto("/dev/login?open=spanish&next=/");
    await expect(page.getByTestId("home-map")).toBeVisible();
    await expect(page.getByTestId("home-start-guide")).toHaveAttribute("href", /\/guide/);
  });
});
