// The owner as help desk (#635): from the Manage Users page (/manage,
// owners only), a temporary password (shown once; the learner must
// choose a new one at the next sign-in) and "Turn off two-factor". The dev
// user is an owner; "Make owner" makes another (#16), and the old
// /superuser address redirects to the page.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";

test("an owner sets a temporary password; the learner signs in with it and must choose a new one", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("temp-pw");
  await signUp(them, a);
  await login(them, a); // registering signs nobody in (#379)

  // The account menu links an owner to the page; the profile lists no accounts.
  await devLogin(page);
  await page.goto("/profile");
  await expect(page.getByTestId("profile-form")).toBeVisible();
  await expect(page.locator("text=" + a.email.toLowerCase())).toHaveCount(0);
  await page.getByTestId("nav-account-toggle").click();
  await expect(page.getByTestId("nav-manage")).toHaveText("Manage Users");
  await page.getByTestId("nav-manage").click();
  await expect(page).toHaveURL(/\/manage$/);
  const row = page.getByTestId("manage-account-row").filter({ hasText: a.email.toLowerCase() });
  await row.getByTestId("manage-account-temp-password").click();
  const dialog = page.getByTestId("manage-password-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(a.email.toLowerCase());
  await dialog.getByTestId("manage-password-confirm").click();

  // Shown once, in a dialog with Copy.
  const shown = page.getByTestId("manage-temp-password-dialog");
  await expect(shown).toBeVisible();
  const temp = await page.getByTestId("manage-temp-password").inputValue();
  expect(temp.length).toBeGreaterThanOrEqual(12);
  await page.getByTestId("manage-temp-password-close").click();
  await page.goto("/manage");
  await expect(page.getByTestId("manage-temp-password")).toHaveCount(0);

  // The learner was signed out; the old password no longer works.
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/login/);
  await them.getByTestId("login-email").fill(a.email);
  await them.getByTestId("login-password").fill(a.password);
  await them.getByTestId("login-submit").click();
  await expect(them.getByTestId("login-error")).toBeVisible();

  // The temporary one signs in, straight to the gate.
  await them.getByTestId("login-email").fill(a.email);
  await them.getByTestId("login-password").fill(temp);
  await them.getByTestId("login-submit").click();
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/password\/change\?next=%2Fprofile$/);
  expect((await them.request.get("/api/progress")).status()).toBe(403);
  await them.getByTestId("password-change-info").click();
  await expect(them.getByTestId("password-change-dialog")).toBeVisible();
  await them.getByTestId("password-change-dialog").getByRole("button").click();

  await them.getByTestId("password-change-new").fill("my-own-new-password");
  await them.getByTestId("password-change-confirm").fill("my-own-new-passwordX");
  await them.getByTestId("password-change-submit").click();
  await expect(them.getByTestId("password-change-error")).toBeVisible();
  await them.getByTestId("password-change-new").fill("my-own-new-password");
  await them.getByTestId("password-change-confirm").fill("my-own-new-password");
  await them.getByTestId("password-change-submit").click();
  await expect(them).toHaveURL(/\/profile$/);
  await expect(them.getByTestId("profile-form")).toBeVisible();
  await them.context().close();
});

test("an owner turns off another account's two-factor; others get 404", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("twofa-off");
  await signUp(them, a);
  await login(them, a); // registering signs nobody in (#379)
  // Not an owner: no link in the menu, and the page and its actions answer 404.
  await them.goto("/profile");
  await them.getByTestId("nav-account-toggle").click();
  await expect(them.getByTestId("nav-manage")).toHaveCount(0);
  expect((await them.request.get("/manage", { maxRedirects: 0 })).status()).toBe(404);
  for (const path of ["/manage/temp-password", "/manage/two-factor-off", "/manage/make-owner"]) {
    expect((await them.request.post(path, { form: { email: "dev@example.com" }, maxRedirects: 0 })).status()).toBe(404);
  }
  await them.context().close();

  await devLogin(page);
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: a.email.toLowerCase() });
  await row.getByTestId("manage-account-two-factor-off").click();
  const dialog = page.getByTestId("manage-two-factor-dialog");
  await expect(dialog).toContainText(a.email.toLowerCase());
  await dialog.getByTestId("manage-two-factor-confirm").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`Two-factor sign-in is off for ${a.email.toLowerCase()}`);
});

test("an owner makes another account an owner; the old /superuser address leads to the page", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("make-owner");
  await signUp(them, a);
  await login(them, a); // registering signs nobody in (#379)

  await devLogin(page);
  await page.goto("/superuser");
  await expect(page).toHaveURL(/\/manage$/);
  const row = page.getByTestId("manage-account-row").filter({ hasText: a.email.toLowerCase() });
  await expect(row).toHaveAttribute("data-owner", "false");
  await row.getByTestId("manage-account-make-owner").click();
  const dialog = page.getByTestId("manage-owner-dialog");
  await expect(dialog).toContainText(a.email.toLowerCase());
  await dialog.getByTestId("manage-owner-confirm").click();
  await expect(page.getByTestId("manage-notice")).toContainText(`${a.email.toLowerCase()} is an owner now`);
  await expect(row).toHaveAttribute("data-owner", "true");
  await expect(row.getByTestId("manage-account-make-owner")).toHaveCount(0);

  // Their account menu now links the page.
  await them.goto("/profile");
  await them.getByTestId("nav-account-toggle").click();
  await expect(them.getByTestId("nav-manage")).toBeVisible();
  await them.context().close();
});

// A new password (or invitation link) is the answer to a POST, rendered in
// place; manage.js sets the address bar back to /manage, so a reload
// asks for the page and does not make another one (#665). The invitation
// card needs SKRONGLE_SIGNUP=invite, which the e2e server does not run; the
// same script covers both, and routes::tests::signup_by_invitation checks the
// invite answer carries it.
test("reloading after a temporary password is set does not set another", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("temp-pw-reload");
  await signUp(them, a);

  await devLogin(page);
  await page.goto("/manage");
  const row = page.getByTestId("manage-account-row").filter({ hasText: a.email.toLowerCase() });
  await row.getByTestId("manage-account-temp-password").click();
  await page.getByTestId("manage-password-confirm").click();
  await expect(page.getByTestId("manage-temp-password-dialog")).toBeVisible();
  const temp = await page.getByTestId("manage-temp-password").inputValue();
  await expect(page).toHaveURL(/\/manage$/);

  await page.reload();
  await expect(page.getByTestId("manage-page")).toBeVisible();
  await expect(page.getByTestId("manage-temp-password")).toHaveCount(0);

  // The password shown first still works: the reload did not replace it.
  await them.goto("/login");
  await them.getByTestId("login-email").fill(a.email);
  await them.getByTestId("login-password").fill(temp);
  await them.getByTestId("login-submit").click();
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/password\/change\?next=%2Fprofile$/);
  await them.context().close();
});
