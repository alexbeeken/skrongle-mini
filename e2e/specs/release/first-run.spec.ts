// A release server's first day (#40): the owner link makes the owner, /register
// is closed without a token, the owner makes an invitation on Manage Users, and a
// learner registers through it. Runs first in the `release` project; the specs
// after it sign the owner in with what it saved. With E2E_OWNER_EMAIL the server
// already has an owner and the owner-link test is skipped.
import { newAccount } from "../../lib/learner.ts";
import { expect, makeInvite, newPerson, ownerLink, ownerAccount, registerWithLink, saveOwner, signInOwner, test } from "../../lib/release.ts";

test("the owner link registers the owner", async ({ browser }) => {
  const link = ownerLink();
  test.skip(!link, "no E2E_OWNER_LINK: this server already has an owner");
  const { page } = await newPerson(browser);
  await page.goto(link!);
  await expect(page.getByTestId("register-owner-note")).toBeVisible();
  const owner = newAccount("owner");
  await page.getByTestId("register-email").fill(owner.email);
  await page.getByTestId("register-password").fill(owner.password);
  await page.getByTestId("register-age").check();
  await page.getByTestId("register-submit").click();
  await expect(page.getByTestId("nav-account-toggle")).toHaveAttribute("data-email", owner.email.toLowerCase());
  saveOwner(owner);
  // The owner has Manage Users.
  await page.goto("/manage");
  await expect(page.getByTestId("manage-page")).toBeVisible();
  await expect(page.getByTestId("manage-account-row")).toHaveCount(1);
  await expect(page.getByTestId("manage-account-row")).toHaveAttribute("data-owner", "true");
  // The link is used up.
  const other = await newPerson(browser);
  await other.page.goto(link!);
  await expect(other.page.getByTestId("register-email")).toHaveCount(0);
});

test("/register without a token is closed", async ({ browser }) => {
  const { page } = await newPerson(browser);
  await page.goto("/register");
  await expect(page.getByTestId("register-closed")).toBeVisible();
  await expect(page.getByTestId("register-email")).toHaveCount(0);
  // A made-up token is no better.
  await page.goto("/register?invite=not-a-real-token");
  await expect(page.getByTestId("register-email")).toHaveCount(0);
  // The login page is open.
  await page.goto("/login");
  await expect(page.getByTestId("login-submit")).toBeVisible();
});

test("the owner makes an invitation on Manage Users and a learner registers through it", async ({ browser }) => {
  const { page: ownerPage } = await newPerson(browser);
  await signInOwner(ownerPage);
  const link = await makeInvite(ownerPage);
  await expect(ownerPage.getByTestId("manage-invite-row")).not.toHaveCount(0);

  const { page } = await newPerson(browser);
  const learner = newAccount("first-day");
  await registerWithLink(page, link, learner);
  await page.goto("/courses");
  await expect(page.locator('[data-testid="courses-card"][data-course-id="spanish"]')).toBeVisible();

  // The invitation is single use.
  const again = await newPerson(browser);
  await again.page.goto(link);
  await expect(again.page.getByTestId("register-email")).toHaveCount(0);

  // The owner sees the new account.
  await ownerPage.goto("/manage");
  await expect(ownerPage.getByTestId("manage-accounts")).toContainText(learner.email.toLowerCase());
  expect(ownerAccount().email).not.toBe(learner.email);
});
