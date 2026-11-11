// A learner's account on a release server (#40): change the password, turn
// two-factor sign-in on and off (the codes are worked out here from the setup
// key, e2e/lib/totp.ts; there is no dev authenticator), sign out and in, switch
// the language, and check the home page and a lesson at phone width.
import { apiProgress, login, logout, openCourse, type Account } from "../../lib/learner.ts";
import { expect, newLearner, newPerson, test } from "../../lib/release.ts";
import { totp } from "../../lib/totp.ts";
import type { Page } from "@playwright/test";

async function choose(page: Page, on: boolean): Promise<void> {
  await page.locator(`[data-testid="profile-two-factor-choice"][data-on="${on}"]`).click();
}

async function confirmWith(page: Page, password: string): Promise<void> {
  const dialog = page.getByTestId("two-factor-confirm-dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("two-factor-confirm-password").fill(password);
  await dialog.getByTestId("two-factor-confirm-submit").click();
}

/** A code for `secret` with at least 4 s left in its 30 s step, different from `not`. */
async function freshCode(secret: string, not?: string): Promise<string> {
  let code = "";
  await expect
    .poll(() => {
      const now = Date.now() / 1000;
      code = totp(secret, now);
      return 30 - (now % 30) >= 4 && code !== not;
    }, { timeout: 40_000, intervals: [500] })
    .toBe(true);
  return code;
}

test("change the password, then two-factor on and off, signing in each time", async ({ browser }) => {
  test.setTimeout(120_000);
  const { page, account } = await newLearner(browser, "account");
  const me: Account = { ...account };

  // Password: the wrong current one is refused, the right one changes it.
  await page.goto("/profile");
  await page.getByTestId("profile-password-open").click();
  const pw = page.getByTestId("profile-password-dialog");
  await page.getByTestId("profile-password-current").fill("not the password");
  await page.getByTestId("profile-password-new").fill("a brand new password");
  await page.getByTestId("profile-password-confirm").fill("a brand new password");
  await page.getByTestId("profile-password-submit").click();
  await expect(pw.getByTestId("profile-password-error")).toContainText("current password is not right");
  await page.getByTestId("profile-password-current").fill(me.password);
  await page.getByTestId("profile-password-new").fill("a brand new password");
  await page.getByTestId("profile-password-confirm").fill("a brand new password");
  await page.getByTestId("profile-password-submit").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Password changed");
  me.password = "a brand new password";

  // The old password no longer signs in; the new one does.
  await logout(page);
  await page.getByTestId("login-email").fill(me.email);
  await page.getByTestId("login-password").fill(account.password);
  await page.getByTestId("login-submit").click();
  await expect(page.getByTestId("login-error")).toBeVisible();
  await login(page, me);

  // Two-factor on: the password, then the setup key, then a code from it.
  await page.goto("/profile");
  await choose(page, true);
  await confirmWith(page, me.password);
  await expect(page.getByTestId("two-factor-totp-dialog")).toBeVisible();
  const secret = (await page.getByTestId("two-factor-totp-secret").innerText()).replace(/\s/g, "");
  const first = await freshCode(secret);
  await page.getByTestId("two-factor-totp-code").fill(first);
  await page.getByTestId("two-factor-totp-confirm").click();
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-on", "true");

  // Signing in now asks for a code; a wrong one is refused, a fresh one works.
  await logout(page);
  await page.getByTestId("login-email").fill(me.email);
  await page.getByTestId("login-password").fill(me.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/login\/code$/);
  await page.getByTestId("login-code-input").fill("000000");
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("login-code-error")).toBeVisible();
  await page.getByTestId("login-code-input").fill(await freshCode(secret, first));
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", me.email.toLowerCase());

  // Two-factor off: the password again; the next sign-in is the password alone.
  await page.goto("/profile");
  await choose(page, false);
  await confirmWith(page, me.password);
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-on", "false");
  await logout(page);
  await login(page, me);
});

test("the language switch changes the pages and the account keeps it", async ({ browser }) => {
  const { page, account } = await newLearner(browser, "lang");
  await page.goto("/profile");
  await page.getByTestId("profile-lang").selectOption("es");
  await expect(page.locator("html")).toHaveAttribute("lang", "es");
  await expect(page.getByTestId("nav-logout")).toHaveText("Cerrar sesión");
  // Signed out and back in on a browser that never chose, it is still Spanish.
  const other = await newPerson(browser);
  await login(other.page, { email: account.email.toLowerCase(), password: account.password });
  await expect(other.page.locator("html")).toHaveAttribute("lang", "es");
  // And back to English.
  await other.page.goto("/profile");
  await other.page.getByTestId("profile-lang").selectOption("en");
  await expect(other.page.locator("html")).toHaveAttribute("lang", "en");
  await expect(other.page.getByTestId("nav-logout")).toHaveText("Log out");
});

test("the home page and a lesson fit a 390 px phone", async ({ browser }) => {
  const { page } = await newLearner(browser, "phone");
  await page.setViewportSize({ width: 390, height: 844 });
  await openCourse(page, "spanish");
  const first = (await apiProgress(page))[0].slug;
  for (const path of ["/", `/lessons/${first}`]) {
    await page.goto(path);
    await page.waitForLoadState("load");
    const { sw, cw } = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth }));
    expect(sw, `${path} is ${sw}px wide in a ${cw}px window`).toBeLessThanOrEqual(390);
  }
});
