// Two-factor sign-in (#292): a phone number verified by a texted code (read
// in the dev phone), and a second step after the password, by e-mail (the
// dev inbox), text (the dev phone) or an authenticator app (the dev
// authenticator, #291). The second step is always taken through the real
// /login; the learner never signs in through /dev/login. The dev mailbox
// needs no sign-in, so codes are read in a window of its own (devPage,
// waitForMail), as a person would with a second browser; on /login/code
// itself, the floating dev device (the frame of /login/code/device, this
// sign-in's texts, mail or authenticator entry) shows the code with a Use
// button.
import { expect, test, type Page } from "@playwright/test";
import { login, logout, newAccount, register, type Account } from "../lib/learner";
import { devPage, mailLink, waitForMail } from "../lib/mail";
import { authenticator } from "../lib/totp";

/** A number for the dev phone: US and Canada only (#496), in the fictional 555 range. */
function newNumber(): string {
  return `+1555555${String(Math.floor(Math.random() * 10000)).padStart(4, "0")}`;
}

/** The newest text the dev phone got for `number`, opened in its Text section. */
async function readText(page: Page, number: string, pattern: RegExp): Promise<string> {
  const phone = await devPage(page.context().browser()!, `/dev/phone?to=${encodeURIComponent(number)}`);
  const row = phone.getByTestId("dev-phone-item").first();
  await expect(row).toBeVisible();
  await row.getByTestId("dev-phone-open").click();
  const body = phone.locator(".devbox-bubble.current [data-testid=dev-text-body]");
  await expect(body).toHaveText(pattern);
  const text = await body.innerText();
  await phone.context().close();
  return text.match(/\d{6}/)![0];
}

/** Sign in with the password; the second step's page shows. */
async function passwordStep(page: Page, a: Account): Promise<void> {
  await page.goto("/login");
  await page.getByTestId("login-email").fill(a.email);
  await page.getByTestId("login-password").fill(a.password);
  await page.getByTestId("login-submit").click();
  await expect(page).toHaveURL(/\/login\/code$/);
}

async function codeStep(page: Page, a: Account, code: string): Promise<void> {
  await page.getByTestId("login-code-input").fill(code);
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
}

/** The floating dev device at the bottom of /login/code: the frame of /login/code/device. */
function device(page: Page) {
  return page.frameLocator('[data-testid="login-code-dev-frame"]');
}

/** Its Use button for `code` (the newest row's when not given) types the code into the form. */
async function useDeviceCode(page: Page, code?: string): Promise<string> {
  const row = device(page).locator(`[data-testid="login-code-device-item"]${code ? `[data-code="${code}"]` : ""}`).first();
  await expect(row).toBeVisible({ timeout: 15_000 });
  const got = (await row.getAttribute("data-code"))!;
  await row.getByTestId("login-code-device-use").click();
  await expect(page.getByTestId("login-code-input")).toHaveValue(got);
  return got;
}

async function confirmEmail(page: Page, a: Account): Promise<void> {
  const mail = await waitForMail(a.email, /Confirm your e-mail address/);
  await page.goto(mailLink(mail, /\/verify-email\?/));
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
}

/** The set-up dialog's QR code (#304): an SVG of the link, sized by CSS, with the modules drawn. */
async function expectQr(page: Page): Promise<void> {
  const svg = page.getByTestId("two-factor-totp-qr").locator("svg");
  await expect(svg).toBeVisible();
  const modules = Number(await svg.getAttribute("data-modules"));
  expect(modules, "a TOTP link needs a version 3 to 10 code").toBeGreaterThanOrEqual(29);
  expect(modules % 4).toBe(1);
  await expect(svg).toHaveAttribute("viewBox", `0 0 ${modules + 8} ${modules + 8}`);
  expect((await svg.locator("path").getAttribute("d"))!.length).toBeGreaterThan(500);
  const box = (await svg.boundingBox())!;
  expect(box.width).toBeGreaterThan(100);
  expect(Math.abs(box.width - box.height)).toBeLessThan(2);
}

async function chooseMethod(page: Page, method: string): Promise<void> {
  await page.locator(`[data-testid="profile-two-factor-method"][data-method="${method}"]`).click();
}

