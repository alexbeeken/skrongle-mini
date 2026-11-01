// Motivators (#559): the learner picks how their progress is dressed up, per
// course in its settings since #574, and the motivator worn draws the top of
// the course home page (src/motivators.rs). "Statistics and metrics" is the
// default; wearing a motivator again, or one the app does not have, loses no
// progress. Each course wears its own.
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, openCourse, register, wearMotivator } from "../../lib/learner.ts";

test("the motivator worn draws the course home, and switching keeps progress", async ({ page }) => {
  await register(page, newAccount("motivator"), "spanish");

  // A new learner wears the default: the statistics hero heads the home page.
  await page.goto("/");
  const hero = page.getByTestId("home-today");
  await expect(hero).toHaveAttribute("data-motivator", "stats");
  await expect(page.getByTestId("home-goal-ring")).toBeVisible();

  // The course's settings offer it, picked, with what each does behind the (i).
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings").click();
  const settings = page.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
  const chips = settings.getByTestId("courses-settings-motivator");
  await expect(settings.locator('[data-testid="courses-settings-motivator"][data-motivator-id="stats"] input')).toBeChecked();
  await settings.getByTestId("courses-settings-motivator-info").click();
  const dialog = page.getByTestId("courses-settings-motivator-dialog").first();
  await expect(dialog).toBeVisible();
  await expect(dialog.locator("dt")).toHaveCount(await chips.count());
  await dialog.getByTestId("courses-settings-motivator-close").click();
  await expect(dialog).toBeHidden();
  // The profile has none to offer any more.
  await page.goto("/profile");
  await expect(page.locator('[data-testid="profile-motivator"]')).toHaveCount(0);
  expect((await page.request.post("/api/profile", { data: { motivator: "stats" } })).status()).toBe(422);

  // Saving the motivator, and refusing one the app lacks, leaves the ring alone.
  await page.goto("/");
  const before = await page.getByTestId("home-goal-ring").getAttribute("data-points");
  await wearMotivator(page, "spanish", "stats");
  expect((await page.request.post("/courses/spanish/settings", { form: { motivator: "no-such-motivator" }, maxRedirects: 0 })).status()).toBe(400);
  await page.goto("/");
  await expect(hero).toHaveAttribute("data-motivator", "stats");
  await expect(page.getByTestId("home-goal-ring")).toHaveAttribute("data-points", before ?? "0");

  // Another course wears its own: XP in French leaves Spanish on Statistics.
  await openCourse(page, "french");
  await wearMotivator(page, "french", "xp");
  await page.goto("/");
  await expect(hero).toHaveAttribute("data-motivator", "xp");
  await openCourse(page, "spanish");
  await expect(hero).toHaveAttribute("data-motivator", "stats");
});
