// Rest days (#237): the Rest days card on the profile saves the weekdays
// picked at once, at most two (a third chip is off), and the home page's
// Today card names them behind its (i).
import { expect, test } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

test("rest days save on the profile and show behind the Today card's (i)", async ({ page }) => {
  await register(page, newAccount("rest"), "spanish");
  await page.goto("/");
  await page.getByTestId("home-info-goal").click();
  await expect(page.getByTestId("home-info-rest-days")).toHaveText("No rest days. You can pick up to two on your profile.");

  await page.goto("/profile");
  const day = (w: number) => page.locator(`[data-testid="profile-rest-day"][data-weekday="${w}"]`);
  await expect(page.getByTestId("profile-rest-day")).toHaveCount(7);
  await day(6).click();
  await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
  await day(0).click();
  await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
  await expect(day(0)).toHaveAttribute("data-saved", "");
  // Two picked: the other days cannot be.
  await expect(day(1).locator("input")).toBeDisabled();
  await expect(day(6).locator("input")).toBeEnabled();

  await page.reload();
  await expect(day(6).locator("input")).toBeChecked();
  await expect(day(0).locator("input")).toBeChecked();
  await expect(day(3).locator("input")).toBeDisabled();

  await page.goto("/");
  await page.getByTestId("home-info-goal").click();
  await expect(page.getByTestId("home-info-rest-days")).toHaveText(/^Your rest days: Saturday, Sunday\./);
});
