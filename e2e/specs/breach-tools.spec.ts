// Breach tools (#1068): from /superuser, sign a list of accounts out and
// e-mail them a notice, each after a typed confirmation word. The dev user is
// a superuser. The specs use "Only these addresses", never Every account, so
// they don't sign out or mail the accounts of specs running beside them.
import { expect, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { login, newAccount, signUp } from "../lib/learner";
import { inbox, mailLink, waitForMail } from "../lib/mail";

test("a superuser signs a listed account out after typing the word", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("breach-signout");
  await signUp(them, a);
  await login(them, a);
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/profile/);

  await devLogin(page);
  await page.goto("/superuser");
  const card = page.getByTestId("superuser-breach-card");
  await card.getByTestId("superuser-breach-info").click();
  await expect(page.getByTestId("superuser-breach-info-dialog")).toContainText("type a word");
  await page.getByTestId("superuser-breach-info-dialog").locator("[data-closes]").click();

  const dialog = page.getByTestId("superuser-breach-sign-out-dialog");
  const fill = async (word: string) => {
    await card.getByTestId("superuser-breach-sign-out").click();
    await dialog.getByTestId("superuser-breach-sign-out-only").check();
    await dialog.getByTestId("superuser-breach-sign-out-list").fill(`${a.email.toUpperCase()}, nobody-${a.email}`);
    await dialog.getByTestId("superuser-breach-sign-out-reason").fill("Incident 1 in the register");
    await dialog.getByTestId("superuser-breach-sign-out-confirm-word").fill(word);
    await dialog.getByTestId("superuser-breach-sign-out-confirm").click();
  };

  // The wrong word does nothing.
  await fill("yes");
  await expect(page.getByTestId("superuser-error")).toContainText("Nothing was done. Type confirm to confirm.");
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/profile/);

  await fill("Confirm");
  await expect(page.getByTestId("superuser-notice")).toContainText("Signed out 1 account.");
  const row = page.locator('[data-testid="superuser-privacy-log-row"][data-action="sign-out"]').first();
  await expect(row).toContainText("Accounts signed out");
  await expect(row).toContainText("Incident 1 in the register");
  // The superuser's own session stays; the learner's is gone.
  await page.goto("/superuser");
  await expect(page.getByTestId("superuser-page")).toBeVisible();
  await them.goto("/profile");
  await expect(them).toHaveURL(/\/login/);
  await them.context().close();
});

test("a superuser e-mails a notice to a confirmed address once, however often Send is pressed", async ({ page, browser }) => {
  const them = await (await browser.newContext()).newPage();
  const a = newAccount("breach-notice");
  await signUp(them, a);
  const email = a.email.toLowerCase();
  await them.goto(mailLink(await waitForMail(email, /Confirm your e-mail address/), /\/verify-email\?/));
  await expect(them.getByTestId("verify-email-confirmed")).toBeVisible();
  await them.context().close();
  const unconfirmed = newAccount("breach-unconfirmed");
  await signUp(await (await browser.newContext()).newPage(), unconfirmed);

  await devLogin(page);
  await page.goto("/superuser");
  const card = page.getByTestId("superuser-breach-card");
  const dialog = page.getByTestId("superuser-breach-notice-dialog");
  const subject = `Security incident ${Date.now()}`;
  const send = async () => {
    await card.getByTestId("superuser-breach-notice").click();
    await dialog.getByTestId("superuser-breach-notice-only").check();
    await dialog.getByTestId("superuser-breach-notice-list").fill(`${a.email}\n${unconfirmed.email}`);
    await dialog.getByTestId("superuser-breach-notice-subject").fill(subject);
    await dialog.getByTestId("superuser-breach-notice-body").fill("Hello,\n\nSomething happened.\n\nSkrongle");
    await dialog.getByTestId("superuser-breach-notice-reason").fill("Incident 2");
    await dialog.getByTestId("superuser-breach-notice-confirm-word").fill("confirm");
    await dialog.getByTestId("superuser-breach-notice-confirm").click();
  };

  await send();
  await expect(page.getByTestId("superuser-notice")).toContainText("Notice queued for 1 account.");
  const mail = await waitForMail(email, new RegExp(subject));
  expect(mail.text).toContain("Something happened.");
  const row = page.locator('[data-testid="superuser-privacy-log-row"][data-action="notice"]').first();
  await expect(row).toContainText("Incident 2");

  // Pressing Send again with the same text mails nobody twice.
  await send();
  await expect(page.getByTestId("superuser-notice")).toContainText("Notice queued for 0 accounts. 1 account had it already.");
  expect((await inbox(email)).filter((m) => m.subject === subject)).toHaveLength(1);
  expect((await inbox(unconfirmed.email.toLowerCase())).filter((m) => m.subject === subject)).toHaveLength(0);
});