/** Off or another method while one is on: the "Confirm it's you" dialog asks for the password. */
async function confirmWith(page: Page, password: string): Promise<void> {
  const dialog = page.getByTestId("two-factor-confirm-dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("two-factor-confirm-password").fill(password);
  await dialog.getByTestId("two-factor-confirm-submit").click();
}

test("a phone number is verified with a texted code, and text-message sign-in then works (#292)", async ({ page }) => {
  const a = newAccount("2fa-sms");
  await register(page, a, "spanish");
  await page.goto("/profile");
  // Not offered until the number is verified.
  await expect(page.locator('[data-testid="profile-two-factor-method"][data-method="sms"] input')).toBeDisabled();
  await expect(page.getByTestId("profile-two-factor-sms-unavailable")).toBeVisible();
  // No phone number until the e-mail address is confirmed (#431): the field is locked, its (i) says why.
  await expect(page.getByTestId("profile-phone")).toBeDisabled();
  await page.getByTestId("phone-locked-info").click();
  await expect(page.getByTestId("phone-locked-dialog")).toBeVisible();
  await page.getByTestId("phone-locked-close").click();
  await confirmEmail(page, a);
  await page.goto("/profile");
  await expect(page.getByTestId("profile-phone")).toBeEnabled();
  await expect(page.getByTestId("phone-locked-info")).toHaveCount(0);

  const number = newNumber();
  await page.getByTestId("profile-phone").fill(number);
  // Typed or pasted with the +1, the field keeps the ten digits, punctuated (#496).
  await expect(page.getByTestId("profile-phone")).toHaveValue(`(555) 555 - ${number.slice(-4)}`);
  await page.getByTestId("profile-phone").blur();
  await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
  await expect(page.getByTestId("phone-unverified")).toBeVisible();
  await expect(page.getByTestId("phone-verified")).toBeHidden();

  await page.getByTestId("phone-unverified").click();
  await expect(page.getByTestId("phone-unverified-dialog")).toBeVisible();
  await page.getByTestId("phone-verify-send").click();
  // Back with the dialog open, waiting for the code.
  await expect(page.getByTestId("phone-unverified-dialog")).toBeVisible();
  const code = await readText(page, number, /code to confirm this phone number/);
  await page.getByTestId("phone-verify-code").fill(code === "000000" ? "111111" : "000000");
  await page.getByTestId("phone-verify-submit").click();
  await expect(page.getByTestId("phone-verify-error")).toContainText("4 tries left");
  await page.getByTestId("phone-verify-code").fill(code);
  await page.getByTestId("phone-verify-submit").click();
  await expect(page.getByTestId("profile-notice")).toContainText("Phone number verified");
  await expect(page.getByTestId("phone-verified")).toBeVisible();
  await expect(page.getByTestId("phone-unverified")).toBeHidden();

  // Turning it on asks for the password too (#374).
  await chooseMethod(page, "sms");
  await expect(page.getByTestId("two-factor-confirm-what")).toHaveText("Turn on two-factor sign-in by text message");
  await confirmWith(page, a.password);
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "sms");
  await expect(page.getByTestId("profile-notice")).toContainText("text message");

  await logout(page);
  await passwordStep(page, a);
  await expect(page.getByTestId("login-code-sent-to")).toContainText(number.slice(-2));
  // The floating dev phone shows the text for this number, and Use types its code.
  await expect(page.getByTestId("login-code-dev")).toContainText("Your phone");
  await expect(device(page).getByTestId("login-code-device-to")).toContainText(number);
  const texted = await readText(page, number, /sign-in code/);
  expect(await useDeviceCode(page, texted)).toBe(texted);
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);
});

