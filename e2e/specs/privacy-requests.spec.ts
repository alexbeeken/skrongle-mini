// Privacy requests that came by e-mail (#981): from /superuser, send an
// account its data (the link goes to the account's own address), forget an
// address with no account, and see both in the log with their reasons. The
// dev user is a superuser. The 404 for anyone else and the log rows are also
// checked over HTTP in moderation::tests.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";
import { mailLink, waitForMail } from "../lib/mail";

test("a superuser sends an account its data; the link goes to that account's address", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("privacy-export");
  await signUp(them, a);
  const email = a.email.toLowerCase();

  await devLogin(page);
  await page.goto("/superuser");
  const card = page.getByTestId("superuser-privacy-card");
  await card.getByTestId("superuser-privacy-info").click();
  await expect(page.getByTestId("superuser-privacy-info-dialog")).toContainText("needs a reason");
  await page.getByTestId("superuser-privacy-info-dialog").locator("[data-closes]").click();

  await card.getByTestId("superuser-privacy-export").click();
  const dialog = page.getByTestId("superuser-privacy-export-dialog");
  await expect(dialog).toContainText("account's own address");
  await dialog.getByTestId("superuser-privacy-export-email").fill(a.email);
  await dialog.getByTestId("superuser-privacy-export-reason").fill("Request 1 in the request log");
  await dialog.getByTestId("superuser-privacy-export-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`The data of ${email} is being gathered.`);
  const row = page.locator('[data-testid="superuser-privacy-log-row"][data-action="export"]').first();
  await expect(row).toContainText("Data sent");
  await expect(row).toContainText("Request 1 in the request log");
  await expect(row).toContainText("dev@example.com");

  // The link arrives at the account's address and opens once signed in as it.
  const link = mailLink(await waitForMail(email, /data is ready/), /\/profile\/export\/download\?token=/);
  await login(them, a);
  const file = await them.request.get(link);
  expect(file.status()).toBe(200);
  expect(await file.text()).toContain(email);
  await them.context().close();
});

test("a superuser forgets an address with no account, not one with an account", async ({ page }) => {
  await devLogin(page);
  await page.goto("/superuser");
  const card = page.getByTestId("superuser-privacy-card");
  const dialog = page.getByTestId("superuser-privacy-forget-dialog");

  await card.getByTestId("superuser-privacy-forget").click();
  await dialog.getByTestId("superuser-privacy-forget-email").fill("dev@example.com");
  await dialog.getByTestId("superuser-privacy-forget-reason").fill("Request 2");
  await dialog.getByTestId("superuser-privacy-forget-confirm").click();
  await expect(page.getByTestId("superuser-error")).toContainText("belongs to an account");

  const nobody = newAccount("privacy-forget").email.toLowerCase();
  await card.getByTestId("superuser-privacy-forget").click();
  await dialog.getByTestId("superuser-privacy-forget-email").fill(nobody);
  await dialog.getByTestId("superuser-privacy-forget-reason").fill("Request 3");
  await dialog.getByTestId("superuser-privacy-forget-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText("The address is forgotten.");
  // The address is not echoed back: not in the URL, not in the notice.
  expect(page.url()).not.toContain(encodeURIComponent(nobody));
  const row = page.locator('[data-testid="superuser-privacy-log-row"][data-action="forget"]').first();
  await expect(row).toContainText("Request 3");
});
