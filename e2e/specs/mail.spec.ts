// The dev mailbox (#208): mail the app sends goes over SMTP to the dev
// server's own mailbox, and /dev/inbox opens it so a flow can be clicked through.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount, register } from "../lib/learner";
import { devPage, inbox, mailLink, waitForMail } from "../lib/mail";

test("a new account's confirmation e-mail opens in the dev inbox, and its link confirms the address", async ({ page, browser }) => {
  const a = newAccount("mail");
  await register(page, a, "spanish");
  await page.goto("/profile");
  await expect(page.getByTestId("email-unverified")).toBeVisible();

  const mail = await waitForMail(a.email, /Confirm your e-mail address/);
  expect(mail.envelope_to).toEqual([a.email.toLowerCase()]);
  expect(mail.outbox_id).not.toBeNull();
  expect(mailLink(mail, /\/verify-email\?/)).toContain("/verify-email?");
  expect(mail.html).toContain("Confirm my e-mail address");

  // The newest message to the address, opened in a browser of its own.
  const dev = await devPage(browser, `/dev/inbox/latest?to=${encodeURIComponent(a.email)}`);
  await expect(dev).toHaveURL(new RegExp(`/dev/inbox/${mail.id}$`));
  await expect(dev.getByTestId("dev-inbox-subject")).toHaveText(mail.subject);
  await expect(dev.getByTestId("dev-inbox-to-header")).toContainText(a.email);

  // The link inside the rendered HTML opens in the tab, like a mail client's would.
  await dev.frameLocator('[data-testid="dev-inbox-html"]').getByRole("link", { name: "Confirm my e-mail address" }).click();
  await expect(dev.getByTestId("verify-email-confirmed")).toBeVisible();
  await dev.context().close();
  await page.goto("/profile");
  await expect(page.getByTestId("email-verified")).toBeVisible();
});

test("the outbox's test message arrives in the inbox and opens from the outbox", async ({ page }) => {
  await devLogin(page);
  const to = `${newAccount("outbox").email}`;
  await page.goto("/dev/mail");
  await page.getByTestId("dev-mail-to").fill(to);
  await page.getByTestId("dev-mail-send").click();
  const mail = await waitForMail(to, /test message/);
  expect(mail.text).toContain("If you can read this, the mail provider took it.");
  expect(mail.html).toContain("<strong>");

  await page.goto("/dev/mail");
  const row = page.locator(`[data-testid="dev-mail-message"][data-to="${to}"]`);
  await expect(row).toHaveAttribute("data-state", "sent");
  await row.getByTestId("dev-mail-open").click();
  await expect(page.getByTestId("dev-inbox-subject")).toHaveText(mail.subject);
  await expect(page.frameLocator('[data-testid="dev-inbox-html"]').locator("strong")).toHaveText("Skrongle");

  // The list filters by address.
  await page.goto(`/dev/inbox?to=${encodeURIComponent(to)}`);
  await expect(page.getByTestId("dev-inbox-item")).toHaveCount(1);
  await expect(page.getByTestId("dev-inbox-smtp")).toHaveAttribute("data-addr", /^127\.0\.0\.1:\d+$/);
});

test("the dev mailbox is a mail client's table: bold until opened, archive strikes out and sinks, delete removes, all without a reload (#223)", async ({ page }) => {
  // Only this test's mail: the filter matches part of an address.
  await devLogin(page);
  const filter = newAccount("table").email.split("@")[0];
  const to = `${filter}-a@example.com`;
  const other = `${filter}-b@example.com`;
  await page.goto(`/dev/inbox?to=${encodeURIComponent(filter)}`);
  await expect(page.getByTestId("dev-inbox-empty")).toBeVisible();
  const rows = page.getByTestId("dev-inbox-item");

  // New mail shows up by itself (the page polls), newest first and bold.
  for (const addr of [other, to]) {
    expect((await page.request.post("/dev/mail/send", { form: { to: addr }, maxRedirects: 0 })).status()).toBe(303);
    await waitForMail(addr, /test message/);
  }
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toHaveAttribute("data-to", to);
  await expect(rows.nth(1)).toHaveAttribute("data-to", other);
  await expect(rows.nth(0)).toHaveAttribute("data-read", "false");
  await expect(rows.nth(0).locator(".devbox-subject")).toHaveCSS("font-weight", "700");
  await expect(page.getByTestId("dev-box-tab-in")).toContainText("2");

  // Archive the newest: struck out, below the other one; the page did not reload.
  await page.evaluate(() => ((window as unknown as { stay: boolean }).stay = true));
  await rows.nth(0).getByTestId("dev-inbox-archive").click();
  await expect(rows.nth(1)).toHaveAttribute("data-to", to);
  await expect(rows.nth(1)).toHaveAttribute("data-archived", "true");
  await expect(rows.nth(1).locator(".devbox-subject")).toHaveCSS("text-decoration-line", "line-through");
  await expect(page.getByTestId("dev-box-tab-in")).toContainText("1");
  expect(await page.evaluate(() => (window as unknown as { stay?: boolean }).stay)).toBe(true);

  // Opening one (a click on the row) marks it read: plain type afterwards.
  await rows.nth(0).locator(".devbox-who").click();
  await expect(page.getByTestId("dev-inbox-subject")).toHaveText(/test message/);
  await page.goBack(); // the filtered list
  await expect(rows.nth(0)).toHaveAttribute("data-read", "true");
  await expect(rows.nth(0).locator(".devbox-subject")).toHaveCSS("font-weight", "400");

  // Delete is for good: gone from the page and from the mailbox.
  await rows.nth(0).getByTestId("dev-inbox-delete").click();
  await expect(rows).toHaveCount(1);
  await expect(rows.nth(0)).toHaveAttribute("data-to", to);
  expect(await inbox(other)).toHaveLength(0);

  // The tab switches to the outbox in place; it has the same table and buttons.
  await page.evaluate(() => ((window as unknown as { stay: boolean }).stay = true));
  await page.getByTestId("dev-box-tab-out").click();
  await expect(page).toHaveURL(/\/dev\/mail\?to=/);
  await expect(page.getByTestId("dev-mail")).toBeVisible();
  await expect(page.getByTestId("dev-inbox")).toBeHidden();
  expect(await page.evaluate(() => (window as unknown as { stay?: boolean }).stay)).toBe(true);
  const sent = page.getByTestId("dev-mail-message");
  await expect(sent).toHaveCount(2);
  await expect(sent.nth(0)).toHaveAttribute("data-state", "sent");
  await sent.filter({ has: page.locator(`[title="${other}"]`) }).getByTestId("dev-mail-archive").click();
  await expect(sent.nth(1)).toHaveAttribute("data-archived", "true");
  await sent.nth(1).getByTestId("dev-mail-delete").click();
  await expect(sent).toHaveCount(1);
  await sent.nth(0).getByTestId("dev-mail-view").click();
  await expect(page.getByTestId("dev-mail-subject")).toHaveText(/test message/);
  await expect(page.getByTestId("dev-mail-state")).toContainText("sent");
});

