// E-mail verification (#150): registering sends a confirmation link, which
// the dev outbox (/dev/mail) lists; its page has the link, and opening that confirms the address. The
// profile's e-mail field shows a red ! until then. Exact: no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { newAccount, register } from "../../lib/learner.ts";
import { devPage } from "../../lib/mail.ts";
import type { Page } from "@playwright/test";

// The mini lane (E2E_MINI=1) has no dev mailbox: the link is only logged.
test.skip(process.env.E2E_MINI === "1", "no dev mailbox in the mini lane: the confirmation link is only logged");

/** The newest confirmation link sent to `email`, from its page in the dev
 * outbox, opened in a browser of its own. */
async function newestLink(page: Page, email: string): Promise<string> {
  const dev = await devPage(page.context().browser()!, "/dev/mail");
  const message = dev.locator(`[data-testid="dev-mail-message"][data-purpose="email-verify"][data-to="${email}"]`).first();
  await expect(message).toBeVisible();
  await message.getByTestId("dev-mail-view").click();
  const href = await dev.getByTestId("dev-mail-link").first().getAttribute("href");
  expect(href).toMatch(/\/verify-email\?token=[0-9a-f]{64}$/);
  await dev.context().close();
  return href!;
}

test("a new learner confirms their e-mail address from the link", async ({ page }) => {
  const account = newAccount("verify");
  await register(page, account, "spanish");

  await page.goto("/profile");
  await expect(page.getByTestId("email-unverified")).toBeVisible();

  const link = await newestLink(page, account.email);
  await page.goto(link);
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
  await expect(page.locator("h1")).toHaveText("Your e-mail address is confirmed");

  await page.goto("/profile");
  await expect(page.getByTestId("email-verified")).toBeVisible();
  await expect(page.getByTestId("email-verify-resend")).toHaveCount(0);
  await expect(page.getByTestId("email-unverified")).toHaveCount(0);

  // The link works once.
  await page.goto(link);
  await expect(page.getByTestId("verify-email-invalid")).toBeVisible();
  await expect(page.getByTestId("verify-email-already")).toBeVisible();
});

test("sending the e-mail again replaces the first link", async ({ page }) => {
  const account = newAccount("verify-again");
  await register(page, account, "spanish");
  const first = await newestLink(page, account.email);

  await page.goto("/profile");
  await expect(page.getByTestId("email-unverified")).toBeVisible();
  await expect(page.getByTestId("profile-form")).not.toContainText("check your inbox");
  await page.getByTestId("email-unverified").click();
  await expect(page.getByTestId("email-unverified-dialog")).toContainText("Email not yet verified. Check your inbox for the link.");
  await page.getByTestId("email-verify-resend").click();
  await expect(page.getByTestId("profile-notice")).toHaveText(`✓ Sent to ${account.email}.`);

  const latest = await newestLink(page, account.email);
  expect(latest).not.toBe(first);
  await page.goto(first);
  await expect(page.getByTestId("verify-email-invalid")).toBeVisible();
  await expect(page.getByTestId("verify-email-resend")).toBeVisible();
  await page.goto(latest);
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
});
