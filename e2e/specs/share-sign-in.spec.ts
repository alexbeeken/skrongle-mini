// A course's share link survives every way of signing in (#1016): the
// two-factor code step, and the Google round trip (a new account's confirm
// step, and an existing account's code step). Each learner signs in with an
// address the link was not sent to, so only the link puts the course on
// their list. A link that no longer works says so before and after signing
// in (#1082).
import { expect, test, type Page } from "@playwright/test";
import { createCourse, devLogin, devReset } from "../lib/app";
import { logout, newAccount, register, signUp } from "../lib/learner";
import { inbox, mailLink, waitForMail } from "../lib/mail";

const google = (params: Record<string, string>) => `/auth/google?${new URLSearchParams(params)}`;

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
  await expect(page.getByTestId("course-share-notice")).toHaveText("Invitation sent to 1 address.");
  const mail = await waitForMail(invited.email, /shared a course with you/);
  await logout(page);
  return { id, link: mailLink(mail, /\/courses\/shared\//) };
}

/** The id of the newest mail to `email` (0 for none). */
async function lastMail(email: string): Promise<number> {
  return (await inbox(email))[0]?.id ?? 0;
}

/** The sign-in code mailed to `email` after mail `seen`, waiting for it so an older code is never used. */
async function signInCode(email: string, seen: number): Promise<string> {
  let code = "";
  await expect
    .poll(async () => {
      const m = (await inbox(email)).find((x) => /sign-in code/.test(x.subject) && x.id > seen);
      code = m?.text.match(/\d{6}/)?.[0] ?? "";
      return code;
    }, { message: `no new sign-in code for ${email}` })
    .not.toBe("");
  return code;
}

/** Signed in, the course from the link is the active one on /courses. */
async function hasCourse(page: Page, id: string): Promise<void> {
  await page.goto("/courses");
  await expect(page.locator(`[data-testid="courses-card"][data-course-id="${id}"]`)).toHaveAttribute("data-active", "true");
}

/** A confirmed account with e-mailed sign-in codes, signed out. */
async function twoFactorAccount(page: Page) {
  const a = newAccount("share-2fa");
  await register(page, a, "spanish");
  const confirm = await waitForMail(a.email, /Confirm your e-mail address/);
  await page.goto(mailLink(confirm, /\/verify-email\?/));
  await page.goto("/profile");
  await page.locator('[data-testid="profile-two-factor-method"][data-method="email"]').click();
  const dialog = page.getByTestId("two-factor-confirm-dialog");
  await dialog.getByTestId("two-factor-confirm-password").fill(a.password);
  await dialog.getByTestId("two-factor-confirm-submit").click();
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "email");
  await logout(page);
  return a;
}

test("a share link survives the two-factor code step, and cancelling it keeps the link", async ({ page }) => {
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

  const seen = await lastMail(a.email);
  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/login\/code$/);
  await page.getByTestId("login-code-input").fill(await signInCode(a.email, seen));
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await hasCourse(page, id);
});

test("a share link survives Google sign-up's confirm step", async ({ page }) => {
  const { id, link } = await sharedCourse(page);
  const a = newAccount("share-google");

  await page.goto(link);
  await expect(page).toHaveURL(/\/login\?share=/);
  const token = new URL(page.url()).searchParams.get("share")!;
  await expect(page.getByTestId("login-google")).toHaveAttribute("href", new RegExp(`/auth/google\\?share=${token}$`));
  await page.getByTestId("login-register-link").click();
  await expect(page.getByTestId("register-google")).toHaveAttribute("href", new RegExp(`/auth/google\\?share=${token}$`));

  await page.goto(google({ mock_email: a.email, share: token }));
  await expect(page).toHaveURL(/\/auth\/google\/confirm$/);
  await page.getByTestId("google-confirm-age").check();
  await page.getByTestId("google-confirm-terms").check();
  await page.getByTestId("google-confirm-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await hasCourse(page, id);
});

test("a share link survives a Google sign-in with two-factor on", async ({ page }) => {
  const a = await twoFactorAccount(page);
  // Google once (and its code), so the account is a Google one too.
  let seen = await lastMail(a.email);
  await page.goto(google({ mock_email: a.email }));
  await page.getByTestId("login-code-input").fill(await signInCode(a.email, seen));
  await page.getByTestId("login-code-submit").click();
  await expect(page).toHaveURL(/\/profile\?saved=google-joined$/);
  await logout(page);

  const { id, link } = await sharedCourse(page);
  await page.goto(link);
  const token = new URL(page.url()).searchParams.get("share")!;
  seen = await lastMail(a.email);
  await page.goto(google({ mock_email: a.email, share: token }));
  await expect(page).toHaveURL(/\/login\/code$/);
  await page.getByTestId("login-code-input").fill(await signInCode(a.email, seen));
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
  if (await signUp(guest, a)) await logout(guest);
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
