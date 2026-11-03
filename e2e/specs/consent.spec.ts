// The profile's Privacy and consent card (#974): each consent with its date,
// explained behind an (i). The AI features have no row: whether they are
// shown is the server's choice, the owner's on Manage Users (#25,
// specs/ai-off.spec.ts).
import { expect, test } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

test("the consent card shows each consent, and no AI switch", async ({ page }) => {
  await register(page, newAccount("consent"), "spanish");
  await page.goto("/profile");
  const card = page.getByTestId("profile-consent");
  await expect(card).toBeVisible();
  await expect(page.locator('[data-testid="profile-nav-link"][data-section="consent"]')).toBeVisible();

  // The Terms version and the age confirmation, both dated by sign-up.
  await expect(card.getByTestId("profile-terms")).not.toHaveAttribute("data-terms-version", "");
  await expect(card.getByTestId("consent-age")).toContainText(/^Confirmed \d/);

  // The (i) explains, without anything on the card itself.
  const info = page.getByTestId("profile-consent-info-dialog");
  await page.getByTestId("profile-consent-info").click();
  await expect(info).toBeVisible();
  await expect(info).toContainText("delete your account");
  await page.getByTestId("profile-consent-info-close").click();
  await expect(info).not.toBeVisible();

  // No AI row and no keys: those are the owner's, on Manage Users.
  await expect(page.locator('[data-testid^="consent-ai"], [data-testid^="profile-api-key"]')).toHaveCount(0);
});
