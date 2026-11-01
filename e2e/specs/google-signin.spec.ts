// "Continue with Google" against dev mode's stand-in Google (#173): a new
// learner signs up (the confirm step: who you are, Terms, "Hide all AI
// features" #181) and signs back in, the login page's dev line forgets
// accounts (#181), a learner connects a Google account with the password
// and unlinks it (#360) and nobody else can take it, and each failure
// the stand-in can produce shows on the login page. The real flow runs end
// to end (redirect, state, PKCE, token exchange, ID token); only Google's
// pages are missing. scripts/e2e.sh serves it: dev mode with no real client.
// The failures start from the dev-only buttons under the dev login (#191).
// One account per address (#277): Google joins a confirmed password account,
// refuses an unconfirmed one, and /register refuses a Google-only address.
// With two-factor sign-in on, the join waits for the code (#677).
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { login, logout, newAccount, register } from "../lib/learner";
import { inbox, mailLink, waitForMail } from "../lib/mail";

/** /auth/google steered by the stand-in's query parameters. */
const google = (params: Record<string, string>) => `/auth/google?${new URLSearchParams(params)}`;

test("a new learner signs up with Google, confirms the Terms, and signs in again", async ({ page }) => {
  const a = newAccount("google");
  await page.goto("/login");
  await expect(page.getByTestId("login-google")).toHaveAttribute("href", /\/auth\/google$/);
  await expect(page.getByTestId("login-dev-google-ok")).toHaveAttribute("href", /\/auth\/google\?mock_email=mock\.learner%40example\.com$/);

  await page.goto(google({ mock_email: a.email }));
  await expect(page).toHaveURL(/\/auth\/google\/confirm$/);
  // The last step of signing up: who you are, the Terms, the same AI box as /register.
  await expect(page.getByTestId("google-confirm-identity")).toContainText("Signing up with Google as");
  await expect(page.getByTestId("google-confirm-email")).toHaveText(a.email);
  // Two boxes, age and Terms: the sentence, the check box on the right in the pill's style.
  await expect(page.getByTestId("google-confirm-age-row")).toContainText(/I am at least \d+ years old/);
  await expect(page.getByTestId("google-confirm-age")).not.toBeChecked();
  await expect(page.getByTestId("google-confirm-terms-row")).toContainText("I agree to the Terms and Privacy Policy");
  await expect(page.getByTestId("google-confirm-terms")).not.toBeChecked();
  await expect(page.getByTestId("google-confirm-terms-link")).toHaveAttribute("href", /\/terms$/);
  await expect(page.getByTestId("google-confirm-privacy-link")).toHaveAttribute("href", /\/privacy$/);
  // The AI choice is the profile's pill: Off, and the info dot explains on hover.
  await expect(page.getByTestId("google-confirm-no-ai")).not.toBeChecked();
  await expect(page.getByTestId("google-confirm-no-ai-row")).toContainText(/Hide all AI features.*Off/s);
  await expect(page.getByTestId("google-confirm-no-ai-tip")).toBeHidden();
  await page.getByTestId("google-confirm-no-ai-info").hover();
  await expect(page.getByTestId("google-confirm-no-ai-tip")).toBeVisible();
  await expect(page.getByTestId("google-confirm-no-ai-tip")).toContainText("AI tutor");
  await page.mouse.move(0, 0);
  await expect(page.getByTestId("google-confirm-no-ai-tip")).toBeHidden();
  await expect(page.getByTestId("google-confirm-error")).toHaveCount(0);
  // The browser's own check (required) is skipped: the server must refuse too,
  // with the error inside the form (under the box) and the AI choice kept.
  const refused = await page.request.post("/auth/google/confirm", { form: { age: "on", no_ai: "on" }, maxRedirects: 0 });
  expect(refused.status()).toBe(200);
  const refusedPage = await refused.text();
  expect(refusedPage).toContain('data-testid="google-confirm-error">Accept the Terms to continue.');
  expect(refusedPage).toContain('data-testid="google-confirm-terms" required aria-invalid="true"');
  expect(refusedPage).toContain('data-testid="google-confirm-age" required checked>');
  expect(refusedPage).toContain('data-testid="google-confirm-no-ai" checked');
  const neither = await page.request.post("/auth/google/confirm", { form: {}, maxRedirects: 0 });
  expect(await neither.text()).toMatch(/google-confirm-error">Confirm you are at least \d+ and accept the Terms to continue\./);

  await page.getByTestId("google-confirm-age").check();
  await page.getByTestId("google-confirm-terms").check();
  await page.getByTestId("google-confirm-no-ai").check();
  await expect(page.getByTestId("google-confirm-no-ai-row")).toContainText(/Hide all AI features.*On/s);
  await page.getByTestId("google-confirm-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  // Hidden AI features, exactly as a password sign-up with the box ticked (ai-off.spec.ts).
  await page.goto("/");
  await expect(page.locator('[data-testid="home-mode"][data-mode="ai"]')).toHaveCount(0);
  await page.goto("/profile");
  await expect(page.getByTestId("consent-ai")).toHaveAttribute("data-ai-off", "true");
  await expect(page.getByTestId("profile-api-keys")).toBeHidden();
  await expect(page.locator("#account")).toContainText("Google account connected");
  await expect(page.locator("#account")).not.toContainText("Password");
  await expect(page.getByTestId("profile-google-connect")).toHaveCount(0);

  // Signing in again with the same address finds the account: no confirm step.
  await logout(page);
  await page.goto(google({ mock_email: a.email.toUpperCase() }));
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);

  // The login page's dev line forgets the account (no user needed): the
  // same address signs up afresh, at the confirm step again.
  await logout(page);
  await page.goto("/login");
  await page.getByTestId("login-dev-forget-google-email").fill(a.email);
  await page.getByTestId("login-dev-forget-google").click();
  await expect(page).toHaveURL(/\/login\?forgot=deleted$/);
  await expect(page.getByTestId("login-notice")).toContainText("Google-only account is deleted");
  await page.goto(google({ mock_email: a.email }));
  await expect(page).toHaveURL(/\/auth\/google\/confirm$/);
  await expect(page.getByTestId("google-confirm-email")).toHaveText(a.email);
});

// A learner of its own, not the dev user: connecting and unlinking sign
// out every other session (#375), which would sign out parallel specs.
test("a learner connects a Google account with the password, nobody else can take it, and unlinks it (#360)", async ({ page }) => {
  const a = newAccount("google-link");
  await register(page, a, "spanish");
  // Signed in, /auth/google no longer links anything: it goes to the profile.
  await page.goto("/auth/google");
  await expect(page).toHaveURL(/\/profile#account$/);
  await page.getByTestId("profile-google-connect").click();
  const link = page.getByTestId("profile-google-link-dialog");
  await expect(link).toBeVisible();
  await link.getByTestId("profile-google-link-password").fill("not-the-password");
  await link.getByTestId("profile-google-link-submit").click();
  await expect(link.getByTestId("profile-google-error")).toContainText("password is not right");
  await link.getByTestId("profile-google-link-password").fill(a.password);
  await link.getByTestId("profile-google-link-submit").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Google account connected");
  await expect(page.locator("#account")).toContainText("Password and Google account connected");
  await expect(page.getByTestId("profile-google-connect")).toHaveCount(0);

  // Another learner tries to connect the same (default) stand-in account.
  await logout(page);
  const b = newAccount("google-taken");
  await register(page, b, "spanish");
  await page.goto("/profile");
  await page.getByTestId("profile-google-connect").click();
  await page.getByTestId("profile-google-link-password").fill(b.password);
  await page.getByTestId("profile-google-link-submit").click();
  await expect(page.locator("main")).toContainText("That Google account is already attached to another user.");
  // Their account is unchanged.
  await page.goto("/profile");
  await expect(page.getByTestId("profile-google-connect")).toBeVisible();

  // Forgetting from the login page disconnects a password account's Google,
  // and it can connect again. The field starts with the stand-in's default address.
  await logout(page);
  await page.goto("/login");
  await expect(page.getByTestId("login-dev-forget-google-email")).toHaveValue("mock.learner@example.com");
  await page.getByTestId("login-dev-forget-google-email").fill(a.email);
  await page.getByTestId("login-dev-forget-google").click();
  await expect(page.getByTestId("login-notice")).toContainText("Google disconnected");
  await login(page, a);
  await page.goto("/profile");
  await expect(page.locator("#account")).not.toContainText("Google account connected");
  await page.getByTestId("profile-google-connect").click();
  await page.getByTestId("profile-google-link-password").fill(a.password);
  await page.getByTestId("profile-google-link-submit").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Google account connected");

  // Unlinking asks for the password too.
  await page.getByTestId("profile-google-unlink-open").click();
  const unlink = page.getByTestId("profile-google-unlink-dialog");
  await unlink.getByTestId("profile-google-unlink-password").fill("not-the-password");
  await unlink.getByTestId("profile-google-unlink-submit").click();
  await expect(unlink.getByTestId("profile-google-error")).toContainText("password is not right");
  await unlink.getByTestId("profile-google-unlink-password").fill(a.password);
  await unlink.getByTestId("profile-google-unlink-submit").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Google account unlinked");
  await expect(page.locator("#account")).not.toContainText("Google account connected");
  await expect(page.getByTestId("profile-google-connect")).toBeVisible();
});

test("Google with a password account's address: refused while unconfirmed, then joins that account once confirmed", async ({ page }) => {
  const a = newAccount("google-join");
  await register(page, a, "spanish");
  await logout(page);

  // Unconfirmed: whoever registered may not own the address, so no sign-in and no second account.
  await page.goto(google({ mock_email: a.email }));
  await expect(page.getByTestId("login-error")).toHaveText(
    "An account with this address exists, but its e-mail address isn't confirmed. Sign in with your password and confirm it, then connect Google from your profile.",
  );
  await expect(page.getByTestId("nav-account-toggle")).toHaveCount(0);

  // Confirmed through the link in the mail: the same Google sign-in now joins it.
  const mail = await waitForMail(a.email, /Confirm your e-mail address/);
  await page.goto(mailLink(mail, /\/verify-email\?/));
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
  await page.goto(google({ mock_email: a.email }));
  await expect(page).toHaveURL(/\/profile\?saved=google-joined$/);
  await expect(page.getByTestId("profile-notice")).toContainText("Google account connected");
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  // The password account itself (a second, Google-only one would have no password).
  await expect(page.locator("#account")).toContainText("Password and Google account connected");

  // Next time Google signs straight in, with no notice; the password still works.
  await logout(page);
  await page.goto(google({ mock_email: a.email }));
  await expect(page).not.toHaveURL(/google-joined/);
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await logout(page);
  await login(page, a);
});

/** The sign-in code mailed to `email` after mail `seen` (newer id): its code and id. Waits for it,
 * so a slow mailer cannot hand back an older code. */
async function newSignInCode(email: string, seen?: number): Promise<{ id: number; code: string }> {
  let found: { id: number; code: string } | undefined;
  await expect
    .poll(async () => {
      const m = (await inbox(email)).find((x) => /sign-in code/.test(x.subject) && (seen === undefined || x.id > seen));
      found = m && { id: m.id, code: m.text.match(/\d{6}/)![0] };
      return found !== undefined;
    }, { message: `no new sign-in code for ${email}` })
    .toBe(true);
  return found!;
}

test("with two-factor sign-in on, Google joins the account only once the code is entered (#677)", async ({ page, context }) => {
  const a = newAccount("google-join-2fa");
  await register(page, a, "spanish");
  const confirm = await waitForMail(a.email, /Confirm your e-mail address/);
  await page.goto(mailLink(confirm, /\/verify-email\?/));
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
  await page.goto("/profile");
  await page.locator('[data-testid="profile-two-factor-method"][data-method="email"]').click();
  const dialog = page.getByTestId("two-factor-confirm-dialog");
  await dialog.getByTestId("two-factor-confirm-password").fill(a.password);
  await dialog.getByTestId("two-factor-confirm-submit").click();
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "email");
  await logout(page);

  // Google with the account's address asks for the code; the tab is closed there.
  await page.goto(google({ mock_email: a.email }));
  await expect(page).toHaveURL(/\/login\/code$/);
  const abandoned = await newSignInCode(a.email);
  await context.clearCookies();

  // The password and its code sign in: Google is not connected.
  await page.goto("/login");
  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/login\/code$/);
  const byPassword = await newSignInCode(a.email, abandoned.id);
  await page.getByTestId("login-code-input").fill(byPassword.code);
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await page.goto("/profile");
  await expect(page.locator("#account")).not.toContainText("Google account connected");
  await expect(page.getByTestId("profile-google-connect")).toBeVisible();
  await logout(page);

  // Google again, this time with the code: now it joins, and the profile says so.
  await page.goto(google({ mock_email: a.email }));
  await expect(page).toHaveURL(/\/login\/code$/);
  const byGoogle = await newSignInCode(a.email, byPassword.id);
  await page.getByTestId("login-code-input").fill(byGoogle.code);
  await page.getByTestId("login-code-submit").click();
  await expect(page).toHaveURL(/\/profile\?saved=google-joined$/);
  await expect(page.getByTestId("profile-notice")).toContainText("Google account connected");
  await expect(page.locator("#account")).toContainText("Password and Google account connected");
});

