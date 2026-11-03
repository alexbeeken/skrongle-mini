// The profile's side list marks the section you are in (#420): the link of
// the section in view carries aria-current="location", after a jump and
// while scrolling. There is no AI section: the keys and the switch are the
// owner's, on Manage Users (#25).
import { expect, test } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

const link = (page: import("@playwright/test").Page, section: string) =>
  page.locator(`[data-testid="profile-nav-link"][data-section="${section}"]`);

test("the profile nav marks the section in view", async ({ page }) => {
  await register(page, newAccount("profnav"), "spanish");
  await page.goto("/profile");
  // The sections: personal, account, two-factor.
  await expect(page.locator('[data-testid="profile-nav-link"]')).toHaveCount(3);
  for (const gone of ["consent", "data"]) await expect(link(page, gone)).toHaveCount(0);
  await expect(link(page, "personal")).toHaveAttribute("aria-current", "location");
  await expect(page.locator('[data-testid="profile-nav-link"][aria-current]')).toHaveCount(1);

  await link(page, "two-factor").click();
  await expect(link(page, "two-factor")).toHaveAttribute("aria-current", "location");
  await expect(link(page, "personal")).not.toHaveAttribute("aria-current", /.*/);

  // Scrolling back to the top marks the first section again.
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(link(page, "personal")).toHaveAttribute("aria-current", "location");
  await expect(page.locator('[data-testid="profile-nav-link"][aria-current]')).toHaveCount(1);

  // Scrolling to the bottom marks the last section.
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(link(page, "two-factor")).toHaveAttribute("aria-current", "location");

  await expect(link(page, "ai")).toHaveCount(0);
  await expect(link(page, "api-keys")).toHaveCount(0);
});
