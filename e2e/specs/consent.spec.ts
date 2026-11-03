// The profile's Privacy and consent card (#974): each consent with its date,
// explained behind an (i). "Do Not Show Me AI Features" is the one a learner
// can switch here, and the profile's only AI switch: on hides them, the
// change survives a reload, and a key on file is asked about first because
// hiding AI deletes it.
import { expect, test } from "@playwright/test";
import { newAccount, register } from "../lib/learner";

test("the consent card shows each consent, and Do Not Show Me AI Features hides them and shows them again across a reload", async ({ page }) => {
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

  const ai = page.getByTestId("consent-ai");
  const sw = page.getByTestId("consent-ai-switch");
  await expect(page.locator("#consent-ai-label")).toHaveText("Do Not Show Me AI Features");
  await expect(page.getByTestId("profile-ai")).toHaveCount(0);
  await expect(ai).toHaveAttribute("data-ai-off", "false");
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await expect(sw).toHaveText("Off");
  await expect(page.getByTestId("consent-ai-state")).toHaveText(/^Shown since /);

  // On hides them, in place.
  await sw.click();
  await expect(ai).toHaveAttribute("data-ai-off", "true");
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(sw).toHaveText("On");
  await expect(page.getByTestId("consent-ai-state")).toHaveText(/^Hidden since /);
  await expect(page.getByTestId("profile-api-keys")).toBeHidden();
  await expect(page).toHaveURL(/\/profile$/);

  await page.goto("/profile");
  await expect(sw).toHaveAttribute("aria-checked", "true");
  await expect(page.getByTestId("consent-ai-state")).toHaveText(/^Hidden since /);

  // Off shows them again.
  await sw.click();
  await expect(sw).toHaveAttribute("aria-checked", "false");
  await expect(page.getByTestId("consent-ai-state")).toHaveText(/^Shown since /);
  await expect(page.getByTestId("profile-api-keys")).toBeVisible();
  await page.goto("/profile");
  await expect(ai).toHaveAttribute("data-ai-off", "false");
  await expect(page.getByTestId("consent-ai-state")).toHaveText(/^Shown since /);
});

test("with a key on file the card's AI switch asks first, and the key line goes", async ({ page }) => {
  await register(page, newAccount("consent-key"), "spanish");
  await page.goto("/profile");
  const row = page.locator('[data-testid="profile-api-key"][data-provider="anthropic"]');
  await row.getByTestId("profile-api-key-toggle").click();
  await row.getByTestId("profile-api-key-input").fill("sk-ant-test-paste-ABCD");
  await expect(row).toHaveAttribute("data-key-set", "true");
  await page.goto("/profile");
  await expect(page.getByTestId("consent-ai-key")).toBeVisible();

  const dialog = page.getByTestId("profile-ai-off-dialog");
  await page.getByTestId("consent-ai-switch").click();
  await expect(dialog).toBeVisible();
  await page.getByTestId("profile-ai-off-cancel").click();
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "false");

  await page.getByTestId("consent-ai-switch").click();
  await page.getByTestId("profile-ai-off-confirm").click();
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "true");
  await expect(page.getByTestId("consent-ai-key")).toHaveCount(0);
  await page.goto("/profile");
  await expect(page.getByTestId("consent-ai-key")).toHaveCount(0);
  await expect(page.getByTestId("consent-ai-switch")).toHaveAttribute("aria-checked", "true");
});
