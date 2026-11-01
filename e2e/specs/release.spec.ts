// Release tools (#934): the superuser (the dev user) turns the waitlist on
// from Superuser → Release; signed out, the home page is one e-mail field
// with no Log in or Create account; joining mails a confirmation link; the
// superuser invites the person, whose e-mail opens the register page with
// the address filled in; the account is made and the list says signed up.
// Turning the waitlist off brings the normal home page back. Hosted only:
// Skrongle mini has no e-mail, so the mini lane skips it.
import { expect, type Page, test } from "@playwright/test";
import { devLogin } from "../lib/app";
import { newAccount } from "../lib/learner";
import { mailLink, waitForMail } from "../lib/mail";

test.skip(process.env.E2E_MINI === "1", "the waitlist is not in Skrongle mini");

/** Turn the waitlist on or off from the Release page, if it isn't already. */
async function setWaitlist(page: Page, on: boolean): Promise<void> {
  await page.goto("/superuser/release");
  if ((await page.getByTestId("release-page").getAttribute("data-waitlist")) === String(on)) return;
  await page.getByTestId("release-switch").click();
  await page.getByTestId("release-switch-confirm").click();
  await expect(page.getByTestId("release-page")).toHaveAttribute("data-waitlist", String(on));
}

// Every other spec signs up the normal way: the waitlist never stays on.
test.afterAll(async ({ browser }) => {
  const page = await browser.newPage();
  await devLogin(page, "next=/superuser");
  await setWaitlist(page, false);
  await page.close();
});

test("a person joins the waitlist, confirms, is invited and signs up", async ({ page, browser }) => {
  await devLogin(page, "next=/superuser");
  await expect(page.getByTestId("superuser-release-state")).toHaveText("Off: anyone can sign up.");
  await page.getByTestId("superuser-release-open").click();
  await expect(page).toHaveURL(/\/superuser\/release$/);
  await setWaitlist(page, true);
  await expect(page.getByTestId("release-notice")).toHaveText("✓ The waitlist is on.");

  // Signed out: the waitlist page, and no way to sign up or log in.
  const visitor = await (await browser.newContext()).newPage();
  await visitor.goto("/");
  await expect(visitor.getByTestId("waitlist-form")).toBeVisible();
  for (const gone of ["nav-register", "nav-login", "waitlist-login", "landing-register"]) await expect(visitor.getByTestId(gone)).toHaveCount(0);
  // The huge "Learn anything." stays clear of the course card beside it (it ran under the card at 1440 px).
  for (const width of [1440, 1280, 1024]) {
    await visitor.setViewportSize({ width, height: 800 });
    const gap = await visitor.evaluate(() => {
      const words = document.createRange();
      words.selectNodeContents(document.querySelector(".wl-hero h1")!);
      return document.querySelector(".wl-build")!.getBoundingClientRect().left - words.getBoundingClientRect().right;
    });
    expect(gap, `headline to card at ${width} px`).toBeGreaterThan(0);
  }
  await visitor.goto("/register");
  await expect(visitor).toHaveURL(/\/$/);

  const person = newAccount("waitlist");
  await visitor.getByTestId("waitlist-email").fill(person.email);
  await visitor.getByTestId("waitlist-submit").click();
  await expect(visitor.getByTestId("waitlist-joined")).toHaveText("✓ Check your e-mail: open the link we sent to confirm your place.");
  await visitor.goto(mailLink(await waitForMail(person.email, /Confirm your place/), /\/waitlist\/confirm\?token=/));
  await expect(visitor.getByTestId("waitlist-confirmed")).toBeVisible();

  // The superuser invites them from the list.
  await page.goto("/superuser/release");
  const row = page.locator(`[data-testid="release-row"][data-email="${person.email.toLowerCase()}"]`);
  await expect(row).toHaveAttribute("data-status", "waiting");
  await row.getByTestId("release-invite").click();
  await expect(page.getByTestId("release-notice")).toContainText(`Invitation sent to ${person.email.toLowerCase()}.`);
  await expect(row).toHaveAttribute("data-status", "invited");
  await expect(row.getByTestId("release-invite")).toHaveText("Resend");

  // The invitation opens the register page with the address filled in.
  await visitor.goto(mailLink(await waitForMail(person.email, /Your invitation/), /\/register\?invite=/));
  await expect(visitor.getByTestId("register-email")).toHaveValue(person.email.toLowerCase());
  await visitor.getByTestId("register-password").fill(person.password);
  await visitor.getByTestId("register-age").check();
  await visitor.getByTestId("register-terms").check();
  await visitor.getByTestId("register-submit").click();
  await expect(visitor.getByTestId("register-sent")).toBeVisible();
  await visitor.context().close();

  await page.reload();
  await expect(row).toHaveAttribute("data-status", "signed-up");
  await expect(row.getByTestId("release-invite")).toHaveCount(0);

  // The launch: the normal home page again.
  await setWaitlist(page, false);
  const after = await (await browser.newContext()).newPage();
  await after.goto("/");
  await expect(after.getByTestId("landing-register").first()).toBeVisible();
  await after.context().close();
});