test("e-mail sign-in codes are not offered until the address is verified; then the code comes by e-mail and a wrong one is refused (#292)", async ({ page }) => {
  const a = newAccount("2fa-mail");
  await register(page, a, "spanish");
  await page.goto("/profile");
  const email = page.locator('[data-testid="profile-two-factor-method"][data-method="email"] input');
  await expect(email).toBeDisabled();
  await page.getByTestId("profile-two-factor-email-unavailable").click();
  await expect(page.getByTestId("profile-two-factor-email-dialog")).toBeVisible();
  // A forged post is refused too.
  const forged = await page.request.post("/profile/two-factor", { form: { method: "email" }, maxRedirects: 0 });
  expect(forged.status()).toBe(422);

  await confirmEmail(page, a);
  await page.goto("/profile");
  await expect(email).toBeEnabled();
  // A session alone can't turn it on (#374).
  const onForged = await page.request.post("/profile/two-factor", { form: { method: "email" }, maxRedirects: 0 });
  expect(onForged.status()).toBe(403);
  await chooseMethod(page, "email");
  await confirmWith(page, a.password);
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "email");
  // The account is told (#374).
  await waitForMail(a.email, /Two-factor sign-in on .* changed/);

  await logout(page);
  await passwordStep(page, a);
  await expect(page.getByTestId("login-code-sent-to")).toContainText("@example.com");
  // No session yet: the profile still wants a sign-in.
  const blocked = await page.request.get("/profile", { maxRedirects: 0 });
  expect(blocked.status()).toBe(303);

  const mail = await waitForMail(a.email, /sign-in code/);
  // The code is in the body only, never the subject (#361).
  expect(mail.subject).not.toMatch(/\d{6}/);
  const code = mail.text.match(/\d{6}/)![0];
  await page.getByTestId("login-code-input").fill(code === "123456" ? "654321" : "123456");
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("login-code-error")).toContainText("not right");
  await expect(page.getByTestId("login-code-error")).toContainText("4 tries left");
  // The floating dev inbox has the mail, its body carrying the code; Use types it.
  await expect(page.getByTestId("login-code-dev")).toContainText("Your inbox");
  const row = device(page).getByTestId("login-code-device-item").first();
  await expect(row).toContainText(mail.subject);
  expect(await useDeviceCode(page, code)).toBe(code);
  // Without the sign-in's cookie the device page is nothing: a stranger can't read the mailbox through it.
  const stranger = await page.context().browser()!.newContext();
  expect((await stranger.request.get("/login/code/device")).status()).toBe(404);
  await stranger.close();
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);

  // Turning it off, or switching, needs the password: a session alone can't.
  await page.goto("/profile");
  const card = page.getByTestId("profile-two-factor");
  const noProof: Record<string, string>[] = [{ method: "" }, { method: "totp" }, { method: "", password: "not-the-password" }, { method: "totp", password: "not-the-password" }];
  for (const form of noProof) {
    const res = await page.request.post("/profile/two-factor", { form, maxRedirects: 0 });
    expect(res.status(), JSON.stringify(form)).toBe(403);
  }
  const removeForged = await page.request.post("/profile/two-factor/totp/remove", { form: { password: "" }, maxRedirects: 0 });
  expect(removeForged.status()).toBe(303); // no app to remove: nothing to protect
  await page.reload();
  await expect(card).toHaveAttribute("data-method", "email");

  // Cancel puts the chips back.
  await chooseMethod(page, "");
  const confirm = page.getByTestId("two-factor-confirm-dialog");
  await expect(confirm).toBeVisible();
  await expect(confirm.getByTestId("two-factor-confirm-what")).toHaveText("Turn two-factor sign-in off");
  await confirm.getByTestId("two-factor-confirm-cancel").click();
  await expect(confirm).toBeHidden();
  await expect(page.locator('[data-testid="profile-two-factor-method"][data-method="email"] input')).toBeChecked();

  // Switching to the authenticator: a wrong password is refused, the right one starts the set-up.
  await chooseMethod(page, "totp");
  await expect(confirm.getByTestId("two-factor-confirm-what")).toHaveText("Switch two-factor sign-in to authenticator app");
  await confirmWith(page, "not-the-password");
  await expect(confirm.getByTestId("two-factor-confirm-error")).toContainText("password is not right");
  await expect(card).toHaveAttribute("data-method", "email");
  await confirmWith(page, a.password);
  await expect(page.getByTestId("two-factor-totp-dialog")).toBeVisible();
  await page.getByTestId("two-factor-totp-cancel").click();
  await expect(card).toHaveAttribute("data-method", "email");

  // Off, with the right password: the next sign-in needs only the password.
  await chooseMethod(page, "");
  await confirmWith(page, "not-the-password");
  await expect(confirm.getByTestId("two-factor-confirm-error")).toContainText("password is not right");
  await confirmWith(page, a.password);
  await expect(page.getByTestId("profile-notice")).toContainText("Two-factor sign-in is off");
  await expect(card).toHaveAttribute("data-method", "");
  await logout(page);
  await login(page, a);
});

