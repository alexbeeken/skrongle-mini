// The profile's side list marks the section you are in (#420): the link of
// the section in view carries aria-current="location", after a jump and
// while scrolling; the AI link is named like its section.
import { expect, test } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

const link = (page: import("@playwright/test").Page, section: string) =>
  page.locator(`[data-testid="profile-nav-link"][data-section="${section}"]`);

test("the profile nav marks the section in view", async ({ page }) => {
  await register(page, newAccount("profnav"), "spanish");
  await page.goto("/profile");
  await expect(link(page, "personal")).toHaveAttribute("aria-current", "location");
  await expect(page.locator('[data-testid="profile-nav-link"][aria-current]')).toHaveCount(1);

  await link(page, "api-keys").click();
  await expect(link(page, "api-keys")).toHaveAttribute("aria-current", "location");
  await expect(link(page, "personal")).not.toHaveAttribute("aria-current", /.*/);

  // Scrolling back to the top marks the first section again.
  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(link(page, "personal")).toHaveAttribute("aria-current", "location");
  await expect(page.locator('[data-testid="profile-nav-link"][aria-current]')).toHaveCount(1);

  // Scrolling to the bottom marks the last section.
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
  await expect(link(page, "data")).toHaveAttribute("aria-current", "location");

  // The AI switch lives on the Privacy and consent card, which has its own link (#974).
  await expect(link(page, "ai")).toHaveCount(0);
});