// #379: a taken address, Google-only or not, gets the same "check your mail"
// page as a new one; the owner is told by mail, and no password account is made.
test("registering with a password for a Google-only address makes nothing and mails its owner", async ({ page }) => {
  const a = newAccount("google-only");
  await page.goto(google({ mock_email: a.email }));
  await page.getByTestId("google-confirm-age").check();
  await page.getByTestId("google-confirm-terms").check();
  await page.getByTestId("google-confirm-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await logout(page);

  await page.goto("/register");
  await page.getByTestId("register-email").fill(a.email.toUpperCase());
  await page.getByTestId("register-password").fill(a.password);
  await page.getByTestId("register-age").check();
  await page.getByTestId("register-terms").check();
  await page.getByTestId("register-submit").click();
  await expect(page.getByTestId("register-sent-email")).toHaveText(a.email);
  await expect(page.getByTestId("nav-account-toggle")).toHaveCount(0);
  await waitForMail(a.email, /already have/);
  // No password was set: the account still signs in with Google only.
  await page.goto("/login");
  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page.getByTestId("login-error")).toBeVisible();
});

test("a Google-only account confirms with Google before its deletion is scheduled (#376, #970)", async ({ page }) => {
  const a = newAccount("google-delete");
  await page.goto(google({ mock_email: a.email }));
  await page.getByTestId("google-confirm-age").check();
  await page.getByTestId("google-confirm-terms").check();
  await page.getByTestId("google-confirm-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await page.goto("/profile");
  // The address on the page is not enough.
  await page.getByTestId("profile-delete-confirm").fill(a.email);
  await page.getByTestId("profile-delete-submit").click();
  await expect(page.getByTestId("profile-delete-error")).toContainText("Confirm with Google");
  // Through the stand-in Google (the account it signs in with) and back.
  await page.getByTestId("profile-delete-google").click();
  await expect(page.getByTestId("profile-delete-google-done")).toBeVisible();
  await page.getByTestId("profile-delete-confirm").fill(a.email);
  await page.getByTestId("profile-delete-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveCount(0);
  // Scheduled, not gone (#970): signing in with Google again shows only the deletion page.
  await page.goto(google({ mock_email: a.email }));
  await expect(page).toHaveURL(/\/account\/deletion$/);
  await expect(page.getByTestId("account-deletion-cancel")).toBeVisible();
});

test("signing out everywhere else ends the learner's other sessions (#375)", async ({ page, browser }) => {
  const a = newAccount("sign-out-others");
  await register(page, a, "spanish");
  const other = await browser.newContext();
  const elsewhere = await other.newPage();
  await login(elsewhere, a);
  await page.goto("/profile");
  await page.getByTestId("profile-sign-out-others-info").click();
  await expect(page.getByTestId("profile-sign-out-others-info-dialog")).toBeVisible();
  await page.getByTestId("profile-sign-out-others-info-close").click();
  await page.getByTestId("profile-sign-out-others").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Signed out everywhere else");
  // This browser is still signed in; the other one is not.
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
  await elsewhere.goto("/profile");
  await expect(elsewhere).toHaveURL(/\/login/);
  await other.close();
});

const failures: [string, string][] = [
  ["cancel", "Google sign-in was cancelled."],
  ["invalid_grant", "Could not complete the sign-in with Google. Please try again."],
  ["unavailable", "Could not complete the sign-in with Google. Please try again."],
  ["unverified", "Google returned an identity this site cannot accept."],
];
for (const [error, shown] of failures) {
  test(`the ${error} button: the sign-in fails and comes back to the login page with the reason`, async ({ page }) => {
    await page.goto("/login");
    await page.getByTestId(`login-dev-google-${error}`).click();
    await expect(page).toHaveURL(/\/auth\/google\/callback\?/);
    await expect(page.getByTestId("login-error")).toHaveText(shown);
    await expect(page.getByTestId("login-form")).toBeVisible();
    await expect(page.getByTestId("nav-account-toggle")).toHaveCount(0);
  });
}

test("the register page has the same buttons, each leading to its message", async ({ page }) => {
  await page.goto("/register");
  const buttons = page.getByTestId("register-dev-google").getByRole("link");
  await expect(buttons).toHaveCount(6);
  await expect(page.getByTestId("register-dev-google-timeout")).toHaveAttribute("href", /\/auth\/google\?mock_error=timeout$/);
  for (const [error, shown] of failures) {
    await page.goto("/register");
    await page.getByTestId(`register-dev-google-${error}`).click();
    await expect(page.getByTestId("login-error")).toHaveText(shown);
  }
  // The normal sign-in: the confirm step for a new address, or straight in if
  // another spec already made (or connected) the sample account.
  await page.goto("/register");
  await page.getByTestId("register-dev-google-ok").click();
  await expect(page.getByTestId("google-confirm-email").or(page.getByTestId("nav-account-toggle"))).toBeVisible();
  await expect(page.getByTestId("login-error")).toHaveCount(0);
});

test("the buttons fit a phone screen", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await page.goto("/login");
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width).toBeLessThanOrEqual(360);
  for (const b of await page.getByTestId("login-dev-google").getByRole("link").all()) {
    const box = await b.boundingBox();
    expect(box!.x + box!.width).toBeLessThanOrEqual(360);
  }
});

test("the timeout button: a Google that never answers fails the sign-in after the app's own timeout", async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto("/login");
  const started = Date.now();
  // The click would wait (5 s) for the held navigation; the URL check waits instead.
  await page.getByTestId("login-dev-google-timeout").click({ noWaitAfter: true });
  await expect(page).toHaveURL(/\/auth\/google\/callback\?/, { timeout: 30_000 });
  await expect(page.getByTestId("login-error")).toHaveText("Could not complete the sign-in with Google. Please try again.");
  expect(Date.now() - started).toBeGreaterThan(10_000);
});