test("an authenticator app is added through the dev authenticator and its codes sign in, each once (#292)", async ({ page }) => {
  test.setTimeout(120_000);
  const a = newAccount("2fa-totp");
  await register(page, a, "spanish");
  await page.goto("/profile");
  await chooseMethod(page, "totp");
  await confirmWith(page, a.password);
  const dialog = page.getByTestId("two-factor-totp-dialog");
  await expect(dialog).toBeVisible();
  const secret = (await page.getByTestId("two-factor-totp-secret").innerText()).replace(/\s/g, "");
  // The setup key is one row (#304): the code shrinks on a narrow screen rather than wrapping.
  const keyBox = await page.getByTestId("two-factor-totp-secret").boundingBox();
  expect(keyBox!.height).toBeLessThan(40);
  // The otpauth:// link the QR encodes (`data-uri` on its wrapper). The link
  // that opened it in an app is gone; passkeys are the next step (#312).
  const uri = (await page.getByTestId("two-factor-totp-qr").getAttribute("data-uri"))!;
  expect(uri).toContain(`secret=${secret}`);
  await expectQr(page);
  // How to scan it sits behind the (i) by the dialog title.
  await dialog.getByTestId("profile-2fa-totp-qr-info").click();
  const info = page.getByTestId("profile-2fa-totp-qr-info-dialog");
  await expect(info).toBeVisible();
  await expect(info).toContainText("authenticator app");
  await info.locator("[data-closes]").click();
  await expect(info).toBeHidden();
  await expect(dialog).toBeVisible();

  // "Scan" it into the dev authenticator, which the dev link opens in a tab of its own.
  const scan = new URL((await page.getByTestId("two-factor-totp-dev").getAttribute("href"))!, page.url());
  expect(scan.pathname).toMatch(/\/dev\/authenticator$/);
  expect(scan.searchParams.get("uri")).toBe(uri);
  const [app] = await Promise.all([page.context().waitForEvent("page"), page.getByTestId("two-factor-totp-dev").click()]);
  await app.getByTestId("dev-authenticator-add").click();
  const row = app.locator(`[data-testid="dev-authenticator-item"][data-account="${a.email.toLowerCase()}"]`);
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-issuer", "Skrongle");

  // A code with time to spare, read off the app's screen.
  const fresh = async (not?: string) => {
    let code = "";
    await expect
      .poll(async () => {
        const left = Number(await row.getByTestId("dev-authenticator-remaining").innerText());
        code = (await row.getAttribute("data-code"))!;
        return left >= 4 && code !== not;
      }, { timeout: 40_000, intervals: [500] })
      .toBe(true);
    return code;
  };
  const first = await fresh();
  await page.getByTestId("two-factor-totp-code").fill(first);
  await page.getByTestId("two-factor-totp-confirm").click();
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "totp");
  await expect(page.getByTestId("profile-two-factor-totp")).toContainText("Authenticator app added");

  await logout(page);
  await passwordStep(page, a);
  await expect(page.getByTestId("login-code")).toHaveAttribute("data-method", "totp");
  // The code used to turn it on works only once.
  await page.getByTestId("login-code-input").fill(first);
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("login-code-error")).toContainText("not right");
  // The floating dev authenticator shows this account's code counting down; Use types the next one.
  await expect(page.getByTestId("login-code-dev")).toContainText("Your authenticator");
  const entry = device(page).getByTestId("login-code-device-item");
  await expect(entry).toHaveCount(1);
  await expect(entry).toContainText("Skrongle");
  await expect
    .poll(async () => Number(await entry.getByTestId("login-code-device-remaining").innerText()) >= 4 && (await entry.getAttribute("data-code")) !== first, { timeout: 40_000, intervals: [500] })
    .toBe(true);
  const next = await useDeviceCode(page);
  expect(next).not.toBe(first);
  await page.getByTestId("login-code-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", a.email);

  // Removing it turns two-factor sign-in off, so it asks for the password; the app forgets it.
  await page.goto("/profile");
  await page.getByTestId("profile-two-factor-totp-remove").click();
  await confirmWith(page, "not-the-password");
  await expect(page.getByTestId("two-factor-confirm-error")).toContainText("password is not right");
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "totp");
  await confirmWith(page, a.password);
  await expect(page.getByTestId("profile-notice")).toContainText("Authenticator app removed");
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "");
  await row.getByTestId("dev-authenticator-remove").click();
  await expect(row).toHaveCount(0);
  expect((await authenticator()).accounts.some((x) => x.account === a.email.toLowerCase())).toBe(false);
  await app.close();
});