test("a superuser resets another account's e-mail verification, and the new link confirms it again", async ({ page, browser }) => {
  // A learner who confirmed their address, in a browser of their own.
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("reset");
  await register(them, a, "spanish");
  const first = await waitForMail(a.email, /Confirm your e-mail address/);
  await them.goto(mailLink(first, /\/verify-email\?/));
  await expect(them.getByTestId("verify-email-confirmed")).toBeVisible();

  // The dev user is a superuser: the learner's row on the Superuser page has the action.
  await devLogin(page);
  await page.goto("/superuser");
  const row = page.getByTestId("superuser-account-row").filter({ hasText: a.email.toLowerCase() });
  await row.getByTestId("superuser-account-reset-verification").click();
  const dialog = page.getByTestId("superuser-verify-dialog");
  await expect(dialog).toContainText(a.email.toLowerCase());
  await dialog.getByTestId("superuser-verify-confirm").click();
  await expect(page.getByTestId("superuser-notice")).toContainText(`${a.email.toLowerCase()} is unconfirmed again`);

  // The learner is unconfirmed again; the new link (opened from the notice) confirms them.
  await them.goto("/profile");
  await expect(them.getByTestId("email-unverified")).toBeVisible();
  await expect.poll(async () => (await inbox(a.email)).length).toBe(2);
  await page.getByTestId("superuser-inbox").click();
  await page.frameLocator('[data-testid="dev-inbox-html"]').getByRole("link", { name: "Confirm my e-mail address" }).click();
  await expect(page.getByTestId("verify-email-confirmed")).toBeVisible();
  await them.goto("/profile");
  await expect(them.getByTestId("email-verified")).toBeVisible();
  // The link sent before the reset no longer works.
  await them.goto(mailLink(first, /\/verify-email\?/));
  await expect(them.getByTestId("verify-email-invalid")).toBeVisible();

  // An address nobody has (the form posts whatever address it is given).
  expect((await page.request.post("/superuser/reset-email-verification", { form: { email: "nobody-here@example.org" }, maxRedirects: 0 })).status()).toBe(303);
  await page.goto("/superuser?did=unknown&email=nobody-here%40example.org");
  await expect(page.getByTestId("superuser-error")).toContainText("nobody-here@example.org");
});

test("only a superuser sees the Superuser page", async ({ page }) => {
  await register(page, newAccount("plain"), "spanish");
  await page.goto("/profile");
  await page.getByTestId("nav-account-toggle").click();
  await expect(page.getByTestId("nav-superuser")).toHaveCount(0);
  expect((await page.request.get("/superuser", { maxRedirects: 0 })).status()).toBe(404);
  expect((await page.request.post("/superuser/reset-email-verification", { form: { email: "dev@example.com" }, maxRedirects: 0 })).status()).toBe(404);
});

test("the dev mailbox needs no sign-in: signed out and as a learner, every section and its JSON open", async ({ page, playwright }) => {
  const anon = await playwright.request.newContext({ baseURL: test.info().project.use.baseURL });
  for (const path of ["/dev/inbox", "/dev/mail", "/dev/authenticator", "/dev/ai"]) {
    expect((await anon.get(`${path}?format=json`)).status(), `${path} signed out`).toBe(200);
  }
  await anon.dispose();

  // A browser that is not signed in opens every section.
  for (const [path, testid] of [["/dev/inbox", "dev-box"], ["/dev/authenticator", "dev-authenticator"], ["/dev/ai", "dev-ai"]]) {
    await page.goto(path);
    await expect(page.getByTestId(testid)).toBeVisible();
  }
  await page.getByTestId("dev-box-kind-mail").click();
  await expect(page).toHaveURL(/\/dev\/inbox$/);

  // So does a learner, as themselves.
  await register(page, newAccount("notdev"), "spanish");
  for (const path of ["/dev/inbox", "/dev/authenticator", "/dev/ai"]) {
    expect((await page.request.get(`${path}?format=json`)).status(), `${path} as a learner`).toBe(200);
  }
  await page.goto("/dev/ai");
  await expect(page.getByTestId("dev-ai")).toBeVisible();
});
