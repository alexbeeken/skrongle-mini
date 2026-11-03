// Privacy (#74): the policy page is public, a learner asks to delete their account (#970), and the AI forms say where notes go. Exact: no
// model, no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { createCourse, devLogin } from "../../lib/app.ts";
import { newAccount, register } from "../../lib/learner.ts";

test("the privacy policy is served to visitors", async ({ page }) => {
  const res = await page.goto("/privacy");
  expect(res?.status()).toBe(200);
  await expect(page.locator("h1")).toHaveText("Privacy policy");
  await expect(page.getByTestId("docs-body")).toContainText("Last updated:");
  await expect(page.getByTestId("docs-link").filter({ hasText: "Cookie Notice" })).toHaveAttribute("data-slug", "cookies");
});

test("deleting the account signs out and waits 14 days; signing in can cancel it (#970)", async ({ page }) => {
  const account = newAccount("privacy");
  await register(page, account, "spanish");
  await page.goto("/profile");

  // There is no data export section.
  await expect(page.getByTestId("profile-export")).toHaveCount(0);

  // A wrong password keeps the account.
  await page.getByTestId("profile-delete-password").fill("not-the-password");
  await page.getByTestId("profile-delete-submit").click();
  await expect(page.getByTestId("profile-delete-error")).toBeVisible();

  await page.getByTestId("profile-delete-password").fill(account.password);
  await page.getByTestId("profile-delete-submit").click();
  // Signed out, with the date on the login page.
  await expect(page).toHaveURL(/\/login\?deletion=\d+/);
  await expect(page.getByTestId("login-notice")).toContainText("will be deleted on");
  await page.goto("/profile");
  await expect(page).toHaveURL(/\/login/);

  // Signing in works, but only the deletion page opens.
  await page.getByTestId("login-email").fill(account.email);
  await page.getByTestId("login-password").fill(account.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/account\/deletion$/);
  await expect(page.getByTestId("account-deletion-date")).toContainText("will be deleted for good on");
  await page.goto("/profile");
  await expect(page).toHaveURL(/\/account\/deletion$/);

  // Cancel deletion: the account works again.
  await page.getByTestId("account-deletion-cancel").click();
  await page.goto("/profile");
  await expect(page).toHaveURL(/\/profile$/);
  await expect(page.getByTestId("profile-delete-form")).toBeVisible();
});

test("the AI forms say which provider gets the notes", async ({ page }) => {
  await devLogin(page, "open=spanish&keys=fake&next=/courses");
  await createCourse(page);
  // The Generate dialog's line, then Use AI's under it once the dialog is closed.
  const disclosure = page.getByTestId("generate").getByTestId("ai-disclosure");
  await expect(disclosure).toBeVisible();
  await expect(disclosure).toHaveText(/^Your notes are sent to .+ using your key\.$/);
  await expect(disclosure).not.toContainText("your provider");
  await expect(page.getByTestId("generate-sensitive-note")).toContainText("sensitive personal data");
  await page.getByTestId("generate-close").click();
  await expect(page.getByTestId("course-ai-form").getByTestId("ai-disclosure")).toHaveText(/^Your notes are sent to .+ using your key\.$/);
  await expect(page.getByTestId("course-ai-sensitive-note")).toContainText("sensitive personal data");
});