/** The dev phone replies `text` to the app from `number`, as a person texting STOP or START would. */
async function replyFromPhone(page: Page, number: string, text: string): Promise<void> {
  const r = await page.request.post("/dev/phone/reply", { form: { to: number, text }, headers: { "x-dev-box": "1" } });
  expect(r.status()).toBe(204);
}

test("a number that replied STOP gets its sign-in code by e-mail, and START plus a new code turns texts back on (#969)", async ({ page }) => {
  const a = newAccount("2fa-stop");
  await register(page, a, "spanish");
  await confirmEmail(page, a);
  await page.goto("/profile");
  const number = newNumber();
  await page.getByTestId("profile-phone").fill(number);
  await page.getByTestId("profile-phone").blur();
  await expect(page.getByTestId("profile-save-status")).toHaveAttribute("data-state", "saved");
  await page.getByTestId("phone-unverified").click();
  await page.getByTestId("phone-verify-send").click();
  await page.getByTestId("phone-verify-code").fill(await readText(page, number, /code to confirm this phone number/));
  await page.getByTestId("phone-verify-submit").click();
  await expect(page.getByTestId("phone-verified")).toBeVisible();
  await chooseMethod(page, "sms");
  await confirmWith(page, a.password);
  await expect(page.getByTestId("profile-two-factor")).toHaveAttribute("data-method", "sms");

  // The phone replies STOP: the next sign-in text is refused, and the page
  // gets a new code sent by e-mail instead, saying why.
  await replyFromPhone(page, number, "STOP");
  await logout(page);
  await passwordStep(page, a);
  await expect(page.getByTestId("login-code-fallback")).toContainText("STOP", { timeout: 20_000 });
  await expect(page.getByTestId("login-code-sent-to")).toContainText("@");
  await expect(page.getByTestId("login-code-dev")).toContainText("Your inbox");
  const mail = await waitForMail(a.email, /sign-in code/);
  await codeStep(page, a, mail.text.match(/\d{6}/)![0]);

  // The profile's phone field shows a red ! whose dialog says how to opt back in.
  await page.goto("/profile");
  await expect(page.getByTestId("phone-opted-out")).toBeVisible();
  await expect(page.getByTestId("phone-verified")).toBeHidden();
  await page.getByTestId("phone-opted-out").click();
  await expect(page.getByTestId("phone-stopped-dialog")).toContainText("START");

  // START from the phone, then a code: it is texted, and the ! goes.
  await replyFromPhone(page, number, "START");
  await page.getByTestId("phone-stopped-send").click();
  await expect(page.getByTestId("phone-unverified-dialog")).toBeVisible();
  await page.getByTestId("phone-verify-code").fill(await readText(page, number, /code to confirm this phone number/));
  await page.getByTestId("phone-verify-submit").click();
  await expect(page.getByTestId("phone-verified")).toBeVisible();
  await expect(page.getByTestId("phone-opted-out")).toBeHidden();

  // Sign-in codes are texted again.
  await logout(page);
  await passwordStep(page, a);
  await expect(page.getByTestId("login-code-sent-to")).toContainText(number.slice(-2));
  await expect(page.getByTestId("login-code-fallback")).toHaveCount(0);
  await codeStep(page, a, await readText(page, number, /sign-in code/));
});
