// A course's share link survives every way of signing in (#1016): the
// two-factor code step. Each learner signs in with an
// address the link was not sent to, so only the link puts the course on
// their list. A link that no longer works says so before and after signing
// in (#1082).
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset, shareLink } from "../lib/app";
import { logout, newAccount, register, signUp } from "../lib/learner";
import { totp } from "../lib/totp";

test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devReset(page);
  await page.close();
});

/** As the dev user: a new course shared with a fresh address; its id and link. */
async function sharedCourse(page: Page): Promise<{ id: string; link: string }> {
  await devLogin(page, "next=/courses");
  const id = await createCourse(page);
  await page.getByTestId("generate-close").click();
  const invited = newAccount("share-invited");
  await page.getByTestId("course-edit-share").click();
  await page.getByTestId("course-share-emails").fill(invited.email);
  await page.getByTestId("course-share-send").click();
  await expect(page.getByTestId("course-share-notice")).toContainText("1 invitation link made");
  const link = await shareLink(page, invited.email);
  await logout(page);
  return { id, link };
}

/** The authenticator's code for `secret` once it is not `used`: each code works once. */
async function nextCode(secret: string, used: string): Promise<string> {
  let code = "";
  await expect
    .poll(() => {
      code = totp(secret);
      return code !== used;
    }, { message: "a fresh authenticator code", timeout: 40_000, intervals: [1000] })
    .toBe(true);
  return code;
}

/** Signed in, the course from the link is the active one on /courses. */
async function hasCourse(page: Page, id: string): Promise<void> {
  await page.goto("/courses");
  await expect(page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveAttribute("data-active", "true");
}

/** An account with an authenticator app as its second step, signed out:
 * the account, the app's setup key and the code that set it up. */
async function twoFactorAccount(page: Page) {
  const a = newAccount("share-2fa");
  await register(page, a, "spanish");
  await page.goto("/profile");
  await page.locator('[data-testid="profile-two-factor-choice"][data-on="true"]').click();
  const dialog = page.getByTestId("two-factor-confirm-dialog");
  await dialog.getByTestId("two-factor-confirm-password").fill(a.password);
  await dialog.getByTestId("two-factor-confirm-submit").click();
  await expect(page.getByTestId("two-factor-totp-dialog")).toBeVisible();
  const secret = (await page.getByTestId("two-factor-totp-secret").innerText()).replace(/\s/g, "");
  const used = totp(secret);
  await page.getByTestId("two-factor-totp-code").fill(used);
  await page.getByTestId("two-factor-totp-confirm").click();
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-on", "true");
  await logout(page);
  return { ...a, secret, used };
}

test("a share link survives the two-factor code step, and cancelling it keeps the link", async ({ page }) => {
  test.setTimeout(120_000); // a fresh authenticator code can be 30 s away
  const a = await twoFactorAccount(page);
  const { id, link } = await sharedCourse(page);

  await page.goto(link);
  await expect(page).toHaveURL(/\/login\?share=/);
  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/login\/code$/);
  // Giving up the code goes back to the login form for the same link.
  await page.getByTestId("login-code-cancel").click();
  await expect(page).toHaveURL(/\/login\?share=/);
  await expect(page.getByTestId("login-share-notice")).toBeVisible();

  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/login\/code$/);
  await page.getByTestId("login-code-input").fill(await nextCode(a.secret, a.used));
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await hasCourse(page, id);
});

test("an old link after the share was removed says it no longer works, signed out and signed in (#1082)", async ({ page, browser }) => {
  const { id, link } = await sharedCourse(page);
  // The owner removes the address: the link in the mail stops working.
  await devLogin(page, `next=/courses/${id}?share=1`);
  await expect(page.getByTestId("course-share")).toBeVisible();
  await page.getByTestId("course-share-remove").click();
  await expect(page.getByTestId("course-share-notice")).toContainText("No longer shared with");
  await logout(page);

  // Signed out: the login page says so, and signing in leads to the course list with the same notice.
  const a = newAccount("share-gone");
  const guest = await (await browser.newContext()).newPage();
  await signUp(guest, a);
  await logout(guest);
  await guest.goto(link);
  await expect(guest).toHaveURL(/\/login\?share=/);
  await expect(guest.getByTestId("login-share-notice")).toHaveText("This link no longer works. Ask the course's owner to share the course with you again.");
  await guest.getByTestId("login-email").fill(a.email);
  await guest.getByTestId("login-password").fill(a.password);
  await guest.getByTestId("login-submit").click();
  await expect(guest).toHaveURL(/\/courses\?notice=share-gone$/);
  await expect(guest.getByTestId("courses-notice")).toHaveText("This link no longer works. Ask the course's owner to share the course with you again.");
  await expect(guest.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveCount(0);

  // Signed in: straight to the list with the notice, not a bare 404.
  await guest.goto(link);
  await expect(guest).toHaveURL(/\/courses\?notice=share-gone$/);
  await expect(guest.getByTestId("courses-notice")).toBeVisible();
  await guest.context().close();
});
