// A date to aim for (#235): set in a course's settings (the gear on
// /courses), it adds one chip to the course home's chip row; pressing it
// opens the lines under the row, Change the date opens that course's
// settings again, Remove it takes the chip away. Another course has no
// date. No key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, openCourse, register } from "../../lib/learner.ts";

/** `days` from today as <input type="date"> holds it, in the browser's time zone. */
function isoIn(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() + days);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

test("a date to aim for is one chip that opens, changes and goes", async ({ page }) => {
  await register(page, newAccount("goal-date"), "spanish");
  await expect(page.getByTestId("home-stats")).toBeVisible();
  await expect(page.getByTestId("home-goal-date")).toHaveCount(0);

  // Set three weeks out behind the gear, and Save.
  await page.goto("/courses");
  await page.getByTestId("courses-projects").locator('[data-course-id="spanish"]').getByTestId("courses-settings").click();
  const dlg = page.locator('[data-testid="courses-settings-dialog"][data-course-id="spanish"]');
  await dlg.getByTestId("courses-settings-goal-date").fill(isoIn(21));
  const saved = page.waitForResponse((r) => r.url().endsWith("/courses/spanish/settings") && r.request().method() === "POST");
  await dlg.getByTestId("courses-settings-save").click();
  expect((await saved).status()).toBe(303);
  await expect(dlg).toBeHidden();

  // One chip on the home page; pressing it opens the lines under the row.
  await page.goto("/");
  const chip = page.getByTestId("home-goal-date-chip");
  await expect(chip).toHaveCount(1);
  await expect(chip).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByTestId("home-goal-date-more")).toBeHidden();
  await chip.click();
  await expect(chip).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByTestId("home-goal-date-line")).toContainText("lessons to pass by");

  // Change the date: the course's settings open on the date.
  await page.getByTestId("home-goal-date-change").click();
  await expect(dlg).toBeVisible();
  await expect(dlg.getByTestId("courses-settings-goal-date")).toHaveValue(isoIn(21));
  await dlg.getByTestId("courses-settings-close").click();

  // Remove it: back on the home page without the chip, and the field is empty.
  await page.goto("/");
  await page.getByTestId("home-goal-date-chip").click();
  await page.getByTestId("home-goal-date-remove").click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByTestId("home-goal-date")).toHaveCount(0);
  await page.goto("/courses");
  await expect(dlg.getByTestId("courses-settings-goal-date")).toHaveValue("");

  // Another course has no date.
  await openCourse(page, "french");
  await expect(page.getByTestId("home-stats")).toBeVisible();
  await expect(page.getByTestId("home-goal-date")).toHaveCount(0);
});
