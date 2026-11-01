// Registration and password reset (#379): registering an address that has an
// account answers like a new registration and mails the address instead; the
// login page's "Forgot your password?" sends a link (read from the dev
// mailbox, /dev/inbox) whose form sets a new password, signs out every
// session, and the new password signs in. Exact: no key, no network.
import { expect, test } from "../../lib/fixtures.ts";
import { login, logout, newAccount, register, signUp } from "../../lib/learner.ts";
import { mailLink, waitForMail } from "../../lib/mail.ts";

test("registering a taken address looks the same and mails its owner", async ({ page, browser }) => {
  const a = newAccount("taken");
  await register(page, a, "spanish");
  await logout(page);

  // Someone else, with another password: the same "check your mail".
  const other = await (await browser.newContext()).newPage();
  await signUp(other, { email: a.email, password: "not-the-owners-password" });
  await expect(other.getByTestId("register-sent-email")).toHaveText(a.email);
  await expect(other.getByTestId("nav-account-toggle")).toHaveCount(0);
  await other.context().close();

  const mail = await waitForMail(a.email, /already have/);
  expect(mailLink(mail, /\/reset$/)).toMatch(/\/reset$/);
  expect(mailLink(mail, /\/login$/)).toMatch(/\/login$/);
  // The owner's password still works.
  await login(page, a);
});

test("a forgotten password is reset from the login page through the mailed link", async ({ page, browser }) => {
  const a = newAccount("reset");
  await register(page, a, "spanish");
  // A second browser stays signed in, to see the reset end its session.
  const elsewhere = await (await browser.newContext()).newPage();
  await login(elsewhere, a);
  await logout(page);

  await page.goto("/login");
  await page.getByTestId("login-reset-link").click();
  await expect(page).toHaveURL(/\/reset$/);
  await page.getByTestId("reset-email").fill(a.email);
  await page.getByTestId("reset-submit").click();
  await expect(page.getByTestId("reset-sent")).toContainText(a.email);

  // An address with no account gets the same answer.
  await page.goto("/reset");
  await page.getByTestId("reset-email").fill(newAccount("nobody").email);
  await page.getByTestId("reset-submit").click();
  await expect(page.getByTestId("reset-sent")).toBeVisible();

  const mail = await waitForMail(a.email, /Reset your/);
  const link = mailLink(mail, /\/reset\?token=[0-9a-f]{64}$/);
  await page.goto(link);
  await expect(page.getByTestId("reset-account")).toHaveText(a.email);
  const fresh = { email: a.email, password: `new-${a.password}` };
  await page.getByTestId("reset-password").fill(fresh.password);
  await page.getByTestId("reset-save").click();
  await expect(page.getByTestId("reset-done")).toBeVisible();

  // Single use.
  await page.goto(link);
  await expect(page.getByTestId("reset-invalid")).toBeVisible();

  // The other browser was signed out; the old password fails, the new one works.
  await elsewhere.goto("/profile");
  await expect(elsewhere).toHaveURL(/\/login/);
  await elsewhere.context().close();
  await page.goto("/login");
  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page.getByTestId("login-error")).toBeVisible();
  await login(page, fresh);
});
